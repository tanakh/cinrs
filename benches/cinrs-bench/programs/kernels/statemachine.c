/* A lexer written as a `goto`-heavy state machine.
 *
 * Twelve labels and a `goto` out of each of them, jumping into one another in
 * both directions, which is the shape `cinrs` has to lower into its own state
 * machine over basic blocks: an *outward* `goto` becomes a labelled block or a
 * labelled loop and costs nothing, but these regions would have to overlap
 * without nesting, so the function becomes a `loop { match block { … } }`. A C
 * program that was already a state machine gets translated into one, and this
 * measures what the second layer costs.
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

    start:
        if (*p == '\0') goto done;
        if (*p == ' ' || *p == '\t' || *p == '\n') {
            p++;
            goto start;
        }
        if (*p >= 'a' && *p <= 'z') goto in_word;
        if (*p >= '0' && *p <= '9') goto in_number;
        if (*p == '"') goto in_string;
        if (*p == '/') goto maybe_comment;
        punct++;
        p++;
        goto start;

    in_word:
        p++;
        if ((*p >= 'a' && *p <= 'z') || (*p >= '0' && *p <= '9')) goto in_word;
        words++;
        goto start;

    in_number:
        p++;
        if (*p >= '0' && *p <= '9') goto in_number;
        numbers++;
        goto start;

    in_string:
        p++;
        if (*p == '\0') goto unterminated;
        if (*p != '"') goto in_string;
        p++;
        strings++;
        goto start;

    unterminated:
        strings--;
        goto done;

    maybe_comment:
        p++; /* past the '/' */
        if (*p != '*') {
            punct++;
            goto start;
        }
        p++; /* past the '*' that opened the comment */
        goto in_comment;

    in_comment:
        if (*p == '\0') goto done;
        if (*p == '*') goto comment_star;
        p++;
        goto in_comment;

    comment_star:
        p++; /* past the '*' */
        if (*p != '/') goto in_comment;
        p++;
        comments++;
        goto start;

    done:;
    }

    printf("statemachine size=%ld words=%ld numbers=%ld strings=%ld comments=%ld punct=%ld\n", size,
           words, numbers, strings, comments, punct);
    free(text);
    return 0;
}
