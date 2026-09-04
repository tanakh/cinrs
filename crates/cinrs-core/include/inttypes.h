/* <inttypes.h> — format conversion of integer types (C99 7.8).
 *
 * The length modifiers follow the same choice <stdint.h> makes: a 64-bit type
 * is `long` wherever `long` is 64 bits wide, so its modifier is `l` rather
 * than `ll`.
 */
#ifndef _CINRS_INTTYPES_H
#define _CINRS_INTTYPES_H

#include <stdint.h>

#if __SIZEOF_LONG__ == 8
#define __CINRS_PRI64 "l"
#else
#define __CINRS_PRI64 "ll"
#endif

#if __SIZEOF_POINTER__ == 8
#define __CINRS_PRIPTR __CINRS_PRI64
#else
#define __CINRS_PRIPTR ""
#endif

#define PRId8 "d"
#define PRId16 "d"
#define PRId32 "d"
#define PRId64 __CINRS_PRI64 "d"
#define PRIi8 "i"
#define PRIi16 "i"
#define PRIi32 "i"
#define PRIi64 __CINRS_PRI64 "i"
#define PRIu8 "u"
#define PRIu16 "u"
#define PRIu32 "u"
#define PRIu64 __CINRS_PRI64 "u"
#define PRIo8 "o"
#define PRIo16 "o"
#define PRIo32 "o"
#define PRIo64 __CINRS_PRI64 "o"
#define PRIx8 "x"
#define PRIx16 "x"
#define PRIx32 "x"
#define PRIx64 __CINRS_PRI64 "x"
#define PRIX8 "X"
#define PRIX16 "X"
#define PRIX32 "X"
#define PRIX64 __CINRS_PRI64 "X"

#define PRIdLEAST8 PRId8
#define PRIdLEAST16 PRId16
#define PRIdLEAST32 PRId32
#define PRIdLEAST64 PRId64
#define PRIuLEAST8 PRIu8
#define PRIuLEAST16 PRIu16
#define PRIuLEAST32 PRIu32
#define PRIuLEAST64 PRIu64
#define PRIxLEAST8 PRIx8
#define PRIxLEAST16 PRIx16
#define PRIxLEAST32 PRIx32
#define PRIxLEAST64 PRIx64

#define PRIdPTR __CINRS_PRIPTR "d"
#define PRIiPTR __CINRS_PRIPTR "i"
#define PRIuPTR __CINRS_PRIPTR "u"
#define PRIoPTR __CINRS_PRIPTR "o"
#define PRIxPTR __CINRS_PRIPTR "x"
#define PRIXPTR __CINRS_PRIPTR "X"

#define PRIdMAX PRId64
#define PRIiMAX PRIi64
#define PRIuMAX PRIu64
#define PRIoMAX PRIo64
#define PRIxMAX PRIx64
#define PRIXMAX PRIX64

#define SCNd8 "hhd"
#define SCNd16 "hd"
#define SCNd32 "d"
#define SCNd64 __CINRS_PRI64 "d"
#define SCNi8 "hhi"
#define SCNi16 "hi"
#define SCNi32 "i"
#define SCNi64 __CINRS_PRI64 "i"
#define SCNu8 "hhu"
#define SCNu16 "hu"
#define SCNu32 "u"
#define SCNu64 __CINRS_PRI64 "u"
#define SCNx8 "hhx"
#define SCNx16 "hx"
#define SCNx32 "x"
#define SCNx64 __CINRS_PRI64 "x"
#define SCNdMAX SCNd64
#define SCNuMAX SCNu64
#define SCNxMAX SCNx64
#define SCNdPTR __CINRS_PRIPTR "d"
#define SCNuPTR __CINRS_PRIPTR "u"
#define SCNxPTR __CINRS_PRIPTR "x"

intmax_t imaxabs(intmax_t j);
intmax_t strtoimax(const char *nptr, char **endptr, int base);
uintmax_t strtoumax(const char *nptr, char **endptr, int base);

#endif /* _CINRS_INTTYPES_H */
