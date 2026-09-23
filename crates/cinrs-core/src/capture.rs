//! Input capture and source mapping.
//!
//! The whole point of `cinrs` is that a diagnostic produced anywhere in the
//! pipeline — by our own lexer/parser/sema, or by `rustc` on the code we
//! generate — must point at the exact *C* token that caused it. To make that
//! possible the front end never works on a `TokenStream` directly: it first
//! recovers a plain-text C translation unit plus a [`SourceMap`] that can turn
//! any byte offset in that text back into a [`proc_macro2::Span`].
//!
//! # Capture strategies
//!
//! There are two shapes of input, and the raw-token shape has two strategies:
//!
//! * **String-literal mode** ([`InputMode::StringLiteral`]). The entire macro
//!   input is a single Rust string literal (`"…"`, `r"…"`, `r#"…"#`). We
//!   unescape it and use the text as-is. This accepts *any* C code, including
//!   the lexemes the Rust 2024 lexer rejects (hex float literals, `'ab'`,
//!   `L"…"`, `\` line continuations, `##`). The catch is that stable Rust has
//!   no way to build a span pointing *inside* a string literal
//!   (`Literal::subspan` is unstable), so every diagnostic is reported at the
//!   literal as a whole and the line/column inside the C text is appended to
//!   the message instead. Such a file is flagged as not [`SourceFile::precise`]
//!   — unless the caller hands capture a [`Subspan`] hook, which is what the
//!   `nightly` feature of `cinrs-macros` does; see [`Subspan`].
//!
//! * **Raw-token mode**, primary path ([`InputMode::FileSlice`]). We flatten
//!   the token trees, ask the first token for [`Span::local_file`], read that
//!   `.rs` file from disk and slice out `first.start() .. last.end()`. This
//!   gives back the *exact* original text, including whitespace, newlines and
//!   comments — which the [preprocessor](crate::pp) needs, since it is
//!   line-oriented and since `#error` reproduces what was written. The slice
//!   is validated against every token's [`Span::source_text`] before it is
//!   trusted.
//!
//! * **File mode** ([`InputMode::CFile`]), which is what
//!   `include_c99!("…")` captures with [`capture_c_file`]: the text is a `.c`
//!   file read from disk, and nothing in the `.rs` file corresponds to a
//!   position in it, so every range resolves to the span of the macro
//!   invocation and the file names its own position in the message — exactly
//!   as an `#include`d header does.
//!
//! * **Raw-token mode**, search path (also [`InputMode::FileSlice`]). A host
//!   may hand a procedural macro tokens with no positions at all:
//!   `rust-analyzer` reports no file, no source text and line 1 column 0 for
//!   every token alike. The text is still on disk, and which text it is can be
//!   *proved* — the `locate` module searches the crate's `.rs` files, the
//!   directory `CARGO_MANIFEST_DIR` names, for an invocation whose token
//!   sequence is exactly the one we were handed. A match gives the same three
//!   things the primary path gives (path, text, anchors), so everything
//!   downstream — diagnostics, `__FILE__`, a quoted `#include`, `include_str!`
//!   tracking, the unit id — is unchanged. See [`Origin`] for what capture has
//!   to be told for this to be possible, and note that it can only run when the
//!   primary path found no position whatsoever: in a normal build it costs
//!   nothing because it never happens.
//!
//! * **Raw-token mode**, fallback path ([`InputMode::Reconstructed`]). When
//!   there is no local file (macro-generated input, some IDE contexts such as
//!   rust-analyzer, or unit tests that build a `TokenStream` with
//!   `TokenStream::from_str`), or when the slice fails validation, we rebuild
//!   the text from the tokens. Where their positions are usable, every token
//!   goes at its original line/column, padded with newlines and spaces:
//!   comments are lost, but the line structure — and therefore all reported
//!   positions — survives. Where they are not (every token at the same
//!   position, which is what `rust-analyzer` gives an unsaved buffer), the text
//!   is rebuilt from the tokens alone: one space between two tokens, none where
//!   the host says they were written together, so that `->`, `<<=` and `++`
//!   survive and `- -` stays two tokens. A *directive* is a line, and tokens
//!   keep no lines; the forms whose end the tokens themselves give away
//!   (`#include <…>`, `#ifdef X`, `#endif`, …) are written on a line of their
//!   own and anything else — `#define`, `#if` — is one clear diagnostic rather
//!   than a guess.
//!
//! # Coordinates
//!
//! [`SourceMap`] owns any number of [`SourceFile`]s in a single, global byte
//! offset space: file *i* occupies `base .. base + text.len()`. A [`Pos`] is
//! therefore enough to identify both a file and an offset inside it, which is
//! what will let `#include` drop extra files into the same map without
//! changing a single signature in the lexer, parser or AST.
//!
//! A file also remembers which `.rs` file it came from and which line of it
//! the text's own line 1 sits on, which is what the preprocessor's `__FILE__`
//! and `__LINE__` are made of.

use proc_macro2::{Delimiter, LineColumn, Spacing, Span, TokenStream, TokenTree};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::diag::{Diagnostic, Diagnostics};
use crate::locate;

/// A position in the global byte-offset space managed by a [`SourceMap`].
pub type Pos = u32;

/// Identifies one file inside a [`SourceMap`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct FileId(u32);

impl FileId {
    /// The index of this file inside its [`SourceMap`].
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A half-open range `[start, end)` in a [`SourceMap`]'s global offset space.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SourceRange {
    /// First byte of the range.
    pub start: Pos,
    /// One past the last byte of the range.
    pub end: Pos,
}

impl SourceRange {
    /// Builds a range, clamping `end` so that it is never before `start`.
    pub fn new(start: Pos, end: Pos) -> Self {
        Self {
            start,
            end: if end < start { start } else { end },
        }
    }

    /// An empty range at `pos`.
    pub fn at(pos: Pos) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }

    /// The smallest range covering both `self` and `other`.
    pub fn join(self, other: Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Length in bytes.
    pub fn len(self) -> u32 {
        self.end - self.start
    }

    /// Whether the range is empty.
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
}

/// `(start, end, span)` triples describing where each Rust token landed in a
/// captured file, in file-local byte offsets.
pub(crate) type AnchorList = Vec<(u32, u32, Span)>;

// ---------------------------------------------------------------------------
// spans inside a string literal
// ---------------------------------------------------------------------------

/// Turns a byte range of the macro input's *source spelling* into a span
/// pointing exactly at those bytes.
///
/// This is how [string-literal mode](InputMode::StringLiteral) stops being
/// imprecise. `proc_macro::Literal::subspan` does exactly this job, but it is
/// unstable (`proc_macro_span`) and `proc_macro2` does not re-export it, so
/// this crate — which never touches `proc_macro` — takes it as a hook instead:
/// `cinrs-macros`, compiled with its `nightly` feature, passes one in, and
/// everything else passes [`None`] and keeps the ` (at line …)` suffix.
///
/// The range is measured in bytes of the literal *as written*, `r#"` prefix,
/// closing `"#` and escape sequences and all, which is what `subspan` wants;
/// [`SourceMap`] does the translation from C-text offsets. Returning [`None`]
/// — which the real `subspan` does whenever the literal has no source of its
/// own, as under `rust-analyzer` — falls back to the whole-literal span.
///
/// Results are memoised, because a span is asked for once per *generated
/// token* and the real implementation is a round trip into the compiler.
#[derive(Clone)]
pub struct Subspan(Rc<SubspanInner>);

struct SubspanInner {
    resolve: Box<dyn Fn(Range<usize>) -> Option<Span>>,
    cache: RefCell<HashMap<(u32, u32), Option<Span>>>,
}

impl Subspan {
    /// Wraps a `subspan` implementation.
    pub fn new(resolve: impl Fn(Range<usize>) -> Option<Span> + 'static) -> Self {
        Self(Rc::new(SubspanInner {
            resolve: Box::new(resolve),
            cache: RefCell::new(HashMap::new()),
        }))
    }

