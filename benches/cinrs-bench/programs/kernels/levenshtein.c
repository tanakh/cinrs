/* Levenshtein edit distance between random strings, two-row dynamic
 * programming.
 *
 * A dense `int` inner loop with three `min`s and a branch on a character
 * comparison. Both back ends turn the `min`s into conditional moves or into
 * branches, and this says which of the two the generated Rust ended up with.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

static int distance(const char *a, int la, const char *b, int lb, int *prev, int *curr) {
    int i, j;
    for (j = 0; j <= lb; j++) prev[j] = j;
    for (i = 1; i <= la; i++) {
        curr[0] = i;
        for (j = 1; j <= lb; j++) {
            int cost = (a[i - 1] == b[j - 1]) ? 0 : 1;
            int del = prev[j] + 1;
            int ins = curr[j - 1] + 1;
            int sub = prev[j - 1] + cost;
            int best = del < ins ? del : ins;
            curr[j] = best < sub ? best : sub;
        }
        {
            int *t = prev;
            prev = curr;
            curr = t;
        }
    }
    return prev[lb];
}

int main(int argc, char **argv) {
    int len = (argc > 1) ? atoi(argv[1]) : 2000;
    int pairs = (argc > 2) ? atoi(argv[2]) : 200;
    char *a = malloc((size_t) len + 1);
    char *b = malloc((size_t) len + 1);
    int *prev = malloc(((size_t) len + 1) * sizeof(int));
    int *curr = malloc(((size_t) len + 1) * sizeof(int));
    unsigned state = 31337u;
    long total = 0;
    int p, i;

    if (!a || !b || !prev || !curr) {
        fprintf(stderr, "levenshtein: out of memory\n");
        return 1;
    }

    for (p = 0; p < pairs; p++) {
        for (i = 0; i < len; i++) {
            state = state * 1664525u + 1013904223u;
            a[i] = (char) ('a' + (state >> 24) % 8u);
            state = state * 1664525u + 1013904223u;
            b[i] = (char) ('a' + (state >> 24) % 8u);
        }
        a[len] = b[len] = '\0';
        total += distance(a, len, b, len, prev, curr);
    }

    printf("levenshtein len=%d pairs=%d total=%ld\n", len, pairs, total);
    free(a);
    free(b);
    free(prev);
    free(curr);
    return 0;
}
