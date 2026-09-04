/* <stdalign.h> — alignment (C11 7.15).
 *
 * The macros are only defined before C23, where `alignas` and `alignof`
 * became keywords in their own right; the `__*_is_defined` macros stay
 * whichever revision this is read in, because that is what a program tests.
 *
 * cinrs honours `_Alignas` on the members of a struct or union, where the
 * generated Rust type can carry the alignment; see the crate documentation
 * for the exact rule.
 */
#ifndef _CINRS_STDALIGN_H
#define _CINRS_STDALIGN_H

#if __STDC_VERSION__ < 202311L
#define alignas _Alignas
#define alignof _Alignof
#endif

#define __alignas_is_defined 1
#define __alignof_is_defined 1

#endif /* _CINRS_STDALIGN_H */