    /// The span of `start .. end` in the literal's spelling.
    fn get(&self, start: u32, end: u32) -> Option<Span> {
        if let Some(hit) = self.0.cache.borrow().get(&(start, end)) {
            return *hit;
        }
        let span = (self.0.resolve)(start as usize..end as usize);
        self.0.cache.borrow_mut().insert((start, end), span);
        span
    }
}

impl std::fmt::Debug for Subspan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Subspan")
    }
}

// ---------------------------------------------------------------------------
// what the host says about the invocation itself
// ---------------------------------------------------------------------------

/// What the *host* can tell the front end about an invocation, over and above
/// the tokens themselves.
///
/// Two of the three capture strategies need something a `TokenStream` does not
/// carry:
///
/// * The search that finds the invocation in the crate's own sources needs the
///   crate's directory — `CARGO_MANIFEST_DIR`, which Cargo sets for `rustc` and
///   for `rust-analyzer` alike — and the name the invocation may be written
///   with, which narrows the candidates.
/// * String-literal mode needs the [`Subspan`] hook to put a caret inside the
///   literal.
///
/// The default is an origin that says nothing: no directory, so no search, and
/// no hook. That is what [`capture`], [`analyze`](crate::analyze) and
/// [`expand`](crate::expand) use — a test building a `TokenStream` from a
/// string must not depend on the developer's working directory, or on what
/// happens to be written in the crate around it. The procedural macros pass
/// [`Origin::new`], which is the real thing.
#[derive(Clone, Default, Debug)]
pub struct Origin {
    entry_names: &'static [&'static str],
    crate_dir: Option<PathBuf>,
    subspan: Option<Subspan>,
}

impl Origin {
    /// An origin that tells capture nothing; see the [type
    /// documentation](Origin).
    pub fn unknown() -> Self {
        Self::default()
    }

    /// The origin of a real expansion of an entry point that may be written
    /// with any of `entry_names` (`&["c89", "c90"]` for the two names one
    /// entry point has), in the crate Cargo names.
    ///
    /// The names carry no `!` and no path: an invocation is looked for by its
    /// last path segment, so `cinrs::c99!` is found by `"c99"`.
    pub fn new(entry_names: &'static [&'static str]) -> Self {
        Self {
            entry_names,
            crate_dir: std::env::var_os(crate::include::MANIFEST_DIR_VAR).map(PathBuf::from),
            subspan: None,
        }
    }

    /// This origin with a hook that can point inside a string literal; see
    /// [`Subspan`].
    pub fn with_subspan(mut self, subspan: Option<Subspan>) -> Self {
        self.subspan = subspan;
        self
    }

    /// This origin with the directory to search set explicitly.
    ///
    /// [`Origin::new`] reads `CARGO_MANIFEST_DIR`, which is process-global; a
    /// test says which directory it means instead.
    pub fn in_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.crate_dir = Some(dir.into());
        self
    }

    /// The names an invocation of this entry point may be written with.
    pub fn entry_names(&self) -> &'static [&'static str] {
        self.entry_names
    }

    /// The directory the search walks, when there is one.
    pub fn crate_dir(&self) -> Option<&Path> {
        self.crate_dir.as_deref()
    }

    /// The entry point's own name, as a diagnostic spells it.
    fn entry_name(&self) -> &str {
        self.entry_names.first().copied().unwrap_or("c99")
    }

    /// Searches the crate's sources for the invocation `toks` came from.
    ///
    /// [`None`] whenever the search cannot run at all — no directory, which is
    /// every context but a real expansion — or found nothing it could prove.
    fn search(&self, toks: &[FlatTok], accept: impl FnMut(&Path) -> bool) -> Option<locate::Slice> {
        let dir = self.crate_dir.as_deref()?;
        locate::Search {
            dir,
            entry_names: self.entry_names,
            caps: locate::Caps::default(),
        }
        .find(toks, accept)
    }
}

/// Where each byte of a captured text sits in the input's source spelling.
enum Spelling {
    /// A raw string literal: the spelling is the text shifted by the length of
    /// the `r#…"` prefix, since nothing between the quotes is decoded.
    Shift(u32),
    /// A string literal with escapes: the spelling offset of every byte of the
    /// text, plus one entry past the end. All the bytes an escape decodes to
    /// share the offset of the backslash it started at.
    Table(Vec<u32>),
}

impl Spelling {
    /// The spelling range a text range came from.
    fn range(&self, start: u32, end: u32) -> Option<(u32, u32)> {
        match self {
            Spelling::Shift(shift) => Some((shift + start, shift + end)),
            Spelling::Table(table) => {
                Some((*table.get(start as usize)?, *table.get(end as usize)?))
            }
        }
    }
}

/// A file whose spans come from a [`Subspan`] hook rather than from anchors.
struct PreciseSpans {
    hook: Subspan,
    spelling: Spelling,
}

impl PreciseSpans {
    /// The span of a file-local byte range, or [`None`] when the hook cannot
    /// produce one.
    fn span(&self, start: u32, end: u32, len: u32) -> Option<Span> {
        let mut start = start.min(len);
        let mut end = end.clamp(start, len);
        // A zero-width caret shows nothing at all, so an empty range — the
        // position of a token that is not there — grows by one byte.
        if start == end {
            if end < len {
                end += 1;
            } else if start > 0 {
                start -= 1;
            } else {
                return None;
            }
        }
        let (start, end) = self.spelling.range(start, end)?;
        if end <= start {
            return None;
        }
        self.hook.get(start, end)
    }
}

/// One Rust token's footprint inside a captured file.
#[derive(Clone, Copy)]
struct Anchor {
    start: Pos,
    end: Pos,
    span: Span,
}

/// How the C source text of a file was obtained.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputMode {
    /// The macro input was a single Rust string literal.
    StringLiteral,
    /// The text was sliced out of the caller's `.rs` file on disk.
    FileSlice,
    /// The text was rebuilt from the token spans' line/column information.
    Reconstructed,
    /// The text came from an `#include`d file.
    Included,
    /// The text is a `.c` file an `include_c99!("…")` named.
    ///
    /// Like an [`Included`](InputMode::Included) file in every way that
    /// matters to a diagnostic — there is no span inside it, so the caret goes
    /// on the macro invocation and the message carries `path:line:col` — and
    /// unlike one in being a translation unit rather than something pulled
    /// into one.
    CFile,
}

/// Everything [`SourceMap::add_file`] needs to know about a new file.
struct FileSpec {
    name: String,
    text: String,
    /// `(start, end, span)` triples in *file-local* byte offsets, sorted and
    /// non-overlapping.
    anchors: AnchorList,
    /// Span used for positions that no anchor covers.
    fallback_span: Span,
    precise: bool,
    mode: InputMode,
    rust_path: Option<String>,
    first_line: usize,
    /// Set in string-literal mode when the caller supplied a [`Subspan`].
    precise_spans: Option<PreciseSpans>,
}

/// A single file of C source together with its Rust-token anchors.
pub struct SourceFile {
    id: FileId,
    name: String,
    base: Pos,
    text: String,
    /// Byte offsets (file-local) at which each line starts.
    line_starts: Vec<u32>,
    /// Sorted, non-overlapping anchors in global coordinates.
    anchors: Vec<Anchor>,
    /// Span used for positions that no anchor covers.
    fallback_span: Span,
    /// Whether spans returned for this file actually point at the position
    /// asked for (raw-token mode) or only at the file as a whole
    /// (string-literal mode).
    precise: bool,
    /// Resolves a position inside a string literal exactly; see [`Subspan`].
    precise_spans: Option<PreciseSpans>,
    mode: InputMode,
    /// The `.rs` file the invocation is written in, when the compiler knows
    /// it. This is what `__FILE__` expands to.
    rust_path: Option<String>,
    /// The 1-based line of that `.rs` file that this file's own line 1 sits
    /// on. This is what makes `__LINE__` a line number the user can find.
    first_line: usize,
}

impl SourceFile {
    /// This file's id.
    pub fn id(&self) -> FileId {
        self.id
    }

    /// A human readable name, used in diagnostics.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The global offset at which this file's text starts.
    pub fn base(&self) -> Pos {
        self.base
    }

