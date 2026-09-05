//! Lexer tests: every constant form, every punctuator, and the errors.

use cinrs_core::Standard;
use cinrs_core::diag::Level;
use cinrs_core::lex::{
    FloatSuffix, Keyword, LexOptions, LongKind, NumBase, Punct, StrKind, Token, TokenKind, lex_text,
};

fn opts() -> LexOptions {
    LexOptions::new(Standard::C99)
}

/// Every message of the given level the lexer attached to `tokens`.
///
/// The lexer never reports anything itself: a problem rides along on the token
/// it was found in until the preprocessor decides whether that token survives.
fn messages(tokens: &[Token], level: Level) -> Vec<String> {
    tokens
        .iter()
        .flat_map(|t| t.errors.iter())
        .filter(|d| d.level == level)
        .map(|d| d.message.clone())
        .collect()
}

/// Lexes `src`, returning the tokens (without EOF) and the error messages.
fn lex(src: &str) -> (Vec<Token>, Vec<String>) {
    let mut tokens = lex_text(src, 0, &opts());
    assert!(
        tokens.last().is_some_and(Token::is_eof),
        "the token list must end with EOF"
    );
    let errors = messages(&tokens, Level::Error);
    tokens.pop();
    (tokens, errors)
}

/// The warning messages produced for `src`.
fn warnings(src: &str) -> Vec<String> {
    messages(&lex_text(src, 0, &opts()), Level::Warning)
}

fn kinds(src: &str) -> Vec<TokenKind> {
    let (tokens, errors) = lex(src);
    assert_eq!(
        errors,
        Vec::<String>::new(),
        "unexpected errors for {src:?}"
    );
    tokens.into_iter().map(|t| t.kind).collect()
}

/// The token kinds of `src`, with the ones that are not C tokens dropped —
/// which is what the preprocessor does with them once it has reported them.
fn valid_kinds(src: &str) -> Vec<TokenKind> {
    lex(src)
        .0
        .into_iter()
        .map(|t| t.kind)
        .filter(|k| !matches!(k, TokenKind::Error(_)))
        .collect()
}

fn one(src: &str) -> TokenKind {
    let mut k = kinds(src);
    assert_eq!(k.len(), 1, "expected exactly one token in {src:?}: {k:?}");
    k.pop().expect("just checked the length")
}

fn errors(src: &str) -> Vec<String> {
    lex(src).1
}

// ---------------------------------------------------------------------------
// identifiers and keywords
// ---------------------------------------------------------------------------

#[test]
fn all_c99_keywords_are_recognised() {
    let all = [
        "auto",
        "break",
        "case",
        "char",
        "const",
        "continue",
        "default",
        "do",
        "double",
        "else",
        "enum",
        "extern",
        "float",
        "for",
        "goto",
        "if",
        "inline",
        "int",
        "long",
        "register",
        "restrict",
        "return",
        "short",
        "signed",
        "sizeof",
        "static",
        "struct",
        "switch",
        "typedef",
        "union",
        "unsigned",
        "void",
        "volatile",
        "while",
        "_Bool",
        "_Complex",
        "_Imaginary",
    ];
    for word in all {
        let kind = one(word);
        let TokenKind::Keyword(k) = kind else {
            panic!("{word} did not lex as a keyword: {kind:?}");
        };
        assert_eq!(k.as_str(), word);
    }
}

