//! Finding a macro invocation in the crate's own sources.
//!
//! [Capture](crate::capture)'s primary strategy for raw-token input is to ask
//! the first token for [`Span::local_file`](proc_macro2::Span::local_file) and
//! slice that `.rs` file between the first and the last token's positions. A
//! host that answers those questions — `rustc` does — never reaches this
//! module.
//!
//! `rust-analyzer` does not answer them. Every span it hands a procedural macro
//! reports no file, no source text, and line 1 column 0 as both its start and
//! its end, for every token alike; what it does hand over is the token trees
//! themselves, spans it can map back to the source when they come out again on
//! generated tokens, and the crate's environment — `CARGO_MANIFEST_DIR`
//! included. A unit whose text is nothing but tokens can be
//! [rebuilt](crate::capture) from them, but a unit with directives cannot: the
//! preprocessor is line-oriented and there are no lines.
//!
//! The text is on disk all the same, and the macro can *prove* which text is
//! its own: search the crate's `.rs` files for an invocation whose token
//! sequence is exactly the one the macro was handed. Proving it is what makes
//! this safe — a candidate is accepted only when every token matches at the
//! cursor, in order, with nothing but whitespace and comments between them and
//! nothing but whitespace and comments left over. Where no candidate matches,
//! nothing is used and capture falls back to the tokens alone.
//!
//! # The scan
//!
//! Finding the invocation needs two things a naive `str::find` cannot do:
//!
//! * **Only code counts.** The same body written in a comment, in a string
//!   literal, or in a `#[cfg(…)]`-ed out block is a decoy, so the scan skips
//!   comments (nested, as Rust's are), string literals with their escapes, raw
//!   strings of any hash count, byte and C strings, and character literals —
//!   which in C code are `'a'`, `'\n'` and `'\''`, and which must not be
//!   confused with a lifetime or a loop label.
//! * **The body's end.** A macro body is found by matching the delimiter it
//!   opens with, which is bracket counting over that same skipping scan.
//!
//! Since the search is the fallback of a fallback, it also has to be impossible
//! for it to cost anything in a normal build: capture only reaches it when the
//! host gave no positions at all, and the work it may do then is capped in
//! every direction — see [`Caps`].
//!
//! # Several candidates
//!
//! Two invocations with the same tokens are common rather than exotic — a test
//! suite repeats small blocks — and under a host with no positions there is
//! nothing in the input that says which of them is being expanded. So the choice
//! has to be deliberate:
//!
//! * Every verified candidate spells out the same tokens, so the *C* is the same
//!   whichever is taken. What can differ is only what is derived from the place
//!   it was written: the directory a quoted `#include "…"` is looked for beside,
//!   `__FILE__` and `__LINE__`, the path `include_str!` tracks for rebuilds, the
//!   unit id and therefore the name of the module the expansion goes into — and,
//!   pathologically, the line structure around a directive, since `#define A 1`
//!   on a line of its own and the same tokens run together mean different
//!   things.
//! * The candidates are tried in **sorted path order**, and within a file the
//!   sites whose name was asked for before the rest, each by where its body
//!   starts. The first whose directory holds every header the unit
//!   `#include "…"`s by name is taken; failing that, the first of all of them.
//!   That is what makes two copies of a block in two directories, only one of
//!   which has the header beside it, both expand correctly — and a header that
//!   comes from an include path instead is beside *none* of the candidates, which
//!   is why the test only ever prefers and never refuses.
//! * Candidates that disagree about anything else, the line structure included,
//!   are not compared and nothing is reported: a `__LINE__` that is off in an
//!   editor is better than a red squiggle under code that compiles.
//!
//! None of this can reach a build. `rustc` gives the positions, so the file and
//! the place in it are known exactly and this module is never entered.

use std::path::{Path, PathBuf};

use crate::capture::{AnchorList, FlatKind, FlatTok, shares_previous_span};

/// How much of the crate's sources one search may read.
///
/// A search that cannot find its invocation walks the whole crate, and a crate
/// is whatever the user has: a `.rs` file a code generator wrote, a vendored
/// tree, a directory of test fixtures. None of that may turn a macro expansion
/// into something an editor waits for, so every dimension of the walk has a
/// ceiling and passing one makes the search give up quietly — which is not a
/// failure, only the fallback below it.
///
/// The defaults are far above any hand-written crate and far below anything
/// that would be felt: a few thousand files, a few megabytes each, thirty-two
/// megabytes in all.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Caps {
    /// The greatest number of `.rs` files considered.
    pub files: usize,
    /// The largest file read; a bigger one is skipped rather than truncated.
    pub file_bytes: u64,
    /// The total number of bytes read, across all files.
    pub total_bytes: u64,
    /// The greatest number of directories opened.
    pub dirs: usize,
    /// The deepest directory nesting walked, counting the crate root as 0.
    pub depth: usize,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            files: 4096,
            file_bytes: 4 << 20,
            total_bytes: 32 << 20,
            dirs: 4096,
            depth: 32,
        }
    }
}

