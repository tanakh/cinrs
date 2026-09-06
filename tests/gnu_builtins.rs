//! The `__builtin_*` forms.

use cinrs::{c11, c99};

// ---------------------------------------------------------------------------
// branch hints and the ones that end a function
// ---------------------------------------------------------------------------

c99! {
    #define likely(x)   __builtin_expect(!!(x), 1)
    #define unlikely(x) __builtin_expect(!!(x), 0)

    int classify(int n) {
        if (unlikely(n < 0)) return -1;
        if (likely(n > 0)) return 1;
        return 0;
    }

    /* `__builtin_expect` has the value of its first operand, and GCC gives it
     * the type `long`. */
    long expect_value(long n) { return __builtin_expect(n, 0); }
    long expect_probability(long n) {
        return __builtin_expect_with_probability(n, 0, 0.9);
    }

    /* Reaching this is undefined behaviour, which is what the C promised. */
    int never(int n) {
        switch (n) {
        case 0: return 10;
        case 1: return 20;
        default: __builtin_unreachable();
        }
    }
}

#[test]
fn the_branch_hints_are_transparent() {
    assert_eq!(unsafe { classify(-5) }, -1);
    assert_eq!(unsafe { classify(5) }, 1);
    assert_eq!(unsafe { classify(0) }, 0);
    assert_eq!(unsafe { expect_value(7) }, 7);
    assert_eq!(unsafe { expect_probability(7) }, 7);
    assert_eq!(unsafe { never(0) }, 10);
    assert_eq!(unsafe { never(1) }, 20);
}

// ---------------------------------------------------------------------------
// the questions about the program
// ---------------------------------------------------------------------------

c99! {
    #define is_constant(x) __builtin_constant_p(x)

    int constant_of_a_literal(void) { return is_constant(1 + 2 * 3); }
    int constant_of_a_variable(int n) { return is_constant(n); }

    /* A string literal lives in the constant pool, so GCC calls its address
     * — and a character read out of it at a constant index — constant. The
     * address of an object is not: the linker decides it. */
    int global;
    int constant_of_a_string(void) { return is_constant("hi"); }
    int constant_of_a_string_char(void) { return is_constant("hi"[1]); }
    int constant_of_an_address(void) { return is_constant(&global); }
    int constant_of_a_local_array(void) { char buf[4]; return is_constant(buf); }
    int constant_of_a_pointer_read(const char *p) { return is_constant(p[3]); }

    int same_type(void) { return __builtin_types_compatible_p(int, signed int); }
    int different_type(void) { return __builtin_types_compatible_p(int, long); }
    int pointer_types(void) {
        return __builtin_types_compatible_p(char *, char *)
             + __builtin_types_compatible_p(char *, const char *) * 2;
    }

    /* Only the chosen operand is even type checked, which is what makes this
     * usable in a macro that has to work for several types. */
    int chosen(void) {
        return __builtin_choose_expr(1, 42, "not an int at all");
    }
    int not_chosen(void) {
        return __builtin_choose_expr(0, "not an int at all", 42);
    }

    /* Without tracking object sizes there are exactly two honest answers. */
    unsigned long unknown_max(char *p) { return __builtin_object_size(p, 0); }
    unsigned long unknown_min(char *p) { return __builtin_object_size(p, 2); }
}

#[test]
fn the_questions_about_the_program_are_answered_at_compile_time() {
    assert_eq!(unsafe { constant_of_a_literal() }, 1);
    assert_eq!(unsafe { constant_of_a_variable(1) }, 0);
    assert_eq!(unsafe { constant_of_a_string() }, 1);
    assert_eq!(unsafe { constant_of_a_string_char() }, 1);
    assert_eq!(unsafe { constant_of_an_address() }, 0);
    assert_eq!(unsafe { constant_of_a_local_array() }, 0);
    assert_eq!(unsafe { constant_of_a_pointer_read(c"hi".as_ptr()) }, 0);
    assert_eq!(unsafe { same_type() }, 1);
    assert_eq!(unsafe { different_type() }, 0);
    // `char *` and `const char *` are different types, as C says.
    assert_eq!(unsafe { pointer_types() }, 1);
    assert_eq!(unsafe { chosen() }, 42);
    assert_eq!(unsafe { not_chosen() }, 42);
    assert_eq!(unsafe { unknown_max(core::ptr::null_mut()) }, u64::MAX);
    assert_eq!(unsafe { unknown_min(core::ptr::null_mut()) }, 0);
}

