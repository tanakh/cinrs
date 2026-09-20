//! Integration tests that *run* the translated C.
//!
//! Every expected value here was derived from the C standard's semantics —
//! integer promotions, the usual arithmetic conversions, truncation towards
//! zero, wrap-around on unsigned overflow — rather than from what the
//! implementation happens to produce.
//!
//! Each test holds its own `c99!` invocation, which is one translation unit;
//! putting it inside the test function keeps the generated items local, so
//! names never collide between tests. Calls are `unsafe` because `c99!`
//! defines `extern "C"` functions.

use cinrs::{c11, c99};

// ---------------------------------------------------------------------------
// the README example
// ---------------------------------------------------------------------------

#[test]
fn readme_factorial() {
    c99! {
        int fact(int n) {
            if (n == 0) {
                return 1;
            } else {
                return n * fact(n - 1);
            }
        }
    }

    assert_eq!(unsafe { fact(0) }, 1);
    assert_eq!(unsafe { fact(1) }, 1);
    assert_eq!(unsafe { fact(5) }, 120);
    assert_eq!(unsafe { fact(10) }, 3_628_800);
}

// ---------------------------------------------------------------------------
// loops
// ---------------------------------------------------------------------------

#[test]
fn for_loop_with_a_c99_declaration() {
    c99! {
        int fib(int n) {
            int a = 0, b = 1;
            for (int i = 0; i < n; i++) {
                int next = a + b;
                a = b;
                b = next;
            }
            return a;
        }
    }

    assert_eq!(unsafe { fib(0) }, 0);
    assert_eq!(unsafe { fib(1) }, 1);
    assert_eq!(unsafe { fib(10) }, 55);
    assert_eq!(unsafe { fib(20) }, 6765);
}

#[test]
fn while_loop() {
    c99! {
        int gcd(int a, int b) {
            while (b != 0) {
                int t = b;
                b = a % b;
                a = t;
            }
            return a;
        }
    }

    assert_eq!(unsafe { gcd(48, 18) }, 6);
    assert_eq!(unsafe { gcd(17, 5) }, 1);
    assert_eq!(unsafe { gcd(270, 192) }, 6);
}

#[test]
fn loop_with_shifts_and_compound_assignment() {
    c99! {
        long ipow(long base, int exp) {
            long result = 1;
            while (exp > 0) {
                if (exp & 1) {
                    result *= base;
                }
                base *= base;
                exp >>= 1;
            }
            return result;
        }
    }

    assert_eq!(unsafe { ipow(2, 10) }, 1024);
    assert_eq!(unsafe { ipow(3, 5) }, 243);
    assert_eq!(unsafe { ipow(7, 0) }, 1);
}

#[test]
fn do_while_with_continue_retests_the_condition() {
    c99! {
        int steps(int n) {
            int count = 0;
            do {
                n--;
                if (n % 2 != 0) {
                    continue;
                }
                count++;
            } while (n > 0);
            return count;
        }
    }

    // n = 1: one iteration leaves n = 0, `continue` is not taken, and the
    // condition `0 > 0` ends the loop.
    assert_eq!(unsafe { steps(1) }, 1);
    // n = 5: n runs 4, 3, 2, 1, 0; the odd values take `continue`, which must
    // still evaluate `n > 0` — were it to skip the test, this would not
    // terminate at all.
    assert_eq!(unsafe { steps(5) }, 3);
}

#[test]
fn do_while_whose_continue_ends_the_loop() {
    c99! {
        int once(int n) {
            int count = 0;
            do {
                n--;
                if (n % 2 == 0) {
                    continue;
                }
                count++;
            } while (n > 0);
            return count;
        }
    }

    // The single iteration leaves n = 0 and takes `continue`; the condition is
    // then false, so the loop ends without ever incrementing `count`.
    assert_eq!(unsafe { once(1) }, 0);
}

#[test]
fn nested_loops_with_break() {
    c99! {
        int count_pairs(int limit) {
            int count = 0;
            for (int i = 1; i <= limit; i++) {
                for (int j = 1; j <= limit; j++) {
                    if (i * j > 6) {
                        break;
                    }
                    count++;
                }
            }
            return count;
        }
    }

    // limit = 4 admits (1,1..4), (2,1..3), (3,1..2) and (4,1): ten pairs.
    assert_eq!(unsafe { count_pairs(4) }, 10);
    assert_eq!(unsafe { count_pairs(1) }, 1);
}

#[test]
fn an_infinite_loop_left_by_break() {
    c99! {
        int first_multiple(int of, int above) {
            int n = above;
            for (;;) {
                n++;
                if (n % of == 0) {
                    break;
                }
            }
            return n;
        }
    }

    assert_eq!(unsafe { first_multiple(7, 10) }, 14);
    assert_eq!(unsafe { first_multiple(3, 0) }, 3);
}

// ---------------------------------------------------------------------------
// switch
// ---------------------------------------------------------------------------

#[test]
fn switch_with_fallthrough() {
    c99! {
        int classify(int n) {
            int out = 0;
            switch (n) {
                case 0:
                case 1:
                    out += 1;
                case 2:
                    out += 10;
                    break;
                default:
                    out = -1;
            }
            return out;
        }
    }

    assert_eq!(unsafe { classify(0) }, 11);
    assert_eq!(unsafe { classify(1) }, 11);
    assert_eq!(unsafe { classify(2) }, 10);
    assert_eq!(unsafe { classify(9) }, -1);
}

#[test]
fn switch_with_default_in_the_middle() {
    c99! {
        int middle(int n) {
            int out = 0;
            switch (n) {
                case 1: out += 1;
                default: out += 2;
                case 3: out += 4;
            }
            return out;
        }
    }

    assert_eq!(unsafe { middle(1) }, 7);
    assert_eq!(unsafe { middle(2) }, 6);
    assert_eq!(unsafe { middle(3) }, 4);
    assert_eq!(unsafe { middle(99) }, 6);
}

#[test]
fn switch_without_a_default() {
    c99! {
        int lookup(int n) {
            int out = 0;
            switch (n) {
                case 1: out = 10; break;
                case 2: out = 20; break;
            }
            return out;
        }
    }

    assert_eq!(unsafe { lookup(1) }, 10);
    assert_eq!(unsafe { lookup(2) }, 20);
    assert_eq!(unsafe { lookup(3) }, 0);
}

#[test]
fn a_switch_may_be_the_whole_body_of_a_function() {
    // Every group ends the function and there is a `default`, so control
    // cannot fall out of the statement: nothing follows it, and the generated
    // Rust has to be accepted without a `return` after it.
    c99! {
        int sign(int n) {
            switch (n) {
                case 0: return 0;
                case 1: case 2: return 1;
                default: return -1;
            }
        }

        /* A group that ends in a loop with no exit terminates just as much. */
        int spin(int n) {
            switch (n) {
                case 0: return 0;
                default: while (1) { n++; if (n == 100) return n; }
            }
        }
    }

    unsafe {
        assert_eq!(sign(0), 0);
        assert_eq!(sign(2), 1);
        assert_eq!(sign(9), -1);
        assert_eq!(spin(0), 0);
        assert_eq!(spin(50), 100);
    }
}

