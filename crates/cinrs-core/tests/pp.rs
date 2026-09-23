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

/// Macros for the spacing tests below, which stringify what they expand to.
const SPACING: &str = "\
#define S_(x) #x
#define S(x) S_(x)
#define SV_(...) #__VA_ARGS__
#define SV(...) SV_(__VA_ARGS__)
#define V(a,b) ((a)+(b))
#define F(x) [x]
#define G(x) [ x ]
#define G1(x) [ x]
#define W(x) x
#define N
#define N2()
#define P(a,b) a ## b
#define P2(a,b) [a ## b]
#define P3(a,b) [ a ## b]
#define P7(a,b) [a ##b ]
#define P11(a,b) [ a ## b x]
#define P12(a,b) [ a ## b(x)]
#define P13(a,b) [a ## b ## a]
#define P14(a,b) [a ## b a]
#define Q(a,b) <a b>
#define R(a) [ a]
#define R2(a) [a ]
#define R3(a,b) [ a b]
#define M(x) [ N x]
#define H(x) - x
#define O [ 1 ]
#define K(x) x x
#define T(x) #x x
#define Y(a) x a
#define M3(a,b) a-b
#define E(f, ...) f(0, ## __VA_ARGS__)
#define E3(f, ...) f(0,##__VA_ARGS__)
#define E4(f, ...) f(0 , ##__VA_ARGS__)
";

#[track_caller]
fn spaced(line: &str) -> String {
    pp(&format!("{SPACING}{line}"))
}

