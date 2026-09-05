/* <wchar.h> — extended multibyte and wide character utilities (C99 7.24).
 *
 * `wchar_t` comes from <stddef.h>, which takes it from `__WCHAR_TYPE__` and so
 * from the target model: `int` on the Unix platforms, `unsigned int` on Arm,
 * `unsigned short` on Windows. That is exactly what the front end gives `L'x'`
 * and `L"…"`, so the two cannot drift apart.
 *
 * `mbstate_t` is the one type whose *layout* a program can observe — it
 * declares one and passes its address to `mbrtowc` — so it is spelled the way
 * each platform's library really lays it out, and the `#if` below picks
 * between them.
 *
 * `wcstold` is absent for the same reason `long double` is: cinrs maps the
 * type onto `double`, and a call that returned one would be reading the wrong
 * register.
 */
#ifndef _CINRS_WCHAR_H
#define _CINRS_WCHAR_H

#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>

/* `unsigned short` on Windows, `int` on Apple's platforms, `unsigned int`
 * elsewhere — which is what `__WINT_TYPE__` already says. */
typedef __WINT_TYPE__ wint_t;

/* The range of whatever `wchar_t` turned out to be; <stdint.h> defines the
 * same two macros, and either header may be included first. */
#ifndef WCHAR_MIN
#define WCHAR_MIN __WCHAR_MIN__
#define WCHAR_MAX __WCHAR_MAX__
#endif

/* `WEOF` is a `wint_t` that is not a character. Where `wint_t` is signed it is
 * `-1`; where it is unsigned it is the all-ones value of its width, which is
 * 0xffff on Windows and 0xffffffff elsewhere. */
#if defined(__APPLE__)
#define WEOF (-1)
#elif __SIZEOF_WINT_T__ == 2
#define WEOF ((wint_t)(0xFFFF))
#else
#define WEOF ((wint_t)(0xffffffffu))
#endif

#if defined(_WIN32)
/* The Microsoft library's `_Mbstatet`: eight bytes, four-byte aligned. */
typedef struct {
    unsigned long _Wchar;
    unsigned short _Byte;
    unsigned short _State;
} mbstate_t;
#elif defined(__APPLE__)
/* Apple's `__mbstate_t` is a 128-byte buffer plus a `long long` that gives it
 * its alignment. */
typedef struct {
    char __mbstate8[128];
    __extension__ long long _mbstateL;
} mbstate_t;
#else
/* glibc's `__mbstate_t`: eight bytes, four-byte aligned. */
typedef struct {
    int __count;
    union {
        unsigned int __wch;
        char __wchb[4];
    } __value;
} mbstate_t;
#endif

/* Only `wcsftime` needs it, and only through a pointer; a program that wants
 * the members includes <time.h>, which completes this very tag. */
struct tm;

/* -- wide string handling (7.24.4) ------------------------------------- */

size_t wcslen(const wchar_t *s);
wchar_t *wcscpy(wchar_t *s1, const wchar_t *s2);
wchar_t *wcsncpy(wchar_t *s1, const wchar_t *s2, size_t n);
wchar_t *wcscat(wchar_t *s1, const wchar_t *s2);
wchar_t *wcsncat(wchar_t *s1, const wchar_t *s2, size_t n);
int wcscmp(const wchar_t *s1, const wchar_t *s2);
int wcsncmp(const wchar_t *s1, const wchar_t *s2, size_t n);
int wcscoll(const wchar_t *s1, const wchar_t *s2);
size_t wcsxfrm(wchar_t *s1, const wchar_t *s2, size_t n);
wchar_t *wcschr(const wchar_t *s, wchar_t c);
wchar_t *wcsrchr(const wchar_t *s, wchar_t c);
wchar_t *wcsstr(const wchar_t *s1, const wchar_t *s2);
wchar_t *wcstok(wchar_t *s1, const wchar_t *s2, wchar_t **ptr);
size_t wcsspn(const wchar_t *s1, const wchar_t *s2);
size_t wcscspn(const wchar_t *s1, const wchar_t *s2);
wchar_t *wcspbrk(const wchar_t *s1, const wchar_t *s2);

wchar_t *wmemcpy(wchar_t *s1, const wchar_t *s2, size_t n);
wchar_t *wmemmove(wchar_t *s1, const wchar_t *s2, size_t n);
wchar_t *wmemset(wchar_t *s, wchar_t c, size_t n);
int wmemcmp(const wchar_t *s1, const wchar_t *s2, size_t n);
wchar_t *wmemchr(const wchar_t *s, wchar_t c, size_t n);

/* -- conversions (7.24.4.1) -------------------------------------------- */

long wcstol(const wchar_t *nptr, wchar_t **endptr, int base);
unsigned long wcstoul(const wchar_t *nptr, wchar_t **endptr, int base);
__extension__ long long wcstoll(const wchar_t *nptr, wchar_t **endptr, int base);
__extension__ unsigned long long wcstoull(const wchar_t *nptr, wchar_t **endptr,
                                          int base);
double wcstod(const wchar_t *nptr, wchar_t **endptr);
float wcstof(const wchar_t *nptr, wchar_t **endptr);

/* -- multibyte/wide conversion with a state (7.24.6) ------------------- */

size_t mbrtowc(wchar_t *pwc, const char *s, size_t n, mbstate_t *ps);
size_t wcrtomb(char *s, wchar_t wc, mbstate_t *ps);
size_t mbrlen(const char *s, size_t n, mbstate_t *ps);
int mbsinit(const mbstate_t *ps);
size_t mbsrtowcs(wchar_t *dst, const char **src, size_t len, mbstate_t *ps);
size_t wcsrtombs(char *dst, const wchar_t **src, size_t len, mbstate_t *ps);
wint_t btowc(int c);
int wctob(wint_t c);

/* -- formatted wide input/output (7.24.2) ------------------------------ */

int wprintf(const wchar_t *format, ...);
int fwprintf(FILE *stream, const wchar_t *format, ...);
int swprintf(wchar_t *s, size_t n, const wchar_t *format, ...);
int vwprintf(const wchar_t *format, va_list arg);
int vfwprintf(FILE *stream, const wchar_t *format, va_list arg);
int vswprintf(wchar_t *s, size_t n, const wchar_t *format, va_list arg);
int wscanf(const wchar_t *format, ...);
int fwscanf(FILE *stream, const wchar_t *format, ...);
int swscanf(const wchar_t *s, const wchar_t *format, ...);

/* -- wide character input/output (7.24.3) ------------------------------ */

wint_t fgetwc(FILE *stream);
wint_t getwc(FILE *stream);
wint_t getwchar(void);
wint_t fputwc(wchar_t c, FILE *stream);
wint_t putwc(wchar_t c, FILE *stream);
wint_t putwchar(wchar_t c);
wint_t ungetwc(wint_t c, FILE *stream);
wchar_t *fgetws(wchar_t *s, int n, FILE *stream);
int fputws(const wchar_t *s, FILE *stream);
int fwide(FILE *stream, int mode);

/* -- wide time formatting (7.24.5) ------------------------------------- */

size_t wcsftime(wchar_t *s, size_t maxsize, const wchar_t *format,
                const struct tm *timeptr);

#endif /* _CINRS_WCHAR_H */