#[test]
fn break_inside_a_loop_inside_a_switch_leaves_the_loop() {
    c99! {
        int inner(int n) {
            int total = 0;
            switch (n) {
                case 1:
                    for (int i = 0; i < 5; i++) {
                        if (i == 3) {
                            break;
                        }
                        total += i;
                    }
                    total += 100;
                    break;
                default:
                    total = -1;
            }
            return total;
        }
    }

    // The inner `break` leaves the `for`, so `total += 100` still runs.
    assert_eq!(unsafe { inner(1) }, 103);
    assert_eq!(unsafe { inner(2) }, -1);
}

#[test]
fn switch_inside_a_loop() {
    c99! {
        int cycle(int n) {
            int total = 0;
            for (int i = 0; i < n; i++) {
                switch (i % 3) {
                    case 0: total += 1; break;
                    case 1: total += 10; break;
                    default: total += 100;
                }
            }
            return total;
        }
    }

    assert_eq!(unsafe { cycle(6) }, 222);
    assert_eq!(unsafe { cycle(1) }, 1);
}

#[test]
fn switch_on_a_character() {
    c99! {
        int vowel(char c) {
            switch (c) {
                case 'a':
                case 'e':
                case 'i':
                case 'o':
                case 'u':
                    return 1;
                default:
                    return 0;
            }
        }
    }

    assert_eq!(unsafe { vowel(b'e' as ::core::ffi::c_char) }, 1);
    assert_eq!(unsafe { vowel(b'z' as ::core::ffi::c_char) }, 0);
}

// ---------------------------------------------------------------------------
// arithmetic
// ---------------------------------------------------------------------------

#[test]
fn unsigned_arithmetic_wraps() {
    c99! {
        unsigned wrap_down(void) {
            unsigned x = 0;
            x = x - 1;
            return x;
        }

        unsigned wrap_up(unsigned x) {
            return x + 1;
        }
    }

    assert_eq!(unsafe { wrap_down() }, 4_294_967_295);
    assert_eq!(unsafe { wrap_up(4_294_967_295) }, 0);
}

#[test]
fn signed_overflow_wraps_rather_than_panicking() {
    c99! {
        int overflow(int x) {
            return x + 1;
        }
    }

    // C leaves this undefined; wrapping is the predictable choice, and it is
    // what a release build of C code does in practice.
    assert_eq!(unsafe { overflow(2_147_483_647) }, -2_147_483_648);
}

#[test]
fn comparing_a_signed_value_with_an_unsigned_one() {
    c99! {
        int literal_compare(void) {
            unsigned u = 1;
            return -1 < u;
        }

        int variable_compare(int s, unsigned u) {
            return s < u;
        }
    }

    // `-1` converts to `4294967295u`, which is not less than 1.
    assert_eq!(unsafe { literal_compare() }, 0);
    assert_eq!(unsafe { variable_compare(-1, 1) }, 0);
    assert_eq!(unsafe { variable_compare(1, 2) }, 1);
}

#[test]
fn the_integer_promotions_widen_before_arithmetic() {
    c99! {
        int promote(void) {
            unsigned char a = 200, b = 100;
            return a + b;
        }

        int promote_short(void) {
            short a = 30000, b = 30000;
            return a + b;
        }
    }

    // Both operands become `int`, so the sum is 300 rather than 44.
    assert_eq!(unsafe { promote() }, 300);
    assert_eq!(unsafe { promote_short() }, 60_000);
}

#[test]
fn char_arithmetic() {
    c99! {
        char next_letter(char c) {
            return c + 1;
        }

        int letter_value(void) {
            char c = 'A';
            return c + 1;
        }

        int newline_value(void) {
            return '\n';
        }

        int signed_char_is_negative(void) {
            signed char c = -1;
            return c;
        }

        int unsigned_char_is_not(void) {
            unsigned char c = 255;
            return c;
        }
    }

    assert_eq!(
        unsafe { next_letter(b'A' as ::core::ffi::c_char) },
        b'B' as _
    );
    assert_eq!(unsafe { letter_value() }, 66);
    assert_eq!(unsafe { newline_value() }, 10);
    assert_eq!(unsafe { signed_char_is_negative() }, -1);
    assert_eq!(unsafe { unsigned_char_is_not() }, 255);
}

#[test]
fn division_truncates_towards_zero_and_the_remainder_follows_the_dividend() {
    c99! {
        int quotient(int a, int b) { return a / b; }
        int remainder(int a, int b) { return a % b; }
        unsigned uquotient(unsigned a, unsigned b) { return a / b; }
    }

    assert_eq!(unsafe { quotient(-7, 2) }, -3);
    assert_eq!(unsafe { quotient(7, -2) }, -3);
    assert_eq!(unsafe { quotient(7, 2) }, 3);
    assert_eq!(unsafe { remainder(-7, 2) }, -1);
    assert_eq!(unsafe { remainder(7, -2) }, 1);
    assert_eq!(unsafe { remainder(7, 2) }, 1);
    assert_eq!(unsafe { uquotient(7, 2) }, 3);
}

#[test]
fn shifts() {
    c99! {
        int shift_left(int x, int n) { return x << n; }
        int shift_right(int x, int n) { return x >> n; }
        unsigned ushift_right(unsigned x, int n) { return x >> n; }
        long shift_long(long x, int n) { return x << n; }
        long long shift_long_long(long long x, int n) { return x << n; }
    }

    assert_eq!(unsafe { shift_left(1, 4) }, 16);
    assert_eq!(unsafe { shift_right(1024, 3) }, 128);
    // A right shift of a negative signed value keeps the sign, as every
    // mainstream C implementation does.
    assert_eq!(unsafe { shift_right(-16, 2) }, -4);
    assert_eq!(unsafe { ushift_right(0x8000_0000, 4) }, 0x0800_0000);
    // A shift wider than an `int`. `long long` is 64 bits on every data model
    // this crate supports, so a shift by 40 is defined there whatever the
    // platform.
    assert_eq!(unsafe { shift_long_long(1, 40) }, 1i64 << 40);
    // `long` is 64 bits only under LP64 — it is 32 on Windows, where `1L << 40`
    // would be undefined — so the count follows the platform's own `long`, and
    // so does the expected value. It is written as a shift of a *variable* one
    // because `1 << 40` typed as a 32-bit `c_long` is a compile-time
    // `arithmetic_overflow` even in a branch that never runs.
    let count = if size_of::<core::ffi::c_long>() == 8 {
        40
    } else {
        20
    };
    let one: core::ffi::c_long = 1;
    assert_eq!(
        unsafe { shift_long(1, count) },
        one << count,
        "a `long` shift by {count}"
    );
}

