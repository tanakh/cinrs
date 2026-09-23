/* <gfniintrin.h> — the gfni intrinsics, as GCC arranges them.
 *
 * Written by crates/cinrs-core/tests/x86_intrinsics.rs; the regions below
 * are regenerated from `core::arch`'s source, and an empty one is an
 * instruction set whose intrinsics are all still unstable there. Needs the
 * instruction set in `__attribute__((target("…")))` on the function that
 * calls them, and a function that passes or returns a 512-bit vector by
 * value needs `target("avx512f")`; see <immintrin.h>.
 */
#ifndef _CINRS_GFNIINTRIN_H
#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#elif !defined(_CINRS_IMMINTRIN_H)
/* On its own: <immintrin.h> includes this file back, in its place. */
#include <immintrin.h>
#else
#define _CINRS_GFNIINTRIN_H

/* @generated gfni — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m256i _mm256_gf2p8affine_epi64_epi8(__m256i, __m256i, const int);
__m256i _mm256_gf2p8affineinv_epi64_epi8(__m256i, __m256i, const int);
__m256i _mm256_gf2p8mul_epi8(__m256i, __m256i);
__m256i _mm256_mask_gf2p8affine_epi64_epi8(__m256i, __mmask32, __m256i, __m256i, const int);
__m256i _mm256_mask_gf2p8affineinv_epi64_epi8(__m256i, __mmask32, __m256i, __m256i, const int);
__m256i _mm256_mask_gf2p8mul_epi8(__m256i, __mmask32, __m256i, __m256i);
__m256i _mm256_maskz_gf2p8affine_epi64_epi8(__mmask32, __m256i, __m256i, const int);
__m256i _mm256_maskz_gf2p8affineinv_epi64_epi8(__mmask32, __m256i, __m256i, const int);
__m256i _mm256_maskz_gf2p8mul_epi8(__mmask32, __m256i, __m256i);
__m512i _mm512_gf2p8affine_epi64_epi8(__m512i, __m512i, const int);
__m512i _mm512_gf2p8affineinv_epi64_epi8(__m512i, __m512i, const int);
__m512i _mm512_gf2p8mul_epi8(__m512i, __m512i);
__m512i _mm512_mask_gf2p8affine_epi64_epi8(__m512i, __mmask64, __m512i, __m512i, const int);
__m512i _mm512_mask_gf2p8affineinv_epi64_epi8(__m512i, __mmask64, __m512i, __m512i, const int);
__m512i _mm512_mask_gf2p8mul_epi8(__m512i, __mmask64, __m512i, __m512i);
__m512i _mm512_maskz_gf2p8affine_epi64_epi8(__mmask64, __m512i, __m512i, const int);
__m512i _mm512_maskz_gf2p8affineinv_epi64_epi8(__mmask64, __m512i, __m512i, const int);
__m512i _mm512_maskz_gf2p8mul_epi8(__mmask64, __m512i, __m512i);
__m128i _mm_gf2p8affine_epi64_epi8(__m128i, __m128i, const int);
__m128i _mm_gf2p8affineinv_epi64_epi8(__m128i, __m128i, const int);
__m128i _mm_gf2p8mul_epi8(__m128i, __m128i);
__m128i _mm_mask_gf2p8affine_epi64_epi8(__m128i, __mmask16, __m128i, __m128i, const int);
__m128i _mm_mask_gf2p8affineinv_epi64_epi8(__m128i, __mmask16, __m128i, __m128i, const int);
__m128i _mm_mask_gf2p8mul_epi8(__m128i, __mmask16, __m128i, __m128i);
__m128i _mm_maskz_gf2p8affine_epi64_epi8(__mmask16, __m128i, __m128i, const int);
__m128i _mm_maskz_gf2p8affineinv_epi64_epi8(__mmask16, __m128i, __m128i, const int);
__m128i _mm_maskz_gf2p8mul_epi8(__mmask16, __m128i, __m128i);
/* @generated end */

#endif
#endif /* _CINRS_GFNIINTRIN_H */
