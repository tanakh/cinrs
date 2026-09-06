//! Conformance measured against GCC's C torture tests.
//!
//! `gcc/testsuite/gcc.c-torture/execute` is the oldest and largest body of
//! "does this compiler get C right" in existence: about 1,800 self-checking
//! programs, most of them a bug report distilled into twenty lines, each of
//! which calls `abort()` when the compiler got it wrong and returns or
//! `exit(0)`s when it did not. There are no expected-output files and no
//! options to get right — **success is exit status zero** — which makes it the
//! cheapest large corpus there is to run through a new front end, and the one
//! whose failures map most directly onto a to-do list.
//!
//! The corpus is *not* checked in and is not a submodule: GCC is far too big
//! for either. `scripts/fetch-testsuites.sh` makes a blobless, sparse checkout
//! of the one directory, pinned to a commit recorded in the script. Without it
//! there is nothing to run, so this harness prints how to fetch it and exits
//! successfully; `CINRS_TESTSUITES_REQUIRED=1` turns that skip into a failure.
//! `doc/gcc-torture.md` has the results and the licence note.
//!
//! The expected-failure lists measure `cinrs` as its **default features** build
//! it, so a build with `complex` off — where `_Complex` is a diagnostic again
//! and the cases that use one stop compiling — skips the suite the same way and
//! under the same variable; see
//! [`conformance::skip_unless_measured_build`].
//!
//! Everything shared with the [c-testsuite](c_testsuite) and [Clang](clang_c)
//! harnesses — the three modes, the expected-failure list with its markers and
//! [category tags](conformance::Category), what "correct" means, the result
//! collector, the timeouts — is in [`support/conformance.rs`](conformance).
//!
//! # The two groups
//!
//! `execute/*.c` and `execute/ieee/*.c` are run as two groups and reported
//! separately, exactly as GCC's own `execute.exp` and `ieee/ieee.exp` do. The
//! `ieee` half is the floating-point corner cases, which GCC compiles with
//! `-ffloat-store` on x86 to keep excess precision out; nothing here can do
//! that, so its rate is expected to be the lower of the two. `execute/builtins`
//! is a third `.exp` and is not run: every case in it is about a specific GCC
//! builtin being expanded a specific way.
//!
//! # The prelude
//!
//! These are C89-era programs, and a great many of them call `abort()` and
//! `exit()` with no declaration in sight, because C89 let them — which is
//! exactly what `c89!` and `gnu89!` implement, so under those two entry points
//! the declarations are left out and the implicit ones do the work. Under
//! every other entry point two lines go in front of every case:
//!
//! ```c
//! #pragma cinrs include_path "…/gcc.c-torture/execute"
//! extern void abort(void); extern void exit(int);
//! ```
//!
//! The declarations are compatible with the ones the cases write for
//! themselves and with the bundled `<stdlib.h>`, so a case that has its own is
//! unaffected; `CINRS_GCC_TORTURE_PRELUDE=0` leaves them out, which is how the
//! "how many cases needed it" number in the documentation was measured. The
//! `include_path` is there because about thirty cases `#include` a sibling
//! `.c` or `.h` out of the corpus directory, which is not where the generated
//! `.rs` file lives.
//!
//! Because the prelude is *prepended*, a line of the upstream file is two lines
//! further down in the generated one. That is the one thing this harness gives
//! up that the c-testsuite one keeps.
//!
//! # The DejaGnu directives
//!
//! GCC's own runner reads `{ dg-… }` directives out of the source. (It used to
//! read a sibling `NAME.x` Tcl file instead; there is not one left in the
//! corpus at the pinned commit, but this harness still reads one if it finds
//! it.) What is honoured, and what is not, is in [`Directives`].
//!
//! # Environment
//!
//! * `CINRS_TESTSUITES_REQUIRED=1` — fail rather than skip when the corpus is
//!   missing, or when this build is not the default-feature one the lists
//!   describe.
//! * `CINRS_GCC_TORTURE_STANDARD=gnu89|gnu99|gnu11|gnu17|gnu23|c89|c99|c11|c17|c23`
//!   — which entry point to translate with. Default `gnu11`, which is closest
//!   to the `-std=gnu17 -w` GCC compiles these with; `gnu89!` is the one these
//!   C89-era programs were really written for, and the document records both
//!   baselines.
//! * `CINRS_GCC_TORTURE_FILTER=<substring>` — only cases whose id contains it.
//! * `CINRS_GCC_TORTURE_PRELUDE=0|1` — leave out the `abort`/`exit`
//!   declarations, or put them in. They go in by default for every entry point
//!   except `c89!` and `gnu89!`, which declare an undeclared `abort` for
//!   themselves.
//! * `CINRS_GCC_TORTURE_REPORT=1` — report mode.
//! * `CINRS_GCC_TORTURE_STRICT=1` — a stale expected-failure entry is a
//!   failure and not a warning.
//! * `CINRS_GCC_TORTURE_UPDATE_EXPECTED=1` — rewrite the list from the report.
//! * `CINRS_GCC_TORTURE_TIMEOUT=<seconds>` — per-case timeout; `0` disables.
//! * `CINRS_MEMORY_LIMIT_MB=<mib>` — the ceiling this harness, every compiler
//!   it spawns and every program it runs work to; 8192 by default and `0` to
//!   switch all of it off. See the memory section of
//!   [`support/conformance.rs`](conformance#memory).
//! * `CINRS_TEST_THREADS=<n>` — the default parallelism, which is otherwise
//!   the smaller of this machine's and eight. `-- --test-threads=<n>` wins
//!   over both.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use ui_test::Args;
use ui_test::color_eyre::eyre::{Result, eyre};
use ui_test::status_emitter::StatusEmitter;

#[path = "support/conformance.rs"]
mod conformance;

use conformance::{
    Bucket, COMPILE_TIMEOUT, Category, Collector, Entry, EntryKind, MainKind, Outcome, Skipped,
    Tally, duration, flag, group_causes, listing, marker_legend, plural, raw_string_hashes,
    read_list, rejections, tally_header, tally_row, watchdog, write_list,
};

// ---------------------------------------------------------------------------
// where things live
// ---------------------------------------------------------------------------

/// The corpus, relative to the package root.
const CORPUS: &str = "third_party/gcc/gcc/testsuite/gcc.c-torture/execute";

