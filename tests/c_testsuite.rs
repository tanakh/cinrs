//! Conformance measured against [c-testsuite].
//!
//! [c-testsuite] is a collaborative database of C compiler test cases. Its
//! `single-exec` suite is 220 standalone programs, each with a `main`, an
//! `.expected` file holding the output the program must produce, and a
//! `.tags` file saying which revision of the language it needs and what it
//! depends on. Its own runners compile the file, run it, require exit status
//! zero and diff the output. This harness does the same through `cinrs`, one
//! generated Rust file per case, so that "how much of C99 does the front end
//! actually get right" is a number rather than an impression — and so that it
//! stops going down.
//!
//! The corpus is a git submodule at `third_party/c-testsuite`, pinned to an
//! upstream commit. Without it there is nothing to run, so the harness prints
//! how to fetch it and exits successfully; `CINRS_CTESTSUITE_REQUIRED=1`
//! turns that skip into a failure, which is what a CI job that means to run
//! it wants. `doc/c-testsuite.md` has the licence note and the recorded
//! baseline.
//!
//! # The two modes
//!
//! **Guard mode** is the default, and is what `cargo test` runs. Every
//! selected case that is *not* listed in
//! `tests/c-testsuite/expected-failures.txt` is generated and must pass. The
//! listed ones are generated into a directory of their own and run as well,
//! but only so that one which has started passing can be reported — as a
//! warning, or as a failure under `CINRS_CTESTSUITE_STRICT=1` — so that the
//! list gets pruned instead of quietly rotting.
//!
//! **Report mode** (`CINRS_CTESTSUITE_REPORT=1`) generates every selected
//! case, runs them all, never fails the process, and prints the pass rate
//! overall and per tag together with a classified list of the failures.
//! `CINRS_CTESTSUITE_UPDATE_EXPECTED=1` then rewrites the expected-failure
//! list from what it saw, keeping the notes already written against ids that
//! still fail.
//!
//! # The three markers
//!
//! A line of an expected-failure list is `[marker]NNNNN  <note>`, and the
//! marker says what kind of claim the line is making. See [`EntryKind`].
//!
//! | marker | means | guard mode | report mode |
//! | --- | --- | --- | --- |
//! | none | `cinrs` gets it wrong | skipped; reported if it starts passing | counted as a failure |
//! | `?` | the answer depends on the toolchain | guarded neither way | counted as it came out, listed separately |
//! | `!` | the standard requires it to be *refused* | **asserted** to fail to compile | counted separately, not as a failure |
//!
//! A `!` line may name the diagnostic it must be refused with, as
//! `!NNNNN  error: "<substring>"  <note>`; guard mode then also requires the
//! compiler's output to contain that substring. `!` is for a case the entry
//! point makes invalid — `00209` calls an `int (*fp)();` with an argument,
//! which is fine through C17 and which C23 removed — so refusing it is
//! conforming behaviour and not a gap, and a `!` case that *compiles and runs*
//! fails the guard.
//!
//! # What is generated
//!
//! For each selected case, `target/c-testsuite/<standard>/NNNNN.rs` — a fresh
//! directory per run, so a case that leaves the corpus leaves the run — with
//! the C source verbatim inside a raw string literal (string-literal input
//! accepts C the Rust lexer would refuse, and the `#` count is computed so
//! that the literal cannot terminate early), a `#pragma cinrs module "ctest"`
//! appended after it, and a Rust `fn main` that calls the C `main` through
//! that module and exits with what it returned. The pragma goes at the *end*
//! because the preprocessor acts on it wherever it reads it, and appending
//! leaves every line number of the original file alone — which is what makes
//! a diagnostic point at the line the upstream file has.
//!
//! The expansion is wrapped in a `mod unit` of its own. The unit exports a
//! function called `main`, and glob re-exporting that into the crate root
//! next to the harness's own `fn main` is a warning, which would then have to
//! be blessed into a `.stderr` file for every single case.
//!
//! # Timeouts
//!
//! A miscompiled program may loop forever, and `ui_test` has no per-test
//! timeout. Two things keep that from wedging the run: the generated program
//! starts a watchdog thread that exits with status 124 after
//! `CINRS_CTESTSUITE_TIMEOUT` seconds (20 by default; `0` disables both
//! halves), and the compiler is invoked through `timeout(1)` where there is
//! one, in case the hang is in the macro rather than in the program. A
//! program killed by a signal — a stack overflow, an `abort` — is an ordinary
//! run failure, since its exit status is not zero.
//!
//! # Environment
//!
//! * `CINRS_CTESTSUITE_REQUIRED=1` — fail rather than skip when the corpus is
//!   missing.
//! * `CINRS_CTESTSUITE_STANDARD=c99|c11|c23` — which entry point to translate
//!   with, and which cases are eligible. Default `c99`.
//! * `CINRS_CTESTSUITE_FILTER=<substring>` — only cases whose id contains it.
//! * `CINRS_CTESTSUITE_REPORT=1` — report mode.
//! * `CINRS_CTESTSUITE_STRICT=1` — a stale expected-failure entry, one that
//!   now passes or one that names nothing, is a failure and not a warning.
//! * `CINRS_CTESTSUITE_UPDATE_EXPECTED=1` — rewrite the list from the report.
//! * `CINRS_CTESTSUITE_TIMEOUT=<seconds>` — per-test timeout; `0` disables.
//!
//! [c-testsuite]: https://github.com/c-testsuite/c-testsuite

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ui_test::color_eyre::eyre::{Result, eyre};
use ui_test::custom_flags::edition::Edition;
use ui_test::dependencies::DependencyBuilder;
use ui_test::status_emitter::{RevisionStyle, StatusEmitter, Summary, TestStatus};
use ui_test::test_result::{TestOk, TestResult};
use ui_test::{Args, Config, Error, error_on_output_conflict, run_tests_generic};

// ---------------------------------------------------------------------------
// where things live
// ---------------------------------------------------------------------------

/// The corpus, relative to the package root — which is `cargo`'s working
/// directory for a test binary.
const SUITE_DIR: &str = "third_party/c-testsuite/tests/single-exec";

/// What to tell someone who has not fetched the submodule.
const FETCH_HINT: &str = "git submodule update --init third_party/c-testsuite";

/// Where the expected-failure lists live.
const LIST_DIR: &str = "tests/c-testsuite";

/// Where the generated `.rs` files go.
const GEN_DIR: &str = "target/c-testsuite";

/// Where `ui_test` puts what it builds.
const BUILD_DIR: &str = "target/c-testsuite-build";

/// Where a generated program runs.
///
/// `ui_test` runs the binary it built with the package root as the working
/// directory, and one case in the corpus writes a file relative to it. Each
/// program is given a subdirectory of its own here instead, so that a test run
/// never leaves anything in the source tree.
const WORK_DIR: &str = "target/c-testsuite-work";

/// How long a generated program may run before it is killed, in seconds.
const DEFAULT_TIMEOUT: u64 = 20;

/// How long the *compiler* may take on one case, in seconds.
///
/// Much larger than [`DEFAULT_TIMEOUT`]: a cold run pays for the dependency
/// build, and a macro that is merely slow should be seen as slow rather than
/// mistaken for one that has hung.
const COMPILE_TIMEOUT: u64 = 300;

