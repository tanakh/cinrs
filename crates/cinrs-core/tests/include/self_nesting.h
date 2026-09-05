/* A header that includes itself, guarded by `__COUNTER__`.
 *
 * C23 5.2.5.2p1 asks an implementation to accept fifteen nesting levels of
 * `#include`d files; this is the shortest way to write them. The counter is
 * one per translation unit and never repeats, so the recursion stops after
 * fifteen levels however many times the header is included.
 */
#if __COUNTER__ < 15
#include "self_nesting.h"
#endif

/* Something to declare at every level, under a name of its own. */
#define CINRS_NEST_CAT_(a, b) a##b
#define CINRS_NEST_CAT(a, b) CINRS_NEST_CAT_(a, b)
extern int CINRS_NEST_CAT(cinrs_nesting_level_, __COUNTER__);
