/* <immintrin.h> — every Intel intrinsic cinrs knows.
 *
 * This is the header everyone includes. It pulls in the whole SSE chain —
 * <xmmintrin.h> through <nmmintrin.h>, and <wmmintrin.h> — and adds the
 * 256-bit types `__m256`, `__m256i` and `__m256d`, the AVX and AVX2
 * intrinsics, FMA, SHA, and the scalar bit-manipulation ones: POPCNT, LZCNT,
 * BMI1 and BMI2.
 *
 * A call is generated as `::core::arch::x86_64::_mm256_add_ps(a, b)` — the
 * function of the same name and signature in Rust's own `core::arch` — rather
 * than as a call to a library, because there is no symbol to link. See
 * <xmmintrin.h> for the rest of the rules and doc/features.md for the prose.
 *
 * **A function that passes or returns a 256-bit vector by value needs
 * `__attribute__((target("avx")))`** (or `avx2`, which implies it): that is
 * the ABI Rust and C both use for a value that wide, and rustc refuses the
 * definition otherwise. A 128-bit vector needs nothing, because SSE2 is the
 * x86-64 baseline.
 *
 * What is **not** here: AVX-512 and the mask types, MMX and `__m64` (see
 * <mmintrin.h>), and the GNU vector extensions — `__attribute__((vector_size))`
 * and arithmetic on vectors — which cinrs refuses with a diagnostic. The
 * intrinsics are the API.
 */
#ifndef _CINRS_IMMINTRIN_H
#define _CINRS_IMMINTRIN_H

#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#else

#include <nmmintrin.h>
#include <wmmintrin.h>

typedef __cinrs_m256 __m256;
typedef __cinrs_m256i __m256i;
typedef __cinrs_m256d __m256d;

/* @generated constants — see crates/cinrs-core/tests/x86_intrinsics.rs */
#define _CMP_EQ_OQ 0x0000
#define _CMP_LT_OS 0x0001
#define _CMP_LE_OS 0x0002
#define _CMP_UNORD_Q 0x0003
#define _CMP_NEQ_UQ 0x0004
#define _CMP_NLT_US 0x0005
#define _CMP_NLE_US 0x0006
#define _CMP_ORD_Q 0x0007
#define _CMP_EQ_UQ 0x0008
#define _CMP_NGE_US 0x0009
#define _CMP_NGT_US 0x000a
#define _CMP_FALSE_OQ 0x000b
#define _CMP_NEQ_OQ 0x000c
#define _CMP_GE_OS 0x000d
#define _CMP_GT_OS 0x000e
#define _CMP_TRUE_UQ 0x000f
#define _CMP_EQ_OS 0x0010
#define _CMP_LT_OQ 0x0011
#define _CMP_LE_OQ 0x0012
#define _CMP_UNORD_S 0x0013
#define _CMP_NEQ_US 0x0014
#define _CMP_NLT_UQ 0x0015
#define _CMP_NLE_UQ 0x0016
#define _CMP_ORD_S 0x0017
#define _CMP_EQ_US 0x0018
#define _CMP_NGE_UQ 0x0019
#define _CMP_NGT_UQ 0x001a
#define _CMP_FALSE_OS 0x001b
#define _CMP_NEQ_OS 0x001c
#define _CMP_GE_OQ 0x001d
#define _CMP_GT_OQ 0x001e
#define _CMP_TRUE_US 0x001f
/* @generated end */

