/* <avx512fintrin.h> — the avx512f intrinsics, as GCC arranges them.
 *
 * Written by crates/cinrs-core/tests/x86_intrinsics.rs; the regions below
 * are regenerated from `core::arch`'s source, and an empty one is an
 * instruction set whose intrinsics are all still unstable there. Needs the
 * instruction set in `__attribute__((target("…")))` on the function that
 * calls them, and a function that passes or returns a 512-bit vector by
 * value needs `target("avx512f")`; see <immintrin.h>.
 */
#ifndef _CINRS_AVX512FINTRIN_H
#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#elif !defined(_CINRS_IMMINTRIN_H)
/* On its own: <immintrin.h> includes this file back, in its place. */
#include <immintrin.h>
#else
#define _CINRS_AVX512FINTRIN_H

typedef __cinrs_m512 __m512;
typedef __cinrs_m512i __m512i;
typedef __cinrs_m512d __m512d;
typedef unsigned char __mmask8;
typedef unsigned short __mmask16;
typedef int _MM_CMPINT_ENUM;
typedef int _MM_MANTISSA_NORM_ENUM;
typedef int _MM_MANTISSA_SIGN_ENUM;
typedef int _MM_PERM_ENUM;

/* @generated constants — see crates/cinrs-core/tests/x86_intrinsics.rs */
#define _MM_CMPINT_EQ 0x0000
#define _MM_CMPINT_LT 0x0001
#define _MM_CMPINT_LE 0x0002
#define _MM_CMPINT_FALSE 0x0003
#define _MM_CMPINT_NE 0x0004
#define _MM_CMPINT_NLT 0x0005
#define _MM_CMPINT_NLE 0x0006
#define _MM_CMPINT_TRUE 0x0007
#define _MM_MANT_NORM_1_2 0x0000
#define _MM_MANT_NORM_P5_2 0x0001
#define _MM_MANT_NORM_P5_1 0x0002
#define _MM_MANT_NORM_P75_1P5 0x0003
#define _MM_MANT_SIGN_SRC 0x0000
#define _MM_MANT_SIGN_ZERO 0x0001
#define _MM_MANT_SIGN_NAN 0x0002
#define _MM_PERM_AAAA 0x0000
#define _MM_PERM_AAAB 0x0001
#define _MM_PERM_AAAC 0x0002
#define _MM_PERM_AAAD 0x0003
#define _MM_PERM_AABA 0x0004
#define _MM_PERM_AABB 0x0005
#define _MM_PERM_AABC 0x0006
#define _MM_PERM_AABD 0x0007
#define _MM_PERM_AACA 0x0008
#define _MM_PERM_AACB 0x0009
#define _MM_PERM_AACC 0x000a
#define _MM_PERM_AACD 0x000b
#define _MM_PERM_AADA 0x000c
#define _MM_PERM_AADB 0x000d
#define _MM_PERM_AADC 0x000e
#define _MM_PERM_AADD 0x000f
#define _MM_PERM_ABAA 0x0010
#define _MM_PERM_ABAB 0x0011
#define _MM_PERM_ABAC 0x0012
#define _MM_PERM_ABAD 0x0013
#define _MM_PERM_ABBA 0x0014
#define _MM_PERM_ABBB 0x0015
#define _MM_PERM_ABBC 0x0016
#define _MM_PERM_ABBD 0x0017
#define _MM_PERM_ABCA 0x0018
#define _MM_PERM_ABCB 0x0019
#define _MM_PERM_ABCC 0x001a
#define _MM_PERM_ABCD 0x001b
#define _MM_PERM_ABDA 0x001c
#define _MM_PERM_ABDB 0x001d
#define _MM_PERM_ABDC 0x001e
#define _MM_PERM_ABDD 0x001f
#define _MM_PERM_ACAA 0x0020
#define _MM_PERM_ACAB 0x0021
#define _MM_PERM_ACAC 0x0022
#define _MM_PERM_ACAD 0x0023
#define _MM_PERM_ACBA 0x0024
#define _MM_PERM_ACBB 0x0025
#define _MM_PERM_ACBC 0x0026
#define _MM_PERM_ACBD 0x0027
#define _MM_PERM_ACCA 0x0028
#define _MM_PERM_ACCB 0x0029
#define _MM_PERM_ACCC 0x002a
#define _MM_PERM_ACCD 0x002b
#define _MM_PERM_ACDA 0x002c
#define _MM_PERM_ACDB 0x002d
#define _MM_PERM_ACDC 0x002e
#define _MM_PERM_ACDD 0x002f
#define _MM_PERM_ADAA 0x0030
#define _MM_PERM_ADAB 0x0031
#define _MM_PERM_ADAC 0x0032
#define _MM_PERM_ADAD 0x0033
#define _MM_PERM_ADBA 0x0034
#define _MM_PERM_ADBB 0x0035
#define _MM_PERM_ADBC 0x0036
#define _MM_PERM_ADBD 0x0037
#define _MM_PERM_ADCA 0x0038
#define _MM_PERM_ADCB 0x0039
#define _MM_PERM_ADCC 0x003a
#define _MM_PERM_ADCD 0x003b
#define _MM_PERM_ADDA 0x003c
#define _MM_PERM_ADDB 0x003d
#define _MM_PERM_ADDC 0x003e
#define _MM_PERM_ADDD 0x003f
#define _MM_PERM_BAAA 0x0040
#define _MM_PERM_BAAB 0x0041
#define _MM_PERM_BAAC 0x0042
#define _MM_PERM_BAAD 0x0043
#define _MM_PERM_BABA 0x0044
#define _MM_PERM_BABB 0x0045
#define _MM_PERM_BABC 0x0046
#define _MM_PERM_BABD 0x0047
#define _MM_PERM_BACA 0x0048
#define _MM_PERM_BACB 0x0049
#define _MM_PERM_BACC 0x004a
#define _MM_PERM_BACD 0x004b
#define _MM_PERM_BADA 0x004c
#define _MM_PERM_BADB 0x004d
#define _MM_PERM_BADC 0x004e
#define _MM_PERM_BADD 0x004f
#define _MM_PERM_BBAA 0x0050
#define _MM_PERM_BBAB 0x0051
#define _MM_PERM_BBAC 0x0052
#define _MM_PERM_BBAD 0x0053
#define _MM_PERM_BBBA 0x0054
#define _MM_PERM_BBBB 0x0055
#define _MM_PERM_BBBC 0x0056
#define _MM_PERM_BBBD 0x0057
#define _MM_PERM_BBCA 0x0058
#define _MM_PERM_BBCB 0x0059
#define _MM_PERM_BBCC 0x005a
#define _MM_PERM_BBCD 0x005b
#define _MM_PERM_BBDA 0x005c
#define _MM_PERM_BBDB 0x005d
#define _MM_PERM_BBDC 0x005e
#define _MM_PERM_BBDD 0x005f
#define _MM_PERM_BCAA 0x0060
#define _MM_PERM_BCAB 0x0061
#define _MM_PERM_BCAC 0x0062
#define _MM_PERM_BCAD 0x0063
#define _MM_PERM_BCBA 0x0064
#define _MM_PERM_BCBB 0x0065
#define _MM_PERM_BCBC 0x0066
#define _MM_PERM_BCBD 0x0067
#define _MM_PERM_BCCA 0x0068
#define _MM_PERM_BCCB 0x0069
#define _MM_PERM_BCCC 0x006a
#define _MM_PERM_BCCD 0x006b
#define _MM_PERM_BCDA 0x006c
#define _MM_PERM_BCDB 0x006d
#define _MM_PERM_BCDC 0x006e
#define _MM_PERM_BCDD 0x006f
#define _MM_PERM_BDAA 0x0070
#define _MM_PERM_BDAB 0x0071
#define _MM_PERM_BDAC 0x0072
#define _MM_PERM_BDAD 0x0073
#define _MM_PERM_BDBA 0x0074
#define _MM_PERM_BDBB 0x0075
#define _MM_PERM_BDBC 0x0076
#define _MM_PERM_BDBD 0x0077
#define _MM_PERM_BDCA 0x0078
#define _MM_PERM_BDCB 0x0079
#define _MM_PERM_BDCC 0x007a
#define _MM_PERM_BDCD 0x007b
#define _MM_PERM_BDDA 0x007c
#define _MM_PERM_BDDB 0x007d
#define _MM_PERM_BDDC 0x007e
#define _MM_PERM_BDDD 0x007f
#define _MM_PERM_CAAA 0x0080
#define _MM_PERM_CAAB 0x0081
#define _MM_PERM_CAAC 0x0082
#define _MM_PERM_CAAD 0x0083
#define _MM_PERM_CABA 0x0084
#define _MM_PERM_CABB 0x0085
#define _MM_PERM_CABC 0x0086
#define _MM_PERM_CABD 0x0087
#define _MM_PERM_CACA 0x0088
#define _MM_PERM_CACB 0x0089
#define _MM_PERM_CACC 0x008a
#define _MM_PERM_CACD 0x008b
#define _MM_PERM_CADA 0x008c
#define _MM_PERM_CADB 0x008d
#define _MM_PERM_CADC 0x008e
#define _MM_PERM_CADD 0x008f
#define _MM_PERM_CBAA 0x0090
#define _MM_PERM_CBAB 0x0091
#define _MM_PERM_CBAC 0x0092
#define _MM_PERM_CBAD 0x0093
#define _MM_PERM_CBBA 0x0094
#define _MM_PERM_CBBB 0x0095
#define _MM_PERM_CBBC 0x0096
#define _MM_PERM_CBBD 0x0097
#define _MM_PERM_CBCA 0x0098
#define _MM_PERM_CBCB 0x0099
#define _MM_PERM_CBCC 0x009a
#define _MM_PERM_CBCD 0x009b
#define _MM_PERM_CBDA 0x009c
#define _MM_PERM_CBDB 0x009d
#define _MM_PERM_CBDC 0x009e
#define _MM_PERM_CBDD 0x009f
#define _MM_PERM_CCAA 0x00a0
#define _MM_PERM_CCAB 0x00a1
#define _MM_PERM_CCAC 0x00a2
#define _MM_PERM_CCAD 0x00a3
#define _MM_PERM_CCBA 0x00a4
#define _MM_PERM_CCBB 0x00a5
#define _MM_PERM_CCBC 0x00a6
#define _MM_PERM_CCBD 0x00a7
#define _MM_PERM_CCCA 0x00a8
#define _MM_PERM_CCCB 0x00a9
#define _MM_PERM_CCCC 0x00aa
#define _MM_PERM_CCCD 0x00ab
#define _MM_PERM_CCDA 0x00ac
#define _MM_PERM_CCDB 0x00ad
#define _MM_PERM_CCDC 0x00ae
#define _MM_PERM_CCDD 0x00af
#define _MM_PERM_CDAA 0x00b0
#define _MM_PERM_CDAB 0x00b1
#define _MM_PERM_CDAC 0x00b2
#define _MM_PERM_CDAD 0x00b3
#define _MM_PERM_CDBA 0x00b4
#define _MM_PERM_CDBB 0x00b5
#define _MM_PERM_CDBC 0x00b6
#define _MM_PERM_CDBD 0x00b7
#define _MM_PERM_CDCA 0x00b8
#define _MM_PERM_CDCB 0x00b9
#define _MM_PERM_CDCC 0x00ba
#define _MM_PERM_CDCD 0x00bb
#define _MM_PERM_CDDA 0x00bc
#define _MM_PERM_CDDB 0x00bd
#define _MM_PERM_CDDC 0x00be
#define _MM_PERM_CDDD 0x00bf
#define _MM_PERM_DAAA 0x00c0
#define _MM_PERM_DAAB 0x00c1
#define _MM_PERM_DAAC 0x00c2
#define _MM_PERM_DAAD 0x00c3
#define _MM_PERM_DABA 0x00c4
#define _MM_PERM_DABB 0x00c5
#define _MM_PERM_DABC 0x00c6
#define _MM_PERM_DABD 0x00c7
#define _MM_PERM_DACA 0x00c8
#define _MM_PERM_DACB 0x00c9
#define _MM_PERM_DACC 0x00ca
#define _MM_PERM_DACD 0x00cb
#define _MM_PERM_DADA 0x00cc
#define _MM_PERM_DADB 0x00cd
#define _MM_PERM_DADC 0x00ce
#define _MM_PERM_DADD 0x00cf
#define _MM_PERM_DBAA 0x00d0
#define _MM_PERM_DBAB 0x00d1
#define _MM_PERM_DBAC 0x00d2
#define _MM_PERM_DBAD 0x00d3
#define _MM_PERM_DBBA 0x00d4
#define _MM_PERM_DBBB 0x00d5
#define _MM_PERM_DBBC 0x00d6
#define _MM_PERM_DBBD 0x00d7
#define _MM_PERM_DBCA 0x00d8
#define _MM_PERM_DBCB 0x00d9
#define _MM_PERM_DBCC 0x00da
#define _MM_PERM_DBCD 0x00db
#define _MM_PERM_DBDA 0x00dc
#define _MM_PERM_DBDB 0x00dd
#define _MM_PERM_DBDC 0x00de
#define _MM_PERM_DBDD 0x00df
#define _MM_PERM_DCAA 0x00e0
#define _MM_PERM_DCAB 0x00e1
#define _MM_PERM_DCAC 0x00e2
#define _MM_PERM_DCAD 0x00e3
#define _MM_PERM_DCBA 0x00e4
#define _MM_PERM_DCBB 0x00e5
#define _MM_PERM_DCBC 0x00e6
#define _MM_PERM_DCBD 0x00e7
#define _MM_PERM_DCCA 0x00e8
#define _MM_PERM_DCCB 0x00e9
#define _MM_PERM_DCCC 0x00ea
#define _MM_PERM_DCCD 0x00eb
#define _MM_PERM_DCDA 0x00ec
#define _MM_PERM_DCDB 0x00ed
#define _MM_PERM_DCDC 0x00ee
#define _MM_PERM_DCDD 0x00ef
#define _MM_PERM_DDAA 0x00f0
#define _MM_PERM_DDAB 0x00f1
#define _MM_PERM_DDAC 0x00f2
#define _MM_PERM_DDAD 0x00f3
#define _MM_PERM_DDBA 0x00f4
#define _MM_PERM_DDBB 0x00f5
#define _MM_PERM_DDBC 0x00f6
#define _MM_PERM_DDBD 0x00f7
#define _MM_PERM_DDCA 0x00f8
#define _MM_PERM_DDCB 0x00f9
#define _MM_PERM_DDCC 0x00fa
#define _MM_PERM_DDCD 0x00fb
#define _MM_PERM_DDDA 0x00fc
#define _MM_PERM_DDDB 0x00fd
#define _MM_PERM_DDDC 0x00fe
#define _MM_PERM_DDDD 0x00ff
/* @generated end */

