//! Integration tests that *run* translated C using GNU's `__int128`.
//!
//! The type is spelled with a leading double underscore, so — like every other
//! GNU extension spelled that way — it works in the strict entry points too,
//! exactly as it does in GCC's own `-std=c99`. It is generated as Rust's
//! `i128`/`u128`, whose x86-64 ABI has matched `__int128`'s since Rust 1.77.
//!
//! Every expected value here comes from C's own rules: 128-bit wrap-around for
//! the unsigned type, the usual arithmetic conversions ranking `__int128`
//! above `long long`, and truncation towards zero on a conversion to an
//! integer.

use cinrs::{c11, c99, gnu11};

// ---------------------------------------------------------------------------
// the reason the type exists: a 64x64 -> 128 product
// ---------------------------------------------------------------------------

#[test]
fn the_high_half_of_a_64_by_64_product() {
    c99! {
        unsigned long long mul_high(unsigned long long a, unsigned long long b) {
            unsigned __int128 product = (unsigned __int128) a * b;
            return (unsigned long long) (product >> 64);
        }

        unsigned long long mul_low(unsigned long long a, unsigned long long b) {
            unsigned __int128 product = (unsigned __int128) a * b;
            return (unsigned long long) product;
        }

        long long smul_high(long long a, long long b) {
            __int128 product = (__int128) a * b;
            return (long long) (product >> 64);
        }
    }

    for (a, b) in [
        (0u64, 0u64),
        (1, 1),
        (u64::MAX, u64::MAX),
        (0x0123_4567_89ab_cdef, 0xfedc_ba98_7654_3210),
        (1 << 63, 2),
    ] {
        let product = u128::from(a) * u128::from(b);
        assert_eq!(unsafe { mul_high(a, b) }, (product >> 64) as u64);
        assert_eq!(unsafe { mul_low(a, b) }, product as u64);
    }

    for (a, b) in [(-1i64, 1i64), (i64::MIN, i64::MIN), (3, -5), (i64::MAX, 2)] {
        let product = i128::from(a) * i128::from(b);
        assert_eq!(unsafe { smul_high(a, b) }, (product >> 64) as i64);
    }
}

// ---------------------------------------------------------------------------
// arithmetic
// ---------------------------------------------------------------------------

#[test]
fn unsigned_arithmetic_wraps_the_way_c_says() {
    c99! {
        unsigned __int128 add(unsigned __int128 a, unsigned __int128 b) { return a + b; }
        unsigned __int128 sub(unsigned __int128 a, unsigned __int128 b) { return a - b; }
        unsigned __int128 mul(unsigned __int128 a, unsigned __int128 b) { return a * b; }
        unsigned __int128 all_ones(void) { return ~(unsigned __int128) 0; }
        unsigned __int128 wraps(void) { return ~(unsigned __int128) 0 + 1; }
    }

    unsafe {
        assert_eq!(all_ones(), u128::MAX);
        // Unsigned overflow is defined: modulo 2^128.
        assert_eq!(wraps(), 0);
        assert_eq!(add(u128::MAX, 3), 2);
        assert_eq!(sub(0, 1), u128::MAX);
        assert_eq!(mul(1 << 100, 1 << 30), 0);
        // 3 * 2^126 is 2^127 + 2^126, which still fits.
        assert_eq!(mul(3, 1 << 126), (1u128 << 127) + (1u128 << 126));
        // 5 * 2^126 does not, and wraps to 2^126.
        assert_eq!(mul(5, 1 << 126), 1u128 << 126);
    }
}

