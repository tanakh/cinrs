//! Integration tests that *run* GCC's vector extensions on vector types of
//! the program's own: `typedef int v4si __attribute__((vector_size(16)));`.
//!
//! Each vector is a `#[repr(C, align(N))]` struct over its elements in the
//! generated Rust, and each operator is written out element by element; see
//! `crates/cinrs-core/src/sema/gnu_vector.rs`. As in `tests/simd.rs`, every
//! expected value is computed in the same C block, in scalar C, so each
//! function answers 1 only if the vector operator and the scalar one agree —
//! and the same C, with a `main` added, answers 1 everywhere under `gcc -O2`
//! (gcc 15.2) too.
//!
//! What must not compile — what GCC refuses, and a vector passed by value to
//! a function the unit does not define — is in `tests/ui/gnu_vector_errors.rs`.

use cinrs::gnu11;

gnu11! {
    #include <string.h>

    typedef int v4si __attribute__((vector_size(16)));
    typedef unsigned int v4su __attribute__((vector_size(16)));
    typedef signed char v16qs __attribute__((vector_size(16)));
    typedef unsigned char v16qu __attribute__((vector_size(16)));
    typedef short v8hi __attribute__((vector_size(16)));
    typedef long long v2di __attribute__((vector_size(16)));
    typedef unsigned long long v2du __attribute__((vector_size(16)));
    typedef float v4sf __attribute__((vector_size(16)));
    typedef double v2df __attribute__((vector_size(16)));
    typedef float v8sf __attribute__((vector_size(32)));
    typedef int v2si __attribute__((vector_size(8)));

    /* Every integer operator, element by element, against the scalar
     * operator on the same elements. */
    int integer_operators(void) {
        v4si a = {7, -9, 100000, -2147483647 - 1};
        v4si b = {3, 4, -7, -1};
        v4si r;
        int i;
        r = a + b; for (i = 0; i < 4; i++) if (r[i] != (int)((unsigned)a[i] + (unsigned)b[i])) return 0;
        r = a - b; for (i = 0; i < 4; i++) if (r[i] != (int)((unsigned)a[i] - (unsigned)b[i])) return 0;
        r = a * b; for (i = 0; i < 4; i++) if (r[i] != (int)((unsigned)a[i] * (unsigned)b[i])) return 0;
        b[3] = 2;
        r = a / b; for (i = 0; i < 4; i++) if (r[i] != a[i] / b[i]) return 0;
        r = a % b; for (i = 0; i < 4; i++) if (r[i] != a[i] % b[i]) return 0;
        r = a & b; for (i = 0; i < 4; i++) if (r[i] != (a[i] & b[i])) return 0;
        r = a | b; for (i = 0; i < 4; i++) if (r[i] != (a[i] | b[i])) return 0;
        r = a ^ b; for (i = 0; i < 4; i++) if (r[i] != (a[i] ^ b[i])) return 0;
        r = -b; for (i = 0; i < 4; i++) if (r[i] != -b[i]) return 0;
        r = ~a; for (i = 0; i < 4; i++) if (r[i] != ~a[i]) return 0;
        r = +a; for (i = 0; i < 4; i++) if (r[i] != a[i]) return 0;
        return 1;
    }

    /* Narrow elements wrap in their own width. */
    int narrow_elements_wrap(void) {
        v16qu a = {250, 1, 2, 255}, b = {10, 255, 3, 1};
        v16qu r = a + b;
        v8hi s = {32767, -32768, 5}, t = {1, -1, 6};
        v8hi u = s + t, w = s * t;
        return r[0] == 4 && r[1] == 0 && r[2] == 5 && r[3] == 0 && r[4] == 0
            && u[0] == -32768 && u[1] == 32767 && u[2] == 11
            && w[0] == 32767 && w[1] == -32768 && w[2] == 30;
    }

    /* A scalar operand is broadcast, on either side. */
    int scalars_are_broadcast(void) {
        v4si a = {1, 2, 3, 4};
        v4si r = a * 3 + 1, s = 10 - a, t = 100 / a;
        v4sf f = {1.5f, 2.5f, -3.0f, 0.25f};
        v4sf g = f * 2.0f, h = 1.0f / f;
        v16qs c = {1, -2, 3};
        v16qs d = c + 'a';
        int i;
        for (i = 0; i < 4; i++) {
            if (r[i] != a[i] * 3 + 1 || s[i] != 10 - a[i] || t[i] != 100 / a[i]) return 0;
            if (g[i] != f[i] * 2.0f || h[i] != 1.0f / f[i]) return 0;
        }
        return d[0] == 'b' && d[1] == 'a' - 2 && d[2] == 'd' && d[3] == 'a';
    }

    /* Shifts by a scalar and by a vector of counts. */
    int shifts(void) {
        v4si a = {1, -16, 0x40000000, 7};
        v4su u = {1u, 0x80000000u, 3u, 0xffffffffu};
        v4si n = {0, 2, 1, 31};
        v4si l = a << 2, r = a >> n;
        v4su ur = u >> 1, ul = u << (v4su){31, 0, 2, 4};
        int i;
        for (i = 0; i < 4; i++) {
            if (l[i] != (int)((unsigned)a[i] << 2)) return 0;
            if (r[i] != a[i] >> n[i]) return 0;
            if (ur[i] != u[i] >> 1) return 0;
        }
        return ul[0] == 0x80000000u && ul[1] == 0x80000000u && ul[2] == 12u && ul[3] == 0xfffffff0u;
    }

    /* A comparison is -1 where it holds and 0 where it does not, converts to
     * any integer vector of its shape, and selects with & and |. */
    int comparisons(void) {
        v4si a = {1, 5, -3, 7}, b = {2, 5, -4, 0};
        v4si lt = a < b, eq = a == b, ge = a >= b, ne = a != b;
        v4sf x = {1.0f, 2.0f, 3.0f, 4.0f}, y = {4.0f, 2.0f, 1.0f, 0.0f};
        v4si fl = x <= y;
        v2df p = {0.5, -1.0}, q = {0.5, 1.0};
        v2di dl = p < q;
        v4su uns = (v4su){1, 0xffffffffu, 3, 0} > (v4su){0, 1, 3, 1};
        v4su inverted = ~(a <= b);
        v4si m = a > b;
        v4si max = (a & m) | (b & ~m);
        int i;
        for (i = 0; i < 4; i++) {
            if (lt[i] != -(a[i] < b[i]) || eq[i] != -(a[i] == b[i])) return 0;
            if (ge[i] != -(a[i] >= b[i]) || ne[i] != -(a[i] != b[i])) return 0;
            if (fl[i] != -(x[i] <= y[i])) return 0;
            if (inverted[i] != (a[i] <= b[i] ? 0u : 0xffffffffu)) return 0;
            if (max[i] != (a[i] > b[i] ? a[i] : b[i])) return 0;
        }
        return dl[0] == 0 && dl[1] == -1 && uns[0] == 0xffffffffu && uns[1] == 0xffffffffu
            && uns[2] == 0 && uns[3] == 0;
    }

    /* Compound assignment, `++` and `--`, and stores into one element. */
    int assignments(void) {
        v4si a = {1, 2, 3, 4};
        v4si *p = &a;
        a += 10;
        a *= (v4si){1, 2, 3, 4};
        *p -= 1;
        a <<= 1;
        a[2] = 5;
        a[3] += 100;
        p[0][1] ^= 1;
        ++a;
        v4si old = a--;
        return a[0] == 20 && a[1] == 47 && a[2] == 5 && a[3] == 210 && old[0] == 21;
    }

    /* Brace initialisers fill the rest with zeros, an array of vectors may
     * leave its inner braces out, and a vector can be static: Redis's
     * `crccombine.c` keeps a table of masks. */
    static v4si table[3] = { {1, 2}, {3, 4, 5, 6} };
    static v2du masks[4] = { {0, 0}, {-1, 0}, {0, -1}, {-1, -1} };
    int initialisers(void) {
        v4si a = {9};
        v4si elided[2] = {1, 2, 3, 4, 5};
        static v4sf k = {0.5f, 1.5f};
        return a[0] == 9 && a[1] == 0 && a[3] == 0
            && table[0][1] == 2 && table[0][2] == 0 && table[1][3] == 6 && table[2][0] == 0
            && masks[1][0] == 0xffffffffffffffffull && masks[1][1] == 0 && masks[3][1] != 0
            && elided[0][3] == 4 && elided[1][0] == 5 && elided[1][1] == 0
            && k[1] == 1.5f && k[2] == 0.0f;
    }

    /* Redis's `gf2_matrix_times_vec2`: a `uint64_t` matrix read two rows at a
     * time as vectors, masked by a table and summed with `^`. */
    unsigned long long gf2_times(unsigned long long *mat, unsigned long long vec) {
        v2du sum = {0, 0}, *mv2 = (v2du *)mat;
        int i;
        for (i = 0; i < 16; i++) {
            sum ^= (*mv2++) & masks[vec & 3];
            vec >>= 2;
        }
        return sum[0] ^ sum[1];
    }
    int redis_crc_shape(void) {
        /* One past an even index: the rows are 8-byte aligned only. */
        unsigned long long storage[33], *mat = storage + 1, want = 0, vec = 0x9e3779b97f4a7c15ull;
        int i;
        for (i = 0; i < 32; i++) mat[i] = 0x0123456789abcdefull * (unsigned long long)(i + 3);
        for (i = 0; i < 32; i++) if (vec >> i & 1) want ^= mat[i];
        return gf2_times(mat, vec) == want;
    }

    /* GCC's size and alignment, an `aligned` typedef that lowers it, and the
     * `V4SI` machine mode. */
    typedef float v8sf_a16 __attribute__((vector_size(32), aligned(16)));
    typedef int v4si_mode __attribute__((mode(V4SI)));
    int layout(void) {
        struct s { char c; v4si v; } s;
        int *pv __attribute__((vector_size(16)));
        (void)s; (void)pv;
        return sizeof(v4si) == 16 && _Alignof(v4si) == 16 && sizeof(v2si) == 8 && _Alignof(v2si) == 8
            && sizeof(v8sf) == 32 && sizeof(v8sf_a16) == 32 && _Alignof(v8sf_a16) == 16
            && sizeof(struct s) == 32 && __builtin_offsetof(struct s, v) == 16
            && sizeof(*pv) == 16 && sizeof(v4si_mode) == 16
            && __builtin_types_compatible_p(v4si_mode, v4si);
    }

    /* A cast between two vectors of one size, or a vector and an integer,
     * keeps the bytes. */
    int casts_keep_the_bytes(void) {
        v4si a = {1, 2, 3, 4};
        v2di d = (v2di)a;
        v16qu b = (v16qu)a;
        v2si s = {-1, 2};
        long long whole = (long long)s;
        v2si back = (v2si)whole;
        v4sf f = {1.0f};
        v4si bits = (v4si)f;
        long long lo;
        memcpy(&lo, &a, 8);
        return d[0] == lo && b[0] == 1 && b[4] == 2 && b[1] == 0
            && back[0] == -1 && back[1] == 2 && bits[0] == 0x3f800000;
    }

    /* `__builtin_convertvector` converts each element. */
    int convertvector(void) {
        typedef short v4hi __attribute__((vector_size(8)));
        v4si a = {1, -2, 300, 7};
        v4sf f = __builtin_convertvector(a, v4sf);
        v4sf g = {1.75f, -2.5f, 3.0f, 1e9f};
        v4si h = __builtin_convertvector(g, v4si);
        v4hi n = __builtin_convertvector(a, v4hi);
        return f[0] == 1.0f && f[1] == -2.0f && f[2] == 300.0f && h[0] == 1 && h[1] == -2
            && h[3] == 1000000000 && n[2] == 300 && sizeof(n) == 8;
    }

    /* `__builtin_shuffle` with one vector and with two; the index is taken
     * modulo the number of elements. */
    int shuffles(void) {
        v4si a = {10, 20, 30, 40}, b = {50, 60, 70, 80};
        v4si r = __builtin_shuffle(a, (v4si){3, 2, 1, 0});
        v4si s = __builtin_shuffle(a, b, (v4si){0, 4, 7, 9});
        v4si t = __builtin_shuffle(a, (v4si){5, -1, 2, 6});
        return r[0] == 40 && r[3] == 10 && s[0] == 10 && s[1] == 50 && s[2] == 80 && s[3] == 20
            && t[0] == 20 && t[1] == 40 && t[2] == 30 && t[3] == 30;
    }

    /* Vectors pass by value between the unit's own functions and as members,
     * and go through pointers that need not be aligned to the vector. */
    struct pair { v4sf lo, hi; };
    static v4sf madd(v4sf a, v4sf b, v4sf c) { return a * b + c; }
    static struct pair swap(struct pair p) { struct pair q = { p.hi, p.lo }; return q; }
    int functions_and_memory(void) {
        v4sf r = madd((v4sf){1, 2, 3, 4}, (v4sf){2, 2, 2, 2}, (v4sf){0.5f});
        struct pair p = { {1}, {2} };
        struct pair q = swap(p);
        float buf[9] = {0, 1, 2, 3, 4, 5, 6, 7, 8};
        v4sf *u = (v4sf *)(buf + 1);
        v4sf loaded = *u;
        *u = loaded * 2;
        union { v4si v; int i[4]; } pun = { .v = {5, 6, 7, 8} };
        return r[0] == 2.5f && r[3] == 8.0f && q.lo[0] == 2.0f && q.hi[0] == 1.0f
            && loaded[0] == 1.0f && buf[1] == 2.0f && buf[4] == 8.0f && buf[5] == 5.0f
            && pun.i[2] == 7;
    }

    /* The 32-byte vectors are plain structs too: no AVX needed. */
    int wide_vectors(void) {
        v8sf a = {1, 2, 3, 4, 5, 6, 7, 8};
        v8sf b = a * a - 1;
        int i;
        for (i = 0; i < 8; i++) if (b[i] != a[i] * a[i] - 1) return 0;
        return 1;
    }

    /* The extension's own spellings answer as GCC's do. */
    #if __has_attribute(vector_size) && __has_builtin(__builtin_convertvector)
    #if __has_builtin(__builtin_shuffle)
    int has(void) { return 1; }
    #else
    int has(void) { return 0; }
    #endif
    #else
    int has(void) { return 0; }
    #endif
}

