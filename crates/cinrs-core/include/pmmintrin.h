/* <pmmintrin.h> — the SSE3 intrinsics.
 *
 * Needs `__attribute__((target("sse3")))` on the function that calls them, or
 * a build whose `-C target-feature` already has it; see <xmmintrin.h>.
 */
#ifndef _CINRS_PMMINTRIN_H
#define _CINRS_PMMINTRIN_H

#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#else

#include <emmintrin.h>

/* What `_mm_monitor`'s and `_mm_mwait`'s hints are; kept for source
 * compatibility with GCC's header. */
#define _MM_DENORMALS_ZERO_MASK 0x0040u
#define _MM_DENORMALS_ZERO_ON 0x0040u
#define _MM_DENORMALS_ZERO_OFF 0x0000u

/* @generated sse3 — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m128d _mm_addsub_pd(__m128d, __m128d);
__m128 _mm_addsub_ps(__m128, __m128);
__m128d _mm_hadd_pd(__m128d, __m128d);
__m128 _mm_hadd_ps(__m128, __m128);
__m128d _mm_hsub_pd(__m128d, __m128d);
__m128 _mm_hsub_ps(__m128, __m128);
__m128i _mm_lddqu_si128(const __m128i *);
__m128d _mm_loaddup_pd(const double *);
__m128d _mm_movedup_pd(__m128d);
__m128 _mm_movehdup_ps(__m128);
__m128 _mm_moveldup_ps(__m128);
/* @generated end */

#endif /* x86 */
#endif /* _CINRS_PMMINTRIN_H */