#[test]
fn division_and_remainder_follow_the_signedness() {
    c99! {
        unsigned __int128 udiv(unsigned __int128 a, unsigned __int128 b) { return a / b; }
        unsigned __int128 urem(unsigned __int128 a, unsigned __int128 b) { return a % b; }
        __int128 sdiv(__int128 a, __int128 b) { return a / b; }
        __int128 srem(__int128 a, __int128 b) { return a % b; }

        /* Folded at expansion time rather than at run time; the answers must
           be the same. */
        unsigned __int128 folded_udiv(void) { return (~(unsigned __int128) 0) / 3; }
        __int128 folded_srem(void) { return ((__int128) -7) % 3; }
    }

    unsafe {
        assert_eq!(udiv(u128::MAX, 3), u128::MAX / 3);
        assert_eq!(urem(u128::MAX, 7), u128::MAX % 7);
        // The signed quotient truncates towards zero, which is what C says.
        assert_eq!(sdiv(-7, 2), -3);
        assert_eq!(srem(-7, 2), -1);
        assert_eq!(sdiv(i128::MIN + 1, -1), i128::MAX);

        assert_eq!(folded_udiv(), u128::MAX / 3);
        assert_eq!(folded_srem(), -1);
    }
}

/// `-2^127` is the one value a signed 128-bit type can hold and whose
/// negation, quotient by `-1` and remainder by `-1` it cannot. C leaves all
/// three undefined; the *folder* has to wrap rather than abort the macro, and
/// these are the only constants that can reach it.
#[test]
fn the_most_negative_value_folds_rather_than_aborting() {
    c11! {
        #define MIN (((__int128) 1) << 127)

        /* Folded at expansion time: a `static` initialiser and a `case` label
           are both constant expressions. */
        static __int128 divided = MIN / -1;
        static __int128 remainder = MIN % -1;
        static __int128 negated = -MIN;
        _Static_assert(MIN < 0, "the top bit is the sign bit");
        _Static_assert((MIN >> 127) == -1, "an arithmetic shift, all the way");
        _Static_assert(MIN / -1 == MIN, "the quotient wraps");

        __int128 the_min(void) { return MIN; }
        __int128 take_divided(void) { return divided; }
        __int128 take_remainder(void) { return remainder; }
        __int128 take_negated(void) { return negated; }
        /* At run time the same negation is `wrapping_neg`, so it is defined
           here too. */
        __int128 negate(__int128 v) { return -v; }
    }

    unsafe {
        assert_eq!(the_min(), i128::MIN);
        assert_eq!(take_divided(), i128::MIN);
        assert_eq!(take_remainder(), 0);
        assert_eq!(take_negated(), i128::MIN);
        assert_eq!(negate(i128::MIN), i128::MIN);
        assert_eq!(negate(1), -1);
    }
}

#[test]
fn shifts_are_arithmetic_or_logical_by_the_type() {
    c99! {
        __int128 sshr(__int128 v, int by) { return v >> by; }
        unsigned __int128 ushr(unsigned __int128 v, int by) { return v >> by; }
        __int128 shl(__int128 v, int by) { return v << by; }

        /* C has no 128-bit literal, so this is the idiom for one. */
        __int128 bit_100(void) { return ((__int128) 1) << 100; }
        unsigned __int128 top_bit(void) { return ((unsigned __int128) 1) << 127; }
    }

    unsafe {
        assert_eq!(sshr(-8, 2), -2);
        assert_eq!(sshr(i128::MIN, 127), -1);
        assert_eq!(ushr(u128::MAX, 127), 1);
        assert_eq!(ushr(1u128 << 127, 127), 1);
        assert_eq!(shl(1, 100), 1i128 << 100);
        assert_eq!(bit_100(), 1i128 << 100);
        assert_eq!(top_bit(), 1u128 << 127);
    }
}

#[test]
fn comparisons_know_which_type_they_are_in() {
    c99! {
        int uless(unsigned __int128 a, unsigned __int128 b) { return a < b; }
        int sless(__int128 a, __int128 b) { return a < b; }

        /* Folded: `-1` as an `unsigned __int128` is the largest value there
           is, and comparing it as a signed number would say the opposite. */
        int folded_unsigned(void) { return ((unsigned __int128) -1) > 1; }
        int folded_signed(void) { return ((__int128) -1) > 1; }

        /* The usual arithmetic conversions: `__int128` outranks `long long`,
           so the `long long` is converted and the comparison is signed. */
        int mixed(long long a, __int128 b) { return a < b; }
    }

    unsafe {
        assert_eq!(uless(1, u128::MAX), 1);
        assert_eq!(uless(u128::MAX, 1), 0);
        assert_eq!(sless(-1, 1), 1);
        assert_eq!(sless(i128::MIN, i128::MAX), 1);
        assert_eq!(folded_unsigned(), 1);
        assert_eq!(folded_signed(), 0);
        assert_eq!(mixed(-1, 0), 1);
    }
}

