//! The ten entry points: five revisions, each strict or GNU.
//!
//! The line between them is GCC's own. Everything spelled with a leading
//! double underscore is available either way, because those names are
//! reserved; the plain spellings `typeof` and `asm` need a GNU dialect, and
//! only there is a construct a later revision introduced accepted without a
//! diagnostic naming the macro to write instead.
//!
//! `c89!` is the one entry point that is *older* than the rest rather than
//! newer, so the gating runs the other way: everything C99 added is refused
//! there, with the same message and the same shape. `gnu89!` switches all of
//! it back on, exactly as `gcc -std=gnu89` does, and keeps only the three
//! rules a later revision *deleted* — which is what `tests/knr.rs` is about.

use cinrs::{c17, c23, c89, c90, gnu11, gnu17, gnu23, gnu89, gnu99};

gnu99! {
    #include <stddef.h>

    /* Every one of these needs a later revision than C99, and `gnu99!` takes
     * them all — exactly as `gcc -std=gnu99` does. */
    _Static_assert(sizeof(int) >= 4, "int is at least 32 bits");

    struct Tagged {
        int tag;
        union { int as_int; double as_double; };   /* C11: anonymous member */
    };

    int kind(int n) { return _Generic(n, int: 1, double: 2, default: 0); }
    size_t alignment(void) { return _Alignof(double); }
    int binary(void) { return 0b1010; }
    int empty_init(void) { struct Tagged t = {}; return t.tag; }

    _Noreturn void gnu99_never(void);
    void gnu99_never(void) { for (;;) { } }

    typeof(int) plain(int n) { return n; }
    int tagged_int(struct Tagged t) { return t.as_int; }
}

gnu11! {
    int gnu11_typeof(int n) { typeof(n) m = n; return m; }
    int gnu11_binary(void) { return 0b11; }
}

gnu17! {
    int gnu17_version(void) { return (int) (__STDC_VERSION__ / 100L); }
}

gnu23! {
    /* A GNU dialect of C23 still has everything C23 has. */
    constexpr int GNU23_LIMIT = 4;
    static_assert(GNU23_LIMIT == 4);
    int gnu23_limit(void) { return GNU23_LIMIT; }
    typeof(int) gnu23_typeof(int n) { return n; }
}

c23! {
    /* `typeof` is C23's own keyword, so the strict entry point has it too. */
    typeof(int) c23_typeof(int n) { return n; }
}

c17! {
    /* And here it is an ordinary identifier, which is the whole reason the
     * strict entry points cannot have it. */
    int typeof_is_a_variable(void) {
        int typeof = 3;
        int asm = 4;
        return typeof + asm;
    }
}

#[test]
fn a_gnu_dialect_accepts_what_a_later_revision_added() {
    assert_eq!(unsafe { kind(0) }, 1);
    assert_eq!(unsafe { alignment() } as usize, align_of::<f64>());
    assert_eq!(unsafe { binary() }, 10);
    assert_eq!(unsafe { empty_init() }, 0);
    assert_eq!(unsafe { plain(7) }, 7);
    let t = Tagged {
        tag: 1,
        __cinrs_anon0: unsafe { core::mem::zeroed() },
    };
    assert_eq!(unsafe { tagged_int(t) }, 0);
}

#[test]
fn every_gnu_entry_point_exists() {
    assert_eq!(unsafe { gnu11_typeof(5) }, 5);
    assert_eq!(unsafe { gnu11_binary() }, 3);
    assert_eq!(unsafe { gnu17_version() }, 2017);
    assert_eq!(unsafe { gnu23_limit() }, 4);
    assert_eq!(unsafe { gnu23_typeof(9) }, 9);
    assert_eq!(unsafe { c23_typeof(9) }, 9);
    assert_eq!(unsafe { typeof_is_a_variable() }, 7);
}

// ---------------------------------------------------------------------------
// what both dialects share
// ---------------------------------------------------------------------------

use cinrs::c99;

c99! {
    #include <stddef.h>

    /* The `__`-spelled extensions are available in a strict entry point, and
     * this is the point of the policy: a program that guards them with
     * `#if defined(__GNUC__)` gets them either way. */
    __typeof__(int) strict_typeof(int n) { return n; }
    int strict_stmt_expr(int n) { return ({ int t = n; t * 2; }); }
    int strict_attribute(int n) __attribute__((const));
    int strict_attribute(int n) { return n; }
    int strict_builtin(unsigned int n) { return __builtin_popcount(n); }
    size_t strict_alignof(void) { return __alignof__(long); }
    unsigned __int128 strict_int128(unsigned long long n) { return (unsigned __int128) n * n; }
    __thread int strict_thread_local = 4;
    int strict_thread_read(void) { return strict_thread_local; }
}

#[test]
fn the_double_underscore_extensions_need_no_gnu_entry_point() {
    assert_eq!(unsafe { strict_typeof(1) }, 1);
    assert_eq!(unsafe { strict_stmt_expr(21) }, 42);
    assert_eq!(unsafe { strict_attribute(3) }, 3);
    assert_eq!(unsafe { strict_builtin(7) }, 3);
    assert_eq!(
        unsafe { strict_alignof() } as usize,
        align_of::<core::ffi::c_long>()
    );
    assert_eq!(
        unsafe { strict_int128(u64::MAX) },
        u128::from(u64::MAX) * u128::from(u64::MAX)
    );
    assert_eq!(unsafe { strict_thread_read() }, 4);
}

