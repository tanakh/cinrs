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

#![no_std]
#![warn(missing_docs)]

pub mod complex;

/// The complex number type the generated code uses.
///
/// A re-export of [`num_complex::Complex`], which is `#[repr(C)]` with the
/// real part first: `Complex<f32>` is C's `float _Complex` and `Complex<f64>`
/// is its `double _Complex` (and its `long double _Complex`, which `cinrs`
/// maps onto `double` exactly as it maps `long double`).
pub use num_complex::Complex;
