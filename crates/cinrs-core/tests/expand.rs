//! End-to-end tests for `expand`: capture, source mapping and the spans that
//! the emitted `compile_error!`s carry.
//!
//! Outside a real procedural macro context `proc_macro2` falls back to its own
//! lexer, which still records accurate line/column information — so the exact
//! position an error will be reported at is testable right here.

use std::cell::RefCell;
use std::rc::Rc;
use std::str::FromStr;

use cinrs_core::capture::InputMode;
use cinrs_core::{Options, Standard, Subspan, analyze, expand, expand_with};
use proc_macro2::{TokenStream, TokenTree};

fn options() -> Options {
    Options::new(Standard::C99)
}

fn stream(src: &str) -> TokenStream {
    TokenStream::from_str(src).expect("the test input must lex as Rust tokens")
}

/// One `compile_error!` found in an expansion.
#[derive(Debug, PartialEq)]
struct EmittedError {
    line: usize,
    column: usize,
    message: String,
}

/// Extracts every `compile_error!` from an expansion, with the position its
/// tokens are spanned at.
///
/// An expansion is a module and a glob re-export of it, so the search goes
/// into the groups as well as along them.
fn emitted_errors(tokens: TokenStream) -> Vec<EmittedError> {
    let mut out = Vec::new();
    collect_errors(tokens, &mut out);
    out
}

fn collect_errors(tokens: TokenStream, out: &mut Vec<EmittedError>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (i, tree) in trees.iter().enumerate() {
        if let TokenTree::Group(group) = tree {
            collect_errors(group.stream(), out);
            continue;
        }
        let TokenTree::Ident(ident) = tree else {
            continue;
        };
        if ident != "compile_error" {
            continue;
        }
        let start = ident.span().start();
        let Some(TokenTree::Group(group)) = trees.get(i + 2) else {
            panic!("`compile_error` must be followed by `!` and a brace group");
        };
        let Some(TokenTree::Literal(literal)) = group.stream().into_iter().next() else {
            panic!("`compile_error!` must contain a string literal");
        };
        out.push(EmittedError {
            line: start.line,
            column: start.column,
            message: unquote(&literal.to_string()),
        });
    }
}

/// Undoes the escaping `Literal::string` applies.
fn unquote(literal: &str) -> String {
    let inner = literal
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(literal);
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('u') => {
                let mut value = 0u32;
                for c in chars.by_ref() {
                    match c {
                        '{' => continue,
                        '}' => break,
                        _ => value = value * 16 + c.to_digit(16).unwrap_or(0),
                    }
                }
                out.push(char::from_u32(value).unwrap_or('\u{fffd}'));
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => break,
        }
    }
    out
}

/// The errors the front end alone reports, with no semantic analysis.
fn front_end_errors(source: &str) -> Vec<EmittedError> {
    let analysis = analyze(stream(source), &options());
    emitted_errors(analysis.diagnostics.to_token_stream(&analysis.source.map))
}

// ---------------------------------------------------------------------------
// success
// ---------------------------------------------------------------------------

#[test]
fn a_valid_translation_unit_expands_to_code() {
    let output = expand(
        stream("int fact(int n) { if (n == 0) { return 1; } return n * fact(n - 1); }"),
        &options(),
    )
    .to_string();
    assert!(!output.contains("compile_error"), "{output}");
    assert!(output.contains("extern \"C\" fn fact"), "{output}");
    // An empty unit still expands to nothing at all.
    assert!(expand(TokenStream::new(), &options()).is_empty());
}

#[test]
fn a_prototype_without_a_definition_becomes_an_extern_declaration() {
    // A promise the unit never keeps is what C's linkage model is made of: the
    // symbol comes from somewhere else.
    let output = expand(
        stream("int abs(int n); int f(int n) { return abs(n); }"),
        &options(),
    );
    let text = output.to_string();
    assert!(!text.contains("compile_error"), "{text}");
    assert!(text.contains("unsafe extern \"C\""), "{text}");
    assert!(text.contains("link_name"), "{text}");
    assert!(text.contains("\"abs\""), "{text}");
}

