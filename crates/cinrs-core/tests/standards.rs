//! What each `Standard` accepts, and what it says about the rest.
//!
//! The point of this file is the *gating*: a feature C11 or C23 added has to
//! be accepted in a block written for that revision and refused, with a
//! message that says which macro to use instead, in an older one. The
//! diagnostics of the features themselves — a failing assertion, a `_Generic`
//! with no match — are here too, because the wording is cheaper to cover
//! exhaustively than in the `.stderr` files under `tests/ui`.

use std::str::FromStr;

use cinrs_core::diag::Level;
use cinrs_core::{Options, Standard, analyze, sema};
use proc_macro2::TokenStream;

/// Analyses `source` under `standard` and returns every error message, in
/// source order — the front end's and sema's alike, since a feature may be
/// gated in either.
fn errors(standard: Standard, source: &str) -> Vec<String> {
    // String-literal mode accepts any C text, including what the Rust lexer
    // refuses (`1'000'000`, `##`, `L"…"`).
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let mut options = Options::new(standard);
    options.c_variadic = true;
    let analysis = analyze(input, &options);
    let mut messages: Vec<(cinrs_core::Pos, String)> = analysis
        .diagnostics
        .sorted()
        .into_iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| (d.range.start, d.message.clone()))
        .collect();
    let (_program, diagnostics) =
        sema::analyze(&analysis.unit, &options, analysis.source.unit_id());
    messages.extend(
        diagnostics
            .sorted()
            .into_iter()
            .filter(|d| d.level == Level::Error)
            .map(|d| (d.range.start, d.message.clone())),
    );
    messages.sort_by_key(|(pos, _)| *pos);
    messages.into_iter().map(|(_, message)| message).collect()
}

#[track_caller]
fn accepted(standard: Standard, source: &str) {
    let found = errors(standard, source);
    assert!(
        found.is_empty(),
        "{} rejected it: {found:#?}",
        standard.as_str()
    );
}

#[track_caller]
fn rejected(standard: Standard, source: &str, expected: &[&str]) {
    let found = errors(standard, source);
    assert_eq!(found, expected, "for:\n{source}");
}

/// A source that is accepted from `since` on and gated before it.
///
/// Only the *first* message is checked: a construct the block's own revision
/// does not have often leaves the parser somewhere it cannot make sense of
/// either, and what matters is that the first thing the user reads says which
/// macro to write instead.
#[track_caller]
fn since(since: Standard, source: &str, message: &str) {
    for standard in [Standard::C99, Standard::C11, Standard::C17, Standard::C23] {
        if standard >= since {
            accepted(standard, source);
            continue;
        }
        let found = errors(standard, source);
        assert_eq!(
            found.first().map(String::as_str),
            Some(standard.requires(message, since).as_str()),
            "for:\n{source}"
        );
    }
}

/// A source that is gated before `since`, without saying what happens after.
#[track_caller]
fn gated_before(since: Standard, source: &str, message: &str) {
    for standard in [Standard::C99, Standard::C11, Standard::C17, Standard::C23] {
        if standard >= since {
            continue;
        }
        let found = errors(standard, source);
        assert_eq!(
            found.first().map(String::as_str),
            Some(standard.requires(message, since).as_str()),
            "for:\n{source}"
        );
    }
}

// ---------------------------------------------------------------------------
// C11
// ---------------------------------------------------------------------------

#[test]
fn c11_features_are_gated() {
    since(
        Standard::C11,
        "_Static_assert(1, \"ok\");",
        "'_Static_assert'",
    );
    since(
        Standard::C11,
        "unsigned long f(void) { return _Alignof(int); }",
        "'_Alignof'",
    );
    since(
        Standard::C11,
        "int f(int x) { return _Generic(x, int: 1, default: 0); }",
        "'_Generic'",
    );
    since(
        Standard::C11,
        "_Noreturn void f(void) { while (1) { } }",
        "'_Noreturn'",
    );
    since(
        Standard::C11,
        "struct S { int a; } ; struct T { struct S; };",
        "an anonymous struct or union member",
    );
    since(
        Standard::C11,
        "struct S { _Alignas(16) int a; };",
        "'_Alignas'",
    );
}