    /// The recovered C source text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The range this file occupies in the global offset space.
    pub fn range(&self) -> SourceRange {
        SourceRange::new(self.base, self.base + self.text.len() as Pos)
    }

    /// Whether diagnostics in this file can point at an exact position.
    pub fn precise(&self) -> bool {
        self.precise
    }

    /// How this file's text was captured.
    pub fn mode(&self) -> InputMode {
        self.mode
    }

    /// Number of lines in the file (at least 1).
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// The `.rs` file this text was written in, when the compiler knows it.
    ///
    /// This is what the preprocessor's `__FILE__` expands to; it is `None`
    /// outside a real macro expansion (a unit test building a `TokenStream`
    /// from a string) and for `#include`d files, which name themselves.
    pub fn rust_path(&self) -> Option<&str> {
        self.rust_path.as_deref()
    }

    /// The 1-based line of the enclosing `.rs` file that this file's line 1
    /// sits on; 1 when nothing better is known.
    ///
    /// The preprocessor adds this to a position's own line to make `__LINE__`
    /// a number that matches what an editor shows.
    pub fn first_line(&self) -> usize {
        self.first_line
    }

    /// Whether this file is one whose diagnostics name it themselves: an
    /// `#include`d header, or the `.c` file of an `include_c99!`.
    ///
    /// Neither has a span of its own — the caret lands on the `#include` or on
    /// the macro invocation — so `file:line:column` goes into the message
    /// instead; see [`SourceMap::header_position`].
    pub fn is_included(&self) -> bool {
        matches!(self.mode, InputMode::Included | InputMode::CFile)
    }

    fn local_line_col(&self, local: u32) -> (usize, usize) {
        let local = local.min(self.text.len() as u32);
        let line = match self.line_starts.binary_search(&local) {
            Ok(i) => i,
            // `line_starts` always begins with 0, so `i` is at least 1 here.
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts[line] as usize;
        // Columns are counted in characters, the way an editor shows them.
        let col = self
            .text
            .get(line_start..local as usize)
            .map_or(0, |s| s.chars().count())
            + 1;
        (line + 1, col)
    }
}

/// Maps byte offsets back to `proc_macro2` spans.
///
/// See the [module documentation](self) for the coordinate scheme.
pub struct SourceMap {
    files: Vec<SourceFile>,
    next_base: Pos,
}

impl Default for SourceMap {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceMap {
    /// Creates an empty map.
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            next_base: 0,
        }
    }

    /// Adds a file whose positions cannot be resolved better than
    /// `spec.fallback_span`.
    fn add_file(&mut self, spec: FileSpec) -> FileId {
        let id = FileId(self.files.len() as u32);
        let base = self.next_base;
        let text = spec.text;
        let mut line_starts = vec![0u32];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i as u32 + 1);
            }
        }
        let anchors = spec
            .anchors
            .into_iter()
            .map(|(s, e, span)| Anchor {
                start: base + s,
                end: base + e,
                span,
            })
            .collect();
        // Leave a one-byte gap so that the end of one file and the start of
        // the next are never the same position.
        self.next_base = base.saturating_add(text.len() as Pos).saturating_add(1);
        self.files.push(SourceFile {
            id,
            name: spec.name,
            base,
            text,
            line_starts,
            anchors,
            fallback_span: spec.fallback_span,
            precise: spec.precise,
            precise_spans: spec.precise_spans,
            mode: spec.mode,
            rust_path: spec.rust_path,
            first_line: spec.first_line.max(1),
        });
        id
    }

    /// The offset the next file added to this map will start at.
    ///
    /// The preprocessor runs on a thread of its own, where a
    /// `proc_macro2::Span` — and therefore this map — cannot follow it, so it
    /// allocates the offsets of the files it opens itself and hands them back
    /// for [`SourceMap::add_included_file`] afterwards. This is what lets the
    /// two agree; see [`crate::analyze`].
    pub fn next_base(&self) -> Pos {
        self.next_base
    }

    /// Adds an `#include`d file to the map.
    ///
    /// The preprocessor uses this to bring headers into the same offset space
    /// as the macro body: positions inside the returned file resolve to
    /// `directive_span` — the span of the `#include` that pulled it in — and,
    /// since the file is not [`SourceFile::precise`], diagnostics inside it
    /// carry their own file, line and column in the message.
    pub fn add_included_file(
        &mut self,
        name: impl Into<String>,
        text: String,
        directive_span: Span,
    ) -> FileId {
        self.add_file(FileSpec {
            name: name.into(),
            text,
            anchors: Vec::new(),
            fallback_span: directive_span,
            precise: false,
            mode: InputMode::Included,
            rust_path: None,
            first_line: 1,
            precise_spans: None,
        })
    }

    /// All files, in insertion order.
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// Looks a file up by id.
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this map.
    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.index()]
    }

    /// Returns the file that contains `pos` (the nearest one if `pos` falls in
    /// the gap between two files).
    pub fn file_of(&self, pos: Pos) -> FileId {
        debug_assert!(!self.files.is_empty(), "source map has no files");
        let idx = self
            .files
            .partition_point(|f| f.base <= pos)
            .saturating_sub(1);
        FileId(idx as u32)
    }

    /// Whether diagnostics anywhere in the file holding `pos` can point at an
    /// exact location.
    ///
    /// String-literal mode with a [`Subspan`] hook is precise range by range
    /// rather than file by file, so a diagnostic asks [`SourceMap::is_precise_at`]
    /// instead.
    pub fn is_precise(&self, pos: Pos) -> bool {
        self.file(self.file_of(pos)).precise
    }

    /// Whether the span returned for `range` points at `range` itself.
    pub fn is_precise_at(&self, range: SourceRange) -> bool {
        self.resolve(range).1
    }

    /// The 1-based line and (character-counted) column of `pos` inside its
    /// file.
    pub fn line_col(&self, pos: Pos) -> (usize, usize) {
        let file = self.file(self.file_of(pos));
        file.local_line_col(pos.saturating_sub(file.base))
    }

    /// `(name, line, column)` when `pos` is inside an `#include`d file.
    ///
    /// A diagnostic there is reported *at* the `#include` directive — that is
    /// the only place a procedural macro can point at — so the position inside
    /// the header travels in the message text instead, as
    /// `header.h:12:5: message`.
    pub fn header_position(&self, pos: Pos) -> Option<(&str, usize, usize)> {
        let file = self.file(self.file_of(pos));
        if !file.is_included() {
            return None;
        }
        let (line, column) = file.local_line_col(pos.saturating_sub(file.base));
        Some((&file.name, line, column))
    }

    /// The line number `pos` is named by in a diagnostic note, counted the way
    /// `__LINE__` counts: a line of the enclosing `.rs` file for the macro's
    /// own text, and a line of the header itself for an `#include`d file.
    pub fn source_line(&self, pos: Pos) -> usize {
        let file = self.file(self.file_of(pos));
        let (line, _) = file.local_line_col(pos.saturating_sub(file.base));
        file.first_line + line - 1
    }

    /// The Rust span to blame for `range`.
    ///
    /// See [`SourceMap::resolve`] for how it is found.
    pub fn span(&self, range: SourceRange) -> Span {
        self.resolve(range).0
    }

    /// The Rust span to blame for `range`, and whether it points at `range`
    /// itself rather than at something enclosing it.
    ///
    /// A file whose caller supplied a [`Subspan`] hook asks it first: that is
    /// string-literal mode under the `nightly` feature, where the span can
    /// point at the exact bytes inside the literal.
    ///
    /// Otherwise the resolution order is: the anchor containing `range.start`,
    /// then the first anchor overlapping `range`, then the anchor nearest to
    /// `range.start`, then the file's fallback span.
    /// `proc_macro::Span::join` is unstable on every channel, so a range
    /// spanning several tokens resolves to its first token rather than to the
    /// whole range.
    pub fn resolve(&self, range: SourceRange) -> (Span, bool) {
        if self.files.is_empty() {
            return (Span::call_site(), false);
        }
        let file = self.file(self.file_of(range.start));
        if let Some(precise) = &file.precise_spans
            && let Some(span) = precise.span(
                range.start.saturating_sub(file.base),
                range.end.saturating_sub(file.base),
                file.text.len() as Pos,
            )
        {
            return (span, true);
        }
        (self.anchored_span(file, range), file.precise)
    }

    /// The span the anchors of `file` give `range`.
    fn anchored_span(&self, file: &SourceFile, range: SourceRange) -> Span {
        if file.anchors.is_empty() {
            return file.fallback_span;
        }
        let idx = file.anchors.partition_point(|a| a.start <= range.start);
        if idx > 0 {
            let a = &file.anchors[idx - 1];
            if a.end > range.start {
                return a.span;
            }
        }
        let before = idx.checked_sub(1).map(|i| &file.anchors[i]);
        let after = file.anchors.get(idx);
        // An anchor that starts inside the range still describes it well.
        if let Some(a) = after
            && a.start < range.end
        {
            return a.span;
        }
        match (before, after) {
            (Some(b), Some(a)) => {
                if range.start - b.end <= a.start - range.start {
                    b.span
                } else {
                    a.span
                }
            }
            (Some(b), None) => b.span,
            (None, Some(a)) => a.span,
            (None, None) => file.fallback_span,
        }
    }
}

