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
 *
 * # Coexisting with the platform's own headers
 *
 * With `#pragma cinrs system_include` this header wins over the platform's
 * `<time.h>`, but the platform's *other* headers are still in play — and
 * glibc's `<pthread.h>`, `<sys/stat.h>`, `<fcntl.h>`, `<sys/time.h>` and
 * `<sys/wait.h>` all reach `bits/types/struct_timespec.h`,
 * `bits/types/struct_tm.h` and the `time_t`/`clock_t` files behind them. Each
 * of those is wrapped in a per-type guard macro, so every definition here
 * *tests and then claims* the guards glibc, musl and mingw-w64 use for the same
 * type. Claiming one is a promise that the layout below is that platform's own,
 * which is what the differential tests in `tests/system_headers.rs` check.
 *
 *   type / macro       glibc                  musl                        mingw-w64
 *   time_t             __time_t_defined       __DEFINED_time_t            _TIME_T_DEFINED
 *   clock_t            __clock_t_defined      __DEFINED_clock_t           _CLOCK_T_DEFINED
 *   clockid_t          __clockid_t_defined    __DEFINED_clockid_t         —
 *   timer_t            __timer_t_defined      __DEFINED_timer_t           —
 *   struct timespec    _STRUCT_TIMESPEC       __DEFINED_struct_timespec   _TIMESPEC_DEFINED
 *   struct tm          __struct_tm_defined    __DEFINED_struct_tm         —
 *   struct itimerspec  __itimerspec_defined   __DEFINED_struct_itimerspec —
 *
 * `_STRUCT_TIMESPEC` is also what `<linux/time.h>` tests, so claiming it keeps
 * the kernel's UAPI headers quiet as well — glibc's own file says so.
 *
 * `CLOCKS_PER_SEC` and `TIME_UTC` have no per-type guard in glibc at all:
 * `bits/time.h` defines `CLOCKS_PER_SEC` as `((__clock_t) 1000000)` behind
 * nothing but its file guard. The two spellings mean the same number, and the
 * preprocessor takes the platform's own as the authoritative one when a bundled
 * header and a platform header disagree about a macro; see `Pp::define`.
 */
#ifndef _CINRS_TIME_H
#define _CINRS_TIME_H

#include <stddef.h>

/* ---- time_t, clock_t --------------------------------------------------- */
#if !defined(__time_t_defined) && !defined(__DEFINED_time_t) \
    && !defined(_TIME_T_DEFINED)
#if defined(_WIN32)
/* `__extension__`: `long long` is C99's, and a header may use it whatever the
 * entry point is. */
__extension__ typedef long long time_t;
#else
typedef long time_t;
#endif
#define __time_t_defined 1
#define __DEFINED_time_t 1
#define _TIME_T_DEFINED 1
#endif

#if !defined(__clock_t_defined) && !defined(__DEFINED_clock_t) \
    && !defined(_CLOCK_T_DEFINED)
typedef long clock_t;
#define __clock_t_defined 1
#define __DEFINED_clock_t 1
#define _CLOCK_T_DEFINED 1
#endif

#if defined(_WIN32)
#define CLOCKS_PER_SEC 1000
#else
#define CLOCKS_PER_SEC 1000000
#endif

/* ---- struct timespec -------------------------------------------------- */
/* C11 7.27.1p3. `tv_nsec` is `long` on glibc, musl, Apple's library and the
 * Microsoft one alike — glibc spells it `__syscall_slong_t`, which is `long`
 * on every target this crate models. */
#if !defined(_STRUCT_TIMESPEC) && !defined(__DEFINED_struct_timespec) \
    && !defined(_TIMESPEC_DEFINED)
struct timespec {
    time_t tv_sec;
    long tv_nsec;
};
#define _STRUCT_TIMESPEC 1
#define __DEFINED_struct_timespec 1
#define _TIMESPEC_DEFINED 1
#endif

/* The one time base C requires, and the value all four libraries give it. */
#define TIME_UTC 1

/* ---- struct tm -------------------------------------------------------- */
#if !defined(__struct_tm_defined) && !defined(__DEFINED_struct_tm)
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
#define __struct_tm_defined 1
#define __DEFINED_struct_tm 1
#endif

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
