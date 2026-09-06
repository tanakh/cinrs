//! Conformance measured against Clang's C standard-conformance tests.
//!
//! `clang/test/C` is Clang's record of *its own* answer to the C status page:
//! one file per WG14 paper (`C99/n617.c`, `C23/n3042.c`) or defect report
//! (`drs/dr0xx.c`), each a `lit` test whose `// RUN:` line compiles it with a
//! particular `-std=` and whose `// expected-error {{…}}` comments say exactly
//! which lines must be diagnosed. Because the file names are paper numbers, the
//! result maps row by row onto [`doc/c-status.md`](../doc/c-status.md) — which
//! is what this harness is for: it turns "believed to work" into "checked".
//!
//! The corpus is fetched by `scripts/fetch-testsuites.sh`, not checked in;
//! without it the harness prints how to get it and exits successfully, unless
//! `CINRS_TESTSUITES_REQUIRED=1`. `doc/clang-c-tests.md` has the results.
//!
//! The expected-failure list measures `cinrs` as its **default features**
//! build it, so a build with `complex` off — where `_Complex` is a diagnostic
//! again, which by itself turns seven revisions into false rejections — skips
//! the suite the same way and under the same variable; see
//! [`conformance::skip_unless_measured_build`].
//!
//! # One RUN line is one revision
//!
//! A file may be compiled several ways — `drs/dr0xx.c` has six RUN lines, one
//! per revision of C — and each is a *revision* of the test with its own
//! result and its own line in the expected-failure list. A revision whose
//! flags this harness cannot honour is **skipped with a reason** rather than
//! guessed at; [`Revision::parse`] is the whole list of what is honoured, what
//! is ignored and what causes a skip.
//!
//! # The oracle
//!
//! `-verify` checks every diagnostic Clang emits against the annotations.
//! `cinrs` has no warnings to speak of — a procedural macro has no way to raise
//! one — so **warnings and notes are ignored** and what is left is the set of
//! *lines* carrying an `expected-error`:
//!
//! * a revision with at least one of them is a **reject** case: `cinrs` must
//!   report at least one error on each of those lines and no error anywhere
//!   else. That is checked with the front end [in
//!   process](front_end_errors) — no `rustc`, no linking, a hundred and thirty
//!   revisions in well under a second;
//! * a revision with none of them (or with `expected-no-diagnostics`) is an
//!   **accept** case: the front end must produce no error *and* the expansion
//!   must compile, which is a `//@check-pass` file put through `ui_test`.
//!   Valid C that produces invalid Rust is a `cinrs` bug and this is what finds
//!   it.
//!
//! An `expected-error` in this directory is a genuine constraint violation:
//! most of the RUN lines pass `-pedantic-errors` or `-pedantic`, which is what
//! turns Clang's extension warnings into errors, and the ones that do not spell
//! the errors out anyway. `expected-warning` is where the extensions live, and
//! that is exactly what is ignored.
//!
//! # Environment
//!
//! * `CINRS_TESTSUITES_REQUIRED=1` — fail rather than skip when the corpus is
//!   missing, or when this build is not the default-feature one the list
//!   describes.
//! * `CINRS_CLANG_C_FILTER=<substring>` — only revisions whose id contains it.
//! * `CINRS_CLANG_C_REPORT=1` — report mode.
//! * `CINRS_CLANG_C_STRICT=1` — a stale expected-failure entry is a failure.
//! * `CINRS_CLANG_C_UPDATE_EXPECTED=1` — rewrite the list from the report.
//! * `CINRS_MEMORY_LIMIT_MB=<mib>` — the ceiling this harness, every compiler
//!   it spawns and every program it runs work to; 8192 by default and `0` to
//!   switch all of it off. See the memory section of
//!   [`support/conformance.rs`](conformance#memory).
//! * `CINRS_TEST_THREADS=<n>` — the default parallelism, which is otherwise
//!   the smaller of this machine's and eight. `-- --test-threads=<n>` wins
//!   over both.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::time::Instant;

use cinrs_core::{Dialect, Level, Options, Standard};
use ui_test::Args;
use ui_test::color_eyre::eyre::{Result, eyre};

#[path = "support/conformance.rs"]
mod conformance;

use conformance::{
    COMPILE_TIMEOUT, Collector, Entry, EntryKind, Outcome, confuses_ui_test, duration, flag,
    group_texts, listing, marker_legend, percent, plural, raw_string_hashes, read_list, write_list,
};

// ---------------------------------------------------------------------------
// where things live
// ---------------------------------------------------------------------------

/// The corpus, relative to the package root.
const CORPUS: &str = "third_party/llvm-project/clang/test/C";

/// What to tell someone who has not fetched it.
const FETCH_HINT: &str = "scripts/fetch-testsuites.sh llvm";

/// The directories that are run, in report order.
///
/// `C2y` is left out: it is the *next* revision, still being drafted, and
/// nothing in `cinrs` claims to implement any of it.
const DIRS: &[&str] = &["C99", "C11", "C23", "drs"];

/// Where the expected-failure list lives.
const LIST: &str = "tests/clang-c/expected-failures.txt";

/// Where the generated `//@check-pass` files go.
const GEN_DIR: &str = "target/clang-c";

/// Where `ui_test` puts what it builds.
const BUILD_DIR: &str = "target/clang-c-build";

/// The module the expansion of every accept case goes into.
const MODULE: &str = "clangtest";

// ---------------------------------------------------------------------------
// entry points
// ---------------------------------------------------------------------------

/// A `cinrs` entry point, as a `-std=` maps onto one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct EntryPoint {
    /// `c11`, `gnu17`, … — the macro's name without the `!`.
    name: &'static str,
    standard: Standard,
    dialect: Dialect,
}

impl EntryPoint {
    fn options(self) -> Options {
        Options::with_dialect(self.standard, self.dialect)
    }
}