/// A shift by a count the shifted type cannot hold.
///
/// `x << -64` and `x >> 100` are undefined behaviour in C (6.5.7p3), and no
/// two compilations need agree: GCC answers `4` for `4 << -64` at `-O0`, where
/// the hardware masks the count, and `0` at `-O2`, where it folds the
/// undefined shift away. So there is nothing here to match GCC against, and
/// what is asserted is only what the *generated Rust* defines —
/// `wrapping_shl` and `wrapping_shr` take the count modulo the width, which is
/// what the hardware does and so what `-O0` GCC agrees with.
///
/// It is a test at all because such a shift used not to compile. A constant
/// count is written out as the `u32` the shift methods take, and `-64 as u32`
/// is read by `rustc` as the negation of a `u32` — `E0600` — however it is
/// bracketed, so the count is reduced to that `u32` before it is emitted.
/// `execute/pr98681` in the GCC torture suite is the case that found it.
#[test]
fn shifts_by_a_count_the_type_cannot_hold() {
    c99! {
        int left_negative(int x) { return x << -64; }
        int right_over(int x) { return x >> 100; }
        unsigned long long uleft_negative(unsigned long long x) { return x << -64; }
        unsigned long long uright_over(unsigned long long x) { return x >> 100; }

        /* The shape `execute/pr98681` has: the shift is behind a branch the
         * two calls below do not take, so the program itself is defined. */
        int pr98681(int x) {
            if (x > 32) return (x << -64) & 255;
            return x;
        }
    }

    // -64 converted to `u32` is 4294967232, which is zero modulo 32 and
    // modulo 64 alike: the value comes back unshifted.
    assert_eq!(unsafe { left_negative(4) }, 4);
    assert_eq!(unsafe { uleft_negative(1) }, 1);
    // 100 modulo 32 is 4; modulo 64 it is 36.
    assert_eq!(unsafe { right_over(0x1234) }, 0x123);
    assert_eq!(unsafe { right_over(-1) }, -1);
    assert_eq!(unsafe { uright_over(u64::MAX) }, u64::MAX >> 36);

    assert_eq!(unsafe { pr98681(32) }, 32);
    assert_eq!(unsafe { pr98681(-150) }, -150);
}

#[test]
fn bitwise_operators() {
    c99! {
        int band(int a, int b) { return a & b; }
        int bor(int a, int b) { return a | b; }
        int bxor(int a, int b) { return a ^ b; }
        int bnot(int a) { return ~a; }
        int double_not(int a) { return ~~a; }
    }

    assert_eq!(unsafe { band(0b1100, 0b1010) }, 0b1000);
    assert_eq!(unsafe { bor(0b1100, 0b1010) }, 0b1110);
    assert_eq!(unsafe { bxor(0b1100, 0b1010) }, 0b0110);
    assert_eq!(unsafe { bnot(0) }, -1);
    assert_eq!(unsafe { double_not(12345) }, 12345);
}

#[test]
fn logical_operators_produce_exactly_zero_or_one() {
    c99! {
        int calls = 0;

        int bump(void) {
            calls++;
            return 1;
        }

        int lnot(int x) { return !x; }
        int land(int a, int b) { return a && b; }
        int lor(int a, int b) { return a || b; }

        int and_short_circuits(void) {
            calls = 0;
            if (0 && bump()) {
                return -1;
            }
            return calls;
        }

        int or_short_circuits(void) {
            calls = 0;
            if (1 || bump()) {
                return calls;
            }
            return -1;
        }
    }

    assert_eq!(unsafe { lnot(0) }, 1);
    assert_eq!(unsafe { lnot(5) }, 0);
    assert_eq!(unsafe { lnot(-5) }, 0);
    // The result is 1, not the operand: `2 && 3` is one, not six.
    assert_eq!(unsafe { land(2, 3) }, 1);
    assert_eq!(unsafe { land(0, 3) }, 0);
    assert_eq!(unsafe { lor(0, 7) }, 1);
    assert_eq!(unsafe { lor(0, 0) }, 0);
    assert_eq!(unsafe { and_short_circuits() }, 0);
    assert_eq!(unsafe { or_short_circuits() }, 0);
}

#[test]
fn ternary_and_comma() {
    c99! {
        int abs_value(int x) {
            return x > 0 ? x : -x;
        }

        int nested_ternary(int x) {
            return x < 0 ? -1 : x > 0 ? 1 : 0;
        }

        int comma_operator(void) {
            int a = 0, b = 0;
            int c = (a = 1, b = 2, a + b);
            return c;
        }

        int comma_in_a_for(int n) {
            int total = 0;
            for (int i = 0, j = n; i < j; i++, j--) {
                total++;
            }
            return total;
        }
    }

    assert_eq!(unsafe { abs_value(-5) }, 5);
    assert_eq!(unsafe { abs_value(5) }, 5);
    assert_eq!(unsafe { nested_ternary(-9) }, -1);
    assert_eq!(unsafe { nested_ternary(0) }, 0);
    assert_eq!(unsafe { nested_ternary(9) }, 1);
    assert_eq!(unsafe { comma_operator() }, 3);
    assert_eq!(unsafe { comma_in_a_for(10) }, 5);
}

#[test]
fn compound_assignment() {
    c99! {
        int chain(int x) {
            x += 5;
            x *= 3;
            x -= 2;
            x /= 4;
            x %= 7;
            return x;
        }

        int narrow_compound(void) {
            unsigned char c = 200;
            c += 100;
            return c;
        }

        int bit_compound(int x) {
            x |= 0xF0;
            x &= 0xFF;
            x ^= 0x0F;
            x <<= 1;
            x >>= 2;
            return x;
        }
    }

    // 1 → 6 → 18 → 16 → 4 → 4
    assert_eq!(unsafe { chain(1) }, 4);
    // 200 + 100 is computed as `int`, then truncated back to `unsigned char`.
    assert_eq!(unsafe { narrow_compound() }, 44);
    // 5 → 0xF5 → 0xF5 → 0xFA → 0x1F4 → 0x7D
    assert_eq!(unsafe { bit_compound(5) }, 0x7D);
}

#[test]
fn increment_and_decrement_as_expressions() {
    c99! {
        int post_increment(int y) {
            int a = y++;
            return a * 100 + y;
        }

        int pre_increment(int y) {
            int a = ++y;
            return a * 100 + y;
        }

        int sequenced(void) {
            int i = 0;
            int a = i++;
            int b = i++;
            return a * 100 + b * 10 + i;
        }

        int decrements(int x) {
            x--;
            --x;
            return x-- + --x;
        }
    }

    assert_eq!(unsafe { post_increment(3) }, 304);
    assert_eq!(unsafe { pre_increment(3) }, 404);
    assert_eq!(unsafe { sequenced() }, 12);
    // 10 → 9 → 8; then `x--` yields 8 leaving 7, and `--x` yields 6.
    assert_eq!(unsafe { decrements(10) }, 14);
}