// ---------------------------------------------------------------------------
// C89 and its GNU dialect
// ---------------------------------------------------------------------------

c89! {
    /* C89 as published has no `__STDC_VERSION__` at all — Amendment 1 added
     * it in 1995 — so a program that tests for it must not find one. */
    #ifdef __STDC_VERSION__
    #error "__STDC_VERSION__ must not be defined in a c89! block"
    #endif
    #if __STDC__ != 1
    #error "__STDC__ is 1 in every entry point, C89 included"
    #endif
    #ifndef __STRICT_ANSI__
    #error "__STRICT_ANSI__ is defined in the strict entry points"
    #endif

    int c89_answer(void) { return 89; }
}

c90! {
    /* ISO/IEC 9899:1990 is the same language under the other name. */
    #ifdef __STDC_VERSION__
    #error "__STDC_VERSION__ must not be defined in a c90! block"
    #endif

    int c90_answer(void) { return 90; }
}

gnu89! {
    #ifdef __STDC_VERSION__
    #error "__STDC_VERSION__ must not be defined in a gnu89! block either"
    #endif
    #ifdef __STRICT_ANSI__
    #error "__STRICT_ANSI__ is for the strict entry points only"
    #endif
    #if __GNUC__ < 4
    #error "__GNUC__ is 4 in every entry point"
    #endif

    // A `//` comment, which C99 took from C++ and `gnu89!` has anyway.
    int gnu89_answer(void) { return 89; }
}

#[test]
fn the_c89_entry_points_define_no_version_macro() {
    assert_eq!(unsafe { c89_answer() }, 89);
    assert_eq!(unsafe { c90_answer() }, 90);
    assert_eq!(unsafe { gnu89_answer() }, 89);
}

// ---------------------------------------------------------------------------
// what `c89!` refuses, in the words it refuses it with
// ---------------------------------------------------------------------------

use std::str::FromStr;

use cinrs_core::{Dialect, Level, Options, Standard, analyze, sema};
use proc_macro2::TokenStream;

/// Every error `source` produces in `options`, the front end's and sema's
/// alike — a feature may be gated in either.
fn errors(options: &Options, source: &str) -> Vec<String> {
    // String-literal mode accepts every C token, `//` comments included.
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let analysis = analyze(input, options);
    let mut out: Vec<(cinrs_core::Pos, String)> = analysis
        .diagnostics
        .sorted()
        .into_iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| (d.range.start, d.message.clone()))
        .collect();
    let (_program, diagnostics) = sema::analyze(&analysis.unit, options, analysis.source.unit_id());
    out.extend(
        diagnostics
            .sorted()
            .into_iter()
            .filter(|d| d.level == Level::Error)
            .map(|d| (d.range.start, d.message.clone())),
    );
    out.sort_by_key(|(pos, _)| *pos);
    out.into_iter().map(|(_, message)| message).collect()
}

/// A C99 construct: refused in `c89!` with `message`, and accepted in
/// `gnu89!`, which is what `gcc -std=gnu89` does with the same text.
#[track_caller]
fn c99_only(source: &str, message: &str) {
    let strict = errors(&Options::new(Standard::C89), source);
    assert_eq!(
        strict.first().map(String::as_str),
        Some(format!("{message} requires C99 or later (this block is c89!)").as_str()),
        "for:\n{source}"
    );
    let gnu = errors(&Options::with_dialect(Standard::C89, Dialect::Gnu), source);
    assert!(gnu.is_empty(), "gnu89! rejected it: {gnu:#?}\n{source}");
    // And of course the revision that introduced it takes it.
    let c99 = errors(&Options::new(Standard::C99), source);
    assert!(c99.is_empty(), "c99! rejected it: {c99:#?}\n{source}");
}

#[test]
fn c89_gates_what_c99_added() {
    c99_only("int f(void) { return 1; } // a comment\n", "a '//' comment");
    c99_only(
        "int f(void) { int a = 1; a++; int b = 2; return a + b; }",
        "a declaration after a statement",
    );
    c99_only(
        "int f(void) { int t = 0; for (int i = 0; i < 3; i++) t += i; return t; }",
        "a declaration in a 'for' clause",
    );
    c99_only("long long f(void) { return 1; }", "'long long'");
    c99_only(
        "struct S { int a; int b; }; int f(void) { struct S s = { .b = 1 }; return s.b; }",
        "a designated initializer",
    );
    c99_only("int f(void) { _Bool b = 1; return b; }", "'_Bool'");
    c99_only("int f(int *restrict p) { return *p; }", "'restrict'");
    c99_only("inline int f(void) { return 1; }", "'inline'");
    c99_only(
        "int f(int n) { int a[n]; return (int)sizeof a; }",
        "a variable length array",
    );
    c99_only(
        "struct S { int a; }; int f(void) { return (struct S){ 1 }.a; }",
        "a compound literal",
    );
    c99_only(
        "#define LIST(...) __VA_ARGS__\nint f(void) { return LIST(1); }",
        "a variadic macro",
    );
    c99_only(
        "struct S { int n; int data[]; };",
        "a flexible array member",
    );
    c99_only("const char *f(void) { return __func__; }", "'__func__'");
    c99_only(
        "enum E { A, B, };",
        "a trailing comma in an enumerator list",
    );
    c99_only(
        "int f(int a[static 3]) { return a[0]; }",
        "'static' in an array parameter declarator",
    );
    c99_only("int f(int a[*]);", "'[*]'");
}