/* @generated avx — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m256d _mm256_add_pd(__m256d, __m256d);
__m256 _mm256_add_ps(__m256, __m256);
__m256d _mm256_addsub_pd(__m256d, __m256d);
__m256 _mm256_addsub_ps(__m256, __m256);
__m256d _mm256_and_pd(__m256d, __m256d);
__m256 _mm256_and_ps(__m256, __m256);
__m256d _mm256_andnot_pd(__m256d, __m256d);
__m256 _mm256_andnot_ps(__m256, __m256);
__m256d _mm256_blend_pd(__m256d, __m256d, const int);
__m256 _mm256_blend_ps(__m256, __m256, const int);
__m256d _mm256_blendv_pd(__m256d, __m256d, __m256d);
__m256 _mm256_blendv_ps(__m256, __m256, __m256);
__m256d _mm256_castpd128_pd256(__m128d);
__m128d _mm256_castpd256_pd128(__m256d);
__m256 _mm256_castpd_ps(__m256d);
__m256i _mm256_castpd_si256(__m256d);
__m256 _mm256_castps128_ps256(__m128);
__m128 _mm256_castps256_ps128(__m256);
__m256d _mm256_castps_pd(__m256);
__m256i _mm256_castps_si256(__m256);
__m256i _mm256_castsi128_si256(__m128i);
__m256d _mm256_castsi256_pd(__m256i);
__m256 _mm256_castsi256_ps(__m256i);
__m128i _mm256_castsi256_si128(__m256i);
__m256d _mm256_ceil_pd(__m256d);
__m256 _mm256_ceil_ps(__m256);
__m256d _mm256_cmp_pd(__m256d, __m256d, const int);
__m256 _mm256_cmp_ps(__m256, __m256, const int);
__m256d _mm256_cvtepi32_pd(__m128i);
__m256 _mm256_cvtepi32_ps(__m256i);
__m128i _mm256_cvtpd_epi32(__m256d);
__m128 _mm256_cvtpd_ps(__m256d);
__m256i _mm256_cvtps_epi32(__m256);
__m256d _mm256_cvtps_pd(__m128);
double _mm256_cvtsd_f64(__m256d);
int _mm256_cvtsi256_si32(__m256i);
float _mm256_cvtss_f32(__m256);
__m128i _mm256_cvttpd_epi32(__m256d);
__m256i _mm256_cvttps_epi32(__m256);
__m256d _mm256_div_pd(__m256d, __m256d);
__m256 _mm256_div_ps(__m256, __m256);
__m256 _mm256_dp_ps(__m256, __m256, const int);
int _mm256_extract_epi32(__m256i, const int);
__m128d _mm256_extractf128_pd(__m256d, const int);
__m128 _mm256_extractf128_ps(__m256, const int);
__m128i _mm256_extractf128_si256(__m256i, const int);
__m256d _mm256_floor_pd(__m256d);
__m256 _mm256_floor_ps(__m256);
__m256d _mm256_hadd_pd(__m256d, __m256d);
__m256 _mm256_hadd_ps(__m256, __m256);
__m256d _mm256_hsub_pd(__m256d, __m256d);
__m256 _mm256_hsub_ps(__m256, __m256);
__m256i _mm256_insert_epi16(__m256i, short, const int);
__m256i _mm256_insert_epi32(__m256i, int, const int);
__m256i _mm256_insert_epi8(__m256i, char, const int);
__m256d _mm256_insertf128_pd(__m256d, __m128d, const int);
__m256 _mm256_insertf128_ps(__m256, __m128, const int);
__m256i _mm256_insertf128_si256(__m256i, __m128i, const int);
__m256i _mm256_lddqu_si256(const __m256i *);
__m256d _mm256_load_pd(const double *);
__m256 _mm256_load_ps(const float *);
__m256i _mm256_load_si256(const __m256i *);
__m256 _mm256_loadu2_m128(const float *, const float *);
__m256d _mm256_loadu2_m128d(const double *, const double *);
__m256i _mm256_loadu2_m128i(const __m128i *, const __m128i *);
__m256d _mm256_loadu_pd(const double *);
__m256 _mm256_loadu_ps(const float *);
__m256i _mm256_loadu_si256(const __m256i *);
__m256d _mm256_maskload_pd(const double *, __m256i);
__m256 _mm256_maskload_ps(const float *, __m256i);
void _mm256_maskstore_pd(double *, __m256i, __m256d);
void _mm256_maskstore_ps(float *, __m256i, __m256);
__m256d _mm256_max_pd(__m256d, __m256d);
__m256 _mm256_max_ps(__m256, __m256);
__m256d _mm256_min_pd(__m256d, __m256d);
__m256 _mm256_min_ps(__m256, __m256);
__m256d _mm256_movedup_pd(__m256d);
__m256 _mm256_movehdup_ps(__m256);
__m256 _mm256_moveldup_ps(__m256);
int _mm256_movemask_pd(__m256d);
int _mm256_movemask_ps(__m256);
__m256d _mm256_mul_pd(__m256d, __m256d);
__m256 _mm256_mul_ps(__m256, __m256);
__m256d _mm256_or_pd(__m256d, __m256d);
__m256 _mm256_or_ps(__m256, __m256);
__m256d _mm256_permute2f128_pd(__m256d, __m256d, const int);
__m256 _mm256_permute2f128_ps(__m256, __m256, const int);
__m256i _mm256_permute2f128_si256(__m256i, __m256i, const int);
__m256d _mm256_permute_pd(__m256d, const int);
__m256 _mm256_permute_ps(__m256, const int);
__m256d _mm256_permutevar_pd(__m256d, __m256i);
__m256 _mm256_permutevar_ps(__m256, __m256i);
__m256 _mm256_rcp_ps(__m256);
__m256d _mm256_round_pd(__m256d, const int);
__m256 _mm256_round_ps(__m256, const int);
__m256 _mm256_rsqrt_ps(__m256);
__m256i _mm256_set1_epi16(short);
__m256i _mm256_set1_epi32(int);
__m256i _mm256_set1_epi64x(long long);
__m256i _mm256_set1_epi8(char);
__m256d _mm256_set1_pd(double);
__m256 _mm256_set1_ps(float);
__m256i _mm256_set_epi16(short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short);
__m256i _mm256_set_epi32(int, int, int, int, int, int, int, int);
__m256i _mm256_set_epi64x(long long, long long, long long, long long);
__m256i _mm256_set_epi8(char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char);
__m256 _mm256_set_m128(__m128, __m128);
__m256d _mm256_set_m128d(__m128d, __m128d);
__m256i _mm256_set_m128i(__m128i, __m128i);
__m256d _mm256_set_pd(double, double, double, double);
__m256 _mm256_set_ps(float, float, float, float, float, float, float, float);
__m256i _mm256_setr_epi16(short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short);
__m256i _mm256_setr_epi32(int, int, int, int, int, int, int, int);
__m256i _mm256_setr_epi64x(long long, long long, long long, long long);
__m256i _mm256_setr_epi8(char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char);
__m256 _mm256_setr_m128(__m128, __m128);
__m256d _mm256_setr_m128d(__m128d, __m128d);
__m256i _mm256_setr_m128i(__m128i, __m128i);
__m256d _mm256_setr_pd(double, double, double, double);
__m256 _mm256_setr_ps(float, float, float, float, float, float, float, float);
__m256d _mm256_setzero_pd(void);
__m256 _mm256_setzero_ps(void);
__m256i _mm256_setzero_si256(void);
__m256d _mm256_shuffle_pd(__m256d, __m256d, const int);
__m256 _mm256_shuffle_ps(__m256, __m256, const int);
__m256d _mm256_sqrt_pd(__m256d);
__m256 _mm256_sqrt_ps(__m256);
void _mm256_store_pd(double *, __m256d);
void _mm256_store_ps(float *, __m256);
void _mm256_store_si256(__m256i *, __m256i);
void _mm256_storeu2_m128(float *, float *, __m256);
void _mm256_storeu2_m128d(double *, double *, __m256d);
void _mm256_storeu2_m128i(__m128i *, __m128i *, __m256i);
void _mm256_storeu_pd(double *, __m256d);
void _mm256_storeu_ps(float *, __m256);
void _mm256_storeu_si256(__m256i *, __m256i);
void _mm256_stream_pd(double *, __m256d);
void _mm256_stream_ps(float *, __m256);
void _mm256_stream_si256(__m256i *, __m256i);
__m256d _mm256_sub_pd(__m256d, __m256d);
__m256 _mm256_sub_ps(__m256, __m256);
int _mm256_testc_pd(__m256d, __m256d);
int _mm256_testc_ps(__m256, __m256);
int _mm256_testc_si256(__m256i, __m256i);
int _mm256_testnzc_pd(__m256d, __m256d);
int _mm256_testnzc_ps(__m256, __m256);
int _mm256_testnzc_si256(__m256i, __m256i);
int _mm256_testz_pd(__m256d, __m256d);
int _mm256_testz_ps(__m256, __m256);
int _mm256_testz_si256(__m256i, __m256i);
__m256d _mm256_undefined_pd(void);
__m256 _mm256_undefined_ps(void);
__m256i _mm256_undefined_si256(void);
__m256d _mm256_unpackhi_pd(__m256d, __m256d);
__m256 _mm256_unpackhi_ps(__m256, __m256);
__m256d _mm256_unpacklo_pd(__m256d, __m256d);
__m256 _mm256_unpacklo_ps(__m256, __m256);
__m256d _mm256_xor_pd(__m256d, __m256d);
__m256 _mm256_xor_ps(__m256, __m256);
void _mm256_zeroall(void);
void _mm256_zeroupper(void);
__m256d _mm256_zextpd128_pd256(__m128d);
__m256 _mm256_zextps128_ps256(__m128);
__m256i _mm256_zextsi128_si256(__m128i);
__m128d _mm_cmp_pd(__m128d, __m128d, const int);
__m128 _mm_cmp_ps(__m128, __m128, const int);
__m128d _mm_cmp_sd(__m128d, __m128d, const int);
__m128 _mm_cmp_ss(__m128, __m128, const int);
__m128d _mm_maskload_pd(const double *, __m128i);
__m128 _mm_maskload_ps(const float *, __m128i);
void _mm_maskstore_pd(double *, __m128i, __m128d);
void _mm_maskstore_ps(float *, __m128i, __m128);
__m128d _mm_permute_pd(__m128d, const int);
__m128 _mm_permute_ps(__m128, const int);
__m128d _mm_permutevar_pd(__m128d, __m128i);
__m128 _mm_permutevar_ps(__m128, __m128i);
int _mm_testc_pd(__m128d, __m128d);
int _mm_testc_ps(__m128, __m128);
int _mm_testnzc_pd(__m128d, __m128d);
int _mm_testnzc_ps(__m128, __m128);
int _mm_testz_pd(__m128d, __m128d);
int _mm_testz_ps(__m128, __m128);
#ifdef __x86_64__
long long _mm256_extract_epi64(__m256i, const int);
__m256i _mm256_insert_epi64(__m256i, long long, const int);
#endif
/* @generated end */

