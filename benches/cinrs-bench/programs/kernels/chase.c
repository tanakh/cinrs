/* Pointer chasing down a linked list whose nodes are in random order.
 *
 * Each load depends on the last, so the processor cannot run ahead and the
 * time is a chain of cache misses. The compiler contributes one instruction
 * per node — the load of `p->next` — so this is a control for the memory
 * system rather than for code generation, and the three builds should be
 * within noise of each other. When they are not, something about the
 * generated addressing is wrong.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

typedef struct Node {
    struct Node *next;
    long payload;
} Node;

int main(int argc, char **argv) {
    long n = (argc > 1) ? atol(argv[1]) : 4000000L;
    long steps = (argc > 2) ? atol(argv[2]) : 40000000L;
    Node *nodes = malloc((size_t) n * sizeof(Node));
    long *order = malloc((size_t) n * sizeof(long));
    unsigned state = 4321u;
    Node *p;
    long i, sum = 0;

    if (!nodes || !order) {
        fprintf(stderr, "chase: out of memory\n");
        return 1;
    }
    for (i = 0; i < n; i++) order[i] = i;
    /* Fisher-Yates, so the list visits every node exactly once in a random
     * order and the prefetcher gets nothing out of it. */
    for (i = n - 1; i > 0; i--) {
        long j;
        long t;
        state = state * 1664525u + 1013904223u;
        j = (long) ((state >> 8) % (unsigned) (i + 1));
        t = order[i];
        order[i] = order[j];
        order[j] = t;
    }
    for (i = 0; i < n; i++) {
        nodes[order[i]].next = &nodes[order[(i + 1) % n]];
        nodes[order[i]].payload = i;
    }

    p = &nodes[order[0]];
    for (i = 0; i < steps; i++) {
        sum += p->payload & 15;
        p = p->next;
    }

    printf("chase n=%ld steps=%ld sum=%ld\n", n, steps, sum);
    free(nodes);
    free(order);
    return 0;
}
