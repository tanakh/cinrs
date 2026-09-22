/* Three SSE2 kernels against the same three written in scalar C.
 *
 * This is the one program here whose point is the *intrinsics*: a call to
 * `_mm_add_ps` is generated as `core::arch::x86_64::_mm_add_ps`, so what it
 * measures is whether that mapping costs anything against `gcc`, which reaches
 * the same instruction through its own header. Each kernel is run in both
 * forms, so the ratio *within* a column says how much the instructions were
 * worth on this machine and the ratio *between* the columns — the only number
 * the report shows — says whether cinrs paid for them.
 *
 * Everything here is SSE2, which is the x86-64 baseline: no `-m` flag for gcc
 * and clang, and no `__attribute__((target))` for cinrs. The three kernels are
 * three shapes real code uses.
 *
 *   dot   a single-precision dot product: a loop-carried vector accumulator,
 *         four multiplies and four adds per iteration.
 *   sad   the sum of absolute differences over two byte buffers, through
 *         `_mm_sad_epu8` — one instruction that has no scalar counterpart at
 *         all, which is what makes it the kernel with the largest ratio.
 *   scan  memchr: compare sixteen bytes, `_mm_movemask_epi8`, count the
 *         trailing zeros. The idiom every C library uses.
 *
 * Each repetition perturbs one element of the input, so that nothing the
 * program computes can be hoisted out of the timing loop.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#include <immintrin.h>
#include <stdio.h>
#include <stdlib.h>

/* ---- dot product -------------------------------------------------------- */

static float dot_simd(const float *a, const float *b, long n) {
    __m128 acc = _mm_setzero_ps();
    float tail[4];
    float sum;
    long i;
    for (i = 0; i + 4 <= n; i += 4) {
        __m128 va = _mm_loadu_ps(a + i);
        __m128 vb = _mm_loadu_ps(b + i);
        acc = _mm_add_ps(acc, _mm_mul_ps(va, vb));
    }
    _mm_storeu_ps(tail, acc);
    sum = (tail[0] + tail[1]) + (tail[2] + tail[3]);
    for (; i < n; i++) sum += a[i] * b[i];
    return sum;
}

/* Unrolled four ways and summed in the same order as the vector form, so the
 * two answers are bit-for-bit equal and the check at the end is exact. */
static float dot_scalar(const float *a, const float *b, long n) {
    float s0 = 0.0f, s1 = 0.0f, s2 = 0.0f, s3 = 0.0f;
    float sum;
    long i;
    for (i = 0; i + 4 <= n; i += 4) {
        s0 += a[i] * b[i];
        s1 += a[i + 1] * b[i + 1];
        s2 += a[i + 2] * b[i + 2];
        s3 += a[i + 3] * b[i + 3];
    }
    sum = (s0 + s1) + (s2 + s3);
    for (; i < n; i++) sum += a[i] * b[i];
    return sum;
}

/* ---- sum of absolute differences ---------------------------------------- */

static unsigned long sad_simd(const unsigned char *x, const unsigned char *y, long n) {
    __m128i acc = _mm_setzero_si128();
    unsigned long halves[2];
    unsigned long total;
    long i;
    for (i = 0; i + 16 <= n; i += 16) {
        __m128i vx = _mm_loadu_si128((const __m128i *) (x + i));
        __m128i vy = _mm_loadu_si128((const __m128i *) (y + i));
        /* Two 16-bit sums, one per eight-byte half, accumulated as 64-bit
         * lanes — which is why nothing can overflow however long the buffer. */
        acc = _mm_add_epi64(acc, _mm_sad_epu8(vx, vy));
    }
    _mm_storeu_si128((__m128i *) halves, acc);
    total = halves[0] + halves[1];
    for (; i < n; i++) {
        int d = (int) x[i] - (int) y[i];
        total += (unsigned long) (d < 0 ? -d : d);
    }
    return total;
}