#[test]
fn later_keywords_are_recognised_where_they_belong() {
    // C11's keywords all start with an underscore, which C99 reserves, so
    // they are keywords in every mode and the parser reports the ones a C99
    // block may not use.
    for word in [
        "_Alignas",
        "_Alignof",
        "_Atomic",
        "_Generic",
        "_Noreturn",
        "_Static_assert",
        "_Thread_local",
    ] {
        let TokenKind::Keyword(k) = one(word) else {
            panic!("{word} did not lex as a keyword");
        };
        assert_eq!(k.as_str(), word);
        assert_eq!(k.since(), Standard::C11);
    }

    // C23's are ordinary identifiers before C23 — `<stdbool.h>` depends on
    // it — and keywords in a `c23!` block.
    let c23 = [
        "bool",
        "true",
        "false",
        "nullptr",
        "typeof",
        "typeof_unqual",
        "constexpr",
        "static_assert",
        "alignof",
        "alignas",
        "thread_local",
    ];
    for word in c23 {
        assert_eq!(one(word), TokenKind::Ident(word.to_owned()));
        let tokens = lex_text(word, 0, &LexOptions::new(Standard::C23));
        let TokenKind::Keyword(k) = tokens[0].kind.clone() else {
            panic!("{word} is a keyword in C23");
        };
        assert_eq!(k.as_str(), word);
        assert_eq!(k.since(), Standard::C23);
    }
}

#[test]
fn identifiers() {
    assert_eq!(one("_foo9"), TokenKind::Ident("_foo9".to_owned()));
    assert_eq!(one("Int"), TokenKind::Ident("Int".to_owned()));
    // `$` is off by default.
    assert!(!errors("a$b").is_empty());
    let tokens = lex_text(
        "a$b",
        0,
        &LexOptions {
            dollar_in_identifiers: true,
            ..opts()
        },
    );
    assert!(tokens.iter().all(|t| t.errors.is_empty()));
    assert_eq!(tokens[0].kind, TokenKind::Ident("a$b".to_owned()));
}

// ---------------------------------------------------------------------------
// integer constants
// ---------------------------------------------------------------------------

fn int(src: &str) -> (u128, NumBase, bool, LongKind) {
    match one(src) {
        TokenKind::Int(lit) => (lit.value, lit.base, lit.unsigned, lit.long),
        other => panic!("{src:?} is not an integer constant: {other:?}"),
    }
}

#[test]
fn integer_constants() {
    assert_eq!(int("0"), (0, NumBase::Decimal, false, LongKind::None));
    assert_eq!(int("42"), (42, NumBase::Decimal, false, LongKind::None));
    assert_eq!(int("0777"), (0o777, NumBase::Octal, false, LongKind::None));
    assert_eq!(int("0x1F"), (0x1f, NumBase::Hex, false, LongKind::None));
    assert_eq!(int("0X1f"), (0x1f, NumBase::Hex, false, LongKind::None));
    assert_eq!(int("0x1e"), (0x1e, NumBase::Hex, false, LongKind::None));
}

#[test]
fn integer_suffixes() {
    assert!(int("42u").2);
    assert!(int("42U").2);
    assert_eq!(int("42l").3, LongKind::Long);
    assert_eq!(int("42L").3, LongKind::Long);
    assert_eq!(int("42ll").3, LongKind::LongLong);
    assert_eq!(int("42LL").3, LongKind::LongLong);
    assert_eq!(int("100UL"), (100, NumBase::Decimal, true, LongKind::Long));
    assert_eq!(int("100lu"), (100, NumBase::Decimal, true, LongKind::Long));
    assert_eq!(
        int("100ULL"),
        (100, NumBase::Decimal, true, LongKind::LongLong)
    );
    assert_eq!(
        int("100llu"),
        (100, NumBase::Decimal, true, LongKind::LongLong)
    );
    // `0x1Fu` must keep `F` as a digit and `u` as the suffix.
    assert_eq!(int("0x1Fu"), (0x1f, NumBase::Hex, true, LongKind::None));
    // `f` is a hex digit, so this is 0x1f with an `ul` suffix.
    assert_eq!(int("0x1ful"), (0x1f, NumBase::Hex, true, LongKind::Long));
}

/// The same as [`int`], for a lexer told to accept C23.
fn int_c23(src: &str) -> (u128, NumBase, bool, LongKind) {
    let options = LexOptions::new(Standard::C23);
    let mut tokens = lex_text(src, 0, &options);
    let errors = messages(&tokens, Level::Error);
    assert_eq!(
        errors,
        Vec::<String>::new(),
        "unexpected errors for {src:?}"
    );
    tokens.pop();
    assert_eq!(tokens.len(), 1, "expected one token in {src:?}: {tokens:?}");
    match &tokens[0].kind {
        TokenKind::Int(lit) => (lit.value, lit.base, lit.unsigned, lit.long),
        other => panic!("{src:?} is not an integer constant: {other:?}"),
    }
}

