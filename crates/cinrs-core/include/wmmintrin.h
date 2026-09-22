/* <wmmintrin.h> — the AES and carry-less multiply intrinsics.
 *
 * Needs `__attribute__((target("aes")))` or `__attribute__((target("pclmul")))`
 * on the function that calls them; see <xmmintrin.h>.
 */
#ifndef _CINRS_WMMINTRIN_H
#define _CINRS_WMMINTRIN_H

#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#else

#include <emmintrin.h>

/* @generated aes — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m128i _mm_aesdec_si128(__m128i, __m128i);
__m128i _mm_aesdeclast_si128(__m128i, __m128i);
__m128i _mm_aesenc_si128(__m128i, __m128i);
__m128i _mm_aesenclast_si128(__m128i, __m128i);
__m128i _mm_aesimc_si128(__m128i);
__m128i _mm_aeskeygenassist_si128(__m128i, const int);
/* @generated end */

/* @generated pclmulqdq — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m128i _mm_clmulepi64_si128(__m128i, __m128i, const int);
/* @generated end */

#endif /* x86 */
#endif /* _CINRS_WMMINTRIN_H */