/// What to tell someone who has not fetched it.
const FETCH_HINT: &str = "scripts/fetch-testsuites.sh gcc";

/// Where the expected-failure lists live.
const LIST_DIR: &str = "tests/gcc-torture";

/// Where the generated `.rs` files go.
const GEN_DIR: &str = "target/gcc-torture";

/// Where `ui_test` puts what it builds.
const BUILD_DIR: &str = "target/gcc-torture-build";

/// Where a generated program runs.
const WORK_DIR: &str = "target/gcc-torture-work";

/// How long a generated program may run before it is killed, in seconds.
const DEFAULT_TIMEOUT: u64 = 20;

/// The two `.exp` files of the suite, as directories under [`CORPUS`].
///
/// `""` is the corpus directory itself. `builtins` is deliberately left out;
/// see the module documentation.
const GROUPS: &[(&str, &str)] = &[("execute", ""), ("ieee", "ieee")];

/// The target the DejaGnu selectors are evaluated against.
///
/// Hard-coded rather than read off `rustc`, because everything else in this
/// harness — the data model `cinrs` compiles for, the `x86_64` in the pass
/// rates recorded in the documentation — assumes it too. A run on another
/// machine is still a run; it just evaluates `{ i?86-*-* }` the way an x86-64
/// GNU/Linux box would.
const DG_TARGET: &str = "x86_64-pc-linux-gnu";

// ---------------------------------------------------------------------------
// the entry point
// ---------------------------------------------------------------------------

/// Which `cinrs` macro a run translates with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Standard(&'static str);

impl Standard {
    /// The macro name, which is also the directory component and the suffix of
    /// the expected-failure list.
    fn name(self) -> &'static str {
        self.0
    }

    /// Whether a call to an undeclared function declares it here (C89
    /// 6.3.2.2), which is what decides whether the prelude is needed.
    fn has_implicit_declarations(self) -> bool {
        matches!(self.0, "c89" | "gnu89")
    }

    fn parse(s: &str) -> Result<Self> {
        const KNOWN: &[&str] = &[
            "c89", "c99", "c11", "c17", "c23", "gnu89", "gnu99", "gnu11", "gnu17", "gnu23",
        ];
        KNOWN
            .iter()
            .find(|name| **name == s)
            .map(|name| Standard(name))
            .ok_or_else(|| {
                eyre!(
                    "CINRS_GCC_TORTURE_STANDARD={s}: expected one of {}",
                    KNOWN.join(", ")
                )
            })
    }
}

/// The entry point a run uses when nothing says otherwise.
///
/// GCC compiles these with its own default — `-std=gnu17` — and `-w`, and
/// seventy-odd of them ask for `-std=gnu89 -fpermissive` on top. `gnu11!` is
/// the closest thing here: the GNU extensions on, and a revision old enough
/// that the C23 keywords a 1990s test may use as identifiers are still
/// identifiers. `CINRS_GCC_TORTURE_STANDARD=gnu89` runs the suite as the
/// language these programs were written in — implicit `int` and implicit
/// function declarations included — and `doc/gcc-torture.md` records that
/// baseline beside this one.
const DEFAULT_STANDARD: Standard = Standard("gnu11");

// ---------------------------------------------------------------------------
// the DejaGnu directives
// ---------------------------------------------------------------------------

/// What GCC's own runner would have made of a case, in so far as it is cheap
/// to work out.
///
/// # What is honoured
///
/// * **`dg-skip-if "reason" { targets } { include-opts } { exclude-opts }`** —
///   the case is skipped, with that reason, when the target selector matches
///   [`DG_TARGET`] *and* the option selectors do not restrict the skip to
///   options this harness does not pass. In practice none of them does at the
///   pinned commit: every `dg-skip-if` in the suite names either another
///   architecture, a freestanding implementation, or a specific `-O` flag.
/// * **`dg-require-effective-target X`** — the case is skipped when `X` is one
///   this harness answers *no* to. The only one that matters is
///   `run_expensive_tests`, which GCC itself runs only when
///   `GCC_TEST_RUN_EXPENSIVE` is set. Everything else the suite asks for —
///   `int32plus`, `int128`, `label_values`, `trampolines`, `c99_runtime` and
///   the rest — an x86-64 GNU/Linux GCC *has*, so the case is run and allowed
///   to fail rather than being filtered out of the count. That is deliberate:
///   `label_values` is computed `goto`, which `cinrs` does not have, and
///   hiding those cases would flatter the pass rate.
/// * **A sibling `NAME.x` file** — the old Tcl form of the same thing. There is
///   none left in the corpus, but if one turns up and it mentions `dg-skip-if`,
///   `unsupported` or a bare `return 1`, the case is skipped and says so.
///
/// # What is read and only reported
///
/// `dg-options`, `dg-additional-options` and `dg-add-options` are recorded in
/// [`Directives::options`] and counted in the report — `-std=gnu89`,
/// `-fpermissive` and `-fwrapv` between them explain a slice of the failures —
/// but nothing acts on them: there is no permissive mode, wrapping is what
/// `cinrs` generates anyway, and the `-std=gnu89` cases are answered by
/// running the whole suite under `gnu89!` rather than case by case.
/// `dg-xfail-if`, `dg-xfail-run-if`, `dg-require-stack-size`,
/// `dg-timeout-factor` and `dg-prune-output` are recorded and ignored.
#[derive(Clone, Debug, Default)]
struct Directives {
    /// Why GCC's runner would not run this case here, if it would not.
    skip: Option<String>,
    /// Every option the case asks to be compiled with.
    options: Vec<String>,
    /// Every `dg-require-effective-target` it names.
    requires: Vec<String>,
    /// Whether it carries an `xfail` for some target.
    xfail: bool,
}

/// One `{ dg-name args }` found in the text.
struct Dg {
    name: String,
    args: String,
}

