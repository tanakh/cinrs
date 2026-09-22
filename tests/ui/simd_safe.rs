//@compile-flags: --crate-type lib
//! An intrinsic and `[[cinrs::safe]]` cannot both be had.
//!
//! Every `core::arch` intrinsic is a `#[target_feature]` function, and Rust
//! makes such a function unsafe to call from anywhere that does not carry the
//! same instruction set — because only the program knows whether the processor
//! running it has those instructions. A safe function has no `unsafe` block to
//! call one from, and it cannot carry the attribute either: that would make
//! *it* unsafe to call, which is the opposite of what `[[cinrs::safe]]`
//! promises. So both are refused, with the instruction set named.

cinrs::c23! {
    #include <immintrin.h>

    [[cinrs::safe]] int lanes(int a) {
        __m128i v = _mm_set1_epi32(a); //~ ERROR: '_mm_set1_epi32' needs the 'sse2' instruction set
        return _mm_cvtsi128_si32(v); //~ ERROR: '_mm_cvtsi128_si32' needs the 'sse2' instruction set
    }

    [[cinrs::safe]] __attribute__((target("avx2"))) int wide(int a) { //~ ERROR: 'wide' asks for the 'avx2' instruction set
        return a;
    }
}