// ---------------------------------------------------------------------------
// bit manipulation
// ---------------------------------------------------------------------------

c99! {
    int popcount(unsigned int n) { return __builtin_popcount(n); }
    int popcountl(unsigned long n) { return __builtin_popcountl(n); }
    int popcountll(unsigned long long n) { return __builtin_popcountll(n); }
    int clz(unsigned int n) { return __builtin_clz(n); }
    int clzll(unsigned long long n) { return __builtin_clzll(n); }
    int ctz(unsigned int n) { return __builtin_ctz(n); }
    int ctzll(unsigned long long n) { return __builtin_ctzll(n); }
    int ffs(int n) { return __builtin_ffs(n); }
    int ffsll(long long n) { return __builtin_ffsll(n); }
    int parity(unsigned int n) { return __builtin_parity(n); }
    int clrsb(int n) { return __builtin_clrsb(n); }
    unsigned short bswap16(unsigned short n) { return __builtin_bswap16(n); }
    unsigned int bswap32(unsigned int n) { return __builtin_bswap32(n); }
    unsigned long long bswap64(unsigned long long n) { return __builtin_bswap64(n); }
}

#[test]
fn the_bit_builtins_agree_with_rusts_integer_methods() {
    assert_eq!(unsafe { popcount(0b1011_0110) }, 5);
    assert_eq!(unsafe { popcountl(u64::MAX) }, 64);
    assert_eq!(unsafe { popcountll(0) }, 0);
    // Undefined for zero in C, so only non-zero operands are asked about.
    assert_eq!(unsafe { clz(1) }, 31);
    assert_eq!(unsafe { clz(0x8000_0000) }, 0);
    assert_eq!(unsafe { clzll(1) }, 63);
    assert_eq!(unsafe { ctz(1) }, 0);
    assert_eq!(unsafe { ctz(0x8000_0000) }, 31);
    assert_eq!(unsafe { ctzll(1 << 40) }, 40);
    assert_eq!(unsafe { ffs(0) }, 0);
    assert_eq!(unsafe { ffs(1) }, 1);
    assert_eq!(unsafe { ffs(0b1000) }, 4);
    assert_eq!(unsafe { ffs(i32::MIN) }, 32);
    assert_eq!(unsafe { ffsll(0) }, 0);
    assert_eq!(unsafe { ffsll(1 << 40) }, 41);
    assert_eq!(unsafe { parity(0b111) }, 1);
    assert_eq!(unsafe { parity(0b1111) }, 0);
    // The number of leading bits that repeat the sign bit, not counting it.
    assert_eq!(unsafe { clrsb(0) }, 31);
    assert_eq!(unsafe { clrsb(-1) }, 31);
    assert_eq!(unsafe { clrsb(1) }, 30);
    assert_eq!(unsafe { clrsb(i32::MIN) }, 0);
    assert_eq!(unsafe { bswap16(0x1234) }, 0x3412);
    assert_eq!(unsafe { bswap32(0x1234_5678) }, 0x7856_3412);
    assert_eq!(
        unsafe { bswap64(0x0123_4567_89ab_cdef) },
        0xefcd_ab89_6745_2301
    );
}