// ---------------------------------------------------------------------------
// conversions
// ---------------------------------------------------------------------------

#[test]
fn conversions_to_and_from_every_other_scalar() {
    c99! {
        __int128 from_int(int v) { return v; }
        __int128 from_ulong(unsigned long v) { return v; }
        int to_int(__int128 v) { return (int) v; }
        unsigned char to_uchar(unsigned __int128 v) { return (unsigned char) v; }
        long long to_llong(__int128 v) { return (long long) v; }

        double to_double(__int128 v) { return (double) v; }
        double utod(unsigned __int128 v) { return (double) v; }
        __int128 from_double(double d) { return (__int128) d; }
        unsigned __int128 from_double_u(double d) { return (unsigned __int128) d; }

        int to_bool(__int128 v) { return !!v; }

        /* A pointer round trip: `__int128` is wide enough for any address. */
        __int128 pointer_bits(void *p) { return (__int128) (unsigned long) p; }
        void *back(__int128 bits) { return (void *) (unsigned long) bits; }
    }

    unsafe {
        assert_eq!(from_int(-3), -3i128);
        assert_eq!(from_ulong(u64::MAX), i128::from(u64::MAX));
        assert_eq!(to_int(1i128 << 70 | 5), 5);
        assert_eq!(to_uchar(u128::MAX), 0xff);
        assert_eq!(to_llong(-1), -1);

        assert_eq!(to_double(1i128 << 100), (1u128 << 100) as f64);
        assert_eq!(to_double(-1), -1.0);
        assert_eq!(utod(u128::MAX), u128::MAX as f64);
        assert_eq!(from_double(1e30), 1e30f64 as i128);
        assert_eq!(from_double(-2.9), -2);
        assert_eq!(from_double_u(1e30), 1e30f64 as u128);

        assert_eq!(to_bool(0), 0);
        assert_eq!(to_bool(1i128 << 127), 1);

        let mut value = 5i32;
        let p: *mut core::ffi::c_void = (&raw mut value).cast();
        assert_eq!(back(pointer_bits(p)), p);
    }
}

// ---------------------------------------------------------------------------
// layout
// ---------------------------------------------------------------------------

#[test]
fn a_struct_with_an_int128_member_matches_rusts_layout() {
    c11! {
        struct Wide {
            int tag;
            unsigned __int128 value;
        };

        unsigned long size(void) { return sizeof (struct Wide); }
        unsigned long align(void) { return _Alignof(struct Wide); }
        unsigned long offset(void) { return __builtin_offsetof(struct Wide, value); }
        unsigned long scalar_size(void) { return sizeof (__int128); }
        unsigned long scalar_align(void) { return _Alignof(unsigned __int128); }

        struct Wide make(int tag, unsigned __int128 value) {
            struct Wide w = { tag, value };
            return w;
        }
    }

    unsafe {
        // What C says has to be what Rust laid out.
        assert_eq!(size() as usize, core::mem::size_of::<Wide>());
        assert_eq!(align() as usize, core::mem::align_of::<Wide>());
        assert_eq!(offset() as usize, core::mem::offset_of!(Wide, value));
        assert_eq!(scalar_size(), 16);
        assert_eq!(scalar_align() as usize, core::mem::align_of::<u128>());

        let w = make(1, u128::MAX);
        assert_eq!(w.tag, 1);
        assert_eq!(w.value, u128::MAX);
    }
}

