/* The C11 thread objects, reported by whichever compiler translates this.
 *
 * It is included both by `tests/c11_threads.rs`'s `c11!` block, so that cinrs
 * compiles it against the bundled `<threads.h>`, and by the `main.c` that test
 * hands to the host C compiler, so that `cc` compiles the same text against
 * the platform's own. The two reports have to be the same string: that is what
 * says the sizes, alignments and constants the bundled header declares are the
 * C library's own, on this machine, today.
 */
#ifndef CINRS_THREADS_PROBE_H
#define CINRS_THREADS_PROBE_H

#include <stddef.h>
#include <stdio.h>
#include <threads.h>
#include <time.h>

/* `%d` throughout, and every number cast to `int`: the report is compared as a
 * string, so the two sides have to agree on the format as well as the value,
 * and `int` is the one width no `%z`/`%ll` spelling can differ over. */
int thr_report(char *out) {
    char *p = out;
    /* Read as bytes rather than printed: glibc spells `once_flag` as a
     * one-member `struct`, so `ONCE_FLAG_INIT` is `{ 0 }` there and cannot be
     * cast — but the object it makes is the four zero bytes cinrs's `int` is,
     * which is the thing worth comparing. */
    once_flag flag = ONCE_FLAG_INIT;
    int flag_bytes = 0;
    {
        int i;
        for (i = 0; i < (int) sizeof(once_flag); i++)
            flag_bytes |= ((const unsigned char *) &flag)[i];
    }
    p += sprintf(p, "mtx_t %d %d\n", (int) sizeof(mtx_t), (int) _Alignof(mtx_t));
    p += sprintf(p, "cnd_t %d %d\n", (int) sizeof(cnd_t), (int) _Alignof(cnd_t));
    p += sprintf(p, "thrd_t %d %d\n", (int) sizeof(thrd_t), (int) _Alignof(thrd_t));
    p += sprintf(p, "tss_t %d %d\n", (int) sizeof(tss_t), (int) _Alignof(tss_t));
    p += sprintf(p, "once_flag %d %d\n",
                 (int) sizeof(once_flag), (int) _Alignof(once_flag));
    p += sprintf(p, "timespec %d %d %d\n",
                 (int) sizeof(struct timespec), (int) _Alignof(struct timespec),
                 (int) offsetof(struct timespec, tv_nsec));
    p += sprintf(p, "thrd %d %d %d %d %d\n",
                 thrd_success, thrd_busy, thrd_error, thrd_nomem, thrd_timedout);
    p += sprintf(p, "mtx %d %d %d\n", mtx_plain, mtx_recursive, mtx_timed);
    p += sprintf(p, "tss_dtor_iterations %d\n", TSS_DTOR_ITERATIONS);
    p += sprintf(p, "time_utc %d\n", TIME_UTC);
    p += sprintf(p, "once_flag_init_is_all_zero %d\n", flag_bytes == 0);
    return (int) (p - out);
}

#endif /* CINRS_THREADS_PROBE_H */
