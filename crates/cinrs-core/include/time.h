/* <time.h> — date and time (C99 7.23).
 *
 * `struct tm` is laid out the way glibc and Apple's library lay it out: the
 * nine standard members followed by the two BSD ones, which both libraries
 * have and which a program that passes a `struct tm` by address would corrupt
 * if they were missing. The Microsoft library has only the nine, and the `#if`
 * below follows it.
 *
 * `_WIN32` is the *target's*, from the model cinrs was told to translate for.
 * `time_t` is `long` on the Unix platforms, which makes it 32 bits wide on a
 * 32-bit target — glibc's own default without `_TIME_BITS=64` — and `long
 * long` on Windows.
 */
#ifndef _CINRS_TIME_H
#define _CINRS_TIME_H

#include <stddef.h>

#if defined(_WIN32)
/* `__extension__`: `long long` is C99's, and a header may use it whatever the
 * entry point is. */
__extension__ typedef long long time_t;
typedef long clock_t;
#define CLOCKS_PER_SEC 1000
#else
typedef long time_t;
typedef long clock_t;
#define CLOCKS_PER_SEC 1000000
#endif

struct tm {
    int tm_sec;
    int tm_min;
    int tm_hour;
    int tm_mday;
    int tm_mon;
    int tm_year;
    int tm_wday;
    int tm_yday;
    int tm_isdst;
#if !defined(_WIN32)
    long tm_gmtoff;
    const char *tm_zone;
#endif
};

time_t time(time_t *timer);
clock_t clock(void);
double difftime(time_t time1, time_t time0);
time_t mktime(struct tm *timeptr);

struct tm *localtime(const time_t *timer);
struct tm *gmtime(const time_t *timer);
char *asctime(const struct tm *timeptr);
char *ctime(const time_t *timer);
size_t strftime(char *s, size_t maxsize, const char *format,
                const struct tm *timeptr);

#endif /* _CINRS_TIME_H */
