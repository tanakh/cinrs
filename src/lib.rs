#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
// The README is the crate documentation, and one of its blocks *is* a
// `build.rs` — a whole file whose `fn main` is the point of showing it. It is
// `no_run`, so the `TARGET` variable it unwraps is never read; clippy sees only
// a doctest wrapped in a `main` it thinks is redundant.
#![allow(clippy::needless_doctest_main)]
#![no_std]

pub use cinrs_macros::{c11, c17, c23, c89, c90, c99, gnu11, gnu17, gnu23, gnu89, gnu99};
pub use cinrs_macros::{
    include_c11, include_c17, include_c23, include_c89, include_c90, include_c99, include_gnu11,
    include_gnu17, include_gnu23, include_gnu89, include_gnu99,
};

/// The runtime the generated code links against; see [`cinrs_rt`].
///
/// It holds one thing — [`rt::Complex`], which is
/// [`num_complex::Complex`](https://docs.rs/num-complex), and C's arithmetic
/// on it — and exists only because a `_Complex` value has to have a type that
/// other crates already speak and because Annex G's multiplication and
/// division are too big to write out per expansion. Everything else `cinrs`
/// generates names `core` alone.
///
/// The generated code spells this path in full, so a renamed dependency needs
/// `#pragma cinrs crate "<path>"` to say where to find it.
///
/// Present only with the `complex` feature, which is on by default.
#[cfg(feature = "complex")]
pub use cinrs_rt as rt;
