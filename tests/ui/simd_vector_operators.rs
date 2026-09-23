//@compile-flags: --crate-type lib
//! What GCC's vector extension gives `__m128i` that cinrs does not.
//!
//! The operators with an intrinsic that does the same — `a + b` on 64-bit
//! lanes, `& | ^ ~`, and the floating ones — are lowered to it (see
//! `tests/simd.rs`). The rest are refused with the intrinsic to write: integer
//! `*` and the comparisons have no single SSE2 instruction for GCC's 64-bit
//! lanes, and `v[0]` and a cast between a vector and an integer are not here;
//! a union or `_mm_extract_epi32` is how to reach a lane.

cinrs::c11! {
    #include <immintrin.h>

    __m128i multiplied(__m128i a, __m128i b) {
        return a * b; //~ ERROR: '*' on '__m128i' is not supported
    }

    int compared(__m128i a, __m128i b) {
        return a == b; //~ ERROR: comparing '__m128i' vectors is not supported
    }

    int subscripted(__m128i v) {
        return v[0]; //~ ERROR: subscripted value is not an array or pointer
    }

    __m128i from_an_integer(long long x) {
        return (__m128i)x; //~ ERROR: a cast to '__m128i' is not allowed
    }

    long long to_an_integer(__m128i v) {
        return (long long)v; //~ ERROR: cannot cast an expression of type '__m128i' to 'long long'
    }

    int as_a_condition(__m128i v) {
        return v ? 1 : 0; //~ ERROR: value of type '__m128i' is not contextually convertible to a condition
    }

    int ia32_builtin(void) {
        return __builtin_ia32_pmovmskb128(_mm_setzero_si128()); //~ ERROR: write the Intel intrinsic instead
    }
}
