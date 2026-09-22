//! Integration tests that *run* translated C using the Intel SIMD intrinsics.
//!
//! The design under test is a name-level mapping: the bundled `<immintrin.h>`
//! declares the intrinsics against C's types, and a call to one is generated as
//! `::core::arch::x86_64::_mm_add_epi32(a, b)` — Rust's own function of the
//! same name and the same signature, because `core::arch` was generated from
//! the same Intel data Intel's own headers are. There is no vector language in
//! `cinrs` and no symbol to link.
//!
//! Every expected value in here is computed **in the same C block**, in scalar
//! C: a lane-by-lane loop, a shift, a comparison, a software CRC-32C. So each
//! function answers 1 only if the instruction and the C agree about what it
//! does — and the same file, with a `main` added, was compiled and run by
//! `gcc -O2` and by `gcc -O2 -mavx2 -mfma -msse4.2 -mbmi -mbmi2 -mlzcnt
//! -mpopcnt` (gcc 15.2), where every function answered 1 too. The oracle is
//! therefore C's own semantics twice over rather than a table of numbers
//! someone typed in.
//!
//! The instruction sets above the x86-64 baseline are asked for with
//! `__attribute__((target("…")))`, which becomes `#[target_feature(enable =
//! "…")]`, and are guarded at run time with `is_x86_feature_detected!` on the
//! Rust side — the same question `__builtin_cpu_supports` asks in C, which has
//! a test of its own below. SSE and SSE2 need neither: they are in every
//! x86-64 target, which is why `cinrs` predefines `__SSE__` and `__SSE2__`
//! there and nothing above them.
//!
//! What must *not* compile — a non-constant immediate, the address of an
//! immediate-operand intrinsic, an intrinsic in a `[[cinrs::safe]]` function,
//! the header on a target that is not x86, MMX — is in `tests/ui/simd_*.rs`,
//! where the message a user sees is what is blessed.
#![cfg(target_arch = "x86_64")]

use cinrs::{c11, gnu11};

// ---------------------------------------------------------------------------
// SSE2: the baseline every x86-64 target has
// ---------------------------------------------------------------------------