#[test]
fn the_operators_work_element_by_element() {
    unsafe {
        assert_eq!(integer_operators(), 1);
        assert_eq!(narrow_elements_wrap(), 1);
        assert_eq!(scalars_are_broadcast(), 1);
        assert_eq!(shifts(), 1);
        assert_eq!(comparisons(), 1);
        assert_eq!(assignments(), 1);
        assert_eq!(wide_vectors(), 1);
    }
}

#[test]
fn initialisers_layout_and_casts() {
    unsafe {
        assert_eq!(initialisers(), 1);
        assert_eq!(layout(), 1);
        assert_eq!(casts_keep_the_bytes(), 1);
        assert_eq!(has(), 1);
    }
}

#[test]
fn the_builtins() {
    unsafe {
        assert_eq!(convertvector(), 1);
        assert_eq!(shuffles(), 1);
    }
}

#[test]
fn functions_members_and_unaligned_memory() {
    unsafe {
        assert_eq!(functions_and_memory(), 1);
        assert_eq!(redis_crc_shape(), 1);
    }
}

/// The Rust side sees the struct a vector is generated as, with its
/// elements in a public array.
#[test]
fn a_vector_is_a_struct_rust_can_read() {
    assert_eq!(std::mem::size_of::<v4si>(), 16);
    assert_eq!(std::mem::align_of::<v4si>(), 16);
    assert_eq!(std::mem::align_of::<v8sf_a16>(), 16);
    let mut storage = [0u64; 33];
    for (i, row) in storage[1..].iter_mut().enumerate() {
        *row = 0x0123_4567_89ab_cdef_u64.wrapping_mul(i as u64 + 3);
    }
    let want = storage[1..]
        .iter()
        .enumerate()
        .filter(|(i, _)| 0x9e37_79b9_7f4a_7c15_u64 >> i & 1 == 1)
        .fold(0, |acc, (_, row)| acc ^ row);
    let got = unsafe { gf2_times(storage[1..].as_mut_ptr(), 0x9e37_79b9_7f4a_7c15) };
    assert_eq!(got, want);
}

/// `__m128i` is GCC's two `long long`s, so it converts to and from that
/// vector without a cast, and with one to any other of its size.
#[cfg(target_arch = "x86_64")]
mod intel {
    use cinrs::gnu11;

    gnu11! {
        #include <emmintrin.h>
        typedef long long v2di __attribute__((vector_size(16)));
        typedef int v4si __attribute__((vector_size(16)));

        int intel_interop(void) {
            v4si a = {1, 2, 3, 4}, b = {10, 20, 30, 40};
            v4si sum = (v4si)_mm_add_epi32((__m128i)a, (__m128i)b);
            v2di wide = {5, 6};
            __m128i m = wide;
            v2di back = m + 1;
            __m128i zeros[4] = { 0U };
            return sum[0] == 11 && sum[3] == 44 && back[0] == 6 && back[1] == 7
                && _mm_cvtsi128_si32(zeros[3]) == 0 && _mm_cvtsi128_si32(m) == 5;
        }
    }

    #[test]
    fn intel_types_are_gccs_vectors() {
        assert_eq!(unsafe { intel_interop() }, 1);
    }
}
