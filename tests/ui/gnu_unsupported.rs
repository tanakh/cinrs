//! The GNU extensions `cinrs` knows about and cannot honour. Each is refused
//! with the reason, because ignoring one would change what the program means.
//!
//! `weak` is the one with two answers: on a *declaration* of something defined
//! elsewhere it costs nothing and is accepted, which is what lets glibc's
//! `<pthread.h>` be read; on a definition it is refused, since that is where
//! Rust's unstable `#[linkage]` would be needed.

cinrs::c99! {
    /* Accepted: nothing here defines it, so there is no linkage to weaken. */
    int declared_weak(void) __attribute__((weak));

    __attribute__((weak)) int defined_weak(void) { return 1; } //~ ERROR: weak linkage cannot be asked for

    typedef int v4si __attribute__((vector_size(16))); //~ ERROR: the vector extensions need `core::simd`

    int aliased(void) __attribute__((alias("declared_weak"))); //~ ERROR: write a function that forwards
}

fn main() {}
