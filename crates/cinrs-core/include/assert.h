/* <assert.h> — diagnostics (C99 7.2).
 *
 * There is deliberately no include guard: 7.2p1 says this header may be
 * included more than once, and that what `assert` means is decided by whether
 * NDEBUG is defined *at the point of inclusion*. Including it again after
 * defining NDEBUG therefore has to redefine the macro, which is what the
 * `#undef` below is for.
 *
 * The failing branch is written in terms of <stdio.h> and <stdlib.h> rather
 * than the platform's own `__assert_fail`, whose name and signature differ on
 * every library.
 */
#include <stdio.h>
#include <stdlib.h>

#undef assert

/* C11's spelling of `_Static_assert`. In C23 `static_assert` is a keyword, so
 * the macro is only defined before it. */
#if __STDC_VERSION__ >= 201112L && __STDC_VERSION__ < 202311L
#define static_assert _Static_assert
#endif

#ifdef NDEBUG
#define assert(ignore) ((void)0)
#else
#define assert(e)                                                            \
    ((e) ? (void)0                                                           \
         : (fprintf(stderr, "Assertion failed: %s, file %s, line %d\n", #e,  \
                    __FILE__, __LINE__),                                     \
            abort()))
#endif
