//! What the conformance harnesses have in common.
//!
//! There are three of them — [c-testsuite](../c_testsuite.rs),
//! [`gcc.c-torture/execute`](../gcc_torture.rs) and
//! [`clang/test/C`](../clang_c.rs) — and they are the same machine pointed at
//! three corpora: turn each case into a file `ui_test` can run, run them,
//! classify what came out, and measure the result against a checked-in list of
//! the ones that are known not to pass. This module is that machine. Each
//! harness keeps only what is genuinely its own: which files are cases, how one
//! becomes a test, and what its report looks like.
//!
//! `tests/*.rs` are separate crates, so it is shared the way a test crate can
//! share code — by path:
//!
//! ```ignore
//! #[path = "support/conformance.rs"]
//! mod conformance;
//! ```
//!
//! which means every harness compiles the whole module and uses part of it;
//! hence the `dead_code` allowance below.
//!
//! # The pieces
//!
//! * **Modes.** Every harness has the same three, spelled with the same
//!   suffixes on its own environment-variable prefix: guard (the default),
//!   `…_REPORT=1` and `…_REPORT=1 …_UPDATE_EXPECTED=1`. [`flag`] and [`var`]
//!   read them.
//! * **The expected-failure list.** One id per line with a note, and a marker
//!   in front of the id saying what kind of claim the line makes — see
//!   [`EntryKind`]. [`read_list`] and [`write_list`] are the two ends of it.
//! * **Outcomes.** `ui_test` renders failures to a terminal; a report has to
//!   *classify* them, so [`Collector`] keeps the errors instead of printing
//!   them and [`classify`] turns each into one line saying what kind of failure
//!   it is. [`normalise_cause`] and [`group_causes`] then fold the lines that
//!   say the same thing into one row with a count, which is what makes a
//!   thousand-case corpus readable.
//! * **Timeouts and memory.** A miscompiled program need never come back, a
//!   front end that is wrong about a size can ask for all of memory, and
//!   `ui_test` has neither a per-test timeout nor a per-test ceiling. So
//!   [`watchdog`] goes inside the generated program — it is a clock *and* a
//!   memory gauge — [`wrap_in_limits`] puts `timeout(1)` and a `ulimit -v` in
//!   front of every compiler the harness spawns, [`start_memory_watchdog`]
//!   watches the harness process itself, and [`cap_threads`] keeps a full
//!   machine's worth of `rustc` processes from being started at once. See
//!   [the memory section](#memory) below.
//! * **Reading C.** [`blank_out_comments_and_literals`], [`detect_main`],
//!   [`raw_string_hashes`] and [`confuses_ui_test`] are the text-level things
//!   every harness needs before it can put a case inside a Rust file.
//!
//! # Memory
//!
//! A conformance run is a thousand `rustc` processes and a thousand programs
//! that a *front end under development* wrote, and either half can ask for all
//! of memory: a wrong array bound is a `[u8; 2^61]` in the expansion, a
//! miscompiled loop is a `malloc` that never stops. `ui_test` bounds neither,
//! and the default parallelism is one test per core, so on a many-core machine
//! the first mistake takes the machine down rather than the test. That is not
//! hypothetical: it took this one down twice.
//!
//! Four things stop it, all of them keyed to one number,
//! `CINRS_MEMORY_LIMIT_MB` (default [`DEFAULT_MEMORY_LIMIT_MB`]):
//!
//! 1. [`start_memory_watchdog`] watches the harness process itself and aborts
//!    it, with a message, when its resident set passes the limit.
//! 2. [`wrap_in_limits`] runs every compiler the harness spawns under
//!    `sh -c '… ulimit -v "$limit"; exec "$@"'` as well as `timeout(1)`, so
//!    one `rustc` that runs away dies on its own.
//! 3. [`watchdog`], the thread every generated *program* starts, polls its own
//!    resident set as well as the clock.
//! 4. [`cap_threads`] holds the default parallelism down to
//!    [`MAX_DEFAULT_TEST_THREADS`], because what actually exhausted this
//!    machine's memory was thirty-two `rustc` processes at once rather than
//!    any single one of them.
//!
//! `CINRS_MEMORY_LIMIT_MB=0` switches all four off.
//! [`tests/harness_safety.rs`](../harness_safety.rs) provokes each one, and
//! `doc/testsuites.md` says how a suite has to be run on top of them.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ui_test::color_eyre::eyre::{Result, eyre};
use ui_test::custom_flags::edition::Edition;
use ui_test::dependencies::DependencyBuilder;
use ui_test::status_emitter::{RevisionStyle, StatusEmitter, Summary, TestStatus};
use ui_test::test_result::{TestOk, TestResult};
use ui_test::{Config, Error, error_on_output_conflict, run_tests_generic};

