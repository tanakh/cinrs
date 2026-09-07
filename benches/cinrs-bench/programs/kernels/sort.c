/* Quicksort and heapsort over an array of `int`.
 *
 * Branchy integer code with a lot of loads and stores: swaps, a partition
 * loop with two moving cursors, and a sift-down with an index computed as
 * `2 * i + 1`.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

static void fill(int *a, long n, unsigned seed) {
    unsigned state = seed;
    long i;
    for (i = 0; i < n; i++) {
        state = state * 1664525u + 1013904223u;
        a[i] = (int) (state >> 3);
    }
}

static void quicksort(int *a, long lo, long hi) {
    while (lo < hi) {
        int pivot = a[lo + (hi - lo) / 2];
        long i = lo, j = hi;
        while (i <= j) {
            while (a[i] < pivot) i++;
            while (a[j] > pivot) j--;
            if (i <= j) {
                int t = a[i];
                a[i] = a[j];
                a[j] = t;
                i++;
                j--;
            }
        }
        if (j - lo < hi - i) {
            quicksort(a, lo, j);
            lo = i;
        } else {
            quicksort(a, i, hi);
            hi = j;
        }
    }
}

static void sift(int *a, long root, long n) {
    while (2 * root + 1 < n) {
        long child = 2 * root + 1;
        if (child + 1 < n && a[child] < a[child + 1]) child++;
        if (a[root] >= a[child]) return;
        {
            int t = a[root];
            a[root] = a[child];
            a[child] = t;
        }
        root = child;
    }
}

static void heapsort(int *a, long n) {
    long i;
    for (i = n / 2 - 1; i >= 0; i--) sift(a, i, n);
    for (i = n - 1; i > 0; i--) {
        int t = a[0];
        a[0] = a[i];
        a[i] = t;
        sift(a, 0, i);
    }
}

static long checksum(const int *a, long n) {
    long s = 0, i;
    for (i = 0; i < n; i++) s += (long) a[i] % (i + 7);
    return s;
}

int main(int argc, char **argv) {
    long n = (argc > 1) ? atol(argv[1]) : 3000000L;
    int reps = (argc > 2) ? atoi(argv[2]) : 1;
    int *a = malloc((size_t) n * sizeof(int));
    long qsum = 0, hsum = 0;
    int r, ordered = 1;
    long i;

    if (!a) {
        fprintf(stderr, "sort: out of memory\n");
        return 1;
    }

    for (r = 0; r < reps; r++) {
        fill(a, n, 20240101u + (unsigned) r);
        quicksort(a, 0, n - 1);
        qsum = checksum(a, n);
        for (i = 1; i < n; i++)
            if (a[i - 1] > a[i]) ordered = 0;

        fill(a, n, 20240101u + (unsigned) r);
        heapsort(a, n);
        hsum = checksum(a, n);
        for (i = 1; i < n; i++)
            if (a[i - 1] > a[i]) ordered = 0;
    }

    printf("sort n=%ld ordered=%d quick=%ld heap=%ld\n", n, ordered, qsum, hsum);
    free(a);
    return 0;
}
