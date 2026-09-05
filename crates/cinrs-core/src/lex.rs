//! A complete C99 lexer working on the captured source text.
//!
//! The lexer is byte-offset based: every [`Token`] carries a
//! [`SourceRange`] into the [`SourceMap`](crate::capture::SourceMap), which is
//! what makes exact diagnostics possible. Constants are decoded here (value,
//! base, suffix, escape sequences) so that neither the parser nor sema has to
//! look at raw text again.
//!
//! Two flags on each token — [`Token::bol`] and [`Token::preceded_by_space`] —
//! are what the [preprocessor](crate::pp) recognises directives with and what
//! its stringification operator reproduces: directive recognition needs "first
//! token on a line", and `#` needs to know where whitespace was.
//!
//! # The lexer never reports anything
//!
//! Lexical errors never stop the lexer, and they never reach [`Diagnostics`]
//! either: each one is attached to the token it was found in, in
//! [`Token::errors`], and the preprocessor reports the ones whose token
//! survives into its output. That is not a detail — a group skipped by
//! `#if 0` may legally hold text that is not C at all, and a macro that is
//! never invoked may hold anything its author liked:
//!
//! ```c
//! #if 0
//! this is not C: 08, 'unterminated, @@@
//! #endif
//! ```
//!
//! Text that is not a token at all becomes a [`TokenKind::Error`] token
//! carrying its own spelling, so that it too can be skipped rather than
//! reported. The preprocessor drops those tokens after reporting them, so
//! nothing downstream ever sees one.
//!
//! [`Diagnostics`]: crate::diag::Diagnostics

use crate::capture::{Pos, Source, SourceRange};
use crate::diag::Diagnostic;
use crate::{Options, Standard};

/// A C99 keyword.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[allow(missing_docs)]
pub enum Keyword {
    Auto,
    Break,
    Case,
    Char,
    Const,
    Continue,
    Default,
    Do,
    Double,
    Else,
    Enum,
    Extern,
    Float,
    For,
    Goto,
    If,
    Inline,
    Int,
    Long,
    Register,
    Restrict,
    Return,
    Short,
    Signed,
    Sizeof,
    Static,
    Struct,
    Switch,
    Typedef,
    Union,
    Unsigned,
    Void,
    Volatile,
    While,
    Bool,
    Complex,
    Imaginary,
    // C11. Every one of these is spelled with a leading underscore, which C99
    // already reserves, so they are recognised in every mode and the parser
    // reports the ones a C99 block may not use — a friendlier answer than
    // "expected a declaration, found identifier '_Static_assert'".
    Alignas,
    Alignof,
    Atomic,
    Generic,
    Noreturn,
    StaticAssert,
    ThreadLocal,
    BitInt,
    // C23. These are ordinary identifiers before C23 — `<stdbool.h>` writes
    // `#define bool _Bool`, and a C99 program may have a variable called
    // `typeof` — so they are keywords only in a `c23!` block.
    BoolName,
    True,
    False,
    Nullptr,
    Typeof,
    TypeofUnqual,
    Constexpr,
    StaticAssertName,
    AlignofName,
    AlignasName,
    ThreadLocalName,
    // The GNU keywords. Every one of them is spelled with a leading double
    // underscore, which C reserves, so they are available in every entry point
    // — exactly as they are in GCC's own strict modes. They are *not* produced
    // by [`Keyword::from_str`]: the [preprocessor](crate::pp) turns the
    // identifiers into them on the way out, after macro replacement, so that
    // `#define __attribute__(x)` — which portability headers really do write —
    // still defines and expands a macro of that name.
    /// `__attribute__`, `__attribute`
    Attribute,
    /// `__extension__`
    Extension,
    /// `__alignof__`, `__alignof`
    AlignofGnu,
    /// `__typeof__`, `__typeof`, and `typeof` in a GNU dialect
    TypeofGnu,
    /// `__typeof_unqual__`
    TypeofUnqualGnu,
    /// `__asm__`, `__asm`, and `asm` in a GNU dialect
    Asm,
    /// `__label__`
    Label,
    /// `__auto_type`
    AutoType,
    /// `__thread`
    ThreadGnu,
    /// `__real__`, `__real`
    RealGnu,
    /// `__imag__`, `__imag`
    ImagGnu,
}

