//! The runtime: the one library the generated code calls besides Rust's own.
//!
//! A unit that uses the complex types calls `::cinrs::rt`, which in a `c99!`
//! block is the `cinrs` crate's re-export of `cinrs-rt`. A C compiler has no
//! Cargo to ask, so `ccinrs` carries `cinrs-rt`'s source, `cinrs_rt::SOURCES`,
//! and builds the same two crates with the `rustc` it runs the first time a
//! program needs them: `cinrs_rt` from that source — without its
//! `num-complex` feature, so with a `Complex` of its own — and `cinrs`, which
//! is `pub use cinrs_rt as rt;`. They are kept, and every later run with the
//! same `rustc`, target and `ccinrs` links the same ones.
//!
//! Nothing here knows what the runtime holds; "Without Cargo" in `cinrs_rt`'s
//! documentation is what keeps it compilable this way.

use std::cell::OnceCell;
use std::ffi::OsString;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::args::Invocation;
use crate::driver::{Failure, run_rustc};
use crate::rustc::Rustc;

/// The crate the generated code names, as the `cinrs` crate has it.
const FACADE: &str = "\
//! `::cinrs::rt`, as the `cinrs` crate provides it.
#![no_std]
pub use cinrs_rt as rt;
";

/// The runtime as one module, `rt`, for the Rust file `-S` writes: `cinrs_rt`'s
/// `lib.rs`, with each `mod name;` replaced by the module's file, and without
/// `#![no_std]`, which only a crate's root may say.
pub fn source() -> String {
    let mut text = String::from(
        "/// The runtime of the complex types: `cinrs_rt`, as ccinrs carries it.\npub mod rt {\n",
    );
    inline("lib.rs", &mut text);
    text.push_str("}\n");
    text
}

/// Appends `cinrs_rt`'s file `name`, with its `mod` declarations inlined.
/// The text is copied as it is, not indented, so that nothing in it changes.
fn inline(name: &str, out: &mut String) {
    let text = cinrs_rt::SOURCES
        .iter()
        .find_map(|(file, text)| (*file == name).then_some(*text))
        .unwrap_or_else(|| panic!("cinrs_rt::SOURCES has no {name}"));
    // A module declared in `lib.rs` or a `mod.rs` is a file beside it; one
    // declared in `a.rs` is in `a/`.
    let dir = match name.rsplit_once('/') {
        _ if name == "lib.rs" => String::new(),
        Some((dir, "mod.rs")) => format!("{dir}/"),
        _ => format!("{}/", name.trim_end_matches(".rs")),
    };
    for line in text.lines() {
        let code = line.trim();
        if code == "#![no_std]" {
            continue;
        }
        let declared = code
            .strip_suffix(';')
            .and_then(|head| Some((head, head.rsplit_once("mod ")?.1)))
            .filter(|(head, module)| {
                (head.starts_with("mod ") || head.starts_with("pub"))
                    && !module.is_empty()
                    && module
                        .chars()
                        .all(|c| c == '_' || c.is_ascii_alphanumeric())
            });
        match declared {
            Some((head, module)) => {
                out.push_str(head);
                out.push_str(" {\n");
                let file = format!("{dir}{module}.rs");
                if cinrs_rt::SOURCES.iter().any(|(name, _)| *name == file) {
                    inline(&file, out);
                } else {
                    inline(&format!("{dir}{module}/mod.rs"), out);
                }
                out.push_str("}\n");
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
}

/// The arguments that let `rustc` find the runtime: the facade by name and
/// `cinrs_rt` beside it.
pub fn extern_args(library: &Path) -> [OsString; 4] {
    let dir = library.parent().unwrap_or(Path::new("."));
    [
        "--extern".into(),
        format!("cinrs={}", library.display()).into(),
        "-L".into(),
        format!("dependency={}", dir.display()).into(),
    ]
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

    /// The `libcinrs.rlib` for `triple`, with `libcinrs_rt.rlib` beside it:
    /// the ones kept from an earlier run, or ones compiled now — into the
    /// cache, or into `work` when there is no cache directory.
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
    let mut hasher = DefaultHasher::new();
    (
        env!("CARGO_PKG_VERSION"),
        &rustc.release,
        &rustc.commit,
        triple,
        cinrs_rt::SOURCES,
        FACADE,
    )
        .hash(&mut hasher);
    let key = format!("{:016x}", hasher.finish());
    let parent = match cache_root() {
        Some(root) => root.join(triple),
        None => work.join("runtime"),
    };
    let dir = parent.join(&key);
    let library = dir.join("libcinrs.rlib");
    if library.is_file() {
        return Ok(library);
    }
    // Built in a directory of this process's own, which becomes `dir` in one
    // rename once it is complete — so that two runs at once, as `make -j`
    // starts them, never see half a runtime.
    let scratch = parent.join(format!("{key}.{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    for (name, text) in cinrs_rt::SOURCES.iter().chain([&("cinrs.rs", FACADE)]) {
        let path = if *name == "cinrs.rs" {
            scratch.join(name)
        } else {
            scratch.join("src").join(name)
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        std::fs::write(&path, text)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    if inv.verbose {
        eprintln!("ccinrs: compiling the runtime into {}", dir.display());
    }
    let compile = |name: &str, root: PathBuf, externs: &[String]| {
        let mut cmd = rustc.command();
        cmd.args([
            "--edition",
            "2024",
            "--crate-type",
            "rlib",
            "--crate-name",
            name,
        ])
        .args(["--target", triple])
        // It is compiled once and called from every complex operation,
        // and it has nothing to check at run time that its tests have not.
        .args([
            "-C",
            "opt-level=3",
            "-C",
            "codegen-units=1",
            "--cap-lints",
            "allow",
        ])
        // What `rustc` says about a type of the runtime points at the
        // source, which stays beside the libraries.
        .arg(format!(
            "--remap-path-prefix={}={}",
            scratch.display(),
            dir.display()
        ));
        for name in externs {
            cmd.arg("--extern").arg(format!(
                "{name}={}",
                scratch.join(format!("lib{name}.rlib")).display()
            ));
        }
        cmd.arg("-o")
            .arg(scratch.join(format!("lib{name}.rlib")))
            .arg(&root);
        run_rustc(inv, cmd, || {
            format!(
                "rustc could not compile the runtime ({}); this is a bug in ccinrs",
                root.display()
            )
        })
    };
    let built = compile("cinrs_rt", scratch.join("src/lib.rs"), &[])
        .and_then(|()| compile("cinrs", scratch.join("cinrs.rs"), &["cinrs_rt".to_owned()]));
    // Another run may have put its runtime in place first, which is as good.
    let placed = built.and_then(|()| match std::fs::rename(&scratch, &dir) {
        Ok(()) => Ok(()),
        Err(_) if library.is_file() => Ok(()),
        Err(error) => Err(Failure::Message(format!(
            "cannot create {}: {error}",
            dir.display()
        ))),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The module `-S` carries is all of `cinrs_rt`, its `mod` declarations
    /// filled in and nothing a module may not say.
    #[test]
    fn the_runtime_inlines_as_one_module() {
        let text = source();
        assert!(!text.lines().any(|line| line.trim() == "#![no_std]"));
        for (name, _) in cinrs_rt::SOURCES {
            let module = name.trim_end_matches(".rs");
            if *name != "lib.rs" {
                assert!(text.contains(&format!("mod {module} {{\n")), "{name}");
                assert!(
                    !text
                        .lines()
                        .any(|line| line.trim().ends_with(&format!("mod {module};"))),
                    "{name}"
                );
            }
        }
        assert!(syn::parse_file(&text).is_ok());
    }
}
