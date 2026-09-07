/* The first `chain.h` on the search path.
 *
 * It answers first and then reaches past itself with `#include_next`, which is
 * the shape a platform header that wraps another one has: define what this
 * copy adds, then chain to the next copy of the same name.
 */
#ifndef CHAIN_A_H
#define CHAIN_A_H

#define CHAIN_A 1

#if __has_include_next(<chain.h>)
#define CHAIN_A_SEES_A_NEXT 1
#endif

#include_next <chain.h>

/* Written after the chain, so it can be built out of what the next one gave. */
#define CHAIN_SUM (CHAIN_A + CHAIN_B)

#endif /* CHAIN_A_H */
