//! What a host that gives a procedural macro no positions gets.
//!
//! `rust-analyzer` is that host today: every span it hands over reports no file
//! (`Span::local_file() == None`), no source text, and line 1 column 0 as both
//! its start and its end — for every token of the input alike. What it does give
//! is the token trees, the `Spacing` of each `Punct`, spans it can map back to
//! the source when they come out again on generated tokens or in a
//! `compile_error!`, and the crate's environment, `CARGO_MANIFEST_DIR` included.
//!
//! Every test here reproduces that exactly, by re-spanning a stream — groups'
//! delimiters and all — to one and the same [`Span::call_site`], which outside a
//! real procedural macro context is a span with precisely those properties. The
//! two strategies that answer under those conditions are then both exercised:
//! finding the invocation in the crate's own sources (a temporary crate
//! directory, since `CARGO_MANIFEST_DIR` is process-global and the search takes
//! the directory as a parameter), and rebuilding the text from the tokens alone.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use cinrs_core::capture::InputMode;
use cinrs_core::{Options, Origin, Standard, analyze, analyze_with, expand, expand_with};
use proc_macro2::{Group, Span, TokenStream, TokenTree};

fn options() -> Options {
    Options::new(Standard::C99)
}

fn stream(src: &str) -> TokenStream {
    TokenStream::from_str(src).expect("the test input must lex as Rust tokens")
}

/// `src`, with every token spanned at the call site.
///
/// This is what makes the test a test: after this, no token has a position
/// different from any other's, none has a source text, and none names a file —
/// which is the whole of what `rust-analyzer` says about an invocation's tokens.
fn unpositioned(src: &str) -> TokenStream {
    respan(stream(src))
}

fn respan(input: TokenStream) -> TokenStream {
    input
        .into_iter()
        .map(|tree| match tree {
            TokenTree::Group(group) => {
                // A group's open and close delimiters carry spans of their own,
                // so the whole group has to be rebuilt rather than re-spanned.
                let mut rebuilt = Group::new(group.delimiter(), respan(group.stream()));
                rebuilt.set_span(Span::call_site());
                TokenTree::Group(rebuilt)
            }
            mut other => {
                other.set_span(Span::call_site());
                other
            }
        })
        .collect()
}

#[test]
fn the_conditions_are_the_ones_rust_analyzer_gives() {
    // If this ever stops holding, every test in this file is testing nothing.
    for tree in unpositioned("int f(int *p) { return p->x; }") {
        let span = tree.span();
        assert_eq!(span.local_file(), None, "{tree}");
        assert_eq!(span.source_text(), None, "{tree}");
        assert_eq!((span.start().line, span.start().column), (1, 0), "{tree}");
        assert_eq!((span.end().line, span.end().column), (1, 0), "{tree}");
        if let TokenTree::Group(group) = &tree {
            assert_eq!(group.span_open().start().line, 1);
            assert_eq!(group.span_close().local_file(), None);
        }
    }
}

// ---------------------------------------------------------------------------
// the text rebuilt from the tokens alone
// ---------------------------------------------------------------------------

/// The C text capture recovers from a stream with no positions.
fn rebuilt(src: &str) -> String {
    let analysis = analyze(unpositioned(src), &options());
    assert_eq!(analysis.source.mode, InputMode::Reconstructed);
    analysis.source.text().to_owned()
}