/// The same builtins over a constant operand are *constants*, which is what
/// lets them stand where C asks for an integer constant expression.
///
/// GCC and Clang both fold them; WG14 DR263 as Clang tests it (`drs/dr2xx.c`)
/// writes `_Static_assert(__builtin_popcount(0) < 1, …)`, and the width the
/// answer depends on is the one the builtin's suffix chose. `clz` and `ctz`
/// are undefined for a zero operand and are left to the generated code there,
/// exactly as they are at run time.
#[test]
fn the_bit_builtins_fold_to_constants() {
    c11! {
        _Static_assert(__builtin_popcount(0) < 1, "zero is not all zero bits");
        _Static_assert(__builtin_popcount(0xffu) == 8, "");
        _Static_assert(__builtin_popcountll(~0ull) == 64, "");
        _Static_assert(__builtin_clz(1u) == 31, "");
        _Static_assert(__builtin_clzll(1ull) == 63, "");
        _Static_assert(__builtin_ctz(0x8000u) == 15, "");
        _Static_assert(__builtin_ffs(0) == 0, "");
        _Static_assert(__builtin_ffs(8) == 4, "");
        _Static_assert(__builtin_parity(7) == 1, "");
        _Static_assert(__builtin_clrsb(1) == 30, "");
        _Static_assert(__builtin_bswap16(0x1234) == 0x3412, "");
        _Static_assert(__builtin_bswap32(1) == 0x1000000, "");

        /* An array bound and an enumerator are the other two places a
           constant expression has to be one. */
        int sized[__builtin_popcount(11) + 1];
        enum Bits { LOW = __builtin_ctz(0x40u) };

        unsigned long how_many(void) { return sizeof sized / sizeof sized[0]; }
        int low(void) { return LOW; }
    }

    unsafe {
        assert_eq!(how_many(), 4);
        assert_eq!(low(), 6);
    }
}

// ---------------------------------------------------------------------------
// the overflow builtins
// ---------------------------------------------------------------------------

c99! {
    int add_int(int a, int b, int *out) { return __builtin_add_overflow(a, b, out); }
    int sub_int(int a, int b, int *out) { return __builtin_sub_overflow(a, b, out); }
    int mul_int(int a, int b, int *out) { return __builtin_mul_overflow(a, b, out); }

    /* The result type is the pointer's, whatever the operands' types are. */
    int add_narrow(int a, int b, signed char *out) {
        return __builtin_add_overflow(a, b, out);
    }
    int add_unsigned(int a, int b, unsigned char *out) {
        return __builtin_add_overflow(a, b, out);
    }
    int mul_wide(long long a, long long b, long long *out) {
        return __builtin_mul_overflow(a, b, out);
    }

    /* The `_p` forms ask without storing. */
    int would_overflow(int a, int b) { return __builtin_add_overflow_p(a, b, (short) 0); }

    /* And the typed spellings, which fix the result type in the name. */
    int sadd(int a, int b, int *out) { return __builtin_sadd_overflow(a, b, out); }
    int umulll(unsigned long long a, unsigned long long b, unsigned long long *out) {
        return __builtin_umulll_overflow(a, b, out);
    }
}

#[test]
fn the_overflow_builtins_compute_in_infinite_precision() {
    let mut out = 0;
    assert_eq!(unsafe { add_int(1, 2, &raw mut out) }, 0);
    assert_eq!(out, 3);
    assert_eq!(unsafe { add_int(i32::MAX, 1, &raw mut out) }, 1);
    assert_eq!(out, i32::MIN);
    assert_eq!(unsafe { sub_int(i32::MIN, 1, &raw mut out) }, 1);
    assert_eq!(out, i32::MAX);
    assert_eq!(unsafe { mul_int(65536, 65536, &raw mut out) }, 1);
    assert_eq!(out, 0);
    assert_eq!(unsafe { mul_int(3, 5, &raw mut out) }, 0);
    assert_eq!(out, 15);

    // A narrower result type overflows sooner, even when the operands do not.
    let mut narrow: i8 = 0;
    assert_eq!(unsafe { add_narrow(100, 100, &raw mut narrow) }, 1);
    assert_eq!(narrow, -56);
    assert_eq!(unsafe { add_narrow(100, 20, &raw mut narrow) }, 0);
    assert_eq!(narrow, 120);
    let mut byte: u8 = 0;
    assert_eq!(unsafe { add_unsigned(-1, 0, &raw mut byte) }, 1);
    assert_eq!(unsafe { add_unsigned(200, 55, &raw mut byte) }, 0);
    assert_eq!(byte, 255);

    // A multiplication that overflows `i128` itself is still an overflow.
    let mut wide = 0;
    assert_eq!(unsafe { mul_wide(i64::MAX, i64::MAX, &raw mut wide) }, 1);
    assert_eq!(unsafe { mul_wide(i64::MAX, 1, &raw mut wide) }, 0);
    assert_eq!(wide, i64::MAX);

    assert_eq!(unsafe { would_overflow(30_000, 30_000) }, 1);
    assert_eq!(unsafe { would_overflow(1, 2) }, 0);

    assert_eq!(unsafe { sadd(i32::MAX, 1, &raw mut out) }, 1);
    let mut u = 0u64;
    assert_eq!(unsafe { umulll(u64::MAX, 2, &raw mut u) }, 1);
    assert_eq!(unsafe { umulll(3, 4, &raw mut u) }, 0);
    assert_eq!(u, 12);
}

