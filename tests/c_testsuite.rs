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
//! Everything this harness has in common with the [GCC torture](gcc_torture)
//! and [Clang](clang_c) ones — the modes, the expected-failure list and its
//! markers, the result collector, the timeouts — lives in
//! [`support/conformance.rs`](conformance); what is left here is c-testsuite
//! itself.
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
//! marker says what kind of claim the line is making. See
//! [`conformance::EntryKind`].
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
//! # Timeouts and memory
//!
//! A miscompiled program may loop forever or allocate until there is nothing
//! left, and `ui_test` bounds neither. Four things keep that from wedging the
//! run — the generated program's watchdog thread, which is a clock *and* a
//! memory gauge; the `timeout(1)` and `ulimit -v` the compiler is invoked
//! through; the ceiling on this process itself; and the cap on how many
//! compilers run at once. `CINRS_CTESTSUITE_TIMEOUT` is the clock (20 seconds
//! by default; `0` disables both halves of it) and `CINRS_MEMORY_LIMIT_MB` the
//! ceiling; the memory section of
//! [`support/conformance.rs`](conformance#memory) is the whole story. A
//! program killed by a signal — a stack overflow, an `abort` — is an ordinary
//! run failure, since its exit status is not zero.
//!
//! # Environment
//!
//! * `CINRS_CTESTSUITE_REQUIRED=1` — fail rather than skip when the corpus is
//!   missing.
//! * `CINRS_CTESTSUITE_STANDARD=c89|c99|c11|c23|gnu89|gnu99|gnu11|gnu23` —
//!   which entry point to translate with, and which cases are eligible.
//!   Default `c99`.
//! * `CINRS_CTESTSUITE_FILTER=<substring>` — only cases whose id contains it.
//! * `CINRS_CTESTSUITE_REPORT=1` — report mode.
//! * `CINRS_CTESTSUITE_STRICT=1` — a stale expected-failure entry, one that
//!   now passes or one that names nothing, is a failure and not a warning.
//! * `CINRS_CTESTSUITE_UPDATE_EXPECTED=1` — rewrite the list from the report.
//! * `CINRS_CTESTSUITE_TIMEOUT=<seconds>` — per-test timeout; `0` disables.
//! * `CINRS_MEMORY_LIMIT_MB=<mib>` — the ceiling this harness, every compiler
//!   it spawns and every program it runs work to; 8192 by default and `0` to
//!   switch all of it off. See the memory section of
//!   [`support/conformance.rs`](conformance#memory).
//! * `CINRS_TEST_THREADS=<n>` — the default parallelism, which is otherwise
//!   the smaller of this machine's and eight. `-- --test-threads=<n>` wins
//!   over both.
//!
//! [c-testsuite]: https://github.com/c-testsuite/c-testsuite

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ui_test::Args;
use ui_test::color_eyre::eyre::{Result, eyre};
use ui_test::status_emitter::StatusEmitter;

#[path = "support/conformance.rs"]
mod conformance;

use conformance::{
    COMPILE_TIMEOUT, Collector, Entry, EntryKind, MainKind, Outcome, Skipped, flag, listing,
    percent, plural, raw_string_hashes, read_list, rejections, report_skipped, watchdog,
    write_list,
};

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

// ---------------------------------------------------------------------------
// the standards
// ---------------------------------------------------------------------------

