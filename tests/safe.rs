//! `safe` functions: C that `rustc` checks, and that Rust may call without
//! `unsafe`.
//!
//! A C function is a foreign function, so the default is what every other
//! `extern "C"` item gets: the caller writes `unsafe`. A function marked safe
//! is generated as a plain `pub extern "C" fn` whose body is *not* wrapped in
//! an `unsafe` block, so the whole translation goes past Rust's own checks —
//! a raw pointer dereference, a read of a C global, a call to a function that
//! is not safe, a `union` member, an `unreachable()` are each an error with
//! the caret on the C that asked for it. What is left is the C that needs
//! none of them, and it is a useful language: arithmetic, control flow,
//! locals, records by value, bit-fields, and calls to other safe functions.
//!
//! The three spellings are all here, because they are one feature: the C23
//! attribute `[[cinrs::safe]]`, the GNU-style `__attribute__((cinrs_safe))`
//! that works in every entry point, and `#pragma cinrs safe f g`, which names
//! functions instead of being written on one.
//!
//! What a safe function must *not* compile is in `tests/ui/safe_*.rs`, where
//! the message a user sees is what is blessed.

use cinrs::{c11, c23, c99, gnu99};

// ---------------------------------------------------------------------------
// the three spellings
// ---------------------------------------------------------------------------

c23! {
    /* C23's own attribute syntax, in this crate's vendor namespace. */
    [[cinrs::safe]] int fact(int n) {
        if (n == 0) {
            return 1;
        } else {
            return n * fact(n - 1);
        }
    }
}

c99! {
    /* The GNU spelling, which every entry point has — `[[…]]` is C23's. */
    __attribute__((cinrs_safe)) int gcd(int a, int b) {
        while (b != 0) {
            int t = a % b;
            a = b;
            b = t;
        }
        return a < 0 ? -a : a;
    }
}

c99! {
    /* The pragma names its functions, so it works in string-literal input and
     * on a function whose declaration is somewhere else entirely. */
    #pragma cinrs safe hypot_squared area

    int hypot_squared(int a, int b) { return a * a + b * b; }
    static int area(int w, int h) { return w * h; }
    int use_area(int w, int h) { return area(w, h); }
}

#[test]
fn a_safe_function_needs_no_unsafe_at_the_call_site() {
    // The whole point: no `unsafe` block anywhere in this test.
    assert_eq!(fact(10), 3_628_800);
    assert_eq!(gcd(48, -18), 6);
    assert_eq!(hypot_squared(3, 4), 25);
}

#[test]
fn a_static_function_may_be_safe_too() {
    // `area` is `static`, so it is private to the unit's module and only the C
    // can call it; that it is safe is what lets `use_area` — which is not —
    // call it exactly as before.
    assert_eq!(unsafe { use_area(3, 5) }, 15);
}

// ---------------------------------------------------------------------------
// what a safe body may hold
// ---------------------------------------------------------------------------

c99! {
    #pragma cinrs safe classify sum_to switch_kind spin

    /* Structured control flow: `if`, `for`, `while`, `do`, `switch`. */
    int classify(int n) {
        if (n < 0) return -1;
        else if (n == 0) return 0;
        return 1;
    }

    long sum_to(int n) {
        long total = 0;
        for (int i = 1; i <= n; i++) total += i;
        return total;
    }

    int switch_kind(int c) {
        int seen = 0;
        switch (c) {
        case 'a':
        case 'b':
            seen = 1;
            /* fallthrough */
        case 'c':
            seen += 10;
            break;
        default:
            seen = -1;
        }
        return seen;
    }

    int spin(int n) {
        int i = 0;
        do {
            i++;
            if (i == n) break;
            continue;
        } while (i < 100);
        return i;
    }
}

#[test]
fn control_flow_is_safe() {
    assert_eq!((classify(-2), classify(0), classify(7)), (-1, 0, 1));
    assert_eq!(sum_to(100), 5050);
    assert_eq!((switch_kind('a' as i32), switch_kind('c' as i32)), (11, 10));
    assert_eq!(switch_kind('z' as i32), -1);
    assert_eq!(spin(7), 7);
}