/// GCC allows a bit-field of a wider type than standard C does, and 70 bits of
/// `unsigned __int128` is one no `u64` window could hold.
#[test]
fn a_bit_field_wider_than_sixty_four_bits() {
    gnu11! {
        struct Packed {
            unsigned __int128 wide : 70;
            __int128 narrow : 100;
            int tail : 3;
        };

        unsigned __int128 read_wide(struct Packed *p) { return p->wide; }
        void write_wide(struct Packed *p, unsigned __int128 v) { p->wide = v; }
        __int128 read_narrow(struct Packed *p) { return p->narrow; }
        void write_narrow(struct Packed *p, __int128 v) { p->narrow = v; }
        int read_tail(struct Packed *p) { return p->tail; }
        void write_tail(struct Packed *p, int v) { p->tail = v; }
    }

    // The bit-field storage is a private `[u8; K]`, so the object starts out
    // as C's own all-bits-zero rather than being written field by field.
    let mut p: Packed = unsafe { core::mem::zeroed() };
    unsafe {
        let mask70 = (1u128 << 70) - 1;
        write_wide(&raw mut p, u128::MAX);
        // Only seventy bits are kept, and reading back does not sign-extend.
        assert_eq!(read_wide(&raw mut p), mask70);

        write_narrow(&raw mut p, -1);
        // A signed field of a hundred bits reads back sign-extended.
        assert_eq!(read_narrow(&raw mut p), -1);
        write_narrow(&raw mut p, 1i128 << 98);
        assert_eq!(read_narrow(&raw mut p), 1i128 << 98);

        write_tail(&raw mut p, -1);
        assert_eq!(read_tail(&raw mut p), -1);

        // Neither neighbour disturbed the other.
        assert_eq!(read_wide(&raw mut p), mask70);
        assert_eq!(read_narrow(&raw mut p), 1i128 << 98);
    }
}

// ---------------------------------------------------------------------------
// the rest of the language
// ---------------------------------------------------------------------------

#[test]
fn passing_and_returning_across_the_abi() {
    c99! {
        __int128 twice(__int128 v) { return v * 2; }

        __int128 sum(__int128 a, __int128 b, __int128 c) { return a + b + c; }

        typedef __int128 (*Op)(__int128);
        __int128 apply(Op op, __int128 v) { return op(v); }
        Op the_op(void) { return twice; }
    }

    unsafe {
        assert_eq!(twice(1i128 << 100), 1i128 << 101);
        assert_eq!(sum(1, 2, 3), 6);
        assert_eq!(apply(the_op(), 21), 42);
        // …and called from Rust, which is the same ABI.
        let op = the_op().expect("the function pointer is not null");
        assert_eq!(op(-5), -10);
    }
}

#[test]
fn generic_selection_picks_the_128_bit_arm() {
    c11! {
        /* Written on one line: a `\` continuation is not something the Rust
           lexer accepts, so raw-token form has no way to spell one. */
        #define KIND(x) _Generic((x), __int128: 1, unsigned __int128: 2, long long: 3, default: 0)

        int of_signed(void) { __int128 v = 0; return KIND(v); }
        int of_unsigned(void) { unsigned __int128 v = 0; return KIND(v); }
        int of_long_long(void) { long long v = 0; return KIND(v); }
        int of_typedef(void) { __int128_t v = 0; return KIND(v); }
        int of_utypedef(void) { __uint128_t v = 0; return KIND(v); }
    }

    unsafe {
        assert_eq!(of_signed(), 1);
        assert_eq!(of_unsigned(), 2);
        assert_eq!(of_long_long(), 3);
        // `__int128_t` and `__uint128_t` are the compiler's own names for the
        // very same two types, so `_Generic` cannot tell them apart.
        assert_eq!(of_typedef(), 1);
        assert_eq!(of_utypedef(), 2);
    }
}

