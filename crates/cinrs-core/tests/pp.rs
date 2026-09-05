//! Preprocessor tests, in the shape `gcc -E` invites: text in, the spelling of
//! every surviving token out.
//!
//! The backbone of the file is C99 6.10.3.5, "Scope of macro definitions",
//! whose worked examples exist precisely because everyone gets rescanning,
//! stringification and pasting wrong the first time. Anything that passes
//! those has a real preprocessor behind it.

use std::path::PathBuf;

use cinrs_core::diag::Level;
use cinrs_core::lex::{LexOptions, lex_text};
use cinrs_core::pp::{Context, Preprocessed, preprocess};
use cinrs_core::{Options, Standard};

fn lex_options() -> LexOptions {
    LexOptions::new(Standard::C99)
}

/// Preprocesses `src` with a context that knows nothing about a `.rs` file, so
/// `__LINE__` counts lines of `src` itself.
fn run(src: &str) -> (Vec<String>, Vec<String>) {
    run_in(src, Context::new(src, 0))
}

fn run_in(src: &str, ctx: Context) -> (Vec<String>, Vec<String>) {
    let mut diags = cinrs_core::Diagnostics::new();
    let tokens = lex_text(src, ctx.base, &lex_options());
    let out = preprocess(&tokens, &ctx, &Options::new(Standard::C99), &mut diags);
    let spellings = out
        .tokens
        .iter()
        .filter(|t| !t.is_eof())
        .map(|t| t.kind.spelling().to_owned())
        .collect();
    let errors = diags
        .items()
        .iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| {
            let mut text = d.message.clone();
            for note in &d.notes {
                text.push_str("\nnote: ");
                text.push_str(&note.message);
            }
            text
        })
        .collect();
    (spellings, errors)
}

/// The token spellings `src` preprocesses to, joined with single spaces.
///
/// Panics if anything was reported: a test that expects an error uses
/// [`errors`] instead.
#[track_caller]
fn pp(src: &str) -> String {
    let (tokens, errors) = run(src);
    assert!(
        errors.is_empty(),
        "unexpected errors for {src:?}: {errors:#?}"
    );
    tokens.join(" ")
}

/// The error messages `src` produces.
#[track_caller]
fn errors(src: &str) -> Vec<String> {
    run(src).1
}

/// Asserts that `src` produces exactly one error, containing `needle`.
#[track_caller]
fn one_error(src: &str, needle: &str) {
    let errors = errors(src);
    assert_eq!(
        errors.len(),
        1,
        "expected one error for {src:?}: {errors:#?}"
    );
    assert!(
        errors[0].contains(needle),
        "expected {needle:?} in {:?}",
        errors[0]
    );
}

// ---------------------------------------------------------------------------
// C99 6.10.3.5 — the standard's own examples
// ---------------------------------------------------------------------------

/// EXAMPLE 3: the one that needs argument pre-expansion, rescanning and hide
/// sets all at once. The expected output is copied from the standard.
#[test]
fn standard_example_3() {
    let source = r"
#define x 3
#define f(a) f(x * (a))
#undef x
#define x 2
#define g f
#define z z[0]
#define h g(~
#define m(a) a(w)
#define w 0,1
#define t(a) a
#define p() int
#define q(x) x
#define r(x,y) x ## y
#define str(x) # x
f(y+1) + f(f(z)) % t(t(g)(0) + t)(1);
g(x+(3,4)-w) | h 5) & m
    (f)^m(m);
p() i[q()] = { q(1), r(2,3), r(4,), r(,5), r(,) };
char c[2][6] = { str(hello), str() };
";
    // 6.10.3.5p5 gives the result; `gcc -E` prints it exactly like this.
    assert_eq!(
        pp(source),
        "f ( 2 * ( y + 1 ) ) + f ( 2 * ( f ( 2 * ( z [ 0 ] ) ) ) ) % \
         f ( 2 * ( 0 ) ) + t ( 1 ) ; \
         f ( 2 * ( 2 + ( 3 , 4 ) - 0 , 1 ) ) | f ( 2 * ( ~ 5 ) ) & \
         f ( 2 * ( 0 , 1 ) ) ^ m ( 0 , 1 ) ; \
         int i [ ] = { 1 , 23 , 4 , 5 , } ; \
         char c [ 2 ] [ 6 ] = { \"hello\" , \"\" } ;"
    );
}

/// EXAMPLE 4: `#` reproduces the spelling of its argument, comments and all —
/// or rather, without them, since a comment is white space by then.
#[test]
fn standard_example_4() {
    let source = r#"
#define str(s) # s
#define xstr(s) str(s)
#define debug(s, t) printf("x" # s "= %d, x" # t "= %s", \
    x ## s, x ## t)
#define INCFILE(n) vers ## n
#define glue(a, b) a ## b
#define xglue(a, b) glue(a, b)
#define HIGHLOW "hello"
#define LOW LOW ", world"
debug(1, 2);
fputs(str(strncmp("abc\0d", "abc", '\4') // this goes away
    == 0) str(: @\n), s);
glue(HIGH, LOW);
xglue(HIGH, LOW)
"#;
    // `gcc -E` prints exactly this. Note `": @\n"`: a `\` outside a character
    // constant or a string literal is *not* escaped, which the standard says
    // and everyone assumes the other way round.
    assert_eq!(
        pp(source),
        r#"printf ( "x" "1" "= %d, x" "2" "= %s" , x1 , x2 ) ; "#.to_owned()
            + r#"fputs ( "strncmp(\"abc\\0d\", \"abc\", '\\4') == 0" ": @\n" , s ) ; "#
            + r#""hello" ; "hello" ", world""#
    );
}

/// EXAMPLE 5: `t(x,y,z)` pastes three ways round, including through empty
/// arguments, which is what placemarkers are for.
#[test]
fn standard_example_5() {
    let source = r"
#define t(x,y,z) x ## y ## z
int j[] = { t(1,2,3), t(,4,5), t(6,,7), t(8,9,),
    t(10,,), t(,11,), t(,,12), t(,,) };
";
    assert_eq!(
        pp(source),
        "int j [ ] = { 123 , 45 , 67 , 89 , 10 , 11 , 12 , } ;"
    );
}

/// EXAMPLE 7: `__VA_ARGS__`, including the `report` case whose second argument
/// is itself a comma-separated list.
#[test]
fn standard_example_7() {
    let source = r#"
#define debug(...) fprintf(stderr, __VA_ARGS__)
#define showlist(...) puts(#__VA_ARGS__)
#define report(test, ...) ((test)?puts(#test):\
    printf(__VA_ARGS__))
debug("Flag");
debug("X = %d\n", x);
showlist(The first, second, and third items.);
report(x>y, "x is %d but y is %d", x, y);
"#;
    assert_eq!(
        pp(source),
        r#"fprintf ( stderr , "Flag" ) ; "#.to_owned()
            + r#"fprintf ( stderr , "X = %d\n" , x ) ; "#
            + r#"puts ( "The first, second, and third items." ) ; "#
            + r#"( ( x > y ) ? puts ( "x>y" ) : printf ( "x is %d but y is %d" , x , y ) ) ;"#
    );
}

/// EXAMPLE 1 of 6.10.3: a definition may be repeated as long as it is repeated
/// exactly, white space included.
#[test]
fn a_definition_may_be_repeated_identically() {
    // The standard's own valid pair.
    assert_eq!(
        pp(r"
#define OBJ_LIKE (1-1)
#define OBJ_LIKE /* white space */ (1-1) /* other */
#define FUNC_LIKE(a) ( a )
#define FUNC_LIKE( a )( /* note the white space */ \
    a /* other stuff on this line
        */ )
OBJ_LIKE FUNC_LIKE(x)
"),
        "( 1 - 1 ) ( x )"
    );
}

