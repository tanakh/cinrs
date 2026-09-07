/* The control for `vla.c`: the same computation with one `malloc` outside the
 * loop instead of a variable length array inside it.
 *
 * It prints the same line as `vla.c` for the same arguments, so the two are
 * directly comparable, and the difference between them is what a per-iteration
 * VLA costs on each of the three back ends.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

static long work(int n, long iters, int *tmp) {
    long total = 0, k;
    for (k = 0; k < iters; k++) {
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
    int *tmp;
    long total;

    if (n < 1) {
        fprintf(stderr, "vla_hoisted: n out of range\n");
        return 1;
    }
    tmp = malloc((size_t) n * sizeof(int));
    if (!tmp) {
        fprintf(stderr, "vla_hoisted: out of memory\n");
        return 1;
    }
    total = work(n, iters, tmp);
    printf("vla n=%d iters=%ld total=%ld\n", n, iters, total);
    free(tmp);
    return 0;
}