#[test]
fn static_assertions_are_evaluated() {
    accepted(
        Standard::C11,
        "_Static_assert(sizeof(int) >= 2, \"int is at least 16 bits\");\n\
         struct S { int x; _Static_assert(1, \"inside a struct\"); };\n\
         int f(void) { _Static_assert(2 > 1, \"inside a block\"); return 0; }",
    );
    rejected(
        Standard::C11,
        "_Static_assert(1 == 2, \"one is not two\");",
        &["static assertion failed: \"one is not two\""],
    );
    rejected(
        Standard::C11,
        "int f(void) { _Static_assert(0, \"in a block\"); return 0; }",
        &["static assertion failed: \"in a block\""],
    );
    rejected(
        Standard::C11,
        "struct S { int x; _Static_assert(0, \"in a struct\"); };",
        &["static assertion failed: \"in a struct\""],
    );
    rejected(
        Standard::C11,
        "int n; _Static_assert(n, \"not constant\");",
        &[
            "the controlling expression of a static assertion is not a compile-time \
           constant expression",
        ],
    );
    rejected(
        Standard::C11,
        "_Static_assert(\"text\", \"not an integer\");",
        &[
            "the controlling expression of a static assertion must have an integer type, \
           not 'char *'",
        ],
    );
    // The message is optional only in C23.
    rejected(
        Standard::C11,
        "_Static_assert(1);",
        &["'_Static_assert' without a message requires C23 or later (this block is c11!)"],
    );
    accepted(Standard::C23, "static_assert(1);");
    rejected(
        Standard::C23,
        "static_assert(1 == 2);",
        &["static assertion failed"],
    );
}

#[test]
fn generic_selection_picks_by_type() {
    let program = |body: &str| {
        format!(
            "int i; double d; char *s; int a[4];\n\
             int f(void) {{ return {body}; }}"
        )
    };
    accepted(Standard::C11, &program("_Generic(i, int: 1, double: 2)"));
    // An array decays before the association is chosen (C11 DR 481).
    accepted(Standard::C11, &program("_Generic(a, int *: 1, default: 0)"));
    accepted(
        Standard::C11,
        &program("_Generic(s, char *: 1, default: 0)"),
    );
    // Only the chosen association is checked: `d.x` is nonsense, and never
    // looked at.
    accepted(
        Standard::C11,
        &program("_Generic(i, int: 1, double: d < 0)"),
    );
    rejected(
        Standard::C11,
        &program("_Generic(d, int: 1, char *: 2)"),
        &["'_Generic' has no association for the controlling expression's type 'double'"],
    );
    rejected(
        Standard::C11,
        &program("_Generic(i, int: 1, int: 2)"),
        &["'_Generic' has two associations for the compatible type 'int'"],
    );
    rejected(
        Standard::C11,
        &program("_Generic(i, default: 1, default: 2)"),
        &["'_Generic' has more than one 'default' association"],
    );
}

#[test]
fn generic_associations_are_compared_with_their_qualifiers() {
    let program = |body: &str| {
        format!(
            "const int c; int i; const int *pc; int *p;\n\
             int f(void) {{ return {body}; }}"
        )
    };
    // C11 6.5.1.1p2 forbids two associations naming *compatible* types, and a
    // qualifier is part of the type: `int` and `const int` are not compatible.
    accepted(
        Standard::C11,
        &program("_Generic(i, int: 1, const int: 2, volatile int: 3)"),
    );
    accepted(
        Standard::C11,
        &program("_Generic(p, int *: 1, const int *: 2, int *const: 3)"),
    );
    accepted(Standard::C11, &program("_Generic(c, int: 1, int[4]: 2)"));
    // Repeating one *with* its qualifiers is still a duplicate, and the
    // message spells the type the way C does.
    rejected(
        Standard::C11,
        &program("_Generic(i, const int: 1, const int: 2, default: 0)"),
        &["'_Generic' has two associations for the compatible type 'const int'"],
    );
    rejected(
        Standard::C11,
        &program("_Generic(p, int *const: 1, int *const: 2, default: 0)"),
        &["'_Generic' has two associations for the compatible type 'int * const'"],
    );
    // The controlling expression's type has had the lvalue conversion applied
    // to it (DR 481), so a qualified association can never be the one chosen.
    rejected(
        Standard::C11,
        &program("_Generic(c, const int: 1)"),
        &["'_Generic' has no association for the controlling expression's type 'int'"],
    );
    rejected(
        Standard::C11,
        &program("_Generic(pc, int *const: 1)"),
        &["'_Generic' has no association for the controlling expression's type 'const int *'"],
    );
}

