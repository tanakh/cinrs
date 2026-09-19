//! Diagnostics: collection and emission as `compile_error!` invocations.
//!
//! Every diagnostic carries a [`SourceRange`] into the captured C text. At
//! emission time the range is resolved through the [`SourceMap`] into a
//! `proc_macro2::Span` and *every* token of the generated
//! `::core::compile_error! { "…" }` is stamped with it, so `rustc` renders the
//! error with its caret on the offending C token.
//!
//! Several diagnostics simply become several `compile_error!` items; `rustc`
//! reports them all.
//!
//! Warnings are kept in the model but are not emitted: there is no stable way
//! for a procedural macro to raise a warning, and turning warnings into hard
//! errors would be worse than staying quiet.

use proc_macro2::{Literal, Span, TokenStream};
use quote::quote_spanned;

use crate::capture::{SourceMap, SourceRange};

/// Severity of a [`Diagnostic`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    /// Stops compilation.
    Error,
    /// Advisory only; currently not emitted (see the [module docs](self)).
    Warning,
}

/// An extra remark attached to a [`Diagnostic`].
///
/// A procedural macro has one span per diagnostic and no way to add a second
/// one, so a note that refers to *another* place — the `#define` a macro came
/// from, the first of two conflicting declarations — cannot point there. Its
/// `range` is rendered into the message instead, as ` at line 5` for the
/// macro's own text or ` in include/foo.h:12` for an `#include`d file; see
/// [`Diagnostics::render`]. Messages are therefore phrased to be completed by
/// that phrase ("macro 'MAX' defined", not "macro 'MAX' defined here").
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Note {
    /// The text of the note.
    pub message: String,
    /// The place the note is about, if any.
    pub range: Option<SourceRange>,
}

/// One problem found in the C source.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    /// Severity.
    pub level: Level,
    /// The primary message, phrased like a C compiler would phrase it.
    pub message: String,
    /// What the message is about.
    pub range: SourceRange,
    /// Additional remarks.
    pub notes: Vec<Note>,
    /// Whether the *text* is what is wrong — the spelling of a preprocessing
    /// token, or the comments before it — rather than the token itself;
    /// something translation phase 3 decided as the token was being formed.
    ///
    /// The lexer's findings are normally held on the token and reported only
    /// if it survives into the preprocessor's output, because C99 6.4p3 makes
    /// any character that fits nothing else a preprocessing token of its own:
    /// a stray `\` or `$` handed to a macro that drops its argument is not an
    /// error at all. A constraint on the text is different — a universal
    /// character name that names a character 6.4.3p2 forbids is ill-formed
    /// where it is *written*, and throwing the token away does not make it
    /// well-formed; nor does an unterminated comment, or a `//` one in C89,
    /// stop being wrong because the token after it opened a directive — so
    /// these are reported as soon as the token is read.
    ///
    /// "As soon as it is read" is still not inside a group `#if 0` skips: that
    /// text is never read at all.
    pub lexical: bool,
}

impl Diagnostic {
    /// Creates an error.
    pub fn error(range: SourceRange, message: impl Into<String>) -> Self {
        Self {
            level: Level::Error,
            message: message.into(),
            range,
            notes: Vec::new(),
            lexical: false,
        }
    }

    /// Creates a warning.
    pub fn warning(range: SourceRange, message: impl Into<String>) -> Self {
        Self {
            level: Level::Warning,
            message: message.into(),
            range,
            notes: Vec::new(),
            lexical: false,
        }
    }

    /// Marks this as a problem with a token's spelling; see
    /// [`Diagnostic::lexical`].
    pub fn at_lexing(mut self) -> Self {
        self.lexical = true;
        self
    }

    /// Attaches a note.
    pub fn with_note(mut self, message: impl Into<String>) -> Self {
        self.notes.push(Note {
            message: message.into(),
            range: None,
        });
        self
    }

    /// Attaches a note that points somewhere.
    pub fn with_note_at(mut self, range: SourceRange, message: impl Into<String>) -> Self {
        self.notes.push(Note {
            message: message.into(),
            range: Some(range),
        });
        self
    }
}

/// A sink collecting every problem found while processing one macro
/// invocation.
#[derive(Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
    /// Set when the input could not be processed at all; reported at the call
    /// site because there is no meaningful C position to point at.
    fatal: Option<String>,
}

impl Diagnostics {
    /// Creates an empty sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a diagnostic.
    pub fn push(&mut self, diag: Diagnostic) {
        self.items.push(diag);
    }

    /// Records an error at `range`.
    pub fn error(&mut self, range: SourceRange, message: impl Into<String>) {
        self.push(Diagnostic::error(range, message));
    }

    /// Records a warning at `range`.
    pub fn warning(&mut self, range: SourceRange, message: impl Into<String>) {
        self.push(Diagnostic::warning(range, message));
    }

    /// Merges another sink's contents into this one.
    pub fn extend(&mut self, other: Diagnostics) {
        self.items.extend(other.items);
        if self.fatal.is_none() {
            self.fatal = other.fatal;
        }
    }