#[test]
fn a_different_definition_is_an_error() {
    // The standard's own invalid pairs.
    let errors = errors(
        r"
#define OBJ_LIKE (1-1)
#define OBJ_LIKE (0)
#define FUNC_LIKE(b) ( a )
#define FUNC_LIKE(b) ( b )
",
    );
    assert_eq!(errors.len(), 2, "{errors:#?}");
    assert!(
        errors[0].contains("macro 'OBJ_LIKE' redefined"),
        "{errors:#?}"
    );
    assert!(
        errors[0].contains("note: previous definition of 'OBJ_LIKE' is"),
        "{errors:#?}"
    );
    assert!(
        errors[1].contains("macro 'FUNC_LIKE' redefined"),
        "{errors:#?}"
    );
}

#[test]
fn white_space_separation_is_part_of_a_definition() {
    // `(1 - 1)` and `(1-1)` hold the same tokens but not in the same places.
    one_error(
        "#define A (1-1)\n#define A (1 - 1)\n",
        "macro 'A' redefined",
    );
}

// ---------------------------------------------------------------------------
// object-like and function-like macros
// ---------------------------------------------------------------------------

#[test]
fn an_object_like_macro_is_replaced_everywhere() {
    assert_eq!(
        pp("#define N 4\nint a[N]; int b = N + N;"),
        "int a [ 4 ] ; int b = 4 + 4 ;"
    );
}

#[test]
fn a_macro_may_expand_to_nothing() {
    assert_eq!(pp("#define EMPTY\nint EMPTY x;"), "int x ;");
    assert_eq!(pp("#define NOTHING(x)\nint NOTHING(a) y;"), "int y ;");
}

#[test]
fn a_function_like_name_without_a_paren_is_not_an_invocation() {
    assert_eq!(
        pp("#define f(x) x\nint (*p)() = f;"),
        "int ( * p ) ( ) = f ;"
    );
    // Not even across a newline, if what follows is not `(`.
    assert_eq!(pp("#define f(x) x\nf\n+ 1"), "f + 1");
    // But the `(` may be on the next line: an invocation is not line-oriented.
    assert_eq!(pp("#define f(x) x\nf\n(1)"), "1");
}

#[test]
fn an_argument_may_hold_commas_inside_parentheses() {
    assert_eq!(pp("#define one(x) x\none((1, 2, 3))"), "( 1 , 2 , 3 )");
    assert_eq!(
        pp("#define call(f, a) f(a)\ncall(g, (1, 2))"),
        "g ( ( 1 , 2 ) )"
    );
}

#[test]
fn an_argument_may_be_empty() {
    assert_eq!(pp("#define two(a, b) [a][b]\ntwo(,)"), "[ ] [ ]");
    assert_eq!(pp("#define two(a, b) [a][b]\ntwo(1,)"), "[ 1 ] [ ]");
    // A macro with no parameters takes no argument, not one empty one.
    assert_eq!(pp("#define none() ok\nnone()"), "ok");
}

#[test]
fn arguments_are_expanded_before_they_are_substituted() {
    assert_eq!(pp("#define A 1\n#define f(x) (x + x)\nf(A)"), "( 1 + 1 )");
    // …but not when `#` or `##` gets them.
    assert_eq!(pp("#define A 1\n#define s(x) #x\ns(A)"), "\"A\"");
    assert_eq!(pp("#define A 1\n#define c(x) x ## _\nc(A)"), "A_");
}

#[test]
fn a_macro_may_expand_to_its_own_name() {
    // Prosser's blue paint: `foo` comes out of `foo`, so it is never replaced
    // again, however many times the result is rescanned.
    assert_eq!(pp("#define foo foo\nfoo"), "foo");
    assert_eq!(pp("#define a b\n#define b a\na b"), "a b");
    assert_eq!(
        pp("#define f(x) f(2*(x))\nf(f(1))"),
        "f ( 2 * ( f ( 2 * ( 1 ) ) ) )"
    );
}

#[test]
fn an_argument_may_span_several_lines() {
    assert_eq!(
        pp("#define sum(a, b) ((a) + (b))\nsum(1,\n    2)"),
        "( ( 1 ) + ( 2 ) )"
    );
}

#[test]
fn a_variadic_macro_may_receive_nothing() {
    assert_eq!(
        pp("#define log(fmt, ...) printf(fmt, __VA_ARGS__)\nlog(\"hi\")"),
        "printf ( \"hi\" , )"
    );
    assert_eq!(
        pp("#define log(fmt, ...) printf(fmt, __VA_ARGS__)\nlog(\"%d\", 1, 2)"),
        "printf ( \"%d\" , 1 , 2 )"
    );
}

// ---------------------------------------------------------------------------
// # and ##
// ---------------------------------------------------------------------------

#[test]
fn stringification_normalises_white_space() {
    assert_eq!(pp("#define s(x) #x\ns(  a   +   b  )"), "\"a + b\"");
    assert_eq!(pp("#define s(x) #x\ns(a/*c*/b)"), "\"a b\"");
    assert_eq!(pp("#define s(x) #x\ns()"), "\"\"");
}

#[test]
fn the_hash_hash_rule_accepts_two_separate_hashes() {
    // Rust's own lexer refuses `##` in raw-token mode, so `a # # b` has to
    // mean the pasting operator — and it means it in string-literal mode too.
    assert_eq!(pp("#define cat(a, b) a # # b\ncat(x, 1)"), "x1");
    assert_eq!(pp("#define cat(a, b) a ## b\ncat(x, 1)"), "x1");
    // A single `#` before a parameter is still stringification.
    assert_eq!(pp("#define both(a, b) #a a # # b\nboth(x, y)"), "\"x\" xy");
}

#[test]
fn pasting_builds_new_tokens() {
    assert_eq!(
        pp("#define j(a, b) a ## b\nj(1, 2) j(x, y) j(+, +) j(0x, 1f)"),
        "12 xy ++ 0x1f"
    );
}

#[test]
fn an_invalid_paste_is_an_error_at_the_invocation() {
    one_error(
        "#define j(a, b) a ## b\nint x = j(+, 1);",
        "pasting '+' and '1' does not give a valid token",
    );
}

// ---------------------------------------------------------------------------
// conditionals
// ---------------------------------------------------------------------------

#[test]
fn a_skipped_group_may_hold_anything_at_all() {
    // Not C, not even lexable C — and that has to be fine, because this is
    // exactly what `#if 0` is for.
    assert_eq!(
        pp("#if 0\nthis is not C: 08 'unterminated @@@ \"also\n#endif\nint x;"),
        "int x ;"
    );
}

#[test]
fn conditionals_nest() {
    let source = r"
#define A 1
#if A
outer
#if 0
#if 1
never
#endif
also never: 08
#else
inner else
#endif
#endif
";
    assert_eq!(pp(source), "outer inner else");
}

#[test]
fn an_elif_chain_takes_exactly_one_branch() {
    let base = r"
#define V 2
#if V == 1
one
#elif V == 2
two
#elif V == 2
two again
#else
other
#endif
";
    assert_eq!(pp(base), "two");
    assert_eq!(pp(&base.replace("#define V 2", "#define V 9")), "other");
}

#[test]
fn ifdef_and_ifndef_and_defined_agree() {
    let source = r"
#define X
#ifdef X
a
#endif
#ifndef X
b
#endif
#if defined X
c
#endif
#if defined(X)
d
#endif
#if !defined(Y) && !defined Y
e
#endif
#undef X
#ifdef X
f
#endif
";
    assert_eq!(pp(source), "a c d e");
}

#[test]
fn an_undefined_identifier_is_zero_in_an_if() {
    // 6.10.1p4, `true` and `false` included: C99 has no keywords for them.
    assert_eq!(pp("#if UNDEFINED\nno\n#endif\nyes"), "yes");
    assert_eq!(pp("#if true\nno\n#endif\nyes"), "yes");
    assert_eq!(pp("#if !false\nyes\n#endif"), "yes");
}

