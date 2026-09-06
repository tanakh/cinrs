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

/// `auto` is a storage-class specifier that stands *beside* the others.
///
/// C23 6.7.1p2 keeps "at most one storage-class specifier" and then makes
/// `auto` the exception: it "may appear with all the others, except
/// `typedef`". So `static auto c = 1UL;` is an object with static storage
/// duration whose type is inferred, and the exception is symmetric —
/// `auto static c = 1UL;` says the same thing. Clang's `C23/n3007.c` and
/// `C23/n3006.c` are where the shapes come from.
#[test]
fn auto_may_stand_beside_another_storage_class() {
    c23! {
        unsigned long counter(void) {
            static auto c = 1UL;
            auto static d = 2UL;
            register auto e = 3UL;
            auto register f = 4UL;
            c += 10;
            return c + d + e + f;
        }

        /* `constexpr` is one of the others, and the object is still a
         * constant of the initialiser's type. */
        int folded(void) {
            constexpr auto limit = 7;
            auto constexpr other = 2;
            return limit * other;
        }

        /* The type is the initialiser's, not `int`: 1UL is `unsigned long`,
         * so `c` is one and `sizeof` says so. */
        int inferred_the_wide_type(void) {
            static auto c = 1UL;
            return sizeof(c) == sizeof(unsigned long);
        }

        /* `_Atomic` is not a *specifier* here but a qualifier, so it applies
         * to whatever was inferred (N3007). */
        int atomic_type(void) {
            _Atomic auto n = 12;
            _Atomic auto p = "really?";
            return _Generic(&n, _Atomic(int) *: 1, default: 0)
                 + _Generic(&p, _Atomic(char *) *: 1, default: 0);
        }
    }

    unsafe {
        assert_eq!(counter(), 11 + 2 + 3 + 4);
        // The `static` really is static: a second call sees the first's store.
        assert_eq!(counter(), 21 + 2 + 3 + 4);
        assert_eq!(folded(), 14);
        assert_eq!(inferred_the_wide_type(), 1);
        assert_eq!(atomic_type(), 2);
    }
}

/// An array's qualifiers belong to its *elements* (C99 6.7.3p9), whichever
/// spelling put them there.
///
/// `const int a[1]` writes the qualifier on `int` and there is nothing to do;
/// `typedef int A[1]; const A a;` writes it on the array, and C moves it to
/// the elements — so `&a` is a `const int (*)[1]` either way, and the two
/// spellings are one type. WG14 N2607 is the paper, and Clang's
/// `C23/n2607.c` asks the question with `_Generic`, which is the only way to
/// see the qualifier: an ordinary controlling expression loses it to the
/// lvalue conversion.
#[test]
fn an_arrays_qualifiers_are_its_elements() {
    c23! {
        typedef int A1[1];
        typedef int A2[2][3];

        int qualifiers_reach_the_elements(void) {
            const int spelled_out[1] = { 0 };
            const A1 through_a_typedef = { 0 };
            const int md[2][3] = {{ 0 }};
            const A2 md_typedef = {{ 0 }};

            return _Generic(&spelled_out, const int (*)[1]: 1, default: 0)
                 + _Generic(&through_a_typedef, const int (*)[1]: 1, default: 0)
                 + _Generic(&through_a_typedef[0], const int *: 1, default: 0)
                 + _Generic(&md, const int (*)[2][3]: 1, default: 0)
                 + _Generic(&md_typedef, const int (*)[2][3]: 1, default: 0)
                 + _Generic(&md_typedef[0], const int (*)[3]: 1, default: 0);
        }

        /* 6.5.15p6 qualifies the composite of a conditional with the
         * qualifiers of *both* operands, which is not a directed question:
         * the answer is the same whichever branch is written first. */
        int the_composite_of_a_conditional(int c) {
            const int konst[1] = { 0 };
            int plain[1] = { 0 };
            return _Generic(c ? &konst : &plain, const int (*)[1]: 1, default: 0)
                 + _Generic(c ? &plain : &konst, const int (*)[1]: 1, default: 0);
        }
    }

    unsafe {
        assert_eq!(qualifiers_reach_the_elements(), 6);
        assert_eq!(the_composite_of_a_conditional(1), 2);
    }
}