/* @generated avx2 — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m256i _mm256_abs_epi16(__m256i);
__m256i _mm256_abs_epi32(__m256i);
__m256i _mm256_abs_epi8(__m256i);
__m256i _mm256_add_epi16(__m256i, __m256i);
__m256i _mm256_add_epi32(__m256i, __m256i);
__m256i _mm256_add_epi64(__m256i, __m256i);
__m256i _mm256_add_epi8(__m256i, __m256i);
__m256i _mm256_adds_epi16(__m256i, __m256i);
__m256i _mm256_adds_epi8(__m256i, __m256i);
__m256i _mm256_adds_epu16(__m256i, __m256i);
__m256i _mm256_adds_epu8(__m256i, __m256i);
__m256i _mm256_alignr_epi8(__m256i, __m256i, const int);
__m256i _mm256_and_si256(__m256i, __m256i);
__m256i _mm256_andnot_si256(__m256i, __m256i);
__m256i _mm256_avg_epu16(__m256i, __m256i);
__m256i _mm256_avg_epu8(__m256i, __m256i);
__m256i _mm256_blend_epi16(__m256i, __m256i, const int);
__m256i _mm256_blend_epi32(__m256i, __m256i, const int);
__m256i _mm256_blendv_epi8(__m256i, __m256i, __m256i);
__m256i _mm256_broadcastb_epi8(__m128i);
__m256i _mm256_broadcastd_epi32(__m128i);
__m256i _mm256_broadcastq_epi64(__m128i);
__m256d _mm256_broadcastsd_pd(__m128d);
__m256i _mm256_broadcastsi128_si256(__m128i);
__m256 _mm256_broadcastss_ps(__m128);
__m256i _mm256_broadcastw_epi16(__m128i);
__m256i _mm256_bslli_epi128(__m256i, const int);
__m256i _mm256_bsrli_epi128(__m256i, const int);
__m256i _mm256_cmpeq_epi16(__m256i, __m256i);
__m256i _mm256_cmpeq_epi32(__m256i, __m256i);
__m256i _mm256_cmpeq_epi64(__m256i, __m256i);
__m256i _mm256_cmpeq_epi8(__m256i, __m256i);
__m256i _mm256_cmpgt_epi16(__m256i, __m256i);
__m256i _mm256_cmpgt_epi32(__m256i, __m256i);
__m256i _mm256_cmpgt_epi64(__m256i, __m256i);
__m256i _mm256_cmpgt_epi8(__m256i, __m256i);
__m256i _mm256_cvtepi16_epi32(__m128i);
__m256i _mm256_cvtepi16_epi64(__m128i);
__m256i _mm256_cvtepi32_epi64(__m128i);
__m256i _mm256_cvtepi8_epi16(__m128i);
__m256i _mm256_cvtepi8_epi32(__m128i);
__m256i _mm256_cvtepi8_epi64(__m128i);
__m256i _mm256_cvtepu16_epi32(__m128i);
__m256i _mm256_cvtepu16_epi64(__m128i);
__m256i _mm256_cvtepu32_epi64(__m128i);
__m256i _mm256_cvtepu8_epi16(__m128i);
__m256i _mm256_cvtepu8_epi32(__m128i);
__m256i _mm256_cvtepu8_epi64(__m128i);
int _mm256_extract_epi16(__m256i, const int);
int _mm256_extract_epi8(__m256i, const int);
__m128i _mm256_extracti128_si256(__m256i, const int);
__m256i _mm256_hadd_epi16(__m256i, __m256i);
__m256i _mm256_hadd_epi32(__m256i, __m256i);
__m256i _mm256_hadds_epi16(__m256i, __m256i);
__m256i _mm256_hsub_epi16(__m256i, __m256i);
__m256i _mm256_hsub_epi32(__m256i, __m256i);
__m256i _mm256_hsubs_epi16(__m256i, __m256i);
__m256i _mm256_i32gather_epi32(const int *, __m256i, const int);
__m256i _mm256_i32gather_epi64(const long long *, __m128i, const int);
__m256d _mm256_i32gather_pd(const double *, __m128i, const int);
__m256 _mm256_i32gather_ps(const float *, __m256i, const int);
__m128i _mm256_i64gather_epi32(const int *, __m256i, const int);
__m256i _mm256_i64gather_epi64(const long long *, __m256i, const int);
__m256d _mm256_i64gather_pd(const double *, __m256i, const int);
__m128 _mm256_i64gather_ps(const float *, __m256i, const int);
__m256i _mm256_inserti128_si256(__m256i, __m128i, const int);
__m256i _mm256_madd_epi16(__m256i, __m256i);
__m256i _mm256_maddubs_epi16(__m256i, __m256i);
__m256i _mm256_mask_i32gather_epi32(__m256i, const int *, __m256i, __m256i, const int);
__m256i _mm256_mask_i32gather_epi64(__m256i, const long long *, __m128i, __m256i, const int);
__m256d _mm256_mask_i32gather_pd(__m256d, const double *, __m128i, __m256d, const int);
__m256 _mm256_mask_i32gather_ps(__m256, const float *, __m256i, __m256, const int);
__m128i _mm256_mask_i64gather_epi32(__m128i, const int *, __m256i, __m128i, const int);
__m256i _mm256_mask_i64gather_epi64(__m256i, const long long *, __m256i, __m256i, const int);
__m256d _mm256_mask_i64gather_pd(__m256d, const double *, __m256i, __m256d, const int);
__m128 _mm256_mask_i64gather_ps(__m128, const float *, __m256i, __m128, const int);
__m256i _mm256_maskload_epi32(const int *, __m256i);
__m256i _mm256_maskload_epi64(const long long *, __m256i);
void _mm256_maskstore_epi32(int *, __m256i, __m256i);
void _mm256_maskstore_epi64(long long *, __m256i, __m256i);
__m256i _mm256_max_epi16(__m256i, __m256i);
__m256i _mm256_max_epi32(__m256i, __m256i);
__m256i _mm256_max_epi8(__m256i, __m256i);
__m256i _mm256_max_epu16(__m256i, __m256i);
__m256i _mm256_max_epu32(__m256i, __m256i);
__m256i _mm256_max_epu8(__m256i, __m256i);
__m256i _mm256_min_epi16(__m256i, __m256i);
__m256i _mm256_min_epi32(__m256i, __m256i);
__m256i _mm256_min_epi8(__m256i, __m256i);
__m256i _mm256_min_epu16(__m256i, __m256i);
__m256i _mm256_min_epu32(__m256i, __m256i);
__m256i _mm256_min_epu8(__m256i, __m256i);
int _mm256_movemask_epi8(__m256i);
__m256i _mm256_mpsadbw_epu8(__m256i, __m256i, const int);
__m256i _mm256_mul_epi32(__m256i, __m256i);
__m256i _mm256_mul_epu32(__m256i, __m256i);
__m256i _mm256_mulhi_epi16(__m256i, __m256i);
__m256i _mm256_mulhi_epu16(__m256i, __m256i);
__m256i _mm256_mulhrs_epi16(__m256i, __m256i);
__m256i _mm256_mullo_epi16(__m256i, __m256i);
__m256i _mm256_mullo_epi32(__m256i, __m256i);
__m256i _mm256_or_si256(__m256i, __m256i);
__m256i _mm256_packs_epi16(__m256i, __m256i);
__m256i _mm256_packs_epi32(__m256i, __m256i);
__m256i _mm256_packus_epi16(__m256i, __m256i);
__m256i _mm256_packus_epi32(__m256i, __m256i);
__m256i _mm256_permute2x128_si256(__m256i, __m256i, const int);
__m256i _mm256_permute4x64_epi64(__m256i, const int);
__m256d _mm256_permute4x64_pd(__m256d, const int);
__m256i _mm256_permutevar8x32_epi32(__m256i, __m256i);
__m256 _mm256_permutevar8x32_ps(__m256, __m256i);
__m256i _mm256_sad_epu8(__m256i, __m256i);
__m256i _mm256_shuffle_epi32(__m256i, const int);
__m256i _mm256_shuffle_epi8(__m256i, __m256i);
__m256i _mm256_shufflehi_epi16(__m256i, const int);
__m256i _mm256_shufflelo_epi16(__m256i, const int);
__m256i _mm256_sign_epi16(__m256i, __m256i);
__m256i _mm256_sign_epi32(__m256i, __m256i);
__m256i _mm256_sign_epi8(__m256i, __m256i);
__m256i _mm256_sll_epi16(__m256i, __m128i);
__m256i _mm256_sll_epi32(__m256i, __m128i);
__m256i _mm256_sll_epi64(__m256i, __m128i);
__m256i _mm256_slli_epi16(__m256i, const int);
__m256i _mm256_slli_epi32(__m256i, const int);
__m256i _mm256_slli_epi64(__m256i, const int);
__m256i _mm256_slli_si256(__m256i, const int);
__m256i _mm256_sllv_epi32(__m256i, __m256i);
__m256i _mm256_sllv_epi64(__m256i, __m256i);
__m256i _mm256_sra_epi16(__m256i, __m128i);
__m256i _mm256_sra_epi32(__m256i, __m128i);
__m256i _mm256_srai_epi16(__m256i, const int);
__m256i _mm256_srai_epi32(__m256i, const int);
__m256i _mm256_srav_epi32(__m256i, __m256i);
__m256i _mm256_srl_epi16(__m256i, __m128i);
__m256i _mm256_srl_epi32(__m256i, __m128i);
__m256i _mm256_srl_epi64(__m256i, __m128i);
__m256i _mm256_srli_epi16(__m256i, const int);
__m256i _mm256_srli_epi32(__m256i, const int);
__m256i _mm256_srli_epi64(__m256i, const int);
__m256i _mm256_srli_si256(__m256i, const int);
__m256i _mm256_srlv_epi32(__m256i, __m256i);
__m256i _mm256_srlv_epi64(__m256i, __m256i);
__m256i _mm256_stream_load_si256(const __m256i *);
__m256i _mm256_sub_epi16(__m256i, __m256i);
__m256i _mm256_sub_epi32(__m256i, __m256i);
__m256i _mm256_sub_epi64(__m256i, __m256i);
__m256i _mm256_sub_epi8(__m256i, __m256i);
__m256i _mm256_subs_epi16(__m256i, __m256i);
__m256i _mm256_subs_epi8(__m256i, __m256i);
__m256i _mm256_subs_epu16(__m256i, __m256i);
__m256i _mm256_subs_epu8(__m256i, __m256i);
__m256i _mm256_unpackhi_epi16(__m256i, __m256i);
__m256i _mm256_unpackhi_epi32(__m256i, __m256i);
__m256i _mm256_unpackhi_epi64(__m256i, __m256i);
__m256i _mm256_unpackhi_epi8(__m256i, __m256i);
__m256i _mm256_unpacklo_epi16(__m256i, __m256i);
__m256i _mm256_unpacklo_epi32(__m256i, __m256i);
__m256i _mm256_unpacklo_epi64(__m256i, __m256i);
__m256i _mm256_unpacklo_epi8(__m256i, __m256i);
__m256i _mm256_xor_si256(__m256i, __m256i);
__m128i _mm_blend_epi32(__m128i, __m128i, const int);
__m128i _mm_broadcastb_epi8(__m128i);
__m128i _mm_broadcastd_epi32(__m128i);
__m128i _mm_broadcastq_epi64(__m128i);
__m128d _mm_broadcastsd_pd(__m128d);
__m256i _mm_broadcastsi128_si256(__m128i);
__m128 _mm_broadcastss_ps(__m128);
__m128i _mm_broadcastw_epi16(__m128i);
__m128i _mm_i32gather_epi32(const int *, __m128i, const int);
__m128i _mm_i32gather_epi64(const long long *, __m128i, const int);
__m128d _mm_i32gather_pd(const double *, __m128i, const int);
__m128 _mm_i32gather_ps(const float *, __m128i, const int);
__m128i _mm_i64gather_epi32(const int *, __m128i, const int);
__m128i _mm_i64gather_epi64(const long long *, __m128i, const int);
__m128d _mm_i64gather_pd(const double *, __m128i, const int);
__m128 _mm_i64gather_ps(const float *, __m128i, const int);
__m128i _mm_mask_i32gather_epi32(__m128i, const int *, __m128i, __m128i, const int);
__m128i _mm_mask_i32gather_epi64(__m128i, const long long *, __m128i, __m128i, const int);
__m128d _mm_mask_i32gather_pd(__m128d, const double *, __m128i, __m128d, const int);
__m128 _mm_mask_i32gather_ps(__m128, const float *, __m128i, __m128, const int);
__m128i _mm_mask_i64gather_epi32(__m128i, const int *, __m128i, __m128i, const int);
__m128i _mm_mask_i64gather_epi64(__m128i, const long long *, __m128i, __m128i, const int);
__m128d _mm_mask_i64gather_pd(__m128d, const double *, __m128i, __m128d, const int);
__m128 _mm_mask_i64gather_ps(__m128, const float *, __m128i, __m128, const int);
__m128i _mm_maskload_epi32(const int *, __m128i);
__m128i _mm_maskload_epi64(const long long *, __m128i);
void _mm_maskstore_epi32(int *, __m128i, __m128i);
void _mm_maskstore_epi64(long long *, __m128i, __m128i);
__m128i _mm_sllv_epi32(__m128i, __m128i);
__m128i _mm_sllv_epi64(__m128i, __m128i);
__m128i _mm_srav_epi32(__m128i, __m128i);
__m128i _mm_srlv_epi32(__m128i, __m128i);
__m128i _mm_srlv_epi64(__m128i, __m128i);
/* @generated end */

