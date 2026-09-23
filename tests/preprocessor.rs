//! Integration tests that *run* code the preprocessor produced.
//!
//! The unit tests in `cinrs-core` check what the preprocessor makes of a piece
//! of text; these check that what it makes compiles and computes the right
//! answer, in both input modes — which matters because Rust's own lexer
//! refuses `##` in raw-token mode, and the `# #` spelling that replaces it has
//! to work just as well.

use cinrs::c99;

// ---------------------------------------------------------------------------
// object-like and function-like macros in real code
// ---------------------------------------------------------------------------

#[test]
fn constants_and_expression_macros() {
    c99! {
        #define WIDTH 8
        #define HEIGHT 4
        #define AREA (WIDTH * HEIGHT)
        #define MAX(a, b) ((a) > (b) ? (a) : (b))
        #define SQUARE(x) ((x) * (x))

        int area(void) { return AREA; }

        int largest(int a, int b, int c) {
            return MAX(MAX(a, b), c);
        }

        /* The classic reason `SQUARE(x)` parenthesises `x`. */
        int square_of_sum(int a, int b) {
            return SQUARE(a + b);
        }

        int table[HEIGHT][WIDTH];

        int fill(void) {
            int total = 0;
            for (int y = 0; y < HEIGHT; y++) {
                for (int x = 0; x < WIDTH; x++) {
                    table[y][x] = y * WIDTH + x;
                    total += table[y][x];
                }
            }
            return total;
        }
    }

    unsafe {
        assert_eq!(area(), 32);
        assert_eq!(largest(3, 9, 4), 9);
        assert_eq!(largest(-3, -9, -4), -3);
        assert_eq!(square_of_sum(2, 3), 25);
        // 0 + 1 + … + 31
        assert_eq!(fill(), 31 * 32 / 2);
        assert_eq!({ table[3][7] }, 31);
    }
}

#[test]
fn a_macro_may_expand_to_a_whole_declaration() {
    c99! {
        #define ACCESSOR(name, value) int name(void) { return value; }

        ACCESSOR(one, 1)
        ACCESSOR(two, 2)
        ACCESSOR(sum, 1 + 2)
    }

    unsafe {
        assert_eq!(one(), 1);
        assert_eq!(two(), 2);
        assert_eq!(sum(), 3);
    }
}

// ---------------------------------------------------------------------------
// conditional compilation
// ---------------------------------------------------------------------------

#[test]
fn an_if_selects_an_implementation() {
    c99! {
        #define USE_FAST 1

        #if USE_FAST
        int triple(int n) { return (n << 1) + n; }
        #else
        int triple(int n) {
            int total = 0;
            for (int i = 0; i < 3; i++) total += n;
            return total;
        }
        #endif

        #if defined(NOT_DEFINED)
        this text is never even lexed: 08 'oops @@@
        #elif USE_FAST && !defined(SLOW)
        int chosen(void) { return 1; }
        #else
        int chosen(void) { return 0; }
        #endif
    }

    unsafe {
        assert_eq!(triple(14), 42);
        assert_eq!(chosen(), 1);
    }
}

#[test]
fn the_crate_identifies_itself() {
    c99! {
        #ifdef __cinrs__
        int built_by_cinrs(void) { return __STDC__ + __STDC_HOSTED__; }
        #else
        int built_by_cinrs(void) { return 0; }
        #endif

        #if __STDC_VERSION__ >= 199901L
        int is_c99(void) { return 1; }
        #else
        int is_c99(void) { return 0; }
        #endif
    }

    unsafe {
        assert_eq!(built_by_cinrs(), 2);
        assert_eq!(is_c99(), 1);
    }
}

#[test]
fn line_and_file_describe_the_rust_source() {
    // `__LINE__` is a line of *this* file, not of the C text inside the macro,
    // so it is a number the user can find in their editor. Rust's own `line!`
    // is the reference.
    let before = line!();
    c99! {
        #define HERE __LINE__
        int first(void) { return HERE; }

        int second(void) { return HERE; }

        const char *source_file(void) { return __FILE__; }
    }

    unsafe {
        // `c99! {` sits on `before + 1`, `#define` on `before + 2`.
        assert_eq!(first() as u32, before + 3);
        assert_eq!(second() as u32, before + 5);
        let file = core::ffi::CStr::from_ptr(source_file()).to_str().unwrap();
        assert!(
            file.ends_with("preprocessor.rs"),
            "__FILE__ should name this file, got {file:?}"
        );
    }
}