/// What a `-std=` on a RUN line means here.
///
/// * The five ISO revisions and the five GNU ones map straight across;
///   `c2x`/`gnu2x` are C23's working names and map to `c23!`/`gnu23!`, and
///   `c90` is the other name for C89.
/// * **C95 has no entry point.** `-std=iso9899:199409` is Amendment 1, which
///   is C89 plus `<iso646.h>`, `<wctype.h>` and a `__STDC_VERSION__` of
///   `199409L`; `c89!` would answer the amendment's own questions wrongly, so
///   those revisions are skipped.
/// * **No `-std=` at all** is `gnu17!`, which is what `clang -cc1` defaults to
///   for C.
/// * C++ is skipped.
fn entry_point(std: Option<&str>) -> std::result::Result<EntryPoint, String> {
    let point = |name, standard, dialect| {
        Ok(EntryPoint {
            name,
            standard,
            dialect,
        })
    };
    match std.unwrap_or("gnu17") {
        "c89" | "c90" | "iso9899:1990" => point("c89", Standard::C89, Dialect::Iso),
        "gnu89" | "gnu90" => point("gnu89", Standard::C89, Dialect::Gnu),
        "c99" => point("c99", Standard::C99, Dialect::Iso),
        "c11" => point("c11", Standard::C11, Dialect::Iso),
        "c17" | "c18" | "iso9899:2017" => point("c17", Standard::C17, Dialect::Iso),
        "c23" | "c2x" => point("c23", Standard::C23, Dialect::Iso),
        "gnu99" => point("gnu99", Standard::C99, Dialect::Gnu),
        "gnu11" => point("gnu11", Standard::C11, Dialect::Gnu),
        "gnu17" | "gnu18" => point("gnu17", Standard::C17, Dialect::Gnu),
        "gnu23" | "gnu2x" => point("gnu23", Standard::C23, Dialect::Gnu),
        other if other.starts_with("c++") || other.starts_with("gnu++") => {
            Err(format!("`-std={other}` is C++"))
        }
        other @ "iso9899:199409" => Err(format!("`-std={other}`: there is no C95 entry point")),
        other => Err(format!("`-std={other}` is not a standard cinrs has")),
    }
}

// ---------------------------------------------------------------------------
// RUN lines
// ---------------------------------------------------------------------------

/// One `// RUN:` line, turned into something this harness can act on.
#[derive(Debug)]
struct Revision {
    /// Which RUN line of the file it is, counting from zero.
    index: usize,
    /// The entry point and the flags, or why the revision is not run.
    plan: std::result::Result<Plan, String>,
}

/// What to compile, and how.
#[derive(Debug)]
struct Plan {
    entry: EntryPoint,
    /// The `-verify=` prefixes; `expected` when `-verify` is bare.
    prefixes: Vec<String>,
    /// `-D` on the command line, as the `#define` lines they become.
    defines: Vec<String>,
    /// The case's own directory, then every `-I` on the command line, as the
    /// `#pragma cinrs include_path` lines they become.
    includes: Vec<PathBuf>,
}

impl Plan {
    /// The lines that go in front of the case, and so the offset between a
    /// line of the generated unit and a line of the upstream file.
    fn prelude(&self) -> Vec<String> {
        let mut out = Vec::new();
        for dir in &self.includes {
            let dir = dir.display().to_string();
            out.push(format!("#pragma cinrs include_path {dir:?}"));
        }
        out.extend(
            self.defines
                .iter()
                .map(|define| format!("#define {define}")),
        );
        out
    }
}

/// Splits a command line on whitespace, keeping `"…"` and `'…'` together.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut started = false;
    for ch in line.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(ch);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(current);
    }
    out
}

/// Whether a `-triple` is this machine.
///
/// `x86_64` on its own, and `x86_64-unknown-unknown`, are the LP64 model
/// `cinrs` assumes; a Windows or 32-bit triple is not, and a revision that
/// names one is skipped rather than measured against the wrong data model.
fn triple_is_ours(triple: &str) -> bool {
    let (arch, rest) = triple.split_once('-').unwrap_or((triple, ""));
    arch == "x86_64" && !rest.contains("win32") && !rest.contains("windows")
}