// ---------------------------------------------------------------------------
// #if arithmetic
// ---------------------------------------------------------------------------

/// `#if <expr>` with `yes` in the taken branch, so that the answer is a string.
#[track_caller]
fn cond(expr: &str) -> bool {
    let source = format!("#if {expr}\nyes\n#endif\n");
    pp(&source) == "yes"
}

#[test]
fn if_arithmetic_covers_every_operator() {
    assert!(cond("1 + 2 * 3 == 7"));
    assert!(cond("(1 + 2) * 3 == 9"));
    assert!(cond("7 / 2 == 3 && 7 % 2 == 1"));
    assert!(cond("-7 / 2 == -3 && -7 % 2 == -1"));
    assert!(cond("(1 << 4) == 16 && (16 >> 2) == 4"));
    assert!(cond("(0xf0 | 0x0f) == 0xff && (0xff & 0x0f) == 0x0f"));
    assert!(cond("(0xff ^ 0x0f) == 0xf0 && ~0 == -1"));
    assert!(cond("1 < 2 && 2 <= 2 && 3 > 2 && 3 >= 3 && 1 != 2"));
    assert!(cond("(1 ? 2 : 3) == 2 && (0 ? 2 : 3) == 3"));
    assert!(cond("(1, 2, 3) == 3"));
    assert!(cond("'A' == 65 && '\\n' == 10"));
    assert!(cond("0x7fffffffffffffff > 0"));
    assert!(!cond("0"));
}

#[test]
fn if_arithmetic_follows_the_unsigned_rule() {
    // If either operand is unsigned, both are — so `-1` becomes a very large
    // number, and this is *not* true.
    assert!(!cond("-1 < 1u"));
    assert!(cond("-1 < 1"));
    assert!(cond("-1 > 0u"));
    // A decimal constant too large for `intmax_t` is `uintmax_t`.
    assert!(cond("18446744073709551615 > 0"));
    assert!(cond("-1 == 18446744073709551615u"));
    // Signed arithmetic wraps at 64 bits, the width of `intmax_t`.
    assert!(cond("9223372036854775807 + 1 < 0"));
}

#[test]
fn an_unevaluated_operand_is_not_evaluated() {
    // The classic idiom: the division must not be attempted when the macro is
    // not defined, so short-circuiting is not an optimisation but a rule.
    assert_eq!(pp("#if defined(N) && 10 / N\nno\n#endif\nyes"), "yes");
    assert_eq!(pp("#if 1 || 1 / 0\nyes\n#endif"), "yes");
    assert_eq!(pp("#if 0 ? 1 / 0 : 1\nyes\n#endif"), "yes");
}

#[test]
fn division_by_zero_is_an_error() {
    one_error("#if 1 / 0\n#endif\n", "division by zero");
    one_error("#if 1 % 0\n#endif\n", "division by zero");
}

#[test]
fn a_float_or_a_string_is_not_a_preprocessor_value() {
    one_error("#if 1.5\n#endif\n", "floating constant is not allowed");
    one_error("#if \"x\"\n#endif\n", "string literal is not allowed");
}

#[test]
fn a_malformed_if_expression_is_an_error() {
    one_error("#if\n#endif\n", "#if with no expression");
    one_error("#if (1\n#endif\n", "expected ')'");
    one_error("#if 1 2\n#endif\n", "unexpected integer constant");
    one_error("#if 1 +\n#endif\n", "expected a value");
    one_error("#if 1 ? 2\n#endif\n", "expected ':'");
}

#[test]
fn macros_are_expanded_inside_an_if() {
    assert_eq!(pp("#define N 3\n#if N * 2 == 6\nyes\n#endif"), "yes");
    // `defined` is resolved before replacement, so `N` is never looked at as a
    // value here.
    assert_eq!(pp("#define N 0\n#if defined N\nyes\n#endif"), "yes");
}

// ---------------------------------------------------------------------------
// predefined macros
// ---------------------------------------------------------------------------

#[test]
fn the_standard_macros_are_defined() {
    assert_eq!(
        pp("__STDC__ __STDC_HOSTED__ __STDC_VERSION__ __cinrs__"),
        "1 1 199901L 1"
    );
    assert_eq!(pp("__DATE__ __TIME__"), "\"??? ?? ????\" \"??:??:??\"");
    assert!(cond("defined(__cinrs__) && __STDC_VERSION__ >= 199901L"));
}

#[test]
fn line_counts_lines_of_the_c_text_when_nothing_better_is_known() {
    assert_eq!(pp("__LINE__\n__LINE__\n\n__LINE__"), "1 2 4");
    // Inside a macro it is the line of the invocation, not of the `#define`.
    assert_eq!(pp("#define HERE __LINE__\n\n\nHERE"), "4");
}

#[test]
fn line_counts_lines_of_the_rs_file_when_the_map_knows_it() {
    // Capture records which line of the `.rs` file the C text starts on; this
    // is what makes `__LINE__` a number the user can find in their editor.
    let src = "__LINE__\n__LINE__";
    let ctx = Context {
        file_name: "src/lib.rs".to_owned(),
        first_line: 40,
        ..Context::new(src, 0)
    };
    assert_eq!(run_in(src, ctx).0, ["40", "41"]);
}

/// C99 6.10.4. Every expectation here is what `gcc -E -P` prints for the same
/// text.
#[test]
fn line_renumbers_the_lines_that_follow_it() {
    assert_eq!(pp("__LINE__\n#line 100\n__LINE__\n__LINE__"), "1 100 101");
    // The name comes with it, and stays until the next directive.
    assert_eq!(
        pp("#line 100 \"foo.c\"\n__LINE__ __FILE__\n__LINE__ __FILE__"),
        "100 \"foo.c\" 101 \"foo.c\""
    );
    // `#line N` on its own keeps whichever name is in force, including one an
    // earlier `#line` gave.
    assert_eq!(
        pp("#line 10 \"a.c\"\n#line 20\n__LINE__ __FILE__"),
        "20 \"a.c\""
    );
    // A digit sequence is not an integer constant: `010` is ten.
    assert_eq!(pp("#line 010\n__LINE__"), "10");
    // `__FILE_NAME__` is `__FILE__` without the directory, so it follows too;
    // `__BASE_FILE__` names the file the translation unit started in and does
    // not.
    assert_eq!(
        pp("#line 1 \"dir/gen.c\"\n__FILE_NAME__ __BASE_FILE__"),
        "\"gen.c\" \"<c99!>\""
    );
    // The line the directive is written on still counts the old way, which is
    // what makes `#line` renumber *the lines that follow*.
    assert_eq!(pp("#define AT __LINE__\n#line 50\nAT"), "50");
}

/// GCC writes `# 42 "file.h" 1 3 4` where it would have written `#line`; the
/// flags describe an `#include` that happened in the compiler that produced the
/// text, so they are read and dropped.
#[test]
fn a_gcc_line_marker_renumbers_like_line_does() {
    assert_eq!(
        pp("# 42 \"gen.c\" 1 3 4\n__LINE__ __FILE__"),
        "42 \"gen.c\""
    );
    assert_eq!(pp("# 7\n__LINE__"), "7");
}

#[test]
fn line_takes_the_macro_expanded_form_too() {
    // 6.10.4p5, and c-testsuite's `00152`.
    assert_eq!(pp("#define line 1000\n#line line\n__LINE__"), "1000");
    assert_eq!(
        pp("#define BOTH 5 \"gen.c\"\n#line BOTH\n__LINE__ __FILE__"),
        "5 \"gen.c\""
    );
}