#[test]
fn c23_binary_constants_and_digit_separators() {
    assert_eq!(
        int_c23("0b1011"),
        (0b1011, NumBase::Binary, false, LongKind::None)
    );
    assert_eq!(
        int_c23("0B11u"),
        (0b11, NumBase::Binary, true, LongKind::None)
    );
    assert_eq!(
        int_c23("1'000'000"),
        (1_000_000, NumBase::Decimal, false, LongKind::None)
    );
    assert_eq!(
        int_c23("0xFF'FFu"),
        (0xffff, NumBase::Hex, true, LongKind::None)
    );
    assert_eq!(
        int_c23("0b1010'1010"),
        (0b1010_1010, NumBase::Binary, false, LongKind::None)
    );
    // The spelling `#` reproduces keeps the separators.
    let mut tokens = lex_text("1'000", 0, &LexOptions::new(Standard::C23));
    tokens.pop();
    assert_eq!(tokens[0].kind.spelling(), "1'000");

    // Both are C23 features, and an older block is told so.
    assert_eq!(
        errors("0b1011"),
        ["a binary integer constant requires C23 or later (this block is c99!)".to_owned()]
    );
    assert_eq!(
        errors("1'000"),
        ["a digit separator requires C23 or later (this block is c99!)".to_owned()]
    );
}

#[test]
fn integer_errors() {
    assert_eq!(
        errors("08"),
        ["invalid digit '8' in octal constant '08'".to_owned()]
    );
    assert_eq!(
        errors("019"),
        ["invalid digit '8' in octal constant '019'".replace('8', "9")]
    );
    assert_eq!(
        errors("123abc"),
        ["invalid suffix 'abc' on integer constant '123abc'".to_owned()]
    );
    // `lL` mixes cases and is not a valid `long long` suffix.
    assert_eq!(
        errors("1lL"),
        ["invalid suffix 'lL' on integer constant '1lL'".to_owned()]
    );
    assert_eq!(
        errors("0x"),
        ["expected digits after '0x' in hexadecimal constant".to_owned()]
    );
    assert_eq!(
        errors("0xffffffffffffffffffffffffffffffffff"),
        ["integer constant '0xffffffffffffffffffffffffffffffffff' is too large".to_owned()]
    );
}

// ---------------------------------------------------------------------------
// floating constants
// ---------------------------------------------------------------------------

fn float(src: &str) -> (f64, FloatSuffix, bool) {
    match one(src) {
        TokenKind::Float(lit) => (lit.value, lit.suffix, lit.hex),
        other => panic!("{src:?} is not a floating constant: {other:?}"),
    }
}

#[test]
fn floating_constants() {
    assert_eq!(float("1.0"), (1.0, FloatSuffix::None, false));
    assert_eq!(float(".5"), (0.5, FloatSuffix::None, false));
    assert_eq!(float("1."), (1.0, FloatSuffix::None, false));
    assert_eq!(float("1e5"), (1e5, FloatSuffix::None, false));
    assert_eq!(float("1.5e-3"), (1.5e-3, FloatSuffix::None, false));
    assert_eq!(float("1.5E+3"), (1.5e3, FloatSuffix::None, false));
    assert_eq!(float("1.0f"), (1.0, FloatSuffix::Float, false));
    assert_eq!(float("1.0F"), (1.0, FloatSuffix::Float, false));
    assert_eq!(float("1.0l"), (1.0, FloatSuffix::LongDouble, false));
    assert_eq!(float("2.5e2L"), (250.0, FloatSuffix::LongDouble, false));
}

#[test]
fn hexadecimal_floating_constants() {
    assert_eq!(float("0x1.8p3"), (12.0, FloatSuffix::None, true));
    assert_eq!(float("0x1p-2"), (0.25, FloatSuffix::None, true));
    assert_eq!(float("0X1.8P3f"), (12.0, FloatSuffix::Float, true));
    assert_eq!(float("0x.8p1"), (1.0, FloatSuffix::None, true));
}