// ---------------------------------------------------------------------------
// environment
// ---------------------------------------------------------------------------

/// Whether an environment variable is set to `1`.
pub fn flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| value == "1")
}

/// An environment variable's value, or `None` when it is unset or empty.
pub fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// A `…_TIMEOUT` variable, in seconds, defaulting to `default`.
pub fn timeout_var(name: &str, default: u64) -> Result<u64> {
    match std::env::var(name) {
        Ok(value) => value.parse().map_err(|err| eyre!("{name}={value}: {err}")),
        Err(_) => Ok(default),
    }
}

/// How long the *compiler* may take on one case, in seconds.
///
/// Much larger than any per-program timeout: a cold run pays for the dependency
/// build, and a macro that is merely slow should be seen as slow rather than
/// mistaken for one that has hung.
pub const COMPILE_TIMEOUT: u64 = 300;

// ---------------------------------------------------------------------------
// memory — see the module documentation
// ---------------------------------------------------------------------------

/// The memory ceiling a harness works to when nothing says otherwise, in MiB.
pub const DEFAULT_MEMORY_LIMIT_MB: u64 = 8192;

/// The most threads a conformance harness starts unless it is told to.
///
/// One `rustc` per core is what `ui_test` does by default and what took this
/// machine down twice; the harnesses are I/O- and memory-bound long before
/// they are CPU-bound, so the cap costs very little.
pub const MAX_DEFAULT_TEST_THREADS: usize = 8;

/// How often a watchdog looks at the resident set, in milliseconds.
pub const MEMORY_POLL_MS: u64 = 250;

/// The exit status a program killed for using too much memory reports.
///
/// 128 + `SIGKILL`, which is what a shell reports for a process the kernel's
/// own out-of-memory killer took, so a reader who knows the convention reads
/// it the same way. [`classify`] turns it back into `out of memory`.
pub const MEMORY_EXIT_CODE: i32 = 137;

/// `CINRS_MEMORY_LIMIT_MB`, in MiB; zero means no limit at all.
pub fn memory_limit_mb() -> u64 {
    match std::env::var("CINRS_MEMORY_LIMIT_MB") {
        Ok(value) => value.trim().parse().unwrap_or(DEFAULT_MEMORY_LIMIT_MB),
        Err(_) => DEFAULT_MEMORY_LIMIT_MB,
    }
}

/// This process's resident set, in MiB.
///
/// Read out of `/proc/self/statm`, whose second field is the resident set in
/// pages. The page size is taken as 4 KiB, which is what Linux uses on every
/// target this is run on; being wrong about it would only move the ceiling,
/// never remove it. `None` anywhere the file cannot be read, which is every
/// system that is not Linux.
pub fn resident_mb() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: u64 = text.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096 / (1 << 20))
}

/// Starts the thread that aborts this process if it outgrows the limit.
///
/// Every [`MEMORY_POLL_MS`] it reads [`resident_mb`]; past the limit it says
/// so on stderr and calls [`std::process::abort`], which is deliberate — a
/// `panic` would be caught by whatever is above it and an `exit` would run
/// destructors that may themselves need memory, whereas an abort is
/// immediate, unmistakable in a CI log, and leaves a core if one is wanted.
///
/// A no-op where the limit is zero or `/proc` is not there.
pub fn start_memory_watchdog(label: &str) {
    let limit = memory_limit_mb();
    if limit == 0 || resident_mb().is_none() {
        return;
    }
    let label = label.to_owned();
    std::thread::Builder::new()
        .name("cinrs-memory-watchdog".to_owned())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(MEMORY_POLL_MS));
                let Some(resident) = resident_mb() else {
                    return;
                };
                if resident > limit {
                    eprintln!(
                        "\n{label}: aborting — this harness process is using {resident} MiB, \
                         past the {limit} MiB CINRS_MEMORY_LIMIT_MB ceiling. Re-run with a \
                         filter, with fewer threads (`-- --test-threads=2`), or with a larger \
                         CINRS_MEMORY_LIMIT_MB if the machine really has the memory."
                    );
                    std::process::abort();
                }
            }
        })
        .ok();
}