/// One search over one crate directory.
pub(crate) struct Search<'a> {
    /// The directory to walk, which is what `CARGO_MANIFEST_DIR` names.
    ///
    /// A parameter rather than a read of the environment, because the
    /// environment is process-global and this has to be testable.
    pub dir: &'a Path,
    /// The names the invocation may be written with — `c99` for a `c99!` block,
    /// `include_gnu11` for an `include_gnu11!` — which is only a way of looking
    /// at the likely candidates first. A renamed import (`use cinrs::c99 as
    /// cc;`) cannot be found by name at all, so a file in which no named site
    /// matches is searched again for *any* identifier followed by `!` and a
    /// delimiter; verification is what decides either way.
    pub entry_names: &'a [&'a str],
    /// The ceilings on the work; see [`Caps`].
    pub caps: Caps,
}

/// What a successful search found: the same three things
/// [`capture`](crate::capture) slices out of a `.rs` file when the compiler
/// says where it is, plus where in that file the text starts.
pub(crate) struct Slice {
    /// The `.rs` file the invocation is written in.
    pub path: PathBuf,
    /// The text from the first token's first byte to the last token's last,
    /// comments, whitespace and all.
    pub text: String,
    /// `(start, end, span)` per token, in offsets local to `text`.
    pub anchors: AnchorList,
    /// The 1-based line of the file that `text` starts on.
    pub line: usize,
    /// The 0-based column, counted in characters as a `Span` counts it, that
    /// `text` starts at.
    pub column: usize,
}

impl Search<'_> {
    /// Finds the invocation `toks` came from, or [`None`] when nothing in the
    /// crate's sources provably is it.
    ///
    /// `accept` is asked about the file each match was found in and can refuse
    /// it, which is how `include_c99!("…")` picks the candidate whose directory
    /// actually holds the `.c` file.
    ///
    /// Of the candidates `accept` allows, the first — in the order described
    /// [above](self#several-candidates) — whose directory holds every header the
    /// unit includes by name is the answer, and failing that the first of them.
    /// A unit that includes no header by name, which is most of them, is
    /// therefore answered by the first match and the search stops there.
    pub(crate) fn find(
        &self,
        toks: &[FlatTok],
        mut accept: impl FnMut(&Path) -> bool,
    ) -> Option<Slice> {
        if toks.is_empty() {
            return None;
        }
        // The headers this unit includes by name, which is the one thing that
        // can tell two invocations with the same tokens apart. The tokens say
        // what they are; only the candidate says whether they are there.
        let quoted = quoted_includes(toks);
        let mut first: Option<Slice> = None;
        let mut budget = self.caps.total_bytes;
        for path in self.candidate_files() {
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            let len = meta.len();
            if len > self.caps.file_bytes {
                continue;
            }
            if len > budget {
                // Out of budget: give up quietly rather than read half a file
                // and answer from it.
                break;
            }
            budget -= len;
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Some(sites) = sites(&text, self.entry_names) else {
                continue;
            };
            for site in sites {
                let Some(found) = verify(&text, site.body, toks) else {
                    continue;
                };
                if !accept(&path) {
                    continue;
                }
                let (line, column) = line_column(&text, found.start);
                let slice = Slice {
                    path: path.clone(),
                    text: text[found.start..found.end].to_owned(),
                    anchors: found.anchors,
                    line,
                    column,
                };
                if headers_exist_beside(&slice.path, &quoted) {
                    return Some(slice);
                }
                if first.is_none() {
                    first = Some(slice);
                }
            }
        }
        first
    }

    /// The `.rs` files of the crate, in the order they are tried: **sorted by
    /// path**, which is the tie-break between two files holding the same tokens.
    ///
    /// They are *collected* depth first with every directory's own entries
    /// sorted and its files before its subdirectories, so that a file at the top
    /// of the crate is reached before one buried in it — which only decides
    /// which files a [cap](Caps) leaves out, the order they are searched in being
    /// the sort below.
    ///
    /// `target` and every hidden directory are skipped, and so is everything
    /// that is not a regular `.rs` file: a symbolic link is never followed,
    /// which is also what keeps a link pointing at its own ancestor from
    /// turning the walk into a loop.
    fn candidate_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut dirs = 0usize;
        self.walk(self.dir, 0, &mut dirs, &mut out);
        out.sort();
        out
    }

    fn walk(&self, dir: &Path, depth: usize, dirs: &mut usize, out: &mut Vec<PathBuf>) {
        if depth > self.caps.depth || *dirs >= self.caps.dirs || out.len() >= self.caps.files {
            return;
        }
        *dirs += 1;
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut files = Vec::new();
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            // A name that is not UTF-8 cannot be an `.rs` file we care about,
            // and a hidden directory (`.git`, `.cargo`) holds no source of the
            // crate's own.
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if name == "target" {
                    continue;
                }
                subdirs.push(entry.path());
            } else if kind.is_file() && name.ends_with(".rs") {
                files.push(entry.path());
            }
        }
        files.sort();
        subdirs.sort();
        for file in files {
            if out.len() >= self.caps.files {
                return;
            }
            out.push(file);
        }
        for subdir in subdirs {
            self.walk(&subdir, depth + 1, dirs, out);
        }
    }
}

