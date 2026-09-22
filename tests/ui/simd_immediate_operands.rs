//@compile-flags: --crate-type lib
//! The operands Intel requires to be integer constant expressions.
//!
//! `core::arch` expresses them as `const` generics — `_mm_slli_epi32::<3>(v)` —
//! so a call has to carry a constant there, and `cinrs` folds the argument and
//! writes the turbofish. A non-constant is a diagnostic naming the intrinsic,
//! which is also what Intel's and GCC's own compilers do with the same C.
//!
//! The address of such an intrinsic cannot be taken at all: a function pointer
//! has nowhere to put the constant. Every *other* intrinsic's address is fine —
//! `tests/simd.rs` takes one.

cinrs::c11! {
    #include <immintrin.h>

    int variable_shift(int n) {
        __m128i v = _mm_set1_epi32(1);
        v = _mm_slli_epi32(v, n); //~ ERROR: argument 2 of '_mm_slli_epi32' has to be an integer constant expression
        return _mm_cvtsi128_si32(v);
    }

    int variable_shuffle(__m128i v, int sel) {
        v = _mm_shuffle_epi32(v, sel); //~ ERROR: argument 2 of '_mm_shuffle_epi32' has to be an integer constant expression
        return _mm_cvtsi128_si32(v);
    }

    /* A constant *expression* is fine, however it is spelled. */
    enum { LANES = _MM_SHUFFLE(3, 2, 1, 0) };
    int constant_shuffle(__m128i v) {
        return _mm_cvtsi128_si32(_mm_shuffle_epi32(v, LANES | 0));
    }

    void *address_of_an_immediate(void) {
        return (void *)_mm_slli_epi32; //~ ERROR: the address of '_mm_slli_epi32' cannot be taken
    }
}
