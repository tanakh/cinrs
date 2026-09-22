//@compile-flags: --crate-type lib
//! `__m128i` is opaque, exactly as C's own compilers make it.
//!
//! The GNU vector extensions — `a + b` on a vector, `v[0]`, a cast between a
//! vector and an integer — are what `__attribute__((vector_size))` buys in GCC,
//! and `cinrs` refuses that attribute (see `tests/ui/gnu_unsupported.rs`). What
//! it has instead is the Intel intrinsics, which is the API real code uses:
//! `_mm_add_epi32(a, b)` rather than `a + b`, and a union or
//! `_mm_extract_epi32` rather than a subscript.

cinrs::c11! {
    #include <immintrin.h>

    __m128i added(__m128i a, __m128i b) {
        return a + b; //~ ERROR: invalid operands to binary '+'
    }

    int compared(__m128i a, __m128i b) {
        return a == b; //~ ERROR: invalid operand of type '__m128i' to unary operator '=='
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
