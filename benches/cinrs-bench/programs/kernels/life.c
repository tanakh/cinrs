/* Conway's Life on a fixed torus, one byte per cell.
 *
 * A nine-neighbour stencil: the inner loop is eight loads at fixed offsets
 * from a moving pointer, a sum and a store. This is where subscripting into a
 * two-dimensional array either turns into one address computation or into
 * eight.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    int n = (argc > 1) ? atoi(argv[1]) : 512;
    int gens = (argc > 2) ? atoi(argv[2]) : 300;
    unsigned char *a, *b, *t;
    unsigned state = 2024u;
    long alive = 0, i, total = (long) n * n;
    int g, x, y;

    a = malloc((size_t) total);
    b = malloc((size_t) total);
    if (!a || !b) {
        fprintf(stderr, "life: out of memory\n");
        return 1;
    }
    for (i = 0; i < total; i++) {
        state = state * 1664525u + 1013904223u;
        a[i] = (unsigned char) ((state >> 20) & 1u);
    }

    for (g = 0; g < gens; g++) {
        for (y = 0; y < n; y++) {
            int up = ((y + n - 1) % n) * n;
            int mid = y * n;
            int down = ((y + 1) % n) * n;
            for (x = 0; x < n; x++) {
                int l = (x + n - 1) % n;
                int r = (x + 1) % n;
                int c = a[up + l] + a[up + x] + a[up + r] + a[mid + l] + a[mid + r] + a[down + l] +
                        a[down + x] + a[down + r];
                b[mid + x] = (unsigned char) ((c == 3) || (c == 2 && a[mid + x]));
            }
        }
        t = a;
        a = b;
        b = t;
    }

    for (i = 0; i < total; i++) alive += a[i];
    printf("life n=%d gens=%d alive=%ld\n", n, gens, alive);
    free(a);
    free(b);
    return 0;
}