/// Every `{ dg-… }` in `source`, in order.
///
/// The directives live inside comments, but a comment is not where the braces
/// are: scanning for `dg-` and then reading to the brace that closes the group
/// it opened is both simpler and exactly what DejaGnu does.
fn scan_dg(source: &str) -> Vec<Dg> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut from = 0;
    while let Some(at) = source[from..].find("dg-") {
        let start = from + at;
        from = start + 3;
        // It has to be the first thing in a `{ … }` group.
        let before = source[..start].trim_end();
        if !before.ends_with('{') {
            continue;
        }
        let name_end = source[start..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .map_or(source.len(), |len| start + len);
        // The group opened just before `dg-`; find the `}` that closes it.
        let mut depth = 1usize;
        let mut at = name_end;
        while at < bytes.len() && depth > 0 {
            match bytes[at] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            at += 1;
        }
        let end = if depth == 0 { at - 1 } else { bytes.len() };
        out.push(Dg {
            name: source[start..name_end].to_owned(),
            args: source[name_end..end].trim().to_owned(),
        });
        from = end;
    }
    out
}

/// Splits `"reason" { a } { b }` into its quoted strings and brace groups.
///
/// A `"…"` string and a `{ … }` group are both one item; anything else that is
/// not whitespace is a bare word, which is one item too.
fn split_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = args.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b' ' | b'\t' | b'\n' | b'\r' => at += 1,
            b'"' => {
                let end = args[at + 1..]
                    .find('"')
                    .map_or(bytes.len(), |len| at + 1 + len);
                out.push(args[at + 1..end.min(bytes.len())].to_owned());
                at = (end + 1).min(bytes.len());
            }
            b'{' => {
                let mut depth = 0usize;
                let start = at;
                while at < bytes.len() {
                    match bytes[at] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                at += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    at += 1;
                }
                out.push(args[start + 1..at.saturating_sub(1)].trim().to_owned());
            }
            _ => {
                let end = args[at..]
                    .find(char::is_whitespace)
                    .map_or(bytes.len(), |len| at + len);
                out.push(args[at..end].to_owned());
                at = end;
            }
        }
    }
    out
}