#[test]
fn floating_point() {
    c99! {
        double average(double a, double b) { return (a + b) / 2; }
        int truncate(double d) { return (int) d; }
        double widen(int n) { return n; }
        float multiply(float a, float b) { return a * b; }
        int mixed(int n, double d) { return n * d; }
        double divide(double a, double b) { return a / b; }
        int compare(double a, double b) { return a < b; }
    }

    assert_eq!(unsafe { average(1.0, 2.0) }, 1.5);
    assert_eq!(unsafe { truncate(3.99) }, 3);
    // Conversion to an integer type discards the fractional part; it does not
    // round.
    assert_eq!(unsafe { truncate(-3.99) }, -3);
    assert_eq!(unsafe { widen(7) }, 7.0);
    assert_eq!(unsafe { multiply(1.5, 2.0) }, 3.0);
    // `n * d` is done in `double` (4.5) and then truncated to `int`.
    assert_eq!(unsafe { mixed(3, 1.5) }, 4);
    assert_eq!(unsafe { divide(1.0, 4.0) }, 0.25);
    assert_eq!(unsafe { compare(1.0, 2.0) }, 1);
    assert_eq!(unsafe { compare(2.0, 1.0) }, 0);
}

#[test]
fn boolean_type() {
    c99! {
        _Bool truthy(int x) {
            _Bool b = x;
            return b;
        }

        int as_int(int x) {
            _Bool b = x;
            return b;
        }

        int negate(_Bool b) {
            return !b;
        }

        int bool_arithmetic(void) {
            _Bool a = 5;
            _Bool b = 0;
            return a + a + b;
        }
    }

    // Conversion to `_Bool` yields 0 or 1, never the original value.
    assert!(unsafe { truthy(5) });
    assert!(!unsafe { truthy(0) });
    assert_eq!(unsafe { as_int(5) }, 1);
    assert_eq!(unsafe { as_int(0) }, 0);
    assert_eq!(unsafe { negate(true) }, 0);
    assert_eq!(unsafe { negate(false) }, 1);
    assert_eq!(unsafe { bool_arithmetic() }, 2);
}

#[test]
fn integer_constant_types() {
    c99! {
        long long big(void) { return 4294967296; }
        unsigned unsigned_suffix(void) { return 3000000000u; }
        int hex_and_octal(void) { return 0x1F + 010; }
        long long hex_wraps_to_unsigned(void) { return 0xFFFFFFFF; }
    }

    assert_eq!(unsafe { big() }, 4_294_967_296);
    assert_eq!(unsafe { unsigned_suffix() }, 3_000_000_000);
    assert_eq!(unsafe { hex_and_octal() }, 31 + 8);
    // A hexadecimal constant may take an unsigned type, so this is positive.
    assert_eq!(unsafe { hex_wraps_to_unsigned() }, 4_294_967_295);
}

#[test]
fn sizeof_is_a_constant() {
    c99! {
        int size_of_int(void) { return sizeof(int); }
        int size_of_double(void) { return sizeof(double); }
        int size_of_expression(void) { char c = 0; return sizeof c; }
    }

    assert_eq!(
        unsafe { size_of_int() },
        core::mem::size_of::<core::ffi::c_int>() as _
    );
    assert_eq!(unsafe { size_of_double() }, 8);
    assert_eq!(unsafe { size_of_expression() }, 1);
}

// ---------------------------------------------------------------------------
// declarations and linkage
// ---------------------------------------------------------------------------

#[test]
fn globals_are_visible_from_rust() {
    c99! {
        int total = 0;
        int start = 41;

        void add_to_total(int n) {
            total += n;
        }

        int read_start(void) {
            return start;
        }
    }

    // A C global becomes a `static mut`, so Rust must read it by value:
    // `assert_eq!(total, 0)` would take a reference to it, which edition 2024
    // refuses.
    unsafe {
        assert_eq!({ total }, 0);
        assert_eq!({ start }, 41);
        add_to_total(5);
        add_to_total(37);
        assert_eq!({ total }, 42);
        start = 1;
        assert_eq!(read_start(), 1);
    }
}

#[test]
fn static_locals_keep_their_value() {
    c99! {
        int next_id(void) {
            static int id = 0;
            id++;
            return id;
        }

        int other_counter(void) {
            static int id = 100;
            id++;
            return id;
        }
    }

    assert_eq!(unsafe { next_id() }, 1);
    assert_eq!(unsafe { next_id() }, 2);
    assert_eq!(unsafe { next_id() }, 3);
    // A `static` in another function is a separate object even under the same
    // name.
    assert_eq!(unsafe { other_counter() }, 101);
    assert_eq!(unsafe { next_id() }, 4);
}

#[test]
fn static_functions_are_private_to_the_unit() {
    c99! {
        static int doubled(int x) {
            return x * 2;
        }

        static long private_total = 0;

        int use_helper(int x) {
            private_total += doubled(x);
            return doubled(x) + 1;
        }

        long read_private_total(void) {
            return private_total;
        }
    }

    assert_eq!(unsafe { use_helper(20) }, 41);
    assert_eq!(unsafe { read_private_total() }, 40);
}

#[test]
fn mutual_recursion() {
    c99! {
        int is_odd(int n);

        int is_even(int n) {
            return n == 0 ? 1 : is_odd(n - 1);
        }

        int is_odd(int n) {
            return n == 0 ? 0 : is_even(n - 1);
        }
    }

    assert_eq!(unsafe { is_even(10) }, 1);
    assert_eq!(unsafe { is_odd(10) }, 0);
    assert_eq!(unsafe { is_even(7) }, 0);
    assert_eq!(unsafe { is_odd(7) }, 1);
}