#[test]
fn two_expansions_do_not_share_their_synthetic_names() {
    // Two `c99!` blocks in one Rust module generate into the same namespace,
    // so the `extern` declarations and the mangled `static` locals of one must
    // not collide with the other's.
    let source = "int abs(int n); int f(void) { static int calls; calls++; return abs(calls); }";
    let first = expand(stream(source), &options()).to_string();
    // A second invocation with different text (and, in a real expansion, a
    // different position) gets a different unit id.
    let second = expand(
        stream("int abs(int n); int g(void) { static int calls; calls++; return abs(calls); }"),
        &options(),
    )
    .to_string();
    let name_of = |text: &str, prefix: &str| {
        text.split_whitespace()
            .find(|word| word.starts_with(prefix))
            .expect("a synthetic name")
            .to_owned()
    };
    assert_ne!(
        name_of(&first, "__cinrs_"),
        name_of(&second, "__cinrs_"),
        "{first}\n{second}"
    );
    // Expanding the same text twice is deterministic, which is what keeps
    // incremental rebuilds and snapshot tests stable.
    assert_eq!(expand(stream(source), &options()).to_string(), first);
}

#[test]
fn a_failed_expansion_still_defines_every_well_formed_function() {
    // Stub definitions keep a Rust call site from producing a second error on
    // top of the real one.
    let output = expand(
        stream("int good(int n) { return n; } int bad(void) { return undefined_name; }"),
        &options(),
    )
    .to_string();
    assert!(output.contains("compile_error"), "{output}");
    assert!(output.contains("fn good"), "{output}");
    assert!(output.contains("fn bad"), "{output}");
    assert!(output.contains("unreachable"), "{output}");
}

// ---------------------------------------------------------------------------
// raw-token mode: spans point at the offending C token
// ---------------------------------------------------------------------------

#[test]
fn a_lexer_error_is_reported_at_the_bad_constant() {
    let errors = emitted_errors(expand(stream("int x = 08;"), &options()));
    assert_eq!(
        errors,
        vec![EmittedError {
            line: 1,
            column: 8,
            message: "invalid digit '8' in octal constant '08'".to_owned(),
        }]
    );
}

#[test]
fn a_parser_error_is_reported_at_the_offending_token() {
    let source = "int add(int a, int b) {\n    return a + ;\n}";
    let errors = emitted_errors(expand(stream(source), &options()));
    assert_eq!(
        errors,
        vec![EmittedError {
            line: 2,
            column: 15,
            message: "expected expression, found ';'".to_owned(),
        }]
    );
    // Sanity check: column 15 of line 2 really is the `;`.
    let line = source.lines().nth(1).expect("two lines");
    assert_eq!(line.chars().nth(15), Some(';'));
}

#[test]
fn several_errors_are_all_emitted_in_source_order() {
    let source = "int a = 08;\nint b = 09;\nint c(void) { return 1 }";
    let errors = emitted_errors(expand(stream(source), &options()));
    assert_eq!(errors.len(), 3, "{errors:#?}");
    assert_eq!((errors[0].line, errors[0].column), (1, 8));
    assert_eq!((errors[1].line, errors[1].column), (2, 8));
    assert_eq!((errors[2].line, errors[2].column), (3, 23));
    assert!(errors[2].message.starts_with("expected ';'"));
}

#[test]
fn columns_are_byte_correct_after_non_ascii_text() {
    // `proc_macro2` counts columns in characters while the C lexer works on
    // bytes; the two must agree even when a literal holds multi-byte text.
    // This is about the front end, so semantic analysis (which has opinions
    // about the pointer here) stays out of it.
    let source = "char *s = \"\u{3042}a\"; int y = 08;";
    let errors = front_end_errors(source);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!((errors[0].line, errors[0].column), (1, 24));
    assert_eq!(source.chars().nth(24), Some('0'));
}

#[test]
fn a_directive_error_is_reported_at_the_hash() {
    // `#` passes the Rust lexer, so this reaches our lexer as a punctuator.
    let errors = emitted_errors(expand(
        stream("int x;\n# error something went wrong"),
        &options(),
    ));
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!((errors[0].line, errors[0].column), (2, 0));
    assert_eq!(errors[0].message, "#error something went wrong");
}

#[test]
fn an_error_inside_an_expansion_points_at_the_invocation() {
    // The mistake is in the `#define`, but the user wrote the invocation, and
    // that is where the caret has to land.
    let source = "#define BAD (nope + 1)\nint f(void) {\n    return BAD;\n}";
    let errors = emitted_errors(expand(stream(source), &options()));
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!((errors[0].line, errors[0].column), (3, 11));
    assert!(
        errors[0]
            .message
            .starts_with("use of undeclared identifier"),
        "got {:?}",
        errors[0].message
    );
    assert!(
        errors[0]
            .message
            .contains("note: in expansion of macro 'BAD'"),
        "got {:?}",
        errors[0].message
    );
}