#[test]
fn floating_errors() {
    assert!(
        errors("0x1.8")[0].contains("a 'p' exponent is required"),
        "got {:?}",
        errors("0x1.8")
    );
    assert_eq!(
        errors("1.0q"),
        ["invalid suffix 'q' on floating constant '1.0q'".to_owned()]
    );
    assert!(!errors("1e").is_empty());
}

// ---------------------------------------------------------------------------
// character constants
// ---------------------------------------------------------------------------

fn ch(src: &str) -> (i64, bool) {
    let (value, kind) = ch_kind(src);
    (value, kind == StrKind::Wide)
}

fn ch_kind(src: &str) -> (i64, StrKind) {
    match one(src) {
        TokenKind::Char(lit) => (lit.value, lit.kind),
        other => panic!("{src:?} is not a character constant: {other:?}"),
    }
}

#[test]
fn character_constants() {
    assert_eq!(ch("'a'"), (97, false));
    assert_eq!(ch(r"'\0'"), (0, false));
    assert_eq!(ch(r"'\n'"), (10, false));
    assert_eq!(ch(r"'\t'"), (9, false));
    assert_eq!(ch(r"'\\'"), (92, false));
    assert_eq!(ch(r"'\''"), (39, false));
    assert_eq!(ch(r"'\a'"), (7, false));
    assert_eq!(ch(r"'\v'"), (11, false));
    assert_eq!(ch(r"'\101'"), (65, false));
    assert_eq!(ch(r"'\x41'"), (65, false));
    assert_eq!(ch(r"'\xff'"), (255, false));
    assert_eq!(ch("L'a'"), (97, true));
    assert_eq!(ch(r"L'é'"), (0xe9, true));
}

#[test]
fn multi_character_constant_packs_big_endian() {
    // `'ab'` is 0x6162 with GCC's implementation-defined packing.
    assert_eq!(ch("'ab'"), (0x6162, false));
    assert_eq!(ch("'abcd'"), (0x61626364, false));
    // Multi-character constants warn rather than error.
    assert!(lex("'ab'").1.is_empty());
    assert_eq!(
        warnings("'ab'"),
        ["multi-character character constant".to_owned()]
    );
}

#[test]
fn character_constant_errors() {
    assert_eq!(errors("''"), ["empty character constant".to_owned()]);
    assert!(
        errors("'a")
            .iter()
            .any(|m| m.contains("missing terminating"))
    );
    assert!(errors(r"'\q'").iter().any(|m| m.contains("unknown escape")));
    assert!(
        errors(r"'\x'")
            .iter()
            .any(|m| m.contains("no following hex"))
    );
    assert!(errors(r"'\400'").iter().any(|m| m.contains("out of range")));
}

// ---------------------------------------------------------------------------
// string literals
// ---------------------------------------------------------------------------

fn string(src: &str) -> (StrKind, Vec<u32>) {
    match one(src) {
        TokenKind::Str(lit) => (lit.kind, lit.values),
        other => panic!("{src:?} is not a string literal: {other:?}"),
    }
}

