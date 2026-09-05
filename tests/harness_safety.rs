//! The conformance harnesses' own safety net, checked.
//!
//! [`tests/support/conformance.rs`](conformance) is shared by the three
//! corpus harnesses, and the part of it that matters most is the part that
//! never runs on a good day: the ceilings that keep a front end under
//! development from taking the machine with it. A thousand generated programs
//! written by a compiler that is being *debugged* will, sooner or later,
//! include one that allocates until there is nothing left, and one `rustc` per
//! core on top of that is what actually exhausted this machine — twice.
//!
//! So each protection has a test here that provokes it:
//!
//! * the harness-process watchdog aborts a child that outgrows a 64 MiB
//!   ceiling, and says so;
//! * the compiler wrapper really is `sh -c 'ulimit -v …' … timeout … rustc`,
//!   and a small enough ceiling really does stop `rustc` starting;
//! * the watchdog compiled into a generated program kills that program, with
//!   a message, rather than letting it swallow the machine;
//! * the default parallelism is capped, and an explicit `--test-threads` is
//!   left alone.
//!
//! The first of them re-runs *this test binary* as a child, with
//! `CINRS_MEMORY_WATCHDOG_CHILD=1` set, which is what turns
//! [`memory_watchdog_child`] from a no-op into the runaway; that is the only
//! way to observe an abort without aborting the test runner.
//!
//! All four are seconds, not minutes: the one compilation is a twenty-line
//! program with no dependencies.

use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "support/conformance.rs"]
mod conformance;

/// The environment variable that turns [`memory_watchdog_child`] on.
const CHILD: &str = "CINRS_MEMORY_WATCHDOG_CHILD";

/// The ceiling the two runaway tests are given, in MiB.
///
/// Comfortably above what a test binary or a `println!`-less Rust program
/// needs to start, and comfortably below anything that would inconvenience
/// the machine if a watchdog were broken.
const SMALL_LIMIT_MB: u64 = 64;

/// Where the compiled runaway goes.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new("target").join("harness-safety").join(name);
    std::fs::create_dir_all(&dir).expect("target/ is writable");
    dir
}

// ---------------------------------------------------------------------------
// the harness-process watchdog
// ---------------------------------------------------------------------------

/// The runaway the test below starts, and a no-op in an ordinary run.
///
/// It has to be a `#[test]` rather than a `main`, because this file is a
/// harness test binary and `--exact` is the only way in.
#[test]
fn memory_watchdog_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    conformance::start_memory_watchdog("harness-safety");
    // Four mebibytes every twenty milliseconds, written rather than merely
    // reserved so that the resident set is what grows. At that rate the
    // watchdog's 250 ms poll sees the ceiling crossed long before the process
    // is large enough to matter.
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    loop {
        blocks.push(vec![1u8; 4 << 20]);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn the_watchdog_aborts_a_harness_process_that_outgrows_the_limit() {
    if conformance::resident_mb().is_none() {
        return; // not Linux; there is nothing to watch with.
    }
    let exe = std::env::current_exe().expect("a test binary has a path");
    let output = Command::new(exe)
        .args(["--exact", "memory_watchdog_child", "--nocapture"])
        .args(["--test-threads", "1"])
        .env(CHILD, "1")
        .env("CINRS_MEMORY_LIMIT_MB", SMALL_LIMIT_MB.to_string())
        .output()
        .expect("the test binary must be runnable");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        killed_by_abort(&output.status),
        "the child should have been aborted, and instead {:?}\n{stderr}",
        output.status
    );
    assert!(
        stderr.contains("harness-safety: aborting") && stderr.contains("CINRS_MEMORY_LIMIT_MB"),
        "the abort must say what happened, and instead said:\n{stderr}"
    );
}

/// Whether the process was killed by `SIGABRT`.
#[cfg(unix)]
fn killed_by_abort(status: &std::process::ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt as _;
    status.signal() == Some(6)
}

/// Whether the process was killed by `SIGABRT`.
#[cfg(not(unix))]
fn killed_by_abort(status: &std::process::ExitStatus) -> bool {
    !status.success()
}

// ---------------------------------------------------------------------------
// the compiler wrapper
// ---------------------------------------------------------------------------