impl Keyword {
    /// The spelling of this keyword in C source.
    pub fn as_str(self) -> &'static str {
        use Keyword::*;
        match self {
            Auto => "auto",
            Break => "break",
            Case => "case",
            Char => "char",
            Const => "const",
            Continue => "continue",
            Default => "default",
            Do => "do",
            Double => "double",
            Else => "else",
            Enum => "enum",
            Extern => "extern",
            Float => "float",
            For => "for",
            Goto => "goto",
            If => "if",
            Inline => "inline",
            Int => "int",
            Long => "long",
            Register => "register",
            Restrict => "restrict",
            Return => "return",
            Short => "short",
            Signed => "signed",
            Sizeof => "sizeof",
            Static => "static",
            Struct => "struct",
            Switch => "switch",
            Typedef => "typedef",
            Union => "union",
            Unsigned => "unsigned",
            Void => "void",
            Volatile => "volatile",
            While => "while",
            Bool => "_Bool",
            Complex => "_Complex",
            Imaginary => "_Imaginary",
            Alignas => "_Alignas",
            Alignof => "_Alignof",
            Atomic => "_Atomic",
            Generic => "_Generic",
            Noreturn => "_Noreturn",
            StaticAssert => "_Static_assert",
            ThreadLocal => "_Thread_local",
            BitInt => "_BitInt",
            BoolName => "bool",
            True => "true",
            False => "false",
            Nullptr => "nullptr",
            Typeof => "typeof",
            TypeofUnqual => "typeof_unqual",
            Constexpr => "constexpr",
            StaticAssertName => "static_assert",
            AlignofName => "alignof",
            AlignasName => "alignas",
            ThreadLocalName => "thread_local",
            Attribute => "__attribute__",
            Extension => "__extension__",
            AlignofGnu => "__alignof__",
            TypeofGnu => "__typeof__",
            TypeofUnqualGnu => "__typeof_unqual__",
            Asm => "__asm__",
            Label => "__label__",
            AutoType => "__auto_type",
            ThreadGnu => "__thread",
            RealGnu => "__real__",
            ImagGnu => "__imag__",
        }
    }

    /// Whether this keyword is one of the GNU spellings the preprocessor
    /// introduces; see the variants' own documentation.
    pub fn is_gnu(self) -> bool {
        use Keyword::*;
        matches!(
            self,
            Attribute
                | Extension
                | AlignofGnu
                | TypeofGnu
                | TypeofUnqualGnu
                | Asm
                | Label
                | AutoType
                | ThreadGnu
                | RealGnu
                | ImagGnu
        )
    }

    /// The revision that made this spelling a keyword.
    ///
    /// The C11 keywords are recognised in every mode — they are reserved
    /// identifiers in C99, so nothing legal can be broken by it, and the
    /// parser's "requires C11 or later" is a better answer than a syntax
    /// error. The C23 ones are *not*: they are ordinary identifiers before
    /// C23, and `<stdbool.h>`'s `#define bool _Bool` depends on it.
    pub fn since(self) -> Standard {
        use Keyword::*;
        match self {
            Alignas | Alignof | Atomic | Generic | Noreturn | StaticAssert | ThreadLocal => {
                Standard::C11
            }
            BitInt | BoolName | True | False | Nullptr | Typeof | TypeofUnqual | Constexpr
            | StaticAssertName | AlignofName | AlignasName | ThreadLocalName => Standard::C23,
            _ => Standard::C99,
        }
    }

    /// Looks a keyword up by spelling, honouring the language standard.
    pub fn from_str(s: &str, standard: Standard) -> Option<Keyword> {
        use Keyword::*;
        if standard >= Standard::C23 {
            let c23 = match s {
                "bool" => Some(BoolName),
                "true" => Some(True),
                "false" => Some(False),
                "nullptr" => Some(Nullptr),
                "typeof" => Some(Typeof),
                "typeof_unqual" => Some(TypeofUnqual),
                "constexpr" => Some(Constexpr),
                "static_assert" => Some(StaticAssertName),
                "alignof" => Some(AlignofName),
                "alignas" => Some(AlignasName),
                "thread_local" => Some(ThreadLocalName),
                _ => None,
            };
            if c23.is_some() {
                return c23;
            }
        }
        Some(match s {
            "_Alignas" => Alignas,
            "_Alignof" => Alignof,
            "_Atomic" => Atomic,
            "_Generic" => Generic,
            "_Noreturn" => Noreturn,
            "_Static_assert" => StaticAssert,
            "_Thread_local" => ThreadLocal,
            "_BitInt" => BitInt,
            "auto" => Auto,
            "break" => Break,
            "case" => Case,
            "char" => Char,
            "const" => Const,
            "continue" => Continue,
            "default" => Default,
            "do" => Do,
            "double" => Double,
            "else" => Else,
            "enum" => Enum,
            "extern" => Extern,
            "float" => Float,
            "for" => For,
            "goto" => Goto,
            "if" => If,
            "inline" => Inline,
            "int" => Int,
            "long" => Long,
            "register" => Register,
            "restrict" => Restrict,
            "return" => Return,
            "short" => Short,
            "signed" => Signed,
            "sizeof" => Sizeof,
            "static" => Static,
            "struct" => Struct,
            "switch" => Switch,
            "typedef" => Typedef,
            "union" => Union,
            "unsigned" => Unsigned,
            "void" => Void,
            "volatile" => Volatile,
            "while" => While,
            "_Bool" => Bool,
            "_Complex" => Complex,
            "_Imaginary" => Imaginary,
            _ => return None,
        })
    }
}

/// A C99 punctuator.
///
/// Digraphs are folded into the token they stand for: `<:` lexes as
/// [`Punct::LBracket`], `%:%:` as [`Punct::HashHash`], and so on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[allow(missing_docs)]
pub enum Punct {
    LBracket,
    RBracket,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Dot,
    Arrow,
    PlusPlus,
    MinusMinus,
    Amp,
    Star,
    Plus,
    Minus,
    Tilde,
    Bang,
    Slash,
    Percent,
    Shl,
    Shr,
    Lt,
    Gt,
    Le,
    Ge,
    EqEq,
    Ne,
    Caret,
    Pipe,
    AmpAmp,
    PipePipe,
    Question,
    Colon,
    Semi,
    Ellipsis,
    Assign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    PlusAssign,
    MinusAssign,
    ShlAssign,
    ShrAssign,
    AmpAssign,
    CaretAssign,
    PipeAssign,
    Comma,
    Hash,
    HashHash,
}

impl Punct {
    /// The canonical spelling of this punctuator (never the digraph form).
    pub fn as_str(self) -> &'static str {
        use Punct::*;
        match self {
            LBracket => "[",
            RBracket => "]",
            LParen => "(",
            RParen => ")",
            LBrace => "{",
            RBrace => "}",
            Dot => ".",
            Arrow => "->",
            PlusPlus => "++",
            MinusMinus => "--",
            Amp => "&",
            Star => "*",
            Plus => "+",
            Minus => "-",
            Tilde => "~",
            Bang => "!",
            Slash => "/",
            Percent => "%",
            Shl => "<<",
            Shr => ">>",
            Lt => "<",
            Gt => ">",
            Le => "<=",
            Ge => ">=",
            EqEq => "==",
            Ne => "!=",
            Caret => "^",
            Pipe => "|",
            AmpAmp => "&&",
            PipePipe => "||",
            Question => "?",
            Colon => ":",
            Semi => ";",
            Ellipsis => "...",
            Assign => "=",
            StarAssign => "*=",
            SlashAssign => "/=",
            PercentAssign => "%=",
            PlusAssign => "+=",
            MinusAssign => "-=",
            ShlAssign => "<<=",
            ShrAssign => ">>=",
            AmpAssign => "&=",
            CaretAssign => "^=",
            PipeAssign => "|=",
            Comma => ",",
            Hash => "#",
            HashHash => "##",
        }
    }
}

