//! The C99 preprocessor: translation phase 4.
//!
//! The preprocessor sits between the [lexer](crate::lex) and the
//! [parser](crate::parse). It consumes the lexer's tokens together with their
//! `bol` / `preceded_by_space` flags, executes the directives it finds and
//! replaces macro invocations, and hands the parser a [`Token`] list that
//! contains no `#` directives at all.
//!
//! # Token origin
//!
//! Every token the preprocessor emits carries an [`Origin`]:
//!
//! * [`Origin::Source`] — the token was written where it is, and its
//!   [`Token::range`] is its own.
//! * [`Origin::Expansion`] — the token came out of a macro's replacement list,
//!   and its range is the range of the *invocation*, not of the `#define`.
//!
//! That distinction is the whole point. A diagnostic — ours or `rustc`'s on
//! the code we generate — must land on something the user wrote, and the
//! `#define` is not where the mistake is being made. Tokens that came from a
//! macro *argument* keep their own ranges, because the argument *was* written
//! at the call site; only the replacement list's own tokens, and the tokens
//! `#` and `##` synthesise, are re-pointed at the invocation.
//!
//! A procedural macro cannot emit secondary spans, so the "which macro was
//! that?" half of the story is appended to the message instead:
//! [`Expansions::annotate`] adds `note: in expansion of macro 'X'` to every
//! diagnostic that lands inside an invocation.
//!
//! # Hide sets
//!
//! Macro replacement follows Dave Prosser's algorithm, the one the standard's
//! rescanning rules were written from. Each token carries a *hide set*: the
//! names of the macros whose expansion it came out of. A name is not replaced
//! again while it is in its own hide set, which is what stops
//!
//! ```c
//! #define foo (4 + foo)
//! ```
//!
//! from running forever while still letting `foo` be replaced somewhere else.
//! For a function-like macro the hide set of the result is
//! `(HS(name) ∩ HS(')')) ∪ {name}`, which is what makes the standard's
//! `f(2 * (f)(z))` example come out right.
//!
//! # The `# #` rule
//!
//! Rust's own lexer refuses `##` in raw-token mode ("reserved multi-hash
//! token"), so a `c99!` block written as raw Rust tokens cannot spell the
//! token-pasting operator. It can spell `a # # b`, and in a *replacement list*
//! a `#` immediately followed by another `#` is ill-formed C anyway — `#` must
//! be followed by a macro parameter — so this preprocessor reads two adjacent
//! `#` tokens in a replacement list as the `##` operator. The rule applies in
//! every input mode, so a macro written with `# #` means the same thing whether
//! it is passed as raw tokens or inside a string literal.
//!
//! # `#include`
//!
//! A header is read at the point the directive is reached, lexed, and pushed
//! onto a stack of open files; the tokens it produces are the tokens the
//! parser sees next. Each file has its own text, its own line numbering and
//! its own idea of what `__FILE__` says, and each is placed in a range of the
//! global offset space of its own, so that a position identifies both a file
//! and a place in it. The [`Preprocessed::included`] list is what a caller
//! adds to its [source map](crate::SourceMap) afterwards, in the order the
//! files were opened; the preprocessor cannot do it itself, because it runs on
//! a thread where a `proc_macro2::Span` cannot follow it.
//!
//! Where a header is looked for — and why the system directories are never
//! looked in — is [`crate::include`]. Reading one twice is avoided the two
//! usual ways: `#pragma once`, and the classic include-guard optimisation. A
//! file that includes itself with neither eventually nests too deeply and is
//! reported.
//!
//! A conditional group a header leaves open ends with the header rather than
//! running on into whatever included it, and is reported against the file that
//! opened it.
//!
//! ## The `cinrs` pragmas
//!
//! ```c
//! #pragma cinrs target "i686-unknown-linux-gnu"
//! #pragma cinrs include_path "vendor/include"
//! #pragma cinrs link "mylib"
//! #pragma cinrs export
//! #pragma cinrs no_std
//! #pragma cinrs module "geometry"
//! ```
//!
//! The first picks the data model the unit is translated for, overriding
//! `CINRS_TARGET`; the second adds a directory to the search path (relative
//! paths resolve against `CARGO_MANIFEST_DIR`); the third puts
//! `#[link(name = "mylib")]` on the generated `extern` block; the fourth gives
//! everything with external linkage a real C symbol, so that another unit can
//! link to it; the last names the module the expansion goes into. Being
//! directives rather than attributes or macro arguments is what makes them
//! mean the same thing in raw-token and in string-literal input. An unknown
//! `#pragma cinrs` option is an error; every other pragma is ignored, as
//! 6.10.6 asks.
//!
//! `target` is the one that cannot be handled where it stands: the predefined
//! macros are built from the model before the first directive is read, so
//! [`scan_target_pragma`] finds it *lexically*, before preprocessing, and the
//! handler here only checks that what it finds agrees. Which is why the pragma
//! has to be written in the unit's own text, ahead of any `#include` or `#if`;
//! anywhere else is a diagnostic rather than a silent half-measure.
//!
//! # What the later revisions add
//!
//! `__VA_OPT__(…)`, `#elifdef` and `#elifndef` are C23's, and are accepted in
//! a [`Standard::C23`] block; an older one is told which macro would have
//! them. `true` and `false` are keywords there too, so an `#if` reads them as
//! 1 and 0 rather than turning them into 0 like any other identifier (C23
//! 6.10.1p6). `#embed` and `__has_embed` are C23's as well: the directive is
//! replaced by the bytes of a file, written as a comma-separated list of
//! `unsigned char` values, and the resource is reported in
//! [`Preprocessed::embedded_files`] so that editing it rebuilds the crate.
//!
//! # Predefined macros
//!
//! | Macro | Value |
//! | --- | --- |
//! | `__STDC__` | `1` |
//! | `__STDC_HOSTED__` | `1` |
//! | `__STDC_VERSION__` | the revision: `199901L`, `201112L`, `201710L` or `202311L`; undefined in `c89!` and `gnu89!` |
//! | `__cinrs__` | `1` |
//! | `__FILE__` | the invoking `.rs` file's path, or `"<c99!>"` |
//! | `__LINE__` | the line of the invoking `.rs` file |
//! | `__DATE__` | `"??? ?? ????"` |
//! | `__TIME__` | `"??:??:??"` |
//!
//! `__FILE__` and `__LINE__` are computed from the position they are *used*
//! at, so inside a header they name the header and the line in it, and a macro
//! defined in `<assert.h>` that mentions them reports the line the assertion
//! is written on.
//!
//! `__DATE__` and `__TIME__` are deliberately fixed placeholders: a build has
//! to be reproducible, and a macro that expanded to the wall clock would make
//! the generated code differ between two builds of the same source.
//!
//! `__LINE__` is a line of the `.rs` file the invocation is written in
//! whenever the compiler tells us where that is — the captured C text
//! remembers which line of its `.rs` file it starts on — and a line inside the
//! C text itself otherwise (a `TokenStream` built from a string in a unit
//! test, for instance).
//!
//! ## `#line`
//!
//! `#line N` and `#line N "name"` (6.10.4), and GCC's `# N "name" flags…` line
//! marker, do what they say: the line after the directive is line N, counting
//! up per physical line from there, and `__FILE__` is the given name until the
//! next directive or the end of that file. The macro-expanded form is
//! supported too — `#line line`, with `line` a macro — and the numbering is
//! per file, so a `#line` inside a header ends with the header. In the
//! macro's own text a `#line` replaces the `.rs`-line convention above from
//! the next line to the end of the block, which is exactly what a program that
//! writes one is asking for.
//!
//! **Nothing else moves.** A diagnostic — this crate's or `rustc`'s — still
//! points at the token that was really written, in the file it was really
//! written in, because that is the position the user can look at; making the
//! caret land on the C is the reason the whole pipeline carries spans.
//! `__BASE_FILE__` names the file the translation unit started in and is not
//! affected either; `__FILE_NAME__` is `__FILE__` without the directory, so it
//! is.
//!
//! On top of those comes a small, deliberately short set of target
//! description macros derived from the machine this crate was compiled for and
//! from [`TargetModel`]: the architecture
//! (`__x86_64__`, `__aarch64__`, …), the operating system (`__linux__`,
//! `__unix__`, `_WIN32`, `__APPLE__`, …), the data model (`__LP64__`,
//! `__ILP32__`, `__CHAR_UNSIGNED__`, `__SIZEOF_INT__` and friends,
//! `__CHAR_BIT__`) and the byte order (`__BYTE_ORDER__`). Nothing about the
//! *language* is described that way — there is no `__GNUC__` — because
//! claiming a compiler's identity would invite headers to use its extensions.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::capture::{Pos, SourceRange};
use crate::diag::{Diagnostic, Diagnostics};
use crate::include;
use crate::lex::{
    self, IntLit, Keyword, LexOptions, LongKind, NumBase, Punct, StrKind, StrLit, TokenKind,
};
use crate::target::{Env, Os, TargetModel, TargetSource};
use crate::{Dialect, Gating, Options, Standard};

// ---------------------------------------------------------------------------
// the tokens the parser sees
// ---------------------------------------------------------------------------

/// One macro expansion a token came out of.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Expansion {
    /// The macro's name.
    pub name: String,
    /// The range of the invocation: the name for an object-like macro, the
    /// name through the closing `)` for a function-like one.
    pub invocation: SourceRange,
    /// Where the macro's name was written in its `#define`.
    pub definition: SourceRange,
    /// The expansion this one was produced inside, if any.
    pub parent: Option<Arc<Expansion>>,
}

/// Where a preprocessed token came from.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Origin {
    /// The token was written where [`Token::range`] says it was.
    #[default]
    Source,
    /// The token came out of a macro's replacement list; [`Token::range`] is
    /// the range of the invocation.
    Expansion(Arc<Expansion>),
}

impl Origin {
    /// The expansion this token came out of, if any.
    pub fn expansion(&self) -> Option<&Arc<Expansion>> {
        match self {
            Origin::Source => None,
            Origin::Expansion(e) => Some(e),
        }
    }
}

/// A preprocessed C token: what the parser consumes.
///
/// Deliberately shaped like [`lex::Token`] minus the flags the preprocessor
/// needed and plus the [`Origin`] it produced, so that the parser's view of a
/// token did not have to change.
#[derive(Clone, PartialEq, Debug)]
pub struct Token {
    /// What the token is.
    pub kind: TokenKind,
    /// Where to blame: the token's own range, or the range of the macro
    /// invocation it came out of.
    pub range: SourceRange,
    /// How the token got here.
    pub origin: Origin,
}