#[test]
fn a_line_directive_that_says_nothing_usable_is_an_error() {
    // 6.10.4p3: the digit sequence names a line between 1 and 2147483647.
    one_error("#line 0", "must be between 1 and 2147483647");
    one_error("#line 2147483648", "must be between 1 and 2147483647");
    one_error("#line 99999999999999999999999", "must be between 1 and");
    one_error("#line", "requires a line number");
    one_error("#line x", "requires a decimal line number");
    one_error("#line 0x10", "requires a decimal line number");
    one_error("#line 1u", "requires a decimal line number");
    one_error("#line 1 x", "must be an ordinary string literal");
}

#[test]
fn line_in_a_header_ends_with_the_header() {
    let (tokens, errors, _) = run_including("#include \"renumbered.h\"\n__FILE__ __LINE__", &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        tokens,
        ["renumbered", "\"generated.c\"", "70", "\"<c99!>\"", "2"]
    );
}

/// The main unit's `__LINE__` is a line of the *`.rs` file*, so that it points
/// where the user is looking. A `#line` replaces that numbering from the next
/// line to the end of the text — which is the whole point of writing one.
#[test]
fn line_replaces_the_rs_line_convention() {
    let src = "__LINE__\n#line 7\n__LINE__\n__LINE__";
    let ctx = Context {
        file_name: "src/lib.rs".to_owned(),
        first_line: 40,
        ..Context::new(src, 0)
    };
    assert_eq!(run_in(src, ctx).0, ["40", "7", "8"]);
}

#[test]
fn file_is_the_rs_path_when_it_is_known() {
    assert_eq!(pp("__FILE__"), "\"<c99!>\"");
    let src = "__FILE__";
    let ctx = Context {
        file_name: "tests/thing.rs".to_owned(),
        ..Context::new(src, 0)
    };
    assert_eq!(run_in(src, ctx).0, ["\"tests/thing.rs\""]);
}

#[test]
fn the_target_is_described_consistently_with_the_target_model() {
    let target = cinrs_core::TargetModel::host();
    let sizeof_long = format!("{}", target.long_bits / 8);
    assert_eq!(
        pp("__SIZEOF_LONG__ __CHAR_BIT__"),
        format!("{sizeof_long} 8")
    );
    assert_eq!(pp("__SIZEOF_POINTER__"), (target.ptr_bits / 8).to_string());
    if target.ptr_bits == 64 && target.long_bits == 64 {
        assert!(cond("defined(__LP64__) && defined(_LP64)"));
    }
    if cfg!(target_os = "linux") {
        assert!(cond("defined(__linux__) && defined(__unix__)"));
    }
    if cfg!(target_arch = "x86_64") {
        assert!(cond("defined(__x86_64__)"));
    }
    // `__GNUC__` is 4.2.1, which is what Clang reports too and for the same
    // reason: it is the version a program's `#if __GNUC__ >= 4` guard is
    // asking about before it uses `__attribute__` or `__builtin_expect`, and
    // those work here. Nothing claims to be Clang, which has extensions of its
    // own this crate does not have.
    assert!(cond(
        "__GNUC__ == 4 && __GNUC_MINOR__ == 2 && __GNUC_PATCHLEVEL__ == 1"
    ));
    assert!(cond("!defined(__clang__)"));
    // The two parts of C11 this crate leaves out say so, which is what makes
    // leaving them out conforming — and atomics and variable length arrays,
    // which it does *not* leave out, deliberately say nothing.
    assert!(cond(
        "defined(__STDC_NO_THREADS__) && defined(__STDC_NO_COMPLEX__)"
    ));
    assert!(cond(
        "!defined(__STDC_NO_ATOMICS__) && !defined(__STDC_NO_VLA__)"
    ));
    // The atomic builtins' own macros, which a program passes to them.
    assert!(cond(
        "__ATOMIC_RELAXED == 0 && __ATOMIC_CONSUME == 1 && __ATOMIC_ACQUIRE == 2 \
         && __ATOMIC_RELEASE == 3 && __ATOMIC_ACQ_REL == 4 && __ATOMIC_SEQ_CST == 5"
    ));
    assert!(cond(
        "__GCC_ATOMIC_INT_LOCK_FREE == 2 && __GCC_ATOMIC_POINTER_LOCK_FREE == 2 \
         && __GCC_ATOMIC_TEST_AND_SET_TRUEVAL == 1"
    ));
    assert!(cond(
        "defined(__GCC_HAVE_SYNC_COMPARE_AND_SWAP_1) \
         && defined(__GCC_HAVE_SYNC_COMPARE_AND_SWAP_4)"
    ));
    // A strict entry point is `-std=c99`, and says so.
    assert!(cond("defined(__STRICT_ANSI__)"));
}

#[test]
fn a_predefined_macro_may_be_redefined_without_complaint() {
    assert_eq!(pp("#define __cinrs__ 2\n__cinrs__"), "2");
    assert_eq!(
        pp("#undef __STDC__\n#ifdef __STDC__\nno\n#endif\nyes"),
        "yes"
    );
}

// ---------------------------------------------------------------------------
// the other directives
// ---------------------------------------------------------------------------

#[test]
fn error_carries_the_rest_of_the_line() {
    one_error(
        "#error unsupported configuration\n",
        "#error unsupported configuration",
    );
    // The raw text, not a rendering of the tokens.
    one_error(
        "#error \"quoted\"  and   spaced\n",
        "#error \"quoted\"  and   spaced",
    );
    one_error("#error\n", "#error");
    // A skipped `#error` is not an error at all.
    assert_eq!(pp("#if 0\n#error never\n#endif\nok"), "ok");
}

#[test]
fn warning_pragma_and_line_produce_no_tokens() {
    assert_eq!(
        pp(
            "#warning careful\n#pragma once\n#pragma GCC diagnostic ignored \"-Wall\"\n#line 42 \"x.c\"\nok"
        ),
        "ok"
    );
}

#[test]
fn the_null_directive_does_nothing() {
    assert_eq!(pp("#\nint x;\n#\n"), "int x ;");
}

#[test]
fn a_bundled_header_needs_no_search_path_at_all() {
    // Nothing about the context says where headers are; the ones cinrs ships
    // are found anyway, because they are inside the crate.
    assert_eq!(pp("#include <stdbool.h>\nbool"), "_Bool");
    one_error(
        "#include \"local.h\"\n",
        "\"local.h\" file not found; searched: <cinrs>",
    );
}

#[test]
fn an_unknown_directive_is_an_error() {
    one_error("#nonsense 1\n", "invalid preprocessing directive #nonsense");
    one_error("#!\n", "invalid preprocessing directive after '#'");
    // A line marker is accepted and ignored, the way `cpp` output is.
    assert_eq!(pp("# 42 \"x.c\"\nok"), "ok");
}

#[test]
fn a_hash_that_does_not_start_a_line_is_not_a_directive() {
    // It is a stray `#`, which the parser will have an opinion about.
    assert_eq!(pp("int x; # define Y 1"), "int x ; # define Y 1");
}

// ---------------------------------------------------------------------------
// errors
// ---------------------------------------------------------------------------

#[test]
fn an_unterminated_conditional_is_an_error() {
    one_error("#if 1\nint x;\n", "unterminated conditional directive");
    one_error("#ifdef X\nint x;\n", "unterminated conditional directive");
}

#[test]
fn a_stray_conditional_directive_is_an_error() {
    one_error("#endif\n", "#endif without #if");
    one_error("#else\n", "#else without #if");
    one_error("#elif 1\n", "#elif without #if");
    one_error("#if 1\n#else\n#else\n#endif\n", "#else after #else");
    one_error("#if 1\n#else\n#elif 1\n#endif\n", "#elif after #else");
}