#[test]
fn anonymous_members_are_reached_through() {
    accepted(
        Standard::C11,
        "struct S { int tag; union { int i; double d; }; };\n\
         int f(struct S *s) { s->i = 1; return s->i; }\n\
         struct S g(void) { struct S s = { .tag = 1, .i = 2 }; return s; }\n\
         unsigned long o(void) { return __builtin_offsetof(struct S, d); }",
    );
    rejected(
        Standard::C11,
        "struct S { int tag; union { int tag; double d; }; };",
        &["member 'tag' of this anonymous member is already a member of the enclosing struct"],
    );
    rejected(
        Standard::C11,
        "struct S { int; };",
        &[
            "a member declaration must declare a member; only a struct or union member \
           may be unnamed",
        ],
    );
}

#[test]
fn alignas_is_honoured_on_a_member() {
    accepted(
        Standard::C11,
        "struct S { _Alignas(16) int a; char c; };\n\
         int f(void) { return sizeof(struct S) == 16 && _Alignof(struct S) == 16; }",
    );
    // A `union` puts every member at offset zero, so any alignment works.
    accepted(Standard::C11, "union U { _Alignas(16) int a; double d; };");
    // A member the alignment has to *move* is honoured too: explicit padding
    // in the generated item puts it where C says it goes.
    accepted(
        Standard::C11,
        "struct S { char c; _Alignas(16) int a; };\n\
         int f(void) { return __builtin_offsetof(struct S, a) == 16 \
                           && sizeof(struct S) == 32; }",
    );
    rejected(
        Standard::C11,
        "_Alignas(16) int global;",
        &[
            "an alignment specifier on an object is not supported yet; '_Alignas' and \
           '__attribute__((aligned))' are honoured on the members of a struct or union, \
           where the generated Rust type can carry the alignment",
        ],
    );
    rejected(
        Standard::C11,
        "struct S { _Alignas(3) int a; };",
        &["the requested alignment 3 is not a power of two"],
    );
}

#[test]
fn noreturn_ends_a_function() {
    // `abort` is `_Noreturn` in the bundled <stdlib.h>, so the function needs
    // no `return` of its own.
    accepted(
        Standard::C11,
        "#include <stdlib.h>\nint f(int x) { if (x > 0) return x; abort(); }",
    );
    accepted(
        Standard::C11,
        "_Noreturn void die(void); int f(int x) { if (x > 0) return x; die(); }",
    );
    rejected(
        Standard::C11,
        "_Noreturn int x;",
        &["'_Noreturn' is only allowed on a function"],
    );
}

#[test]
fn what_c11_added_and_this_crate_does_not_do() {
    rejected(
        Standard::C11,
        "_Thread_local int counter;",
        &["'_Thread_local' is not supported yet; Rust's own `#[thread_local]` is unstable"],
    );
    rejected(
        Standard::C11,
        "_Atomic int counter;",
        &["'_Atomic' is not supported yet"],
    );
    rejected(
        Standard::C23,
        "_BitInt(7) narrow;",
        &["'_BitInt' is not supported yet"],
    );
}