/// The checked-overflow builtins compute one width up from their operands, so
/// a 128-bit operand is refused — but a 128-bit *result* is not, and it is the
/// shape real code uses.
#[test]
fn the_overflow_builtins_take_a_128_bit_result() {
    gnu11! {
        int mul_into_wide(int a, int b, unsigned __int128 *out) {
            return __builtin_mul_overflow(a, b, out);
        }

        int mul_into_signed_wide(long long a, long long b, __int128 *out) {
            return __builtin_mul_overflow(a, b, out);
        }
    }

    unsafe {
        let mut out: u128 = 0;
        // No overflow: the exact product is non-negative and fits.
        assert_eq!(mul_into_wide(1 << 30, 4, &raw mut out), 0);
        assert_eq!(out, 1u128 << 32);
        // A negative exact answer does not fit an unsigned result, so the
        // builtin says so — and stores the wrapped value, as GCC does.
        assert_eq!(mul_into_wide(4, -16, &raw mut out), 1);
        assert_eq!(out, (-64i128) as u128);

        let mut signed: i128 = 0;
        // Two `long long`s cannot make a product an `__int128` misses.
        assert_eq!(mul_into_signed_wide(i64::MAX, i64::MAX, &raw mut signed), 0);
        assert_eq!(signed, i128::from(i64::MAX) * i128::from(i64::MAX));
        assert_eq!(mul_into_signed_wide(-3, 5, &raw mut signed), 0);
        assert_eq!(signed, -15);
    }
}

#[test]
fn the_preprocessor_knows_the_type_is_there() {
    c99! {
        #ifdef __SIZEOF_INT128__
        int sizeof_int128(void) { return __SIZEOF_INT128__; }
        #else
        int sizeof_int128(void) { return 0; }
        #endif

        #if __SIZEOF_INT128__ == 16
        int is_sixteen(void) { return 1; }
        #else
        int is_sixteen(void) { return 0; }
        #endif
    }

    unsafe {
        assert_eq!(sizeof_int128(), 16);
        assert_eq!(is_sixteen(), 1);
    }
}

#[test]
fn static_objects_and_constant_expressions() {
    c11! {
        static unsigned __int128 counter = ~(unsigned __int128) 0;
        static __int128 low = ((__int128) 1) << 100;

        _Static_assert(sizeof (__int128) == 16, "sixteen bytes");
        _Static_assert(((unsigned __int128) -1) > 0, "unsigned is unsigned");
        _Static_assert((((__int128) 1) << 100) > 0, "the shift is in 128 bits");

        unsigned __int128 take_counter(void) { return counter; }
        __int128 take_low(void) { return low; }

        int classify(unsigned __int128 v) {
            switch (v) {
            case 0: return 0;
            case 1: return 1;
            default: return -1;
            }
        }
    }

    unsafe {
        assert_eq!(take_counter(), u128::MAX);
        assert_eq!(take_low(), 1i128 << 100);
        assert_eq!(classify(0), 0);
        assert_eq!(classify(1), 1);
        assert_eq!(classify(u128::MAX), -1);
    }
}

// ---------------------------------------------------------------------------
// variadic arguments
// ---------------------------------------------------------------------------

/// A 128-bit value *passes* through `...` like anything else — that is the
/// call site, and the ABI. Reading one back out with `va_arg` is the part
/// stable Rust cannot do; `tests/ui/gnu_int128_errors.rs` has the diagnostic.
#[rustversion::since(1.99)]
#[test]
fn a_128_bit_argument_passes_through_an_ellipsis() {
    c99! {
        #include <stdarg.h>

        /* Read back as two `unsigned long long` halves, which is what a
           program without `va_arg(ap, __int128)` has to do anyway. */
        unsigned long long low_half(int n, ...) {
            va_list ap;
            va_start(ap, n);
            unsigned long long v = va_arg(ap, unsigned long long);
            va_end(ap);
            return v;
        }

        unsigned long long call_it(void) {
            unsigned __int128 wide = ((unsigned __int128) 7 << 64) | 42;
            return low_half(1, wide);
        }
    }

    // The low half of the 128-bit argument is the first eight bytes of it.
    assert_eq!(unsafe { call_it() }, 42);
}