#[test]
fn line_redirects_line_and_file_and_nothing_else() {
    c99! {
        int before(void) { return __LINE__; }
        #line 500 "generated.c"
        int after(void) { return __LINE__; }
        int later(void) { return __LINE__; }
        const char *renumbered_source(void) { return __FILE__; }
    }

    unsafe {
        // Before the directive `__LINE__` is still a line of *this* file, which
        // is what makes it a number the user can find in their editor; after
        // it, it is what the directive asked for.
        assert!(before() > 100, "the .rs line, not the C one: {}", before());
        assert_eq!(after(), 500);
        assert_eq!(later(), 501);
        let file = core::ffi::CStr::from_ptr(renumbered_source())
            .to_str()
            .unwrap();
        assert_eq!(file, "generated.c");
    }
}

#[test]
fn a_line_directive_in_a_header_ends_with_the_header() {
    c99! {
        #include "include/renumbered.h"

        const char *outer_file(void) { return __FILE__; }
    }

    unsafe {
        assert_eq!(renumbered_line(), 90);
        let inner = core::ffi::CStr::from_ptr(renumbered_file())
            .to_str()
            .unwrap();
        assert_eq!(inner, "elsewhere.c");
        // Back in the includer, `__FILE__` is this `.rs` file again.
        let outer = core::ffi::CStr::from_ptr(outer_file()).to_str().unwrap();
        assert!(
            outer.ends_with("preprocessor.rs"),
            "__FILE__ should name this file again, got {outer:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// token pasting, in both input modes
// ---------------------------------------------------------------------------

// String-literal mode can spell `##` directly.
c99! { r#"
#define DEFINE_ADDER(suffix, amount) \
    int add_ ## suffix(int n) { return n + amount; }

DEFINE_ADDER(one, 1)
DEFINE_ADDER(ten, 10)
DEFINE_ADDER(hundred, 100)

#define NAME(a, b) a ## b
int NAME(joined, _name)(void) { return 7; }
"# }

// Raw-token mode cannot: Rust's lexer reserves `##`. `# #` means the same
// thing to this preprocessor, in every input mode.
c99! {
    /* A raw-token block cannot use `\` to continue a line either, so the
       replacement list stays on one. */
    #define DEFINE_SCALER(suffix, factor) int scale_ # # suffix(int n) { return n * factor; }

    DEFINE_SCALER(two, 2)
    DEFINE_SCALER(three, 3)

    #define STRINGIFY(x) #x
    const char *name_of_two(void) { return STRINGIFY(scale_two); }
}

// A substituted argument is spaced like the parameter it replaces, not like it
// was written in the invocation — brotli stringifies
// `BROTLI_MAKE_VERSION(__GNUC__, __GNUC_MINOR__, __GNUC_PATCHLEVEL__)` through
// two macros. Each expectation is what GCC and Clang make of the line.
c99! { r#"
#include <string.h>

#define V(a, b) ((a)+(b))
#define F(x) [x]
#define EMPTY
#define S_(x) #x
#define S(x) S_(x)
#define SV_(...) #__VA_ARGS__
#define SV(...) SV_(__VA_ARGS__)

/* Bit i set when case i comes out wrong. */
int stringified_spacing_failures(void) {
    const char *got[] = {
        S(V(1, 2)),
        S_(V(1, 2)),
        SV(a, b),
        S(F( 1 )),
        S_(  a
             +   b  ),
        S(F()),
        S([ EMPTY]),
    };
    const char *want[] = {
        "((1)+(2))",
        "V(1, 2)",
        "a, b",
        "[1]",
        "a + b",
        "[]",
        "[ ]",
    };
    int failures = 0;
    for (int i = 0; i < (int)(sizeof got / sizeof got[0]); i++) {
        if (strcmp(got[i], want[i]) != 0) {
            failures |= 1 << i;
        }
    }
    return failures;
}
"# }

#[test]
fn stringified_arguments_are_spaced_like_gcc() {
    unsafe {
        assert_eq!(stringified_spacing_failures(), 0);
    }
}

#[test]
fn pasting_generates_functions_in_string_literal_mode() {
    unsafe {
        assert_eq!(add_one(41), 42);
        assert_eq!(add_ten(32), 42);
        assert_eq!(add_hundred(-58), 42);
        assert_eq!(joined_name(), 7);
    }
}

#[test]
fn pasting_generates_functions_in_raw_token_mode() {
    unsafe {
        assert_eq!(scale_two(21), 42);
        assert_eq!(scale_three(14), 42);
        let name = core::ffi::CStr::from_ptr(name_of_two()).to_str().unwrap();
        assert_eq!(name, "scale_two");
    }
}

// ---------------------------------------------------------------------------
// variadic macros
// ---------------------------------------------------------------------------

#[test]
fn a_variadic_macro_forwards_to_a_variadic_extern_call() {
    c99! {
        int snprintf(char *out, unsigned long size, const char *fmt, ...);

        #define LOG(buf, fmt, ...) snprintf(buf, 64, "[log] " fmt, __VA_ARGS__)
        /* `...` matching nothing is allowed, so this one needs no arguments. */
        #define LOG0(buf, fmt) snprintf(buf, 64, "[log] " fmt)

        int log_pair(char *buf, int a, double b) {
            return LOG(buf, "a=%d b=%.1f", a, b);
        }

        int log_plain(char *buf) {
            return LOG0(buf, "nothing to say");
        }
    }

    let mut buffer = [0i8; 64];
    unsafe {
        let written = log_pair(buffer.as_mut_ptr(), 42, 1.5);
        let text = core::ffi::CStr::from_ptr(buffer.as_ptr());
        assert_eq!(text.to_bytes(), b"[log] a=42 b=1.5");
        assert_eq!(written, 16);

        let written = log_plain(buffer.as_mut_ptr());
        let text = core::ffi::CStr::from_ptr(buffer.as_ptr());
        assert_eq!(text.to_bytes(), b"[log] nothing to say");
        assert_eq!(written, 20);
    }
}

// ---------------------------------------------------------------------------
// names that would shadow a generated item (E0530)
// ---------------------------------------------------------------------------

#[test]
fn a_parameter_may_share_a_name_with_a_global() {
    // `static mut counter` and the parameter are two different objects, and
    // Rust refuses a binding that shadows a static — so the parameter has to
    // be renamed apart.
    c99! {
        int counter;

        int bump(int counter) {
            return counter + 1;
        }

        int bump_the_global(void) {
            counter++;
            return counter;
        }
    }

    unsafe {
        assert_eq!(bump(41), 42);
        assert_eq!(bump_the_global(), 1);
        assert_eq!(bump_the_global(), 2);
        assert_eq!({ counter }, 2);
        // The global is untouched by the parameter of the same name.
        assert_eq!(bump(0), 1);
        assert_eq!({ counter }, 2);
    }
}

#[test]
fn a_local_may_share_a_name_with_an_enumerator() {
    // An enumerator becomes a `const`, which a `let` may not shadow either.
    c99! {
        enum Colour { Red, Green, Blue };

        int pick(int which) {
            int Red = 10;
            int Green = 20;
            if (which == Blue) {
                return Red + Green;
            }
            return Red;
        }

        int enumerator_values(void) {
            return Red * 100 + Green * 10 + Blue;
        }
    }

    unsafe {
        assert_eq!(pick(2), 30);
        assert_eq!(pick(0), 10);
        assert_eq!(enumerator_values(), 12);
    }
}

#[test]
fn a_switch_hoisted_local_may_share_a_name_with_a_global() {
    // A local declared directly in a `switch` body is defined ahead of the
    // dispatch, which is one more place the rename has to reach.
    c99! {
        int total;
        enum Step { First, Second };

        int step(int n) {
            switch (n) {
                int total;
                int First;
            case 0:
                total = 1;
                First = 2;
                return total + First;
            case 1:
                total = 10;
                return total;
            }
            return -1;
        }
    }

    unsafe {
        total = 99;
        assert_eq!(step(0), 3);
        assert_eq!(step(1), 10);
        assert_eq!(step(7), -1);
        assert_eq!({ total }, 99);
    }
}

#[test]
fn a_local_may_be_called_some_or_none() {
    // Rust's prelude puts `Some`, `None`, `Ok` and `Err` in the pattern
    // namespace, so a C variable of that name needs the same treatment.
    c99! {
        int optional(int flag) {
            int Some = 1;
            int None = 2;
            int Ok = 4;
            int Err = 8;
            if (flag) {
                return Some + Ok;
            }
            return None + Err;
        }
    }

    unsafe {
        assert_eq!(optional(1), 5);
        assert_eq!(optional(0), 10);
    }
}

#[test]
fn a_shadowing_name_survives_the_state_machine_lowering() {
    // A function that jumps is lowered into a state machine, where every local
    // is hoisted to the top and renamed apart; the shadowing rename has to
    // compose with that one.
    c99! {
        int limit;

        int count_to(int limit) {
            int i = 0;
        again:
            i++;
            if (i < limit) goto again;
            return i;
        }
    }

    unsafe {
        limit = 5;
        assert_eq!(count_to(3), 3);
        assert_eq!({ limit }, 5);
    }
}

// ---------------------------------------------------------------------------
// the GCC predefined macros, and phase 2
// ---------------------------------------------------------------------------

#[test]
fn the_gcc_limit_and_type_macros_have_the_right_values() {
    // `__INT_MAX__`, `__SIZE_TYPE__` and the rest of that family are what a
    // program written for an unknown compiler tests before it has included
    // anything, and a program that finds one undefined does not fail to
    // compile — it silently takes the wrong branch. So the values are checked
    // against `<limits.h>` and against `sizeof`, which is the only way to
    // find out that one of them is *wrong* rather than merely present.
    c99! {
        #include <limits.h>
        #include <stddef.h>
        #include <stdint.h>

        int int_max_agrees(void) { return __INT_MAX__ == INT_MAX; }
        int long_max_agrees(void) { return __LONG_MAX__ == LONG_MAX; }
        int schar_max_agrees(void) { return __SCHAR_MAX__ == SCHAR_MAX; }
        int shrt_max_agrees(void) { return __SHRT_MAX__ == SHRT_MAX; }

        /* The type macros are spelled out as types, so a `sizeof` of one is
         * the check that it names the same type the header does. */
        int size_type_agrees(void) {
            return sizeof(__SIZE_TYPE__) == sizeof(size_t)
                && (__SIZE_TYPE__)-1 > 0;
        }
        int ptrdiff_type_agrees(void) {
            return sizeof(__PTRDIFF_TYPE__) == sizeof(ptrdiff_t)
                && (__PTRDIFF_TYPE__)-1 < 0;
        }
        int intptr_type_agrees(void) {
            return sizeof(__INTPTR_TYPE__) == sizeof(void *);
        }
        int exact_widths_agree(void) {
            return sizeof(__INT8_TYPE__) == 1
                && sizeof(__INT16_TYPE__) == 2
                && sizeof(__INT32_TYPE__) == 4
                && sizeof(__INT64_TYPE__) == 8
                && sizeof(__UINT32_TYPE__) == 4
                && (__UINT32_TYPE__)-1 == __UINT32_MAX__;
        }
        int widths_agree(void) {
            return __INT_WIDTH__ == sizeof(int) * __CHAR_BIT__
                && __LONG_WIDTH__ == sizeof(long) * __CHAR_BIT__
                && __SIZE_WIDTH__ == sizeof(size_t) * __CHAR_BIT__;
        }
        int intmax_agrees(void) {
            return sizeof(__INTMAX_TYPE__) == sizeof(intmax_t)
                && __INTMAX_MAX__ == INTMAX_MAX;
        }
        double largest_double(void) { return __DBL_MAX__; }
    }

    unsafe {
        assert_eq!(int_max_agrees(), 1);
        assert_eq!(long_max_agrees(), 1);
        assert_eq!(schar_max_agrees(), 1);
        assert_eq!(shrt_max_agrees(), 1);
        assert_eq!(size_type_agrees(), 1);
        assert_eq!(ptrdiff_type_agrees(), 1);
        assert_eq!(intptr_type_agrees(), 1);
        assert_eq!(exact_widths_agree(), 1);
        assert_eq!(widths_agree(), 1);
        assert_eq!(intmax_agrees(), 1);
        assert_eq!(largest_double(), f64::MAX);
    }
}

/// GCC and Clang do not predefine the *same* `__…_WIDTH__` macros, so both
/// spellings are here.
///
/// GCC has `__LONG_LONG_WIDTH__` and `__SCHAR_WIDTH__`; Clang has
/// `__LLONG_WIDTH__`, `__BOOL_WIDTH__`, `__POINTER_WIDTH__`,
/// `__UINTMAX_WIDTH__` and `__UINTPTR_WIDTH__`. Code in the wild tests
/// whichever its author's compiler had, and a program that finds one
/// undefined does not fail to compile — `#if __LLONG_WIDTH__ > __LONG_WIDTH__`
/// silently takes the wrong branch, which is what makes Clang's own
/// `drs/dr2xx.c` `#error` out. So the union is defined, and the two spellings
/// of one width are one value.
#[test]
fn both_compilers_spellings_of_the_width_macros_are_defined() {
    c99! {
        #include <limits.h>
        #include <stddef.h>

        #if __LLONG_WIDTH__ != __LONG_LONG_WIDTH__
        #error "the two spellings of the same width disagree"
        #endif

        int widths_agree(void) {
            return __BOOL_WIDTH__ == 1
                && __SCHAR_WIDTH__ == CHAR_BIT
                && __SHRT_WIDTH__ == sizeof(short) * CHAR_BIT
                && __INT_WIDTH__ == sizeof(int) * CHAR_BIT
                && __LONG_WIDTH__ == sizeof(long) * CHAR_BIT
                && __LLONG_WIDTH__ == sizeof(long long) * CHAR_BIT
                && __POINTER_WIDTH__ == sizeof(void *) * CHAR_BIT
                && __UINTPTR_WIDTH__ == sizeof(void *) * CHAR_BIT
                && __SIZE_WIDTH__ == sizeof(size_t) * CHAR_BIT
                && __UINTMAX_WIDTH__ == __INTMAX_WIDTH__
                && __INT_LEAST8_WIDTH__ == 8
                && __INT_LEAST16_WIDTH__ == 16
                && __INT_LEAST32_WIDTH__ == 32
                && __INT_LEAST64_WIDTH__ == 64
                && __INT_FAST8_WIDTH__ == 8
                && __INT_FAST64_WIDTH__ == 64;
        }
    }

    // `<stdint.h>` is included by the second block so that the first one shows
    // the macros are there before any header is.
    c99! {
        #include <stdint.h>
        #define BITS(t) (sizeof(t) * __CHAR_BIT__)

        /* The "fast" widths have to agree with the typedefs <stdint.h>
         * writes, or the macro and a `sizeof` would answer differently. */
        int fast_widths_match_the_typedefs(void) {
            return __INT_FAST16_WIDTH__ == BITS(int_fast16_t)
                && __INT_FAST32_WIDTH__ == BITS(int_fast32_t);
        }
    }

    unsafe {
        assert_eq!(widths_agree(), 1);
        assert_eq!(fast_widths_match_the_typedefs(), 1);
    }
}

#[test]
fn a_line_splice_may_sit_inside_a_token() {
    // Translation phase 2 deletes a backslash-newline *before* the source is
    // split into tokens, so one may appear in the middle of an identifier:
    // Clang's own `drs/dr464.c` writes `__LI\<newline>NE__`, and this is that
    // test. The C goes in as a string literal because a line continuation is
    // not something Rust's own lexer will hand over.
    c99! { r#"
        #line 10000
        int line_number(void) { return __LI\
NE__; }

        #define GRE\
ETING 42
        int greeting(void) { return GREETING; }

        int spliced_declaration(void) {
            int val\
ue = 7;
            return value;
        }
    "# }

    unsafe {
        // `#line 10000` makes the *next* line 10000, and the line of a
        // pp-token is the line its first character is on — which for a
        // spliced identifier is the line the splice starts on.
        assert_eq!(line_number(), 10000);
        assert_eq!(greeting(), 42);
        assert_eq!(spliced_declaration(), 7);
    }
}
