/* Two pseudo-random generators driven flat out: a 32-bit LCG and a 64-bit
 * xorshift.
 *
 * Pure integer arithmetic with a loop-carried dependency and nothing else —
 * a multiply, an add and some shifts. C says unsigned arithmetic wraps, so
 * `cinrs` generates `wrapping_mul` and `wrapping_add`, which is what this
 * measures: whether a `wrapping_*` call costs anything at all next to the
 * machine instruction gcc emits.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    long iters = (argc > 1) ? atol(argv[1]) : 400000000L;
    unsigned lcg = 12345u;
    unsigned long long xs = 88172645463325252ull;
    unsigned long long acc = 0;
    long i;

    for (i = 0; i < iters; i++) {
        lcg = lcg * 1664525u + 1013904223u;
        xs ^= xs << 13;
        xs ^= xs >> 7;
        xs ^= xs << 17;
        acc += (unsigned long long) lcg ^ xs;
    }

    printf("prng iters=%ld lcg=%08x xs=%016llx acc=%016llx\n", iters, lcg, xs, acc);
    return 0;
}
