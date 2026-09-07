/* The second `chain.h`, which `next-a/chain.h` reaches with `#include_next`.
 *
 * Nothing of that name comes after it on the path, so its own
 * `__has_include_next` answers no — which is the question a header at the end
 * of a chain asks to find out that it is the last one.
 */
#ifndef CHAIN_B_H
#define CHAIN_B_H

#define CHAIN_B 2

#if __has_include_next(<chain.h>)
#define CHAIN_B_SEES_A_NEXT 1
#endif

#endif /* CHAIN_B_H */
