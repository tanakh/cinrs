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
//! # The cache
//!
//! A host with no positions asks for this search once per block, and the blocks
//! come in bursts: rust-analyzer re-expands every macro of a crate after one
//! edit. This repository's own tests hold some five hundred of them, and the
//! walk each expansion repeated is a hundred directories and five thousand
//! entries — measured at 25 ms a block in the unoptimized build a proc-macro
//! server runs (8 ms with optimizations), so thirteen seconds of that server's
//! time for one re-analysis of a tree in which nothing changed between the first
//! block and the last. Remembering it brings the block to 0.95 ms, which is one
//! `stat` per candidate and the token matching. So the search remembers, per
//! process and per directory:
//!
//! * **The candidate list, for [two seconds](LISTING_WINDOW).** Asking whether a
//!   listing is still right means asking every directory, and asking every
//!   directory *is* the walk; a directory's own modification time would answer
//!   only about entries added and removed, which is not what an edit changes.
//!   A short window costs one re-walk a second or two after a file is created,
//!   renamed or deleted, and spares the other few hundred.
//! * **The text of each file, with the `(mtime, len)` it was read at**, for as
//!   long as the file still reports them. The search already asks every
//!   candidate for its metadata before reading it, so this costs no extra call
//!   and needs no window at all: the expansion after a save reads the file
//!   again, because the file itself says to.
//! * **The [sites](Site) in that text**, which is what the scan above finds.
//!   They follow from the text alone, the entry point's name being compared
//!   against a site rather than during the scan.
//!
//! Three things make that safe to do:
//!
//! * **A stale text cannot win.** Verification walks the tokens the host handed
//!   over across the text one by one, so a text that is out of date fails to
//!   match — exactly as an unsaved buffer's does — and the block falls back to
//!   its tokens. The cache can cost a match; it cannot invent one.
//! * **The answer does not depend on the cache.** Every candidate is charged its
//!   length against the [byte budget](Caps::total_bytes) whether its text came
//!   from the disk or from memory, and a listing is reused only when it was
//!   collected under the same [`Caps`], so a cold search and a warm one accept
//!   the same candidate.
//! * **Memory is bounded by the ceiling that was already there.** Everything
//!   cached, over every directory, is bounded by the [`total_bytes`](Caps) a
//!   single search may read; passing it throws the whole cache away rather than
//!   letting it grow a directory at a time. At most [`DIRS`] directories are
//!   kept, the one longest unused going first.
//!
//! One mutex guards the whole of it, held to look a text up or to put one in and
//! never across a read of the disk, because a proc-macro server may expand on
//! several threads at once.
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

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime};

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
///
/// They are also part of what a [remembered](self#the-cache) candidate list is
/// keyed by, since they decide which files the walk collects at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        let candidates = self.candidate_files();
        for path in candidates.iter() {
            let Ok(meta) = std::fs::metadata(path) else {
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
            // Charged whether the text is read or remembered, so that a warm
            // cache cannot let a search reach a candidate a cold one would not;
            // see [the cache](self#the-cache).
            budget -= len;
            let Some(file) = self.file(path, &meta) else {
                continue;
            };
            let Some(sites) = file.sites.as_deref() else {
                continue;
            };
            let text = file.text.as_str();
            // The sites whose name was asked for, then the rest, each group in
            // the order the bodies start — which is the order the scan left them
            // in. The name is not part of what is cached: the text says where
            // the sites are, the entry point which of them are likely.
            for wanted in [true, false] {
                for site in sites {
                    if site.named(text, self.entry_names) != wanted {
                        continue;
                    }
                    let Some(found) = verify(text, site.body.clone(), toks) else {
                        continue;
                    };
                    if !accept(path) {
                        continue;
                    }
                    let (line, column) = line_column(text, found.start);
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
        }
        first
    }

    /// The text of one candidate and the sites in it, from the
    /// [cache](self#the-cache) when the file still reports the `(mtime, len)`
    /// the text was read at, and from the disk otherwise.
    ///
    /// [`None`] only when the file cannot be read or is not UTF-8, which is what
    /// passing over it means everywhere else here too.
    fn file(&self, path: &Path, meta: &std::fs::Metadata) -> Option<File> {
        let stamp = (meta.modified().ok(), meta.len());
        if let Some(hit) = cache().file(self.dir, path, stamp) {
            return Some(hit);
        }
        #[cfg(test)]
        counted(|counts| counts.reads += 1);
        let text = std::fs::read_to_string(path).ok()?;
        let file = File {
            stamp,
            sites: sites(&text).map(Arc::from),
            text: Arc::new(text),
        };
        cache().remember_file(self.dir, path, &file, self.caps.total_bytes);
        Some(file)
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
    ///
    /// The walk is the expensive half of the search, so its result is
    /// [remembered](self#the-cache) for a short while: a burst of expansions
    /// after one edit walks the crate once.
    fn candidate_files(&self) -> Arc<[PathBuf]> {
        if let Some(listed) = cache().listing(self.dir, &self.caps) {
            return listed;
        }
        let mut out = Vec::new();
        let mut dirs = 0usize;
        self.walk(self.dir, 0, &mut dirs, &mut out);
        out.sort();
        let files: Arc<[PathBuf]> = Arc::from(out);
        cache().remember_listing(self.dir, self.caps, &files);
        files
    }

    fn walk(&self, dir: &Path, depth: usize, dirs: &mut usize, out: &mut Vec<PathBuf>) {
        if depth > self.caps.depth || *dirs >= self.caps.dirs || out.len() >= self.caps.files {
            return;
        }
        *dirs += 1;
        #[cfg(test)]
        counted(|counts| counts.dirs += 1);
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
// the cache
// ---------------------------------------------------------------------------

/// How long a candidate list is believed without walking the crate again.
///
/// Long enough that the burst of expansions one edit provokes walks once, short
/// enough that a file created, renamed or deleted is a candidate — or stops
/// being one — while the editor is still on the same screen. What an *edit*
/// changes is never hidden by it: a file's text is keyed by what the file says
/// about itself, not by this. See [the cache](self#the-cache).
const LISTING_WINDOW: Duration = Duration::from_secs(2);

/// The greatest number of crate directories remembered at once.
///
/// A proc-macro server serves a whole workspace, so this is not one; and the
/// candidate list of a directory is a few thousand paths, so it is not many.
/// Passing it throws away the directory that has gone longest unsearched, which
/// in a workspace whose crates are analysed in turn is the one furthest from
/// being asked for again.
const DIRS: usize = 8;

/// What a file has to still report for its remembered text to be used: the
/// modification time it was read at, when the platform gives one, and its
/// length.
type Stamp = (Option<SystemTime>, u64);

/// One file as it was last read.
#[derive(Clone)]
struct File {
    /// What the file reported when this text was read; see [`Stamp`].
    stamp: Stamp,
    /// The file's text.
    text: Arc<String>,
    /// The invocations in `text`, in the order the bodies start, or [`None`]
    /// when the file does not scan at all — which is worth remembering too, so
    /// that a file that does not compile is scanned once rather than once per
    /// block.
    sites: Option<Arc<[Site]>>,
}

/// One crate directory's candidate list.
struct Listing {
    /// The candidate files, in the order they are tried.
    files: Arc<[PathBuf]>,
    /// The caps the walk was made under, which decide what it collected.
    caps: Caps,
    /// When it was collected; see [`LISTING_WINDOW`].
    at: Instant,
}

/// Everything remembered about one crate directory.
struct Dir {
    /// The directory, which is what the entry is found by.
    dir: PathBuf,
    /// Its candidate list, while there is one worth keeping.
    listing: Option<Listing>,
    /// The text of every file read from it, by path.
    files: HashMap<PathBuf, File>,
    /// The bytes those texts hold.
    bytes: u64,
    /// When this entry was last asked about, which is what decides the order
    /// entries are thrown away in.
    used: Instant,
}

/// The whole cache; see [the module docs](self#the-cache).
#[derive(Default)]
struct Cache {
    /// One entry per directory searched, in no particular order.
    dirs: Vec<Dir>,
    /// The bytes every entry's texts hold together.
    bytes: u64,
}

/// The one cache, which lives as long as the process.
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

/// The cache, locked.
///
/// Nothing but the cache's own bookkeeping runs while the lock is held — never a
/// read of the disk, and nothing that can panic — so a poisoned lock is not a
/// state this can really be in; and a procedural macro that gave up over a cache
/// would be the worst answer available, so a poisoned one is used anyway.
fn cache() -> MutexGuard<'static, Cache> {
    CACHE
        .get_or_init(|| Mutex::new(Cache::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Cache {
    /// The entry for `dir`, made if there is none, marked as just used.
    fn dir(&mut self, dir: &Path) -> &mut Dir {
        if let Some(at) = self.dirs.iter().position(|entry| entry.dir == dir) {
            let entry = &mut self.dirs[at];
            entry.used = Instant::now();
            return entry;
        }
        while self.dirs.len() >= DIRS {
            self.forget_oldest();
        }
        self.dirs.push(Dir {
            dir: dir.to_owned(),
            listing: None,
            files: HashMap::new(),
            bytes: 0,
            used: Instant::now(),
        });
        self.dirs.last_mut().expect("the entry just pushed")
    }

    /// Throws away the directory that has gone longest unsearched.
    fn forget_oldest(&mut self) {
        let Some(at) = (0..self.dirs.len()).min_by_key(|at| self.dirs[*at].used) else {
            return;
        };
        let gone = self.dirs.swap_remove(at);
        self.bytes -= gone.bytes;
    }

    /// The candidate list of `dir`, while it is worth believing: collected under
    /// the same caps, and no older than [`LISTING_WINDOW`].
    fn listing(&mut self, dir: &Path, caps: &Caps) -> Option<Arc<[PathBuf]>> {
        let listing = self.dir(dir).listing.as_ref()?;
        (listing.caps == *caps && listing.at.elapsed() < LISTING_WINDOW)
            .then(|| Arc::clone(&listing.files))
    }

    /// Remembers a walk's result as the candidate list of `dir`.
    fn remember_listing(&mut self, dir: &Path, caps: Caps, files: &Arc<[PathBuf]>) {
        self.dir(dir).listing = Some(Listing {
            files: Arc::clone(files),
            caps,
            at: Instant::now(),
        });
    }

    /// The remembered text of `path` under `dir`, when the file still reports
    /// the `(mtime, len)` it was read at.
    fn file(&mut self, dir: &Path, path: &Path, stamp: Stamp) -> Option<File> {
        let file = self.dir(dir).files.get(path)?;
        (file.stamp == stamp).then(|| file.clone())
    }

    /// Remembers `file` as the text of `path` under `dir`.
    ///
    /// `cap` is the [`Caps::total_bytes`] of the search doing the remembering:
    /// what one search may read is also all the cache may hold, and passing it
    /// empties the cache rather than shrinking it, since a ceiling that let the
    /// cache grow one directory at a time would be no ceiling.
    fn remember_file(&mut self, dir: &Path, path: &Path, file: &File, cap: u64) {
        let len = file.text.len() as u64;
        let entry = self.dir(dir);
        let was = entry
            .files
            .insert(path.to_owned(), file.clone())
            .map_or(0, |old| old.text.len() as u64);
        entry.bytes = entry.bytes + len - was;
        self.bytes = self.bytes + len - was;
        if self.bytes > cap {
            self.dirs.clear();
            self.bytes = 0;
        }
    }
}

/// Makes one crate's candidate list as old as the window, so that a test can
/// watch the walk happen again without waiting two seconds for it.
///
/// One crate's and not every crate's, because the tests run concurrently and a
/// test that watches its own crate being walked may not have another's ageing
/// done to it.
#[cfg(test)]
fn expire_listing(dir: &Path) {
    let mut cache = cache();
    let entry = cache.dir(dir);
    entry.listing = entry.listing.take().and_then(|listing| {
        // A monotonic clock too young to subtract from is no reason to fail a
        // test: forgetting the listing is where the window leads anyway.
        Instant::now()
            .checked_sub(LISTING_WINDOW)
            .map(|then| Listing {
                at: then,
                ..listing
            })
    });
}

/// What the searches on this thread have asked of the file system, which is what
/// the cache's tests watch.
///
/// Per thread rather than per process, because the test harness runs tests
/// concurrently and each of them on a thread of its own.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    /// Directories listed — [`Search::walk`]'s `read_dir` calls.
    dirs: usize,
    /// Files read.
    reads: usize,
}

#[cfg(test)]
thread_local! {
    static COUNTS: std::cell::Cell<Counts> = const { std::cell::Cell::new(Counts { dirs: 0, reads: 0 }) };
}

#[cfg(test)]
fn counted(f: impl FnOnce(&mut Counts)) {
    COUNTS.with(|counts| {
        let mut now = counts.get();
        f(&mut now);
        counts.set(now);
    });
}

#[cfg(test)]
impl Counts {
    /// What this thread has done since the process started.
    fn now() -> Self {
        COUNTS.with(std::cell::Cell::get)
    }

    /// What it has done since `self` was read.
    fn since(self) -> Self {
        let now = Self::now();
        Self {
            dirs: now.dirs - self.dirs,
            reads: now.reads - self.reads,
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
    /// The identifier before the `!`, as a range of the same text.
    ///
    /// Not the answer to "is this the name asked for?" but what the answer is
    /// read off, so that a site follows from the text alone and can be
    /// [remembered](self#the-cache) with it however the block is spelled.
    name: std::ops::Range<usize>,
}

impl Site {
    /// Whether this site's name is one of `names` — which only decides whether
    /// it is looked at before the rest; see [`Search::entry_names`].
    fn named(&self, text: &str, names: &[&str]) -> bool {
        names.contains(&&text[self.name.clone()])
    }
}

/// Every `name! (…)`, `name! […]` and `name! {…}` in `text`, in the order the
/// bodies start.
///
/// [`None`] means the scan lost its way and nothing in the file may be believed:
/// a delimiter that closes nothing or closes the wrong thing, an unterminated
/// comment or character literal. (An unterminated *raw string* only ends the
/// scan, since what precedes it was read correctly.) Either way the file is
/// passed over, and it does not compile as it stands anyway.
fn sites(text: &str) -> Option<Vec<Site>> {
    let bytes = text.as_bytes();
    let mut out: Vec<Site> = Vec::new();
    // The open delimiters currently entered: where the body starts, which
    // character closes it, and the name of the macro it is the body of.
    let mut open: Vec<(usize, u8, Option<std::ops::Range<usize>>)> = Vec::new();
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
            open.push((at + 1, close, bang.take()));
            ident = None;
            at += 1;
            continue;
        }
        if matches!(b, b')' | b']' | b'}') {
            // Rust cannot be lexed with unbalanced delimiters outside a comment
            // or a literal, so a mismatch here means the scan has lost its way
            // and nothing it goes on to find may be believed.
            let (start, close, name) = open.pop()?;
            if close != b {
                return None;
            }
            if let Some(name) = name {
                out.push(Site {
                    body: start..at,
                    name,
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

    // A site is found when its body *closes*, so a nested one is found before
    // the site it is nested in; the order they are tried in starts from where
    // each body begins, and no two bodies begin in the same place.
    out.sort_by_key(|site| site.body.start);
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

    /// The bodies `sites` finds, as text, in the order they are tried: the ones
    /// whose name was asked for first, then the rest — which is what
    /// [`Search::find`] does with them.
    fn found(text: &str, names: &[&str]) -> Vec<String> {
        let sites = sites(text).expect("the fixture scans");
        let mut out = Vec::new();
        for wanted in [true, false] {
            for site in &sites {
                if site.named(text, names) == wanted {
                    out.push(text[site.body.clone()].to_owned());
                }
            }
        }
        out
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
        let text = "xc99! { a }";
        assert_eq!(found(text, &["c99"]), vec![" a "]);
        let sites = sites(text).expect("scans");
        assert!(!sites[0].named(text, &["c99"]));
        assert!(sites[0].named(text, &["xc99"]));
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
        assert!(sites("} c99! { a }").is_none());
        // Mismatched delimiters.
        assert!(sites("c99! ( a } ").is_none());
        // An unterminated block comment.
        assert!(sites("/* c99! { a }").is_none());
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

    // -----------------------------------------------------------------------
    // the cache
    // -----------------------------------------------------------------------

    /// Held by every test that counts what the file system was asked for.
    ///
    /// The cache is one per process, so the test that fills it past its ceiling
    /// empties every crate's, and a test watching its own crate may not have
    /// that happen behind its back. Nothing else here can: no other test
    /// remembers a text under a ceiling small enough to pass.
    static WATCHING: Mutex<()> = Mutex::new(());

    fn watching() -> MutexGuard<'static, ()> {
        WATCHING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn a_burst_of_searches_walks_the_crate_once() {
        let _watching = watching();
        let krate = Crate::new("burst");
        krate
            .file("src/lib.rs", "c99! { int x; }\n")
            .file("src/other.rs", "// nothing to find here\n");

        let before = Counts::now();
        assert!(krate.find("int x;").is_some());
        // The crate root and `src`; and one file read, since `src/lib.rs` sorts
        // before `src/other.rs` and the search stops at the match.
        assert_eq!(
            before.since(),
            Counts { dirs: 2, reads: 1 },
            "the first search does the work"
        );

        // What an editor asks for after one edit: every block of the crate over
        // again. Nothing on disk has changed, so nothing is walked or read a
        // second time — only each candidate's own metadata is asked for.
        let before = Counts::now();
        for _ in 0..16 {
            assert!(krate.find("int x;").is_some());
        }
        assert_eq!(
            before.since(),
            Counts::default(),
            "the rest of the burst is served from memory"
        );
    }

    #[test]
    fn a_file_saved_again_is_read_again() {
        let krate = Crate::new("edited");
        krate.file("src/lib.rs", "c99! { int x = 1; }\n");
        assert!(krate.find("int x = 1;").is_some());

        // The editor saves. The candidate list is still the one from a moment
        // ago, but a text is remembered under what the file reports about itself,
        // so the very next expansion sees the new bytes — no window, no wait.
        krate.file("src/lib.rs", "c99! { int x = 22; }\n");
        assert!(krate.find("int x = 22;").is_some());
        // And the text that is gone is gone: nothing matches it any more.
        assert!(krate.find("int x = 1;").is_none());
    }

    #[test]
    fn a_new_file_becomes_a_candidate_when_the_crate_is_walked_again() {
        let krate = Crate::new("listed");
        krate.file("src/a.rs", "c99! { int x; }\n");
        assert!(krate.find("int x;").is_some());

        // A file that did not exist when the crate was listed is not a candidate
        // yet. That is the whole of what the window costs — and the block it
        // holds is answered from its tokens meanwhile, as it would be anyway
        // while the file was unsaved.
        let written = Instant::now();
        krate.file("src/b.rs", "c99! { int y; }\n");
        let missed = krate.find("int y;").is_none();
        if written.elapsed() < LISTING_WINDOW {
            assert!(missed, "the listing is the one from before the file");
        }
        // Once the listing is as old as the window, it is a candidate.
        expire_listing(&krate.dir);
        assert!(krate.find("int y;").is_some());
    }

    #[test]
    fn everything_is_forgotten_when_the_ceiling_is_passed() {
        let _watching = watching();
        // Two crates whose texts do not fit in the cache together: what one
        // search may read is all the cache may hold.
        let caps = Caps {
            total_bytes: 600,
            ..Caps::default()
        };
        let pad = "pad".repeat(100);
        let a = Crate::new("ceiling-a");
        a.file("src/lib.rs", &format!("// {pad}\nc99! {{ int x; }}\n"));
        let b = Crate::new("ceiling-b");
        b.file("src/lib.rs", &format!("// {pad}\nc99! {{ int y; }}\n"));

        assert!(a.find_with("int x;", caps).is_some());
        assert!(b.find_with("int y;", caps).is_some());
        let before = Counts::now();
        assert!(a.find_with("int x;", caps).is_some());
        let again = before.since();
        assert!(again.reads > 0, "the text was thrown away: {again:?}");
        assert!(again.dirs > 0, "and so was the listing: {again:?}");
    }
}
