/* <avx512vbmi2vlintrin.h> — the avx512vbmi2+VL intrinsics, as GCC arranges them.
 *
 * Written by crates/cinrs-core/tests/x86_intrinsics.rs; the regions below
 * are regenerated from `core::arch`'s source, and an empty one is an
 * instruction set whose intrinsics are all still unstable there. Needs the
 * instruction set in `__attribute__((target("…")))` on the function that
 * calls them, and a function that passes or returns a 512-bit vector by
 * value needs `target("avx512f")`; see <immintrin.h>.
 */
#ifndef _CINRS_AVX512VBMI2VLINTRIN_H
#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#elif !defined(_CINRS_IMMINTRIN_H)
/* On its own: <immintrin.h> includes this file back, in its place. */
#include <immintrin.h>
#else
#define _CINRS_AVX512VBMI2VLINTRIN_H

/* @generated avx512vbmi2-vl — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m256i _mm256_mask_compress_epi16(__m256i, __mmask16, __m256i);
__m256i _mm256_mask_compress_epi8(__m256i, __mmask32, __m256i);
void _mm256_mask_compressstoreu_epi16(void *, __mmask16, __m256i);
void _mm256_mask_compressstoreu_epi8(void *, __mmask32, __m256i);
__m256i _mm256_mask_expand_epi16(__m256i, __mmask16, __m256i);
__m256i _mm256_mask_expand_epi8(__m256i, __mmask32, __m256i);
__m256i _mm256_mask_expandloadu_epi16(__m256i, __mmask16, const void *);
__m256i _mm256_mask_expandloadu_epi8(__m256i, __mmask32, const void *);
__m256i _mm256_mask_shldi_epi16(__m256i, __mmask16, __m256i, __m256i, const int);
__m256i _mm256_mask_shldi_epi32(__m256i, __mmask8, __m256i, __m256i, const int);
__m256i _mm256_mask_shldi_epi64(__m256i, __mmask8, __m256i, __m256i, const int);
__m256i _mm256_mask_shldv_epi16(__m256i, __mmask16, __m256i, __m256i);
__m256i _mm256_mask_shldv_epi32(__m256i, __mmask8, __m256i, __m256i);
__m256i _mm256_mask_shldv_epi64(__m256i, __mmask8, __m256i, __m256i);
__m256i _mm256_mask_shrdi_epi16(__m256i, __mmask16, __m256i, __m256i, const int);
__m256i _mm256_mask_shrdi_epi32(__m256i, __mmask8, __m256i, __m256i, const int);
__m256i _mm256_mask_shrdi_epi64(__m256i, __mmask8, __m256i, __m256i, const int);
__m256i _mm256_mask_shrdv_epi16(__m256i, __mmask16, __m256i, __m256i);
__m256i _mm256_mask_shrdv_epi32(__m256i, __mmask8, __m256i, __m256i);
__m256i _mm256_mask_shrdv_epi64(__m256i, __mmask8, __m256i, __m256i);
__m256i _mm256_maskz_compress_epi16(__mmask16, __m256i);
__m256i _mm256_maskz_compress_epi8(__mmask32, __m256i);
__m256i _mm256_maskz_expand_epi16(__mmask16, __m256i);
__m256i _mm256_maskz_expand_epi8(__mmask32, __m256i);
__m256i _mm256_maskz_expandloadu_epi16(__mmask16, const void *);
__m256i _mm256_maskz_expandloadu_epi8(__mmask32, const void *);
__m256i _mm256_maskz_shldi_epi16(__mmask16, __m256i, __m256i, const int);
__m256i _mm256_maskz_shldi_epi32(__mmask8, __m256i, __m256i, const int);
__m256i _mm256_maskz_shldi_epi64(__mmask8, __m256i, __m256i, const int);
__m256i _mm256_maskz_shldv_epi16(__mmask16, __m256i, __m256i, __m256i);
__m256i _mm256_maskz_shldv_epi32(__mmask8, __m256i, __m256i, __m256i);
__m256i _mm256_maskz_shldv_epi64(__mmask8, __m256i, __m256i, __m256i);
__m256i _mm256_maskz_shrdi_epi16(__mmask16, __m256i, __m256i, const int);
__m256i _mm256_maskz_shrdi_epi32(__mmask8, __m256i, __m256i, const int);
__m256i _mm256_maskz_shrdi_epi64(__mmask8, __m256i, __m256i, const int);
__m256i _mm256_maskz_shrdv_epi16(__mmask16, __m256i, __m256i, __m256i);
__m256i _mm256_maskz_shrdv_epi32(__mmask8, __m256i, __m256i, __m256i);
__m256i _mm256_maskz_shrdv_epi64(__mmask8, __m256i, __m256i, __m256i);
__m256i _mm256_shldi_epi16(__m256i, __m256i, const int);
__m256i _mm256_shldi_epi32(__m256i, __m256i, const int);
__m256i _mm256_shldi_epi64(__m256i, __m256i, const int);
__m256i _mm256_shldv_epi16(__m256i, __m256i, __m256i);
__m256i _mm256_shldv_epi32(__m256i, __m256i, __m256i);
__m256i _mm256_shldv_epi64(__m256i, __m256i, __m256i);
__m256i _mm256_shrdi_epi16(__m256i, __m256i, const int);
__m256i _mm256_shrdi_epi32(__m256i, __m256i, const int);
__m256i _mm256_shrdi_epi64(__m256i, __m256i, const int);
__m256i _mm256_shrdv_epi16(__m256i, __m256i, __m256i);
__m256i _mm256_shrdv_epi32(__m256i, __m256i, __m256i);
__m256i _mm256_shrdv_epi64(__m256i, __m256i, __m256i);
__m128i _mm_mask_compress_epi16(__m128i, __mmask8, __m128i);
__m128i _mm_mask_compress_epi8(__m128i, __mmask16, __m128i);
void _mm_mask_compressstoreu_epi16(void *, __mmask8, __m128i);
void _mm_mask_compressstoreu_epi8(void *, __mmask16, __m128i);
__m128i _mm_mask_expand_epi16(__m128i, __mmask8, __m128i);
__m128i _mm_mask_expand_epi8(__m128i, __mmask16, __m128i);
__m128i _mm_mask_expandloadu_epi16(__m128i, __mmask8, const void *);
__m128i _mm_mask_expandloadu_epi8(__m128i, __mmask16, const void *);
__m128i _mm_mask_shldi_epi16(__m128i, __mmask8, __m128i, __m128i, const int);
__m128i _mm_mask_shldi_epi32(__m128i, __mmask8, __m128i, __m128i, const int);
__m128i _mm_mask_shldi_epi64(__m128i, __mmask8, __m128i, __m128i, const int);
__m128i _mm_mask_shldv_epi16(__m128i, __mmask8, __m128i, __m128i);
__m128i _mm_mask_shldv_epi32(__m128i, __mmask8, __m128i, __m128i);
__m128i _mm_mask_shldv_epi64(__m128i, __mmask8, __m128i, __m128i);
__m128i _mm_mask_shrdi_epi16(__m128i, __mmask8, __m128i, __m128i, const int);
__m128i _mm_mask_shrdi_epi32(__m128i, __mmask8, __m128i, __m128i, const int);
__m128i _mm_mask_shrdi_epi64(__m128i, __mmask8, __m128i, __m128i, const int);
__m128i _mm_mask_shrdv_epi16(__m128i, __mmask8, __m128i, __m128i);
__m128i _mm_mask_shrdv_epi32(__m128i, __mmask8, __m128i, __m128i);
__m128i _mm_mask_shrdv_epi64(__m128i, __mmask8, __m128i, __m128i);
__m128i _mm_maskz_compress_epi16(__mmask8, __m128i);
__m128i _mm_maskz_compress_epi8(__mmask16, __m128i);
__m128i _mm_maskz_expand_epi16(__mmask8, __m128i);
__m128i _mm_maskz_expand_epi8(__mmask16, __m128i);
__m128i _mm_maskz_expandloadu_epi16(__mmask8, const void *);
__m128i _mm_maskz_expandloadu_epi8(__mmask16, const void *);
__m128i _mm_maskz_shldi_epi16(__mmask8, __m128i, __m128i, const int);
__m128i _mm_maskz_shldi_epi32(__mmask8, __m128i, __m128i, const int);
__m128i _mm_maskz_shldi_epi64(__mmask8, __m128i, __m128i, const int);
__m128i _mm_maskz_shldv_epi16(__mmask8, __m128i, __m128i, __m128i);
__m128i _mm_maskz_shldv_epi32(__mmask8, __m128i, __m128i, __m128i);
__m128i _mm_maskz_shldv_epi64(__mmask8, __m128i, __m128i, __m128i);
__m128i _mm_maskz_shrdi_epi16(__mmask8, __m128i, __m128i, const int);
__m128i _mm_maskz_shrdi_epi32(__mmask8, __m128i, __m128i, const int);
__m128i _mm_maskz_shrdi_epi64(__mmask8, __m128i, __m128i, const int);
__m128i _mm_maskz_shrdv_epi16(__mmask8, __m128i, __m128i, __m128i);
__m128i _mm_maskz_shrdv_epi32(__mmask8, __m128i, __m128i, __m128i);
__m128i _mm_maskz_shrdv_epi64(__mmask8, __m128i, __m128i, __m128i);
__m128i _mm_shldi_epi16(__m128i, __m128i, const int);
__m128i _mm_shldi_epi32(__m128i, __m128i, const int);
__m128i _mm_shldi_epi64(__m128i, __m128i, const int);
__m128i _mm_shldv_epi16(__m128i, __m128i, __m128i);
__m128i _mm_shldv_epi32(__m128i, __m128i, __m128i);
__m128i _mm_shldv_epi64(__m128i, __m128i, __m128i);
__m128i _mm_shrdi_epi16(__m128i, __m128i, const int);
__m128i _mm_shrdi_epi32(__m128i, __m128i, const int);
__m128i _mm_shrdi_epi64(__m128i, __m128i, const int);
__m128i _mm_shrdv_epi16(__m128i, __m128i, __m128i);
__m128i _mm_shrdv_epi32(__m128i, __m128i, __m128i);
__m128i _mm_shrdv_epi64(__m128i, __m128i, __m128i);
/* @generated end */

#endif
#endif /* _CINRS_AVX512VBMI2VLINTRIN_H */