/* @generated avx512f — see crates/cinrs-core/tests/x86_intrinsics.rs */
unsigned int _cvtmask16_u32(__mmask16);
__mmask16 _cvtu32_mask16(unsigned int);
__mmask16 _kand_mask16(__mmask16, __mmask16);
__mmask16 _kandn_mask16(__mmask16, __mmask16);
__mmask16 _knot_mask16(__mmask16);
__mmask16 _kor_mask16(__mmask16, __mmask16);
unsigned char _kortest_mask16_u8(__mmask16, __mmask16, unsigned char *);
unsigned char _kortestc_mask16_u8(__mmask16, __mmask16);
unsigned char _kortestz_mask16_u8(__mmask16, __mmask16);
__mmask16 _kshiftli_mask16(__mmask16, const unsigned int);
__mmask16 _kshiftri_mask16(__mmask16, const unsigned int);
__mmask16 _kxnor_mask16(__mmask16, __mmask16);
__mmask16 _kxor_mask16(__mmask16, __mmask16);
__mmask16 _load_mask16(const __mmask16 *);
__m512i _mm512_abs_epi32(__m512i);
__m512i _mm512_abs_epi64(__m512i);
__m512d _mm512_abs_pd(__m512d);
__m512 _mm512_abs_ps(__m512);
__m512i _mm512_add_epi32(__m512i, __m512i);
__m512i _mm512_add_epi64(__m512i, __m512i);
__m512d _mm512_add_pd(__m512d, __m512d);
__m512 _mm512_add_ps(__m512, __m512);
__m512d _mm512_add_round_pd(__m512d, __m512d, const int);
__m512 _mm512_add_round_ps(__m512, __m512, const int);
__m512i _mm512_alignr_epi32(__m512i, __m512i, const int);
__m512i _mm512_alignr_epi64(__m512i, __m512i, const int);
__m512i _mm512_and_epi32(__m512i, __m512i);
__m512i _mm512_and_epi64(__m512i, __m512i);
__m512i _mm512_and_si512(__m512i, __m512i);
__m512i _mm512_andnot_epi32(__m512i, __m512i);
__m512i _mm512_andnot_epi64(__m512i, __m512i);
__m512i _mm512_andnot_si512(__m512i, __m512i);
__m512 _mm512_broadcast_f32x4(__m128);
__m512d _mm512_broadcast_f64x4(__m256d);
__m512i _mm512_broadcast_i32x4(__m128i);
__m512i _mm512_broadcast_i64x4(__m256i);
__m512i _mm512_broadcastd_epi32(__m128i);
__m512i _mm512_broadcastq_epi64(__m128i);
__m512d _mm512_broadcastsd_pd(__m128d);
__m512 _mm512_broadcastss_ps(__m128);
__m512d _mm512_castpd128_pd512(__m128d);
__m512d _mm512_castpd256_pd512(__m256d);
__m128d _mm512_castpd512_pd128(__m512d);
__m256d _mm512_castpd512_pd256(__m512d);
__m512 _mm512_castpd_ps(__m512d);
__m512i _mm512_castpd_si512(__m512d);
__m512 _mm512_castps128_ps512(__m128);
__m512 _mm512_castps256_ps512(__m256);
__m128 _mm512_castps512_ps128(__m512);
__m256 _mm512_castps512_ps256(__m512);
__m512d _mm512_castps_pd(__m512);
__m512i _mm512_castps_si512(__m512);
__m512i _mm512_castsi128_si512(__m128i);
__m512i _mm512_castsi256_si512(__m256i);
__m512d _mm512_castsi512_pd(__m512i);
__m512 _mm512_castsi512_ps(__m512i);
__m128i _mm512_castsi512_si128(__m512i);
__m256i _mm512_castsi512_si256(__m512i);
__mmask16 _mm512_cmp_epi32_mask(__m512i, __m512i, const int);
__mmask8 _mm512_cmp_epi64_mask(__m512i, __m512i, const int);
__mmask16 _mm512_cmp_epu32_mask(__m512i, __m512i, const int);
__mmask8 _mm512_cmp_epu64_mask(__m512i, __m512i, const int);
__mmask8 _mm512_cmp_pd_mask(__m512d, __m512d, const int);
__mmask16 _mm512_cmp_ps_mask(__m512, __m512, const int);
__mmask8 _mm512_cmp_round_pd_mask(__m512d, __m512d, const int, const int);
__mmask16 _mm512_cmp_round_ps_mask(__m512, __m512, const int, const int);
__mmask16 _mm512_cmpeq_epi32_mask(__m512i, __m512i);
__mmask8 _mm512_cmpeq_epi64_mask(__m512i, __m512i);
__mmask16 _mm512_cmpeq_epu32_mask(__m512i, __m512i);
__mmask8 _mm512_cmpeq_epu64_mask(__m512i, __m512i);
__mmask8 _mm512_cmpeq_pd_mask(__m512d, __m512d);
__mmask16 _mm512_cmpeq_ps_mask(__m512, __m512);
__mmask16 _mm512_cmpge_epi32_mask(__m512i, __m512i);
__mmask8 _mm512_cmpge_epi64_mask(__m512i, __m512i);
__mmask16 _mm512_cmpge_epu32_mask(__m512i, __m512i);
__mmask8 _mm512_cmpge_epu64_mask(__m512i, __m512i);
__mmask16 _mm512_cmpgt_epi32_mask(__m512i, __m512i);
__mmask8 _mm512_cmpgt_epi64_mask(__m512i, __m512i);
__mmask16 _mm512_cmpgt_epu32_mask(__m512i, __m512i);
__mmask8 _mm512_cmpgt_epu64_mask(__m512i, __m512i);
__mmask16 _mm512_cmple_epi32_mask(__m512i, __m512i);
__mmask8 _mm512_cmple_epi64_mask(__m512i, __m512i);
__mmask16 _mm512_cmple_epu32_mask(__m512i, __m512i);
__mmask8 _mm512_cmple_epu64_mask(__m512i, __m512i);
__mmask8 _mm512_cmple_pd_mask(__m512d, __m512d);
__mmask16 _mm512_cmple_ps_mask(__m512, __m512);
__mmask16 _mm512_cmplt_epi32_mask(__m512i, __m512i);
__mmask8 _mm512_cmplt_epi64_mask(__m512i, __m512i);
__mmask16 _mm512_cmplt_epu32_mask(__m512i, __m512i);
__mmask8 _mm512_cmplt_epu64_mask(__m512i, __m512i);
__mmask8 _mm512_cmplt_pd_mask(__m512d, __m512d);
__mmask16 _mm512_cmplt_ps_mask(__m512, __m512);
__mmask16 _mm512_cmpneq_epi32_mask(__m512i, __m512i);
__mmask8 _mm512_cmpneq_epi64_mask(__m512i, __m512i);
__mmask16 _mm512_cmpneq_epu32_mask(__m512i, __m512i);
__mmask8 _mm512_cmpneq_epu64_mask(__m512i, __m512i);
__mmask8 _mm512_cmpneq_pd_mask(__m512d, __m512d);
__mmask16 _mm512_cmpneq_ps_mask(__m512, __m512);
__mmask8 _mm512_cmpnle_pd_mask(__m512d, __m512d);
__mmask16 _mm512_cmpnle_ps_mask(__m512, __m512);
__mmask8 _mm512_cmpnlt_pd_mask(__m512d, __m512d);
__mmask16 _mm512_cmpnlt_ps_mask(__m512, __m512);
__mmask8 _mm512_cmpord_pd_mask(__m512d, __m512d);
__mmask16 _mm512_cmpord_ps_mask(__m512, __m512);
__mmask8 _mm512_cmpunord_pd_mask(__m512d, __m512d);
__mmask16 _mm512_cmpunord_ps_mask(__m512, __m512);
__m512 _mm512_cvt_roundepi32_ps(__m512i, const int);
__m512 _mm512_cvt_roundepu32_ps(__m512i, const int);
__m256i _mm512_cvt_roundpd_epi32(__m512d, const int);
__m256i _mm512_cvt_roundpd_epu32(__m512d, const int);
__m256 _mm512_cvt_roundpd_ps(__m512d, const int);
__m512 _mm512_cvt_roundph_ps(__m256i, const int);
__m512i _mm512_cvt_roundps_epi32(__m512, const int);
__m512i _mm512_cvt_roundps_epu32(__m512, const int);
__m512d _mm512_cvt_roundps_pd(__m256, const int);
__m256i _mm512_cvt_roundps_ph(__m512, const int);
__m512i _mm512_cvtepi16_epi32(__m256i);
__m512i _mm512_cvtepi16_epi64(__m128i);
__m256i _mm512_cvtepi32_epi16(__m512i);
__m512i _mm512_cvtepi32_epi64(__m256i);
__m128i _mm512_cvtepi32_epi8(__m512i);
__m512d _mm512_cvtepi32_pd(__m256i);
__m512 _mm512_cvtepi32_ps(__m512i);
__m512d _mm512_cvtepi32lo_pd(__m512i);
__m128i _mm512_cvtepi64_epi16(__m512i);
__m256i _mm512_cvtepi64_epi32(__m512i);
__m128i _mm512_cvtepi64_epi8(__m512i);
__m512i _mm512_cvtepi8_epi32(__m128i);
__m512i _mm512_cvtepi8_epi64(__m128i);
__m512i _mm512_cvtepu16_epi32(__m256i);
__m512i _mm512_cvtepu16_epi64(__m128i);
__m512i _mm512_cvtepu32_epi64(__m256i);
__m512d _mm512_cvtepu32_pd(__m256i);
__m512 _mm512_cvtepu32_ps(__m512i);
__m512d _mm512_cvtepu32lo_pd(__m512i);
__m512i _mm512_cvtepu8_epi32(__m128i);
__m512i _mm512_cvtepu8_epi64(__m128i);
__m256i _mm512_cvtpd_epi32(__m512d);
__m256i _mm512_cvtpd_epu32(__m512d);
__m256 _mm512_cvtpd_ps(__m512d);
__m512 _mm512_cvtpd_pslo(__m512d);
__m512 _mm512_cvtph_ps(__m256i);
__m512i _mm512_cvtps_epi32(__m512);
__m512i _mm512_cvtps_epu32(__m512);
__m512d _mm512_cvtps_pd(__m256);
__m256i _mm512_cvtps_ph(__m512, const int);
__m512d _mm512_cvtpslo_pd(__m512);
double _mm512_cvtsd_f64(__m512d);
__m256i _mm512_cvtsepi32_epi16(__m512i);
__m128i _mm512_cvtsepi32_epi8(__m512i);
__m128i _mm512_cvtsepi64_epi16(__m512i);
__m256i _mm512_cvtsepi64_epi32(__m512i);
__m128i _mm512_cvtsepi64_epi8(__m512i);
int _mm512_cvtsi512_si32(__m512i);
float _mm512_cvtss_f32(__m512);
__m256i _mm512_cvtt_roundpd_epi32(__m512d, const int);
__m256i _mm512_cvtt_roundpd_epu32(__m512d, const int);
__m512i _mm512_cvtt_roundps_epi32(__m512, const int);
__m512i _mm512_cvtt_roundps_epu32(__m512, const int);
__m256i _mm512_cvttpd_epi32(__m512d);
__m256i _mm512_cvttpd_epu32(__m512d);
__m512i _mm512_cvttps_epi32(__m512);
__m512i _mm512_cvttps_epu32(__m512);
__m256i _mm512_cvtusepi32_epi16(__m512i);
__m128i _mm512_cvtusepi32_epi8(__m512i);
__m128i _mm512_cvtusepi64_epi16(__m512i);
__m256i _mm512_cvtusepi64_epi32(__m512i);
__m128i _mm512_cvtusepi64_epi8(__m512i);
__m512d _mm512_div_pd(__m512d, __m512d);
__m512 _mm512_div_ps(__m512, __m512);
__m512d _mm512_div_round_pd(__m512d, __m512d, const int);
__m512 _mm512_div_round_ps(__m512, __m512, const int);
__m128 _mm512_extractf32x4_ps(__m512, const int);
__m256d _mm512_extractf64x4_pd(__m512d, const int);
__m128i _mm512_extracti32x4_epi32(__m512i, const int);
__m256i _mm512_extracti64x4_epi64(__m512i, const int);
__m512d _mm512_fixupimm_pd(__m512d, __m512d, __m512i, const int);
__m512 _mm512_fixupimm_ps(__m512, __m512, __m512i, const int);
__m512d _mm512_fixupimm_round_pd(__m512d, __m512d, __m512i, const int, const int);
__m512 _mm512_fixupimm_round_ps(__m512, __m512, __m512i, const int, const int);
__m512d _mm512_fmadd_pd(__m512d, __m512d, __m512d);
__m512 _mm512_fmadd_ps(__m512, __m512, __m512);
__m512d _mm512_fmadd_round_pd(__m512d, __m512d, __m512d, const int);
__m512 _mm512_fmadd_round_ps(__m512, __m512, __m512, const int);
__m512d _mm512_fmaddsub_pd(__m512d, __m512d, __m512d);
__m512 _mm512_fmaddsub_ps(__m512, __m512, __m512);
__m512d _mm512_fmaddsub_round_pd(__m512d, __m512d, __m512d, const int);
__m512 _mm512_fmaddsub_round_ps(__m512, __m512, __m512, const int);
__m512d _mm512_fmsub_pd(__m512d, __m512d, __m512d);
__m512 _mm512_fmsub_ps(__m512, __m512, __m512);
__m512d _mm512_fmsub_round_pd(__m512d, __m512d, __m512d, const int);
__m512 _mm512_fmsub_round_ps(__m512, __m512, __m512, const int);
__m512d _mm512_fmsubadd_pd(__m512d, __m512d, __m512d);
__m512 _mm512_fmsubadd_ps(__m512, __m512, __m512);
__m512d _mm512_fmsubadd_round_pd(__m512d, __m512d, __m512d, const int);
__m512 _mm512_fmsubadd_round_ps(__m512, __m512, __m512, const int);
__m512d _mm512_fnmadd_pd(__m512d, __m512d, __m512d);
__m512 _mm512_fnmadd_ps(__m512, __m512, __m512);
__m512d _mm512_fnmadd_round_pd(__m512d, __m512d, __m512d, const int);
__m512 _mm512_fnmadd_round_ps(__m512, __m512, __m512, const int);
__m512d _mm512_fnmsub_pd(__m512d, __m512d, __m512d);
__m512 _mm512_fnmsub_ps(__m512, __m512, __m512);
__m512d _mm512_fnmsub_round_pd(__m512d, __m512d, __m512d, const int);
__m512 _mm512_fnmsub_round_ps(__m512, __m512, __m512, const int);
__m512d _mm512_getexp_pd(__m512d);
__m512 _mm512_getexp_ps(__m512);
__m512d _mm512_getexp_round_pd(__m512d, const int);
__m512 _mm512_getexp_round_ps(__m512, const int);
__m512d _mm512_getmant_pd(__m512d, const int, const int);
__m512 _mm512_getmant_ps(__m512, const int, const int);
__m512d _mm512_getmant_round_pd(__m512d, const int, const int, const int);
__m512 _mm512_getmant_round_ps(__m512, const int, const int, const int);
__m512i _mm512_i32gather_epi32(__m512i, const int *, const int);
__m512i _mm512_i32gather_epi64(__m256i, const long long *, const int);
__m512d _mm512_i32gather_pd(__m256i, const double *, const int);
__m512 _mm512_i32gather_ps(__m512i, const float *, const int);
__m512i _mm512_i32logather_epi64(__m512i, const long long *, const int);
__m512d _mm512_i32logather_pd(__m512i, const double *, const int);
void _mm512_i32loscatter_epi64(long long *, __m512i, __m512i, const int);
void _mm512_i32loscatter_pd(double *, __m512i, __m512d, const int);
void _mm512_i32scatter_epi32(int *, __m512i, __m512i, const int);
void _mm512_i32scatter_epi64(long long *, __m256i, __m512i, const int);
void _mm512_i32scatter_pd(double *, __m256i, __m512d, const int);
void _mm512_i32scatter_ps(float *, __m512i, __m512, const int);
__m256i _mm512_i64gather_epi32(__m512i, const int *, const int);
__m512i _mm512_i64gather_epi64(__m512i, const long long *, const int);
__m512d _mm512_i64gather_pd(__m512i, const double *, const int);
__m256 _mm512_i64gather_ps(__m512i, const float *, const int);
void _mm512_i64scatter_epi32(int *, __m512i, __m256i, const int);
void _mm512_i64scatter_epi64(long long *, __m512i, __m512i, const int);
void _mm512_i64scatter_pd(double *, __m512i, __m512d, const int);
void _mm512_i64scatter_ps(float *, __m512i, __m256, const int);
__m512 _mm512_insertf32x4(__m512, __m128, const int);
__m512d _mm512_insertf64x4(__m512d, __m256d, const int);
__m512i _mm512_inserti32x4(__m512i, __m128i, const int);
__m512i _mm512_inserti64x4(__m512i, __m256i, const int);
__mmask16 _mm512_int2mask(int);
__mmask16 _mm512_kand(__mmask16, __mmask16);
__mmask16 _mm512_kandn(__mmask16, __mmask16);
__mmask16 _mm512_kmov(__mmask16);
__mmask16 _mm512_knot(__mmask16);
__mmask16 _mm512_kor(__mmask16, __mmask16);
int _mm512_kortestc(__mmask16, __mmask16);
int _mm512_kortestz(__mmask16, __mmask16);
__mmask16 _mm512_kunpackb(__mmask16, __mmask16);
__mmask16 _mm512_kxnor(__mmask16, __mmask16);
__mmask16 _mm512_kxor(__mmask16, __mmask16);
__m512i _mm512_load_epi32(const int *);
__m512i _mm512_load_epi64(const long long *);
__m512d _mm512_load_pd(const double *);
__m512 _mm512_load_ps(const float *);
__m512i _mm512_load_si512(const __m512i *);
__m512i _mm512_loadu_epi32(const int *);
__m512i _mm512_loadu_epi64(const long long *);
__m512d _mm512_loadu_pd(const double *);
__m512 _mm512_loadu_ps(const float *);
__m512i _mm512_loadu_si512(const __m512i *);
__m512i _mm512_mask2_permutex2var_epi32(__m512i, __m512i, __mmask16, __m512i);
__m512i _mm512_mask2_permutex2var_epi64(__m512i, __m512i, __mmask8, __m512i);
__m512d _mm512_mask2_permutex2var_pd(__m512d, __m512i, __mmask8, __m512d);
__m512 _mm512_mask2_permutex2var_ps(__m512, __m512i, __mmask16, __m512);
int _mm512_mask2int(__mmask16);
__m512d _mm512_mask3_fmadd_pd(__m512d, __m512d, __m512d, __mmask8);
__m512 _mm512_mask3_fmadd_ps(__m512, __m512, __m512, __mmask16);
__m512d _mm512_mask3_fmadd_round_pd(__m512d, __m512d, __m512d, __mmask8, const int);
__m512 _mm512_mask3_fmadd_round_ps(__m512, __m512, __m512, __mmask16, const int);
__m512d _mm512_mask3_fmaddsub_pd(__m512d, __m512d, __m512d, __mmask8);
__m512 _mm512_mask3_fmaddsub_ps(__m512, __m512, __m512, __mmask16);
__m512d _mm512_mask3_fmaddsub_round_pd(__m512d, __m512d, __m512d, __mmask8, const int);
__m512 _mm512_mask3_fmaddsub_round_ps(__m512, __m512, __m512, __mmask16, const int);
__m512d _mm512_mask3_fmsub_pd(__m512d, __m512d, __m512d, __mmask8);
__m512 _mm512_mask3_fmsub_ps(__m512, __m512, __m512, __mmask16);
__m512d _mm512_mask3_fmsub_round_pd(__m512d, __m512d, __m512d, __mmask8, const int);
__m512 _mm512_mask3_fmsub_round_ps(__m512, __m512, __m512, __mmask16, const int);
__m512d _mm512_mask3_fmsubadd_pd(__m512d, __m512d, __m512d, __mmask8);
__m512 _mm512_mask3_fmsubadd_ps(__m512, __m512, __m512, __mmask16);
__m512d _mm512_mask3_fmsubadd_round_pd(__m512d, __m512d, __m512d, __mmask8, const int);
__m512 _mm512_mask3_fmsubadd_round_ps(__m512, __m512, __m512, __mmask16, const int);
__m512d _mm512_mask3_fnmadd_pd(__m512d, __m512d, __m512d, __mmask8);
__m512 _mm512_mask3_fnmadd_ps(__m512, __m512, __m512, __mmask16);
__m512d _mm512_mask3_fnmadd_round_pd(__m512d, __m512d, __m512d, __mmask8, const int);
__m512 _mm512_mask3_fnmadd_round_ps(__m512, __m512, __m512, __mmask16, const int);
__m512d _mm512_mask3_fnmsub_pd(__m512d, __m512d, __m512d, __mmask8);
__m512 _mm512_mask3_fnmsub_ps(__m512, __m512, __m512, __mmask16);
__m512d _mm512_mask3_fnmsub_round_pd(__m512d, __m512d, __m512d, __mmask8, const int);
__m512 _mm512_mask3_fnmsub_round_ps(__m512, __m512, __m512, __mmask16, const int);
__m512i _mm512_mask_abs_epi32(__m512i, __mmask16, __m512i);
__m512i _mm512_mask_abs_epi64(__m512i, __mmask8, __m512i);
__m512d _mm512_mask_abs_pd(__m512d, __mmask8, __m512d);
__m512 _mm512_mask_abs_ps(__m512, __mmask16, __m512);
__m512i _mm512_mask_add_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_add_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512d _mm512_mask_add_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_add_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_add_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_add_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512i _mm512_mask_alignr_epi32(__m512i, __mmask16, __m512i, __m512i, const int);
__m512i _mm512_mask_alignr_epi64(__m512i, __mmask8, __m512i, __m512i, const int);
__m512i _mm512_mask_and_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_and_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512i _mm512_mask_andnot_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_andnot_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512i _mm512_mask_blend_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_mask_blend_epi64(__mmask8, __m512i, __m512i);
__m512d _mm512_mask_blend_pd(__mmask8, __m512d, __m512d);
__m512 _mm512_mask_blend_ps(__mmask16, __m512, __m512);
__m512 _mm512_mask_broadcast_f32x4(__m512, __mmask16, __m128);
__m512d _mm512_mask_broadcast_f64x4(__m512d, __mmask8, __m256d);
__m512i _mm512_mask_broadcast_i32x4(__m512i, __mmask16, __m128i);
__m512i _mm512_mask_broadcast_i64x4(__m512i, __mmask8, __m256i);
__m512i _mm512_mask_broadcastd_epi32(__m512i, __mmask16, __m128i);
__m512i _mm512_mask_broadcastq_epi64(__m512i, __mmask8, __m128i);
__m512d _mm512_mask_broadcastsd_pd(__m512d, __mmask8, __m128d);
__m512 _mm512_mask_broadcastss_ps(__m512, __mmask16, __m128);
__mmask16 _mm512_mask_cmp_epi32_mask(__mmask16, __m512i, __m512i, const int);
__mmask8 _mm512_mask_cmp_epi64_mask(__mmask8, __m512i, __m512i, const int);
__mmask16 _mm512_mask_cmp_epu32_mask(__mmask16, __m512i, __m512i, const int);
__mmask8 _mm512_mask_cmp_epu64_mask(__mmask8, __m512i, __m512i, const int);
__mmask8 _mm512_mask_cmp_pd_mask(__mmask8, __m512d, __m512d, const int);
__mmask16 _mm512_mask_cmp_ps_mask(__mmask16, __m512, __m512, const int);
__mmask8 _mm512_mask_cmp_round_pd_mask(__mmask8, __m512d, __m512d, const int, const int);
__mmask16 _mm512_mask_cmp_round_ps_mask(__mmask16, __m512, __m512, const int, const int);
__mmask16 _mm512_mask_cmpeq_epi32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_cmpeq_epi64_mask(__mmask8, __m512i, __m512i);
__mmask16 _mm512_mask_cmpeq_epu32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_cmpeq_epu64_mask(__mmask8, __m512i, __m512i);
__mmask8 _mm512_mask_cmpeq_pd_mask(__mmask8, __m512d, __m512d);
__mmask16 _mm512_mask_cmpeq_ps_mask(__mmask16, __m512, __m512);
__mmask16 _mm512_mask_cmpge_epi32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_cmpge_epi64_mask(__mmask8, __m512i, __m512i);
__mmask16 _mm512_mask_cmpge_epu32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_cmpge_epu64_mask(__mmask8, __m512i, __m512i);
__mmask16 _mm512_mask_cmpgt_epi32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_cmpgt_epi64_mask(__mmask8, __m512i, __m512i);
__mmask16 _mm512_mask_cmpgt_epu32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_cmpgt_epu64_mask(__mmask8, __m512i, __m512i);
__mmask16 _mm512_mask_cmple_epi32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_cmple_epi64_mask(__mmask8, __m512i, __m512i);
__mmask16 _mm512_mask_cmple_epu32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_cmple_epu64_mask(__mmask8, __m512i, __m512i);
__mmask8 _mm512_mask_cmple_pd_mask(__mmask8, __m512d, __m512d);
__mmask16 _mm512_mask_cmple_ps_mask(__mmask16, __m512, __m512);
__mmask16 _mm512_mask_cmplt_epi32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_cmplt_epi64_mask(__mmask8, __m512i, __m512i);
__mmask16 _mm512_mask_cmplt_epu32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_cmplt_epu64_mask(__mmask8, __m512i, __m512i);
__mmask8 _mm512_mask_cmplt_pd_mask(__mmask8, __m512d, __m512d);
__mmask16 _mm512_mask_cmplt_ps_mask(__mmask16, __m512, __m512);
__mmask16 _mm512_mask_cmpneq_epi32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_cmpneq_epi64_mask(__mmask8, __m512i, __m512i);
__mmask16 _mm512_mask_cmpneq_epu32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_cmpneq_epu64_mask(__mmask8, __m512i, __m512i);
__mmask8 _mm512_mask_cmpneq_pd_mask(__mmask8, __m512d, __m512d);
__mmask16 _mm512_mask_cmpneq_ps_mask(__mmask16, __m512, __m512);
__mmask8 _mm512_mask_cmpnle_pd_mask(__mmask8, __m512d, __m512d);
__mmask16 _mm512_mask_cmpnle_ps_mask(__mmask16, __m512, __m512);
__mmask8 _mm512_mask_cmpnlt_pd_mask(__mmask8, __m512d, __m512d);
__mmask16 _mm512_mask_cmpnlt_ps_mask(__mmask16, __m512, __m512);
__mmask8 _mm512_mask_cmpord_pd_mask(__mmask8, __m512d, __m512d);
__mmask16 _mm512_mask_cmpord_ps_mask(__mmask16, __m512, __m512);
__mmask8 _mm512_mask_cmpunord_pd_mask(__mmask8, __m512d, __m512d);
__mmask16 _mm512_mask_cmpunord_ps_mask(__mmask16, __m512, __m512);
__m512i _mm512_mask_compress_epi32(__m512i, __mmask16, __m512i);
__m512i _mm512_mask_compress_epi64(__m512i, __mmask8, __m512i);
__m512d _mm512_mask_compress_pd(__m512d, __mmask8, __m512d);
__m512 _mm512_mask_compress_ps(__m512, __mmask16, __m512);
void _mm512_mask_compressstoreu_epi32(int *, __mmask16, __m512i);
void _mm512_mask_compressstoreu_epi64(long long *, __mmask8, __m512i);
void _mm512_mask_compressstoreu_pd(double *, __mmask8, __m512d);
void _mm512_mask_compressstoreu_ps(float *, __mmask16, __m512);
__m512 _mm512_mask_cvt_roundepi32_ps(__m512, __mmask16, __m512i, const int);
__m512 _mm512_mask_cvt_roundepu32_ps(__m512, __mmask16, __m512i, const int);
__m256i _mm512_mask_cvt_roundpd_epi32(__m256i, __mmask8, __m512d, const int);
__m256i _mm512_mask_cvt_roundpd_epu32(__m256i, __mmask8, __m512d, const int);
__m256 _mm512_mask_cvt_roundpd_ps(__m256, __mmask8, __m512d, const int);
__m512 _mm512_mask_cvt_roundph_ps(__m512, __mmask16, __m256i, const int);
__m512i _mm512_mask_cvt_roundps_epi32(__m512i, __mmask16, __m512, const int);
__m512i _mm512_mask_cvt_roundps_epu32(__m512i, __mmask16, __m512, const int);
__m512d _mm512_mask_cvt_roundps_pd(__m512d, __mmask8, __m256, const int);
__m256i _mm512_mask_cvt_roundps_ph(__m256i, __mmask16, __m512, const int);
__m512i _mm512_mask_cvtepi16_epi32(__m512i, __mmask16, __m256i);
__m512i _mm512_mask_cvtepi16_epi64(__m512i, __mmask8, __m128i);
__m256i _mm512_mask_cvtepi32_epi16(__m256i, __mmask16, __m512i);
__m512i _mm512_mask_cvtepi32_epi64(__m512i, __mmask8, __m256i);
__m128i _mm512_mask_cvtepi32_epi8(__m128i, __mmask16, __m512i);
__m512d _mm512_mask_cvtepi32_pd(__m512d, __mmask8, __m256i);
__m512 _mm512_mask_cvtepi32_ps(__m512, __mmask16, __m512i);
void _mm512_mask_cvtepi32_storeu_epi16(short *, __mmask16, __m512i);
void _mm512_mask_cvtepi32_storeu_epi8(char *, __mmask16, __m512i);
__m512d _mm512_mask_cvtepi32lo_pd(__m512d, __mmask8, __m512i);
__m128i _mm512_mask_cvtepi64_epi16(__m128i, __mmask8, __m512i);
__m256i _mm512_mask_cvtepi64_epi32(__m256i, __mmask8, __m512i);
__m128i _mm512_mask_cvtepi64_epi8(__m128i, __mmask8, __m512i);
void _mm512_mask_cvtepi64_storeu_epi16(short *, __mmask8, __m512i);
void _mm512_mask_cvtepi64_storeu_epi32(int *, __mmask8, __m512i);
void _mm512_mask_cvtepi64_storeu_epi8(char *, __mmask8, __m512i);
__m512i _mm512_mask_cvtepi8_epi32(__m512i, __mmask16, __m128i);
__m512i _mm512_mask_cvtepi8_epi64(__m512i, __mmask8, __m128i);
__m512i _mm512_mask_cvtepu16_epi32(__m512i, __mmask16, __m256i);
__m512i _mm512_mask_cvtepu16_epi64(__m512i, __mmask8, __m128i);
__m512i _mm512_mask_cvtepu32_epi64(__m512i, __mmask8, __m256i);
__m512d _mm512_mask_cvtepu32_pd(__m512d, __mmask8, __m256i);
__m512 _mm512_mask_cvtepu32_ps(__m512, __mmask16, __m512i);
__m512d _mm512_mask_cvtepu32lo_pd(__m512d, __mmask8, __m512i);
__m512i _mm512_mask_cvtepu8_epi32(__m512i, __mmask16, __m128i);
__m512i _mm512_mask_cvtepu8_epi64(__m512i, __mmask8, __m128i);
__m256i _mm512_mask_cvtpd_epi32(__m256i, __mmask8, __m512d);
__m256i _mm512_mask_cvtpd_epu32(__m256i, __mmask8, __m512d);
__m256 _mm512_mask_cvtpd_ps(__m256, __mmask8, __m512d);
__m512 _mm512_mask_cvtpd_pslo(__m512, __mmask8, __m512d);
__m512 _mm512_mask_cvtph_ps(__m512, __mmask16, __m256i);
__m512i _mm512_mask_cvtps_epi32(__m512i, __mmask16, __m512);
__m512i _mm512_mask_cvtps_epu32(__m512i, __mmask16, __m512);
__m512d _mm512_mask_cvtps_pd(__m512d, __mmask8, __m256);
__m256i _mm512_mask_cvtps_ph(__m256i, __mmask16, __m512, const int);
__m512d _mm512_mask_cvtpslo_pd(__m512d, __mmask8, __m512);
__m256i _mm512_mask_cvtsepi32_epi16(__m256i, __mmask16, __m512i);
__m128i _mm512_mask_cvtsepi32_epi8(__m128i, __mmask16, __m512i);
void _mm512_mask_cvtsepi32_storeu_epi16(short *, __mmask16, __m512i);
void _mm512_mask_cvtsepi32_storeu_epi8(char *, __mmask16, __m512i);
__m128i _mm512_mask_cvtsepi64_epi16(__m128i, __mmask8, __m512i);
__m256i _mm512_mask_cvtsepi64_epi32(__m256i, __mmask8, __m512i);
__m128i _mm512_mask_cvtsepi64_epi8(__m128i, __mmask8, __m512i);
void _mm512_mask_cvtsepi64_storeu_epi16(short *, __mmask8, __m512i);
void _mm512_mask_cvtsepi64_storeu_epi32(int *, __mmask8, __m512i);
void _mm512_mask_cvtsepi64_storeu_epi8(char *, __mmask8, __m512i);
__m256i _mm512_mask_cvtt_roundpd_epi32(__m256i, __mmask8, __m512d, const int);
__m256i _mm512_mask_cvtt_roundpd_epu32(__m256i, __mmask8, __m512d, const int);
__m512i _mm512_mask_cvtt_roundps_epi32(__m512i, __mmask16, __m512, const int);
__m512i _mm512_mask_cvtt_roundps_epu32(__m512i, __mmask16, __m512, const int);
__m256i _mm512_mask_cvttpd_epi32(__m256i, __mmask8, __m512d);
__m256i _mm512_mask_cvttpd_epu32(__m256i, __mmask8, __m512d);
__m512i _mm512_mask_cvttps_epi32(__m512i, __mmask16, __m512);
__m512i _mm512_mask_cvttps_epu32(__m512i, __mmask16, __m512);
__m256i _mm512_mask_cvtusepi32_epi16(__m256i, __mmask16, __m512i);
__m128i _mm512_mask_cvtusepi32_epi8(__m128i, __mmask16, __m512i);
void _mm512_mask_cvtusepi32_storeu_epi16(short *, __mmask16, __m512i);
void _mm512_mask_cvtusepi32_storeu_epi8(char *, __mmask16, __m512i);
__m128i _mm512_mask_cvtusepi64_epi16(__m128i, __mmask8, __m512i);
__m256i _mm512_mask_cvtusepi64_epi32(__m256i, __mmask8, __m512i);
__m128i _mm512_mask_cvtusepi64_epi8(__m128i, __mmask8, __m512i);
void _mm512_mask_cvtusepi64_storeu_epi16(short *, __mmask8, __m512i);
void _mm512_mask_cvtusepi64_storeu_epi32(int *, __mmask8, __m512i);
void _mm512_mask_cvtusepi64_storeu_epi8(char *, __mmask8, __m512i);
__m512d _mm512_mask_div_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_div_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_div_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_div_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512i _mm512_mask_expand_epi32(__m512i, __mmask16, __m512i);
__m512i _mm512_mask_expand_epi64(__m512i, __mmask8, __m512i);
__m512d _mm512_mask_expand_pd(__m512d, __mmask8, __m512d);
__m512 _mm512_mask_expand_ps(__m512, __mmask16, __m512);
__m512i _mm512_mask_expandloadu_epi32(__m512i, __mmask16, const int *);
__m512i _mm512_mask_expandloadu_epi64(__m512i, __mmask8, const long long *);
__m512d _mm512_mask_expandloadu_pd(__m512d, __mmask8, const double *);
__m512 _mm512_mask_expandloadu_ps(__m512, __mmask16, const float *);
__m128 _mm512_mask_extractf32x4_ps(__m128, __mmask8, __m512, const int);
__m256d _mm512_mask_extractf64x4_pd(__m256d, __mmask8, __m512d, const int);
__m128i _mm512_mask_extracti32x4_epi32(__m128i, __mmask8, __m512i, const int);
__m256i _mm512_mask_extracti64x4_epi64(__m256i, __mmask8, __m512i, const int);
__m512d _mm512_mask_fixupimm_pd(__m512d, __mmask8, __m512d, __m512i, const int);
__m512 _mm512_mask_fixupimm_ps(__m512, __mmask16, __m512, __m512i, const int);
__m512d _mm512_mask_fixupimm_round_pd(__m512d, __mmask8, __m512d, __m512i, const int, const int);
__m512 _mm512_mask_fixupimm_round_ps(__m512, __mmask16, __m512, __m512i, const int, const int);
__m512d _mm512_mask_fmadd_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_fmadd_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_fmadd_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_fmadd_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512d _mm512_mask_fmaddsub_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_fmaddsub_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_fmaddsub_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_fmaddsub_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512d _mm512_mask_fmsub_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_fmsub_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_fmsub_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_fmsub_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512d _mm512_mask_fmsubadd_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_fmsubadd_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_fmsubadd_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_fmsubadd_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512d _mm512_mask_fnmadd_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_fnmadd_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_fnmadd_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_fnmadd_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512d _mm512_mask_fnmsub_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_fnmsub_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_fnmsub_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_fnmsub_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512d _mm512_mask_getexp_pd(__m512d, __mmask8, __m512d);
__m512 _mm512_mask_getexp_ps(__m512, __mmask16, __m512);
__m512d _mm512_mask_getexp_round_pd(__m512d, __mmask8, __m512d, const int);
__m512 _mm512_mask_getexp_round_ps(__m512, __mmask16, __m512, const int);
__m512d _mm512_mask_getmant_pd(__m512d, __mmask8, __m512d, const int, const int);
__m512 _mm512_mask_getmant_ps(__m512, __mmask16, __m512, const int, const int);
__m512d _mm512_mask_getmant_round_pd(__m512d, __mmask8, __m512d, const int, const int, const int);
__m512 _mm512_mask_getmant_round_ps(__m512, __mmask16, __m512, const int, const int, const int);
__m512i _mm512_mask_i32gather_epi32(__m512i, __mmask16, __m512i, const int *, const int);
__m512i _mm512_mask_i32gather_epi64(__m512i, __mmask8, __m256i, const long long *, const int);
__m512d _mm512_mask_i32gather_pd(__m512d, __mmask8, __m256i, const double *, const int);
__m512 _mm512_mask_i32gather_ps(__m512, __mmask16, __m512i, const float *, const int);
__m512i _mm512_mask_i32logather_epi64(__m512i, __mmask8, __m512i, const long long *, const int);
__m512d _mm512_mask_i32logather_pd(__m512d, __mmask8, __m512i, const double *, const int);
void _mm512_mask_i32loscatter_epi64(long long *, __mmask8, __m512i, __m512i, const int);
void _mm512_mask_i32loscatter_pd(double *, __mmask8, __m512i, __m512d, const int);
void _mm512_mask_i32scatter_epi32(int *, __mmask16, __m512i, __m512i, const int);
void _mm512_mask_i32scatter_epi64(long long *, __mmask8, __m256i, __m512i, const int);
void _mm512_mask_i32scatter_pd(double *, __mmask8, __m256i, __m512d, const int);
void _mm512_mask_i32scatter_ps(float *, __mmask16, __m512i, __m512, const int);
__m256i _mm512_mask_i64gather_epi32(__m256i, __mmask8, __m512i, const int *, const int);
__m512i _mm512_mask_i64gather_epi64(__m512i, __mmask8, __m512i, const long long *, const int);
__m512d _mm512_mask_i64gather_pd(__m512d, __mmask8, __m512i, const double *, const int);
__m256 _mm512_mask_i64gather_ps(__m256, __mmask8, __m512i, const float *, const int);
void _mm512_mask_i64scatter_epi32(int *, __mmask8, __m512i, __m256i, const int);
void _mm512_mask_i64scatter_epi64(long long *, __mmask8, __m512i, __m512i, const int);
void _mm512_mask_i64scatter_pd(double *, __mmask8, __m512i, __m512d, const int);
void _mm512_mask_i64scatter_ps(float *, __mmask8, __m512i, __m256, const int);
__m512 _mm512_mask_insertf32x4(__m512, __mmask16, __m512, __m128, const int);
__m512d _mm512_mask_insertf64x4(__m512d, __mmask8, __m512d, __m256d, const int);
__m512i _mm512_mask_inserti32x4(__m512i, __mmask16, __m512i, __m128i, const int);
__m512i _mm512_mask_inserti64x4(__m512i, __mmask8, __m512i, __m256i, const int);
__m512i _mm512_mask_load_epi32(__m512i, __mmask16, const int *);
__m512i _mm512_mask_load_epi64(__m512i, __mmask8, const long long *);
__m512d _mm512_mask_load_pd(__m512d, __mmask8, const double *);
__m512 _mm512_mask_load_ps(__m512, __mmask16, const float *);
__m512i _mm512_mask_loadu_epi32(__m512i, __mmask16, const int *);
__m512i _mm512_mask_loadu_epi64(__m512i, __mmask8, const long long *);
__m512d _mm512_mask_loadu_pd(__m512d, __mmask8, const double *);
__m512 _mm512_mask_loadu_ps(__m512, __mmask16, const float *);
__m512i _mm512_mask_max_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_max_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512i _mm512_mask_max_epu32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_max_epu64(__m512i, __mmask8, __m512i, __m512i);
__m512d _mm512_mask_max_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_max_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_max_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_max_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512i _mm512_mask_min_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_min_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512i _mm512_mask_min_epu32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_min_epu64(__m512i, __mmask8, __m512i, __m512i);
__m512d _mm512_mask_min_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_min_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_min_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_min_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512i _mm512_mask_mov_epi32(__m512i, __mmask16, __m512i);
__m512i _mm512_mask_mov_epi64(__m512i, __mmask8, __m512i);
__m512d _mm512_mask_mov_pd(__m512d, __mmask8, __m512d);
__m512 _mm512_mask_mov_ps(__m512, __mmask16, __m512);
__m512d _mm512_mask_movedup_pd(__m512d, __mmask8, __m512d);
__m512 _mm512_mask_movehdup_ps(__m512, __mmask16, __m512);
__m512 _mm512_mask_moveldup_ps(__m512, __mmask16, __m512);
__m512i _mm512_mask_mul_epi32(__m512i, __mmask8, __m512i, __m512i);
__m512i _mm512_mask_mul_epu32(__m512i, __mmask8, __m512i, __m512i);
__m512d _mm512_mask_mul_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_mul_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_mul_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_mul_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512i _mm512_mask_mullo_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_mullox_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512i _mm512_mask_or_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_or_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512d _mm512_mask_permute_pd(__m512d, __mmask8, __m512d, const int);
__m512 _mm512_mask_permute_ps(__m512, __mmask16, __m512, const int);
__m512i _mm512_mask_permutevar_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512d _mm512_mask_permutevar_pd(__m512d, __mmask8, __m512d, __m512i);
__m512 _mm512_mask_permutevar_ps(__m512, __mmask16, __m512, __m512i);
__m512i _mm512_mask_permutex2var_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_permutex2var_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512d _mm512_mask_permutex2var_pd(__m512d, __mmask8, __m512i, __m512d);
__m512 _mm512_mask_permutex2var_ps(__m512, __mmask16, __m512i, __m512);
__m512i _mm512_mask_permutex_epi64(__m512i, __mmask8, __m512i, const int);
__m512d _mm512_mask_permutex_pd(__m512d, __mmask8, __m512d, const int);
__m512i _mm512_mask_permutexvar_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_permutexvar_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512d _mm512_mask_permutexvar_pd(__m512d, __mmask8, __m512i, __m512d);
__m512 _mm512_mask_permutexvar_ps(__m512, __mmask16, __m512i, __m512);
__m512d _mm512_mask_rcp14_pd(__m512d, __mmask8, __m512d);
__m512 _mm512_mask_rcp14_ps(__m512, __mmask16, __m512);
int _mm512_mask_reduce_add_epi32(__mmask16, __m512i);
long long _mm512_mask_reduce_add_epi64(__mmask8, __m512i);
double _mm512_mask_reduce_add_pd(__mmask8, __m512d);
float _mm512_mask_reduce_add_ps(__mmask16, __m512);
int _mm512_mask_reduce_and_epi32(__mmask16, __m512i);
long long _mm512_mask_reduce_and_epi64(__mmask8, __m512i);
int _mm512_mask_reduce_max_epi32(__mmask16, __m512i);
long long _mm512_mask_reduce_max_epi64(__mmask8, __m512i);
unsigned int _mm512_mask_reduce_max_epu32(__mmask16, __m512i);
unsigned long long _mm512_mask_reduce_max_epu64(__mmask8, __m512i);
double _mm512_mask_reduce_max_pd(__mmask8, __m512d);
float _mm512_mask_reduce_max_ps(__mmask16, __m512);
int _mm512_mask_reduce_min_epi32(__mmask16, __m512i);
long long _mm512_mask_reduce_min_epi64(__mmask8, __m512i);
unsigned int _mm512_mask_reduce_min_epu32(__mmask16, __m512i);
unsigned long long _mm512_mask_reduce_min_epu64(__mmask8, __m512i);
double _mm512_mask_reduce_min_pd(__mmask8, __m512d);
float _mm512_mask_reduce_min_ps(__mmask16, __m512);
int _mm512_mask_reduce_mul_epi32(__mmask16, __m512i);
long long _mm512_mask_reduce_mul_epi64(__mmask8, __m512i);
double _mm512_mask_reduce_mul_pd(__mmask8, __m512d);
float _mm512_mask_reduce_mul_ps(__mmask16, __m512);
int _mm512_mask_reduce_or_epi32(__mmask16, __m512i);
long long _mm512_mask_reduce_or_epi64(__mmask8, __m512i);
__m512i _mm512_mask_rol_epi32(__m512i, __mmask16, __m512i, const int);
__m512i _mm512_mask_rol_epi64(__m512i, __mmask8, __m512i, const int);
__m512i _mm512_mask_rolv_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_rolv_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512i _mm512_mask_ror_epi32(__m512i, __mmask16, __m512i, const int);
__m512i _mm512_mask_ror_epi64(__m512i, __mmask8, __m512i, const int);
__m512i _mm512_mask_rorv_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_rorv_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512d _mm512_mask_roundscale_pd(__m512d, __mmask8, __m512d, const int);
__m512 _mm512_mask_roundscale_ps(__m512, __mmask16, __m512, const int);
__m512d _mm512_mask_roundscale_round_pd(__m512d, __mmask8, __m512d, const int, const int);
__m512 _mm512_mask_roundscale_round_ps(__m512, __mmask16, __m512, const int, const int);
__m512d _mm512_mask_rsqrt14_pd(__m512d, __mmask8, __m512d);
__m512 _mm512_mask_rsqrt14_ps(__m512, __mmask16, __m512);
__m512d _mm512_mask_scalef_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_scalef_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_scalef_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_scalef_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512i _mm512_mask_set1_epi32(__m512i, __mmask16, int);
__m512i _mm512_mask_set1_epi64(__m512i, __mmask8, long long);
__m512i _mm512_mask_shuffle_epi32(__m512i, __mmask16, __m512i, const int);
__m512 _mm512_mask_shuffle_f32x4(__m512, __mmask16, __m512, __m512, const int);
__m512d _mm512_mask_shuffle_f64x2(__m512d, __mmask8, __m512d, __m512d, const int);
__m512i _mm512_mask_shuffle_i32x4(__m512i, __mmask16, __m512i, __m512i, const int);
__m512i _mm512_mask_shuffle_i64x2(__m512i, __mmask8, __m512i, __m512i, const int);
__m512d _mm512_mask_shuffle_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_shuffle_ps(__m512, __mmask16, __m512, __m512, const int);
__m512i _mm512_mask_sll_epi32(__m512i, __mmask16, __m512i, __m128i);
__m512i _mm512_mask_sll_epi64(__m512i, __mmask8, __m512i, __m128i);
__m512i _mm512_mask_slli_epi32(__m512i, __mmask16, __m512i, const unsigned int);
__m512i _mm512_mask_slli_epi64(__m512i, __mmask8, __m512i, const unsigned int);
__m512i _mm512_mask_sllv_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_sllv_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512d _mm512_mask_sqrt_pd(__m512d, __mmask8, __m512d);
__m512 _mm512_mask_sqrt_ps(__m512, __mmask16, __m512);
__m512d _mm512_mask_sqrt_round_pd(__m512d, __mmask8, __m512d, const int);
__m512 _mm512_mask_sqrt_round_ps(__m512, __mmask16, __m512, const int);
__m512i _mm512_mask_sra_epi32(__m512i, __mmask16, __m512i, __m128i);
__m512i _mm512_mask_sra_epi64(__m512i, __mmask8, __m512i, __m128i);
__m512i _mm512_mask_srai_epi32(__m512i, __mmask16, __m512i, const unsigned int);
__m512i _mm512_mask_srai_epi64(__m512i, __mmask8, __m512i, const unsigned int);
__m512i _mm512_mask_srav_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_srav_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512i _mm512_mask_srl_epi32(__m512i, __mmask16, __m512i, __m128i);
__m512i _mm512_mask_srl_epi64(__m512i, __mmask8, __m512i, __m128i);
__m512i _mm512_mask_srli_epi32(__m512i, __mmask16, __m512i, const unsigned int);
__m512i _mm512_mask_srli_epi64(__m512i, __mmask8, __m512i, const unsigned int);
__m512i _mm512_mask_srlv_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_srlv_epi64(__m512i, __mmask8, __m512i, __m512i);
void _mm512_mask_store_epi32(int *, __mmask16, __m512i);
void _mm512_mask_store_epi64(long long *, __mmask8, __m512i);
void _mm512_mask_store_pd(double *, __mmask8, __m512d);
void _mm512_mask_store_ps(float *, __mmask16, __m512);
void _mm512_mask_storeu_epi32(int *, __mmask16, __m512i);
void _mm512_mask_storeu_epi64(long long *, __mmask8, __m512i);
void _mm512_mask_storeu_pd(double *, __mmask8, __m512d);
void _mm512_mask_storeu_ps(float *, __mmask16, __m512);
__m512i _mm512_mask_sub_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_sub_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512d _mm512_mask_sub_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_sub_ps(__m512, __mmask16, __m512, __m512);
__m512d _mm512_mask_sub_round_pd(__m512d, __mmask8, __m512d, __m512d, const int);
__m512 _mm512_mask_sub_round_ps(__m512, __mmask16, __m512, __m512, const int);
__m512i _mm512_mask_ternarylogic_epi32(__m512i, __mmask16, __m512i, __m512i, const int);
__m512i _mm512_mask_ternarylogic_epi64(__m512i, __mmask8, __m512i, __m512i, const int);
__mmask16 _mm512_mask_test_epi32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_test_epi64_mask(__mmask8, __m512i, __m512i);
__mmask16 _mm512_mask_testn_epi32_mask(__mmask16, __m512i, __m512i);
__mmask8 _mm512_mask_testn_epi64_mask(__mmask8, __m512i, __m512i);
__m512i _mm512_mask_unpackhi_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_unpackhi_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512d _mm512_mask_unpackhi_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_unpackhi_ps(__m512, __mmask16, __m512, __m512);
__m512i _mm512_mask_unpacklo_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_unpacklo_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512d _mm512_mask_unpacklo_pd(__m512d, __mmask8, __m512d, __m512d);
__m512 _mm512_mask_unpacklo_ps(__m512, __mmask16, __m512, __m512);
__m512i _mm512_mask_xor_epi32(__m512i, __mmask16, __m512i, __m512i);
__m512i _mm512_mask_xor_epi64(__m512i, __mmask8, __m512i, __m512i);
__m512i _mm512_maskz_abs_epi32(__mmask16, __m512i);
__m512i _mm512_maskz_abs_epi64(__mmask8, __m512i);
__m512i _mm512_maskz_add_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_add_epi64(__mmask8, __m512i, __m512i);
__m512d _mm512_maskz_add_pd(__mmask8, __m512d, __m512d);
__m512 _mm512_maskz_add_ps(__mmask16, __m512, __m512);
__m512d _mm512_maskz_add_round_pd(__mmask8, __m512d, __m512d, const int);
__m512 _mm512_maskz_add_round_ps(__mmask16, __m512, __m512, const int);
__m512i _mm512_maskz_alignr_epi32(__mmask16, __m512i, __m512i, const int);
__m512i _mm512_maskz_alignr_epi64(__mmask8, __m512i, __m512i, const int);
__m512i _mm512_maskz_and_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_and_epi64(__mmask8, __m512i, __m512i);
__m512i _mm512_maskz_andnot_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_andnot_epi64(__mmask8, __m512i, __m512i);
__m512 _mm512_maskz_broadcast_f32x4(__mmask16, __m128);
__m512d _mm512_maskz_broadcast_f64x4(__mmask8, __m256d);
__m512i _mm512_maskz_broadcast_i32x4(__mmask16, __m128i);
__m512i _mm512_maskz_broadcast_i64x4(__mmask8, __m256i);
__m512i _mm512_maskz_broadcastd_epi32(__mmask16, __m128i);
__m512i _mm512_maskz_broadcastq_epi64(__mmask8, __m128i);
__m512d _mm512_maskz_broadcastsd_pd(__mmask8, __m128d);
__m512 _mm512_maskz_broadcastss_ps(__mmask16, __m128);
__m512i _mm512_maskz_compress_epi32(__mmask16, __m512i);
__m512i _mm512_maskz_compress_epi64(__mmask8, __m512i);
__m512d _mm512_maskz_compress_pd(__mmask8, __m512d);
__m512 _mm512_maskz_compress_ps(__mmask16, __m512);
__m512 _mm512_maskz_cvt_roundepi32_ps(__mmask16, __m512i, const int);
__m512 _mm512_maskz_cvt_roundepu32_ps(__mmask16, __m512i, const int);
__m256i _mm512_maskz_cvt_roundpd_epi32(__mmask8, __m512d, const int);
__m256i _mm512_maskz_cvt_roundpd_epu32(__mmask8, __m512d, const int);
__m256 _mm512_maskz_cvt_roundpd_ps(__mmask8, __m512d, const int);
__m512 _mm512_maskz_cvt_roundph_ps(__mmask16, __m256i, const int);
__m512i _mm512_maskz_cvt_roundps_epi32(__mmask16, __m512, const int);
__m512i _mm512_maskz_cvt_roundps_epu32(__mmask16, __m512, const int);
__m512d _mm512_maskz_cvt_roundps_pd(__mmask8, __m256, const int);
__m256i _mm512_maskz_cvt_roundps_ph(__mmask16, __m512, const int);
__m512i _mm512_maskz_cvtepi16_epi32(__mmask16, __m256i);
__m512i _mm512_maskz_cvtepi16_epi64(__mmask8, __m128i);
__m256i _mm512_maskz_cvtepi32_epi16(__mmask16, __m512i);
__m512i _mm512_maskz_cvtepi32_epi64(__mmask8, __m256i);
__m128i _mm512_maskz_cvtepi32_epi8(__mmask16, __m512i);
__m512d _mm512_maskz_cvtepi32_pd(__mmask8, __m256i);
__m512 _mm512_maskz_cvtepi32_ps(__mmask16, __m512i);
__m128i _mm512_maskz_cvtepi64_epi16(__mmask8, __m512i);
__m256i _mm512_maskz_cvtepi64_epi32(__mmask8, __m512i);
__m128i _mm512_maskz_cvtepi64_epi8(__mmask8, __m512i);
__m512i _mm512_maskz_cvtepi8_epi32(__mmask16, __m128i);
__m512i _mm512_maskz_cvtepi8_epi64(__mmask8, __m128i);
__m512i _mm512_maskz_cvtepu16_epi32(__mmask16, __m256i);
__m512i _mm512_maskz_cvtepu16_epi64(__mmask8, __m128i);
__m512i _mm512_maskz_cvtepu32_epi64(__mmask8, __m256i);
__m512d _mm512_maskz_cvtepu32_pd(__mmask8, __m256i);
__m512 _mm512_maskz_cvtepu32_ps(__mmask16, __m512i);
__m512i _mm512_maskz_cvtepu8_epi32(__mmask16, __m128i);
__m512i _mm512_maskz_cvtepu8_epi64(__mmask8, __m128i);
__m256i _mm512_maskz_cvtpd_epi32(__mmask8, __m512d);
__m256i _mm512_maskz_cvtpd_epu32(__mmask8, __m512d);
__m256 _mm512_maskz_cvtpd_ps(__mmask8, __m512d);
__m512 _mm512_maskz_cvtph_ps(__mmask16, __m256i);
__m512i _mm512_maskz_cvtps_epi32(__mmask16, __m512);
__m512i _mm512_maskz_cvtps_epu32(__mmask16, __m512);
__m512d _mm512_maskz_cvtps_pd(__mmask8, __m256);
__m256i _mm512_maskz_cvtps_ph(__mmask16, __m512, const int);
__m256i _mm512_maskz_cvtsepi32_epi16(__mmask16, __m512i);
__m128i _mm512_maskz_cvtsepi32_epi8(__mmask16, __m512i);
__m128i _mm512_maskz_cvtsepi64_epi16(__mmask8, __m512i);
__m256i _mm512_maskz_cvtsepi64_epi32(__mmask8, __m512i);
__m128i _mm512_maskz_cvtsepi64_epi8(__mmask8, __m512i);
__m256i _mm512_maskz_cvtt_roundpd_epi32(__mmask8, __m512d, const int);
__m256i _mm512_maskz_cvtt_roundpd_epu32(__mmask8, __m512d, const int);
__m512i _mm512_maskz_cvtt_roundps_epi32(__mmask16, __m512, const int);
__m512i _mm512_maskz_cvtt_roundps_epu32(__mmask16, __m512, const int);
__m256i _mm512_maskz_cvttpd_epi32(__mmask8, __m512d);
__m256i _mm512_maskz_cvttpd_epu32(__mmask8, __m512d);
__m512i _mm512_maskz_cvttps_epi32(__mmask16, __m512);
__m512i _mm512_maskz_cvttps_epu32(__mmask16, __m512);
__m256i _mm512_maskz_cvtusepi32_epi16(__mmask16, __m512i);
__m128i _mm512_maskz_cvtusepi32_epi8(__mmask16, __m512i);
__m128i _mm512_maskz_cvtusepi64_epi16(__mmask8, __m512i);
__m256i _mm512_maskz_cvtusepi64_epi32(__mmask8, __m512i);
__m128i _mm512_maskz_cvtusepi64_epi8(__mmask8, __m512i);
__m512d _mm512_maskz_div_pd(__mmask8, __m512d, __m512d);
__m512 _mm512_maskz_div_ps(__mmask16, __m512, __m512);
__m512d _mm512_maskz_div_round_pd(__mmask8, __m512d, __m512d, const int);
__m512 _mm512_maskz_div_round_ps(__mmask16, __m512, __m512, const int);
__m512i _mm512_maskz_expand_epi32(__mmask16, __m512i);
__m512i _mm512_maskz_expand_epi64(__mmask8, __m512i);
__m512d _mm512_maskz_expand_pd(__mmask8, __m512d);
__m512 _mm512_maskz_expand_ps(__mmask16, __m512);
__m512i _mm512_maskz_expandloadu_epi32(__mmask16, const int *);
__m512i _mm512_maskz_expandloadu_epi64(__mmask8, const long long *);
__m512d _mm512_maskz_expandloadu_pd(__mmask8, const double *);
__m512 _mm512_maskz_expandloadu_ps(__mmask16, const float *);
__m128 _mm512_maskz_extractf32x4_ps(__mmask8, __m512, const int);
__m256d _mm512_maskz_extractf64x4_pd(__mmask8, __m512d, const int);
__m128i _mm512_maskz_extracti32x4_epi32(__mmask8, __m512i, const int);
__m256i _mm512_maskz_extracti64x4_epi64(__mmask8, __m512i, const int);
__m512d _mm512_maskz_fixupimm_pd(__mmask8, __m512d, __m512d, __m512i, const int);
__m512 _mm512_maskz_fixupimm_ps(__mmask16, __m512, __m512, __m512i, const int);
__m512d _mm512_maskz_fixupimm_round_pd(__mmask8, __m512d, __m512d, __m512i, const int, const int);
__m512 _mm512_maskz_fixupimm_round_ps(__mmask16, __m512, __m512, __m512i, const int, const int);
__m512d _mm512_maskz_fmadd_pd(__mmask8, __m512d, __m512d, __m512d);
__m512 _mm512_maskz_fmadd_ps(__mmask16, __m512, __m512, __m512);
__m512d _mm512_maskz_fmadd_round_pd(__mmask8, __m512d, __m512d, __m512d, const int);
__m512 _mm512_maskz_fmadd_round_ps(__mmask16, __m512, __m512, __m512, const int);
__m512d _mm512_maskz_fmaddsub_pd(__mmask8, __m512d, __m512d, __m512d);
__m512 _mm512_maskz_fmaddsub_ps(__mmask16, __m512, __m512, __m512);
__m512d _mm512_maskz_fmaddsub_round_pd(__mmask8, __m512d, __m512d, __m512d, const int);
__m512 _mm512_maskz_fmaddsub_round_ps(__mmask16, __m512, __m512, __m512, const int);
__m512d _mm512_maskz_fmsub_pd(__mmask8, __m512d, __m512d, __m512d);
__m512 _mm512_maskz_fmsub_ps(__mmask16, __m512, __m512, __m512);
__m512d _mm512_maskz_fmsub_round_pd(__mmask8, __m512d, __m512d, __m512d, const int);
__m512 _mm512_maskz_fmsub_round_ps(__mmask16, __m512, __m512, __m512, const int);
__m512d _mm512_maskz_fmsubadd_pd(__mmask8, __m512d, __m512d, __m512d);
__m512 _mm512_maskz_fmsubadd_ps(__mmask16, __m512, __m512, __m512);
__m512d _mm512_maskz_fmsubadd_round_pd(__mmask8, __m512d, __m512d, __m512d, const int);
__m512 _mm512_maskz_fmsubadd_round_ps(__mmask16, __m512, __m512, __m512, const int);
__m512d _mm512_maskz_fnmadd_pd(__mmask8, __m512d, __m512d, __m512d);
__m512 _mm512_maskz_fnmadd_ps(__mmask16, __m512, __m512, __m512);
__m512d _mm512_maskz_fnmadd_round_pd(__mmask8, __m512d, __m512d, __m512d, const int);
__m512 _mm512_maskz_fnmadd_round_ps(__mmask16, __m512, __m512, __m512, const int);
__m512d _mm512_maskz_fnmsub_pd(__mmask8, __m512d, __m512d, __m512d);
__m512 _mm512_maskz_fnmsub_ps(__mmask16, __m512, __m512, __m512);
__m512d _mm512_maskz_fnmsub_round_pd(__mmask8, __m512d, __m512d, __m512d, const int);
__m512 _mm512_maskz_fnmsub_round_ps(__mmask16, __m512, __m512, __m512, const int);
__m512d _mm512_maskz_getexp_pd(__mmask8, __m512d);
__m512 _mm512_maskz_getexp_ps(__mmask16, __m512);
__m512d _mm512_maskz_getexp_round_pd(__mmask8, __m512d, const int);
__m512 _mm512_maskz_getexp_round_ps(__mmask16, __m512, const int);
__m512d _mm512_maskz_getmant_pd(__mmask8, __m512d, const int, const int);
__m512 _mm512_maskz_getmant_ps(__mmask16, __m512, const int, const int);
__m512d _mm512_maskz_getmant_round_pd(__mmask8, __m512d, const int, const int, const int);
__m512 _mm512_maskz_getmant_round_ps(__mmask16, __m512, const int, const int, const int);
__m512 _mm512_maskz_insertf32x4(__mmask16, __m512, __m128, const int);
__m512d _mm512_maskz_insertf64x4(__mmask8, __m512d, __m256d, const int);
__m512i _mm512_maskz_inserti32x4(__mmask16, __m512i, __m128i, const int);
__m512i _mm512_maskz_inserti64x4(__mmask8, __m512i, __m256i, const int);
__m512i _mm512_maskz_load_epi32(__mmask16, const int *);
__m512i _mm512_maskz_load_epi64(__mmask8, const long long *);
__m512d _mm512_maskz_load_pd(__mmask8, const double *);
__m512 _mm512_maskz_load_ps(__mmask16, const float *);
__m512i _mm512_maskz_loadu_epi32(__mmask16, const int *);
__m512i _mm512_maskz_loadu_epi64(__mmask8, const long long *);
__m512d _mm512_maskz_loadu_pd(__mmask8, const double *);
__m512 _mm512_maskz_loadu_ps(__mmask16, const float *);
__m512i _mm512_maskz_max_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_max_epi64(__mmask8, __m512i, __m512i);
__m512i _mm512_maskz_max_epu32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_max_epu64(__mmask8, __m512i, __m512i);
__m512d _mm512_maskz_max_pd(__mmask8, __m512d, __m512d);
__m512 _mm512_maskz_max_ps(__mmask16, __m512, __m512);
__m512d _mm512_maskz_max_round_pd(__mmask8, __m512d, __m512d, const int);
__m512 _mm512_maskz_max_round_ps(__mmask16, __m512, __m512, const int);
__m512i _mm512_maskz_min_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_min_epi64(__mmask8, __m512i, __m512i);
__m512i _mm512_maskz_min_epu32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_min_epu64(__mmask8, __m512i, __m512i);
__m512d _mm512_maskz_min_pd(__mmask8, __m512d, __m512d);
__m512 _mm512_maskz_min_ps(__mmask16, __m512, __m512);
__m512d _mm512_maskz_min_round_pd(__mmask8, __m512d, __m512d, const int);
__m512 _mm512_maskz_min_round_ps(__mmask16, __m512, __m512, const int);
__m512i _mm512_maskz_mov_epi32(__mmask16, __m512i);
__m512i _mm512_maskz_mov_epi64(__mmask8, __m512i);
__m512d _mm512_maskz_mov_pd(__mmask8, __m512d);
__m512 _mm512_maskz_mov_ps(__mmask16, __m512);
__m512d _mm512_maskz_movedup_pd(__mmask8, __m512d);
__m512 _mm512_maskz_movehdup_ps(__mmask16, __m512);
__m512 _mm512_maskz_moveldup_ps(__mmask16, __m512);
__m512i _mm512_maskz_mul_epi32(__mmask8, __m512i, __m512i);
__m512i _mm512_maskz_mul_epu32(__mmask8, __m512i, __m512i);
__m512d _mm512_maskz_mul_pd(__mmask8, __m512d, __m512d);
__m512 _mm512_maskz_mul_ps(__mmask16, __m512, __m512);
__m512d _mm512_maskz_mul_round_pd(__mmask8, __m512d, __m512d, const int);
__m512 _mm512_maskz_mul_round_ps(__mmask16, __m512, __m512, const int);
__m512i _mm512_maskz_mullo_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_or_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_or_epi64(__mmask8, __m512i, __m512i);
__m512d _mm512_maskz_permute_pd(__mmask8, __m512d, const int);
__m512 _mm512_maskz_permute_ps(__mmask16, __m512, const int);
__m512d _mm512_maskz_permutevar_pd(__mmask8, __m512d, __m512i);
__m512 _mm512_maskz_permutevar_ps(__mmask16, __m512, __m512i);
__m512i _mm512_maskz_permutex2var_epi32(__mmask16, __m512i, __m512i, __m512i);
__m512i _mm512_maskz_permutex2var_epi64(__mmask8, __m512i, __m512i, __m512i);
__m512d _mm512_maskz_permutex2var_pd(__mmask8, __m512d, __m512i, __m512d);
__m512 _mm512_maskz_permutex2var_ps(__mmask16, __m512, __m512i, __m512);
__m512i _mm512_maskz_permutex_epi64(__mmask8, __m512i, const int);
__m512d _mm512_maskz_permutex_pd(__mmask8, __m512d, const int);
__m512i _mm512_maskz_permutexvar_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_permutexvar_epi64(__mmask8, __m512i, __m512i);
__m512d _mm512_maskz_permutexvar_pd(__mmask8, __m512i, __m512d);
__m512 _mm512_maskz_permutexvar_ps(__mmask16, __m512i, __m512);
__m512d _mm512_maskz_rcp14_pd(__mmask8, __m512d);
__m512 _mm512_maskz_rcp14_ps(__mmask16, __m512);
__m512i _mm512_maskz_rol_epi32(__mmask16, __m512i, const int);
__m512i _mm512_maskz_rol_epi64(__mmask8, __m512i, const int);
__m512i _mm512_maskz_rolv_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_rolv_epi64(__mmask8, __m512i, __m512i);
__m512i _mm512_maskz_ror_epi32(__mmask16, __m512i, const int);
__m512i _mm512_maskz_ror_epi64(__mmask8, __m512i, const int);
__m512i _mm512_maskz_rorv_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_rorv_epi64(__mmask8, __m512i, __m512i);
__m512d _mm512_maskz_roundscale_pd(__mmask8, __m512d, const int);
__m512 _mm512_maskz_roundscale_ps(__mmask16, __m512, const int);
__m512d _mm512_maskz_roundscale_round_pd(__mmask8, __m512d, const int, const int);
__m512 _mm512_maskz_roundscale_round_ps(__mmask16, __m512, const int, const int);
__m512d _mm512_maskz_rsqrt14_pd(__mmask8, __m512d);
__m512 _mm512_maskz_rsqrt14_ps(__mmask16, __m512);
__m512d _mm512_maskz_scalef_pd(__mmask8, __m512d, __m512d);
__m512 _mm512_maskz_scalef_ps(__mmask16, __m512, __m512);
__m512d _mm512_maskz_scalef_round_pd(__mmask8, __m512d, __m512d, const int);
__m512 _mm512_maskz_scalef_round_ps(__mmask16, __m512, __m512, const int);
__m512i _mm512_maskz_set1_epi32(__mmask16, int);
__m512i _mm512_maskz_set1_epi64(__mmask8, long long);
__m512i _mm512_maskz_shuffle_epi32(__mmask16, __m512i, const int);
__m512 _mm512_maskz_shuffle_f32x4(__mmask16, __m512, __m512, const int);
__m512d _mm512_maskz_shuffle_f64x2(__mmask8, __m512d, __m512d, const int);
__m512i _mm512_maskz_shuffle_i32x4(__mmask16, __m512i, __m512i, const int);
__m512i _mm512_maskz_shuffle_i64x2(__mmask8, __m512i, __m512i, const int);
__m512d _mm512_maskz_shuffle_pd(__mmask8, __m512d, __m512d, const int);
__m512 _mm512_maskz_shuffle_ps(__mmask16, __m512, __m512, const int);
__m512i _mm512_maskz_sll_epi32(__mmask16, __m512i, __m128i);
__m512i _mm512_maskz_sll_epi64(__mmask8, __m512i, __m128i);
__m512i _mm512_maskz_slli_epi32(__mmask16, __m512i, const unsigned int);
__m512i _mm512_maskz_slli_epi64(__mmask8, __m512i, const unsigned int);
__m512i _mm512_maskz_sllv_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_sllv_epi64(__mmask8, __m512i, __m512i);
__m512d _mm512_maskz_sqrt_pd(__mmask8, __m512d);
__m512 _mm512_maskz_sqrt_ps(__mmask16, __m512);
__m512d _mm512_maskz_sqrt_round_pd(__mmask8, __m512d, const int);
__m512 _mm512_maskz_sqrt_round_ps(__mmask16, __m512, const int);
__m512i _mm512_maskz_sra_epi32(__mmask16, __m512i, __m128i);
__m512i _mm512_maskz_sra_epi64(__mmask8, __m512i, __m128i);
__m512i _mm512_maskz_srai_epi32(__mmask16, __m512i, const unsigned int);
__m512i _mm512_maskz_srai_epi64(__mmask8, __m512i, const unsigned int);
__m512i _mm512_maskz_srav_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_srav_epi64(__mmask8, __m512i, __m512i);
__m512i _mm512_maskz_srl_epi32(__mmask16, __m512i, __m128i);
__m512i _mm512_maskz_srl_epi64(__mmask8, __m512i, __m128i);
__m512i _mm512_maskz_srli_epi32(__mmask16, __m512i, const unsigned int);
__m512i _mm512_maskz_srli_epi64(__mmask8, __m512i, const unsigned int);
__m512i _mm512_maskz_srlv_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_srlv_epi64(__mmask8, __m512i, __m512i);
__m512i _mm512_maskz_sub_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_sub_epi64(__mmask8, __m512i, __m512i);
__m512d _mm512_maskz_sub_pd(__mmask8, __m512d, __m512d);
__m512 _mm512_maskz_sub_ps(__mmask16, __m512, __m512);
__m512d _mm512_maskz_sub_round_pd(__mmask8, __m512d, __m512d, const int);
__m512 _mm512_maskz_sub_round_ps(__mmask16, __m512, __m512, const int);
__m512i _mm512_maskz_ternarylogic_epi32(__mmask16, __m512i, __m512i, __m512i, const int);
__m512i _mm512_maskz_ternarylogic_epi64(__mmask8, __m512i, __m512i, __m512i, const int);
__m512i _mm512_maskz_unpackhi_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_unpackhi_epi64(__mmask8, __m512i, __m512i);
__m512d _mm512_maskz_unpackhi_pd(__mmask8, __m512d, __m512d);
__m512 _mm512_maskz_unpackhi_ps(__mmask16, __m512, __m512);
__m512i _mm512_maskz_unpacklo_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_unpacklo_epi64(__mmask8, __m512i, __m512i);
__m512d _mm512_maskz_unpacklo_pd(__mmask8, __m512d, __m512d);
__m512 _mm512_maskz_unpacklo_ps(__mmask16, __m512, __m512);
__m512i _mm512_maskz_xor_epi32(__mmask16, __m512i, __m512i);
__m512i _mm512_maskz_xor_epi64(__mmask8, __m512i, __m512i);
__m512i _mm512_max_epi32(__m512i, __m512i);
__m512i _mm512_max_epi64(__m512i, __m512i);
__m512i _mm512_max_epu32(__m512i, __m512i);
__m512i _mm512_max_epu64(__m512i, __m512i);
__m512d _mm512_max_pd(__m512d, __m512d);
__m512 _mm512_max_ps(__m512, __m512);
__m512d _mm512_max_round_pd(__m512d, __m512d, const int);
__m512 _mm512_max_round_ps(__m512, __m512, const int);
__m512i _mm512_min_epi32(__m512i, __m512i);
__m512i _mm512_min_epi64(__m512i, __m512i);
__m512i _mm512_min_epu32(__m512i, __m512i);
__m512i _mm512_min_epu64(__m512i, __m512i);
__m512d _mm512_min_pd(__m512d, __m512d);
__m512 _mm512_min_ps(__m512, __m512);
__m512d _mm512_min_round_pd(__m512d, __m512d, const int);
__m512 _mm512_min_round_ps(__m512, __m512, const int);
__m512d _mm512_movedup_pd(__m512d);
__m512 _mm512_movehdup_ps(__m512);
__m512 _mm512_moveldup_ps(__m512);
__m512i _mm512_mul_epi32(__m512i, __m512i);
__m512i _mm512_mul_epu32(__m512i, __m512i);
__m512d _mm512_mul_pd(__m512d, __m512d);
__m512 _mm512_mul_ps(__m512, __m512);
__m512d _mm512_mul_round_pd(__m512d, __m512d, const int);
__m512 _mm512_mul_round_ps(__m512, __m512, const int);
__m512i _mm512_mullo_epi32(__m512i, __m512i);
__m512i _mm512_mullox_epi64(__m512i, __m512i);
__m512i _mm512_or_epi32(__m512i, __m512i);
__m512i _mm512_or_epi64(__m512i, __m512i);
__m512i _mm512_or_si512(__m512i, __m512i);
__m512d _mm512_permute_pd(__m512d, const int);
__m512 _mm512_permute_ps(__m512, const int);
__m512i _mm512_permutevar_epi32(__m512i, __m512i);
__m512d _mm512_permutevar_pd(__m512d, __m512i);
__m512 _mm512_permutevar_ps(__m512, __m512i);
__m512i _mm512_permutex2var_epi32(__m512i, __m512i, __m512i);
__m512i _mm512_permutex2var_epi64(__m512i, __m512i, __m512i);
__m512d _mm512_permutex2var_pd(__m512d, __m512i, __m512d);
__m512 _mm512_permutex2var_ps(__m512, __m512i, __m512);
__m512i _mm512_permutex_epi64(__m512i, const int);
__m512d _mm512_permutex_pd(__m512d, const int);
__m512i _mm512_permutexvar_epi32(__m512i, __m512i);
__m512i _mm512_permutexvar_epi64(__m512i, __m512i);
__m512d _mm512_permutexvar_pd(__m512i, __m512d);
__m512 _mm512_permutexvar_ps(__m512i, __m512);
__m512d _mm512_rcp14_pd(__m512d);
__m512 _mm512_rcp14_ps(__m512);
int _mm512_reduce_add_epi32(__m512i);
long long _mm512_reduce_add_epi64(__m512i);
double _mm512_reduce_add_pd(__m512d);
float _mm512_reduce_add_ps(__m512);
int _mm512_reduce_and_epi32(__m512i);
long long _mm512_reduce_and_epi64(__m512i);
int _mm512_reduce_max_epi32(__m512i);
long long _mm512_reduce_max_epi64(__m512i);
unsigned int _mm512_reduce_max_epu32(__m512i);
unsigned long long _mm512_reduce_max_epu64(__m512i);
double _mm512_reduce_max_pd(__m512d);
float _mm512_reduce_max_ps(__m512);
int _mm512_reduce_min_epi32(__m512i);
long long _mm512_reduce_min_epi64(__m512i);
unsigned int _mm512_reduce_min_epu32(__m512i);
unsigned long long _mm512_reduce_min_epu64(__m512i);
double _mm512_reduce_min_pd(__m512d);
float _mm512_reduce_min_ps(__m512);
int _mm512_reduce_mul_epi32(__m512i);
long long _mm512_reduce_mul_epi64(__m512i);
double _mm512_reduce_mul_pd(__m512d);
float _mm512_reduce_mul_ps(__m512);
int _mm512_reduce_or_epi32(__m512i);
long long _mm512_reduce_or_epi64(__m512i);
__m512i _mm512_rol_epi32(__m512i, const int);
__m512i _mm512_rol_epi64(__m512i, const int);
__m512i _mm512_rolv_epi32(__m512i, __m512i);
__m512i _mm512_rolv_epi64(__m512i, __m512i);
__m512i _mm512_ror_epi32(__m512i, const int);
__m512i _mm512_ror_epi64(__m512i, const int);
__m512i _mm512_rorv_epi32(__m512i, __m512i);
__m512i _mm512_rorv_epi64(__m512i, __m512i);
__m512d _mm512_roundscale_pd(__m512d, const int);
__m512 _mm512_roundscale_ps(__m512, const int);
__m512d _mm512_roundscale_round_pd(__m512d, const int, const int);
__m512 _mm512_roundscale_round_ps(__m512, const int, const int);
__m512d _mm512_rsqrt14_pd(__m512d);
__m512 _mm512_rsqrt14_ps(__m512);
__m512d _mm512_scalef_pd(__m512d, __m512d);
__m512 _mm512_scalef_ps(__m512, __m512);
__m512d _mm512_scalef_round_pd(__m512d, __m512d, const int);
__m512 _mm512_scalef_round_ps(__m512, __m512, const int);
__m512i _mm512_set1_epi16(short);
__m512i _mm512_set1_epi32(int);
__m512i _mm512_set1_epi64(long long);
__m512i _mm512_set1_epi8(char);
__m512d _mm512_set1_pd(double);
__m512 _mm512_set1_ps(float);
__m512i _mm512_set4_epi32(int, int, int, int);
__m512i _mm512_set4_epi64(long long, long long, long long, long long);
__m512d _mm512_set4_pd(double, double, double, double);
__m512 _mm512_set4_ps(float, float, float, float);
__m512i _mm512_set_epi16(short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short, short);
__m512i _mm512_set_epi32(int, int, int, int, int, int, int, int, int, int, int, int, int, int, int, int);
__m512i _mm512_set_epi64(long long, long long, long long, long long, long long, long long, long long, long long);
__m512i _mm512_set_epi8(char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char, char);
__m512d _mm512_set_pd(double, double, double, double, double, double, double, double);
__m512 _mm512_set_ps(float, float, float, float, float, float, float, float, float, float, float, float, float, float, float, float);
__m512i _mm512_setr4_epi32(int, int, int, int);
__m512i _mm512_setr4_epi64(long long, long long, long long, long long);
__m512d _mm512_setr4_pd(double, double, double, double);
__m512 _mm512_setr4_ps(float, float, float, float);
__m512i _mm512_setr_epi32(int, int, int, int, int, int, int, int, int, int, int, int, int, int, int, int);
__m512i _mm512_setr_epi64(long long, long long, long long, long long, long long, long long, long long, long long);
__m512d _mm512_setr_pd(double, double, double, double, double, double, double, double);
__m512 _mm512_setr_ps(float, float, float, float, float, float, float, float, float, float, float, float, float, float, float, float);
__m512 _mm512_setzero(void);
__m512i _mm512_setzero_epi32(void);
__m512d _mm512_setzero_pd(void);
__m512 _mm512_setzero_ps(void);
__m512i _mm512_setzero_si512(void);
__m512i _mm512_shuffle_epi32(__m512i, const int);
__m512 _mm512_shuffle_f32x4(__m512, __m512, const int);
__m512d _mm512_shuffle_f64x2(__m512d, __m512d, const int);
__m512i _mm512_shuffle_i32x4(__m512i, __m512i, const int);
__m512i _mm512_shuffle_i64x2(__m512i, __m512i, const int);
__m512d _mm512_shuffle_pd(__m512d, __m512d, const int);
__m512 _mm512_shuffle_ps(__m512, __m512, const int);
__m512i _mm512_sll_epi32(__m512i, __m128i);
__m512i _mm512_sll_epi64(__m512i, __m128i);
__m512i _mm512_slli_epi32(__m512i, const unsigned int);
__m512i _mm512_slli_epi64(__m512i, const unsigned int);
__m512i _mm512_sllv_epi32(__m512i, __m512i);
__m512i _mm512_sllv_epi64(__m512i, __m512i);
__m512d _mm512_sqrt_pd(__m512d);
__m512 _mm512_sqrt_ps(__m512);
__m512d _mm512_sqrt_round_pd(__m512d, const int);
__m512 _mm512_sqrt_round_ps(__m512, const int);
__m512i _mm512_sra_epi32(__m512i, __m128i);
__m512i _mm512_sra_epi64(__m512i, __m128i);
__m512i _mm512_srai_epi32(__m512i, const unsigned int);
__m512i _mm512_srai_epi64(__m512i, const unsigned int);
__m512i _mm512_srav_epi32(__m512i, __m512i);
__m512i _mm512_srav_epi64(__m512i, __m512i);
__m512i _mm512_srl_epi32(__m512i, __m128i);
__m512i _mm512_srl_epi64(__m512i, __m128i);
__m512i _mm512_srli_epi32(__m512i, const unsigned int);
__m512i _mm512_srli_epi64(__m512i, const unsigned int);
__m512i _mm512_srlv_epi32(__m512i, __m512i);
__m512i _mm512_srlv_epi64(__m512i, __m512i);
void _mm512_store_epi32(int *, __m512i);
void _mm512_store_epi64(long long *, __m512i);
void _mm512_store_pd(double *, __m512d);
void _mm512_store_ps(float *, __m512);
void _mm512_store_si512(__m512i *, __m512i);
void _mm512_storeu_epi32(int *, __m512i);
void _mm512_storeu_epi64(long long *, __m512i);
void _mm512_storeu_pd(double *, __m512d);
void _mm512_storeu_ps(float *, __m512);
void _mm512_storeu_si512(__m512i *, __m512i);
__m512i _mm512_stream_load_si512(const __m512i *);
void _mm512_stream_pd(double *, __m512d);
void _mm512_stream_ps(float *, __m512);
void _mm512_stream_si512(__m512i *, __m512i);
__m512i _mm512_sub_epi32(__m512i, __m512i);
__m512i _mm512_sub_epi64(__m512i, __m512i);
__m512d _mm512_sub_pd(__m512d, __m512d);
__m512 _mm512_sub_ps(__m512, __m512);
__m512d _mm512_sub_round_pd(__m512d, __m512d, const int);
__m512 _mm512_sub_round_ps(__m512, __m512, const int);
__m512i _mm512_ternarylogic_epi32(__m512i, __m512i, __m512i, const int);
__m512i _mm512_ternarylogic_epi64(__m512i, __m512i, __m512i, const int);
__mmask16 _mm512_test_epi32_mask(__m512i, __m512i);
__mmask8 _mm512_test_epi64_mask(__m512i, __m512i);
__mmask16 _mm512_testn_epi32_mask(__m512i, __m512i);
__mmask8 _mm512_testn_epi64_mask(__m512i, __m512i);
__m512 _mm512_undefined(void);
__m512i _mm512_undefined_epi32(void);
__m512d _mm512_undefined_pd(void);
__m512 _mm512_undefined_ps(void);
__m512i _mm512_unpackhi_epi32(__m512i, __m512i);
__m512i _mm512_unpackhi_epi64(__m512i, __m512i);
__m512d _mm512_unpackhi_pd(__m512d, __m512d);
__m512 _mm512_unpackhi_ps(__m512, __m512);
__m512i _mm512_unpacklo_epi32(__m512i, __m512i);
__m512i _mm512_unpacklo_epi64(__m512i, __m512i);
__m512d _mm512_unpacklo_pd(__m512d, __m512d);
__m512 _mm512_unpacklo_ps(__m512, __m512);
__m512i _mm512_xor_epi32(__m512i, __m512i);
__m512i _mm512_xor_epi64(__m512i, __m512i);
__m512i _mm512_xor_si512(__m512i, __m512i);
__m512d _mm512_zextpd128_pd512(__m128d);
__m512d _mm512_zextpd256_pd512(__m256d);
__m512 _mm512_zextps128_ps512(__m128);
__m512 _mm512_zextps256_ps512(__m256);
__m512i _mm512_zextsi128_si512(__m128i);
__m512i _mm512_zextsi256_si512(__m256i);
__m128d _mm_add_round_sd(__m128d, __m128d, const int);
__m128 _mm_add_round_ss(__m128, __m128, const int);
__mmask8 _mm_cmp_round_sd_mask(__m128d, __m128d, const int, const int);
__mmask8 _mm_cmp_round_ss_mask(__m128, __m128, const int, const int);
__mmask8 _mm_cmp_sd_mask(__m128d, __m128d, const int);
__mmask8 _mm_cmp_ss_mask(__m128, __m128, const int);
int _mm_comi_round_sd(__m128d, __m128d, const int, const int);
int _mm_comi_round_ss(__m128, __m128, const int, const int);
__m128 _mm_cvt_roundi32_ss(__m128, int, const int);
int _mm_cvt_roundsd_i32(__m128d, const int);
int _mm_cvt_roundsd_si32(__m128d, const int);
__m128 _mm_cvt_roundsd_ss(__m128, __m128d, const int);
unsigned int _mm_cvt_roundsd_u32(__m128d, const int);
__m128 _mm_cvt_roundsi32_ss(__m128, int, const int);
int _mm_cvt_roundss_i32(__m128, const int);
__m128d _mm_cvt_roundss_sd(__m128d, __m128, const int);
int _mm_cvt_roundss_si32(__m128, const int);
unsigned int _mm_cvt_roundss_u32(__m128, const int);
__m128 _mm_cvt_roundu32_ss(__m128, unsigned int, const int);
__m128d _mm_cvti32_sd(__m128d, int);
__m128 _mm_cvti32_ss(__m128, int);
int _mm_cvtsd_i32(__m128d);
unsigned int _mm_cvtsd_u32(__m128d);
int _mm_cvtss_i32(__m128);
unsigned int _mm_cvtss_u32(__m128);
int _mm_cvtt_roundsd_i32(__m128d, const int);
int _mm_cvtt_roundsd_si32(__m128d, const int);
unsigned int _mm_cvtt_roundsd_u32(__m128d, const int);
int _mm_cvtt_roundss_i32(__m128, const int);
int _mm_cvtt_roundss_si32(__m128, const int);
unsigned int _mm_cvtt_roundss_u32(__m128, const int);
int _mm_cvttsd_i32(__m128d);
unsigned int _mm_cvttsd_u32(__m128d);
int _mm_cvttss_i32(__m128);
unsigned int _mm_cvttss_u32(__m128);
__m128d _mm_cvtu32_sd(__m128d, unsigned int);
__m128 _mm_cvtu32_ss(__m128, unsigned int);
__m128d _mm_div_round_sd(__m128d, __m128d, const int);
__m128 _mm_div_round_ss(__m128, __m128, const int);
__m128d _mm_fixupimm_round_sd(__m128d, __m128d, __m128i, const int, const int);
__m128 _mm_fixupimm_round_ss(__m128, __m128, __m128i, const int, const int);
__m128d _mm_fixupimm_sd(__m128d, __m128d, __m128i, const int);
__m128 _mm_fixupimm_ss(__m128, __m128, __m128i, const int);
__m128d _mm_fmadd_round_sd(__m128d, __m128d, __m128d, const int);
__m128 _mm_fmadd_round_ss(__m128, __m128, __m128, const int);
__m128d _mm_fmsub_round_sd(__m128d, __m128d, __m128d, const int);
__m128 _mm_fmsub_round_ss(__m128, __m128, __m128, const int);
__m128d _mm_fnmadd_round_sd(__m128d, __m128d, __m128d, const int);
__m128 _mm_fnmadd_round_ss(__m128, __m128, __m128, const int);
__m128d _mm_fnmsub_round_sd(__m128d, __m128d, __m128d, const int);
__m128 _mm_fnmsub_round_ss(__m128, __m128, __m128, const int);
__m128d _mm_getexp_round_sd(__m128d, __m128d, const int);
__m128 _mm_getexp_round_ss(__m128, __m128, const int);
__m128d _mm_getexp_sd(__m128d, __m128d);
__m128 _mm_getexp_ss(__m128, __m128);
__m128d _mm_getmant_round_sd(__m128d, __m128d, const int, const int, const int);
__m128 _mm_getmant_round_ss(__m128, __m128, const int, const int, const int);
__m128d _mm_getmant_sd(__m128d, __m128d, const int, const int);
__m128 _mm_getmant_ss(__m128, __m128, const int, const int);
__m128d _mm_mask3_fmadd_round_sd(__m128d, __m128d, __m128d, __mmask8, const int);
__m128 _mm_mask3_fmadd_round_ss(__m128, __m128, __m128, __mmask8, const int);
__m128d _mm_mask3_fmadd_sd(__m128d, __m128d, __m128d, __mmask8);
__m128 _mm_mask3_fmadd_ss(__m128, __m128, __m128, __mmask8);
__m128d _mm_mask3_fmsub_round_sd(__m128d, __m128d, __m128d, __mmask8, const int);
__m128 _mm_mask3_fmsub_round_ss(__m128, __m128, __m128, __mmask8, const int);
__m128d _mm_mask3_fmsub_sd(__m128d, __m128d, __m128d, __mmask8);
__m128 _mm_mask3_fmsub_ss(__m128, __m128, __m128, __mmask8);
__m128d _mm_mask3_fnmadd_round_sd(__m128d, __m128d, __m128d, __mmask8, const int);
__m128 _mm_mask3_fnmadd_round_ss(__m128, __m128, __m128, __mmask8, const int);
__m128d _mm_mask3_fnmadd_sd(__m128d, __m128d, __m128d, __mmask8);
__m128 _mm_mask3_fnmadd_ss(__m128, __m128, __m128, __mmask8);
__m128d _mm_mask3_fnmsub_round_sd(__m128d, __m128d, __m128d, __mmask8, const int);
__m128 _mm_mask3_fnmsub_round_ss(__m128, __m128, __m128, __mmask8, const int);
__m128d _mm_mask3_fnmsub_sd(__m128d, __m128d, __m128d, __mmask8);
__m128 _mm_mask3_fnmsub_ss(__m128, __m128, __m128, __mmask8);
__m128d _mm_mask_add_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_add_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_add_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_add_ss(__m128, __mmask8, __m128, __m128);
__mmask8 _mm_mask_cmp_round_sd_mask(__mmask8, __m128d, __m128d, const int, const int);
__mmask8 _mm_mask_cmp_round_ss_mask(__mmask8, __m128, __m128, const int, const int);
__mmask8 _mm_mask_cmp_sd_mask(__mmask8, __m128d, __m128d, const int);
__mmask8 _mm_mask_cmp_ss_mask(__mmask8, __m128, __m128, const int);
__m128 _mm_mask_cvt_roundsd_ss(__m128, __mmask8, __m128, __m128d, const int);
__m128d _mm_mask_cvt_roundss_sd(__m128d, __mmask8, __m128d, __m128, const int);
__m128 _mm_mask_cvtsd_ss(__m128, __mmask8, __m128, __m128d);
__m128d _mm_mask_cvtss_sd(__m128d, __mmask8, __m128d, __m128);
__m128d _mm_mask_div_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_div_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_div_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_div_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_fixupimm_round_sd(__m128d, __mmask8, __m128d, __m128i, const int, const int);
__m128 _mm_mask_fixupimm_round_ss(__m128, __mmask8, __m128, __m128i, const int, const int);
__m128d _mm_mask_fixupimm_sd(__m128d, __mmask8, __m128d, __m128i, const int);
__m128 _mm_mask_fixupimm_ss(__m128, __mmask8, __m128, __m128i, const int);
__m128d _mm_mask_fmadd_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_fmadd_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_fmadd_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_fmadd_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_fmsub_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_fmsub_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_fmsub_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_fmsub_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_fnmadd_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_fnmadd_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_fnmadd_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_fnmadd_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_fnmsub_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_fnmsub_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_fnmsub_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_fnmsub_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_getexp_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_getexp_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_getexp_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_getexp_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_getmant_round_sd(__m128d, __mmask8, __m128d, __m128d, const int, const int, const int);
__m128 _mm_mask_getmant_round_ss(__m128, __mmask8, __m128, __m128, const int, const int, const int);
__m128d _mm_mask_getmant_sd(__m128d, __mmask8, __m128d, __m128d, const int, const int);
__m128 _mm_mask_getmant_ss(__m128, __mmask8, __m128, __m128, const int, const int);
__m128d _mm_mask_load_sd(__m128d, __mmask8, const double *);
__m128 _mm_mask_load_ss(__m128, __mmask8, const float *);
__m128d _mm_mask_max_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_max_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_max_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_max_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_min_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_min_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_min_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_min_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_move_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_move_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_mul_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_mul_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_mul_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_mul_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_rcp14_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_rcp14_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_roundscale_round_sd(__m128d, __mmask8, __m128d, __m128d, const int, const int);
__m128 _mm_mask_roundscale_round_ss(__m128, __mmask8, __m128, __m128, const int, const int);
__m128d _mm_mask_roundscale_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_roundscale_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_rsqrt14_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_rsqrt14_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_scalef_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_scalef_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_scalef_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_scalef_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_mask_sqrt_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_sqrt_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_sqrt_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_sqrt_ss(__m128, __mmask8, __m128, __m128);
void _mm_mask_store_sd(double *, __mmask8, __m128d);
void _mm_mask_store_ss(float *, __mmask8, __m128);
__m128d _mm_mask_sub_round_sd(__m128d, __mmask8, __m128d, __m128d, const int);
__m128 _mm_mask_sub_round_ss(__m128, __mmask8, __m128, __m128, const int);
__m128d _mm_mask_sub_sd(__m128d, __mmask8, __m128d, __m128d);
__m128 _mm_mask_sub_ss(__m128, __mmask8, __m128, __m128);
__m128d _mm_maskz_add_round_sd(__mmask8, __m128d, __m128d, const int);
__m128 _mm_maskz_add_round_ss(__mmask8, __m128, __m128, const int);
__m128d _mm_maskz_add_sd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_add_ss(__mmask8, __m128, __m128);
__m128 _mm_maskz_cvt_roundsd_ss(__mmask8, __m128, __m128d, const int);
__m128d _mm_maskz_cvt_roundss_sd(__mmask8, __m128d, __m128, const int);
__m128 _mm_maskz_cvtsd_ss(__mmask8, __m128, __m128d);
__m128d _mm_maskz_cvtss_sd(__mmask8, __m128d, __m128);
__m128d _mm_maskz_div_round_sd(__mmask8, __m128d, __m128d, const int);
__m128 _mm_maskz_div_round_ss(__mmask8, __m128, __m128, const int);
__m128d _mm_maskz_div_sd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_div_ss(__mmask8, __m128, __m128);
__m128d _mm_maskz_fixupimm_round_sd(__mmask8, __m128d, __m128d, __m128i, const int, const int);
__m128 _mm_maskz_fixupimm_round_ss(__mmask8, __m128, __m128, __m128i, const int, const int);
__m128d _mm_maskz_fixupimm_sd(__mmask8, __m128d, __m128d, __m128i, const int);
__m128 _mm_maskz_fixupimm_ss(__mmask8, __m128, __m128, __m128i, const int);
__m128d _mm_maskz_fmadd_round_sd(__mmask8, __m128d, __m128d, __m128d, const int);
__m128 _mm_maskz_fmadd_round_ss(__mmask8, __m128, __m128, __m128, const int);
__m128d _mm_maskz_fmadd_sd(__mmask8, __m128d, __m128d, __m128d);
__m128 _mm_maskz_fmadd_ss(__mmask8, __m128, __m128, __m128);
__m128d _mm_maskz_fmsub_round_sd(__mmask8, __m128d, __m128d, __m128d, const int);
__m128 _mm_maskz_fmsub_round_ss(__mmask8, __m128, __m128, __m128, const int);
__m128d _mm_maskz_fmsub_sd(__mmask8, __m128d, __m128d, __m128d);
__m128 _mm_maskz_fmsub_ss(__mmask8, __m128, __m128, __m128);
__m128d _mm_maskz_fnmadd_round_sd(__mmask8, __m128d, __m128d, __m128d, const int);
__m128 _mm_maskz_fnmadd_round_ss(__mmask8, __m128, __m128, __m128, const int);
__m128d _mm_maskz_fnmadd_sd(__mmask8, __m128d, __m128d, __m128d);
__m128 _mm_maskz_fnmadd_ss(__mmask8, __m128, __m128, __m128);
__m128d _mm_maskz_fnmsub_round_sd(__mmask8, __m128d, __m128d, __m128d, const int);
__m128 _mm_maskz_fnmsub_round_ss(__mmask8, __m128, __m128, __m128, const int);
__m128d _mm_maskz_fnmsub_sd(__mmask8, __m128d, __m128d, __m128d);
__m128 _mm_maskz_fnmsub_ss(__mmask8, __m128, __m128, __m128);
__m128d _mm_maskz_getexp_round_sd(__mmask8, __m128d, __m128d, const int);
__m128 _mm_maskz_getexp_round_ss(__mmask8, __m128, __m128, const int);
__m128d _mm_maskz_getexp_sd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_getexp_ss(__mmask8, __m128, __m128);
__m128d _mm_maskz_getmant_round_sd(__mmask8, __m128d, __m128d, const int, const int, const int);
__m128 _mm_maskz_getmant_round_ss(__mmask8, __m128, __m128, const int, const int, const int);
__m128d _mm_maskz_getmant_sd(__mmask8, __m128d, __m128d, const int, const int);
__m128 _mm_maskz_getmant_ss(__mmask8, __m128, __m128, const int, const int);
__m128d _mm_maskz_load_sd(__mmask8, const double *);
__m128 _mm_maskz_load_ss(__mmask8, const float *);
__m128d _mm_maskz_max_round_sd(__mmask8, __m128d, __m128d, const int);
__m128 _mm_maskz_max_round_ss(__mmask8, __m128, __m128, const int);
__m128d _mm_maskz_max_sd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_max_ss(__mmask8, __m128, __m128);
__m128d _mm_maskz_min_round_sd(__mmask8, __m128d, __m128d, const int);
__m128 _mm_maskz_min_round_ss(__mmask8, __m128, __m128, const int);
__m128d _mm_maskz_min_sd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_min_ss(__mmask8, __m128, __m128);
__m128d _mm_maskz_move_sd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_move_ss(__mmask8, __m128, __m128);
__m128d _mm_maskz_mul_round_sd(__mmask8, __m128d, __m128d, const int);
__m128 _mm_maskz_mul_round_ss(__mmask8, __m128, __m128, const int);
__m128d _mm_maskz_mul_sd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_mul_ss(__mmask8, __m128, __m128);
__m128d _mm_maskz_rcp14_sd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_rcp14_ss(__mmask8, __m128, __m128);
__m128d _mm_maskz_roundscale_round_sd(__mmask8, __m128d, __m128d, const int, const int);
__m128 _mm_maskz_roundscale_round_ss(__mmask8, __m128, __m128, const int, const int);
__m128d _mm_maskz_roundscale_sd(__mmask8, __m128d, __m128d, const int);
__m128 _mm_maskz_roundscale_ss(__mmask8, __m128, __m128, const int);
__m128d _mm_maskz_rsqrt14_sd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_rsqrt14_ss(__mmask8, __m128, __m128);
__m128d _mm_maskz_scalef_round_sd(__mmask8, __m128d, __m128d, const int);
__m128 _mm_maskz_scalef_round_ss(__mmask8, __m128, __m128, const int);
__m128d _mm_maskz_scalef_sd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_scalef_ss(__mmask8, __m128, __m128);
__m128d _mm_maskz_sqrt_round_sd(__mmask8, __m128d, __m128d, const int);
__m128 _mm_maskz_sqrt_round_ss(__mmask8, __m128, __m128, const int);
__m128d _mm_maskz_sqrt_sd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_sqrt_ss(__mmask8, __m128, __m128);
__m128d _mm_maskz_sub_round_sd(__mmask8, __m128d, __m128d, const int);
__m128 _mm_maskz_sub_round_ss(__mmask8, __m128, __m128, const int);
__m128d _mm_maskz_sub_sd(__mmask8, __m128d, __m128d);
__m128 _mm_maskz_sub_ss(__mmask8, __m128, __m128);
__m128d _mm_max_round_sd(__m128d, __m128d, const int);
__m128 _mm_max_round_ss(__m128, __m128, const int);
__m128d _mm_min_round_sd(__m128d, __m128d, const int);
__m128 _mm_min_round_ss(__m128, __m128, const int);
__m128d _mm_mul_round_sd(__m128d, __m128d, const int);
__m128 _mm_mul_round_ss(__m128, __m128, const int);
__m128d _mm_rcp14_sd(__m128d, __m128d);
__m128 _mm_rcp14_ss(__m128, __m128);
__m128d _mm_roundscale_round_sd(__m128d, __m128d, const int, const int);
__m128 _mm_roundscale_round_ss(__m128, __m128, const int, const int);
__m128d _mm_roundscale_sd(__m128d, __m128d, const int);
__m128 _mm_roundscale_ss(__m128, __m128, const int);
__m128d _mm_rsqrt14_sd(__m128d, __m128d);
__m128 _mm_rsqrt14_ss(__m128, __m128);
__m128d _mm_scalef_round_sd(__m128d, __m128d, const int);
__m128 _mm_scalef_round_ss(__m128, __m128, const int);
__m128d _mm_scalef_sd(__m128d, __m128d);
__m128 _mm_scalef_ss(__m128, __m128);
__m128d _mm_sqrt_round_sd(__m128d, __m128d, const int);
__m128 _mm_sqrt_round_ss(__m128, __m128, const int);
__m128d _mm_sub_round_sd(__m128d, __m128d, const int);
__m128 _mm_sub_round_ss(__m128, __m128, const int);
void _store_mask16(__mmask16 *, __mmask16);
#ifdef __x86_64__
__m128d _mm_cvt_roundi64_sd(__m128d, long long, const int);
__m128 _mm_cvt_roundi64_ss(__m128, long long, const int);
long long _mm_cvt_roundsd_i64(__m128d, const int);
long long _mm_cvt_roundsd_si64(__m128d, const int);
unsigned long long _mm_cvt_roundsd_u64(__m128d, const int);
__m128d _mm_cvt_roundsi64_sd(__m128d, long long, const int);
__m128 _mm_cvt_roundsi64_ss(__m128, long long, const int);
long long _mm_cvt_roundss_i64(__m128, const int);
long long _mm_cvt_roundss_si64(__m128, const int);
unsigned long long _mm_cvt_roundss_u64(__m128, const int);
__m128d _mm_cvt_roundu64_sd(__m128d, unsigned long long, const int);
__m128 _mm_cvt_roundu64_ss(__m128, unsigned long long, const int);
__m128d _mm_cvti64_sd(__m128d, long long);
__m128 _mm_cvti64_ss(__m128, long long);
long long _mm_cvtsd_i64(__m128d);
unsigned long long _mm_cvtsd_u64(__m128d);
long long _mm_cvtss_i64(__m128);
unsigned long long _mm_cvtss_u64(__m128);
long long _mm_cvtt_roundsd_i64(__m128d, const int);
long long _mm_cvtt_roundsd_si64(__m128d, const int);
unsigned long long _mm_cvtt_roundsd_u64(__m128d, const int);
long long _mm_cvtt_roundss_i64(__m128, const int);
long long _mm_cvtt_roundss_si64(__m128, const int);
unsigned long long _mm_cvtt_roundss_u64(__m128, const int);
long long _mm_cvttsd_i64(__m128d);
unsigned long long _mm_cvttsd_u64(__m128d);
long long _mm_cvttss_i64(__m128);
unsigned long long _mm_cvttss_u64(__m128);
__m128d _mm_cvtu64_sd(__m128d, unsigned long long);
__m128 _mm_cvtu64_ss(__m128, unsigned long long);
#endif
/* @generated end */

#endif
#endif /* _CINRS_AVX512FINTRIN_H */