// ---------------------------------------------------------------------------
// telling two candidates apart
// ---------------------------------------------------------------------------

/// The headers a unit includes *by name*: `#include "point.h"`.
///
/// This is read off the tokens, which every candidate has in common, and it is
/// the one question whose answer depends on where the invocation is written: a
/// quoted header is looked for in the directory of the `.rs` file first, so a
/// copy of the block in a directory that holds the header is the copy that will
/// work.
///
/// Deliberately literal, because it decides nothing but a preference: a
/// directive whose name a macro stands for, one inside a group `#if 0` skips, and
/// an angled `<stdio.h>` are all simply not in the list. String-literal input is
/// the one case where the tokens are not the C — there the literal's own text is
/// scanned instead.
fn quoted_includes(toks: &[FlatTok]) -> Vec<String> {
    if let [only] = toks
        && only.kind() == FlatKind::Literal
        && let Some(text) = crate::capture::string_literal_text(only.text())
    {
        return quoted_includes_in_text(&text);
    }
    let mut out = Vec::new();
    for window in toks.windows(3) {
        let [hash, name, operand] = window else {
            continue;
        };
        if hash.kind() != FlatKind::Punct || hash.text() != "#" {
            continue;
        }
        if name.kind() != FlatKind::Ident || !matches!(name.text(), "include" | "include_next") {
            continue;
        }
        if operand.kind() != FlatKind::Literal {
            continue;
        }
        if let Some(header) = crate::capture::string_literal_text(operand.text()) {
            out.push(header);
        }
    }
    out
}

/// The same, read from C text rather than from tokens; see [`quoted_includes`].
fn quoted_includes_in_text(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix('#') else {
            continue;
        };
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix("include_next")
            .or_else(|| rest.strip_prefix("include"))
            .map(str::trim_start);
        let Some(rest) = rest else {
            continue;
        };
        if let Some(inner) = rest.strip_prefix('"')
            && let Some(end) = inner.find('"')
        {
            out.push(inner[..end].to_owned());
        }
    }
    out
}

/// Whether every header in `quoted` is in the directory `path` is in.
///
/// A header that is not — one the include paths or the bundled set supply — is
/// beside no candidate at all, which is why this only ever *prefers* one; see
/// [`Search::find`].
fn headers_exist_beside(path: &Path, quoted: &[String]) -> bool {
    if quoted.is_empty() {
        return true;
    }
    let Some(dir) = path.parent() else {
        return false;
    };
    quoted.iter().all(|header| dir.join(header).is_file())
}

/// The 1-based line and 0-based character column of `at` in `text`.
///
/// Columns are counted the way [`proc_macro2::Span`] counts them — in
/// characters — because this is what the unit id is built from, and the id has
/// to come out the same here as it does under a compiler that gave the position
/// itself.
fn line_column(text: &str, at: usize) -> (usize, usize) {
    let before = &text[..at];
    let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[start..].chars().count())
}

// ---------------------------------------------------------------------------
// candidate sites
// ---------------------------------------------------------------------------

/// A macro invocation found in a `.rs` file.
struct Site {
    /// The bytes strictly between the delimiters.
    body: std::ops::Range<usize>,
    /// Whether the identifier before the `!` is one of the names searched for.
    named: bool,
}