/* @generated fma — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m256d _mm256_fmadd_pd(__m256d, __m256d, __m256d);
__m256 _mm256_fmadd_ps(__m256, __m256, __m256);
__m256d _mm256_fmaddsub_pd(__m256d, __m256d, __m256d);
__m256 _mm256_fmaddsub_ps(__m256, __m256, __m256);
__m256d _mm256_fmsub_pd(__m256d, __m256d, __m256d);
__m256 _mm256_fmsub_ps(__m256, __m256, __m256);
__m256d _mm256_fmsubadd_pd(__m256d, __m256d, __m256d);
__m256 _mm256_fmsubadd_ps(__m256, __m256, __m256);
__m256d _mm256_fnmadd_pd(__m256d, __m256d, __m256d);
__m256 _mm256_fnmadd_ps(__m256, __m256, __m256);
__m256d _mm256_fnmsub_pd(__m256d, __m256d, __m256d);
__m256 _mm256_fnmsub_ps(__m256, __m256, __m256);
__m128d _mm_fmadd_pd(__m128d, __m128d, __m128d);
__m128 _mm_fmadd_ps(__m128, __m128, __m128);
__m128d _mm_fmadd_sd(__m128d, __m128d, __m128d);
__m128 _mm_fmadd_ss(__m128, __m128, __m128);
__m128d _mm_fmaddsub_pd(__m128d, __m128d, __m128d);
__m128 _mm_fmaddsub_ps(__m128, __m128, __m128);
__m128d _mm_fmsub_pd(__m128d, __m128d, __m128d);
__m128 _mm_fmsub_ps(__m128, __m128, __m128);
__m128d _mm_fmsub_sd(__m128d, __m128d, __m128d);
__m128 _mm_fmsub_ss(__m128, __m128, __m128);
__m128d _mm_fmsubadd_pd(__m128d, __m128d, __m128d);
__m128 _mm_fmsubadd_ps(__m128, __m128, __m128);
__m128d _mm_fnmadd_pd(__m128d, __m128d, __m128d);
__m128 _mm_fnmadd_ps(__m128, __m128, __m128);
__m128d _mm_fnmadd_sd(__m128d, __m128d, __m128d);
__m128 _mm_fnmadd_ss(__m128, __m128, __m128);
__m128d _mm_fnmsub_pd(__m128d, __m128d, __m128d);
__m128 _mm_fnmsub_ps(__m128, __m128, __m128);
__m128d _mm_fnmsub_sd(__m128d, __m128d, __m128d);
__m128 _mm_fnmsub_ss(__m128, __m128, __m128);
/* @generated end */