#[test]
fn a_wrong_argument_count_is_an_error_at_the_invocation() {
    one_error(
        "#define two(a, b) a + b\nint x = 1; two(1);",
        "macro 'two' requires 2 arguments, but only 1 given",
    );
    one_error(
        "#define two(a, b) a + b\nint x = 1; two(1, 2, 3);",
        "macro 'two' passed 3 arguments, but takes just 2",
    );
    one_error(
        "#define va(a, b, ...) a\nint x = 1; va(1);",
        "macro 'va' requires at least 2 arguments, but only 1 given",
    );
    // `...` matching nothing is deliberately allowed; see the module docs.
    assert_eq!(pp("#define va(a, ...) a\nva(1)"), "1");
    one_error(
        "#define one(a) a\nint x = one(1",
        "unterminated argument list",
    );
}

#[test]
fn the_constraints_on_a_replacement_list_are_enforced() {
    one_error("#define a ## b\n", "'##' cannot appear at the start");
    one_error("#define f(x) x ##\n", "'##' cannot appear at the end");
    one_error(
        "#define f(x) # y\n",
        "'#' must be followed by a macro parameter",
    );
    // In an object-like macro `#` is just a token.
    assert_eq!(pp("#define h # y\nh"), "# y");
}

#[test]
fn defined_and_va_args_are_protected() {
    one_error(
        "#define defined 1\n",
        "'defined' cannot be used as a macro name",
    );
    one_error(
        "#undef defined\n",
        "'defined' cannot be used as a macro name",
    );
    one_error(
        "#define __VA_ARGS__ 1\n",
        "'__VA_ARGS__' can only appear in the replacement list of a variadic macro",
    );
    one_error(
        "#define f(x) __VA_ARGS__\n",
        "'__VA_ARGS__' can only appear in the replacement list of a variadic macro",
    );
    one_error(
        "#define g __VA_ARGS__\n",
        "'__VA_ARGS__' can only appear in the replacement list of a variadic macro",
    );
    one_error(
        "#define h(__VA_ARGS__) 1\n",
        "'__VA_ARGS__' cannot be used as a macro parameter name",
    );
}

#[test]
fn a_malformed_define_is_an_error() {
    one_error("#define\n", "no macro name given in #define");
    one_error("#define 1 2\n", "macro name must be an identifier");
    one_error("#define f(a, a) a\n", "duplicate macro parameter 'a'");
    one_error("#define f(a\n", "missing ')' in the parameter list");
    one_error("#define f(a b) a\n", "expected ',' or ')'");
    one_error("#undef\n", "no macro name given in #undef");
}

#[test]
fn a_lexical_error_is_reported_only_where_it_survives() {
    // The same bad constant, once skipped and once not.
    assert!(errors("#if 0\nint x = 08;\n#endif\n").is_empty());
    one_error("int x = 08;\n", "invalid digit '8' in octal constant '08'");
    // Inside a macro that is never invoked, it is not there either.
    assert!(errors("#define UNUSED 08\nint x;").is_empty());
    one_error("#define USED 08\nint x = USED;", "invalid digit '8'");
    // Used twice, reported once.
    one_error(
        "#define USED 08\nint x = USED, y = USED;",
        "invalid digit '8'",
    );
}

#[test]
fn a_keyword_may_be_a_macro_name() {
    // Keywords are ordinary identifiers in translation phase 4, and
    // `#define restrict` is what a header for an older compiler does.
    assert_eq!(pp("#define restrict\nint *restrict p;"), "int * p ;");
    // `__inline` is the GNU spelling of `inline`, and the preprocessor turns
    // it into a keyword on the way to the parser — after the macro has been
    // replaced, which is what keeps `#define __inline` and `#ifdef __restrict`
    // about the names that were written. It is a keyword of its own rather
    // than `inline` itself, because `c89!` gates the plain spelling and not
    // the reserved one.
    assert_eq!(
        pp("#define inline __inline\ninline int f();"),
        "__inline__ int f ( ) ;"
    );
    assert_eq!(pp("#define __restrict\nint *__restrict p;"), "int * p ;");
    assert_eq!(pp("#ifdef int\nno\n#endif\nyes"), "yes");
}

// ---------------------------------------------------------------------------
// robustness
// ---------------------------------------------------------------------------

#[test]
fn malformed_directives_always_terminate() {
    // None of these may hang, panic or recurse away; each is allowed to be
    // an error, and each has to *finish*.
    let cases = [
        "#",
        "#define",
        "#define f(",
        "#define f(x",
        "#define f(x) #",
        "#if",
        "#if (((",
        "#if )))",
        "#if 1 ? ? :",
        "#endif",
        "#else",
        "#elif",
        "#ifdef",
        "#ifndef",
        "#undef",
        "#include",
        "#error",
        "#pragma",
        "#line",
        "#if 0\n#if 1\n",
        "#define A A\nA",
        "#define A B\n#define B A\nA B",
        "#define f(x) f(x)\nf(1)",
        "#define f(x) x\nf(",
        "#define f(x) x\nf((((",
        "#define f(x) x ## x\nf(#)",
        "#define f(...) __VA_ARGS__\nf(",
    ];
    for case in cases {
        let (tokens, errors) = run(case);
        // Something must come of it — output, a diagnostic, or a clean nothing.
        let _ = (tokens, errors);
    }
}

#[test]
fn deeply_nested_macro_arguments_become_a_diagnostic() {
    // Argument pre-expansion recurses, and a procedural macro that overflows
    // the stack takes the whole compiler down with no message at all.
    let depth = 1_000;
    let source = format!(
        "#define f(x) (x)\n{}1{}",
        "f(".repeat(depth),
        ")".repeat(depth)
    );
    let errors = errors(&source);
    assert!(
        errors.iter().any(|e| e.contains("nest too deeply")),
        "{errors:#?}"
    );
}

#[test]
fn a_deeply_nested_if_expression_becomes_a_diagnostic() {
    let depth = 5_000;
    let source = format!("#if {}1{}\n#endif\n", "(".repeat(depth), ")".repeat(depth));
    let errors = errors(&source);
    assert!(
        errors.iter().any(|e| e.contains("nests too deeply")),
        "{errors:#?}"
    );
}

#[test]
fn realistically_nested_macros_are_accepted() {
    // The limits must be far above anything real code contains.
    let source = format!("#define f(x) (x)\n{}1{}", "f(".repeat(40), ")".repeat(40));
    assert_eq!(pp(&source).matches('(').count(), 40);
    let source = format!("#if {}1{}\nyes\n#endif\n", "(".repeat(40), ")".repeat(40));
    assert_eq!(pp(&source), "yes");
}

// ---------------------------------------------------------------------------
// #include
// ---------------------------------------------------------------------------

/// Where the fixtures live, relative to this package's directory — which is
/// the working directory Cargo runs a test binary in.
const HEADERS: &str = "tests/include";
/// A second directory, for the tests of the search order.
const MORE_HEADERS: &str = "tests/include2";

/// Preprocesses `src` as if it were written in a file in `HEADERS`'s parent,
/// with `search` as the configured include directories.
fn run_including(src: &str, search: &[&str]) -> (Vec<String>, Vec<String>, Preprocessed) {
    let mut options = Options::new(Standard::C99);
    options.include_paths = search.iter().map(PathBuf::from).collect();
    let ctx = Context {
        dir: Some(PathBuf::from(HEADERS)),
        ..Context::new(src, 0)
    };
    let mut diags = cinrs_core::Diagnostics::new();
    let tokens = lex_text(src, ctx.base, &lex_options());
    let out = preprocess(&tokens, &ctx, &options, &mut diags);
    let spellings = out
        .tokens
        .iter()
        .filter(|t| !t.is_eof())
        .map(|t| t.kind.spelling().to_owned())
        .collect();
    let errors = diags
        .items()
        .iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| d.message.clone())
        .collect();
    (spellings, errors, out)
}

/// The token spellings `src` preprocesses to, with headers available.
#[track_caller]
fn pp_including(src: &str, search: &[&str]) -> String {
    let (tokens, errors, _) = run_including(src, search);
    assert!(errors.is_empty(), "unexpected errors: {errors:#?}");
    tokens.join(" ")
}