/// Whether `pattern`, a shell-style glob, matches `text`.
fn glob_matches(pattern: &str, text: &str) -> bool {
    fn go(pattern: &[char], text: &[char]) -> bool {
        match pattern.first() {
            None => text.is_empty(),
            Some('*') => {
                (0..=text.len()).any(|split| go(&pattern[1..], &text[split..]))
                    || go(&pattern[1..], text)
            }
            Some('?') => !text.is_empty() && go(&pattern[1..], &text[1..]),
            Some('[') => {
                let close = match pattern.iter().position(|&c| c == ']') {
                    Some(at) => at,
                    None => return false,
                };
                let (negate, set) = match pattern.get(1) {
                    Some('!' | '^') => (true, &pattern[2..close]),
                    _ => (false, &pattern[1..close]),
                };
                !text.is_empty()
                    && (set.contains(&text[0]) != negate)
                    && go(&pattern[close + 1..], &text[1..])
            }
            Some(&ch) => !text.is_empty() && text[0] == ch && go(&pattern[1..], &text[1..]),
        }
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    go(&pattern, &text)
}

/// What this harness claims about one DejaGnu "effective target".
///
/// `None` is "no opinion", which every caller reads in the direction that
/// keeps a case in the run rather than out of it.
fn effective_target(name: &str) -> Option<bool> {
    match name {
        // The two we answer no to. Everything the suite requires other than
        // these an x86-64 GNU/Linux GCC has, so answering yes is both accurate
        // and the honest choice: a case `cinrs` cannot compile should be a
        // failure and not a skip.
        "run_expensive_tests" => Some(false),
        "freestanding" => Some(false),
        // 32-bit x86, and a newlib option; neither is this machine.
        "ia32" | "ilp32" | "newlib_nano_io" | "avr_tiny" => Some(false),
        "lp64" | "int32" | "int32plus" | "int128" | "longlong64" | "double64" | "double64plus"
        | "size20plus" | "stdint_types" | "c99_runtime" | "fileio" | "mmap" | "signal" | "fpic"
        | "alias" | "weak" | "label_values" | "trampolines" | "indirect_jumps"
        | "indirect_calls" | "return_address" | "untyped_assembly" | "unwrapped"
        | "sse2_runtime" | "dfp" | "dfprt" | "bfloat16_runtime" | "float16_runtime"
        | "float32_runtime" | "float32x_runtime" | "float64_runtime" | "float64x_runtime"
        | "float128_runtime" | "float128x_runtime" => Some(true),
        _ => None,
    }
}

/// Evaluates a DejaGnu target selector against [`DG_TARGET`].
///
/// The grammar in use in this corpus is a list of alternatives, each of which
/// is a triple glob, an effective-target name, a `!`-negated one, or a
/// parenthesised (`{ … }`) group, with `&&` and `||` written out where the
/// alternation is not implicit. An unknown effective target is `false`, which
/// for the one caller — `dg-skip-if` — means "do not skip".
fn selector_matches(selector: &str) -> bool {
    let items = split_args(selector);
    // A `{ … }` group came back with its braces stripped, and a bare word did
    // not; the two are told apart by whether the word can be an operand.
    let mut any = false;
    let mut all = true;
    let mut conjunctive = false;
    let mut negate = false;
    let mut seen = false;
    for item in items {
        match item.as_str() {
            "!" => {
                negate = !negate;
                continue;
            }
            "&&" => {
                conjunctive = true;
                continue;
            }
            "||" => continue,
            _ => {}
        }
        let mut value = if item.contains(' ') || item.contains('!') {
            selector_matches(&item)
        } else if item.contains('-') {
            glob_matches(&item, DG_TARGET)
        } else {
            effective_target(&item).unwrap_or(false)
        };
        if negate {
            value = !value;
            negate = false;
        }
        any |= value;
        all &= value;
        seen = true;
    }
    if !seen {
        return false;
    }
    if conjunctive { all } else { any }
}

/// Whether an option selector — the third and fourth arguments of
/// `dg-skip-if` — leaves the skip applying to a run with no options at all.
///
/// `{ "*" }` matches anything; a list naming actual flags matches a run that
/// passes one of them, which this harness never does.
fn options_select_nothing(group: &str) -> bool {
    let items = split_args(group);
    items.is_empty() || items.iter().all(|item| item == "*")
}

impl Directives {
    /// Reads the directives of one case out of its source and its `.x` file.
    fn read(source: &str, x_file: Option<&str>) -> Self {
        let mut out = Directives::default();
        for dg in scan_dg(source) {
            let args = split_args(&dg.args);
            match dg.name.as_str() {
                "dg-skip-if" => {
                    let reason = args.first().cloned().unwrap_or_default();
                    let target = args.get(1).map_or("*-*-*", String::as_str);
                    let include = args.get(2);
                    let exclude = args.get(3);
                    let applies = selector_matches(target)
                        && include.is_none_or(|group| options_select_nothing(group))
                        && exclude.is_none_or(|group| !options_select_nothing(group));
                    if applies && out.skip.is_none() {
                        out.skip = Some(if reason.is_empty() {
                            format!("upstream `dg-skip-if` for {{ {target} }}")
                        } else {
                            format!("upstream `dg-skip-if`: {reason}")
                        });
                    }
                }
                "dg-require-effective-target" => {
                    if let Some(name) = args.first() {
                        out.requires.push(name.clone());
                        if effective_target(name) == Some(false) && out.skip.is_none() {
                            out.skip = Some(format!("requires the effective target `{name}`"));
                        }
                    }
                }
                "dg-options" | "dg-additional-options" | "dg-add-options" => {
                    if let Some(options) = args.first() {
                        out.options
                            .extend(options.split_whitespace().map(str::to_owned));
                    }
                }
                "dg-xfail-if" | "dg-xfail-run-if" => out.xfail = true,
                _ => {}
            }
        }
        // The old Tcl form. Nothing in the corpus has one any more, so this is
        // insurance rather than a code path with a baseline behind it.
        if let Some(text) = x_file {
            let asks_to_skip = ["dg-skip-if", "unsupported", "return 1"]
                .iter()
                .any(|needle| text.contains(needle));
            if asks_to_skip && out.skip.is_none() {
                out.skip = Some("its `.x` file asks for the case to be skipped".to_owned());
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// the corpus
// ---------------------------------------------------------------------------

/// One case.
struct Case {
    /// `execute/20000112-1` — the group and the file stem.
    id: String,
    /// `execute` or `ieee`.
    group: &'static str,
    /// The file stem alone.
    stem: String,
    /// The C program.
    source: String,
    /// What GCC's runner would have made of it.
    directives: Directives,
    /// Whether the case declares `abort` or `exit` for itself.
    ///
    /// Only used for the report line that says how much work the prelude is
    /// doing; the prelude goes in either way, since a compatible redeclaration
    /// is not an error.
    declares_own: bool,
}

/// Reads every `.c` in each group.
fn load_cases(corpus: &Path) -> Result<Vec<Case>> {
    let mut cases = Vec::new();
    for (group, subdir) in GROUPS {
        let dir = corpus.join(subdir);
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|err| eyre!("reading {}: {err}", dir.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<_>>()?;
        entries.sort();
        for path in entries {
            if path.extension().is_none_or(|ext| ext != "c") {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| eyre!("{} has no usable name", path.display()))?
                .to_owned();
            // Not every file in the suite is UTF-8 (one is Latin-1), and the
            // C has to travel through a Rust string literal.
            let Ok(source) = std::fs::read_to_string(&path) else {
                cases.push(Case {
                    id: format!("{group}/{stem}"),
                    group,
                    stem,
                    source: String::new(),
                    directives: Directives {
                        skip: Some("its source is not valid UTF-8".to_owned()),
                        ..Directives::default()
                    },
                    declares_own: false,
                });
                continue;
            };
            let x_file = std::fs::read_to_string(path.with_extension("x")).ok();
            let directives = Directives::read(&source, x_file.as_deref());
            let declares_own = declares_abort_or_exit(&source);
            cases.push(Case {
                id: format!("{group}/{stem}"),
                group,
                stem,
                source,
                directives,
                declares_own,
            });
        }
    }
    Ok(cases)
}

/// Whether the case declares `abort` or `exit`, or includes a header that
/// would.
fn declares_abort_or_exit(source: &str) -> bool {
    let text = conformance::blank_out_comments_and_literals(source);
    text.contains("<stdlib.h>")
        || text
            .split(';')
            .any(|decl| decl.contains("void") && (decl.contains("abort") || decl.contains("exit")))
}

// ---------------------------------------------------------------------------
// generating one Rust file
// ---------------------------------------------------------------------------

/// The three lines that go in front of every case; see the module docs.
///
/// Two include paths, because an `#include "sibling.c"` has to resolve against
/// the directory the *upstream* case lives in — its own group's, and the
/// corpus root, which is where the cases in `ieee/` reach for a shared header.
/// For a case in `execute/` itself the two are the same path, and naming it
/// twice costs nothing.
fn prelude(dirs: &[PathBuf], enabled: bool) -> String {
    let mut out = String::new();
    for dir in dirs {
        let path = dir.display().to_string();
        out.push_str(&format!("#pragma cinrs include_path {path:?}\n"));
    }
    // Always the same number of lines, so that a run with the declarations and
    // one without report the same positions.
    out.push_str(if enabled {
        "extern void abort(void); extern void exit(int);\n"
    } else {
        "\n"
    });
    out
}

/// How this case's `main` is declared, looking through a local `#include`.
///
/// About thirty cases are a `#define` and an `#include "sibling.c"`, and the
/// `main` is in the sibling.
fn detect_main_through_includes(dirs: &[PathBuf], source: &str) -> Option<MainKind> {
    if let Some(kind) = conformance::detect_main(source) {
        return Some(kind);
    }
    for name in quoted_includes(source) {
        for dir in dirs {
            if let Ok(included) = std::fs::read_to_string(dir.join(&name))
                && let Some(kind) = conformance::detect_main(&included)
            {
                return Some(kind);
            }
        }
    }
    None
}

/// The names of every `#include "…"` in `source`, in order.
///
/// The search runs over the text with comments and literals blanked out, which
/// is what keeps a directive inside a comment from counting; blanking preserves
/// byte offsets, so the *name* is then read out of the original at the position
/// the blanked copy found.
fn quoted_includes(source: &str) -> Vec<String> {
    let text = conformance::blank_out_comments_and_literals(source);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find("include") {
        let start = from + at;
        from = start + "include".len();
        // `#` first, with only whitespace between it and the start of a line.
        let before = text[..start].trim_end_matches([' ', '\t']);
        let Some(before) = before.strip_suffix('#') else {
            continue;
        };
        let line_start = before.rfind('\n').map_or(0, |at| at + 1);
        if !before[line_start..].trim().is_empty() {
            continue;
        }
        // The name comes from the original: the blanked copy has spaces there.
        let rest = source[from..].trim_start_matches([' ', '\t']);
        let Some(rest) = rest.strip_prefix('"') else {
            continue;
        };
        let Some((name, _)) = rest.split_once('"') else {
            continue;
        };
        if !name.contains('\n') {
            out.push(name.to_owned());
        }
    }
    out
}

/// The Rust file for one case.
fn generate_source(
    case: &Case,
    standard: Standard,
    timeout: u64,
    main: MainKind,
    prelude: &str,
    work_root: &Path,
) -> String {
    let mut c = String::with_capacity(case.source.len() + prelude.len() + 64);
    c.push_str(prelude);
    // Everything below is one raw string literal holding the whole unit.
    c.push_str(&case.source);
    if !c.ends_with('\n') {
        c.push('\n');
    }
    // The blank line first: had the file ended in a backslash continuation, it
    // splices with that and not with the pragma.
    c.push_str("\n#pragma cinrs module \"torture\"\n");
    let hashes = "#".repeat(raw_string_hashes(&c));

    let id = &case.id;
    let entry_point = standard.name();
    let options = if case.directives.options.is_empty() {
        "(none)".to_owned()
    } else {
        case.directives.options.join(" ")
    };
    let work = work_root.join(case.group).join(&case.stem);
    let work = work.display().to_string();

    let mut out = format!(
        "\
// GENERATED FILE - do not edit, do not check in.
//
// Written by `tests/gcc_torture.rs` from `{CORPUS}/{id}.c`.
//
// The C below is the upstream case verbatim behind a two-line prelude, so a
// line of the original is two lines further down here.
//
// GCC's testsuite is GPLv3+ with the GCC Runtime Library Exception; nothing
// from it is copied into this repository, and this file lives in `target/`.
//
// Upstream options: {options}
// Regenerate by running `cargo test --test gcc_torture`.

//@run
//@edition: 2024

mod unit {{
    cinrs::{entry_point}! {{ r{hashes}\"{c}\"{hashes} }}
}}

fn main() {{
    // A handful of cases write a file relative to the working directory.
    let work = std::path::Path::new({work:?});
    std::fs::create_dir_all(work).expect(\"the work directory\");
    std::env::set_current_dir(work).expect(\"the work directory\");
"
    );

    out.push_str(&watchdog(
        timeout,
        conformance::memory_limit_mb(),
        "gcc-torture",
    ));

    out.push_str(match main {
        MainKind::NoArgs => "    let status = unsafe { unit::torture::main() };\n",
        MainKind::ArgcArgv => {
            "    // GCC runs these with no arguments, so `argv` holds the program name
    // and the null pointer C requires after it.
    let mut arg0 = *b\"torture\\0\";
    let mut argv: [*mut core::ffi::c_char; 2] =
        [arg0.as_mut_ptr().cast(), core::ptr::null_mut()];
    let status = unsafe { unit::torture::main(1, argv.as_mut_ptr()) };\n"
        }
    });
    out.push_str("    std::process::exit(status as i32);\n}\n");
    out
}

/// Writes every case in `cases` into `dir`, which is emptied first.
fn generate(dir: &Path, cases: &[&Case], run: &Run<'_>) -> Result<Vec<Skipped>> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    for (group, _) in GROUPS {
        std::fs::create_dir_all(dir.join(group))?;
    }
    let mut skipped = Vec::new();
    for case in cases {
        let mut skip = |reason: String| {
            skipped.push(Skipped {
                id: case.id.clone(),
                reason,
            });
        };
        if let Some(reason) = &case.directives.skip {
            skip(reason.clone());
            continue;
        }
        if let Some(reason) = conformance::unquotable(&case.source) {
            skip(reason.to_owned());
            continue;
        }
        if conformance::confuses_ui_test(&case.source) {
            skip("its C source holds a line `ui_test` would read as a command".to_owned());
            continue;
        }
        let dirs = run.include_dirs(case.group);
        let Some(main) = detect_main_through_includes(&dirs, &case.source) else {
            skip("no `main` could be found in it or in what it includes".to_owned());
            continue;
        };
        std::fs::write(
            dir.join(case.group).join(format!("{}.rs", case.stem)),
            generate_source(
                case,
                run.standard,
                run.timeout,
                main,
                &prelude(&dirs, run.with_prelude),
                &run.work_root,
            ),
        )?;
    }
    Ok(skipped)
}

// ---------------------------------------------------------------------------
// the expected-failure list
// ---------------------------------------------------------------------------

/// Where the list for `standard` lives.
fn list_path(standard: Standard) -> PathBuf {
    let dir = Path::new(LIST_DIR);
    if standard == DEFAULT_STANDARD {
        dir.join("expected-failures.txt")
    } else {
        dir.join(format!("expected-failures-{}.txt", standard.name()))
    }
}

/// What this run says the list should hold, before it is merged with the file.
///
/// Two adjustments, both of which only matter when the list is being written.
///
/// * A note keeps the *message* and drops the ` (at line …)` every `cinrs`
///   diagnostic ends with in string-literal input. The position is of the
///   generated file, which the prelude has shifted, so it says nothing a
///   reader of this list can use.
/// * A case whose diagnostic ends `(this toolchain is older)` — a variadic
///   definition or a `va_list` object, both of which need Rust 1.99 — becomes
///   a `?` line instead of a plain one, because that is exactly what `?`
///   means: it passes on a newer compiler and fails on an older one, and
///   neither answer says the list is wrong. Doing it here rather than by hand
///   keeps it true after a regeneration, and keeps `cargo +beta test` quiet.
fn list_inputs(
    results: &BTreeMap<String, Outcome>,
    previous: &BTreeMap<String, Entry>,
) -> (BTreeMap<String, Outcome>, BTreeMap<String, Entry>) {
    let mut tidied = BTreeMap::new();
    let mut entries = previous.clone();
    for (id, outcome) in results {
        let Outcome::Failed {
            classification,
            detail,
            output,
        } = outcome
        else {
            continue;
        };
        let note = conformance::strip_position(classification).to_owned();
        // The exact phrase both of sema's toolchain gates end with — the one
        // for a variadic *definition* and the one for a `va_list` object.
        if detail.contains("(this toolchain is older)") && !entries.contains_key(id) {
            entries.insert(
                id.clone(),
                Entry {
                    kind: EntryKind::ToolchainDependent,
                    note: note.clone(),
                },
            );
        }
        tidied.insert(
            id.clone(),
            Outcome::Failed {
                classification: note,
                detail: detail.clone(),
                output: output.clone(),
            },
        );
    }
    (tidied, entries)
}

/// The comment block written above the list.
fn list_header(standard: Standard) -> String {
    let standard = standard.name();
    let regenerate = format!(
        "#     CINRS_GCC_TORTURE_REPORT=1 CINRS_GCC_TORTURE_UPDATE_EXPECTED=1 \\\n\
         #         CINRS_GCC_TORTURE_STANDARD={standard} cargo test --test gcc_torture\n"
    );
    format!(
        "# The `gcc.c-torture/execute` cases `cinrs` does not pass under `{standard}!`.\n\
         #\n\
         # Guard mode — the default, and what `cargo test` runs — skips these and\n\
         # requires every other selected case to pass. It also runs them, so that\n\
         # one which has started passing is reported and the line can go. See\n\
         # `doc/gcc-torture.md` for what the causes are, with counts.\n\
         #\n\
         # The corpus is not checked in: `scripts/fetch-testsuites.sh gcc` makes a\n\
         # sparse checkout of it, and without it the harness skips itself. This\n\
         # file therefore holds names and notes and nothing of GCC's own.\n\
         {}",
        marker_legend(&regenerate)
    )
}

// ---------------------------------------------------------------------------
// reporting
// ---------------------------------------------------------------------------

/// What the run got right, then what it got wrong and why.
///
/// The headline is the **correct** rate — passed plus the cases this entry
/// point is required to refuse — because a case that must fail and does fail
/// is not a shortfall. What is left is broken down by category, and the point
/// of the report is the section after that: about thirty distinct causes
/// account for every failure in a corpus of eighteen hundred programs, and the
/// order they come out in is the order the work should be done in.
fn print_report(
    run: &Run<'_>,
    results: &BTreeMap<String, Outcome>,
    skipped: &[Skipped],
    took: &str,
) {
    let cases = &run.selected;
    let by_id: BTreeMap<&str, &&Case> = cases.iter().map(|case| (case.id.as_str(), case)).collect();
    let skipped_ids: BTreeSet<&str> = skipped.iter().map(|s| s.id.as_str()).collect();

    let rejections = rejections(&run.expected_failures, results);
    let rejected: BTreeSet<&str> = rejections
        .iter()
        .filter(|(_, _, how)| how.is_as_required())
        .map(|(id, _, _)| *id)
        .collect();

    // A skipped case is out of the denominator: GCC would not have run it
    // either, or it could not be turned into a test at all.
    let ran: Vec<&&Case> = cases
        .iter()
        .filter(|case| !skipped_ids.contains(case.id.as_str()))
        .collect();
    let bucket = |case: &Case| -> Bucket {
        if results.get(&case.id).is_some_and(Outcome::is_pass) {
            return Bucket::Passed;
        }
        conformance::bucket(
            run.expected_failures.get(&case.id),
            rejected.contains(case.id.as_str()),
        )
    };
    let tally = |cases: &[&&Case]| {
        let mut tally = Tally::default();
        for case in cases {
            tally.count(bucket(case));
        }
        tally
    };

    let overall = tally(&ran);
    println!();
    println!(
        "gcc.c-torture/execute through `{}!`: {}",
        run.standard.name(),
        overall.headline()
    );
    println!("  {}", overall.errors_line());
    println!("  ({} not generated) — {took}", skipped.len());

    println!();
    println!("{}", tally_header("by group"));
    for (group, _) in GROUPS {
        let in_group: Vec<&&Case> = ran.iter().copied().filter(|c| c.group == *group).collect();
        println!("{}", tally_row(group, &tally(&in_group)));
    }
    let unlisted: Vec<&str> = ran
        .iter()
        .map(|case| case.id.as_str())
        .filter(|id| {
            !rejected.contains(id)
                && !run.expected_failures.contains_key(*id)
                && !results.get(*id).is_some_and(Outcome::is_pass)
        })
        .collect();
    if !unlisted.is_empty() {
        println!();
        println!(
            "  {} not in {} yet, and counted as bugs until classified:",
            plural(unlisted.len(), "failure is", "failures are"),
            run.list.display()
        );
        for id in &unlisted {
            println!("    {id}");
        }
    }

    // What the prelude is doing, and what the upstream options say.
    let needed_prelude = ran.iter().filter(|case| !case.declares_own).count();
    println!();
    println!("  the corpus");
    println!(
        "    {needed_prelude:>5}  cases declare neither `abort` nor `exit` themselves, and so \
         lean on the prelude"
    );
    let mut options: BTreeMap<&str, usize> = BTreeMap::new();
    for case in &ran {
        for option in &case.directives.options {
            *options.entry(option.as_str()).or_default() += 1;
        }
    }
    for (option, count) in options
        .iter()
        .filter(|(_, count)| **count >= 5)
        .map(|(option, count)| (*option, *count))
        .collect::<BTreeSet<_>>()
        .iter()
        .rev()
    {
        println!("    {count:>5}  ask upstream for `{option}`");
    }

    if !skipped.is_empty() {
        println!();
        println!("  not run ({})", skipped.len());
        let reasons =
            conformance::group_texts(skipped.iter().map(|s| (s.id.as_str(), s.reason.as_str())));
        for group in &reasons {
            println!("    {:>5}  {}", group.count(), group.cause);
            println!("           e.g. {}", group.example);
        }
    }

    // The failures, category by category — bugs first, since they are the
    // only rows anybody has to do something about. Within a category a
    // run-time failure and a compile-time one are different kinds of news, so
    // they are grouped separately even though both are one line of "what went
    // wrong".
    let mut compile: BTreeMap<Category, Vec<(&str, &str)>> = BTreeMap::new();
    let mut runtime: BTreeMap<Category, Vec<(&str, &str)>> = BTreeMap::new();
    let mut missing: Vec<&str> = Vec::new();
    for case in &ran {
        let id = case.id.as_str();
        let Bucket::Error(category) = bucket(case) else {
            continue;
        };
        match results.get(id) {
            Some(Outcome::Failed {
                classification,
                detail,
                ..
            }) => {
                let what = if classification.starts_with("compile error") {
                    &mut compile
                } else {
                    &mut runtime
                };
                let cause = if classification.starts_with("compile error") && !detail.is_empty() {
                    detail
                } else {
                    classification
                };
                what.entry(category).or_default().push((id, cause));
            }
            Some(_) => {}
            None => missing.push(id),
        }
    }

    // `every` names each case rather than one example. A compile failure is
    // one of sixty causes over five hundred cases and only the counts are
    // readable; a run-time failure is a *miscompilation* — the program built,
    // ran and did the wrong thing — and there are few enough of them that
    // each one is a to-do item with a name.
    let section = |title: &str, items: Option<&Vec<(&str, &str)>>, every: bool| {
        let Some(items) = items.filter(|items| !items.is_empty()) else {
            return;
        };
        let groups = group_causes(items.iter().copied());
        println!(
            "    {title}: {} in {} distinct causes",
            items.len(),
            groups.len()
        );
        for group in &groups {
            println!("      {:>5}  {}", group.count(), group.cause);
            if every {
                for case in &group.cases {
                    println!("             {case}");
                }
            } else {
                println!("             e.g. {}", group.example);
            }
        }
    };
    for category in Category::ALL {
        let count = overall.of(category);
        if count == 0 {
            continue;
        }
        println!();
        println!("  {} ({count})", category.label());
        section("compile failures", compile.get(&category), false);
        section("run-time failures", runtime.get(&category), true);
    }

    // The fourth category. Not a cause group, because there is only ever one
    // cause: this compiler is older than the feature.
    let toolchain: Vec<&str> = run
        .expected_failures
        .iter()
        .filter(|(id, entry)| {
            entry.kind == EntryKind::ToolchainDependent && by_id.contains_key(id.as_str())
        })
        .map(|(id, _)| id.as_str())
        .collect();
    if !toolchain.is_empty() {
        println!();
        println!(
            "  toolchain ({} listed, {} of them failing here)",
            toolchain.len(),
            overall.toolchain
        );
        for id in toolchain {
            println!("    {id}");
            println!("        {}", run.expected_failures[id].note);
            println!(
                "        here: {}",
                results
                    .get(id)
                    .map_or("not run".to_owned(), Outcome::describe)
            );
        }
    }

    if !rejected.is_empty() {
        println!();
        println!("  rejected as the standard requires ({})", rejected.len());
        for (id, entry, _) in rejections.iter().filter(|(_, _, how)| how.is_as_required()) {
            println!("    {id}");
            println!("        {}", entry.note);
        }
    }
    let anomalies: Vec<_> = rejections
        .iter()
        .filter_map(|(id, _, how)| Some((*id, how.complaint()?)))
        .collect();
    if !anomalies.is_empty() {
        println!();
        println!("  marked `!` but not rejected ({})", anomalies.len());
        for (id, complaint) in anomalies {
            println!("    {id}");
            println!("        {complaint}");
        }
    }

    if !missing.is_empty() {
        println!();
        println!("  no result recorded ({})", missing.len());
        for id in missing.iter().take(20) {
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
    corpus: PathBuf,
    standard: Standard,
    selected: Vec<&'a Case>,
    gen_root: PathBuf,
    build_root: PathBuf,
    work_root: PathBuf,
    with_prelude: bool,
    timeout: u64,
    compile_timeout: u64,
    args: Args,
    list: PathBuf,
    expected_failures: BTreeMap<String, Entry>,
}

impl Run<'_> {
    /// Where an `#include "…"` in a case of `group` is looked for.
    ///
    /// Always two entries, so that the prelude is always the same number of
    /// lines; for a case in `execute/` they are the same directory twice.
    fn include_dirs(&self, group: &str) -> Vec<PathBuf> {
        let subdir = GROUPS
            .iter()
            .find(|(name, _)| *name == group)
            .map_or("", |(_, subdir)| *subdir);
        let own = if subdir.is_empty() {
            self.corpus.clone()
        } else {
            self.corpus.join(subdir)
        };
        vec![own, self.corpus.clone()]
    }

    /// The `ui_test` configuration for one directory of generated cases.
    ///
    /// `-Awarnings` because these are a thousand generated crates and a
    /// warning in one of them would be compared against a `.stderr` file that
    /// does not exist; the stdout filter because the suite's contract is the
    /// *exit status* and a handful of its cases print as they go.
    fn config(&self, root: &Path) -> Result<ui_test::Config> {
        let mut config = conformance::test_config(
            root,
            &self.build_root,
            self.compile_timeout,
            &["-Awarnings"],
        )?;
        config.stdout_filter("(?s).*", "");
        Ok(config)
    }
}

fn main() -> Result<()> {
    conformance::start_memory_watchdog("gcc-torture");
    if let Some(result) = conformance::skip_unless_measured_build("gcc-torture") {
        return result;
    }
    let corpus = Path::new(CORPUS);
    if !corpus.is_dir() {
        let message = format!(
            "the GCC torture corpus is not checked out at `{CORPUS}`.\n\
             Fetch it with\n\n    {FETCH_HINT}\n\n\
             and run the test again; see doc/gcc-torture.md."
        );
        if flag("CINRS_TESTSUITES_REQUIRED") {
            return Err(eyre!(
                "{message}\n\
                 (CINRS_TESTSUITES_REQUIRED=1, so this is a failure and not a skip.)"
            ));
        }
        println!("gcc-torture: skipped — {message}");
        return Ok(());
    }
    // Absolute, because it goes into an `#include` search path inside a
    // generated file that is compiled from somewhere else.
    let corpus = std::env::current_dir()?.join(corpus);

    let standard = match std::env::var("CINRS_GCC_TORTURE_STANDARD") {
        Ok(value) => Standard::parse(&value)?,
        Err(_) => DEFAULT_STANDARD,
    };
    let timeout = conformance::timeout_var("CINRS_GCC_TORTURE_TIMEOUT", DEFAULT_TIMEOUT)?;
    let filter = std::env::var("CINRS_GCC_TORTURE_FILTER").unwrap_or_default();
    // The two declarations go in unless the entry point has C89's implicit
    // function declarations of its own — `c89!` and `gnu89!`, where a call to
    // an undeclared `abort` *is* a declaration of it and the linker resolves
    // it exactly as the prelude's would. Everywhere else they are what keeps a
    // third of the corpus from failing on the call rather than on what it is
    // testing.
    let with_prelude = match std::env::var("CINRS_GCC_TORTURE_PRELUDE") {
        Ok(value) => value != "0",
        Err(_) => !standard.has_implicit_declarations(),
    };

    let all = load_cases(&corpus)?;
    let selected: Vec<&Case> = all
        .iter()
        .filter(|case| filter.is_empty() || case.id.contains(&filter))
        .collect();

    println!(
        "gcc-torture: {} of {} cases selected for `{}!`{}{}",
        selected.len(),
        all.len(),
        standard.name(),
        if filter.is_empty() {
            String::new()
        } else {
            format!(", matching {filter:?}")
        },
        if with_prelude {
            String::new()
        } else {
            ", without the `abort`/`exit` prelude".to_owned()
        },
    );

    let work_root = std::env::current_dir()?
        .join(WORK_DIR)
        .join(standard.name());
    if work_root.exists() {
        std::fs::remove_dir_all(&work_root)?;
    }

    let list = list_path(standard);
    let run = Run {
        corpus,
        with_prelude,
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

    if flag("CINRS_GCC_TORTURE_REPORT") {
        report(&run, flag("CINRS_GCC_TORTURE_UPDATE_EXPECTED"))
    } else {
        guard(&run, flag("CINRS_GCC_TORTURE_STRICT"), !filter.is_empty())
    }
}

/// Report mode: run everything, fail nothing, say what happened.
fn report(run: &Run<'_>, update: bool) -> Result<()> {
    let started = Instant::now();
    let skipped = generate(&run.gen_root, &run.selected, run)?;

    let collector = Collector::rooted(&run.gen_root);
    let mut config = run.config(&run.gen_root)?;
    config.with_args(&run.args);
    println!(
        "gcc-torture: running {} cases…",
        run.selected.len() - skipped.len()
    );
    drop(conformance::run(config, Box::new(()), &collector));

    let results = collector.results();
    let took = duration(started.elapsed());
    print_report(run, &results, &skipped, &took);

    if update {
        for (id, _, how) in rejections(&run.expected_failures, &results) {
            if let Some(complaint) = how.complaint() {
                println!(
                    "gcc-torture: warning: {id} in {} {complaint}",
                    run.list.display()
                );
                println!(
                    "gcc-torture: warning: the `!{id}` line is kept; delete it by hand if it is wrong."
                );
            }
        }
        let (tidied, entries) = list_inputs(&results, &run.expected_failures);
        let written = write_list(&run.list, &list_header(run.standard), &tidied, &entries)?;
        println!(
            "gcc-torture: wrote {written} expected failures to {}",
            run.list.display()
        );
    }
    Ok(())
}

/// Guard mode: everything not listed as failing must pass.
fn guard(run: &Run<'_>, strict: bool, filtered: bool) -> Result<()> {
    let started = Instant::now();
    let (known_bad, expect_pass): (Vec<&Case>, Vec<&Case>) = run
        .selected
        .iter()
        .copied()
        .partition(|case| run.expected_failures.contains_key(&case.id));

    let skipped = generate(&run.gen_root, &expect_pass, run)?;
    let known_bad_root = run
        .gen_root
        .with_file_name(format!("{}-known-failures", run.standard.name()));
    let skipped_bad = generate(&known_bad_root, &known_bad, run)?;

    println!(
        "gcc-torture: {} must pass ({} of them not generated); {} listed as failing in {}",
        plural(expect_pass.len() - skipped.len(), "case", "cases"),
        skipped.len(),
        plural(known_bad.len(), "case is", "cases are"),
        run.list.display(),
    );

    let mut config = run.config(&run.gen_root)?;
    config.with_args(&run.args);
    let emitter: Box<dyn StatusEmitter> = run.args.format.into();
    let guarded = conformance::run(config, emitter, &Collector::default());

    // The known-failing ones, silently, so that one which has started passing
    // can be reported and a `!` one can be *required* to fail.
    let mut now_passing: Vec<String> = Vec::new();
    let mut nonconforming: Vec<String> = Vec::new();
    if !known_bad.is_empty() {
        let collector = Collector::rooted(&known_bad_root);
        let config = run.config(&known_bad_root)?;
        drop(conformance::run(config, Box::new(()), &collector));
        let results = collector.results();
        now_passing = known_bad
            .iter()
            .filter(|case| {
                run.expected_failures[&case.id].kind.is_failure()
                    && results.get(&case.id).is_some_and(Outcome::is_pass)
            })
            .map(|case| case.id.clone())
            .collect();
        nonconforming = rejections(&run.expected_failures, &results)
            .iter()
            .filter_map(|(id, _, how)| {
                let complaint = how.complaint()?;
                Some(format!("{id} in {} {complaint}", run.list.display()))
            })
            .collect();
    }

    // Entries naming a case this run does not have, and entries naming one it
    // decided not to generate at all — a `dg-skip-if`, say. Both mean the line
    // has nothing to guard.
    let selected_ids: BTreeSet<&str> = run.selected.iter().map(|case| case.id.as_str()).collect();
    let ungenerated: BTreeSet<&str> = skipped_bad.iter().map(|s| s.id.as_str()).collect();
    let unknown: Vec<&str> = run
        .expected_failures
        .keys()
        .map(String::as_str)
        .filter(|id| !selected_ids.contains(id) || ungenerated.contains(id))
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
                "{} in {} does not name a case this run ran",
                plural(unknown.len(), "entry", "entries"),
                run.list.display()
            ),
            unknown.into_iter(),
        ));
    }
    for problem in &problems {
        println!("gcc-torture: warning: {problem}");
    }
    if !problems.is_empty() {
        println!("gcc-torture: CINRS_GCC_TORTURE_STRICT=1 makes the above a failure.");
    }
    for complaint in &nonconforming {
        println!("gcc-torture: error: {complaint}");
    }

    guarded?;
    if !nonconforming.is_empty() {
        return Err(eyre!(
            "{}\n{}",
            nonconforming.join("\n"),
            "A `!` line records what the standard requires this entry point to refuse, \
             so this is a failure however CINRS_GCC_TORTURE_STRICT is set."
        ));
    }
    if strict && !problems.is_empty() {
        return Err(eyre!("{}", problems.join("\n")));
    }
    println!(
        "gcc-torture: all {} selected cases accounted for in {}",
        run.selected.len(),
        duration(started.elapsed())
    );
    Ok(())
}
