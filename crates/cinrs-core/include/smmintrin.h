/* <smmintrin.h> — the SSE4.1 intrinsics.
 *
 * Needs `__attribute__((target("sse4.1")))` on the function that calls them;
 * see <xmmintrin.h>.
 */
#ifndef _CINRS_SMMINTRIN_H
#define _CINRS_SMMINTRIN_H

#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#else

#include <tmmintrin.h>

/* @generated constants — see crates/cinrs-core/tests/x86_intrinsics.rs */
#define _MM_FROUND_TO_NEAREST_INT 0x0000
#define _MM_FROUND_TO_NEG_INF 0x0001
#define _MM_FROUND_TO_POS_INF 0x0002
#define _MM_FROUND_TO_ZERO 0x0003
#define _MM_FROUND_CUR_DIRECTION 0x0004
#define _MM_FROUND_RAISE_EXC 0x0000
#define _MM_FROUND_NO_EXC 0x0008
#define _MM_FROUND_NINT 0x0000
#define _MM_FROUND_FLOOR (_MM_FROUND_RAISE_EXC | _MM_FROUND_TO_NEG_INF)
#define _MM_FROUND_CEIL (_MM_FROUND_RAISE_EXC | _MM_FROUND_TO_POS_INF)
#define _MM_FROUND_TRUNC (_MM_FROUND_RAISE_EXC | _MM_FROUND_TO_ZERO)
#define _MM_FROUND_RINT (_MM_FROUND_RAISE_EXC | _MM_FROUND_CUR_DIRECTION)
#define _MM_FROUND_NEARBYINT (_MM_FROUND_NO_EXC | _MM_FROUND_CUR_DIRECTION)
/* @generated end */

/* @generated sse4.1 — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m128i _mm_blend_epi16(__m128i, __m128i, const int);
__m128d _mm_blend_pd(__m128d, __m128d, const int);
__m128 _mm_blend_ps(__m128, __m128, const int);
__m128i _mm_blendv_epi8(__m128i, __m128i, __m128i);
__m128d _mm_blendv_pd(__m128d, __m128d, __m128d);
__m128 _mm_blendv_ps(__m128, __m128, __m128);
__m128d _mm_ceil_pd(__m128d);
__m128 _mm_ceil_ps(__m128);
__m128d _mm_ceil_sd(__m128d, __m128d);
__m128 _mm_ceil_ss(__m128, __m128);
__m128i _mm_cmpeq_epi64(__m128i, __m128i);
__m128i _mm_cvtepi16_epi32(__m128i);
__m128i _mm_cvtepi16_epi64(__m128i);
__m128i _mm_cvtepi32_epi64(__m128i);
__m128i _mm_cvtepi8_epi16(__m128i);
__m128i _mm_cvtepi8_epi32(__m128i);
__m128i _mm_cvtepi8_epi64(__m128i);
__m128i _mm_cvtepu16_epi32(__m128i);
__m128i _mm_cvtepu16_epi64(__m128i);
__m128i _mm_cvtepu32_epi64(__m128i);
__m128i _mm_cvtepu8_epi16(__m128i);
__m128i _mm_cvtepu8_epi32(__m128i);
__m128i _mm_cvtepu8_epi64(__m128i);
__m128d _mm_dp_pd(__m128d, __m128d, const int);
__m128 _mm_dp_ps(__m128, __m128, const int);
int _mm_extract_epi32(__m128i, const int);
int _mm_extract_epi8(__m128i, const int);
int _mm_extract_ps(__m128, const int);
__m128d _mm_floor_pd(__m128d);
__m128 _mm_floor_ps(__m128);
__m128d _mm_floor_sd(__m128d, __m128d);
__m128 _mm_floor_ss(__m128, __m128);
__m128i _mm_insert_epi32(__m128i, int, const int);
__m128i _mm_insert_epi8(__m128i, int, const int);
__m128 _mm_insert_ps(__m128, __m128, const int);
__m128i _mm_max_epi32(__m128i, __m128i);
__m128i _mm_max_epi8(__m128i, __m128i);
__m128i _mm_max_epu16(__m128i, __m128i);
__m128i _mm_max_epu32(__m128i, __m128i);
__m128i _mm_min_epi32(__m128i, __m128i);
__m128i _mm_min_epi8(__m128i, __m128i);
__m128i _mm_min_epu16(__m128i, __m128i);
__m128i _mm_min_epu32(__m128i, __m128i);
__m128i _mm_minpos_epu16(__m128i);
__m128i _mm_mpsadbw_epu8(__m128i, __m128i, const int);
__m128i _mm_mul_epi32(__m128i, __m128i);
__m128i _mm_mullo_epi32(__m128i, __m128i);
__m128i _mm_packus_epi32(__m128i, __m128i);
__m128d _mm_round_pd(__m128d, const int);
__m128 _mm_round_ps(__m128, const int);
__m128d _mm_round_sd(__m128d, __m128d, const int);
__m128 _mm_round_ss(__m128, __m128, const int);
__m128i _mm_stream_load_si128(const __m128i *);
int _mm_test_all_ones(__m128i);
int _mm_test_all_zeros(__m128i, __m128i);
int _mm_test_mix_ones_zeros(__m128i, __m128i);
int _mm_testc_si128(__m128i, __m128i);
int _mm_testnzc_si128(__m128i, __m128i);
int _mm_testz_si128(__m128i, __m128i);
#ifdef __x86_64__
long long _mm_extract_epi64(__m128i, const int);
__m128i _mm_insert_epi64(__m128i, long long, const int);
#endif
/* @generated end */

#endif /* x86 */
#endif /* _CINRS_SMMINTRIN_H */
