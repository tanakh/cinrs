//@compile-flags: --crate-type lib
//! Asking for `safe` and getting the request itself wrong.
//!
//! `cinrs` is this crate's own attribute namespace and `#pragma cinrs` its own
//! pragma, so a name neither of them knows is a mistake worth reporting rather
//! than another vendor's hint to be ignored — which is what C23 6.7.13.1p3
//! asks for, and what `[[clang::…]]` gets here.

cinrs::c23! {
    #pragma cinrs safe nowhere //~ ERROR: which this unit does not declare

    #pragma cinrs safe //~ ERROR: needs the name of at least one function

    //~v ERROR: unknown 'cinrs' attribute 'sound'
    [[cinrs::sound]] int f(int n) { return n; }
}