#[test]
fn a_quoted_header_is_looked_for_next_to_the_including_file_first() {
    // The including file's own directory wins over the search path...
    assert_eq!(
        pp_including("#include \"order.h\"", &[MORE_HEADERS]),
        "from_the_including_directory"
    );
    // ... and an angled include does not look there at all.
    assert_eq!(
        pp_including("#include <order.h>", &[MORE_HEADERS]),
        "from_the_search_path"
    );
}

#[test]
fn a_configured_directory_is_searched_before_the_bundled_headers() {
    assert_eq!(
        pp_including("#include <stdbool.h>", &[MORE_HEADERS]),
        "shadowing_stdbool"
    );
    // Without it, the bundled one is what `<stdbool.h>` means.
    assert_eq!(
        pp_including("#include <stdbool.h>\n__bool_true_false_are_defined", &[]),
        "1"
    );
}

#[test]
fn an_include_guard_stops_the_file_being_read_again() {
    let (tokens, errors, out) = run_including(
        "#include \"guarded.h\"\n#include \"guarded.h\"\n#include \"guarded.h\"",
        &[],
    );
    assert!(errors.is_empty(), "{errors:#?}");
    // Once, not three times — and the file was only opened once.
    assert_eq!(tokens, ["int", "guarded", ";"]);
    assert_eq!(out.included.len(), 1);
    assert_eq!(out.included[0].name, "tests/include/guarded.h");
}

#[test]
fn pragma_once_stops_the_file_being_read_again() {
    let (tokens, errors, out) = run_including("#include \"once.h\"\n#include \"once.h\"", &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(tokens, ["int", "once", ";"]);
    // Noticed on the way through, and remembered before the second directive
    // gets as far as opening anything.
    assert_eq!(out.included.len(), 1);
}

#[test]
fn a_header_includes_its_neighbours_by_their_bare_names() {
    assert_eq!(
        pp_including("#include \"nested.h\"", &[]),
        "int guarded ; int nested ;"
    );
}

#[test]
fn file_and_line_inside_a_header_name_the_header() {
    let (tokens, errors, _) = run_including("#include \"where.h\"\n__FILE__ __LINE__", &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        tokens,
        ["where", "\"tests/include/where.h\"", "3", "\"<c99!>\"", "2"]
    );
}

#[test]
fn a_missing_header_says_where_it_looked() {
    let (_, errors, _) = run_including("#include \"nowhere.h\"", &[MORE_HEADERS]);
    assert_eq!(
        errors,
        ["\"nowhere.h\" file not found; searched: tests/include, tests/include2, <cinrs>"]
    );
    let (_, errors, _) = run_including("#include <nowhere.h>", &[]);
    assert_eq!(errors, ["<nowhere.h> file not found; searched: <cinrs>"]);
}

#[test]
fn a_header_may_include_itself_by_file_macro() {
    // `#include __FILE__`, guarded by `__COUNTER__`: the name is the path the
    // header was found at, relative to the working directory, and the
    // directive is written inside that directory — so it resolves only
    // because `include::resolve` also looks for a path-shaped quoted name
    // from the working directory. Clang's `C99/n590.c` reaches its fifteen
    // levels of nested `#include` exactly this way.
    let (tokens, errors, _) = run_including("#include \"file_macro.h\"", &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(tokens, ["level"; 4]);
}

#[test]
fn a_header_that_includes_itself_hits_the_depth_limit() {
    // Written to a temporary file so that the fixtures stay readable.
    let dir = std::env::temp_dir().join(format!("cinrs-pp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the temporary directory must be creatable");
    let path = dir.join("recursive.h");
    std::fs::write(&path, "#include \"recursive.h\"\n").expect("the header must be writable");

    let (_, errors, _) = run_including("#include \"recursive.h\"", &[&dir.display().to_string()]);
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].starts_with("#include nested too deeply"),
        "{errors:#?}"
    );
}

#[test]
fn the_header_name_may_be_produced_by_a_macro() {
    assert_eq!(
        pp_including("#define H \"guarded.h\"\n#include H", &[]),
        "int guarded ;"
    );
    assert_eq!(
        pp_including("#define H <order.h>\n#include H", &[MORE_HEADERS]),
        "from_the_search_path"
    );
}

#[test]
fn a_user_header_is_recorded_for_rebuild_tracking() {
    let (_, errors, out) = run_including("#include \"nested.h\"\n#include <stdbool.h>", &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    let names: Vec<String> = out
        .user_headers
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    // Both files that were read, and nothing for the bundled header.
    assert_eq!(names, ["nested.h", "guarded.h"]);
    assert!(out.user_headers.iter().all(|p| p.is_absolute()));
}

#[test]
fn the_cinrs_pragmas_configure_the_unit() {
    let (tokens, errors, out) = run_including(
        "#pragma cinrs link \"m\"\n#pragma cinrs link \"m\"\n#pragma cinrs link \"z\"\nkept",
        &[],
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(tokens, ["kept"]);
    assert_eq!(out.link_libraries, ["m", "z"]);

    // An option that does not exist is a mistake, and every other pragma is
    // still ignored.
    let (_, errors, _) = run_including("#pragma cinrs frobnicate \"x\"\n#pragma other", &[]);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].starts_with("unknown #pragma cinrs option"),
        "{errors:#?}"
    );
}

#[test]
fn the_export_pragma_is_unit_wide_and_takes_no_argument() {
    let (tokens, errors, out) = run_including("#pragma cinrs export\nkept", &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(tokens, ["kept"]);
    assert!(out.export);

    // It says nothing about *what* is exported, so an argument is a mistake.
    let (_, errors, out) = run_including("#pragma cinrs export all", &[]);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].starts_with("unexpected identifier 'all' after #pragma cinrs export"),
        "{errors:#?}"
    );
    // Reported, but still honoured: the unit did ask.
    assert!(out.export);

    let (_, _, out) = run_including("kept", &[]);
    assert!(!out.export);
}

#[test]
fn the_no_std_pragma_is_unit_wide_and_takes_no_argument() {
    let (tokens, errors, out) = run_including("#pragma cinrs no_std\nkept", &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(tokens, ["kept"]);
    assert!(out.no_std);

    let (_, errors, out) = run_including("#pragma cinrs no_std yes", &[]);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].starts_with("unexpected identifier 'yes' after #pragma cinrs no_std"),
        "{errors:#?}"
    );
    assert!(out.no_std);

    let (_, _, out) = run_including("kept", &[]);
    assert!(!out.no_std);
}

#[test]
fn the_module_pragma_names_the_unit() {
    let (tokens, errors, out) = run_including("#pragma cinrs module \"geometry\"\nkept", &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(tokens, ["kept"]);
    assert_eq!(out.module.as_deref(), Some("geometry"));

    // The name becomes a Rust identifier, so it has to be one.
    for bad in ["two words", "1st", "struct", "self", "e\u{301}"] {
        let (_, errors, out) = run_including(&format!("#pragma cinrs module \"{bad}\""), &[]);
        assert_eq!(errors.len(), 1, "{bad}: {errors:#?}");
        assert!(
            errors[0].ends_with("is not usable as a Rust module name"),
            "{bad}: {errors:#?}"
        );
        assert_eq!(out.module, None, "{bad}");
    }

    // Saying the same name twice is harmless; saying two is a mistake.
    let (_, errors, out) = run_including(
        "#pragma cinrs module \"a\"\n#pragma cinrs module \"a\"",
        &[],
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(out.module.as_deref(), Some("a"));

    let (_, errors, out) = run_including(
        "#pragma cinrs module \"a\"\n#pragma cinrs module \"b\"",
        &[],
    );
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].starts_with("this unit is already named 'a'"),
        "{errors:#?}"
    );
    assert_eq!(out.module.as_deref(), Some("a"));
}