/// The captured macro input: a [`SourceMap`] plus the id of the root file.
pub struct Source {
    /// All source files seen so far.
    pub map: SourceMap,
    /// The file holding the macro's own C text.
    pub root: FileId,
    /// How the root file was captured.
    pub mode: InputMode,
    /// A hash identifying this invocation; see [`Source::unit_id`].
    unit_id: u64,
}

impl Source {
    /// A number that identifies this macro invocation.
    ///
    /// Two `c99!` blocks in one Rust module generate items into the same
    /// namespace, so every *synthetic* name — the module holding the `extern`
    /// block, the mangling of a function-local `static`, the name given to an
    /// anonymous `struct` — has to carry something that tells the two apart.
    /// This is that something: a hash of where the invocation is (its file,
    /// line and column) together with its text, which makes it deterministic
    /// across compilations of the same code, unique between invocations, and
    /// stable enough for a snapshot test to record.
    pub fn unit_id(&self) -> u64 {
        self.unit_id
    }
}

impl Source {
    /// The root file's C text.
    pub fn text(&self) -> &str {
        self.map.file(self.root).text()
    }

    /// The global offset of the root file's first byte.
    pub fn base(&self) -> Pos {
        self.map.file(self.root).base()
    }

    /// The range covering the whole root file.
    pub fn root_range(&self) -> SourceRange {
        self.map.file(self.root).range()
    }
}

/// Recovers the C source text of a macro invocation.
///
/// Never fails outright: a malformed string literal produces an empty source
/// file plus a diagnostic, so that the rest of the pipeline can run normally.
pub fn capture(input: TokenStream, diags: &mut Diagnostics) -> Source {
    capture_with(input, diags, &Origin::unknown())
}

/// Recovers the C source text of a macro invocation, with everything the host
/// can say about the invocation itself.
///
/// [`Origin`] is what the two strategies that need more than the tokens are
/// given: the [`Subspan`] hook that lets a diagnostic land inside a string
/// literal, and the crate directory the search walks when the
/// host reports no positions at all. [`capture`] is this with an origin that
/// says nothing.
pub fn capture_with(input: TokenStream, diags: &mut Diagnostics, origin: &Origin) -> Source {
    let trees: Vec<TokenTree> = input.into_iter().collect();

    // --- string-literal mode -------------------------------------------------
    if trees.len() == 1
        && let TokenTree::Literal(lit) = &trees[0]
    {
        let repr = lit.to_string();
        if is_string_literal(&repr) {
            let span = lit.span();
            // The hook measures offsets in the literal's spelling, so it can
            // only be trusted if the spelling we decoded is the source itself.
            let hook = origin
                .subspan
                .clone()
                .filter(|_| span.source_text().is_none_or(|text| text == repr));
            let want_spelling = hook.is_some();
            let (text, spelling, error) = match decode_string_literal(&repr, want_spelling) {
                Ok((text, spelling)) => (text, spelling, None),
                Err(msg) => (String::new(), None, Some(msg)),
            };
            let precise_spans = match (hook, spelling) {
                (Some(hook), Some(spelling)) => Some(PreciseSpans { hook, spelling }),
                _ => None,
            };
            // The text is complete either way; what the `.rs` file adds is its
            // own name — which `__FILE__` expands to and a quoted `#include`
            // searches beside — and the line the literal starts on. The
            // compiler normally says both; where it says nothing, the
            // invocation is looked for in the crate's sources like any other.
            let mut written_in = rust_path_of(span).map(|path| (path, span.start()));
            if written_in.is_none() {
                let mut toks = Vec::new();
                flatten(vec![trees[0].clone()], &mut toks);
                written_in = origin.search(&toks, |_| true).map(|slice| {
                    (
                        slice.path.display().to_string(),
                        LineColumn {
                            line: slice.line,
                            column: slice.column,
                        },
                    )
                });
            }
            let (rust_path, at) = match written_in {
                Some((path, at)) => (Some(path), at),
                None => (None, span.start()),
            };
            let mut map = SourceMap::new();
            let root = map.add_file(FileSpec {
                name: "<c99! string literal>".to_owned(),
                text,
                anchors: Vec::new(),
                fallback_span: span,
                precise: false,
                precise_spans,
                mode: InputMode::StringLiteral,
                rust_path: rust_path.clone(),
                // The C text starts just after the literal's opening quote, so
                // its first line is the line the literal starts on.
                first_line: at.line,
            });
            let unit_id = unit_id_of(rust_path.as_deref(), at, map.file(root).text());
            let source = Source {
                map,
                root,
                mode: InputMode::StringLiteral,
                unit_id,
            };
            if let Some(msg) = error {
                diags.error(SourceRange::at(source.base()), msg);
            }
            return source;
        }
    }

    // --- raw-token mode ------------------------------------------------------
    let mut toks = Vec::new();
    flatten(trees, &mut toks);
    let fallback_span = toks.first().map_or_else(Span::call_site, |t| t.span);
    // Decided once for the whole stream rather than token by token, because a
    // host that gives no positions gives none to any token; see
    // [`positions_are_usable`].
    let positioned = positions_are_usable(&toks);

    // The `.rs` file this text is written in: sliced at the positions the
    // compiler gave, or — where it gave none at all — found by searching the
    // crate's sources for an invocation these very tokens spell out. The search
    // is the fallback of a fallback: it cannot run while there is a position to
    // be had, so a normal build never reaches it.
    let located = capture_file_slice(&toks).or_else(|| {
        if positioned {
            return None;
        }
        origin.search(&toks, |_| true).map(Located::from)
    });

    if let Some(located) = located {
        let name = located.path.display().to_string();
        let at = LineColumn {
            line: located.line,
            column: located.column,
        };
        let mut map = SourceMap::new();
        let root = map.add_file(FileSpec {
            rust_path: Some(name.clone()),
            name,
            text: located.text,
            anchors: located.anchors,
            fallback_span,
            precise: true,
            precise_spans: None,
            mode: InputMode::FileSlice,
            first_line: located.line,
        });
        let unit_id = unit_id_of(
            Some(map.file(root).rust_path().expect("just set")),
            at,
            map.file(root).text(),
        );
        return Source {
            map,
            root,
            mode: InputMode::FileSlice,
            unit_id,
        };
    }

    let rebuilt = if positioned {
        reconstruct(&toks)
    } else {
        reconstruct_from_tokens(&toks, origin.entry_name())
    };
    // A directive whose end the tokens do not give away stops the rebuild: the
    // caret goes on its `#`, which is the one position the host does resolve.
    let fallback_span = rebuilt
        .blocked
        .as_ref()
        .map_or(fallback_span, |blocked| blocked.span);
    let first_line = toks.first().map_or(1, |t| t.span.start().line);
    let mut map = SourceMap::new();
    let root = map.add_file(FileSpec {
        name: "<c99! macro input>".to_owned(),
        text: rebuilt.text,
        anchors: rebuilt.anchors,
        fallback_span,
        precise: true,
        precise_spans: None,
        mode: InputMode::Reconstructed,
        rust_path: rust_path_of(fallback_span),
        first_line,
    });
    let unit_id = unit_id_of(
        map.file(root).rust_path(),
        fallback_span.start(),
        map.file(root).text(),
    );
    let source = Source {
        map,
        root,
        mode: InputMode::Reconstructed,
        unit_id,
    };
    if let Some(blocked) = rebuilt.blocked {
        let mut diag = Diagnostic::error(SourceRange::at(source.base()), blocked.message);
        for note in blocked.notes {
            diag = diag.with_note(note);
        }
        diags.push(diag);
    }
    source
}