impl Revision {
    /// Reads one RUN line.
    ///
    /// # What is honoured
    ///
    /// `-std=`, `-verify` and `-verify=<prefixes>`, `-D`, `-I` (with `%S`
    /// substituted for the file's directory) and `-triple`.
    ///
    /// # What is ignored
    ///
    /// `-fsyntax-only` (everything here is a syntax-only check anyway),
    /// `-pedantic`, `-pedantic-errors`, every `-W…`, `-O…`,
    /// `-fno-dollars-in-identifiers` (which is what `cinrs` does by default),
    /// and `-Wno-…`. Whether `-pedantic-errors` is passed makes no difference
    /// to the oracle: an `expected-error` is an error either way, and the
    /// warnings the flag would promote are the ones that are ignored.
    ///
    /// # What causes the revision to be skipped
    ///
    /// * a RUN line that is not a `%clang_cc1`, or whose input is not `%s` — a
    ///   generated `%t` file, or a `%python` line that writes one;
    /// * a pipeline (`| FileCheck`), *unless* the file has no `-verify` RUN
    ///   line at all, in which case there are no annotations for anything and
    ///   the revision is taken as an accept case: valid C that has to compile;
    /// * a `-triple` that is not x86-64 (see [`triple_is_ours`]);
    /// * `-x c++` or a C++ `-std=`, and `-std=iso9899:199409` (see
    ///   [`entry_point`]);
    /// * `-ffreestanding`, which `cinrs` has no mode for: `__STDC_HOSTED__` is
    ///   1 and the bundled headers are the hosted ones, so a test written
    ///   against a freestanding implementation would be measured against the
    ///   wrong thing;
    /// * `-ftrigraphs`, which asks for trigraphs in a mode that does not have
    ///   them; `cinrs` has them exactly where the standard puts them — every
    ///   strict entry point below `c23!` — and no switch that moves the line;
    /// * any other `-f…` or `-m…` flag the list above does not name — Clang
    ///   language extensions (`-fms-extensions`, `-fblocks`,
    ///   `-fexperimental-…`) and anything new upstream adds. The flag is named
    ///   in the skip reason, so the list of them is a to-do rather than a
    ///   silent hole.
    fn parse(index: usize, command: &str, dir: &Path, file_has_verify: bool) -> Revision {
        let mut plan = Plan {
            entry: EntryPoint {
                name: "gnu17",
                standard: Standard::C17,
                dialect: Dialect::Gnu,
            },
            prefixes: Vec::new(),
            // The case's own directory, always. A quoted `#include` is
            // looked for beside the file it is written in, and the file this
            // harness compiles is a generated one under `target/` — so
            // without this the `#include "./abc_123.h"` in `drs/dr3xx.c`
            // resolves against the wrong directory and the first error is one
            // Clang never sees. It is the same two lines the gcc-torture
            // harness puts in front of every case, for the same reason.
            includes: vec![dir.to_path_buf()],
            defines: Vec::new(),
        };
        let mut standard: Option<String> = None;
        let mut skip: Option<String> = None;
        let mut input: Option<String> = None;
        let mut piped = false;
        let mut fail = |reason: String| {
            if skip.is_none() {
                skip = Some(reason);
            }
        };

        let words = words(command);
        let mut it = words.iter().enumerate().peekable();
        if words.first().is_none_or(|first| first != "%clang_cc1") {
            return Revision {
                index,
                plan: Err("not a `%clang_cc1` invocation".to_owned()),
            };
        }
        while let Some((_, word)) = it.next() {
            let word = word.as_str();
            if word == "|" {
                piped = true;
                break;
            }
            let mut value_of = |flag: &str| -> Option<String> {
                if let Some(rest) = word.strip_prefix(&format!("{flag}=")) {
                    return Some(rest.to_owned());
                }
                if let Some(rest) = word.strip_prefix(flag).filter(|rest| !rest.is_empty()) {
                    return Some(rest.to_owned());
                }
                (word == flag).then(|| it.next().map(|(_, next)| next.clone()))?
            };
            match word {
                "%clang_cc1" => {}
                "-verify" => plan.prefixes.push("expected".to_owned()),
                _ if word.starts_with("-verify=") => {
                    plan.prefixes.extend(
                        word["-verify=".len()..]
                            .split(',')
                            .map(str::trim)
                            .filter(|p| !p.is_empty())
                            .map(str::to_owned),
                    );
                }
                _ if word.starts_with("-std=") => standard = Some(word["-std=".len()..].to_owned()),
                "-x" => match it.next().map(|(_, next)| next.as_str()) {
                    Some("c") => {}
                    Some(other) => fail(format!("`-x {other}` is not C")),
                    None => fail("a trailing `-x`".to_owned()),
                },
                "-triple" | "-triple=" => {}
                _ if word.starts_with("-triple") => {
                    match value_of("-triple") {
                        Some(triple) if triple_is_ours(&triple) => {}
                        Some(triple) => fail(format!("`-triple {triple}` is not x86-64 Linux")),
                        None => fail("a trailing `-triple`".to_owned()),
                    }
                    continue;
                }
                _ if word.starts_with("-D") => {
                    if let Some(define) = value_of("-D") {
                        plan.defines.push(define.replacen('=', " ", 1));
                    }
                    continue;
                }
                _ if word.starts_with("-I") => {
                    if let Some(path) = value_of("-I") {
                        plan.includes.push(substitute(&path, dir));
                    }
                    continue;
                }
                "-ffreestanding" => {
                    fail("`-ffreestanding`: cinrs has no freestanding mode".to_owned());
                }
                "-ftrigraphs" | "-trigraphs" => {
                    fail(
                        "`-ftrigraphs`: cinrs has trigraphs in every strict entry point below \
                         c23! and no switch that moves the line"
                            .to_owned(),
                    );
                }
                // Diagnostics knobs, output knobs and optimisation knobs, none
                // of which changes what is *accepted*.
                "-fsyntax-only"
                | "-pedantic"
                | "-pedantic-errors"
                | "-emit-llvm"
                | "-ast-dump"
                | "-E"
                | "-o"
                | "-w"
                | "-disable-llvm-passes"
                | "-fno-dollars-in-identifiers" => {
                    if word == "-o" {
                        it.next();
                    }
                }
                _ if word.starts_with("-W") || word.starts_with("-O") => {}
                _ if word.starts_with("-f") || word.starts_with("-m") => {
                    fail(format!("`{word}` is a Clang-only flag"));
                }
                _ if word.starts_with('-') => fail(format!("`{word}` is not understood")),
                _ => input = Some(word.to_owned()),
            }
        }

        // A pipeline is a FileCheck test: there is nothing here that can check
        // what it checks. It is still worth compiling when the file carries no
        // annotations at all, because then the whole file is valid C.
        if piped && file_has_verify {
            fail("a `| FileCheck` pipeline, and the file has a `-verify` run too".to_owned());
        } else if piped && plan.prefixes.is_empty() {
            // Left as an accept case.
        }
        match input.as_deref() {
            Some("%s") | None => {}
            Some(other) => fail(format!("the input is `{other}` rather than `%s`")),
        }
        match entry_point(standard.as_deref()) {
            Ok(entry) => plan.entry = entry,
            Err(reason) => fail(reason),
        }

        Revision {
            index,
            plan: match skip {
                Some(reason) => Err(reason),
                None => Ok(plan),
            },
        }
    }
}

/// `%S` is the directory the test file is in; `%t` has no meaning here.
fn substitute(argument: &str, dir: &Path) -> PathBuf {
    PathBuf::from(argument.replace("%S", &dir.display().to_string()))
}

// ---------------------------------------------------------------------------
// -verify annotations
// ---------------------------------------------------------------------------

/// One `<prefix>-<kind>` directive.
#[derive(Clone, Debug)]
struct Annotation {
    prefix: String,
    /// `error`, `warning`, `note`, `remark`, or `no-diagnostics`.
    kind: String,
    /// The line it applies to, once `@…` has been resolved.
    line: Option<usize>,
    /// Whether the count was `0+`, which makes the diagnostic optional.
    optional: bool,
    /// Why the line could not be resolved, when it could not.
    unresolved: Option<String>,
}

/// The `// #name` markers an `@#name` refers to.
fn anchors(source: &str) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (number, line) in source.lines().enumerate() {
        let Some(comment) = line
            .find("//")
            .or_else(|| line.find("/*"))
            .map(|at| &line[at + 2..])
        else {
            continue;
        };
        let mut rest = comment;
        while let Some(at) = rest.find('#') {
            let name: String = rest[at + 1..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            rest = &rest[at + 1..];
            if !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) {
                out.entry(name).or_insert(number + 1);
            }
        }
    }
    out
}

/// The line a directive on `line` is really about.
///
/// Two rules, both Clang's own `-verify`:
///
/// * a directive split over several lines with a trailing `\` belongs to the
///   first of them;
/// * a directive written on a *continuation* line of a `/* … */` comment
///   belongs to the line the **comment** started on. That is how a run of
///   directives under one diagnostic all point at it, and `drs/dr1xx.c`,
///   `drs/dr0xx.c` and half of `drs/` are written that way:
///
///   ```c
///   void f(struct S s) {} /* expected-warning {{…}}
///                            expected-error {{…}} */
///   ```
///
///   both of which Clang expects on the line of the definition. Reading the
///   second as its own line answered these files' questions on the wrong
///   lines, which showed up as "wrong line" against `cinrs`.
fn base_line(lines: &[&str], comment_starts: &[usize], line: usize) -> usize {
    let mut at = comment_starts.get(line).copied().unwrap_or(line);
    while at > 1 && lines[at - 2].trim_end().ends_with('\\') {
        at -= 1;
    }
    at
}

