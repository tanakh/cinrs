/* The control for `statemachine.c`: the same lexer, over the same input,
 * written with structured control flow instead of `goto`.
 *
 * `statemachine.c` is twelve labels and a `goto` out of each of them, which is
 * the shape `cinrs` lowers into a `loop { match block { … } }` over basic
 * blocks. This file computes exactly the same counts with `while` and
 * `switch`, so nothing in it needs that lowering. It prints the same line as
 * `statemachine.c` for the same arguments, and the difference between the two
 * rows is what the `goto` lowering costs.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    long size = (argc > 1) ? atol(argv[1]) : 8000000L;
    int reps = (argc > 2) ? atoi(argv[2]) : 30;
    char *text = malloc((size_t) size + 1);
    unsigned state = 1234u;
    long i;
    int r;
    long words = 0, numbers = 0, strings = 0, comments = 0, punct = 0;
    const char *p;

    if (!text) {
        fprintf(stderr, "statemachine: out of memory\n");
        return 1;
    }
    for (i = 0; i < size; i++) {
        static const char alphabet[] = "abcdefgh0123456789 \t\n\"/*+-=;()";
        state = state * 1664525u + 1013904223u;
        text[i] = alphabet[(state >> 16) % (sizeof alphabet - 1)];
    }
    text[size] = '\0';

    for (r = 0; r < reps; r++) {
        words = numbers = strings = comments = punct = 0;
        p = text;

        while (*p) {
            char c = *p;
            if (c == ' ' || c == '\t' || c == '\n') {
                p++;
            } else if (c >= 'a' && c <= 'z') {
                do {
                    p++;
                } while ((*p >= 'a' && *p <= 'z') || (*p >= '0' && *p <= '9'));
                words++;
            } else if (c >= '0' && c <= '9') {
                do {
                    p++;
                } while (*p >= '0' && *p <= '9');
                numbers++;
            } else if (c == '"') {
                int closed = 0;
                do {
                    p++;
                    if (*p == '\0') break;
                    if (*p == '"') {
                        p++;
                        closed = 1;
                    }
                } while (!closed);
                if (closed) {
                    strings++;
                } else {
                    strings--;
                    break;
                }
            } else if (c == '/') {
                p++;
                if (*p != '*') {
                    punct++;
                } else {
                    int closed = 0;
                    p++;
                    while (*p) {
                        if (*p != '*') {
                            p++;
                            continue;
                        }
                        p++;
                        if (*p == '/') {
                            p++;
                            closed = 1;
                            break;
                        }
                    }
                    if (closed) {
                        comments++;
                    } else {
                        break;
                    }
                }
            } else {
                punct++;
                p++;
            }
        }
    }

    printf("statemachine size=%ld words=%ld numbers=%ld strings=%ld comments=%ld punct=%ld\n", size,
           words, numbers, strings, comments, punct);
    free(text);
    return 0;
}