/// The reserved spellings of the two C99 keywords a `c89!` block may not
/// write, which GCC keeps in `-std=c89` for the same reason this does: the
/// names belong to the implementation.
#[test]
fn the_reserved_spellings_work_in_a_c89_block() {
    let source = "__inline int f(int *__restrict p) { return *p; }";
    let found = errors(&Options::new(Standard::C89), source);
    assert!(found.is_empty(), "c89! rejected it: {found:#?}");
}

/// `__extension__` is GCC's "this is an extension and I know it", and it
/// switches the gate off for the declaration it is written on — which is what
/// lets the bundled headers declare `llabs` to a `c89!` block.
#[test]
fn extension_switches_the_gate_off() {
    let c89 = Options::new(Standard::C89);
    let accepted = [
        "__extension__ long long wide(void) { return 1; }",
        "__extension__ typedef struct { long long q; long long r; } lldiv_t;",
        "int f(void) { __extension__ long long n = 1; return (int)n; }",
        "#include <stdlib.h>\nint f(void) { return abs(-1); }",
        "#include <stdio.h>\n#include <string.h>\n#include <stdint.h>\nint f(void) { return 0; }",
    ];
    for source in accepted {
        let found = errors(&c89, source);
        assert!(found.is_empty(), "c89! rejected it: {found:#?}\n{source}");
    }
    // It covers the declaration it is written on and no more.
    let found = errors(
        &c89,
        "__extension__ long long wide(void) { long long n = 1; return n; }",
    );
    assert_eq!(
        found.first().map(String::as_str),
        Some("'long long' requires C99 or later (this block is c89!)"),
    );
}

// ---------------------------------------------------------------------------
// trigraphs
// ---------------------------------------------------------------------------

c99! { r##"
??=include <string.h>
??=define JOIN(a, b) a ??=??= b

int trigraph_brackets(void) { int a??(3??) = ??<1, 2, 3??>; return a??(1??); }
int trigraph_ops(int a, int b) { return ((a ??! b) ??' (a ??!??! b)) + ??-a; }
int trigraph_assign(int a, int b) { a ??!= b; return a; }
const char *trigraph_string(void) { return "??!??'??-x"; }
int trigraph_question_marks(void) { return (int) sizeof("what??") - 1; }
int trigraph_caret(int a) { a ??'= 3; return a; }

/* `??/` at the end of a line is the backslash that splices it, so this is one
   identifier and one `return`. */
int trigraph_splice(void) {
    int JOIN(xy, z) = 5;
    return xy??/
z;
}
"## }

/// The nine trigraphs, each doing what `gcc -std=c99` does with the same text.
///
/// Every expected value here was checked against that compiler.
#[test]
fn a_strict_entry_point_replaces_trigraphs() {
    unsafe {
        assert_eq!(trigraph_brackets(), 2);
        assert_eq!(trigraph_ops(12, 10), 2);
        assert_eq!(trigraph_assign(12, 10), 14);
        let s = core::ffi::CStr::from_ptr(trigraph_string())
            .to_str()
            .unwrap();
        assert_eq!(s, "|^~x");
        assert_eq!(trigraph_question_marks(), 6);
        assert_eq!(trigraph_caret(12), 15);
        assert_eq!(trigraph_splice(), 5);
    }
}

/// Which entry points have them: every strict one below `c23!`, and no other.
///
/// C23 removed trigraphs (N2940) and GCC's `-std=gnu*` never had them on, so
/// `??!` there is two question marks and a `!` — which is a syntax error in
/// this expression, and that is exactly how the test tells the two apart.
#[test]
fn trigraphs_are_a_strict_pre_c23_feature() {
    let source = "int f(void) { return 1 ??! 2; }";
    for standard in [Standard::C89, Standard::C99, Standard::C11, Standard::C17] {
        let found = errors(&Options::new(standard), source);
        assert!(
            found.is_empty(),
            "{} rejected a trigraph: {found:#?}",
            standard.macro_name()
        );
        let gnu = errors(&Options::with_dialect(standard, Dialect::Gnu), source);
        assert!(
            !gnu.is_empty(),
            "{} replaced a trigraph",
            standard.macro_name_in(Dialect::Gnu)
        );
    }
    for options in [
        Options::new(Standard::C23),
        Options::with_dialect(Standard::C23, Dialect::Gnu),
    ] {
        let found = errors(&options, source);
        assert!(
            !found.is_empty(),
            "{} replaced a trigraph",
            options.macro_name()
        );
    }
}
