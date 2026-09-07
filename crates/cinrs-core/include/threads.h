/* <threads.h> — the C11 thread support library (C11 7.26).
 *
 * These are the platform's own threads: `thrd_create` is the C library's
 * `thrd_create`, and the objects declared below are laid out the way that
 * library lays them out, so a `mtx_t` an expansion of `cinrs` puts on the
 * stack is a `pthread_mutex_t` the library can lock. That is the whole point
 * of the header, and it is also why it is *not* one type-for-type table like
 * `<stdio.h>`: two of the types are opaque blocks of bytes whose size is a
 * property of the C library rather than of C, so the branches below are by
 * library rather than by data model alone.
 *
 * # Which libraries
 *
 * * **Linux with glibc** (`__cinrs_glibc__`): `thrd_t` is glibc's `__thrd_t`,
 *   an `unsigned long`; `mtx_t` and `cnd_t` are `pthread_mutex_t`'s and
 *   `pthread_cond_t`'s sizes, written the way glibc writes them — a union of
 *   a `char` array and an alignment carrier. The functions themselves have
 *   been in glibc since **2.28** (2018); an older one has the `pthread_*`
 *   they are built on but not these names, and the link will fail.
 * * **Linux with musl** (`__cinrs_musl__`): the same three objects with musl's
 *   own sizes and alignments — its `pthread_t` is a pointer, and its mutex is
 *   forty bytes on a 32-bit target where glibc's is twenty-four.
 * * **Apple**: there is no `<threads.h>` in libSystem at all — no `thrd_*`
 *   symbols to link against — so the header refuses rather than declaring
 *   functions that are not there.
 * * **Windows**: the Microsoft UCRT has no such header either. mingw-w64
 *   supplies one on top of winpthreads, whose layouts are winpthreads' rather
 *   than the runtime's; that is a fourth model and this header does not claim
 *   it.
 * * **Everything else** — the BSDs, Android's bionic, uClibc, a freestanding
 *   target: refused with the reason. FreeBSD does have `<threads.h>`, and its
 *   layouts are its own; a header that guessed at one of them would corrupt
 *   memory rather than fail to compile.
 *
 * `<threads.h>` working is what decides `__STDC_NO_THREADS__`: the macro is
 * predefined exactly on the targets where this header refuses, which is C11
 * 6.10.8.3's way of saying the feature is not there.
 *
 * # What is deliberately left out
 *
 * `thrd_sleep`, `mtx_timedlock` and `cnd_timedwait` take a `struct timespec`,
 * which is in `<time.h>` where C puts it. Nothing here declares a
 * `pthread_*` type: a program that wants those wants the platform's own
 * `<pthread.h>`, through `#pragma cinrs include_path`.
 */
#ifndef _CINRS_THREADS_H
#define _CINRS_THREADS_H

#if defined(_WIN32)

/* Every declaration is in the branch below, so that the one diagnostic a
 * refused target gets is this one and not a cascade of undeclared types. */
#error "<threads.h> is not available on Windows: the Microsoft UCRT has no header of this name and exports no thrd_* symbols. mingw-w64 provides one on top of winpthreads, whose mtx_t and cnd_t layouts are winpthreads' own and are not modelled here; use <windows.h> or the winpthreads header through '#pragma cinrs include_path'."

#elif defined(__APPLE__)

#error "<threads.h> is not available on Apple's platforms: libSystem implements POSIX threads but not C11's, and there are no thrd_* symbols to link against. Use <pthread.h> through '#pragma cinrs include_path'."

#elif defined(__cinrs_glibc__) || defined(__cinrs_musl__)

/* `struct timespec`, which three of the functions below take. */
#include <time.h>

/* C11 7.26.1p3: the header defines `thread_local` as `_Thread_local`. C23
 * (N2934) made `thread_local` a keyword instead, so defining it there would be
 * defining a keyword as a macro; glibc's own header has the same `#if`. */
#if !defined(__STDC_VERSION__) || __STDC_VERSION__ <= 201710L
#define thread_local _Thread_local
#endif

/* C11 7.26.6.1p3: how many times a thread's storage is swept for values whose
 * destructor set a new one. Four on glibc, musl and POSIX alike. */
#define TSS_DTOR_ITERATIONS 4

/* -- the objects ---------------------------------------------------------- */

/* glibc spells `once_flag` as a one-member `struct { int __data; }` and musl
 * as a plain `int`; both are four bytes holding zero to begin with, and the
 * flag is only ever passed by address, so the two are the same object to a
 * caller. `int` is the spelling that lets `once_flag f = ONCE_FLAG_INIT;`
 * work without braces, which is what programs write. */
typedef int once_flag;
#define ONCE_FLAG_INIT 0

#if defined(__cinrs_musl__)
/* musl's `thrd_t` is its `pthread_t`, a pointer to an incomplete structure. */
typedef struct __cinrs_musl_thread *thrd_t;
#else
/* glibc's `__thrd_t`. */
typedef unsigned long thrd_t;
#endif

