//! Function declarators with no prototype: `int f();` and `int (*fp)();`.
//!
//! C99 6.7.5.3p14 makes an empty parameter list in a *declaration* mean "the
//! parameters are unspecified", and 6.5.2.2p6 says a call through such a type
//! passes its arguments with the default argument promotions applied. C23
//! removed the form — there `int f()` is `int f(void)` — so the two behaviours
//! live in different entry points, and both are checked here.

use cinrs::{c11, c23, c99};

// ---------------------------------------------------------------------------
// declaration, then a call with arguments
// ---------------------------------------------------------------------------

#[test]
fn a_declaration_without_a_prototype_takes_any_arguments() {
    c99! {
        /* Declared with no prototype, defined below with one. Both are legal
           C, and the composite type is the prototype's. */
        int adder();
        double scaler();

        int adder(int a, int b) { return a + b; }
        double scaler(double x) { return x * 2.0; }

        int sums(void) { return adder(2, 3) + adder(10, 20); }
        double doubled(void) { return scaler(1.5); }
    }

    unsafe {
        assert_eq!(sums(), 35);
        assert_eq!(doubled(), 3.0);
    }
}

#[test]
fn the_arguments_of_an_unprototyped_call_are_promoted() {
    c99! {
        /* Nothing here says what `report` takes; the call sites decide, and
           the promotions are what the callee is really written for. */
        int report();

        int report(int c, int s, double f) {
            return c + s + (int)f;
        }

        int small(void) {
            char c = 3;
            short s = 4;
            float f = 5.5f;
            /* `char` and `short` promote to `int`, `float` to `double`. */
            return report(c, s, f);
        }
    }

    assert_eq!(unsafe { small() }, 12);
}

// ---------------------------------------------------------------------------
// pointers to functions with no prototype
// ---------------------------------------------------------------------------

#[test]
fn a_function_pointer_without_a_prototype_holds_any_function() {
    c99! {
        int one_arg(int n) { return n * 2; }
        int two_args(int a, int b) { return a - b; }
        double one_double(double x) { return x + 0.25; }

        int through_pointer(void) {
            int (*fp)() = one_arg;
            int total = fp(21);
            fp = two_args;
            total += fp(10, 4);
            return total;
        }

        double double_through_pointer(void) {
            double (*fp)() = one_double;
            return fp(1.5);
        }
    }

    unsafe {
        assert_eq!(through_pointer(), 48);
        assert_eq!(double_through_pointer(), 1.75);
    }
}

/// c-testsuite `00209`'s shape: a `typedef` for an unprototyped function
/// pointer, passed to functions declared with it and called through `(*fp)(i)`.
#[test]
fn the_c_testsuite_00209_pattern_works() {
    c99! {
        typedef int (*fptr1)();
        int f1 (int (), int);
        typedef int (*fptr2)(int x);
        int f2 (int (int x), int);
        typedef int (*fptr5)(fptr1);
        int f5 (int (int()), fptr1);

        int f1 (fptr1 fp, int i) { return (*fp)(i); }
        int f2 (fptr2 fp, int i) { return (*fp)(i); }
        int f5 (fptr5 fp, fptr1 i) { return fp(i); }

        int negate(int n) { return -n; }
        int count(fptr1 fp) { return fp ? 1 : 0; }

        int run(void) {
            return f1(negate, 3) + f2(negate, 4) + f5(count, negate);
        }
    }

    assert_eq!(unsafe { run() }, -7 + 1);
}

// ---------------------------------------------------------------------------
// a libc function declared without a prototype
// ---------------------------------------------------------------------------

#[test]
fn an_unprototyped_extern_declaration_links_and_is_called_with_promotions() {
    c99! {
        /* The way K&R-era code declares the library: no prototype at all. The
           generated `extern` block says `fn abs()`, and each call site is what
           says how the arguments go. */
        int abs();
        double fabs();

        int magnitudes(void) {
            short s = -7;
            /* `s` promotes to `int`, which is what `abs` really takes. */
            return abs(s) + abs(-3);
        }

        double distance(void) {
            /* `-1.5` is already a `double`, which is what `fabs` takes. */
            return fabs(-1.5) + fabs(2.5);
        }
    }

    unsafe {
        assert_eq!(magnitudes(), 10);
        assert_eq!(distance(), 4.0);
    }
}

// ---------------------------------------------------------------------------
// a definition written `f()`
// ---------------------------------------------------------------------------

#[test]
fn a_definition_with_an_empty_list_takes_no_parameters() {
    c99! {
        int answer() { return 42; }

        /* C99 6.9.1p7: the definition takes no parameters, but its *type* has
           no prototype either, so a call with an argument is legal C — the
           callee simply never looks at it. */
        int called_both_ways(void) { return answer() + answer(1); }
    }

    assert_eq!(unsafe { called_both_ways() }, 84);
}

// ---------------------------------------------------------------------------
// compatibility, which `_Generic` asks about
// ---------------------------------------------------------------------------

/// C11 6.5.1.1p2 selects the association whose type is *compatible* with the
/// controlling expression's, and 6.7.5.3p15 says an empty parameter list is
/// compatible with a prototype whose parameters are their own promoted forms.
#[test]
fn generic_selects_the_compatible_prototype() {
    c11! {
        int twice(int n) { return n * 2; }

        int chosen(void) {
            int (*fp)() = twice;
            /* `int (*)()` and `int (*)(int)` are compatible types, so this is
               the association that matches. */
            return _Generic(fp, int (*)(int): 1, double: 2, default: 0);
        }
    }

    assert_eq!(unsafe { chosen() }, 1);
}

// ---------------------------------------------------------------------------
// C23 removed the form
// ---------------------------------------------------------------------------

#[test]
fn c23_reads_an_empty_list_as_void() {
    c23! {
        int nothing();
        int nothing(void) { return 5; }

        int call(void) { return nothing(); }
    }

    assert_eq!(unsafe { call() }, 5);
}