// ---------------------------------------------------------------------------
// the standards
// ---------------------------------------------------------------------------

/// The entry point a run translates with, which is also the newest revision a
/// case may ask for.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Standard {
    C99,
    C11,
    C23,
    /// `gnu99!`, which is `c99!` with the GNU extensions switched on. It sorts
    /// above every strict revision because it accepts what all of them do.
    Gnu99,
    Gnu11,
    Gnu23,
}

impl Standard {
    /// The name in `CINRS_CTESTSUITE_STANDARD`, the `cinrs` entry point and
    /// the directory component, which are all the same string.
    fn name(self) -> &'static str {
        match self {
            Standard::C99 => "c99",
            Standard::C11 => "c11",
            Standard::C23 => "c23",
            Standard::Gnu99 => "gnu99",
            Standard::Gnu11 => "gnu11",
            Standard::Gnu23 => "gnu23",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        match s {
            "c99" => Ok(Standard::C99),
            "c11" => Ok(Standard::C11),
            "c23" => Ok(Standard::C23),
            "gnu99" => Ok(Standard::Gnu99),
            "gnu11" => Ok(Standard::Gnu11),
            "gnu23" => Ok(Standard::Gnu23),
            other => Err(eyre!(
                "CINRS_CTESTSUITE_STANDARD={other}: expected c99, c11, c23, gnu99, gnu11 \
                 or gnu23"
            )),
        }
    }

    /// The revision a `cNN` tag asks for.
    ///
    /// The corpus documents `c89` as implying `c99` and `c99` as implying
    /// `c11`: a tag names the *oldest* revision the program is valid in, so a
    /// C89 program runs everywhere. `c89` and `c99` therefore both come out as
    /// "C99 is enough".
    fn of_tag(tag: &str) -> Option<Self> {
        match tag {
            "c89" | "c99" => Some(Standard::C99),
            "c11" | "c17" => Some(Standard::C11),
            "c23" => Some(Standard::C23),
            _ => None,
        }
    }

    /// Whether a case tagged for `needs` may be translated by this entry
    /// point.
    ///
    /// A GNU dialect accepts everything a later revision added, so every case
    /// is eligible for one however it is tagged.
    fn accepts(self, needs: Standard) -> bool {
        matches!(self, Standard::Gnu99 | Standard::Gnu11 | Standard::Gnu23) || self >= needs
    }
}

// ---------------------------------------------------------------------------
// the corpus
// ---------------------------------------------------------------------------

/// One case: `NNNNN.c`, `NNNNN.c.expected` and `NNNNN.c.tags`.
struct Test {
    /// The file stem, `00001`.
    id: String,
    /// The contents of the `.tags` file, one tag per line.
    tags: BTreeSet<String>,
    /// The C program.
    source: String,
    /// What running it must print.
    ///
    /// The corpus merges the program's stdout and stderr into this file. No
    /// case in it writes to stderr, so it is compared against stdout, with
    /// stderr required to be empty — which says the same thing, and says it
    /// in the two files `ui_test` compares.
    expected: Vec<u8>,
}

impl Test {
    /// The tags, for a report line.
    fn tag_list(&self) -> String {
        if self.tags.is_empty() {
            "no tags".to_owned()
        } else {
            self.tags.iter().cloned().collect::<Vec<_>>().join(" ")
        }
    }
}

/// Reads every `NNNNN.c` in `dir`, with the files that go with it.
fn load_tests(dir: &Path) -> Result<Vec<Test>> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|err| eyre!("reading {}: {err}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<_>>()?;
    entries.sort();

    let mut tests = Vec::new();
    for path in entries {
        if path.extension().is_none_or(|ext| ext != "c") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| eyre!("{} has no usable name", path.display()))?
            .to_owned();
        let source = std::fs::read_to_string(&path)
            .map_err(|err| eyre!("reading {}: {err}", path.display()))?;
        let expected = std::fs::read(dir.join(format!("{id}.c.expected"))).unwrap_or_default();
        let tags = std::fs::read_to_string(dir.join(format!("{id}.c.tags")))
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        tests.push(Test {
            id,
            tags,
            source,
            expected,
        });
    }
    Ok(tests)
}

/// Whether a case is eligible for this run.
///
/// Two rules, both out of the corpus's own tag vocabulary. A case has to be
/// runnable on this machine: `portable`, or an `arch-…` tag naming the host
/// (a case with neither kind of tag makes no claim, and is kept). And it must
/// not need a revision of C newer than the entry point being used, so that a
/// `c11`-tagged program is not fed to `c99!` only to be told that anonymous
/// members need C11.
///
/// Nothing is excluded by the two remaining tags. `needs-cpp` is fine —
/// `cinrs` has the whole C99 preprocessor — and so is `needs-libc`, since the
/// bundled headers declare the platform's real library and the calls link
/// against it. A program that needs a header `cinrs` does not bundle fails
/// like any other failing case rather than being filtered out of the count.
fn is_selected(test: &Test, standard: Standard) -> bool {
    let arch_tags = || test.tags.iter().filter(|tag| tag.starts_with("arch-"));
    let host_arch = format!("arch-{}", std::env::consts::ARCH);
    let runs_here = test.tags.contains("portable")
        || arch_tags().next().is_none()
        || arch_tags().any(|tag| *tag == host_arch);

    let needs = test.tags.iter().filter_map(|t| Standard::of_tag(t)).max();
    let recent_enough = needs.is_none_or(|needs| standard.accepts(needs));

    runs_here && recent_enough
}

// ---------------------------------------------------------------------------
// generating one Rust file
// ---------------------------------------------------------------------------

/// How the C program spells `main`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MainKind {
    /// `int main(void)`, or `int main()` with an empty identifier list.
    NoArgs,
    /// `int main(int argc, char **argv)`.
    ArgcArgv,
}

/// The C source with comments and literals blanked out.
///
/// Blanking rather than deleting keeps the length, so a position in the
/// result means the same thing in the original.
fn blank_out_comments_and_literals(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        let end = if rest.starts_with(b"/*") {
            source[i + 2..]
                .find("*/")
                .map_or(bytes.len(), |at| i + 2 + at + 2)
        } else if rest.starts_with(b"//") {
            source[i..].find('\n').map_or(bytes.len(), |at| i + at)
        } else if rest[0] == b'"' || rest[0] == b'\'' {
            // A string or character constant: skip to the closing quote,
            // letting a backslash escape whatever follows it.
            let quote = rest[0];
            let mut at = i + 1;
            while at < bytes.len() && bytes[at] != quote {
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            (at + 1).min(bytes.len())
        } else {
            // The source is UTF-8, so copy a whole character rather than a
            // byte.
            let ch = source[i..].chars().next().expect("i is a char boundary");
            out.push(ch);
            i += ch.len_utf8();
            continue;
        };
        // One space per *byte*, so that the two strings stay the same length.
        for _ in i..end {
            out.push(' ');
        }
        i = end;
    }
    out
}

