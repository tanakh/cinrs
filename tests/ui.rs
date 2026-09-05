//! Compile-fail tests.
//!
//! These are the tests that actually prove the crate's central promise: that
//! an error inside a `c99!` invocation is rendered by `rustc` with its caret
//! on the offending C token. The blessed `.stderr` files record exactly what
//! the user sees.
//!
//! There are three suites. `tests/ui` runs on every supported toolchain and
//! with or without the `nightly` feature, and its `.stderr` files must
//! therefore be identical in all of them — `sema::analyze` holds back the
//! diagnostics about what a *toolchain* cannot do until it knows the program
//! itself is well formed, and the dependency it is built against never carries
//! the feature, which is what makes that possible. `tests/ui-since-1.99` holds
//! the tests that need C-variadic definitions, and only runs where the
//! compiler has them. `tests/ui-nightly` holds the ones that show a diagnostic
//! pointing *inside* a string literal, which only the `nightly` feature can
//! do, so it is built against a dependency that has it.
//!
//! Bless them with `cargo test --test ui -- --bless`, and the last suite with
//! `cargo +nightly test --features nightly --test ui -- --bless`.

use ui_test::custom_flags::edition::Edition;
use ui_test::dependencies::DependencyBuilder;
use ui_test::{CommandBuilder, Config, run_tests};

#[path = "support/conformance.rs"]
mod conformance;

/// The suite that only a toolchain with `c_variadic` can compile.
#[rustversion::since(1.99)]
const C_VARIADIC_SUITE: Option<&str> = Some("tests/ui-since-1.99");
/// The suite that only a toolchain with `c_variadic` can compile.
#[rustversion::before(1.99)]
const C_VARIADIC_SUITE: Option<&str> = None;

/// The suite that needs `cinrs`'s `nightly` feature.
const SUBSPAN_SUITE: Option<&str> = if cfg!(feature = "nightly") {
    Some("tests/ui-nightly")
} else {
    None
};

/// A suite whose dependency on `cinrs` is built with the default features.
fn config(root: &str) -> Config {
    let mut config = build(root, DependencyBuilder::default());
    config.bless_command = Some("cargo test --test ui -- --bless".to_owned());
    config
}

/// A suite whose dependency on `cinrs` is built with the `nightly` feature.
///
/// The feature changes what a diagnostic inside a string literal looks like,
/// so it must reach the *dependency* and not only this test binary; and the
/// artifacts go somewhere of their own, so that the two feature sets never
/// share a build directory.
fn nightly_config(root: &str) -> Config {
    let dependencies = DependencyBuilder {
        program: CommandBuilder {
            args: ["build", "--features", "nightly"]
                .iter()
                .map(Into::into)
                .collect(),
            ..CommandBuilder::cargo()
        },
        ..DependencyBuilder::default()
    };
    let mut config = build(root, dependencies);
    config.out_dir.set_file_name("ui-nightly");
    config.bless_command =
        Some("cargo +nightly test --features nightly --test ui -- --bless".to_owned());
    config
}

fn build(root: &str, dependencies: DependencyBuilder) -> Config {
    let mut config = Config::rustc(root);
    let defaults = config.comment_defaults.base();
    defaults.set_custom("edition", Edition("2024".to_owned()));
    defaults.set_custom("dependencies", dependencies);
    // `rustc` reworded `E0433` for a crate that is not there between 1.97 and
    // 1.99 — "in the crate root" became "in the list of imported crates" —
    // and the blessed output of `tests/ui` has to be the same on every
    // supported toolchain. The message is `rustc`'s own and appears in exactly
    // one test, `no_std_without_the_pragma.rs`, where what is being blessed is
    // that the caret lands on the C declaration that needed the crate.
    config.stderr_filter("in the list of imported crates", "in the crate root");
    config
}

/// Runs one suite under the same ceilings the conformance harnesses use.
///
/// The `ui` suite is sixty tests rather than a thousand, so it was never what
/// exhausted this machine's memory — but it spawns `rustc` the same way, and
/// the protections cost nothing: see the memory section of
/// [`conformance`](conformance#memory). Wrapping the compiler has to come
/// after the host has been read off it, which is what
/// `fill_host_and_target` does; none of the three changes a byte of the
/// blessed `.stderr` files, which hold the compiler's diagnostics and not the
/// command line that produced them.
fn run(mut config: Config) -> ui_test::color_eyre::Result<()> {
    config.fill_host_and_target()?;
    conformance::wrap_in_limits(
        &mut config,
        conformance::COMPILE_TIMEOUT,
        conformance::memory_limit_mb(),
    );
    let mut args = ui_test::Args::test()?;
    conformance::cap_threads(&mut args)?;
    config.with_args(&args);
    run_tests(config)
}

fn main() -> ui_test::color_eyre::Result<()> {
    conformance::start_memory_watchdog("ui");
    run(config("tests/ui"))?;
    if let Some(root) = C_VARIADIC_SUITE {
        run(config(root))?;
    }
    if let Some(root) = SUBSPAN_SUITE {
        run(nightly_config(root))?;
    }
    Ok(())
}