/// For every line, the line on which the block comment it is inside began.
///
/// A line that is not inside one — or on which the comment itself opens — is
/// its own answer. Index 0 is unused, so that the vector is indexed by line
/// number. See [`base_line`].
fn comment_starts(source: &str) -> Vec<usize> {
    let mut out = vec![0usize];
    let mut open: Option<usize> = None;
    for (index, text) in source.lines().enumerate() {
        let line = index + 1;
        out.push(open.unwrap_or(line));
        let bytes = text.as_bytes();
        let mut at = 0usize;
        let mut quote: Option<u8> = None;
        while at < bytes.len() {
            if open.is_some() {
                if bytes[at] == b'*' && bytes.get(at + 1) == Some(&b'/') {
                    open = None;
                    at += 2;
                } else {
                    at += 1;
                }
                continue;
            }
            // A `/*` inside a string literal opens nothing, and a `"` inside a
            // comment closes nothing; tracking both keeps the two apart.
            if let Some(delimiter) = quote {
                if bytes[at] == b'\\' {
                    at += 2;
                    continue;
                }
                if bytes[at] == delimiter {
                    quote = None;
                }
                at += 1;
                continue;
            }
            match (bytes[at], bytes.get(at + 1)) {
                (b'/', Some(b'*')) => {
                    open = Some(line);
                    at += 2;
                }
                (b'/', Some(b'/')) => break,
                (b'"' | b'\'', _) => {
                    quote = Some(bytes[at]);
                    at += 1;
                }
                _ => at += 1,
            }
        }
    }
    out
}

/// Reads every `-verify` directive out of the file.
///
/// The scan is over the raw text rather than over the comments, because a
/// directive *is* a comment and the shapes they take — inside `//`, inside
/// `/* … */`, continued with `\` — are more work to isolate than the false
/// positives are worth. The one thing that is skipped over deliberately is the
/// `{{…}}` message, so that a directive quoted inside another directive's text
/// is not read as one.
fn annotations(source: &str) -> Vec<Annotation> {
    const KINDS: &[&str] = &["error", "warning", "note", "remark", "no-diagnostics"];
    let lines: Vec<&str> = source.lines().collect();
    // Byte offset of the start of each line, for turning an offset into a line.
    let mut line_of = Vec::with_capacity(source.len());
    let mut number = 1usize;
    for byte in source.bytes() {
        line_of.push(number);
        if byte == b'\n' {
            number += 1;
        }
    }
    line_of.push(number);
    let comment_start_of = comment_starts(source);

    let mut out = Vec::new();
    let mut at = 0usize;
    while at < source.len() {
        let Some(found) = source[at..].find('-') else {
            break;
        };
        let dash = at + found;
        at = dash + 1;
        let Some(kind) = KINDS.iter().find(|kind| source[at..].starts_with(**kind)) else {
            continue;
        };
        // The prefix is the identifier immediately before the dash — and a
        // `-verify=` prefix may itself hold a dash, which `C23/n2940.c`'s
        // `no-trigraphs-error` does: stopping at the first one would read that
        // as the `trigraphs` prefix and answer the wrong revision's question.
        let prefix_start = source[..dash]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .map_or(0, |at| at + 1);
        let prefix = source[prefix_start..dash].trim_start_matches('-');
        if prefix.is_empty() {
            continue;
        }
        let mut rest = &source[at + kind.len()..];
        let directive_line = base_line(&lines, &comment_start_of, line_of[dash]);
        if *kind == "no-diagnostics" {
            out.push(Annotation {
                prefix: prefix.to_owned(),
                kind: (*kind).to_owned(),
                line: Some(directive_line),
                optional: false,
                unresolved: None,
            });
            continue;
        }
        // `-re` makes the message a regular expression; the message is not
        // what is checked here, so it changes nothing.
        rest = rest.strip_prefix("-re").unwrap_or(rest);
        // `@…` moves the directive to another line.
        let (line, unresolved) = if let Some(after) = rest.strip_prefix('@') {
            let token: String = after
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '{')
                .collect();
            rest = &after[token.len()..];
            resolve_location(&token, directive_line, source)
        } else {
            (Some(directive_line), None)
        };
        // An optional count: `2`, `0+`, `1+`.
        let rest_trimmed = rest.trim_start();
        let digits: String = rest_trimmed
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let optional = digits == "0";
        if !digits.is_empty() {
            rest = &rest_trimmed[digits.len()..];
            rest = rest.strip_prefix('+').unwrap_or(rest);
        }
        // Everything up to the end of the `{{…}}` message belongs to this
        // directive and must not be rescanned.
        if let Some(open) = rest.find("{{") {
            let after_open = &rest[open + 2..];
            let close = after_open.find("}}").map_or(after_open.len(), |at| at + 2);
            at = source.len() - after_open.len() + close;
        } else {
            at = source.len() - rest.len();
        }
        out.push(Annotation {
            prefix: prefix.to_owned(),
            kind: (*kind).to_owned(),
            line,
            optional,
            unresolved,
        });
    }
    out
}

/// Resolves the token after `@` in a directive.
fn resolve_location(
    token: &str,
    directive_line: usize,
    source: &str,
) -> (Option<usize>, Option<String>) {
    let offset = |sign: isize, digits: &str| {
        digits
            .parse::<isize>()
            .ok()
            .map(|n| (directive_line as isize + sign * n).max(1) as usize)
    };
    if let Some(digits) = token.strip_prefix('+') {
        return (offset(1, digits), None);
    }
    if let Some(digits) = token.strip_prefix('-') {
        return (offset(-1, digits), None);
    }
    if let Some(name) = token.strip_prefix('#') {
        return match anchors(source).get(name) {
            Some(line) => (Some(*line), None),
            None => (None, Some(format!("the anchor `#{name}` is not defined"))),
        };
    }
    match token.parse::<usize>() {
        Ok(line) => (Some(line), None),
        Err(_) => (
            None,
            Some(format!("the location `@{token}` is not understood")),
        ),
    }
}

// ---------------------------------------------------------------------------
// the corpus
// ---------------------------------------------------------------------------

/// One `.c` file of the suite.
struct TestFile {
    /// `C99/n617.c`.
    name: String,
    dir: &'static str,
    /// `n617` — the paper number, which is the row of `doc/c-status.md`.
    stem: String,
    source: String,
    revisions: Vec<Revision>,
    annotations: Vec<Annotation>,
}

