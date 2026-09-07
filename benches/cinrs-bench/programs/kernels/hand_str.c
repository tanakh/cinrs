/* The same work as `libc_str.c`, with the string routines written out in C.
 *
 * A hand-written `strchr`, `strlen` and `strcmp`: a byte load, a compare and a
 * pointer increment per iteration, in code the compiler must optimise itself
 * rather than hand to a hand-written assembly routine in libc. Next to
 * `libc_str.c` this separates "how fast is the platform's `strlen`" from "how
 * good is the code generated for a pointer walk".
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

static long my_strlen(const char *s) {
    const char *p = s;
    while (*p) p++;
    return p - s;
}

static const char *my_strchr(const char *s, int c) {
    while (*s) {
        if (*s == (char) c) return s;
        s++;
    }
    return (c == 0) ? s : 0;
}

static int my_strcmp(const char *a, const char *b) {
    while (*a && *a == *b) {
        a++;
        b++;
    }
    return (int) (unsigned char) *a - (int) (unsigned char) *b;
}

int main(int argc, char **argv) {
    long len = (argc > 1) ? atol(argv[1]) : 4096L;
    long reps = (argc > 2) ? atol(argv[2]) : 60000L;
    char *a = malloc((size_t) len + 1);
    char *b = malloc((size_t) len + 1);
    unsigned state = 5u;
    long i, r, lensum = 0, cmpsum = 0, chrsum = 0;

    if (!a || !b) {
        fprintf(stderr, "hand_str: out of memory\n");
        return 1;
    }
    for (i = 0; i < len; i++) {
        state = state * 1664525u + 1013904223u;
        a[i] = (char) ('a' + (state >> 24) % 26u);
        b[i] = a[i];
    }
    a[len] = b[len] = '\0';

    for (r = 0; r < reps; r++) {
        const char *hit;
        int c;
        b[r % len] = a[r % len];
        lensum += my_strlen(a) + my_strlen(b);
        c = my_strcmp(a, b);
        cmpsum += (c > 0) - (c < 0);
        hit = my_strchr(a, 'a' + (int) (r % 26));
        chrsum += hit ? (hit - a) : -1;
        b[(r * 7) % len] = (char) ('A' + (r % 26));
    }

    printf("hand_str len=%ld reps=%ld lensum=%ld cmpsum=%ld chrsum=%ld\n", len, reps, lensum,
           cmpsum, chrsum);
    free(a);
    free(b);
    return 0;
}
