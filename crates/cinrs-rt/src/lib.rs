//! The runtime the code [`cinrs`](https://docs.rs/cinrs) generates links
//! against.
//!
//! There is exactly one thing in it — C's complex arithmetic — and it is here
//! for two reasons.
//!
//! * **A type the ecosystem already has.** `double _Complex` needs a Rust type
//!   with the same layout, and inventing one would make every `c99!` block's
//!   complex values incompatible with every other crate's. [`Complex`] is
//!   [`num_complex::Complex`], which is `#[repr(C)]`, `Copy`, and the type the
//!   numeric half of crates.io already speaks; `Complex<f64>` is two `f64`s in
//!   declaration order, so it *is* C's `double _Complex` as far as the ABI is
//!   concerned.
//! * **Semantics that would otherwise be copied into every expansion.** C's
//!   multiplication and division of two complex values are not the naive
//!   formulas: C99 Annex G.5.1 requires an infinite result to be recovered
//!   where the naive one would be NaN + iNaN, which is a couple of dozen lines
//!   apiece. A procedural macro that wrote them out per unit — or worse, per
//!   expression — would bloat every expansion; [`complex`] holds one copy.
//!
//! Nothing else belongs here. Everything else `cinrs` generates is `core`-only
//! (with `alloc` or `std` for a variable length array, `alloca` and
//! `_Thread_local`), and this crate is `#![no_std]` so that `_Complex` does not
//! change that.
//!
//! # Versioning
//!
//! This crate is versioned *with* `cinrs` and is not a stable interface of its
//! own: the generated code names `::cinrs::rt`, the re-export the `cinrs`
//! crate provides under its `complex` feature, and the two always come from
//! the same release. Use [`Complex`] from here — or from `num-complex`
//! directly, which is the same type — to hand a complex value to a `c99!`
//! function or to read one back.
//!
//! ```
//! use cinrs_rt::Complex;
//! use cinrs_rt::complex::mul_f64;
//!
//! let z = Complex::new(1.0f64, 2.0);
//! let w = Complex::new(3.0f64, -4.0);
//! assert_eq!(mul_f64(z, w), Complex::new(11.0, 2.0));
//! ```
//!
//! # Without Cargo
//!
//! `ccinrs`, the C compiler built on cinrs, cannot hand its user a Cargo
//! dependency, so it carries this crate's source ([`SOURCES`]) and compiles
//! it with the `rustc` it runs. That is why the crate keeps to three rules:
//!
//! * **`core` and nothing else.** No dependency is reachable without Cargo,
//!   which is why `num-complex` is an optional (default) feature: without it,
//!   [`Complex`] is a type of this crate's own with the same layout and the
//!   same componentwise `+`, `-` and unary `-`, which is all the generated
//!   code asks of it.
//! * **Every file in [`SOURCES`]**, which a test checks. (It is there under
//!   the default `sources` feature, which the copy `ccinrs` compiles goes
//!   without.)
//! * **Paths relative to the module, not to the crate** (`super::Complex`,
//!   never `crate::Complex`): the Rust file `ccinrs -S` writes carries the
//!   crate as a module of its own.

#![no_std]
#![warn(missing_docs)]

pub mod complex;
#[cfg(not(feature = "num-complex"))]
mod standalone;

/// The complex number type the generated code uses.
///
/// A re-export of [`num_complex::Complex`], which is `#[repr(C)]` with the
/// real part first: `Complex<f32>` is C's `float _Complex` and `Complex<f64>`
/// is its `double _Complex` (and its `long double _Complex`, which `cinrs`
/// maps onto `double` exactly as it maps `long double`).
#[cfg(feature = "num-complex")]
pub use num_complex::Complex;
#[cfg(not(feature = "num-complex"))]
pub use standalone::Complex;

/// This crate's source, file by file, by its path under `src/`, for `ccinrs`;
/// see [Without Cargo](self#without-cargo). Not an interface: it changes with
/// every release.
#[cfg(feature = "sources")]
#[doc(hidden)]
pub const SOURCES: &[(&str, &str)] = &[
    ("lib.rs", include_str!("lib.rs")),
    ("complex.rs", include_str!("complex.rs")),
    ("standalone.rs", include_str!("standalone.rs")),
];

#[cfg(all(test, feature = "sources"))]
mod tests {
    extern crate std;

    use std::string::String;
    use std::vec::Vec;

    /// [`super::SOURCES`] is every file under `src/`, as it is.
    #[test]
    fn the_sources_are_all_here() {
        fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).expect("src/ reads") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    walk(&path, root, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let relative = path.strip_prefix(root).expect("under src/");
                    out.push(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        walk(&root, &root, &mut files);
        files.sort();
        let mut listed: Vec<String> = super::SOURCES
            .iter()
            .map(|(name, _)| String::from(*name))
            .collect();
        listed.sort();
        assert_eq!(listed, files, "SOURCES in src/lib.rs must list every file");
        for (name, text) in super::SOURCES {
            let on_disk = std::fs::read_to_string(root.join(name)).expect("the file");
            assert_eq!(*text, on_disk, "{name}");
        }
    }
}