/// Reads every `.c` in the four directories.
fn load_files(corpus: &Path) -> Result<Vec<TestFile>> {
    let mut files = Vec::new();
    for dir in DIRS {
        let path = corpus.join(dir);
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&path)
            .map_err(|err| eyre!("reading {}: {err}", path.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<_>>()?;
        entries.sort();
        for file in entries {
            if file.extension().is_none_or(|ext| ext != "c") {
                continue;
            }
            let stem = file
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| eyre!("{} has no usable name", file.display()))?
                .to_owned();
            // A couple of these files are deliberately not UTF-8 — they are
            // about extended characters — and the C has to travel through a
            // Rust string literal to reach the front end.
            let Ok(source) = std::fs::read_to_string(&file) else {
                files.push(TestFile {
                    name: format!("{dir}/{stem}.c"),
                    dir,
                    stem,
                    source: String::new(),
                    annotations: Vec::new(),
                    revisions: vec![Revision {
                        index: 0,
                        plan: Err("the file is not valid UTF-8".to_owned()),
                    }],
                });
                continue;
            };
            let commands = run_lines(&source);
            let has_verify = commands.iter().any(|line| line.contains("-verify"));
            let revisions = commands
                .iter()
                .enumerate()
                .map(|(index, command)| Revision::parse(index, command, &path, has_verify))
                .collect();
            files.push(TestFile {
                name: format!("{dir}/{stem}.c"),
                dir,
                stem,
                annotations: annotations(&source),
                source,
                revisions,
            });
        }
    }
    Ok(files)
}

/// The text after each `RUN:` in the file.
///
/// `lit` accepts a RUN line anywhere in a comment, and several of these files
/// put a run of them inside one `/* … */`, so the search is for the marker
/// rather than for a comment shape.
fn run_lines(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| line.split_once("RUN:"))
        .map(|(_, rest)| rest.trim().trim_end_matches("*/").trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// the front end, in process
// ---------------------------------------------------------------------------

/// One error the front end reported.
struct Reported {
    /// The line of the *upstream* file, or `None` for an error in a header or
    /// in the harness's own prelude.
    line: Option<usize>,
    /// The message, without the position it ends with.
    message: String,
}

/// Stack for the thread the front end runs on.
///
/// `cinrs` gives its own passes a large stack for the same reason: recursive
/// descent turns nesting in the input into stack frames, and a debug build
/// spends tens of kilobytes on each.
const WORKER_STACK: usize = 64 << 20;

/// Runs the whole front end over one unit and returns the errors it found.
///
/// Capture, preprocessing, parsing *and* semantic analysis, which is what
/// `cinrs_core::expand` does before code generation; `analyze` alone stops
/// after parsing and would miss every constraint violation, which is most of
/// what this suite is about.
///
/// `prelude_lines` is how many lines the harness put in front of the case, so
/// that a reported position comes back as a line of the upstream file.
fn front_end_errors(unit: String, options: Options, prelude_lines: usize) -> Result<Vec<Reported>> {
    let worker = std::thread::Builder::new()
        .name("clang-c-front-end".to_owned())
        .stack_size(WORKER_STACK)
        .spawn(move || {
            let hashes = "#".repeat(raw_string_hashes(&unit));
            let literal = format!("r{hashes}\"{unit}\"{hashes}");
            let stream = match proc_macro2::TokenStream::from_str(&literal) {
                Ok(stream) => stream,
                Err(err) => {
                    return vec![Reported {
                        line: None,
                        message: format!("the C could not be put in a Rust string literal: {err}"),
                    }];
                }
            };
            let analysis = cinrs_core::analyze(stream, &options);
            let unit_id = analysis.source.unit_id();
            let mut diagnostics = analysis.diagnostics;
            let (_program, mut sema) = cinrs_core::sema::analyze(&analysis.unit, &options, unit_id);
            analysis.expansions.annotate(&mut sema);
            diagnostics.extend(sema);

            let map = &analysis.source.map;
            let mut out: Vec<Reported> = diagnostics
                .sorted()
                .into_iter()
                .filter(|diag| diag.level == Level::Error)
                .map(|diag| {
                    let message = conformance::strip_position(&diag.message).to_owned();
                    if let Some((header, line, _)) = map.header_position(diag.range.start) {
                        return Reported {
                            line: None,
                            message: format!("{header}:{line}: {message}"),
                        };
                    }
                    let (line, _) = map.line_col(diag.range.start);
                    Reported {
                        line: line.checked_sub(prelude_lines).filter(|line| *line > 0),
                        message,
                    }
                })
                .collect();
            // A fatal diagnostic — the input could not be captured at all — is
            // not in `items()`, and losing it would look like acceptance.
            if out.is_empty() && diagnostics.has_errors() {
                out.push(Reported {
                    line: None,
                    message: "the macro input could not be read".to_owned(),
                });
            }
            out
        })?;
    worker
        .join()
        .map_err(|_| eyre!("the front-end worker thread panicked"))
}

// ---------------------------------------------------------------------------
// what one revision is asking for
// ---------------------------------------------------------------------------

/// The lines a revision requires an error on, and the ones where one is
/// allowed but not required.
struct Oracle {
    required: BTreeSet<usize>,
    optional: BTreeSet<usize>,
    /// A directive whose line could not be worked out at all.
    unresolved: Option<String>,
}

impl Oracle {
    fn read(file: &TestFile, plan: &Plan) -> Oracle {
        let mut out = Oracle {
            required: BTreeSet::new(),
            optional: BTreeSet::new(),
            unresolved: None,
        };
        for annotation in &file.annotations {
            if !plan.prefixes.contains(&annotation.prefix) {
                continue;
            }
            if annotation.kind != "error" {
                continue;
            }
            if let Some(reason) = &annotation.unresolved {
                out.unresolved.get_or_insert_with(|| reason.clone());
                continue;
            }
            let Some(line) = annotation.line else {
                continue;
            };
            if annotation.optional {
                out.optional.insert(line);
            } else {
                out.required.insert(line);
            }
        }
        out
    }
}

/// How a revision came out.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Class {
    /// Clang wants no error and there was none, and the expansion compiled.
    AcceptedAsRequired,
    /// Clang wants errors on a set of lines, and that is what came out.
    RejectedAsRequired,
    /// Clang wants errors and `cinrs` reported none: invalid code accepted.
    MissedRejection,
    /// Clang wants no error and `cinrs` reported one.
    FalseRejection(String),
    /// Errors, but not on the lines asked for.
    WrongLine {
        expected: Option<usize>,
        got: Option<usize>,
    },
    /// Valid C whose expansion `rustc` refused.
    RustCompileError(String),
    /// Not run, and why.
    Skipped(String),
}

