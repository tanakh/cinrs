//@compile-flags: --crate-type lib
//! Declaring an intrinsic by hand instead of including the header.
//!
//! The name is what makes an intrinsic one, so a declaration that agrees with
//! `<immintrin.h>` works without it — which is what code that writes its own
//! prototypes to avoid the header does. One that *disagrees* is refused rather
//! than left to be a symbol: there is no `_mm_add_ps` anywhere to link against,
//! so a call would be an unresolved reference with nothing in the message about
//! the C. A definition is different again: a unit that defines `_mm_movemask_epi8`
//! means its own function, whatever the name says, and gets it.

cinrs::c11! {
    int _mm_add_ps(void); //~ ERROR: '_mm_add_ps' is an x86 intrinsic and takes 2 arguments, not 0

    /* Defined here, so it is this unit's own function and nothing is mapped. */
    int _mm_movemask_epi8(int x) { return x + 1; }
    int use_own(void) { return _mm_movemask_epi8(3); }
}

cinrs::c11! {
    #pragma cinrs target "i686-unknown-linux-gnu"
    #include <immintrin.h>

    /* `core::arch::x86` has no 64-bit deposit, and the header hides it behind
     * `#ifdef __x86_64__`; declaring it by hand says so. */
    unsigned long long _pdep_u64(unsigned long long, unsigned long long); //~ ERROR: core::arch has only on x86-64
}