#[test]
fn the_compiler_is_wrapped_in_a_clock_and_a_ceiling() {
    let mut config = ui_test::Config::rustc("tests/ui");
    let compiler = config.program.program.display().to_string();
    conformance::wrap_in_limits(&mut config, 300, 4096);

    assert_eq!(config.program.program, Path::new("sh"));
    let line: Vec<String> = config
        .program
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    // `sh -c 'ulimit -v "$0"; exec "$@"' 4194304 timeout -k 5 300 <rustc> …`
    assert_eq!(line[0], "-c");
    assert!(line[1].contains("ulimit -v"), "{line:?}");
    assert_eq!(line[2], (4096u64 * 1024).to_string(), "{line:?}");
    assert_eq!(line[3], "timeout", "{line:?}");
    assert!(line.contains(&compiler), "{line:?}");
}

#[test]
fn a_small_enough_ceiling_stops_the_compiler() {
    // Eight mebibytes of address space is less than `rustc` needs to reach
    // `main`, so this is a check that the limit is really applied and not a
    // measurement of anything. Under a ceiling it can live with, `rustc
    // --version` succeeds — which is the other half of the claim.
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let run = |limit_mb: u64| {
        let (shell, mut args) =
            conformance::memory_limit_prefix(limit_mb).expect("a non-zero limit has a prefix");
        args.push(rustc.clone().into());
        args.push("--version".into());
        Command::new(shell)
            .args(args)
            .output()
            .expect("sh must be runnable")
    };
    assert!(
        run(4096).status.success(),
        "`rustc --version` must survive a 4 GiB ceiling"
    );
    assert!(
        !run(8).status.success(),
        "`rustc --version` must not survive an 8 MiB ceiling"
    );
    assert!(
        conformance::memory_limit_prefix(0).is_none(),
        "a zero limit switches the wrapper off"
    );
}

// ---------------------------------------------------------------------------
// the watchdog inside a generated program
// ---------------------------------------------------------------------------

#[test]
fn the_generated_watchdog_kills_a_program_that_allocates_without_bound() {
    if conformance::resident_mb().is_none() {
        return; // not Linux; the generated check reads `/proc`.
    }
    let dir = scratch("runaway");
    let source = dir.join("runaway.rs");
    let program = dir.join("runaway");
    // Ten seconds of clock as well, so that a broken memory check ends the
    // test with `timed out` rather than hanging the suite.
    let watchdog = conformance::watchdog(10, SMALL_LIMIT_MB, "runaway");
    assert!(watchdog.contains("statm"), "{watchdog}");
    std::fs::write(
        &source,
        format!(
            "fn main() {{\n{watchdog}\
             \n    let mut blocks: Vec<Vec<u8>> = Vec::new();\
             \n    loop {{\
             \n        blocks.push(vec![1u8; 4 << 20]);\
             \n        std::thread::sleep(std::time::Duration::from_millis(20));\
             \n    }}\n}}\n"
        ),
    )
    .expect("the scratch directory is writable");

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let built = Command::new(rustc)
        .args(["--edition", "2021"])
        .arg(&source)
        .arg("-o")
        .arg(&program)
        .output()
        .expect("rustc must be runnable");
    assert!(
        built.status.success(),
        "the generated watchdog must compile:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );

    let output = Command::new(&program)
        .output()
        .expect("the runaway must be runnable");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(conformance::MEMORY_EXIT_CODE),
        "the runaway must exit with the memory status, and instead {:?}\n{stderr}",
        output.status
    );
    assert!(
        stderr.contains(&format!("past the {SMALL_LIMIT_MB} MiB limit")),
        "the exit must say what happened, and instead said:\n{stderr}"
    );
}

#[test]
fn a_watchdog_with_nothing_to_watch_is_nothing() {
    assert!(conformance::watchdog(0, 0, "none").is_empty());
    assert!(conformance::watchdog(0, 64, "memory").contains("statm"));
    assert!(!conformance::watchdog(0, 64, "memory").contains("timed out"));
    assert!(conformance::watchdog(5, 0, "clock").contains("timed out"));
    assert!(!conformance::watchdog(5, 0, "clock").contains("statm"));
}

// ---------------------------------------------------------------------------
// the default parallelism
// ---------------------------------------------------------------------------

#[test]
fn the_default_thread_count_is_capped() {
    let mut args = ui_test::Args::default();
    conformance::cap_threads(&mut args).expect("the default needs no environment");
    let threads = args.threads.expect("a default was filled in").get();
    assert!(
        (1..=conformance::MAX_DEFAULT_TEST_THREADS).contains(&threads),
        "{threads} threads is outside the cap"
    );
}

#[test]
fn an_explicit_thread_count_wins() {
    let mut args = ui_test::Args {
        threads: std::num::NonZeroUsize::new(31),
        ..ui_test::Args::default()
    };
    conformance::cap_threads(&mut args).expect("an explicit count needs no environment");
    assert_eq!(args.threads.map(std::num::NonZeroUsize::get), Some(31));
}