#[test]
fn a_macro_argument_keeps_its_own_position() {
    // An argument was written at the call site, so it points at itself rather
    // than at the invocation as a whole.
    let source = "#define ID(x) x\nint f(void) { return ID(nope); }";
    let errors = emitted_errors(expand(stream(source), &options()));
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!((errors[0].line, errors[0].column), (2, 24));
    assert!(source.lines().nth(1).expect("two lines")[24..].starts_with("nope"));
}

// ---------------------------------------------------------------------------
// capture
// ---------------------------------------------------------------------------

#[test]
fn raw_tokens_are_reconstructed_with_their_line_structure() {
    // No `local_file()` outside a real macro expansion, so this exercises the
    // reconstruction fallback.
    let analysis = analyze(stream("int x;\n\nint y;"), &options());
    assert_eq!(analysis.source.mode, InputMode::Reconstructed);
    assert_eq!(analysis.source.text(), "int x;\n\nint y;");
}

#[test]
fn reconstruction_keeps_the_first_token_at_column_zero() {
    // The primary capture path slices the file starting at the first token,
    // so the fallback must produce the same coordinates.
    let analysis = analyze(stream("  int x;\n  int y;"), &options());
    assert_eq!(analysis.source.text(), "int x;\n  int y;");
}

#[test]
fn string_literal_mode_recovers_the_text_verbatim() {
    let analysis = analyze(stream("r#\"int x; // comment\n\"#"), &options());
    assert_eq!(analysis.source.mode, InputMode::StringLiteral);
    assert_eq!(analysis.source.text(), "int x; // comment\n");
}

#[test]
fn string_literal_mode_unescapes_plain_literals() {
    let analysis = analyze(stream(r#""int x;\nint y;""#), &options());
    assert_eq!(analysis.source.mode, InputMode::StringLiteral);
    assert_eq!(analysis.source.text(), "int x;\nint y;");
}

// ---------------------------------------------------------------------------
// string-literal mode: position information moves into the message
// ---------------------------------------------------------------------------

#[test]
fn string_literal_mode_appends_the_position_to_the_message() {
    // Stable Rust cannot build a span pointing inside a literal, so the C
    // position is spelled out in the message instead.
    let errors = emitted_errors(expand(stream("r#\"int x = 08;\"#"), &options()));
    assert_eq!(
        errors,
        vec![EmittedError {
            line: 1,
            column: 0,
            message: "invalid digit '8' in octal constant '08' \
                      (at line 1, column 9 of the C source)"
                .to_owned(),
        }]
    );
}

/// Expands `input` with a hook that records every range it is asked about and
/// answers `span`, standing in for `proc_macro::Literal::subspan`.
fn with_subspan(input: &str, span: Option<proc_macro2::Span>) -> (Vec<EmittedError>, Vec<Range>) {
    let asked = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&asked);
    let hook = Subspan::new(move |range| {
        seen.borrow_mut().push((range.start, range.end));
        span
    });
    let errors = emitted_errors(expand_with(stream(input), &options(), Some(hook)));
    let ranges = asked.borrow().clone();
    (errors, ranges)
}

/// A byte range of a literal's spelling, as the hook is asked about it.
type Range = (usize, usize);

#[test]
fn a_subspan_hook_is_asked_about_the_bytes_of_the_literal_as_written() {
    // `r#"int x = 08;"#`: the `08` is at 8..10 of the C text and, past the
    // three-byte `r#"`, at 11..13 of the literal.
    let (errors, ranges) = with_subspan("r#\"int x = 08;\"#", Some(proc_macro2::Span::call_site()));
    assert!(ranges.contains(&(11, 13)), "{ranges:?}");
    // A hook that answered makes the position redundant: it is in the caret.
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert_eq!(
        errors[0].message,
        "invalid digit '8' in octal constant '08'"
    );
}

#[test]
fn a_subspan_hook_is_told_where_an_escape_was_written() {
    // `"int y;\nint x = 08;"`: `\n` is one byte of C text and two of the
    // literal, so the `08` at 15..17 of the text is at 17..19 as written.
    let (_, ranges) = with_subspan(
        r#""int y;\nint x = 08;""#,
        Some(proc_macro2::Span::call_site()),
    );
    assert!(ranges.contains(&(17, 19)), "{ranges:?}");
}

#[test]
fn a_subspan_hook_that_declines_falls_back_to_the_message() {
    // What `rust-analyzer` does, and what a literal with no source of its own
    // does: the whole-literal span and the position in the message come back.
    let (errors, ranges) = with_subspan("r#\"int x = 08;\"#", None);
    assert!(!ranges.is_empty(), "the hook must have been asked");
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0]
            .message
            .ends_with("(at line 1, column 9 of the C source)"),
        "got {:?}",
        errors[0].message
    );
}

