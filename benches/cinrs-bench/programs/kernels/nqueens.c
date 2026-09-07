/* N-queens by the three-bitmask backtracking search.
 *
 * Integer bit twiddling and recursion, with no memory traffic to speak of.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

static unsigned all;

static long solve(unsigned cols, unsigned diag1, unsigned diag2) {
    if (cols == all) return 1;
    long count = 0;
    unsigned open = ~(cols | diag1 | diag2) & all;
    while (open) {
        unsigned bit = open & (0u - open); /* lowest set bit, unsigned so it wraps */
        open -= bit;
        count += solve(cols | bit, (diag1 | bit) << 1, (diag2 | bit) >> 1);
    }
    return count;
}

int main(int argc, char **argv) {
    int n = (argc > 1) ? atoi(argv[1]) : 13;
    long total;

    if (n < 1 || n > 31) {
        fprintf(stderr, "nqueens: n out of range\n");
        return 1;
    }
    all = (n == 31) ? 0x7fffffffu : ((1u << n) - 1u);
    total = solve(0, 0, 0);

    printf("nqueens n=%d solutions=%ld\n", n, total);
    return 0;
}
