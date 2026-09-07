//@compile-flags: --crate-type lib
//! `safe` where there is no function definition to apply it to.
//!
//! A `typedef` for a function type and an object of function pointer type both
//! *look* like the place to write it, and neither has a body: a call through a
//! function pointer is a foreign call whatever any declaration said, so the
//! attribute would promise something nothing delivers.

cinrs::c23! {
    [[cinrs::safe]] typedef int Unary(int); //~ ERROR: 'safe' is not allowed on a 'typedef'

    [[cinrs::safe]] int (*handler)(int); //~ ERROR: 'safe' is not allowed on an object
}
