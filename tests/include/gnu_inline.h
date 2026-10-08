/* An inline-only definition the way a system header writes one — macOS's
   `__header_inline` and glibc's `__extern_inline` are both `extern __inline
   __attribute__((__gnu_inline__))` — included by two exported units. Under
   GNU89's rules, which `gnu_inline` asks for, it provides no external
   definition, so the two units do not define the symbol twice. */
#ifndef CINRS_TEST_GNU_INLINE_H
#define CINRS_TEST_GNU_INLINE_H

#define CINRS_TEST_EXTERN_INLINE extern __inline __attribute__((__gnu_inline__))

CINRS_TEST_EXTERN_INLINE int cinrs_test_check_fd(int fd) {
    return fd >= 0 && fd < 1024;
}

/* A C99 inline definition, which provides none either. */
inline int cinrs_test_twice(int x) { return 2 * x; }

#endif