/// Every `name! (…)`, `name! […]` and `name! {…}` in `text`, in the order they
/// are tried: the ones whose name was asked for first, then the rest, each
/// group in the order the bodies start.
///
/// [`None`] means the scan lost its way and nothing in the file may be believed:
/// a delimiter that closes nothing or closes the wrong thing, an unterminated
/// comment or character literal. (An unterminated *raw string* only ends the
/// scan, since what precedes it was read correctly.) Either way the file is
/// passed over, and it does not compile as it stands anyway.
fn sites(text: &str, names: &[&str]) -> Option<Vec<Site>> {
    let bytes = text.as_bytes();
    let mut out: Vec<Site> = Vec::new();
    // The open delimiters currently entered: where the body starts, which
    // character closes it, and the name of the macro it is the body of.
    let mut open: Vec<(usize, u8, Option<bool>)> = Vec::new();
    // The identifier last read, and then whether a `!` followed it: a site is
    // `ident`, `!` and an open delimiter, with nothing but whitespace and
    // comments in between.
    let mut ident: Option<std::ops::Range<usize>> = None;
    let mut bang: Option<std::ops::Range<usize>> = None;
    let mut at = 0usize;

    while at < bytes.len() {
        let b = bytes[at];
        // Whitespace and comments separate tokens without being ones, so they
        // leave `ident` and `bang` alone.
        if b.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if b == b'/' && bytes.get(at + 1) == Some(&b'/') {
            at = bytes[at..]
                .iter()
                .position(|c| *c == b'\n')
                .map_or(bytes.len(), |i| at + i + 1);
            continue;
        }
        if b == b'/' && bytes.get(at + 1) == Some(&b'*') {
            at = block_comment_end(bytes, at)?;
            continue;
        }
        if b == b'"' {
            at = string_end(bytes, at)?;
            ident = None;
            bang = None;
            continue;
        }
        if b == b'\'' {
            at = char_or_lifetime_end(text, at)?;
            ident = None;
            bang = None;
            continue;
        }
        if is_ident_start(b) {
            let end = ident_end(bytes, at);
            let word = &text[at..end];
            // `r"…"`, `r#"…"#`, `br"…"`, `cr#"…"#`: a prefix that turns what
            // follows into a string rather than into two tokens. `r#foo` is a
            // raw *identifier* and not one of them, which is what checking for
            // the quote after the hashes is for.
            if let Some(after) = raw_string_end(bytes, end, word) {
                at = after;
                ident = None;
                bang = None;
                continue;
            }
            if matches!(word, "b" | "c") && bytes.get(end) == Some(&b'"') {
                at = string_end(bytes, end)?;
                ident = None;
                bang = None;
                continue;
            }
            ident = Some(at..end);
            bang = None;
            at = end;
            continue;
        }
        if b == b'!' {
            // `a != b` is not an invocation, so the `!` only counts while it is
            // the whole token; the `=` that follows clears it below.
            bang = ident.take();
            at += 1;
            continue;
        }
        if let Some(close) = closing_delimiter(b) {
            let named = bang
                .take()
                .map(|range| names.contains(&&text[range.clone()]));
            open.push((at + 1, close, named));
            ident = None;
            at += 1;
            continue;
        }
        if matches!(b, b')' | b']' | b'}') {
            // Rust cannot be lexed with unbalanced delimiters outside a comment
            // or a literal, so a mismatch here means the scan has lost its way
            // and nothing it goes on to find may be believed.
            let (start, close, named) = open.pop()?;
            if close != b {
                return None;
            }
            if let Some(named) = named {
                out.push(Site {
                    body: start..at,
                    named,
                });
            }
            ident = None;
            bang = None;
            at += 1;
            continue;
        }
        ident = None;
        bang = None;
        at += 1;
    }

    out.sort_by_key(|site| (!site.named, site.body.start));
    Some(out)
}

/// The delimiter that closes `b`, when `b` opens one.
fn closing_delimiter(b: u8) -> Option<u8> {
    match b {
        b'(' => Some(b')'),
        b'[' => Some(b']'),
        b'{' => Some(b'}'),
        _ => None,
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// One past the end of the identifier starting at `at`.
///
/// Every byte of a non-ASCII character is `>= 0x80`, so scanning bytes never
/// stops inside one.
fn ident_end(bytes: &[u8], at: usize) -> usize {
    let mut end = at + 1;
    while end < bytes.len() && is_ident_continue(bytes[end]) {
        end += 1;
    }
    end
}

/// One past the `*/` that closes the block comment opening at `at`.
///
/// Rust's block comments nest, so this counts them.
fn block_comment_end(bytes: &[u8], at: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = at;
    while i + 1 < bytes.len() {
        match (bytes[i], bytes[i + 1]) {
            (b'/', b'*') => {
                depth += 1;
                i += 2;
            }
            (b'*', b'/') => {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => i += 1,
        }
    }
    None
}

/// One past the `"` that closes the string literal opening at `at`, whose
/// escapes are honoured.
fn string_end(bytes: &[u8], at: usize) -> Option<usize> {
    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// One past the end of the raw string that `prefix` — the identifier ending at
/// `at` — opens, or [`None`] when it opens none.
///
/// The prefixes are `r`, `br` and `cr`; what follows is any number of `#` and
/// then a quote, and the literal ends at the first quote followed by that many
/// `#`. A prefix with no quote after its hashes is a raw identifier
/// (`r#match`), which is not a literal at all.
fn raw_string_end(bytes: &[u8], at: usize, prefix: &str) -> Option<usize> {
    if !matches!(prefix, "r" | "br" | "cr") {
        return None;
    }
    let mut i = at;
    while bytes.get(i) == Some(&b'#') {
        i += 1;
    }
    let hashes = i - at;
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    i += 1;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let after = i + 1;
            if bytes.len() >= after + hashes
                && bytes[after..after + hashes].iter().all(|b| *b == b'#')
            {
                return Some(after + hashes);
            }
        }
        i += 1;
    }
    // An unterminated raw string, in a file that therefore does not compile.
    // Answering anything inside it would desynchronise the scan — its contents
    // may be anything at all — so the scan ends here, keeping whatever it found
    // before it.
    Some(bytes.len())
}

/// One past the end of the character literal, lifetime or label starting at the
/// `'` at `at`.
///
/// C code inside the macro body is full of the first (`'a'`, `'\n'`, `'\''`)
/// and Rust code around it of the other two (`&'a str`, `'outer: loop`), and
/// the two are told apart the way Rust's own lexer tells them apart: a `'`
/// followed by an escape is a character literal, a `'` followed by one
/// character and another `'` is a character literal, and anything else is a
/// lifetime — whose name is an identifier and holds nothing that could be
/// mistaken for a delimiter.
fn char_or_lifetime_end(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let rest = text.get(at + 1..)?;
    let mut chars = rest.char_indices();
    let Some((_, first)) = chars.next() else {
        return Some(bytes.len());
    };
    if first == '\\' {
        // An escape: `\'`, `\\`, `\n`, `\x41`, `\u{41}`. The closing quote is
        // the first one that is not itself escaped, which is why the scan starts
        // *at* the backslash: in `'\''` the quote it protects is the third
        // character and only the fourth one closes the literal.
        let mut i = at + 1;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'\'' => return Some(i + 1),
                _ => i += 1,
            }
        }
        return None;
    }
    if let Some((next, _)) = chars.next() {
        if rest.as_bytes()[next] == b'\'' {
            return Some(at + 1 + next + 1);
        }
    } else {
        return Some(bytes.len());
    }
    // A lifetime or a label.
    if is_ident_start(bytes[at + 1]) {
        return Some(ident_end(bytes, at + 1));
    }
    Some(at + 1)
}

