/* `strlen`, `strcmp`, `memcpy` and `memcmp` in a loop, through the C library.
 *
 * Every one of these is a call into the platform's libc, which both back ends
 * link against — so this is a control: the times should be the same to within
 * noise, and a difference here is call overhead and nothing else. GCC also
 * expands some of them inline for constant sizes, which is why the sizes are
 * run-time values.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    long len = (argc > 1) ? atol(argv[1]) : 4096L;
    long reps = (argc > 2) ? atol(argv[2]) : 300000L;
    char *a = malloc((size_t) len + 1);
    char *b = malloc((size_t) len + 1);
    unsigned state = 5u;
    long i, r, lensum = 0, cmpsum = 0, memsum = 0;

    if (!a || !b) {
        fprintf(stderr, "libc_str: out of memory\n");
        return 1;
    }
    for (i = 0; i < len; i++) {
        state = state * 1664525u + 1013904223u;
        a[i] = (char) ('a' + (state >> 24) % 26u);
    }
    a[len] = '\0';
    memcpy(b, a, (size_t) len + 1);

    /* Only the *sign* of `strcmp` and `memcmp` is specified, and gcc's inline
     * expansions do not always agree with glibc's return value on magnitude,
     * so the checksum takes the sign and the comparison stays meaningful. */
    for (r = 0; r < reps; r++) {
        int c, m;
        b[r % len] = a[r % len];
        lensum += (long) strlen(a) + (long) strlen(b);
        c = strcmp(a, b);
        cmpsum += (c > 0) - (c < 0);
        memcpy(b, a, (size_t) len);
        m = memcmp(a, b, (size_t) len);
        memsum += (m > 0) - (m < 0);
        b[(r * 7) % len] = (char) ('A' + (r % 26));
    }

    printf("libc_str len=%ld reps=%ld lensum=%ld cmpsum=%ld memsum=%ld\n", len, reps, lensum,
           cmpsum, memsum);
    free(a);
    free(b);
    return 0;
}