/// The errors a stream with no positions produces, message only.
fn errors(src: &str) -> Vec<String> {
    let analysis = analyze(unpositioned(src), &options());
    analysis
        .diagnostics
        .items()
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

#[test]
fn every_token_survives_a_stream_with_no_positions() {
    // The bug this file exists for: the rule that one multi-character operator
    // arriving as several `Punct`s sharing a span must be written once used to
    // compare positions, and with every position equal it dropped the whole
    // unit but its first token.
    assert_eq!(
        rebuilt("int fact(int n) { return n == 0 ? 1 : n * fact(n - 1); }"),
        "int fact ( int n ) { return n == 0 ? 1 : n * fact ( n - 1 ) ; }"
    );
    assert!(errors("int fact(int n) { return n == 0 ? 1 : n * fact(n - 1); }").is_empty());
}

#[test]
fn the_program_from_the_bug_report_expands_to_the_same_items() {
    // Not only the same text: the same items, so that Rust code calling `fact`
    // sees exactly what a build gives it. Only the unit id differs, and it
    // differs because the text it is hashed from does — one line here, three in
    // the file — which is what it is for.
    let src = "int fact(int n) { return n == 0 ? 1 : n * fact(n - 1); }";
    let without = expand(unpositioned(src), &options()).to_string();
    let with = expand(stream(src), &options()).to_string();
    assert!(!without.contains("compile_error"), "{without}");
    assert!(without.contains("extern \"C\" fn fact"), "{without}");
    assert_eq!(items(&without), items(&with));
}

#[test]
fn a_multi_character_operator_stays_one_operator() {
    // `Spacing::Joint` is the one thing a host with no positions still says
    // about where the tokens were written, and it is enough.
    // A space goes between any two tokens and none inside an operator, so the
    // text is not the one that was written but the tokens are.
    assert_eq!(rebuilt("a->b"), "a -> b");
    assert_eq!(rebuilt("a == b"), "a == b");
    assert_eq!(rebuilt("a <<= b"), "a <<= b");
    assert_eq!(rebuilt("a >>= b"), "a >>= b");
    assert_eq!(rebuilt("a && b || c"), "a && b || c");
    assert_eq!(rebuilt("a++ + --b"), "a ++ + -- b");
    assert_eq!(rebuilt("a != b <= c >= d"), "a != b <= c >= d");
    assert_eq!(rebuilt("void f(int a, ...)"), "void f ( int a , ... )");
    // And two tokens written apart stay two: `a - -b` is not `a-- b`.
    assert_eq!(rebuilt("a - -b"), "a - - b");
    assert_eq!(rebuilt("a + +b"), "a + + b");
    assert_eq!(rebuilt("a - - -b"), "a - - - b");
}

#[test]
fn the_operators_mean_the_same_thing_after_the_round_trip() {
    // The text above is what the C lexer sees; this is what it makes of it.
    let src = "int f(int a, int b) { a <<= 2; a >>= 1; a++; --a; return a && b || (a != b); }";
    let analysis = analyze(unpositioned(src), &options());
    assert!(
        analysis.diagnostics.items().is_empty(),
        "{:#?}",
        analysis.diagnostics.items()
    );
    let spelled: Vec<String> = analysis
        .tokens
        .iter()
        .filter(|t| !t.is_eof())
        .map(|t| t.kind.spelling().to_owned())
        .collect();
    assert!(spelled.contains(&"<<=".to_owned()), "{spelled:?}");
    assert!(spelled.contains(&">>=".to_owned()), "{spelled:?}");
    assert!(spelled.contains(&"++".to_owned()), "{spelled:?}");
    assert!(spelled.contains(&"--".to_owned()), "{spelled:?}");
    assert!(spelled.contains(&"&&".to_owned()), "{spelled:?}");
    assert!(spelled.contains(&"!=".to_owned()), "{spelled:?}");
}

#[test]
fn a_literal_keeps_its_spelling() {
    // A host with no positions still spells a literal as it was written —
    // suffixes, radix, escapes and all — because that is what `to_string` on
    // the token gives.
    assert_eq!(
        rebuilt("unsigned long a = 10UL; int b = 0x1F; int c = 007;"),
        "unsigned long a = 10UL ; int b = 0x1F ; int c = 007 ;"
    );
    assert_eq!(
        rebuilt("float a = 1.0f; double b = 1.5e-3; double c = 1.;"),
        "float a = 1.0f ; double b = 1.5e-3 ; double c = 1. ;"
    );
    assert_eq!(
        rebuilt("char a = 'a'; char b = '\\n';"),
        "char a = 'a' ; char b = '\\n' ;"
    );
    // Two adjacent string literals are two tokens, and C concatenates them.
    assert_eq!(
        rebuilt("char *s = \"a\\n\" \"b\";"),
        "char * s = \"a\\n\" \"b\" ;"
    );
    let analysis = analyze(unpositioned("char *s = \"a\\n\" \"b\";"), &options());
    assert!(
        analysis.diagnostics.items().is_empty(),
        "{:#?}",
        analysis.diagnostics.items()
    );
}

#[test]
fn a_directive_whose_end_the_tokens_give_away_gets_its_own_line() {
    assert_eq!(
        rebuilt("#include <stdio.h>\nint x;"),
        "#include <stdio.h>\nint x ;"
    );
    assert_eq!(
        rebuilt("#include \"mine.h\"\nint x;"),
        "#include \"mine.h\"\nint x ;"
    );
    // A directive ends its line too, so a unit ending in one ends in a newline.
    assert_eq!(
        rebuilt("#ifdef X\nint x;\n#else\nint y;\n#endif"),
        "#ifdef X\nint x ;\n#else\nint y ;\n#endif\n"
    );
    assert_eq!(
        rebuilt("#ifndef H\n#undef X\n#pragma once\n#endif"),
        "#ifndef H\n#undef X\n#pragma once\n#endif\n"
    );
}

#[test]
fn a_unit_with_an_include_works_through_the_tokens_alone() {
    // `<stdio.h>` is bundled, so this is the whole preprocessor running over a
    // text that was rebuilt from tokens.
    let src = "#include <stdio.h>\nint greet(void) { return printf(\"hi\\n\"); }";
    let output = expand(unpositioned(src), &options()).to_string();
    assert!(!output.contains("compile_error"), "{output}");
    assert!(output.contains("extern \"C\" fn greet"), "{output}");
    // And the same C, expanded from a stream that does have positions, gives the
    // same items: the declaration of `printf` came from the header either way.
    let positioned = expand(stream(src), &options()).to_string();
    assert_eq!(items(&output), items(&positioned));
}

#[test]
fn a_directive_whose_end_is_unknown_is_one_clear_diagnostic() {
    for src in [
        "#define TWICE(x) ((x) + (x))\nint f(void) { return TWICE(2); }",
        "#if 1\nint x;\n#endif",
        "#elif 1\n",
        "#error nope",
        "#warning hm",
        "#line 4\n",
        "#pragma cinrs export\n",
        "# 42 \"file.c\"\n",
    ] {
        let reported = errors(src);
        assert_eq!(reported.len(), 1, "{src:?} gave {reported:#?}");
        assert!(
            reported[0].starts_with("cannot tell where "),
            "{src:?} gave {:?}",
            reported[0]
        );
        // Nothing is guessed: the text is empty rather than half a unit.
        let analysis = analyze(unpositioned(src), &options());
        assert_eq!(analysis.source.text(), "");
    }
}

#[test]
fn the_one_diagnostic_says_what_to_do() {
    let analysis = analyze(unpositioned("#define X 1\nint x = X;"), &options());
    let diagnostics = analysis.diagnostics.items();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    let rendered = analysis
        .diagnostics
        .render(&analysis.source.map, &diagnostics[0]);
    assert!(rendered.contains("the '#define' directive"), "{rendered}");
    assert!(rendered.contains("rust-analyzer"), "{rendered}");
    assert!(rendered.contains("save the file"), "{rendered}");
    assert!(rendered.contains("r#\"…\"#"), "{rendered}");
    // The caret goes on the `#`, which is the one span such a host resolves.
    assert!(analysis.source.map.is_precise(analysis.source.base()));
}

// ---------------------------------------------------------------------------
// the invocation found in the crate's sources
// ---------------------------------------------------------------------------

/// A crate directory holding `.rs` files, for the search to walk.
struct Crate {
    dir: PathBuf,
}

impl Crate {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cinrs-ra-{}-{name}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("src")).expect("a temporary directory");
        Self { dir }
    }

    fn file(&self, name: &str, text: &str) -> &Self {
        std::fs::write(self.dir.join(name), text).expect("a writable temporary file");
        self
    }

    /// The origin a real expansion in this crate would have.
    fn origin(&self) -> Origin {
        Origin::new(&["c99"]).in_dir(&self.dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Crate {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

#[test]
fn a_saved_file_is_found_and_gives_the_text_back_whole() {
    let body = "#define TWICE(x) ((x) + (x))\n    int f(void) { /* the comment survives */ return TWICE(21); }";
    let krate = Crate::new("found");
    krate.file(
        "src/lib.rs",
        &format!(
            "mod decoy {{ cinrs::c99! {{ int g(void); }} }}\ncinrs::c99! {{\n    {body}\n}}\n"
        ),
    );

    let analysis = analyze_with(unpositioned(body), &options(), &krate.origin());
    // The file was found, so this is the primary strategy's result: the text is
    // the file's own, directives, comments and line structure intact.
    assert_eq!(analysis.source.mode, InputMode::FileSlice);
    assert_eq!(
        analysis.source.map.file(analysis.source.root).rust_path(),
        Some(krate.path("src/lib.rs").display().to_string().as_str())
    );
    assert_eq!(analysis.source.text(), body);
    assert!(
        analysis.diagnostics.items().is_empty(),
        "{:#?}",
        analysis.diagnostics.items()
    );
    // `__LINE__` counts the lines of the `.rs` file, so the body's own line 1
    // has to be the line it is written on.
    assert_eq!(
        analysis.source.map.file(analysis.source.root).first_line(),
        3
    );
}

#[test]
fn a_found_unit_preprocesses_like_any_other() {
    let body = "#if 2 > 1\nint answer(void) { return 42; }\n#else\nthis is not C\n#endif";
    let krate = Crate::new("preprocessed");
    krate.file("src/lib.rs", &format!("c99! {{\n{body}\n}}\n"));

    let output = expand_with(unpositioned(body), &options(), &krate.origin()).to_string();
    assert!(!output.contains("compile_error"), "{output}");
    assert!(output.contains("extern \"C\" fn answer"), "{output}");
}

#[test]
fn a_quoted_include_is_searched_beside_the_file_that_was_found() {
    let krate = Crate::new("header");
    let body = "#include \"point.h\"\nint x(struct Point *p) { return p->x; }";
    krate
        .file("src/lib.rs", &format!("c99! {{\n{body}\n}}\n"))
        .file("src/point.h", "struct Point { int x; int y; };\n");

    let output = expand_with(unpositioned(body), &options(), &krate.origin()).to_string();
    assert!(!output.contains("compile_error"), "{output}");
    // The header was read from the directory of the `.rs` file the search found,
    // and is tracked so that editing it rebuilds the crate.
    assert!(output.contains("include_str"), "{output}");
    assert!(output.contains("point.h"), "{output}");
}

#[test]
fn an_unsaved_body_is_not_matched_and_falls_back_to_the_tokens() {
    let krate = Crate::new("unsaved");
    // What is on disk is one edit behind what is being typed.
    krate.file("src/lib.rs", "c99! { int x = 1; }\n");

    let analysis = analyze_with(unpositioned("int x = 2;"), &options(), &krate.origin());
    assert_eq!(analysis.source.mode, InputMode::Reconstructed);
    assert_eq!(analysis.source.text(), "int x = 2 ;");
    assert!(analysis.diagnostics.items().is_empty());

    // And with a directive in it, that is the one diagnostic.
    let analysis = analyze_with(
        unpositioned("#define X 2\nint x = X;"),
        &options(),
        &krate.origin(),
    );
    assert_eq!(analysis.diagnostics.items().len(), 1);
}

#[test]
fn a_string_literal_body_takes_the_file_it_was_found_in() {
    let krate = Crate::new("literal");
    krate
        .file(
            "src/lib.rs",
            "// a decoy\nconst S: &str = \"#include \\\"point.h\\\"\";\nc99! { r#\"\n#include \"point.h\"\nint x(struct Point *p) { return p->x; }\n\"# }\n",
        )
        .file("src/point.h", "struct Point { int x; int y; };\n");

    let input =
        unpositioned("r#\"\n#include \"point.h\"\nint x(struct Point *p) { return p->x; }\n\"#");
    let analysis = analyze_with(input, &options(), &krate.origin());
    // The text came from the literal, as always; what the search added is the
    // file it is written in, which is what the quoted `#include` searches beside.
    assert_eq!(analysis.source.mode, InputMode::StringLiteral);
    assert!(
        analysis.diagnostics.items().is_empty(),
        "{:#?}",
        analysis.diagnostics.items()
    );
    assert_eq!(
        analysis.source.map.file(analysis.source.root).rust_path(),
        Some(krate.path("src/lib.rs").display().to_string().as_str())
    );
    assert_eq!(
        analysis.source.map.file(analysis.source.root).first_line(),
        3
    );
}

#[test]
fn the_unit_id_is_the_one_the_compiler_would_have_computed() {
    // Two identical blocks in one module are two units, and the names their
    // expansions generate must differ — which is what the unit id is for. Under
    // a host with no positions the id comes from the position the *search*
    // found, so an IDE and a build agree about every generated name.
    let krate = Crate::new("unit-id");
    krate.file("src/lib.rs", "c99! { int x; }\nc99! { int x; }\n");
    let first = analyze_with(unpositioned("int x;"), &options(), &krate.origin());
    assert_eq!(first.source.mode, InputMode::FileSlice);

    // The same text, at the same place, hashes to the same number however it got
    // there: this is the recipe `Span::local_file` and `Span::start` feed.
    let path = krate.path("src/lib.rs");
    let expected = fnv_unit_id(&path, 1, 7, "int x;");
    assert_eq!(first.source.unit_id(), expected);
    // The second block in the file is a different unit.
    assert_ne!(expected, fnv_unit_id(&path, 2, 7, "int x;"));
}

/// The unit id of a block: FNV-1a over the file, the position and the text.
fn fnv_unit_id(path: &Path, line: u64, column: u64, text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    eat(path.display().to_string().as_bytes());
    eat(&line.to_le_bytes());
    eat(&column.to_le_bytes());
    eat(text.as_bytes());
    hash
}

// ---------------------------------------------------------------------------
// several invocations with the same tokens
// ---------------------------------------------------------------------------

#[test]
fn the_same_block_in_two_files_expands_from_either() {
    // A host with no positions cannot say *which* of two identical invocations
    // it is expanding, and there is nothing in the tokens that would. Both must
    // therefore expand, to the same items: the C is the same either way.
    let krate = Crate::new("twice");
    krate
        .file("src/a.rs", "c99! { int twice(int x) { return 2 * x; } }\n")
        .file("src/b.rs", "c99! { int twice(int x) { return 2 * x; } }\n");

    let body = "int twice(int x) { return 2 * x; }";
    let first = expand_with(unpositioned(body), &options(), &krate.origin()).to_string();
    let second = expand_with(unpositioned(body), &options(), &krate.origin()).to_string();
    assert!(!first.contains("compile_error"), "{first}");
    assert!(first.contains("extern \"C\" fn twice"), "{first}");
    // The same answer every time — the candidates are tried in sorted path
    // order — and the same items as the other file's copy would give.
    assert_eq!(first, second);
    assert_eq!(items(&first), items(&second));
    // And the one that was taken is the first in that order.
    let analysis = analyze_with(unpositioned(body), &options(), &krate.origin());
    assert_eq!(
        analysis.source.map.file(analysis.source.root).rust_path(),
        Some(krate.path("src/a.rs").display().to_string().as_str())
    );
}

#[test]
fn the_candidate_with_the_header_beside_it_is_preferred() {
    // Two copies of a block in two directories, one of which holds the header it
    // includes by name: that one is the copy that will work, and sorted path
    // order would have chosen the other.
    let krate = Crate::new("beside");
    let body = "#include \"point.h\"\nint x(struct Point *p) { return p->x; }";
    std::fs::create_dir_all(krate.path("src/with")).expect("a temporary directory");
    krate
        .file("src/a.rs", &format!("c99! {{\n{body}\n}}\n"))
        .file("src/with/b.rs", &format!("c99! {{\n{body}\n}}\n"))
        .file("src/with/point.h", "struct Point { int x; int y; };\n");

    let analysis = analyze_with(unpositioned(body), &options(), &krate.origin());
    assert_eq!(
        analysis.source.map.file(analysis.source.root).rust_path(),
        Some(krate.path("src/with/b.rs").display().to_string().as_str())
    );
    let output = expand_with(unpositioned(body), &options(), &krate.origin()).to_string();
    assert!(!output.contains("compile_error"), "{output}");
    assert!(output.contains("point.h"), "{output}");

    // With the header beside neither of them, the first in sorted order is taken
    // and the header is looked for wherever else it may be — which is the same
    // thing that happens when an include path supplies it.
    std::fs::remove_file(krate.path("src/with/point.h")).expect("a removable file");
    let analysis = analyze_with(unpositioned(body), &options(), &krate.origin());
    assert_eq!(
        analysis.source.map.file(analysis.source.root).rust_path(),
        Some(krate.path("src/a.rs").display().to_string().as_str())
    );
}

#[test]
fn candidates_whose_line_structure_differs_are_still_deterministic() {
    // The tokens `# define A 1 int x ;` are one line in one file and three in
    // the other, and the two mean different things — the run-together one makes
    // `int x;` part of the replacement list. Nothing can tell which invocation
    // is being expanded, so the rule is only that the answer is the same every
    // time, that it is the preferred candidate, and that nothing panics.
    let krate = Crate::new("lines");
    krate
        .file("src/a.rs", "c99! { #define A 1 int x; }\n")
        .file("src/b.rs", "c99! {\n#define A 1\nint x;\n}\n");

    let body = "#define A 1\nint x;";
    let first = analyze_with(unpositioned(body), &options(), &krate.origin());
    let second = analyze_with(unpositioned(body), &options(), &krate.origin());
    assert_eq!(first.source.mode, InputMode::FileSlice);
    assert_eq!(first.source.text(), second.source.text());
    assert_eq!(first.source.unit_id(), second.source.unit_id());
    // Sorted path order, and neither candidate has a header beside it to prefer:
    // `src/a.rs`, the one that runs the directive together with the code.
    assert_eq!(
        first.source.map.file(first.source.root).rust_path(),
        Some(krate.path("src/a.rs").display().to_string().as_str())
    );
    assert_eq!(first.source.text(), "#define A 1 int x;");
    // A unit that declares nothing, which is what that text means — and no
    // diagnostic, because there is nothing wrong with it.
    assert!(
        first.diagnostics.items().is_empty(),
        "{:#?}",
        first.diagnostics.items()
    );
    let output = expand_with(unpositioned(body), &options(), &krate.origin()).to_string();
    assert!(!output.contains("compile_error"), "{output}");
}

#[test]
fn two_identical_invocations_in_one_scope_share_a_unit_id() {
    // Both find the same first candidate, so both expansions are the same — down
    // to the name of the module they go into, which is then defined twice in one
    // scope. Under `rustc` the two have different positions and therefore
    // different ids; there is nothing here to tell them apart, and one name
    // defined twice is the honest answer rather than something to invent an
    // ordinal for. What matters is that it is that, and not a panic: rust-analyzer
    // as of 0.3.3049 reports nothing at all about it.
    let krate = Crate::new("same-scope");
    krate.file("src/lib.rs", "c99! { int x; }\nc99! { int x; }\n");
    let first = expand_with(unpositioned("int x;"), &options(), &krate.origin()).to_string();
    let second = expand_with(unpositioned("int x;"), &options(), &krate.origin()).to_string();
    assert_eq!(first, second);
    assert!(first.contains("mod __cinrs_unit_"), "{first}");
    assert!(!first.contains("compile_error"), "{first}");
}

#[test]
fn an_included_c_file_is_resolved_beside_the_candidate_that_has_it() {
    // `include_c99!("c/x.c")` written the same way in two directories: the
    // directory the path is resolved against is the one where the file is.
    let krate = Crate::new("include-c");
    std::fs::create_dir_all(krate.path("src/with/c")).expect("a temporary directory");
    krate
        .file("src/a.rs", "include_c99!(\"c/x.c\");\n")
        .file("src/with/b.rs", "include_c99!(\"c/x.c\");\n");
    std::fs::write(
        krate.path("src/with/c/x.c"),
        "int one(void) { return 1; }\n",
    )
    .expect("a writable temporary file");

    let origin = Origin::new(&["include_c99"]).in_dir(&krate.dir);
    let literal = unpositioned("\"c/x.c\"")
        .into_iter()
        .next()
        .expect("one token");
    let dir = cinrs_core::capture::invocation_directory(&literal, &origin, |dir| {
        dir.join("c/x.c").is_file()
    });
    assert_eq!(dir, Some(krate.path("src/with")));

    // With the file beside neither, nothing is preferred and the fallback —
    // `CARGO_MANIFEST_DIR`, which `expand_include` applies — takes over.
    std::fs::remove_file(krate.path("src/with/c/x.c")).expect("a removable file");
    let dir = cinrs_core::capture::invocation_directory(&literal, &origin, |dir| {
        dir.join("c/x.c").is_file()
    });
    assert_eq!(dir, None);
}

#[test]
fn the_search_does_not_run_when_the_positions_are_usable() {
    // A stream built from a string has positions, so the text is rebuilt from
    // them and no file is searched for — even one that would have matched.
    let krate = Crate::new("positioned");
    krate.file("src/lib.rs", "c99! { int x; }\n");
    let analysis = analyze_with(stream("int x;"), &options(), &krate.origin());
    assert_eq!(analysis.source.mode, InputMode::Reconstructed);
    assert_eq!(
        analysis.source.map.file(analysis.source.root).rust_path(),
        None
    );
}

/// The items of an expansion, with every unit id taken out of the names built
/// from it: the module (`__cinrs_unit_050a9987`), the renamed `extern` *objects*
/// (`__cinrs_050a9987_stdout`) and the function-local `static`s.
///
/// Two expansions of the same C differ in those whenever they differ in where
/// the C was written or in how its text was recovered, which is exactly what the
/// id is there for; everything else has to be identical.
fn items(expansion: &str) -> String {
    const PREFIX: &str = "__cinrs_";
    let mut out = String::with_capacity(expansion.len());
    let mut rest = expansion;
    while let Some(at) = rest.find(PREFIX) {
        out.push_str(&rest[..at + PREFIX.len()]);
        rest = &rest[at + PREFIX.len()..];
        // The module's name has `unit_` in front of the number.
        if let Some(after) = rest.strip_prefix("unit_") {
            out.push_str("unit_");
            rest = after;
        }
        // Exactly eight hexadecimal digits, so that a name like `__cinrs_bits0`
        // is left alone.
        let (id, tail) = rest.split_at(rest.len().min(8));
        if id.len() == 8
            && id.bytes().all(|b| b.is_ascii_hexdigit())
            && !tail.starts_with(|c: char| c.is_ascii_hexdigit())
        {
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}
