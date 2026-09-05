//! Real cross-compilation, checked with `cargo check --target`.
//!
//! Everything else about the target model is measured inside the front end,
//! where a `TargetModel` is just a struct; this is the test that puts a real
//! `rustc` on the other side of it. For each installed target it compiles
//! `tests/cross`, a small crate whose build script exports `CINRS_TARGET` and
//! whose C and Rust each compute the same layout from opposite ends — see
//! `tests/cross/src/lib.rs` — and it does so twice:
//!
//! * **with** the variable, where the crate must compile: `cinrs` translated
//!   for the target, so its `sizeof` agrees with `core::ffi` and the padding
//!   it wrote into each `#[repr(C)]` item is the padding `rustc` expects;
//! * **without** it, where the crate must *fail* with the data-model
//!   assertion, because `cinrs` then translated for the host. That is the
//!   whole point of the assertion, and this is where it is shown to work
//!   rather than only described.
//!
//! Nothing is linked — `cargo check` is enough, and it is what makes a Windows
//! or a wasm target testable on a Linux machine with no cross toolchain at
//! all. A target whose `rust-std` is not installed is skipped with a line
//! saying so; `rustup target add` is the fix, and the module documentation of
//! `crates/cinrs-core/src/target.rs` lists the ones this exercises.
//!
//! `CINRS_SKIP_CROSS=1` skips the whole file, for a machine where spawning
//! Cargo from inside a test is not wanted.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The targets to try, and what makes each of them worth trying.
const TARGETS: &[(&str, &str)] = &[
    (
        "i686-unknown-linux-gnu",
        "ILP32, and the i386 System V alignment of 'long long'",
    ),
    (
        "x86_64-pc-windows-gnu",
        "LLP64: a 64-bit pointer with a 32-bit 'long', and a 16-bit 'wchar_t'",
    ),
    ("wasm32-unknown-unknown", "ILP32 with no operating system"),
    (
        "aarch64-unknown-linux-gnu",
        "LP64 with an unsigned plain 'char' and an unsigned 'wchar_t'",
    ),
];

/// Where the sub-crate's artifacts go: its own directory, so that the outer
/// build is never disturbed and the two runs share a cache.
fn target_dir() -> PathBuf {
    repo_root().join("target/cross-check")
}

/// The repository root, which is where Cargo runs a test from.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The targets `rustup` has a `rust-std` for.
///
/// `None` when `rustup` cannot be asked at all, which is a reason to skip
/// rather than to fail: a distribution toolchain is a perfectly good way to
/// build this crate.
fn installed_targets() -> Option<BTreeSet<String>> {
    let out = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|line| line.trim().to_owned())
            .filter(|line| !line.is_empty())
            .collect(),
    )
}

/// The triple this machine is, which `rustc -vV` prints as `host:`.
///
/// A procedural macro cannot ask for this — that is the whole reason
/// `CINRS_TARGET` exists — but a test may, and it is what says whether a
/// target in the list above really differs from where the test is running.
fn host_triple() -> Option<String> {
    let out = Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned()))
        .arg("-vV")
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(|host| host.trim().to_owned())
}

/// Runs `cargo check --target <target>` over the sub-crate, with the build
/// script's `CINRS_TARGET` on or off, and returns what came out.
fn check(target: &str, manifest: &Path, export: bool) -> (bool, String) {
    check_with(target, manifest, export, None)
}

/// The same, with a `CINRS_TARGET` forced into the environment `cargo` passes
/// down — which is how a value the *user* set, rather than a build script,
/// reaches the macro.
fn check_with(
    target: &str,
    manifest: &Path,
    export: bool,
    env_target: Option<&str>,
) -> (bool, String) {
    let mut cargo = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
    cargo
        .arg("check")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--target")
        .arg(target)
        .env("CARGO_TARGET_DIR", target_dir())
        .env("CINRS_CROSS_EXPORT", if export { "1" } else { "0" })
        // A sub-crate driven by a test must not inherit the harness's own
        // knobs, which would reach the very macro under test.
        .env_remove("CINRS_TARGET")
        .env_remove("CINRS_INCLUDE_PATH")
        .env_remove("RUSTFLAGS");
    if let Some(triple) = env_target {
        cargo.env("CINRS_TARGET", triple);
    }
    let out = match cargo.output() {
        Ok(out) => out,
        Err(e) => return (false, format!("could not run cargo: {e}")),
    };
    let mut text = String::from_utf8_lossy(&out.stderr).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stdout));
    (out.status.success(), text)
}

