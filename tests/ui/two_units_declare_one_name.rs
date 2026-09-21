//@compile-flags: --crate-type lib
//! Two units in one Rust module both declaring `strlen`, and Rust naming it.
//!
//! A function a unit declares and does not define is generated under its own C
//! name and glob re-exported, which is what makes `#include <string.h>` enough
//! for Rust to call the C library. Two units in one Rust scope therefore export
//! the same name twice — which is not a problem for either unit's C, and not a
//! problem for Rust either until Rust *uses* the name. Then it is `rustc`'s own
//! `E0659`, exactly as it is for the `struct` two units both generate, and an
//! ordinary Rust `mod` around one of the invocations is the answer.

cinrs::c99! {
    #include <string.h>
    unsigned long first_length(const char *s) { return strlen(s); }
}

cinrs::c99! {
    #include <string.h>
    unsigned long second_length(const char *s) { return strlen(s); }
}

/// Each unit's own C is unaffected: `first_length` and `second_length` each
/// call the `strlen` their own unit declared.
pub unsafe fn lengths(s: *const core::ffi::c_char) -> (u64, u64) {
    unsafe { (first_length(s) as u64, second_length(s) as u64) }
}

/// Rust asking for `strlen` is the ambiguity, and the one thing that is.
pub unsafe fn length(s: *const core::ffi::c_char) -> u64 {
    unsafe { strlen(s) as u64 } //~ ERROR: `strlen` is ambiguous
}
