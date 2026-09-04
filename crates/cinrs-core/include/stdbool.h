/* <stdbool.h> — boolean type and values (C99 7.16).
 *
 * In C23 `bool`, `true` and `false` are keywords, and defining macros over
 * them would both be pointless and — since a keyword cannot be redefined —
 * wrong, so the definitions below are made only before C23. The header itself
 * stays, because including it is still correct C.
 */
#ifndef _CINRS_STDBOOL_H
#define _CINRS_STDBOOL_H

#if __STDC_VERSION__ < 202311L
#define bool _Bool
#define true 1
#define false 0
#endif

#define __bool_true_false_are_defined 1

#endif /* _CINRS_STDBOOL_H */