/// The directory of the `.rs` file an invocation whose whole input is `input` is
/// written in, found by searching the crate's sources.
///
/// This is what `include_c99!("…")` needs where the compiler gives no position:
/// a relative path is resolved against the directory of the `.rs` file the macro
/// is written in, and that directory has to come from somewhere. `accept` is
/// asked about each candidate directory, so that of several invocations spelled
/// the same way the one whose directory really holds the file wins; [`None`] when
/// nothing can be proved, and the caller falls back to `CARGO_MANIFEST_DIR`.
pub fn invocation_directory(
    input: &TokenTree,
    origin: &Origin,
    mut accept: impl FnMut(&Path) -> bool,
) -> Option<PathBuf> {
    let mut toks = Vec::new();
    flatten(vec![input.clone()], &mut toks);
    let slice = origin.search(&toks, |path| path.parent().is_some_and(&mut accept))?;
    slice.path.parent().map(Path::to_path_buf)
}

/// Makes a `.c` file read from disk the source of a translation unit.
///
/// This is what [`include_c99!`](crate::expand_include) captures instead of a
/// token stream: there is no C in the `.rs` file at all, so there is nothing to
/// slice and nothing to point a span into. The file becomes the map's root,
/// named by its own path, and every position in it resolves to `span` — the
/// macro invocation, which is the only place in the `.rs` file a caret can go.
/// Being [`InputMode::CFile`] is what puts `file.c:12:5: ` in front of the
/// message, exactly as a header's position travels there.
///
/// `name` is how diagnostics write the path — relative to the working
/// directory wherever it can be, since an absolute one differs between two
/// machines — and is also what `__FILE__` expands to and what the file's own
/// `#include "…"` searches beside.
pub fn capture_c_file(name: String, text: String, span: Span) -> Source {
    let mut map = SourceMap::new();
    let unit_id = unit_id_of(rust_path_of(span).as_deref(), span.start(), &text);
    let root = map.add_file(FileSpec {
        rust_path: Some(name.clone()),
        name,
        text,
        anchors: Vec::new(),
        fallback_span: span,
        precise: false,
        precise_spans: None,
        mode: InputMode::CFile,
        // Its own lines: `__LINE__` in a `.c` file names a line of that file.
        first_line: 1,
    });
    Source {
        map,
        root,
        mode: InputMode::CFile,
        unit_id,
    }
}

// ---------------------------------------------------------------------------
// unit identity
// ---------------------------------------------------------------------------

/// The path of the `.rs` file `span` points into, when the compiler knows it.
fn rust_path_of(span: Span) -> Option<String> {
    span.local_file().map(|p| p.display().to_string())
}

/// Hashes where an invocation is and what it says into a number that
/// distinguishes it from every other invocation in the crate.
///
/// The position alone would do in a real expansion, but the file is not always
/// known (macro-generated input, some IDE contexts, unit tests that build a
/// `TokenStream` from a string), and a process-wide counter would make the
/// generated names depend on compilation order — which would in turn make
/// snapshot tests and incremental rebuilds unstable. Hashing the text as well
/// keeps the result deterministic in every context.
///
/// `path` and `at` are where the C text was written, whether the compiler said so
/// or the search worked it out — and the two have to agree, or the names an IDE's
/// expansion generates would differ from the names the build generates for the
/// same code.
fn unit_id_of(path: Option<&str>, at: LineColumn, text: &str) -> u64 {
    let mut hash = FNV_OFFSET;
    if let Some(path) = path {
        hash = fnv(hash, path.as_bytes());
    }
    hash = fnv(hash, &(at.line as u64).to_le_bytes());
    hash = fnv(hash, &(at.column as u64).to_le_bytes());
    fnv(hash, text.as_bytes())
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a, which is short enough to keep here and has no business being a
/// dependency.
fn fnv(mut hash: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// string literals
// ---------------------------------------------------------------------------

fn is_string_literal(repr: &str) -> bool {
    repr.starts_with('"') || repr.starts_with("r\"") || repr.starts_with("r#")
}

/// The text a `Literal` denotes, when it is a plain or raw string literal.
///
/// [`crate::expand_include`] is what needs it: the argument of
/// `include_c99!("…")` is a path, and a path with a `\` in it has to be the
/// one the user wrote rather than the escape sequence it is spelled as.
/// Anything that is not a string literal — a byte string, a number, an
/// identifier — answers [`None`].
pub fn string_literal_value(literal: &proc_macro2::Literal) -> Option<String> {
    string_literal_text(&literal.to_string())
}

/// The text a string literal's *spelling* denotes, for a caller that has the
/// spelling rather than the token.
///
/// The [search](self) is what needs it: a `#include "point.h"` in the input is a
/// `#`, an identifier and a literal, and the header's name is what that literal
/// says — not how it is written.
pub(crate) fn string_literal_text(repr: &str) -> Option<String> {
    if !is_string_literal(repr) {
        return None;
    }
    decode_string_literal(repr, false)
        .ok()
        .map(|(text, _)| text)
}

/// Unescapes a Rust string literal (plain or raw) into the text it denotes.
///
/// With `spelling` set, it also records where every byte of the result was
/// written, which is what a [`Subspan`] hook needs; see [`Spelling`].
fn decode_string_literal(repr: &str, spelling: bool) -> Result<(String, Option<Spelling>), String> {
    const BAD: &str = "cannot decode this string literal; expected a plain or raw string literal";

    if let Some(rest) = repr.strip_prefix('r') {
        let hashes = rest.bytes().take_while(|b| *b == b'#').count();
        let body = &rest[hashes..];
        if !body.starts_with('"') {
            return Err(BAD.to_owned());
        }
        let inner = &body[1..];
        let mut closing = String::with_capacity(1 + hashes);
        closing.push('"');
        closing.extend(std::iter::repeat_n('#', hashes));
        let text = inner
            .strip_suffix(&closing)
            .ok_or_else(|| BAD.to_owned())?
            .to_owned();
        // Nothing between the quotes is decoded, so the whole text sits at a
        // constant distance from the start of `r#…"`.
        let prefix = (repr.len() - inner.len()) as u32;
        return Ok((text, spelling.then_some(Spelling::Shift(prefix))));
    }

    let inner = repr
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| BAD.to_owned())?;

    let mut out = String::with_capacity(inner.len());
    let mut table: Vec<u32> = Vec::new();
    // Every char of `out`, tagged with where in `repr` it was written.
    let mut push = |out: &mut String, c: char, at: usize| {
        if spelling {
            let mut buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut buf).len();
            table.extend(std::iter::repeat_n(at as u32, encoded));
        }
        out.push(c);
    };
    // The opening quote is one byte, so an offset into `inner` is one less
    // than the same offset into `repr`.
    let mut chars = inner.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let at = index + 1;
        if c != '\\' {
            push(&mut out, c, at);
            continue;
        }
        let Some((_, e)) = chars.next() else {
            return Err(BAD.to_owned());
        };
        match e {
            'n' => push(&mut out, '\n', at),
            'r' => push(&mut out, '\r', at),
            't' => push(&mut out, '\t', at),
            '0' => push(&mut out, '\0', at),
            '\\' => push(&mut out, '\\', at),
            '\'' => push(&mut out, '\'', at),
            '"' => push(&mut out, '"', at),
            'x' => {
                let mut v = 0u32;
                for _ in 0..2 {
                    let Some(d) = chars.next().and_then(|(_, c)| c.to_digit(16)) else {
                        return Err(BAD.to_owned());
                    };
                    v = v * 16 + d;
                }
                match char::from_u32(v) {
                    Some(c) => push(&mut out, c, at),
                    None => return Err(BAD.to_owned()),
                }
            }
            'u' => {
                if chars.next().map(|(_, c)| c) != Some('{') {
                    return Err(BAD.to_owned());
                }
                let mut v = 0u32;
                loop {
                    match chars.next().map(|(_, c)| c) {
                        Some('}') => break,
                        Some('_') => continue,
                        Some(c) => match c.to_digit(16) {
                            Some(d) => v = v.saturating_mul(16).saturating_add(d),
                            None => return Err(BAD.to_owned()),
                        },
                        None => return Err(BAD.to_owned()),
                    }
                }
                match char::from_u32(v) {
                    Some(c) => push(&mut out, c, at),
                    None => return Err(BAD.to_owned()),
                }
            }
            '\n' => {
                // Rust's string continuation: skip leading whitespace. It
                // produces nothing, so it takes no place in the table either.
                while matches!(chars.peek(), Some((_, ' ' | '\t' | '\n' | '\r'))) {
                    chars.next();
                }
            }
            _ => return Err(BAD.to_owned()),
        }
    }
    let spelling = spelling.then(|| {
        // One past the last byte: the closing quote, which is where a range
        // ending at the end of the text stops.
        table.push((repr.len() - 1) as u32);
        Spelling::Table(table)
    });
    Ok((out, spelling))
}

