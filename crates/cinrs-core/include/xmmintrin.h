/* <xmmintrin.h> — the SSE intrinsics, and the `__m128` type.
 *
 * This is the first header of the Intel intrinsics chain: <emmintrin.h>
 * includes it and adds SSE2, <pmmintrin.h> adds SSE3, and so on up to
 * <nmmintrin.h>; <immintrin.h> includes the whole chain and adds AVX, AVX2,
 * FMA and the scalar bit-manipulation intrinsics. Code written for an older
 * compiler includes the header its instruction set arrived in, which is why
 * they are separate files here as well.
 *
 * A call to one of these functions is **not** a call to a library: cinrs
 * generates `::core::arch::x86_64::_mm_add_ps(a, b)`, the function of the same
 * name and the same signature in Rust's own `core::arch`. The declarations
 * below were generated from `core::arch`'s source, so the two cannot disagree;
 * see `crates/cinrs-core/tests/x86_intrinsics.rs` and doc/features.md.
 *
 * An intrinsic whose last operand is an immediate — `_mm_shuffle_ps(a, b, m)`
 * — is a `const` generic in Rust, so that operand has to be an integer
 * constant expression, exactly as Intel's own compilers require. It is written
 * `const int` below.
 *
 * cinrs predefines `__SSE__` and `__SSE2__` on x86-64, which is the baseline
 * every x86-64 target has, and nothing above them: a procedural macro cannot
 * see rustc's `-C target-feature`. Ask for a higher instruction set with
 * `__attribute__((target("avx2")))` on the function, and test for it at run
 * time with `__builtin_cpu_supports("avx2")`.
 */
#ifndef _CINRS_XMMINTRIN_H
#define _CINRS_XMMINTRIN_H

#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#else

/* The 128-bit vector types are the compiler's own: they are
 * `::core::arch::x86_64::__m128` and friends in the generated Rust, sixteen
 * bytes long and sixteen-byte aligned, and they may be members, elements,
 * parameters, return values and the members of a union that punnes them.
 */
typedef __cinrs_m128 __m128;

/* The shuffle selector every `_mm_shuffle_*` takes: four two-bit lane
 * indices, most significant first. */
#define _MM_SHUFFLE(fp3, fp2, fp1, fp0) \
    (((fp3) << 6) | ((fp2) << 4) | ((fp1) << 2) | (fp0))

/* @generated constants — see crates/cinrs-core/tests/x86_intrinsics.rs */
#define _MM_EXCEPT_INVALID 0x0001u
#define _MM_EXCEPT_DENORM 0x0002u
#define _MM_EXCEPT_DIV_ZERO 0x0004u
#define _MM_EXCEPT_OVERFLOW 0x0008u
#define _MM_EXCEPT_UNDERFLOW 0x0010u
#define _MM_EXCEPT_INEXACT 0x0020u
#define _MM_EXCEPT_MASK 0x003fu
#define _MM_MASK_INVALID 0x0080u
#define _MM_MASK_DENORM 0x0100u
#define _MM_MASK_DIV_ZERO 0x0200u
#define _MM_MASK_OVERFLOW 0x0400u
#define _MM_MASK_UNDERFLOW 0x0800u
#define _MM_MASK_INEXACT 0x1000u
#define _MM_MASK_MASK 0x1f80u
#define _MM_ROUND_NEAREST 0x0000u
#define _MM_ROUND_DOWN 0x2000u
#define _MM_ROUND_UP 0x4000u
#define _MM_ROUND_TOWARD_ZERO 0x6000u
#define _MM_ROUND_MASK 0x6000u
#define _MM_FLUSH_ZERO_MASK 0x8000u
#define _MM_FLUSH_ZERO_ON 0x8000u
#define _MM_FLUSH_ZERO_OFF 0x0000u
#define _MM_HINT_T0 0x0003
#define _MM_HINT_T1 0x0002
#define _MM_HINT_T2 0x0001
#define _MM_HINT_NTA 0x0000
#define _MM_HINT_ET0 0x0007
#define _MM_HINT_ET1 0x0006
/* @generated end */