/// How the program's `main` is declared, read off the text.
///
/// The *first* declaration wins. One case in the corpus (`00182`) carries a
/// second `main` inside an `#ifndef` that its own `#define` has already made
/// false; finding that out would mean running the preprocessor, and taking
/// the first is both simpler and right.
fn detect_main(source: &str) -> Option<MainKind> {
    let text = blank_out_comments_and_literals(source);
    let bytes = text.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut from = 0;
    while let Some(at) = text[from..].find("main") {
        let start = from + at;
        from = start + "main".len();
        if start > 0 && is_ident(bytes[start - 1]) {
            continue;
        }
        let Some(args) = text[from..].trim_start().strip_prefix('(') else {
            continue;
        };
        let Some(end) = args.find(')') else { continue };
        let args = args[..end].trim();
        return Some(if args.is_empty() || args == "void" {
            MainKind::NoArgs
        } else {
            MainKind::ArgcArgv
        });
    }
    None
}

/// How many `#` a raw string literal holding `source` needs.
///
/// A `r#…#"` literal ends at the first `"` followed by that many `#`, so one
/// more than the longest run of `#` that follows a `"` anywhere inside is
/// always enough — and never fewer than one, so that a bare `"` is safe.
fn raw_string_hashes(source: &str) -> usize {
    let bytes = source.as_bytes();
    let mut needed = 1;
    for at in bytes
        .iter()
        .enumerate()
        .filter_map(|(at, &b)| (b == b'"').then_some(at))
    {
        let run = bytes[at + 1..].iter().take_while(|&&b| b == b'#').count();
        needed = needed.max(run + 1);
    }
    needed
}

/// Whether a line of the C source would be read as a `ui_test` command.
///
/// `ui_test` scans every line of the generated file, and the C goes into it
/// verbatim, so `//@` at the start of a line — or `//` immediately followed
/// by one of the sigils it reserves — is a comment directive rather than C.
/// Nothing in the corpus does that; a case that did would be skipped with a
/// note rather than failing with a baffling parse error.
fn confuses_ui_test(source: &str) -> bool {
    source.lines().any(|line| {
        line.starts_with("//@")
            || line
                .match_indices("//")
                .any(|(at, _)| matches!(line.as_bytes().get(at + 2), Some(b'@' | b'~')))
    })
}

