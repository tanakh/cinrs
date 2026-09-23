/* <avx512vbmi2intrin.h> — the avx512vbmi2 intrinsics, as GCC arranges them.
 *
 * Written by crates/cinrs-core/tests/x86_intrinsics.rs; the regions below
 * are regenerated from `core::arch`'s source, and an empty one is an
 * instruction set whose intrinsics are all still unstable there. Needs the
 * instruction set in `__attribute__((target("…")))` on the function that
 * calls them, and a function that passes or returns a 512-bit vector by
 * value needs `target("avx512f")`; see <immintrin.h>.
 */
#ifndef _CINRS_AVX512VBMI2INTRIN_H
#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#elif !defined(_CINRS_IMMINTRIN_H)
/* On its own: <immintrin.h> includes this file back, in its place. */
#include <immintrin.h>
#else
#define _CINRS_AVX512VBMI2INTRIN_H

/* @generated avx512vbmi2 — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m512i _mm512_mask_compress_epi16(__m512i, __mmask32, __m512i);
__m512i _mm512_mask_compress_epi8(__m512i, __mmask64, __m512i);
void _mm512_mask_compressstoreu_epi16(short *, __mmask32, __m512i);
void _mm512_mask_compressstoreu_epi8(char *, __mmask64, __m512i);
__m512i _mm512_mask_expand_epi16(__m512i, __mmask32, __m512i);
__m512i _mm512_mask_expand_epi8(__m512i, __mmask64, __m512i);
__m512i _mm512_mask_expandloadu_epi16(__m512i, __mmask32, const short *);
__m512i _mm512_mask_expandloadu_epi8(__m512i, __mmask64, const char *);
__m512i _mm512_mask_shldi_epi16(__m512i, __mmask32, __m512i, __m512i, const int);
__m512i _mm512_mask_shldi_epi32(__m512i, __mmask16, __m512i, __m512i, const int);
__m512i _mm512_mask_shldi_epi64(__m512i, __mmask8, __m512i, __m512i, const int);
__m512i _mm512_mask_shldv_epi16(__m512i, __mmask32, __m512i, __m512i);
__m512i _mm512_mask_shldv_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_shldv_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512i _mm512_mask_shrdi_epi16(__m512i, __mmask32, __m512i, __m512i, const int);
__m512i _mm512_mask_shrdi_epi32(__m512i, __mmask16, __m512i, __m512i, const int);
__m512i _mm512_mask_shrdi_epi64(__m512i, __mmask8, __m512i, __m512i, const int);
__m512i _mm512_mask_shrdv_epi16(__m512i, __mmask32, __m512i, __m512i);
__m512i _mm512_mask_shrdv_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_shrdv_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512i _mm512_maskz_compress_epi16(__mmask32, __m512i);
__m512i _mm512_maskz_compress_epi8(__mmask64, __m512i);
__m512i _mm512_maskz_expand_epi16(__mmask32, __m512i);
__m512i _mm512_maskz_expand_epi8(__mmask64, __m512i);
__m512i _mm512_maskz_expandloadu_epi16(__mmask32, const short *);
__m512i _mm512_maskz_expandloadu_epi8(__mmask64, const char *);
__m512i _mm512_maskz_shldi_epi16(__mmask32, __m512i, __m512i, const int);
__m512i _mm512_maskz_shldi_epi32(__mmask16, __m512i, __m512i, const int);
__m512i _mm512_maskz_shldi_epi64(__mmask8, __m512i, __m512i, const int);
__m512i _mm512_maskz_shldv_epi16(__mmask32, __m512i, __m512i, __m512i);
__m512i _mm512_maskz_shldv_epi32(__mmask16, __m512i, __m512i, __m512i);
__m512i _mm512_maskz_shldv_epi64(__mmask8, __m512i, __m512i, __m512i);
__m512i _mm512_maskz_shrdi_epi16(__mmask32, __m512i, __m512i, const int);
__m512i _mm512_maskz_shrdi_epi32(__mmask16, __m512i, __m512i, const int);
__m512i _mm512_maskz_shrdi_epi64(__mmask8, __m512i, __m512i, const int);
__m512i _mm512_maskz_shrdv_epi16(__mmask32, __m512i, __m512i, __m512i);
__m512i _mm512_maskz_shrdv_epi32(__mmask16, __m512i, __m512i, __m512i);
__m512i _mm512_maskz_shrdv_epi64(__mmask8, __m512i, __m512i, __m512i);
__m512i _mm512_shldi_epi16(__m512i, __m512i, const int);
__m512i _mm512_shldi_epi32(__m512i, __m512i, const int);
__m512i _mm512_shldi_epi64(__m512i, __m512i, const int);
__m512i _mm512_shldv_epi16(__m512i, __m512i, __m512i);
__m512i _mm512_shldv_epi32(__m512i, __m512i, __m512i);
__m512i _mm512_shldv_epi64(__m512i, __m512i, __m512i);
__m512i _mm512_shrdi_epi16(__m512i, __m512i, const int);
__m512i _mm512_shrdi_epi32(__m512i, __m512i, const int);
__m512i _mm512_shrdi_epi64(__m512i, __m512i, const int);
__m512i _mm512_shrdv_epi16(__m512i, __m512i, __m512i);
__m512i _mm512_shrdv_epi32(__m512i, __m512i, __m512i);
__m512i _mm512_shrdv_epi64(__m512i, __m512i, __m512i);
/* @generated end */

#endif
#endif /* _CINRS_AVX512VBMI2INTRIN_H */
