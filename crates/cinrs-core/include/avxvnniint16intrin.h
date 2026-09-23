/* <avxvnniint16intrin.h> — the avxvnniint16 intrinsics, as GCC arranges them.
 *
 * Written by crates/cinrs-core/tests/x86_intrinsics.rs; the regions below
 * are regenerated from `core::arch`'s source, and an empty one is an
 * instruction set whose intrinsics are all still unstable there. Needs the
 * instruction set in `__attribute__((target("…")))` on the function that
 * calls them, and a function that passes or returns a 512-bit vector by
 * value needs `target("avx512f")`; see <immintrin.h>.
 */
#ifndef _CINRS_AVXVNNIINT16INTRIN_H
#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#elif !defined(_CINRS_IMMINTRIN_H)
/* On its own: <immintrin.h> includes this file back, in its place. */
#include <immintrin.h>
#else
#define _CINRS_AVXVNNIINT16INTRIN_H

/* @generated avxvnniint16 — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m256i _mm256_dpwsud_epi32(__m256i, __m256i, __m256i);
__m256i _mm256_dpwsuds_epi32(__m256i, __m256i, __m256i);
__m256i _mm256_dpwusd_epi32(__m256i, __m256i, __m256i);
__m256i _mm256_dpwusds_epi32(__m256i, __m256i, __m256i);
__m256i _mm256_dpwuud_epi32(__m256i, __m256i, __m256i);
__m256i _mm256_dpwuuds_epi32(__m256i, __m256i, __m256i);
__m128i _mm_dpwsud_epi32(__m128i, __m128i, __m128i);
__m128i _mm_dpwsuds_epi32(__m128i, __m128i, __m128i);
__m128i _mm_dpwusd_epi32(__m128i, __m128i, __m128i);
__m128i _mm_dpwusds_epi32(__m128i, __m128i, __m128i);
__m128i _mm_dpwuud_epi32(__m128i, __m128i, __m128i);
__m128i _mm_dpwuuds_epi32(__m128i, __m128i, __m128i);
/* @generated end */

#endif
#endif /* _CINRS_AVXVNNIINT16INTRIN_H */
