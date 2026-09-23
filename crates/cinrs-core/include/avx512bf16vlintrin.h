/* <avx512bf16vlintrin.h> — the avx512bf16+VL intrinsics, as GCC arranges them.
 *
 * Written by crates/cinrs-core/tests/x86_intrinsics.rs; the regions below
 * are regenerated from `core::arch`'s source, and an empty one is an
 * instruction set whose intrinsics are all still unstable there. Needs the
 * instruction set in `__attribute__((target("…")))` on the function that
 * calls them, and a function that passes or returns a 512-bit vector by
 * value needs `target("avx512f")`; see <immintrin.h>.
 */
#ifndef _CINRS_AVX512BF16VLINTRIN_H
#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#elif !defined(_CINRS_IMMINTRIN_H)
/* On its own: <immintrin.h> includes this file back, in its place. */
#include <immintrin.h>
#else
#define _CINRS_AVX512BF16VLINTRIN_H

/* @generated avx512bf16-vl — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m256bh _mm256_cvtne2ps_pbh(__m256, __m256);
__m128bh _mm256_cvtneps_pbh(__m256);
__m256 _mm256_cvtpbh_ps(__m128bh);
__m256 _mm256_dpbf16_ps(__m256, __m256bh, __m256bh);
__m256bh _mm256_mask_cvtne2ps_pbh(__m256bh, __mmask16, __m256, __m256);
__m128bh _mm256_mask_cvtneps_pbh(__m128bh, __mmask8, __m256);
__m256 _mm256_mask_cvtpbh_ps(__m256, __mmask8, __m128bh);
__m256 _mm256_mask_dpbf16_ps(__m256, __mmask8, __m256bh, __m256bh);
__m256bh _mm256_maskz_cvtne2ps_pbh(__mmask16, __m256, __m256);
__m128bh _mm256_maskz_cvtneps_pbh(__mmask8, __m256);
__m256 _mm256_maskz_cvtpbh_ps(__mmask8, __m128bh);
__m256 _mm256_maskz_dpbf16_ps(__mmask8, __m256, __m256bh, __m256bh);
__m128bh _mm_cvtne2ps_pbh(__m128, __m128);
__m128bh _mm_cvtneps_pbh(__m128);
__m128 _mm_cvtpbh_ps(__m128bh);
__m128 _mm_dpbf16_ps(__m128, __m128bh, __m128bh);
__m128bh _mm_mask_cvtne2ps_pbh(__m128bh, __mmask8, __m128, __m128);
__m128bh _mm_mask_cvtneps_pbh(__m128bh, __mmask8, __m128);
__m128 _mm_mask_cvtpbh_ps(__m128, __mmask8, __m128bh);
__m128 _mm_mask_dpbf16_ps(__m128, __mmask8, __m128bh, __m128bh);
__m128bh _mm_maskz_cvtne2ps_pbh(__mmask8, __m128, __m128);
__m128bh _mm_maskz_cvtneps_pbh(__mmask8, __m128);
__m128 _mm_maskz_cvtpbh_ps(__mmask8, __m128bh);
__m128 _mm_maskz_dpbf16_ps(__mmask8, __m128, __m128bh, __m128bh);
/* @generated end */

#endif
#endif /* _CINRS_AVX512BF16VLINTRIN_H */
