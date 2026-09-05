//! Extended identifiers: C99 6.4.2.1, C11 Annex D and C23's N2836.
//!
//! A C identifier may hold characters outside the basic character set, written
//! either as a universal character name (`café`, which is C99's own
//! spelling) or as the character itself (`café`, which GCC and Clang have
//! accepted since GCC 10 and which this crate takes in every entry point).
//! Both spellings name the same identifier, because a UCN *is* the character.
//!
//! The characters allowed are Unicode Annex #31's `XID_Start` and
//! `XID_Continue`, which is what C23 settled on and — not by coincidence —
//! what Rust's own identifiers are, so the name survives into the generated
//! item unchanged.

use cinrs::{c99, gnu99};

// Written as the characters themselves, in raw-token form: Rust's lexer takes
// UTF-8 identifiers, so this needs no string literal.
c99! {
    int café(int n) { return n + 1; }
    int Ω(int n) { return n * 2; }
    int 日本語(int n) { return n - 1; }

    struct Δ { int λ; };

    int sum(int n) {
        struct Δ d;
        d.λ = café(n) + Ω(n) + 日本語(n);
        return d.λ;
    }
}

// The same names written as universal character names. `\u` is not something
// Rust's lexer will hand over in raw-token form, so this half is the
// string-literal input the crate documents for exactly that case.
c99! { r##"
/* U+00E9 is `é`, so this defines `café` — and the caller below writes the
   character itself, which is the same identifier. */
int café_ucn(int n) { return n + 1; }
int through_the_character(int n) { return café_ucn(n); }

/* A macro name is an ordinary identifier to the preprocessor. */
#define λ(x) ((x) * 3)
int lambda(int n) { return λ(n); }
"## }

#[test]
fn extended_identifiers_name_the_same_things_either_way() {
    unsafe {
        assert_eq!(café(10), 11);
        assert_eq!(Ω(10), 20);
        assert_eq!(日本語(10), 9);
        assert_eq!(sum(10), 11 + 20 + 9);
        assert_eq!(café_ucn(10), 11);
        assert_eq!(through_the_character(10), 11);
        assert_eq!(lambda(4), 12);
    }
}

// A GNU dialect has them too — the feature is C's, not an extension.
gnu99! {
    int größe(int n) { return n * n; }
}

#[test]
fn a_gnu_dialect_has_them_as_well() {
    assert_eq!(unsafe { größe(5) }, 25);
}

// ---------------------------------------------------------------------------
// what is refused
// ---------------------------------------------------------------------------

use cinrs_core::diag::Level;
use cinrs_core::{Options, Standard, analyze};
use proc_macro2::TokenStream;
use std::str::FromStr;

/// The error messages the front end produces for `source`.
fn errors(standard: Standard, source: &str) -> Vec<String> {
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let analysis = analyze(input, &Options::new(standard));
    analysis
        .diagnostics
        .sorted()
        .into_iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| d.message.clone())
        .collect()
}

/// Rust requires Normalization Form C and `rustc` *applies* it to a procedural
/// macro's identifiers rather than refusing them, so two C names that differ
/// only by normalization would silently become one Rust item. C23 asks for NFC
/// too, which makes refusing both the safe answer and the conforming one.
#[test]
fn an_identifier_that_is_not_in_nfc_is_refused() {
    // `e` followed by U+0301 COMBINING ACUTE ACCENT — the decomposed spelling
    // of `é`.
    let found = errors(Standard::C99, "int cafe\u{301}(void) { return 1; }");
    assert_eq!(
        found,
        [concat!(
            "identifier 'cafe\u{301}' is not in Unicode Normalization Form C; ",
            "write the composed form"
        )]
    );
    // The composed spelling of the very same name is fine.
    assert!(errors(Standard::C99, "int caf\u{e9}(void) { return 1; }").is_empty());
    // And so is the universal character name for it.
    assert!(errors(Standard::C99, "int caf\\u00e9(void) { return 1; }").is_empty());
}

/// A character that is not an identifier character stays what it was.
#[test]
fn a_character_outside_the_identifier_syntax_is_still_refused() {
    // U+00A9 COPYRIGHT SIGN is neither `XID_Start` nor `XID_Continue`.
    let found = errors(Standard::C99, "int \u{a9}(void) { return 1; }");
    assert!(
        found.iter().any(|m| m.contains("unexpected character")),
        "{found:#?}"
    );
    // U+0301 is `XID_Continue` but not `XID_Start`.
    let found = errors(Standard::C99, "int \u{301}x(void) { return 1; }");
    assert!(
        found.iter().any(|m| m.contains("unexpected character")),
        "{found:#?}"
    );
    // A universal character name that spells a basic character is not one
    // either (6.4.3p2).
    let found = errors(Standard::C99, "int \\u0041(void) { return 1; }");
    assert!(
        found
            .iter()
            .any(|m| m.contains("'\\u0041' is not a valid character in an identifier")),
        "{found:#?}"
    );
}

/// C99 introduced the universal character name; `c89!` says so.
#[test]
fn c89_has_no_universal_character_names() {
    let found = errors(Standard::C89, "int caf\\u00e9(void) { return 1; }");
    assert_eq!(
        found.first().map(String::as_str),
        Some("a universal character name requires C99 or later (this block is c89!)")
    );
    // The character itself is a GCC extension rather than a C99 feature, and
    // is taken in every entry point exactly as GCC takes it.
    assert!(errors(Standard::C89, "int caf\u{e9}(void) { return 1; }").is_empty());
}
