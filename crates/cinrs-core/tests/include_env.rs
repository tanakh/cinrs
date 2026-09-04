//! `CINRS_INCLUDE_PATH`, in a test binary of its own.
//!
//! Setting an environment variable is a process-wide act, and in Rust 2024 an
//! `unsafe` one, because another thread reading the environment at the same
//! moment is undefined behaviour. Cargo runs each test binary as its own
//! process, so a file holding exactly one test is a place where it is safe.

use std::path::PathBuf;

use cinrs_core::lex::{LexOptions, lex_text};
use cinrs_core::pp::{Context, preprocess};
use cinrs_core::{Diagnostics, Options, Standard};

#[test]
fn the_environment_variable_is_searched_last() {
    let separator = if cfg!(windows) { ';' } else { ':' };
    let value = format!("does/not/exist{separator}tests/include2");
    // SAFETY: this binary holds one test and starts no threads of its own, so
    // nothing can be reading the environment while it is written.
    unsafe { std::env::set_var(cinrs_core::include::ENV_VAR, &value) };

    // `<order.h>` is in the environment's directory only.
    assert_eq!(run("#include <order.h>", &[]), "from_the_search_path");
    // A directory given through the options is searched before it: both hold
    // an `order.h`, and this is the one that wins.
    assert_eq!(
        run("#include <order.h>", &["tests/include"]),
        "from_the_including_directory"
    );

    unsafe { std::env::remove_var(cinrs_core::include::ENV_VAR) };
    // With the variable gone, nothing finds it any more.
    assert!(run_errors("#include <order.h>", &[]).starts_with("<order.h> file not found"));
}

/// Preprocesses `src`, asserting that nothing was reported.
#[track_caller]
fn run(src: &str, search: &[&str]) -> String {
    let (tokens, errors) = preprocess_text(src, search);
    assert!(errors.is_empty(), "unexpected errors: {errors:#?}");
    tokens.join(" ")
}

/// The first error `src` produces.
#[track_caller]
fn run_errors(src: &str, search: &[&str]) -> String {
    let (_, errors) = preprocess_text(src, search);
    errors.into_iter().next().expect("an error was expected")
}

fn preprocess_text(src: &str, search: &[&str]) -> (Vec<String>, Vec<String>) {
    let mut options = Options::new(Standard::C99);
    options.include_paths = search.iter().map(PathBuf::from).collect();
    let lex_options = LexOptions {
        standard: Standard::C99,
        dollar_in_identifiers: false,
    };
    let ctx = Context::new(src, 0);
    let mut diags = Diagnostics::new();
    let tokens = lex_text(src, ctx.base, &lex_options);
    let out = preprocess(&tokens, &ctx, &options, &mut diags);
    let spellings = out
        .tokens
        .iter()
        .filter(|t| !t.is_eof())
        .map(|t| t.kind.spelling().to_owned())
        .collect();
    let errors = diags.items().iter().map(|d| d.message.clone()).collect();
    (spellings, errors)
}