impl Token {
    /// The keyword this token is, if any.
    pub fn keyword(&self) -> Option<lex::Keyword> {
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
    pub fn is_keyword(&self, k: lex::Keyword) -> bool {
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

// ---------------------------------------------------------------------------
// the expansion map
// ---------------------------------------------------------------------------

/// Every macro invocation the preprocessor replaced, so that a diagnostic
/// landing inside one can say which macro it was.
///
/// A procedural macro has exactly one span per diagnostic and no way to add a
/// second one, so the extra context has to travel in the message text.
#[derive(Clone, Default, Debug)]
pub struct Expansions {
    /// One entry per expansion, in the order they happened, which puts an
    /// outer macro before the inner ones it produced.
    entries: Vec<ExpansionSite>,
}

/// Where one macro was invoked, and where it was defined.
#[derive(Clone, Debug)]
struct ExpansionSite {
    invocation: SourceRange,
    name: String,
    definition: SourceRange,
}

impl Expansions {
    fn record(&mut self, range: SourceRange, name: &str, definition: SourceRange) {
        self.entries.push(ExpansionSite {
            invocation: range,
            name: name.to_owned(),
            definition,
        });
    }

    /// The macro whose invocation most tightly encloses `pos`.
    fn enclosing(&self, pos: Pos) -> Option<&ExpansionSite> {
        let mut best: Option<&ExpansionSite> = None;
        for entry in &self.entries {
            if entry.invocation.start > pos || entry.invocation.end < pos {
                continue;
            }
            // The narrowest invocation wins; ties go to the one recorded
            // first, which is the outermost of a nest sharing one range.
            match best {
                Some(b) if b.invocation.len() <= entry.invocation.len() => {}
                _ => best = Some(entry),
            }
        }
        best
    }

    /// Adds `note: in expansion of macro 'X', defined at line N` to every
    /// diagnostic that landed inside a macro invocation.
    ///
    /// Called once per pass, on the diagnostics that pass produced, so that no
    /// diagnostic is ever annotated twice.
    pub fn annotate(&self, diags: &mut Diagnostics) {
        if self.entries.is_empty() {
            return;
        }
        for diag in diags.items_mut() {
            if let Some(site) = self.enclosing(diag.range.start) {
                diag.notes.push(crate::diag::Note {
                    message: format!("in expansion of macro '{}', defined", site.name),
                    range: Some(site.definition),
                });
            }
        }
    }

    /// Whether any macro was expanded at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// hide sets
// ---------------------------------------------------------------------------

/// The set of macro names a token must not be replaced by again.
///
/// Tiny by construction — a handful of names at most — so a sorted vector
/// behind an `Arc` beats a hash set, and the `None` case makes the common
/// "no hide set at all" free.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
struct HideSet(Option<Arc<Vec<String>>>);

impl HideSet {
    fn contains(&self, name: &str) -> bool {
        match &self.0 {
            None => false,
            Some(names) => names.iter().any(|n| n == name),
        }
    }

    fn add(&self, name: &str) -> HideSet {
        if self.contains(name) {
            return self.clone();
        }
        let mut names = match &self.0 {
            None => Vec::with_capacity(1),
            Some(names) => (**names).clone(),
        };
        names.push(name.to_owned());
        HideSet(Some(Arc::new(names)))
    }

    /// The names in both sets, which is what a function-like macro's result
    /// hides (6.10.3.4, via Prosser).
    fn intersect(&self, other: &HideSet) -> HideSet {
        let (Some(a), Some(b)) = (&self.0, &other.0) else {
            return HideSet::default();
        };
        let names: Vec<String> = a.iter().filter(|n| b.contains(n)).cloned().collect();
        if names.is_empty() {
            HideSet::default()
        } else {
            HideSet(Some(Arc::new(names)))
        }
    }

    /// Every name of `other`, added to this set.
    fn union(&self, other: &HideSet) -> HideSet {
        let Some(names) = &other.0 else {
            return self.clone();
        };
        let mut out = self.clone();
        for name in names.iter() {
            out = out.add(name);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// the preprocessor's own token
// ---------------------------------------------------------------------------

/// A token inside the preprocessor: the lexer's, plus a hide set and an origin.
#[derive(Clone, Debug)]
struct PTok {
    kind: TokenKind,
    range: SourceRange,
    bol: bool,
    space: bool,
    origin: Origin,
    hide: HideSet,
    errors: Vec<Diagnostic>,
}

impl PTok {
    fn from_lexed(tok: &lex::Token) -> Self {
        Self {
            kind: tok.kind.clone(),
            range: tok.range,
            bol: tok.bol,
            space: tok.preceded_by_space,
            origin: Origin::Source,
            hide: HideSet::default(),
            errors: tok.errors.clone(),
        }
    }

    fn is_eof(&self) -> bool {
        self.kind == TokenKind::Eof
    }

    fn is_punct(&self, p: Punct) -> bool {
        self.kind == TokenKind::Punct(p)
    }

    fn name(&self) -> Option<&str> {
        self.kind.macro_name()
    }

    fn spelling(&self) -> &str {
        self.kind.spelling()
    }
}

// ---------------------------------------------------------------------------
// macro definitions
// ---------------------------------------------------------------------------

/// A macro the preprocessor synthesises rather than stores tokens for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Builtin {
    /// `__LINE__`, whose value depends on where it is used.
    Line,
    /// `__FILE__`, likewise: inside an `#include`d file it names the header.
    File,
    /// `__FILE_NAME__` — GNU's `__FILE__` without the directory.
    FileName,
    /// `__INCLUDE_LEVEL__` — how many `#include`s deep the use is.
    IncludeLevel,
    /// `__COUNTER__` — a fresh integer at every use.
    Counter,
}

/// One `#define`.
#[derive(Debug)]
struct MacroDef {
    /// The parameter names, or `None` for an object-like macro.
    params: Option<Vec<String>>,
    /// Whether the parameter list ended with `...`.
    variadic: bool,
    /// The name GNU's `#define log(fmt, args...)` gave the variable arguments,
    /// which is then another spelling of `__VA_ARGS__`.
    va_name: Option<String>,
    /// The replacement list.
    body: Vec<PTok>,
    /// Where the macro's name was written.
    name_range: SourceRange,
    /// Whether this macro was built in rather than written by the user.
    predefined: bool,
    /// The value this macro computes, for the ones that are not just tokens.
    builtin: Option<Builtin>,
}

impl MacroDef {
    /// Whether two definitions are the same one, as 6.10.3p2 requires:
    /// the same kind, the same parameter spellings, and replacement lists that
    /// agree token for token *and* on where the white space was.
    fn same_as(&self, other: &MacroDef) -> bool {
        if self.params != other.params
            || self.variadic != other.variadic
            || self.va_name != other.va_name
        {
            return false;
        }
        if self.body.len() != other.body.len() {
            return false;
        }
        self.body
            .iter()
            .zip(&other.body)
            .enumerate()
            .all(|(i, (a, b))| a.spelling() == b.spelling() && (i == 0 || a.space == b.space))
    }

    /// The index of the parameter `name` stands for, `__VA_ARGS__` included.
    fn param_index(&self, name: &str) -> Option<usize> {
        let params = self.params.as_ref()?;
        if let Some(i) = params.iter().position(|p| p == name) {
            return Some(i);
        }
        let variable = name == VA_ARGS || self.va_name.as_deref() == Some(name);
        (self.variadic && variable).then_some(params.len())
    }

    /// The index the variable arguments occupy, if there are any.
    fn va_index(&self) -> Option<usize> {
        self.variadic
            .then(|| self.params.as_ref().map_or(0, Vec::len))
    }
}

/// The pieces of a parsed macro parameter list.
struct ParamList {
    params: Vec<String>,
    variadic: bool,
    va_name: Option<String>,
    /// How many tokens the list occupied, including its parentheses.
    used: usize,
}

const VA_ARGS: &str = "__VA_ARGS__";

/// C23's conditional-expansion operator (6.10.5.2).
const VA_OPT: &str = "__VA_OPT__";

/// C99's `_Pragma` operator (6.10.9).
const PRAGMA_OPERATOR: &str = "_Pragma";

/// Undoes what `#` did: `L"a\"b\\c"` becomes `a"b\c`.
fn destringize(text: &str) -> String {
    let inner = text
        .strip_prefix("L\"")
        .or_else(|| text.strip_prefix('"'))
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(text);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(next @ ('"' | '\\')) => out.push(next),
            Some(next) => {
                out.push('\\');
                out.push(next);
            }
            None => out.push('\\'),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// limits
// ---------------------------------------------------------------------------

/// How deeply argument pre-expansion may nest.
///
/// Hide sets already make runaway recursion impossible, but a macro whose
/// arguments are themselves deeply nested invocations turns into recursion in
/// *this* code, which runs inside a compiler that must not be taken down by a
/// stack overflow.
const MAX_EXPANSION_DEPTH: u32 = 200;

/// How deeply `#include` may nest.
///
/// A header that includes itself is the ordinary way to reach this, and it is
/// always a mistake — the include guard that would have stopped it is missing.
/// The limit is a count of open files rather than a recursion limit: the
/// preprocessor reads a nested file iteratively, so nothing here is at risk of
/// a stack overflow, but a program that never stops including is still a
/// program that never finishes compiling.
const MAX_INCLUDE_DEPTH: usize = 200;

/// `__STDC_EMBED_NOT_FOUND__`, the answer `__has_embed` gives for a resource
/// that is not there or that carries a parameter this does not have.
const EMBED_NOT_FOUND: u128 = 0;
/// `__STDC_EMBED_FOUND__`: the resource exists and has at least one byte.
const EMBED_FOUND: u128 = 1;
/// `__STDC_EMBED_EMPTY__`: the resource exists and `#embed` would produce
/// nothing from it.
const EMBED_EMPTY: u128 = 2;

/// How many tokens one translation unit's macro expansion may produce.
///
/// `#define A B B` repeated thirty times is a legal program whose expansion
/// does not fit in memory. Refusing it with a diagnostic beats spending the
/// rest of the build on it.
const MAX_EXPANDED_TOKENS: usize = 4_000_000;

// ---------------------------------------------------------------------------
// entry point
// ---------------------------------------------------------------------------

/// What the preprocessor needs to know about the text it is running over.
#[derive(Clone, Debug)]
pub struct Context {
    /// The C source text, which `#error` and `#include <…>` read back
    /// verbatim.
    pub text: String,
    /// The global offset of `text`'s first byte.
    pub base: Pos,
    /// What `__FILE__` expands to.
    pub file_name: String,
    /// The line `text`'s own line 1 sits on; see the [module docs](self).
    pub first_line: usize,
    /// The directory an `#include "…"` written in this text looks in first —
    /// the directory of the invoking `.rs` file. `None` when the compiler will
    /// not say where that is.
    pub dir: Option<std::path::PathBuf>,
    /// The global offset the first `#include`d file is placed at.
    ///
    /// The preprocessor allocates the offsets of the files it opens, because
    /// it runs where a [`SourceMap`](crate::SourceMap) cannot follow it; see
    /// [`crate::SourceMap::next_base`].
    pub next_base: Pos,
    /// The `#pragma cinrs target` directives [`scan_target_pragma`] already
    /// read out of `text`, so that the preprocessor does not report one twice
    /// and can tell a header's from the unit's own.
    pub target_pragmas: TargetPragmas,
}

impl Context {
    /// A context for `text` with nothing known about where it came from.
    pub fn new(text: impl Into<String>, base: Pos) -> Self {
        let text = text.into();
        // One byte of gap, exactly as `SourceMap::add_file` leaves, so that the
        // end of one file is never the start of the next.
        let next_base = base
            .saturating_add(text.len() as Pos)
            .saturating_add(FILE_GAP);
        Self {
            text,
            base,
            file_name: DEFAULT_FILE_NAME.to_owned(),
            first_line: 1,
            dir: None,
            next_base,
            target_pragmas: TargetPragmas::default(),
        }
    }
}

/// The gap left between two files in the global offset space.
///
/// It must match [`crate::SourceMap`]'s, since the preprocessor allocates the
/// offsets and the map hands out the spans for them.
const FILE_GAP: Pos = 1;

/// What `__FILE__` expands to when the compiler will not say where the
/// invocation is.
pub const DEFAULT_FILE_NAME: &str = "<c99!>";

/// One file `#include` brought in, for the caller to add to its source map.
#[derive(Clone, Debug)]
pub struct IncludedFile {
    /// What diagnostics call it: `include/foo.h`, or `<cinrs>/stdio.h` for a
    /// bundled header.
    pub name: String,
    /// Its text.
    pub text: String,
    /// The global offset its text starts at.
    pub base: Pos,
    /// The `#include` directive that pulled it in, which is where a diagnostic
    /// inside it points.
    pub directive: SourceRange,
}

/// What `#embed`'s parameters asked for (C23 6.10.3.2–6.10.3.5).
#[derive(Clone, Debug, Default)]
struct EmbedParams {
    /// `limit(N)`: at most this many bytes of the resource.
    limit: Option<usize>,
    /// `prefix(…)`: tokens before the bytes, when there are any.
    prefix: Vec<PTok>,
    /// `suffix(…)`: tokens after them, likewise.
    suffix: Vec<PTok>,
    /// `if_empty(…)`: the whole expansion when there are none.
    if_empty: Vec<PTok>,
}

/// Everything one run of the preprocessor produced.
#[derive(Debug)]
pub struct Preprocessed {
    /// The token list, always ending with [`TokenKind::Eof`].
    pub tokens: Vec<Token>,
    /// The macro invocations that were replaced.
    pub expansions: Expansions,
    /// The files `#include` opened, in the order they were opened, which is
    /// also the order they must be added to a source map: a file is always
    /// listed after the one whose directive pulled it in.
    pub included: Vec<IncludedFile>,
    /// The absolute paths of the *user* headers that were read, for rebuild
    /// tracking. A bundled header cannot change without the crate changing, so
    /// it is not listed.
    pub user_headers: Vec<std::path::PathBuf>,
    /// The absolute paths of the resources `#embed` read, for the same reason
    /// and by the same route — except that they are bytes rather than text, so
    /// the expansion tracks them with `include_bytes!`.
    pub embedded_files: Vec<std::path::PathBuf>,
    /// The libraries `#pragma cinrs link` asked for, in the order asked.
    pub link_libraries: Vec<String>,
    /// Whether `#pragma cinrs export` asked for real C symbols.
    pub export: bool,
    /// Whether `#pragma cinrs no_std` said the expansion goes into a
    /// `#![no_std]` crate.
    pub no_std: bool,
    /// The module name `#pragma cinrs module` asked for.
    pub module: Option<String>,
    /// The Rust path `#pragma cinrs crate` gave the `cinrs` facade crate,
    /// which the generated code names when it needs the runtime.
    pub crate_path: Option<String>,
    /// Every `#pragma pack` the unit wrote, as `(token index, alignment)`.
    ///
    /// A pragma is not a token, so the change is recorded against the position
    /// in [`Preprocessed::tokens`] it takes effect at; [`PackMap`] answers what
    /// was in force where a `struct` was defined.
    pub pack_events: Vec<(usize, Option<u32>)>,
}

/// What `#pragma pack` asked for, at every point of the token list.
#[derive(Clone, Debug, Default)]
pub struct PackMap {
    events: Vec<(usize, Option<u32>)>,
}

impl PackMap {
    /// Builds the map from the preprocessor's events, which are in order.
    pub fn new(events: Vec<(usize, Option<u32>)>) -> Self {
        Self { events }
    }

    /// Whether any `#pragma pack` was written at all.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The maximum member alignment in force at token `index`.
    pub fn at(&self, index: usize) -> Option<u32> {
        let at = self.events.partition_point(|(pos, _)| *pos <= index);
        self.events[..at].last().and_then(|(_, value)| *value)
    }
}

/// Runs the preprocessor over a lexed translation unit.
///
/// The returned list always ends with [`TokenKind::Eof`]. Problems the lexer
/// found are reported here — but only for the tokens that survive, because a
/// group skipped by `#if 0` may hold anything at all.
pub fn preprocess(
    tokens: &[lex::Token],
    ctx: &Context,
    options: &Options,
    diags: &mut Diagnostics,
) -> Preprocessed {
    let mut pp = Pp::new(tokens, ctx, options, diags);
    pp.run();
    Preprocessed {
        tokens: pp.out,
        expansions: pp.expansions,
        included: pp.included,
        user_headers: pp.user_headers,
        embedded_files: pp.embedded_files,
        link_libraries: pp.link_libraries,
        export: pp.export,
        no_std: pp.no_std,
        module: pp.module,
        crate_path: pp.crate_path,
        pack_events: pp.pack_events,
    }
}

/// What [`scan_target_pragma`] found, which the preprocessor needs in order
/// not to report the same directive twice.
#[derive(Clone, Debug, Default)]
pub struct TargetPragmas {
    /// Where each `#pragma cinrs target` in the unit's own text begins — the
    /// offset of its `#`, which is where the preprocessor's own range for a
    /// directive starts too.
    pub at: Vec<Pos>,
    /// Whether one of them really chose the model. False when there was none,
    /// and false when there was one that has already been reported.
    pub applied: bool,
}

impl TargetPragmas {
    /// Whether the directive at `range` is one the scan read.
    fn scanned(&self, range: SourceRange) -> bool {
        self.at.contains(&range.start)
    }
}

/// Finds `#pragma cinrs target "<triple>"` in a freshly lexed unit and puts the
/// model it names into `options`.
///
/// This runs *before* the preprocessor, and has to. Everything the
/// preprocessor does with the model — the hundred-odd predefined macros, and
/// therefore which branch every `#if` and every bundled header takes — is
/// settled when it starts, so a pragma handled where it stands would arrive
/// too late to mean what it says. Reading it lexically is the price: the
/// directive is recognised by its shape, in the unit's own text, whether or
/// not a conditional group would later have skipped it, and a second one
/// naming a different triple is an error rather than a last-one-wins.
///
/// The preprocessor sees the same directives again during the real run —
/// `Pp::target_pragma` is where — which is what catches the two cases this
/// scan cannot serve: a `target` pragma the scan never read, because it is in
/// a header or came out of `_Pragma`, and one written after an `#include` or
/// an `#if` that the old model had already answered.
///
/// The caller must lex the text again when the model changed: how wide
/// `wchar_t` is decides what `L'…'` may hold.
pub fn scan_target_pragma(
    tokens: &[lex::Token],
    options: &mut Options,
    diags: &mut Diagnostics,
) -> (TargetPragmas, bool) {
    let mut found = TargetPragmas::default();
    let mut chosen: Option<(String, SourceRange)> = None;
    let mut failed = false;
    for (i, hash) in tokens.iter().enumerate() {
        // `# pragma cinrs target "…"`, the directive spelled out; the lexer
        // marks the token that begins a logical line.
        if !hash.bol || !hash.is_punct(Punct::Hash) {
            continue;
        }
        // Five tokens are enough for `pragma cinrs target "…"` and the one
        // trailing token the diagnostic complains about; a `#define` whose
        // replacement list runs to a hundred is not walked to the end just to
        // discover it is not this.
        let words: Vec<&lex::Token> = tokens[i + 1..]
            .iter()
            .take_while(|t| !t.bol && !matches!(t.kind, TokenKind::Eof))
            .take(5)
            .collect();
        let [pragma, cinrs, option, rest @ ..] = words.as_slice() else {
            continue;
        };
        if pragma.ident() != Some("pragma")
            || cinrs.ident() != Some("cinrs")
            || option.ident() != Some("target")
        {
            continue;
        }
        found.at.push(hash.range.start);
        let range = SourceRange::new(hash.range.start, option.range.end);
        let Some(triple) = target_pragma_triple(rest, range, diags) else {
            failed = true;
            continue;
        };
        match &chosen {
            // The same triple twice says the same thing twice, which is no
            // mistake at all.
            Some((first, _)) if *first == triple => {}
            Some((first, _)) => {
                diags.error(
                    range,
                    format!(
                        "this unit is already translated for '{first}' by an earlier \
                         #pragma cinrs target"
                    ),
                );
                failed = true;
            }
            None => chosen = Some((triple, range)),
        }
    }
    let Some((triple, range)) = chosen.filter(|_| !failed) else {
        return (found, false);
    };
    let source = TargetSource::Pragma(triple);
    match TargetModel::from_triple(source.triple().expect("Pragma carries its triple")) {
        Ok(model) => {
            let relex = options.target != model;
            options.target = model;
            options.target_source = source;
            found.applied = true;
            (found, relex)
        }
        Err(unknown) => {
            diags.error(range, unknown.message(&source));
            (found, false)
        }
    }
}

/// The one string literal `#pragma cinrs target` takes, as the pre-scan reads
/// it. The messages match [`Pp::pragma_string`]'s, since the same mistake must
/// read the same whichever pass notices it.
fn target_pragma_triple(
    rest: &[&lex::Token],
    range: SourceRange,
    diags: &mut Diagnostics,
) -> Option<String> {
    let Some(tok) = rest.first() else {
        diags.error(range, "#pragma cinrs target needs a string literal");
        return None;
    };
    let TokenKind::Str(lit) = &tok.kind else {
        diags.error(
            tok.range,
            format!(
                "#pragma cinrs target needs a string literal, found {}",
                tok.kind.describe()
            ),
        );
        return None;
    };
    let Some(bytes) = lit.as_bytes() else {
        diags.error(
            tok.range,
            "#pragma cinrs target does not take a wide string literal",
        );
        return None;
    };
    let value = String::from_utf8_lossy(&bytes).into_owned();
    if value.is_empty() {
        diags.error(tok.range, "#pragma cinrs target was given an empty string");
        return None;
    }
    if let Some(extra) = rest.get(1) {
        diags.error(
            extra.range,
            format!(
                "unexpected {} after #pragma cinrs target",
                extra.kind.describe()
            ),
        );
    }
    Some(value)
}

// ---------------------------------------------------------------------------
// the machine
// ---------------------------------------------------------------------------

/// One `#if` / `#ifdef` / `#ifndef` group.
struct Cond {
    /// Where the directive that opened the group is.
    range: SourceRange,
    /// Whether the enclosing group was itself being processed.
    outer_active: bool,
    /// Whether a branch has already been taken.
    taken: bool,
    /// Whether the branch now open is being processed.
    active: bool,
    /// Whether `#else` has been seen.
    seen_else: bool,
}

/// What one `#line` — or one GCC line marker — did to a file's numbering.
///
/// See [`Pp::line_directive`]. Only `__LINE__` and `__FILE__` are affected:
/// a diagnostic still points at the token that was really written, which is the
/// whole point of this crate.
struct LineDirective {
    /// The zero-based index of the *physical* line the directive is written on.
    at: usize,
    /// The number the next physical line is given.
    line: usize,
    /// What `__FILE__` says from that line on: the name the directive gave, or
    /// the one in force when it was written.
    name: String,
}

/// One file the preprocessor has read, kept for as long as positions inside it
/// can still be reported.
struct FileEntry {
    /// The text, which `#error` and `#include` read back verbatim.
    text: String,
    /// The global offset of its first byte.
    base: Pos,
    /// File-local byte offsets at which each line starts.
    line_starts: Vec<u32>,
    /// The line of the enclosing `.rs` file its own line 1 sits on; 1 for a
    /// header, which counts its own lines.
    first_line: usize,
    /// What `__FILE__` says inside it.
    name: String,
    /// The `#line` directives it has executed so far, in the order they were
    /// reached — which is the order of their positions, since a file is only
    /// ever read forwards.
    lines: Vec<LineDirective>,
}

impl FileEntry {
    fn new(text: String, base: Pos, first_line: usize, name: String) -> Self {
        let mut line_starts = vec![0u32];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i as u32 + 1);
            }
        }
        Self {
            text,
            base,
            line_starts,
            first_line: first_line.max(1),
            name,
            lines: Vec::new(),
        }
    }

    /// The zero-based index of the physical line `local` sits on.
    fn physical_line(&self, local: Pos) -> usize {
        self.line_starts
            .partition_point(|start| *start <= local)
            .saturating_sub(1)
    }

    /// The `#line` in force on physical line `index`, if there is one.
    ///
    /// A directive takes effect on the line *after* itself, so its own line
    /// still counts the way the one before it did.
    fn directive_for(&self, index: usize) -> Option<&LineDirective> {
        let after = self.lines.partition_point(|d| d.at < index);
        self.lines[..after].last()
    }

    /// The line `local` sits on, counted the way `__LINE__` counts.
    fn line_of(&self, local: Pos) -> usize {
        let index = self.physical_line(local);
        match self.directive_for(index) {
            // The directive named the line after itself; every line after that
            // one counts up from there.
            Some(d) => d.line + (index - d.at - 1),
            None => self.first_line + index,
        }
    }

    /// The name `__FILE__` reports for `local`.
    fn name_of(&self, local: Pos) -> &str {
        match self.directive_for(self.physical_line(local)) {
            Some(d) => &d.name,
            None => &self.name,
        }
    }
}

/// A file that is open: being read right now, or waiting for the `#include`
/// inside it to finish.
struct OpenFile {
    /// The whole file, lexed once.
    input: Vec<PTok>,
    /// Where in `input` the next unread token is.
    pos: usize,
    /// Where an `#include "…"` written in it looks first.
    origin: include::Origin,
    /// What identifies it for `#pragma once` and the include-guard
    /// optimisation: its canonical path, or the name of a bundled header.
    key: String,
    /// How many conditional groups were open when it was entered, so that one
    /// it leaves unterminated is reported against it rather than leaking into
    /// the file that included it.
    cond_base: usize,
}

struct Pp<'a> {
    /// Every file read so far, in the order they were opened.
    files: Vec<FileEntry>,
    /// The files being read, outermost first.
    open: Vec<OpenFile>,
    /// Tokens produced by macro replacement, innermost last.
    pending: Vec<PTok>,
    out: Vec<Token>,
    macros: HashMap<String, Arc<MacroDef>>,
    conds: Vec<Cond>,
    diags: &'a mut Diagnostics,
    expansions: Expansions,
    /// Lexer problems already reported, so that a macro used twice does not
    /// report the same bad token in its body twice.
    reported: HashSet<(Pos, Pos, String)>,
    /// The global offset of the root file, where anything with no position of
    /// its own is reported.
    base: Pos,
    lex_options: LexOptions,
    /// How a construct of a newer revision is gated, and whether the plain GNU
    /// spellings are on.
    gating: Gating,
    depth: u32,
    /// Tokens still allowed to come out of macro replacement.
    budget: usize,
    /// Set once the budget ran out; stops all further replacement.
    aborted: bool,
    // -- `#include` ---------------------------------------------------------
    /// Where the next included file's text is placed.
    next_base: Pos,
    /// The included files, for the caller's source map.
    included: Vec<IncludedFile>,
    /// The directories headers are looked for in.
    search: include::SearchPaths,
    /// The files `#pragma once` has closed for good.
    once: HashSet<String>,
    /// The stacks `#pragma push_macro("X")` pushed, by macro name.
    macro_stacks: HashMap<String, Vec<Option<Arc<MacroDef>>>>,
    /// The identifiers `#pragma GCC poison` made unusable.
    poisoned: HashSet<String>,
    /// The next value `__COUNTER__` expands to.
    counter: u64,
    /// The member alignment `#pragma pack` is currently asking for.
    pack: Option<u32>,
    /// What `#pragma pack(push)` saved.
    pack_stack: Vec<Option<u32>>,
    /// Every change of that value, by the index in `out` it takes effect at.
    pack_events: Vec<(usize, Option<u32>)>,
    /// The name of the outermost file, which `__BASE_FILE__` reports.
    base_file: String,
    /// The include guard of a file that has one: its name, and the macro that
    /// makes reading it again pointless.
    guards: HashMap<String, String>,
    /// The absolute paths of the user headers that were read.
    user_headers: Vec<std::path::PathBuf>,
    /// The absolute paths of the resources `#embed` read.
    embedded_files: Vec<std::path::PathBuf>,
    /// The libraries `#pragma cinrs link` asked for.
    link_libraries: Vec<String>,
    /// Set by `#pragma cinrs export`.
    export: bool,
    /// Set by `#pragma cinrs no_std`.
    no_std: bool,
    /// The name `#pragma cinrs module` gave the generated module.
    module: Option<String>,
    /// The Rust path `#pragma cinrs crate` gave the facade crate.
    crate_path: Option<String>,
    /// Where the data model in force came from, which is what a
    /// `#pragma cinrs target` the scan never read is reported against.
    target_source: TargetSource,
    /// The `target` pragmas [`scan_target_pragma`] already dealt with.
    target_pragmas: TargetPragmas,
    /// Whether anything has yet been decided *by* the data model: a header
    /// opened, or an `#if` evaluated. A `#pragma cinrs target` after that
    /// point cannot mean what it says, so it is reported.
    model_observed: bool,
}

impl<'a> Pp<'a> {
    fn new(
        tokens: &[lex::Token],
        ctx: &'a Context,
        options: &Options,
        diags: &'a mut Diagnostics,
    ) -> Self {
        let root = FileEntry::new(
            ctx.text.clone(),
            ctx.base,
            ctx.first_line,
            ctx.file_name.clone(),
        );
        let mut input: Vec<PTok> = tokens.iter().map(PTok::from_lexed).collect();
        if input.is_empty() {
            input.push(eof_token(ctx.base));
        }
        let mut pp = Pp {
            files: vec![root],
            open: vec![OpenFile {
                input,
                pos: 0,
                origin: match &ctx.dir {
                    Some(dir) => include::Origin::Dir(dir.clone()),
                    None => include::Origin::Unknown,
                },
                key: ctx.file_name.clone(),
                cond_base: 0,
            }],
            pending: Vec::new(),
            out: Vec::new(),
            macros: HashMap::new(),
            conds: Vec::new(),
            diags,
            expansions: Expansions::default(),
            reported: HashSet::new(),
            base: ctx.base,
            lex_options: options.into(),
            gating: options.gating(),
            depth: 0,
            budget: MAX_EXPANDED_TOKENS,
            aborted: false,
            next_base: ctx.next_base,
            included: Vec::new(),
            search: include::SearchPaths::new(&options.include_paths),
            once: HashSet::new(),
            macro_stacks: HashMap::new(),
            poisoned: HashSet::new(),
            counter: 0,
            pack: None,
            pack_stack: Vec::new(),
            pack_events: Vec::new(),
            base_file: ctx.file_name.clone(),
            guards: HashMap::new(),
            user_headers: Vec::new(),
            embedded_files: Vec::new(),
            link_libraries: Vec::new(),
            export: false,
            no_std: false,
            module: None,
            crate_path: None,
            target_source: options.target_source.clone(),
            target_pragmas: ctx.target_pragmas.clone(),
            model_observed: false,
        };
        pp.define_predefined(options);
        pp
    }

    // -- reading ------------------------------------------------------------

    /// The file being read.
    fn cur(&self) -> &OpenFile {
        self.open
            .last()
            .expect("the root file is only closed when the run ends")
    }

    fn cur_mut(&mut self) -> &mut OpenFile {
        self.open
            .last_mut()
            .expect("the root file is only closed when the run ends")
    }

    /// The file's next token, which is its end-of-file token once it has run
    /// out.
    fn ahead(&self) -> &PTok {
        let file = self.cur();
        &file.input[file.pos]
    }

    /// The next token without consuming it, or `None` when replacement output
    /// has run out and the caller may not read the file itself.
    fn peek(&self, allow_input: bool) -> Option<&PTok> {
        if let Some(t) = self.pending.last() {
            return Some(t);
        }
        allow_input.then(|| self.ahead())
    }

    /// The next token, consumed.
    ///
    /// Reading never crosses a file boundary: at the end of an `#include`d
    /// file this keeps answering with that file's end-of-file token, so that a
    /// macro invocation left unfinished there is reported instead of quietly
    /// swallowing what follows the directive. [`Pp::run`] is what closes a
    /// file.
    fn bump(&mut self, allow_input: bool) -> Option<PTok> {
        if let Some(t) = self.pending.pop() {
            return Some(t);
        }
        if !allow_input {
            return None;
        }
        let tok = self.ahead().clone();
        if !tok.is_eof() {
            self.cur_mut().pos += 1;
        }
        Some(tok)
    }

    /// Whether the file's next token opens a directive.
    fn at_directive(&self) -> bool {
        let tok = self.ahead();
        tok.bol && tok.is_punct(Punct::Hash)
    }

    fn skipping(&self) -> bool {
        self.conds.last().is_some_and(|c| !c.active)
    }

    // -- the main loop ------------------------------------------------------

    fn run(&mut self) {
        loop {
            // Directives, the end of a file and skipped groups are all
            // properties of the *file*, so they are only looked at once
            // everything macro replacement produced has been dealt with.
            if self.pending.is_empty() {
                if self.ahead().is_eof() {
                    if self.open.len() > 1 {
                        self.close_file();
                        continue;
                    }
                    self.finish();
                    return;
                }
                if self.at_directive() {
                    self.directive();
                    continue;
                }
                if self.skipping() {
                    // A skipped group is not even lexically C: discard its
                    // tokens without looking at them, and without reporting
                    // anything.
                    self.cur_mut().pos += 1;
                    continue;
                }
            }
            let Some(tok) = self.bump(true) else {
                unreachable!("reading the file is always allowed here");
            };
            if tok.is_eof() {
                // Handled above; nothing puts an end-of-file token into the
                // replacement output.
                continue;
            }
            if tok.name().is_some() && self.try_expand(&tok, true) {
                continue;
            }
            if tok.name() == Some(PRAGMA_OPERATOR) && self.pragma_operator(&tok) {
                continue;
            }
            self.emit(tok);
        }
    }

    /// C99's `_Pragma ( string-literal )`, which is a pragma written where an
    /// expression could go — and therefore the only way a *macro* can produce
    /// one.
    ///
    /// Returns whether it really was one: the name on its own is an ordinary
    /// identifier.
    fn pragma_operator(&mut self, tok: &PTok) -> bool {
        if !self.peek(true).is_some_and(|t| t.is_punct(Punct::LParen)) {
            return false;
        }
        self.require_standard(Standard::C99, "'_Pragma'", tok.range);
        self.bump(true);
        let literal = self.bump(true);
        let Some(text) = literal.as_ref().and_then(|t| match &t.kind {
            TokenKind::Str(lit) => Some(destringize(&lit.text)),
            _ => None,
        }) else {
            self.diags
                .error(tok.range, "'_Pragma' takes one string literal");
            return true;
        };
        if !self.bump(true).is_some_and(|t| t.is_punct(Punct::RParen)) {
            self.diags.error(tok.range, "missing ')' after '_Pragma'");
            return true;
        }
        // The destringized text is a directive line without its `#pragma`, so
        // it is lexed and handed to the same code the directive uses. The
        // tokens are placed at the `_Pragma` itself, which is where a
        // diagnostic about them belongs.
        let tokens: Vec<PTok> = lex::lex_text(&text, tok.range.start, &self.lex_options)
            .iter()
            .filter(|t| !matches!(t.kind, TokenKind::Eof))
            .map(PTok::from_lexed)
            .collect();
        self.pragma(&tokens, tok.range);
        true
    }

    /// Leaves an `#include`d file, reporting the conditionals it left open.
    fn close_file(&mut self) {
        let base = self.cur().cond_base;
        for cond in self.conds.drain(base..).collect::<Vec<_>>() {
            self.diags
                .error(cond.range, "unterminated conditional directive");
        }
        self.open.pop();
    }

    /// Reports what never closed and emits the end-of-input token.
    fn finish(&mut self) {
        for cond in std::mem::take(&mut self.conds) {
            self.diags
                .error(cond.range, "unterminated conditional directive");
        }
        let file = self.cur();
        let eof = file.input[file.input.len() - 1].clone();
        self.emit(eof);
    }

    fn emit(&mut self, mut tok: PTok) {
        self.report_errors(&tok);
        if matches!(tok.kind, TokenKind::Error(_)) {
            // Not a C token at all: reported above, and dropped so that the
            // parser never has to have an opinion about it.
            return;
        }
        // The GNU keywords are recognised *here*, on the way to the parser,
        // rather than in the lexer: until this point `__attribute__` is an
        // ordinary identifier, so `#define __attribute__(x)` — which every
        // portability header writes — defines and expands a macro of that
        // name, and `#ifdef __restrict` answers about the name that was
        // written.
        if let TokenKind::Ident(name) = &tok.kind {
            if self.poisoned.contains(name) {
                let range = tok.range;
                let name = name.clone();
                self.diags.error(
                    range,
                    format!("attempt to use the poisoned identifier '{name}'"),
                );
            }
            if let Some(keyword) = gnu_keyword(name, self.gating.dialect) {
                tok.kind = TokenKind::Keyword(keyword);
            }
        }
        self.out.push(Token {
            kind: tok.kind,
            range: tok.range,
            origin: tok.origin,
        });
    }

    /// Reports the problems the lexer attached to a token that survived.
    fn report_errors(&mut self, tok: &PTok) {
        self.report_token_diags(tok, false);
    }

    /// Reports only what is wrong with a token's *spelling*, which stands
    /// whether or not the token goes anywhere; see [`Diagnostic::lexical`].
    fn report_lexical_errors(&mut self, tok: &PTok) {
        self.report_token_diags(tok, true);
    }

    fn report_token_diags(&mut self, tok: &PTok, lexical_only: bool) {
        for diag in &tok.errors {
            if lexical_only && !diag.lexical {
                continue;
            }
            let key = (diag.range.start, diag.range.end, diag.message.clone());
            if self.reported.insert(key) {
                self.diags.push(diag.clone());
            }
        }
    }

    /// The file a position is in.
    ///
    /// Every file ever opened stays in `files`, and their bases only ever
    /// increase, so a position identifies one of them even after it has been
    /// left — which is what a diagnostic about a macro defined in a header
    /// that was closed long ago needs.
    fn file_at(&self, pos: Pos) -> &FileEntry {
        &self.files[self.file_index(pos)]
    }

    /// The index in `files` of the file a position is in.
    fn file_index(&self, pos: Pos) -> usize {
        self.files
            .partition_point(|f| f.base <= pos)
            .saturating_sub(1)
    }

    /// A position's offset within its own file.
    fn local_pos(file: &FileEntry, pos: Pos) -> Pos {
        pos.saturating_sub(file.base).min(file.text.len() as Pos)
    }

    /// The line number `__LINE__` reports for a position.
    fn line_of(&self, pos: Pos) -> usize {
        let file = self.file_at(pos);
        file.line_of(Self::local_pos(file, pos))
    }

    /// The name `__FILE__` reports for a position.
    fn file_name_of(&self, pos: Pos) -> &str {
        let file = self.file_at(pos);
        file.name_of(Self::local_pos(file, pos))
    }

    /// The verbatim source text between two positions.
    fn raw_text(&self, from: Pos, to: Pos) -> &str {
        let file = self.file_at(from);
        let start = from.saturating_sub(file.base) as usize;
        let end = to.saturating_sub(file.base) as usize;
        file.text.get(start..end).unwrap_or("")
    }
}

/// The GNU keyword an identifier spells, if it spells one.
///
/// Everything with a leading double underscore is available in every entry
/// point, exactly as it is in GCC's `-std=c99`: the names are reserved, so
/// nothing a program may legally call its own is taken away. The two *plain*
/// spellings GCC keeps for its `gnu*` modes — `typeof` and `asm` — need a GNU
/// dialect, and `typeof` is already a keyword of its own in `c23!`.
fn gnu_keyword(name: &str, dialect: Dialect) -> Option<Keyword> {
    let keyword = match name {
        "__inline" | "__inline__" => Keyword::InlineGnu,
        "__const" | "__const__" => Keyword::Const,
        "__signed" | "__signed__" => Keyword::Signed,
        "__volatile" | "__volatile__" => Keyword::Volatile,
        "__restrict" | "__restrict__" => Keyword::RestrictGnu,
        "__complex__" | "__complex" => Keyword::Complex,
        "__attribute" | "__attribute__" => Keyword::Attribute,
        "__extension__" => Keyword::Extension,
        "__alignof" | "__alignof__" => Keyword::AlignofGnu,
        "__typeof" | "__typeof__" => Keyword::TypeofGnu,
        "__typeof_unqual__" | "__typeof_unqual" => Keyword::TypeofUnqualGnu,
        "__asm" | "__asm__" => Keyword::Asm,
        "__label__" => Keyword::Label,
        "__auto_type" => Keyword::AutoType,
        "__thread" => Keyword::ThreadGnu,
        "__int128" => Keyword::Int128,
        "__real" | "__real__" => Keyword::RealGnu,
        "__imag" | "__imag__" => Keyword::ImagGnu,
        "asm" if dialect.is_gnu() => Keyword::Asm,
        "typeof" if dialect.is_gnu() => Keyword::TypeofGnu,
        _ => return None,
    };
    Some(keyword)
}

/// The end-of-file token an empty file still has to produce.
fn eof_token(base: Pos) -> PTok {
    PTok {
        kind: TokenKind::Eof,
        range: SourceRange::at(base),
        bol: true,
        space: true,
        origin: Origin::Source,
        hide: HideSet::default(),
        errors: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// macro replacement
// ---------------------------------------------------------------------------

/// The arguments of one function-like invocation.
struct Args {
    /// The arguments as written.
    raw: Vec<Vec<PTok>>,
    /// The arguments after full macro replacement, computed on demand: an
    /// argument used only by `#` or `##` must never be expanded, and expanding
    /// an unused one could report an error the program does not contain.
    expanded: Vec<Option<Vec<PTok>>>,
}

impl Args {
    fn new(raw: Vec<Vec<PTok>>) -> Self {
        Self {
            expanded: vec![None; raw.len()],
            raw,
        }
    }

    fn get(&self, index: usize) -> &[PTok] {
        self.raw.get(index).map_or(&[], Vec::as_slice)
    }
}

/// One element of a replacement list under construction.
///
/// The placemarker is the standard's own device (6.10.3.3p2): it stands where
/// an empty argument was, so that `a ## b` with an empty `b` pastes into `a`
/// rather than into whatever came next.
enum Piece {
    Tok(PTok),
    Placemarker,
}

impl Pp<'_> {
    /// Replaces `tok` if it invokes a macro, pushing the result back onto the
    /// stream so that it is rescanned.
    ///
    /// `allow_input` says whether the `(` of a function-like invocation may be
    /// read from the file. It is false while an argument is being
    /// pre-expanded, where the standard says the argument behaves as if it
    /// were the whole rest of the file.
    fn try_expand(&mut self, tok: &PTok, allow_input: bool) -> bool {
        if self.aborted {
            return false;
        }
        let Some(name) = tok.name() else {
            return false;
        };
        if tok.hide.contains(name) {
            // Painted blue: the token was produced by this very macro, and the
            // hide set travels with it, so it stays unreplaceable for good.
            return false;
        }
        let Some(def) = self.macros.get(name).cloned() else {
            return false;
        };
        let name = name.to_owned();

        if let Some(builtin) = def.builtin {
            let value = self.builtin_token(builtin, tok, &def, &name);
            self.push_pending(vec![value], tok.space);
            return true;
        }

        let Some(params) = &def.params else {
            let hide = tok.hide.add(&name);
            let exp = self.expansion_of(&name, tok.range, &def, tok);
            let mut args = Args::new(Vec::new());
            let body = self.subst(&def, &mut args, &hide, tok.range, &exp);
            self.push_pending(body, tok.space);
            if !def.predefined {
                self.expansions.record(tok.range, &name, def.name_range);
            }
            return true;
        };

        // A function-like macro's name is only an invocation when the very
        // next token is `(`.
        if !self
            .peek(allow_input)
            .is_some_and(|t| t.is_punct(Punct::LParen))
        {
            return false;
        }
        let params = params.clone();
        self.bump(allow_input);

        let Some((mut raw, rparen)) = self.collect_args(&def, &params, tok.range, allow_input)
        else {
            return true;
        };
        let invocation = tok.range.join(rparen.range);
        if !self.check_arity(&def, &params, raw.len(), &name, invocation) {
            return true;
        }
        if def.variadic {
            // C99 asks for one more argument than there are parameters; GCC
            // and everyone who writes `LOG("done")` disagree, so `...` is
            // allowed to match nothing and `__VA_ARGS__` is then empty.
            while raw.len() <= params.len() {
                raw.push(Vec::new());
            }
        }

        let hide = tok.hide.intersect(&rparen.hide).add(&name);
        let exp = self.expansion_of(&name, invocation, &def, tok);
        let mut args = Args::new(raw);
        let body = self.subst(&def, &mut args, &hide, invocation, &exp);
        self.push_pending(body, tok.space);
        if !def.predefined {
            self.expansions.record(invocation, &name, def.name_range);
        }
        true
    }

    fn expansion_of(
        &self,
        name: &str,
        invocation: SourceRange,
        def: &MacroDef,
        tok: &PTok,
    ) -> Arc<Expansion> {
        Arc::new(Expansion {
            name: name.to_owned(),
            invocation,
            definition: def.name_range,
            parent: tok.origin.expansion().cloned(),
        })
    }

    /// Pushes replacement output back onto the stream, innermost first.
    fn push_pending(&mut self, mut toks: Vec<PTok>, space: bool) {
        if self.budget < toks.len() {
            if !self.aborted {
                let range = toks.first().map_or(SourceRange::at(self.base), |t| t.range);
                self.diags
                    .error(range, "macro expansion produced too many tokens");
                self.aborted = true;
            }
            return;
        }
        self.budget -= toks.len();
        if let Some(first) = toks.first_mut() {
            // The replacement stands where the invocation did, so it inherits
            // its spacing — and it can never open a directive.
            first.space = space;
            first.bol = false;
        }
        self.pending.extend(toks.into_iter().rev());
    }

    /// Collects a function-like invocation's arguments, returning them
    /// together with the `)` that closed the list.
    fn collect_args(
        &mut self,
        def: &MacroDef,
        params: &[String],
        name_range: SourceRange,
        allow_input: bool,
    ) -> Option<(Vec<Vec<PTok>>, PTok)> {
        let mut args: Vec<Vec<PTok>> = vec![Vec::new()];
        let mut depth = 0u32;
        loop {
            // Running out of tokens is the same failure whether the file ended
            // or the argument being pre-expanded did.
            let Some(tok) = self.bump(allow_input).filter(|t| !t.is_eof()) else {
                self.diags.error(
                    name_range,
                    "unterminated argument list of a function-like macro",
                );
                return None;
            };
            // An argument's tokens were formed in translation phase 3, and a
            // universal character name that is ill formed there is ill formed
            // whatever the macro does with it — including nothing. Every UCN
            // in Clang's own `C99/n717.c` is written as the argument of a
            // macro that expands to nothing, and each one still has to be
            // diagnosed. Only what is wrong with the *spelling* travels this
            // way; a stray `\` or `$` is a preprocessing token like any other
            // until something tries to parse it (6.4p3). `Pp::reported` keeps
            // it to one diagnostic if the token survives as well.
            self.report_lexical_errors(&tok);
            if tok.is_punct(Punct::LParen) {
                depth += 1;
            } else if tok.is_punct(Punct::RParen) {
                if depth == 0 {
                    // `f()` for a macro that takes nothing is no argument at
                    // all, rather than one empty one.
                    if !def.variadic && params.is_empty() && args.len() == 1 && args[0].is_empty() {
                        args.clear();
                    }
                    return Some((args, tok));
                }
                depth -= 1;
            } else if tok.is_punct(Punct::Comma)
                && depth == 0
                && (!def.variadic || args.len() <= params.len())
            {
                args.push(Vec::new());
                continue;
            }
            args.last_mut()
                .expect("the argument list is never empty")
                .push(tok);
        }
    }

    /// Checks the number of arguments against the parameter list, reporting a
    /// mismatch at the invocation and answering whether to go on.
    ///
    /// An invocation whose arity is wrong expands to nothing: the error has
    /// been reported, and substituting made-up arguments would only add
    /// syntax errors on top of it.
    fn check_arity(
        &mut self,
        def: &MacroDef,
        params: &[String],
        given: usize,
        name: &str,
        invocation: SourceRange,
    ) -> bool {
        let wanted = params.len();
        let ok = if def.variadic {
            given >= wanted
        } else {
            given == wanted
        };
        if ok {
            return true;
        }
        let message = if given < wanted {
            let least = if def.variadic { "at least " } else { "" };
            format!("macro '{name}' requires {least}{wanted} arguments, but only {given} given")
        } else {
            format!("macro '{name}' passed {given} arguments, but takes just {wanted}")
        };
        self.diags.push(
            Diagnostic::error(invocation, message)
                .with_note_at(def.name_range, format!("macro '{name}' defined")),
        );
        false
    }

    /// Builds a replacement list: the standard's `subst`, placemarkers and all.
    fn subst(
        &mut self,
        def: &MacroDef,
        args: &mut Args,
        hide: &HideSet,
        invocation: SourceRange,
        exp: &Arc<Expansion>,
    ) -> Vec<PTok> {
        // `__VA_OPT__` is resolved first, so that everything below sees an
        // ordinary replacement list.
        let expanded;
        let body: &[PTok] = match expand_va_opt(def, args) {
            Some(tokens) => {
                expanded = tokens;
                &expanded
            }
            None => &def.body,
        };
        let mut pieces: Vec<Piece> = Vec::with_capacity(body.len());
        let mut i = 0;
        while i < body.len() {
            let tok = &body[i];

            // `# parameter` — stringification.
            if def.params.is_some()
                && tok.is_punct(Punct::Hash)
                && let Some(next) = body.get(i + 1)
                && let Some(index) = next.name().and_then(|n| def.param_index(n))
            {
                let kind = self.stringify(args.get(index));
                pieces.push(Piece::Tok(self.synthetic(kind, tok, invocation, exp)));
                i += 2;
                continue;
            }

            // GNU's comma elision, `printf(fmt, ## __VA_ARGS__)`: the `##`
            // between a comma and the variable arguments deletes the comma
            // when the invocation passed none, and does nothing at all when it
            // passed some — the arguments are then macro-replaced as usual,
            // which is what makes it different from an ordinary paste.
            // `__VA_OPT__` is C23's way of saying the same thing.
            if tok.is_punct(Punct::Comma)
                && body.get(i + 1).is_some_and(|t| t.is_punct(Punct::HashHash))
                && let Some(index) = body
                    .get(i + 2)
                    .and_then(PTok::name)
                    .and_then(|n| def.param_index(n))
                && Some(index) == def.va_index()
            {
                if !args.get(index).is_empty() {
                    let mut comma = tok.clone();
                    comma.range = invocation;
                    comma.origin = Origin::Expansion(exp.clone());
                    pieces.push(Piece::Tok(comma));
                    let arg = self.expanded_arg(args, index);
                    pieces.extend(arg.into_iter().map(Piece::Tok));
                }
                i += 3;
                continue;
            }

            // `## something` — pasting.
            if tok.is_punct(Punct::HashHash)
                && let Some(next) = body.get(i + 1)
            {
                let rhs = paste_operand(def, args, next);
                self.paste_pieces(&mut pieces, rhs, invocation, exp);
                i += 2;
                continue;
            }

            // A parameter: `##` on either side keeps it unexpanded.
            if let Some(index) = tok.name().and_then(|n| def.param_index(n)) {
                let raw = body.get(i + 1).is_some_and(|t| t.is_punct(Punct::HashHash));
                if raw {
                    let arg = args.get(index).to_vec();
                    if arg.is_empty() {
                        pieces.push(Piece::Placemarker);
                    } else {
                        pieces.extend(arg.into_iter().map(Piece::Tok));
                    }
                } else {
                    let arg = self.expanded_arg(args, index);
                    pieces.extend(arg.into_iter().map(Piece::Tok));
                }
                i += 1;
                continue;
            }

            let mut copy = tok.clone();
            // The replacement list was written in the `#define`, but it stands
            // where the invocation is, and that is where a diagnostic belongs.
            copy.range = invocation;
            copy.origin = Origin::Expansion(exp.clone());
            pieces.push(Piece::Tok(copy));
            i += 1;
        }

        pieces
            .into_iter()
            .filter_map(|p| match p {
                Piece::Tok(mut t) => {
                    t.hide = t.hide.union(hide);
                    Some(t)
                }
                Piece::Placemarker => None,
            })
            .collect()
    }

    /// Pastes the last piece built so far onto the first of `rhs`.
    fn paste_pieces(
        &mut self,
        pieces: &mut Vec<Piece>,
        mut rhs: Vec<Piece>,
        invocation: SourceRange,
        exp: &Arc<Expansion>,
    ) {
        if rhs.is_empty() {
            return;
        }
        let head = rhs.remove(0);
        let left = pieces.pop();
        let joined = match (left, head) {
            (None, head) => head,
            (Some(Piece::Placemarker), head) => head,
            (Some(left), Piece::Placemarker) => left,
            (Some(Piece::Tok(l)), Piece::Tok(r)) => match self.paste(&l, &r, invocation) {
                Some(kind) => Piece::Tok(self.synthetic(kind, &l, invocation, exp)),
                None => {
                    // Already reported; keep both halves so that the rest of
                    // the expansion still makes some kind of sense.
                    pieces.push(Piece::Tok(l));
                    Piece::Tok(r)
                }
            },
        };
        pieces.push(joined);
        pieces.extend(rhs);
    }

    /// Concatenates two spellings and lexes the result.
    fn paste(&mut self, lhs: &PTok, rhs: &PTok, invocation: SourceRange) -> Option<TokenKind> {
        let text = format!("{}{}", lhs.spelling(), rhs.spelling());
        if text.is_empty() {
            return None;
        }
        let tokens = lex::lex_text(&text, 0, &self.lex_options);
        let valid = tokens.len() == 2
            && tokens[0].errors.is_empty()
            && !matches!(tokens[0].kind, TokenKind::Error(_))
            && tokens[0].range.end as usize == text.len();
        if !valid {
            self.diags.error(
                invocation,
                format!(
                    "pasting '{}' and '{}' does not give a valid token",
                    lhs.spelling(),
                    rhs.spelling()
                ),
            );
            return None;
        }
        Some(tokens[0].kind.clone())
    }

    /// A token that `#` or `##` made up, standing at the invocation.
    fn synthetic(
        &self,
        kind: TokenKind,
        like: &PTok,
        invocation: SourceRange,
        exp: &Arc<Expansion>,
    ) -> PTok {
        PTok {
            kind,
            range: invocation,
            bol: false,
            space: like.space,
            origin: Origin::Expansion(exp.clone()),
            hide: like.hide.clone(),
            errors: Vec::new(),
        }
    }

    /// An argument after full macro replacement, computed once.
    fn expanded_arg(&mut self, args: &mut Args, index: usize) -> Vec<PTok> {
        if let Some(Some(done)) = args.expanded.get(index) {
            return done.clone();
        }
        let raw = args.get(index).to_vec();
        let done = self.expand_sequence(raw);
        if let Some(slot) = args.expanded.get_mut(index) {
            *slot = Some(done.clone());
        }
        done
    }

    /// Fully replaces the macros in a self-contained token sequence.
    ///
    /// "Self-contained" is the point: 6.10.3.1 says an argument is expanded as
    /// if it were the whole rest of the file, so a function-like macro name at
    /// its end does not reach out for a `(` that follows the invocation.
    fn expand_sequence(&mut self, toks: Vec<PTok>) -> Vec<PTok> {
        if toks.is_empty() {
            return toks;
        }
        self.depth += 1;
        if self.depth > MAX_EXPANSION_DEPTH {
            self.depth -= 1;
            if !self.aborted {
                let range = toks[0].range;
                self.diags.error(range, "macro arguments nest too deeply");
                self.aborted = true;
            }
            return toks;
        }
        let saved = std::mem::replace(&mut self.pending, toks.into_iter().rev().collect());
        let mut out = Vec::new();
        while let Some(tok) = self.pending.pop() {
            if tok.name().is_some() && self.try_expand(&tok, false) {
                continue;
            }
            out.push(tok);
        }
        self.pending = saved;
        self.depth -= 1;
        out
    }

    /// The token a built-in macro stands for at this use.
    fn builtin_token(&mut self, builtin: Builtin, tok: &PTok, def: &MacroDef, name: &str) -> PTok {
        let kind = match builtin {
            Builtin::Line => {
                let line = self.line_of(tok.range.start) as u128;
                TokenKind::Int(IntLit {
                    value: line,
                    base: NumBase::Decimal,
                    unsigned: false,
                    long: LongKind::None,
                    text: line.to_string(),
                })
            }
            // Both of these read the position of the *use*, which is what
            // makes `assert(x)` — whose `__FILE__` and `__LINE__` are written
            // in <assert.h> — report the line the assertion is on.
            Builtin::File => {
                let file = self.file_name_of(tok.range.start).to_owned();
                string_token_kind(&file)
            }
            Builtin::FileName => {
                let file = self.file_name_of(tok.range.start);
                let base = file
                    .rsplit_once(['/', '\\'])
                    .map_or(file, |(_, base)| base)
                    .to_owned();
                string_token_kind(&base)
            }
            Builtin::IncludeLevel => int_token_kind((self.open.len() - 1) as u128),
            Builtin::Counter => {
                let value = self.counter;
                self.counter += 1;
                int_token_kind(u128::from(value))
            }
        };
        let exp = self.expansion_of(name, tok.range, def, tok);
        PTok {
            kind,
            range: tok.range,
            bol: false,
            space: tok.space,
            origin: Origin::Expansion(exp),
            hide: tok.hide.add(name),
            errors: Vec::new(),
        }
    }
}

/// Wraps `text` in quotes and re-lexes it as a C string literal.
///
/// Falling back to the raw bytes cannot normally happen — everything this is
/// handed was built to be a string literal — but a decoded value has to come
/// out either way.
fn relex_string(text: String, options: &LexOptions) -> TokenKind {
    let tokens = lex::lex_text(&text, 0, options);
    if tokens.len() == 2
        && tokens[0].errors.is_empty()
        && matches!(tokens[0].kind, TokenKind::Str(_))
        && tokens[0].range.end as usize == text.len()
    {
        return tokens[0].kind.clone();
    }
    let inner = text.trim_matches('"');
    TokenKind::Str(StrLit {
        kind: StrKind::Narrow,
        values: inner.bytes().map(u32::from).collect(),
        text,
    })
}

impl Pp<'_> {
    /// Turns an argument into the string literal `#` makes of it (6.10.3.2).
    ///
    /// White space between tokens becomes exactly one space and leading and
    /// trailing white space is dropped. A `"` or `\` is escaped only where the
    /// standard says it is — *inside* a character constant or a string literal
    /// — which is why `str(: @\n)` comes out as `": @\n"`, backslash intact,
    /// while `str("a\0b")` comes out as `"\"a\\0b\""`.
    ///
    /// The text is then lexed back, so the literal's decoded value is whatever
    /// a C compiler would make of the literal that was written.
    fn stringify(&self, arg: &[PTok]) -> TokenKind {
        let mut text = String::from('"');
        for (i, tok) in arg.iter().enumerate() {
            if i > 0 && tok.space {
                text.push(' ');
            }
            let quoted = matches!(tok.kind, TokenKind::Char(_) | TokenKind::Str(_));
            for c in tok.spelling().chars() {
                if quoted && (c == '"' || c == '\\') {
                    text.push('\\');
                }
                text.push(c);
            }
        }
        text.push('"');
        relex_string(text, &self.lex_options)
    }
}

/// The pieces the right-hand operand of `##` contributes.
///
/// A parameter here is *never* macro-replaced first (6.10.3.3p1), and an empty
/// argument leaves a placemarker so that the paste happens to whatever is on
/// the other side rather than to whatever comes next.
fn paste_operand(def: &MacroDef, args: &Args, tok: &PTok) -> Vec<Piece> {
    let Some(index) = tok.name().and_then(|n| def.param_index(n)) else {
        return vec![Piece::Tok(tok.clone())];
    };
    let arg = args.get(index);
    if arg.is_empty() {
        return vec![Piece::Placemarker];
    }
    arg.iter().cloned().map(Piece::Tok).collect()
}

/// The largest line number `#line` may name (C99 6.10.4p3).
const MAX_LINE_NUMBER: u64 = 2_147_483_647;

/// The value of a `digit-sequence` token, which is what `#line` takes.
///
/// A *digit sequence* is not an integer constant: `#line 010` is line ten, not
/// line eight, and `#line 0x10`, `#line 1u` and `#line 1.0` are none of the
/// three. Reading the spelling rather than the lexer's value is what says so.
/// A sequence too long for the range check below comes back saturated, so it is
/// reported as out of range rather than as not a number at all.
fn digit_sequence(kind: &TokenKind) -> Option<u64> {
    let TokenKind::Int(lit) = kind else {
        return None;
    };
    if lit.text.is_empty() || !lit.text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(lit.text.parse::<u64>().unwrap_or(u64::MAX))
}

/// Whether a `#line`'s operands are already one of the two forms 6.10.4 gives,
/// in which case they are used as they stand rather than macro-replaced first.
fn is_line_form(rest: &[PTok]) -> bool {
    let Some(first) = rest.first() else {
        return false;
    };
    if digit_sequence(&first.kind).is_none() {
        return false;
    }
    match rest.len() {
        1 => true,
        2 => matches!(&rest[1].kind, TokenKind::Str(lit) if lit.kind == StrKind::Narrow),
        _ => false,
    }
}

/// The decimal integer token a built-in macro expands to.
fn int_token_kind(value: u128) -> TokenKind {
    TokenKind::Int(IntLit {
        value,
        base: NumBase::Decimal,
        unsigned: false,
        long: LongKind::None,
        text: value.to_string(),
    })
}

/// The narrow string literal token a predefined macro expands to.
fn string_token_kind(value: &str) -> TokenKind {
    TokenKind::Str(StrLit {
        kind: StrKind::Narrow,
        values: value.bytes().map(u32::from).collect(),
        text: quote_c_string(value),
    })
}

/// Wraps `text` in quotes, escaping what a C string literal cannot hold plain.
fn quote_c_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// directives
// ---------------------------------------------------------------------------

impl Pp<'_> {
    /// Executes the directive the file's next token opens.
    fn directive(&mut self) {
        let hash = self.ahead().clone();
        self.cur_mut().pos += 1;
        let start = self.cur().pos;
        while !self.ahead().is_eof() && !self.ahead().bol {
            self.cur_mut().pos += 1;
        }
        let file = self.cur();
        let line: Vec<PTok> = file.input[start..file.pos].to_vec();

        let Some(first) = line.first() else {
            // The null directive, which does nothing at all.
            return;
        };
        let range = hash.range.join(first.range);
        let Some(name) = first.name() else {
            if matches!(first.kind, TokenKind::Int(_)) {
                // A GCC line marker, `# 42 "file.h" 1 3 4`: `#line` without the
                // keyword, with flags saying whether the compiler is entering
                // or leaving a file. The numbering is all this needs from it.
                if !self.skipping() {
                    self.line_directive(&line, range, true);
                }
                return;
            }
            if !self.skipping() {
                self.diags.error(
                    range,
                    format!(
                        "invalid preprocessing directive after '#': {}",
                        first.kind.describe()
                    ),
                );
            }
            return;
        };
        let rest = &line[1..];

        match name {
            "if" => self.open_cond(range, |pp| pp.eval_condition(rest, range)),
            "ifdef" | "ifndef" => {
                let want = name == "ifdef";
                self.open_cond(range, |pp| {
                    pp.macro_name_operand(rest, range, name)
                        .is_some_and(|n| pp.macros.contains_key(&n) == want)
                });
            }
            "elif" => self.elif(range, "elif", |pp| pp.eval_condition(rest, range)),
            // C23's `#elifdef` / `#elifndef`, which say what
            // `#elif defined(X)` says.
            "elifdef" | "elifndef" => {
                self.require_standard(Standard::C23, &format!("'#{name}'"), range);
                let want = name == "elifdef";
                self.elif(range, name, |pp| {
                    pp.macro_name_operand(rest, range, name)
                        .is_some_and(|n| pp.macros.contains_key(&n) == want)
                });
            }
            "else" => self.else_(rest, range),
            "endif" => self.endif(range),
            _ if self.skipping() => {
                // Inside a skipped group only the conditionals are tracked;
                // everything else is text, and text is not our business.
            }
            "define" => self.define(rest, range),
            "undef" => self.undef(rest, range),
            "include" => self.include(&line, range),
            // `#include_next` exists to reach the *next* header of a name on
            // the search path, which only makes sense when the system
            // directories are on it — and they never are here.
            "include_next" => {
                self.diags.error(
                    range,
                    "#include_next is not supported; cinrs never searches the platform's \
                     include directories, so there is no next header to reach",
                );
            }
            "error" => {
                let text = self.directive_text(&line, 1);
                let message = if text.is_empty() {
                    "#error".to_owned()
                } else {
                    format!("#error {text}")
                };
                self.diags.error(range, message);
            }
            "warning" => {
                let text = self.directive_text(&line, 1);
                self.diags.warning(range, format!("#warning {text}"));
            }
            "pragma" => self.pragma(rest, range),
            "embed" => {
                self.require_standard(Standard::C23, "'#embed'", range);
                self.embed(rest, range);
            }
            "line" => self.line_directive(rest, range, false),
            // `#ident "string"` and `#sccs` put a string into a section of the
            // object file that nothing here has; GCC ignores them too when the
            // target has no such section.
            "ident" | "sccs" => {}
            other => {
                self.diags
                    .error(range, format!("invalid preprocessing directive #{other}"));
            }
        }
    }

    /// `#line` (C99 6.10.4), and GCC's `# 42 "file.h" 1 3 4` line marker.
    ///
    /// Both say the same thing: the line after the directive is line N, and
    /// `__FILE__` is the name that follows until the next directive or the end
    /// of the file. The marker's trailing flags — which of "entering",
    /// "returning", "system header" and "extern C" applies — describe an
    /// `#include` that has already happened elsewhere, so they are read and
    /// dropped.
    ///
    /// **Only `__LINE__` and `__FILE__` move.** A diagnostic still points at
    /// the token that was really written, in the file it was really written
    /// in, because that is the position the user can look at — the whole
    /// reason this crate maps every token back to a `proc_macro2::Span`. A
    /// `#line` in generated C therefore renumbers what the *program* observes
    /// without hiding where the compiler found it.
    fn line_directive(&mut self, rest: &[PTok], range: SourceRange, marker: bool) {
        let what = if marker { "line marker" } else { "#line" };
        // 6.10.4p5: a `#line` matching neither of the two forms the grammar
        // gives has its tokens macro-replaced first, and the result must then
        // match one of them. c-testsuite's `00152` is `#line line`, with
        // `line` a macro for 1000.
        let expanded: Vec<PTok>;
        let toks: &[PTok] = if marker || is_line_form(rest) {
            rest
        } else {
            expanded = self.expand_sequence(rest.to_vec());
            &expanded
        };

        let Some(first) = toks.first() else {
            self.diags
                .error(range, format!("'{what}' requires a line number"));
            return;
        };
        let Some(digits) = digit_sequence(&first.kind) else {
            self.diags.error(
                first.range,
                format!(
                    "'{what}' requires a decimal line number, found {}",
                    first.kind.describe()
                ),
            );
            return;
        };
        // 6.10.4p3: the digit sequence shall not specify zero, nor a number
        // greater than 2147483647.
        if digits == 0 || digits > MAX_LINE_NUMBER {
            self.diags.error(
                first.range,
                format!(
                    "the line number of '{what}' must be between 1 and {MAX_LINE_NUMBER}, \
                     not {digits}"
                ),
            );
            return;
        }

        let mut used = 1;
        let mut name = None;
        if let Some(tok) = toks.get(1) {
            match &tok.kind {
                TokenKind::Str(lit) if lit.kind == StrKind::Narrow => {
                    name = Some(
                        lit.values
                            .iter()
                            .map(|v| char::from_u32(*v).unwrap_or('\u{fffd}'))
                            .collect::<String>(),
                    );
                    used = 2;
                }
                _ if marker => {}
                _ => {
                    self.diags.error(
                        tok.range,
                        format!(
                            "the file name of '{what}' must be an ordinary string literal, \
                             found {}",
                            tok.kind.describe()
                        ),
                    );
                    return;
                }
            }
        }
        // A marker's flags are digits the compiler that wrote it understood;
        // anything else on a `#line` is what GCC calls "extra tokens at end of
        // directive" and, like GCC, warns about rather than refuses.
        if !marker && toks.len() > used {
            self.diags.warning(
                toks[used].range,
                format!("extra tokens at the end of '{what}'"),
            );
        }
        self.set_line(range.start, digits as usize, name);
    }

    /// Records what a `#line` did to the file it was written in.
    fn set_line(&mut self, pos: Pos, line: usize, name: Option<String>) {
        let index = self.file_index(pos);
        let local = Self::local_pos(&self.files[index], pos);
        let file = &mut self.files[index];
        let at = file.physical_line(local);
        // Without a name of its own the directive keeps whichever one is in
        // force, which may itself have come from an earlier `#line`.
        let name = match name {
            Some(name) => name,
            None => file.name_of(local).to_owned(),
        };
        // The list is searched by binary search, so it has to stay sorted. A
        // file is only ever read forwards, so this drops nothing in practice.
        while file.lines.last().is_some_and(|d| d.at >= at) {
            file.lines.pop();
        }
        file.lines.push(LineDirective { at, line, name });
    }

    /// The raw source text of a directive line from its `skip`-th token on.
    ///
    /// `#error` has to reproduce what was written rather than a rendering of
    /// the tokens, and `#include <stdio.h>` will need the same thing: a
    /// header name in angle brackets is not one token either.
    fn directive_text(&self, line: &[PTok], skip: usize) -> String {
        let Some(first) = line.get(skip) else {
            return String::new();
        };
        let last = line.last().unwrap_or(first);
        self.raw_text(first.range.start, last.range.end)
            .trim()
            .to_owned()
    }

    /// `#pragma`.
    ///
    /// Every pragma this implementation does not know is silently ignored,
    /// which is what 6.10.6 asks for — with one exception: a `#pragma cinrs`
    /// is addressed to *us*, so an option we do not know is a mistake worth
    /// reporting rather than a hint some other compiler might understand.
    ///
    /// The five it does know configure the unit:
    ///
    /// ```c
    /// #pragma cinrs include_path "vendor/include"
    /// #pragma cinrs link "m"
    /// #pragma cinrs export
    /// #pragma cinrs no_std
    /// #pragma cinrs module "geometry"
    /// ```
    ///
    /// They are directives rather than macro arguments or attributes so that
    /// they read the same, and mean the same, in raw-token and in
    /// string-literal input.
    fn pragma(&mut self, rest: &[PTok], range: SourceRange) {
        match rest.first().and_then(PTok::name) {
            Some("once") => {
                let key = self.cur_key();
                self.once.insert(key);
            }
            Some("cinrs") => self.cinrs_pragma(&rest[1..], range),
            Some("pack") => self.pack_pragma(&rest[1..], range),
            Some("push_macro") => self.push_macro_pragma(&rest[1..], range, true),
            Some("pop_macro") => self.push_macro_pragma(&rest[1..], range, false),
            Some("GCC") => self.gcc_pragma(&rest[1..], range),
            // `#pragma message`, `#pragma region` / `#pragma endregion`,
            // `#pragma weak` and everything else are ignored, which 6.10.6 is
            // explicit about. `weak` is the one worth knowing about: it asks
            // for weak linkage, which stable Rust cannot express at all, so
            // ignoring it is the same answer `__attribute__((weak))` gets —
            // see `doc/gnu-extensions.md`.
            _ => {}
        }
    }

    /// `#pragma GCC …`.
    fn gcc_pragma(&mut self, rest: &[PTok], range: SourceRange) {
        match rest.first().and_then(PTok::name) {
            // A program that poisons a name means it never to be written
            // again, and honouring that costs one lookup per identifier.
            Some("poison") => {
                for tok in &rest[1..] {
                    match tok.name() {
                        Some(name) => {
                            self.poisoned.insert(name.to_owned());
                        }
                        None => self.diags.error(
                            tok.range,
                            format!(
                                "'#pragma GCC poison' takes identifiers, found {}",
                                tok.kind.describe()
                            ),
                        ),
                    }
                }
            }
            Some("error") => {
                let text = self.pragma_message(&rest[1..]);
                self.diags.error(range, format!("#pragma GCC error {text}"));
            }
            Some("warning") => {
                let text = self.pragma_message(&rest[1..]);
                self.diags
                    .warning(range, format!("#pragma GCC warning {text}"));
            }
            // `diagnostic push/pop/ignored/warning/error`, `system_header`,
            // `visibility` and the rest: there are no warnings of ours to
            // suppress and no visibility to set, so they are accepted and
            // ignored.
            _ => {}
        }
    }

    /// The text of a pragma that carries a message.
    fn pragma_message(&self, rest: &[PTok]) -> String {
        match rest.first() {
            Some(tok) => tok.spelling().to_owned(),
            None => String::new(),
        }
    }

    /// `#pragma push_macro("X")` and `#pragma pop_macro("X")`.
    ///
    /// MSVC's, and in GCC since 4.4: a header that has to redefine a macro for
    /// a few lines saves the old definition and puts it back. c-testsuite's
    /// `00206` is exactly that, and it is the reason this is here.
    fn push_macro_pragma(&mut self, rest: &[PTok], range: SourceRange, push: bool) {
        let what = if push { "push_macro" } else { "pop_macro" };
        // The name is a *string literal*, which is then read as an identifier.
        let inner = match rest {
            [tok] if tok.is_punct(Punct::LParen) => None,
            _ => rest
                .iter()
                .find_map(|tok| match &tok.kind {
                    TokenKind::Str(lit) => lit.as_bytes(),
                    _ => None,
                })
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
        };
        let Some(name) = inner.filter(|name| !name.is_empty()) else {
            self.diags.error(
                range,
                format!("#pragma {what} needs a string literal naming a macro"),
            );
            return;
        };
        if push {
            let saved = self.macros.get(&name).cloned();
            self.macro_stacks.entry(name).or_default().push(saved);
            return;
        }
        match self.macro_stacks.get_mut(&name).and_then(Vec::pop) {
            Some(Some(def)) => {
                self.macros.insert(name, def);
            }
            Some(None) => {
                self.macros.remove(&name);
            }
            // GCC ignores a `pop_macro` with nothing pushed.
            None => {}
        }
    }

    /// `#pragma pack(…)`, which changes the alignment a record's members are
    /// laid out with until the next one.
    ///
    /// The value in effect where a `struct` is *defined* is what applies to it;
    /// [`Preprocessed::pack_events`] carries the changes to the parser, which
    /// records the one each specifier saw.
    fn pack_pragma(&mut self, rest: &[PTok], range: SourceRange) {
        let bad = |pp: &mut Self, at: SourceRange| {
            pp.diags.error(
                at,
                "#pragma pack expects '(N)', '(push, N)', '(push)', '(pop)' or '()', \
                 where N is a power of two up to 16",
            );
        };
        if !rest.first().is_some_and(|t| t.is_punct(Punct::LParen))
            || !rest.last().is_some_and(|t| t.is_punct(Punct::RParen))
            || rest.len() < 2
        {
            bad(self, range);
            return;
        }
        let inner = &rest[1..rest.len() - 1];
        let value = |pp: &mut Self, tok: &PTok| -> Option<u32> {
            let TokenKind::Int(lit) = &tok.kind else {
                bad(pp, tok.range);
                return None;
            };
            let n = u32::try_from(lit.value)
                .ok()
                .filter(|n| n.is_power_of_two() && *n <= 16);
            if n.is_none() {
                bad(pp, tok.range);
            }
            n
        };
        let next = match inner {
            [] => Some(None),
            [tok] if tok.name() == Some("pop") => match self.pack_stack.pop() {
                Some(value) => Some(value),
                None => {
                    self.diags
                        .error(range, "#pragma pack(pop) with nothing pushed");
                    return;
                }
            },
            [tok] if tok.name() == Some("push") => {
                self.pack_stack.push(self.pack);
                Some(self.pack)
            }
            [tok] => value(self, tok).map(Some),
            [push, comma, tok] if push.name() == Some("push") && comma.is_punct(Punct::Comma) => {
                self.pack_stack.push(self.pack);
                value(self, tok).map(Some)
            }
            _ => {
                bad(self, range);
                return;
            }
        };
        let Some(next) = next else { return };
        self.pack = next;
        let at = self.out.len();
        self.pack_events.push((at, next));
    }

    /// The identity of the file being read, for `#pragma once`.
    fn cur_key(&self) -> String {
        self.cur().key.clone()
    }

    /// The `#pragma cinrs` options, for the diagnostics that list them.
    const OPTIONS: &'static str =
        "'target', 'include_path', 'link', 'export', 'no_std', 'module' and 'crate'";

    /// `#pragma cinrs …`.
    fn cinrs_pragma(&mut self, rest: &[PTok], range: SourceRange) {
        let Some(option) = rest.first() else {
            self.diags.error(
                range,
                format!("#pragma cinrs needs an option: {}", Self::OPTIONS),
            );
            return;
        };
        let name = option.name().unwrap_or_default();
        match name {
            // The scan before preprocessing already read this one and applied
            // it; all that is left is to say so when it cannot have worked.
            "target" => self.target_pragma(range),
            "include_path" | "link" | "module" | "crate" => {
                let Some(value) = self.pragma_string(&rest[1..], option.range, name) else {
                    return;
                };
                match name {
                    "include_path" => self.search.add_pragma(&value),
                    "link" => {
                        if !self.link_libraries.contains(&value) {
                            self.link_libraries.push(value);
                        }
                    }
                    "crate" => self.crate_pragma(value, rest[1].range),
                    _ => self.module_pragma(value, rest[1].range),
                }
            }
            // Unit-wide and argument-less: everything with external linkage
            // becomes a real C symbol, and the `Vec` a variable length array
            // or `alloca` needs comes from `alloc` rather than from `std`.
            "export" | "no_std" => {
                if let Some(extra) = rest.get(1) {
                    self.diags.error(
                        extra.range,
                        format!(
                            "unexpected {} after #pragma cinrs {name}, which takes no argument",
                            extra.kind.describe()
                        ),
                    );
                }
                if name == "export" {
                    self.export = true;
                } else {
                    self.no_std = true;
                }
            }
            other => {
                let what = if other.is_empty() {
                    option.kind.describe().to_owned()
                } else {
                    format!("'{other}'")
                };
                self.diags.error(
                    option.range,
                    format!(
                        "unknown #pragma cinrs option {what}; the options are {}",
                        Self::OPTIONS
                    ),
                );
            }
        }
    }

    /// `#pragma cinrs target "<triple>"`, seen a second time.
    ///
    /// [`scan_target_pragma`] read every one in the unit's own text before
    /// this pass began and either applied it or reported it, so there is
    /// nothing left to do — except in the two cases the scan cannot serve, and
    /// where silence would mean translating for the wrong machine:
    ///
    /// * the directive is one the scan never saw, because it is in a *header*
    ///   or came out of `_Pragma`, so the model it names was never applied;
    /// * it stands after an `#include` or an `#if`, both of which had already
    ///   been answered with the old model.
    ///
    /// A `target` inside a group `#if 0` skips is the mirror image — the scan
    /// applied it and this pass never sees it — which the module
    /// documentation says, and which is why this is the only pragma read
    /// twice.
    fn target_pragma(&mut self, range: SourceRange) {
        if !self.target_pragmas.scanned(range) {
            let now = match self.target_source.triple() {
                Some(triple) => format!("for '{triple}'"),
                None => format!("for the model {} named", self.target_source.as_str()),
            };
            self.diags.error(
                range,
                format!(
                    "'#pragma cinrs target' is read before preprocessing, so it has to be a \
                     directive in the unit's own text: a header's comes too late, and one \
                     out of '_Pragma' is never seen. This unit is being translated {now}"
                ),
            );
            return;
        }
        // The scan read this one. If it did not like it, it has said so
        // already and a second message would only get in the way.
        if !self.target_pragmas.applied {
            return;
        }
        if self.model_observed {
            self.diags.error(
                range,
                "'#pragma cinrs target' must come before every '#include' and '#if', which \
                 were already answered with the previous data model",
            );
        }
    }

    /// `#pragma cinrs module "name"`, which names the module the expansion is
    /// generated into.
    ///
    /// The name has to be a Rust identifier, since that is what it becomes;
    /// naming the unit twice is a mistake rather than a silent last-one-wins.
    fn module_pragma(&mut self, name: String, range: SourceRange) {
        if !crate::codegen::is_module_name(&name) {
            self.diags.error(
                range,
                format!("'{name}' is not usable as a Rust module name"),
            );
            return;
        }
        if let Some(previous) = &self.module
            && *previous != name
        {
            self.diags.error(
                range,
                format!(
                    "this unit is already named '{previous}' by an earlier #pragma cinrs module"
                ),
            );
            return;
        }
        self.module = Some(name);
    }

    /// `#pragma cinrs crate "::my_cinrs"`, which says where the `cinrs` facade
    /// crate is to be found.
    ///
    /// The generated code names it only where it needs the runtime — today
    /// that is a complex type, and nothing else — but it has to name it in
    /// full, because the expansion goes into a module of its own and cannot
    /// rely on anything being in scope there. The default is `::cinrs`; a
    /// dependency renamed in `Cargo.toml`, or one reached through a re-export,
    /// needs this.
    fn crate_pragma(&mut self, path: String, range: SourceRange) {
        if !crate::codegen::is_crate_path(&path) {
            self.diags.error(
                range,
                format!(
                    "'{path}' is not usable as a Rust path to a crate; write something like \
                     '::my_cinrs' or 'crate::vendor::cinrs'"
                ),
            );
            return;
        }
        if let Some(previous) = &self.crate_path
            && *previous != path
        {
            self.diags.error(
                range,
                format!(
                    "this unit already reaches the cinrs crate as '{previous}', by an earlier \
                     #pragma cinrs crate"
                ),
            );
            return;
        }
        self.crate_path = Some(path);
    }

    /// The single string literal a `#pragma cinrs` option takes.
    fn pragma_string(&mut self, rest: &[PTok], range: SourceRange, option: &str) -> Option<String> {
        let Some(tok) = rest.first() else {
            self.diags.error(
                range,
                format!("#pragma cinrs {option} needs a string literal"),
            );
            return None;
        };
        let TokenKind::Str(lit) = &tok.kind else {
            self.diags.error(
                tok.range,
                format!(
                    "#pragma cinrs {option} needs a string literal, found {}",
                    tok.kind.describe()
                ),
            );
            return None;
        };
        let Some(bytes) = lit.as_bytes() else {
            self.diags.error(
                tok.range,
                format!("#pragma cinrs {option} does not take a wide string literal"),
            );
            return None;
        };
        let value = String::from_utf8_lossy(&bytes).into_owned();
        if value.is_empty() {
            self.diags.error(
                tok.range,
                format!("#pragma cinrs {option} was given an empty string"),
            );
            return None;
        }
        if let Some(extra) = rest.get(1) {
            self.diags.error(
                extra.range,
                format!(
                    "unexpected {} after #pragma cinrs {option}",
                    extra.kind.describe()
                ),
            );
        }
        Some(value)
    }

    // -- #include -----------------------------------------------------------

    /// `#include <name>`, `#include "name"` and `#include MACRO`.
    fn include(&mut self, line: &[PTok], range: SourceRange) {
        // A header reads the model — every bundled one branches on `_WIN32`
        // or on `__SIZEOF_POINTER__` — so once one is opened the model is
        // settled; see `Pp::target_pragma`.
        self.model_observed = true;
        let Some((name, form)) = self.header_name(line, range) else {
            return;
        };
        if self.open.len() >= MAX_INCLUDE_DEPTH {
            self.diags.error(
                range,
                format!("#include nested too deeply (more than {MAX_INCLUDE_DEPTH} files)"),
            );
            return;
        }
        let origin = self.cur().origin.clone();
        let found = match include::resolve(&name, form, &origin, &self.search) {
            Ok(found) => found,
            Err(include::Error::Unreadable { path, error }) => {
                self.diags
                    .error(range, format!("cannot read '{path}': {error}"));
                return;
            }
            Err(include::Error::NotFound { searched }) => {
                let quoted = match form {
                    include::Form::Angled => format!("<{name}>"),
                    include::Form::Quoted => format!("\"{name}\""),
                };
                self.diags.error(
                    range,
                    format!("{quoted} file not found; searched: {}", searched.join(", ")),
                );
                return;
            }
        };

        // The two ways a file already read can be skipped without reading it
        // again: it said `#pragma once`, or it is wrapped in an include guard
        // whose macro is still defined.
        if self.once.contains(&found.key) {
            return;
        }
        if let Some(guard) = self.guards.get(&found.key)
            && self.macros.contains_key(guard)
        {
            return;
        }
        if let Some(path) = &found.path
            && !self.user_headers.contains(path)
        {
            self.user_headers.push(path.clone());
        }
        self.open_file(found, range);
    }

    // -- #embed -------------------------------------------------------------

    /// `#embed "resource"` and `#embed <resource>` (C23 6.10.3, N3017).
    ///
    /// The directive is replaced by the bytes of the resource, written as a
    /// comma-separated list of integer constants in the range of `unsigned
    /// char` — so that `unsigned char logo[] = {\n#embed "logo.png"\n};` is an
    /// array of the file. The four standard parameters shape the list:
    /// `limit(N)` takes only the first N bytes, `prefix(…)` and `suffix(…)`
    /// bracket a *non-empty* list, and `if_empty(…)` replaces an empty one.
    ///
    /// The tokens go through the ordinary pending queue, so they are rescanned
    /// for macros exactly as any other replacement is — and are charged
    /// against [`MAX_EXPANDED_TOKENS`] like any others, which puts the ceiling
    /// on a resource near two megabytes.
    fn embed(&mut self, rest: &[PTok], range: SourceRange) {
        let Some((name, form, after)) = self.embed_operand(rest, range) else {
            return;
        };
        let Some(params) = self.embed_parameters(&rest[after..], range, true) else {
            return;
        };
        let origin = self.cur().origin.clone();
        let found = match include::resolve_embed(&name, form, &origin, &self.search) {
            Ok(found) => found,
            Err(include::Error::Unreadable { path, error }) => {
                self.diags
                    .error(range, format!("cannot read '{path}': {error}"));
                return;
            }
            Err(include::Error::NotFound { searched }) => {
                let quoted = match form {
                    include::Form::Angled => format!("<{name}>"),
                    include::Form::Quoted => format!("\"{name}\""),
                };
                // There are no bundled resources, so unlike `#include` the
                // list of places looked in really can be empty.
                let where_ = if searched.is_empty() {
                    "there is nowhere to look; name a directory with \
                     `#pragma cinrs include_path`"
                        .to_owned()
                } else {
                    format!("searched: {}", searched.join(", "))
                };
                self.diags.error(
                    range,
                    format!("{quoted} resource not found for #embed; {where_}"),
                );
                return;
            }
        };
        if !self.embedded_files.contains(&found.path) {
            self.embedded_files.push(found.path.clone());
        }

        let take = params.limit.unwrap_or(found.bytes.len());
        let bytes = &found.bytes[..take.min(found.bytes.len())];
        let mut out: Vec<PTok> = Vec::new();
        if bytes.is_empty() {
            // 6.10.3.2: `if_empty` stands for the whole expansion, and the
            // prefix and suffix are not emitted at all.
            out.extend(params.if_empty);
        } else {
            out.extend(params.prefix);
            for (i, byte) in bytes.iter().enumerate() {
                if i > 0 {
                    out.push(self.embed_token(TokenKind::Punct(Punct::Comma), range));
                }
                out.push(self.embed_token(int_token_kind(u128::from(*byte)), range));
            }
            out.extend(params.suffix);
        }
        self.push_pending(out, true);
    }

    /// The resource name an `#embed` names, how it was spelled, and how many
    /// of the directive's tokens it took.
    fn embed_operand(
        &mut self,
        rest: &[PTok],
        range: SourceRange,
    ) -> Option<(String, include::Form, usize)> {
        if let Some(found) = self.embed_name_of(rest) {
            return Some(found);
        }
        // 6.10.3p1 allows the whole operand to come out of a macro, exactly as
        // `#include`'s does.
        if !rest.is_empty() {
            let expanded = self.expand_sequence(rest.to_vec());
            if let Some((name, form, _)) = self.embed_name_of(&expanded) {
                // A macro cannot be followed by parameters here: the whole run
                // was replaced, so there is nothing of the original left to
                // read them from.
                return Some((name, form, rest.len()));
            }
        }
        self.diags
            .error(range, "#embed expects \"RESOURCE\" or <RESOURCE>");
        None
    }

    /// Reads a resource name off the front of a token run.
    fn embed_name_of(&self, toks: &[PTok]) -> Option<(String, include::Form, usize)> {
        match &toks.first()?.kind {
            // A quoted name is *not* a string literal's value: no escape
            // sequence is processed, so the spelling between the quotes is it.
            TokenKind::Str(lit) if lit.kind == lex::StrKind::Narrow => {
                let spelling = lit.text.as_str();
                let name = spelling
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(spelling);
                (!name.is_empty()).then(|| (name.to_owned(), include::Form::Quoted, 1))
            }
            TokenKind::Punct(Punct::Lt) => {
                let close = toks[1..].iter().position(|t| t.is_punct(Punct::Gt))? + 1;
                let raw = self
                    .raw_text(toks[0].range.end, toks[close].range.start)
                    .trim()
                    .to_owned();
                let name = if raw.is_empty() {
                    // The tokens came out of a macro and have no source text
                    // of their own; their spellings are the name.
                    let mut spelled = String::new();
                    for (i, tok) in toks[1..close].iter().enumerate() {
                        if i > 0 && tok.space {
                            spelled.push(' ');
                        }
                        spelled.push_str(tok.spelling());
                    }
                    spelled
                } else {
                    raw
                };
                (!name.is_empty()).then(|| (name, include::Form::Angled, close + 1))
            }
            _ => None,
        }
    }

    /// Reads `#embed`'s parameters, which follow the resource name.
    ///
    /// `report` says whether a parameter this implementation does not have is
    /// a diagnostic. It is for the *directive*, and is not for `__has_embed`:
    /// 6.10.1p5 answers "not found" for a parameter it cannot honour, which is
    /// how a program asks whether one is supported before writing it.
    fn embed_parameters(
        &mut self,
        mut rest: &[PTok],
        range: SourceRange,
        report: bool,
    ) -> Option<EmbedParams> {
        let mut params = EmbedParams::default();
        while let Some(first) = rest.first() {
            let Some(name) = first.name() else {
                if report {
                    self.diags.error(
                        first.range,
                        format!(
                            "expected an #embed parameter, found {}",
                            first.kind.describe()
                        ),
                    );
                }
                return None;
            };
            if rest.get(1).is_none_or(|t| !t.is_punct(Punct::LParen)) {
                if report {
                    self.diags.error(
                        first.range,
                        format!("#embed parameter '{name}' takes an argument list"),
                    );
                }
                return None;
            }
            // The matching `)`, counting nested parentheses.
            let mut depth = 0usize;
            let mut close = None;
            for (i, tok) in rest[1..].iter().enumerate() {
                if tok.is_punct(Punct::LParen) {
                    depth += 1;
                } else if tok.is_punct(Punct::RParen) {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i + 1);
                        break;
                    }
                }
            }
            let Some(close) = close else {
                if report {
                    self.diags.error(
                        first.range,
                        format!("unterminated argument list for '{name}'"),
                    );
                }
                return None;
            };
            let inner = &rest[2..close];
            // GCC spells every one of them both ways; the reserved form is
            // what a header uses so as not to collide with a user's macro.
            let plain = name
                .strip_prefix("__")
                .and_then(|n| n.strip_suffix("__"))
                .unwrap_or(name);
            match plain {
                "limit" => {
                    if inner.is_empty() {
                        if report {
                            self.diags
                                .error(first.range, "#embed 'limit' takes a constant expression");
                        }
                        return None;
                    }
                    let value = self.eval_expression(inner, range)?;
                    if value < 0 {
                        if report {
                            self.diags
                                .error(first.range, "#embed 'limit' cannot be negative");
                        }
                        return None;
                    }
                    params.limit = Some(usize::try_from(value).unwrap_or(usize::MAX));
                }
                "prefix" => params.prefix = inner.to_vec(),
                "suffix" => params.suffix = inner.to_vec(),
                "if_empty" => params.if_empty = inner.to_vec(),
                _ => {
                    if report {
                        self.diags
                            .error(first.range, format!("unknown #embed parameter '{name}'"));
                    }
                    return None;
                }
            }
            rest = &rest[close + 1..];
        }
        Some(params)
    }