c11! {
    #include <immintrin.h>
    #include <stdint.h>

    /* Packed 32-bit add, subtract, shift and compare, each checked against the
     * same operation written out lane by lane. */
    int sse2_integer(void) {
        int32_t a[4] = {1, -2, 3, 1000000};
        int32_t b[4] = {10, 20, -30, 7};
        int32_t got[4], want[4];
        int i;
        __m128i va = _mm_loadu_si128((const __m128i *)a);
        __m128i vb = _mm_loadu_si128((const __m128i *)b);

        _mm_storeu_si128((__m128i *)got, _mm_add_epi32(va, vb));
        for (i = 0; i < 4; i++) want[i] = a[i] + b[i];
        for (i = 0; i < 4; i++) if (got[i] != want[i]) return 0;

        _mm_storeu_si128((__m128i *)got, _mm_sub_epi32(va, vb));
        for (i = 0; i < 4; i++) want[i] = a[i] - b[i];
        for (i = 0; i < 4; i++) if (got[i] != want[i]) return 0;

        _mm_storeu_si128((__m128i *)got, _mm_slli_epi32(va, 3));
        for (i = 0; i < 4; i++) want[i] = (int32_t)((uint32_t)a[i] << 3);
        for (i = 0; i < 4; i++) if (got[i] != want[i]) return 0;

        _mm_storeu_si128((__m128i *)got, _mm_srai_epi32(va, 2));
        for (i = 0; i < 4; i++) want[i] = a[i] >> 2;
        for (i = 0; i < 4; i++) if (got[i] != want[i]) return 0;

        _mm_storeu_si128((__m128i *)got, _mm_cmpgt_epi32(va, vb));
        for (i = 0; i < 4; i++) want[i] = a[i] > b[i] ? -1 : 0;
        for (i = 0; i < 4; i++) if (got[i] != want[i]) return 0;

        _mm_storeu_si128((__m128i *)got, _mm_and_si128(va, vb));
        for (i = 0; i < 4; i++) want[i] = a[i] & b[i];
        for (i = 0; i < 4; i++) if (got[i] != want[i]) return 0;

        return 1;
    }

    /* The 16-bit multiplies, where the saturating forms are the point. */
    int sse2_multiply(void) {
        int16_t a[8] = {1, -2, 3, -4, 300, 1000, -32768, 7};
        int16_t b[8] = {10, 20, -30, 40, 200, -3, 3, 0};
        int16_t got[8], want[8];
        int i;
        __m128i va = _mm_loadu_si128((const __m128i *)a);
        __m128i vb = _mm_loadu_si128((const __m128i *)b);

        _mm_storeu_si128((__m128i *)got, _mm_mullo_epi16(va, vb));
        for (i = 0; i < 8; i++) want[i] = (int16_t)((uint16_t)a[i] * (uint16_t)b[i]);
        for (i = 0; i < 8; i++) if (got[i] != want[i]) return 0;

        _mm_storeu_si128((__m128i *)got, _mm_mulhi_epi16(va, vb));
        for (i = 0; i < 8; i++) want[i] = (int16_t)(((int32_t)a[i] * (int32_t)b[i]) >> 16);
        for (i = 0; i < 8; i++) if (got[i] != want[i]) return 0;

        _mm_storeu_si128((__m128i *)got, _mm_adds_epi16(va, vb));
        for (i = 0; i < 8; i++) {
            int32_t sum = (int32_t)a[i] + (int32_t)b[i];
            if (sum > 32767) sum = 32767;
            if (sum < -32768) sum = -32768;
            want[i] = (int16_t)sum;
        }
        for (i = 0; i < 8; i++) if (got[i] != want[i]) return 0;
        return 1;
    }

    /* `_mm_movemask_epi8` gathers the sign bit of every byte, which is how
     * every `memchr`-style scan reads the result of a compare. */
    int sse2_movemask(void) {
        unsigned char bytes[16];
        int i, want = 0;
        __m128i v;
        for (i = 0; i < 16; i++) bytes[i] = (unsigned char)(i * 17);
        v = _mm_loadu_si128((const __m128i *)bytes);
        for (i = 0; i < 16; i++) if (bytes[i] & 0x80) want |= 1 << i;
        return _mm_movemask_epi8(v) == want;
    }

    /* A `__m128i` object is sixteen-byte aligned, so a store to its address is
     * the *aligned* form; the unaligned one reads from the middle of an array
     * of `int32_t`, which nothing promises the alignment of. */
    int loads_stores(void) {
        int32_t src[8] = {11, 22, 33, 44, 55, 66, 77, 88};
        int32_t out[8] = {0, 0, 0, 0, 0, 0, 0, 0};
        __m128i aligned;
        int i;
        _mm_store_si128(&aligned, _mm_loadu_si128((const __m128i *)(src + 1)));
        _mm_storeu_si128((__m128i *)out, _mm_load_si128(&aligned));
        for (i = 0; i < 4; i++) if (out[i] != src[i + 1]) return 0;
        if (out[4] != 0) return 0;
        if (_Alignof(__m128i) != 16 || sizeof(__m128i) != 16) return 0;
        if (_Alignof(__m256d) != 32 || sizeof(__m256d) != 32) return 0;
        return 1;
    }

    /* `_mm_set_*` takes its arguments from the *high* lane down and
     * `_mm_setr_*` from the low one up, which is the trap in every SIMD code
     * base. */
    int setters(void) {
        int32_t out[4];
        _mm_storeu_si128((__m128i *)out, _mm_set_epi32(4, 3, 2, 1));
        if (out[0] != 1 || out[1] != 2 || out[2] != 3 || out[3] != 4) return 0;
        _mm_storeu_si128((__m128i *)out, _mm_setr_epi32(4, 3, 2, 1));
        if (out[0] != 4 || out[1] != 3 || out[2] != 2 || out[3] != 1) return 0;
        _mm_storeu_si128((__m128i *)out, _mm_set1_epi32(-7));
        if (out[0] != -7 || out[3] != -7) return 0;
        _mm_storeu_si128((__m128i *)out, _mm_setzero_si128());
        if (out[0] != 0 || out[3] != 0) return 0;
        return 1;
    }

    /* `_MM_SHUFFLE` is an ordinary macro of the bundled header, and the
     * selector it builds is an integer constant expression — which is what the
     * generated turbofish needs. */
    int shuffles(void) {
        int32_t out[4];
        __m128i v = _mm_setr_epi32(10, 20, 30, 40);
        _mm_storeu_si128((__m128i *)out, _mm_shuffle_epi32(v, _MM_SHUFFLE(0, 1, 2, 3)));
        if (out[0] != 40 || out[1] != 30 || out[2] != 20 || out[3] != 10) return 0;
        _mm_storeu_si128((__m128i *)out, _mm_shuffle_epi32(v, _MM_SHUFFLE(3, 3, 3, 3)));
        if (out[0] != 40 || out[3] != 40) return 0;
        _mm_storeu_si128((__m128i *)out, _mm_unpacklo_epi32(v, _mm_setzero_si128()));
        if (out[0] != 10 || out[1] != 0 || out[2] != 20 || out[3] != 0) return 0;
        return 1;
    }
}