/* @generated sha — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m128i _mm_sha1msg1_epu32(__m128i, __m128i);
__m128i _mm_sha1msg2_epu32(__m128i, __m128i);
__m128i _mm_sha1nexte_epu32(__m128i, __m128i);
__m128i _mm_sha1rnds4_epu32(__m128i, __m128i, const int);
__m128i _mm_sha256msg1_epu32(__m128i, __m128i);
__m128i _mm_sha256msg2_epu32(__m128i, __m128i);
__m128i _mm_sha256rnds2_epu32(__m128i, __m128i, __m128i);
/* @generated end */

/* @generated bmi1 — see crates/cinrs-core/tests/x86_intrinsics.rs */
unsigned int _andn_u32(unsigned int, unsigned int);
unsigned int _bextr2_u32(unsigned int, unsigned int);
unsigned int _bextr_u32(unsigned int, unsigned int, unsigned int);
unsigned int _blsi_u32(unsigned int);
unsigned int _blsmsk_u32(unsigned int);
unsigned int _blsr_u32(unsigned int);
int _mm_tzcnt_32(unsigned int);
unsigned short _tzcnt_u16(unsigned short);
unsigned int _tzcnt_u32(unsigned int);
#ifdef __x86_64__
unsigned long long _andn_u64(unsigned long long, unsigned long long);
unsigned long long _bextr2_u64(unsigned long long, unsigned long long);
unsigned long long _bextr_u64(unsigned long long, unsigned int, unsigned int);
unsigned long long _blsi_u64(unsigned long long);
unsigned long long _blsmsk_u64(unsigned long long);
unsigned long long _blsr_u64(unsigned long long);
long long _mm_tzcnt_64(unsigned long long);
unsigned long long _tzcnt_u64(unsigned long long);
#endif
/* @generated end */