    /// One token of an `#embed` expansion, standing where the directive was.
    fn embed_token(&self, kind: TokenKind, range: SourceRange) -> PTok {
        PTok {
            kind,
            range,
            bol: false,
            space: true,
            origin: Origin::Source,
            hide: HideSet::default(),
            errors: Vec::new(),
        }
    }

    /// Places a header's text in the offset space and starts reading it.
    fn open_file(&mut self, found: include::Resolved, directive: SourceRange) {
        let base = self.next_base;
        self.next_base = base
            .saturating_add(found.text.len() as Pos)
            .saturating_add(FILE_GAP);
        let mut input: Vec<PTok> = lex::lex_text(&found.text, base, &self.lex_options)
            .iter()
            .map(PTok::from_lexed)
            .collect();
        if input.is_empty() {
            input.push(eof_token(base));
        }
        if let Some(guard) = detect_include_guard(&input) {
            self.guards.insert(found.key.clone(), guard);
        }
        self.included.push(IncludedFile {
            name: found.name.clone(),
            text: found.text.clone(),
            base,
            directive,
        });
        self.files
            .push(FileEntry::new(found.text, base, 1, found.name));
        self.open.push(OpenFile {
            input,
            pos: 0,
            origin: found.origin,
            key: found.key,
            cond_base: self.conds.len(),
        });
    }