// ---------------------------------------------------------------------------
// raw tokens
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlatKind {
    Ident,
    Literal,
    Punct,
    Delimiter,
}

/// One token of the input, flattened out of its groups.
pub(crate) struct FlatTok {
    span: Span,
    text: String,
    /// `true` when `text` came from [`Span::source_text`] and is therefore the
    /// verbatim source spelling of the whole span.
    exact: bool,
    kind: FlatKind,
    /// Whether the token after this one was written immediately after it, with
    /// nothing at all in between.
    ///
    /// Only a `Punct` knows — that is `Spacing::Joint`, and it is how one C
    /// operator made of several of them is told from several operators: `-` `>`
    /// jointly is `->`, and `-` `-` apart is two unary minuses. A host that
    /// reports no positions still reports this, which is what makes a text
    /// rebuilt from the tokens alone come out right.
    joint: bool,
}

impl FlatTok {
    fn new(span: Span, fallback: String, kind: FlatKind, joint: bool) -> Self {
        match span.source_text() {
            Some(text) => FlatTok {
                span,
                text,
                exact: true,
                kind,
                joint,
            },
            None => FlatTok {
                span,
                text: fallback,
                exact: false,
                kind,
                joint,
            },
        }
    }

    /// Where this token was written, as far as the host will say.
    pub(crate) fn span(&self) -> Span {
        self.span
    }

    /// The token's spelling: its source text where the host gives one, and
    /// otherwise what the token itself says it is — a single character for a
    /// `Punct`, the bracket for a delimiter, `Literal::to_string` for a literal.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Which kind of token this is.
    pub(crate) fn kind(&self) -> FlatKind {
        self.kind
    }
}

/// Whether `tok` was already written out by `prev`.
///
/// Several `Punct`s may be handed over sharing the span — and therefore the
/// source text — of the one multi-character operator they make up, in which case
/// that text belongs to all of them and must be used once.
///
/// Only a token whose text *is* that source text can be dropped this way. With
/// the per-character fallback text, which is all a host reporting no source text
/// gives, every token carries its own single character and dropping one would
/// lose it — and under `rust-analyzer`, where every span equals every other,
/// dropping on the span alone would lose all but the first token of the unit.
pub(crate) fn shares_previous_span(prev: &FlatTok, tok: &FlatTok) -> bool {
    tok.exact && at_same_position(prev.span, tok.span)
}

/// Whether two spans start and end at the same line and column.
fn at_same_position(a: Span, b: Span) -> bool {
    let (a_start, a_end) = (a.start(), a.end());
    let (b_start, b_end) = (b.start(), b.end());
    (a_start.line, a_start.column, a_end.line, a_end.column)
        == (b_start.line, b_start.column, b_end.line, b_end.column)
}

/// Whether the host gave these tokens positions that mean anything.
///
/// Decided once for the whole stream, because a host either gives positions or
/// does not: `rust-analyzer` reports line 1, column 0 as both the start and the
/// end of every token alike, so a stream in which nothing sits anywhere but
/// where its first token does carries no positions at all. A single token is no
/// evidence either way, and both strategies write it identically.
fn positions_are_usable(toks: &[FlatTok]) -> bool {
    let Some(first) = toks.first() else {
        return false;
    };
    toks.iter().any(|t| !at_same_position(first.span, t.span))
}

fn flatten(trees: Vec<TokenTree>, out: &mut Vec<FlatTok>) {
    for tt in trees {
        match tt {
            TokenTree::Group(g) => {
                let (open, close) = match g.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    // Invisible groups have no source text of their own.
                    Delimiter::None => {
                        flatten(g.stream().into_iter().collect(), out);
                        continue;
                    }
                };
                out.push(FlatTok::new(
                    g.span_open(),
                    open.to_owned(),
                    FlatKind::Delimiter,
                    false,
                ));
                flatten(g.stream().into_iter().collect(), out);
                out.push(FlatTok::new(
                    g.span_close(),
                    close.to_owned(),
                    FlatKind::Delimiter,
                    false,
                ));
            }
            TokenTree::Ident(i) => {
                out.push(FlatTok::new(
                    i.span(),
                    i.to_string(),
                    FlatKind::Ident,
                    false,
                ));
            }
            TokenTree::Punct(p) => {
                let joint = p.spacing() == Spacing::Joint;
                out.push(FlatTok::new(
                    p.span(),
                    p.as_char().to_string(),
                    FlatKind::Punct,
                    joint,
                ));
            }
            TokenTree::Literal(l) => {
                out.push(FlatTok::new(
                    l.span(),
                    l.to_string(),
                    FlatKind::Literal,
                    false,
                ));
            }
        }
    }
}

/// The flattened tokens of an input stream, which is what the search in
/// `locate.rs` matches a candidate body against.
#[cfg(test)]
pub(crate) fn flat_tokens(input: TokenStream) -> Vec<FlatTok> {
    let mut toks = Vec::new();
    flatten(input.into_iter().collect(), &mut toks);
    toks
}

/// Byte offsets of the start of every line of a string.
struct LineIndex {
    line_starts: Vec<usize>,
    len: usize,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            line_starts,
            len: text.len(),
        }
    }

    /// Converts a 1-based line / 0-based character column into a byte offset.
    ///
    /// `proc_macro2` counts columns in `char`s, so this is where non-ASCII
    /// source (a Japanese comment, say) would otherwise go wrong.
    fn offset(&self, text: &str, lc: LineColumn) -> Option<usize> {
        let line = lc.line.checked_sub(1)?;
        let start = *self.line_starts.get(line)?;
        let end = self.line_starts.get(line + 1).copied().unwrap_or(self.len);
        let slice = text.get(start..end)?;
        let mut count = 0usize;
        for (i, _) in slice.char_indices() {
            if count == lc.column {
                return Some(start + i);
            }
            count += 1;
        }
        (count == lc.column).then_some(end)
    }
}

/// The `.rs` file an invocation is written in, its text, and where in the file
/// that text begins.
///
/// What both strategies that answer with a real file produce: the primary one
/// from the positions the compiler gave, the search in `locate.rs` by proving
/// which text it is.
struct Located {
    path: PathBuf,
    text: String,
    anchors: AnchorList,
    /// The 1-based line of the `.rs` file the text starts on.
    line: usize,
    /// The 0-based column, in characters, the text starts at.
    column: usize,
}