// ---------------------------------------------------------------------------
// floating constants, hints and the library builtins
// ---------------------------------------------------------------------------

c99! {
    double huge(void) { return __builtin_huge_val(); }
    float hugef(void) { return __builtin_huge_valf(); }
    double infinity(void) { return __builtin_inf(); }
    double not_a_number(void) { return __builtin_nan(""); }
    float not_a_numberf(void) { return __builtin_nanf(""); }

    void hint(const char *p) { __builtin_prefetch(p, 0, 3); }
    void *aligned(void *p) { return __builtin_assume_aligned(p, 16); }

    /* A library builtin is a call to the library function, declared into the
     * unit if the header that would have declared it was not included. */
    int compare(const char *a, const char *b) { return __builtin_strcmp(a, b); }
    unsigned long length(const char *s) { return __builtin_strlen(s); }
    void copy(void *dst, const void *src, unsigned long n) {
        __builtin_memcpy(dst, src, n);
    }
    int magnitude(int n) { return __builtin_abs(n); }
    double root(double x) { return __builtin_sqrt(x); }

    int line_of_this(void) { return __builtin_LINE(); }
    const char *function_of_this(void) { return __builtin_FUNCTION(); }
}

#[test]
fn the_constants_hints_and_library_builtins() {
    assert!(unsafe { huge() }.is_infinite());
    assert!(unsafe { hugef() }.is_infinite());
    assert!(unsafe { infinity() }.is_infinite());
    assert!(unsafe { not_a_number() }.is_nan());
    assert!(unsafe { not_a_numberf() }.is_nan());

    unsafe { hint(c"x".as_ptr()) };
    let mut buf = [1u8, 2, 3, 4];
    assert_eq!(
        unsafe { aligned(buf.as_mut_ptr().cast()) },
        buf.as_mut_ptr().cast()
    );

    assert_eq!(unsafe { compare(c"a".as_ptr(), c"a".as_ptr()) }, 0);
    assert!(unsafe { compare(c"a".as_ptr(), c"b".as_ptr()) } < 0);
    assert_eq!(unsafe { length(c"hello".as_ptr()) }, 5);
    let mut dst = [0u8; 4];
    unsafe { copy(dst.as_mut_ptr().cast(), buf.as_ptr().cast(), 4) };
    assert_eq!(dst, [1, 2, 3, 4]);
    assert_eq!(unsafe { magnitude(-9) }, 9);
    assert_eq!(unsafe { root(16.0) }, 4.0);

    // `__builtin_LINE` and `__builtin_FUNCTION` report the *use*.
    assert!(unsafe { line_of_this() } > 0);
    let name = unsafe { core::ffi::CStr::from_ptr(function_of_this()) };
    assert_eq!(name.to_bytes(), b"function_of_this");
}

// ---------------------------------------------------------------------------
// `__builtin_trap`
// ---------------------------------------------------------------------------

c99! {
    /* `abort` is what the C library gives a translated program; GCC's own
     * trap instruction has no stable Rust counterpart. */
    void die(int n) { if (n) __builtin_trap(); }
}

#[test]
fn trap_is_reachable_only_when_asked_for() {
    unsafe { die(0) };
}

// ---------------------------------------------------------------------------
// the quiet comparisons (C99 7.12.14)
// ---------------------------------------------------------------------------

c99! {
    int unordered(double x, double y) { return __builtin_isunordered(x, y); }
    int greater(double x, double y) { return __builtin_isgreater(x, y); }
    int greater_equal(double x, double y) { return __builtin_isgreaterequal(x, y); }
    int less(double x, double y) { return __builtin_isless(x, y); }
    int less_equal(double x, double y) { return __builtin_islessequal(x, y); }
    int less_greater(double x, double y) { return __builtin_islessgreater(x, y); }

    /* Mixed widths go through the usual arithmetic conversions. */
    int mixed(float x, double y) { return __builtin_isless(x, y); }
}

