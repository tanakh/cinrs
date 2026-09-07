/* Iterative radix-2 Cooley-Tukey FFT, written out over two `double` arrays.
 *
 * Bit-reversal permutation and then log2(n) butterfly passes. Double-precision
 * arithmetic with a strided access pattern, no library calls in the inner
 * loops.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <math.h>
#include <stdio.h>
#include <stdlib.h>

static void fft(double *re, double *im, long n, int inverse) {
    long i, j, len;
    /* bit reversal */
    for (i = 1, j = 0; i < n; i++) {
        long bit = n >> 1;
        for (; j & bit; bit >>= 1) j ^= bit;
        j ^= bit;
        if (i < j) {
            double t = re[i];
            re[i] = re[j];
            re[j] = t;
            t = im[i];
            im[i] = im[j];
            im[j] = t;
        }
    }
    for (len = 2; len <= n; len <<= 1) {
        double ang = 2.0 * 3.14159265358979323846 / (double) len * (inverse ? 1.0 : -1.0);
        double wr = cos(ang), wi = sin(ang);
        for (i = 0; i < n; i += len) {
            double cr = 1.0, ci = 0.0;
            for (j = 0; j < len / 2; j++) {
                double ur = re[i + j], ui = im[i + j];
                double vr = re[i + j + len / 2] * cr - im[i + j + len / 2] * ci;
                double vi = re[i + j + len / 2] * ci + im[i + j + len / 2] * cr;
                double nr;
                re[i + j] = ur + vr;
                im[i + j] = ui + vi;
                re[i + j + len / 2] = ur - vr;
                im[i + j + len / 2] = ui - vi;
                nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
    }
    if (inverse) {
        for (i = 0; i < n; i++) {
            re[i] /= (double) n;
            im[i] /= (double) n;
        }
    }
}

int main(int argc, char **argv) {
    int logn = (argc > 1) ? atoi(argv[1]) : 20;
    int reps = (argc > 2) ? atoi(argv[2]) : 4;
    long n = 1L << logn;
    double *re = malloc((size_t) n * sizeof(double));
    double *im = malloc((size_t) n * sizeof(double));
    double err = 0.0, energy = 0.0;
    unsigned state = 4242u;
    long i;
    int r;

    if (!re || !im) {
        fprintf(stderr, "fft: out of memory\n");
        return 1;
    }
    for (i = 0; i < n; i++) {
        state = state * 1664525u + 1013904223u;
        re[i] = (double) (state >> 8) / 16777216.0 - 0.5;
        im[i] = 0.0;
    }

    for (r = 0; r < reps; r++) {
        fft(re, im, n, 0);
        for (i = 0; i < n; i += 1024) energy += re[i] * re[i] + im[i] * im[i];
        fft(re, im, n, 1);
    }

    state = 4242u;
    for (i = 0; i < n; i++) {
        double want;
        state = state * 1664525u + 1013904223u;
        want = (double) (state >> 8) / 16777216.0 - 0.5;
        err += fabs(re[i] - want);
    }

    printf("fft n=%ld reps=%d energy=%.6f roundtrip_err=%.3e\n", n, reps, energy, err / (double) n);
    free(re);
    free(im);
    return 0;
}
