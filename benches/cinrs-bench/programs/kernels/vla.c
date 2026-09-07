/* A variable length array made afresh on every pass through a loop.
 *
 * C99 says the bound is evaluated at the declaration and the object lives to
 * the end of the block, so a VLA inside a loop body is created and destroyed
 * once per iteration. gcc and clang move the stack pointer; Rust cannot move
 * it by an amount chosen at run time, so `cinrs` emulates the storage on the
 * heap — a `Vec` per iteration. This kernel is the price of that, measured:
 * the work inside the array is deliberately small, so what is left is the
 * allocation.
 *
 * `vla_hoisted.c` is the same computation with the array allocated once, which
 * is what the difference should be attributed to.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

static long work(int n, long iters) {
    long total = 0, k;
    for (k = 0; k < iters; k++) {
        int tmp[n];
        int i;
        for (i = 0; i < n; i++) tmp[i] = (int) ((k + i) % 251);
        for (i = 1; i < n; i++) tmp[i] += tmp[i - 1];
        total += tmp[n - 1];
    }
    return total;
}

int main(int argc, char **argv) {
    int n = (argc > 1) ? atoi(argv[1]) : 64;
    long iters = (argc > 2) ? atol(argv[2]) : 5000000L;
    long total;

    if (n < 1) {
        fprintf(stderr, "vla: n out of range\n");
        return 1;
    }
    total = work(n, iters);
    printf("vla n=%d iters=%ld total=%ld\n", n, iters, total);
    return 0;
}
