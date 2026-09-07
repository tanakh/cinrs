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
 *
 * `struct timespec` (C11 7.27.1) is `{ time_t; long; }` on all three, which is
 * eight bytes on a 32-bit Unix and sixteen everywhere else. It is here rather
 * than in `<threads.h>` alone because C puts it here, and `thrd_sleep`,
 * `mtx_timedlock` and `cnd_timedwait` all take one.
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

/* C11 7.27.1p3. `tv_nsec` is `long` on glibc, musl, Apple's library and the
 * Microsoft one alike — glibc spells it `__syscall_slong_t`, which is `long`
 * on every target this crate models. */
struct timespec {
    time_t tv_sec;
    long tv_nsec;
};

/* The one time base C requires, and the value all four libraries give it. */
#define TIME_UTC 1

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

/* C11 7.27.2.5. glibc has had it since 2.16, musl since 1.1.5, Apple since
 * macOS 10.15 and the Microsoft UCRT since Visual Studio 2015. */
int timespec_get(struct timespec *ts, int base);

#endif /* _CINRS_TIME_H */