    /// The header name an `#include` names, and how it was spelled.
    ///
    /// `<stdio.h>` is not one token, so the name comes from the source text
    /// the directive covers rather than from the tokens. 6.10.2p4 also allows
    /// the whole thing to come out of a macro, which is what the second half
    /// handles: there is no source text to read then, so the name is rebuilt
    /// from the spellings of the tokens the macro produced.
    fn header_name(
        &mut self,
        line: &[PTok],
        range: SourceRange,
    ) -> Option<(String, include::Form)> {
        let text = self.directive_text(line, 1);
        if let Some(found) = parse_header_name(&text) {
            return Some(found);
        }
        if line.len() <= 1 {
            self.diags
                .error(range, "#include expects \"FILENAME\" or <FILENAME>");
            return None;
        }
        // `#include MACRO`: replace, then read the result back as text.
        let expanded = self.expand_sequence(line[1..].to_vec());
        let mut spelled = String::new();
        for (i, tok) in expanded.iter().enumerate() {
            if i > 0 && tok.space {
                spelled.push(' ');
            }
            spelled.push_str(tok.spelling());
        }
        if let Some(found) = parse_header_name(&spelled) {
            return Some(found);
        }
        self.diags.error(
            range,
            "#include expects \"FILENAME\" or <FILENAME>".to_owned(),
        );
        None
    }