#[test]
fn sse2_is_the_baseline() {
    unsafe {
        assert_eq!(sse2_integer(), 1);
        assert_eq!(sse2_multiply(), 1);
        assert_eq!(sse2_movemask(), 1);
        assert_eq!(loads_stores(), 1);
        assert_eq!(setters(), 1);
        assert_eq!(shuffles(), 1);
    }
}

// ---------------------------------------------------------------------------
// SSE: the floating-point half
// ---------------------------------------------------------------------------

c11! {
    #include <immintrin.h>
    #include <stdint.h>

    static int close_enough(float x, float y) {
        float d = x - y;
        if (d < 0) d = -d;
        return d <= 0.001f * (x < 0 ? -x : x) + 0.0001f;
    }

    int sse_float(void) {
        float a[4] = {1.5f, -2.25f, 9.0f, 0.5f};
        float b[4] = {4.0f, 8.0f, 3.0f, -1.0f};
        float got[4];
        int i;
        __m128 va = _mm_loadu_ps(a);
        __m128 vb = _mm_loadu_ps(b);

        _mm_storeu_ps(got, _mm_add_ps(va, vb));
        for (i = 0; i < 4; i++) if (got[i] != a[i] + b[i]) return 0;

        _mm_storeu_ps(got, _mm_sub_ps(va, vb));
        for (i = 0; i < 4; i++) if (got[i] != a[i] - b[i]) return 0;

        _mm_storeu_ps(got, _mm_mul_ps(va, vb));
        for (i = 0; i < 4; i++) if (got[i] != a[i] * b[i]) return 0;

        _mm_storeu_ps(got, _mm_div_ps(va, vb));
        for (i = 0; i < 4; i++) if (got[i] != a[i] / b[i]) return 0;

        _mm_storeu_ps(got, _mm_min_ps(va, vb));
        for (i = 0; i < 4; i++) if (got[i] != (a[i] < b[i] ? a[i] : b[i])) return 0;

        _mm_storeu_ps(got, _mm_max_ps(va, vb));
        for (i = 0; i < 4; i++) if (got[i] != (a[i] > b[i] ? a[i] : b[i])) return 0;

        /* `sqrt` is exact for these, and `rcp` promises only twelve bits. */
        _mm_storeu_ps(got, _mm_sqrt_ps(_mm_setr_ps(1.0f, 4.0f, 9.0f, 16.0f)));
        if (got[0] != 1.0f || got[1] != 2.0f || got[2] != 3.0f || got[3] != 4.0f) return 0;
        _mm_storeu_ps(got, _mm_rcp_ps(vb));
        for (i = 0; i < 4; i++) if (!close_enough(got[i], 1.0f / b[i])) return 0;

        if (_mm_cvtss_f32(va) != a[0]) return 0;
        if (_mm_movemask_ps(_mm_cmplt_ps(va, _mm_setzero_ps())) != 0x2) return 0;
        return 1;
    }

    /* The 4x4 transpose Intel documents as a macro, which the bundled header
     * writes out in terms of the unpack and move intrinsics. */
    int transpose(void) {
        __m128 r0 = _mm_setr_ps(0.0f, 1.0f, 2.0f, 3.0f);
        __m128 r1 = _mm_setr_ps(10.0f, 11.0f, 12.0f, 13.0f);
        __m128 r2 = _mm_setr_ps(20.0f, 21.0f, 22.0f, 23.0f);
        __m128 r3 = _mm_setr_ps(30.0f, 31.0f, 32.0f, 33.0f);
        float out[4];
        _MM_TRANSPOSE4_PS(r0, r1, r2, r3);
        _mm_storeu_ps(out, r0);
        if (out[0] != 0.0f || out[1] != 10.0f || out[2] != 20.0f || out[3] != 30.0f) return 0;
        _mm_storeu_ps(out, r3);
        if (out[0] != 3.0f || out[1] != 13.0f || out[2] != 23.0f || out[3] != 33.0f) return 0;
        return 1;
    }

    /* The casts are intrinsics like any other: no instruction, one type. */
    int casts(void) {
        int32_t out[4];
        __m128 f = _mm_setr_ps(1.0f, 2.0f, 3.0f, 4.0f);
        __m128i i = _mm_castps_si128(f);
        _mm_storeu_si128((__m128i *)out, i);
        if (out[0] != 0x3f800000) return 0;
        if (_mm_cvtss_f32(_mm_castsi128_ps(i)) != 1.0f) return 0;
        if (_mm_cvtsd_f64(_mm_castps_pd(f)) == 0.0) return 0;
        return 1;
    }
}

