/* <avxvnniint8intrin.h> — the avxvnniint8 intrinsics, as GCC arranges them.
 *
 * Written by crates/cinrs-core/tests/x86_intrinsics.rs; the regions below
 * are regenerated from `core::arch`'s source, and an empty one is an
 * instruction set whose intrinsics are all still unstable there. Needs the
 * instruction set in `__attribute__((target("…")))` on the function that
 * calls them, and a function that passes or returns a 512-bit vector by
 * value needs `target("avx512f")`; see <immintrin.h>.
 */
#ifndef _CINRS_AVXVNNIINT8INTRIN_H
#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#elif !defined(_CINRS_IMMINTRIN_H)
/* On its own: <immintrin.h> includes this file back, in its place. */
#include <immintrin.h>
#else
#define _CINRS_AVXVNNIINT8INTRIN_H

/* @generated avxvnniint8 — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m256i _mm256_dpbssd_epi32(__m256i, __m256i, __m256i);
__m256i _mm256_dpbssds_epi32(__m256i, __m256i, __m256i);
__m256i _mm256_dpbsud_epi32(__m256i, __m256i, __m256i);
__m256i _mm256_dpbsuds_epi32(__m256i, __m256i, __m256i);
__m256i _mm256_dpbuud_epi32(__m256i, __m256i, __m256i);
__m256i _mm256_dpbuuds_epi32(__m256i, __m256i, __m256i);
__m128i _mm_dpbssd_epi32(__m128i, __m128i, __m128i);
__m128i _mm_dpbssds_epi32(__m128i, __m128i, __m128i);
__m128i _mm_dpbsud_epi32(__m128i, __m128i, __m128i);
__m128i _mm_dpbsuds_epi32(__m128i, __m128i, __m128i);
__m128i _mm_dpbuud_epi32(__m128i, __m128i, __m128i);
__m128i _mm_dpbuuds_epi32(__m128i, __m128i, __m128i);
/* @generated end */

#endif
#endif /* _CINRS_AVXVNNIINT8INTRIN_H */