impl From<locate::Slice> for Located {
    fn from(slice: locate::Slice) -> Self {
        Self {
            path: slice.path,
            text: slice.text,
            anchors: slice.anchors,
            line: slice.line,
            column: slice.column,
        }
    }
}

/// Primary raw-token strategy: slice the caller's `.rs` file.
///
/// Returns `None` (so that the caller falls back to the search and then to
/// rebuilding the text from the tokens) whenever anything at all looks
/// inconsistent, because a wrong slice would produce silently wrong code rather
/// than a diagnostic.
fn capture_file_slice(toks: &[FlatTok]) -> Option<Located> {
    let first = toks.first()?;
    let last = toks.last()?;
    let path = first.span.local_file()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let index = LineIndex::new(&content);

    let start = index.offset(&content, first.span.start())?;
    let end = index.offset(&content, last.span.end())?;
    if end < start || end - start > u32::MAX as usize {
        return None;
    }

    let mut anchors = Vec::with_capacity(toks.len());
    let mut prev_end = start;
    for t in toks {
        let s = index.offset(&content, t.span.start())?;
        let e = index.offset(&content, t.span.end())?;
        if s < start || e > end || e < s {
            return None;
        }
        let slice = content.get(s..e)?;
        if t.exact {
            if slice != t.text {
                return None;
            }
        } else {
            match t.kind {
                FlatKind::Ident | FlatKind::Literal => {
                    if slice != t.text {
                        return None;
                    }
                }
                // A multi-character operator may be delivered as several
                // `Punct`s sharing one span, so only require containment.
                FlatKind::Punct | FlatKind::Delimiter => {
                    if !slice.contains(&t.text) {
                        return None;
                    }
                }
            }
        }
        if s < prev_end {
            continue;
        }
        prev_end = e;
        anchors.push(((s - start) as u32, (e - start) as u32, t.span));
    }

    let at = first.span.start();
    Some(Located {
        path,
        text: content[start..end].to_owned(),
        anchors,
        line: at.line,
        column: at.column,
    })
}

/// A text rebuilt from the tokens, with the anchors that map it back to them.
struct Rebuilt {
    text: String,
    anchors: AnchorList,
    /// Set when a preprocessing directive stopped the rebuild; see
    /// [`Blocked`].
    blocked: Option<Blocked>,
}

/// A directive whose end the tokens alone do not give away, and the one
/// diagnostic it earns.
///
/// `#define X 1` is a *line*, and a token stream with no positions keeps no
/// lines: there is no way to tell the `1` that ends the replacement list from
/// the `int` that begins the next line. Guessing would silently mistranslate,
/// so the unit is refused with a message that says what is missing and how to
/// get it, and the text comes out empty so that this is the *only* thing
/// reported.
struct Blocked {
    /// The `#` the message is reported at. Under a host that gives no positions
    /// this is still a span it can resolve — that is how it maps a
    /// `compile_error!` back to the source — so the caret lands on the
    /// directive even though nothing could be read from the span itself.
    span: Span,
    message: String,
    notes: Vec<String>,
}

/// Fallback raw-token strategy: rebuild the text from token positions.
fn reconstruct(toks: &[FlatTok]) -> Rebuilt {
    let mut out = String::new();
    let mut anchors = Vec::new();
    let Some(first) = toks.first() else {
        return Rebuilt {
            text: out,
            anchors,
            blocked: None,
        };
    };
    let line_base = first.span.start().line;
    let col_base = first.span.start().column;
    let mut cur_line = 1usize;
    let mut cur_col = 0usize;
    let mut prev: Option<&FlatTok> = None;

    for t in toks {
        let s = t.span.start();
        // Several `Punct`s can share the span — and the source text — of one
        // multi-character operator (`->`, `==`, ...); write it once.
        if let Some(p) = prev
            && shares_previous_span(p, t)
        {
            continue;
        }
        prev = Some(t);

        let target_line = s.line.saturating_sub(line_base) + 1;
        let target_col = if s.line == line_base {
            s.column.saturating_sub(col_base)
        } else {
            s.column
        };

        if target_line < cur_line || (target_line == cur_line && target_col < cur_col) {
            // This token was written *before* the one in front of it, which a
            // stream assembled from more than one place can be. (A stream with
            // no positions at all never gets here: that is
            // `reconstruct_from_tokens`.) Keep the tokens; separate them with a
            // space so that they do not merge into one C token.
            if !out.is_empty() {
                out.push(' ');
                cur_col += 1;
            }
        } else {
            while cur_line < target_line {
                out.push('\n');
                cur_line += 1;
                cur_col = 0;
            }
            while cur_col < target_col {
                out.push(' ');
                cur_col += 1;
            }
        }

        let anchor_start = out.len();
        out.push_str(&t.text);
        for ch in t.text.chars() {
            if ch == '\n' {
                cur_line += 1;
                cur_col = 0;
            } else {
                cur_col += 1;
            }
        }
        anchors.push((anchor_start as u32, out.len() as u32, t.span));
    }

    Rebuilt {
        text: out,
        anchors,
        blocked: None,
    }
}

/// Last-resort raw-token strategy: rebuild the text from the tokens alone.
///
/// Where the host gives no positions there is nothing to place tokens *at*, and
/// the one thing left to get right is which of them were written together. A
/// space goes between any two tokens except after a `Punct` the next token
/// followed immediately, so `->`, `==`, `<<=`, `&&`, `++` and `...` come out as
/// the single C operators they are while `- -` stays two tokens and `a + +b`
/// stays an addition of a unary plus. Nothing is ever dropped — only a token
/// that carries another's source text may be, and a host with no positions gives
/// no source text either.
///
/// Comments, line breaks and the columns are gone for good, which costs the C
/// text nothing that is not a *directive*: those are lines. The forms whose end
/// the tokens themselves give away are written on a line of their own, and any
/// other — where the end would have to be guessed — stops the rebuild with one
/// diagnostic; see [`Blocked`] and [`directive_shape`].
fn reconstruct_from_tokens(toks: &[FlatTok], entry: &str) -> Rebuilt {
    let mut out = Builder::default();
    // Whether the token just written was a `Punct` the next one follows
    // immediately, so that the two make up one C operator.
    let mut glued = false;
    let mut prev: Option<&FlatTok> = None;
    let mut i = 0;
    while i < toks.len() {
        let tok = &toks[i];
        if let Some(p) = prev
            && shares_previous_span(p, tok)
        {
            i += 1;
            continue;
        }
        prev = Some(tok);

        if !is_hash(tok) {
            out.write(tok, !glued);
            glued = tok.kind == FlatKind::Punct && tok.joint;
            i += 1;
            continue;
        }

        // A directive: its own line, and only if the tokens say where it ends.
        let rest = &toks[i..];
        let Some(shape) = directive_shape(rest) else {
            return Rebuilt {
                text: String::new(),
                anchors: AnchorList::new(),
                blocked: Some(blocked_directive(rest, entry)),
            };
        };
        out.newline();
        out.write(&rest[0], false);
        for (n, tok) in rest[1..shape.tokens()].iter().enumerate() {
            // One space after the directive's name, and none anywhere else: a
            // header name is a single token to the preprocessor, so
            // `<` `stdio` `.` `h` `>` has to come back out as `<stdio.h>`.
            out.write(tok, n == 1);
        }
        out.newline();
        glued = false;
        i += shape.tokens();
    }
    Rebuilt {
        text: out.text,
        anchors: out.anchors,
        blocked: None,
    }
}

/// The text a token-only rebuild is writing, and where each token went.
#[derive(Default)]
struct Builder {
    text: String,
    anchors: AnchorList,
    /// Whether nothing has been written on the current line yet, in which case
    /// no separator may be.
    at_line_start: bool,
}

impl Builder {
    /// Writes one token, with a space in front of it when `space` asks for one
    /// and it is not starting a line.
    fn write(&mut self, tok: &FlatTok, space: bool) {
        if space && !self.at_line_start && !self.text.is_empty() {
            self.text.push(' ');
        }
        let start = self.text.len() as u32;
        self.text.push_str(&tok.text);
        self.anchors.push((start, self.text.len() as u32, tok.span));
        self.at_line_start = false;
    }