/* @generated bmi2 — see crates/cinrs-core/tests/x86_intrinsics.rs */
unsigned int _bzhi_u32(unsigned int, unsigned int);
unsigned int _pdep_u32(unsigned int, unsigned int);
unsigned int _pext_u32(unsigned int, unsigned int);
#ifdef __x86_64__
unsigned long long _bzhi_u64(unsigned long long, unsigned int);
unsigned long long _pdep_u64(unsigned long long, unsigned long long);
unsigned long long _pext_u64(unsigned long long, unsigned long long);
#endif
/* @generated end */

/* @generated lzcnt — see crates/cinrs-core/tests/x86_intrinsics.rs */
unsigned int _lzcnt_u32(unsigned int);
int _popcnt32(int);
#ifdef __x86_64__
unsigned long long _lzcnt_u64(unsigned long long);
int _popcnt64(long long);
#endif
/* @generated end */

/* GCC and Clang spell the population count both ways; `core::arch` has only
 * `_popcnt32` and `_popcnt64`, so the Intel spellings are macros over those. */
#define _mm_popcnt_u32(a) ((unsigned int)_popcnt32((int)(a)))
#ifdef __x86_64__
#define _mm_popcnt_u64(a) ((unsigned long long)_popcnt64((long long)(a)))
#endif