// ---------------------------------------------------------------------------
// verification
// ---------------------------------------------------------------------------

/// A candidate body that every token matched.
struct Match {
    /// The first token's first byte, in the file.
    start: usize,
    /// One past the last token's last byte, in the file.
    end: usize,
    /// `(start, end, span)` per token, relative to `start`.
    anchors: AnchorList,
}

/// Walks `toks` over `text[body]`, requiring each to be written at the cursor.
///
/// This is the whole safety of the search. A token matches when its spelling is
/// at the cursor and ends there: an identifier may not run into a longer one, a
/// literal must be spelled exactly as the host spells it (`10UL`, `0x1f`,
/// `1.0f`, `'a'`, `"s\n"` — `rust-analyzer` and `rustc` both hand over the
/// spelling as written, so there is nothing to normalise), a delimiter must be
/// its own bracket. Between two tokens there may be whitespace and comments and
/// nothing else, and after the last token the body must hold nothing else
/// either — which is what stops a body that merely *starts* with these tokens
/// from being mistaken for them.
fn verify(text: &str, body: std::ops::Range<usize>, toks: &[FlatTok]) -> Option<Match> {
    let bytes = text.as_bytes();
    let mut at = body.start;
    let mut anchors = AnchorList::with_capacity(toks.len());
    let mut start = None;
    let mut end = body.start;
    let mut prev: Option<&FlatTok> = None;

    for tok in toks {
        // Several `Punct`s can share the span — and with it the source text —
        // of one multi-character operator; such a token was matched with the
        // one before it. See `shares_previous_span`.
        if let Some(prev) = prev
            && shares_previous_span(prev, tok)
        {
            continue;
        }
        prev = Some(tok);
        at = skip_trivia(bytes, at, body.end)?;
        let spelling = tok.text();
        let stop = at + spelling.len();
        if stop > body.end || !text[at..body.end].starts_with(spelling) {
            return None;
        }
        // An identifier or a literal that runs into more of itself is a
        // different token: `int` is not the start of `integer`, and `1` is not
        // the start of `1u`.
        if matches!(tok.kind(), FlatKind::Ident | FlatKind::Literal)
            && bytes.get(stop).copied().is_some_and(is_ident_continue)
        {
            return None;
        }
        if start.is_none() {
            start = Some(at);
        }
        let base = start.expect("the first token set the start");
        anchors.push(((at - base) as u32, (stop - base) as u32, tok.span()));
        at = stop;
        end = stop;
    }

    let start = start?;
    if skip_trivia(bytes, at, body.end)? != body.end {
        return None;
    }
    if end.checked_sub(start)? > u32::MAX as usize {
        return None;
    }
    Some(Match {
        start,
        end,
        anchors,
    })
}

