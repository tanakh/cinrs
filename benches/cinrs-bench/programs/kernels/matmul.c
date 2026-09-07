/* Dense double matrix multiply over flat arrays, ikj order.
 *
 * The plain-array half of the matrix-multiply pair: every subscript is
 * `a[i * n + k]`, so the address arithmetic is explicit and the inner loop is
 * a textbook candidate for vectorisation. `matmul_vla.c` is the same
 * computation written with the C99 parameter form `double a[n][n]`.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

static void fill(double *m, int n, unsigned seed) {
    unsigned state = seed;
    int i;
    for (i = 0; i < n * n; i++) {
        state = state * 1664525u + 1013904223u;
        m[i] = (double) (state >> 8) / 16777216.0 - 0.5;
    }
}

static void mul(const double *a, const double *b, double *c, int n) {
    int i, j, k;
    for (i = 0; i < n * n; i++) c[i] = 0.0;
    for (i = 0; i < n; i++) {
        for (k = 0; k < n; k++) {
            double aik = a[i * n + k];
            const double *brow = b + k * n;
            double *crow = c + i * n;
            for (j = 0; j < n; j++) crow[j] += aik * brow[j];
        }
    }
}

int main(int argc, char **argv) {
    int n = (argc > 1) ? atoi(argv[1]) : 512;
    int reps = (argc > 2) ? atoi(argv[2]) : 1;
    double *a = malloc((size_t) n * n * sizeof(double));
    double *b = malloc((size_t) n * n * sizeof(double));
    double *c = malloc((size_t) n * n * sizeof(double));
    double trace = 0.0, sum = 0.0;
    int r, i;

    if (!a || !b || !c) {
        fprintf(stderr, "matmul: out of memory\n");
        return 1;
    }
    fill(a, n, 12345u);
    fill(b, n, 98765u);

    for (r = 0; r < reps; r++) mul(a, b, c, n);

    for (i = 0; i < n; i++) trace += c[i * n + i];
    for (i = 0; i < n * n; i++) sum += c[i];

    printf("matmul n=%d trace=%.9f sum=%.9f\n", n, trace, sum);
    free(a);
    free(b);
    free(c);
    return 0;
}
