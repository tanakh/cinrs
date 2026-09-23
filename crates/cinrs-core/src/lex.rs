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
use crate::{Dialect, Options, Standard};

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
    /// `__int128`
    ///
    /// A type specifier of its own, which `signed` and `unsigned` combine
    /// with; the `__int128_t` and `__uint128_t` spellings are `typedef` names
    /// the compiler owns rather than keywords, exactly as they are in GCC.
    Int128,
    /// `__real__`, `__real`
    RealGnu,
    /// `__imag__`, `__imag`
    ImagGnu,
    /// `__inline__`, `__inline`
    ///
    /// A variant of its own because the plain spelling is C99's and `c89!`
    /// gates it, while this one — being reserved — is available everywhere.
    InlineGnu,
    /// `__restrict__`, `__restrict`; see [`Keyword::InlineGnu`].
    RestrictGnu,
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
            Int128 => "__int128",
            RealGnu => "__real__",
            ImagGnu => "__imag__",
            InlineGnu => "__inline__",
            RestrictGnu => "__restrict__",
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
                | Int128
                | RealGnu
                | ImagGnu
                | InlineGnu
                | RestrictGnu
        )
    }

    /// The revision that made this spelling a keyword.
    ///
    /// The C11 keywords are recognised in every mode — they are reserved
    /// identifiers in C99, so nothing legal can be broken by it, and the
    /// parser's "requires C11 or later" is a better answer than a syntax
    /// error. The C23 ones are *not*: they are ordinary identifiers before
    /// C23, and `<stdbool.h>`'s `#define bool _Bool` depends on it.
    ///
    /// The four C99 added — `inline`, `restrict`, `_Bool` and `_Complex` (with
    /// `_Imaginary` beside it) — are recognised in every mode for the same
    /// reason the C11 ones are, and gated where they are parsed; everything
    /// else has been a keyword since C89.
    pub fn since(self) -> Standard {
        use Keyword::*;
        match self {
            Alignas | Alignof | Atomic | Generic | Noreturn | StaticAssert | ThreadLocal => {
                Standard::C11
            }
            BitInt | BoolName | True | False | Nullptr | Typeof | TypeofUnqual | Constexpr
            | StaticAssertName | AlignofName | AlignasName | ThreadLocalName => Standard::C23,
            Inline | Restrict | Bool | Complex | Imaginary => Standard::C99,
            _ => Standard::C89,
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
    /// Whether the constant carried GNU's imaginary suffix, `i` or `j`.
    ///
    /// `2.0i` is `(0, 2)` of the complex type [`FloatLit::suffix`] names the
    /// real part of, which is what makes `1.0 + 2.0i` read the way every C
    /// program writes a complex constant. It is a GNU extension that C11
    /// blessed by giving `<complex.h>` a `CMPLX` built on it; the *standard*
    /// spelling of the same thing is `_Complex_I`.
    pub imaginary: bool,
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
    /// Which prefix the constant was written with, which is what fixes its
    /// type: `'x'` is an `int`, `L'x'` a `wchar_t`, `u'x'` a `char16_t`,
    /// `U'x'` a `char32_t` and `u8'x'` a `char8_t`.
    pub kind: StrKind,
    /// The exact source spelling, including quotes.
    pub text: String,
}

/// Which prefix a character constant or string literal was written with.
///
/// The five of them are the same set for both, which is why one enum serves
/// both: `'x'`/`"…"`, `u8'x'`/`u8"…"`, `u'x'`/`u"…"`, `U'x'`/`U"…"` and
/// `L'x'`/`L"…"`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrKind {
    /// `"…"` — bytes of the execution character set, which is UTF-8.
    Narrow,
    /// `u8"…"` (C11) and `u8'x'` (C23) — UTF-8 bytes, of type `char` before
    /// C23 and `char8_t` (an `unsigned char`) from C23 on.
    Utf8,
    /// `u"…"` and `u'x'` (C11) — UTF-16 code units, of type `char16_t`.
    Utf16,
    /// `U"…"` and `U'x'` (C11) — UTF-32 code units, of type `char32_t`.
    Utf32,
    /// `L"…"` — `wchar_t`.
    Wide,
}

