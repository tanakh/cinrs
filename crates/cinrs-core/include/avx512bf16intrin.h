/* <avx512bf16intrin.h> — the avx512bf16 intrinsics, as GCC arranges them.
 *
 * Written by crates/cinrs-core/tests/x86_intrinsics.rs; the regions below
 * are regenerated from `core::arch`'s source, and an empty one is an
 * instruction set whose intrinsics are all still unstable there. Needs the
 * instruction set in `__attribute__((target("…")))` on the function that
 * calls them, and a function that passes or returns a 512-bit vector by
 * value needs `target("avx512f")`; see <immintrin.h>.
 */
#ifndef _CINRS_AVX512BF16INTRIN_H
#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#elif !defined(_CINRS_IMMINTRIN_H)
/* On its own: <immintrin.h> includes this file back, in its place. */
#include <immintrin.h>
#else
#define _CINRS_AVX512BF16INTRIN_H

typedef __cinrs_m128bh __m128bh;
typedef __cinrs_m256bh __m256bh;
typedef __cinrs_m512bh __m512bh;

/* @generated avx512bf16 — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m512bh _mm512_cvtne2ps_pbh(__m512, __m512);
__m256bh _mm512_cvtneps_pbh(__m512);
__m512 _mm512_cvtpbh_ps(__m256bh);
__m512 _mm512_dpbf16_ps(__m512, __m512bh, __m512bh);
__m512bh _mm512_mask_cvtne2ps_pbh(__m512bh, __mmask32, __m512, __m512);
__m256bh _mm512_mask_cvtneps_pbh(__m256bh, __mmask16, __m512);
__m512 _mm512_mask_cvtpbh_ps(__m512, __mmask16, __m256bh);
__m512 _mm512_mask_dpbf16_ps(__m512, __mmask16, __m512bh, __m512bh);
__m512bh _mm512_maskz_cvtne2ps_pbh(__mmask32, __m512, __m512);
__m256bh _mm512_maskz_cvtneps_pbh(__mmask16, __m512);
__m512 _mm512_maskz_cvtpbh_ps(__mmask16, __m256bh);
__m512 _mm512_maskz_dpbf16_ps(__mmask16, __m512, __m512bh, __m512bh);
/* @generated end */

#endif
#endif /* _CINRS_AVX512BF16INTRIN_H */