/// The Rust file for one case.
fn generate_source(
    test: &Test,
    standard: Standard,
    timeout: u64,
    main: MainKind,
    work_root: &Path,
) -> String {
    let mut c = test.source.clone();
    if !c.ends_with('\n') {
        c.push('\n');
    }
    // The blank line first: had the file ended in a backslash continuation, it
    // splices with that and not with the pragma.
    c.push_str("\n#pragma cinrs module \"ctest\"\n");
    let hashes = "#".repeat(raw_string_hashes(&c));

    let id = &test.id;
    let entry_point = standard.name();
    let tags = test.tag_list();
    // Absolute, so the program can be run from anywhere by hand as well.
    let work = work_root.join(id).display().to_string();

    // The whole translation unit goes into one raw string literal, which is the
    // input form that accepts every C token — the corpus has hexadecimal
    // floating constants and `'ab'` in it, and the Rust lexer refuses those.
    let mut out = format!(
        "\
// GENERATED FILE - do not edit, do not check in.
//
// Written by `tests/c_testsuite.rs` from `{SUITE_DIR}/{id}.c`.
// The output it must produce is in `{id}.run.stdout`, next to this file.
//
// The C below is the upstream case verbatim, with only a `#pragma cinrs
// module` appended, so every line number of the original still holds.
//
// c-testsuite is MIT licensed; the individual cases carry their own licences,
// recorded in the corpus's `.otags` files.
//
// Tags: {tags}
// Regenerate by running `cargo test --test c_testsuite`.

//@run
//@edition: 2024

mod unit {{
    cinrs::{entry_point}! {{ r{hashes}\"{c}\"{hashes} }}
}}

fn main() {{
    // `ui_test` runs this with the package root as the working directory, and
    // one case in the corpus writes a file relative to it.
    let work = std::path::Path::new({work:?});
    std::fs::create_dir_all(work).expect(\"the work directory\");
    std::env::set_current_dir(work).expect(\"the work directory\");
"
    );

    if timeout > 0 {
        out.push_str(&format!(
            "    // A miscompiled program need never come back; the harness must.
    std::thread::spawn(|| {{
        std::thread::sleep(std::time::Duration::from_secs({timeout}));
        eprintln!(\"cinrs c-testsuite: timed out after {timeout} seconds\");
        std::process::exit(124);
    }});
"
        ));
    }

    out.push_str(match main {
        MainKind::NoArgs => "    let status = unsafe { unit::ctest::main() };\n",
        MainKind::ArgcArgv => {
            "    // The corpus runs its cases with no arguments, so `argv` holds the
    // program name and the null pointer C requires after it.
    let mut arg0 = *b\"ctest\\0\";
    let mut argv: [*mut core::ffi::c_char; 2] =
        [arg0.as_mut_ptr().cast(), core::ptr::null_mut()];
    let status = unsafe { unit::ctest::main(1, argv.as_mut_ptr()) };\n"
        }
    });
    out.push_str("    std::process::exit(status as i32);\n}\n");
    out
}

/// A case that could not be turned into a Rust file, and why.
struct Skipped {
    id: String,
    reason: String,
}

/// Writes every case in `tests` into `dir`, which is emptied first.
fn generate(
    dir: &Path,
    tests: &[&Test],
    standard: Standard,
    timeout: u64,
    work_root: &Path,
) -> Result<Vec<Skipped>> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    let mut skipped = Vec::new();
    for test in tests {
        if confuses_ui_test(&test.source) {
            skipped.push(Skipped {
                id: test.id.clone(),
                reason: "its C source holds a line `ui_test` would read as a command".to_owned(),
            });
            continue;
        }
        let Some(main) = detect_main(&test.source) else {
            skipped.push(Skipped {
                id: test.id.clone(),
                reason: "no `main` could be found in its C source".to_owned(),
            });
            continue;
        };
        std::fs::write(
            dir.join(format!("{}.rs", test.id)),
            generate_source(test, standard, timeout, main, work_root),
        )?;
        std::fs::write(dir.join(format!("{}.run.stdout", test.id)), &test.expected)?;
    }
    Ok(skipped)
}

// ---------------------------------------------------------------------------
// collecting results
// ---------------------------------------------------------------------------

/// How a case ended.
#[derive(Clone, Debug)]
enum Outcome {
    Passed,
    Ignored,
    Failed {
        /// `compile error: …`, `runtime: exit code 1`, and so on.
        classification: String,
        /// The first interesting line of what the failing command wrote.
        detail: String,
        /// Everything the failing command wrote, for a `!` entry that names a
        /// substring its diagnostic has to contain.
        output: String,
    },
}

impl Outcome {
    /// Whether the case was refused by the compiler, rather than built and
    /// then found wanting.
    fn is_compile_error(&self) -> bool {
        matches!(self, Outcome::Failed { classification, .. } if classification.starts_with("compile error"))
    }
}

/// One recorded run of one file, in one phase.
#[derive(Clone, Debug)]
struct Record {
    id: String,
    outcome: Outcome,
}

/// A [`StatusEmitter`] that keeps the results instead of printing them.
///
/// `ui_test` renders failures to the terminal; a report has to *classify*
/// them, which means holding the errors themselves rather than the text they
/// were rendered to.
#[derive(Clone, Default)]
struct Collector {
    records: Arc<Mutex<Vec<Record>>>,
}

impl Collector {
    /// The outcome of each case, the worse phase winning.
    fn results(&self) -> BTreeMap<String, Outcome> {
        let mut out: BTreeMap<String, Outcome> = BTreeMap::new();
        for record in self.records.lock().expect("collector poisoned").iter() {
            // Compiling and running are two phases of one case, and the first
            // reports success even when the second then fails.
            if matches!(out.get(&record.id), Some(Outcome::Failed { .. })) {
                continue;
            }
            out.insert(record.id.clone(), record.outcome.clone());
        }
        out
    }
}

impl StatusEmitter for Collector {
    fn register_test(&self, path: PathBuf) -> Box<dyn TestStatus + 'static> {
        Box::new(CollectStatus {
            path,
            revision: String::new(),
            records: self.records.clone(),
        })
    }

    fn finalize(
        &self,
        _failed: usize,
        _succeeded: usize,
        _ignored: usize,
        _filtered: usize,
        _aborted: bool,
    ) -> Box<dyn Summary> {
        Box::new(())
    }
}

/// The [`TestStatus`] half of [`Collector`].
struct CollectStatus {
    path: PathBuf,
    revision: String,
    records: Arc<Mutex<Vec<Record>>>,
}

impl TestStatus for CollectStatus {
    fn for_revision(&self, revision: &str, _style: RevisionStyle) -> Box<dyn TestStatus> {
        Box::new(CollectStatus {
            path: self.path.clone(),
            revision: revision.to_owned(),
            records: self.records.clone(),
        })
    }

    fn for_path(&self, path: &Path) -> Box<dyn TestStatus> {
        Box::new(CollectStatus {
            path: path.to_path_buf(),
            revision: self.revision.clone(),
            records: self.records.clone(),
        })
    }

    fn failed_test<'a>(
        &'a self,
        _cmd: &'a str,
        _stderr: &'a [u8],
        _stdout: &'a [u8],
    ) -> Box<dyn std::fmt::Debug + 'a> {
        Box::new(())
    }

    fn done(&self, result: &TestResult, aborted: bool) {
        if aborted {
            return;
        }
        let id = self
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_owned();
        let outcome = match result {
            Ok(TestOk::Ok) => Outcome::Passed,
            Ok(TestOk::Ignored) => Outcome::Ignored,
            Err(errored) => classify(&self.revision, &errored.errors, &errored.stderr),
        };
        self.records
            .lock()
            .expect("collector poisoned")
            .push(Record { id, outcome });
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn revision(&self) -> &str {
        &self.revision
    }
}

/// The first line worth showing out of what a command wrote.
///
/// `rustc`'s own `error: ` prefix goes, since the classification already says
/// this was an error; the `[E0282]` of one that has a code stays, because it
/// is the shortest thing that names the problem.
fn first_diagnostic_line(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    let line = text
        .lines()
        .map(str::trim_end)
        .find(|line| line.starts_with("error"))
        .or_else(|| {
            text.lines()
                .map(str::trim_end)
                .find(|line| !line.is_empty())
        })
        .unwrap_or_default();
    if let Some(rest) = line.strip_prefix("error: ") {
        rest.to_owned()
    } else if let Some(rest) = line.strip_prefix("error[")
        && let Some((code, message)) = rest.split_once("]: ")
    {
        format!("{code}: {message}")
    } else {
        line.to_owned()
    }
}

/// Turns a failure into one line saying what kind of failure it is.
fn classify(revision: &str, errors: &[Error], output: &[u8]) -> Outcome {
    let detail = first_diagnostic_line(output);
    // The `run` revision is the phase that executes the compiled program;
    // everything else is the compilation itself.
    let classification = if revision == "run" {
        // The corpus's own runner checks the exit status first and diffs the
        // output only then, so a program that both fails and prints the wrong
        // thing is reported as having failed.
        let status = errors.iter().find_map(|error| match error {
            Error::ExitStatus { status, .. } => Some(*status),
            _ => None,
        });
        match status {
            Some(status) => match status.code() {
                Some(124) => "runtime: timed out".to_owned(),
                Some(code) => format!("runtime: exit code {code}"),
                None => format!("runtime: {}", killed_by(status)),
            },
            None if wrote_to_stderr(errors) => "runtime: unexpected output on stderr".to_owned(),
            None => "runtime: stdout mismatch".to_owned(),
        }
    } else if detail.is_empty() {
        "compile error".to_owned()
    } else {
        format!("compile error: {detail}")
    };
    Outcome::Failed {
        classification,
        detail,
        output: String::from_utf8_lossy(output).into_owned(),
    }
}

/// Whether the output that differed was the program's stderr.
fn wrote_to_stderr(errors: &[Error]) -> bool {
    errors.iter().any(|error| match error {
        Error::OutputDiffers { path, .. } => path
            .to_str()
            .is_some_and(|path| path.ends_with(".run.stderr")),
        _ => false,
    })
}

/// How a process that produced no exit code came to an end.
#[cfg(unix)]
fn killed_by(status: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt as _;
    match status.signal() {
        Some(signal) => format!("killed by signal {signal}"),
        None => format!("ended with {status}"),
    }
}

/// How a process that produced no exit code came to an end.
#[cfg(not(unix))]
fn killed_by(status: std::process::ExitStatus) -> String {
    format!("ended with {status}")
}

// ---------------------------------------------------------------------------
// the expected-failure list
// ---------------------------------------------------------------------------

/// Where the list for `standard` lives.
///
/// The default entry point keeps the plain name, since that is the file
/// `cargo test` guards against; the others get one of their own, because a
/// case that needs C11 fails under `c99!` and passes under `c11!`, and one
/// list cannot say both.
fn list_path(standard: Standard) -> PathBuf {
    let dir = Path::new(LIST_DIR);
    match standard {
        Standard::C99 => dir.join("expected-failures.txt"),
        other => dir.join(format!("expected-failures-{}.txt", other.name())),
    }
}

/// What the marker on an expected-failure line claims about its case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum EntryKind {
    /// A plain `NNNNN`: something `cinrs` gets wrong. Guard mode skips the
    /// case and reports it if it starts passing, so that the line can go.
    #[default]
    Failure,
    /// `?NNNNN`: the result depends on the *toolchain*, so neither answer says
    /// anything about the list, and the case is guarded neither way. `00140`
    /// needs C-variadic *definitions*, which Rust stabilised in 1.99, and so
    /// passes on a new enough compiler and not on an older one.
    ToolchainDependent,
    /// `!NNNNN`: the entry point makes the case *invalid*, so refusing it is
    /// conforming behaviour rather than a gap. Guard mode asserts that the
    /// case fails to compile — a `!` case that builds and runs means the
    /// compiler has stopped conforming, and is a failure.
    Rejected {
        /// A substring the diagnostic has to contain, written
        /// `error: "<substring>"` in front of the note. Plain text: there is
        /// no escape, so the substring cannot itself hold a `"`.
        diagnostic: Option<String>,
    },
}

impl EntryKind {
    /// The character an id of this kind is written with.
    fn marker(&self) -> &'static str {
        match self {
            EntryKind::Failure => "",
            EntryKind::ToolchainDependent => "?",
            EntryKind::Rejected { .. } => "!",
        }
    }

    /// Whether a line of this kind survives an update untouched, whatever the
    /// run made of its case.
    ///
    /// A `?` entry may well have passed in *this* run and still be right about
    /// the next compiler along; a `!` entry is a statement about the language,
    /// which a run cannot refute — only report on.
    fn is_permanent(&self) -> bool {
        !matches!(self, EntryKind::Failure)
    }
}

/// One line of an expected-failure list.
#[derive(Clone, Debug, Default)]
struct Entry {
    /// What the marker in front of the id says.
    kind: EntryKind,
    /// What the line says about why the case does not pass.
    note: String,
}

/// Splits an `error: "<substring>"` prefix off a `!` line's note.
///
/// Returns the substring the diagnostic must contain, and what is left of the
/// note. A note that does not open with `error:` names no substring.
fn split_required_diagnostic(note: &str) -> Result<(Option<String>, String)> {
    let Some(rest) = note.strip_prefix("error:") else {
        return Ok((None, note.to_owned()));
    };
    let malformed = || {
        eyre!(
            "`error:` in {note:?} must be followed by a quoted substring, as \
             `error: \"too many arguments\"`"
        )
    };
    let rest = rest.trim_start().strip_prefix('"').ok_or_else(malformed)?;
    let (wanted, note) = rest.split_once('"').ok_or_else(malformed)?;
    if wanted.is_empty() {
        return Err(malformed());
    }
    Ok((Some(wanted.to_owned()), note.trim().to_owned()))
}

/// Reads a list of `[?!]id [note]` lines, ignoring `#` comments and blanks.
fn read_list(path: &Path) -> Result<BTreeMap<String, Entry>> {
    let mut out = BTreeMap::new();
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(err) => return Err(eyre!("reading {}: {err}", path.display())),
    };
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let at = |err| eyre!("{}:{}: {err}", path.display(), number + 1);
        let (id, note) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let note = note.trim().trim_start_matches('#').trim();
        let (kind, note) = match id.as_bytes().first() {
            Some(b'?') => (EntryKind::ToolchainDependent, note.to_owned()),
            Some(b'!') => {
                let (diagnostic, note) = split_required_diagnostic(note).map_err(at)?;
                (EntryKind::Rejected { diagnostic }, note)
            }
            _ => (EntryKind::Failure, note.to_owned()),
        };
        let id = id.trim_start_matches(['?', '!']);
        out.insert(id.to_owned(), Entry { kind, note });
    }
    Ok(out)
}