/// Holds the default parallelism down to [`MAX_DEFAULT_TEST_THREADS`].
///
/// An explicit `--test-threads=N` always wins, and `CINRS_TEST_THREADS=N` is
/// the way to say it in the environment; this only fills in the default, which
/// `ui_test` would otherwise take from
/// [`std::thread::available_parallelism`].
pub fn cap_threads(args: &mut ui_test::Args) -> Result<()> {
    if args.threads.is_some() {
        return Ok(());
    }
    let wanted = match var("CINRS_TEST_THREADS") {
        Some(value) => value
            .trim()
            .parse::<usize>()
            .map_err(|err| eyre!("CINRS_TEST_THREADS={value}: {err}"))?,
        None => std::thread::available_parallelism()
            .map_or(1, std::num::NonZeroUsize::get)
            .min(MAX_DEFAULT_TEST_THREADS),
    };
    args.threads = std::num::NonZeroUsize::new(wanted.max(1));
    Ok(())
}

/// The shell script a capped command is run through.
///
/// `sh -c 'script' a b c` puts `a` in `$0` and `b c` in `$@`, so the limit
/// travels as `$0` and the command as `"$@"` with no quoting to get wrong,
/// and `exec` keeps the shell from staying around as a second process for the
/// whole compilation. `ulimit` is in every POSIX shell, which `prlimit(1)` is
/// not.
///
/// Two details are not decoration:
///
/// * **The limit is clamped to the hard limit.** The documented way to run
///   these suites is itself inside a `ulimit -v`, and asking for *more* than
///   the inherited hard limit is `EPERM` — which the shell reports on stderr,
///   which `ui_test` then compares against a blessed `.stderr` file. Taking
///   the smaller of the two is both quieter and what was meant.
/// * **Nothing is allowed to write to stderr.** For the same reason: this
///   shell shares its stderr with the compiler, and every byte of that is part
///   of a test's expected output. A shell that has no `ulimit -v` at all runs
///   the command without one rather than failing the test with a message
///   about `ulimit`.
///
/// The limit is on *address space* rather than on the resident set, because
/// that is the only one a shell can set; it is generous by nature, and is here
/// to stop a runaway rather than to measure anything. The
/// [in-process watchdog](start_memory_watchdog) and the
/// [one inside a generated program](watchdog) are the resident-set halves.
const LIMIT_SCRIPT: &str = "\
limit=$0; \
hard=$(ulimit -H -v 2>/dev/null) || hard=; \
case $hard in '' | unlimited) ;; \
    *) [ \"$limit\" -gt \"$hard\" ] 2>/dev/null && limit=$hard ;; \
esac; \
ulimit -v \"$limit\" 2>/dev/null; \
exec \"$@\"";

/// The `sh -c …` prefix that runs a command under an address-space limit.
///
/// See [`LIMIT_SCRIPT`]. `None` when there is no limit to apply.
pub fn memory_limit_prefix(limit_mb: u64) -> Option<(PathBuf, Vec<OsString>)> {
    if limit_mb == 0 {
        return None;
    }
    let kb = limit_mb.saturating_mul(1024);
    Some((
        PathBuf::from("sh"),
        vec!["-c".into(), LIMIT_SCRIPT.into(), kb.to_string().into()],
    ))
}

// ---------------------------------------------------------------------------
// reading C source text
// ---------------------------------------------------------------------------

/// The C source with comments and literals blanked out.
///
/// Blanking rather than deleting keeps the length, so a position in the
/// result means the same thing in the original.
pub fn blank_out_comments_and_literals(source: &str) -> String {
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

/// How the C program spells `main`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MainKind {
    /// `int main(void)`, or `int main()` with an empty identifier list.
    NoArgs,
    /// `int main(int argc, char **argv)`.
    ArgcArgv,
}

