//@compile-flags: --crate-type lib
//! A variable length array in a `#![no_std]` crate that did not say so.
//!
//! Nothing in the C says which kind of crate the expansion is going into, and
//! a procedural macro cannot ask, so the `Vec` behind a variable length array
//! is spelled `::std::vec::Vec` unless `#pragma cinrs no_std` says otherwise.
//! What the user then sees is `rustc`'s own "unresolved crate `std`", with the
//! caret on the C declaration that needed it — which is what this test is
//! blessing. The fix is the pragma above plus `extern crate alloc;`; see
//! `doc/no-std.md`.

#![no_std]

cinrs::c99! {
    long sum(int n) {
        int a[n]; //~ ERROR: cannot find `std`
        a[0] = 1;
        return a[0] + (long)sizeof a;
    }
}