    /// Ends the current line, so that what comes next begins one.
    ///
    /// This is the only way a newline is ever written, because the only thing a
    /// line means here is "a directive starts here" — and the preprocessor
    /// decides that by the `#` being the first token on its line.
    fn newline(&mut self) {
        if !self.text.is_empty() && !self.at_line_start {
            self.text.push('\n');
        }
        self.at_line_start = true;
    }
}

/// Whether this token is the `#` that opens a directive.
///
/// In C a `#` can be nothing else: the two preprocessor *operators* spelled
/// with one live in a `#define` replacement list, which is a directive itself.
fn is_hash(tok: &FlatTok) -> bool {
    tok.kind == FlatKind::Punct && tok.text == "#"
}

/// A directive whose end is known from the tokens alone.
///
/// Everything a directive may be followed by is another line, and there are no
/// lines here — so a form is usable exactly when its own tokens say where it
/// stops. These do; `#define`, `#if`, `#elif`, `#error`, `#warning`, `#line`,
/// `#embed`, `#pragma` other than `once` and a `#` followed by anything but a
/// directive name do not, and are [`Blocked`].
#[derive(Clone, Copy)]
enum Shape {
    /// A `#` with nothing at all after it: the null directive.
    Null,
    /// `#` and the directive's name: `#else`, `#endif`.
    Bare,
    /// `#`, the name and one identifier: `#ifdef X`, `#undef X`, `#pragma once`.
    Word,
    /// `#include` and a quoted header name, which is one string literal.
    Quoted,
    /// `#include` and an angled header name, holding `tokens` of path.
    Angled(usize),
}

impl Shape {
    /// How many tokens the directive is, `#` included.
    fn tokens(self) -> usize {
        match self {
            Shape::Null => 1,
            Shape::Bare => 2,
            Shape::Word | Shape::Quoted => 3,
            // `#`, `include`, `<`, the path, `>`.
            Shape::Angled(tokens) => 4 + tokens,
        }
    }
}

/// The shape of the directive `toks` opens with — `toks[0]` is its `#` — or
/// [`None`] when its end cannot be known; see [`Shape`].
fn directive_shape(toks: &[FlatTok]) -> Option<Shape> {
    let Some(name) = toks.get(1) else {
        return Some(Shape::Null);
    };
    if name.kind != FlatKind::Ident {
        return None;
    }
    let operand = toks.get(2);
    let is_ident = |tok: Option<&FlatTok>, text: &str| {
        tok.is_some_and(|t| t.kind == FlatKind::Ident && (text.is_empty() || t.text == text))
    };
    match name.text.as_str() {
        "include" | "include_next" => {
            // A quoted name is one literal; an angled one runs to the `>`. A
            // name a macro stands for (`#include HEADER`) does not say where the
            // line ends, and neither does a raw string literal — which a C
            // preprocessor could not read anyway.
            if operand.is_some_and(|t| t.kind == FlatKind::Literal && t.text.starts_with('"')) {
                return Some(Shape::Quoted);
            }
            if operand.is_some_and(|t| t.kind == FlatKind::Punct && t.text == "<") {
                let path = toks
                    .get(3..)?
                    .iter()
                    .position(|t| t.kind == FlatKind::Punct && t.text == ">")?;
                return Some(Shape::Angled(path));
            }
            None
        }
        // `#elifdef` and `#elifndef` are C23's, and take one name like the rest.
        "ifdef" | "ifndef" | "undef" | "elifdef" | "elifndef" => {
            is_ident(operand, "").then_some(Shape::Word)
        }
        "else" | "endif" => Some(Shape::Bare),
        // `#pragma once` is the only pragma whose operands are a fixed set.
        "pragma" => is_ident(operand, "once").then_some(Shape::Word),
        _ => None,
    }
}

/// The one diagnostic a directive that cannot be delimited earns.
///
/// It has three jobs: say what is missing (the positions), say why it is likely
/// to be missing (an editor looking at an unsaved file, or input another macro
/// built), and say what to do about it — both ways out, since saving the file is
/// not always the one the reader wants.
fn blocked_directive(toks: &[FlatTok], entry: &str) -> Blocked {
    let what = match toks.get(1) {
        Some(name) if name.kind == FlatKind::Ident => format!("the '#{}' directive", name.text),
        _ => "this preprocessing directive".to_owned(),
    };
    Blocked {
        span: toks[0].span,
        message: format!(
            "cannot tell where {what} ends: this block's tokens carry no source positions, so its \
             text had to be rebuilt from the tokens alone — and a directive is a line, of which \
             tokens keep nothing"
        ),
        notes: vec![
            "the compiler that expanded this gave no position for any token, and the invocation \
             was not found in this crate's sources either. An editor analysing a file you have \
             not saved yet looks exactly like that — rust-analyzer gives a procedural macro no \
             positions, and what is on disk no longer matches what you are typing — and so does \
             input another macro built."
                .to_owned(),
            format!(
                "save the file and this block is read from disk again, directives and all; or \
                 write it as a string literal — {entry}! {{ r#\"…\"# }} — which needs no \
                 positions at all"
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The decoded text of a literal, ignoring the spelling map.
    fn unescape(repr: &str) -> Result<String, String> {
        decode_string_literal(repr, false).map(|(text, _)| text)
    }

    /// The spelling range a text range maps back to.
    fn spelling_of(repr: &str, start: u32, end: u32) -> Option<(u32, u32)> {
        let (_, spelling) = decode_string_literal(repr, true).unwrap();
        spelling.unwrap().range(start, end)
    }

    #[test]
    fn unescape_plain() {
        assert_eq!(unescape(r#""a\nb""#).unwrap(), "a\nb");
        assert_eq!(unescape(r#""\x41\u{3042}""#).unwrap(), "A\u{3042}");
    }

    #[test]
    fn the_spelling_map_finds_the_bytes_a_character_was_written_as() {
        // `"a\nb"`: the `b` is the third byte of the text and the fifth of the
        // spelling, because `\n` was written as two.
        assert_eq!(spelling_of(r#""a\nb""#, 2, 3), Some((4, 5)));
        // The escape itself covers both of its bytes.
        assert_eq!(spelling_of(r#""a\nb""#, 1, 2), Some((2, 4)));
        // A raw literal is a constant shift past `r##"`.
        assert_eq!(spelling_of(r####"r##"abc"##"####, 1, 2), Some((5, 6)));
        // The end of the text is the closing quote.
        assert_eq!(spelling_of(r#""ab""#, 0, 2), Some((1, 3)));
    }

    #[test]
    fn several_files_share_one_offset_space() {
        // What `#include` will need: extra files slot into the same
        // coordinate system without changing any signature downstream.
        let mut map = SourceMap::new();
        let root = map.add_file(FileSpec {
            name: "<input>".to_owned(),
            text: "int x;\n".to_owned(),
            anchors: Vec::new(),
            fallback_span: Span::call_site(),
            precise: true,
            precise_spans: None,
            mode: InputMode::Reconstructed,
            rust_path: None,
            first_line: 1,
        });
        let header = map.add_included_file(
            "stdio.h",
            "int printf();\nint puts();\n".to_owned(),
            Span::call_site(),
        );
        assert_ne!(root, header);
        assert_eq!(map.file_of(map.file(root).base()), root);
        assert_eq!(map.file_of(map.file(header).base()), header);
        assert_eq!(map.file_of(map.file(header).base() + 14), header);
        assert_eq!(map.line_col(map.file(header).base() + 14), (2, 1));
        assert!(map.is_precise(map.file(root).base()));
        assert!(!map.is_precise(map.file(header).base()));
    }

    #[test]
    fn unescape_raw() {
        assert_eq!(unescape(r####"r#"a\nb"#"####).unwrap(), r"a\nb");
        assert_eq!(unescape(r#"r"x""#).unwrap(), "x");
    }
}
