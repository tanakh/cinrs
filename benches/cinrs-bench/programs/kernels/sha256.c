/* SHA-256 (FIPS 180-4), written out here rather than linked from a library.
 *
 * Thirty-two-bit rotates, shifts, additions and xors in a fully unrollable
 * sixty-four-round loop over a sixteen-word message schedule. The rotates are
 * written as `(x >> n) | (x << (32 - n))`, which both back ends are expected
 * to recognise; if one of them does not, this is where it shows.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef unsigned int u32;

static const u32 K[64] = {
    0x428a2f98u, 0x71374491u, 0xb5c0fbcfu, 0xe9b5dba5u, 0x3956c25bu, 0x59f111f1u, 0x923f82a4u,
    0xab1c5ed5u, 0xd807aa98u, 0x12835b01u, 0x243185beu, 0x550c7dc3u, 0x72be5d74u, 0x80deb1feu,
    0x9bdc06a7u, 0xc19bf174u, 0xe49b69c1u, 0xefbe4786u, 0x0fc19dc6u, 0x240ca1ccu, 0x2de92c6fu,
    0x4a7484aau, 0x5cb0a9dcu, 0x76f988dau, 0x983e5152u, 0xa831c66du, 0xb00327c8u, 0xbf597fc7u,
    0xc6e00bf3u, 0xd5a79147u, 0x06ca6351u, 0x14292967u, 0x27b70a85u, 0x2e1b2138u, 0x4d2c6dfcu,
    0x53380d13u, 0x650a7354u, 0x766a0abbu, 0x81c2c92eu, 0x92722c85u, 0xa2bfe8a1u, 0xa81a664bu,
    0xc24b8b70u, 0xc76c51a3u, 0xd192e819u, 0xd6990624u, 0xf40e3585u, 0x106aa070u, 0x19a4c116u,
    0x1e376c08u, 0x2748774cu, 0x34b0bcb5u, 0x391c0cb3u, 0x4ed8aa4au, 0x5b9cca4fu, 0x682e6ff3u,
    0x748f82eeu, 0x78a5636fu, 0x84c87814u, 0x8cc70208u, 0x90befffau, 0xa4506cebu, 0xbef9a3f7u,
    0xc67178f2u};

#define ROR(x, n) (((x) >> (n)) | ((x) << (32 - (n))))

static void sha256_block(u32 h[8], const unsigned char *p) {
    u32 w[64];
    u32 a, b, c, d, e, f, g, hh;
    int i;

    for (i = 0; i < 16; i++)
        w[i] = ((u32) p[4 * i] << 24) | ((u32) p[4 * i + 1] << 16) | ((u32) p[4 * i + 2] << 8) |
               (u32) p[4 * i + 3];
    for (i = 16; i < 64; i++) {
        u32 s0 = ROR(w[i - 15], 7) ^ ROR(w[i - 15], 18) ^ (w[i - 15] >> 3);
        u32 s1 = ROR(w[i - 2], 17) ^ ROR(w[i - 2], 19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16] + s0 + w[i - 7] + s1;
    }

    a = h[0];
    b = h[1];
    c = h[2];
    d = h[3];
    e = h[4];
    f = h[5];
    g = h[6];
    hh = h[7];

    for (i = 0; i < 64; i++) {
        u32 S1 = ROR(e, 6) ^ ROR(e, 11) ^ ROR(e, 25);
        u32 ch = (e & f) ^ ((~e) & g);
        u32 t1 = hh + S1 + ch + K[i] + w[i];
        u32 S0 = ROR(a, 2) ^ ROR(a, 13) ^ ROR(a, 22);
        u32 maj = (a & b) ^ (a & c) ^ (b & c);
        u32 t2 = S0 + maj;
        hh = g;
        g = f;
        f = e;
        e = d + t1;
        d = c;
        c = b;
        b = a;
        a = t1 + t2;
    }

    h[0] += a;
    h[1] += b;
    h[2] += c;
    h[3] += d;
    h[4] += e;
    h[5] += f;
    h[6] += g;
    h[7] += hh;
}

static void sha256(const unsigned char *msg, long len, unsigned char out[32]) {
    u32 h[8] = {0x6a09e667u, 0xbb67ae85u, 0x3c6ef372u, 0xa54ff53au,
                0x510e527fu, 0x9b05688cu, 0x1f83d9abu, 0x5be0cd19u};
    unsigned char tail[128];
    long i, full = len / 64, restlen = len - full * 64;
    long taillen;

    for (i = 0; i < full; i++) sha256_block(h, msg + i * 64);

    memcpy(tail, msg + full * 64, (size_t) restlen);
    tail[restlen] = 0x80;
    taillen = (restlen < 56) ? 64 : 128;
    for (i = restlen + 1; i < taillen - 8; i++) tail[i] = 0;
    {
        unsigned long bits = (unsigned long) len * 8ul;
        for (i = 0; i < 8; i++) tail[taillen - 1 - i] = (unsigned char) ((bits >> (8 * i)) & 0xff);
    }
    for (i = 0; i < taillen; i += 64) sha256_block(h, tail + i);

    for (i = 0; i < 8; i++) {
        out[4 * i] = (unsigned char) (h[i] >> 24);
        out[4 * i + 1] = (unsigned char) (h[i] >> 16);
        out[4 * i + 2] = (unsigned char) (h[i] >> 8);
        out[4 * i + 3] = (unsigned char) h[i];
    }
}

int main(int argc, char **argv) {
    long size = (argc > 1) ? atol(argv[1]) : 200000L;
    int reps = (argc > 2) ? atoi(argv[2]) : 300;
    unsigned char *buf = malloc((size_t) size);
    unsigned char digest[32];
    unsigned state = 9u;
    long i;
    int r;

    if (!buf) {
        fprintf(stderr, "sha256: out of memory\n");
        return 1;
    }
    for (i = 0; i < size; i++) {
        state = state * 1103515245u + 12345u;
        buf[i] = (unsigned char) (state >> 16);
    }

    for (r = 0; r < reps; r++) {
        sha256(buf, size, digest);
        buf[0] = digest[0];
    }

    printf("sha256 size=%ld reps=%d digest=", size, reps);
    for (i = 0; i < 32; i++) printf("%02x", digest[i]);
    printf("\n");
    free(buf);
    return 0;
}
