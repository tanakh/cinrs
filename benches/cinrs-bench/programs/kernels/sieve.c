/* Sieve of Eratosthenes over a byte array.
 *
 * A byte-store-and-load loop with a strided inner loop: the thing to look at
 * is whether `flags[j] = 0` costs more than a store through a raw pointer.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    long limit = (argc > 1) ? atol(argv[1]) : 40000000L;
    long rounds = (argc > 2) ? atol(argv[2]) : 1;
    unsigned char *flags = malloc((size_t) limit + 1);
    long count = 0, sum = 0;
    long r, i, j;

    if (!flags) {
        fprintf(stderr, "sieve: out of memory\n");
        return 1;
    }

    for (r = 0; r < rounds; r++) {
        memset(flags, 1, (size_t) limit + 1);
        flags[0] = flags[1] = 0;
        for (i = 2; i * i <= limit; i++) {
            if (flags[i]) {
                for (j = i * i; j <= limit; j += i) flags[j] = 0;
            }
        }
        count = 0;
        sum = 0;
        for (i = 2; i <= limit; i++) {
            if (flags[i]) {
                count++;
                sum += i;
            }
        }
    }

    printf("sieve limit=%ld primes=%ld sum=%ld\n", limit, count, sum);
    free(flags);
    return 0;
}