/* The broadcast-from-memory forms take a *reference* in `core::arch`, which a
 * C pointer cannot be passed as, so they are written as the load-and-splat
 * they are defined to be. `_mm256_broadcast_ss(p)` is `_mm256_set1_ps(*p)` by
 * definition, and the compiler emits the same `vbroadcastss` for both. */
#define _mm_broadcast_ss(p) _mm_set1_ps(*(const float *)(p))
#define _mm256_broadcast_ss(p) _mm256_set1_ps(*(const float *)(p))
#define _mm256_broadcast_sd(p) _mm256_set1_pd(*(const double *)(p))
#define _mm256_broadcast_ps(p) \
    _mm256_set_m128(*(const __m128 *)(p), *(const __m128 *)(p))
#define _mm256_broadcast_pd(p) \
    _mm256_set_m128d(*(const __m128d *)(p), *(const __m128d *)(p))

/* AVX-512 and the instruction sets that arrived with it, one header each as
 * GCC has them, in the order their typedefs need: `__m512*`, `__mmask8` and
 * `__mmask16` come from <avx512fintrin.h>, the wide masks from
 * <avx512bwintrin.h>. Each of these, included on its own, includes this file.
 * A function that passes or returns a 512-bit vector by value needs
 * `__attribute__((target("avx512f")))`, as a 256-bit one needs "avx". */
