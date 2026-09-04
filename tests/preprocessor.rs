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
