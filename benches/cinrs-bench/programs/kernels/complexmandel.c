/* The Mandelbrot set computed with C's `double _Complex`.
 *
 * `float _Complex` and `double _Complex` are `cinrs::rt::Complex<f32>` and
 * `Complex<f64>` — `num_complex::Complex`, a `#[repr(C)]` pair — and the
 * arithmetic is C's, Annex G's infinity recovery included. That recovery is
 * exactly what makes complex *multiplication* more than four multiplies and a
 * couple of adds: this kernel measures whether the recovery path costs
 * anything when it is not taken. gcc and clang do the same thing (both follow
 * Annex G by default), so the three outputs must agree bit for bit; the
 * harness checks that they do.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <complex.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    int n = (argc > 1) ? atoi(argv[1]) : 1200;
    int maxiter = (argc > 2) ? atoi(argv[2]) : 200;
    long inside = 0, total = 0;
    double sum = 0.0;
    int px, py;

    for (py = 0; py < n; py++) {
        for (px = 0; px < n; px++) {
            double _Complex c = (2.0 * px / n - 1.5) + (2.0 * py / n - 1.0) * I;
            double _Complex z = 0.0;
            int i;
            for (i = 0; i < maxiter; i++) {
                z = z * z + c;
                if (__real__ z * __real__ z + __imag__ z * __imag__ z > 4.0) break;
            }
            total += i;
            if (i == maxiter) inside++;
            sum += __real__ z * 1e-9;
        }
    }

    printf("complexmandel n=%d maxiter=%d inside=%ld iters=%ld sum=%.9f\n", n, maxiter, inside,
           total, sum);
    return 0;
}