/// How the program's `main` is declared, read off the text.
///
/// The *first* declaration wins. One case in c-testsuite (`00182`) carries a
/// second `main` inside an `#ifndef` that its own `#define` has already made
/// false; finding that out would mean running the preprocessor, and taking
/// the first is both simpler and right.
pub fn detect_main(source: &str) -> Option<MainKind> {
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
pub fn raw_string_hashes(source: &str) -> usize {
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
/// A case that does that is skipped with a note rather than failing with a
/// baffling parse error.
pub fn confuses_ui_test(source: &str) -> bool {
    source.lines().any(|line| {
        line.starts_with("//@")
            || line
                .match_indices("//")
                .any(|(at, _)| matches!(line.as_bytes().get(at + 2), Some(b'@' | b'~')))
    })
}

/// Whether the source can go inside a Rust raw string literal at all.
///
/// The Rust lexer refuses a bare carriage return in one, and the file has to
/// be UTF-8 to have been read in the first place. Returns the reason it
/// cannot, or `None`.
pub fn unquotable(source: &str) -> Option<&'static str> {
    source
        .contains('\r')
        .then_some("it holds a carriage return, which a Rust raw string literal may not")
}

/// The watchdog thread a generated program starts, as Rust source.
///
/// A miscompiled program may loop forever *or* allocate without bound, and
/// `ui_test` bounds neither, so every generated program watches itself. The
/// thread wakes every [`MEMORY_POLL_MS`] and looks at two things:
///
/// * the clock — past `seconds` it exits 124, which is what `timeout(1)` uses
///   and what [`classify`] reads back as `timed out`;
/// * its own resident set, out of `/proc/self/statm` — past `limit_mb` it
///   exits [`MEMORY_EXIT_CODE`], read back as `out of memory`.
///
/// The memory half is there as well as the `ulimit -v` of
/// [`wrap_in_limits`] because a limit is only inherited by a process the
/// harness itself started: a case that forks, or a run started by hand, would
/// otherwise have none. Where `/proc` is not readable the check simply never
/// fires.
///
/// An empty string when both are zero, which is how the watchdog is switched
/// off altogether.
pub fn watchdog(seconds: u64, limit_mb: u64, label: &str) -> String {
    if seconds == 0 && limit_mb == 0 {
        return String::new();
    }
    let deadline = if seconds == 0 {
        String::new()
    } else {
        format!(
            "            if started.elapsed().as_secs() >= {seconds} {{
                eprintln!(\"cinrs {label}: timed out after {seconds} seconds\");
                std::process::exit(124);
            }}
"
        )
    };
    let memory = if limit_mb == 0 {
        String::new()
    } else {
        format!(
            "            let resident = std::fs::read_to_string(\"/proc/self/statm\")
                .ok()
                .and_then(|text| text.split_whitespace().nth(1)?.parse::<u64>().ok())
                .map_or(0, |pages| pages * 4096 / (1 << 20));
            if resident > {limit_mb} {{
                eprintln!(
                    \"cinrs {label}: using {{resident}} MiB, past the {limit_mb} MiB limit\"
                );
                std::process::exit({MEMORY_EXIT_CODE});
            }}
"
        )
    };
    format!(
        "    // A miscompiled program need never come back and need never stop
    // allocating; the harness must survive both.
    std::thread::spawn(|| {{
        let started = std::time::Instant::now();
        loop {{
            std::thread::sleep(std::time::Duration::from_millis({MEMORY_POLL_MS}));
{deadline}{memory}        }}
    }});
"
    )
}

/// A case that could not be turned into a test file, and why.
pub struct Skipped {
    /// The case's id.
    pub id: String,
    /// What stopped it.
    pub reason: String,
}

/// Says which cases could not be turned into a test file at all.
pub fn report_skipped(prefix: &str, skipped: &[Skipped]) {
    for Skipped { id, reason } in skipped {
        println!("{prefix}: warning: {id} was not generated: {reason}");
    }
}

// ---------------------------------------------------------------------------
// collecting results
// ---------------------------------------------------------------------------

