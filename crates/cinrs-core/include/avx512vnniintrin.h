/* <avx512vnniintrin.h> — the avx512vnni intrinsics, as GCC arranges them.
 *
 * Written by crates/cinrs-core/tests/x86_intrinsics.rs; the regions below
 * are regenerated from `core::arch`'s source, and an empty one is an
 * instruction set whose intrinsics are all still unstable there. Needs the
 * instruction set in `__attribute__((target("…")))` on the function that
 * calls them, and a function that passes or returns a 512-bit vector by
 * value needs `target("avx512f")`; see <immintrin.h>.
 */
#ifndef _CINRS_AVX512VNNIINTRIN_H
#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#elif !defined(_CINRS_IMMINTRIN_H)
/* On its own: <immintrin.h> includes this file back, in its place. */
#include <immintrin.h>
#else
#define _CINRS_AVX512VNNIINTRIN_H

/* @generated avx512vnni — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m512i _mm512_dpbusd_epi32(__m512i, __m512i, __m512i);
__m512i _mm512_dpbusds_epi32(__m512i, __m512i, __m512i);
__m512i _mm512_dpwssd_epi32(__m512i, __m512i, __m512i);
__m512i _mm512_dpwssds_epi32(__m512i, __m512i, __m512i);
__m512i _mm512_mask_dpbusd_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_dpbusds_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_dpwssd_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_dpwssds_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_maskz_dpbusd_epi32(__mmask16, __m512i, __m512i, __m512i);
__m512i _mm512_maskz_dpbusds_epi32(__mmask16, __m512i, __m512i, __m512i);
__m512i _mm512_maskz_dpwssd_epi32(__mmask16, __m512i, __m512i, __m512i);
__m512i _mm512_maskz_dpwssds_epi32(__mmask16, __m512i, __m512i, __m512i);
/* @generated end */

#endif
#endif /* _CINRS_AVX512VNNIINTRIN_H */
