/* <stdckdint.h> — checked integer arithmetic (C23 7.20, N2683).
 *
 * The three macros store `a op b` into `*r` and answer whether the
 * mathematical result did *not* fit the type `*r` has. The arithmetic is done
 * in infinite precision, so the type of `a` and `b` decides nothing: only the
 * type pointed at by `r` does.
 *
 * They are `__builtin_add_overflow` and friends, which cinrs implements
 * directly, so `ckd_add(&d, a, b)` is exactly the GCC spelling every code base
 * already writes — which is what the standard settled on.
 */
#ifndef _CINRS_STDCKDINT_H
#define _CINRS_STDCKDINT_H

#define __STDC_VERSION_STDCKDINT_H__ 202311L

#define ckd_add(r, a, b) ((_Bool) __builtin_add_overflow((a), (b), (r)))
#define ckd_sub(r, a, b) ((_Bool) __builtin_sub_overflow((a), (b), (r)))
#define ckd_mul(r, a, b) ((_Bool) __builtin_mul_overflow((a), (b), (r)))

#endif /* _CINRS_STDCKDINT_H */