/// The underlying type of an enumeration, repeated or asked nothing about.
///
/// N3030 makes every declaration of one enumeration agree about the fixed
/// underlying type; `tests/ui/c23_enum_underlying_type.rs` is what happens
/// when they do not. Repeating the same type is fine, and a mention that
/// writes no type at all — `enum E x;` — asks nothing and settles nothing.
#[test]
fn the_fixed_underlying_type_may_be_repeated() {
    c23! {
        enum Repeated : short;
        enum Repeated : short { r1 = 3 };
        enum Repeated repeated_value = r1;

        enum Plain : unsigned char { p1 = 200 };
        enum Plain plain_value = p1;

        int widths(void) {
            return sizeof(enum Repeated) == sizeof(short)
                && sizeof(enum Plain) == 1
                && repeated_value == 3
                && plain_value == 200;
        }
    }

    assert_eq!(unsafe { widths() }, 1);
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
        #[allow(deprecated)]
        let value = old_name();
        assert_eq!(value, 42);
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

/// C23's `#embed` (N3017), with all four of its standard parameters.
///
/// Every expected value here was checked against `gcc -std=c23` reading the
/// same two files. The resource is a real binary — a PNG signature, a NUL and
/// a `0xff` — so that nothing about the test would work if the bytes went
/// through text.
#[test]
fn embed_puts_the_bytes_of_a_file_into_the_program() {
    c23! { r##"
        /* The quoted form looks next to the file the directive is written in,
           which for the macro's own text is the directory of the `.rs`. The
           angled form takes the include path, and nothing else. */
        #pragma cinrs include_path "tests/include"

        static const unsigned char logo[] = {
        #embed "include/data.bin"
        };
        static const unsigned char angled[] = {
        #embed <data.bin>
        };
        static const unsigned char capped[] = {
        #embed <data.bin> limit(4) prefix(0xAA,) suffix(, 0xBB)
        };
        /* An empty resource is `if_empty`'s tokens and nothing else: the
           prefix and the suffix are not emitted at all. */
        static const unsigned char nothing[] = {
        #embed <empty.bin> if_empty(1, 2) prefix(7,) suffix(, 9)
        };

        #if __has_embed(<data.bin>) == __STDC_EMBED_FOUND__
        #define FOUND 1
        #endif
        #if __has_embed(<empty.bin>) == __STDC_EMBED_EMPTY__
        #define EMPTY 1
        #endif
        #if __has_embed(<nowhere.bin>) == __STDC_EMBED_NOT_FOUND__
        #define MISSING 1
        #endif
        /* `limit(0)` makes even a resource with bytes in it an empty one. */
        #if __has_embed(<data.bin> limit(0)) == __STDC_EMBED_EMPTY__
        #define LIMITED 1
        #endif

        int logo_size(void) { return (int) sizeof logo; }
        int logo_at(int i) { return logo[i]; }
        int angled_size(void) { return (int) sizeof angled; }
        int capped_size(void) { return (int) sizeof capped; }
        int capped_at(int i) { return capped[i]; }
        int nothing_size(void) { return (int) sizeof nothing; }
        int nothing_at(int i) { return nothing[i]; }
        int answers(void) { return FOUND + EMPTY * 10 + MISSING * 100 + LIMITED * 1000; }
    "## }

    unsafe {
        assert_eq!(logo_size(), 10);
        assert_eq!(
            (logo_at(0), logo_at(1), logo_at(8), logo_at(9)),
            (0x89, 0x50, 0x00, 0xff)
        );
        assert_eq!(angled_size(), 10);
        // 0xAA, the first four bytes, 0xBB.
        assert_eq!(capped_size(), 6);
        assert_eq!(
            (capped_at(0), capped_at(1), capped_at(4), capped_at(5)),
            (0xaa, 0x89, 0x47, 0xbb)
        );
        assert_eq!(nothing_size(), 2);
        assert_eq!((nothing_at(0), nothing_at(1)), (1, 2));
        assert_eq!(answers(), 1111);
    }
}

/// C23's `u8` character prefix (N2418) and `char8_t` (N2653).
///
/// The revision also changed the element type of a `u8"…"` string from `char`
/// to `char8_t`, which is an `unsigned char` — the one part of the Unicode
/// literals that is not the same in `c11!`.
#[test]
fn the_u8_character_prefix_and_char8_t() {
    c23! { r##"
        #include <uchar.h>

        static const char8_t utf8[] = u8"héllo";

        int u8_char(void) { return u8'x'; }
        unsigned long u8_char_size(void) { return sizeof u8'x'; }
        int u8_char_is_unsigned_char(void) {
            return _Generic(u8'x', unsigned char: 1, default: 0);
        }
        int u8_string_is_char8_t(void) {
            return _Generic(u8"x", unsigned char *: 1, default: 0);
        }
        unsigned long u8_at(int i) { return utf8[i]; }
    "## }

    unsafe {
        assert_eq!(u8_char(), 0x78);
        assert_eq!(u8_char_size(), 1);
        assert_eq!(u8_char_is_unsigned_char(), 1);
        assert_eq!(u8_string_is_char8_t(), 1);
        assert_eq!((u8_at(0), u8_at(1), u8_at(2)), (0x68, 0xc3, 0xa9));
    }
}

#[test]
fn the_headers_follow_the_revision() {
    c23! {
        #include <assert.h>
        #include <stdalign.h>
        #include <stdbool.h>
        #include <stddef.h>
        #include <uchar.h>

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

// ---------------------------------------------------------------------------
// what C23 added to declarations
// ---------------------------------------------------------------------------

#[test]
fn a_parameter_of_a_definition_may_go_unnamed() {
    // N2480. The body cannot reach the parameter, but the caller still passes
    // one, so the generated item still takes an argument in that position.
    c23! {
        int second(int, int b) { return b; }
        int third(int, int, int c) { return c; }
    }

    unsafe {
        assert_eq!(second(1, 2), 2);
        assert_eq!(third(1, 2, 3), 3);
    }
}

#[test]
fn an_enumerator_too_large_for_int_widens_the_enumeration() {
    // N3029: the enumeration's underlying type widens so that every value
    // fits, and *every* enumerator then has that type — which is what
    // `_Generic` selects on. Before C23 it was a constraint violation.
    c23! {
        #include <limits.h>

        enum wide { small = 1, huge = ULLONG_MAX };
        enum signed_too { low = -1, high = 3000000000 };

        /* The widened type is the narrowest that holds every value, which on
           an LP64 target makes `ULLONG_MAX` an `unsigned long` — the same
           choice Clang makes. What the standard fixes is not *which* type it
           is but that every enumerator has it. */
        int same_type(void) {
            return _Generic(small, unsigned long: 1, default: 0)
                == _Generic(huge, unsigned long: 1, default: 0);
        }
        int widened_to_unsigned(void) {
            return _Generic(huge, unsigned long: 1, default: 0);
        }
        int widened_to_signed(void) {
            return _Generic(high, long: 1, default: 0)
                && _Generic(low, long: 1, default: 0);
        }
        unsigned long long biggest(void) { return huge; }
        long negative(void) { return low; }
        int fits_int_stays_int(void) {
            enum narrow { a = 1, b = 2 };
            return _Generic(a, int: 1, default: 0);
        }
    }

    unsafe {
        assert_eq!(same_type(), 1);
        assert_eq!(widened_to_unsigned(), 1);
        assert_eq!(widened_to_signed(), 1);
        assert_eq!(biggest(), u64::MAX);
        assert_eq!(negative(), -1);
        assert_eq!(fits_int_stays_int(), 1);
    }
}

#[test]
fn stdckdint_reports_overflow() {
    // C23 7.20 (N2683). The arithmetic is done in infinite precision and the
    // answer is whether the result fit the type `*r` has — the types of the
    // operands decide nothing.
    c23! {
        #include <stdckdint.h>
        #include <stdint.h>

        int add_fits(void) {
            int64_t r = 0;
            bool overflowed = ckd_add(&r, INT32_MAX, 1);
            return !overflowed && r == 2147483648LL;
        }
        int sub_overflows(void) {
            int32_t r = 0;
            bool overflowed = ckd_sub(&r, INT32_MAX, -1);
            return overflowed;
        }
        int mul_fits(void) {
            int r = 0;
            int a = 3;
            bool overflowed = ckd_mul(&r, a, 2);
            return !overflowed && r == 6;
        }
        int version_macro(void) {
            return __STDC_VERSION_STDCKDINT_H__ >= 202311L;
        }
    }

    unsafe {
        assert_eq!(add_fits(), 1);
        assert_eq!(sub_overflows(), 1);
        assert_eq!(mul_fits(), 1);
        assert_eq!(version_macro(), 1);
    }
}

#[test]
fn a_label_may_stand_before_a_case_group_that_is_empty() {
    // N2508 again, for the labels the earlier test does not cover: `case` and
    // `default` at the end of a compound statement, and a declaration
    // immediately after one.
    c23! {
        int trailing_case(int x) {
            switch (x) {
            case 1:
                return 1;
            case 2:
            }
            return 0;
        }

        int declaration_after_case(int x) {
            switch (x) {
            case 1:
                static_assert(1, "");
                int y = 7;
                return y;
            default:
                static_assert(1, "");
                return -1;
            }
        }
    }

    unsafe {
        assert_eq!(trailing_case(1), 1);
        assert_eq!(trailing_case(2), 0);
        assert_eq!(declaration_after_case(1), 7);
        assert_eq!(declaration_after_case(9), -1);
    }
}