#[test]
fn string_literal_positions_count_characters_not_bytes() {
    // The comment holds multi-byte text; the reported column must be the one
    // an editor would show.
    let c_source = "// \u{30b3}\u{30e1}\u{30f3}\u{30c8}\nint x = 08;";
    let errors = emitted_errors(expand(stream(&format!("r#\"{c_source}\"#")), &options()));
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0]
            .message
            .ends_with("(at line 2, column 9 of the C source)"),
        "got {:?}",
        errors[0].message
    );
}

#[test]
fn string_literal_mode_accepts_c_that_the_rust_lexer_rejects() {
    // Hexadecimal floating constants, multi-character character constants and
    // wide string literals are all fine here.
    let c_source = r#"typedef int wchar_t;
double d = 0x1.8p3;
int c = 'ab';
const wchar_t *w = L"x";
int hashes(void) { return 1; }
"#;
    let literal = format!("r####\"{c_source}\"####");
    let analysis = analyze(stream(&literal), &options());
    let errors: Vec<&str> = analysis
        .diagnostics
        .items()
        .iter()
        .filter(|d| d.level == cinrs_core::Level::Error)
        .map(|d| d.message.as_str())
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(analysis.unit.items.len(), 5);
}

// ---------------------------------------------------------------------------
// span resolution details
// ---------------------------------------------------------------------------

#[test]
fn a_position_between_tokens_snaps_to_the_nearest_one() {
    let analysis = analyze(stream("int  x;"), &options());
    let map = &analysis.source.map;
    let base = analysis.source.base();
    // Offset 4 is the second space, between `int` and `x`.
    let whitespace = cinrs_core::SourceRange::at(base + 4);
    let span = map.span(whitespace);
    assert_eq!(span.start().line, 1);
    // Equidistant from `int` (ends at column 3) and `x` (starts at column 5);
    // the tie goes to the earlier token.
    assert_eq!(span.start().column, 0);
}

#[test]
fn line_col_is_one_based() {
    let analysis = analyze(stream("int x;\nint y;"), &options());
    let base = analysis.source.base();
    assert_eq!(analysis.source.map.line_col(base), (1, 1));
    assert_eq!(analysis.source.map.line_col(base + 7), (2, 1));
}

// ---------------------------------------------------------------------------
// robustness
// ---------------------------------------------------------------------------

/// Expands `c_source` in string-literal mode, where any text is accepted.
fn expand_c(c_source: &str) -> Vec<EmittedError> {
    let literal = format!("r####\"{c_source}\"####");
    emitted_errors(expand(stream(&literal), &options()))
}

#[test]
fn deeply_nested_input_becomes_a_diagnostic() {
    // Recursive descent turns nesting into stack frames; overflowing the stack
    // would take the whole compiler down with no message at all.
    let depth = 5_000;
    let source = format!("int x = {}1{};", "(".repeat(depth), ")".repeat(depth));
    let errors = expand_c(&source);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0].message.contains("nests too deeply"),
        "got {:?}",
        errors[0].message
    );
}

#[test]
fn deeply_nested_blocks_become_a_diagnostic() {
    let depth = 5_000;
    let source = format!("void f(void) {}{}", "{".repeat(depth), "}".repeat(depth));
    let errors = expand_c(&source);
    assert!(!errors.is_empty());
    assert!(
        errors[0].message.contains("nests too deeply"),
        "got {:?}",
        errors[0].message
    );
}

#[test]
fn malformed_input_always_terminates() {
    // None of these may hang, panic, or expand to nothing: each must produce
    // at least one located diagnostic.
    let cases = [
        "}}}",
        "int (",
        "struct {",
        "int x = (((",
        "typedef",
        ";;;",
        "int f(void) { {{{{ }",
        "int f(void) { return; ",
        "enum",
        "int a[",
        "int f(int",
        "*",
        "\"unterminated",
        "'",
        "/* unterminated",
        "int x = 1 +",
        "struct S { int",
        "int f(void) { switch (1) { case: } }",
    ];
    for case in cases {
        let errors = expand_c(case);
        assert!(!errors.is_empty(), "no diagnostic for {case:?}");
    }
}