/// The first byte at or after `at`, and before `limit`, that is neither
/// whitespace nor part of a comment.
///
/// [`None`] when a comment is not closed before `limit`, which means the body
/// is not what it looked like and the candidate is refused.
fn skip_trivia(bytes: &[u8], mut at: usize, limit: usize) -> Option<usize> {
    while at < limit {
        if bytes[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'/') {
            at = bytes[at..limit]
                .iter()
                .position(|c| *c == b'\n')
                .map_or(limit, |i| at + i + 1);
            continue;
        }
        if bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'*') {
            let end = block_comment_end(&bytes[..limit], at)?;
            at = end;
            continue;
        }
        break;
    }
    Some(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bodies `sites` finds, as text, in the order they are tried.
    fn found(text: &str, names: &[&str]) -> Vec<String> {
        sites(text, names)
            .expect("the fixture scans")
            .into_iter()
            .map(|site| text[site.body].to_owned())
            .collect()
    }

    #[test]
    fn a_site_is_an_identifier_a_bang_and_a_delimiter() {
        assert_eq!(found("c99! { int x; }", &["c99"]), vec![" int x; "]);
        assert_eq!(found("cinrs::c99!(int x;)", &["c99"]), vec!["int x;"]);
        assert_eq!(found("c99![int x;]", &["c99"]), vec!["int x;"]);
        // Whitespace and comments may sit anywhere between the three.
        assert_eq!(found("c99 /*x*/ ! { a }", &["c99"]), vec![" a "]);
    }

    #[test]
    fn a_name_that_was_not_asked_for_is_only_a_bare_site() {
        let text = "c99! { a }\nother! { b }";
        // Named first, then the rest.
        assert_eq!(found(text, &["c99"]), vec![" a ", " b "]);
        // With no name asked for, everything is bare and in file order.
        assert_eq!(found(text, &[]), vec![" a ", " b "]);
        assert_eq!(
            found("x99! { a }\nc99! { b }", &["c99"]),
            vec![" b ", " a "]
        );
    }

    #[test]
    fn an_identifier_that_merely_ends_with_the_name_is_not_a_site() {
        assert_eq!(found("xc99! { a }", &["c99"]), vec![" a "]);
        assert!(!found("xc99! { a }", &["c99"]).is_empty());
        let sites = sites("xc99! { a }", &["c99"]).expect("scans");
        assert!(!sites[0].named);
    }

    #[test]
    fn a_comment_or_a_string_is_not_code() {
        assert_eq!(found("// c99! { a }\nc99! { b }", &["c99"]), vec![" b "]);
        assert_eq!(found("/* c99! { a } */ c99! { b }", &["c99"]), vec![" b "]);
        // Block comments nest.
        assert_eq!(
            found("/* /* c99! { a } */ */ c99! { b }", &["c99"]),
            vec![" b "]
        );
        assert_eq!(
            found(r#"let s = "c99! { a }"; c99! { b }"#, &["c99"]),
            vec![" b "]
        );
        assert_eq!(
            found(
                r##"let s = r#"c99! { a } "unbalanced { "#; c99! { b }"##,
                &["c99"]
            ),
            vec![" b "]
        );
        assert_eq!(found(r#"let s = b"{"; c99! { b }"#, &["c99"]), vec![" b "]);
    }

    #[test]
    fn a_lifetime_is_not_a_character_literal() {
        // The `'` of a lifetime must not swallow the rest of the file.
        assert_eq!(
            found(
                "fn f<'a>(x: &'a str) -> &'static str { x }\nc99! { a }",
                &["c99"]
            ),
            vec![" a "]
        );
        assert_eq!(
            found("'outer: loop { break 'outer; }\nc99! { a }", &["c99"]),
            vec![" a "]
        );
        // And a character literal must not be read as one.
        assert_eq!(found("let c = '}'; c99! { a }", &["c99"]), vec![" a "]);
        assert_eq!(found(r#"let c = '\''; c99! { a }"#, &["c99"]), vec![" a "]);
        assert_eq!(found(r#"let c = '\\'; c99! { a }"#, &["c99"]), vec![" a "]);
        assert_eq!(found("let c = b'{'; c99! { a }", &["c99"]), vec![" a "]);
    }

    #[test]
    fn a_raw_identifier_is_not_a_raw_string() {
        assert_eq!(found("let r#match = 1; c99! { a }", &["c99"]), vec![" a "]);
    }

    #[test]
    fn nested_invocations_are_all_candidates() {
        let text = "outer! { c99! { a } }";
        assert_eq!(found(text, &["c99"]), vec![" a ", " c99! { a } "]);
    }

    #[test]
    fn a_file_that_cannot_be_scanned_is_passed_over() {
        // A delimiter that closes nothing.
        assert!(sites("} c99! { a }", &["c99"]).is_none());
        // Mismatched delimiters.
        assert!(sites("c99! ( a } ", &["c99"]).is_none());
        // An unterminated block comment.
        assert!(sites("/* c99! { a }", &["c99"]).is_none());
    }

    #[test]
    fn a_line_and_column_are_counted_the_way_a_span_counts_them() {
        assert_eq!(line_column("abc", 0), (1, 0));
        assert_eq!(line_column("ab\ncd", 4), (2, 1));
        // Columns are characters, not bytes.
        assert_eq!(line_column("// ★\nx", 8), (2, 1));
        assert_eq!(line_column("★x", "★".len()), (1, 1));
    }

    // -----------------------------------------------------------------------
    // the search itself
    // -----------------------------------------------------------------------

    use std::str::FromStr;

    use proc_macro2::TokenStream;

    use crate::capture::flat_tokens;

    /// A crate directory of its own, named after the test that asked for it.
    ///
    /// `CARGO_MANIFEST_DIR` is process-global, which is why the search takes the
    /// directory as a parameter; these are the directories it is given.
    struct Crate {
        dir: PathBuf,
    }

    impl Crate {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("cinrs-locate-{}-{name}", std::process::id()));
            std::fs::remove_dir_all(&dir).ok();
            std::fs::create_dir_all(&dir).expect("a temporary directory");
            Self { dir }
        }

        /// Writes one `.rs` file, creating the directories it sits in.
        fn file(&self, name: &str, text: &str) -> &Self {
            let path = self.dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("a temporary directory");
            }
            std::fs::write(path, text).expect("a writable temporary file");
            self
        }

        /// What the search makes of `input`, written as Rust tokens.
        fn find(&self, input: &str) -> Option<Slice> {
            self.find_with(input, Caps::default())
        }

        fn find_with(&self, input: &str, caps: Caps) -> Option<Slice> {
            let toks = flat_tokens(TokenStream::from_str(input).expect("the input lexes"));
            Search {
                dir: &self.dir,
                entry_names: &["c99"],
                caps,
            }
            .find(&toks, |_| true)
        }
    }

    impl Drop for Crate {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    /// The file a slice was found in, relative to the crate directory.
    fn found_in(krate: &Crate, slice: &Slice) -> String {
        slice
            .path
            .strip_prefix(&krate.dir)
            .expect("the match is inside the crate")
            .to_string_lossy()
            .replace('\\', "/")
    }

    #[test]
    fn the_invocation_is_found_among_decoys() {
        let body = "int fact(int n) { return n == 0 ? 1 : n * fact(n - 1); }";
        let krate = Crate::new("decoys");
        krate
            // A different body under the same name.
            .file(
                "src/other.rs",
                "cinrs::c99! { int g(void) { return 2; } }\n",
            )
            // The right body, but in a comment and in a string literal.
            .file(
                "src/comment.rs",
                &format!("// cinrs::c99! {{ {body} }}\n/* c99! {{ {body} }} */\n"),
            )
            .file(
                "src/string.rs",
                &format!("const S: &str = r#\"c99! {{ {body} }}\"#;\n"),
            )
            // A body that is only a prefix of the real one.
            .file(
                "src/prefix.rs",
                "c99! { int fact(int n) { return n == 0 ? 1 : n }\n",
            )
            // Rust that the scanner has to get past: lifetimes, labels, raw
            // strings, character literals with braces and quotes in them.
            .file(
                "src/real.rs",
                &format!(
                    "fn f<'a>(s: &'a str) -> char {{ 'outer: loop {{ break 'outer }} '}}' }}\n\
                     const R: &str = r##\"c99! {{ }} \"#\"##;\n\
                     const C: char = '\\'';\n\
                     mod inner {{\n    cinrs::c99! {{ {body} }}\n}}\n"
                ),
            );

        let slice = krate.find(body).expect("the invocation is found");
        assert_eq!(found_in(&krate, &slice), "src/real.rs");
        assert_eq!(slice.text, body);
        // The line and column the text starts at, which is what `__LINE__` and
        // the unit id are made of.
        assert_eq!((slice.line, slice.column), (5, 18));
        // One anchor per token handed over, `==` being two of them.
        let tokens = flat_tokens(TokenStream::from_str(body).expect("lexes")).len();
        assert_eq!(slice.anchors.len(), tokens);
        // Every anchor is inside the text and in order.
        let mut last = 0;
        for (start, end, _) in &slice.anchors {
            assert!(last <= *start && start <= end && *end as usize <= slice.text.len());
            last = *end;
        }
    }

    #[test]
    fn a_renamed_import_is_found_without_the_name() {
        let krate = Crate::new("renamed");
        krate.file(
            "src/lib.rs",
            "use cinrs::c99 as compile_c;\ncompile_c! { int x; }\n",
        );
        let slice = krate.find("int x;").expect("found by the bare search");
        assert_eq!(slice.text, "int x;");
    }

    #[test]
    fn every_delimiter_holds_a_body() {
        for (open, close) in [('{', '}'), ('(', ')'), ('[', ']')] {
            let krate = Crate::new(&format!("delim-{open}"));
            krate.file("src/lib.rs", &format!("c99!{open} int x; {close}\n"));
            assert_eq!(
                krate.find("int x;").expect("found").text,
                "int x;",
                "{open}{close}"
            );
        }
    }

    #[test]
    fn a_body_that_differs_from_the_tokens_is_not_matched() {
        // What an unsaved buffer looks like: the file on disk still says `1`
        // where the tokens say `2`.
        let krate = Crate::new("unsaved");
        krate.file("src/lib.rs", "c99! { int x = 1; }\n");
        assert!(krate.find("int x = 2;").is_none());
        // A token too many, and a token too few.
        assert!(krate.find("int x = 1").is_none());
        assert!(krate.find("int x = 1;;").is_none());
        // An identifier that only starts the same way.
        krate.file("src/lib.rs", "c99! { int xs; }\n");
        assert!(krate.find("int x;").is_none());
        // A literal spelled differently is a different literal.
        krate.file("src/lib.rs", "c99! { int x = 10UL; }\n");
        assert!(krate.find("int x = 10;").is_none());
        assert!(krate.find("int x = 10UL;").is_some());
    }

    #[test]
    fn comments_inside_the_body_are_skipped_and_kept() {
        let krate = Crate::new("comments");
        krate.file(
            "src/lib.rs",
            "c99! {\n    int x; // a comment\n    /* and\n       another */ int y;\n}\n",
        );
        let slice = krate.find("int x; int y;").expect("found");
        // The text is the file's, comments and line structure and all — which
        // is exactly why the search is worth doing.
        assert_eq!(
            slice.text,
            "int x; // a comment\n    /* and\n       another */ int y;"
        );
    }

    #[test]
    fn the_first_of_several_matches_wins_deterministically() {
        let krate = Crate::new("several");
        krate
            .file("src/a.rs", "c99! { int x; }\n")
            .file("src/b.rs", "c99! { int x; }\n")
            .file("lib.rs", "c99! { int x; }\n");
        // Files before subdirectories, each set sorted: the crate root's own
        // `lib.rs` comes first.
        assert_eq!(
            found_in(&krate, &krate.find("int x;").expect("found")),
            "lib.rs"
        );
    }

    #[test]
    fn the_headers_a_unit_includes_by_name_are_read_off_its_tokens() {
        let toks = flat_tokens(
            TokenStream::from_str("#include \"point.h\"\n#include <stdio.h>\nint x;")
                .expect("lexes"),
        );
        assert_eq!(quoted_includes(&toks), vec!["point.h".to_owned()]);
        // A body that is one string literal is not tokens at all, so the text
        // itself is what is read.
        let literal = flat_tokens(
            TokenStream::from_str("r#\"\n  #include \"a/b.h\"\n#include <stdio.h>\n\"#")
                .expect("lexes"),
        );
        assert_eq!(quoted_includes(&literal), vec!["a/b.h".to_owned()]);
        // Nothing to prefer by, which is the usual case.
        let plain = flat_tokens(TokenStream::from_str("int x;").expect("lexes"));
        assert!(quoted_includes(&plain).is_empty());
    }

    #[test]
    fn the_candidate_whose_directory_holds_the_header_is_preferred() {
        let krate = Crate::new("prefer");
        std::fs::create_dir_all(krate.dir.join("src/with")).expect("a temporary directory");
        krate
            .file("src/a.rs", "c99! { #include \"h.h\"\nint x; }\n")
            .file("src/with/b.rs", "c99! { #include \"h.h\"\nint x; }\n")
            .file("src/with/h.h", "int declared(void);\n");

        // Sorted path order would take `src/a.rs`; the header decides otherwise.
        let slice = krate
            .find("#include \"h.h\"\nint x;")
            .expect("the invocation is found");
        assert_eq!(found_in(&krate, &slice), "src/with/b.rs");

        // With the header beside neither — an include path would supply it — the
        // first in sorted order is the answer after all.
        std::fs::remove_file(krate.dir.join("src/with/h.h")).expect("a removable file");
        let slice = krate
            .find("#include \"h.h\"\nint x;")
            .expect("the invocation is found");
        assert_eq!(found_in(&krate, &slice), "src/a.rs");
    }

    #[test]
    fn a_candidate_the_caller_refuses_is_passed_over() {
        let krate = Crate::new("accept");
        krate
            .file("src/a.rs", "include_c99!(\"x.c\");\n")
            .file("src/b.rs", "include_c99!(\"x.c\");\n");
        let toks = flat_tokens(TokenStream::from_str("\"x.c\"").expect("lexes"));
        let search = Search {
            dir: &krate.dir,
            entry_names: &["include_c99"],
            caps: Caps::default(),
        };
        let slice = search
            .find(&toks, |path| path.ends_with("b.rs"))
            .expect("the second is accepted");
        assert_eq!(found_in(&krate, &slice), "src/b.rs");
    }

    #[test]
    fn target_and_hidden_directories_are_not_searched() {
        let krate = Crate::new("skipped");
        krate
            .file("target/debug/build/generated.rs", "c99! { int x; }\n")
            .file(".hidden/lib.rs", "c99! { int x; }\n");
        assert!(krate.find("int x;").is_none());
    }

    #[test]
    fn the_caps_stop_the_search() {
        let krate = Crate::new("caps");
        krate
            .file(
                "src/a.rs",
                &format!("// {}\nc99! {{ int y; }}\n", "pad".repeat(64)),
            )
            .file("src/b.rs", "c99! { int x; }\n");
        // Both files are there…
        assert!(krate.find("int x;").is_some());
        // … but only one of them may be looked at, and it is not the one.
        let one_file = Caps {
            files: 1,
            ..Caps::default()
        };
        assert!(krate.find_with("int x;", one_file).is_none());
        // The same for the byte budget: `a.rs` alone exhausts it.
        let few_bytes = Caps {
            total_bytes: 100,
            ..Caps::default()
        };
        assert!(krate.find_with("int x;", few_bytes).is_none());
        // A file larger than the per-file cap is skipped rather than read.
        let small_files = Caps {
            file_bytes: 20,
            ..Caps::default()
        };
        assert!(krate.find_with("int x;", small_files).is_some());
        // Nothing at all is read past the directory cap.
        let no_dirs = Caps {
            dirs: 0,
            ..Caps::default()
        };
        assert!(krate.find_with("int x;", no_dirs).is_none());
    }
}
