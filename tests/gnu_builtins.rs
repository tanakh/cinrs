//! The `__builtin_*` forms.

use cinrs::c99;

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