#[test]
fn an_include_inside_a_skipped_group_is_not_read() {
    let (tokens, errors, out) = run_including("#if 0\n#include \"nowhere.h\"\n#endif\nkept", &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(tokens, ["kept"]);
    assert!(out.included.is_empty());
}

#[test]
fn a_conditional_left_open_by_a_header_is_reported_against_it() {
    let dir = std::env::temp_dir().join(format!("cinrs-pp-open-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the temporary directory must be creatable");
    std::fs::write(dir.join("open.h"), "#if 1\nfrom_header\n").expect("writable");

    let (tokens, errors, _) =
        run_including("#include \"open.h\"\nafter", &[&dir.display().to_string()]);
    std::fs::remove_dir_all(&dir).ok();

    // The group ends with the file, so what follows the directive is not
    // swallowed by it.
    assert_eq!(tokens, ["from_header", "after"]);
    assert_eq!(errors, ["unterminated conditional directive"]);
}

#[test]
fn a_header_that_cannot_be_read_is_a_diagnostic_rather_than_a_panic() {
    let dir = std::env::temp_dir().join(format!("cinrs-pp-unreadable-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the temporary directory must be creatable");
    // Not UTF-8, which is the failure a real header can plausibly have.
    std::fs::write(dir.join("binary.h"), [0xffu8, 0xfe, 0x00, 0x41]).expect("writable");
    // A *directory* of the right name is not a header either, and must not
    // stop the search.
    std::fs::create_dir_all(dir.join("shadow.h")).expect("creatable");

    let path = dir.display().to_string();
    let (_, errors, _) = run_including("#include <binary.h>", &[&path]);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(errors[0].starts_with("cannot read '"), "{errors:#?}");

    let (_, errors, _) = run_including("#include <shadow.h>", &[&path]);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].starts_with("<shadow.h> file not found"),
        "{errors:#?}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// C23
// ---------------------------------------------------------------------------

/// Preprocesses `src` as a `c23!` block would.
fn run_c23(src: &str) -> (Vec<String>, Vec<String>) {
    let options = Options::new(Standard::C23);
    let ctx = Context::new(src, 0);
    let mut diags = cinrs_core::Diagnostics::new();
    let tokens = lex_text(src, ctx.base, &LexOptions::new(Standard::C23));
    let out = preprocess(&tokens, &ctx, &options, &mut diags);
    let spellings = out
        .tokens
        .iter()
        .filter(|t| !t.is_eof())
        .map(|t| t.kind.spelling().to_owned())
        .collect();
    let errors = diags
        .items()
        .iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| d.message.clone())
        .collect();
    (spellings, errors)
}

/// The token spellings `src` preprocesses to in a `c23!` block.
#[track_caller]
fn pp23(src: &str) -> String {
    let (tokens, errors) = run_c23(src);
    assert!(
        errors.is_empty(),
        "unexpected errors for {src:?}: {errors:#?}"
    );
    tokens.join(" ")
}

#[test]
fn va_opt_drops_the_comma_when_there_are_no_variable_arguments() {
    let source = r#"
#define LOG(fmt, ...) log(fmt __VA_OPT__(,) __VA_ARGS__)
LOG("plain");
LOG("n=%d", 7);
"#;
    assert_eq!(pp23(source), r#"log ( "plain" ) ; log ( "n=%d" , 7 ) ;"#);
    // The contents are ordinary replacement-list tokens: parameters inside
    // them are substituted, and `##` next to them still pastes.
    assert_eq!(
        pp23("#define F(a, ...) [a __VA_OPT__(: __VA_ARGS__)]\nF(1) F(1, 2, 3)"),
        "[ 1 ] [ 1 : 2 , 3 ]"
    );
    // `##` *inside* the contents pastes as it would anywhere else in a
    // replacement list; only one at the very start or the very end of them is
    // a constraint violation (C23 6.10.5.2p1).
    assert_eq!(
        pp23("#define G(a, ...) x __VA_OPT__(a ## 1)\nG(p) G(p, q)"),
        "x x p1"
    );
}

#[test]
fn va_opt_is_checked_and_gated() {
    let (_, errors) = run_c23("#define F(a) __VA_OPT__(x)\nF(1)");
    assert_eq!(
        errors,
        ["'__VA_OPT__' can only appear in the replacement list of a variadic macro"]
    );
    let (_, errors) = run_c23("#define F(...) __VA_OPT__ x\nF(1)");
    assert_eq!(errors, ["'__VA_OPT__' must be followed by '('"]);
    let (_, errors) = run_c23("#define F(...) __VA_OPT__(__VA_OPT__(x))\nF(1)");
    assert_eq!(errors, ["'__VA_OPT__' cannot be nested inside another"]);
    // C23 6.10.5.2p1: the token sequence of the argument may neither begin nor
    // end with `##`, for the same reason a replacement list may not — there is
    // nothing on that side of it to paste to. Clang's `C23/n3033_2.c` is the
    // first of these.
    let (_, errors) = run_c23("#define F(X, ...) X __VA_OPT__(##) __VA_ARGS__\nF(1, 2)");
    assert_eq!(
        errors,
        ["'##' cannot appear at the start of a '__VA_OPT__' argument"]
    );
    let (_, errors) = run_c23("#define F(X, ...) X __VA_OPT__(X ##) __VA_ARGS__\nF(1, 2)");
    assert_eq!(
        errors,
        ["'##' cannot appear at the end of a '__VA_OPT__' argument"]
    );
    // Before C23 it is not there at all.
    one_error(
        "#define F(...) f(__VA_OPT__(x))\nF(1)",
        "'__VA_OPT__' requires C23 or later (this block is c99!)",
    );
}

#[test]
fn elifdef_and_elifndef_test_definedness() {
    let source = r"
#define B 1
#ifdef A
a
#elifdef B
b
#elifndef C
c
#else
d
#endif
";
    assert_eq!(pp23(source), "b");
    assert_eq!(pp23("#ifdef A\na\n#elifndef B\nb\n#endif"), "b");
    one_error(
        "#ifdef A\na\n#elifdef B\nb\n#endif",
        "'#elifdef' requires C23 or later (this block is c99!)",
    );
    let (_, errors) = run_c23("#elifdef A\n#endif");
    assert_eq!(errors, ["#elifdef without #if", "#endif without #if"]);
}

#[test]
fn true_and_false_are_one_and_zero_in_a_c23_condition() {
    assert_eq!(pp23("#if true\nyes\n#endif"), "yes");
    assert_eq!(pp23("#if false\nno\n#endif"), "");
    assert_eq!(pp23("#if 0b1011 == 11\nbinary\n#endif"), "binary");
    // Before C23 they are ordinary identifiers, which 6.10.1p4 turns into 0.
    assert_eq!(pp("#if true\nyes\n#endif"), "");
}

#[test]
fn has_include_answers_from_the_search_path() {
    let (tokens, errors) = run_c23("#if __has_include(<stdio.h>)\nyes\n#endif");
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(tokens.join(" "), "yes");
    let (tokens, errors) = run_c23("#if __has_include(<nowhere.h>)\nyes\n#endif");
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(tokens.join(" "), "");
}

#[test]
fn the_has_family_answers_from_this_implementations_tables() {
    // `packed` and `cleanup` are honoured, `vector_size` is refused, and the
    // answers say so.
    assert_eq!(pp("#if __has_attribute(packed)\n1\n#endif"), "1");
    assert_eq!(pp("#if __has_attribute(__packed__)\n1\n#endif"), "1");
    assert_eq!(pp("#if __has_attribute(cleanup)\n1\n#endif"), "1");
    assert_eq!(pp("#if __has_attribute(vector_size)\n1\n#endif"), "");
    assert_eq!(pp("#if __has_attribute(no_such_thing)\n1\n#endif"), "");
    assert_eq!(pp("#if __has_builtin(__builtin_popcount)\n1\n#endif"), "1");
    assert_eq!(pp("#if __has_builtin(__builtin_apply)\n1\n#endif"), "");
    // The atomic builtins are builtins under names of their own, and
    // `__has_builtin` answers about all three families.
    assert_eq!(pp("#if __has_builtin(__atomic_load_n)\n1\n#endif"), "1");
    assert_eq!(pp("#if __has_builtin(__sync_synchronize)\n1\n#endif"), "1");
    assert_eq!(pp("#if __has_builtin(__c11_atomic_load)\n1\n#endif"), "1");
    assert_eq!(pp("#if __has_builtin(__atomic_no_such)\n1\n#endif"), "");
    assert_eq!(pp("#if __has_feature(c_static_assert)\n1\n#endif"), "1");
    assert_eq!(pp("#if __has_feature(c_atomic)\n1\n#endif"), "1");
    assert_eq!(pp("#if __has_c_attribute(fallthrough)\n1\n#endif"), "1");
    assert_eq!(pp("#if __has_c_attribute(packed)\n1\n#endif"), "");
}

// ---------------------------------------------------------------------------
// #embed (C23 6.10.3)
// ---------------------------------------------------------------------------

/// Preprocesses `src` in `standard`, with `HEADERS` as the origin *and* the
/// include path, so that both `#embed "x"` and `#embed <x>` find the fixtures.
fn run_embedding(standard: Standard, src: &str) -> (Vec<String>, Vec<String>, Preprocessed) {
    let mut options = Options::new(standard);
    options.include_paths = vec![PathBuf::from(HEADERS)];
    let ctx = Context {
        dir: Some(PathBuf::from(HEADERS)),
        ..Context::new(src, 0)
    };
    let mut diags = cinrs_core::Diagnostics::new();
    let tokens = lex_text(src, ctx.base, &LexOptions::new(standard));
    let out = preprocess(&tokens, &ctx, &options, &mut diags);
    let spellings = out
        .tokens
        .iter()
        .filter(|t| !t.is_eof())
        .map(|t| t.kind.spelling().to_owned())
        .collect();
    let errors = diags
        .items()
        .iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| d.message.clone())
        .collect();
    (spellings, errors, out)
}

/// The token spellings `src` preprocesses to in a `c23!` block that can embed.
#[track_caller]
fn pp_embedding(src: &str) -> String {
    let (tokens, errors, _) = run_embedding(Standard::C23, src);
    assert!(errors.is_empty(), "unexpected errors: {errors:#?}");
    tokens.join(" ")
}

#[test]
fn embed_expands_to_the_bytes_of_the_resource() {
    // `data.bin` is a PNG signature followed by a NUL and a `0xff`.
    assert_eq!(
        pp_embedding("#embed \"data.bin\""),
        "137 , 80 , 78 , 71 , 13 , 10 , 26 , 10 , 0 , 255"
    );
    // The angled form takes the include path.
    assert_eq!(pp_embedding("#embed <data.bin> limit(2)"), "137 , 80");
    // A prefix and a suffix bracket a non-empty list...
    assert_eq!(
        pp_embedding("#embed <data.bin> limit(1) prefix(A ,) suffix(, B)"),
        "A , 137 , B"
    );
    // ... and are left out of an empty one, which `if_empty` replaces whole.
    assert_eq!(
        pp_embedding("#embed <empty.bin> prefix(A ,) suffix(, B) if_empty(Z)"),
        "Z"
    );
    assert_eq!(pp_embedding("#embed <empty.bin>"), "");
    // `limit(0)` makes any resource an empty one.
    assert_eq!(pp_embedding("#embed <data.bin> limit(0) if_empty(Z)"), "Z");
    // The reserved spellings GCC gives the parameters work too.
    assert_eq!(pp_embedding("#embed <data.bin> __limit__(1)"), "137");
    // The bytes are rescanned like any other replacement, so a macro in the
    // prefix is expanded.
    assert_eq!(
        pp_embedding("#define TAG 42\n#embed <data.bin> limit(1) prefix(TAG ,)"),
        "42 , 137"
    );
}

#[test]
fn embed_records_the_resource_for_rebuild_tracking() {
    let (_, errors, out) = run_embedding(Standard::C23, "#embed \"data.bin\"");
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(out.embedded_files.len(), 1);
    assert!(
        out.embedded_files[0].ends_with("data.bin"),
        "{:?}",
        out.embedded_files
    );
}

#[test]
fn has_embed_answers_found_empty_or_not_found() {
    // Like `defined` and `__has_include`, it is only an operator inside the
    // controlling expression of a conditional.
    let answer = |operand: &str| {
        pp_embedding(&format!(
            "#if __has_embed({operand}) == 0\nnone\n#elif __has_embed({operand}) == 1\nfound\
             \n#elif __has_embed({operand}) == 2\nempty\n#endif"
        ))
    };
    assert_eq!(answer("<data.bin>"), "found");
    assert_eq!(answer("<empty.bin>"), "empty");
    assert_eq!(answer("<nowhere.bin>"), "none");
    assert_eq!(answer("<data.bin> limit(0)"), "empty");
    // An unknown parameter is 6.10.1p5's "not found".
    assert_eq!(answer("<data.bin> nonsense(1)"), "none");
    // The three macros a program compares against are predefined.
    assert_eq!(
        pp_embedding(
            "#if __STDC_EMBED_NOT_FOUND__ == 0 && __STDC_EMBED_FOUND__ == 1 \
             && __STDC_EMBED_EMPTY__ == 2\nok\n#endif"
        ),
        "ok"
    );
}

#[test]
fn embed_reports_what_it_cannot_do() {
    let (_, errors, _) = run_embedding(Standard::C23, "#embed <nowhere.bin>");
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].starts_with("<nowhere.bin> resource not found for #embed; searched: "),
        "{:?}",
        errors[0]
    );

    let (_, errors, _) = run_embedding(Standard::C23, "#embed");
    assert_eq!(errors, ["#embed expects \"RESOURCE\" or <RESOURCE>"]);

    let (_, errors, _) = run_embedding(Standard::C23, "#embed <data.bin> nonsense(1)");
    assert_eq!(errors, ["unknown #embed parameter 'nonsense'"]);

    let (_, errors, _) = run_embedding(Standard::C23, "#embed <data.bin> limit");
    assert_eq!(errors, ["#embed parameter 'limit' takes an argument list"]);
}

