//! Integration tests that *run* translated C23.
//!
//! `c23!` is `c11!` plus the keywords C23 promoted (`bool`, `true`, `false`,
//! `nullptr`, `static_assert`, `alignof`, `constexpr`, `typeof`), `[[…]]`
//! attributes, `__VA_OPT__`, `#elifdef`, binary constants, digit separators,
//! empty initialisers, `auto`, enumerations with a fixed underlying type and
//! `unreachable()`.

use cinrs::{c17, c23};

// ---------------------------------------------------------------------------
// keywords
// ---------------------------------------------------------------------------

#[test]
fn bool_true_false_and_nullptr_need_no_header() {
    c23! {
        bool negate(bool b) { return !b; }
        bool always(void) { return true; }
        bool never(void) { return false; }

        int *nothing(void) { return nullptr; }
        bool is_null(const int *p) { return p == nullptr; }

        static_assert(sizeof(bool) == 1);
        static_assert(true, "true is 1");

        #if true
        int with_true_in_a_condition(void) { return 1; }
        #else
        int with_true_in_a_condition(void) { return 0; }
        #endif
    }

    unsafe {
        assert!(!negate(true));
        assert!(negate(false));
        assert!(always());
        assert!(!never());
        assert!(nothing().is_null());
        assert!(is_null(core::ptr::null()));
        let one = 1;
        assert!(!is_null(&raw const one));
        assert_eq!(with_true_in_a_condition(), 1);
    }
}

#[test]
fn constexpr_objects_are_constants() {
    c23! {
        constexpr int LIMIT = 4;
        constexpr double HALF = 0.5;

        int table[LIMIT] = { 1, 2, 3, 4 };

        int total(void) {
            int sum = 0;
            for (int i = 0; i < LIMIT; i++) sum += table[i];
            return sum;
        }

        double halve(int n) { return n * HALF; }

        /* A `constexpr` object is a constant expression, so it may be a
           `case` label and an array bound. */
        int classify(int n) {
            switch (n) {
                case LIMIT: return 1;
                default: return 0;
            }
        }
    }

    unsafe {
        assert_eq!(total(), 10);
        assert_eq!(halve(7), 3.5);
        assert_eq!(classify(4), 1);
        assert_eq!(classify(5), 0);
    }
}

#[test]
fn typeof_names_the_type_of_an_expression() {
    c23! {
        int widen(int n) {
            typeof(n) copy = n;
            typeof(1.5) scaled = copy * 2.0;
            return (int)scaled;
        }

        /* An array keeps its type: `typeof` does not decay. */
        int first(void) {
            int values[3] = { 7, 8, 9 };
            typeof(values) copy = { 1, 2, 3 };
            return (int)(sizeof(copy) / sizeof(copy[0])) + values[0] + copy[0];
        }

        typeof(int *) allocate(void) { return nullptr; }

        int unqualified(void) {
            const int n = 5;
            typeof_unqual(n) m = n;
            m++;
            return m;
        }
    }

    unsafe {
        assert_eq!(widen(3), 6);
        assert_eq!(first(), 11);
        assert!(allocate().is_null());
        assert_eq!(unqualified(), 6);
    }
}

#[test]
fn auto_infers_the_type_of_a_local() {
    c23! {
        int inferred(void) {
            auto n = 3;
            auto d = 1.5;
            auto p = &n;
            return n + (int)d + *p;
        }
    }

    assert_eq!(unsafe { inferred() }, 7);
}

// ---------------------------------------------------------------------------
// literals
// ---------------------------------------------------------------------------

#[test]
fn binary_constants_work_in_code_and_in_conditions() {
    c23! {
        unsigned mask(void) { return 0b1011; }
        unsigned wide(void) { return 0B10000000 >> 3; }

        #if 0b101 == 5
        int binary_in_if(void) { return 1; }
        #else
        int binary_in_if(void) { return 0; }
        #endif
    }

    unsafe {
        assert_eq!(mask(), 0b1011);
        assert_eq!(wide(), 0b10000);
        assert_eq!(binary_in_if(), 1);
    }
}

