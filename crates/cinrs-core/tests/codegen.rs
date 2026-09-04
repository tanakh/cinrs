//! Snapshot tests of the generated Rust code.
//!
//! The expansion is what a user reads when they wonder what their C turned
//! into, so its shape is part of the crate's contract: these snapshots exist
//! to make a change in it visible rather than accidental.
//!
//! Bless them with `INSTA_UPDATE=always cargo test -p cinrs-core --test codegen`.

use std::str::FromStr;

use cinrs_core::{Options, Standard, expand};
use proc_macro2::TokenStream;

/// Expands `source` and pretty-prints the result.
///
/// The long `#[allow(…)]` every item carries is collapsed to a placeholder: it
/// is identical everywhere and covered by its own test, so leaving it in would
/// bury the code these snapshots exist to show. The short one on the glob
/// re-export is left alone, since it says something about that line.
fn generate(source: &str) -> String {
    generate_for(Standard::C99, source)
}

/// Expands `source` as the given standard's entry point would.
fn generate_for(standard: Standard, source: &str) -> String {
    let input = TokenStream::from_str(source).expect("the C must lex as Rust tokens");
    // Variadic definitions are generated whatever the toolchain: this test only
    // ever reads the text back, and the snapshots have to be the same
    // everywhere.
    let mut options = Options::new(standard);
    options.c_variadic = true;
    let output = expand(input, &options);
    let text = output.to_string();
    assert!(
        !text.contains("compile_error"),
        "expansion of\n{source}\nfailed:\n{text}"
    );
    let file: syn::File = match syn::parse2(output) {
        Ok(file) => file,
        Err(error) => panic!("the expansion must be valid Rust: {error}\n{text}"),
    };
    collapse_allow_attributes(&prettyplease::unparse(&file))
}

