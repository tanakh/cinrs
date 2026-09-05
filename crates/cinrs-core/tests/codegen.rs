//! Snapshot tests of the generated Rust code.
//!
//! The expansion is what a user reads when they wonder what their C turned
//! into, so its shape is part of the crate's contract: these snapshots exist
//! to make a change in it visible rather than accidental.
//!
//! Bless them with `INSTA_UPDATE=always cargo test -p cinrs-core --test codegen`.

use std::str::FromStr;

use cinrs_core::{Options, Standard, expand};
use proc_macro2::{TokenStream, TokenTree};

/// Expands `source` and pretty-prints the result.
///
/// The long `#[allow(…)]` every item carries is collapsed to a placeholder: it
/// is identical everywhere and covered by its own test, so leaving it in would
/// bury the code these snapshots exist to show. The short one on the glob
/// re-export is left alone, since it says something about that line. The
/// [data-model check](the_data_model_is_asserted) every unit opens with is
/// collapsed for the same reason.
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
    // Whether the C checked out is asked of the front end rather than of the
    // expansion's text: the expansion may hold a `compile_error!` of its own —
    // the one a `constructor` puts behind a `cfg` for a target with no
    // initialiser table — which says nothing about this source.
    let analysis = cinrs_core::analyze(input.clone(), &options);
    let failures = |diags: &cinrs_core::Diagnostics| {
        diags
            .sorted()
            .into_iter()
            .filter(|d| d.level == cinrs_core::Level::Error)
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
    };
    let mut errors = failures(&analysis.diagnostics);
    let (_program, sema_diagnostics) =
        cinrs_core::sema::analyze(&analysis.unit, &options, analysis.source.unit_id());
    errors.extend(failures(&sema_diagnostics));
    assert!(
        errors.is_empty(),
        "expansion of\n{source}\nfailed: {errors:#?}"
    );
    let output = expand(input, &options);
    let text = output.to_string();
    let file: syn::File = match syn::parse2(output) {
        Ok(file) => file,
        Err(error) => panic!("the expansion must be valid Rust: {error}\n{text}"),
    };
    collapse_data_model_check(&collapse_allow_attributes(&prettyplease::unparse(&file)))
}

