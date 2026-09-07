/* Table-driven CRC-32 (the IEEE 802.3 polynomial, reflected) over a buffer.
 *
 * One byte load, one table load, a shift and an xor per iteration. A tight
 * serial dependency chain, so it measures how well the generated Rust keeps
 * the loop-carried value in a register.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>

static unsigned table[256];

static void build_table(void) {
    unsigned i, j, c;
    for (i = 0; i < 256; i++) {
        c = i;
        for (j = 0; j < 8; j++) c = (c & 1u) ? (0xEDB88320u ^ (c >> 1)) : (c >> 1);
        table[i] = c;
    }
}

static unsigned crc32(const unsigned char *buf, long len, unsigned crc) {
    long i;
    crc = ~crc;
    for (i = 0; i < len; i++) crc = table[(crc ^ buf[i]) & 0xffu] ^ (crc >> 8);
    return ~crc;
}

int main(int argc, char **argv) {
    long size = (argc > 1) ? atol(argv[1]) : 4000000L;
    int reps = (argc > 2) ? atoi(argv[2]) : 100;
    unsigned char *buf = malloc((size_t) size);
    unsigned state = 1u, crc = 0u;
    long i;
    int r;

    if (!buf) {
        fprintf(stderr, "crc32: out of memory\n");
        return 1;
    }
    for (i = 0; i < size; i++) {
        state = state * 1103515245u + 12345u;
        buf[i] = (unsigned char) (state >> 16);
    }
    build_table();

    for (r = 0; r < reps; r++) crc = crc32(buf, size, crc);

    printf("crc32 size=%ld reps=%d crc=%08x\n", size, reps, crc);
    free(buf);
    return 0;
}