/// The text after the id, `error: "…"` prefix and all.
fn entry_note(entry: &Entry) -> String {
    let note = entry.note.replace('\n', " ");
    match &entry.kind {
        EntryKind::Rejected {
            diagnostic: Some(wanted),
        } => format!("error: \"{wanted}\"  {}", note.trim()),
        _ => note.trim().to_owned(),
    }
}

/// Rewrites the list from a report, keeping the notes already written.
///
/// Every case that failed goes in, together with every `?` and `!` entry the
/// file already had, marker and note and all. Those two say something a single
/// run cannot check: a `?` entry may well have passed *here* and still be
/// right about the next compiler along, and a `!` entry is a claim about the
/// language rather than about `cinrs`, so an update never downgrades one to a
/// plain failure or drops it because the case happened to build. A `!` case
/// that passed is warned about instead, and kept.
fn write_list(
    path: &Path,
    standard: Standard,
    results: &BTreeMap<String, Outcome>,
    previous: &BTreeMap<String, Entry>,
) -> Result<usize> {
    let mut lines: BTreeMap<&str, Entry> = previous
        .iter()
        .filter(|(_, entry)| entry.kind.is_permanent())
        .map(|(id, entry)| (id.as_str(), entry.clone()))
        .collect();
    for (id, outcome) in results {
        let Outcome::Failed { classification, .. } = outcome else {
            continue;
        };
        let entry = lines.entry(id.as_str()).or_default();
        if entry.note.is_empty() {
            entry.note = previous
                .get(id)
                .map(|entry| entry.note.clone())
                .filter(|note| !note.is_empty())
                .unwrap_or_else(|| classification.clone());
        }
    }

    let mut body = String::new();
    for (id, entry) in &lines {
        let id = format!("{}{id}", entry.kind.marker());
        let line = format!("{id:<8}{}", entry_note(entry));
        writeln!(body, "{}", line.trim_end()).expect("writing to a String");
    }

    let standard = standard.name();
    let header = format!(
        "# The c-testsuite cases `cinrs` does not pass under `{standard}!`.\n\
         #\n\
         # Guard mode — the default, and what `cargo test` runs — skips these and\n\
         # requires every other selected case to pass. It also runs them, so that\n\
         # one which has started passing is reported and the line can go. See\n\
         # `doc/c-testsuite.md` for what the causes are.\n\
         #\n\
         # One test id per line, with a note after it, and a marker in front of\n\
         # the id saying what kind of claim the line makes:\n\
         #\n\
         #   NNNNN   `cinrs` gets this one wrong.\n\
         #   ?NNNNN  It passes or fails depending on the toolchain, and is\n\
         #           guarded neither way.\n\
         #   !NNNNN  This entry point makes the case invalid, so refusing it is\n\
         #           conforming behaviour and not a gap. Guard mode requires the\n\
         #           case to fail to *compile*; one that builds and runs is a\n\
         #           failure. The note may open with `error: \"<substring>\"`,\n\
         #           which the diagnostic then has to contain.\n\
         #\n\
         # Regenerate with\n\
         #\n\
         #     CINRS_CTESTSUITE_REPORT=1 CINRS_CTESTSUITE_UPDATE_EXPECTED=1 \\\n\
         #         CINRS_CTESTSUITE_STANDARD={standard} cargo test --test c_testsuite\n\
         #\n\
         # which keeps every `?` and `!` line as it stands.\n\
         \n"
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, format!("{header}{body}"))?;
    Ok(lines.len())
}

// ---------------------------------------------------------------------------
// the `!` assertion
// ---------------------------------------------------------------------------

/// What a `!` case actually did, measured against what its line requires.
#[derive(Clone, Debug)]
enum Rejection {
    /// Refused at compile time, with the diagnostic the line asked for.
    AsRequired,
    /// Refused, but not in the words the line names.
    WrongDiagnostic { wanted: String, detail: String },
    /// It got past the compiler; what happened after that is beside the point.
    NotRejected { what: String },
}

impl Rejection {
    /// The sentence that goes after `NNNNN in <list> …`.
    fn complaint(&self) -> Option<String> {
        match self {
            Rejection::AsRequired => None,
            Rejection::WrongDiagnostic { wanted, detail } => Some(format!(
                "is marked `!` and must be refused with a diagnostic containing {wanted:?}. \
                 It was refused, but the diagnostic was: {detail}"
            )),
            Rejection::NotRejected { what } => Some(format!(
                "is marked `!`, which says this entry point must refuse it, but {what}. \
                 Either the entry point has stopped conforming or the entry is wrong."
            )),
        }
    }
}

/// Measures one `!` case against its line.
fn check_rejection(diagnostic: Option<&str>, outcome: &Outcome) -> Rejection {
    match outcome {
        Outcome::Failed { detail, output, .. } if outcome.is_compile_error() => match diagnostic {
            Some(wanted) if !output.contains(wanted) => Rejection::WrongDiagnostic {
                wanted: wanted.to_owned(),
                detail: detail.clone(),
            },
            _ => Rejection::AsRequired,
        },
        Outcome::Failed { classification, .. } => Rejection::NotRejected {
            what: format!("it compiled and then failed at run time ({classification})"),
        },
        Outcome::Passed => Rejection::NotRejected {
            what: "it compiled and ran successfully".to_owned(),
        },
        Outcome::Ignored => Rejection::NotRejected {
            what: "it was ignored".to_owned(),
        },
    }
}