/// Punctuator spellings, longest first so that a greedy match is also the
/// maximal munch the standard asks for.
const PUNCTUATORS: &[(&str, Punct)] = &[
    ("%:%:", Punct::HashHash),
    ("...", Punct::Ellipsis),
    ("<<=", Punct::ShlAssign),
    (">>=", Punct::ShrAssign),
    ("->", Punct::Arrow),
    ("++", Punct::PlusPlus),
    ("--", Punct::MinusMinus),
    ("<<", Punct::Shl),
    (">>", Punct::Shr),
    ("<=", Punct::Le),
    (">=", Punct::Ge),
    ("==", Punct::EqEq),
    ("!=", Punct::Ne),
    ("&&", Punct::AmpAmp),
    ("||", Punct::PipePipe),
    ("*=", Punct::StarAssign),
    ("/=", Punct::SlashAssign),
    ("%=", Punct::PercentAssign),
    ("+=", Punct::PlusAssign),
    ("-=", Punct::MinusAssign),
    ("&=", Punct::AmpAssign),
    ("^=", Punct::CaretAssign),
    ("|=", Punct::PipeAssign),
    ("##", Punct::HashHash),
    ("<:", Punct::LBracket),
    (":>", Punct::RBracket),
    ("<%", Punct::LBrace),
    ("%>", Punct::RBrace),
    ("%:", Punct::Hash),
    ("[", Punct::LBracket),
    ("]", Punct::RBracket),
    ("(", Punct::LParen),
    (")", Punct::RParen),
    ("{", Punct::LBrace),
    ("}", Punct::RBrace),
    (".", Punct::Dot),
    ("&", Punct::Amp),
    ("*", Punct::Star),
    ("+", Punct::Plus),
    ("-", Punct::Minus),
    ("~", Punct::Tilde),
    ("!", Punct::Bang),
    ("/", Punct::Slash),
    ("%", Punct::Percent),
    ("<", Punct::Lt),
    (">", Punct::Gt),
    ("^", Punct::Caret),
    ("|", Punct::Pipe),
    ("?", Punct::Question),
    (":", Punct::Colon),
    (";", Punct::Semi),
    ("=", Punct::Assign),
    (",", Punct::Comma),
    ("#", Punct::Hash),
];

/// The `l`/`ll` part of an integer suffix.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LongKind {
    /// No `l` suffix.
    #[default]
    None,
    /// `l` or `L`.
    Long,
    /// `ll` or `LL`.
    LongLong,
}

/// The base an integer constant was written in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NumBase {
    /// `0b101` — C23.
    Binary,
    /// `0777`
    Octal,
    /// `42`
    Decimal,
    /// `0x2a`
    Hex,
}

impl NumBase {
    /// The radix.
    pub fn radix(self) -> u32 {
        match self {
            NumBase::Binary => 2,
            NumBase::Octal => 8,
            NumBase::Decimal => 10,
            NumBase::Hex => 16,
        }
    }

    /// How a diagnostic names this base.
    pub fn as_str(self) -> &'static str {
        match self {
            NumBase::Binary => "binary",
            NumBase::Octal => "octal",
            NumBase::Decimal => "decimal",
            NumBase::Hex => "hexadecimal",
        }
    }
}

/// A decoded integer constant.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IntLit {
    /// The value, before any type is chosen for it.
    pub value: u128,
    /// How it was written.
    pub base: NumBase,
    /// Whether a `u`/`U` suffix was present.
    pub unsigned: bool,
    /// Whether an `l`/`ll` suffix was present.
    pub long: LongKind,
    /// The exact source spelling.
    pub text: String,
}

/// The suffix of a floating constant.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FloatSuffix {
    /// No suffix: the constant has type `double`.
    #[default]
    None,
    /// `f`/`F`: `float`.
    Float,
    /// `l`/`L`: `long double`.
    LongDouble,
}

/// A decoded floating constant.
#[derive(Clone, PartialEq, Debug)]
pub struct FloatLit {
    /// The value, rounded to `f64`.
    pub value: f64,
    /// The suffix, which fixes the constant's type.
    pub suffix: FloatSuffix,
    /// Whether the constant was written in hexadecimal form.
    pub hex: bool,
    /// The exact source spelling.
    pub text: String,
}

/// A decoded character constant.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CharLit {
    /// The value of the constant.
    ///
    /// For a plain single-character constant this is the unsigned value of the
    /// execution character (so `'\xff'` is `255`); whether that is later
    /// interpreted as `-1` is up to sema, which knows the signedness of
    /// `char`. Multi-character constants are packed big-endian, as GCC does.
    pub value: i64,
    /// Whether the constant had an `L` prefix.
    pub wide: bool,
    /// The exact source spelling, including quotes.
    pub text: String,
}

/// Narrow or wide string literal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrKind {
    /// `"…"`
    Narrow,
    /// `L"…"`
    Wide,
}

/// A decoded string literal.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StrLit {
    /// Narrow or wide.
    pub kind: StrKind,
    /// The decoded elements: bytes (`0..=255`) for a narrow literal, wide
    /// character values for a wide one. The terminating NUL is *not* included.
    pub values: Vec<u32>,
    /// The exact source spelling, including quotes.
    pub text: String,
}

impl StrLit {
    /// The literal's bytes, if it is narrow.
    pub fn as_bytes(&self) -> Option<Vec<u8>> {
        (self.kind == StrKind::Narrow).then(|| self.values.iter().map(|v| *v as u8).collect())
    }
}

/// What a [`Token`] is.
#[derive(Clone, PartialEq, Debug)]
pub enum TokenKind {
    /// End of the token list. Always present, exactly once, last.
    Eof,
    /// An identifier that is not a keyword.
    Ident(String),
    /// A keyword.
    Keyword(Keyword),
    /// An integer constant.
    Int(IntLit),
    /// A floating constant.
    Float(FloatLit),
    /// A character constant.
    Char(CharLit),
    /// A string literal.
    Str(StrLit),
    /// A punctuator.
    Punct(Punct),
    /// Text that is not a C token at all, kept so that a skipped group may
    /// contain it. Holds its own spelling.
    Error(String),
}

