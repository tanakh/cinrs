/* <stdint.h> — integer types of fixed width (C99 7.18).
 *
 * The mapping follows the platform's, because these types have to name the
 * same underlying types the platform's own headers name: `int64_t` is `long`
 * wherever `long` is 64 bits wide, as it is on every LP64 system, and
 * `long long` otherwise.
 */
#ifndef _CINRS_STDINT_H
#define _CINRS_STDINT_H

typedef signed char int8_t;
typedef unsigned char uint8_t;
typedef short int16_t;
typedef unsigned short uint16_t;
typedef int int32_t;
typedef unsigned int uint32_t;

#if __SIZEOF_LONG__ == 8
typedef long int64_t;
typedef unsigned long uint64_t;
#define INT64_MAX 9223372036854775807L
#define UINT64_MAX 18446744073709551615UL
#define INT64_C(value) value##L
#define UINT64_C(value) value##UL
#else
typedef long long int64_t;
typedef unsigned long long uint64_t;
#define INT64_MAX 9223372036854775807LL
#define UINT64_MAX 18446744073709551615ULL
#define INT64_C(value) value##LL
#define UINT64_C(value) value##ULL
#endif

typedef int8_t int_least8_t;
typedef int16_t int_least16_t;
typedef int32_t int_least32_t;
typedef int64_t int_least64_t;
typedef uint8_t uint_least8_t;
typedef uint16_t uint_least16_t;
typedef uint32_t uint_least32_t;
typedef uint64_t uint_least64_t;

/* The "fast" types follow the platform's choice, which on a 64-bit system is
 * a 64-bit type for everything but the 8-bit one. */
typedef int8_t int_fast8_t;
typedef uint8_t uint_fast8_t;
#if __SIZEOF_POINTER__ == 8
typedef int64_t int_fast16_t;
typedef int64_t int_fast32_t;
typedef uint64_t uint_fast16_t;
typedef uint64_t uint_fast32_t;
#else
typedef int32_t int_fast16_t;
typedef int32_t int_fast32_t;
typedef uint32_t uint_fast16_t;
typedef uint32_t uint_fast32_t;
#endif
typedef int64_t int_fast64_t;
typedef uint64_t uint_fast64_t;

#if __SIZEOF_POINTER__ == __SIZEOF_LONG__
typedef long intptr_t;
typedef unsigned long uintptr_t;
#elif __SIZEOF_POINTER__ == __SIZEOF_LONG_LONG__
typedef long long intptr_t;
typedef unsigned long long uintptr_t;
#else
typedef int intptr_t;
typedef unsigned int uintptr_t;
#endif

typedef int64_t intmax_t;
typedef uint64_t uintmax_t;

#define INT8_MIN (-128)
#define INT8_MAX 127
#define UINT8_MAX 255
#define INT16_MIN (-32768)
#define INT16_MAX 32767
#define UINT16_MAX 65535
#define INT32_MIN (-2147483647 - 1)
#define INT32_MAX 2147483647
#define UINT32_MAX 4294967295U
/* Written the long way round because 9223372036854775808 does not fit in any
 * signed type, so negating the maximum is the only way to spell the minimum. */
#define INT64_MIN (-INT64_MAX - 1)

#define INT_LEAST8_MIN INT8_MIN
#define INT_LEAST8_MAX INT8_MAX
#define UINT_LEAST8_MAX UINT8_MAX
#define INT_LEAST16_MIN INT16_MIN
#define INT_LEAST16_MAX INT16_MAX
#define UINT_LEAST16_MAX UINT16_MAX
#define INT_LEAST32_MIN INT32_MIN
#define INT_LEAST32_MAX INT32_MAX
#define UINT_LEAST32_MAX UINT32_MAX
#define INT_LEAST64_MIN INT64_MIN
#define INT_LEAST64_MAX INT64_MAX
#define UINT_LEAST64_MAX UINT64_MAX

#define INT_FAST8_MIN INT8_MIN
#define INT_FAST8_MAX INT8_MAX
#define UINT_FAST8_MAX UINT8_MAX
#if __SIZEOF_POINTER__ == 8
#define INT_FAST16_MIN INT64_MIN
#define INT_FAST16_MAX INT64_MAX
#define UINT_FAST16_MAX UINT64_MAX
#define INT_FAST32_MIN INT64_MIN
#define INT_FAST32_MAX INT64_MAX
#define UINT_FAST32_MAX UINT64_MAX
#else
#define INT_FAST16_MIN INT32_MIN
#define INT_FAST16_MAX INT32_MAX
#define UINT_FAST16_MAX UINT32_MAX
#define INT_FAST32_MIN INT32_MIN
#define INT_FAST32_MAX INT32_MAX
#define UINT_FAST32_MAX UINT32_MAX
#endif
#define INT_FAST64_MIN INT64_MIN
#define INT_FAST64_MAX INT64_MAX
#define UINT_FAST64_MAX UINT64_MAX

#if __SIZEOF_POINTER__ == 8
#define INTPTR_MIN INT64_MIN
#define INTPTR_MAX INT64_MAX
#define UINTPTR_MAX UINT64_MAX
#define PTRDIFF_MIN INT64_MIN
#define PTRDIFF_MAX INT64_MAX
#define SIZE_MAX UINT64_MAX
#else
#define INTPTR_MIN INT32_MIN
#define INTPTR_MAX INT32_MAX
#define UINTPTR_MAX UINT32_MAX
#define PTRDIFF_MIN INT32_MIN
#define PTRDIFF_MAX INT32_MAX
#define SIZE_MAX UINT32_MAX
#endif

#define INTMAX_MIN INT64_MIN
#define INTMAX_MAX INT64_MAX
#define UINTMAX_MAX UINT64_MAX

/* cinrs makes a wide character constant an `int`; see <stddef.h>. */
#define WCHAR_MIN INT32_MIN
#define WCHAR_MAX INT32_MAX
#define WINT_MIN INT32_MIN
#define WINT_MAX INT32_MAX
#define SIG_ATOMIC_MIN INT32_MIN
#define SIG_ATOMIC_MAX INT32_MAX

#define INT8_C(value) value
#define INT16_C(value) value
#define INT32_C(value) value
#define UINT8_C(value) value
#define UINT16_C(value) value
#define UINT32_C(value) value##U
#define INTMAX_C(value) INT64_C(value)
#define UINTMAX_C(value) UINT64_C(value)

#endif /* _CINRS_STDINT_H */