    /// Reads the single macro name a directive takes.
    fn macro_name_operand(
        &mut self,
        rest: &[PTok],
        range: SourceRange,
        directive: &str,
    ) -> Option<String> {
        let Some(first) = rest.first() else {
            self.diags
                .error(range, format!("no macro name given in #{directive}"));
            return None;
        };
        let Some(name) = first.name() else {
            self.diags.error(
                first.range,
                format!(
                    "macro name must be an identifier, found {}",
                    first.kind.describe()
                ),
            );
            return None;
        };
        if name == "defined" {
            self.diags.error(
                first.range,
                format!("'defined' cannot be used as a macro name in #{directive}"),
            );
            return None;
        }
        Some(name.to_owned())
    }

    // -- conditionals -------------------------------------------------------

    /// Opens a conditional group, evaluating its controlling condition only
    /// when the enclosing group is being processed.
    fn open_cond(&mut self, range: SourceRange, test: impl FnOnce(&mut Self) -> bool) {
        let outer_active = !self.skipping();
        let value = outer_active && test(self);
        self.conds.push(Cond {
            range,
            outer_active,
            taken: value,
            active: outer_active && value,
            seen_else: false,
        });
    }

    /// Opens the next branch of a conditional group.
    ///
    /// `test` is only run when the branch could be taken at all, which is what
    /// makes `#elif 1/N` after a branch that already ran harmless — and what
    /// keeps `#elifdef` from reporting a missing name in a group nothing will
    /// read.
    fn elif(&mut self, range: SourceRange, directive: &str, test: impl FnOnce(&mut Self) -> bool) {
        let Some(cond) = self.conds.last() else {
            self.diags.error(range, format!("#{directive} without #if"));
            return;
        };
        if cond.seen_else {
            let seen = cond.range;
            self.diags.push(
                Diagnostic::error(range, format!("#{directive} after #else"))
                    .with_note_at(seen, "the conditional started"),
            );
            return;
        }
        let (outer_active, taken) = (cond.outer_active, cond.taken);
        let value = outer_active && !taken && test(self);
        let cond = self
            .conds
            .last_mut()
            .expect("the stack was not touched in between");
        cond.active = value;
        cond.taken |= value;
    }