/// Every `!` entry of `list` that names a case with a result, with what that
/// case did.
fn rejections<'a>(
    list: &'a BTreeMap<String, Entry>,
    results: &BTreeMap<String, Outcome>,
) -> Vec<(&'a str, &'a Entry, Rejection)> {
    list.iter()
        .filter_map(|(id, entry)| {
            let EntryKind::Rejected { diagnostic } = &entry.kind else {
                return None;
            };
            let outcome = results.get(id)?;
            Some((
                id.as_str(),
                entry,
                check_rejection(diagnostic.as_deref(), outcome),
            ))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// running
// ---------------------------------------------------------------------------

/// The `ui_test` configuration for a directory of generated cases.
///
/// The dependency is this crate, built by `cargo` exactly as the `ui` suite
/// builds it. `-Cstrip=symbols` is there because 220 unstripped binaries are
/// most of a gigabyte and the same 220 stripped are eighty megabytes.
fn test_config(root: &Path, out_dir: &Path, compile_timeout: u64) -> Result<Config> {
    let mut config = Config::rustc(root);
    config.out_dir = out_dir.to_path_buf();
    let defaults = config.comment_defaults.base();
    defaults.set_custom("edition", Edition("2024".to_owned()));
    defaults.set_custom("dependencies", DependencyBuilder::default());
    defaults.compile_flags.push("-Cstrip=symbols".to_owned());
    // Blessing would overwrite the corpus's own expected output with whatever
    // we produced, which is the opposite of the point.
    config.output_conflict_handling = error_on_output_conflict;
    // The host has to be read off the compiler *before* the compiler is
    // wrapped in anything, since reading it means running it with `-vV`.
    config.fill_host_and_target()?;
    wrap_in_timeout(&mut config, compile_timeout);
    Ok(config)
}

/// Puts `timeout(1)` in front of the compiler, where there is one.
///
/// `ui_test` has no timeout of its own, and a front end that hung would hang
/// `cargo test` with it. The generated programs carry their own watchdog for
/// the other half of the problem; this is for a macro expansion that never
/// comes back.
fn wrap_in_timeout(config: &mut Config, seconds: u64) {
    if seconds == 0 || !have_timeout() {
        return;
    }
    let mut args: Vec<OsString> = vec!["-k".into(), "5".into(), seconds.to_string().into()];
    args.push(config.program.program.clone().into_os_string());
    args.append(&mut config.program.args);
    config.program.program = PathBuf::from("timeout");
    config.program.args = args;
}

/// Whether `timeout(1)` can be run at all.
fn have_timeout() -> bool {
    std::process::Command::new("timeout")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Runs one directory of generated cases.
///
/// The collector is always attached; `emitter` is what the person watching
/// sees, and is the silent one for the runs whose failures are the subject
/// rather than a problem.
fn run(config: Config, emitter: Box<dyn StatusEmitter>, collector: &Collector) -> Result<()> {
    run_tests_generic(
        vec![config],
        ui_test::default_file_filter,
        ui_test::default_per_file_config,
        (emitter, collector.clone()),
    )
}

// ---------------------------------------------------------------------------
// reporting
// ---------------------------------------------------------------------------

/// The pass rate overall and per tag, and every failure with its cause.
///
/// Three categories come out of a run, and each gets a list of its own so that
/// the summary line can be read straight off them: the cases that **failed**,
/// the ones the standard requires the entry point to **reject** (a `!` line,
/// which is conforming behaviour and so not a failure), and the ones whose
/// answer is **toolchain-dependent** (a `?` line, which is counted however it
/// came out here). A rejected case stays in the denominator everywhere — it is
/// one of the cases the run looked at — but is never counted as a failure, and
/// the per-tag table gives it a column rather than folding it into the rate.
fn print_report(run: &Run<'_>, results: &BTreeMap<String, Outcome>) {
    let tests = &run.selected;
    let by_id: BTreeMap<&str, &&Test> = tests.iter().map(|test| (test.id.as_str(), test)).collect();

    let rejections = rejections(&run.expected_failures, results);
    let rejected: BTreeSet<&str> = rejections
        .iter()
        .filter(|(_, _, how)| matches!(how, Rejection::AsRequired))
        .map(|(id, _, _)| *id)
        .collect();
    let toolchain: BTreeSet<&str> = run
        .expected_failures
        .iter()
        .filter(|(_, entry)| entry.kind == EntryKind::ToolchainDependent)
        .map(|(id, _)| id.as_str())
        .collect();

    let passed = |id: &str| matches!(results.get(id), Some(Outcome::Passed));
    // Passed, rejected as required, and everything else — which is a failure,
    // whether it built and misbehaved, was refused, or never ran at all.
    let tally = |tests: &[&&Test]| {
        let ok = tests.iter().filter(|test| passed(&test.id)).count();
        let refused = tests
            .iter()
            .filter(|test| rejected.contains(test.id.as_str()))
            .count();
        (ok, refused, tests.len() - ok - refused, tests.len())
    };

    let (ok, refused, failed, total) = tally(&tests.iter().collect::<Vec<_>>());
    println!();
    println!(
        "c-testsuite / single-exec through `{}!`: {ok}/{total} passed ({}){}, {failed} failed",
        run.standard.name(),
        percent(ok, total),
        match refused {
            0 => String::new(),
            n => format!(", {n} rejected as the standard requires"),
        },
    );

    let mut tags: BTreeSet<&str> = BTreeSet::new();
    for test in tests {
        tags.extend(test.tags.iter().map(String::as_str));
    }
    println!();
    if refused == 0 {
        println!("  by tag");
    } else {
        println!("  by tag (`rejected` is a case the standard requires this entry point to");
        println!("          refuse: it is in the total, but it is not a failure)");
    }
    let row = |name: &str, tagged: &[&&Test]| {
        let (ok, refused, _, total) = tally(tagged);
        let refused = match refused {
            0 => String::new(),
            n => format!("   {n} rejected"),
        };
        println!(
            "    {name:<14}{ok:>4}/{total:<4}  {:>6}{refused}",
            percent(ok, total)
        );
    };
    for tag in tags {
        let tagged: Vec<_> = tests.iter().filter(|t| t.tags.contains(tag)).collect();
        row(tag, &tagged);
    }
    let untagged: Vec<_> = tests.iter().filter(|test| test.tags.is_empty()).collect();
    if !untagged.is_empty() {
        row("(none)", &untagged);
    }

    // The failures, grouped by the kind of failure they are. A rejection and a
    // toolchain-dependent case are neither of them a failure of `cinrs`, so
    // they come out of these groups and into lists of their own below.
    let mut groups: BTreeMap<&str, Vec<(&str, &str, &str)>> = BTreeMap::new();
    let mut missing = Vec::new();
    for test in tests {
        let id = test.id.as_str();
        if rejected.contains(id) || toolchain.contains(id) {
            continue;
        }
        match results.get(id) {
            Some(Outcome::Failed {
                classification,
                detail,
                ..
            }) => {
                let kind = classification
                    .split_once(':')
                    .map_or(classification.as_str(), |(kind, _)| kind);
                groups.entry(kind).or_default().push((
                    id,
                    classification.as_str(),
                    detail.as_str(),
                ));
            }
            Some(_) => {}
            None => missing.push(id),
        }
    }

    if groups.is_empty() {
        println!();
        println!("  no failures");
    }
    for (kind, failures) in &groups {
        println!();
        println!("  {kind} ({})", failures.len());
        for (id, classification, detail) in failures {
            let what = classification
                .split_once(": ")
                .map_or(*classification, |(_, rest)| rest);
            println!("    {id}  [{}]", by_id[id].tag_list());
            println!("        {what}");
            if !detail.is_empty() && *detail != what {
                println!("        {detail}");
            }
        }
    }

    if !rejected.is_empty() {
        println!();
        println!("  rejected as the standard requires ({})", rejected.len());
        for (id, entry, _) in rejections
            .iter()
            .filter(|(_, _, how)| matches!(how, Rejection::AsRequired))
        {
            let Some(test) = by_id.get(id) else { continue };
            println!("    {id}  [{}]", test.tag_list());
            println!("        {}", entry.note);
            if let Some(Outcome::Failed { detail, .. }) = results.get(*id) {
                println!("        {detail}");
            }
        }
    }

    // A `!` line the run did not bear out. Report mode fails nothing, but this
    // is the one thing in the list a run *can* contradict, so it is not left
    // to be inferred from a missing line above.
    let anomalies: Vec<_> = rejections
        .iter()
        .filter_map(|(id, _, how)| Some((*id, how.complaint()?)))
        .collect();
    if !anomalies.is_empty() {
        println!();
        println!("  marked `!` but not rejected ({})", anomalies.len());
        for (id, complaint) in anomalies {
            match by_id.get(id) {
                Some(test) => println!("    {id}  [{}]", test.tag_list()),
                None => println!("    {id}"),
            }
            println!("        {complaint}");
        }
    }

    let toolchain: Vec<&str> = toolchain
        .into_iter()
        .filter(|id| by_id.contains_key(id))
        .collect();
    if !toolchain.is_empty() {
        println!();
        println!("  toolchain-dependent ({})", toolchain.len());
        for id in toolchain {
            println!("    {id}  [{}]", by_id[id].tag_list());
            println!("        {}", run.expected_failures[id].note);
            println!(
                "        here: {}",
                match results.get(id) {
                    Some(Outcome::Passed) => "passed".to_owned(),
                    Some(Outcome::Ignored) => "ignored".to_owned(),
                    Some(Outcome::Failed { classification, .. }) => classification.clone(),
                    None => "not run".to_owned(),
                }
            );
        }
    }

    if !missing.is_empty() {
        println!();
        println!("  not run ({})", missing.len());
        for id in missing {
            println!("    {id}");
        }
    }
    println!();
}

/// `90.0%`, or `n/a` when there is nothing to divide by.
fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "n/a".to_owned();
    }
    #[allow(clippy::cast_precision_loss)]
    let rate = part as f64 * 100.0 / whole as f64;
    format!("{rate:.1}%")
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

/// Whether a `CINRS_CTESTSUITE_…` flag is set to `1`.
fn flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| value == "1")
}

/// Everything the two modes both need.
struct Run<'a> {
    standard: Standard,
    selected: Vec<&'a Test>,
    gen_root: PathBuf,
    build_root: PathBuf,
    work_root: PathBuf,
    timeout: u64,
    compile_timeout: u64,
    args: Args,
    list: PathBuf,
    expected_failures: BTreeMap<String, Entry>,
}

