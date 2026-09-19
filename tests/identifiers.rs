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
// `$`
// ---------------------------------------------------------------------------

// WG14 DR027 asks whether an implementation may take characters outside the
// required source character set in an identifier, and the answer is yes. GCC
// takes `$` unconditionally and Clang takes it with a warning that only
// `-pedantic-errors` promotes, so it is an identifier character here too, in
// every entry point.
//
// Rust has no spelling for it — not even a raw identifier — so a `$` that
// reaches the generated item is written `_dollar_` there, and the C name is
// what the symbol still links by. That is invisible from C, which is the
// point: the name is used below exactly as it was written.
c99! { r##"
#define THIS$AND$THAT(a, b) ((a) + (b))
int dollar$sum(int a, int b) { return THIS$AND$THAT(a, b); }

struct has$members { int one$field; };
int read$member(void) {
    struct has$members s;
    s.one$field = 41;
    return s.one$field + 1;
}
"## }

#[test]
fn a_dollar_sign_is_an_identifier_character() {
    unsafe {
        assert_eq!(dollar_dollar_sum(2, 3), 5);
        assert_eq!(read_dollar_member(), 42);
    }
}

// ---------------------------------------------------------------------------
// names Rust spells differently
// ---------------------------------------------------------------------------

// A C name that is a Rust keyword becomes a raw identifier, which is a token of
// its own and can collide with nothing. Two rules do change the spelling: the
// five names Rust cannot write even as raw identifiers — `self`, `Self`,
// `super`, `crate` and `_` — grow an underscore, and a `$` is written
// `_dollar_`. A program is entitled to use the result itself, so the spelling
// is settled once for the whole translation unit: a name whose spelling is
// already taken grows another `_` until it is not.
//
// Untreated, the first of these is a *silent* miscompilation — two locals
// become one binding — which is why it has a test of its own for every name
// space.

c99! {
    /* Two locals. Untreated, both are `self_`, the second binding shadows the
       first, and the function quietly returns 22. */
    int colliding_locals(void) {
        int self = 1;
        int self_ = 2;
        return self * 10 + self_;
    }

    /* The same for two parameters. */
    int colliding_params(int self, int self_) { return self * 10 + self_; }

    /* Members, reached through `.`, through `->` and by a designator. */
    struct with_self { int self; int self_; int super; int super_; };

    int colliding_members(void) {
        struct with_self s = { .self = 1, .self_ = 2, .super = 3, .super_ = 4 };
        struct with_self *p = &s;
        return s.self * 1000 + s.self_ * 100 + p->super * 10 + p->super_;
    }

    /* Two file-scope functions: `E0428` rather than a wrong answer, but the
       same collision. */
    int crate(void) { return 7; }
    int crate_(void) { return 8; }

    /* Two file-scope objects, read from Rust below by their Rust names. */
    int Self = 11;
    int Self_ = 12;

    /* A bit-field next to a plain member of the changed spelling. */
    struct bit_self { unsigned int self : 4; int self_; };

    int read_bit_self(void) {
        struct bit_self b;
        b.self = 5;
        b.self_ = 6;
        return b.self * 10 + b.self_;
    }
}

#[test]
fn a_changed_spelling_never_collides_with_a_name_the_program_uses() {
    unsafe {
        assert_eq!(colliding_locals(), 12);
        assert_eq!(colliding_params(1, 2), 12);
        assert_eq!(colliding_members(), 1234);
        assert_eq!(crate__(), 7);
        assert_eq!(crate_(), 8);
        assert_eq!(read_bit_self(), 56);
    }
}

#[test]
fn the_two_spellings_name_two_things_from_rust_as_well() {
    // `self` is spelled `self__` because the unit has a `self_` of its own,
    // and the members read the same way everywhere: as an item's fields here,
    // through a designator inside the C.
    let s = with_self {
        self__: 1,
        self_: 2,
        super__: 3,
        super_: 4,
    };
    assert_eq!(
        s.self__ * 1000 + s.self_ * 100 + s.super__ * 10 + s.super_,
        1234
    );
    unsafe {
        assert_eq!({ Self__ }, 11);
        assert_eq!({ Self_ }, 12);
    }
    // The bit-field's accessors are named after the member, so they take the
    // same underscore the plain member of that name forced on it.
    let mut b = bit_self {
        __cinrs_bits0: [0; 1],
        self_: 6,
    };
    b.set_self(5);
    assert_eq!(b.self__(), 5);
    assert_eq!(b.self_, 6);
}

// `$` is the same story, and needs the string-literal entry point because
// Rust's own lexer would not hand `a$b` over as one token.
c99! { r##"
int dollars(void) {
    int a$b = 1;
    int a_dollar_b = 2;
    return a$b * 10 + a_dollar_b;
}
"## }

#[test]
fn a_dollar_that_is_spelled_out_does_not_take_a_name_the_program_has() {
    assert_eq!(unsafe { dollars() }, 12);
}

// Every Rust keyword that C allows as an identifier, as a local of one
// function. `const`, `continue`, `do`, `else`, `enum`, `extern`, `for`, `if`,
// `return`, `static`, `struct`, `while` and `break` are C's own keywords and
// are therefore left out; everything else in Rust's list, strict and reserved,
// is here.
c99! {
    int every_keyword(void) {
        int abstract = 1, as = 1, async = 1, await = 1, become = 1, box = 1;
        int crate = 1, dyn = 1, final = 1, fn = 1, gen = 1, impl = 1, in = 1;
        int let = 1, loop = 1, macro = 1, match = 1, mod = 1, move = 1;
        int override = 1, priv = 1, pub = 1, ref = 1, self = 1, trait = 1;
        int true = 1, try = 1, type = 1, typeof = 1, unsafe = 1, unsized = 1;
        int use = 1, virtual = 1, where = 1, yield = 1, Self = 1, _ = 1;
        return abstract + as + async + await + become + box + crate + dyn
             + final + fn + gen + impl + in + let + loop + macro + match + mod
             + move + override + priv + pub + ref + self + trait + true + try
             + type + typeof + unsafe + unsized + use + virtual + where + yield
             + Self + _;
    }
}

#[test]
fn every_rust_keyword_c_allows_is_usable_as_a_name() {
    assert_eq!(unsafe { every_keyword() }, 37);
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

/// A universal character name is checked where it is *written*, not where the
/// token it is part of is used.
///
/// C99 6.4.3p2 forbids a UCN from naming a character below U+00A0 or a
/// surrogate; that is a constraint on the *spelling*, so translation phase 3
/// settles it and a macro that throws the argument away does not make it
/// well formed. Clang's `C99/n717.c` is a whole file written this way — every
/// name in it is the argument of a `#define M(arg)` that expands to nothing —
/// and each one still has to be diagnosed.
///
/// What does *not* travel that way is a stray character: C99 6.4p3 makes any
/// character that fits no other category a preprocessing token of its own, so
/// a lone `\` or `$` inside an argument nobody looks at is not an error at
/// all. Only a `#define` that *uses* the parameter reaches the parser with it.
#[test]
fn a_universal_character_name_is_checked_where_it_is_written() {
    for name in ["\\u0024", "\\U00000024", "\\u0040", "\\u0060", "\\uD800"] {
        let found = errors(Standard::C99, &format!("#define M(arg)\nM({name})\nint x;"));
        assert!(
            found
                .iter()
                .any(|m| m.contains("is not a valid character in an identifier")),
            "{name}: {found:#?}"
        );
    }
    // A name that is merely not an identifier character — one outside Unicode
    // altogether — says nothing about a token nothing parses.
    assert!(
        errors(
            Standard::C99,
            "#define M(arg)\nM(\\U12345678)\nM($)\nM(\\u12)\nint x;"
        )
        .is_empty()
    );
    // A string literal's spelling is settled there too.
    let found = errors(Standard::C99, "#define M(arg)\nM(\"\\U00110000\")\nint x;");
    assert!(
        found
            .iter()
            .any(|m| m.contains("is not a valid universal character name")),
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