static unsigned long sad_scalar(const unsigned char *x, const unsigned char *y, long n) {
    unsigned long total = 0;
    long i;
    for (i = 0; i < n; i++) {
        int d = (int) x[i] - (int) y[i];
        total += (unsigned long) (d < 0 ? -d : d);
    }
    return total;
}

/* ---- memchr ------------------------------------------------------------- */

static long scan_simd(const unsigned char *buf, long n, unsigned char needle) {
    __m128i want = _mm_set1_epi8((char) needle);
    long i;
    for (i = 0; i + 16 <= n; i += 16) {
        __m128i v = _mm_loadu_si128((const __m128i *) (buf + i));
        int mask = _mm_movemask_epi8(_mm_cmpeq_epi8(v, want));
        if (mask) {
            int bit = 0;
            while ((mask & 1) == 0) {
                mask >>= 1;
                bit++;
            }
            return i + bit;
        }
    }
    for (; i < n; i++) if (buf[i] == needle) return i;
    return -1;
}

static long scan_scalar(const unsigned char *buf, long n, unsigned char needle) {
    long i;
    for (i = 0; i < n; i++) if (buf[i] == needle) return i;
    return -1;
}

/* ---- the driver --------------------------------------------------------- */

int main(int argc, char **argv) {
    long size = (argc > 1) ? atol(argv[1]) : 200000L;
    int reps = (argc > 2) ? atoi(argv[2]) : 400;
    float *a = malloc((size_t) size * sizeof(float));
    float *b = malloc((size_t) size * sizeof(float));
    unsigned char *x = malloc((size_t) size);
    unsigned char *y = malloc((size_t) size);
    unsigned state = 12345u;
    double dot_v = 0.0, dot_s = 0.0;
    unsigned long sad_v = 0, sad_s = 0;
    long scan_v = 0, scan_s = 0;
    long needle_at = size - size / 4;
    long i;
    int r;
    int ok = 1;

    if (!a || !b || !x || !y) {
        fprintf(stderr, "simd-dot: out of memory\n");
        return 1;
    }
    for (i = 0; i < size; i++) {
        state = state * 1103515245u + 12345u;
        a[i] = (float) (int) ((state >> 20) & 0xff) - 128.0f;
        state = state * 1103515245u + 12345u;
        b[i] = (float) (int) ((state >> 20) & 0xff) - 128.0f;
        state = state * 1103515245u + 12345u;
        x[i] = (unsigned char) (state >> 16);
        state = state * 1103515245u + 12345u;
        y[i] = (unsigned char) (state >> 16);
    }
    /* One byte the scan will find, three quarters of the way along, and nowhere
     * before it. */
    for (i = 0; i < size; i++) if (x[i] == 0xa5u) x[i] = 0xa6u;
    x[needle_at] = 0xa5u;

    for (r = 0; r < reps; r++) {
        /* Perturb one element per repetition so that neither form can be
         * hoisted out of the loop; both forms see the same data. */
        a[r % size] = (float) (r % 31) - 15.0f;
        y[r % size] = (unsigned char) r;
        dot_v += (double) dot_simd(a, b, size);
        dot_s += (double) dot_scalar(a, b, size);
        sad_v += sad_simd(x, y, size);
        sad_s += sad_scalar(x, y, size);
        scan_v += scan_simd(x, size, 0xa5u);
        scan_s += scan_scalar(x, size, 0xa5u);
    }

    if (dot_v != dot_s) ok = 0;
    if (sad_v != sad_s) ok = 0;
    if (scan_v != scan_s) ok = 0;
    if (scan_v != (long) reps * needle_at) ok = 0;

    printf("simd-dot size=%ld reps=%d dot=%.1f sad=%lu found=%ld agree=%d\n",
           size, reps, dot_v, sad_v, scan_v, ok);
    free(a);
    free(b);
    free(x);
    free(y);
    return ok ? 0 : 1;
}
