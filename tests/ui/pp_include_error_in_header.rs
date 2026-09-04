//! A mistake inside a header.
//!
//! The caret is on the `#include` that pulled the header in — a procedural
//! macro can only point at tokens the user wrote — and the message says where
//! in the header the problem is.

cinrs::c99! {
    #include "include/bad.h" //~ ERROR: tests/ui/include/bad.h:6:12: use of undeclared identifier
    int ok(void) { return 1; }
}

fn main() {}