    /// Reports a directive the block's own standard does not have.
    fn require_standard(&mut self, needed: Standard, what: &str, range: SourceRange) {
        if let Some(message) = self.gating.requires(what, needed) {
            self.diags.error(range, message);
        }
    }

    fn else_(&mut self, rest: &[PTok], range: SourceRange) {
        let Some(cond) = self.conds.last_mut() else {
            self.diags.error(range, "#else without #if");
            return;
        };
        if cond.seen_else {
            let seen = cond.range;
            self.diags.push(
                Diagnostic::error(range, "#else after #else")
                    .with_note_at(seen, "the conditional started"),
            );
            return;
        }
        cond.seen_else = true;
        cond.active = cond.outer_active && !cond.taken;
        cond.taken = true;
        let active = cond.active;
        if active && !rest.is_empty() {
            self.diags
                .warning(range, "extra tokens at the end of #else");
        }
    }

    fn endif(&mut self, range: SourceRange) {
        if self.conds.pop().is_none() {
            self.diags.error(range, "#endif without #if");
        }
    }

    // -- #define / #undef ---------------------------------------------------

    fn undef(&mut self, rest: &[PTok], range: SourceRange) {
        if let Some(name) = self.macro_name_operand(rest, range, "undef") {
            self.macros.remove(&name);
        }
    }

    fn define(&mut self, rest: &[PTok], range: SourceRange) {
        let Some(name_tok) = rest.first() else {
            self.diags.error(range, "no macro name given in #define");
            return;
        };
        let Some(name) = name_tok.name().map(str::to_owned) else {
            self.diags.error(
                name_tok.range,
                format!(
                    "macro name must be an identifier, found {}",
                    name_tok.kind.describe()
                ),
            );
            return;
        };
        if name == "defined" {
            self.diags
                .error(name_tok.range, "'defined' cannot be used as a macro name");
            return;
        }
        if name == VA_ARGS {
            self.diags.error(
                name_tok.range,
                "'__VA_ARGS__' can only appear in the replacement list of a variadic macro",
            );
            return;
        }

        // A `(` *immediately* after the name — no white space — makes the
        // macro function-like; `#define f (x)` is object-like and expands to
        // `(x)`.
        let mut rest = &rest[1..];
        let function_like = rest
            .first()
            .is_some_and(|t| t.is_punct(Punct::LParen) && !t.space);
        let (params, variadic, va_name) = if function_like {
            let Some(parsed) = self.parse_params(rest, range) else {
                return;
            };
            rest = &rest[parsed.used..];
            (Some(parsed.params), parsed.variadic, parsed.va_name)
        } else {
            (None, false, None)
        };

        let body = fuse_hash_hash(rest);
        let def = MacroDef {
            params,
            variadic,
            va_name,
            body,
            name_range: name_tok.range,
            predefined: false,
            builtin: None,
        };
        if !self.check_body(&def, &name, range) {
            return;
        }

        if let Some(previous) = self.macros.get(&name)
            && !previous.predefined
            && !previous.same_as(&def)
        {
            self.diags.push(
                Diagnostic::error(name_tok.range, format!("macro '{name}' redefined"))
                    .with_note_at(
                        previous.name_range,
                        format!("previous definition of '{name}' is"),
                    ),
            );
            return;
        }
        self.macros.insert(name, Arc::new(def));
    }

    /// Parses `( a, b, ... )`, or GNU's `( a, rest... )`.
    fn parse_params(&mut self, rest: &[PTok], range: SourceRange) -> Option<ParamList> {
        let mut params: Vec<String> = Vec::new();
        let mut variadic = false;
        let mut va_name = None;
        let mut i = 1; // past the `(`
        if rest.get(i).is_some_and(|t| t.is_punct(Punct::RParen)) {
            return Some(ParamList {
                params,
                variadic,
                va_name,
                used: i + 1,
            });
        }
        loop {
            let Some(tok) = rest.get(i) else {
                self.diags
                    .error(range, "missing ')' in the parameter list of a macro");
                return None;
            };
            if tok.is_punct(Punct::Ellipsis) {
                self.require_standard(Standard::C99, "a variadic macro", tok.range);
                variadic = true;
                i += 1;
                break;
            }
            let Some(name) = tok.name() else {
                self.diags.error(
                    tok.range,
                    format!(
                        "expected a macro parameter name, found {}",
                        tok.kind.describe()
                    ),
                );
                return None;
            };
            if name == VA_ARGS {
                self.diags.error(
                    tok.range,
                    "'__VA_ARGS__' cannot be used as a macro parameter name",
                );
                return None;
            }
            if params.iter().any(|p| p == name) {
                self.diags
                    .error(tok.range, format!("duplicate macro parameter '{name}'"));
                return None;
            }
            // GNU's named variable arguments: `args...` makes `args` another
            // spelling of `__VA_ARGS__` rather than one more parameter.
            if rest.get(i + 1).is_some_and(|t| t.is_punct(Punct::Ellipsis)) {
                self.require_standard(Standard::C99, "a variadic macro", tok.range);
                variadic = true;
                va_name = Some(name.to_owned());
                i += 2;
                break;
            }
            params.push(name.to_owned());
            i += 1;
            match rest.get(i) {
                Some(t) if t.is_punct(Punct::Comma) => i += 1,
                Some(t) if t.is_punct(Punct::RParen) => break,
                Some(t) => {
                    self.diags.error(
                        t.range,
                        format!(
                            "expected ',' or ')' in a macro parameter list, found {}",
                            t.kind.describe()
                        ),
                    );
                    return None;
                }
                None => {
                    self.diags
                        .error(range, "missing ')' in the parameter list of a macro");
                    return None;
                }
            }
        }
        match rest.get(i) {
            Some(t) if t.is_punct(Punct::RParen) => Some(ParamList {
                params,
                variadic,
                va_name,
                used: i + 1,
            }),
            _ => {
                self.diags
                    .error(range, "missing ')' in the parameter list of a macro");
                None
            }
        }
    }

    /// Checks the constraints a replacement list has to satisfy.
    fn check_body(&mut self, def: &MacroDef, name: &str, range: SourceRange) -> bool {
        let body = &def.body;
        if let Some(first) = body.first()
            && first.is_punct(Punct::HashHash)
        {
            self.diags.error(
                first.range,
                "'##' cannot appear at the start of a macro replacement list",
            );
            return false;
        }
        if let Some(last) = body.last()
            && last.is_punct(Punct::HashHash)
            && body.len() > 1
        {
            self.diags.error(
                last.range,
                "'##' cannot appear at the end of a macro replacement list",
            );
            return false;
        }
        for (i, tok) in body.iter().enumerate() {
            if def.params.is_some() && tok.is_punct(Punct::Hash) {
                let ok = body
                    .get(i + 1)
                    .and_then(PTok::name)
                    .is_some_and(|n| def.param_index(n).is_some());
                if !ok {
                    self.diags
                        .error(tok.range, "'#' must be followed by a macro parameter");
                    return false;
                }
            }
            if tok.name() == Some(VA_ARGS) && def.param_index(VA_ARGS).is_none() {
                self.diags.error(
                    tok.range,
                    "'__VA_ARGS__' can only appear in the replacement list of a variadic macro",
                );
                return false;
            }
            if tok.name() == Some(VA_OPT) && !self.check_va_opt(def, body, i) {
                return false;
            }
        }
        let _ = (name, range);
        true
    }

    /// Checks one `__VA_OPT__` in a replacement list.
    ///
    /// It has to be in a variadic macro, it has to be followed by a balanced
    /// `( … )`, its contents may neither begin nor end with `##` (C23
    /// 6.10.5.2p1, for the same reason a replacement list may not — there is
    /// nothing on that side to paste to), and — since the standard says so and
    /// since the expansion here is a single pass — it may not hold another
    /// one.
    fn check_va_opt(&mut self, def: &MacroDef, body: &[PTok], at: usize) -> bool {
        let tok = &body[at];
        if def.param_index(VA_ARGS).is_none() {
            self.diags.error(
                tok.range,
                "'__VA_OPT__' can only appear in the replacement list of a variadic macro",
            );
            return false;
        }
        self.require_standard(Standard::C23, "'__VA_OPT__'", tok.range);
        if !body.get(at + 1).is_some_and(|t| t.is_punct(Punct::LParen)) {
            self.diags
                .error(tok.range, "'__VA_OPT__' must be followed by '('");
            return false;
        }
        let mut depth = 0i32;
        let mut contents: Vec<&PTok> = Vec::new();
        for tok in &body[at + 1..] {
            if tok.is_punct(Punct::LParen) {
                depth += 1;
                // The `(` that opens the argument is not part of it.
                if depth == 1 {
                    continue;
                }
            } else if tok.is_punct(Punct::RParen) {
                depth -= 1;
                if depth == 0 {
                    return self.check_va_opt_contents(&contents);
                }
            } else if tok.name() == Some(VA_OPT) {
                self.diags
                    .error(tok.range, "'__VA_OPT__' cannot be nested inside another");
                return false;
            }
            contents.push(tok);
        }
        self.diags.error(
            tok.range,
            "unterminated '__VA_OPT__(' in a macro definition",
        );
        false
    }

    /// C23 6.10.5.2p1 for the token sequence inside a `__VA_OPT__( … )`.
    fn check_va_opt_contents(&mut self, contents: &[&PTok]) -> bool {
        for (end, tok) in [("start", contents.first()), ("end", contents.last())] {
            if let Some(tok) = tok
                && tok.is_punct(Punct::HashHash)
            {
                self.diags.error(
                    tok.range,
                    format!("'##' cannot appear at the {end} of a '__VA_OPT__' argument"),
                );
                return false;
            }
        }
        true
    }
}

/// Replaces every `__VA_OPT__( … )` in a replacement list with its contents,
/// or with nothing when the invocation passed no variable arguments (C23
/// 6.10.5.2).
///
/// Doing it before substitution rather than during it is what makes the rest
/// of the rules fall out: the contents are ordinary replacement-list tokens
/// afterwards, so `#` and `##` next to them, and the parameters inside them,
/// are handled by the code that was already there. `None` means the
/// replacement list has no `__VA_OPT__` and can be used as it stands.
fn expand_va_opt(def: &MacroDef, args: &Args) -> Option<Vec<PTok>> {
    let params = def.params.as_ref()?;
    if !def.variadic || !def.body.iter().any(|t| t.name() == Some(VA_OPT)) {
        return None;
    }
    // The variable arguments are the one past the named parameters; `subst`
    // is only reached once `try_expand` has padded the list out to that.
    let present = !args.get(params.len()).is_empty();
    let body = &def.body;
    let mut out: Vec<PTok> = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        let is_va_opt = body[i].name() == Some(VA_OPT)
            && body.get(i + 1).is_some_and(|t| t.is_punct(Punct::LParen));
        if !is_va_opt {
            out.push(body[i].clone());
            i += 1;
            continue;
        }
        let space = body[i].space;
        let mut depth = 0i32;
        let mut inner: Vec<PTok> = Vec::new();
        let mut j = i + 1;
        while j < body.len() {
            let tok = &body[j];
            j += 1;
            if tok.is_punct(Punct::LParen) {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            } else if tok.is_punct(Punct::RParen) {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            inner.push(tok.clone());
        }
        if present {
            if let Some(first) = inner.first_mut() {
                // The expansion stands where `__VA_OPT__` did, spacing and all.
                first.space = space;
            }
            out.extend(inner);
        }
        i = j;
    }
    Some(out)
}

/// Reads `<name>` or `"name"` out of the text of an `#include` directive.
///
/// The name is taken verbatim, which is what 6.4.7 asks for: a `\` in a header
/// name is a directory separator on the platforms that use one, not the start
/// of an escape sequence. Anything after the closing delimiter is ignored, the
/// way every compiler ignores it.
fn parse_header_name(text: &str) -> Option<(String, include::Form)> {
    let text = text.trim();
    let (form, close) = match text.as_bytes().first()? {
        b'<' => (include::Form::Angled, '>'),
        b'"' => (include::Form::Quoted, '"'),
        _ => return None,
    };
    let rest = &text[1..];
    let end = rest.find(close)?;
    let name = &rest[..end];
    (!name.is_empty()).then(|| (name.to_owned(), form))
}