#[test]
fn the_quiet_comparisons_answer_no_to_every_nan() {
    let nan = f64::NAN;
    unsafe {
        assert_eq!(unordered(nan, 1.0), 1);
        assert_eq!(unordered(1.0, nan), 1);
        assert_eq!(unordered(1.0, 2.0), 0);

        assert_eq!(greater(2.0, 1.0), 1);
        assert_eq!(greater(1.0, 2.0), 0);
        assert_eq!(greater(nan, 1.0), 0);
        assert_eq!(greater_equal(1.0, 1.0), 1);
        assert_eq!(greater_equal(nan, nan), 0);
        assert_eq!(less(1.0, 2.0), 1);
        assert_eq!(less(nan, 2.0), 0);
        assert_eq!(less_equal(1.0, 1.0), 1);
        assert_eq!(less_equal(1.0, nan), 0);

        // `x < y || x > y`, which is `!=` without the NaN case.
        assert_eq!(less_greater(1.0, 2.0), 1);
        assert_eq!(less_greater(1.0, 1.0), 0);
        assert_eq!(less_greater(nan, 1.0), 0);

        assert_eq!(mixed(1.0, 2.0), 1);
    }
}

// ---------------------------------------------------------------------------
// classification, sign and magnitude
// ---------------------------------------------------------------------------

c99! {
    int nan_p(double x) { return __builtin_isnan(x); }
    int nan_pf(float x) { return __builtin_isnanf(x); }
    int inf_p(double x) { return __builtin_isinf(x); }
    int inf_sign(double x) { return __builtin_isinf_sign(x); }
    int finite_p(double x) { return __builtin_isfinite(x); }
    int normal_p(double x) { return __builtin_isnormal(x); }
    int signalling_p(double x) { return __builtin_issignaling(x); }
    int sign_bit(double x) { return __builtin_signbit(x); }
    int sign_bitf(float x) { return __builtin_signbitf(x); }

    /* The five answers are the operands, which is how <math.h>'s own
     * `fpclassify` names its `FP_*` macros. */
    int classify_fp(double x) { return __builtin_fpclassify(0, 1, 2, 3, 4, x); }

    double magnitude_d(double x) { return __builtin_fabs(x); }
    float magnitude_f(float x) { return __builtin_fabsf(x); }
    /* `long double` is `double`, so the `l` form is the plain one. */
    double magnitude_l(double x) { return __builtin_fabsl(x); }
    double with_sign(double x, double y) { return __builtin_copysign(x, y); }
    float with_signf(float x, float y) { return __builtin_copysignf(x, y); }

    double signalling(void) { return __builtin_nans(""); }
    double payload(void) { return __builtin_nan("0x123"); }
    double payload_decimal(void) { return __builtin_nan("291"); }
    float payloadf(void) { return __builtin_nansf("0x123"); }
    double infinity_l(void) { return __builtin_infl(); }
    double huge_l(void) { return __builtin_huge_vall(); }

    /* A payload NaN is a constant, so it may initialise an object with static
     * storage duration. */
    static double static_payload = __builtin_nan("0x1");
    double read_static_payload(void) { return static_payload; }
}

#[test]
fn the_classification_builtins() {
    let nan = f64::NAN;
    let inf = f64::INFINITY;
    unsafe {
        assert_eq!(nan_p(nan), 1);
        assert_eq!(nan_p(1.0), 0);
        assert_eq!(nan_pf(f32::NAN), 1);
        assert_eq!(inf_p(inf), 1);
        assert_eq!(inf_p(-inf), 1);
        assert_eq!(inf_p(1.0), 0);
        assert_eq!(inf_sign(inf), 1);
        assert_eq!(inf_sign(-inf), -1);
        assert_eq!(inf_sign(1.0), 0);
        assert_eq!(finite_p(1.0), 1);
        assert_eq!(finite_p(inf), 0);
        assert_eq!(normal_p(1.0), 1);
        assert_eq!(normal_p(0.0), 0);
        assert_eq!(normal_p(f64::from_bits(1)), 0); // subnormal
        assert_eq!(sign_bit(-0.0), 1);
        assert_eq!(sign_bit(-1.0), 1);
        assert_eq!(sign_bit(1.0), 0);
        assert_eq!(sign_bitf(-0.0), 1);

        assert_eq!(classify_fp(nan), 0);
        assert_eq!(classify_fp(inf), 1);
        assert_eq!(classify_fp(1.0), 2);
        assert_eq!(classify_fp(f64::from_bits(1)), 3);
        assert_eq!(classify_fp(0.0), 4);
    }
}