#[test]
fn sse_floating_point() {
    unsafe {
        assert_eq!(sse_float(), 1);
        assert_eq!(transpose(), 1);
        assert_eq!(casts(), 1);
    }
}

// ---------------------------------------------------------------------------
// the instruction sets above the baseline, asked for with `target`
// ---------------------------------------------------------------------------

gnu11! {
    #include <immintrin.h>
    #include <stdint.h>

    __attribute__((target("ssse3"))) int ssse3_shuffle(void) {
        unsigned char out[16];
        int i;
        __m128i v = _mm_setr_epi8(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15);
        __m128i idx = _mm_setr_epi8(15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0);
        _mm_storeu_si128((__m128i *)out, _mm_shuffle_epi8(v, idx));
        for (i = 0; i < 16; i++) if (out[i] != (unsigned char)(15 - i)) return 0;
        /* The high bit of an index zeroes the lane. */
        idx = _mm_set1_epi8((char)0x80);
        _mm_storeu_si128((__m128i *)out, _mm_shuffle_epi8(v, idx));
        for (i = 0; i < 16; i++) if (out[i] != 0) return 0;
        return 1;
    }

    __attribute__((target("sse4.1"))) int sse41_ops(void) {
        float a[4] = {1.0f, 2.0f, 3.0f, 4.0f};
        float b[4] = {-1.0f, -2.0f, -3.0f, -4.0f};
        int32_t mask[4] = {0, -1, 0, -1};
        int32_t p[4] = {1, -2, 3, 1000000};
        int32_t q[4] = {10, 20, -30, 7};
        int32_t out[4], want[4];
        float got[4];
        int i;
        __m128 va = _mm_loadu_ps(a), vb = _mm_loadu_ps(b);
        __m128 m = _mm_castsi128_ps(_mm_loadu_si128((const __m128i *)mask));
        __m128i iv;

        _mm_storeu_ps(got, _mm_blendv_ps(va, vb, m));
        for (i = 0; i < 4; i++) if (got[i] != (mask[i] ? b[i] : a[i])) return 0;

        iv = _mm_setr_epi32(100, 200, 300, 400);
        if (_mm_extract_epi32(iv, 2) != 300) return 0;
        iv = _mm_insert_epi32(iv, -5, 1);
        if (_mm_extract_epi32(iv, 1) != -5) return 0;
        if (_mm_extract_epi32(iv, 0) != 100) return 0;

        _mm_storeu_si128((__m128i *)out,
                         _mm_mullo_epi32(_mm_loadu_si128((const __m128i *)p),
                                         _mm_loadu_si128((const __m128i *)q)));
        for (i = 0; i < 4; i++) want[i] = (int32_t)((uint32_t)p[i] * (uint32_t)q[i]);
        for (i = 0; i < 4; i++) if (out[i] != want[i]) return 0;
        return 1;
    }

    /* CRC-32C — the reflected 0x1edc6f41 polynomial the instruction
     * implements — folding `bits` bits of `v` into `crc`, in plain C. */
    static unsigned int crc32c_bits(unsigned int crc, unsigned int v, int bits) {
        int i;
        crc ^= v;
        for (i = 0; i < bits; i++) {
            unsigned int low = crc & 1u;
            crc >>= 1;
            if (low) crc ^= 0x82f63b78u;
        }
        return crc;
    }

    __attribute__((target("sse4.2"))) int sse42_crc(void) {
        unsigned int seeds[3] = {0u, 0xffffffffu, 0x12345678u};
        unsigned int values[3] = {0x12345678u, 0u, 0xdeadbeefu};
        int i;
        for (i = 0; i < 3; i++)
            if (_mm_crc32_u32(seeds[i], values[i]) != crc32c_bits(seeds[i], values[i], 32))
                return 0;
        if (_mm_crc32_u8(0u, 0x5au) != crc32c_bits(0u, 0x5au, 8)) return 0;
        if (_mm_crc32_u16(7u, 0xbeefu) != crc32c_bits(7u, 0xbeefu, 16)) return 0;
        return 1;
    }

    __attribute__((target("fma"))) int fma_ops(void) {
        float a[4] = {1.0f, 2.0f, 3.0f, 4.0f};
        float b[4] = {5.0f, 6.0f, 7.0f, 8.0f};
        float c[4] = {0.5f, 0.5f, 0.5f, 0.5f};
        float got[4];
        int i;
        _mm_storeu_ps(got, _mm_fmadd_ps(_mm_loadu_ps(a), _mm_loadu_ps(b), _mm_loadu_ps(c)));
        for (i = 0; i < 4; i++) if (got[i] != a[i] * b[i] + c[i]) return 0;
        _mm_storeu_ps(got, _mm_fmsub_ps(_mm_loadu_ps(a), _mm_loadu_ps(b), _mm_loadu_ps(c)));
        for (i = 0; i < 4; i++) if (got[i] != a[i] * b[i] - c[i]) return 0;
        return 1;
    }
}

