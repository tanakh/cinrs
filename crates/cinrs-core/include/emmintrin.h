/* <emmintrin.h> — the SSE2 intrinsics, and `__m128i` / `__m128d`.
 *
 * SSE2 is the baseline of every x86-64 target, which is why `__SSE2__` is
 * predefined there and why a C file that uses nothing above it needs no
 * `__attribute__((target(…)))` at all. See <xmmintrin.h> for how a call is
 * generated and what an immediate operand has to be.
 */
#ifndef _CINRS_EMMINTRIN_H
#define _CINRS_EMMINTRIN_H

#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#else

#include <xmmintrin.h>

typedef __cinrs_m128i __m128i;
typedef __cinrs_m128d __m128d;

/* The two-lane shuffle selector `_mm_shuffle_pd` takes. */
#define _MM_SHUFFLE2(fp1, fp0) (((fp1) << 1) | (fp0))

/* @generated sse2 — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m128i _mm_add_epi16(__m128i, __m128i);
__m128i _mm_add_epi32(__m128i, __m128i);
__m128i _mm_add_epi64(__m128i, __m128i);
__m128i _mm_add_epi8(__m128i, __m128i);
__m128d _mm_add_pd(__m128d, __m128d);
__m128d _mm_add_sd(__m128d, __m128d);
__m128i _mm_adds_epi16(__m128i, __m128i);
__m128i _mm_adds_epi8(__m128i, __m128i);
__m128i _mm_adds_epu16(__m128i, __m128i);
__m128i _mm_adds_epu8(__m128i, __m128i);
__m128d _mm_and_pd(__m128d, __m128d);
__m128i _mm_and_si128(__m128i, __m128i);
__m128d _mm_andnot_pd(__m128d, __m128d);
__m128i _mm_andnot_si128(__m128i, __m128i);
__m128i _mm_avg_epu16(__m128i, __m128i);
__m128i _mm_avg_epu8(__m128i, __m128i);
__m128i _mm_bslli_si128(__m128i, const int);
__m128i _mm_bsrli_si128(__m128i, const int);
__m128 _mm_castpd_ps(__m128d);
__m128i _mm_castpd_si128(__m128d);
__m128d _mm_castps_pd(__m128);
__m128i _mm_castps_si128(__m128);
__m128d _mm_castsi128_pd(__m128i);
__m128 _mm_castsi128_ps(__m128i);
void _mm_clflush(const unsigned char *);
__m128i _mm_cmpeq_epi16(__m128i, __m128i);
__m128i _mm_cmpeq_epi32(__m128i, __m128i);
__m128i _mm_cmpeq_epi8(__m128i, __m128i);
__m128d _mm_cmpeq_pd(__m128d, __m128d);
__m128d _mm_cmpeq_sd(__m128d, __m128d);
__m128d _mm_cmpge_pd(__m128d, __m128d);
__m128d _mm_cmpge_sd(__m128d, __m128d);
__m128i _mm_cmpgt_epi16(__m128i, __m128i);
__m128i _mm_cmpgt_epi32(__m128i, __m128i);
__m128i _mm_cmpgt_epi8(__m128i, __m128i);
__m128d _mm_cmpgt_pd(__m128d, __m128d);
__m128d _mm_cmpgt_sd(__m128d, __m128d);
__m128d _mm_cmple_pd(__m128d, __m128d);
__m128d _mm_cmple_sd(__m128d, __m128d);
__m128i _mm_cmplt_epi16(__m128i, __m128i);
__m128i _mm_cmplt_epi32(__m128i, __m128i);
__m128i _mm_cmplt_epi8(__m128i, __m128i);
__m128d _mm_cmplt_pd(__m128d, __m128d);
__m128d _mm_cmplt_sd(__m128d, __m128d);
__m128d _mm_cmpneq_pd(__m128d, __m128d);
__m128d _mm_cmpneq_sd(__m128d, __m128d);
__m128d _mm_cmpnge_pd(__m128d, __m128d);
__m128d _mm_cmpnge_sd(__m128d, __m128d);
__m128d _mm_cmpngt_pd(__m128d, __m128d);
__m128d _mm_cmpngt_sd(__m128d, __m128d);
__m128d _mm_cmpnle_pd(__m128d, __m128d);
__m128d _mm_cmpnle_sd(__m128d, __m128d);
__m128d _mm_cmpnlt_pd(__m128d, __m128d);
__m128d _mm_cmpnlt_sd(__m128d, __m128d);
__m128d _mm_cmpord_pd(__m128d, __m128d);
__m128d _mm_cmpord_sd(__m128d, __m128d);
__m128d _mm_cmpunord_pd(__m128d, __m128d);
__m128d _mm_cmpunord_sd(__m128d, __m128d);
int _mm_comieq_sd(__m128d, __m128d);
int _mm_comige_sd(__m128d, __m128d);
int _mm_comigt_sd(__m128d, __m128d);
int _mm_comile_sd(__m128d, __m128d);
int _mm_comilt_sd(__m128d, __m128d);
int _mm_comineq_sd(__m128d, __m128d);
__m128d _mm_cvtepi32_pd(__m128i);
__m128 _mm_cvtepi32_ps(__m128i);
__m128i _mm_cvtpd_epi32(__m128d);
__m128 _mm_cvtpd_ps(__m128d);
__m128i _mm_cvtps_epi32(__m128);
__m128d _mm_cvtps_pd(__m128);
double _mm_cvtsd_f64(__m128d);
int _mm_cvtsd_si32(__m128d);
__m128 _mm_cvtsd_ss(__m128, __m128d);
int _mm_cvtsi128_si32(__m128i);
__m128d _mm_cvtsi32_sd(__m128d, int);
__m128i _mm_cvtsi32_si128(int);
__m128d _mm_cvtss_sd(__m128d, __m128);
__m128i _mm_cvttpd_epi32(__m128d);
__m128i _mm_cvttps_epi32(__m128);
int _mm_cvttsd_si32(__m128d);
__m128d _mm_div_pd(__m128d, __m128d);
__m128d _mm_div_sd(__m128d, __m128d);
int _mm_extract_epi16(__m128i, const int);
__m128i _mm_insert_epi16(__m128i, int, const int);
void _mm_lfence(void);
__m128d _mm_load1_pd(const double *);
__m128d _mm_load_pd(const double *);
__m128d _mm_load_pd1(const double *);
__m128d _mm_load_sd(const double *);
__m128i _mm_load_si128(const __m128i *);
__m128d _mm_loadh_pd(__m128d, const double *);
__m128i _mm_loadl_epi64(const __m128i *);
__m128d _mm_loadl_pd(__m128d, const double *);
__m128d _mm_loadr_pd(const double *);
__m128d _mm_loadu_pd(const double *);
__m128i _mm_loadu_si128(const __m128i *);
__m128i _mm_loadu_si16(const unsigned char *);
__m128i _mm_loadu_si32(const unsigned char *);
__m128i _mm_loadu_si64(const unsigned char *);
__m128i _mm_madd_epi16(__m128i, __m128i);
void _mm_maskmoveu_si128(__m128i, __m128i, char *);
__m128i _mm_max_epi16(__m128i, __m128i);
__m128i _mm_max_epu8(__m128i, __m128i);
__m128d _mm_max_pd(__m128d, __m128d);
__m128d _mm_max_sd(__m128d, __m128d);
void _mm_mfence(void);
__m128i _mm_min_epi16(__m128i, __m128i);
__m128i _mm_min_epu8(__m128i, __m128i);
__m128d _mm_min_pd(__m128d, __m128d);
__m128d _mm_min_sd(__m128d, __m128d);
__m128i _mm_move_epi64(__m128i);
__m128d _mm_move_sd(__m128d, __m128d);
int _mm_movemask_epi8(__m128i);
int _mm_movemask_pd(__m128d);
__m128i _mm_mul_epu32(__m128i, __m128i);
__m128d _mm_mul_pd(__m128d, __m128d);
__m128d _mm_mul_sd(__m128d, __m128d);
__m128i _mm_mulhi_epi16(__m128i, __m128i);
__m128i _mm_mulhi_epu16(__m128i, __m128i);
__m128i _mm_mullo_epi16(__m128i, __m128i);
__m128d _mm_or_pd(__m128d, __m128d);
__m128i _mm_or_si128(__m128i, __m128i);
__m128i _mm_packs_epi16(__m128i, __m128i);
__m128i _mm_packs_epi32(__m128i, __m128i);
__m128i _mm_packus_epi16(__m128i, __m128i);
void _mm_pause(void);
__m128i _mm_sad_epu8(__m128i, __m128i);
__m128i _mm_set1_epi16(short);
__m128i _mm_set1_epi32(int);
__m128i _mm_set1_epi64x(long long);
__m128i _mm_set1_epi8(char);
__m128d _mm_set1_pd(double);
__m128i _mm_set_epi16(short, short, short, short, short, short, short, short);
__m128i _mm_set_epi32(int, int, int, int);
__m128i _mm_set_epi64x(long long, long long);
__m128i _mm_set_epi8(char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char);
__m128d _mm_set_pd(double, double);
__m128d _mm_set_pd1(double);
__m128d _mm_set_sd(double);
__m128i _mm_setr_epi16(short, short, short, short, short, short, short, short);
__m128i _mm_setr_epi32(int, int, int, int);
__m128i _mm_setr_epi8(char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char);
__m128d _mm_setr_pd(double, double);
__m128d _mm_setzero_pd(void);
__m128i _mm_setzero_si128(void);
__m128i _mm_shuffle_epi32(__m128i, const int);
__m128d _mm_shuffle_pd(__m128d, __m128d, const int);
__m128i _mm_shufflehi_epi16(__m128i, const int);
__m128i _mm_shufflelo_epi16(__m128i, const int);
__m128i _mm_sll_epi16(__m128i, __m128i);
__m128i _mm_sll_epi32(__m128i, __m128i);
__m128i _mm_sll_epi64(__m128i, __m128i);
__m128i _mm_slli_epi16(__m128i, const int);
__m128i _mm_slli_epi32(__m128i, const int);
__m128i _mm_slli_epi64(__m128i, const int);
__m128i _mm_slli_si128(__m128i, const int);
__m128d _mm_sqrt_pd(__m128d);
__m128d _mm_sqrt_sd(__m128d, __m128d);
__m128i _mm_sra_epi16(__m128i, __m128i);
__m128i _mm_sra_epi32(__m128i, __m128i);
__m128i _mm_srai_epi16(__m128i, const int);
__m128i _mm_srai_epi32(__m128i, const int);
__m128i _mm_srl_epi16(__m128i, __m128i);
__m128i _mm_srl_epi32(__m128i, __m128i);
__m128i _mm_srl_epi64(__m128i, __m128i);
__m128i _mm_srli_epi16(__m128i, const int);
__m128i _mm_srli_epi32(__m128i, const int);
__m128i _mm_srli_epi64(__m128i, const int);
__m128i _mm_srli_si128(__m128i, const int);
void _mm_store1_pd(double *, __m128d);
void _mm_store_pd(double *, __m128d);
void _mm_store_pd1(double *, __m128d);
void _mm_store_sd(double *, __m128d);
void _mm_store_si128(__m128i *, __m128i);
void _mm_storeh_pd(double *, __m128d);
void _mm_storel_epi64(__m128i *, __m128i);
void _mm_storel_pd(double *, __m128d);
void _mm_storer_pd(double *, __m128d);
void _mm_storeu_pd(double *, __m128d);
void _mm_storeu_si128(__m128i *, __m128i);
void _mm_storeu_si16(unsigned char *, __m128i);
void _mm_storeu_si32(unsigned char *, __m128i);
void _mm_storeu_si64(unsigned char *, __m128i);
void _mm_stream_pd(double *, __m128d);
void _mm_stream_si128(__m128i *, __m128i);
void _mm_stream_si32(int *, int);
__m128i _mm_sub_epi16(__m128i, __m128i);
__m128i _mm_sub_epi32(__m128i, __m128i);
__m128i _mm_sub_epi64(__m128i, __m128i);
__m128i _mm_sub_epi8(__m128i, __m128i);
__m128d _mm_sub_pd(__m128d, __m128d);
__m128d _mm_sub_sd(__m128d, __m128d);
__m128i _mm_subs_epi16(__m128i, __m128i);
__m128i _mm_subs_epi8(__m128i, __m128i);
__m128i _mm_subs_epu16(__m128i, __m128i);
__m128i _mm_subs_epu8(__m128i, __m128i);
int _mm_ucomieq_sd(__m128d, __m128d);
int _mm_ucomige_sd(__m128d, __m128d);
int _mm_ucomigt_sd(__m128d, __m128d);
int _mm_ucomile_sd(__m128d, __m128d);
int _mm_ucomilt_sd(__m128d, __m128d);
int _mm_ucomineq_sd(__m128d, __m128d);
__m128d _mm_undefined_pd(void);
__m128i _mm_undefined_si128(void);
__m128i _mm_unpackhi_epi16(__m128i, __m128i);
__m128i _mm_unpackhi_epi32(__m128i, __m128i);
__m128i _mm_unpackhi_epi64(__m128i, __m128i);
__m128i _mm_unpackhi_epi8(__m128i, __m128i);
__m128d _mm_unpackhi_pd(__m128d, __m128d);
__m128i _mm_unpacklo_epi16(__m128i, __m128i);
__m128i _mm_unpacklo_epi32(__m128i, __m128i);
__m128i _mm_unpacklo_epi64(__m128i, __m128i);
__m128i _mm_unpacklo_epi8(__m128i, __m128i);
__m128d _mm_unpacklo_pd(__m128d, __m128d);
__m128d _mm_xor_pd(__m128d, __m128d);
__m128i _mm_xor_si128(__m128i, __m128i);
#ifdef __x86_64__
long long _mm_cvtsd_si64(__m128d);
long long _mm_cvtsd_si64x(__m128d);
long long _mm_cvtsi128_si64(__m128i);
long long _mm_cvtsi128_si64x(__m128i);
__m128d _mm_cvtsi64_sd(__m128d, long long);
__m128i _mm_cvtsi64_si128(long long);
__m128d _mm_cvtsi64x_sd(__m128d, long long);
__m128i _mm_cvtsi64x_si128(long long);
long long _mm_cvttsd_si64(__m128d);
long long _mm_cvttsd_si64x(__m128d);
void _mm_stream_si64(long long *, long long);
#endif
/* @generated end */

#endif /* x86 */
#endif /* _CINRS_EMMINTRIN_H */