#[test]
fn string_literals() {
    assert_eq!(string(r#""abc""#), (StrKind::Narrow, vec![97, 98, 99]));
    assert_eq!(string(r#""%d\n""#), (StrKind::Narrow, vec![37, 100, 10]));
    assert_eq!(string(r#""a\0b""#), (StrKind::Narrow, vec![97, 0, 98]));
    assert_eq!(string(r#"L"ab""#), (StrKind::Wide, vec![97, 98]));
    // A universal character name in a narrow literal is encoded as UTF-8.
    assert_eq!(string(r#""é""#), (StrKind::Narrow, vec![0xc3, 0xa9]));
    // Raw non-ASCII text keeps its UTF-8 bytes.
    assert_eq!(
        string("\"\u{3042}\""),
        (StrKind::Narrow, vec![0xe3, 0x81, 0x82])
    );
    assert_eq!(string("L\"\u{3042}\""), (StrKind::Wide, vec![0x3042]));
}

#[test]
fn the_unicode_string_prefixes() {
    let c11 = LexOptions::new(Standard::C11);
    let string11 = |src: &str| match lex_text(src, 0, &c11).swap_remove(0).kind {
        TokenKind::Str(lit) => (lit.kind, lit.values),
        other => panic!("{src:?} is not a string literal: {other:?}"),
    };
    // `u8"…"` holds the same UTF-8 bytes an unprefixed literal does.
    assert_eq!(string11(r#"u8"é""#), (StrKind::Utf8, vec![0xc3, 0xa9]));
    assert_eq!(string11(r#"u"é""#), (StrKind::Utf16, vec![0xe9]));
    assert_eq!(string11(r#"U"é""#), (StrKind::Utf32, vec![0xe9]));
    // Outside the basic multilingual plane, UTF-16 needs a surrogate pair and
    // UTF-32 does not.
    assert_eq!(
        string11(r#"u"\U0001F600""#),
        (StrKind::Utf16, vec![0xd83d, 0xde00])
    );
    assert_eq!(
        string11(r#"U"\U0001F600""#),
        (StrKind::Utf32, vec![0x1f600])
    );
    assert_eq!(
        string11("u\"\u{1F600}\""),
        (StrKind::Utf16, vec![0xd83d, 0xde00])
    );
    // A numeric escape is a code unit, not a character, so it is not
    // re-encoded.
    assert_eq!(string11(r#"u"\xd83d""#), (StrKind::Utf16, vec![0xd83d]));
    // A prefix is only one when a quote follows it.
    assert_eq!(
        lex_text("u8x", 0, &c11)[0].kind,
        TokenKind::Ident("u8x".to_owned())
    );
    assert_eq!(
        lex_text("unsigned", 0, &c11)[0].kind,
        TokenKind::Keyword(Keyword::Unsigned)
    );
}

#[test]
fn the_unicode_character_prefixes() {
    let c23 = LexOptions::new(Standard::C23);
    let ch23 = |src: &str| match lex_text(src, 0, &c23).swap_remove(0).kind {
        TokenKind::Char(lit) => (lit.value, lit.kind),
        other => panic!("{src:?} is not a character constant: {other:?}"),
    };
    assert_eq!(ch23("u'x'"), (0x78, StrKind::Utf16));
    assert_eq!(ch23("U'x'"), (0x78, StrKind::Utf32));
    assert_eq!(ch23("u8'x'"), (0x78, StrKind::Utf8));
    assert_eq!(ch23(r"u'é'"), (0xe9, StrKind::Utf16));
    assert_eq!(ch23(r"U'\U0001F600'"), (0x1f600, StrKind::Utf32));
    assert_eq!(ch_kind("'x'"), (0x78, StrKind::Narrow));
    assert_eq!(ch_kind("L'x'"), (0x78, StrKind::Wide));
}

#[test]
fn unterminated_string() {
    assert!(
        errors("\"abc\n\"")
            .iter()
            .any(|m| m.contains("missing terminating"))
    );
}

#[test]
fn line_continuation_inside_a_string() {
    assert_eq!(
        string("\"ab\\\ncd\""),
        (StrKind::Narrow, b"abcd".iter().map(|b| *b as u32).collect())
    );
}

// ---------------------------------------------------------------------------
// punctuators
// ---------------------------------------------------------------------------

#[test]
fn all_punctuators() {
    let all = [
        "[", "]", "(", ")", "{", "}", ".", "->", "++", "--", "&", "*", "+", "-", "~", "!", "/",
        "%", "<<", ">>", "<", ">", "<=", ">=", "==", "!=", "^", "|", "&&", "||", "?", ":", ";",
        "...", "=", "*=", "/=", "%=", "+=", "-=", "<<=", ">>=", "&=", "^=", "|=", ",", "#", "##",
    ];
    for spelling in all {
        let kind = one(spelling);
        let TokenKind::Punct(p) = kind else {
            panic!("{spelling} did not lex as a punctuator: {kind:?}");
        };
        assert_eq!(p.as_str(), spelling);
    }
}

#[test]
fn digraphs() {
    assert_eq!(one("<:"), TokenKind::Punct(Punct::LBracket));
    assert_eq!(one(":>"), TokenKind::Punct(Punct::RBracket));
    assert_eq!(one("<%"), TokenKind::Punct(Punct::LBrace));
    assert_eq!(one("%>"), TokenKind::Punct(Punct::RBrace));
    assert_eq!(one("%:"), TokenKind::Punct(Punct::Hash));
    assert_eq!(one("%:%:"), TokenKind::Punct(Punct::HashHash));
    assert_eq!(
        kinds("a<:b:>"),
        vec![
            TokenKind::Ident("a".to_owned()),
            TokenKind::Punct(Punct::LBracket),
            TokenKind::Ident("b".to_owned()),
            TokenKind::Punct(Punct::RBracket),
        ]
    );
}

#[test]
fn maximal_munch() {
    assert_eq!(
        kinds("a>>=b"),
        vec![
            TokenKind::Ident("a".to_owned()),
            TokenKind::Punct(Punct::ShrAssign),
            TokenKind::Ident("b".to_owned()),
        ]
    );
    assert_eq!(
        kinds("x---y"),
        vec![
            TokenKind::Ident("x".to_owned()),
            TokenKind::Punct(Punct::MinusMinus),
            TokenKind::Punct(Punct::Minus),
            TokenKind::Ident("y".to_owned()),
        ]
    );
    assert_eq!(
        kinds("p->x"),
        vec![
            TokenKind::Ident("p".to_owned()),
            TokenKind::Punct(Punct::Arrow),
            TokenKind::Ident("x".to_owned()),
        ]
    );
}

// ---------------------------------------------------------------------------
// whitespace, comments and the preprocessor flags
// ---------------------------------------------------------------------------

#[test]
fn comments_are_whitespace() {
    assert_eq!(
        kinds("a /* comment */ b // trailing\nc"),
        vec![
            TokenKind::Ident("a".to_owned()),
            TokenKind::Ident("b".to_owned()),
            TokenKind::Ident("c".to_owned()),
        ]
    );
}

#[test]
fn unterminated_comment() {
    assert_eq!(errors("a /* b"), ["unterminated comment".to_owned()]);
}

#[test]
fn non_ascii_comment_and_string() {
    let src = "int x; // \u{30b3}\u{30e1}\u{30f3}\u{30c8}\nchar *s = \"\u{3042}a\";";
    let (tokens, errs) = lex(src);
    assert!(errs.is_empty(), "{errs:?}");
    // The string literal token's range must be a byte range into the text.
    let last_string = tokens
        .iter()
        .find(|t| matches!(t.kind, TokenKind::Str(_)))
        .expect("a string literal");
    let quoted = &src[last_string.range.start as usize..last_string.range.end as usize];
    assert_eq!(quoted, "\"\u{3042}a\"");
}

#[test]
fn bol_and_space_flags() {
    let (tokens, errs) = lex("a b\n  c/*x*/d");
    assert!(errs.is_empty());
    let flags: Vec<(bool, bool)> = tokens
        .iter()
        .map(|t| (t.bol, t.preceded_by_space))
        .collect();
    assert_eq!(
        flags,
        vec![(true, false), (false, true), (true, true), (false, true)]
    );
}

#[test]
fn hash_at_line_start_is_marked() {
    let (tokens, errs) = lex("int x;\n#define FOO 1\nint y;");
    assert!(errs.is_empty());
    let hash = tokens
        .iter()
        .find(|t| t.kind == TokenKind::Punct(Punct::Hash))
        .expect("a '#' token");
    assert!(hash.bol, "'#' at the start of a line must have bol set");
}

#[test]
fn a_line_splice_is_deleted_before_the_source_is_tokenised() {
    // Translation phase 2 deletes a backslash-newline; phase 3 then splits
    // what is left into tokens. So a splice inside a name is not a token
    // boundary at all: `a\<newline>b` is the one identifier `ab`, which is
    // how `__LI\<newline>NE__` comes out as `__LINE__` — Clang's own
    // `drs/dr464.c` relies on exactly that.
    assert_eq!(kinds("a\\\nb"), vec![TokenKind::Ident("ab".to_owned())]);
    // A splice that nothing continues over is whitespace, as before.
    assert_eq!(
        kinds("a\\\n b"),
        vec![
            TokenKind::Ident("a".to_owned()),
            TokenKind::Ident("b".to_owned()),
        ]
    );
    assert_eq!(
        kinds("a\\\n+b"),
        vec![
            TokenKind::Ident("a".to_owned()),
            TokenKind::Punct(cinrs_core::lex::Punct::Plus),
            TokenKind::Ident("b".to_owned()),
        ]
    );
}

#[test]
fn unexpected_character() {
    let (tokens, errs) = lex("a @ b");
    assert_eq!(errs, ["unexpected character '@' in program".to_owned()]);
    // The bad character is kept as a token of its own so that a skipped `#if`
    // group can contain it without anyone reporting it.
    assert_eq!(
        tokens.into_iter().map(|t| t.kind).collect::<Vec<_>>(),
        vec![
            TokenKind::Ident("a".to_owned()),
            TokenKind::Error("@".to_owned()),
            TokenKind::Ident("b".to_owned()),
        ]
    );
    assert_eq!(
        valid_kinds("a @ b"),
        vec![
            TokenKind::Ident("a".to_owned()),
            TokenKind::Ident("b".to_owned()),
        ]
    );
}

#[test]
fn every_problem_rides_on_the_token_it_was_found_in() {
    // Nothing is reported to a `Diagnostics` here: the preprocessor decides
    // which of these tokens survives and therefore which error is real.
    let tokens = lex_text("08 /* unterminated", 0, &opts());
    let constant = &tokens[0];
    assert_eq!(constant.errors.len(), 1);
    assert!(constant.errors[0].message.contains("octal constant"));
    // A problem found in white space belongs to the token that follows it,
    // which at the end of the file is the end-of-input token.
    let eof = tokens.last().expect("EOF");
    assert!(eof.is_eof());
    assert_eq!(eof.errors.len(), 1);
    assert_eq!(eof.errors[0].message, "unterminated comment");
}

#[test]
fn a_token_knows_its_own_spelling() {
    // `#` and `##` stringify and paste with the text as written, so every
    // token has to be able to produce it.
    let spellings: Vec<String> = lex_text("0x1f 'a' \"s\\n\" int foo ->", 0, &opts())
        .iter()
        .filter(|t| !t.is_eof())
        .map(|t| t.kind.spelling().to_owned())
        .collect();
    assert_eq!(spellings, ["0x1f", "'a'", "\"s\\n\"", "int", "foo", "->"]);
}

#[test]
fn token_ranges_are_byte_ranges() {
    let src = "int  main;";
    let (tokens, _) = lex(src);
    assert_eq!(tokens[0].kind, TokenKind::Keyword(Keyword::Int));
    assert_eq!((tokens[0].range.start, tokens[0].range.end), (0, 3));
    assert_eq!((tokens[1].range.start, tokens[1].range.end), (5, 9));
    assert_eq!((tokens[2].range.start, tokens[2].range.end), (9, 10));
}

// ---------------------------------------------------------------------------
// trigraphs (translation phase 1)
// ---------------------------------------------------------------------------

/// The kinds of the tokens `src` lexes to, in the given entry point.
fn kinds_in(src: &str, options: &LexOptions) -> Vec<TokenKind> {
    let mut tokens = lex_text(src, 0, options);
    tokens.pop();
    tokens.into_iter().map(|t| t.kind).collect()
}

#[test]
fn the_nine_trigraphs_are_replaced_before_anything_else() {
    let puncts = |src: &str| kinds_in(src, &opts());
    assert_eq!(puncts("??="), vec![TokenKind::Punct(Punct::Hash)]);
    assert_eq!(puncts("??("), vec![TokenKind::Punct(Punct::LBracket)]);
    assert_eq!(puncts("??)"), vec![TokenKind::Punct(Punct::RBracket)]);
    assert_eq!(puncts("??<"), vec![TokenKind::Punct(Punct::LBrace)]);
    assert_eq!(puncts("??>"), vec![TokenKind::Punct(Punct::RBrace)]);
    assert_eq!(puncts("??!"), vec![TokenKind::Punct(Punct::Pipe)]);
    assert_eq!(puncts("??'"), vec![TokenKind::Punct(Punct::Caret)]);
    assert_eq!(puncts("??-"), vec![TokenKind::Punct(Punct::Tilde)]);
    // Maximal munch applies to the replaced characters, so a punctuator may be
    // spelled with two trigraphs, or with one and an ordinary character.
    assert_eq!(puncts("??!??!"), vec![TokenKind::Punct(Punct::PipePipe)]);
    assert_eq!(puncts("??=??="), vec![TokenKind::Punct(Punct::HashHash)]);
    assert_eq!(puncts("??'="), vec![TokenKind::Punct(Punct::CaretAssign)]);
    assert_eq!(puncts("??!="), vec![TokenKind::Punct(Punct::PipeAssign)]);
    // `??` followed by anything else is two question marks.
    assert_eq!(
        puncts("???"),
        vec![
            TokenKind::Punct(Punct::Question),
            TokenKind::Punct(Punct::Question),
            TokenKind::Punct(Punct::Question),
        ]
    );
}

#[test]
fn a_trigraph_backslash_splices_the_line() {
    // Phase 1 runs before phase 2, so `??/` at the end of a line is the
    // backslash that deletes the newline.
    assert_eq!(
        kinds_in("ab??/\ncd", &opts()),
        vec![TokenKind::Ident("abcd".to_owned())]
    );
    assert_eq!(
        string("\"ab??/\ncd\""),
        (StrKind::Narrow, b"abcd".iter().map(|b| *b as u32).collect())
    );
    // And inside a literal it is the backslash of an escape sequence.
    assert_eq!(string("\"a??/nb\""), (StrKind::Narrow, vec![97, 10, 98]));
    assert_eq!(ch("'??/\\'"), (92, false));
}

#[test]
fn trigraphs_are_replaced_inside_string_literals() {
    assert_eq!(
        string("\"??!??'??-\""),
        (StrKind::Narrow, vec![124, 94, 126])
    );
    // Two question marks that begin nothing are two question marks.
    assert_eq!(
        string("\"what??\""),
        (StrKind::Narrow, vec![119, 104, 97, 116, 63, 63])
    );
}

#[test]
fn no_gnu_dialect_and_no_c23_entry_point_has_trigraphs() {
    let mut gnu = LexOptions::new(Standard::C99);
    gnu.trigraphs = cinrs_core::lex::trigraphs_enabled(Standard::C99, cinrs_core::Dialect::Gnu);
    assert_eq!(
        kinds_in("??!", &gnu),
        vec![
            TokenKind::Punct(Punct::Question),
            TokenKind::Punct(Punct::Question),
            TokenKind::Punct(Punct::Bang),
        ]
    );
    let c23 = LexOptions::new(Standard::C23);
    assert_eq!(
        kinds_in("??!", &c23),
        vec![
            TokenKind::Punct(Punct::Question),
            TokenKind::Punct(Punct::Question),
            TokenKind::Punct(Punct::Bang),
        ]
    );
    for standard in [Standard::C89, Standard::C99, Standard::C11, Standard::C17] {
        assert!(cinrs_core::lex::trigraphs_enabled(
            standard,
            cinrs_core::Dialect::Iso
        ));
        assert!(!cinrs_core::lex::trigraphs_enabled(
            standard,
            cinrs_core::Dialect::Gnu
        ));
    }
    assert!(!cinrs_core::lex::trigraphs_enabled(
        Standard::C23,
        cinrs_core::Dialect::Iso
    ));
}