impl TokenKind {
    /// A short description used in "expected …, found …" messages.
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Eof => "end of input".to_owned(),
            TokenKind::Ident(name) => format!("identifier '{name}'"),
            TokenKind::Keyword(k) => format!("keyword '{}'", k.as_str()),
            TokenKind::Int(_) => "integer constant".to_owned(),
            TokenKind::Float(_) => "floating constant".to_owned(),
            TokenKind::Char(_) => "character constant".to_owned(),
            TokenKind::Str(_) => "string literal".to_owned(),
            TokenKind::Punct(p) => format!("'{}'", p.as_str()),
            TokenKind::Error(text) => format!("'{text}'"),
        }
    }

    /// The exact source spelling of this token.
    ///
    /// This is what `#` stringifies and what `##` pastes, so it has to be the
    /// text as written — `0x1f` rather than `31`, `'\n'` rather than a
    /// newline. [`TokenKind::Eof`] has no spelling and gives `""`.
    pub fn spelling(&self) -> &str {
        match self {
            TokenKind::Eof => "",
            TokenKind::Ident(name) => name,
            TokenKind::Keyword(k) => k.as_str(),
            TokenKind::Int(lit) => &lit.text,
            TokenKind::Float(lit) => &lit.text,
            TokenKind::Char(lit) => &lit.text,
            TokenKind::Str(lit) => &lit.text,
            TokenKind::Punct(p) => p.as_str(),
            TokenKind::Error(text) => text,
        }
    }

    /// The name this token has when it is used as a macro name.
    ///
    /// Keywords are ordinary identifiers during translation phase 4 — our
    /// lexer classifies them early, so `#define restrict` and `#ifdef inline`
    /// would otherwise be unusable — which is why a keyword answers with its
    /// spelling here.
    pub fn macro_name(&self) -> Option<&str> {
        match self {
            TokenKind::Ident(name) => Some(name),
            TokenKind::Keyword(k) => Some(k.as_str()),
            _ => None,
        }
    }
}

/// A lexed C token.
#[derive(Clone, PartialEq, Debug)]
pub struct Token {
    /// What the token is.
    pub kind: TokenKind,
    /// Where it is.
    pub range: SourceRange,
    /// Whether it is the first token on its logical line (needed by the
    /// preprocessor to spot directives).
    pub bol: bool,
    /// Whether whitespace or a comment preceded it (needed by the
    /// preprocessor's stringification and macro replacement).
    pub preceded_by_space: bool,
    /// Everything wrong with this token, reported by the
    /// [preprocessor](crate::pp) if and only if the token reaches its output.
    ///
    /// An error found in the whitespace *before* a token — an unterminated
    /// comment — is attached to that token, which is why this is never lost:
    /// the end-of-input token always survives.
    pub errors: Vec<Diagnostic>,
}

impl Token {
    /// The keyword this token is, if any.
    pub fn keyword(&self) -> Option<Keyword> {
        match &self.kind {
            TokenKind::Keyword(k) => Some(*k),
            _ => None,
        }
    }

    /// Whether this token is the given punctuator.
    pub fn is_punct(&self, p: Punct) -> bool {
        self.kind == TokenKind::Punct(p)
    }

    /// Whether this token is the given keyword.
    pub fn is_keyword(&self, k: Keyword) -> bool {
        self.kind == TokenKind::Keyword(k)
    }

    /// Whether this token ends the input.
    pub fn is_eof(&self) -> bool {
        self.kind == TokenKind::Eof
    }

    /// The identifier this token is, if any.
    pub fn ident(&self) -> Option<&str> {
        match &self.kind {
            TokenKind::Ident(name) => Some(name),
            _ => None,
        }
    }
}

/// Knobs for the lexer.
#[derive(Clone, Copy, Debug)]
pub struct LexOptions {
    /// Which standard's lexical rules to apply.
    pub standard: Standard,
    /// How a constant form a newer revision introduced is gated.
    pub gating: crate::Gating,
    /// Accept `$` in identifiers, like GCC's `-fdollars-in-identifiers`.
    pub dollar_in_identifiers: bool,
}

impl LexOptions {
    /// The lexical rules of `standard`, in the strict ISO dialect.
    pub fn new(standard: Standard) -> Self {
        Self {
            standard,
            gating: crate::Gating {
                standard,
                dialect: crate::Dialect::Iso,
            },
            dollar_in_identifiers: false,
        }
    }
}

impl From<&Options> for LexOptions {
    fn from(o: &Options) -> Self {
        Self {
            standard: o.standard,
            gating: o.gating(),
            dollar_in_identifiers: o.dollar_in_identifiers,
        }
    }
}

/// Lexes the root file of `source`.
///
/// The returned vector always ends with a [`TokenKind::Eof`] token. Problems
/// are attached to the tokens they were found in ([`Token::errors`]) rather
/// than reported; scanning always runs to the end of the input.
pub fn lex(source: &Source, options: &Options) -> Vec<Token> {
    let file = source.map.file(source.root);
    lex_text(file.text(), file.base(), &options.into())
}

/// Lexes `text`, whose first byte lives at global offset `base`.
pub fn lex_text(text: &str, base: Pos, options: &LexOptions) -> Vec<Token> {
    Lexer {
        text,
        bytes: text.as_bytes(),
        base,
        pos: 0,
        options: *options,
        pending: Vec::new(),
    }
    .run()
}

