//! The GNU extensions `cinrs` knows about and cannot honour. Each is refused
//! with the reason, because ignoring one would change what the program means.
//!
//! `weak` is not among them: on a *declaration* of something defined
//! elsewhere it is a weak reference, which is what glibc's `<pthread.h>` and
//! zstd's tracing hooks write, and on a definition it is an ordinary
//! definition with a warning, since Rust's `#[linkage]` is unstable.
//! `tests/ui/gnu_weak_errors.rs` has what it does refuse.

cinrs::c99! {
    /* Accepted: nothing here defines it, so every reference is a weak one. */
    int declared_weak(void) __attribute__((weak));

    /* Accepted with a warning, as an ordinary definition. */
    __attribute__((weak)) int defined_weak(void) { return 1; }

    int aliased(void) __attribute__((alias("declared_weak"))); //~ ERROR: write a function that forwards
}

fn main() {}