impl Class {
    /// Whether this is the answer the test wanted.
    fn is_pass(&self) -> bool {
        matches!(
            self,
            Class::AcceptedAsRequired | Class::RejectedAsRequired | Class::Skipped(_)
        )
    }

    /// Whether the revision was run at all.
    fn ran(&self) -> bool {
        !matches!(self, Class::Skipped(_))
    }

    /// The name of the class, without the detail — the report groups by this.
    fn name(&self) -> &'static str {
        match self {
            Class::AcceptedAsRequired => "accepted as required",
            Class::RejectedAsRequired => "rejected as required (lines match)",
            Class::MissedRejection => "missed rejection (accepted invalid code)",
            Class::FalseRejection(_) => "false rejection (rejected valid code)",
            Class::WrongLine { .. } => "wrong line",
            Class::RustCompileError(_) => "rust compile error (valid C, invalid expansion)",
            Class::Skipped(_) => "skipped",
        }
    }

    /// The whole answer, detail and all.
    fn describe(&self) -> String {
        match self {
            Class::FalseRejection(message) => {
                format!("false rejection (rejected valid code: {message})")
            }
            Class::WrongLine { expected, got } => {
                let show = |line: &Option<usize>| match line {
                    Some(line) => line.to_string(),
                    None => "none".to_owned(),
                };
                format!(
                    "wrong line (expected {}, got {})",
                    show(expected),
                    show(got)
                )
            }
            Class::RustCompileError(message) => {
                format!("rust compile error (valid C, invalid expansion): {message}")
            }
            Class::Skipped(reason) => format!("skipped ({reason})"),
            other => other.name().to_owned(),
        }
    }
}

/// One revision with its result.
struct Result_ {
    /// `C99/n617.c` or `drs/dr0xx.c:3`.
    id: String,
    /// `C99`, `C11`, `C23`, `drs`.
    dir: &'static str,
    /// The paper the file is named for.
    stem: String,
    /// Which entry point it used, when it used one.
    entry: Option<&'static str>,
    class: Class,
}

/// The id of one revision of one file.
///
/// A file with a single RUN line is named by itself, which is what makes the
/// list readable; one with several gets the index of the line, counting from
/// zero, because two RUN lines may well use the same `-std=`.
fn revision_id(file: &TestFile, revision: &Revision) -> String {
    if file.revisions.len() == 1 {
        file.name.clone()
    } else {
        format!("{}:{}", file.name, revision.index)
    }
}

// ---------------------------------------------------------------------------
// running one revision
// ---------------------------------------------------------------------------

/// The whole translation unit for a revision: prelude, case, module pragma.
fn unit_text(file: &TestFile, plan: &Plan) -> (String, usize) {
    let prelude = plan.prelude();
    let mut out = String::with_capacity(file.source.len() + 128);
    for line in &prelude {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&file.source);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!("\n#pragma cinrs module {MODULE:?}\n"));
    (out, prelude.len())
}

/// Classifies one revision from what the front end said.
///
/// Returns `Ok(class)` when the answer is final, and `Err(unit)` when the
/// revision is an accept case that got past the front end and now has to be
/// put through `rustc`.
fn classify(
    file: &TestFile,
    plan: &Plan,
    oracle: &Oracle,
    errors: &[Reported],
) -> std::result::Result<Class, String> {
    let (unit, _) = unit_text(file, plan);
    if oracle.required.is_empty() {
        return match errors.first() {
            None => Err(unit),
            Some(first) => Ok(Class::FalseRejection(first.message.clone())),
        };
    }
    if errors.is_empty() {
        return Ok(Class::MissedRejection);
    }
    let reported: BTreeSet<usize> = errors.iter().filter_map(|error| error.line).collect();
    let allowed: BTreeSet<usize> = oracle.required.union(&oracle.optional).copied().collect();
    let missing = oracle.required.difference(&reported).next().copied();
    let extra = reported.difference(&allowed).next().copied();
    // An error the harness could not place — inside a header, or in the
    // prelude — is an error somewhere other than a line that was asked for.
    let unplaced = errors.iter().any(|error| error.line.is_none());
    if missing.is_none() && extra.is_none() && !unplaced {
        return Ok(Class::RejectedAsRequired);
    }
    Ok(Class::WrongLine {
        expected: missing.or_else(|| oracle.required.first().copied()),
        got: extra.or_else(|| reported.first().copied()),
    })
}

// ---------------------------------------------------------------------------
// the `rustc` half
// ---------------------------------------------------------------------------

