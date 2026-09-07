/* Small `struct`s passed and returned by value.
 *
 * On x86-64 System V a two-`double` struct goes in two SSE registers and a
 * three-`long` struct goes in memory, so this exercises both halves of the
 * classification. `cinrs` generates `#[repr(C)]` items and lets `rustc`'s own
 * C ABI carry them, so the question is whether the values stay in registers
 * or get spilled through the stack on every call.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

typedef struct {
    double x, y;
} Vec2;

/* Unsigned, because the values below wrap and signed overflow is undefined —
 * which the two native compilers are entitled to exploit and `cinrs` is not. */
typedef struct {
    unsigned long a, b, c;
} Triple;

static Vec2 add2(Vec2 p, Vec2 q) {
    Vec2 r;
    r.x = p.x + q.x;
    r.y = p.y + q.y;
    return r;
}

static Vec2 scale2(Vec2 p, double s) {
    Vec2 r;
    r.x = p.x * s;
    r.y = p.y * s;
    return r;
}

static double dot2(Vec2 p, Vec2 q) { return p.x * q.x + p.y * q.y; }

static Triple step3(Triple t) {
    Triple r;
    r.a = t.b + t.c;
    r.b = t.c ^ t.a;
    r.c = t.a - t.b;
    return r;
}

int main(int argc, char **argv) {
    long iters = (argc > 1) ? atol(argv[1]) : 60000000L;
    Vec2 p, q;
    Triple t;
    double acc = 0.0;
    long i;

    p.x = 1.0;
    p.y = 2.0;
    q.x = 0.5;
    q.y = -0.25;
    t.a = 1;
    t.b = 2;
    t.c = 3;

    for (i = 0; i < iters; i++) {
        p = add2(p, scale2(q, 1e-9));
        acc += dot2(p, q);
        t = step3(t);
        if ((i & 0xffff) == 0) {
            p.x = 1.0;
            p.y = 2.0;
        }
    }

    printf("structval iters=%ld acc=%.9f t=%lu,%lu,%lu\n", iters, acc, t.a, t.b, t.c);
    return 0;
}