impl StrKind {
    /// The prefix a literal of this kind is written with.
    pub fn prefix(self) -> &'static str {
        match self {
            StrKind::Narrow => "",
            StrKind::Utf8 => "u8",
            StrKind::Utf16 => "u",
            StrKind::Utf32 => "U",
            StrKind::Wide => "L",
        }
    }

    /// The revision that introduced the prefix, in the form the constant is
    /// being used in.
    ///
    /// `u8"…"` is C11 (N1488) and `u8'x'` is C23 (N2418); the other three
    /// prefixes are the same revision either way.
    pub fn since(self, character: bool) -> Standard {
        match self {
            StrKind::Narrow | StrKind::Wide => Standard::C89,
            StrKind::Utf8 if character => Standard::C23,
            StrKind::Utf8 | StrKind::Utf16 | StrKind::Utf32 => Standard::C11,
        }
    }

    /// Whether the elements are the bytes of the source's UTF-8, rather than
    /// character values.
    fn is_bytes(self) -> bool {
        matches!(self, StrKind::Narrow | StrKind::Utf8)
    }

    /// The largest value one element of this kind can hold.
    ///
    /// `wide_bits` is how wide the target's `wchar_t` is, which is the one
    /// answer here that is not fixed by the language: 32 bits on the Unix
    /// platforms and 16 on Windows, where `L"…"` is UTF-16 and a character
    /// outside the basic multilingual plane takes a surrogate pair, exactly as
    /// `u"…"` does.
    fn max_element(self, wide_bits: u32) -> u32 {
        match self {
            StrKind::Narrow | StrKind::Utf8 => 0xff,
            StrKind::Utf16 => 0xffff,
            StrKind::Wide if wide_bits <= 16 => 0xffff,
            StrKind::Utf32 | StrKind::Wide => u32::MAX,
        }
    }

    /// Whether one element holds a UTF-16 code unit, so that a character
    /// beyond the basic multilingual plane becomes a surrogate pair.
    fn is_utf16(self, wide_bits: u32) -> bool {
        self == StrKind::Utf16 || (self == StrKind::Wide && wide_bits <= 16)
    }
}

/// A decoded string literal.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StrLit {
    /// Which prefix it was written with.
    pub kind: StrKind,
    /// The decoded elements: bytes (`0..=255`) for a narrow or UTF-8 literal,
    /// UTF-16 code units (surrogate pairs and all) for a `u"…"` one, and
    /// character values for `U"…"` and `L"…"`. The terminating NUL is *not*
    /// included.
    pub values: Vec<u32>,
    /// The exact source spelling, including quotes.
    pub text: String,
}

impl StrLit {
    /// The literal's bytes, if it is an ordinary narrow one.
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
    /// Everything wrong with this token, and with the text between it and the
    /// token before it.
    ///
    /// The [preprocessor](crate::pp) reports what is wrong with the token
    /// *itself* if and only if the token reaches its output. What stands
    /// whatever becomes of the token — its spelling, and the comments that
    /// preceded it — is reported as soon as the token is read from the file,
    /// which is what keeps it from being lost when the token opens a directive,
    /// names a macro or is an argument the macro drops; see
    /// [`Diagnostic::lexical`].
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
    /// Accept `$` in identifiers, like GCC's `-fdollars-in-identifiers`, which
    /// is on by default; see [`crate::Options::dollar_in_identifiers`].
    pub dollar_in_identifiers: bool,
    /// Whether translation phase 1 replaces the nine trigraphs.
    ///
    /// See [`trigraphs_enabled`] for who has them and why.
    pub trigraphs: bool,
    /// How wide the target's `wchar_t` is.
    ///
    /// The one target property the *lexer* has an opinion about: it decides
    /// what an `L'\xffff'` escape may hold and whether `L"😀"` is one element
    /// or a surrogate pair, since Windows makes `wchar_t` 16 bits.
    pub wchar_bits: u32,
    /// Whether the complex types are available, which is what decides whether
    /// an imaginary constant (`2.0i`) has a type; see
    /// [`crate::Options::complex`].
    pub complex: bool,
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
            dollar_in_identifiers: true,
            trigraphs: trigraphs_enabled(standard, crate::Dialect::Iso),
            wchar_bits: crate::TargetModel::host().wchar_bits,
            complex: crate::COMPLEX_SUPPORTED,
        }
    }
}

impl From<&Options> for LexOptions {
    fn from(o: &Options) -> Self {
        Self {
            standard: o.standard,
            gating: o.gating(),
            dollar_in_identifiers: o.dollar_in_identifiers,
            trigraphs: trigraphs_enabled(o.standard, o.dialect),
            wchar_bits: o.target.wchar_bits,
            complex: o.complex,
        }
    }
}

/// Whether translation phase 1 replaces trigraphs in this entry point.
///
/// The nine of them were in C from the beginning and C23 removed them
/// (N2940), so every strict entry point below `c23!` has them and `c23!` does
/// not. No *GNU* dialect has them: `gcc -std=gnu99` switches them off, because
/// `"what??!"` in a string is far more likely to be an exclamation than a
/// pipe, and that is the line Clang draws too.
pub fn trigraphs_enabled(standard: Standard, dialect: crate::Dialect) -> bool {
    standard < Standard::C23 && !dialect.is_gnu()
}

/// The nine trigraphs of C 5.2.1.1, as `(third character, replacement)`.
const TRIGRAPHS: &[(u8, u8)] = &[
    (b'=', b'#'),
    (b'(', b'['),
    (b'/', b'\\'),
    (b')', b']'),
    (b'\'', b'^'),
    (b'<', b'{'),
    (b'!', b'|'),
    (b'>', b'}'),
    (b'-', b'~'),
];

/// Lexes the root file of `source`.
///
/// The returned vector always ends with a [`TokenKind::Eof`] token. Problems
/// are attached to the tokens they were found in ([`Token::errors`]) rather
/// than reported; scanning always runs to the end of the input.
pub fn lex(source: &Source, options: &Options) -> Vec<Token> {
    let file = source.map.file(source.root);
    lex_file(file.text(), file.base(), &options.into())
}