c99! {
    #pragma cinrs safe collatz_steps jump_in zigzag

    /* An outward `goto` keeps Rust's own control flow: a labelled loop for
     * `again` and a labelled block for `done`. Nothing here is unsafe. */
    int collatz_steps(long n) {
        int steps = 0;
    again:
        if (n == 1) goto done;
        if (n % 2 == 0) {
            n = n / 2;
        } else {
            n = 3 * n + 1;
        }
        steps++;
        goto again;
    done:
        return steps;
    }

    /* A jump *into* a loop body is lowered through the control-flow graph,
     * which is read back into a Rust loop with a state variable picking the
     * head — and every piece of that is safe too. */
    int jump_in(int n, int inside) {
        int t = 0;
        if (inside) goto mid;
        while (n > 0) {
            n--;
        mid:
            t += 2;
        }
        return t;
    }

    /* Two loops that jump into each other, which is the same again with the
     * state variable read twice. */
    int zigzag(int n, int odd) {
        int t = 0;
        if (odd) goto b;
    a:
        t += n;
        if (--n <= 0) return t;
    b:
        t += 2 * n;
        if (--n <= 0) return t;
        goto a;
    }
}

#[test]
fn a_function_lowered_through_the_cfg_is_safe() {
    assert_eq!(collatz_steps(27), 111);
    assert_eq!((jump_in(3, 0), jump_in(3, 1)), (6, 8));
    assert_eq!((jump_in(0, 1), jump_in(0, 0)), (2, 0));
    assert_eq!((zigzag(4, 0), zigzag(4, 1)), (14, 16));
}

c11! {
    #pragma cinrs safe manhattan tag_of level_of shifted over

    struct Point { int x; int y; };

    /* A record passed and returned *by value* is a Rust value like any
     * other. */
    int manhattan(struct Point p) {
        return (p.x < 0 ? -p.x : p.x) + (p.y < 0 ? -p.y : p.y);
    }

    enum Level { LOW, HIGH };

    int tag_of(enum Level level) { return level == HIGH; }

    struct Flags {
        unsigned int ready : 1;
        int          level : 3;
        unsigned int mask  : 20;
    };

    /* The bit-field accessors are ordinary inherent methods taking `&self`,
     * so reading one of a *local* needs nothing unsafe. */
    int level_of(struct Flags f) { return f.level; }

    struct Flags shifted(struct Flags f, int by) {
        f.level = f.level + by;
        f.ready = 1;
        return f;
    }

    _Bool over(double x, double limit) { return x > limit; }
}

c11! {
    #pragma cinrs safe union_ready

    union Packed {
        unsigned int ready : 1;
        int whole;
    };

    /* The one corner where a safe function reaches something Rust would
     * otherwise ask for `unsafe` around: a bit-field accessor of a *union* is
     * a safe method that reads the storage inside an `unsafe` block of its
     * own, so this compiles where `v.whole` — an ordinary union member — does
     * not. `doc/features.md` ("Safe functions") says so. */
    int union_ready(union Packed v) { return v.ready; }
}

#[test]
fn a_union_bit_field_accessor_is_reachable_from_a_safe_function() {
    let v = Packed { __cinrs_bits0: [1] };
    assert_eq!(union_ready(v), 1);
}

#[test]
fn records_enums_and_bit_fields_are_safe() {
    assert_eq!(manhattan(Point { x: -3, y: 4 }), 7);
    assert_eq!((tag_of(LOW), tag_of(HIGH)), (0, 1));

    let mut f = Flags {
        __cinrs_bits0: [0; 3],
    };
    f.set_level(-3);
    assert_eq!(level_of(f), -3);
    let g = shifted(f, 4);
    assert_eq!((g.level(), g.ready()), (1, 1));

    assert!(over(2.0, 1.5));
    assert!(!over(1.0, 1.5));
}

c99! {
    #pragma cinrs safe half remainder_of

    /* Integer division is Rust's, which truncates towards zero exactly as C99
     * says — and which panics on a zero divisor, where C leaves the program
     * undefined. A panic cannot cross the `extern "C"` frame, so it aborts;
     * that is a runtime property, and nothing a safe function promises. */
    int half(int n) { return n / 2; }
    int remainder_of(int a, int b) { return a % b; }
}

#[test]
fn integer_arithmetic_is_safe() {
    assert_eq!((half(7), half(-7)), (3, -3));
    assert_eq!(remainder_of(-7, 2), -1);
}

// ---------------------------------------------------------------------------
// safe functions calling each other
// ---------------------------------------------------------------------------

