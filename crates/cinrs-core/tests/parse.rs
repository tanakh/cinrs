//! Parser snapshot tests over a cross-section of the C99 grammar.
//!
//! Each case goes through the real pipeline (`analyze`) using string-literal
//! capture, so the snapshots also cover the string-literal input mode.

use std::str::FromStr;

use cinrs_core::diag::Level;
use cinrs_core::dump::dump_translation_unit;
use cinrs_core::{Options, Standard, analyze};
use proc_macro2::TokenStream;

/// Runs the front end over `src` and returns the AST dump, asserting that no
/// error was reported.
fn parse_dump(src: &str) -> String {
    parse_dump_as(Standard::C99, src)
}

/// The same, for a block written in a later revision.
fn parse_dump_as(standard: Standard, src: &str) -> String {
    let literal = format!("r####\"{src}\"####");
    let stream = TokenStream::from_str(&literal).expect("the wrapper literal must lex");
    let analysis = analyze(stream, &Options::new(standard));
    let errors: Vec<&str> = analysis
        .diagnostics
        .items()
        .iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:#?}");
    dump_translation_unit(&analysis.unit)
}

/// Runs the front end over `src` and returns `line:column: message` for every
/// diagnostic, in order.
fn parse_diagnostics(src: &str) -> String {
    let literal = format!("r####\"{src}\"####");
    let stream = TokenStream::from_str(&literal).expect("the wrapper literal must lex");
    let analysis = analyze(stream, &Options::new(Standard::C99));
    let map = &analysis.source.map;
    analysis
        .diagnostics
        .sorted()
        .iter()
        .map(|d| {
            let (line, column) = map.line_col(d.range.start);
            let level = match d.level {
                Level::Error => "error",
                Level::Warning => "warning",
            };
            format!("{line}:{column}: {level}: {}", d.message)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn readme_fact_example() {
    insta::assert_snapshot!(parse_dump(
        r#"
int fact(int n) {
    if (n == 0) {
        return 1;
    } else {
        return n * fact(n - 1);
    }
}
"#
    ));
}

#[test]
fn declarators_of_every_shape() {
    insta::assert_snapshot!(parse_dump(
        r#"
int (*fp[3])(int, ...);
const char *const *p;
int *f(void);
char (*g(int))[10];
int matrix[2][3];
void (*signal(int, void (*)(int)))(int);
int arr[];
void vla(int n, int a[static const 4], int b[*]);
"#
    ));
}

#[test]
fn struct_union_enum_with_bit_fields() {
    // `enum Color : 4;` is an unnamed bit-field of the enumeration's type and
    // not C23's fixed underlying type: the `:` only introduces one of those
    // when a *type* follows it.
    insta::assert_snapshot!(parse_dump(
        r#"
struct Flags {
    unsigned a : 1;
    unsigned : 3;
    signed b : 5, c : 2;
    struct Flags *next;
};
union U { int i; double d; char bytes[8]; };
enum Color { Red, Green = 7, Blue, };
enum Color pick(union U u);
struct Tagged { enum Color kind : 4; enum Color : 2; long long wide : 40; };
"#
    ));
}

#[test]
fn typedef_then_usage() {
    // `T * x;` is a declaration because `T` is a typedef name, and `(T)-1` is
    // a cast for the same reason. `U` is not, so `U * y` is a multiplication.
    insta::assert_snapshot!(parse_dump(
        r#"
typedef int T;
typedef struct Node { int v; struct Node *next; } Node;
T * x;
int y = (T)-1;
Node n;
int U;
void f(void) { U * y; }
void shadow(void) { int T; T * 2; }
"#
    ));
}

#[test]
fn designated_initializers() {
    insta::assert_snapshot!(parse_dump(
        r#"
int a[6] = { [4] = 29, [2] = 15 };
struct Point { int x, y; };
struct Point pts[2] = { [0].x = 1, [0].y = 2, { .x = 3, .y = 4 } };
int nested[2][2] = { { 1, 2 }, { 3, 4 } };
char msg[] = "hi";
"#
    ));
}

#[test]
fn control_flow() {
    insta::assert_snapshot!(parse_dump(
        r#"
int classify(int n) {
    int total = 0;
    for (int i = 0; i < n; i++) {
        switch (i % 3) {
        case 0:
            total += 1;
            break;
        case 1:
            continue;
        default:
            goto done;
        }
    }
    do { total--; } while (total > 0);
    while (total < 0) total++;
    if (total) total = 1; else total = 2;
done:
    return total;
}
"#
    ));
}

#[test]
fn operator_precedence() {
    insta::assert_snapshot!(parse_dump(
        r#"
int prec(int a, int b, int c, int d, int e) {
    a = b ? c : d, e;
    a = b = c;
    a += b * c - d / e % b;
    a = b << c | d & e ^ b;
    a = b < c == d > e;
    a = !b && c || ~d;
    a = -b + +c;
    a = *&b;
    return a;
}
"#
    ));
}

#[test]
fn postfix_and_member_access() {
    insta::assert_snapshot!(parse_dump(
        r#"
struct S { int x; struct S *next; };
int use(struct S *p, int (*fn)(int), int a[]) {
    p->next->x++;
    --p->x;
    a[1] = fn(a[0]);
    return (*p).x;
}
"#
    ));
}

#[test]
fn sizeof_forms() {
    insta::assert_snapshot!(parse_dump(
        r#"
typedef int T;
struct S { int a; };
int sizes(int v) {
    return sizeof v
         + sizeof(int)
         + sizeof(T)
         + sizeof(struct S)
         + sizeof(int *)
         + sizeof(int (*)(void))
         + sizeof(int[4])
         + sizeof (T){1};
}
"#
    ));
}

#[test]
fn compound_literals_and_casts() {
    insta::assert_snapshot!(parse_dump(
        r#"
struct P { int x, y; };
typedef struct P P;
int f(void) {
    struct P q = (struct P){ .x = 1, .y = 2 };
    int *v = (int[]){ 1, 2, 3 };
    long l = (long)(P){ 0, 0 }.x;
    return (int)(char)l + v[0] + q.x;
}
"#
    ));
}

#[test]
fn old_style_function_definition() {
    insta::assert_snapshot!(parse_dump(
        r#"
int add(a, b)
    int a;
    int b;
{
    return a + b;
}
static inline int twice(int x) { return x + x; }
extern int global;
"#
    ));
}

#[test]
fn string_literal_concatenation() {
    insta::assert_snapshot!(parse_dump(
        r#"
const char *msg = "hello, " "world" "\n";
const char *wide = "a" L"b";
"#
    ));
}

#[test]
fn several_errors_are_reported_with_recovery() {
    // Each broken declaration produces one error; parsing resumes at the next
    // `;` or `}`, so the later ones are still reported.
    insta::assert_snapshot!(parse_diagnostics(
        r#"
int bad1(void) { return 1 }
int ok(void) { return 0; }
int bad2 = ;
int x = 08;
int bad3(void) { int y = ; return y; }
"#
    ));
}

#[test]
fn the_preprocessor_runs_before_the_parser() {
    // What reaches the parser is what the preprocessor left: no directives,
    // and `SIZE` already replaced.
    insta::assert_snapshot!(parse_dump(
        r#"
#define SIZE 4
#define ARRAY(name) int name[SIZE]
ARRAY(a);
#if SIZE > 2
int big;
#else
int small;
#endif
"#
    ));
}

#[test]
fn a_missing_header_is_reported_at_the_directive() {
    insta::assert_snapshot!(parse_diagnostics(
        r#"
#include <nowhere.h>
int x;
"#
    ));
}

// ---------------------------------------------------------------------------
// __builtin_offsetof
// ---------------------------------------------------------------------------

#[test]
fn offsetof_is_a_special_form() {
    // The second operand is a member name rather than an expression, which is
    // why the parser has to know about it at all.
    let dump = parse_dump(
        "struct S { int a; };
         unsigned long f(void) { return __builtin_offsetof(struct S, a); }",
    );
    assert!(dump.contains("offsetof struct S .a"), "{dump}");
}

#[test]
fn a_nested_member_designator_parses() {
    // C99 7.17p3's member designator: an identifier and then any number of
    // `.member` and `[expr]` steps.
    let dump = parse_dump(
        "struct T { int b[4]; }; struct S { struct T a; };
         unsigned long f(void) { return __builtin_offsetof(struct S, a.b[2]); }",
    );
    assert!(dump.contains("offsetof struct S .a .b [2]"), "{dump}");
}

// ---------------------------------------------------------------------------
// C11 and C23 syntax
// ---------------------------------------------------------------------------

#[test]
fn the_later_revisions_parse_into_the_same_tree() {
    insta::assert_snapshot!(parse_dump_as(
        Standard::C23,
        r#"
        _Static_assert(1, "at file scope");

        struct S {
            alignas(8) int aligned;
            struct { int x; int y; };
            static_assert(1);
        };

        enum Level : unsigned char { LOW, HIGH };

        [[nodiscard]] int pick([[maybe_unused]] int n) {
            static_assert(1, "in a block");
            constexpr int limit = 4;
            typeof(n) copy = n;
            auto inferred = 1.5;
            bool flag = true;
            void *p = nullptr;
            struct S empty = {};
            return _Generic(n, int: alignof(int), default: sizeof(n)) + limit + copy;
        }
        "#
    ));
}