/// The UTF-8 byte order mark, which an editor on Windows may write at the start
/// of a file.
const BYTE_ORDER_MARK: char = '\u{feff}';

/// Lexes the whole of a file's `text` — a unit's own or an `#include`d one —
/// whose first byte lives at global offset `base`.
///
/// A byte order mark at the very start is skipped, as GCC and Clang skip it.
/// It is skipped rather than removed: scanning simply starts three bytes in,
/// so every token's offset, and every column the source map computes from one,
/// is still the file's own. A byte order mark anywhere else is an ordinary
/// stray character and an error, which [`lex_text`] reports.
pub fn lex_file(text: &str, base: Pos, options: &LexOptions) -> Vec<Token> {
    let start = if text.starts_with(BYTE_ORDER_MARK) {
        BYTE_ORDER_MARK.len_utf8()
    } else {
        0
    };
    lex_from(text, base, start, options)
}

/// Lexes `text`, whose first byte lives at global offset `base`.
pub fn lex_text(text: &str, base: Pos, options: &LexOptions) -> Vec<Token> {
    lex_from(text, base, 0, options)
}

/// Lexes `text` from byte `start` on; the tokens' offsets count from the
/// beginning of `text` all the same.
fn lex_from(text: &str, base: Pos, start: usize, options: &LexOptions) -> Vec<Token> {
    Lexer {
        text,
        bytes: text.as_bytes(),
        base,
        pos: start,
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

    /// Records a problem that stands whether or not the token being scanned
    /// ever reaches the parser: one with its *spelling*, or one in the text
    /// between it and the token before it. See [`Diagnostic::lexical`].
    fn lexical_error(&mut self, range: SourceRange, message: impl Into<String>) {
        self.pending
            .push(Diagnostic::error(range, message).at_lexing());
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
                // `??/` is that backslash where trigraphs are on.
                Some(b'\\' | b'?') if self.is_line_splice(self.pos) => {
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
                        // Both of the problems a *comment* can have belong to
                        // the token that follows it for want of anywhere else
                        // to put them, and neither is about that token: whether
                        // a comment is terminated, and whether this revision
                        // has `//`, is settled in translation phase 3 by the
                        // text alone. So they are recorded as lexical, and
                        // reported wherever the token ends up — including
                        // nowhere, which is what a `#` opening a directive and
                        // a macro name do with it.
                        let range = self.range(start, self.bytes.len());
                        self.lexical_error(range, "unterminated comment");
                    }
                    space = true;
                }
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    let start = self.pos;
                    self.pos += 2;
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        if matches!(c, b'\\' | b'?') && self.is_line_splice(self.pos) {
                            self.pos += self.line_splice_len(self.pos);
                            continue;
                        }
                        self.pos += 1;
                    }
                    // C99 took the `//` comment from C++ (N644); before that
                    // `a //* b */ c` was a division, which is why the gate is
                    // here rather than being a warning. Lexical for the reason
                    // above: a `c89!` block that writes one has to be told so
                    // whatever follows it.
                    if let Some(message) = self
                        .options
                        .gating
                        .requires("a '//' comment", Standard::C99)
                    {
                        let range = self.range(start, self.pos);
                        self.lexical_error(range, message);
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
    ///
    /// Translation phase 1 runs *before* phase 2, so `??/` at the end of a
    /// line splices it exactly as a written backslash does — which is the one
    /// trigraph whose replacement is not a character the lexer can simply hand
    /// on.
    fn line_splice_len(&self, at: usize) -> usize {
        let lead = match self.trigraph_at(at) {
            Some(b'\\') => 3,
            Some(_) => return 0,
            None if self.bytes.get(at) == Some(&b'\\') => 1,
            None => return 0,
        };
        match (self.bytes.get(at + lead), self.bytes.get(at + lead + 1)) {
            (Some(b'\n'), _) => lead + 1,
            (Some(b'\r'), Some(b'\n')) => lead + 2,
            _ => 0,
        }
    }

    /// The character a trigraph at `at` stands for, if there is one there.
    ///
    /// C 5.2.1.1: the nine three-character sequences beginning `??` are
    /// replaced in translation phase 1, before line splicing and before the
    /// source is split into tokens — so this is consulted from everywhere the
    /// lexer looks at a raw byte, rather than the text being rewritten. Not
    /// rewriting it is what keeps every [`SourceRange`] a range of the source
    /// the user really wrote: a diagnostic about `??=` points at all three
    /// characters.
    fn trigraph_at(&self, at: usize) -> Option<u8> {
        if !self.options.trigraphs
            || self.bytes.get(at) != Some(&b'?')
            || self.bytes.get(at + 1) != Some(&b'?')
        {
            return None;
        }
        let third = *self.bytes.get(at + 2)?;
        TRIGRAPHS
            .iter()
            .find(|(c, _)| *c == third)
            .map(|(_, replacement)| *replacement)
    }

    /// The length in source bytes of the character at `at`, which is three for
    /// a trigraph and one otherwise.
    fn trigraph_len(&self, at: usize) -> usize {
        if self.trigraph_at(at).is_some() { 3 } else { 1 }
    }

    /// The character-constant or string-literal prefix starting here, with its
    /// length in bytes.
    ///
    /// A prefix is only one when a quote follows it, which is what keeps
    /// `unsigned`, `u8x` and a variable called `U` ordinary identifiers.
    fn literal_prefix(&self, first: u8) -> Option<(StrKind, usize)> {
        let (kind, len) = match first {
            b'L' => (StrKind::Wide, 1),
            b'U' => (StrKind::Utf32, 1),
            b'u' if self.peek_at(1) == Some(b'8') => (StrKind::Utf8, 2),
            b'u' => (StrKind::Utf16, 1),
            _ => return None,
        };
        matches!(self.peek_at(len), Some(b'"' | b'\'')).then_some((kind, len))
    }

    /// Whether an identifier that begins with an extended character starts
    /// here.
    fn extended_ident_start(&self) -> bool {
        match self.peek() {
            Some(b'\\') if matches!(self.peek_at(1), Some(b'u' | b'U')) => {
                let want = if self.peek_at(1) == Some(b'u') { 4 } else { 8 };
                (0..want).all(|i| self.peek_at(2 + i).is_some_and(|c| c.is_ascii_hexdigit()))
            }
            Some(c) if c >= 0x80 => self.text[self.pos..]
                .chars()
                .next()
                .is_some_and(|ch| is_extended_ident_char(ch, true)),
            _ => false,
        }
    }

    /// Scans one token, which is [`TokenKind::Error`] for text that is not a C
    /// token at all.
    fn scan_token(&mut self) -> TokenKind {
        let Some(c) = self.peek() else {
            return TokenKind::Eof;
        };
        if is_ident_start(c, self.options.dollar_in_identifiers) {
            // `L'x'`, `u8"…"`, `u'x'` and the rest are constants rather than an
            // identifier followed by one. The prefix is recognised in every
            // entry point and *gated* rather than not recognised at all: a
            // `c99!` block that writes `u"x"` is told which macro has it,
            // instead of being told that `u` is undeclared.
            if let Some((kind, len)) = self.literal_prefix(c) {
                let start = self.pos;
                self.pos += len;
                let character = self.peek() == Some(b'\'');
                if let Some(message) = self.options.gating.requires(
                    &format!("a '{}' literal", kind.prefix()),
                    kind.since(character),
                ) {
                    let range = self.range(start, self.pos);
                    self.error(range, message);
                }
                return if character {
                    self.scan_char_constant(kind)
                } else {
                    self.scan_string_literal(kind)
                };
            }
            return self.scan_ident();
        }
        // An extended identifier, written either as the character itself —
        // which GCC and Clang have taken since GCC 10 — or as the universal
        // character name C99 6.4.2.1 introduced for it.
        if self.extended_ident_start() {
            return self.scan_ident();
        }
        if c.is_ascii_digit() || (c == b'.' && self.peek_at(1).is_some_and(|d| d.is_ascii_digit()))
        {
            return self.scan_number();
        }
        if c == b'\'' {
            return self.scan_char_constant(StrKind::Narrow);
        }
        if c == b'"' {
            return self.scan_string_literal(StrKind::Narrow);
        }
        if let Some(p) = self.scan_punctuator() {
            return TokenKind::Punct(p);
        }

        // Anything else is not a C token at all.
        let start = self.pos;
        // `??/` that does not splice a line is the stray backslash a written
        // one would be, and is reported as one rather than as two question
        // marks.
        let ch = match self.trigraph_at(start) {
            Some(c) => {
                self.pos += 3;
                c as char
            }
            None => {
                let ch = self.text[start..].chars().next().unwrap_or('\u{fffd}');
                self.pos += ch.len_utf8();
                ch
            }
        };
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
        // The text before the current splice or universal character name, when
        // there has been one.
        let mut spliced: Option<String> = None;
        // Where the run of characters that is still a slice begins.
        let mut segment = start;
        // Whether anything outside the basic character set was written, which
        // is the only case that has to be checked for normalization.
        let mut extended = false;
        loop {
            match self.peek() {
                Some(c) if c < 0x80 && is_ident_continue(c, self.options.dollar_in_identifiers) => {
                    self.pos += 1;
                }
                // Only a splice that the identifier *continues* over: one at
                // the end of it is whitespace, and belongs to whatever comes
                // next.
                Some(b'\\' | b'?')
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
                // A universal character name spells one extended character:
                // `café` and `café` are the same identifier, which is
                // exactly what C99 6.4.2.1 says.
                Some(b'\\') if matches!(self.peek_at(1), Some(b'u' | b'U')) => {
                    let at = self.pos;
                    let Some(ch) = self.scan_ident_ucn(at == start) else {
                        break;
                    };
                    let text = spliced.get_or_insert_with(String::new);
                    text.push_str(&self.text[segment..at]);
                    text.push(ch);
                    segment = self.pos;
                    extended = true;
                }
                Some(c) if c >= 0x80 => {
                    let ch = self.text[self.pos..].chars().next().unwrap_or('\u{fffd}');
                    if !is_extended_ident_char(ch, self.pos == start) {
                        break;
                    }
                    self.pos += ch.len_utf8();
                    extended = true;
                }
                _ => break,
            }
        }
        let text = match spliced {
            Some(mut text) => {
                text.push_str(&self.text[segment..self.pos]);
                text
            }
            None => self.text[start..self.pos].to_owned(),
        };
        // Nothing was an identifier character after all — the whole of it was
        // one bad universal character name, which has been reported. Something
        // has to be consumed, or the scanner would sit here forever.
        if text.is_empty() {
            if self.pos == start {
                let ch = self.text[start..].chars().next().unwrap_or('\u{fffd}');
                self.pos += ch.len_utf8();
            }
            return TokenKind::Error(self.text[start..self.pos].to_owned());
        }
        // Rust identifiers have to be in Normalization Form C, and `rustc`
        // *normalises* the ones a procedural macro hands it rather than
        // refusing them — so two C identifiers that differ only by
        // normalization would silently become one Rust item. C23 (N2836) asks
        // for NFC as well, so refusing is both the safe answer and the
        // conforming one.
        if extended && !unicode_normalization::is_nfc(&text) {
            let range = self.range(start, self.pos);
            self.error(
                range,
                format!(
                    "identifier '{text}' is not in Unicode Normalization Form C; \
                     write the composed form"
                ),
            );
        }
        match Keyword::from_str(&text, self.options.standard) {
            Some(k) => TokenKind::Keyword(k),
            None => TokenKind::Ident(text),
        }
    }

    /// Reads a `\uXXXX` or `\UXXXXXXXX` written inside an identifier.
    ///
    /// A name that is too short to be one leaves the position where it was, so
    /// that the identifier simply ends there and the backslash is reported by
    /// [`Lexer::scan_token`] as the stray character it is. One that is the
    /// right shape but names something an identifier may not hold is *always*
    /// consumed, and reported: leaving it would be a second diagnostic about
    /// the same text, and — where it is the first character of the identifier
    /// — a token that consumed nothing at all.
    fn scan_ident_ucn(&mut self, start: bool) -> Option<char> {
        let at = self.pos;
        let want = if self.peek_at(1) == Some(b'u') { 4 } else { 8 };
        let mut value: u32 = 0;
        for i in 0..want {
            let digit = self.peek_at(2 + i).and_then(|c| (c as char).to_digit(16))?;
            value = value * 16 + digit;
        }
        let spelling = if want == 4 { 'u' } else { 'U' };
        let ch = char::from_u32(value);
        if !ch.is_some_and(|ch| is_extended_ident_char(ch, start)) {
            self.pos += 2 + want;
            let range = self.range(at, self.pos);
            let digits = if want == 4 {
                format!("{value:04X}")
            } else {
                format!("{value:08X}")
            };
            let message =
                format!("'\\{spelling}{digits}' is not a valid character in an identifier");
            // Two different refusals wear the same words. Naming a character
            // below U+00A0, or a surrogate, is what C99 6.4.3p2 forbids of the
            // *name* — C23 puts `$`, `@` and `` ` `` in the basic character
            // set, and Clang refuses those in every mode — so it is ill formed
            // where it is written and is reported even where the token goes
            // nowhere: Clang's own `C99/n717.c` is a file of them, each
            // written as the argument of a macro that expands to nothing.
            // Everything else here — a value outside Unicode, a character
            // that simply is not `XID_Continue` — is about the *identifier*,
            // and a preprocessing token that never reaches the parser is
            // allowed to be one C has no other use for (6.4p3).
            if value < 0xA0 || (0xD800..=0xDFFF).contains(&value) {
                self.lexical_error(range, message);
            } else {
                self.error(range, message);
            }
            return None;
        }
        if let Some(message) = self
            .options
            .gating
            .requires("a universal character name", Standard::C99)
        {
            let range = self.range(at, at + 2 + want);
            self.error(range, message);
        }
        self.pos += 2 + want;
        ch
    }

    fn scan_punctuator(&mut self) -> Option<Punct> {
        if self.trigraph_at(self.pos).is_some() {
            return self.scan_trigraph_punctuator();
        }
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

    /// A punctuator that begins with a trigraph.
    ///
    /// Phase 1 happens before tokens exist, so a punctuator may be spelled
    /// partly or wholly with trigraphs — `??!??!` is `||`, `??'=` is `^=`,
    /// `??=??=` is `##` — and maximal munch applies to the *replaced*
    /// characters. Up to the length of the longest punctuator is replaced into
    /// a small buffer, matched there, and the source position advanced by what
    /// the matched characters really cost.
    fn scan_trigraph_punctuator(&mut self) -> Option<Punct> {
        const LONGEST: usize = 4;
        let mut logical = [0u8; LONGEST];
        let mut widths = [0usize; LONGEST];
        let mut count = 0;
        let mut at = self.pos;
        while count < LONGEST {
            let (c, width) = match self.trigraph_at(at) {
                Some(c) => (c, 3),
                None => match self.bytes.get(at) {
                    Some(c) => (*c, 1),
                    None => break,
                },
            };
            // A backslash is not part of any punctuator, and neither is
            // anything outside the basic character set.
            if c == b'\\' || !c.is_ascii() {
                break;
            }
            logical[count] = c;
            widths[count] = width;
            count += 1;
            at += width;
        }
        let text = std::str::from_utf8(&logical[..count]).ok()?;
        for (spelling, punct) in PUNCTUATORS {
            if text.starts_with(spelling) {
                self.pos += widths[..spelling.len()].iter().sum::<usize>();
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
                // `3i` is GCC's `_Complex int`, one of the two complex
                // *integer* types that are a GNU extension of their own and
                // that cinrs does not have. Saying what to write instead is
                // more use than "invalid suffix".
                if matches!(suffix, "i" | "j" | "I" | "J") {
                    let digits = text.strip_suffix(suffix).unwrap_or(text);
                    self.error(
                        range,
                        format!(
                            "'{text}' is a complex integer constant, which is a GNU extension \
                             cinrs does not support; write '{digits}.0{suffix}' for the \
                             complex floating constant"
                        ),
                    );
                } else {
                    self.error(
                        range,
                        format!("invalid suffix '{suffix}' on integer constant '{text}'"),
                    );
                }
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
        let (body, suffix) = split_float_suffix(digits, hex);
        let (suffix_kind, imaginary) = self.float_suffix(suffix, text, range);

        if hex
            && let Some(message) = self
                .options
                .gating
                .requires("a hexadecimal floating constant", Standard::C99)
        {
            self.error(range, message);
        }
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
            imaginary,
            hex,
            text: text.to_owned(),
        }
    }

    /// The type a floating constant's suffix gives it.
    ///
    /// C has three: none, `f` and `l`. GCC has a dozen more, and they fall
    /// into three groups here.
    ///
    /// * **The ones that name a format wider than `double`** — `d`, `w`
    ///   (`__float80`), `q` (`__float128`), and the `_FloatN` and `_FloatNx`
    ///   suffixes `f64`, `f64x`, `f32x` and `f128`. Every one of them is
    ///   `double` in this implementation, exactly as `long double` is, so each
    ///   is accepted in a GNU dialect and **loses precision** where GCC would
    ///   not; `doc/gnu-extensions.md` records that. `f32` is `float`.
    /// * **The decimal floating suffixes** `df`, `dd` and `dl`, whose types
    ///   are radix-10 and have no Rust counterpart at all.
    /// * **The imaginary suffixes** `i` and `j`, which make the constant an
    ///   imaginary one — `2.0i` is `(0, 2)` — and so need the complex types.
    ///
    /// The decimal ones are refused with the reason, and so are the imaginary
    /// ones when the complex types are switched off. A strict entry point
    /// refuses the first group too, naming the GNU entry point that has it —
    /// the suffixes are spelled without underscores, which is the line
    /// [`crate::Dialect`] draws.
    ///
    /// The second half of the answer is whether the constant is imaginary; see
    /// [`FloatLit::imaginary`].
    fn float_suffix(
        &mut self,
        suffix: &str,
        text: &str,
        range: SourceRange,
    ) -> (FloatSuffix, bool) {
        let lower = suffix.to_ascii_lowercase();
        match lower.as_str() {
            "" => return (FloatSuffix::None, false),
            "f" => return (FloatSuffix::Float, false),
            "l" => return (FloatSuffix::LongDouble, false),
            _ => {}
        }
        // GNU's imaginary suffix, on its own or beside `f` or `l`. Both
        // spellings are the same thing: `j` is what Fortran and engineering
        // habit write, and GCC takes either.
        let imaginary = match lower.as_str() {
            "i" | "j" => Some(FloatSuffix::None),
            "if" | "fi" | "jf" | "fj" => Some(FloatSuffix::Float),
            "il" | "li" | "jl" | "lj" => Some(FloatSuffix::LongDouble),
            _ => None,
        };
        if let Some(kind) = imaginary {
            if !self.options.complex {
                self.error(
                    range,
                    format!(
                        "invalid suffix '{suffix}' on floating constant '{text}': an \
                         imaginary constant needs _Complex. {}",
                        crate::COMPLEX_UNSUPPORTED
                    ),
                );
                return (FloatSuffix::None, false);
            }
            if let Some(message) = self
                .options
                .gating
                .requires("an imaginary constant", Standard::C99)
            {
                self.error(range, message);
                return (FloatSuffix::None, false);
            }
            return (kind, true);
        }
        // A decimal constant is refused whatever the entry point: it has no
        // type this crate can give it.
        let refusal = match lower.as_str() {
            "df" | "dd" | "dl" => Some(
                "the decimal floating types (_Decimal32, _Decimal64, _Decimal128) are not \
                 supported: they are radix-10 and no Rust type is",
            ),
            "f16" | "f16x" | "bf16" => Some(
                "'_Float16' is not supported: Rust's `f16` is unstable, and rounding the \
                 constant to a wider type would change what the program computes",
            ),
            _ => None,
        };
        if let Some(reason) = refusal {
            self.error(
                range,
                format!("invalid suffix '{suffix}' on floating constant '{text}': {reason}"),
            );
            return (FloatSuffix::None, false);
        }
        // The rest are the GNU widths. `f32` is `float`; every other one names
        // a format this implementation makes a `double`.
        let wider = matches!(
            lower.as_str(),
            "d" | "w" | "q" | "f64" | "f64x" | "f32x" | "f128" | "f128x"
        );
        if !wider && lower != "f32" {
            self.error(
                range,
                format!("invalid suffix '{suffix}' on floating constant '{text}'"),
            );
            return (FloatSuffix::None, false);
        }
        if !self.options.gating.dialect.is_gnu() {
            let gnu = self.options.gating.standard.macro_name_in(Dialect::Gnu);
            let here = self
                .options
                .gating
                .standard
                .macro_name_in(self.options.gating.dialect);
            self.error(
                range,
                format!(
                    "the suffix '{suffix}' on a floating constant is a GNU extension, and \
                     requires a GNU dialect ({gnu}) (this block is {here})"
                ),
            );
            return (FloatSuffix::None, false);
        }
        if lower == "f32" {
            return (FloatSuffix::Float, false);
        }
        // `LongDouble` is `double`, which is what all of these come to.
        (FloatSuffix::LongDouble, false)
    }

    // -- character and string constants -------------------------------------

    fn scan_char_constant(&mut self, kind: StrKind) -> TokenKind {
        let start = self.pos;
        debug_assert_eq!(self.peek(), Some(b'\''));
        self.pos += 1;
        let mut values: Vec<u32> = Vec::new();
        let mut terminated = false;
        // How many *characters* were written, which is not how many elements
        // they came to: one `\U0001F600` is one character and two UTF-16 code
        // units, and the two say different things about what is wrong.
        let mut characters = 0usize;
        while let Some(c) = self.peek() {
            if c == b'\'' {
                self.pos += 1;
                terminated = true;
                break;
            }
            if c == b'\n' {
                break;
            }
            let before = values.len();
            self.read_char_element(kind, &mut values);
            if values.len() > before {
                characters += 1;
            }
        }
        let range = self.range(start, self.pos);
        if !terminated {
            self.error(range, "missing terminating \' character");
        }
        if values.is_empty() {
            self.error(range, "empty character constant");
        }
        // Only `'ab'` and `L'ab'` have an implementation-defined meaning; C11
        // 6.4.4.4p2 makes more than one character in a `u8`, `u` or `U`
        // constant a constraint violation, because there is no room for a
        // second one in the type — and so is one character that needs more
        // than one code unit, which is what `u8'é'` and `u'😀'` are.
        if values.len() > 1 {
            match kind {
                StrKind::Narrow | StrKind::Wide => {
                    self.warning(range, "multi-character character constant");
                }
                _ if characters > 1 => {
                    self.error(
                        range,
                        format!(
                            "a '{}' character constant holds exactly one character",
                            kind.prefix()
                        ),
                    );
                }
                _ => {
                    self.error(
                        range,
                        format!(
                            "the character in a '{}' character constant must fit in a \
                             single code unit",
                            kind.prefix()
                        ),
                    );
                }
            }
        }

        let value = if kind != StrKind::Narrow {
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
            kind,
            text: self.text[start..self.pos].to_owned(),
        })
    }

    fn scan_string_literal(&mut self, kind: StrKind) -> TokenKind {
        let start = self.pos;
        debug_assert_eq!(self.peek(), Some(b'"'));
        self.pos += 1;
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
            self.read_char_element(kind, &mut values);
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
    fn read_char_element(&mut self, kind: StrKind, out: &mut Vec<u32>) {
        if self.is_line_splice(self.pos) {
            self.pos += self.line_splice_len(self.pos);
            return;
        }
        let start = self.pos;
        // Phase 1 replaces trigraphs inside literals too: `"??!"` is `"|"`,
        // and `"??/n"` is `"\n"`.
        let trigraph = self.trigraph_at(self.pos);
        if trigraph != Some(b'\\') && self.peek() != Some(b'\\') {
            match trigraph {
                Some(c) => {
                    self.pos += 3;
                    out.push(u32::from(c));
                }
                None if kind.is_bytes() => {
                    // A narrow or UTF-8 literal keeps the raw
                    // execution-charset bytes, so UTF-8 text in one survives
                    // byte for byte.
                    self.pos += 1;
                    out.push(self.bytes[start] as u32);
                }
                None => {
                    let ch = self.text[start..].chars().next().unwrap_or('\u{fffd}');
                    self.pos += ch.len_utf8();
                    push_character(ch as u32, kind, self.options.wchar_bits, out);
                }
            }
            return;
        }

        self.pos += self.trigraph_len(self.pos);
        let e = match self.trigraph_at(self.pos) {
            Some(c) => c,
            None => match self.peek() {
                Some(c) => c,
                None => {
                    let range = self.range(start, self.pos);
                    self.error(range, "incomplete escape sequence");
                    return;
                }
            },
        };
        self.pos += self.trigraph_len(self.pos);
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
                self.push_escape_value(v, kind, start, out);
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
                self.push_escape_value(v, kind, start, out);
            }
            b'u' | b'U' => {
                if let Some(message) = self
                    .options
                    .gating
                    .requires("a universal character name", Standard::C99)
                {
                    let range = self.range(start, self.pos);
                    self.error(range, message);
                }
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
                    Some(ch) if kind.is_bytes() => {
                        // The execution character set is UTF-8.
                        let mut buf = [0u8; 4];
                        for b in ch.encode_utf8(&mut buf).as_bytes() {
                            out.push(*b as u32);
                        }
                    }
                    Some(ch) => push_character(ch as u32, kind, self.options.wchar_bits, out),
                    None => {
                        // Outside Unicode, or a surrogate: 6.4.3p2 again, and
                        // the *literal token's* spelling is what is wrong, so
                        // this stands wherever the token ends up.
                        self.lexical_error(
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

    /// Pushes the value of a numeric escape sequence, which unlike a character
    /// is *not* re-encoded: `u"\xd83d"` is that one code unit.
    fn push_escape_value(&mut self, v: u32, kind: StrKind, start: usize, out: &mut Vec<u32>) {
        let max = kind.max_element(self.options.wchar_bits);
        if v > max {
            let range = self.range(start, self.pos);
            let ty = match kind {
                StrKind::Narrow => "char",
                StrKind::Utf8 => "char8_t",
                StrKind::Utf16 => "char16_t",
                StrKind::Utf32 => "char32_t",
                StrKind::Wide => "wchar_t",
            };
            self.error(
                range,
                format!("escape sequence out of range for type '{ty}'"),
            );
            out.push(v & max);
        } else {
            out.push(v);
        }
    }
}

/// Appends one character, encoded the way `kind` stores its elements.
///
/// A `u"…"` literal holds UTF-16 code units, so a character outside the basic
/// multilingual plane becomes the two halves of a surrogate pair — which is
/// what makes `sizeof(u"\U0001F600")` six rather than four. On a target whose
/// `wchar_t` is 16 bits wide, `L"…"` is UTF-16 too and does the same.
fn push_character(value: u32, kind: StrKind, wchar_bits: u32, out: &mut Vec<u32>) {
    if !kind.is_utf16(wchar_bits) || value <= 0xffff {
        out.push(value);
        return;
    }
    let v = value - 0x1_0000;
    out.push(0xd800 + (v >> 10));
    out.push(0xdc00 + (v & 0x3ff));
}

fn is_ident_start(c: u8, dollar: bool) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || (dollar && c == b'$')
}

fn is_ident_continue(c: u8, dollar: bool) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || (dollar && c == b'$')
}

/// Whether an extended character may appear in an identifier.
///
/// C99 Annex D listed the ranges by hand, C11 revised the list, and C23 (N2836,
/// N2939) replaced all of it with Unicode Annex #31's `XID_Start` and
/// `XID_Continue` — which is also what Rust's own identifiers are, and what
/// makes a C name usable as a Rust one. The one list is used in every entry
/// point: the earlier annexes are approximations of the same intent, and a
/// program that uses a character C11 left out is one this would otherwise
/// refuse for no reason a user could act on.
///
/// A character of the basic character set is never one of these: the ASCII
/// path has already decided about it, and a universal character name is not
/// allowed to spell one (6.4.3p2).
fn is_extended_ident_char(ch: char, start: bool) -> bool {
    if ch.is_ascii() {
        return false;
    }
    if start {
        unicode_ident::is_xid_start(ch)
    } else {
        unicode_ident::is_xid_continue(ch)
    }
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
///
/// The *body* is scanned forward rather than the suffix backwards, because a
/// suffix may hold digits of its own: `1.0f128` names `_Float128` and the
/// `128` is no part of the number. What is left after the digits, the point
/// and the exponent is the suffix, whatever it looks like; naming it is
/// [`Lexer::float_suffix`]'s business.
fn split_float_suffix(text: &str, hex: bool) -> (&str, &str) {
    let b = text.as_bytes();
    let mut i = 0;
    let (exponent, digit): (u8, fn(u8) -> bool) = if hex {
        i = 2; // the `0x` the caller has already recognised
        (b'p', |c| c.is_ascii_hexdigit())
    } else {
        (b'e', |c| c.is_ascii_digit())
    };
    while i < b.len() && (digit(b[i]) || b[i] == b'.') {
        i += 1;
    }
    if i < b.len() && b[i] | 0x20 == exponent {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    (&text[..i], &text[i..])
}

/// Parses the numeric body of a decimal floating constant.
fn parse_decimal_float(body: &str) -> Option<f64> {
    if body.is_empty() {
        return None;
    }
    // `None` is "no exponent at all", which is a different thing from an `e`
    // with nothing after it — `1e` is not a constant.
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
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
    let exponent = match exponent {
        None => 0i32,
        Some(exponent) => {
            let (sign, digits) = match exponent.as_bytes().first() {
                Some(b'+') => (1, &exponent[1..]),
                Some(b'-') => (-1, &exponent[1..]),
                _ => (1, exponent),
            };
            if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            // Saturate: an absurd exponent simply becomes 0 or infinity.
            sign * digits.parse::<i32>().unwrap_or(i32::MAX / 2)
        }
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