#[test]
fn magnitude_sign_and_the_named_nans() {
    unsafe {
        assert_eq!(magnitude_d(-3.5), 3.5);
        assert_eq!(magnitude_f(-3.5), 3.5);
        assert_eq!(magnitude_l(-3.5), 3.5);
        // `fabs` clears the sign bit, which is exact for a zero and a NaN.
        assert!(magnitude_d(-0.0).is_sign_positive());
        assert!(magnitude_d(f64::NAN).is_nan());

        assert_eq!(with_sign(3.5, -1.0), -3.5);
        assert_eq!(with_sign(-3.5, 1.0), 3.5);
        assert!(with_sign(0.0, -1.0).is_sign_negative());
        assert_eq!(with_signf(3.5, -1.0), -3.5);

        // The payload is part of the value, so the bits are what GCC's are.
        assert_eq!(signalling().to_bits(), 0x7ff4_0000_0000_0000);
        assert_eq!(payload().to_bits(), 0x7ff8_0000_0000_0123);
        // The string is read the way `strtoull` reads it, in base 0.
        assert_eq!(payload_decimal().to_bits(), payload().to_bits());
        assert_eq!(payloadf().to_bits(), 0x7f80_0123);
        assert!(signalling().is_nan());
        assert_eq!(signalling_p(signalling()), 1);
        assert_eq!(signalling_p(f64::NAN), 0);

        assert!(infinity_l().is_infinite());
        assert!(huge_l().is_infinite());
        assert_eq!(read_static_payload().to_bits(), 0x7ff8_0000_0000_0001);
    }
}

// ---------------------------------------------------------------------------
// `__builtin_classify_type`
// ---------------------------------------------------------------------------

c99! {
    struct point { int x; };
    union thing { int i; };
    enum colour { red };

    /* The operand is not evaluated, and the type it is classified by is the
     * one the default argument promotions give it — which is why `char`, an
     * enumeration and an array answer as `int` and as a pointer. */
    int class_of_int(void) { int x = 0; return __builtin_classify_type(x); }
    int class_of_char(void) { char c = 0; return __builtin_classify_type(c); }
    int class_of_enum(void) { enum colour e = red; return __builtin_classify_type(e); }
    int class_of_double(void) { return __builtin_classify_type(1.0); }
    int class_of_float(void) { return __builtin_classify_type(1.0f); }
    int class_of_pointer(void) { char *p = 0; return __builtin_classify_type(p); }
    int class_of_array(void) { char a[4]; return __builtin_classify_type(a); }
    int class_of_struct(void) { struct point p; return __builtin_classify_type(p); }
    int class_of_union(void) { union thing u; return __builtin_classify_type(u); }
    int class_of_function(void) {
        return __builtin_classify_type(class_of_function);
    }

    /* An integer constant expression, so it may be an array bound. */
    int class_is_constant(void) {
        int a[__builtin_classify_type(1.0) == 8 ? 3 : 1];
        return sizeof a / sizeof a[0];
    }
}

#[test]
fn classify_type_answers_gccs_numbers() {
    unsafe {
        assert_eq!(class_of_int(), 1);
        assert_eq!(class_of_char(), 1);
        assert_eq!(class_of_enum(), 1);
        assert_eq!(class_of_double(), 8);
        assert_eq!(class_of_float(), 8);
        assert_eq!(class_of_pointer(), 5);
        assert_eq!(class_of_array(), 5);
        assert_eq!(class_of_struct(), 12);
        assert_eq!(class_of_union(), 13);
        assert_eq!(class_of_function(), 5);
        assert_eq!(class_is_constant(), 3);
    }
}

// ---------------------------------------------------------------------------
// the library builtins that declare the function themselves
// ---------------------------------------------------------------------------