#[test]
fn ssse3_and_sse4_and_fma() {
    if is_x86_feature_detected!("ssse3") {
        assert_eq!(unsafe { ssse3_shuffle() }, 1);
    }
    if is_x86_feature_detected!("sse4.1") {
        assert_eq!(unsafe { sse41_ops() }, 1);
    }
    if is_x86_feature_detected!("sse4.2") {
        assert_eq!(unsafe { sse42_crc() }, 1);
    }
    if is_x86_feature_detected!("fma") {
        assert_eq!(unsafe { fma_ops() }, 1);
    }
}

// ---------------------------------------------------------------------------
// AVX and AVX2, and the ABI rule that comes with them
// ---------------------------------------------------------------------------

gnu11! {
    #include <immintrin.h>
    #include <stdint.h>

    __attribute__((target("avx2"))) int avx2_ops(void) {
        int32_t a[8] = {1, 2, 3, 4, 5, 6, 7, 8};
        int32_t b[8] = {10, -20, 30, -40, 50, -60, 70, -80};
        int32_t got[8], want[8], half[4];
        float fa[8], fgot[8];
        int i;
        __m256i va = _mm256_loadu_si256((const __m256i *)a);
        __m256i vb = _mm256_loadu_si256((const __m256i *)b);
        __m256 fv;
        __m128i lo, hi;

        _mm256_storeu_si256((__m256i *)got, _mm256_add_epi32(va, vb));
        for (i = 0; i < 8; i++) want[i] = a[i] + b[i];
        for (i = 0; i < 8; i++) if (got[i] != want[i]) return 0;

        _mm256_storeu_si256((__m256i *)got, _mm256_mullo_epi32(va, vb));
        for (i = 0; i < 8; i++) want[i] = (int32_t)((uint32_t)a[i] * (uint32_t)b[i]);
        for (i = 0; i < 8; i++) if (got[i] != want[i]) return 0;

        _mm256_storeu_si256((__m256i *)got, _mm256_slli_epi32(va, 4));
        for (i = 0; i < 8; i++) want[i] = (int32_t)((uint32_t)a[i] << 4);
        for (i = 0; i < 8; i++) if (got[i] != want[i]) return 0;

        if (_mm256_extract_epi32(vb, 5) != -60) return 0;
        if (_mm256_movemask_ps(_mm256_castsi256_ps(vb)) != 0xaa) return 0;

        for (i = 0; i < 8; i++) fa[i] = (float)(i + 1);
        fv = _mm256_loadu_ps(fa);
        _mm256_storeu_ps(fgot, _mm256_mul_ps(fv, _mm256_set1_ps(3.0f)));
        for (i = 0; i < 8; i++) if (fgot[i] != fa[i] * 3.0f) return 0;

        /* the two 128-bit halves, and back */
        lo = _mm256_castsi256_si128(va);
        hi = _mm256_extracti128_si256(va, 1);
        _mm_storeu_si128((__m128i *)half, lo);
        if (half[0] != 1 || half[3] != 4) return 0;
        _mm_storeu_si128((__m128i *)half, hi);
        if (half[0] != 5 || half[3] != 8) return 0;
        return 1;
    }

    /* A 256-bit vector passed or returned *by value* through the C ABI needs
     * the `avx` feature at the definition and at every call — that is Rust's
     * rule as much as gcc's, and the `target` attribute is how both are told.
     * This is the shape a real code base uses, and the reason the attribute
     * matters beyond the instruction selection. */
    __attribute__((target("avx2"))) __m256i twice(__m256i v) {
        return _mm256_add_epi32(v, v);
    }

    __attribute__((target("avx2"))) int by_value(void) {
        int32_t got[8];
        int i;
        __m256i v = _mm256_setr_epi32(1, 2, 3, 4, 5, 6, 7, 8);
        _mm256_storeu_si256((__m256i *)got, twice(twice(v)));
        for (i = 0; i < 8; i++) if (got[i] != 4 * (i + 1)) return 0;
        return 1;
    }
}