#[test]
fn digit_separators_need_string_literal_form() {
    // Rust's own lexer reads `1'000` as a literal followed by a lifetime and
    // refuses it, so a constant with separators goes in a string literal.
    c23! { r#"
        long million(void) { return 1'000'000; }
        unsigned bytes(void) { return 0xFF'FF; }
        double precise(void) { return 1.234'567; }
    "# }

    unsafe {
        assert_eq!(million(), 1_000_000);
        assert_eq!(bytes(), 0xffff);
        assert!((precise() - 1.234_567).abs() < 1e-9);
    }
}

#[test]
fn an_empty_initializer_zeroes() {
    c23! {
        struct Point { int x; int y; };

        int zeroed(void) {
            struct Point p = {};
            int a[4] = {};
            int n = {};
            return p.x + p.y + a[0] + a[3] + n;
        }
    }

    assert_eq!(unsafe { zeroed() }, 0);
}

// ---------------------------------------------------------------------------
// enumerations
// ---------------------------------------------------------------------------

#[test]
fn an_enum_can_fix_its_underlying_type() {
    c23! {
        enum Colour : unsigned char { RED, GREEN, BLUE = 200 };

        enum Colour brightest(void) { return BLUE; }
        unsigned long width(void) { return sizeof(enum Colour); }
        int is_green(enum Colour c) { return c == GREEN; }
    }

    unsafe {
        assert_eq!(brightest(), 200);
        assert_eq!(width(), 1);
        assert_eq!(is_green(1), 1);
        // The tag is a Rust alias for the underlying type.
        let c: Colour = RED;
        assert_eq!(c, 0u8);
    }
}

// ---------------------------------------------------------------------------
// attributes
// ---------------------------------------------------------------------------

#[test]
fn attributes_are_accepted_and_ignored() {
    c23! {
        [[nodiscard]] int important([[maybe_unused]] int ignored) { return 42; }

        [[deprecated("use important")]] int old_name(void) { return important(0); }

        struct Marked {
            [[deprecated]] int legacy;
            int current;
        };

        int fallthrough(int n) {
            int out = 0;
            switch (n) {
                case 0:
                    out += 1;
                    [[fallthrough]];
                case 1:
                    out += 10;
                    break;
                default:
                    out = -1;
            }
            return out;
        }
    }

    unsafe {
        assert_eq!(important(1), 42);
        assert_eq!(old_name(), 42);
        assert_eq!(fallthrough(0), 11);
        assert_eq!(fallthrough(1), 10);
        assert_eq!(fallthrough(2), -1);
        assert_eq!(size_of::<Marked>(), 8);
    }
}

// ---------------------------------------------------------------------------
// the preprocessor
// ---------------------------------------------------------------------------

#[test]
fn va_opt_makes_a_logging_macro_work_with_no_arguments() {
    c23! {
        #include <stdio.h>

        #define LOG(fmt, ...) snprintf(buf, size, fmt __VA_OPT__(,) __VA_ARGS__)

        int log_plain(char *buf, unsigned long size) { return LOG("done"); }
        int log_one(char *buf, unsigned long size) { return LOG("n=%d", 7); }
        int log_two(char *buf, unsigned long size) { return LOG("%d/%d", 3, 4); }
    }

    let mut buf = [0u8; 32];
    unsafe {
        let n = log_plain(buf.as_mut_ptr().cast(), 32);
        assert_eq!(&buf[..n as usize], b"done");
        let n = log_one(buf.as_mut_ptr().cast(), 32);
        assert_eq!(&buf[..n as usize], b"n=7");
        let n = log_two(buf.as_mut_ptr().cast(), 32);
        assert_eq!(&buf[..n as usize], b"3/4");
    }
}

#[test]
fn elifdef_and_elifndef_choose_a_branch() {
    c23! {
        #define SECOND 1

        #ifdef FIRST
        int which(void) { return 1; }
        #elifdef SECOND
        int which(void) { return 2; }
        #elifndef THIRD
        int which(void) { return 3; }
        #else
        int which(void) { return 4; }
        #endif

        #ifdef FIRST
        int other(void) { return 1; }
        #elifndef FIRST
        int other(void) { return 2; }
        #endif
    }

    unsafe {
        assert_eq!(which(), 2);
        assert_eq!(other(), 2);
    }
}

#[test]
fn unreachable_says_control_never_gets_here() {
    c23! {
        #include <stddef.h>

        int positive(int n) {
            if (n > 0) return n;
            if (n <= 0) return -n;
            unreachable();
        }
    }

    unsafe {
        assert_eq!(positive(3), 3);
        assert_eq!(positive(-3), 3);
    }
}

// ---------------------------------------------------------------------------
// versions
// ---------------------------------------------------------------------------

#[test]
fn the_version_macros_say_c23_and_c17() {
    c23! {
        long c23_version(void) { return __STDC_VERSION__; }
    }

    c17! {
        long c17_version(void) { return __STDC_VERSION__; }

        /* C17 is C11 with a different version, so C11's features are there
           and C23's are not. */
        _Static_assert(__STDC_VERSION__ == 201710L, "C17");
    }

    unsafe {
        assert_eq!(c23_version(), 202311);
        assert_eq!(c17_version(), 201710);
    }
}

#[test]
fn labels_may_stand_before_a_declaration_and_at_the_end_of_a_block() {
    c23! {
        int labelled(int n) {
            int total = n;
        again:
            /* C23 lets a label stand before a declaration ... */
            int step = 1;
            total += step;
            if (total < 3) goto again;
            return total;
        done:
            /* ... and at the very end of a compound statement. */
        }
    }

    unsafe {
        assert_eq!(labelled(0), 3);
        assert_eq!(labelled(5), 6);
    }
}

#[test]
fn the_headers_follow_the_revision() {
    c23! {
        #include <assert.h>
        #include <stdalign.h>
        #include <stdbool.h>
        #include <stddef.h>

        /* In C23 `bool`, `true`, `alignas` and `static_assert` are keywords,
           so the headers define nothing for them — and everything below still
           has to work. */
        static_assert(__bool_true_false_are_defined == 1, "<stdbool.h> was read");
        static_assert(__alignas_is_defined == 1, "<stdalign.h> was read");

        struct Wide { alignas(16) int first; };

        bool truth(void) { return true; }
        unsigned long wide(void) { return alignof(struct Wide); }
        nullptr_t nothing(void) { return nullptr; }

        int checked(int n) {
            assert(n >= 0);
            return n;
        }
    }

    unsafe {
        assert!(truth());
        assert_eq!(wide(), 16);
        assert!(nothing().is_null());
        assert_eq!(checked(1), 1);
    }
}

#[test]
fn attributes_are_accepted_after_the_declared_name_too() {
    c23! {
        int counted [[deprecated]];
        int with_attribute [[maybe_unused]] = 3;

        int read(void) { return with_attribute + counted; }
    }

    assert_eq!(unsafe { read() }, 3);
}