#[test]
fn c_names_that_are_rust_keywords() {
    c99! {
        int match(int type) {
            int let = type * 2;
            return let;
        }

        int fn(int impl) {
            return impl + 1;
        }

        int uses_self(int self) {
            int crate = self;
            return crate * 10;
        }
    }

    assert_eq!(unsafe { r#match(21) }, 42);
    assert_eq!(unsafe { r#fn(41) }, 42);
    assert_eq!(unsafe { uses_self(4) }, 40);
}

#[test]
fn typedefs_of_scalar_types_resolve() {
    c99! {
        typedef int myint;
        typedef unsigned long size_type;
        typedef myint alias_of_alias;

        size_type length(myint n) {
            alias_of_alias doubled = n * 2;
            return doubled;
        }
    }

    assert_eq!(unsafe { length(21) }, 42);
}

#[test]
fn a_void_function_without_a_return_statement() {
    c99! {
        int side_effect = 0;

        void set_side_effect(int n) {
            side_effect = n;
        }

        void does_nothing(void) {
        }

        void early_return(int n) {
            if (n < 0) {
                return;
            }
            side_effect = n * 2;
        }
    }

    unsafe {
        set_side_effect(7);
        assert_eq!({ side_effect }, 7);
        does_nothing();
        early_return(-1);
        assert_eq!({ side_effect }, 7);
        early_return(21);
        assert_eq!({ side_effect }, 42);
    }
}

#[test]
fn a_function_that_falls_off_its_end_returns_zero() {
    c99! {
        int no_return_on_every_path(int n) {
            if (n > 0) {
                return n;
            }
            /* C leaves the value indeterminate here; we return a zero. */
        }

        int infinite_loop_needs_no_return(int n) {
            while (1) {
                if (n > 100) {
                    return n;
                }
                n = n * 2 + 1;
            }
        }
    }

    assert_eq!(unsafe { no_return_on_every_path(5) }, 5);
    assert_eq!(unsafe { no_return_on_every_path(-5) }, 0);
    assert_eq!(unsafe { infinite_loop_needs_no_return(1) }, 127);
}

#[test]
fn inline_functions() {
    c99! {
        inline int quick(int x) {
            return x + 1;
        }

        static inline int quicker(int x) {
            return quick(x) + 1;
        }

        int use_both(int x) {
            return quicker(x);
        }
    }

    assert_eq!(unsafe { use_both(40) }, 42);
}

#[test]
fn const_qualified_objects_are_readable() {
    c99! {
        const int limit = 10;

        int under_limit(int n) {
            const int local_limit = limit;
            return n < local_limit;
        }
    }

    assert_eq!(unsafe { under_limit(5) }, 1);
    assert_eq!(unsafe { under_limit(50) }, 0);
}

#[test]
fn declarations_may_shadow_in_inner_blocks() {
    c99! {
        int shadow(int x) {
            int total = x;
            {
                int x = 100;
                total += x;
            }
            total += x;
            return total;
        }
    }

    assert_eq!(unsafe { shadow(1) }, 102);
}

#[test]
fn a_block_scope_extern_names_the_object_with_linkage() {
    c99! {
        int shared = 3;
        static int internal = 7;

        /* C99 6.2.2p4: a block-scope object declared without `extern` has no
         * linkage at all, so a prior declaration of one says nothing about
         * what an `extern` declaration in an inner block names — which is the
         * file-scope object, however deeply the local one shadows it. */
        int reaches_the_global(void) {
            int shared = 4;
            {
                extern int shared;
                return shared;
            }
        }

        int the_local_still_wins(void) {
            int shared = 4;
            return shared;
        }

        /* The prior declaration here *has* linkage — internal, in this case —
         * and the `extern` inherits it rather than making a second object. */
        int reaches_the_static(void) {
            extern int internal;
            return internal;
        }

        void set_shared(int v) {
            extern int shared;
            shared = v;
        }
    }

    unsafe {
        assert_eq!(reaches_the_global(), 3);
        assert_eq!(the_local_still_wins(), 4);
        assert_eq!(reaches_the_static(), 7);
        set_shared(9);
        assert_eq!({ shared }, 9);
        assert_eq!(reaches_the_global(), 9);
    }
}

#[test]
fn an_array_parameters_bound_is_evaluated_on_entry() {
    c99! {
        /* C99 6.9.1p10: the bound is not part of the adjusted parameter type
         * — `a` is an `int *` — but the size expressions of a definition are
         * still evaluated when the function is entered, in the scope where
         * the parameters before them are already visible. */
        int bump_once(int n, int a[n++]) { (void) a; return n; }

        static int calls;
        int side_effects(void) { calls++; return 4; }
        int call_count(void) { return calls; }

        void twice(int a[side_effects()], int b[side_effects()]) {
            (void) a;
            (void) b;
        }

        int only_declared(int n, int a[n++]);
        int only_declared(int n, int a[n++]) { (void) a; return n; }
    }

    unsafe {
        let mut storage = [0; 8];
        assert_eq!(bump_once(10, storage.as_mut_ptr()), 11);
        assert_eq!(only_declared(1, storage.as_mut_ptr()), 2);
        assert_eq!(call_count(), 0);
        twice(storage.as_mut_ptr(), storage.as_mut_ptr());
        assert_eq!(call_count(), 2);
    }
}

#[test]
fn a_compound_assignment_is_one_evaluation_around_a_call() {
    c99! {
        /* C11 6.5.16.2p3: with respect to an indeterminately sequenced
         * function call, `E1 op= E2` is a *single* evaluation, so the call
         * cannot happen between the read of `E1` and the write back to it.
         * Writing it out as `E1 = E1 op E2` would do exactly that. */
        unsigned int cell[1] = { 2 };

        unsigned int side_effect(void) {
            cell[0] |= 128;
            return 1;
        }

        unsigned int compound(void) {
            cell[0] |= side_effect();
            return cell[0];
        }

        static int index_calls;
        int which(void) { index_calls++; return 0; }
        int index_call_count(void) { return index_calls; }

        unsigned int the_place_is_computed_once(void) {
            cell[which()] += side_effect();
            return cell[0];
        }
    }

    unsafe {
        // The call sets bit 7 and returns 1; both survive.
        assert_eq!(compound(), 2 | 128 | 1);
        cell[0] = 0;
        assert_eq!(the_place_is_computed_once(), 128 + 1);
        assert_eq!(index_call_count(), 1);
    }
}

// ---------------------------------------------------------------------------
// robustness
// ---------------------------------------------------------------------------

#[test]
fn a_deeply_nested_expression() {
    // Code generation recurses on the caller's stack, so a real `c99!`
    // invocation is the only honest way to test that it fits. `~` nests one
    // level per token, which reaches the parser's limit with the least text.
    c99! {
        int deep(int x) {
            return ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
                   ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
                   ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~ x;
        }
    }

    // 150 complements of a value are the value itself.
    assert_eq!(unsafe { deep(12345) }, 12345);
    assert_eq!(unsafe { deep(-1) }, -1);
}

#[test]
fn a_deeply_nested_statement() {
    c99! {
        int nest(int n) {
            if (n > 0) { if (n > 1) { if (n > 2) { if (n > 3) { if (n > 4) {
            if (n > 5) { if (n > 6) { if (n > 7) { if (n > 8) { if (n > 9) {
            if (n > 10) { if (n > 11) { if (n > 12) { if (n > 13) {
                return 99;
            } } } }
            } } } } } }
            } } } }
            return n;
        }
    }

    assert_eq!(unsafe { nest(20) }, 99);
    assert_eq!(unsafe { nest(3) }, 3);
}

#[test]
fn string_literal_mode_produces_the_same_code() {
    c99! { r#"
        int add_from_a_string(int a, int b) {
            return a + b;
        }
    "# }

    assert_eq!(unsafe { add_from_a_string(19, 23) }, 42);
}

// ---------------------------------------------------------------------------
// corners
// ---------------------------------------------------------------------------

#[test]
fn switch_corner_cases() {
    c99! {
        int empty_switch(int n) {
            switch (n) { }
            return n;
        }

        int body_is_not_a_block(int n) {
            switch (n) n = 5;
            return n;
        }

        int declarations_in_the_body(int n) {
            switch (n) {
                int local;
                int with_init = 7;
                case 1:
                    local = 10;
                    return local + with_init;
                default:
                    return 0;
            }
        }

        int statements_before_the_first_label(int n) {
            int x = 1;
            switch (n) {
                x = 99;
                case 1: return x;
                default: return 2;
            }
        }
    }

    // Without a label nothing in the body can run.
    assert_eq!(unsafe { empty_switch(3) }, 3);
    assert_eq!(unsafe { body_is_not_a_block(3) }, 3);
    // The jump into `case 1:` skips the initialiser of `with_init`, which C
    // leaves indeterminate and we leave zeroed.
    assert_eq!(unsafe { declarations_in_the_body(1) }, 10);
    assert_eq!(unsafe { declarations_in_the_body(2) }, 0);
    assert_eq!(unsafe { statements_before_the_first_label(1) }, 1);
}

#[test]
fn casts_chain_and_compare() {
    c99! {
        int cast_then_compare(long v) {
            return (int) v < 5;
        }

        int cast_chain(double d) {
            return (int)(char)(long) d;
        }

        void discard(int n) {
            (void) n;
        }
    }

    assert_eq!(unsafe { cast_then_compare(3) }, 1);
    assert_eq!(unsafe { cast_then_compare(9) }, 0);
    // 300 truncated to `char` is 44 on a target with 8-bit, signed `char`.
    assert_eq!(unsafe { cast_chain(300.7) }, 44);
    unsafe { discard(1) };
}

#[test]
fn assignment_and_comma_as_expressions() {
    c99! {
        int chained_assignment(int n) {
            int a, b;
            a = b = n;
            return a + b;
        }

        int comma_statement(int n) {
            n++, n++;
            return n;
        }

        int assignment_in_a_condition(int n) {
            int x;
            if ((x = n * 2)) {
                return x;
            }
            return -1;
        }
    }

    assert_eq!(unsafe { chained_assignment(4) }, 8);
    assert_eq!(unsafe { comma_statement(1) }, 3);
    assert_eq!(unsafe { assignment_in_a_condition(3) }, 6);
    assert_eq!(unsafe { assignment_in_a_condition(0) }, -1);
}

#[test]
fn extreme_constants() {
    c99! {
        unsigned long long largest(void) {
            return 18446744073709551615ULL;
        }

        long long smallest(void) {
            return -9223372036854775807LL - 1;
        }

        double infinite(void) {
            return 1e400;
        }

        int is_infinite(void) {
            double d = 1e400;
            return d > 1e300;
        }
    }

    assert_eq!(unsafe { largest() }, u64::MAX);
    assert_eq!(unsafe { smallest() }, i64::MIN);
    assert!(unsafe { infinite() }.is_infinite());
    assert_eq!(unsafe { is_infinite() }, 1);
}

#[test]
fn conditions_of_every_shape() {
    c99! {
        int bool_condition(int x) {
            _Bool b = x;
            return b ? 10 : 20;
        }

        int nested_conditions(int a, int b) {
            return (a ? 1 : 0) ? b : -b;
        }

        int double_condition(double d) {
            if (d) {
                return 1;
            }
            return 0;
        }
    }

    assert_eq!(unsafe { bool_condition(3) }, 10);
    assert_eq!(unsafe { bool_condition(0) }, 20);
    assert_eq!(unsafe { nested_conditions(1, 5) }, 5);
    assert_eq!(unsafe { nested_conditions(0, 5) }, -5);
    assert_eq!(unsafe { double_condition(0.5) }, 1);
    assert_eq!(unsafe { double_condition(0.0) }, 0);
}

#[test]
fn a_tentative_definition_completed_later() {
    c99! {
        int counter;
        int counter = 7;

        int read_counter(void) {
            return counter;
        }
    }

    assert_eq!(unsafe { read_counter() }, 7);
    let value = unsafe { counter };
    assert_eq!(value, 7);
}

#[test]
fn an_extern_declaration_the_unit_then_defines() {
    // C99 6.9.2: `extern` says only that the name has external linkage. A
    // later declaration of it with no storage class is a *definition* in this
    // unit — a tentative one when it writes no initialiser — so the object
    // stops being an external declaration and gets storage here.
    c99! {
        extern int tentative_after_extern;
        int tentative_after_extern;

        extern int initialized_after_extern;
        int initialized_after_extern = 5;

        int extern_after_tentative;
        extern int extern_after_tentative;

        extern int defined_then_declared_extern(int n);
        int defined_then_declared_extern(int n) { return n * 3; }

        int totals(void) {
            tentative_after_extern = 3;
            extern_after_tentative = 7;
            return tentative_after_extern
                 + initialized_after_extern
                 + extern_after_tentative
                 + defined_then_declared_extern(2);
        }
    }

    unsafe {
        // A tentative definition starts out zero, as C says.
        assert_eq!({ tentative_after_extern }, 0);
        assert_eq!({ initialized_after_extern }, 5);
        assert_eq!(totals(), 3 + 5 + 7 + 6);
    }
}

#[test]
fn an_integer_constant_expression_keeps_its_type() {
    // `(c ? -1 : 1) * (int) sizeof(int)` is a `{integer}` in Rust unless the
    // conditional says what it is, and `{integer}.wrapping_mul(…)` is `E0689`.
    // (c-testsuite 00200.)
    c99! {
        /* 00200's own idiom: the sign of the result says whether the type is
           signed, and its magnitude is the width. */
        int signed_width(int n) {
            return ((n) < 0 || -(n) < 0 ? -1 : 1) * (int) sizeof(n + 0);
        }

        int unsigned_width(unsigned n) {
            return ((n) < 0 || -(n) < 0 ? -1 : 1) * (int) sizeof(n + 0);
        }

        long long_width(int n) {
            /* The same shape at a width where Rust's `i32` fallback would be
               wrong as well as ambiguous. */
            return (n ? 3000000000L : 1L) / 2;
        }

        int shifted(int n) {
            return (n ? 1 : 2) << 3;
        }

        int dispatched(int n) {
            switch (n ? 1L : 2L) {
                case 1: return 100;
                default: return 200;
            }
        }

        #include <stdio.h>

        /* An argument matched by `...` has no parameter to take its type from,
           so the conditional has to carry its own. `long long` and `%lld`
           rather than `long` and `%ld`, because the two have to agree on every
           platform: a `long` is four bytes on Windows, where `3000000000L` is
           a `long long` already (6.4.4.1p5) and `%ld` would print the low half
           of it. */
        int formatted(char *buf, unsigned long size, int n) {
            return snprintf(buf, size, "%lld", n ? 3000000000LL : 1LL);
        }
    }

    let mut buf = [0u8; 32];
    unsafe {
        assert_eq!(signed_width(1), -4);
        assert_eq!(signed_width(-1), -4);
        assert_eq!(unsigned_width(1), 4);
        assert_eq!(long_width(1), 1_500_000_000);
        assert_eq!(long_width(0), 0);
        assert_eq!(shifted(1), 8);
        assert_eq!(shifted(0), 16);
        assert_eq!(dispatched(1), 100);
        assert_eq!(dispatched(0), 200);
        let n = formatted(buf.as_mut_ptr().cast(), 32, 1);
        assert_eq!(&buf[..n as usize], b"3000000000");
    }
}

#[test]
fn conversions_between_every_scalar_kind() {
    c99! {
        double bool_to_double(int x) { _Bool b = x; double d = b; return d; }
        float bool_to_float(int x) { _Bool b = x; float f = b; return f; }
        int char_to_bool(char c) { _Bool b = c; return b; }
        int double_to_bool(double d) { _Bool b = d; return b; }
        long long_from_uchar(unsigned char c) { return c; }
        unsigned char uchar_from_long(long v) { return v; }
        int compare_after_cast(unsigned u) { return (int) u < 0; }
        int negate_unsigned(unsigned u) { return -u == 0; }
        double negate_double(double d) { return -d; }
        int unary_plus_promotes(char c) { return +c; }
    }

    unsafe {
        assert_eq!(bool_to_double(5), 1.0);
        assert_eq!(bool_to_float(0), 0.0);
        assert_eq!(char_to_bool(3), 1);
        assert_eq!(double_to_bool(0.5), 1);
        assert_eq!(double_to_bool(0.0), 0);
        assert_eq!(long_from_uchar(200), 200);
        // 300 does not fit in `unsigned char`; the value wraps.
        assert_eq!(uchar_from_long(300), 44);
        // `(int) 4294967295u` is -1, which is less than zero. Rust would read
        // `x as i32 < 0` as the start of generic arguments, so the cast has to
        // be parenthesised in the generated code.
        assert_eq!(compare_after_cast(4_294_967_295), 1);
        assert_eq!(negate_unsigned(0), 1);
        assert_eq!(negate_double(1.5), -1.5);
        assert_eq!(unary_plus_promotes(65), 65);
    }
}

#[test]
fn nested_switches_and_continue_from_inside_one() {
    c99! {
        int nested(int a, int b) {
            int out = 0;
            switch (a) {
                case 1:
                    switch (b) {
                        case 1: out = 11; break;
                        case 2: out = 12; break;
                        default: out = 19;
                    }
                    break;
                case 2:
                    for (int i = 0; i < 3; i++) {
                        switch (b) {
                            case 1: out += 1; break;
                            default: out += 10; continue;
                        }
                        out += 100;
                    }
                    break;
                default:
                    out = -1;
            }
            return out;
        }
    }

    unsafe {
        // The inner `break` leaves the inner `switch` only.
        assert_eq!(nested(1, 1), 11);
        assert_eq!(nested(1, 2), 12);
        assert_eq!(nested(1, 9), 19);
        // Each of three iterations adds 1 and then 100.
        assert_eq!(nested(2, 1), 303);
        // `continue` looks past the `switch` to the enclosing `for`, so the
        // `out += 100` after the switch is skipped.
        assert_eq!(nested(2, 5), 30);
        assert_eq!(nested(9, 0), -1);
    }
}

// ---------------------------------------------------------------------------
// the block a selection or iteration statement is
// ---------------------------------------------------------------------------

#[test]
fn a_tag_declared_in_a_controlling_expression_is_scoped_to_the_statement() {
    // C99 6.8.4p3 and 6.8.5p5: a selection statement and an iteration
    // statement are each a block, and the controlling expression is inside
    // it. So an enumeration declared there is invisible afterwards, and the
    // outer `b` is what the code after the statement sees. In C89 it was the
    // inner one, which is exactly what the change was for; Clang's own
    // `C99/block-scopes.c` is this test.
    c99! {
        enum { a, b };

        int outer_after_if(void) {
            if (sizeof(enum { b, a }) != sizeof(int)) {
                return -1;
            }
            return b;
        }

        int inner_inside_if(void) {
            if (sizeof(enum { b, a }) == sizeof(int)) {
                return a;
            }
            return -1;
        }

        int outer_after_while(void) {
            while (sizeof(enum { b, a }) == 0) {
                return -1;
            }
            return b;
        }

        int outer_after_switch(void) {
            switch (sizeof(enum { b, a }) != sizeof(int)) {
                case 1:
                    return -1;
                default:
                    break;
            }
            return b;
        }
    }

    unsafe {
        // The file-scope `enum { a, b }` has `b == 1`...
        assert_eq!(outer_after_if(), 1);
        assert_eq!(outer_after_while(), 1);
        assert_eq!(outer_after_switch(), 1);
        // ...while inside the `if`, the one declared in its controlling
        // expression is in scope, and there `a == 1`.
        assert_eq!(inner_inside_if(), 1);
    }
}

// ---------------------------------------------------------------------------
// an identifier is in scope for its own initialiser
// ---------------------------------------------------------------------------

#[test]
fn an_initializer_may_name_the_object_it_initializes() {
    // C99 6.2.1p7: "the scope of an identifier ... begins just after the
    // completion of its declarator", so the object being declared is already
    // visible in its own initialiser. The circular list `{ &head, &head }` is
    // the idiom this rule exists for; `T *p = malloc(sizeof *p)` is the one
    // every allocation is written with.
    c99! {
        #include <stdlib.h>

        struct node { struct node *next; int v; };
        struct node head = { &head, 7 };

        struct pair { int v; struct pair *self; };
        struct pair table[2] = { { 0, &table[1] }, { 1, &table[0] } };

        int file_scope_self(void) { return head.next == &head && head.v == 7; }
        int file_scope_array(void) {
            return table[0].self == &table[1] && table[1].self == &table[0];
        }

        int automatic_self(void) {
            struct node local = { &local, 3 };
            return local.next == &local && local.v == 3;
        }

        int automatic_array(void) {
            struct pair pairs[2] = { { 4, &pairs[1] }, { 5, &pairs[0] } };
            return pairs[0].self == &pairs[1] && pairs[1].self[0].v == 4;
        }

        int block_static_self(void) {
            static struct node local = { &local, 9 };
            return local.next == &local && local.v == 9;
        }

        int sizeof_of_itself(void) {
            struct node *p = malloc(sizeof *p);
            int ok;
            if (!p) { return 0; }
            p->v = 11;
            ok = p->v == 11;
            free(p);
            return ok;
        }
    }

    unsafe {
        assert_eq!(file_scope_self(), 1);
        assert_eq!(file_scope_array(), 1);
        assert_eq!(automatic_self(), 1);
        assert_eq!(automatic_array(), 1);
        assert_eq!(block_static_self(), 1);
        assert_eq!(sizeof_of_itself(), 1);
    }
}

// ---------------------------------------------------------------------------
// a subobject of structure type takes a whole value
// ---------------------------------------------------------------------------

#[test]
fn a_struct_valued_element_initializes_the_whole_subobject() {
    // C11 6.7.9p13: an object of structure or union type may be initialised by
    // "a single expression that has compatible structure or union type", and a
    // *subobject* may too — braces or not. Only when the element is of some
    // other type do the elided braces of p9 send it to the subobject's first
    // member instead.
    c99! {
        struct inner { int x; int y; };
        struct outer { int z; struct inner b; };
        struct wrap { struct inner rows[2]; int tail; };

        int nested_member(void) {
            struct inner b = { 1, 2 };
            struct outer a = { 3, b };
            return a.z == 3 && a.b.x == 1 && a.b.y == 2;
        }

        int nested_array_element(void) {
            struct inner p = { 4, 5 };
            struct inner q = { 6, 7 };
            struct wrap w = { { p, q }, 8 };
            return w.rows[0].x == 4 && w.rows[0].y == 5
                && w.rows[1].x == 6 && w.rows[1].y == 7 && w.tail == 8;
        }

        int designated_array_of_structs(void) {
            struct inner p = { 9, 10 };
            struct wrap w = { .rows = { p }, .tail = 11 };
            return w.rows[0].x == 9 && w.rows[0].y == 10
                && w.rows[1].x == 0 && w.tail == 11;
        }

        int elided_braces_still_work(void) {
            /* The elements are `int`s, so they fill the members of `b` one by
               one — the rule p13 replaces only when the type matches. */
            struct outer a = { 12, 13, 14 };
            return a.z == 12 && a.b.x == 13 && a.b.y == 14;
        }

        int compound_literal_element(void) {
            struct outer a = { 15, (struct inner){ 16, 17 } };
            return a.z == 15 && a.b.x == 16 && a.b.y == 17;
        }
    }

    unsafe {
        assert_eq!(nested_member(), 1);
        assert_eq!(nested_array_element(), 1);
        assert_eq!(designated_array_of_structs(), 1);
        assert_eq!(elided_braces_still_work(), 1);
        assert_eq!(compound_literal_element(), 1);
    }
}

// ---------------------------------------------------------------------------
// address constants
// ---------------------------------------------------------------------------

#[test]
fn an_integer_constant_cast_to_a_pointer_initializes_static_storage() {
    // 6.6p9's address constants do not include one, but 6.6p10 lets an
    // implementation accept other forms of constant expression and every one
    // does: `(unsigned int *) 0xa000` is how a program names a memory-mapped
    // register.
    c99! {
        unsigned int *reg = (unsigned int *)0xa000;
        char *offset_from_one = (char *)1 + 2;
        int *from_unsigned = (int *)358273621U;

        long as_integers(void) {
            return (long)reg + (long)offset_from_one + (long)from_unsigned;
        }
    }

    unsafe {
        assert_eq!(as_integers(), 0xa000 + 3 + 358_273_621);
    }
}

// ---------------------------------------------------------------------------
// incomplete array types
// ---------------------------------------------------------------------------

#[test]
fn an_incomplete_array_type_is_completed_by_the_unit() {
    // C99 6.2.5p22 and 6.9.2p5: `int j[];` at file scope is a tentative
    // definition whose type the *end of the translation unit* completes to one
    // element, a later declaration with a bound completes it sooner, and
    // `extern int j[];` never completes at all — its size is another unit's
    // business. `typedef int A[]; A a = { 1, 2 };` takes its length from the
    // initialiser, as the `int a[]` spelling does.
    c99! {
        int assumed[];
        int completed[];
        int completed[4];
        typedef int Row[];
        Row through_a_typedef = { 5, 6, 7 };

        unsigned long completed_size(void) { return sizeof completed; }
        unsigned long typedef_size(void) { return sizeof through_a_typedef; }
        int compatible(void) { return __builtin_types_compatible_p(int[5], int[]); }
        int read_them(void) {
            return assumed[0] + completed[3] + through_a_typedef[2];
        }
    }

    // `sizeof assumed` is an error *inside the unit* — the type is still
    // incomplete where the function is written, which is what GCC says too —
    // so the one element the end of the unit gave it is checked from here, on
    // the generated item's own type.
    let _one_element: *mut [core::ffi::c_int; 1] = &raw mut assumed;

    unsafe {
        assert_eq!(
            completed_size() as usize,
            4 * core::mem::size_of::<core::ffi::c_int>()
        );
        assert_eq!(
            typedef_size() as usize,
            3 * core::mem::size_of::<core::ffi::c_int>()
        );
        assert_eq!(compatible(), 1);
        assert_eq!(read_them(), 7);
    }
}

// ---------------------------------------------------------------------------
// offsetof
// ---------------------------------------------------------------------------

#[test]
fn offsetof_takes_a_member_designator_and_folds_to_a_constant() {
    // C99 7.17p3: the member designator may reach through members, array
    // elements and anonymous members, and the result is an integer constant
    // expression — which is what makes `char b[sizeof (struct A) - offsetof
    // (struct A, a)];` a member declaration. The values are checked against
    // Rust's own `offset_of!`, which is the layout the expansion really has.
    c11! {
        #include <stddef.h>

        struct Inner { char c; int i; };
        struct Outer { int head; struct Inner one; struct Inner rows[3]; };
        struct Anon { int a; struct { int x; int y; }; };

        char probe[offsetof(struct Outer, rows[2].i)];

        size_t nested(void) { return offsetof(struct Outer, one.i); }
        size_t element(void) { return offsetof(struct Outer, rows[2].c); }
        size_t through_element(void) { return offsetof(struct Outer, rows[2].i); }
        size_t anonymous(void) { return offsetof(struct Anon, y); }
        size_t probe_size(void) { return sizeof probe; }
    }

    unsafe {
        assert_eq!(
            nested() as usize,
            core::mem::offset_of!(Outer, one) + core::mem::offset_of!(Inner, i)
        );
        assert_eq!(
            element() as usize,
            core::mem::offset_of!(Outer, rows) + 2 * core::mem::size_of::<Inner>()
        );
        assert_eq!(
            through_element() as usize,
            core::mem::offset_of!(Outer, rows)
                + 2 * core::mem::size_of::<Inner>()
                + core::mem::offset_of!(Inner, i)
        );
        assert_eq!(
            anonymous() as usize,
            core::mem::offset_of!(Anon, __cinrs_anon0.y)
        );
        assert_eq!(probe_size() as usize, through_element() as usize);
    }
}