#[test]
fn avx2_when_the_processor_has_it() {
    if !is_x86_feature_detected!("avx2") {
        return;
    }
    unsafe {
        assert_eq!(avx2_ops(), 1);
        assert_eq!(by_value(), 1);
    }
}

// ---------------------------------------------------------------------------
// the scalar bit-manipulation intrinsics
// ---------------------------------------------------------------------------

gnu11! {
    #include <immintrin.h>
    #include <stdint.h>

    static int popcount_scalar(unsigned int x) {
        int n = 0;
        while (x) { n += (int)(x & 1u); x >>= 1; }
        return n;
    }

    __attribute__((target("popcnt"))) int bits_popcnt(unsigned int x) {
        if (_popcnt32((int)x) != popcount_scalar(x)) return 0;
        /* `_mm_popcnt_u32` is the Intel spelling, a macro over `_popcnt32`. */
        if ((int)_mm_popcnt_u32(x) != popcount_scalar(x)) return 0;
        if (_popcnt64((long long)x) != popcount_scalar(x)) return 0;
        return 1;
    }

    __attribute__((target("lzcnt"))) int bits_lzcnt(void) {
        if (_lzcnt_u32(0x0000ffffu) != 16u) return 0;
        if (_lzcnt_u32(0u) != 32u) return 0;
        if (_lzcnt_u32(0x80000000u) != 0u) return 0;
        if (_lzcnt_u64(0x0000ffffu) != 48u) return 0;
        return 1;
    }

    __attribute__((target("bmi"))) int bits_bmi1(void) {
        if (_tzcnt_u32(0x00010000u) != 16u) return 0;
        if (_tzcnt_u32(0u) != 32u) return 0;
        if (_tzcnt_u64(0x0000000100000000ull) != 32u) return 0;
        /* andn is ~a & b, and blsr clears the lowest set bit. */
        if (_andn_u32(0x0fu, 0x3cu) != 0x30u) return 0;
        if (_blsr_u32(0x3cu) != 0x38u) return 0;
        if (_blsi_u32(0x3cu) != 0x04u) return 0;
        return 1;
    }

    __attribute__((target("bmi2"))) int bits_bmi2(void) {
        if (_pdep_u32(0x5u, 0xff00u) != 0x0500u) return 0;
        if (_pext_u32(0x12345678u, 0x0000ffffu) != 0x5678u) return 0;
        if (_pdep_u64(0x3ull, 0xf0f0ull) != 0x30ull) return 0;
        if (_pext_u64(0xfedcba9876543210ull, 0xffull) != 0x10ull) return 0;
        if (_bzhi_u32(0xffffffffu, 8u) != 0x000000ffu) return 0;
        return 1;
    }
}

#[test]
fn the_scalar_bit_intrinsics() {
    if is_x86_feature_detected!("popcnt") {
        for x in [0u32, 1, 0xdead_beef, u32::MAX, 0x8000_0000] {
            assert_eq!(unsafe { bits_popcnt(x) }, 1, "popcnt of {x:#x}");
        }
    }
    if is_x86_feature_detected!("lzcnt") {
        assert_eq!(unsafe { bits_lzcnt() }, 1);
    }
    if is_x86_feature_detected!("bmi1") {
        assert_eq!(unsafe { bits_bmi1() }, 1);
    }
    if is_x86_feature_detected!("bmi2") {
        assert_eq!(unsafe { bits_bmi2() }, 1);
    }
}

// ---------------------------------------------------------------------------
// the vector types as objects: unions, members, arrays, locals, statics
// ---------------------------------------------------------------------------