/* glibc's `__tss_t` and musl's `pthread_key_t` are both `unsigned int`. */
typedef unsigned int tss_t;

typedef void (*tss_dtor_t)(void *);
typedef int (*thrd_start_t)(void *);

/* The size of the mutex, which is the C library's `sizeof(pthread_mutex_t)`.
 *
 * glibc: forty bytes on a 64-bit target — five `int`s, two `short`s and a
 * two-pointer list node — twenty-four on a 32-bit one, and thirty-two on the
 * x32 ABI, which keeps the 64-bit `struct` with 32-bit pointers.
 * musl: forty everywhere, its mutex being a union of ten `int`s and five
 * pointers.
 *
 * The alignment carrier is glibc's own `long`, which gives eight bytes on a
 * 64-bit target and four on a 32-bit one — the alignment musl's pointer union
 * has as well. */
#if defined(__cinrs_musl__)
#define __CINRS_SIZEOF_MTX 40
#elif defined(__x86_64__) && __SIZEOF_POINTER__ == 4
#define __CINRS_SIZEOF_MTX 32
#elif __SIZEOF_POINTER__ == 8
#define __CINRS_SIZEOF_MTX 40
#else
#define __CINRS_SIZEOF_MTX 24
#endif

typedef union {
    char __size[__CINRS_SIZEOF_MTX];
    long __align;
} mtx_t;

/* Forty-eight bytes in both libraries and on every data model.
 *
 * The carrier differs: glibc aligns `pthread_cond_t` to `long long` — its
 * internal counters are 64-bit even on a 32-bit machine — and musl aligns its
 * to a pointer. `__extension__` because `long long` is C99's and a header may
 * use it whatever entry point included it. */
#if defined(__cinrs_musl__)
typedef union {
    char __size[48];
    long __align;
} cnd_t;
#else
typedef union {
    char __size[48];
    __extension__ long long __align;
} cnd_t;
#endif

/* -- the constants -------------------------------------------------------- */

/* C11 7.26.1p5. The values are the two libraries', which agree; a program
 * that compares a return value against `thrd_success` is comparing against
 * what the real function returned. */
enum {
    thrd_success = 0,
    thrd_busy = 1,
    thrd_error = 2,
    thrd_nomem = 3,
    thrd_timedout = 4
};

/* C11 7.26.1p4: `mtx_timed` and `mtx_recursive` may be combined, which is why
 * they are powers of two in some libraries — but not in these two, where the
 * values are plain 0, 1 and 2 and `mtx_timed | mtx_recursive` is 3. */
enum {
    mtx_plain = 0,
    mtx_recursive = 1,
    mtx_timed = 2
};

/* -- threads (7.26.5) ----------------------------------------------------- */

int thrd_create(thrd_t *thr, thrd_start_t func, void *arg);
int thrd_equal(thrd_t lhs, thrd_t rhs);
thrd_t thrd_current(void);
int thrd_sleep(const struct timespec *duration, struct timespec *remaining);
void thrd_yield(void);
__cinrs_noreturn void thrd_exit(int res);
int thrd_detach(thrd_t thr);
int thrd_join(thrd_t thr, int *res);

/* -- mutexes (7.26.4) ----------------------------------------------------- */

int mtx_init(mtx_t *mtx, int type);
int mtx_lock(mtx_t *mtx);
int mtx_timedlock(mtx_t *mtx, const struct timespec *time_point);
int mtx_trylock(mtx_t *mtx);
int mtx_unlock(mtx_t *mtx);
void mtx_destroy(mtx_t *mtx);

/* -- call once (7.26.2) --------------------------------------------------- */

void call_once(once_flag *flag, void (*func)(void));

/* -- condition variables (7.26.3) ----------------------------------------- */

int cnd_init(cnd_t *cond);
int cnd_signal(cnd_t *cond);
int cnd_broadcast(cnd_t *cond);
int cnd_wait(cnd_t *cond, mtx_t *mtx);
int cnd_timedwait(cnd_t *cond, mtx_t *mtx, const struct timespec *time_point);
void cnd_destroy(cnd_t *cond);

/* -- thread-specific storage (7.26.6) ------------------------------------- */

int tss_create(tss_t *key, tss_dtor_t dtor);
void *tss_get(tss_t key);
int tss_set(tss_t key, void *val);
void tss_delete(tss_t key);

#else

#error "<threads.h> is not modelled for this target. The C11 thread types are blocks of bytes whose size belongs to the C library, and cinrs knows two: glibc (2.28 and later) and musl, both on Linux. A BSD, Android's bionic and uClibc each lay them out differently, and a header that guessed would corrupt memory rather than fail to compile; point '#pragma cinrs include_path' at the platform's own header instead."

#endif

#endif /* _CINRS_THREADS_H */