/// `#embed` is C23's; a strict entry point older than that says so, and a GNU
/// one takes it as GCC 15 does.
#[test]
fn embed_is_gated_before_c23() {
    let (_, errors, _) = run_embedding(Standard::C17, "#embed \"data.bin\"");
    assert_eq!(
        errors,
        ["'#embed' requires C23 or later (this block is c17!)"]
    );

    let mut options = Options::with_dialect(Standard::C17, cinrs_core::Dialect::Gnu);
    options.include_paths = vec![PathBuf::from(HEADERS)];
    let src = "#embed \"data.bin\" limit(1)";
    let ctx = Context {
        dir: Some(PathBuf::from(HEADERS)),
        ..Context::new(src, 0)
    };
    let mut diags = cinrs_core::Diagnostics::new();
    let tokens = lex_text(src, ctx.base, &LexOptions::new(Standard::C17));
    let out = preprocess(&tokens, &ctx, &options, &mut diags);
    assert!(
        !diags.items().iter().any(|d| d.level == Level::Error),
        "gnu17! refused #embed: {:#?}",
        diags.items()
    );
    let spellings: Vec<String> = out
        .tokens
        .iter()
        .filter(|t| !t.is_eof())
        .map(|t| t.kind.spelling().to_owned())
        .collect();
    assert_eq!(spellings, ["137"]);
}