/// The macro an include guard is built on, if a file is nothing but one.
///
/// The classic optimisation (6.10.2, and every compiler since 1987): a file
/// whose whole contents are
///
/// ```c
/// #ifndef GUARD
/// #define GUARD
/// …
/// #endif
/// ```
///
/// need not be read again while `GUARD` is defined, because reading it would
/// produce nothing. Recognising it is what keeps a header included from twenty
/// places from being lexed twenty times — and, here, from taking twenty copies
/// of its text into the source map.
fn detect_include_guard(toks: &[PTok]) -> Option<String> {
    // `#ifndef NAME`, with nothing else on the line.
    if !(toks.first()?.bol && toks[0].is_punct(Punct::Hash)) {
        return None;
    }
    if toks.get(1)?.name()? != "ifndef" {
        return None;
    }
    let name = toks.get(2)?.name()?.to_owned();
    let after_ifndef = toks.get(3)?;
    if !after_ifndef.bol && !after_ifndef.is_eof() {
        return None;
    }
    // `#define NAME` immediately after it.
    if !after_ifndef.is_punct(Punct::Hash)
        || toks.get(4)?.name()? != "define"
        || toks.get(5)?.name()? != name
    {
        return None;
    }

    // The `#endif` that closes it must be the last thing in the file.
    let mut depth = 0i32;
    let mut i = 0;
    while i < toks.len() && !toks[i].is_eof() {
        if toks[i].bol && toks[i].is_punct(Punct::Hash) {
            match toks.get(i + 1).and_then(PTok::name) {
                Some("if" | "ifdef" | "ifndef") => depth += 1,
                Some("endif") => {
                    depth -= 1;
                    if depth == 0 {
                        let mut j = i + 2;
                        while toks.get(j).is_some_and(|t| !t.bol && !t.is_eof()) {
                            j += 1;
                        }
                        return toks.get(j).is_none_or(PTok::is_eof).then_some(name);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Applies the [`# #` rule](self#the----rule) to a replacement list.
fn fuse_hash_hash(rest: &[PTok]) -> Vec<PTok> {
    let mut body: Vec<PTok> = Vec::with_capacity(rest.len());
    let mut i = 0;
    while i < rest.len() {
        if rest[i].is_punct(Punct::Hash)
            && let Some(next) = rest.get(i + 1)
            && next.is_punct(Punct::Hash)
            && !next.bol
        {
            let mut fused = rest[i].clone();
            fused.kind = TokenKind::Punct(Punct::HashHash);
            fused.range = fused.range.join(next.range);
            body.push(fused);
            i += 2;
            continue;
        }
        body.push(rest[i].clone());
        i += 1;
    }
    if let Some(first) = body.first_mut() {
        // Leading white space is not part of a replacement list, and 6.10.3p2
        // compares two definitions on where their white space is.
        first.space = false;
    }
    body
}

// ---------------------------------------------------------------------------
// #if expressions
// ---------------------------------------------------------------------------

/// A value in an `#if` expression: `intmax_t` or `uintmax_t`, which C99 6.10.1
/// fixes as the only two types such an expression has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Val {
    /// The value, held signed but normalised to whichever type `unsigned` says.
    v: i128,
    unsigned: bool,
}

impl Val {
    fn signed(v: i128) -> Val {
        Val {
            v: v as i64 as i128,
            unsigned: false,
        }
    }

    fn make(v: i128, unsigned: bool) -> Val {
        if unsigned {
            Val {
                v: (v as u64) as i128,
                unsigned,
            }
        } else {
            Val::signed(v)
        }
    }

    fn boolean(b: bool) -> Val {
        Val::signed(i128::from(b))
    }

    fn is_true(self) -> bool {
        self.v != 0
    }

    /// The value as it takes part in an operation of the given signedness.
    fn as_operand(self, unsigned: bool) -> i128 {
        if unsigned && self.v < 0 {
            self.v + (1i128 << 64)
        } else {
            self.v
        }
    }
}

impl Pp<'_> {
    /// Evaluates the controlling expression of an `#if` or `#elif`.
    fn eval_condition(&mut self, line: &[PTok], range: SourceRange) -> bool {
        // An `#if` may read `__SIZEOF_LONG__` or `_WIN32`, so from here on the
        // data model has been committed to; see `Pp::target_pragma`.
        self.model_observed = true;
        if line.is_empty() {
            self.diags.error(range, "#if with no expression");
            return false;
        }
        self.eval_expression(line, range)
            .is_some_and(|value| value != 0)
    }

    /// Evaluates a constant expression with the preprocessor's own arithmetic.
    ///
    /// `None` says something was wrong and has been reported. This is what
    /// `#if` asks a question of, and what `#embed`'s `limit(…)` parameter is.
    fn eval_expression(&mut self, line: &[PTok], range: SourceRange) -> Option<i128> {
        let prepared = self.resolve_defined(line, range)?;
        let expanded = self.expand_sequence(prepared);
        for tok in &expanded {
            let tok = tok.clone();
            self.report_errors(&tok);
        }
        let mut eval = Eval {
            toks: &expanded,
            pos: 0,
            fallback: range,
            errors: Vec::new(),
            depth: 0,
        };
        let value = eval.expression(true);
        if eval.errors.is_empty()
            && eval.pos < eval.toks.len()
            && let Some(tok) = eval.toks.get(eval.pos)
        {
            let found = tok.kind.describe();
            eval.errors.push(Diagnostic::error(
                tok.range,
                format!("unexpected {found} in a preprocessor expression"),
            ));
        }
        let failed = !eval.errors.is_empty();
        for diag in eval.errors {
            self.diags.push(diag);
        }
        (!failed).then_some(value.v)
    }

    /// Replaces every `defined X` and `defined(X)` with `1` or `0`.
    ///
    /// This happens before macro replacement, as 6.10.1p1 requires: the
    /// operand of `defined` is a name, not something a macro may rewrite.
    fn resolve_defined(&mut self, line: &[PTok], range: SourceRange) -> Option<Vec<PTok>> {
        let mut out = Vec::with_capacity(line.len());
        let mut i = 0;
        while i < line.len() {
            let tok = &line[i];
            // The `__has_…` family is answered here too, for the same reason
            // `defined` is: their operands are names and header names, not
            // things a macro may rewrite.
            if let Some(name) = tok.name()
                && name.starts_with("__has_")
            {
                let (value, end) = self.has_operator(name, line, i)?;
                let mut answer = tok.clone();
                answer.kind = int_token_kind(value);
                answer.hide = answer.hide.add(name);
                answer.errors.clear();
                out.push(answer);
                i = end;
                continue;
            }
            if tok.name() != Some("defined") {
                out.push(tok.clone());
                i += 1;
                continue;
            }
            let parenthesised = line.get(i + 1).is_some_and(|t| t.is_punct(Punct::LParen));
            let name_at = if parenthesised { i + 2 } else { i + 1 };
            let Some(name) = line.get(name_at).and_then(PTok::name) else {
                self.diags.error(
                    tok.range,
                    "operator 'defined' requires an identifier as its operand",
                );
                return None;
            };
            let defined = self.macros.contains_key(name);
            let mut end = name_at + 1;
            if parenthesised {
                if !line.get(end).is_some_and(|t| t.is_punct(Punct::RParen)) {
                    self.diags.error(range, "missing ')' after 'defined'");
                    return None;
                }
                end += 1;
            }
            let mut value = tok.clone();
            value.kind = TokenKind::Int(IntLit {
                value: u128::from(defined),
                base: NumBase::Decimal,
                unsigned: false,
                long: LongKind::None,
                text: u8::from(defined).to_string(),
            });
            // Already answered: nothing here may be replaced again.
            value.hide = value.hide.add("defined");
            value.errors.clear();
            out.push(value);
            i = end;
        }
        Some(out)
    }

    /// Answers one `__has_…(…)` operator, returning its value and the index
    /// just past its closing `)`.
    ///
    /// Everything here is answered from `cinrs`'s own tables (see
    /// [`crate::gnu`]) rather than from GCC's, which is the point: a program
    /// that writes `#if __has_attribute(cleanup)` must be told *no*, because
    /// this implementation does not have it.
    fn has_operator(&mut self, name: &str, line: &[PTok], at: usize) -> Option<(u128, usize)> {
        let range = line[at].range;
        if !line.get(at + 1).is_some_and(|t| t.is_punct(Punct::LParen)) {
            // An identifier that is not an invocation is an ordinary one, and
            // 6.10.1p4 turns it into 0 like any other.
            return Some((0, at + 1));
        }
        let mut depth = 0i32;
        let mut end = at + 1;
        while end < line.len() {
            if line[end].is_punct(Punct::LParen) {
                depth += 1;
            } else if line[end].is_punct(Punct::RParen) {
                depth -= 1;
                if depth == 0 {
                    end += 1;
                    break;
                }
            }
            end += 1;
        }
        if depth != 0 {
            self.diags
                .error(range, format!("missing ')' after '{name}'"));
            return None;
        }
        let inner = &line[at + 2..end - 1];
        let value = match name {
            // The header name is spelled as it is in an `#include`, so it is
            // read out of the source text rather than out of the tokens.
            "__has_include" | "__has_include_next" => {
                let Some((header, form)) = self.operand_header_name(inner) else {
                    self.diags.error(
                        range,
                        format!("'{name}' expects \"FILENAME\" or <FILENAME>"),
                    );
                    return None;
                };
                // `__has_include_next` looks past the file the directive is
                // written in, which for us is the angled search alone.
                let form = if name == "__has_include_next" {
                    include::Form::Angled
                } else {
                    form
                };
                let origin = self.cur().origin.clone();
                u128::from(include::resolve(&header, form, &origin, &self.search).is_ok())
            }
            "__has_attribute" | "__has_declspec_attribute" => u128::from(
                inner
                    .first()
                    .and_then(PTok::name)
                    .is_some_and(crate::gnu::has_attribute),
            ),
            "__has_c_attribute" => inner
                .first()
                .and_then(PTok::name)
                .map_or(0, |n| u128::from(crate::gnu::has_c_attribute(n))),
            "__has_builtin" => u128::from(
                inner
                    .first()
                    .and_then(PTok::name)
                    .is_some_and(crate::gnu::has_builtin),
            ),
            "__has_feature" | "__has_extension" => u128::from(
                inner
                    .first()
                    .and_then(PTok::name)
                    .is_some_and(crate::gnu::has_feature),
            ),
            // C23 6.10.1p5: not found, found and empty, spelled with the same
            // three macros `<stdembed.h>` would have. Parameters after the
            // resource name are read so that an unknown one answers "not
            // found", which is what the clause asks for.
            "__has_embed" => {
                let Some((resource, form, after)) = self.embed_name_of(inner) else {
                    self.diags.error(
                        range,
                        format!("'{name}' expects \"RESOURCE\" or <RESOURCE>"),
                    );
                    return None;
                };
                let params = self.embed_parameters(&inner[after..], range, false);
                let origin = self.cur().origin.clone();
                match (
                    params,
                    include::resolve_embed(&resource, form, &origin, &self.search),
                ) {
                    (Some(params), Ok(found)) => {
                        let take = params.limit.unwrap_or(found.bytes.len());
                        if take == 0 || found.bytes.is_empty() {
                            EMBED_EMPTY
                        } else {
                            EMBED_FOUND
                        }
                    }
                    _ => EMBED_NOT_FOUND,
                }
            }
            _ => 0,
        };
        Some((value, end))
    }

    /// The header name written inside `__has_include(…)`.
    fn operand_header_name(&mut self, inner: &[PTok]) -> Option<(String, include::Form)> {
        let first = inner.first()?;
        let last = inner.last()?;
        let text = self.raw_text(first.range.start, last.range.end).trim();
        if let Some(found) = parse_header_name(text) {
            return Some(found);
        }
        // The name came out of a macro, so there is no source text to read: it
        // is rebuilt from the spellings, exactly as `#include MACRO` is.
        let expanded = self.expand_sequence(inner.to_vec());
        let mut spelled = String::new();
        for (i, tok) in expanded.iter().enumerate() {
            if i > 0 && tok.space {
                spelled.push(' ');
            }
            spelled.push_str(tok.spelling());
        }
        parse_header_name(&spelled)
    }
}

/// A recursive-descent evaluator over preprocessing tokens.
///
/// Separate from the parser's constant evaluator on purpose: this one works on
/// tokens rather than on an AST, has no types beyond `intmax_t`/`uintmax_t`,
/// turns every leftover identifier into `0`, and must not evaluate the
/// unreached arm of `&&`, `||` or `?:` — `#if defined(N) && 10/N` is a
/// perfectly ordinary thing to write.
struct Eval<'a> {
    toks: &'a [PTok],
    pos: usize,
    /// Where to report something that has no token of its own.
    fallback: SourceRange,
    errors: Vec<Diagnostic>,
    /// How deep the parentheses and `?:` are, so that `#if ((((…))))` becomes
    /// a diagnostic rather than a stack overflow inside a compiler.
    depth: u32,
}

/// How deeply an `#if` expression may nest; the parser's own limit, for the
/// same reason.
const MAX_EVAL_DEPTH: u32 = 200;

impl Eval<'_> {
    fn peek(&self) -> Option<&PTok> {
        self.toks.get(self.pos)
    }

    fn at(&self, p: Punct) -> bool {
        self.peek().is_some_and(|t| t.is_punct(p))
    }

    fn eat(&mut self, p: Punct) -> bool {
        if self.at(p) {
            self.pos += 1;
            return true;
        }
        false
    }

    fn range(&self) -> SourceRange {
        self.peek().map_or(self.fallback, |t| t.range)
    }

    fn error(&mut self, range: SourceRange, message: impl Into<String>) {
        self.errors.push(Diagnostic::error(range, message));
    }

    /// `expr , expr` — the comma operator, which `#if` does allow.
    fn expression(&mut self, eval: bool) -> Val {
        self.depth += 1;
        if self.depth > MAX_EVAL_DEPTH {
            let range = self.range();
            self.error(range, "this preprocessor expression nests too deeply");
            // Consume the rest so that the caller's loops all terminate.
            self.pos = self.toks.len();
            self.depth -= 1;
            return Val::signed(0);
        }
        let mut value = self.conditional(eval);
        while self.eat(Punct::Comma) {
            value = self.conditional(eval);
        }
        self.depth -= 1;
        value
    }

    fn conditional(&mut self, eval: bool) -> Val {
        let cond = self.binary(0, eval);
        if !self.eat(Punct::Question) {
            return cond;
        }
        let take_then = cond.is_true();
        let then_value = self.expression(eval && take_then);
        if !self.eat(Punct::Colon) {
            let range = self.range();
            self.error(range, "expected ':' in a preprocessor expression");
            return cond;
        }
        let else_value = self.conditional(eval && !take_then);
        let (a, b) = (then_value, else_value);
        let unsigned = a.unsigned || b.unsigned;
        let picked = if take_then { a } else { b };
        Val::make(picked.as_operand(unsigned), unsigned)
    }

    /// Precedence climbing over the binary operators.
    fn binary(&mut self, min_prec: u8, eval: bool) -> Val {
        let mut lhs = self.unary(eval);
        loop {
            let Some((op, prec)) = self.peek().and_then(|t| binary_op(&t.kind)) else {
                return lhs;
            };
            if prec < min_prec {
                return lhs;
            }
            let op_range = self.range();
            self.pos += 1;
            // `&&` and `||` do not evaluate their right operand when the left
            // one already decides the answer.
            let rhs_eval = match op {
                BinOp::LogAnd => eval && lhs.is_true(),
                BinOp::LogOr => eval && !lhs.is_true(),
                _ => eval,
            };
            let rhs = self.binary(prec + 1, rhs_eval);
            lhs = self.apply(op, lhs, rhs, op_range, eval);
        }
    }

    fn apply(&mut self, op: BinOp, a: Val, b: Val, range: SourceRange, eval: bool) -> Val {
        use BinOp::*;
        if op == LogAnd {
            return Val::boolean(a.is_true() && b.is_true());
        }
        if op == LogOr {
            return Val::boolean(a.is_true() || b.is_true());
        }
        // The usual arithmetic conversions, in the only shape they have here:
        // if either operand is unsigned, both are.
        let unsigned = a.unsigned || b.unsigned;
        let (x, y) = (a.as_operand(unsigned), b.as_operand(unsigned));
        match op {
            Eq => Val::boolean(x == y),
            Ne => Val::boolean(x != y),
            Lt => Val::boolean(x < y),
            Gt => Val::boolean(x > y),
            Le => Val::boolean(x <= y),
            Ge => Val::boolean(x >= y),
            Add => Val::make(x.wrapping_add(y), unsigned),
            Sub => Val::make(x.wrapping_sub(y), unsigned),
            Mul => Val::make(x.wrapping_mul(y), unsigned),
            Div | Rem => {
                if y == 0 {
                    if eval {
                        self.error(range, "division by zero in a preprocessor expression");
                    }
                    return Val::make(0, unsigned);
                }
                let v = if op == Div {
                    x.wrapping_div(y)
                } else {
                    x.wrapping_rem(y)
                };
                Val::make(v, unsigned)
            }
            BitAnd => Val::make(x & y, unsigned),
            BitOr => Val::make(x | y, unsigned),
            BitXor => Val::make(x ^ y, unsigned),
            Shl => Val::make(x.wrapping_shl((y as u64 & 63) as u32), unsigned),
            Shr => {
                let count = (y as u64 & 63) as u32;
                if unsigned {
                    Val::make(((x as u64) >> count) as i128, unsigned)
                } else {
                    Val::make((x as i64 >> count) as i128, unsigned)
                }
            }
            LogAnd | LogOr => unreachable!("handled above"),
        }
    }

    fn unary(&mut self, eval: bool) -> Val {
        let range = self.range();
        if self.eat(Punct::Plus) {
            return self.unary(eval);
        }
        if self.eat(Punct::Minus) {
            let v = self.unary(eval);
            return Val::make(v.as_operand(v.unsigned).wrapping_neg(), v.unsigned);
        }
        if self.eat(Punct::Tilde) {
            let v = self.unary(eval);
            return Val::make(!v.as_operand(v.unsigned), v.unsigned);
        }
        if self.eat(Punct::Bang) {
            let v = self.unary(eval);
            return Val::boolean(!v.is_true());
        }
        if self.eat(Punct::LParen) {
            let v = self.expression(eval);
            if !self.eat(Punct::RParen) {
                let at = self.range();
                self.error(at, "expected ')' in a preprocessor expression");
            }
            return v;
        }
        self.primary(range)
    }

    fn primary(&mut self, range: SourceRange) -> Val {
        let Some(tok) = self.peek() else {
            self.error(range, "expected a value in a preprocessor expression");
            return Val::signed(0);
        };
        let value = match &tok.kind {
            TokenKind::Int(lit) => {
                // An `#if` has only `intmax_t` and `uintmax_t`; a constant is
                // unsigned when it says so or when it does not fit signed.
                let unsigned = lit.unsigned || lit.value > i64::MAX as u128;
                let too_large = lit.value > u64::MAX as u128;
                let value = Val::make((lit.value & u128::from(u64::MAX)) as i128, unsigned);
                if too_large {
                    let range = tok.range;
                    self.error(
                        range,
                        "integer constant is too large for a preprocessor expression",
                    );
                }
                value
            }
            TokenKind::Char(lit) => Val::signed(i128::from(lit.value)),
            TokenKind::Float(_) => {
                let range = tok.range;
                self.error(
                    range,
                    "a floating constant is not allowed in a preprocessor expression",
                );
                Val::signed(0)
            }
            TokenKind::Str(_) => {
                let range = tok.range;
                self.error(
                    range,
                    "a string literal is not allowed in a preprocessor expression",
                );
                Val::signed(0)
            }
            // C23 6.10.1p6: `true` and `false` are keywords there, and an
            // `#if` reads them as 1 and 0. Before C23 they are identifiers,
            // and the rule below turns them into 0 like any other.
            TokenKind::Keyword(lex::Keyword::True) => Val::signed(1),
            TokenKind::Keyword(lex::Keyword::False) => Val::signed(0),
            // 6.10.1p4: every identifier still standing after macro
            // replacement is replaced by 0.
            TokenKind::Ident(_) | TokenKind::Keyword(_) => Val::signed(0),
            other => {
                let range = tok.range;
                let found = other.describe();
                self.error(
                    range,
                    format!("unexpected {found} in a preprocessor expression"),
                );
                Val::signed(0)
            }
        };
        self.pos += 1;
        value
    }
}

/// The binary operators an `#if` expression may use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BinOp {
    LogOr,
    LogAnd,
    BitOr,
    BitXor,
    BitAnd,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Shl,
    Shr,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

/// The operator a token is, with its binding power (tightest last).
fn binary_op(kind: &TokenKind) -> Option<(BinOp, u8)> {
    let TokenKind::Punct(p) = kind else {
        return None;
    };
    Some(match p {
        Punct::PipePipe => (BinOp::LogOr, 1),
        Punct::AmpAmp => (BinOp::LogAnd, 2),
        Punct::Pipe => (BinOp::BitOr, 3),
        Punct::Caret => (BinOp::BitXor, 4),
        Punct::Amp => (BinOp::BitAnd, 5),
        Punct::EqEq => (BinOp::Eq, 6),
        Punct::Ne => (BinOp::Ne, 6),
        Punct::Lt => (BinOp::Lt, 7),
        Punct::Gt => (BinOp::Gt, 7),
        Punct::Le => (BinOp::Le, 7),
        Punct::Ge => (BinOp::Ge, 7),
        Punct::Shl => (BinOp::Shl, 8),
        Punct::Shr => (BinOp::Shr, 8),
        Punct::Plus => (BinOp::Add, 9),
        Punct::Minus => (BinOp::Sub, 9),
        Punct::Star => (BinOp::Mul, 10),
        Punct::Slash => (BinOp::Div, 10),
        Punct::Percent => (BinOp::Rem, 10),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// predefined macros
// ---------------------------------------------------------------------------

impl Standard {
    /// The value of `__STDC_VERSION__` for this revision, or `None` where the
    /// revision has none.
    ///
    /// C89 as published had no `__STDC_VERSION__` at all — Amendment 1 added
    /// it in 1995 — so `c89!` and `gnu89!` leave the macro undefined, which is
    /// what `gcc -std=c89` does and what a program testing
    /// `#ifdef __STDC_VERSION__` is looking for. `__STDC__` is still `1`.
    pub fn stdc_version(self) -> Option<&'static str> {
        Some(match self {
            Standard::C89 => return None,
            Standard::C99 => "199901L",
            Standard::C11 => "201112L",
            Standard::C17 => "201710L",
            Standard::C23 => "202311L",
        })
    }
}

impl Pp<'_> {
    fn define_predefined(&mut self, options: &Options) {
        self.define_object("__STDC__", "1");
        self.define_object("__STDC_HOSTED__", "1");
        if let Some(version) = options.standard.stdc_version() {
            self.define_object("__STDC_VERSION__", version);
        }
        self.define_object("__cinrs__", "1");
        // C11 6.10.8.3 makes four parts of the language optional and gives an
        // implementation a macro to say it left each one out. Two of them
        // depend on how this expansion was configured rather than on the
        // crate: complex arithmetic is absent when the `complex` feature is
        // off, in which case `_Complex` is a diagnostic and saying so turns
        // the gap into a conforming omission that a portable program can take
        // the other branch on; threads are absent on the targets whose C
        // library `<threads.h>` does not model, where that header is an
        // `#error` and the macro is what a program tests instead of hitting
        // it. The other two are *not* among them: `_Atomic`,
        // `<stdatomic.h>` and the `__atomic_*` builtins are all here, and so
        // are variable length arrays and the variably modified types built on
        // them — `int a[n][m]`, `int (*p)[n]`, `typedef int T[n]` and the
        // parameter forms — so neither `__STDC_NO_ATOMICS__` nor
        // `__STDC_NO_VLA__` is defined.
        //
        // `__STDC_IEC_559_COMPLEX__` is never defined either way: it claims
        // the whole of Annex G, and cinrs implements G.5.1's arithmetic
        // without claiming the rest of it.
        if !options.complex {
            self.define_object("__STDC_NO_COMPLEX__", "1");
        }
        if !threads_available(&options.target) {
            self.define_object("__STDC_NO_THREADS__", "1");
        }
        self.define_atomic_macros(options.target.max_scalar_align.min(8));
        // C11 7.28p2: these two say that `char16_t` and `char32_t` really are
        // UTF-16 and UTF-32, which is what the lexer encodes `u"…"` and `U"…"`
        // as. The value is the standard's own: the ISO/IEC 10646 revision the
        // encodings come from.
        self.define_object("__STDC_UTF_16__", "1");
        self.define_object("__STDC_UTF_32__", "1");
        // C23 6.10.1p5's three answers for `__has_embed`. GCC predefines them
        // in every mode it has, because a program that tests `__has_embed`
        // wants to compare against them whichever `-std=` it is compiled with.
        self.define_object("__STDC_EMBED_NOT_FOUND__", "0");
        self.define_object("__STDC_EMBED_FOUND__", "1");
        self.define_object("__STDC_EMBED_EMPTY__", "2");
        // Only a strict entry point is `-std=c99`; a GNU one is `-std=gnu99`.
        if !options.dialect.is_gnu() {
            self.define_object("__STRICT_ANSI__", "1");
        }
        // The GNU extensions this crate implements are the ones a program
        // guards with `#if defined(__GNUC__) && __GNUC__ >= 4`, so claiming
        // 4.2.1 is what makes those guards take the branch that uses them.
        // Clang set the same precedent for the same reason.
        self.define_object("__GNUC__", "4");
        self.define_object("__GNUC_MINOR__", "2");
        self.define_object("__GNUC_PATCHLEVEL__", "1");
        self.define_string(
            "__VERSION__",
            &format!("cinrs {}", env!("CARGO_PKG_VERSION")),
        );
        // Fixed placeholders: a build has to give the same output twice.
        self.define_string("__DATE__", "??? ?? ????");
        self.define_string("__TIME__", "??:??:??");
        self.define_string("__TIMESTAMP__", "??? ??? ?? ??:??:?? ????");
        let base_file = self.base_file.clone();
        self.define_string("__BASE_FILE__", &base_file);
        self.define_builtin("__LINE__", Builtin::Line);
        self.define_builtin("__FILE__", Builtin::File);
        self.define_builtin("__FILE_NAME__", Builtin::FileName);
        self.define_builtin("__INCLUDE_LEVEL__", Builtin::IncludeLevel);
        self.define_builtin("__COUNTER__", Builtin::Counter);
        // GCC's `__builtin_LINE()`, `__builtin_FILE()` and
        // `__builtin_FUNCTION()` say what `__LINE__`, `__FILE__` and
        // `__func__` say; being macros rather than builtins is what makes
        // them report the *use* rather than the definition, exactly as GCC's
        // do for a default argument.
        self.define_function("__builtin_LINE", "__LINE__");
        self.define_function("__builtin_FILE", "__FILE__");
        self.define_function("__builtin_FUNCTION", "__func__");
        for (name, value) in target_macros(&options.target) {
            self.define_object(name, &value);
        }
    }

    /// The macros GCC predefines for the atomic builtins, in every mode.
    ///
    /// The six `__ATOMIC_*` values are the argument the `__atomic_*` family
    /// takes, and their numbering is GCC's own — `<stdatomic.h>`'s
    /// `memory_order` enumeration has the same values, because a program may
    /// pass either to either. The `__GCC_ATOMIC_*_LOCK_FREE` family answers
    /// `2`, "always lock free", for every type there is a Rust atomic of, and
    /// `<stdatomic.h>`'s `ATOMIC_*_LOCK_FREE` macros are defined from these.
    fn define_atomic_macros(&mut self, max_atomic: u64) {
        for (name, value) in [
            ("__ATOMIC_RELAXED", "0"),
            ("__ATOMIC_CONSUME", "1"),
            ("__ATOMIC_ACQUIRE", "2"),
            ("__ATOMIC_RELEASE", "3"),
            ("__ATOMIC_ACQ_REL", "4"),
            ("__ATOMIC_SEQ_CST", "5"),
        ] {
            self.define_object(name, value);
        }
        for name in [
            "__GCC_ATOMIC_BOOL_LOCK_FREE",
            "__GCC_ATOMIC_CHAR_LOCK_FREE",
            "__GCC_ATOMIC_CHAR8_T_LOCK_FREE",
            "__GCC_ATOMIC_CHAR16_T_LOCK_FREE",
            "__GCC_ATOMIC_CHAR32_T_LOCK_FREE",
            "__GCC_ATOMIC_WCHAR_T_LOCK_FREE",
            "__GCC_ATOMIC_SHORT_LOCK_FREE",
            "__GCC_ATOMIC_INT_LOCK_FREE",
            "__GCC_ATOMIC_LONG_LOCK_FREE",
            "__GCC_ATOMIC_LLONG_LOCK_FREE",
            "__GCC_ATOMIC_POINTER_LOCK_FREE",
        ] {
            self.define_object(name, "2");
        }
        // What `__atomic_test_and_set` writes, which GCC also predefines.
        self.define_object("__GCC_ATOMIC_TEST_AND_SET_TRUEVAL", "1");
        // The `__sync_*` family's own advertisement, which a program tests
        // before writing one of them. Eight bytes only where an eight-byte
        // object is aligned well enough for a lock-free instruction; see
        // `TargetModel::max_scalar_align`.
        for width in [1u64, 2, 4, 8] {
            if width <= max_atomic {
                self.define_object(
                    match width {
                        1 => "__GCC_HAVE_SYNC_COMPARE_AND_SWAP_1",
                        2 => "__GCC_HAVE_SYNC_COMPARE_AND_SWAP_2",
                        4 => "__GCC_HAVE_SYNC_COMPARE_AND_SWAP_4",
                        _ => "__GCC_HAVE_SYNC_COMPARE_AND_SWAP_8",
                    },
                    "1",
                );
            }
        }
    }

    /// Defines a predefined function-like macro that takes no arguments.
    fn define_function(&mut self, name: &str, body: &str) {
        let tokens = lex::lex_text(body, self.base, &self.lex_options);
        let body: Vec<PTok> = tokens
            .iter()
            .filter(|t| !matches!(t.kind, TokenKind::Eof))
            .map(PTok::from_lexed)
            .collect();
        self.macros.insert(
            name.to_owned(),
            Arc::new(MacroDef {
                params: Some(Vec::new()),
                variadic: false,
                va_name: None,
                body,
                name_range: SourceRange::at(self.base),
                predefined: true,
                builtin: None,
            }),
        );
    }

    /// Defines a predefined object-like macro from the C text of its body.
    fn define_object(&mut self, name: &str, body: &str) {
        let tokens = lex::lex_text(body, self.base, &self.lex_options);
        let body: Vec<PTok> = tokens
            .iter()
            .filter(|t| !matches!(t.kind, TokenKind::Eof))
            .map(PTok::from_lexed)
            .collect();
        self.insert_predefined(name, body, None);
    }

    /// Defines a predefined macro whose body is one string literal.
    fn define_string(&mut self, name: &str, value: &str) {
        let kind = string_token_kind(value);
        let body = vec![PTok {
            kind,
            range: SourceRange::at(self.base),
            bol: false,
            space: false,
            origin: Origin::Source,
            hide: HideSet::default(),
            errors: Vec::new(),
        }];
        self.insert_predefined(name, body, None);
    }

    fn define_builtin(&mut self, name: &str, builtin: Builtin) {
        self.insert_predefined(name, Vec::new(), Some(builtin));
    }

    fn insert_predefined(&mut self, name: &str, body: Vec<PTok>, builtin: Option<Builtin>) {
        self.macros.insert(
            name.to_owned(),
            Arc::new(MacroDef {
                params: None,
                variadic: false,
                va_name: None,
                body,
                name_range: SourceRange::at(self.base),
                predefined: true,
                builtin,
            }),
        );
    }
}

/// Whether the bundled `<threads.h>` declares anything on this target, which
/// is what decides `__STDC_NO_THREADS__` (C11 6.10.8.3).
///
/// The C11 thread types are blocks of bytes whose size belongs to the C
/// library rather than to C, so the header models the two libraries whose
/// layouts it knows — glibc and musl, both on Linux — and refuses everywhere
/// else: Apple's libSystem and the Microsoft UCRT have no `<threads.h>` at
/// all, and the BSDs, bionic and uClibc each lay the objects out their own
/// way. Where it refuses, the macro says so, which is what lets a portable
/// program take the other branch instead of hitting the `#error`.
fn threads_available(target: &TargetModel) -> bool {
    target.os == Os::Linux && matches!(target.env, Env::Gnu | Env::Musl)
}

/// The target description macros, every one of them derived from `target`.
///
/// Nothing here reads a `cfg!`: the model may be the host's or may be a
/// `CINRS_TARGET` away, and a macro that answered for the host while `sizeof`
/// answered for the target would send a header down the wrong branch — which
/// is exactly how the bundled `<errno.h>`, `<stdio.h>`, `<time.h>` and
/// `<wchar.h>` choose their platform, through `_WIN32` and `__APPLE__`.
///
/// Deliberately short. Anything a real header would test for that is not here
/// simply comes out as 0 in an `#if`, which is the behaviour a C program
/// written for an unknown compiler expects; claiming to *be* GCC or Clang
/// would invite code paths built on extensions this crate does not have.
fn target_macros(target: &TargetModel) -> Vec<(&'static str, String)> {
    // The architecture, the operating system and the object format; see
    // `TargetModel::macros`.
    let mut out: Vec<(&'static str, String)> = target.macros();
    let flag = |out: &mut Vec<(&'static str, String)>, name: &'static str| {
        out.push((name, "1".to_owned()))
    };

    // The data model, which is exactly what `TargetModel` describes.
    if target.ptr_bits == 64 && target.long_bits == 64 {
        flag(&mut out, "__LP64__");
        flag(&mut out, "_LP64");
    } else if target.ptr_bits == 32 && target.int_bits == 32 && target.long_bits == 32 {
        flag(&mut out, "__ILP32__");
        flag(&mut out, "_ILP32");
    }
    if !target.char_signed {
        flag(&mut out, "__CHAR_UNSIGNED__");
    }
    out.push(("__CHAR_BIT__", "8".to_owned()));
    out.push(("__SIZEOF_SHORT__", (target.short_bits / 8).to_string()));
    out.push(("__SIZEOF_INT__", (target.int_bits / 8).to_string()));
    out.push(("__SIZEOF_LONG__", (target.long_bits / 8).to_string()));
    out.push((
        "__SIZEOF_LONG_LONG__",
        (target.long_long_bits / 8).to_string(),
    ));
    out.push(("__SIZEOF_POINTER__", (target.ptr_bits / 8).to_string()));
    // The macro a program tests before writing `__int128`. GCC defines it
    // exactly where the type exists, which is on the 64-bit architectures, so
    // a program guarding on it takes the other branch on an ILP32 target
    // rather than meeting the diagnostic.
    if target.has_int128 {
        out.push(("__SIZEOF_INT128__", "16".to_owned()));
    }

    // Byte order, spelled the way GCC spells it.
    out.push(("__ORDER_LITTLE_ENDIAN__", "1234".to_owned()));
    out.push(("__ORDER_BIG_ENDIAN__", "4321".to_owned()));
    out.push((
        "__BYTE_ORDER__",
        if target.big_endian {
            "4321".to_owned()
        } else {
            "1234".to_owned()
        },
    ));
    limit_macros(target, &mut out);
    out
}

/// The largest value a signed type of `bits` bits holds, as a decimal string.
fn signed_max(bits: u32) -> String {
    ((1u128 << (bits - 1)) - 1).to_string()
}

/// The largest value an unsigned type of `bits` bits holds.
fn unsigned_max(bits: u32) -> String {
    (u128::MAX >> (128 - bits)).to_string()
}

/// GCC's `__INT_MAX__`, `__SIZE_TYPE__` and the rest of that family.
///
/// A great deal of portable C is written against these rather than against
/// `<limits.h>` and `<stdint.h>`, because they are available before any header
/// is included and are what those headers are written in terms of. GCC's own
/// torture suite uses `__INT_MAX__` in ninety-five files and `__SIZE_TYPE__`
/// in seventy, and a program that tests one of them and finds it undefined
/// does not fail to compile — it silently takes the wrong branch, which is
/// worse. So the whole family is defined here, from the same
/// [`TargetModel`] everything else is derived from.
///
/// The spellings of the *types* are GCC's own (`long unsigned int` rather than
/// `unsigned long`), because a program may paste one into a `typedef` and
/// diff the result, and the suffixes on the *values* are the ones that give
/// each constant the type its name says it has.
///
/// What is deliberately absent: `__OPTIMIZE__` (nothing here optimises) and
/// the `__INT8_C`-style function-like macros, which take an argument.
/// `__SIZEOF_INT128__` is not here but among the data-model macros, and only
/// on a target that has `__int128` at all.
fn limit_macros(target: &TargetModel, out: &mut Vec<(&'static str, String)>) {
    let int_bits = target.int_bits;
    let long_bits = target.long_bits;
    let llong_bits = target.long_long_bits;
    let ptr_bits = target.ptr_bits;

    // `size_t`, `ptrdiff_t` and `intptr_t` are the *narrowest* standard type
    // as wide as a pointer, which is how GCC picks them: `unsigned int` on
    // i686, `long unsigned int` on LP64, `long long unsigned int` on 64-bit
    // Windows, where `long` is only 32 bits.
    let (ptr_signed, ptr_unsigned, ptr_suffix) = if int_bits >= ptr_bits {
        ("int", "unsigned int", "")
    } else if long_bits >= ptr_bits {
        ("long int", "long unsigned int", "L")
    } else {
        ("long long int", "long long unsigned int", "LL")
    };
    // `intmax_t` is the widest standard integer type there is, which is
    // `long long` unless `long` is just as wide — GCC says `long int` on LP64
    // and `long long int` on i686 and on Windows. It does *not* follow the
    // pointer: an ILP32 target still has a 64-bit `intmax_t`, and C99 6.10.1
    // makes it the type all `#if` arithmetic is done in.
    let (max_signed, max_unsigned, max_suffix) = if long_bits >= llong_bits {
        ("long int", "long unsigned int", "L")
    } else {
        ("long long int", "long long unsigned int", "LL")
    };
    let max_bits = long_bits.max(llong_bits);

    let mut push = |name: &'static str, value: String| out.push((name, value));

    // The limits of the standard integer types.
    push("__SCHAR_MAX__", signed_max(8));
    push("__SHRT_MAX__", signed_max(target.short_bits));
    push("__INT_MAX__", signed_max(int_bits));
    push("__LONG_MAX__", format!("{}L", signed_max(long_bits)));
    push("__LONG_LONG_MAX__", format!("{}LL", signed_max(llong_bits)));

    // Their widths, which C23 added to <limits.h> and GCC has always had.
    // The two compilers do not spell the same set: GCC has
    // `__LONG_LONG_WIDTH__` and `__SCHAR_WIDTH__`, Clang has `__LLONG_WIDTH__`
    // and `__BOOL_WIDTH__`, and code in the wild tests whichever its author's
    // compiler had — `clang/test/C/drs/dr2xx.c` `#error`s out on
    // `__LLONG_WIDTH__` alone. So the *union* is defined, and the two
    // spellings of one width are one value by construction.
    push("__BOOL_WIDTH__", "1".to_owned());
    push("__SCHAR_WIDTH__", "8".to_owned());
    push("__SHRT_WIDTH__", target.short_bits.to_string());
    push("__INT_WIDTH__", int_bits.to_string());
    push("__LONG_WIDTH__", long_bits.to_string());
    push("__LONG_LONG_WIDTH__", llong_bits.to_string());
    push("__LLONG_WIDTH__", llong_bits.to_string());

    // The library types, and how wide each is.
    push("__SIZE_TYPE__", ptr_unsigned.to_owned());
    push(
        "__SIZE_MAX__",
        format!("{}U{ptr_suffix}", unsigned_max(ptr_bits)),
    );
    push("__SIZE_WIDTH__", ptr_bits.to_string());
    push("__SIZEOF_SIZE_T__", (ptr_bits / 8).to_string());
    push("__PTRDIFF_TYPE__", ptr_signed.to_owned());
    push(
        "__PTRDIFF_MAX__",
        format!("{}{ptr_suffix}", signed_max(ptr_bits)),
    );
    push("__PTRDIFF_WIDTH__", ptr_bits.to_string());
    push("__SIZEOF_PTRDIFF_T__", (ptr_bits / 8).to_string());
    push("__INTMAX_TYPE__", max_signed.to_owned());
    push(
        "__INTMAX_MAX__",
        format!("{}{max_suffix}", signed_max(max_bits)),
    );
    push("__INTMAX_WIDTH__", max_bits.to_string());
    push("__SIZEOF_INTMAX__", (max_bits / 8).to_string());
    push("__UINTMAX_TYPE__", max_unsigned.to_owned());
    push(
        "__UINTMAX_MAX__",
        format!("{}U{max_suffix}", unsigned_max(max_bits)),
    );
    push("__UINTMAX_WIDTH__", max_bits.to_string());
    push("__INTPTR_TYPE__", ptr_signed.to_owned());
    push(
        "__INTPTR_MAX__",
        format!("{}{ptr_suffix}", signed_max(ptr_bits)),
    );
    push("__INTPTR_WIDTH__", ptr_bits.to_string());
    push("__UINTPTR_TYPE__", ptr_unsigned.to_owned());
    push(
        "__UINTPTR_MAX__",
        format!("{}U{ptr_suffix}", unsigned_max(ptr_bits)),
    );
    push("__UINTPTR_WIDTH__", ptr_bits.to_string());
    push("__POINTER_WIDTH__", ptr_bits.to_string());

    // `wchar_t` and `wint_t`, which the bundled <stddef.h> and <wchar.h>
    // typedef from these very macros. Windows makes both 16 bits, Arm makes
    // `wchar_t` unsigned, and Apple makes `wint_t` an `int`.
    let wchar_bits = target.wchar_bits;
    let (wchar_type, wchar_max, wchar_min) = if target.wchar_signed {
        (
            if wchar_bits == 16 { "short int" } else { "int" },
            signed_max(wchar_bits),
            format!("(-{}-1)", signed_max(wchar_bits)),
        )
    } else {
        (
            if wchar_bits == 16 {
                "short unsigned int"
            } else {
                "unsigned int"
            },
            unsigned_max(wchar_bits),
            "0".to_owned(),
        )
    };
    push("__WCHAR_TYPE__", wchar_type.to_owned());
    push("__WCHAR_MAX__", wchar_max);
    push("__WCHAR_MIN__", wchar_min);
    push("__WCHAR_WIDTH__", wchar_bits.to_string());
    push("__SIZEOF_WCHAR_T__", (wchar_bits / 8).to_string());
    if !target.wchar_signed {
        push("__WCHAR_UNSIGNED__", "1".to_owned());
    }
    let wint_bits = target.wint_bits;
    push(
        "__WINT_TYPE__",
        match (target.wint_signed, wint_bits) {
            (true, 16) => "short int",
            (true, _) => "int",
            (false, 16) => "short unsigned int",
            (false, _) => "unsigned int",
        }
        .to_owned(),
    );
    push("__WINT_WIDTH__", wint_bits.to_string());
    push("__SIZEOF_WINT_T__", (wint_bits / 8).to_string());
    push("__SIG_ATOMIC_TYPE__", "int".to_owned());
    push("__SIG_ATOMIC_MAX__", signed_max(int_bits));
    push(
        "__SIG_ATOMIC_MIN__",
        format!("(-{}-1)", signed_max(int_bits)),
    );
    push("__SIG_ATOMIC_WIDTH__", int_bits.to_string());
    push("__CHAR16_TYPE__", "short unsigned int".to_owned());
    push("__CHAR32_TYPE__", "unsigned int".to_owned());

    // The floating types. `long double` is `double` here, and the values are
    // the ones `include/float.h` gives.
    push("__SIZEOF_FLOAT__", "4".to_owned());
    push("__SIZEOF_DOUBLE__", "8".to_owned());
    push("__SIZEOF_LONG_DOUBLE__", "8".to_owned());
    // Everything `<float.h>` says about a floating type, under the names GCC
    // gives it: a great deal of portable C tests `__DBL_MIN_EXP__` rather than
    // including the header, and a program that finds one of these undefined
    // does not fail to compile — it silently takes the wrong branch.
    // `execute/ieee/pr30704` is exactly that.
    push("__FLT_RADIX__", "2".to_owned());
    push("__FLT_EVAL_METHOD__", "0".to_owned());
    push("__FLT_MANT_DIG__", "24".to_owned());
    push("__FLT_DIG__", "6".to_owned());
    push("__FLT_MIN_EXP__", "(-125)".to_owned());
    push("__FLT_MIN_10_EXP__", "(-37)".to_owned());
    push("__FLT_MAX_EXP__", "128".to_owned());
    push("__FLT_MAX_10_EXP__", "38".to_owned());
    push("__FLT_DECIMAL_DIG__", "9".to_owned());
    push("__FLT_MAX__", "3.40282346638528859812e+38F".to_owned());
    push("__FLT_NORM_MAX__", "3.40282346638528859812e+38F".to_owned());
    push("__FLT_MIN__", "1.17549435082228750797e-38F".to_owned());
    push("__FLT_EPSILON__", "1.19209289550781250000e-7F".to_owned());
    push(
        "__FLT_DENORM_MIN__",
        "1.40129846432481707092e-45F".to_owned(),
    );
    push("__FLT_HAS_DENORM__", "1".to_owned());
    push("__FLT_HAS_INFINITY__", "1".to_owned());
    push("__FLT_HAS_QUIET_NAN__", "1".to_owned());
    push("__FLT_IS_IEC_60559__", "1".to_owned());
    push("__DBL_MANT_DIG__", "53".to_owned());
    push("__DBL_DIG__", "15".to_owned());
    push("__DBL_MIN_EXP__", "(-1021)".to_owned());
    push("__DBL_MIN_10_EXP__", "(-307)".to_owned());
    push("__DBL_MAX_EXP__", "1024".to_owned());
    push("__DBL_MAX_10_EXP__", "308".to_owned());
    push("__DBL_DECIMAL_DIG__", "17".to_owned());
    push("__DBL_MAX__", "1.79769313486231570815e+308".to_owned());
    push("__DBL_NORM_MAX__", "1.79769313486231570815e+308".to_owned());
    push("__DBL_MIN__", "2.22507385850720138309e-308".to_owned());
    push("__DBL_EPSILON__", "2.22044604925031308085e-16".to_owned());
    push(
        "__DBL_DENORM_MIN__",
        "4.94065645841246544177e-324".to_owned(),
    );
    push("__DBL_HAS_DENORM__", "1".to_owned());
    push("__DBL_HAS_INFINITY__", "1".to_owned());
    push("__DBL_HAS_QUIET_NAN__", "1".to_owned());
    push("__DBL_IS_IEC_60559__", "1".to_owned());
    // `long double` is `double` here — there is no portable Rust type with the
    // layout of an x87 extended double — so its family repeats `double`'s with
    // the suffix that gives each constant the type its name says it has, which
    // is what the bundled `<float.h>` does too.
    push("__LDBL_MANT_DIG__", "53".to_owned());
    push("__LDBL_DIG__", "15".to_owned());
    push("__LDBL_MIN_EXP__", "(-1021)".to_owned());
    push("__LDBL_MIN_10_EXP__", "(-307)".to_owned());
    push("__LDBL_MAX_EXP__", "1024".to_owned());
    push("__LDBL_MAX_10_EXP__", "308".to_owned());
    push("__LDBL_DECIMAL_DIG__", "17".to_owned());
    push("__DECIMAL_DIG__", "17".to_owned());
    push("__LDBL_MAX__", "1.79769313486231570815e+308L".to_owned());
    push(
        "__LDBL_NORM_MAX__",
        "1.79769313486231570815e+308L".to_owned(),
    );
    push("__LDBL_MIN__", "2.22507385850720138309e-308L".to_owned());
    push("__LDBL_EPSILON__", "2.22044604925031308085e-16L".to_owned());
    push(
        "__LDBL_DENORM_MIN__",
        "4.94065645841246544177e-324L".to_owned(),
    );
    push("__LDBL_HAS_DENORM__", "1".to_owned());
    push("__LDBL_HAS_INFINITY__", "1".to_owned());
    push("__LDBL_HAS_QUIET_NAN__", "1".to_owned());
    push("__LDBL_IS_IEC_60559__", "1".to_owned());

    // The exact-width types of <stdint.h>, which GCC's own <stdint.h> is
    // written in terms of. `int64_t` follows `long` wherever `long` is 64
    // bits, exactly as GCC has it.
    let (i64_type, u64_type, s64, u64) = if long_bits == 64 {
        ("long int", "long unsigned int", "L", "UL")
    } else {
        ("long long int", "long long unsigned int", "LL", "ULL")
    };
    let widths: [(
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        u32,
    ); 4] = [
        ("8", "signed char", "unsigned char", "", "", 8),
        ("16", "short int", "short unsigned int", "", "", 16),
        ("32", "int", "unsigned int", "", "U", 32),
        ("64", i64_type, u64_type, s64, u64, 64),
    ];
    // The names have to be `'static`, so the four sets are written out rather
    // than built; the values still come from the loop above.
    const EXACT: [[&str; 8]; 4] = [
        [
            "__INT8_TYPE__",
            "__UINT8_TYPE__",
            "__INT8_MAX__",
            "__UINT8_MAX__",
            "__INT_LEAST8_TYPE__",
            "__UINT_LEAST8_TYPE__",
            "__INT_LEAST8_MAX__",
            "__UINT_LEAST8_MAX__",
        ],
        [
            "__INT16_TYPE__",
            "__UINT16_TYPE__",
            "__INT16_MAX__",
            "__UINT16_MAX__",
            "__INT_LEAST16_TYPE__",
            "__UINT_LEAST16_TYPE__",
            "__INT_LEAST16_MAX__",
            "__UINT_LEAST16_MAX__",
        ],
        [
            "__INT32_TYPE__",
            "__UINT32_TYPE__",
            "__INT32_MAX__",
            "__UINT32_MAX__",
            "__INT_LEAST32_TYPE__",
            "__UINT_LEAST32_TYPE__",
            "__INT_LEAST32_MAX__",
            "__UINT_LEAST32_MAX__",
        ],
        [
            "__INT64_TYPE__",
            "__UINT64_TYPE__",
            "__INT64_MAX__",
            "__UINT64_MAX__",
            "__INT_LEAST64_TYPE__",
            "__UINT_LEAST64_TYPE__",
            "__INT_LEAST64_MAX__",
            "__UINT_LEAST64_MAX__",
        ],
    ];
    for (names, (_, signed, unsigned, s_suffix, u_suffix, bits)) in EXACT.iter().zip(widths) {
        let smax = format!("{}{s_suffix}", signed_max(bits));
        let umax = format!("{}{u_suffix}", unsigned_max(bits));
        for at in [0, 4] {
            out.push((names[at], signed.to_owned()));
            out.push((names[at + 1], unsigned.to_owned()));
            out.push((names[at + 2], smax.clone()));
            out.push((names[at + 3], umax.clone()));
        }
    }

    // How wide each of those is. The `least` widths are exact by
    // construction; the `fast` ones follow the choice `include/stdint.h`
    // makes for the typedefs, so the macro and a `sizeof` on the type give
    // one answer. (`__BITINT_MAXWIDTH__` is deliberately absent: it is the
    // signal that `_BitInt` exists, and here it does not.)
    let fast_mid = if ptr_bits == 64 { "64" } else { "32" };
    for (name, value) in [
        ("__INT_LEAST8_WIDTH__", "8"),
        ("__INT_LEAST16_WIDTH__", "16"),
        ("__INT_LEAST32_WIDTH__", "32"),
        ("__INT_LEAST64_WIDTH__", "64"),
        ("__INT_FAST8_WIDTH__", "8"),
        ("__INT_FAST16_WIDTH__", fast_mid),
        ("__INT_FAST32_WIDTH__", fast_mid),
        ("__INT_FAST64_WIDTH__", "64"),
    ] {
        out.push((name, value.to_owned()));
    }
}