/// The C11 and C23 literal prefixes, and the revision each one needs.
///
/// The prefix is recognised in every entry point so that the diagnostic names
/// the macro to write instead of complaining that `u` is undeclared.
#[test]
fn the_unicode_literal_prefixes_are_gated() {
    accepted(Standard::C11, "const char *s = u8\"utf8\";");
    accepted(Standard::C11, "const unsigned short *s = u\"utf16\";");
    accepted(Standard::C11, "const unsigned int *s = U\"utf32\";");
    accepted(Standard::C11, "int c = u'x'; int d = U'x';");
    accepted(Standard::C23, "int c = u8'x';");
    // `const void *`, because C23 changed the element type from `char` to
    // `char8_t` and this is about the gate rather than about that.
    since(
        Standard::C11,
        "const void *s = u8\"utf8\";",
        "a 'u8' literal",
    );
    since(
        Standard::C11,
        "const void *s = u\"utf16\";",
        "a 'u' literal",
    );
    since(
        Standard::C11,
        "const void *s = U\"utf32\";",
        "a 'U' literal",
    );
    since(Standard::C11, "int c = u'x';", "a 'u' literal");
    // `u8'x'` is C23's, though `u8"…"` has been there since C11.
    since(Standard::C23, "int c = u8'x';", "a 'u8' literal");
    rejected(
        Standard::C11,
        "int c = u'ab';",
        &["a 'u' character constant holds exactly one character"],
    );
    rejected(
        Standard::C23,
        "int c = u8'\\u00e9';",
        &["the character in a 'u8' character constant must fit in a single code unit"],
    );
    rejected(
        Standard::C11,
        "int c = u'\\U0001F600';",
        &["the character in a 'u' character constant must fit in a single code unit"],
    );
    rejected(
        Standard::C11,
        "const void *s = u\"a\" U\"b\";",
        &["cannot concatenate a 'u' string literal with a 'U' one"],
    );
    rejected(
        Standard::C11,
        "const void *s = u\"\\U00110000\";",
        &["'\\u110000' is not a valid universal character name"],
    );
}

// ---------------------------------------------------------------------------
// C23
// ---------------------------------------------------------------------------

#[test]
fn c23_features_are_gated() {
    // A C23 keyword is an ordinary identifier in an older block, so the gate
    // is reported where the name is *used*: as a declaration specifier, or as
    // the undeclared identifier it would otherwise be.
    since(Standard::C23, "bool f(void) { return 1; }", "'bool'");
    since(Standard::C23, "int f(void) { return true; }", "'true'");
    since(
        Standard::C23,
        "void *f(void) { return nullptr; }",
        "'nullptr'",
    );
    since(Standard::C23, "constexpr int n = 4;", "'constexpr'");
    // `typeof` is the one C23 keyword a GNU dialect also has, so its message
    // names both ways out; see `Gating::newer_keyword`.
    accepted(Standard::C23, "typeof(int) x;");
    for standard in [Standard::C99, Standard::C11, Standard::C17] {
        let found = errors(standard, "typeof(int) x;");
        assert_eq!(
            found.first().map(String::as_str),
            Some(
                format!(
                    "'typeof' requires a GNU dialect ({}) or C23 or later (this block is {})",
                    standard.macro_name_in(cinrs_core::Dialect::Gnu),
                    standard.macro_name()
                )
                .as_str()
            ),
        );
    }
    since(
        Standard::C23,
        "struct S { alignas(16) int a; };",
        "'alignas'",
    );
    // These two are gated before C23 and unsupported after it, so only the
    // gate is checked.
    gated_before(Standard::C23, "static_assert(1);", "'static_assert'");
    gated_before(Standard::C23, "thread_local int x;", "'thread_local'");
    since(
        Standard::C23,
        "int f(void) { return 0b1011; }",
        "a binary integer constant",
    );
    since(
        Standard::C23,
        "int f(void) { return 1'000; }",
        "a digit separator",
    );
    since(
        Standard::C23,
        "[[maybe_unused]] int f(void) { return 0; }",
        "an attribute specifier",
    );
    since(
        Standard::C23,
        "struct P { int a; int b; }; struct P p = {};",
        "an empty initializer",
    );
    since(
        Standard::C23,
        "enum E : unsigned char { A };",
        "an enum with a fixed underlying type",
    );
    since(
        Standard::C23,
        "int f(void) { done: }",
        "a label at the end of a compound statement",
    );
    since(
        Standard::C23,
        "int f(void) { here: int x = 1; return x; }",
        "a label before a declaration",
    );
}

#[test]
fn c23_keywords_are_ordinary_identifiers_before_c23() {
    // This is what `<stdbool.h>` depends on, and it is why the C23 keywords
    // are not recognised in an older block.
    accepted(
        Standard::C99,
        "#include <stdbool.h>\nbool f(void) { return true; }",
    );
    accepted(Standard::C99, "int typeof; int f(void) { return typeof; }");
    // In C23 the header defines nothing at all, and the keywords do the work.
    accepted(
        Standard::C23,
        "#include <stdbool.h>\nbool f(void) { return true; }",
    );
    accepted(
        Standard::C11,
        "typedef int bool; bool f(void) { return 0; }",
    );
}

