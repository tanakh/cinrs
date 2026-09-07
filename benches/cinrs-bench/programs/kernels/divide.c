/* Integer division and remainder by run-time divisors.
 *
 * Rust's `/` and `%` panic on a zero divisor, so the generated code has a
 * check the C does not: the interesting question is whether the back end
 * removes it once it can see the divisor is non-zero, and what it costs when
 * it cannot. Every divisor here is derived from a running value, so neither
 * compiler can turn the division into a multiply-and-shift.
 *
 * Signed and unsigned are both here because they lower to different
 * instructions and, in Rust, to different checks — signed division also has to
 * rule out `INT_MIN / -1`.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    long iters = (argc > 1) ? atol(argv[1]) : 60000000L;
    unsigned long long uacc = 0, un = 0x9E3779B97F4A7C15ull;
    long sacc = 0;
    int sn = 1234567;
    double facc = 0.0, fd = 1.0;
    long i;

    for (i = 0; i < iters; i++) {
        unsigned long long ud = (un & 0xffffull) + 1ull; /* never zero */
        int sd = (int) ((i & 0x7fff) + 1);               /* never zero */
        uacc += un / ud;
        uacc += un % ud;
        un = un * 6364136223846793005ull + 1442695040888963407ull;

        sacc += sn / sd;
        sacc += sn % sd;
        sn = (int) ((sacc & 0x7fffffff) | 1);

        fd = 1.0 + (double) (i & 1023);
        facc += (double) i / fd;
    }

    printf("divide iters=%ld uacc=%llu sacc=%ld facc=%.9f\n", iters, uacc, sacc, facc);
    return 0;
}