#[test]
fn each_installed_target_compiles_with_cinrs_target_and_fails_without_it() {
    if std::env::var("CINRS_SKIP_CROSS").as_deref() == Ok("1") {
        println!("cross-targets: skipped, CINRS_SKIP_CROSS=1");
        return;
    }
    let manifest = repo_root().join("tests/cross/Cargo.toml");
    if !manifest.exists() {
        println!("cross-targets: skipped, {} is missing", manifest.display());
        return;
    }
    let Some(installed) = installed_targets() else {
        println!("cross-targets: skipped, `rustup target list --installed` did not answer");
        return;
    };

    let Some(host) = host_triple() else {
        println!("cross-targets: skipped, `rustc -vV` did not name a host");
        return;
    };
    let host = host.as_str();
    let mut ran = 0;
    for (target, why) in TARGETS {
        if !installed.contains(*target) {
            println!(
                "cross-targets: {target} skipped, no rust-std installed \
                 (`rustup target add {target}`) — {why}"
            );
            continue;
        }
        ran += 1;

        // With the build script's `CINRS_TARGET`, everything has to line up.
        let (ok, output) = check(target, &manifest, true);
        assert!(
            ok,
            "cross-targets: {target} ({why}) failed to compile *with* CINRS_TARGET:\n{output}"
        );
        println!("cross-targets: {target} ok with CINRS_TARGET — {why}");

        // Without it the model is the host's. Every target in the list above
        // differs from a 64-bit Linux host in at least one thing the
        // assertion checks, so on such a host the crate must be refused; on
        // any other host, only the ones that really differ can be demanded.
        let (ok, output) = check(target, &manifest, false);
        if host == *target {
            assert!(
                ok,
                "cross-targets: {target} is the host and must compile:\n{output}"
            );
            continue;
        }
        if !differs_from_host(target, host) {
            println!(
                "cross-targets: {target} not checked without CINRS_TARGET \
                 (its data model matches this host's)"
            );
            continue;
        }
        assert!(
            !ok,
            "cross-targets: {target} compiled *without* CINRS_TARGET, so the data-model \
             assertion did not catch a host-model translation:\n{output}"
        );
        assert!(
            output.contains("cinrs: ") && output.contains("CINRS_TARGET"),
            "cross-targets: {target} failed without CINRS_TARGET, but not with the \
             data-model assertion:\n{output}"
        );
        println!("cross-targets: {target} correctly refused without CINRS_TARGET");
    }
    if ran == 0 {
        println!(
            "cross-targets: nothing to do — none of {} is installed",
            TARGETS
                .iter()
                .map(|(t, _)| *t)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // The other half of `CINRS_TARGET`: a value that names no machine cinrs
    // models is a located error at the invocation, not a silent fallback to
    // the host. The host is the target here, so nothing but the variable is
    // wrong; the build script is switched off so that its own value does not
    // override this one.
    let (ok, output) = check_with(host, &manifest, false, Some("pdp11-unknown-unix"));
    assert!(
        !ok,
        "cross-targets: a CINRS_TARGET naming an unknown machine was accepted:\n{output}"
    );
    assert!(
        output.contains("CINRS_TARGET: unknown architecture 'pdp11'")
            && output.contains("set #pragma cinrs target or unset the variable"),
        "cross-targets: an unknown CINRS_TARGET did not say what to do about it:\n{output}"
    );
    println!("cross-targets: an unknown CINRS_TARGET is reported at the invocation");
}

/// Whether the data-model assertion can be expected to fire for `target` when
/// the model used was `host`'s.
///
/// Asked of the *models*, not of a hard-coded list, so that the test says
/// something true on a host this was not written on.
fn differs_from_host(target: &str, host: &str) -> bool {
    let (Ok(t), Ok(h)) = (
        cinrs_core::TargetModel::from_triple(target),
        cinrs_core::TargetModel::from_triple(host),
    ) else {
        // An unfamiliar host: assume nothing.
        return false;
    };
    (
        t.short_bits,
        t.int_bits,
        t.long_bits,
        t.long_long_bits,
        t.ptr_bits,
        t.char_signed,
        t.max_scalar_align,
    ) != (
        h.short_bits,
        h.int_bits,
        h.long_bits,
        h.long_long_bits,
        h.ptr_bits,
        h.char_signed,
        h.max_scalar_align,
    )
}