fn main() -> Result<()> {
    let suite = Path::new(SUITE_DIR);
    if !suite.is_dir() {
        let message = format!(
            "the c-testsuite corpus is not checked out at `{SUITE_DIR}`.\n\
             Fetch it with\n\n    {FETCH_HINT}\n\n\
             and run the test again; see doc/c-testsuite.md."
        );
        if flag("CINRS_CTESTSUITE_REQUIRED") {
            return Err(eyre!(
                "{message}\n\
                 (CINRS_CTESTSUITE_REQUIRED=1, so this is a failure and not a skip.)"
            ));
        }
        println!("c-testsuite: skipped — {message}");
        return Ok(());
    }

    let standard = match std::env::var("CINRS_CTESTSUITE_STANDARD") {
        Ok(value) => Standard::parse(&value)?,
        Err(_) => Standard::C99,
    };
    let timeout: u64 = match std::env::var("CINRS_CTESTSUITE_TIMEOUT") {
        Ok(value) => value
            .parse()
            .map_err(|err| eyre!("CINRS_CTESTSUITE_TIMEOUT={value}: {err}"))?,
        Err(_) => DEFAULT_TIMEOUT,
    };
    let filter = std::env::var("CINRS_CTESTSUITE_FILTER").unwrap_or_default();

    let all = load_tests(suite)?;
    let selected: Vec<&Test> = all
        .iter()
        .filter(|test| is_selected(test, standard))
        .filter(|test| filter.is_empty() || test.id.contains(&filter))
        .collect();

    println!(
        "c-testsuite: {} of {} cases selected for `{}!`{}",
        selected.len(),
        all.len(),
        standard.name(),
        if filter.is_empty() {
            String::new()
        } else {
            format!(", matching {filter:?}")
        },
    );

    // Absolute, since the generated programs `chdir` into it and are free to
    // be run from anywhere; emptied here, since they only ever add to it.
    let work_root = std::env::current_dir()?
        .join(WORK_DIR)
        .join(standard.name());
    if work_root.exists() {
        std::fs::remove_dir_all(&work_root)?;
    }

    let list = list_path(standard);
    let run = Run {
        standard,
        selected,
        gen_root: Path::new(GEN_DIR).join(standard.name()),
        build_root: Path::new(BUILD_DIR).join(standard.name()),
        work_root,
        timeout,
        compile_timeout: if timeout == 0 { 0 } else { COMPILE_TIMEOUT },
        args: Args::test()?,
        expected_failures: read_list(&list)?,
        list,
    };

    if flag("CINRS_CTESTSUITE_REPORT") {
        report(&run, flag("CINRS_CTESTSUITE_UPDATE_EXPECTED"))
    } else {
        guard(&run, flag("CINRS_CTESTSUITE_STRICT"), !filter.is_empty())
    }
}

