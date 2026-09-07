/* A binary search loop over a sorted array.
 *
 * Unpredictable branches and a random-ish access pattern, with a tiny amount
 * of arithmetic between the loads: the memory system and the branch predictor
 * do the work, so a back end that generates the same loads should land on the
 * same time.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    long n = (argc > 1) ? atol(argv[1]) : 2000000L;
    long queries = (argc > 2) ? atol(argv[2]) : 20000000L;
    int *a = malloc((size_t) n * sizeof(int));
    unsigned state = 777u;
    long hits = 0, sum = 0, q;
    long i;

    if (!a) {
        fprintf(stderr, "binsearch: out of memory\n");
        return 1;
    }
    for (i = 0; i < n; i++) a[i] = (int) (i * 3 + 1);

    for (q = 0; q < queries; q++) {
        int key;
        long lo = 0, hi = n - 1, found = -1;
        state = state * 1664525u + 1013904223u;
        key = (int) ((state >> 8) % (unsigned long) (3 * n));
        while (lo <= hi) {
            long mid = lo + (hi - lo) / 2;
            if (a[mid] == key) {
                found = mid;
                break;
            }
            if (a[mid] < key)
                lo = mid + 1;
            else
                hi = mid - 1;
        }
        if (found >= 0) {
            hits++;
            sum += found;
        }
    }

    printf("binsearch n=%ld queries=%ld hits=%ld sum=%ld\n", n, queries, hits, sum);
    free(a);
    return 0;
}