/// The entry point a run translates with, which is also the newest revision a
/// case may ask for.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Standard {
    C89,
    C99,
    C11,
    C23,
    /// `gnu89!`, and below it the three strict revisions it does *not* sort
    /// above: a GNU dialect accepts what every later revision added, which is
    /// what [`Standard::accepts`] says instead of the ordering.
    Gnu89,
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
            Standard::C89 => "c89",
            Standard::Gnu89 => "gnu89",
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
            "c89" | "c90" => Ok(Standard::C89),
            "gnu89" => Ok(Standard::Gnu89),
            "c99" => Ok(Standard::C99),
            "c11" => Ok(Standard::C11),
            "c23" => Ok(Standard::C23),
            "gnu99" => Ok(Standard::Gnu99),
            "gnu11" => Ok(Standard::Gnu11),
            "gnu23" => Ok(Standard::Gnu23),
            other => Err(eyre!(
                "CINRS_CTESTSUITE_STANDARD={other}: expected c89, c99, c11, c23, gnu89, \
                 gnu99, gnu11 or gnu23"
            )),
        }
    }

    /// The revision a `cNN` tag asks for.
    ///
    /// The corpus documents `c89` as implying `c99` and `c99` as implying
    /// `c11`: a tag names the *oldest* revision the program is valid in, so a
    /// C89 program runs under every later entry point as well as under
    /// `c89!` — which is what makes the `c89`-tagged cases, the majority of
    /// the corpus, the ones that measure the oldest entry point.
    fn of_tag(tag: &str) -> Option<Self> {
        match tag {
            "c89" => Some(Standard::C89),
            "c99" => Some(Standard::C99),
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
        matches!(
            self,
            Standard::Gnu89 | Standard::Gnu99 | Standard::Gnu11 | Standard::Gnu23
        ) || self >= needs
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

    out.push_str(&watchdog(
        timeout,
        conformance::memory_limit_mb(),
        "c-testsuite",
    ));

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
        if conformance::confuses_ui_test(&test.source) {
            skipped.push(Skipped {
                id: test.id.clone(),
                reason: "its C source holds a line `ui_test` would read as a command".to_owned(),
            });
            continue;
        }
        let Some(main) = conformance::detect_main(&test.source) else {
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

/// The comment block written above the list.
fn list_header(standard: Standard) -> String {
    let standard = standard.name();
    format!(
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
        .filter(|(_, _, how)| how.is_as_required())
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
        for (id, entry, _) in rejections.iter().filter(|(_, _, how)| how.is_as_required()) {
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
                    Some(outcome) => outcome.describe(),
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

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

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

impl Run<'_> {
    /// The `ui_test` configuration for one directory of generated cases.
    fn config(&self, root: &Path) -> Result<ui_test::Config> {
        conformance::test_config(root, &self.build_root, self.compile_timeout, &[])
    }
}

fn main() -> Result<()> {
    conformance::start_memory_watchdog("c-testsuite");
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
    let timeout = conformance::timeout_var("CINRS_CTESTSUITE_TIMEOUT", DEFAULT_TIMEOUT)?;
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
        args: {
            let mut args = Args::test()?;
            conformance::cap_threads(&mut args)?;
            args
        },
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
    report_skipped("c-testsuite", &skipped);

    let collector = Collector::default();
    let mut config = run.config(&run.gen_root)?;
    config.with_args(&run.args);
    config.output_conflict_handling = ui_test::error_on_output_conflict;
    println!("c-testsuite: running {} cases…", run.selected.len());
    // Failures are the subject of the report, not a reason to stop.
    drop(conformance::run(config, Box::new(()), &collector));

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
        let written = write_list(
            &run.list,
            &list_header(run.standard),
            &results,
            &run.expected_failures,
        )?;
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
    report_skipped("c-testsuite", &skipped);
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
    let mut config = run.config(&run.gen_root)?;
    config.with_args(&run.args);
    config.output_conflict_handling = ui_test::error_on_output_conflict;
    let emitter: Box<dyn StatusEmitter> = run.args.format.into();
    let guarded = conformance::run(config, emitter, &Collector::default());

    // The known-failing ones, silently, so that one which has started passing
    // can be reported and so that a `!` one can be *required* to fail. They
    // share a build directory with the run above, so the dependency is built
    // once.
    let mut now_passing: Vec<String> = Vec::new();
    let mut nonconforming: Vec<String> = Vec::new();
    if !known_bad.is_empty() {
        let collector = Collector::default();
        let config = run.config(&known_bad_root)?;
        drop(conformance::run(config, Box::new(()), &collector));
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