#include <avx512fintrin.h>
#include <avx512bwintrin.h>
#include <avx512cdintrin.h>
#include <avx512dqintrin.h>
#include <avx512vlintrin.h>
#include <avx512vlbwintrin.h>
#include <avx512vldqintrin.h>
#include <avx512vbmiintrin.h>
#include <avx512vbmivlintrin.h>
#include <avx512vbmi2intrin.h>
#include <avx512vbmi2vlintrin.h>
#include <avx512vnniintrin.h>
#include <avx512vnnivlintrin.h>
#include <avx512bitalgintrin.h>
#include <avx512bitalgvlintrin.h>
#include <avx512vpopcntdqintrin.h>
#include <avx512vpopcntdqvlintrin.h>
#include <avx512ifmaintrin.h>
#include <avx512ifmavlintrin.h>
#include <avx512bf16intrin.h>
#include <avx512bf16vlintrin.h>
#include <avx512fp16intrin.h>
#include <avx512fp16vlintrin.h>
#include <avx512vp2intersectintrin.h>
#include <gfniintrin.h>
#include <vaesintrin.h>
#include <vpclmulqdqintrin.h>
#include <avxvnniintrin.h>
#include <avxvnniint8intrin.h>
#include <avxvnniint16intrin.h>
#include <avxifmaintrin.h>
#include <f16cintrin.h>
#include <sha512intrin.h>
#include <sm3intrin.h>
#include <sm4intrin.h>

#endif /* x86 */
#endif /* _CINRS_IMMINTRIN_H */