c11! {
    #include <immintrin.h>
    #include <stdint.h>

    /* The way every real code base reaches a lane without an intrinsic. */
    union pun {
        __m128i v;
        int32_t i[4];
        unsigned char b[16];
    };

    int punning(void) {
        union pun u;
        int i;
        u.v = _mm_setr_epi32(0x01020304, 0x05060708, 0x090a0b0c, 0x0d0e0f10);
        if (u.i[0] != 0x01020304 || u.i[3] != 0x0d0e0f10) return 0;
        /* little-endian lane order */
        if (u.b[0] != 0x04 || u.b[1] != 0x03 || u.b[15] != 0x0d) return 0;
        for (i = 0; i < 16; i++) u.b[i] = (unsigned char)(i + 1);
        if (_mm_cvtsi128_si32(u.v) != 0x04030201) return 0;
        if (sizeof(union pun) != 16 || _Alignof(union pun) != 16) return 0;
        return 1;
    }

    struct rows {
        __m128 first;
        __m128 rest[3];
        int tag;
    };

    int members(void) {
        struct rows r;
        float out[4];
        int i;
        r.tag = 7;
        r.first = _mm_setr_ps(1.0f, 2.0f, 3.0f, 4.0f);
        for (i = 0; i < 3; i++) r.rest[i] = _mm_set1_ps((float)(i + 1));
        _mm_storeu_ps(out, _mm_add_ps(r.first, r.rest[2]));
        for (i = 0; i < 4; i++) if (out[i] != (float)(i + 1) + 3.0f) return 0;
        if (r.tag != 7) return 0;
        /* 16 + 48 + 4, rounded up to the type's own alignment */
        if (sizeof(struct rows) != 80 || _Alignof(struct rows) != 16) return 0;
        return 1;
    }

    /* A vector object with static storage duration, zero-initialised, and one
     * a function fills in. */
    static __m128i saved;

    void remember(const int32_t *p) { saved = _mm_loadu_si128((const __m128i *)p); }
    int recall(void) { return _mm_cvtsi128_si32(saved); }

    /* The accumulator pattern: one vector local carried across a loop. */
    float dot(const float *a, const float *b, int n) {
        __m128 acc = _mm_setzero_ps();
        float tail[4];
        float sum;
        int i;
        for (i = 0; i + 4 <= n; i += 4) {
            __m128 va = _mm_loadu_ps(a + i);
            __m128 vb = _mm_loadu_ps(b + i);
            acc = _mm_add_ps(acc, _mm_mul_ps(va, vb));
        }
        _mm_storeu_ps(tail, acc);
        sum = tail[0] + tail[1] + tail[2] + tail[3];
        for (; i < n; i++) sum += a[i] * b[i];
        return sum;
    }

    float dot_scalar(const float *a, const float *b, int n) {
        float sum = 0.0f;
        int i;
        for (i = 0; i < n; i++) sum += a[i] * b[i];
        return sum;
    }
}

#[test]
fn the_types_as_objects() {
    unsafe {
        assert_eq!(punning(), 1);
        assert_eq!(members(), 1);
    }
}

#[test]
fn a_static_of_vector_type() {
    unsafe {
        assert_eq!(recall(), 0, "a static starts zeroed");
        let values: [i32; 4] = [0x1234_5678, 2, 3, 4];
        remember(values.as_ptr());
        assert_eq!(recall(), 0x1234_5678);
    }
}

#[test]
fn a_vector_accumulator_across_a_loop() {
    // Powers of two, so the two summation orders are bit-for-bit equal.
    let a: Vec<f32> = (0..11).map(|i| (1 << i) as f32).collect();
    let b: Vec<f32> = (0..11).map(|i| (1 << (10 - i)) as f32).collect();
    for n in 0..=11 {
        let vector = unsafe { dot(a.as_ptr(), b.as_ptr(), n) };
        let scalar = unsafe { dot_scalar(a.as_ptr(), b.as_ptr(), n) };
        assert_eq!(vector, scalar, "n = {n}");
    }
}

// ---------------------------------------------------------------------------
// run-time detection, and what the predefined macros say
// ---------------------------------------------------------------------------