/// How a case ended.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// Compiled, ran, and produced what was asked for.
    Passed,
    /// `ui_test` skipped it.
    Ignored,
    /// Anything else.
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
    pub fn is_compile_error(&self) -> bool {
        matches!(self, Outcome::Failed { classification, .. } if classification.starts_with("compile error"))
    }

    /// Whether the case passed.
    pub fn is_pass(&self) -> bool {
        matches!(self, Outcome::Passed)
    }

    /// The one-line classification of a failure.
    pub fn classification(&self) -> Option<&str> {
        match self {
            Outcome::Failed { classification, .. } => Some(classification),
            _ => None,
        }
    }

    /// How this outcome reads in a one-line summary.
    pub fn describe(&self) -> String {
        match self {
            Outcome::Passed => "passed".to_owned(),
            Outcome::Ignored => "ignored".to_owned(),
            Outcome::Failed { classification, .. } => classification.clone(),
        }
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
pub struct Collector {
    records: Arc<Mutex<Vec<Record>>>,
    /// The directory case ids are counted from; see [`Collector::rooted`].
    root: Option<PathBuf>,
}

impl Collector {
    /// A collector whose ids are paths relative to `root`, without the `.rs`.
    ///
    /// The default is the file stem alone, which is what a flat directory of
    /// cases wants. A corpus with subdirectories — `execute/` and `ieee/` —
    /// needs the directory in the id, both because two of its cases may share
    /// a stem and because `execute/pr17252` is the name a reader can find
    /// upstream.
    pub fn rooted(root: impl Into<PathBuf>) -> Self {
        Self {
            records: Arc::default(),
            root: Some(root.into()),
        }
    }

    /// The id of the case in `path`.
    fn id_of(&self, path: &Path) -> String {
        let relative = self
            .root
            .as_deref()
            .and_then(|root| path.strip_prefix(root).ok());
        match relative {
            Some(relative) => relative
                .with_extension("")
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/"),
            None => path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .to_owned(),
        }
    }

    /// The outcome of each case, the worse phase winning.
    pub fn results(&self) -> BTreeMap<String, Outcome> {
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
            id: self.id_of(&path),
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
    /// What the result is filed under; see [`Collector::rooted`].
    id: String,
    path: PathBuf,
    revision: String,
    records: Arc<Mutex<Vec<Record>>>,
}

impl TestStatus for CollectStatus {
    fn for_revision(&self, revision: &str, _style: RevisionStyle) -> Box<dyn TestStatus> {
        Box::new(CollectStatus {
            id: self.id.clone(),
            path: self.path.clone(),
            revision: revision.to_owned(),
            records: self.records.clone(),
        })
    }

    fn for_path(&self, path: &Path) -> Box<dyn TestStatus> {
        Box::new(CollectStatus {
            id: self.id.clone(),
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
        let id = self.id.clone();
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
pub fn first_diagnostic_line(output: &[u8]) -> String {
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

/// The first `error…` line of a failure, wherever `ui_test` put it.
///
/// The command's own output is the usual place, but a compilation that never
/// happened has none — the dependency would not build, an auxiliary file
/// failed, the process was killed — and a `//@check-pass` file whose
/// expansion `rustc` refuses has its diagnostics inside an
/// [`Error::OutputDiffers`] rather than on the stream, because `ui_test`
/// compares them against a `.stderr` file that does not exist. Reporting an
/// empty string for any of those turns sixty cases into sixty blank rows, so
/// every place it can be is looked in, in order of how specific it is.
pub fn failure_detail(errors: &[Error], output: &[u8]) -> String {
    let line = first_diagnostic_line(output);
    if !line.is_empty() {
        return line;
    }
    for error in errors {
        let found = match error {
            // What a `//@check-pass` file that did not compile produces: the
            // normalised output is the compiler's rendered diagnostics.
            Error::OutputDiffers { actual, output, .. } => {
                let line = first_diagnostic_line(actual);
                if line.is_empty() {
                    first_diagnostic_line(output)
                } else {
                    line
                }
            }
            // Diagnostics `ui_test` matched out of the stream and kept.
            Error::ErrorsWithoutPattern { msgs, .. } => msgs
                .iter()
                .map(|msg| match &msg.code {
                    Some(code) => format!("{code}: {}", msg.message),
                    None => msg.message.clone(),
                })
                .find(|text| !text.is_empty())
                .unwrap_or_default(),
            Error::Command { kind, status } => format!("the {kind} step {status}"),
            Error::Aux { path, errors } => {
                let inner = failure_detail(errors, b"");
                format!("the auxiliary build of {} failed: {inner}", path.display())
            }
            Error::Bug(message) | Error::ConfigError(message) => message.clone(),
            Error::ExitStatus { status, .. } => format!("the compiler {status}"),
            // Everything else — a `ui_test` comment this harness generated
            // wrongly, a pattern that did not match, `rustfix` — has no one
            // line of its own. Its `Debug` is better than nothing, and
            // nothing is what a reader of the report gets otherwise.
            other => format!("{other:?}").lines().next().unwrap_or("").to_owned(),
        };
        if !found.is_empty() {
            return found;
        }
    }
    String::new()
}

/// Turns a failure into one line saying what kind of failure it is.
pub fn classify(revision: &str, errors: &[Error], output: &[u8]) -> Outcome {
    let detail = failure_detail(errors, output);
    // The `run` revision is the phase that executes the compiled program;
    // everything else is the compilation itself.
    let classification = if revision == "run" {
        // A corpus runner checks the exit status first and diffs the output
        // only then, so a program that both fails and prints the wrong thing
        // is reported as having failed.
        let status = errors.iter().find_map(|error| match error {
            Error::ExitStatus { status, .. } => Some(*status),
            _ => None,
        });
        match status {
            Some(status) => match status.code() {
                Some(124) => "runtime: timed out".to_owned(),
                // What the generated program's own watchdog exits with when
                // it outgrows `CINRS_MEMORY_LIMIT_MB`; see [`watchdog`].
                Some(MEMORY_EXIT_CODE) => "runtime: out of memory".to_owned(),
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
// grouping failures by cause
// ---------------------------------------------------------------------------

/// One diagnostic with everything case-specific taken out of it.
///
/// A corpus of a thousand programs produces a thousand failure lines and about
/// thirty distinct *causes*; the difference is the identifiers, numbers and
/// paths in the message. Blanking those turns `use of undeclared identifier
/// 'foo'` and `use of undeclared identifier 'bar'` into one row with a count
/// of two, which is what makes the report a to-do list rather than a log.
///
/// Anything quoted — `'…'`, `"…"`, `` `…` `` — becomes the ellipsis, every run
/// of digits becomes `N`, and the ` (at line L, column C of the C source)`
/// that every `cinrs` diagnostic carries in string-literal input goes.
pub fn normalise_cause(message: &str) -> String {
    let message = strip_position(message);
    let mut out = String::with_capacity(message.len());
    let mut chars = message.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\'' | '"' | '`' => {
                // A quoted run, up to the matching close. An unterminated one
                // swallows the rest, which is the conservative answer: the tail
                // of such a message is case-specific anyway.
                let close = if ch == '`' { '\'' } else { ch };
                out.push(ch);
                out.push('…');
                for inner in chars.by_ref() {
                    if inner == close || inner == ch {
                        break;
                    }
                }
                out.push(if ch == '`' { '\'' } else { ch });
            }
            '0'..='9' => {
                while chars.peek().is_some_and(char::is_ascii_digit) {
                    chars.next();
                }
                out.push('N');
            }
            ch if ch.is_whitespace() => {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                out.push(' ');
            }
            ch => out.push(ch),
        }
    }
    out.trim().to_owned()
}

/// A `cinrs` message without the position it ends with.
///
/// String-literal input cannot put a caret inside the literal on stable, so
/// every diagnostic says ` (at line L, column C of the C source)` instead. In
/// a report and in an expected-failure list that is noise — the line is of the
/// *generated* file, which a harness that prepends a prelude has shifted
/// anyway — so it comes off.
pub fn strip_position(message: &str) -> &str {
    match message.rfind(" (at line ") {
        Some(at) if message.ends_with(')') => &message[..at],
        _ => message,
    }
}

/// A group of failures that say the same thing.
pub struct CauseGroup {
    /// The [normalised](normalise_cause) message.
    pub cause: String,
    /// One case that has it, for the reader to go and look at.
    pub example: String,
    /// The ids in the group, in the order they were given.
    pub cases: Vec<String>,
}

impl CauseGroup {
    /// How many cases have this cause.
    pub fn count(&self) -> usize {
        self.cases.len()
    }
}

/// Folds `(id, message)` pairs into groups, most common first.
///
/// Ties are broken by the cause, so the order is stable from run to run and a
/// report can be diffed against the last one.
pub fn group_causes<'a>(items: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<CauseGroup> {
    group_by(items, normalise_cause)
}

/// [`group_causes`] without the normalisation, for text that is already a
/// fixed phrase — a skip reason, an outcome class — where blanking the
/// identifiers would throw away the only thing that distinguishes two rows.
pub fn group_texts<'a>(items: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<CauseGroup> {
    group_by(items, str::to_owned)
}

/// The two above, with the key function passed in.
fn group_by<'a>(
    items: impl IntoIterator<Item = (&'a str, &'a str)>,
    key: impl Fn(&str) -> String,
) -> Vec<CauseGroup> {
    let mut groups: BTreeMap<String, CauseGroup> = BTreeMap::new();
    for (id, message) in items {
        let cause = key(message);
        let group = groups.entry(cause.clone()).or_insert_with(|| CauseGroup {
            cause,
            example: id.to_owned(),
            cases: Vec::new(),
        });
        group.cases.push(id.to_owned());
    }
    let mut out: Vec<CauseGroup> = groups.into_values().collect();
    out.sort_by(|a, b| {
        b.count()
            .cmp(&a.count())
            .then_with(|| a.cause.cmp(&b.cause))
    });
    out
}

// ---------------------------------------------------------------------------
// the expected-failure list
// ---------------------------------------------------------------------------

/// What the marker on an expected-failure line claims about its case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum EntryKind {
    /// A plain id: something `cinrs` gets wrong. Guard mode skips the case and
    /// reports it if it starts passing, so that the line can go.
    #[default]
    Failure,
    /// `?id`: the result depends on the *toolchain*, so neither answer says
    /// anything about the list, and the case is guarded neither way.
    ToolchainDependent,
    /// `!id`: the entry point makes the case *invalid*, so refusing it is
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
    pub fn marker(&self) -> &'static str {
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
    pub fn is_permanent(&self) -> bool {
        !matches!(self, EntryKind::Failure)
    }
}

/// One line of an expected-failure list.
#[derive(Clone, Debug, Default)]
pub struct Entry {
    /// What the marker in front of the id says.
    pub kind: EntryKind,
    /// What the line says about why the case does not pass.
    pub note: String,
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
pub fn read_list(path: &Path) -> Result<BTreeMap<String, Entry>> {
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
pub fn entry_note(entry: &Entry) -> String {
    let note = entry.note.replace('\n', " ");
    match &entry.kind {
        EntryKind::Rejected {
            diagnostic: Some(wanted),
        } => format!("error: \"{wanted}\"  {}", note.trim()),
        _ => note.trim().to_owned(),
    }
}

/// The narrowest column the ids fit in, and never narrower than c-testsuite's.
///
/// Eight is what the five-digit c-testsuite ids have always used; a corpus
/// whose names are longer widens the column rather than running the note into
/// the id.
fn id_column(ids: impl IntoIterator<Item = usize>) -> usize {
    ids.into_iter().map(|len| len + 2).max().unwrap_or(0).max(8)
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
///
/// `header` is the comment block written above the list, which says what the
/// list is for and how to regenerate it; it is the one part that differs
/// between harnesses.
pub fn write_list(
    path: &Path,
    header: &str,
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

    let width = id_column(
        lines
            .iter()
            .map(|(id, entry)| id.len() + entry.kind.marker().len()),
    );
    let mut body = String::new();
    for (id, entry) in &lines {
        let id = format!("{}{id}", entry.kind.marker());
        let line = format!("{id:<width$}{}", entry_note(entry));
        writeln!(body, "{}", line.trim_end()).expect("writing to a String");
    }

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, format!("{header}{body}"))?;
    Ok(lines.len())
}

/// The paragraph every list's header ends with, explaining the markers.
///
/// `regenerate` is the command that rewrites this particular list.
pub fn marker_legend(regenerate: &str) -> String {
    format!(
        "#\n\
         # One id per line, with a note after it, and a marker in front of the\n\
         # id saying what kind of claim the line makes:\n\
         #\n\
         #   ID    `cinrs` gets this one wrong.\n\
         #   ?ID   It passes or fails depending on the toolchain, and is\n\
         #         guarded neither way.\n\
         #   !ID   This entry point makes the case invalid, so refusing it is\n\
         #         conforming behaviour and not a gap. Guard mode requires the\n\
         #         case to fail to *compile*; one that builds and runs is a\n\
         #         failure. The note may open with `error: \"<substring>\"`,\n\
         #         which the diagnostic then has to contain.\n\
         #\n\
         # Regenerate with\n\
         #\n\
         {regenerate}\
         #\n\
         # which keeps every `?` and `!` line as it stands.\n\
         \n"
    )
}

// ---------------------------------------------------------------------------
// the `!` assertion
// ---------------------------------------------------------------------------

/// What a `!` case actually did, measured against what its line requires.
#[derive(Clone, Debug)]
pub enum Rejection {
    /// Refused at compile time, with the diagnostic the line asked for.
    AsRequired,
    /// Refused, but not in the words the line names.
    WrongDiagnostic {
        /// The substring the line demanded.
        wanted: String,
        /// The first line of what the compiler actually said.
        detail: String,
    },
    /// It got past the compiler; what happened after that is beside the point.
    NotRejected {
        /// What it did instead.
        what: String,
    },
}

impl Rejection {
    /// Whether the case did what its `!` line requires.
    pub fn is_as_required(&self) -> bool {
        matches!(self, Rejection::AsRequired)
    }

    /// The sentence that goes after `ID in <list> …`.
    pub fn complaint(&self) -> Option<String> {
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
pub fn check_rejection(diagnostic: Option<&str>, outcome: &Outcome) -> Rejection {
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
pub fn rejections<'a>(
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
/// builds it. `-Cstrip=symbols` is there because a thousand unstripped
/// binaries are several gigabytes and the same thousand stripped are a
/// fraction of that; `extra_flags` is what a harness whose cases are not
/// programs — a `//@check-pass` library — adds on top.
pub fn test_config(
    root: &Path,
    out_dir: &Path,
    compile_timeout: u64,
    extra_flags: &[&str],
) -> Result<Config> {
    let mut config = Config::rustc(root);
    config.out_dir = out_dir.to_path_buf();
    let defaults = config.comment_defaults.base();
    defaults.set_custom("edition", Edition("2024".to_owned()));
    defaults.set_custom("dependencies", DependencyBuilder::default());
    defaults.compile_flags.push("-Cstrip=symbols".to_owned());
    for flag in extra_flags {
        defaults.compile_flags.push((*flag).to_owned());
    }
    // Blessing would overwrite the corpus's own expected output with whatever
    // we produced, which is the opposite of the point.
    config.output_conflict_handling = error_on_output_conflict;
    // The host has to be read off the compiler *before* the compiler is
    // wrapped in anything, since reading it means running it with `-vV`.
    config.fill_host_and_target()?;
    wrap_in_limits(&mut config, compile_timeout, memory_limit_mb());
    Ok(config)
}

/// Puts a clock and a memory ceiling in front of the compiler.
///
/// `ui_test` has neither a timeout nor a memory limit of its own, and a front
/// end that hung — or that asked for a hundred gigabytes because it was wrong
/// about an array bound — would take `cargo test` with it. The generated
/// programs carry their own [`watchdog`] for the other half of the problem;
/// this is for the compilation.
///
/// What comes out is
///
/// ```text
/// sh -c 'ulimit -v "$0"; exec "$@"' <kb> timeout -k 5 <seconds> rustc …
/// ```
///
/// with either half left out when it is switched off or unavailable. The
/// order matters: the shell sets the limit and then `exec`s, so `timeout` and
/// the compiler under it both inherit it, and `timeout` is the one process
/// left waiting rather than a shell as well.
///
/// Call it *after* [`Config::fill_host_and_target`], which reads the host off
/// the compiler by running it.
pub fn wrap_in_limits(config: &mut Config, seconds: u64, limit_mb: u64) {
    if seconds > 0 && have_timeout() {
        let mut args: Vec<OsString> = vec!["-k".into(), "5".into(), seconds.to_string().into()];
        args.push(config.program.program.clone().into_os_string());
        args.append(&mut config.program.args);
        config.program.program = PathBuf::from("timeout");
        config.program.args = args;
    }
    if let Some((shell, mut prefix)) = memory_limit_prefix(limit_mb) {
        prefix.push(config.program.program.clone().into_os_string());
        prefix.append(&mut config.program.args);
        config.program.program = shell;
        config.program.args = prefix;
    }
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
/// sees, and is the silent one — `Box::new(())` — for the runs whose failures
/// are the subject rather than a problem.
pub fn run(config: Config, emitter: Box<dyn StatusEmitter>, collector: &Collector) -> Result<()> {
    run_tests_generic(
        vec![config],
        ui_test::default_file_filter,
        ui_test::default_per_file_config,
        (emitter, collector.clone()),
    )
}

// ---------------------------------------------------------------------------
// formatting
// ---------------------------------------------------------------------------

/// `90.0%`, or `n/a` when there is nothing to divide by.
pub fn percent(part: usize, whole: usize) -> String {
    if whole == 0 {
        return "n/a".to_owned();
    }
    #[allow(clippy::cast_precision_loss)]
    let rate = part as f64 * 100.0 / whole as f64;
    format!("{rate:.1}%")
}

/// `1 entry` or `3 entries`.
pub fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// A headline with an indented list of ids under it.
pub fn listing<'a>(headline: &str, ids: impl Iterator<Item = &'a str>) -> String {
    let mut out = format!("{headline}:");
    for id in ids {
        write!(out, "\n    {id}").expect("writing to a String");
    }
    out
}

/// `1 m 23 s`, for a wall-clock figure a reader can compare runs with.
pub fn duration(elapsed: std::time::Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds >= 60 {
        format!("{} m {:02} s", seconds / 60, seconds % 60)
    } else {
        format!("{:.1} s", elapsed.as_secs_f64())
    }
}