/* @generated sse — see crates/cinrs-core/tests/x86_intrinsics.rs */
__m128 _mm_add_ps(__m128, __m128);
__m128 _mm_add_ss(__m128, __m128);
__m128 _mm_and_ps(__m128, __m128);
__m128 _mm_andnot_ps(__m128, __m128);
__m128 _mm_cmpeq_ps(__m128, __m128);
__m128 _mm_cmpeq_ss(__m128, __m128);
__m128 _mm_cmpge_ps(__m128, __m128);
__m128 _mm_cmpge_ss(__m128, __m128);
__m128 _mm_cmpgt_ps(__m128, __m128);
__m128 _mm_cmpgt_ss(__m128, __m128);
__m128 _mm_cmple_ps(__m128, __m128);
__m128 _mm_cmple_ss(__m128, __m128);
__m128 _mm_cmplt_ps(__m128, __m128);
__m128 _mm_cmplt_ss(__m128, __m128);
__m128 _mm_cmpneq_ps(__m128, __m128);
__m128 _mm_cmpneq_ss(__m128, __m128);
__m128 _mm_cmpnge_ps(__m128, __m128);
__m128 _mm_cmpnge_ss(__m128, __m128);
__m128 _mm_cmpngt_ps(__m128, __m128);
__m128 _mm_cmpngt_ss(__m128, __m128);
__m128 _mm_cmpnle_ps(__m128, __m128);
__m128 _mm_cmpnle_ss(__m128, __m128);
__m128 _mm_cmpnlt_ps(__m128, __m128);
__m128 _mm_cmpnlt_ss(__m128, __m128);
__m128 _mm_cmpord_ps(__m128, __m128);
__m128 _mm_cmpord_ss(__m128, __m128);
__m128 _mm_cmpunord_ps(__m128, __m128);
__m128 _mm_cmpunord_ss(__m128, __m128);
int _mm_comieq_ss(__m128, __m128);
int _mm_comige_ss(__m128, __m128);
int _mm_comigt_ss(__m128, __m128);
int _mm_comile_ss(__m128, __m128);
int _mm_comilt_ss(__m128, __m128);
int _mm_comineq_ss(__m128, __m128);
__m128 _mm_cvt_si2ss(__m128, int);
int _mm_cvt_ss2si(__m128);
__m128 _mm_cvtsi32_ss(__m128, int);
float _mm_cvtss_f32(__m128);
int _mm_cvtss_si32(__m128);
int _mm_cvtt_ss2si(__m128);
int _mm_cvttss_si32(__m128);
__m128 _mm_div_ps(__m128, __m128);
__m128 _mm_div_ss(__m128, __m128);
__m128 _mm_load1_ps(const float *);
__m128 _mm_load_ps(const float *);
__m128 _mm_load_ps1(const float *);
__m128 _mm_load_ss(const float *);
__m128 _mm_loadr_ps(const float *);
__m128 _mm_loadu_ps(const float *);
__m128 _mm_max_ps(__m128, __m128);
__m128 _mm_max_ss(__m128, __m128);
__m128 _mm_min_ps(__m128, __m128);
__m128 _mm_min_ss(__m128, __m128);
__m128 _mm_move_ss(__m128, __m128);
__m128 _mm_movehl_ps(__m128, __m128);
__m128 _mm_movelh_ps(__m128, __m128);
int _mm_movemask_ps(__m128);
__m128 _mm_mul_ps(__m128, __m128);
__m128 _mm_mul_ss(__m128, __m128);
__m128 _mm_or_ps(__m128, __m128);
void _mm_prefetch(const char *, const int);
__m128 _mm_rcp_ps(__m128);
__m128 _mm_rcp_ss(__m128);
__m128 _mm_rsqrt_ps(__m128);
__m128 _mm_rsqrt_ss(__m128);
__m128 _mm_set1_ps(float);
__m128 _mm_set_ps(float, float, float, float);
__m128 _mm_set_ps1(float);
__m128 _mm_set_ss(float);
__m128 _mm_setr_ps(float, float, float, float);
__m128 _mm_setzero_ps(void);
void _mm_sfence(void);
__m128 _mm_shuffle_ps(__m128, __m128, const int);
__m128 _mm_sqrt_ps(__m128);
__m128 _mm_sqrt_ss(__m128);
void _mm_store1_ps(float *, __m128);
void _mm_store_ps(float *, __m128);
void _mm_store_ps1(float *, __m128);
void _mm_store_ss(float *, __m128);
void _mm_storer_ps(float *, __m128);
void _mm_storeu_ps(float *, __m128);
void _mm_stream_ps(float *, __m128);
__m128 _mm_sub_ps(__m128, __m128);
__m128 _mm_sub_ss(__m128, __m128);
int _mm_ucomieq_ss(__m128, __m128);
int _mm_ucomige_ss(__m128, __m128);
int _mm_ucomigt_ss(__m128, __m128);
int _mm_ucomile_ss(__m128, __m128);
int _mm_ucomilt_ss(__m128, __m128);
int _mm_ucomineq_ss(__m128, __m128);
__m128 _mm_undefined_ps(void);
__m128 _mm_unpackhi_ps(__m128, __m128);
__m128 _mm_unpacklo_ps(__m128, __m128);
__m128 _mm_xor_ps(__m128, __m128);
#ifdef __x86_64__
__m128 _mm_cvtsi64_ss(__m128, long long);
long long _mm_cvtss_si64(__m128);
long long _mm_cvttss_si64(__m128);
#endif
/* @generated end */

/* The 4x4 single-precision transpose Intel documents as a macro, in the
 * definition Intel gives it. */
#define _MM_TRANSPOSE4_PS(row0, row1, row2, row3)         \
    do {                                                  \
        __m128 __cinrs_t0 = _mm_unpacklo_ps(row0, row1);  \
        __m128 __cinrs_t1 = _mm_unpacklo_ps(row2, row3);  \
        __m128 __cinrs_t2 = _mm_unpackhi_ps(row0, row1);  \
        __m128 __cinrs_t3 = _mm_unpackhi_ps(row2, row3);  \
        (row0) = _mm_movelh_ps(__cinrs_t0, __cinrs_t1);   \
        (row1) = _mm_movehl_ps(__cinrs_t1, __cinrs_t0);   \
        (row2) = _mm_movelh_ps(__cinrs_t2, __cinrs_t3);   \
        (row3) = _mm_movehl_ps(__cinrs_t3, __cinrs_t2);   \
    } while (0)

#endif /* x86 */
#endif /* _CINRS_XMMINTRIN_H */
