/* Arithmetic that wraps, at every width C has.
 *
 * C says unsigned arithmetic is modular, so `cinrs` generates `wrapping_add`,
 * `wrapping_mul`, `wrapping_sub` and `wrapping_neg` rather than Rust's `+`,
 * whose overflow is a panic in a debug build. Those are `#[inline]` intrinsics
 * that compile to the bare instruction, so the expectation is that this kernel
 * costs nothing at all next to gcc — and if it does not, every arithmetic
 * benchmark above is paying for it too.
 *
 * The signed half is deliberately kept inside its range, since signed overflow
 * is undefined and the three builds would then be free to disagree.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    long iters = (argc > 1) ? atol(argv[1]) : 120000000L;
    unsigned char b = 1;
    unsigned short h = 2;
    unsigned int w = 3;
    unsigned long long q = 4;
    signed int s = 5;
    long i;

    for (i = 0; i < iters; i++) {
        b = (unsigned char) (b * 31u + 7u);
        h = (unsigned short) (h * 65521u + b);
        w = w * 2654435761u + h;
        q = q * 6364136223846793005ull + 1442695040888963407ull;
        q ^= (unsigned long long) w << 32;
        s = (s * 7 + 3) % 1000003; /* stays inside `int`: no undefined overflow */
        b ^= (unsigned char) (q >> 56);
    }

    printf("wrapping iters=%ld b=%u h=%u w=%u q=%llu s=%d\n", iters, (unsigned) b, (unsigned) h, w,
           q, s);
    return 0;
}