/// Report mode: run everything, fail nothing, say what happened.
fn report(run: &Run<'_>, update: bool) -> Result<()> {
    let skipped = generate(
        &run.gen_root,
        &run.selected,
        run.standard,
        run.timeout,
        &run.work_root,
    )?;
    report_skipped(&skipped);

    let collector = Collector::default();
    let mut config = test_config(&run.gen_root, &run.build_root, run.compile_timeout)?;
    config.with_args(&run.args);
    config.output_conflict_handling = error_on_output_conflict;
    println!("c-testsuite: running {} cases…", run.selected.len());
    // Failures are the subject of the report, not a reason to stop.
    drop(self::run(config, Box::new(()), &collector));

    let results = collector.results();
    print_report(run, &results);

    if update {
        // A `!` line says what the *language* requires, so a run that does not
        // bear it out is news about the compiler and not a reason to rewrite
        // the line; `write_list` keeps it either way, and says so here.
        for (id, _, how) in rejections(&run.expected_failures, &results) {
            if let Some(complaint) = how.complaint() {
                println!(
                    "c-testsuite: warning: {id} in {} {complaint}",
                    run.list.display()
                );
                println!(
                    "c-testsuite: warning: the `!{id}` line is kept; delete it by hand if it is wrong."
                );
            }
        }
        let written = write_list(&run.list, run.standard, &results, &run.expected_failures)?;
        println!(
            "c-testsuite: wrote {written} expected failures to {}",
            run.list.display()
        );
    } else {
        // The `?` entries are left out of the comparison: whether they fail
        // depends on the compiler, so neither answer means the list is wrong.
        // A `!` entry stays in — it does fail, and one that stopped failing is
        // exactly the news the comparison is for.
        let optional = |id: &str| {
            run.expected_failures
                .get(id)
                .is_some_and(|entry| entry.kind == EntryKind::ToolchainDependent)
        };
        let failed: BTreeSet<&str> = results
            .iter()
            .filter(|(id, outcome)| matches!(outcome, Outcome::Failed { .. }) && !optional(id))
            .map(|(id, _)| id.as_str())
            .collect();
        let listed: BTreeSet<&str> = run
            .expected_failures
            .iter()
            .filter(|(_, entry)| entry.kind != EntryKind::ToolchainDependent)
            .map(|(id, _)| id.as_str())
            .collect();
        if failed != listed {
            println!(
                "c-testsuite: {} lists {} expected failures and this run had {}; \
                 CINRS_CTESTSUITE_UPDATE_EXPECTED=1 rewrites it.",
                run.list.display(),
                listed.len(),
                failed.len(),
            );
        }
    }
    Ok(())
}

/// Guard mode: everything not listed as failing must pass.
fn guard(run: &Run<'_>, strict: bool, filtered: bool) -> Result<()> {
    let (known_bad, expect_pass): (Vec<&Test>, Vec<&Test>) = run
        .selected
        .iter()
        .copied()
        .partition(|test| run.expected_failures.contains_key(&test.id));

    let skipped = generate(
        &run.gen_root,
        &expect_pass,
        run.standard,
        run.timeout,
        &run.work_root,
    )?;
    report_skipped(&skipped);
    let known_bad_root = run
        .gen_root
        .with_file_name(format!("{}-known-failures", run.standard.name()));
    generate(
        &known_bad_root,
        &known_bad,
        run.standard,
        run.timeout,
        &run.work_root,
    )?;

    let must_be_refused = known_bad
        .iter()
        .filter(|test| {
            matches!(
                run.expected_failures[&test.id].kind,
                EntryKind::Rejected { .. }
            )
        })
        .count();
    println!(
        "c-testsuite: {} must pass; {} listed as failing in {}{}",
        plural(expect_pass.len(), "case", "cases"),
        plural(known_bad.len(), "case is", "cases are"),
        run.list.display(),
        match must_be_refused {
            0 => String::new(),
            n => format!(", {n} of them marked `!` and required to be refused"),
        },
    );

    // The cases that must pass. Their failures are real failures, rendered by
    // `ui_test` exactly as the `ui` suite's are, so nothing here needs the
    // collector's classification of them.
    let mut config = test_config(&run.gen_root, &run.build_root, run.compile_timeout)?;
    config.with_args(&run.args);
    config.output_conflict_handling = error_on_output_conflict;
    let emitter: Box<dyn StatusEmitter> = run.args.format.into();
    let guarded = self::run(config, emitter, &Collector::default());

    // The known-failing ones, silently, so that one which has started passing
    // can be reported and so that a `!` one can be *required* to fail. They
    // share a build directory with the run above, so the dependency is built
    // once.
    let mut now_passing: Vec<String> = Vec::new();
    let mut nonconforming: Vec<String> = Vec::new();
    if !known_bad.is_empty() {
        let collector = Collector::default();
        let config = test_config(&known_bad_root, &run.build_root, run.compile_timeout)?;
        drop(self::run(config, Box::new(()), &collector));
        let results = collector.results();
        now_passing = known_bad
            .iter()
            .filter(|test| {
                // A `?` entry is expected to depend on the compiler and a `!`
                // one is checked below, so neither passing here says anything
                // about the list being stale.
                run.expected_failures[&test.id].kind == EntryKind::Failure
                    && matches!(results.get(&test.id), Some(Outcome::Passed))
            })
            .map(|test| test.id.clone())
            .collect();
        // The `!` assertion: the standard requires these to be refused, so one
        // that builds means the entry point has stopped conforming. That is a
        // failure whatever `CINRS_CTESTSUITE_STRICT` says, since it is a claim
        // about the language and not a note that has gone stale.
        nonconforming = rejections(&run.expected_failures, &results)
            .iter()
            .filter_map(|(id, _, how)| {
                let complaint = how.complaint()?;
                Some(format!("{id} in {} {complaint}", run.list.display()))
            })
            .collect();
    }

    // Entries naming something this run does not have: a case that left the
    // corpus, or one the standard excluded. A filter excludes cases on
    // purpose, so it silences this.
    let selected_ids: BTreeSet<&str> = run.selected.iter().map(|test| test.id.as_str()).collect();
    let unknown: Vec<&str> = run
        .expected_failures
        .keys()
        .map(String::as_str)
        .filter(|id| !selected_ids.contains(id))
        .collect();

    let mut problems = Vec::new();
    if !now_passing.is_empty() {
        problems.push(listing(
            &format!(
                "{} in {} now passes; delete the line",
                plural(now_passing.len(), "case", "cases"),
                run.list.display()
            ),
            now_passing.iter().map(String::as_str),
        ));
    }
    if !unknown.is_empty() && !filtered {
        problems.push(listing(
            &format!(
                "{} in {} does not name a case this run selected",
                plural(unknown.len(), "entry", "entries"),
                run.list.display()
            ),
            unknown.into_iter(),
        ));
    }
    for problem in &problems {
        println!("c-testsuite: warning: {problem}");
    }
    if !problems.is_empty() {
        println!("c-testsuite: CINRS_CTESTSUITE_STRICT=1 makes the above a failure.");
    }
    for complaint in &nonconforming {
        println!("c-testsuite: error: {complaint}");
    }

    guarded?;
    if !nonconforming.is_empty() {
        return Err(eyre!(
            "{}\n{}",
            nonconforming.join("\n"),
            "A `!` line records what the standard requires this entry point to refuse, \
             so this is a failure however CINRS_CTESTSUITE_STRICT is set."
        ));
    }
    if strict && !problems.is_empty() {
        return Err(eyre!("{}", problems.join("\n")));
    }
    println!(
        "c-testsuite: all {} selected cases accounted for",
        run.selected.len()
    );
    Ok(())
}

/// `1 entry` or `3 entries`.
fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// A headline with an indented list of ids under it.
fn listing<'a>(headline: &str, ids: impl Iterator<Item = &'a str>) -> String {
    let mut out = format!("{headline}:");
    for id in ids {
        write!(out, "\n    {id}").expect("writing to a String");
    }
    out
}

/// Says which cases could not be turned into a Rust file at all.
fn report_skipped(skipped: &[Skipped]) {
    for Skipped { id, reason } in skipped {
        println!("c-testsuite: warning: {id} was not generated: {reason}");
    }
}