/// Replaces the unit's data-model assertions with a one-line placeholder.
///
/// Every expansion opens with the same block — half a dozen `assert!`s over
/// `core::ffi` sizes — and it has a test of its own; spelling it out in
/// thirty-six snapshots would bury what each of them is about.
fn collapse_data_model_check(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut inside = false;
    for line in code.lines() {
        if !inside && line.trim_start() == "const _: () = {" {
            let indent = &line[..line.len() - line.trim_start().len()];
            out.push_str(indent);
            out.push_str("const _: () = { /* the data-model check */ };\n");
            inside = true;
            continue;
        }
        if inside {
            inside = line.trim_start() != "};";
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Replaces the long lint exemption every generated item carries with a
/// placeholder.
///
/// It is identical everywhere and has a test of its own, so leaving it in
/// would bury the code these snapshots exist to show. Matching is on the
/// *text* rather than on whole lines because `prettyplease` wraps the list
/// differently inside a macro invocation than in front of an item; the short
/// `#[allow(…)]` on the glob re-export says something about that line and is
/// recognised by not mentioning `clippy::all`.
fn collapse_allow_attributes(code: &str) -> String {
    const OPEN: &str = "#[allow(";
    const CLOSE: &str = ")]";
    let mut out = String::with_capacity(code.len());
    let mut rest = code;
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        let Some(end) = after.find(CLOSE) else { break };
        if !after[..end].contains("clippy::all") {
            out.push_str(&rest[..start + OPEN.len() + end + CLOSE.len()]);
            rest = &after[end + CLOSE.len()..];
            continue;
        }
        out.push_str(&rest[..start]);
        out.push_str("#[allow(…)]");
        rest = &after[end + CLOSE.len()..];
    }
    out.push_str(rest);
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

/// A call through a function type with no prototype is a call through a
/// signature the *arguments* make, so the callee is transmuted to it at each
/// call site. The zero-argument calls are left alone, since there is nothing to
/// reinterpret.
#[test]
fn a_call_without_a_prototype_casts_at_the_call_site() {
    insta::assert_snapshot!(generate(
        r"
        int taker();
        int made_here() { return 1; }

        int call_them(void) {
            char c = 3;
            int (*fp)() = made_here;
            return taker() + taker(c) + made_here() + fp(1.5f, c);
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
fn offsetof_is_folded_to_a_constant() {
    // Sema knows the layout — `ir::Field::offset` is where it put every member
    // — so `offsetof` is an integer constant in the expansion rather than a
    // `core::mem::offset_of!`, which is what C99 6.6 needs it to be. The two
    // agree, and `tests/aggregates.rs` is where that is checked.
    insta::assert_snapshot!(generate(
        r"
        #include <stddef.h>

        struct Mixed { char tag; int count; double weight; };
        struct Nested { int head; struct Mixed rows[4]; };

        size_t weight_offset(void) {
            return offsetof(struct Mixed, weight);
        }

        char probe[offsetof(struct Nested, rows[2].count)];
        "
    ));
}

#[test]
fn bit_fields_become_storage_bytes_and_accessors() {
    // A bit-field has no address, so it is not a Rust field: the run shares
    // one `[u8; K]`, a pair of inherent methods reads and writes it, the
    // `: 0` and the alignment of the fields' own type decide where `tag`
    // lands and how strict the item is, and a constant initialiser is folded
    // into the bytes so that a `static` can hold one.
    insta::assert_snapshot!(generate(
        r"
        struct Flags {
            unsigned int ready : 1;
            int          level : 3;
            unsigned int       : 0;
            unsigned int mask  : 30;
            char         tag;
        };

        static struct Flags defaults = { 1, -2, .mask = 5, .tag = 'x' };

        void arm(struct Flags *f, int level) {
            f->ready = 1;
            f->level = level;
            f->mask += 2;
        }

        int level_of(struct Flags f) { return f.level; }
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

#[test]
fn compound_literals_become_a_hidden_object() {
    // The object has to outlive the expression — C gives it the lifetime of
    // the enclosing block — so it cannot be a temporary inside one. It is a
    // binding at the head of the block, and the value is stored into it
    // *where the literal was written*, which is what keeps C's evaluation
    // order and rebuilds the object on every pass through a loop. A literal at
    // file scope has static storage duration and becomes an item instead.
    insta::assert_snapshot!(generate(
        r"
        struct S { int a; int b; };

        struct S *global = &(struct S){ 1, 2 };

        int local(int n) {
            struct S *p = &(struct S){ n, n + 1 };
            int total = 0;
            for (int i = 0; i < n; i++) {
                total += (int[]){ i, i * 2 }[1];
            }
            return p->a + p->b + total;
        }
        "
    ));
}

#[test]
fn a_definition_after_an_extern_declaration_leaves_the_extern_block() {
    // C99 6.9.2: the later declaration *defines* the object here, so it has to
    // stop being an entry in the `extern` block or the link fails.
    let defined = generate("extern int x; int x; int f(void) { return x; }");
    assert!(
        defined.contains("pub static mut x: ::core::ffi::c_int = 0;"),
        "{defined}"
    );
    assert!(!defined.contains("link_name"), "{defined}");

    let initialized = generate("extern int x; int x = 3; int f(void) { return x; }");
    assert!(
        initialized.contains("pub static mut x: ::core::ffi::c_int = 3;"),
        "{initialized}"
    );
    assert!(!initialized.contains("link_name"), "{initialized}");

    // An `extern` the unit never defines is still linked from elsewhere.
    let declared = generate("extern int x; int f(void) { return x; }");
    assert!(declared.contains("#[link_name = \"x\"]"), "{declared}");
    assert!(!declared.contains("static mut x"), "{declared}");
}

/// Every bare integer literal in `tokens` that is the receiver of a method
/// call, which Rust cannot give a type to (`E0689`).
///
/// Working on the tokens rather than on the pretty-printed text is what makes
/// this independent of how the output happens to be formatted.
fn literal_method_receivers(tokens: TokenStream) -> Vec<String> {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut found = Vec::new();
    for (index, tree) in trees.iter().enumerate() {
        if let TokenTree::Group(group) = tree {
            found.extend(literal_method_receivers(group.stream()));
            continue;
        }
        let TokenTree::Literal(literal) = tree else {
            continue;
        };
        // A suffixed literal (`4u32`) says what it is, and so does anything
        // with a `.` or an `x` in it; only a plain run of digits is open.
        let text = literal.to_string();
        if !text.chars().all(|c| c.is_ascii_digit() || c == '_') {
            continue;
        }
        let dot = matches!(trees.get(index + 1), Some(TokenTree::Punct(p)) if p.as_char() == '.');
        let method = matches!(trees.get(index + 2), Some(TokenTree::Ident(_)));
        if dot && method {
            found.push(text);
        }
    }
    found
}

#[test]
fn a_bare_literal_is_never_the_receiver_of_a_method() {
    // `{integer}.wrapping_mul(…)` is `E0689`: Rust refuses to guess. The
    // shapes below are the ones that get close to it — a folded constant, and
    // a conditional whose arms are both bare literals, which has no more of a
    // type than a literal does. (c-testsuite 00200.)
    for source in [
        "int f(int n) { return ((n) < 0 || -(n) < 0 ? -1 : 1) * (int) sizeof(n + 0); }",
        "long f(int n) { return (n ? 1 : 2) * 3L; }",
        "int f(int n) { return (n ? 1 : 2) << 3; }",
        "int f(int n) { return -(n ? 1 : 2); }",
        "int f(int n) { return 2 * 3 + n; }",
        "unsigned long f(void) { return sizeof(int) * 4; }",
    ] {
        let input = TokenStream::from_str(source).expect("the C must lex as Rust tokens");
        let output = expand(input, &Options::new(Standard::C99));
        let found = literal_method_receivers(output);
        assert!(found.is_empty(), "in\n{source}\nfound receivers {found:?}");
    }
}

// ---------------------------------------------------------------------------
// the GNU extensions
// ---------------------------------------------------------------------------

#[test]
fn a_statement_expression_becomes_a_rust_block() {
    // GNU's `({ …; e; })` is what a Rust block expression already is, so the
    // translation is direct — declarations and all.
    insta::assert_snapshot!(generate(
        r#"
        int max(int a, int b) {
            return ({ __typeof__(a) _a = (a); __typeof__(b) _b = (b); _a > _b ? _a : _b; });
        }

        int side_effects(int n) {
            return ({ int total = 0; for (int i = 0; i < n; i++) total += i; total; });
        }

        int elvis(int a, int b) { return a ?: b; }
        "#
    ));
}

#[test]
fn the_function_attributes_become_rust_ones() {
    insta::assert_snapshot!(generate(
        r#"
        __attribute__((always_inline)) int fast(int n) { return n + 1; }
        __attribute__((noinline, cold)) int slow(int n) { return n + 2; }
        __attribute__((deprecated("use fast"))) int old(int n) { return n; }
        __attribute__((section(".text.hot"))) int placed(void) { return 1; }
        int renamed(int n) __asm__("other_symbol");
        int call_renamed(int n) { return renamed(n); }
        "#
    ));
}

#[test]
fn a_packed_record_is_a_packed_rust_item() {
    // The Rust item has to have the layout C computed, which `packed(N)` says
    // directly and explicit padding fills in where a member was moved along.
    insta::assert_snapshot!(generate(
        r#"
        struct __attribute__((packed)) Header {
            unsigned char kind;
            unsigned int length;
            unsigned short flags : 12;
        };

        #pragma pack(2)
        struct Halved { char c; int x; short s; };
        #pragma pack()

        struct Moved { char c; __attribute__((aligned(16))) int x; };

        unsigned int length_of(struct Header *h) { return h->length; }
        "#
    ));
}

#[test]
fn a_constructor_becomes_an_init_array_entry() {
    insta::assert_snapshot!(generate(
        r#"
        static int started;
        __attribute__((constructor)) static void begin(void) { started = 1; }
        __attribute__((destructor)) static void end(void) { started = 0; }
        int has_started(void) { return started; }
        "#
    ));
}

#[test]
fn case_ranges_become_rust_range_patterns() {
    insta::assert_snapshot!(generate(
        r#"
        int classify(int c) {
            switch (c) {
            case '0' ... '9': return 1;
            case 'a' ... 'z':
            case 'A' ... 'Z': return 2;
            case '_': return 3;
            default: return 0;
            }
        }
        "#
    ));
}

#[test]
fn a_flexible_array_member_is_a_zero_length_tail() {
    insta::assert_snapshot!(generate(
        r#"
        struct Buffer { int len; int data[]; };

        int sum(struct Buffer *b) {
            int total = 0;
            for (int i = 0; i < b->len; i++) total += b->data[i];
            return total;
        }

        unsigned long header_size(void) { return sizeof(struct Buffer); }
        "#
    ));
}

#[test]
fn a_variable_length_array_lives_in_a_hidden_vec() {
    // Three bindings: the number of elements, evaluated once where the
    // declaration stands; the `Vec` that holds them, whose `Drop` at the end of
    // the block is the object's lifetime; and the object itself, which is a
    // pointer to the first element. `sizeof a` reads the first of the three
    // back, so it is a run-time value — and a declaration inside a loop
    // allocates afresh on every pass.
    insta::assert_snapshot!(generate(
        r#"
        unsigned long sum(int n) {
            unsigned long total = 0;
            for (int i = 0; i < n; i++) {
                char buf[i + 1];
                buf[i] = (char)i;
                total += sizeof buf + (unsigned long)buf[i];
            }
            return total;
        }
        "#
    ));
}

#[test]
fn alloca_allocates_from_a_function_wide_arena() {
    // `alloca`'s memory belongs to the *function*, so the arena is opened at
    // the top and dropped by the `return`; every call pushes one 16-byte
    // aligned block onto it.
    insta::assert_snapshot!(generate(
        r#"
        void *first(unsigned long n) {
            char *a = __builtin_alloca(n);
            char *b = __builtin_alloca(2 * n);
            a[0] = b[0];
            return a;
        }
        "#
    ));
}

#[test]
fn the_no_std_pragma_moves_the_storage_to_the_alloc_crate() {
    // The `Vec` behind a variable length array and `alloca` is the only thing
    // the expansion needs beyond `core`. Nothing in the C says which kind of
    // crate it is going into, so the pragma does.
    let source = r#"
        long sum(int n) {
            int a[n];
            char *p = __builtin_alloca(n);
            a[0] = p[0];
            return a[0] + (long)sizeof a;
        }
        "#;
    let with_std = generate(source);
    assert!(with_std.contains("::std::vec::Vec"), "{with_std}");
    assert!(with_std.contains("::std::vec::from_elem"), "{with_std}");
    assert!(!with_std.contains("::alloc::"), "{with_std}");

    let no_std = generate(&format!("#pragma cinrs no_std\n{source}"));
    assert!(no_std.contains("::alloc::vec::Vec"), "{no_std}");
    assert!(no_std.contains("::alloc::vec::from_elem"), "{no_std}");
    assert!(!no_std.contains("::std::"), "{no_std}");

    // Nothing else the crate generates needs either of them, which is what
    // makes an ordinary expansion `core`-only — the C library it calls is a
    // link-time dependency of the program, not a Rust one.
    let ordinary = generate_for(
        Standard::C23,
        r#"
        #include <assert.h>
        #include <stddef.h>
        #include <stdarg.h>

        struct S { unsigned int flag : 1; int other; };

        static int counter = 3;
        int table[4] = { 1, 2, 3, 4 };

        __attribute__((constructor)) static void begin(void) { counter = 1; }

        int printf(const char *, ...);

        int f(struct S *s, int n) {
            int a[4];
            for (int i = 0; i < n; i++) a[i & 3] = i;
            assert(n >= 0);
            if (n == 7) __builtin_trap();
            if (n == 8) unreachable();
            const char *text = "core only";
            printf("%s %d\n", text, n);
            struct S *p = &(struct S){ 1, 0 };
            int biggest = ({ int x = n; int y = 2; x > y ? x : y; });
            if (n) goto out;
            s->flag = 1;
        out:
            return a[0] + (int)s->flag + (int)p->flag + biggest + counter
                 + table[0] + (int)offsetof(struct S, other)
                 + (int)text[0] + __builtin_popcount(n);
        }
        "#,
    );
    assert!(!ordinary.contains("::std::"), "{ordinary}");
    assert!(!ordinary.contains("::alloc::"), "{ordinary}");
}

// ---------------------------------------------------------------------------
// the data-model check
// ---------------------------------------------------------------------------

/// Expands `source` for `target` and returns just the data-model check.
fn data_model_check(target: cinrs_core::TargetModel, source: &str) -> String {
    let input = TokenStream::from_str(source).expect("the C must lex as Rust tokens");
    let mut options = Options::new(Standard::C99);
    options.target = target;
    let file: syn::File = syn::parse2(expand(input, &options)).expect("the expansion is Rust");
    let text = prettyplease::unparse(&file);
    let mut lines = Vec::new();
    for line in text
        .lines()
        .skip_while(|line| line.trim_start() != "const _: () = {")
    {
        lines.push(line.strip_prefix("    ").unwrap_or(line));
        if line.trim_start() == "};" {
            break;
        }
    }
    assert!(!lines.is_empty(), "no data-model check in\n{text}");
    lines.join("\n")
}

/// Every expansion states the data model it was translated for, so that
/// cross-compiling to a machine with a different one is a failed assertion
/// rather than a program that quietly computes the wrong thing.
#[test]
fn the_data_model_is_asserted() {
    insta::assert_snapshot!(data_model_check(
        cinrs_core::TargetModel::LP64,
        "int f(void) { return 0; }"
    ));
}

/// The numbers come from the model, not from the host: a unit expanded for a
/// different data model asserts *that* one, which is what makes the check a
/// cross-compilation guard rather than a tautology.
#[test]
fn a_different_data_model_is_asserted_differently() {
    let ilp32 = squeeze(&data_model_check(
        cinrs_core::TargetModel::ILP32,
        "int f(void) { return 0; }",
    ));
    // `long` and a pointer are four bytes in ILP32 and eight in LP64, so the
    // check an ILP32 expansion carries is one an LP64 target fails.
    assert!(
        ilp32.contains("size_of::<::core::ffi::c_long>()==4"),
        "{ilp32}"
    );
    assert!(
        ilp32.contains("size_of::<*const::core::ffi::c_void>()==4"),
        "{ilp32}"
    );
    // …while `long long` is eight either way, which is what makes the check a
    // statement of the model rather than of the pointer width.
    assert!(
        ilp32.contains("size_of::<::core::ffi::c_longlong>()==8"),
        "{ilp32}"
    );

    // An unsigned-`char` model asserts the other way round.
    let mut unsigned_char = cinrs_core::TargetModel::LP64;
    unsigned_char.char_signed = false;
    let text = data_model_check(unsigned_char, "int f(void) { return 0; }");
    assert!(text.contains("c_char::MIN == 0"), "{text}");
    assert!(text.contains("plain 'char' is unsigned"), "{text}");
}

/// `__int128`'s alignment is the one thing that depends on more than a width,
/// so it is asserted exactly where the unit has one.
#[test]
fn the_int128_alignment_is_asserted_only_where_it_is_used() {
    // Every unit asserts the alignment of `long long` and `double`, which is
    // where two ILP32 targets part company; only one that really has an
    // `__int128` asserts *its* alignment.
    let without = squeeze(&data_model_check(
        cinrs_core::TargetModel::LP64,
        "int f(void) { return 0; }",
    ));
    assert!(
        without.contains("align_of::<::core::ffi::c_longlong>()==8"),
        "{without}"
    );
    assert!(!without.contains("primitive::i128"), "{without}");

    let with = squeeze(&data_model_check(
        cinrs_core::TargetModel::LP64,
        "__int128 f(__int128 v) { return v + 1; }",
    ));
    assert!(
        with.contains("align_of::<::core::primitive::i128>()==16"),
        "{with}"
    );

    let mut narrow = cinrs_core::TargetModel::LP64;
    narrow.int128_align = 8;
    let text = squeeze(&data_model_check(
        narrow,
        "__int128 f(__int128 v) { return v + 1; }",
    ));
    assert!(
        text.contains("align_of::<::core::primitive::i128>()==8"),
        "{text}"
    );
}

/// Drops every space, so that an assertion about the generated code does not
/// also depend on where `prettyplease` puts one.
fn squeeze(code: &str) -> String {
    code.chars().filter(|c| !c.is_whitespace()).collect()
}

/// A unit that declares nothing expands to nothing at all — the check included,
/// since there is no generated code for the data model to be wrong about.
#[test]
fn an_empty_unit_carries_no_check() {
    let input = TokenStream::from_str("").expect("valid tokens");
    assert!(expand(input, &Options::new(Standard::C99)).is_empty());
}

// ---------------------------------------------------------------------------
// __int128
// ---------------------------------------------------------------------------

#[test]
fn int128_becomes_i128_and_u128() {
    insta::assert_snapshot!(generate(
        r"
        __int128 mul_high(long long a, long long b) {
            __int128 product = (__int128) a * (__int128) b;
            return product >> 64;
        }

        unsigned __int128 divide(unsigned __int128 v) {
            return v / 3;
        }

        struct Wide { int tag; unsigned __int128 value; };

        unsigned long widths(void) {
            return sizeof (struct Wide) + __alignof__(__int128) + sizeof (__int128_t);
        }

        __uint128_t from_typedef(__int128_t v) { return (__uint128_t) v; }
        "
    ));
}

// ---------------------------------------------------------------------------
// _Thread_local
// ---------------------------------------------------------------------------

#[test]
fn a_thread_local_object_becomes_a_thread_local_item() {
    insta::assert_snapshot!(generate_for(
        Standard::C11,
        r"
        _Thread_local int counter = 1;
        static _Thread_local long private_total;

        int bump(int by) {
            counter += by;
            return counter;
        }

        int *address(void) { return &counter; }

        int calls(void) {
            static _Thread_local int n = 0;
            return ++n;
        }
        "
    ));
}

/// An initialiser whose value is the address of another item cannot go inside
/// a `const` block — `E0013` — so the item takes `thread_local!`'s lazy form.
#[test]
fn a_thread_local_whose_initializer_names_an_item_is_not_const() {
    insta::assert_snapshot!(generate_for(
        Standard::C11,
        r"
        int anchor;
        _Thread_local int *cursor = &anchor;
        int read(void) { return *cursor; }
        "
    ));
}

// ---------------------------------------------------------------------------
// atomics
// ---------------------------------------------------------------------------

/// The `_Atomic` object model: the object is a plain one of the underlying
/// type, and every access goes through `AtomicX::from_ptr` over its address.
#[test]
fn an_atomic_object_is_reached_through_from_ptr() {
    insta::assert_snapshot!(generate_for(
        Standard::C11,
        r"
        _Atomic int counter;
        _Atomic(int *) cursor;

        int read(void) { return counter; }
        void write(int v) { counter = v; }
        int bump(void) { return ++counter; }
        int add(int v) { return counter += v; }
        int scale(int v) { return counter *= v; }
        int *step(void) { return cursor++; }
        "
    ));
}

/// The three builtin families, and what each becomes.
#[test]
fn the_atomic_builtins_become_core_sync_atomic() {
    insta::assert_snapshot!(generate_for(
        Standard::C11,
        r"
        int load(int *p) { return __atomic_load_n(p, __ATOMIC_ACQUIRE); }
        void store(int *p, int v) { __atomic_store_n(p, v, __ATOMIC_RELEASE); }
        int fetch_add(int *p, int v) { return __atomic_fetch_add(p, v, __ATOMIC_RELAXED); }
        int add_fetch(int *p, int v) { return __atomic_add_fetch(p, v, __ATOMIC_SEQ_CST); }
        int nand(int *p, int v) { return __atomic_fetch_nand(p, v, __ATOMIC_SEQ_CST); }
        int cas(int *p, int *expected, int desired) {
            return __atomic_compare_exchange_n(p, expected, desired, 1,
                                               __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE);
        }
        int test_and_set(char *p) { return __atomic_test_and_set(p, __ATOMIC_ACQUIRE); }
        void clear(char *p) { __atomic_clear(p, __ATOMIC_RELEASE); }
        void fences(void) {
            __atomic_thread_fence(__ATOMIC_SEQ_CST);
            __atomic_signal_fence(__ATOMIC_ACQUIRE);
        }
        double load_double(double *p) { return __atomic_load_n(p, __ATOMIC_SEQ_CST); }
        int *ptr_add(int **p) { return __atomic_fetch_add(p, 8, __ATOMIC_SEQ_CST); }
        int older(int *p, int v) { return __sync_fetch_and_add(p, v); }
        int older_cas(int *p, int old, int fresh) {
            return __sync_val_compare_and_swap(p, old, fresh);
        }
        void unlock(int *p) { __sync_lock_release(p); }
        "
    ));
}

#[test]
fn a_wide_bit_field_reads_through_a_u128_window() {
    insta::assert_snapshot!(generate(
        r"
        struct Packed { unsigned __int128 wide : 70; int tail : 3; };

        unsigned __int128 read_wide(struct Packed *p) { return p->wide; }
        void write_wide(struct Packed *p, unsigned __int128 v) { p->wide = v; }
        "
    ));
}
