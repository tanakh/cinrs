/* The same matrix multiply as `matmul.c`, written with C99 variably modified
 * parameters: `void mul(int n, double a[n][n], ...)`, which adjusts to
 * `double (*a)[n]` and reads its bound on entry (6.9.1p10).
 *
 * The storage is still `malloc`, so what is being compared against `matmul.c`
 * is the *subscripting*: `a[i][k]` on a pointer-to-VLA, whose row stride is a
 * run-time value, against `a[i * n + k]` written out by hand. It prints the
 * same numbers as `matmul.c` for the same `n`.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

static void fill(int n, double m[n][n], unsigned seed) {
    unsigned state = seed;
    int i, j;
    for (i = 0; i < n; i++) {
        for (j = 0; j < n; j++) {
            state = state * 1664525u + 1013904223u;
            m[i][j] = (double) (state >> 8) / 16777216.0 - 0.5;
        }
    }
}

static void mul(int n, double a[n][n], double b[n][n], double c[n][n]) {
    int i, j, k;
    for (i = 0; i < n; i++)
        for (j = 0; j < n; j++) c[i][j] = 0.0;
    for (i = 0; i < n; i++) {
        for (k = 0; k < n; k++) {
            double aik = a[i][k];
            for (j = 0; j < n; j++) c[i][j] += aik * b[k][j];
        }
    }
}

int main(int argc, char **argv) {
    int n = (argc > 1) ? atoi(argv[1]) : 512;
    int reps = (argc > 2) ? atoi(argv[2]) : 1;
    double(*a)[n] = malloc((size_t) n * n * sizeof(double));
    double(*b)[n] = malloc((size_t) n * n * sizeof(double));
    double(*c)[n] = malloc((size_t) n * n * sizeof(double));
    double trace = 0.0, sum = 0.0;
    int r, i, j;

    if (!a || !b || !c) {
        fprintf(stderr, "matmul_vla: out of memory\n");
        return 1;
    }
    fill(n, a, 12345u);
    fill(n, b, 98765u);

    for (r = 0; r < reps; r++) mul(n, a, b, c);

    for (i = 0; i < n; i++) trace += c[i][i];
    for (i = 0; i < n; i++)
        for (j = 0; j < n; j++) sum += c[i][j];

    printf("matmul n=%d trace=%.9f sum=%.9f\n", n, trace, sum);
    free(a);
    free(b);
    free(c);
    return 0;
}
