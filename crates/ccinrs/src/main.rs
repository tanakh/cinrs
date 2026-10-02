//! `ccinrs` — a C compiler with GCC's command line, built on cinrs.
//!
//! Every C file becomes Rust through `cinrs-core`, the same front end the
//! `c99!` family of macros runs, and `rustc` compiles that Rust and links the
//! program: `make CC=ccinrs` builds a C project with no C compiler involved.
//! See `doc/ccinrs.md` for what it takes and what it does not.

mod args;
mod diag;
mod driver;
mod rustc;

use std::process::ExitCode;

fn main() -> ExitCode {
    let inv = match args::parse(std::env::args().skip(1)) {
        Ok(inv) => inv,
        Err(message) => {
            eprintln!("ccinrs: error: {message}");
            return ExitCode::FAILURE;
        }
    };
    match driver::run(&inv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(driver::Failure::Reported) => ExitCode::FAILURE,
        Err(driver::Failure::Message(message)) => {
            eprintln!("ccinrs: error: {message}");
            ExitCode::FAILURE
        }
    }
}