/// The `//@check-pass` file for one accept case.
///
/// `--crate-type lib`, because a translation unit of declarations has no
/// `fn main` and the default crate type is a binary: without it every single
/// case fails with `E0601`, which says nothing about the C at all.
/// `-Awarnings` because these are a hundred generated crates and a warning in
/// one of them would be compared against a `.stderr` file that does not
/// exist.
fn generate_check_pass(name: &str, unit: &str, entry: &str, source: &str) -> String {
    let hashes = "#".repeat(raw_string_hashes(unit));
    format!(
        "\
// GENERATED FILE - do not edit, do not check in.
//
// Written by `tests/clang_c.rs` from `{CORPUS}/{source}`.
//
// This revision of that test expects no error, so the C is valid and its
// expansion has to compile, which is what the check-pass command below
// asserts. (Do not write that command's spelling anywhere but on its own
// line: ui_test scans every line of this file and refuses one that looks
// like a command it cannot obey.)
//
// LLVM is Apache-2.0 with LLVM exceptions; nothing from it is copied into this
// repository, and this file lives in `target/`.
//
// Regenerate by running `cargo test --test clang_c`.
// Revision: {name}

//@check-pass
//@edition: 2024
//@compile-flags: --crate-type lib -Awarnings

mod unit {{
    cinrs::{entry}! {{ r{hashes}\"{unit}\"{hashes} }}
}}
"
    )
}

/// A generated file's name, which is also how a `ui_test` result is matched
/// back to the revision it came from.
fn check_pass_stem(file: &TestFile, revision: &Revision) -> String {
    format!("{}__{}__{}", file.dir, file.stem, revision.index)
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    conformance::start_memory_watchdog("clang-c");
    if let Some(result) = conformance::skip_unless_measured_build("clang-c") {
        return result;
    }
    let corpus = Path::new(CORPUS);
    if !corpus.is_dir() {
        let message = format!(
            "the Clang C conformance corpus is not checked out at `{CORPUS}`.\n\
             Fetch it with\n\n    {FETCH_HINT}\n\n\
             and run the test again; see doc/clang-c-tests.md."
        );
        if flag("CINRS_TESTSUITES_REQUIRED") {
            return Err(eyre!(
                "{message}\n\
                 (CINRS_TESTSUITES_REQUIRED=1, so this is a failure and not a skip.)"
            ));
        }
        println!("clang-c: skipped — {message}");
        return Ok(());
    }
    let corpus = std::env::current_dir()?.join(corpus);

    let started = Instant::now();
    let filter = std::env::var("CINRS_CLANG_C_FILTER").unwrap_or_default();
    let files = load_files(&corpus)?;
    let list = Path::new(LIST);
    let expected_failures = read_list(list)?;

    println!(
        "clang-c: {} files, {} RUN lines{}",
        files.len(),
        files.iter().map(|file| file.revisions.len()).sum::<usize>(),
        if filter.is_empty() {
            String::new()
        } else {
            format!(", filtered by {filter:?}")
        },
    );

    // --- the front end, in process ------------------------------------------
    let mut results: Vec<Result_> = Vec::new();
    let mut pending: Vec<(usize, String, String, String)> = Vec::new();
    for file in &files {
        for revision in &file.revisions {
            let id = revision_id(file, revision);
            if !filter.is_empty() && !id.contains(&filter) {
                continue;
            }
            let plan = match &revision.plan {
                Ok(plan) => plan,
                Err(reason) => {
                    results.push(Result_ {
                        id,
                        dir: file.dir,
                        stem: file.stem.clone(),
                        entry: None,
                        class: Class::Skipped(reason.clone()),
                    });
                    continue;
                }
            };
            let oracle = Oracle::read(file, plan);
            if let Some(reason) = &oracle.unresolved {
                results.push(Result_ {
                    id,
                    dir: file.dir,
                    stem: file.stem.clone(),
                    entry: Some(plan.entry.name),
                    class: Class::Skipped(reason.clone()),
                });
                continue;
            }
            let (unit, prelude_lines) = unit_text(file, plan);
            let errors = front_end_errors(unit, plan.entry.options(), prelude_lines)?;
            let class = match classify(file, plan, &oracle, &errors) {
                Ok(class) => class,
                Err(unit) if confuses_ui_test(&unit) => {
                    // The C goes into the generated file verbatim, and
                    // `ui_test` reads every line of that file: a line opening
                    // with `//@`, or a `//` followed by one of the sigils it
                    // reserves, is a command it will refuse rather than C.
                    // The front end has already had its say, so what is lost
                    // is only the `rustc` half.
                    Class::Skipped(
                        "a line of the C would be read as a `ui_test` command".to_owned(),
                    )
                }
                Err(unit) => {
                    // An accept case that got past the front end: `rustc` has
                    // the last word on it.
                    pending.push((
                        results.len(),
                        check_pass_stem(file, revision),
                        unit,
                        plan.entry.name.to_owned(),
                    ));
                    Class::AcceptedAsRequired
                }
            };
            results.push(Result_ {
                id,
                dir: file.dir,
                stem: file.stem.clone(),
                entry: Some(plan.entry.name),
                class,
            });
        }
    }

    // --- the accept cases, through `rustc` ----------------------------------
    let gen_root = Path::new(GEN_DIR);
    if gen_root.exists() {
        std::fs::remove_dir_all(gen_root)?;
    }
    std::fs::create_dir_all(gen_root)?;
    let mut by_stem: BTreeMap<String, usize> = BTreeMap::new();
    for (at, stem, unit, entry) in &pending {
        let source = results[*at]
            .id
            .split(':')
            .next()
            .unwrap_or_default()
            .to_owned();
        std::fs::write(
            gen_root.join(format!("{stem}.rs")),
            generate_check_pass(&results[*at].id, unit, entry, &source),
        )?;
        by_stem.insert(stem.clone(), *at);
    }
    if !pending.is_empty() {
        println!(
            "clang-c: compiling {} accept cases…",
            plural(pending.len(), "case", "cases")
        );
        let collector = Collector::default();
        let mut config =
            conformance::test_config(gen_root, Path::new(BUILD_DIR), COMPILE_TIMEOUT, &[])?;
        let mut args = Args::test()?;
        conformance::cap_threads(&mut args)?;
        config.with_args(&args);
        // A failure here is a failure of the *run*, not of a case: the
        // dependency would not build, `rustc` is not there. Saying so beats
        // reporting sixty cases as broken.
        if let Err(err) = conformance::run(config, Box::new(()), &collector) {
            println!("clang-c: warning: the accept cases could not all be run: {err}");
        }
        for (stem, outcome) in collector.results() {
            let Some(at) = by_stem.get(&stem) else {
                continue;
            };
            if let Outcome::Failed { detail, .. } = outcome {
                results[*at].class = Class::RustCompileError(detail);
            }
        }
    }

    let took = duration(started.elapsed());
    if flag("CINRS_CLANG_C_REPORT") {
        report(
            &results,
            &expected_failures,
            list,
            &took,
            flag("CINRS_CLANG_C_UPDATE_EXPECTED"),
        )
    } else {
        guard(
            &results,
            &expected_failures,
            list,
            &took,
            flag("CINRS_CLANG_C_STRICT"),
            !filter.is_empty(),
        )
    }
}

/// Report mode: say what happened, fail nothing.
fn report(
    results: &[Result_],
    expected_failures: &BTreeMap<String, Entry>,
    list: &Path,
    took: &str,
    update: bool,
) -> Result<()> {
    let ran: Vec<&Result_> = results.iter().filter(|r| r.class.ran()).collect();
    let passed = ran.iter().filter(|r| r.class.is_pass()).count();
    println!();
    println!(
        "clang/test/C: {passed}/{} revisions came out as required ({}), \
         {} skipped — {took}",
        ran.len(),
        percent(passed, ran.len()),
        results.len() - ran.len(),
    );

    println!();
    println!("  by directory and outcome");
    for dir in DIRS {
        let here: Vec<&Result_> = results.iter().filter(|r| r.dir == *dir).collect();
        let ran = here.iter().filter(|r| r.class.ran()).count();
        let ok = here
            .iter()
            .filter(|r| r.class.ran() && r.class.is_pass())
            .count();
        println!(
            "    {dir:<6}{ok:>4}/{ran:<4}  {:>6}   ({} revisions, {} skipped)",
            percent(ok, ran),
            here.len(),
            here.len() - ran,
        );
        let mut classes: BTreeMap<&str, usize> = BTreeMap::new();
        for result in &here {
            *classes.entry(result.class.name()).or_default() += 1;
        }
        for (class, count) in classes {
            println!("        {count:>4}  {class}");
        }
    }

    let skipped: Vec<(&str, String)> = results
        .iter()
        .filter_map(|r| match &r.class {
            Class::Skipped(reason) => Some((r.id.as_str(), reason.clone())),
            _ => None,
        })
        .collect();
    if !skipped.is_empty() {
        println!();
        println!("  skipped revisions ({})", skipped.len());
        let borrowed: Vec<(&str, &str)> = skipped
            .iter()
            .map(|(id, reason)| (*id, reason.as_str()))
            .collect();
        for group in group_texts(borrowed) {
            println!("    {:>4}  {}", group.count(), group.cause);
            println!("          e.g. {}", group.example);
        }
    }

    let mismatches: Vec<&Result_> = ran.iter().copied().filter(|r| !r.class.is_pass()).collect();
    if !mismatches.is_empty() {
        println!();
        println!("  mismatches ({})", mismatches.len());
        for result in &mismatches {
            println!(
                "    {:<28} {}  [{}]",
                result.id,
                result.stem,
                result.entry.unwrap_or("-")
            );
            println!("        {}", result.class.describe());
        }
    }

    if update {
        let outcomes = as_outcomes(results);
        let written = write_list(list, &list_header(), &outcomes, expected_failures)?;
        println!();
        println!(
            "clang-c: wrote {written} expected failures to {}",
            list.display()
        );
    }
    println!();
    Ok(())
}

/// Guard mode: every revision not listed must come out as required.
fn guard(
    results: &[Result_],
    expected_failures: &BTreeMap<String, Entry>,
    list: &Path,
    took: &str,
    strict: bool,
    filtered: bool,
) -> Result<()> {
    let listed = |id: &str| expected_failures.get(id);
    let mut failures: Vec<String> = Vec::new();
    let mut now_passing: Vec<&str> = Vec::new();
    let mut nonconforming: Vec<String> = Vec::new();

    for result in results {
        let entry = listed(&result.id);
        match entry.map(|entry| &entry.kind) {
            // Guarded neither way.
            Some(EntryKind::ToolchainDependent) => {}
            // `!`: `cinrs` refuses something Clang accepts, and is right to.
            // The assertion is that it still does.
            Some(EntryKind::Rejected { .. }) => {
                if !matches!(result.class, Class::FalseRejection(_)) {
                    nonconforming.push(format!(
                        "{} in {} is marked `!`, which says cinrs refuses this revision on \
                         purpose, but it came out as `{}`. Either the entry is wrong or the \
                         deliberate refusal has gone.",
                        result.id,
                        list.display(),
                        result.class.describe()
                    ));
                }
            }
            Some(EntryKind::Failure) => {
                if result.class.is_pass() {
                    now_passing.push(&result.id);
                }
            }
            None => {
                if !result.class.is_pass() {
                    failures.push(format!("    {}: {}", result.id, result.class.describe()));
                }
            }
        }
    }

    let ran = results.iter().filter(|r| r.class.ran()).count();
    println!(
        "clang-c: {ran} revisions run, {} listed as failing in {} — {took}",
        expected_failures.len(),
        list.display()
    );

    let ids: BTreeSet<&str> = results.iter().map(|r| r.id.as_str()).collect();
    let unknown: Vec<&str> = expected_failures
        .keys()
        .map(String::as_str)
        .filter(|id| !ids.contains(id))
        .collect();

    let mut problems = Vec::new();
    if !now_passing.is_empty() {
        problems.push(listing(
            &format!(
                "{} in {} now comes out as required; delete the line",
                plural(now_passing.len(), "revision", "revisions"),
                list.display()
            ),
            now_passing.into_iter(),
        ));
    }
    if !unknown.is_empty() && !filtered {
        problems.push(listing(
            &format!(
                "{} in {} does not name a revision this run had",
                plural(unknown.len(), "entry", "entries"),
                list.display()
            ),
            unknown.into_iter(),
        ));
    }
    for problem in &problems {
        println!("clang-c: warning: {problem}");
    }
    if !problems.is_empty() {
        println!("clang-c: CINRS_CLANG_C_STRICT=1 makes the above a failure.");
    }

    if !failures.is_empty() {
        return Err(eyre!(
            "{} not listed in {} did not come out as required:\n{}\n\n\
             Run `CINRS_CLANG_C_REPORT=1 cargo test --test clang_c` to see them in context, \
             and `CINRS_CLANG_C_UPDATE_EXPECTED=1` as well to rewrite the list.",
            plural(failures.len(), "revision", "revisions"),
            list.display(),
            failures.join("\n"),
        ));
    }
    if !nonconforming.is_empty() {
        return Err(eyre!("{}", nonconforming.join("\n")));
    }
    if strict && !problems.is_empty() {
        return Err(eyre!("{}", problems.join("\n")));
    }
    println!("clang-c: all {} revisions accounted for", results.len());
    Ok(())
}

/// The results in the shape [`write_list`] wants.
fn as_outcomes(results: &[Result_]) -> BTreeMap<String, Outcome> {
    results
        .iter()
        .map(|result| {
            let outcome = if result.class.is_pass() {
                Outcome::Passed
            } else {
                Outcome::Failed {
                    classification: result.class.describe(),
                    detail: result.class.describe(),
                    output: String::new(),
                }
            };
            (result.id.clone(), outcome)
        })
        .collect()
}

/// The comment block written above the list.
fn list_header() -> String {
    let regenerate = "#     CINRS_CLANG_C_REPORT=1 CINRS_CLANG_C_UPDATE_EXPECTED=1 \\\n\
                      #         cargo test --test clang_c\n";
    format!(
        "# The `clang/test/C` revisions whose answer `cinrs` does not match.\n\
         #\n\
         # One line per *revision* — one `// RUN:` line of one file — named\n\
         # `<dir>/<file>.c` when the file has a single RUN line and\n\
         # `<dir>/<file>.c:<n>` when it has more. Guard mode requires every\n\
         # revision not listed here to come out as the test asks; see\n\
         # `doc/clang-c-tests.md` for the outcome classes and the results.\n\
         #\n\
         # The corpus is not checked in: `scripts/fetch-testsuites.sh llvm` makes\n\
         # a sparse checkout of it, and without it the harness skips itself. This\n\
         # file therefore holds names and notes and nothing of LLVM's own.\n\
         #\n\
         # `!` here means `cinrs` refuses the revision on purpose and is right to\n\
         # — a Clang extension it declines rather than mistranslates. Guard mode\n\
         # asserts the refusal is still there.\n\
         {}",
        marker_legend(regenerate)
    )
}
