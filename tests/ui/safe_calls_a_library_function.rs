//@compile-flags: --crate-type lib
//! What a safe function may not do: call the C library.
//!
//! A function this unit only declares is compiled somewhere else, and nothing
//! about it can be checked — which is what an `unsafe extern "C"` block says
//! and why the call needs `unsafe`. This one is left to `rustc` deliberately:
//! there is no attribute that could make `abs` safe, so a message of this
//! crate's would only be in the way of the true one.

cinrs::c99! {
    #include <stdlib.h>

    #pragma cinrs safe magnitude

    int magnitude(int n) {
        return abs(n); //~ ERROR: call to unsafe function
    }
}
