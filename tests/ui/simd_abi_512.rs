//! A 512-bit vector by value, and the target feature the ABI needs for it.
//!
//! This is rustc's rule, not cinrs's: a function that passes or returns an
//! `__m512`, `__m512i` or `__m512d` by value needs `avx512f`, as a 256-bit
//! vector needs `avx` — without it there is no register to put the value in,
//! and rustc refuses the definition. GCC warns about the same C with
//! `-Wpsabi`. `target("avx2")` is not enough; `target("avx512f")` is. A mask
//! is a plain integer and needs nothing.

cinrs::c11! {
    #include <immintrin.h>

    __m512 no_feature(__m512 a) { return a; } //~ ERROR: requires the `avx512f` target feature

    __attribute__((target("avx2"))) __m512d only_avx2(__m512d a) { return a; } //~ ERROR: requires the `avx512f` target feature

    __attribute__((target("avx512f"))) __m512i with_avx512f(__m512i a) { return a; }

    __attribute__((target("avx512f"))) __m512i adds(__m512i a, __m512i b) {
        return _mm512_add_epi32(a, b);
    }

    __mmask16 a_mask(__mmask16 k) { return k; }
}

fn main() {
    // Taking each function's address makes rustc generate it, which is where
    // the ABI is checked.
    std::hint::black_box(no_feature as *const ());
    std::hint::black_box(only_avx2 as *const ());
    std::hint::black_box(with_avx512f as *const ());
    std::hint::black_box(adds as *const ());
    std::hint::black_box(a_mask as *const ());
}
