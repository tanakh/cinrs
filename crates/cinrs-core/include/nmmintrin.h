/* <nmmintrin.h> — the SSE4.2 intrinsics: `_mm_crc32_*` and the string
 * comparisons.
 *
 * Needs `__attribute__((target("sse4.2")))` on the function that calls them;
 * see <xmmintrin.h>.
 */
#ifndef _CINRS_NMMINTRIN_H
#define _CINRS_NMMINTRIN_H

#if !defined(__i386__) && !defined(__x86_64__)
#error "the Intel intrinsics headers are x86 only; this unit is being translated for another architecture. Guard the #include with #ifdef __x86_64__, or see doc/features.md, 'SIMD intrinsics'."
#else

#include <smmintrin.h>

/* The `_mm_cmp?str*` control bits, which `core::arch` carries as constants of
 * its own. */
/* @generated constants — see crates/cinrs-core/tests/x86_intrinsics.rs */
#define _SIDD_UBYTE_OPS 0x0000
#define _SIDD_UWORD_OPS 0x0001
#define _SIDD_SBYTE_OPS 0x0002
#define _SIDD_SWORD_OPS 0x0003
#define _SIDD_CMP_EQUAL_ANY 0x0000
#define _SIDD_CMP_RANGES 0x0004
#define _SIDD_CMP_EQUAL_EACH 0x0008
#define _SIDD_CMP_EQUAL_ORDERED 0x000c
#define _SIDD_POSITIVE_POLARITY 0x0000
#define _SIDD_NEGATIVE_POLARITY 0x0010
#define _SIDD_MASKED_POSITIVE_POLARITY 0x0020
#define _SIDD_MASKED_NEGATIVE_POLARITY 0x0030
#define _SIDD_LEAST_SIGNIFICANT 0x0000
#define _SIDD_MOST_SIGNIFICANT 0x0040
#define _SIDD_BIT_MASK 0x0000
#define _SIDD_UNIT_MASK 0x0040
/* @generated end */

/* @generated sse4.2 — see crates/cinrs-core/tests/x86_intrinsics.rs */
int _mm_cmpestra(__m128i, int, __m128i, int, const int);
int _mm_cmpestrc(__m128i, int, __m128i, int, const int);
int _mm_cmpestri(__m128i, int, __m128i, int, const int);
__m128i _mm_cmpestrm(__m128i, int, __m128i, int, const int);
int _mm_cmpestro(__m128i, int, __m128i, int, const int);
int _mm_cmpestrs(__m128i, int, __m128i, int, const int);
int _mm_cmpestrz(__m128i, int, __m128i, int, const int);
__m128i _mm_cmpgt_epi64(__m128i, __m128i);
int _mm_cmpistra(__m128i, __m128i, const int);
int _mm_cmpistrc(__m128i, __m128i, const int);
int _mm_cmpistri(__m128i, __m128i, const int);
__m128i _mm_cmpistrm(__m128i, __m128i, const int);
int _mm_cmpistro(__m128i, __m128i, const int);
int _mm_cmpistrs(__m128i, __m128i, const int);
int _mm_cmpistrz(__m128i, __m128i, const int);
unsigned int _mm_crc32_u16(unsigned int, unsigned short);
unsigned int _mm_crc32_u32(unsigned int, unsigned int);
unsigned int _mm_crc32_u8(unsigned int, unsigned char);
#ifdef __x86_64__
unsigned long long _mm_crc32_u64(unsigned long long, unsigned long long);
#endif
/* @generated end */

#endif /* x86 */
#endif /* _CINRS_NMMINTRIN_H */
