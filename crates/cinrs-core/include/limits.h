/* <limits.h> — sizes of integer types (C99 7.10). */
#ifndef _CINRS_LIMITS_H
#define _CINRS_LIMITS_H

#define CHAR_BIT 8
#define MB_LEN_MAX 16

#define SCHAR_MIN (-127 - 1)
#define SCHAR_MAX 127
#define UCHAR_MAX 255

/* Whether plain `char` is signed is the target's business; the preprocessor
 * predefines __CHAR_UNSIGNED__ exactly where it is not. */
#ifdef __CHAR_UNSIGNED__
#define CHAR_MIN 0
#define CHAR_MAX 255
#else
#define CHAR_MIN (-127 - 1)
#define CHAR_MAX 127
#endif

#define SHRT_MIN (-32767 - 1)
#define SHRT_MAX 32767
#define USHRT_MAX 65535

#if __SIZEOF_INT__ == 2
#define INT_MIN (-32767 - 1)
#define INT_MAX 32767
#define UINT_MAX 65535U
#else
#define INT_MIN (-2147483647 - 1)
#define INT_MAX 2147483647
#define UINT_MAX 4294967295U
#endif

#if __SIZEOF_LONG__ == 8
#define LONG_MAX 9223372036854775807L
#define ULONG_MAX 18446744073709551615UL
#else
#define LONG_MAX 2147483647L
#define ULONG_MAX 4294967295UL
#endif
#define LONG_MIN (-LONG_MAX - 1L)

#define LLONG_MAX 9223372036854775807LL
#define LLONG_MIN (-LLONG_MAX - 1LL)
#define ULLONG_MAX 18446744073709551615ULL

#endif /* _CINRS_LIMITS_H */
