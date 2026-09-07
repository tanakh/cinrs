/* Recursive `fib` and Takeuchi's `tak`.
 *
 * Nothing but calls: what is measured is the cost of a call to a generated
 * `unsafe extern "C" fn` against the cost of a call to a C function the same
 * back end compiled. Neither can be inlined away entirely, `tak` least of all.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

static long fib(long n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }

static int tak(int x, int y, int z) {
    if (y >= x) return z;
    return tak(tak(x - 1, y, z), tak(y - 1, z, x), tak(z - 1, x, y));
}

static long ack(long m, long n) {
    if (m == 0) return n + 1;
    if (n == 0) return ack(m - 1, 1);
    return ack(m - 1, ack(m, n - 1));
}

/* `tak` is called with (3t, 2t, t), so its cost climbs very steeply in `t`:
 * the classic figure is t = 6, and each step up multiplies the number of calls
 * by roughly seven. `reps` is there to bring the whole kernel to about a
 * second without pushing `t` past the point where it dwarfs everything else. */
int main(int argc, char **argv) {
    long n = (argc > 1) ? atol(argv[1]) : 32;
    int t = (argc > 2) ? atoi(argv[2]) : 10;
    long a = (argc > 3) ? atol(argv[3]) : 3;
    int reps = (argc > 4) ? atoi(argv[4]) : 1;
    long f = 0, v = 0;
    int k = 0, r;

    for (r = 0; r < reps; r++) {
        f = fib(n);
        k = tak(t * 3, t * 2, t);
        v = ack(a, 7);
    }

    printf("fib(%ld)=%ld tak(%d,%d,%d)=%d ack(%ld,7)=%ld\n", n, f, t * 3, t * 2, t, k, a, v);
    return 0;
}