fn collapse_allow_attributes(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut depth = 0usize;
    for line in code.lines() {
        // The list is long enough that `prettyplease` always breaks it, so an
        // `#[allow(` alone on its line is it; a short list stays as written.
        if depth == 0 && line.trim_start() == "#[allow(" {
            let indent = &line[..line.len() - line.trim_start().len()];
            out.push_str(indent);
            out.push_str("#[allow(…)]\n");
            if !line.trim_end().ends_with(")]") {
                depth = 1;
            }
            continue;
        }
        if depth > 0 {
            if line.trim_end().ends_with(")]") {
                depth = 0;
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The lint exemptions are part of the contract: a naive translation of C
/// trips a lot of Rust lints, and a user must never see them.
#[test]
fn generated_items_carry_the_lint_exemptions() {
    let input = TokenStream::from_str("int f(void) { return 0; }").expect("valid tokens");
    let output = expand(input, &Options::new(Standard::C99));
    let file: syn::File = syn::parse2(output).expect("valid Rust");
    // The items live one module deep; the list itself is what is snapshotted,
    // so it is taken back out to the left margin.
    let attribute = prettyplease::unparse(&file)
        .lines()
        .skip_while(|line| !line.trim_start().starts_with("#[allow("))
        .take_while(|line| !line.trim_start().starts_with("pub "))
        .map(|line| line.strip_prefix("    ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(attribute);
}

#[test]
fn readme_example() {
    insta::assert_snapshot!(generate(
        r"
        int fact(int n) {
            if (n == 0) {
                return 1;
            } else {
                return n * fact(n - 1);
            }
        }
        "
    ));
}

#[test]
fn loops_and_labels() {
    insta::assert_snapshot!(generate(
        r"
        int sum(int n) {
            int total = 0;
            for (int i = 0; i < n; i++) {
                if (i == 3) continue;
                total += i;
            }
            while (n > 0) {
                n--;
                if (n == 1) break;
            }
            return total;
        }
        "
    ));
}

#[test]
fn do_while_with_continue() {
    insta::assert_snapshot!(generate(
        r"
        int count_odd(int n) {
            int seen = 0;
            do {
                n--;
                if (n % 2 == 0) continue;
                seen++;
            } while (n > 0);
            return seen;
        }
        "
    ));
}

#[test]
fn switch_with_fallthrough() {
    insta::assert_snapshot!(generate(
        r"
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
        "
    ));
}

#[test]
fn compound_assignment_and_inc_dec() {
    insta::assert_snapshot!(generate(
        r"
        int mix(int x) {
            unsigned char c = 200;
            c += 100;
            x <<= 2;
            x %= 7;
            int y = x++ + ++x;
            return y + c;
        }
        "
    ));
}

#[test]
fn promotions_and_conversions() {
    insta::assert_snapshot!(generate(
        r"
        int promote(void) {
            unsigned char a = 200;
            unsigned char b = 100;
            int wide = a + b;
            unsigned u = 1;
            int signed_vs_unsigned = -1 < u;
            double d = wide;
            float f = (float) d / 2;
            _Bool flag = f;
            return wide + signed_vs_unsigned + (int) f + flag;
        }
        "
    ));
}

#[test]
fn globals_statics_and_keywords() {
    insta::assert_snapshot!(generate(
        r"
        int counter = 0;
        static long private_total;

        static void bump(void) {
            static int calls = 1;
            calls++;
            counter += calls;
            private_total += counter;
        }

        int match(int type) {
            bump();
            return type + counter;
        }
        "
    ));
}

#[test]
fn pointers_and_arrays() {
    insta::assert_snapshot!(generate(
        r"
        int sum(const int *values, int n) {
            int total = 0;
            for (int i = 0; i < n; i++) {
                total += values[i];
            }
            return total;
        }

        void reverse(int values[], int n) {
            int *front = values;
            int *back = values + n - 1;
            while (front < back) {
                int t = *front;
                *front++ = *back;
                *back-- = t;
            }
        }

        long span(const char *from, const char *to) {
            return to - from;
        }

        int first(void) {
            int grid[2][3] = {{1, 2, 3}, {4, 5, 6}};
            int *p = &grid[1][2];
            return *p + grid[0][0] + (p == 0);
        }
        "
    ));
}

#[test]
fn structs_unions_and_enums() {
    insta::assert_snapshot!(generate(
        r"
        enum Kind { EMPTY, FULL = 4 };

        struct Point { int x; int y; };

        union Bits { float f; unsigned int u; };

        typedef struct Point Point;

        struct Shape {
            enum Kind kind;
            struct Point origin;
            union Bits tag;
        };

        static struct Shape current = {.kind = FULL, .origin = {1, 2}};

        int move_by(struct Shape *shape, int dx, int dy) {
            shape->origin.x += dx;
            shape->origin.y += dy;
            return shape->origin.x;
        }

        Point centre(struct Shape s) {
            return s.origin;
        }

        unsigned long shape_size(void) {
            return sizeof(struct Shape);
        }
        "
    ));
}

#[test]
fn strings_and_extern_declarations() {
    insta::assert_snapshot!(generate(
        r#"
        unsigned long strlen(const char *s);
        int printf(const char *fmt, ...);
        extern int errno;

        static const char *messages[2] = {"ok", "failed"};

        int report(int failed) {
            const char *message = messages[failed != 0];
            printf("%s (%lu)\n", message, strlen(message));
            return errno;
        }
        "#
    ));
}

#[test]
fn function_pointers() {
    insta::assert_snapshot!(generate(
        r"
        typedef int (*BinOp)(int, int);

        int add(int a, int b) { return a + b; }

        int apply(BinOp op, int a, int b) {
            if (op == 0) {
                return 0;
            }
            return op(a, b);
        }

        int use_add(int a, int b) {
            BinOp op = add;
            return apply(op, a, b);
        }
        "
    ));
}

#[test]
fn goto_becomes_a_state_machine() {
    insta::assert_snapshot!(generate(
        r"
        int find(const int *values, int n, int needle) {
            int i = 0;
        loop:
            if (i >= n) goto missing;
            if (values[i] == needle) goto found;
            i++;
            goto loop;
        found:
            return i;
        missing:
            return -1;
        }
        "
    ));
}

#[test]
fn hoisted_locals_are_renamed_apart() {
    insta::assert_snapshot!(generate(
        r"
        int shadowing(int n) {
            int total = 0;
            { int x = 1; total += x; }
            { int x = 20; { int x = 300; total += x; } total += x; }
            if (n) goto out;
            total = -1;
        out:
            return total;
        }
        "
    ));
}

#[test]
fn duffs_device() {
    insta::assert_snapshot!(generate(
        r"
        void copy(char *to, const char *from, int count) {
            int n = (count + 3) / 4;
            switch (count % 4) {
            case 0: do { *to++ = *from++;
            case 3:      *to++ = *from++;
            case 2:      *to++ = *from++;
            case 1:      *to++ = *from++;
                    } while (--n > 0);
            }
        }
        "
    ));
}

#[test]
fn variadic_definitions() {
    insta::assert_snapshot!(generate(
        r"
        #include <stdarg.h>

        int vsnprintf(char *buf, unsigned long size, const char *fmt, va_list ap);

        int sum(int n, ...) {
            va_list ap;
            int total = 0;
            va_start(ap, n);
            for (int i = 0; i < n; i++) {
                total += va_arg(ap, int);
            }
            va_end(ap);
            return total;
        }

        int format(char *buf, unsigned long size, const char *fmt, ...) {
            va_list ap;
            va_list copy;
            va_start(ap, fmt);
            va_copy(copy, ap);
            int written = vsnprintf(buf, size, fmt, copy);
            double first = va_arg(ap, double);
            va_end(ap);
            return written + (int) first;
        }
        "
    ));
}

#[test]
fn a_switch_that_always_returns_needs_no_trailing_return() {
    // Every group returns and there is a `default`, so control cannot fall out
    // of the statement and the synthesised `return 0;` is not needed.
    insta::assert_snapshot!(generate(
        r"
        int sign(int n) {
            switch (n) {
            case 0:
                return 0;
            case 1:
            case 2:
                return 1;
            default:
                return -1;
            }
        }
        "
    ));
}

#[test]
fn a_macro_heavy_program() {
    // The expansion is what a reader compares against the C they wrote, and
    // after the preprocessor has been through it there is not much left to
    // compare *to* — so the snapshot is the record of what the macros became.
    insta::assert_snapshot!(generate(
        r"
        #define WIDTH 4
        #define AREA(h) (WIDTH * (h))
        #define MAX(a, b) ((a) > (b) ? (a) : (b))
        #define ACCESSOR(name, expr) int name(void) { return expr; }

        #if WIDTH >= 4
        #define WIDE 1
        #else
        #define WIDE 0
        #endif

        ACCESSOR(width, WIDTH)
        ACCESSOR(area_of_three, AREA(3))
        ACCESSOR(is_wide, WIDE)

        int clamp(int v) {
            return MAX(v, WIDTH);
        }

        int table[WIDTH];
        "
    ));
}

#[test]
fn a_name_that_would_shadow_a_file_scope_item_is_renamed() {
    // Rust refuses a binding that shadows a `static` or a `const`, so a C
    // program that names a parameter or a local after one of its own globals
    // or enumerators needs the binding renamed apart.
    insta::assert_snapshot!(generate(
        r"
        int counter;
        enum Level { Low, High };

        int bump(int counter) {
            int Low = counter + 1;
            return Low;
        }

        int step(int n) {
            switch (n) {
                int counter;
            case 0:
                counter = 1;
                return counter;
            }
            return 0;
        }
        "
    ));
}

#[test]
fn offsetof_asks_rust_for_the_layout() {
    insta::assert_snapshot!(generate(
        r"
        #include <stddef.h>

        struct Mixed { char tag; int count; double weight; };

        size_t weight_offset(void) {
            return offsetof(struct Mixed, weight);
        }
        "
    ));
}

#[test]
fn a_named_module_holds_the_unit() {
    // `#pragma cinrs module` replaces the generated name with one the user can
    // write, which is how an ambiguous glob re-export is disambiguated.
    insta::assert_snapshot!(generate(
        r#"
        #pragma cinrs module "geometry"

        struct Point { int x; int y; };

        int manhattan(struct Point p) {
            return (p.x < 0 ? -p.x : p.x) + (p.y < 0 ? -p.y : p.y);
        }
        "#
    ));
}

#[test]
fn an_exported_unit_defines_real_c_symbols() {
    // Everything with external linkage gets the C name as its symbol, so that
    // another unit — or a C library — can link against it. A `static` keeps
    // its internal linkage, and a name that is a Rust keyword says its symbol
    // outright rather than exporting `r#match`.
    insta::assert_snapshot!(generate(
        r#"
        #pragma cinrs export

        int counter;
        static int hidden;

        inline int helper(int n) { return n + 1; }
        static int secret(int n) { return n - 1; }
        int match(int n) { return helper(n); }
        "#
    ));
}

// ---------------------------------------------------------------------------
// C11 and C23
// ---------------------------------------------------------------------------

#[test]
fn generic_selection_emits_only_the_chosen_arm() {
    // The controlling expression is never evaluated, and the associations that
    // were not chosen are not generated at all — only parsed.
    insta::assert_snapshot!(generate_for(
        Standard::C11,
        r#"
        int as_int(double d) { return (int)d; }

        int describe(int i, double d, char *s) {
            return _Generic(i, int: 1, double: 2, char *: 3, default: 0)
                 + _Generic(d, int: 1, double: as_int(d), default: 0)
                 + _Generic(s, char *: (int)*s, default: 0);
        }

        struct Pair { int a; double b; };
        double d;

        unsigned long alignments(void) {
            return _Alignof(double) + _Alignof(struct Pair) + _Alignof(d);
        }
        "#
    ));
}

#[test]
fn anonymous_members_become_a_synthetic_field() {
    // The anonymous member is a field of a generated type of its own, and
    // every access through it — including `offsetof` — is a path.
    insta::assert_snapshot!(generate_for(
        Standard::C11,
        r#"
        struct Value {
            int tag;
            union {
                int as_int;
                double as_double;
            };
        };

        int read(struct Value *v) { return v->as_int; }
        void write(struct Value *v, double d) { v->as_double = d; }
        struct Value make(void) {
            struct Value v = { .tag = 2, .as_double = 1.5 };
            return v;
        }
        unsigned long where(void) { return __builtin_offsetof(struct Value, as_double); }
        "#
    ));
}

#[test]
fn alignas_raises_the_alignment_of_the_record() {
    insta::assert_snapshot!(generate_for(
        Standard::C11,
        r#"
        struct Aligned { _Alignas(16) int first; char rest; };
        unsigned long size(void) { return sizeof(struct Aligned); }
        "#
    ));
}

#[test]
fn typeof_and_constexpr_are_resolved_before_code_generation() {
    // `typeof` is a type and `constexpr` is a value: neither leaves anything
    // of its own in the expansion.
    insta::assert_snapshot!(generate_for(
        Standard::C23,
        r#"
        constexpr int LIMIT = 4;

        int sum(void) {
            int values[LIMIT] = { 1, 2, 3, 4 };
            typeof(values[0]) total = 0;
            for (auto i = 0; i < LIMIT; i++) total += values[i];
            return total;
        }

        enum Small : unsigned char { RED, GREEN };
        bool is_red(enum Small s) { return s == RED; }
        void *nothing(void) { return nullptr; }
        "#
    ));
}

#[test]
fn a_noreturn_call_ends_the_function() {
    // The call has the function's declared return type, so Rust has to be
    // told that control does not come back from it.
    insta::assert_snapshot!(generate_for(
        Standard::C11,
        r#"
        _Noreturn void die(const char *why);

        int checked(int n) {
            if (n > 0) return n;
            die("not positive");
        }
        "#
    ));
}