c11! {
    #include <immintrin.h>

    /* `__builtin_cpu_init` is a no-op here — `std_detect` does its own lazy
     * detection — and `__builtin_cpu_supports` is
     * `std::is_x86_feature_detected!`. */
    int has(int which) {
        __builtin_cpu_init();
        switch (which) {
        case 0: return __builtin_cpu_supports("sse2") != 0;
        case 1: return __builtin_cpu_supports("avx2") != 0;
        case 2: return __builtin_cpu_supports("fma") != 0;
        case 3: return __builtin_cpu_supports("popcnt") != 0;
        /* GCC's own name for LZCNT and POPCNT together; LLVM splits them. */
        case 4: return __builtin_cpu_supports("abm") != 0;
        default: return -1;
        }
    }

    /* A procedural macro cannot see rustc's `-C target-feature`, so only the
     * x86-64 baseline is predefined and `#ifdef __AVX2__` takes the other
     * branch. The run-time question above is the one to ask. */
    int predefined(void) {
        int flags = 0;
    #ifdef __SSE__
        flags |= 1;
    #endif
    #ifdef __SSE2__
        flags |= 2;
    #endif
    #ifdef __SSE2_MATH__
        flags |= 4;
    #endif
    #ifdef __AVX2__
        flags |= 8;
    #endif
    #ifdef __MMX__
        flags |= 16;
    #endif
    #ifdef __x86_64__
        flags |= 32;
    #endif
        return flags;
    }
}

#[test]
fn builtin_cpu_supports_asks_the_processor() {
    unsafe {
        assert_eq!(has(0), 1, "every x86-64 processor has SSE2");
        assert_eq!(has(1), i32::from(is_x86_feature_detected!("avx2")));
        assert_eq!(has(2), i32::from(is_x86_feature_detected!("fma")));
        assert_eq!(has(3), i32::from(is_x86_feature_detected!("popcnt")));
        assert_eq!(
            has(4),
            i32::from(is_x86_feature_detected!("lzcnt") && is_x86_feature_detected!("popcnt"))
        );
    }
}

#[test]
fn only_the_baseline_is_predefined() {
    // SSE, SSE2, SSE2_MATH and __x86_64__; not AVX2, and not MMX, which has no
    // mapping at all.
    assert_eq!(unsafe { predefined() }, 1 | 2 | 4 | 32);
}

// ---------------------------------------------------------------------------
// `#pragma GCC target`, which asks for an instruction set for a whole region
// ---------------------------------------------------------------------------

gnu11! {
    #include <immintrin.h>
    #include <stdint.h>

    #pragma GCC push_options
    #pragma GCC target("avx2")

    int region_avx2(const int32_t *p) {
        __m256i v = _mm256_loadu_si256((const __m256i *)p);
        return _mm256_extract_epi32(_mm256_add_epi32(v, v), 7);
    }

    #pragma GCC target("fma")

    float region_avx2_and_fma(float a, float b, float c) {
        __m128 x = _mm_set_ss(a), y = _mm_set_ss(b), z = _mm_set_ss(c);
        return _mm_cvtss_f32(_mm_fmadd_ps(x, y, z));
    }

    #pragma GCC pop_options

    /* Outside the region again: this one carries no instruction set, and would
     * not compile if it took a 256-bit vector by value. */
    int after_the_region(const int32_t *p) { return (int)_mm_cvtsi128_si32(
        _mm_loadu_si128((const __m128i *)p)); }
}

#[test]
fn a_region_pragma_reaches_the_functions_after_it() {
    let values: [i32; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
    if is_x86_feature_detected!("avx2") {
        assert_eq!(unsafe { region_avx2(values.as_ptr()) }, 16);
    }
    if is_x86_feature_detected!("fma") {
        assert_eq!(unsafe { region_avx2_and_fma(3.0, 4.0, 5.0) }, 17.0);
    }
    assert_eq!(unsafe { after_the_region(values.as_ptr()) }, 1);
}

// ---------------------------------------------------------------------------
// the address of an intrinsic
// ---------------------------------------------------------------------------

gnu11! {
    #include <immintrin.h>
    #include <stdint.h>

    typedef __m128i (*binop)(__m128i, __m128i);

    /* GCC's intrinsics are `static inline` functions, so their address can be
     * taken; `core::arch`'s have the Rust ABI, so cinrs generates a private
     * `extern "C"` shim and hands out its address. The one that cannot work is
     * an intrinsic with an immediate operand, which is a diagnostic — see
     * tests/ui/simd_address_immediate.rs. */
    binop chosen(int which) { return which ? _mm_add_epi32 : &_mm_sub_epi32; }

    int through_a_pointer(int which) {
        int32_t out[4];
        binop f = chosen(which);
        _mm_storeu_si128((__m128i *)out, f(_mm_set1_epi32(10), _mm_set1_epi32(3)));
        return out[0];
    }
}

#[test]
fn the_address_of_an_intrinsic_is_a_shim() {
    unsafe {
        assert_eq!(through_a_pointer(1), 13);
        assert_eq!(through_a_pointer(0), 7);
        // The same shim both times: one item, two uses.
        assert!(chosen(1).is_some());
    }
}
