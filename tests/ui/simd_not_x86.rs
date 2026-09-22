//@compile-flags: --crate-type lib
//! The Intel intrinsics headers on a target that is not x86.
//!
//! There is nothing to map them onto — `core::arch::aarch64` has NEON, which is
//! a different instruction set with different names — so the header says so
//! where the `#include` is written rather than letting a thousand unknown type
//! names out. A program that means to be portable guards the include, and the
//! diagnostic says which macro to guard it with.
//!
//! `<mmintrin.h>` is the same answer for a different reason: `core::arch`
//! dropped MMX and `__m64` altogether.

cinrs::c11! {
    #pragma cinrs target "aarch64-unknown-linux-gnu"
    #include <immintrin.h> //~ ERROR: the Intel intrinsics headers are x86 only
    int f(void) { return 0; }
}

cinrs::c11! {
    #include <mmintrin.h> //~ ERROR: cinrs has no MMX and no '__m64'
    int g(void) { return 0; }
}