    /// Records an unrecoverable problem with the macro input itself.
    pub fn fatal(&mut self, message: impl Into<String>) {
        if self.fatal.is_none() {
            self.fatal = Some(message.into());
        }
    }

    /// All recorded diagnostics, in the order they were found.
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// All recorded diagnostics, for a pass that annotates them after the
    /// fact.
    ///
    /// [`Expansions::annotate`](crate::pp::Expansions::annotate) is the reason
    /// this exists: whether a diagnostic happened inside a macro expansion is
    /// only known to the preprocessor, and only after the pass that produced
    /// the diagnostic has finished.
    pub fn items_mut(&mut self) -> &mut [Diagnostic] {
        &mut self.items
    }

    /// Whether anything would stop compilation.
    pub fn has_errors(&self) -> bool {
        self.fatal.is_some() || self.items.iter().any(|d| d.level == Level::Error)
    }

    /// Number of errors recorded.
    pub fn error_count(&self) -> usize {
        self.items
            .iter()
            .filter(|d| d.level == Level::Error)
            .count()
    }

    /// The diagnostics in source order.
    ///
    /// The lexer runs to completion before the parser starts, so the raw
    /// insertion order interleaves badly; reporting top to bottom is what a
    /// reader expects.
    pub fn sorted(&self) -> Vec<&Diagnostic> {
        let mut out: Vec<&Diagnostic> = self.items.iter().collect();
        out.sort_by_key(|d| d.range.start);
        out
    }

    /// Renders every error as a `compile_error!` invocation spanned at the C
    /// token it refers to.
    pub fn to_token_stream(&self, map: &SourceMap) -> TokenStream {
        let mut out = TokenStream::new();
        if let Some(msg) = &self.fatal {
            out.extend(compile_error_at(Span::call_site(), msg));
        }
        for diag in self.sorted() {
            if diag.level != Level::Error {
                continue;
            }
            let span = map.span(diag.range);
            out.extend(compile_error_at(span, &self.render(map, diag)));
        }
        out
    }

    /// The full message text of `diag`, including position information for
    /// sources whose spans cannot point at an exact location.
    ///
    /// Three things can be added to the text the pass wrote:
    ///
    /// * A `header.h:12:5: ` prefix, when the problem is inside an `#include`d
    ///   file. The caret is on the `#include` that pulled the header in, which
    ///   says nothing about *where* in it the problem is.
    /// * A ` (at line L, column C of the C source)` suffix, when the input was
    ///   a string literal, which stable Rust cannot span into.
    /// * A location for every [`Note`] that has one; see there.
    pub fn render(&self, map: &SourceMap, diag: &Diagnostic) -> String {
        let mut msg = position_prefix(map, diag.range);
        msg.push_str(&diag.message);
        msg.push_str(&position_suffix(map, diag.range));
        for note in &diag.notes {
            msg.push_str("\nnote: ");
            msg.push_str(&note.message);
            if let Some(range) = note.range {
                msg.push_str(&note_location(map, range));
            }
        }
        msg
    }
}

/// `header.h:12:5: `, for a problem found inside an `#include`d file.
fn position_prefix(map: &SourceMap, range: SourceRange) -> String {
    match map.header_position(range.start) {
        Some((name, line, column)) => format!("{name}:{line}:{column}: "),
        None => String::new(),
    }
}

/// ` (at line L, column C of the C source)`, but only where a span cannot
/// point at the position by itself (i.e. string-literal mode, unless the
/// caller supplied a [`Subspan`](crate::Subspan) hook that answered).
///
/// An `#include`d file is not precise either, but it carries its position in
/// the [prefix](position_prefix) instead, which names the header as well.
fn position_suffix(map: &SourceMap, range: SourceRange) -> String {
    if map.is_precise_at(range) || map.header_position(range.start).is_some() {
        return String::new();
    }
    let (line, column) = map.line_col(range.start);
    format!(" (at line {line}, column {column} of the C source)")
}

/// ` at line 5` or ` in include/foo.h:12` — where a [`Note`] is pointing.
///
/// The line is counted exactly as `__LINE__` counts it: a line of the `.rs`
/// file the invocation is written in for the macro's own text, and a line of
/// the header itself for an `#include`d file, which is also named.
fn note_location(map: &SourceMap, range: SourceRange) -> String {
    match map.header_position(range.start) {
        Some((name, line, _)) => format!(" in {name}:{line}"),
        None => format!(" at line {}", map.source_line(range.start)),
    }
}

/// Builds `::core::compile_error! { "message" }` with every token spanned at
/// `span`.
pub fn compile_error_at(span: Span, message: &str) -> TokenStream {
    let mut lit = Literal::string(message);
    lit.set_span(span);
    // Braces (rather than parentheses) so that the expansion is valid both in
    // item position and in statement position.
    quote_spanned! { span => ::core::compile_error! { #lit } }
}
