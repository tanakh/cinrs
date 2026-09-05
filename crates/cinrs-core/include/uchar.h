/* <uchar.h> — Unicode utilities (C11 7.28, from TR 19769; N1326).
 *
 * `char16_t` and `char32_t` are the types the front end gives `u"…"` and
 * `U"…"`: `uint_least16_t` and `uint_least32_t`, which on every target cinrs
 * models are `unsigned short` and `unsigned int`. None of the three names is a
 * keyword in C — that is C++ — so all of them are typedefs here, exactly as
 * they are in a real library's header.
 *
 * `mbstate_t` and `size_t` come from <wchar.h>, which spells the first the way
 * each platform's library really lays it out; including it here is what keeps
 * a program that includes both headers from seeing two different `mbstate_t`.
 *
 * `char8_t`, `mbrtoc8` and `c8rtomb` are C23's (N2653), and are declared only
 * there.
 */
#ifndef _CINRS_UCHAR_H
#define _CINRS_UCHAR_H

#include <stddef.h>
#include <wchar.h>

typedef unsigned short char16_t;
typedef unsigned int char32_t;

#if __STDC_VERSION__ >= 202311L
typedef unsigned char char8_t;
#endif

/* -- restartable multibyte/wide string conversion (7.28.1) ------------- */

size_t mbrtoc16(char16_t *pc16, const char *s, size_t n, mbstate_t *ps);
size_t c16rtomb(char *s, char16_t c16, mbstate_t *ps);
size_t mbrtoc32(char32_t *pc32, const char *s, size_t n, mbstate_t *ps);
size_t c32rtomb(char *s, char32_t c32, mbstate_t *ps);

#if __STDC_VERSION__ >= 202311L
size_t mbrtoc8(char8_t *pc8, const char *s, size_t n, mbstate_t *ps);
size_t c8rtomb(char *s, char8_t c8, mbstate_t *ps);
#endif

#endif /* _CINRS_UCHAR_H */
