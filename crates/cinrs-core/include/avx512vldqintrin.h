/* <avx512vldqintrin.h> — the avx512dq+VL intrinsics, as GCC arranges them.
 *
 * Written by crates/cinrs-core/tests/x86_intrinsics.rs; the regions below
 * are regenerated from `core::arch`'s source, and an empty one is an
 * instruction set whose intrinsics are all still unstable there. Needs the
 * instruction set in `__attribute__((target("…")))` on the function that
 * calls them, and a function that passes or returns a 512-bit vector by
 * value needs `target("avx512f")`; see <immintrin.h>.
 */
#ifndef _CINRS_AVX512VLDQINTRIN_H
#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#elif !defined(_CINRS_IMMINTRIN_H)
/* On its own: <immintrin.h> includes this file back, in its place. */
#include <immintrin.h>
#else
#define _CINRS_AVX512VLDQINTRIN_H

/* @generated avx512dq-vl — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m256 _mm256_broadcast_f32x2(__m128);
__m256d _mm256_broadcast_f64x2(__m128d);
__m256i _mm256_broadcast_i32x2(__m128i);
__m256i _mm256_broadcast_i64x2(__m128i);
__m256d _mm256_cvtepi64_pd(__m256i);
__m128 _mm256_cvtepi64_ps(__m256i);
__m256d _mm256_cvtepu64_pd(__m256i);
__m128 _mm256_cvtepu64_ps(__m256i);
__m256i _mm256_cvtpd_epi64(__m256d);
__m256i _mm256_cvtpd_epu64(__m256d);
__m256i _mm256_cvtps_epi64(__m128);
__m256i _mm256_cvtps_epu64(__m128);
__m256i _mm256_cvttpd_epi64(__m256d);
__m256i _mm256_cvttpd_epu64(__m256d);
__m256i _mm256_cvttps_epi64(__m128);
__m256i _mm256_cvttps_epu64(__m128);
__m128d _mm256_extractf64x2_pd(__m256d, const int);
__m128i _mm256_extracti64x2_epi64(__m256i, const int);
__mmask8 _mm256_fpclass_pd_mask(__m256d, const int);
__mmask8 _mm256_fpclass_ps_mask(__m256, const int);
__m256d _mm256_insertf64x2(__m256d, __m128d, const int);
__m256i _mm256_inserti64x2(__m256i, __m128i, const int);
__m256d _mm256_mask_and_pd(__m256d, __mmask8, __m256d, __m256d);
__m256 _mm256_mask_and_ps(__m256, __mmask8, __m256, __m256);
__m256d _mm256_mask_andnot_pd(__m256d, __mmask8, __m256d, __m256d);
__m256 _mm256_mask_andnot_ps(__m256, __mmask8, __m256, __m256);
__m256 _mm256_mask_broadcast_f32x2(__m256, __mmask8, __m128);
__m256d _mm256_mask_broadcast_f64x2(__m256d, __mmask8, __m128d);
__m256i _mm256_mask_broadcast_i32x2(__m256i, __mmask8, __m128i);
__m256i _mm256_mask_broadcast_i64x2(__m256i, __mmask8, __m128i);
__m256d _mm256_mask_cvtepi64_pd(__m256d, __mmask8, __m256i);
__m128 _mm256_mask_cvtepi64_ps(__m128, __mmask8, __m256i);
__m256d _mm256_mask_cvtepu64_pd(__m256d, __mmask8, __m256i);
__m128 _mm256_mask_cvtepu64_ps(__m128, __mmask8, __m256i);
__m256i _mm256_mask_cvtpd_epi64(__m256i, __mmask8, __m256d);
__m256i _mm256_mask_cvtpd_epu64(__m256i, __mmask8, __m256d);
__m256i _mm256_mask_cvtps_epi64(__m256i, __mmask8, __m128);
__m256i _mm256_mask_cvtps_epu64(__m256i, __mmask8, __m128);
__m256i _mm256_mask_cvttpd_epi64(__m256i, __mmask8, __m256d);
__m256i _mm256_mask_cvttpd_epu64(__m256i, __mmask8, __m256d);
__m256i _mm256_mask_cvttps_epi64(__m256i, __mmask8, __m128);
__m256i _mm256_mask_cvttps_epu64(__m256i, __mmask8, __m128);
__m128d _mm256_mask_extractf64x2_pd(__m128d, __mmask8, __m256d, const int);
__m128i _mm256_mask_extracti64x2_epi64(__m128i, __mmask8, __m256i, const int);
__mmask8 _mm256_mask_fpclass_pd_mask(__mmask8, __m256d, const int);
__mmask8 _mm256_mask_fpclass_ps_mask(__mmask8, __m256, const int);
__m256d _mm256_mask_insertf64x2(__m256d, __mmask8, __m256d, __m128d, const int);
__m256i _mm256_mask_inserti64x2(__m256i, __mmask8, __m256i, __m128i, const int);
__m256i _mm256_mask_mullo_epi64(__m256i, __mmask8, __m256i, __m256i);
__m256d _mm256_mask_or_pd(__m256d, __mmask8, __m256d, __m256d);
__m256 _mm256_mask_or_ps(__m256, __mmask8, __m256, __m256);
__m256d _mm256_mask_range_pd(__m256d, __mmask8, __m256d, __m256d, const int);
__m256 _mm256_mask_range_ps(__m256, __mmask8, __m256, __m256, const int);
__m256d _mm256_mask_reduce_pd(__m256d, __mmask8, __m256d, const int);
__m256 _mm256_mask_reduce_ps(__m256, __mmask8, __m256, const int);
__m256d _mm256_mask_xor_pd(__m256d, __mmask8, __m256d, __m256d);
__m256 _mm256_mask_xor_ps(__m256, __mmask8, __m256, __m256);
__m256d _mm256_maskz_and_pd(__mmask8, __m256d, __m256d);
__m256 _mm256_maskz_and_ps(__mmask8, __m256, __m256);
__m256d _mm256_maskz_andnot_pd(__mmask8, __m256d, __m256d);
__m256 _mm256_maskz_andnot_ps(__mmask8, __m256, __m256);
__m256 _mm256_maskz_broadcast_f32x2(__mmask8, __m128);
__m256d _mm256_maskz_broadcast_f64x2(__mmask8, __m128d);
__m256i _mm256_maskz_broadcast_i32x2(__mmask8, __m128i);
__m256i _mm256_maskz_broadcast_i64x2(__mmask8, __m128i);
__m256d _mm256_maskz_cvtepi64_pd(__mmask8, __m256i);
__m128 _mm256_maskz_cvtepi64_ps(__mmask8, __m256i);
__m256d _mm256_maskz_cvtepu64_pd(__mmask8, __m256i);
__m128 _mm256_maskz_cvtepu64_ps(__mmask8, __m256i);
__m256i _mm256_maskz_cvtpd_epi64(__mmask8, __m256d);
__m256i _mm256_maskz_cvtpd_epu64(__mmask8, __m256d);
__m256i _mm256_maskz_cvtps_epi64(__mmask8, __m128);
__m256i _mm256_maskz_cvtps_epu64(__mmask8, __m128);
__m256i _mm256_maskz_cvttpd_epi64(__mmask8, __m256d);
__m256i _mm256_maskz_cvttpd_epu64(__mmask8, __m256d);
__m256i _mm256_maskz_cvttps_epi64(__mmask8, __m128);
__m256i _mm256_maskz_cvttps_epu64(__mmask8, __m128);
__m128d _mm256_maskz_extractf64x2_pd(__mmask8, __m256d, const int);
__m128i _mm256_maskz_extracti64x2_epi64(__mmask8, __m256i, const int);
__m256d _mm256_maskz_insertf64x2(__mmask8, __m256d, __m128d, const int);
__m256i _mm256_maskz_inserti64x2(__mmask8, __m256i, __m128i, const int);
__m256i _mm256_maskz_mullo_epi64(__mmask8, __m256i, __m256i);
__m256d _mm256_maskz_or_pd(__mmask8, __m256d, __m256d);
__m256 _mm256_maskz_or_ps(__mmask8, __m256, __m256);
__m256d _mm256_maskz_range_pd(__mmask8, __m256d, __m256d, const int);
__m256 _mm256_maskz_range_ps(__mmask8, __m256, __m256, const int);
__m256d _mm256_maskz_reduce_pd(__mmask8, __m256d, const int);
__m256 _mm256_maskz_reduce_ps(__mmask8, __m256, const int);
__m256d _mm256_maskz_xor_pd(__mmask8, __m256d, __m256d);
__m256 _mm256_maskz_xor_ps(__mmask8, __m256, __m256);
__mmask8 _mm256_movepi32_mask(__m256i);
__mmask8 _mm256_movepi64_mask(__m256i);
__m256i _mm256_movm_epi32(__mmask8);
__m256i _mm256_movm_epi64(__mmask8);
__m256i _mm256_mullo_epi64(__m256i, __m256i);
__m256d _mm256_range_pd(__m256d, __m256d, const int);
__m256 _mm256_range_ps(__m256, __m256, const int);
__m256d _mm256_reduce_pd(__m256d, const int);
__m256 _mm256_reduce_ps(__m256, const int);
__m128i _mm_broadcast_i32x2(__m128i);
__m128d _mm_cvtepi64_pd(__m128i);
__m128 _mm_cvtepi64_ps(__m128i);
__m128d _mm_cvtepu64_pd(__m128i);
__m128 _mm_cvtepu64_ps(__m128i);
__m128i _mm_cvtpd_epi64(__m128d);
__m128i _mm_cvtpd_epu64(__m128d);
__m128i _mm_cvtps_epi64(__m128);
__m128i _mm_cvtps_epu64(__m128);
__m128i _mm_cvttpd_epi64(__m128d);
__m128i _mm_cvttpd_epu64(__m128d);
__m128i _mm_cvttps_epi64(__m128);
__m128i _mm_cvttps_epu64(__m128);
__mmask8 _mm_fpclass_pd_mask(__m128d, const int);
__mmask8 _mm_fpclass_ps_mask(__m128, const int);
__m128d _mm_mask_and_pd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_and_ps(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_andnot_pd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_andnot_ps(__m128, __mmask8, __m128, __m128);
__m128i _mm_mask_broadcast_i32x2(__m128i, __mmask8, __m128i);
__m128d _mm_mask_cvtepi64_pd(__m128d, __mmask8, __m128i);
__m128 _mm_mask_cvtepi64_ps(__m128, __mmask8, __m128i);
__m128d _mm_mask_cvtepu64_pd(__m128d, __mmask8, __m128i);
__m128 _mm_mask_cvtepu64_ps(__m128, __mmask8, __m128i);
__m128i _mm_mask_cvtpd_epi64(__m128i, __mmask8, __m128d);
__m128i _mm_mask_cvtpd_epu64(__m128i, __mmask8, __m128d);
__m128i _mm_mask_cvtps_epi64(__m128i, __mmask8, __m128);
__m128i _mm_mask_cvtps_epu64(__m128i, __mmask8, __m128);
__m128i _mm_mask_cvttpd_epi64(__m128i, __mmask8, __m128d);
__m128i _mm_mask_cvttpd_epu64(__m128i, __mmask8, __m128d);
__m128i _mm_mask_cvttps_epi64(__m128i, __mmask8, __m128);
__m128i _mm_mask_cvttps_epu64(__m128i, __mmask8, __m128);
__mmask8 _mm_mask_fpclass_pd_mask(__mmask8, __m128d, const int);
__mmask8 _mm_mask_fpclass_ps_mask(__mmask8, __m128, const int);
__m128i _mm_mask_mullo_epi64(__m128i, __mmask8, __m128i, __m128i);
__m128d _mm_mask_or_pd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_or_ps(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_range_pd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_range_ps(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_reduce_pd(__m128d, __mmask8, __m128d, const int);
__m128 _mm_mask_reduce_ps(__m128, __mmask8, __m128, const int);
__m128d _mm_mask_xor_pd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_xor_ps(__m128, __mmask8, __m128, __m128);
__m128d _mm_maskz_and_pd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_and_ps(__mmask8, __m128, __m128);
__m128d _mm_maskz_andnot_pd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_andnot_ps(__mmask8, __m128, __m128);
__m128i _mm_maskz_broadcast_i32x2(__mmask8, __m128i);
__m128d _mm_maskz_cvtepi64_pd(__mmask8, __m128i);
__m128 _mm_maskz_cvtepi64_ps(__mmask8, __m128i);
__m128d _mm_maskz_cvtepu64_pd(__mmask8, __m128i);
__m128 _mm_maskz_cvtepu64_ps(__mmask8, __m128i);
__m128i _mm_maskz_cvtpd_epi64(__mmask8, __m128d);
__m128i _mm_maskz_cvtpd_epu64(__mmask8, __m128d);
__m128i _mm_maskz_cvtps_epi64(__mmask8, __m128);
__m128i _mm_maskz_cvtps_epu64(__mmask8, __m128);
__m128i _mm_maskz_cvttpd_epi64(__mmask8, __m128d);
__m128i _mm_maskz_cvttpd_epu64(__mmask8, __m128d);
__m128i _mm_maskz_cvttps_epi64(__mmask8, __m128);
__m128i _mm_maskz_cvttps_epu64(__mmask8, __m128);
__m128i _mm_maskz_mullo_epi64(__mmask8, __m128i, __m128i);
__m128d _mm_maskz_or_pd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_or_ps(__mmask8, __m128, __m128);
__m128d _mm_maskz_range_pd(__mmask8, __m128d, __m128d, const int);
__m128 _mm_maskz_range_ps(__mmask8, __m128, __m128, const int);
__m128d _mm_maskz_reduce_pd(__mmask8, __m128d, const int);
__m128 _mm_maskz_reduce_ps(__mmask8, __m128, const int);
__m128d _mm_maskz_xor_pd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_xor_ps(__mmask8, __m128, __m128);
__mmask8 _mm_movepi32_mask(__m128i);
__mmask8 _mm_movepi64_mask(__m128i);
__m128i _mm_movm_epi32(__mmask8);
__m128i _mm_movm_epi64(__mmask8);
__m128i _mm_mullo_epi64(__m128i, __m128i);
__m128d _mm_range_pd(__m128d, __m128d, const int);
__m128 _mm_range_ps(__m128, __m128, const int);
__m128d _mm_reduce_pd(__m128d, const int);
__m128 _mm_reduce_ps(__m128, const int);
/* @generated end */

#endif
#endif /* _CINRS_AVX512VLDQINTRIN_H */