c99! {
    #pragma cinrs safe square sum_of_squares

    int square(int n) { return n * n; }

    /* A safe function may call a safe function. Calling one that is not is a
     * diagnostic of this crate's, in C's words: see
     * `tests/ui/safe_calls_unsafe_function.rs`. */
    int sum_of_squares(int a, int b) { return square(a) + square(b); }
}

#[test]
fn a_safe_function_may_call_another() {
    assert_eq!(sum_of_squares(3, 4), 25);
}

gnu99! {
    /* Both spellings on one function, and `inline` beside them. */
    [[cinrs::safe]] __attribute__((cinrs_safe)) inline int triple(int n) {
        return n * 3;
    }
}

c99! {
    #pragma cinrs safe negate

    int negate(int n) { return -n; }

    /* The address of a safe function is an ordinary function pointer: the item
     * is `extern "C" fn`, which coerces to the `unsafe extern "C" fn` a C
     * function pointer is. So a safe function can still be a callback. */
    int through_a_pointer(int n) {
        int (*f)(int) = negate;
        return f(n);
    }
}

#[test]
fn safe_composes_with_inline() {
    assert_eq!(triple(14), 42);
}

#[test]
fn the_address_of_a_safe_function_is_a_function_pointer() {
    assert_eq!(unsafe { through_a_pointer(7) }, -7);
    // And Rust may hand the item out as one too.
    let f: unsafe extern "C" fn(core::ffi::c_int) -> core::ffi::c_int = negate;
    assert_eq!(unsafe { f(3) }, -3);
}

// ---------------------------------------------------------------------------
// safe and exported
// ---------------------------------------------------------------------------

/// A unit whose functions are real C symbols *and* safe: the two are
/// independent, and `#[unsafe(no_mangle)] pub extern "C" fn` is what comes out.
mod exported {
    use cinrs::c99;

    c99! {
        #pragma cinrs export
        #pragma cinrs safe cinrs_test_safe_double

        int cinrs_test_safe_double(int n) { return n * 2; }
    }
}

/// Another unit, which links against the symbol above rather than sharing an
/// item with it. The call is through a declaration, so it is a foreign call
/// and stays `unsafe` — safety belongs to a body, and this unit has none.
mod client {
    use cinrs::c99;

    c99! {
        int cinrs_test_safe_double(int n);
        int quadruple(int n) { return cinrs_test_safe_double(cinrs_test_safe_double(n)); }
    }
}

#[test]
fn an_exported_function_may_be_safe() {
    assert_eq!(exported::cinrs_test_safe_double(21), 42);
    assert_eq!(unsafe { client::quadruple(10) }, 40);
}

// ---------------------------------------------------------------------------
// complex arithmetic
// ---------------------------------------------------------------------------

/// The runtime's complex arithmetic is safe Rust, so a safe function may use
/// it; this needs the `complex` feature, which is on by default.
#[cfg(feature = "complex")]
mod complex {
    use cinrs::c99;
    use cinrs::rt::Complex;

    c99! {
        #pragma cinrs safe scaled real_part

        double _Complex scaled(double _Complex z, double by) { return z * by; }
        double real_part(double _Complex z) { return __real__ z; }
    }

    #[test]
    fn complex_arithmetic_is_safe() {
        let z = Complex::new(1.0, 2.0);
        assert_eq!(scaled(z, 3.0), Complex::new(3.0, 6.0));
        assert_eq!(real_part(z), 1.0);
    }
}

// ---------------------------------------------------------------------------
// the boundary
// ---------------------------------------------------------------------------

c99! {
    #pragma cinrs safe checked

    /* A pointer *value* is safe to have, to compare and to return: it is
     * reading through one that is not. So a safe function can still take and
     * hand back a `T *`, which is what makes one usable at all in a program
     * that has pointers in it. */
    int checked(const int *p) { return p == 0; }

    /* And a non-safe function is free to do the dereferencing, and to call
     * safe functions while it is at it. */
    int first_or_zero(const int *p) { return checked(p) ? 0 : *p; }
}

#[test]
fn a_safe_function_may_hold_a_pointer_without_reading_it() {
    let n = 7;
    assert_eq!(checked(&raw const n), 0);
    assert_eq!(checked(core::ptr::null()), 1);
    assert_eq!(unsafe { first_or_zero(&raw const n) }, 7);
}