#[test]
fn a_long_left_associative_chain_costs_no_stack() {
    // `a + b + c`, `a, b, c` and `a && b && c` are left-associative, so every
    // operand is a *sibling* rather than a level: the parser takes them in a
    // loop, and sema and code generation walk the spine iteratively. Nothing
    // but memory bounds how many there may be, which is what lets a logical
    // source line hold the 4095 characters C23 5.2.5.2p1 asks for — about two
    // thousand comma operands.
    for source in [
        format!("int x = 1{};", " + 1".repeat(3_000)),
        format!("int f(int x) {{ return (x{}); }}", ", x".repeat(3_000)),
        format!("int g(int x) {{ return x{}; }}", " && x".repeat(3_000)),
        format!("int h(int x) {{ return x{}; }}", " | x".repeat(3_000)),
    ] {
        assert_eq!(expand_c(&source), Vec::new(), "for {:.40}…", source);
    }
}

#[test]
fn a_deeply_nested_chain_of_operators_is_one_diagnostic() {
    // The right-associative `?:` and `=`, and a run of postfix operators, are
    // the other way round: each operator is one more *level* of the tree, and
    // code generation walks that on the caller's stack — the 8 MiB `rustc`
    // gives macro expansion. So they are charged to
    // `parse::MAX_RECURSION_DEPTH`, exactly as `((((…))))` is, and going past
    // it is a diagnostic rather than a stack overflow with no message at all.
    for source in [
        format!("int g(int x) {{ return {}x; }}", "x ? x : ".repeat(600)),
        format!("int h(int *p) {{ return {}*p; }}", "*p = ".repeat(600)),
        format!(
            "struct n {{ struct n *next; }};\n\
             struct n *f(struct n *p) {{ return p{}; }}",
            "->next".repeat(600)
        ),
    ] {
        let errors = expand_c(&source);
        assert_eq!(errors.len(), 1, "{errors:?} for {:.40}…", source);
        assert!(
            errors[0].message.contains("nests too deeply"),
            "{:?} for {:.40}…",
            errors[0],
            source
        );
    }
}

/// The stack the deep-nesting test below gives the whole expansion.
///
/// Measured on this machine with an unoptimised build: nesting 180 deep needs
/// between 768 KiB (which overflows) and 1 MiB (which does not), so roughly
/// 5 KiB per level, and the parser's limit of 200 levels comes to about
/// 1 MiB. `rustc` runs macro expansion on 8 MiB, so the real margin is around
/// eightfold; 2 MiB here still fails long before a build would, while leaving
/// enough room that the test does not go off on an unrelated change.
///
/// A failure here is a genuine stack overflow, which aborts the test binary
/// rather than failing politely. That is exactly the point: the same overflow
/// inside a procedural macro takes the compiler down with no message at all.
const SMALL_STACK: usize = 2 << 20;

#[test]
fn codegen_of_deeply_nested_input_fits_in_a_small_stack() {
    // Lexing, parsing and semantic analysis get a thread with a large stack of
    // their own, but capture and code generation must run on the caller's
    // thread: both handle `proc_macro2::Span`s, which are not `Send`. Their
    // recursion is bounded by `parse::MAX_RECURSION_DEPTH`, and this checks
    // that the bound is one a small stack can absorb.
    let depth = 180;
    let handle = std::thread::Builder::new()
        .name("cinrs-small-stack".to_owned())
        .stack_size(SMALL_STACK)
        .spawn(move || {
            let source = format!("int deep(int x) {{ return {}x; }}", "~".repeat(depth));
            let output = expand(stream(&source), &options()).to_string();
            assert!(!output.contains("compile_error"), "{output}");
            assert!(output.contains("fn deep"), "{output}");
            output.len()
        })
        .expect("the test thread must start");
    assert!(handle.join().expect("the expansion must not overflow") > 0);
}

#[test]
fn realistically_deep_nesting_is_accepted() {
    // The recursion limit must be far above anything real code contains.
    let blocks = format!("void f(void) {}{}", "{".repeat(40), "}".repeat(40));
    assert_eq!(expand_c(&blocks), Vec::new());
    let parens = format!("int x = {}1{};", "(".repeat(40), ")".repeat(40));
    assert_eq!(expand_c(&parens), Vec::new());
    let ifs = format!(
        "int g(int n) {{ {} return 0; {} }}",
        "if (n) {".repeat(30),
        "}".repeat(30)
    );
    assert_eq!(expand_c(&ifs), Vec::new());
}