c99! {
    /* No `#include <stdio.h>`: GCC knows the prototype of every function it
     * has a builtin for, and declares it on the spot. A `#include` that
     * arrives later redeclares it compatibly. */
    int say(const char *text) { return __builtin_printf("%s", text); }
    int into(char *buf, const char *text) { return __builtin_sprintf(buf, "[%s]", text); }
    int into_n(char *buf, unsigned long n, int value) {
        return __builtin_snprintf(buf, n, "%d", value);
    }

    /* The GNU string functions, which `<string.h>` does not declare. */
    void *after(void *dst, const void *src, unsigned long n) {
        return __builtin_mempcpy(dst, src, n);
    }
    char *end_of(char *dst, const char *src) { return __builtin_stpcpy(dst, src); }
    void zero(void *p, unsigned long n) { __builtin_bzero(p, n); }
    char *first(const char *s, int c) { return __builtin_index(s, c); }

    /* A `long double` maths builtin is the `double` one: `long double` *is*
     * `double` here, and calling the platform's `sqrtl` would pass it an
     * eighty-bit value the generated Rust cannot make. */
    double root_l(double x) { return __builtin_sqrtl(x); }
    double power_l(double x, double y) { return __builtin_powl(x, y); }
    double biggest(double x, double y) { return __builtin_fmaxl(x, y); }

    /* And the ones that were simply missing a prototype. */
    double cube_root(double x) { return __builtin_cbrt(x); }
    double scaled(double x, int n) { return __builtin_ldexp(x, n); }
    double fused(double x, double y, double z) { return __builtin_fma(x, y, z); }
    double rounded(double x) { return __builtin_nearbyint(x); }
    long widest_abs(long n) { return __builtin_imaxabs(n); }
}

#[test]
fn a_library_builtin_declares_its_own_function() {
    let mut buf = [0u8; 32];
    unsafe {
        assert_eq!(say(c"".as_ptr()), 0);
        assert_eq!(into(buf.as_mut_ptr().cast(), c"hi".as_ptr()), 4);
        assert_eq!(&buf[..4], b"[hi]");
        assert_eq!(into_n(buf.as_mut_ptr().cast(), buf.len() as _, 42), 2);
        assert_eq!(&buf[..2], b"42");

        let src = [1u8, 2, 3, 4];
        let mut dst = [0u8; 4];
        let end = after(dst.as_mut_ptr().cast(), src.as_ptr().cast(), 4);
        assert_eq!(end, dst.as_mut_ptr().wrapping_add(4).cast());
        assert_eq!(dst, [1, 2, 3, 4]);

        let mut text = [0u8; 8];
        let end = end_of(text.as_mut_ptr().cast(), c"abc".as_ptr());
        assert_eq!(end, text.as_mut_ptr().wrapping_add(3).cast());
        zero(dst.as_mut_ptr().cast(), 4);
        assert_eq!(dst, [0, 0, 0, 0]);
        assert_eq!(
            first(c"abc".as_ptr(), b'b' as _),
            c"abc".as_ptr().add(1) as _
        );

        assert_eq!(root_l(16.0), 4.0);
        assert_eq!(power_l(2.0, 10.0), 1024.0);
        assert_eq!(biggest(1.0, 2.0), 2.0);
        assert_eq!(cube_root(27.0), 3.0);
        assert_eq!(scaled(1.5, 3), 12.0);
        assert_eq!(fused(2.0, 3.0, 4.0), 10.0);
        assert_eq!(rounded(2.5), 2.0);
        assert_eq!(widest_abs(-9), 9);
    }
}

// ---------------------------------------------------------------------------
// `__has_builtin` answers about *this* implementation
// ---------------------------------------------------------------------------

c99! {
    int knows_isunordered(void) {
    #if __has_builtin(__builtin_isunordered)
        return 1;
    #else
        return 0;
    #endif
    }
    int knows_sqrtl(void) {
    #if __has_builtin(__builtin_sqrtl)
        return 1;
    #else
        return 0;
    #endif
    }
    int knows_return_address(void) {
    #if __has_builtin(__builtin_return_address)
        return 1;
    #else
        return 0;
    #endif
    }
}

#[test]
fn has_builtin_follows_what_is_implemented() {
    unsafe {
        assert_eq!(knows_isunordered(), 1);
        assert_eq!(knows_sqrtl(), 1);
        assert_eq!(knows_return_address(), 0);
    }
}