#[test]
fn a_substituted_argument_is_spaced_like_its_parameter() {
    // brotli's `BROTLI_MAKE_VERSION(__GNUC__, __GNUC_MINOR__, ...)`, through
    // two macros: the white space before `2` in the invocation is not part
    // of the argument, and `(b)` has none before `b`.
    assert_eq!(spaced("S(V(1, 2))"), r#""((1)+(2))""#);
    // `#` does not expand its operand, and the space inside it stays.
    assert_eq!(spaced("S_(V(1, 2))"), r#""V(1, 2)""#);
    assert_eq!(spaced("SV(a, b)"), r#""a, b""#);
    assert_eq!(spaced("SV_(a,   b)"), r#""a, b""#);
    assert_eq!(spaced("S(F( 1 ))"), r#""[1]""#);
    assert_eq!(spaced("S(G(1))"), r#""[ 1 ]""#);
    assert_eq!(spaced("S(-W( 1))"), r#""-1""#);
    assert_eq!(spaced("S(H(  1))"), r#""- 1""#);
    assert_eq!(spaced("S(W( V(1,2) ))"), r#""((1)+(2))""#);
    // Line breaks and runs of white space inside an argument are one space.
    assert_eq!(spaced("S_(  a\n   +\n     b  )"), r#""a + b""#);
    assert_eq!(spaced("S(Q( 1 ,\n   2 ))"), r#""<1 2>""#);
    // An empty argument.
    assert_eq!(spaced("S(F())"), r#""[]""#);
    assert_eq!(spaced("S(V( , ))"), r#""(()+())""#);
}

#[test]
fn spacing_around_macros_matches_gcc_padding() {
    // Each expectation is what `gcc -E` makes of the line.
    let cases = [
        ("S(P(, x))", r#""x""#),
        ("S(P( x , y ))", r#""xy""#),
        ("S(F( W( 1 ) ))", r#""[1]""#),
        ("S(F(N2()))", r#""[]""#),
        ("S( P(x, ) y)", r#""x y""#),
        ("S(-F(1))", r#""-[1]""#),
        ("S(-W(1))", r#""-1""#),
        ("S(E(SV_,  1,  2))", r#""\"0, 1, 2\"""#),
        ("S(E3(SV_,1,2))", r#""\"0,1,2\"""#),
        ("S(E3(SV_,  1,  2))", r#""\"0, 1, 2\"""#),
        ("S(E4(SV_, 1))", r#""\"0 , 1\"""#),
        ("S(H(1))", r#""- 1""#),
        ("S(P2( x, y))", r#""[xy]""#),
        ("S(P3( x, y))", r#""[ xy]""#),
        ("S(P2(, y))", r#""[y]""#),
        ("S(P2( , y))", r#""[y]""#),
        ("S(P3(,y))", r#""[ y]""#),
        ("S(P7( , y))", r#""[y ]""#),
        ("S(P7(x, ))", r#""[x ]""#),
        ("S(F(O))", r#""[[ 1 ]]""#),
        ("S(K( 1 ))", r#""1 1""#),
        ("S(T( 1 ))", r#""\"1\" 1""#),
        ("S(Q(,2))", r#""< 2>""#),
        ("S(Q(1,))", r#""<1 >""#),
        ("S(R())", r#""[ ]""#),
        ("S(R2())", r#""[ ]""#),
        ("S(R3(,))", r#""[ ]""#),
        ("S(R3(,1))", r#""[ 1]""#),
        ("S(M(1))", r#""[ 1]""#),
        ("S(F(N 1))", r#""[ 1]""#),
        ("S(F( N 1))", r#""[ 1]""#),
        ("S([ N])", r#""[ ]""#),
        ("S(x N y)", r#""x y""#),
        ("S(F(N2() 1))", r#""[ 1]""#),
        ("S(-W())", r#""-""#),
        ("S(- W() +)", r#""- +""#),
        ("S(-W()+)", r#""-+""#),
        ("S(F(1 N))", r#""[1 ]""#),
        ("S(F(N))", r#""[]""#),
        ("S(G1(N))", r#""[ ]""#),
        ("S(Y()z)", r#""x z""#),
        ("S(+M3(,1))", r#""+-1""#),
        ("S(F(N)z)", r#""[]z""#),
        ("S(-F(N 1))", r#""-[ 1]""#),
        ("S(P2(,))", r#""[]""#),
        ("S(P3(,))", r#""[ ]""#),
        ("S(P11(,))", r#""[ x]""#),
        ("S(P11(,y))", r#""[ y x]""#),
        ("S(P12(,))", r#""[ (x)]""#),
        ("S(P13(,))", r#""[]""#),
        ("S(P13(, y))", r#""[y]""#),
        ("S(P14(, y))", r#""[y ]""#),
        ("S(P3( x, ))", r#""[ x]""#),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|&(line, want)| {
            let got = spaced(line);
            (got != want).then(|| format!("{line}: got {got}, want {want}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// An operand of `##` is substituted without macro replacement (6.10.3.3p2),
/// and that includes the variable arguments of GNU's `, ## __VA_ARGS__`: they
/// are replaced only on the rescan, too late for a `#` in the macro they are
/// handed to. Every expectation is what `gcc -E` and `clang -E` print.
#[test]
fn an_operand_of_hash_hash_is_not_expanded_first() {
    const DEFS: &str = "\
#define ONE 1
#define S_(...) #__VA_ARGS__
#define XS(...) S_(__VA_ARGS__)
#define E3(f, ...) f(0, ## __VA_ARGS__)
#define E4(...) XS(0, ## __VA_ARGS__)
#define E6(...) S_(0, ## __VA_ARGS__) XS(__VA_ARGS__)
#define ID(...) __VA_ARGS__
#define CAT(a,b) a ## b
#define A1 hit
#define G(x) x ## 1
#define TWICE(x) x ## x x
#define E5(f, ...) f(__VA_ARGS__ ## 2)
#define STR(x) #x
";
    let cases = [
        // The comma kept, the arguments unexpanded, their spacing their own.
        ("E3(S_, ONE)", r#""0, ONE""#),
        ("E3(S_, ONE, ONE)", r#""0, ONE, ONE""#),
        ("E3(S_,ONE , 2)", r#""0,ONE , 2""#),
        ("E3(S_, ID(ONE))", r#""0, ID(ONE)""#),
        // The comma dropped.
        ("E3(S_)", r#""0""#),
        // The rescan still replaces them where nothing stringifies.
        ("E4(ONE)", r#""0,1""#),
        ("E3(ID, ONE)", "0 , 1"),
        // Unexpanded at the `##`, expanded elsewhere (6.10.3.1p1).
        ("E6(ONE)", r#""0,ONE" "1""#),
        ("TWICE(ONE)", "ONEONE 1"),
        // The ordinary paste, on either side.
        ("CAT(ONE, X)", "ONEX"),
        ("E5(S_, ONE)", r#""ONE2""#),
        // What the paste makes is rescanned.
        ("G(A)", "hit"),
        ("CAT(ONE, )", "1"),
        ("CAT(, ONE)", "1"),
        // `#` never expands.
        ("STR(ONE)", r#""ONE""#),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|&(line, want)| {
            let got = pp(&format!("{DEFS}{line}"));
            (got != want).then(|| format!("{line}: got {got}, want {want}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
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
    // cinrs presents itself as GCC 14.2, the version the world's `__GNUC__`
    // gates are written against, and says who it really is with `__CINRS__`.
    // Nothing claims to be Clang, which has extensions of its own this crate
    // does not have.
    assert!(cond(
        "__GNUC__ == 14 && __GNUC_MINOR__ == 2 && __GNUC_PATCHLEVEL__ == 0"
    ));
    assert!(cond("!defined(__clang__)"));
    assert!(cond("__CINRS__ == 1 && __cinrs__ == 1"));
    assert!(cond(&format!(
        "__CINRS_MAJOR__ == {} && __CINRS_MINOR__ == {} && __CINRS_PATCH__ == {}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    )));
    assert_eq!(
        pp("__VERSION__"),
        format!("\"14.2.0 (cinrs {})\"", env!("CARGO_PKG_VERSION"))
    );
    // C99's `inline` in every revision that has it; see
    // [`c89_says_gnu_inline`] for the other one.
    assert!(cond(
        "defined(__GNUC_STDC_INLINE__) && !defined(__GNUC_GNU_INLINE__)"
    ));
    // Annexes F and G are not claimed, and `0` is what keeps glibc's
    // `<stdc-predef.h>` from claiming them on cinrs's behalf.
    assert!(cond("__GCC_IEC_559 == 0 && __GCC_IEC_559_COMPLEX == 0"));
    // Flag outputs (`=@cc`) are refused, so the macro that promises them is
    // absent; so are the ones for a binary128 type that cannot hold a value
    // and for an optimiser.
    assert!(cond(
        "!defined(__GCC_ASM_FLAG_OUTPUTS__) && !defined(__SIZEOF_FLOAT128__) \
         && !defined(__OPTIMIZE__) && !defined(__NO_INLINE__)"
    ));
    assert!(cond("__BIGGEST_ALIGNMENT__ == 16"));
    // Atomics and variable length arrays are not left out, so the macros that
    // would say so are deliberately absent. The other two do depend on how the
    // expansion was configured: complex arithmetic on a cargo feature — see
    // [`stdc_no_complex_follows_the_feature`] — and threads on the *target*,
    // since `<threads.h>` declares the platform's own; see
    // [`stdc_no_threads_follows_the_target`].
    assert!(cond(
        "!defined(__STDC_NO_ATOMICS__) && !defined(__STDC_NO_VLA__)"
    ));
    // Annex G is never claimed: cinrs implements G.5.1's arithmetic and not
    // the rest of the annex.
    assert!(cond("!defined(__STDC_IEC_559_COMPLEX__)"));
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

/// GCC's `-std=gnu89` and `-std=c89` say `inline` has GNU89's meaning.
#[test]
fn c89_says_gnu_inline() {
    let errors = c89_errors(
        "#if !defined(__GNUC_GNU_INLINE__) || defined(__GNUC_STDC_INLINE__)\n\
         #error wrong inline macro\n\
         #endif\n\
         #if __GNUC__ != 14 || !defined(__CINRS__)\n\
         #error wrong identity\n\
         #endif\n",
        &[],
    );
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn a_predefined_macro_may_be_redefined_without_complaint() {
    assert_eq!(pp("#define __cinrs__ 2\n__cinrs__"), "2");
    assert_eq!(
        pp("#undef __STDC__\n#ifdef __STDC__\nno\n#endif\nyes"),
        "yes"
    );
}

/// `__STDC_NO_COMPLEX__` is C11 6.10.8.3's way of saying "this
/// implementation has no complex arithmetic", so it is defined exactly when
/// the `complex` feature is off — and never otherwise.
#[test]
fn stdc_no_complex_follows_the_feature() {
    let source = "#ifdef __STDC_NO_COMPLEX__\nabsent\n#else\npresent\n#endif\n";
    for (complex, want) in [(true, "present"), (false, "absent")] {
        let mut options = Options::new(Standard::C99);
        options.complex = complex;
        let mut diags = cinrs_core::Diagnostics::new();
        let ctx = Context::new(source, 0);
        let tokens = lex_text(source, ctx.base, &lex_options());
        let out = preprocess(&tokens, &ctx, &options, &mut diags);
        let spellings: Vec<String> = out
            .tokens
            .iter()
            .filter(|t| !t.is_eof())
            .map(|t| t.kind.spelling().to_owned())
            .collect();
        assert_eq!(spellings, [want], "for complex = {complex}");
    }
}

/// `__STDC_NO_THREADS__` says the same thing about C11's thread library, and
/// the answer is the *target's*: the bundled `<threads.h>` declares the
/// platform's own threads, so it exists where cinrs can lay the C library's
/// objects out — glibc and musl, both on Linux — and refuses everywhere else,
/// which is where the macro is predefined.
#[test]
fn stdc_no_threads_follows_the_target() {
    let source = "#ifdef __STDC_NO_THREADS__\nabsent\n#else\npresent\n#endif\n";
    for (triple, want) in [
        ("x86_64-unknown-linux-gnu", "present"),
        ("i686-unknown-linux-gnu", "present"),
        ("aarch64-unknown-linux-musl", "present"),
        ("aarch64-apple-darwin", "absent"),
        ("x86_64-pc-windows-msvc", "absent"),
        ("x86_64-unknown-freebsd", "absent"),
        ("aarch64-linux-android", "absent"),
        ("wasm32-unknown-unknown", "absent"),
    ] {
        let target = cinrs_core::TargetModel::from_triple(triple).expect("a model cinrs knows");
        let options = Options::new(Standard::C11).for_target(target);
        let mut diags = cinrs_core::Diagnostics::new();
        let ctx = Context::new(source, 0);
        let tokens = lex_text(source, ctx.base, &lex_options());
        let out = preprocess(&tokens, &ctx, &options, &mut diags);
        let spellings: Vec<String> = out
            .tokens
            .iter()
            .filter(|t| !t.is_eof())
            .map(|t| t.kind.spelling().to_owned())
            .collect();
        assert_eq!(spellings, [want], "for {triple}");
    }
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

/// Every error a strict `c89!` block finds in `src`, as `(the text the
/// diagnostic points at, the message)`.
///
/// Where a diagnostic points matters as much as what it says here, so the text
/// under its range is what this returns — from `src` itself, or from whichever
/// header the range turns out to be in. `search` is the include path, and a
/// relative `#include` looks in `HEADERS`.
fn c89_errors(src: &str, search: &[&str]) -> Vec<(String, String)> {
    let mut options = Options::new(Standard::C89);
    options.include_paths = search.iter().map(PathBuf::from).collect();
    let ctx = Context {
        dir: Some(PathBuf::from(HEADERS)),
        ..Context::new(src, 0)
    };
    let mut diags = cinrs_core::Diagnostics::new();
    let tokens = lex_text(src, ctx.base, &LexOptions::new(Standard::C89));
    let out = preprocess(&tokens, &ctx, &options, &mut diags);
    // One global offset space: the unit's own text starts at 0, and every
    // included file says where its own was placed.
    let mut files: Vec<(usize, &str)> = vec![(0, src)];
    files.extend(out.included.iter().map(|f| (f.base as usize, &*f.text)));
    diags
        .sorted()
        .into_iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| {
            let (start, end) = (d.range.start as usize, d.range.end as usize);
            let at = files
                .iter()
                .find(|(base, text)| start >= *base && end <= base + text.len())
                .map(|(base, text)| text[start - base..end - base].to_owned())
                .unwrap_or_else(|| "<nowhere>".to_owned());
            (at, d.message.clone())
        })
        .collect()
}

/// The one diagnostic `c89!` has about a comment.
const NOT_C89: &str = "a '//' comment requires C99 or later (this block is c89!)";

#[test]
fn a_comment_is_diagnosed_wherever_the_token_after_it_ends_up() {
    // A comment is text, not a token: the lexer has nowhere to hang what is
    // wrong with one but the token that follows it, and everything about that
    // token — whether it survives, opens a directive, names a macro or is an
    // argument that is dropped — is beside the point.
    // In each of these the comment is the last thing before the token named,
    // which is what makes that token the one carrying the diagnostic.
    let ordinary = "int a; // one\nint b;\n";
    let hash_of_a_directive = "int a; // one\n#define X 1\nint b = X;\n";
    let object_like_name = "#define OBJ int b;\nint a; // one\nOBJ\n";
    let function_like_name = "#define F(x) int b = x;\nint a; // one\nF(1)\n";
    let dropped_argument = "#define FIRST(a, b) a\nint b = FIRST(1, // one\n2);\n";
    let end_of_the_unit = "int a; // one";
    let hash_of_the_next_directive = "#define X 1 // one\n#define Y 2\nint b = X + Y;\n";
    for src in [
        ordinary,
        hash_of_a_directive,
        object_like_name,
        function_like_name,
        dropped_argument,
        end_of_the_unit,
        hash_of_the_next_directive,
    ] {
        assert_eq!(
            c89_errors(src, &[]),
            [("// one".to_owned(), NOT_C89.to_owned())],
            "for:\n{src}"
        );
    }
    // The other thing a comment can be wrong about, which always runs to the
    // end of the file and so is always the last thing in it.
    assert_eq!(
        c89_errors("int a; /* one", &[]),
        [("/* one".to_owned(), "unterminated comment".to_owned())]
    );
}

#[test]
fn a_comment_in_a_skipped_group_is_not_diagnosed() {
    // A skipped group is not even lexically C, and the directive that closes
    // one is the token the comments inside it are attached to.
    assert!(c89_errors("#if 0\nint a; // one\n#endif\nint b;\n", &[]).is_empty());
    assert!(c89_errors("#if 0\n// one\n#else\n#endif\nint b;\n", &[]).is_empty());
    assert!(c89_errors("#if 1\n#else\nint a; // one\n#endif\nint b;\n", &[]).is_empty());
    // A group nothing ever closes is skipped too, so the conditional itself is
    // the only thing wrong with this.
    assert_eq!(
        c89_errors("#if 0\nint a; /* one\n", &[]),
        [(
            "#if".to_owned(),
            "unterminated conditional directive".to_owned()
        )]
    );
    // The group the `#else` takes is read, and the one before it is not.
    assert_eq!(
        c89_errors("#if 0\nint a; // one\n#else\nint b; // two\n#endif\n", &[]),
        [("// two".to_owned(), NOT_C89.to_owned())]
    );
    // And a comment *after* the `#endif` is live text, directive or not.
    assert_eq!(
        c89_errors("#if 0\n#endif // one\n#define X 1\nint b = X;\n", &[]),
        [("// one".to_owned(), NOT_C89.to_owned())]
    );
}

#[test]
fn a_comment_at_the_end_of_a_header_is_diagnosed() {
    // The header's own end-of-file token is the one carrying it, and that token
    // never reaches the output: only the unit's does.
    assert_eq!(
        c89_errors("#include \"tail_comment.h\"\nint b;\n", &[]),
        [(
            "// the last thing in the file, with no token after it".to_owned(),
            NOT_C89.to_owned()
        )]
    );
}

#[test]
fn a_comment_is_diagnosed_once() {
    // Whatever the token it is attached to goes through — being read, being
    // pre-expanded as an argument, being substituted twice and rescanned — the
    // comment was written once.
    assert_eq!(
        c89_errors(
            "#define TWICE(a) ((a) + (a))\nint x = TWICE( // one\n1);\n",
            &[]
        ),
        [("// one".to_owned(), NOT_C89.to_owned())]
    );
    assert_eq!(
        c89_errors("#define OBJ 1 // one\nint a = OBJ, b = OBJ;\n", &[]),
        [("// one".to_owned(), NOT_C89.to_owned())]
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
fn the_crate_pragma_says_where_the_facade_crate_is() {
    let (tokens, errors, out) =
        run_including("#pragma cinrs crate \"crate::vendor::cinrs\"\nkept", &[]);
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(tokens, ["kept"]);
    assert_eq!(out.crate_path.as_deref(), Some("crate::vendor::cinrs"));

    // The value is pasted into the expansion as tokens, so it has to be a Rust
    // path; `crate`, `self` and `super` are the three keywords a segment may be.
    for good in [
        "::cinrs",
        "cinrs",
        "crate::a::b",
        "self::c",
        "super::super::d",
    ] {
        let (_, errors, out) = run_including(&format!("#pragma cinrs crate \"{good}\""), &[]);
        assert!(errors.is_empty(), "{good}: {errors:#?}");
        assert_eq!(out.crate_path.as_deref(), Some(good), "{good}");
    }
    for bad in ["", "a b", "::", "a::", "1st", "a::struct::b", "a-b"] {
        let (_, errors, out) = run_including(&format!("#pragma cinrs crate \"{bad}\""), &[]);
        assert_eq!(errors.len(), 1, "{bad}: {errors:#?}");
        assert!(
            errors[0].contains("is not usable as a Rust path to a crate")
                || errors[0].ends_with("was given an empty string"),
            "{bad}: {errors:#?}"
        );
        assert_eq!(out.crate_path, None, "{bad}");
    }

    // Saying it twice is harmless; saying two different things is a mistake,
    // and the first one stands.
    let (_, errors, out) = run_including(
        "#pragma cinrs crate \"::a\"\n#pragma cinrs crate \"::a\"",
        &[],
    );
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(out.crate_path.as_deref(), Some("::a"));

    let (_, errors, out) = run_including(
        "#pragma cinrs crate \"::a\"\n#pragma cinrs crate \"::b\"",
        &[],
    );
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].starts_with("this unit already reaches the cinrs crate as '::a'"),
        "{errors:#?}"
    );
    assert_eq!(out.crate_path.as_deref(), Some("::a"));
}

/// `#pragma GCC error` and `#pragma GCC warning`, which are `#error` and
/// `#warning` spelled as pragmas — the two `#pragma GCC` forms that say
/// something rather than being accepted and ignored.
#[test]
fn the_gcc_message_pragmas_report_what_they_are_given() {
    let mut options = Options::new(Standard::C99);
    options.dialect = cinrs_core::Dialect::Gnu;
    let src = "#pragma GCC warning \"untested\"\n\
               #pragma GCC error \"unsupported\"\n\
               #pragma GCC diagnostic error \"-Wall\"\n\
               kept";
    let ctx = Context::new(src, 0);
    let mut diags = cinrs_core::Diagnostics::new();
    let tokens = lex_text(src, ctx.base, &lex_options());
    let out = preprocess(&tokens, &ctx, &options, &mut diags);
    let said: Vec<(Level, String)> = diags
        .items()
        .iter()
        .map(|d| (d.level, d.message.clone()))
        .collect();
    // The text is reported as it was written, quotes and all — and the
    // `diagnostic` form is the one that is ignored.
    assert_eq!(
        said,
        [
            (
                Level::Warning,
                "#pragma GCC warning \"untested\"".to_owned()
            ),
            (Level::Error, "#pragma GCC error \"unsupported\"".to_owned()),
        ]
    );
    let spellings: Vec<String> = out
        .tokens
        .iter()
        .filter(|t| !t.is_eof())
        .map(|t| t.kind.spelling().to_owned())
        .collect();
    assert_eq!(spellings, ["kept"]);
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

/// GCC defines its `__has_…` operators as special macros, so `#ifdef
/// __has_include` is how code asks whether it may use one (xxHash's
/// `XXH_HAS_INCLUDE` and friends). The ones this preprocessor answers count as
/// defined; one it does not answer stays undefined.
#[test]
fn the_has_operators_count_as_defined() {
    assert_eq!(pp("#ifdef __has_include\nyes\n#endif"), "yes");
    assert_eq!(pp("#if defined(__has_attribute)\nyes\n#endif"), "yes");
    assert_eq!(pp("#if defined __has_builtin\nyes\n#endif"), "yes");
    assert_eq!(pp("#ifndef __has_c_attribute\nno\n#endif\nend"), "end");
    for op in [
        "__has_include_next",
        "__has_feature",
        "__has_extension",
        "__has_embed",
    ] {
        assert!(cond(&format!("defined({op})")), "{op}");
    }
    // Not answered here (C++ only), so not defined — and the usual fallback
    // a header writes for it is taken.
    assert_eq!(pp("#ifdef __has_cpp_attribute\nyes\n#endif\nno"), "no");
    assert_eq!(
        pp(
            "#ifndef __has_cpp_attribute\n#define __has_cpp_attribute(x) 0\n#endif\n__has_cpp_attribute(y)"
        ),
        "0"
    );
    // The idiom xxHash is written with, end to end.
    assert_eq!(
        pp(
            "#ifdef __has_include\n#define HAS_INC(x) __has_include(x)\n#else\n#define HAS_INC(x) 0\n#endif\n#if HAS_INC(<stddef.h>)\nyes\n#endif"
        ),
        "yes"
    );
}

/// The feature macros `#pragma GCC target` defines on x86-64, and GCC's rules
/// for them: the implied instruction sets come too, `push_options` and
/// `pop_options` save and restore them, `reset_options` goes back to the
/// baseline, and `no-…` takes a name and everything that implies it away.
#[test]
#[cfg(target_arch = "x86_64")]
fn pragma_gcc_target_defines_the_feature_macros() {
    let defined = |src: &str, names: &str| -> String {
        let probes: String = names
            .split(' ')
            .map(|n| format!("#ifdef {n}\n{}\n#endif\n", n.trim_matches('_')))
            .collect();
        pp(&format!("{src}\n{probes}"))
    };
    // The baseline, with nothing asked for.
    assert_eq!(defined("", "__SSE2__ __SSE3__ __AVX2__"), "SSE2");
    // avx2 brings the whole chain below it, and nothing above.
    assert_eq!(
        defined(
            "#pragma GCC target(\"avx2\")",
            "__AVX2__ __AVX__ __SSE4_2__ __SSE4_1__ __SSSE3__ __SSE3__ __SSE2__ __SSE__ __POPCNT__ __AVX512F__ __FMA__"
        ),
        "AVX2 AVX SSE4_2 SSE4_1 SSSE3 SSE3 SSE2 SSE POPCNT"
    );
    // avx512f implies avx2; each AVX-512 name has its own macro.
    assert_eq!(
        defined(
            "#pragma GCC target(\"avx512f,avx512vl\")",
            "__AVX512F__ __AVX512VL__ __AVX2__ __AVX__ __AVX512BW__"
        ),
        "AVX512F AVX512VL AVX2 AVX"
    );
    assert_eq!(
        defined(
            "#pragma GCC target(\"avx512fp16\")",
            "__AVX512FP16__ __AVX512BW__ __AVX512F__ __AVX512VL__"
        ),
        "AVX512FP16 AVX512BW AVX512F"
    );
    assert_eq!(
        defined(
            "#pragma GCC target(\"fma\")\n#pragma GCC target(\"bmi,pclmul\")",
            "__FMA__ __AVX__ __BMI__ __PCLMUL__ __AVX2__"
        ),
        "FMA AVX BMI PCLMUL"
    );
    // push_options / pop_options, and reset_options.
    assert_eq!(
        defined(
            "#pragma GCC push_options\n#pragma GCC target(\"avx2\")\n#pragma GCC pop_options",
            "__AVX2__ __AVX__ __SSE2__"
        ),
        "SSE2"
    );
    assert_eq!(
        pp(
            "#pragma GCC target(\"sse4.1\")\n#pragma GCC push_options\n#pragma GCC target(\"avx2\")\n\
            #ifdef __AVX2__\na\n#endif\n#pragma GCC pop_options\n#ifdef __AVX2__\nb\n#endif\n\
            #ifdef __SSE4_1__\nc\n#endif\n#pragma GCC reset_options\n#ifdef __SSE4_1__\nd\n#endif\n\
            #ifdef __SSE2__\ne\n#endif"
        ),
        "a c e"
    );
    // no-: the name and every name that implies it go; what it implies stays.
    assert_eq!(
        defined(
            "#pragma GCC target(\"avx512f\")\n#pragma GCC target(\"no-avx2\")",
            "__AVX512F__ __AVX2__ __AVX__ __SSE4_2__"
        ),
        "AVX SSE4_2"
    );
    // A pop puts back what a no- took away.
    assert_eq!(
        defined(
            "#pragma GCC target(\"avx\")\n#pragma GCC push_options\n#pragma GCC target(\"no-avx\")\n#pragma GCC pop_options",
            "__AVX__"
        ),
        "AVX"
    );
    // A function attribute is not the pragma: no macro changes.
    assert_eq!(
        defined(
            "__attribute__((target(\"avx2\"))) void f(void) {}",
            "__AVX2__"
        ),
        "__attribute__ ( ( target ( \"avx2\" ) ) ) void f ( void ) { }"
    );
}

/// `_Pragma`'s operand is macro-replaced before it is destringized, as in GCC
/// and Clang: CRoaring (and simdjson) open a target region with
/// `_Pragma(STRINGIFY(GCC target(T)))` out of a macro.
#[test]
#[cfg(target_arch = "x86_64")]
fn pragma_operator_expands_its_operand() {
    let croaring = "#define STRINGIFY_IMPLEMENTATION_(a) #a\n\
        #define STRINGIFY(a) STRINGIFY_IMPLEMENTATION_(a)\n\
        #define CROARING_TARGET_REGION(T) _Pragma(\"GCC push_options\") _Pragma(STRINGIFY(GCC target(T)))\n\
        #define CROARING_UNTARGET_REGION _Pragma(\"GCC pop_options\")\n\
        #define CROARING_TARGET_AVX2 CROARING_TARGET_REGION(\"avx2,bmi,pclmul,lzcnt,popcnt\")\n\
        CROARING_TARGET_AVX2\n\
        #ifdef __AVX2__\na\n#endif\n\
        #ifdef __BMI__\nb\n#endif\n\
        static inline int f(void) { return 1; }\n\
        CROARING_UNTARGET_REGION\n\
        #ifdef __AVX2__\nc\n#endif\n\
        #ifdef __BMI__\nd\n#endif\n\
        end";
    assert_eq!(
        pp(croaring),
        "a b static inline int f ( void ) { return 1 ; } end"
    );
    // Written directly, with an object-like macro as the operand.
    assert_eq!(
        pp("#define X \"GCC target(\\\"avx2\\\")\"\n_Pragma(X)\n#ifdef __AVX2__\na\n#endif"),
        "a"
    );
    // The `(` may come out of a macro as well.
    assert_eq!(
        pp("#define LP (\n_Pragma LP \"GCC target(\\\"avx2\\\")\")\n#ifdef __AVX2__\na\n#endif"),
        "a"
    );
    // `_Pragma` on its own is an ordinary name.
    assert_eq!(pp("#define Y 1\n_Pragma Y"), "_Pragma 1");
}

/// An operand that is not one string literal after replacement is one error,
/// and the whole parenthesised operand goes with it.
#[test]
fn pragma_operator_with_a_bad_operand_is_one_error() {
    one_error("_Pragma(1) int y;", "'_Pragma' takes one string literal");
    assert_eq!(run("_Pragma(1) int y;").0.join(" "), "int y ;");
    one_error(
        "#define S(a) a\n_Pragma(S(GCC target(\"avx2\"))) int y;",
        "'_Pragma' takes one string literal",
    );
    assert_eq!(
        run("#define S(a) a\n_Pragma(S(GCC target(\"avx2\"))) int y;")
            .0
            .join(" "),
        "int y ;"
    );
    one_error(
        "_Pragma(\"a\" \"b\") int z;",
        "'_Pragma' takes one string literal",
    );
    one_error("_Pragma(\"once\"\nint z;", "missing ')' after '_Pragma'");
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