#[test]
fn constexpr_objects_fold_to_their_value() {
    accepted(
        Standard::C23,
        "constexpr int N = 4; int a[N]; int f(void) { return sizeof(a) / sizeof(a[0]); }",
    );
    accepted(
        Standard::C23,
        "int f(int x) { constexpr double half = 0.5; return x * half; }",
    );
    rejected(
        Standard::C23,
        "int n; constexpr int m = n;",
        &["the initializer of a 'constexpr' object is not a compile-time constant expression"],
    );
    rejected(
        Standard::C23,
        "constexpr int n;",
        &["'n' is declared 'constexpr' and needs an initializer"],
    );
    rejected(
        Standard::C23,
        "constexpr int f(void) { return 1; }",
        &["'constexpr' is not supported on a function"],
    );
    rejected(
        Standard::C23,
        "struct P { int a; }; constexpr struct P p = { 1 };",
        &[
            "a 'constexpr' object of type 'struct P' is not supported yet; only the \
           arithmetic types are",
        ],
    );
    // A constant is not an object: it cannot be assigned to.
    rejected(
        Standard::C23,
        "int f(void) { constexpr int n = 1; n = 2; return n; }",
        &["expression is not assignable"],
    );
}

#[test]
fn typeof_takes_the_type_without_evaluating() {
    accepted(
        Standard::C23,
        "int f(void) { int a[4]; typeof(a) b = { 1, 2, 3, 4 }; return b[0]; }",
    );
    accepted(
        Standard::C23,
        "int f(void) { const char *s = \"x\"; typeof_unqual(s) t = s; return *t; }",
    );
    accepted(
        Standard::C23,
        "typeof(int *) p; int f(void) { return p == 0; }",
    );
}

#[test]
fn auto_infers_from_the_initialiser() {
    accepted(
        Standard::C23,
        "int f(void) { auto x = 3; auto y = 1.5; return x * y; }",
    );
    rejected(
        Standard::C23,
        "int f(void) { auto x; return x; }",
        &["'x' is declared 'auto' and needs an initializer"],
    );
    rejected(
        Standard::C23,
        "int f(void) { auto x = { 1 }; return 0; }",
        &["the type of 'x' cannot be inferred from a braced initializer"],
    );
}

#[test]
fn c23_enums_can_fix_their_underlying_type() {
    accepted(
        Standard::C23,
        "enum Small : unsigned char { A, B }; int f(void) { return sizeof(enum Small) == 1; }",
    );
    rejected(
        Standard::C23,
        "enum Small : unsigned char { A = 300 };",
        &["enumerator value 300 is outside the range of 'unsigned char'"],
    );
    rejected(
        Standard::C23,
        "enum Bad : double { A };",
        &["the underlying type of an enum must be an integer type, not 'double'"],
    );
}

#[test]
fn nullptr_is_a_null_pointer_constant() {
    accepted(
        Standard::C23,
        "int f(int *p) { return p == nullptr; }\n\
         int *g(void) { return nullptr; }\n\
         #include <stddef.h>\n\
         nullptr_t h(void) { return nullptr; }",
    );
}

#[test]
fn unreachable_is_a_promise() {
    accepted(
        Standard::C23,
        "#include <stddef.h>\nint f(int x) { if (x > 0) return x; unreachable(); }",
    );
}

#[test]
fn attributes_are_parsed_and_ignored() {
    accepted(
        Standard::C23,
        "[[deprecated(\"use g\")]] int f([[maybe_unused]] int x) { return x; }\n\
         [[nodiscard]] int g(void) { return 0; }\n\
         struct S { [[deprecated]] int x; };\n\
         int h(int x) { switch (x) { case 1: [[fallthrough]]; default: return 0; } }",
    );
    // `[[noreturn]]` says what `_Noreturn` says.
    accepted(
        Standard::C23,
        "[[noreturn]] void die(void); int f(int x) { if (x) return x; die(); }",
    );
}

#[test]
fn c17_is_c11_with_a_different_version() {
    accepted(
        Standard::C17,
        "_Static_assert(__STDC_VERSION__ == 201710L, \"C17\");",
    );
    accepted(
        Standard::C11,
        "_Static_assert(__STDC_VERSION__ == 201112L, \"C11\");",
    );
    accepted(
        Standard::C23,
        "static_assert(__STDC_VERSION__ == 202311L, \"C23\");",
    );
}
