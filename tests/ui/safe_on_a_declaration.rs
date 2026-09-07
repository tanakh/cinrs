//@compile-flags: --crate-type lib
//! `safe` on a function this unit only declares.
//!
//! Safety is a property of a *body*: the attribute drops the `unsafe` from the
//! generated item and lets `rustc` check what is inside it. A function
//! declared here and compiled elsewhere has nothing to check, and saying it is
//! safe would be a promise about a foreign object file — so it is refused
//! where it was written, rather than becoming a call that Rust believes.

cinrs::c23! {
    [[cinrs::safe]] int elsewhere(int n); //~ ERROR: 'elsewhere' is only declared here

    int use_it(int n) { return elsewhere(n); }
}
