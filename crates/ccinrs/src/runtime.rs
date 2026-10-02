//! The runtime: the one library the generated code calls besides Rust's own.
//!
//! A unit that uses the complex types calls `::cinrs::rt` — its `Complex`, and
//! the arithmetic of `cinrs_rt::complex`, which C's Annex G makes more than
//! the naive formulas. A `c99!` block has it from Cargo. A C compiler has no
//! Cargo to ask, so `ccinrs` carries the module's source and compiles it with
//! the `rustc` it runs, the first time a program needs it, into a library
//! named `cinrs`; that library is kept, and every later run with the same
//! `rustc`, target and `ccinrs` links the same one.
//!
//! The `Complex` beside the module is `ccinrs`'s own. `cinrs-rt` re-exports
//! `num-complex`'s, whose layout — `#[repr(C)]`, the real part first — is all
//! the module and the generated code ask of it, and a C program has no use
//! for the rest of that crate.

use std::cell::OnceCell;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::args::Invocation;
use crate::driver::{Failure, run_rustc};
use crate::rustc::Rustc;

/// The runtime's own `Complex`: `num_complex::Complex` in layout, and in the
/// three operators the generated code uses, which are componentwise. The
/// product and the quotient are Annex G's, in `complex`.
const COMPLEX_TYPE: &str = "\
    /// C's `float _Complex` and `double _Complex`: the real part, then the
    /// imaginary part, as `num_complex::Complex` lays them out.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Complex<T> {
        /// The real part.
        pub re: T,
        /// The imaginary part.
        pub im: T,
    }

    impl<T> Complex<T> {
        /// `re + im·i`.
        pub const fn new(re: T, im: T) -> Self {
            Self { re, im }
        }
    }

    impl<T: core::ops::Add<Output = T>> core::ops::Add for Complex<T> {
        type Output = Self;
        #[inline]
        fn add(self, other: Self) -> Self {
            Self::new(self.re + other.re, self.im + other.im)
        }
    }

    impl<T: core::ops::Sub<Output = T>> core::ops::Sub for Complex<T> {
        type Output = Self;
        #[inline]
        fn sub(self, other: Self) -> Self {
            Self::new(self.re - other.re, self.im - other.im)
        }
    }

    impl<T: core::ops::Neg<Output = T>> core::ops::Neg for Complex<T> {
        type Output = Self;
        #[inline]
        fn neg(self) -> Self {
            Self::new(-self.re, -self.im)
        }
    }
";

/// The runtime as Rust: the module the generated code names `rt`.
pub fn source() -> String {
    let mut text = String::from(
        "/// The runtime of the complex types: `cinrs_rt`, as ccinrs carries it.\npub mod rt {\n",
    );
    text.push_str(COMPLEX_TYPE);
    text.push_str("\n    pub mod complex {\n");
    // `complex.rs` has no string literal that spans lines, so indenting it
    // changes nothing but its looks.
    for line in cinrs_rt::COMPLEX_SOURCE.lines() {
        if !line.is_empty() {
            text.push_str("        ");
            text.push_str(line);
        }
        text.push('\n');
    }
    text.push_str("    }\n}\n");
    text
}

/// The compiled runtime of one run, found or compiled when it is first asked
/// for — which a program without a complex value never does.
pub struct Runtime {
    library: OnceCell<PathBuf>,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            library: OnceCell::new(),
        }
    }

    /// The `libcinrs.rlib` for `triple`: the one kept from an earlier run, or
    /// one compiled now — into the cache, or into `work` when there is no
    /// cache directory.
    pub fn library(
        &self,
        inv: &Invocation,
        rustc: &Rustc,
        triple: &str,
        work: &Path,
    ) -> Result<&Path, Failure> {
        if let Some(library) = self.library.get() {
            return Ok(library);
        }
        let library = find_or_compile(inv, rustc, triple, work)?;
        Ok(self.library.get_or_init(|| library))
    }
}

fn find_or_compile(
    inv: &Invocation,
    rustc: &Rustc,
    triple: &str,
    work: &Path,
) -> Result<PathBuf, Failure> {
    let source = source();
    let mut hasher = DefaultHasher::new();
    (
        env!("CARGO_PKG_VERSION"),
        &rustc.release,
        &rustc.commit,
        triple,
        &source,
    )
        .hash(&mut hasher);
    let dir = match cache_root() {
        Some(root) => root.join(triple).join(format!("{:016x}", hasher.finish())),
        None => work.join("runtime"),
    };
    let library = dir.join("libcinrs.rlib");
    if library.is_file() {
        return Ok(library);
    }
    // Compiled in a directory of this process's own and renamed into place,
    // so that two runs at once — `make -j` — never see half a library.
    let scratch = dir.join(format!("build-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)
        .map_err(|error| format!("cannot create {}: {error}", scratch.display()))?;
    let rust = scratch.join("cinrs.rs");
    std::fs::write(
        &rust,
        format!(
            "//! The runtime of the programs ccinrs {} compiles.\n#![no_std]\n\n{source}",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .map_err(|error| format!("cannot write {}: {error}", rust.display()))?;
    if inv.verbose {
        eprintln!("ccinrs: compiling the runtime into {}", dir.display());
    }
    let built = scratch.join("libcinrs.rlib");
    let mut cmd = rustc.command();
    cmd.args([
        "--edition",
        "2024",
        "--crate-type",
        "rlib",
        "--crate-name",
        "cinrs",
    ])
    .args(["--target", triple])
    // It is compiled once and called from every complex operation, and
    // it has nothing to check at run time that its tests have not.
    .args([
        "-C",
        "opt-level=3",
        "-C",
        "codegen-units=1",
        "--cap-lints",
        "allow",
    ])
    // The source is kept beside the library, where what `rustc` says
    // about a type of the runtime points.
    .arg(format!(
        "--remap-path-prefix={}={}",
        scratch.display(),
        dir.display()
    ))
    .arg("-o")
    .arg(&built)
    .arg(&rust);
    let compiled = run_rustc(inv, cmd, || {
        format!(
            "rustc could not compile the runtime ({}); this is a bug in ccinrs",
            rust.display()
        )
    });
    // Another run may have put its library in place first, which is as good.
    let placed = compiled.and_then(|()| {
        let source = dir.join("cinrs.rs");
        std::fs::rename(&rust, &source)
            .and_then(|()| std::fs::rename(&built, &library))
            .or_else(|error| {
                if library.is_file() {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .map_err(|error| {
                Failure::Message(format!("cannot write {}: {error}", library.display()))
            })
    });
    let _ = std::fs::remove_dir_all(&scratch);
    placed.map(|()| library)
}

/// Where compiled runtimes are kept: `CCINRS_CACHE_DIR`, or `ccinrs` in the
/// platform's directory for caches.
fn cache_root() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    if let Some(dir) = var("CCINRS_CACHE_DIR") {
        return Some(dir);
    }
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| home.join("Library/Caches"))
    } else {
        var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|home| home.join(".cache")))
    };
    base.map(|base| base.join("ccinrs"))
}