struct Lexer<'a> {
    text: &'a str,
    bytes: &'a [u8],
    base: Pos,
    pos: usize,
    options: LexOptions,
    /// Problems found since the last token was finished; they belong to the
    /// token currently being scanned.
    pending: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn range(&self, start: usize, end: usize) -> SourceRange {
        SourceRange::new(self.base + start as Pos, self.base + end as Pos)
    }

    /// Records a problem with the token being scanned.
    fn error(&mut self, range: SourceRange, message: impl Into<String>) {
        self.pending.push(Diagnostic::error(range, message));
    }

    /// Records an advisory remark about the token being scanned.
    fn warning(&mut self, range: SourceRange, message: impl Into<String>) {
        self.pending.push(Diagnostic::warning(range, message));
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<u8> {
        self.bytes.get(self.pos + n).copied()
    }

    fn eof(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn run(mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut bol = true;
        let mut space = false;
        loop {
            let (saw_newline, saw_space) = self.skip_whitespace();
            bol |= saw_newline;
            space |= saw_space || saw_newline;
            if self.eof() {
                let end = self.bytes.len();
                tokens.push(Token {
                    kind: TokenKind::Eof,
                    range: self.range(end, end),
                    bol,
                    preceded_by_space: space,
                    errors: std::mem::take(&mut self.pending),
                });
                break;
            }
            let start = self.pos;
            let kind = self.scan_token();
            tokens.push(Token {
                kind,
                range: self.range(start, self.pos),
                bol,
                preceded_by_space: space,
                errors: std::mem::take(&mut self.pending),
            });
            bol = false;
            space = false;
        }
        tokens
    }

    /// Skips whitespace, comments and line splices.
    ///
    /// Returns `(saw_newline, saw_space)`.
    fn skip_whitespace(&mut self) -> (bool, bool) {
        let mut newline = false;
        let mut space = false;
        loop {
            match self.peek() {
                Some(b'\n') => {
                    self.pos += 1;
                    newline = true;
                }
                Some(b' ' | b'\t' | b'\r' | 0x0b | 0x0c) => {
                    self.pos += 1;
                    space = true;
                }
                // Translation phase 2: a backslash immediately followed by a
                // newline splices the two lines, so it is *not* a line break.
                Some(b'\\') if self.is_line_splice(self.pos) => {
                    self.pos += self.line_splice_len(self.pos);
                    space = true;
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => {
                    // Translation phase 3 replaces the whole comment with one
                    // space, so a newline *inside* it is not a line break at
                    // all: a directive may span one, and a `#` after one does
                    // not start a directive. Both are what GCC does, and the
                    // standard's own `FUNC_LIKE` example in 6.10.3 depends on
                    // the first.
                    let start = self.pos;
                    self.pos += 2;
                    let mut closed = false;
                    while let Some(c) = self.peek() {
                        if c == b'*' && self.peek_at(1) == Some(b'/') {
                            self.pos += 2;
                            closed = true;
                            break;
                        }
                        self.pos += 1;
                    }
                    if !closed {
                        let range = self.range(start, self.bytes.len());
                        self.error(range, "unterminated comment");
                    }
                    space = true;
                }
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    self.pos += 2;
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        if c == b'\\' && self.is_line_splice(self.pos) {
                            self.pos += self.line_splice_len(self.pos);
                            continue;
                        }
                        self.pos += 1;
                    }
                    space = true;
                }
                _ => return (newline, space),
            }
        }
    }

    fn is_line_splice(&self, at: usize) -> bool {
        self.line_splice_len(at) > 0
    }

    /// Length of a `\`-newline splice starting at `at`, or 0.
    fn line_splice_len(&self, at: usize) -> usize {
        if self.bytes.get(at) != Some(&b'\\') {
            return 0;
        }
        match (self.bytes.get(at + 1), self.bytes.get(at + 2)) {
            (Some(b'\n'), _) => 2,
            (Some(b'\r'), Some(b'\n')) => 3,
            _ => 0,
        }
    }

    /// Scans one token, which is [`TokenKind::Error`] for text that is not a C
    /// token at all.
    fn scan_token(&mut self) -> TokenKind {
        let Some(c) = self.peek() else {
            return TokenKind::Eof;
        };
        if is_ident_start(c, self.options.dollar_in_identifiers) {
            // `L'x'` / `L"…"` are wide constants, not an identifier.
            if c == b'L' {
                match self.peek_at(1) {
                    Some(b'\'') => {
                        self.pos += 1;
                        return self.scan_char_constant(true);
                    }
                    Some(b'"') => {
                        self.pos += 1;
                        return self.scan_string_literal(StrKind::Wide);
                    }
                    _ => {}
                }
            }
            // `u8"…"`, `u"…"` and `U"…"` are C11's UTF-8 and UTF-16/32
            // literals, which this crate has no `char16_t`/`char32_t` to
            // decode into. Recognising the prefix is what turns them into one
            // clear diagnostic instead of a syntax error about an identifier
            // followed by a string.
            if matches!(c, b'u' | b'U') && self.options.standard >= Standard::C11 {
                let len = if c == b'u' && self.peek_at(1) == Some(b'8') {
                    2
                } else {
                    1
                };
                if matches!(self.peek_at(len), Some(b'"') | Some(b'\'')) {
                    let start = self.pos;
                    self.pos += len;
                    let prefix = self.text[start..self.pos].to_owned();
                    let range = self.range(start, self.pos);
                    self.error(
                        range,
                        format!("'{prefix}' literals are not supported; use a narrow literal"),
                    );
                    return if self.peek() == Some(b'\'') {
                        self.scan_char_constant(false)
                    } else {
                        self.scan_string_literal(StrKind::Narrow)
                    };
                }
            }
            return self.scan_ident();
        }
        if c.is_ascii_digit() || (c == b'.' && self.peek_at(1).is_some_and(|d| d.is_ascii_digit()))
        {
            return self.scan_number();
        }
        if c == b'\'' {
            return self.scan_char_constant(false);
        }
        if c == b'"' {
            return self.scan_string_literal(StrKind::Narrow);
        }
        if let Some(p) = self.scan_punctuator() {
            return TokenKind::Punct(p);
        }

        // Anything else is not a C token at all.
        let start = self.pos;
        let ch = self.text[start..].chars().next().unwrap_or('\u{fffd}');
        self.pos += ch.len_utf8();
        let range = self.range(start, self.pos);
        self.error(
            range,
            format!("unexpected character '{}' in program", ch.escape_debug()),
        );
        TokenKind::Error(ch.to_string())
    }

    /// Scans an identifier or a keyword.
    ///
    /// Translation phase 2 deletes a backslash-newline *before* the source is
    /// split into tokens, so one may sit in the middle of an identifier:
    /// `__LI\<newline>NE__` is `__LINE__`, and Clang's own `drs/dr464.c`
    /// writes exactly that. Almost no identifier has one, so the spelling
    /// stays a slice of the source until the first splice is found and only
    /// then becomes a `String`.
    fn scan_ident(&mut self) -> TokenKind {
        let start = self.pos;
        // The text before the current splice, when there has been one.
        let mut spliced: Option<String> = None;
        // Where the run of characters that is still a slice begins.
        let mut segment = start;
        loop {
            match self.peek() {
                Some(c) if is_ident_continue(c, self.options.dollar_in_identifiers) => {
                    self.pos += 1;
                }
                // Only a splice that the identifier *continues* over: one at
                // the end of it is whitespace, and belongs to whatever comes
                // next.
                Some(b'\\')
                    if self.line_splice_len(self.pos) > 0
                        && self
                            .bytes
                            .get(self.pos + self.line_splice_len(self.pos))
                            .is_some_and(|c| {
                                is_ident_continue(*c, self.options.dollar_in_identifiers)
                            }) =>
                {
                    let text = spliced.get_or_insert_with(String::new);
                    text.push_str(&self.text[segment..self.pos]);
                    self.pos += self.line_splice_len(self.pos);
                    segment = self.pos;
                }
                _ => break,
            }
        }
        let joined;
        let text = match spliced {
            Some(mut text) => {
                text.push_str(&self.text[segment..self.pos]);
                joined = text;
                joined.as_str()
            }
            None => &self.text[start..self.pos],
        };
        match Keyword::from_str(text, self.options.standard) {
            Some(k) => TokenKind::Keyword(k),
            None => TokenKind::Ident(text.to_owned()),
        }
    }

    fn scan_punctuator(&mut self) -> Option<Punct> {
        let rest = &self.text[self.pos..];
        for (spelling, punct) in PUNCTUATORS {
            if rest.starts_with(spelling) {
                // `<:` is a digraph for `[`, but `1<::x` must not be mangled;
                // C99 has no `::`, so a plain greedy match is correct here.
                self.pos += spelling.len();
                return Some(*punct);
            }
        }
        None
    }

    // -- numbers ------------------------------------------------------------

    /// Scans a preprocessing number and classifies it as integer or float.
    ///
    /// Scanning the whole pp-number first (rather than stopping at the first
    /// character that does not fit) is what lets `08` or `1.0q` be reported as
    /// one bad token instead of two good ones.
    fn scan_number(&mut self) -> TokenKind {
        let start = self.pos;
        if self.peek() == Some(b'.') {
            self.pos += 1;
        }
        self.pos += 1;
        let mut separators = false;
        while let Some(c) = self.peek() {
            if matches!(c, b'e' | b'E' | b'p' | b'P')
                && matches!(self.peek_at(1), Some(b'+') | Some(b'-'))
            {
                self.pos += 2;
                continue;
            }
            // C23's digit separator. It is part of the pp-number when a digit
            // or a nondigit follows it, which is what keeps `1'a'` from
            // swallowing the character constant that follows a constant.
            if c == b'\''
                && self
                    .peek_at(1)
                    .is_some_and(|d| d.is_ascii_alphanumeric() || d == b'_')
            {
                separators = true;
                self.pos += 1;
                continue;
            }
            if c.is_ascii_alphanumeric()
                || c == b'_'
                || c == b'.'
                || (c == b'$' && self.options.dollar_in_identifiers)
            {
                self.pos += 1;
                continue;
            }
            break;
        }
        let text = &self.text[start..self.pos];
        let range = self.range(start, self.pos);
        if separators
            && let Some(message) = self
                .options
                .gating
                .requires("a digit separator", Standard::C23)
        {
            self.error(range, message);
        }
        // Everything below reads the digits; the separators are not part of
        // the value, while `text` keeps the spelling `#` has to reproduce.
        let stripped: String;
        let digits = if separators {
            stripped = text.replace('\'', "");
            stripped.as_str()
        } else {
            text
        };
        let lower = digits.to_ascii_lowercase();
        let hex = lower.starts_with("0x");
        let is_float = if hex {
            digits.contains('.') || lower[2..].contains('p')
        } else {
            digits.contains('.') || (!lower.starts_with("0b") && lower.contains('e'))
        };
        if is_float {
            TokenKind::Float(self.decode_float(digits, text, range, hex))
        } else {
            TokenKind::Int(self.decode_int(digits, text, range))
        }
    }

    /// Decodes an integer constant from its separator-free `digits`; `text` is
    /// the spelling as written, which is what diagnostics quote and what `#`
    /// reproduces.
    fn decode_int(&mut self, digits: &str, text: &str, range: SourceRange) -> IntLit {
        let bytes = digits.as_bytes();
        let (base, digits_start) =
            if digits.len() >= 2 && (bytes[1] | 0x20) == b'x' && bytes[0] == b'0' {
                (NumBase::Hex, 2)
            } else if digits.len() >= 2 && (bytes[1] | 0x20) == b'b' && bytes[0] == b'0' {
                (NumBase::Binary, 2)
            } else if bytes[0] == b'0' && digits.len() > 1 {
                (NumBase::Octal, 1)
            } else {
                (NumBase::Decimal, 0)
            };
        if base == NumBase::Binary
            && let Some(message) = self
                .options
                .gating
                .requires("a binary integer constant", Standard::C23)
        {
            self.error(range, message);
        }

        let radix = base.radix();
        // Accept every decimal digit in an octal or binary constant so that
        // the whole constant is consumed and reported once, the way C
        // compilers do.
        let scan_radix = if radix < 10 { 10 } else { radix };

        let mut i = digits_start;
        let mut value: u128 = 0;
        let mut overflow = false;
        let mut bad_digit: Option<char> = None;
        while i < bytes.len() {
            let c = bytes[i] as char;
            let digit = match c.to_digit(scan_radix) {
                Some(d) => d,
                None => break,
            };
            if digit >= radix && bad_digit.is_none() {
                bad_digit = Some(c);
            }
            match value
                .checked_mul(radix as u128)
                .and_then(|v| v.checked_add(digit as u128))
            {
                Some(v) => value = v,
                None => overflow = true,
            }
            i += 1;
        }

        if i == digits_start && matches!(base, NumBase::Hex | NumBase::Binary) {
            let prefix = &digits[..2];
            self.error(
                range,
                format!(
                    "expected digits after '{prefix}' in {} constant",
                    base.as_str()
                ),
            );
        }
        if let Some(c) = bad_digit {
            self.error(
                range,
                format!("invalid digit '{c}' in {} constant '{text}'", base.as_str()),
            );
        }
        if overflow {
            self.error(range, format!("integer constant '{text}' is too large"));
        }

        let suffix = &digits[i..];
        let (unsigned, long) = match parse_int_suffix(suffix) {
            Some(v) => v,
            None => {
                self.error(
                    range,
                    format!("invalid suffix '{suffix}' on integer constant '{text}'"),
                );
                (false, LongKind::None)
            }
        };

        IntLit {
            value,
            base,
            unsigned,
            long,
            text: text.to_owned(),
        }
    }

    fn decode_float(
        &mut self,
        digits: &str,
        text: &str,
        range: SourceRange,
        hex: bool,
    ) -> FloatLit {
        let (body, suffix) = split_float_suffix(digits);
        let suffix_kind = match suffix {
            "" => FloatSuffix::None,
            "f" | "F" => FloatSuffix::Float,
            "l" | "L" => FloatSuffix::LongDouble,
            _ => {
                self.error(
                    range,
                    format!("invalid suffix '{suffix}' on floating constant '{text}'"),
                );
                FloatSuffix::None
            }
        };

        let value = if hex {
            match parse_hex_float(body) {
                Some(v) => v,
                None => {
                    self.error(
                        range,
                        format!(
                            "invalid hexadecimal floating constant '{text}'; \
                             a 'p' exponent is required"
                        ),
                    );
                    0.0
                }
            }
        } else {
            match parse_decimal_float(body) {
                Some(v) => v,
                None => {
                    self.error(range, format!("invalid floating constant '{text}'"));
                    0.0
                }
            }
        };

        FloatLit {
            value,
            suffix: suffix_kind,
            hex,
            text: text.to_owned(),
        }
    }

    // -- character and string constants -------------------------------------

    fn scan_char_constant(&mut self, wide: bool) -> TokenKind {
        let start = self.pos;
        debug_assert_eq!(self.peek(), Some(b'\''));
        self.pos += 1;
        let mut values: Vec<u32> = Vec::new();
        let mut terminated = false;
        while let Some(c) = self.peek() {
            if c == b'\'' {
                self.pos += 1;
                terminated = true;
                break;
            }
            if c == b'\n' {
                break;
            }
            self.read_char_element(wide, &mut values);
        }
        let range = self.range(start, self.pos);
        if !terminated {
            self.error(range, "missing terminating \' character");
        }
        if values.is_empty() {
            self.error(range, "empty character constant");
        }
        if values.len() > 1 {
            self.warning(range, "multi-character character constant");
        }

        let value = if wide {
            values.last().copied().unwrap_or(0) as i64
        } else if values.len() <= 1 {
            values.first().copied().unwrap_or(0) as i64
        } else {
            // GCC packs the bytes big-endian into an `int`.
            let mut v: u32 = 0;
            for b in &values {
                v = (v << 8) | (*b & 0xff);
            }
            v as i32 as i64
        };

        TokenKind::Char(CharLit {
            value,
            wide,
            text: self.text[start..self.pos].to_owned(),
        })
    }

    fn scan_string_literal(&mut self, kind: StrKind) -> TokenKind {
        let start = self.pos;
        debug_assert_eq!(self.peek(), Some(b'"'));
        self.pos += 1;
        let wide = kind == StrKind::Wide;
        let mut values: Vec<u32> = Vec::new();
        let mut terminated = false;
        while let Some(c) = self.peek() {
            if c == b'"' {
                self.pos += 1;
                terminated = true;
                break;
            }
            if c == b'\n' {
                break;
            }
            self.read_char_element(wide, &mut values);
        }
        let range = self.range(start, self.pos);
        if !terminated {
            self.error(range, "missing terminating \" character");
        }
        TokenKind::Str(StrLit {
            kind,
            values,
            text: self.text[start..self.pos].to_owned(),
        })
    }

    /// Reads one element of a character constant or string literal, appending
    /// its decoded value(s) to `out`.
    fn read_char_element(&mut self, wide: bool, out: &mut Vec<u32>) {
        if self.is_line_splice(self.pos) {
            self.pos += self.line_splice_len(self.pos);
            return;
        }
        let start = self.pos;
        if self.peek() != Some(b'\\') {
            if wide {
                let ch = self.text[start..].chars().next().unwrap_or('\u{fffd}');
                self.pos += ch.len_utf8();
                out.push(ch as u32);
            } else {
                // Narrow literals keep the raw execution-charset bytes, so
                // UTF-8 text in a literal survives byte for byte.
                self.pos += 1;
                out.push(self.bytes[start] as u32);
            }
            return;
        }

        self.pos += 1;
        let Some(e) = self.peek() else {
            let range = self.range(start, self.pos);
            self.error(range, "incomplete escape sequence");
            return;
        };
        self.pos += 1;
        let simple = match e {
            b'\'' => Some(0x27),
            b'"' => Some(0x22),
            b'?' => Some(0x3f),
            b'\\' => Some(0x5c),
            b'a' => Some(0x07),
            b'b' => Some(0x08),
            // `\e` is GNU's escape for ESC. GCC accepts it in every mode (with
            // a pedantic warning), and `"\e[0m"` is how a program writes a
            // terminal colour; refusing it would be refusing the extension in
            // the one place a strict mode still has it.
            b'e' => Some(0x1b),
            b'f' => Some(0x0c),
            b'n' => Some(0x0a),
            b'r' => Some(0x0d),
            b't' => Some(0x09),
            b'v' => Some(0x0b),
            _ => None,
        };
        if let Some(v) = simple {
            out.push(v);
            return;
        }
        match e {
            b'0'..=b'7' => {
                let mut v: u32 = (e - b'0') as u32;
                for _ in 0..2 {
                    match self.peek() {
                        Some(d @ b'0'..=b'7') => {
                            v = v * 8 + (d - b'0') as u32;
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                self.push_escape_value(v, wide, start, out);
            }
            b'x' => {
                let mut v: u32 = 0;
                let mut any = false;
                let mut overflow = false;
                while let Some(d) = self.peek().and_then(|c| (c as char).to_digit(16)) {
                    any = true;
                    v = match v.checked_mul(16).and_then(|v| v.checked_add(d)) {
                        Some(v) => v,
                        None => {
                            overflow = true;
                            v
                        }
                    };
                    self.pos += 1;
                }
                let range = self.range(start, self.pos);
                if !any {
                    self.error(range, "'\\x' used with no following hex digits");
                } else if overflow {
                    self.error(range, "hex escape sequence out of range");
                }
                self.push_escape_value(v, wide, start, out);
            }
            b'u' | b'U' => {
                let want = if e == b'u' { 4 } else { 8 };
                let mut v: u32 = 0;
                let mut count = 0;
                while count < want {
                    match self.peek().and_then(|c| (c as char).to_digit(16)) {
                        Some(d) => {
                            v = v.wrapping_mul(16).wrapping_add(d);
                            self.pos += 1;
                            count += 1;
                        }
                        None => break,
                    }
                }
                let range = self.range(start, self.pos);
                if count != want {
                    self.error(
                        range,
                        format!("incomplete universal character name; expected {want} hex digits"),
                    );
                    return;
                }
                match char::from_u32(v) {
                    Some(ch) if wide => out.push(ch as u32),
                    Some(ch) => {
                        // The execution character set is UTF-8.
                        let mut buf = [0u8; 4];
                        for b in ch.encode_utf8(&mut buf).as_bytes() {
                            out.push(*b as u32);
                        }
                    }
                    None => {
                        self.error(
                            range,
                            format!("'\\u{v:04X}' is not a valid universal character name"),
                        );
                    }
                }
            }
            _ => {
                let range = self.range(start, self.pos);
                self.error(
                    range,
                    format!("unknown escape sequence '\\{}'", (e as char).escape_debug()),
                );
                out.push(e as u32);
            }
        }
    }

    fn push_escape_value(&mut self, v: u32, wide: bool, start: usize, out: &mut Vec<u32>) {
        if !wide && v > 0xff {
            let range = self.range(start, self.pos);
            self.error(range, "escape sequence out of range for type 'char'");
            out.push(v & 0xff);
        } else {
            out.push(v);
        }
    }
}

fn is_ident_start(c: u8, dollar: bool) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || (dollar && c == b'$')
}

fn is_ident_continue(c: u8, dollar: bool) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || (dollar && c == b'$')
}

/// Validates an integer suffix, returning `(unsigned, long_kind)`.
fn parse_int_suffix(s: &str) -> Option<(bool, LongKind)> {
    if s.is_empty() {
        return Some((false, LongKind::None));
    }
    let b = s.as_bytes();
    let mut i = 0;
    let mut unsigned = false;
    let mut long = LongKind::None;

    if b[i] == b'u' || b[i] == b'U' {
        unsigned = true;
        i += 1;
    }
    if i < b.len() && (b[i] == b'l' || b[i] == b'L') {
        // `ll` and `LL` must not be mixed as `lL` or `Ll`.
        if i + 1 < b.len() && b[i + 1] == b[i] {
            long = LongKind::LongLong;
            i += 2;
        } else {
            long = LongKind::Long;
            i += 1;
        }
    }
    if !unsigned && i < b.len() && (b[i] == b'u' || b[i] == b'U') {
        unsigned = true;
        i += 1;
    }
    (i == b.len()).then_some((unsigned, long))
}

/// Splits a floating constant into its numeric body and its suffix.
fn split_float_suffix(text: &str) -> (&str, &str) {
    let b = text.as_bytes();
    let mut i = b.len();
    while i > 0 && b[i - 1].is_ascii_alphabetic() {
        // The `p`/`e` of an exponent is part of the body, not the suffix.
        let c = b[i - 1] | 0x20;
        if (c == b'p' || c == b'e') && i < b.len() {
            break;
        }
        i -= 1;
    }
    // Only ever treat a trailing `f`/`l` (one character) as a suffix; anything
    // longer is reported as an invalid suffix by the caller.
    (&text[..i], &text[i..])
}

/// Parses the numeric body of a decimal floating constant.
fn parse_decimal_float(body: &str) -> Option<f64> {
    if body.is_empty() {
        return None;
    }
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], &body[i + 1..]),
        None => (body, ""),
    };
    let (int_part, frac_part) = match mantissa.find('.') {
        Some(i) => (&mantissa[..i], &mantissa[i + 1..]),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.bytes().all(|c| c.is_ascii_digit())
        || !frac_part.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let exponent = if exponent.is_empty() {
        0i32
    } else {
        let (sign, digits) = match exponent.as_bytes()[0] {
            b'+' => (1, &exponent[1..]),
            b'-' => (-1, &exponent[1..]),
            _ => (1, exponent),
        };
        if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        // Saturate: an absurd exponent simply becomes 0 or infinity.
        sign * digits.parse::<i32>().unwrap_or(i32::MAX / 2)
    };
    let normalized = format!(
        "{}.{}e{}",
        if int_part.is_empty() { "0" } else { int_part },
        if frac_part.is_empty() { "0" } else { frac_part },
        exponent
    );
    normalized.parse::<f64>().ok()
}

/// Parses the numeric body of a hexadecimal floating constant (`0x1.8p3`).
fn parse_hex_float(body: &str) -> Option<f64> {
    let rest = body
        .strip_prefix("0x")
        .or_else(|| body.strip_prefix("0X"))?;
    let p = rest.find(['p', 'P'])?;
    let (mantissa, exponent) = (&rest[..p], &rest[p + 1..]);
    let (int_part, frac_part) = match mantissa.find('.') {
        Some(i) => (&mantissa[..i], &mantissa[i + 1..]),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let mut value = 0f64;
    for c in int_part.chars() {
        value = value * 16.0 + c.to_digit(16)? as f64;
    }
    let mut scale = 1.0 / 16.0;
    for c in frac_part.chars() {
        value += c.to_digit(16)? as f64 * scale;
        scale /= 16.0;
    }
    let (sign, digits) = match exponent.as_bytes().first() {
        Some(b'+') => (1i32, &exponent[1..]),
        Some(b'-') => (-1i32, &exponent[1..]),
        _ => (1i32, exponent),
    };
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let exp = sign * digits.parse::<i32>().unwrap_or(i32::MAX / 2);
    Some(value * 2f64.powi(exp))
}
