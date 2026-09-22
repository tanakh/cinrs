/* <tmmintrin.h> — the SSSE3 intrinsics, `_mm_shuffle_epi8` among them.
 *
 * Needs `__attribute__((target("ssse3")))` on the function that calls them;
 * see <xmmintrin.h>.
 */
#ifndef _CINRS_TMMINTRIN_H
#define _CINRS_TMMINTRIN_H

#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#else

#include <pmmintrin.h>

/* @generated ssse3 — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m128i _mm_abs_epi16(__m128i);
__m128i _mm_abs_epi32(__m128i);
__m128i _mm_abs_epi8(__m128i);
__m128i _mm_alignr_epi8(__m128i, __m128i, const int);
__m128i _mm_hadd_epi16(__m128i, __m128i);
__m128i _mm_hadd_epi32(__m128i, __m128i);
__m128i _mm_hadds_epi16(__m128i, __m128i);
__m128i _mm_hsub_epi16(__m128i, __m128i);
__m128i _mm_hsub_epi32(__m128i, __m128i);
__m128i _mm_hsubs_epi16(__m128i, __m128i);
__m128i _mm_maddubs_epi16(__m128i, __m128i);
__m128i _mm_mulhrs_epi16(__m128i, __m128i);
__m128i _mm_shuffle_epi8(__m128i, __m128i);
__m128i _mm_sign_epi16(__m128i, __m128i);
__m128i _mm_sign_epi32(__m128i, __m128i);
__m128i _mm_sign_epi8(__m128i, __m128i);
/* @generated end */

#endif /* x86 */
#endif /* _CINRS_TMMINTRIN_H */