// ---------------------------------------------------------------------------
// #include
// ---------------------------------------------------------------------------

/// Options that can find this package's test headers.
fn options_with_headers() -> Options {
    let mut options = options();
    options.include_paths = vec![std::path::PathBuf::from("tests/include")];
    options
}

#[test]
fn a_user_header_is_named_by_an_include_str_in_the_expansion() {
    let output = expand(
        stream("#include <nested.h>\nint x;"),
        &options_with_headers(),
    )
    .to_string();
    assert!(!output.contains("compile_error"), "{output}");
    // One `include_str!` per user header read, `nested.h` and the `guarded.h`
    // it includes, both by absolute path so that the item resolves from
    // whichever module the invocation is written in.
    let tracked: Vec<&str> = output
        .match_indices("include_str")
        .map(|(_, s)| s)
        .collect();
    assert_eq!(tracked.len(), 2, "{output}");
    // `str` takes the `core::primitive` path like every other primitive the
    // expansion writes: the item goes into the unit's own module, which a
    // `typedef` named `str` could otherwise have taken over.
    assert!(
        output.contains("const _ : & :: core :: primitive :: str = :: core :: include_str !"),
        "{output}"
    );
    let root = std::env::current_dir().expect("a working directory");
    let nested = root.join("tests/include/nested.h");
    assert!(output.contains(&nested.display().to_string()), "{output}");
}

#[test]
fn a_bundled_header_is_not_tracked_for_rebuilds() {
    let output = expand(stream("#include <stdbool.h>\nint x;"), &options()).to_string();
    assert!(!output.contains("compile_error"), "{output}");
    assert!(!output.contains("include_str"), "{output}");
}

#[test]
fn a_diagnostic_inside_a_header_names_the_header_and_points_at_the_directive() {
    let source = "int before;\n#include <broken.h>\nint after;";
    let mut options = options();
    let dir = std::env::temp_dir().join(format!("cinrs-expand-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the temporary directory must be creatable");
    std::fs::write(dir.join("broken.h"), "int ok;\n  int bad = 08;\n").expect("writable");
    options.include_paths = vec![dir.clone()];

    let errors = emitted_errors(expand(stream(source), &options));
    std::fs::remove_dir_all(&dir).ok();

    assert_eq!(errors.len(), 1, "{errors:#?}");
    // The message carries the position inside the header...
    assert!(
        errors[0]
            .message
            .starts_with(&format!("{}:2:13: ", dir.join("broken.h").display())),
        "{:?}",
        errors[0].message
    );
    // ... and the span is the `#include` line of the macro's own text.
    assert_eq!(errors[0].line, 2);
}

/// Every bundled header, on its own, through the whole front end.
///
/// A header that does not parse is worse than a missing one: the program that
/// includes it is buried in diagnostics that are not its fault. They are
/// written in the C99 this crate itself accepts, and this is what keeps them
/// that way.
#[test]
fn every_bundled_header_compiles_on_its_own() {
    for (name, _) in cinrs_core::include::BUNDLED {
        if *name == "setjmp.h" {
            continue;
        }
        let source = format!("#include <{name}>");
        let output = expand(stream(&source), &options()).to_string();
        assert!(
            !output.contains("compile_error"),
            "<{name}> did not survive the front end: {output}"
        );
    }
}

/// The one bundled header that is a refusal rather than a set of
/// declarations.
#[test]
fn setjmp_says_it_is_not_supported() {
    let errors = emitted_errors(expand(stream("#include <setjmp.h>"), &options()));
    assert_eq!(errors.len(), 1, "{errors:#?}");
    assert!(
        errors[0]
            .message
            .contains("setjmp/longjmp are not supported by cinrs"),
        "{:?}",
        errors[0].message
    );
    // Reported inside the header, and pointed at the directive.
    assert!(
        errors[0].message.starts_with("<cinrs>/setjmp.h:"),
        "{:?}",
        errors[0].message
    );
}

#[test]
fn a_link_pragma_puts_a_link_attribute_on_the_extern_block() {
    let output = expand(
        stream("#pragma cinrs link \"mylib\"\nint helper(int);"),
        &options(),
    )
    .to_string();
    assert!(!output.contains("compile_error"), "{output}");
    assert!(output.contains("# [link (name = \"mylib\")]"), "{output}");
    // Nothing is added when nothing asks for it.
    let plain = expand(stream("int helper(int);"), &options()).to_string();
    assert!(!plain.contains("link (name"), "{plain}");
}
