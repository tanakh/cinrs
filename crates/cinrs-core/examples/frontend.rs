//! Runs the front end (capture, preprocessor, parser, sema) over one C file and
//! prints the diagnostics — a debugging aid for bisecting pathological inputs
//! without going through `rustc`.
//!
//!     cargo run -q -p cinrs-core --example frontend -- file.c [c99|c11|c17|c23|gnu99|…] [-I dir]…
//!
//! Exit status 0 when the unit has no errors, 1 when it has, 2 on a usage error.

use std::str::FromStr;

use cinrs_core::{Dialect, Options, Standard};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let expand = args.iter().any(|a| a == "--expand" || a == "--print");
    let print = args.iter().any(|a| a == "--print");
    args.retain(|a| a != "--expand" && a != "--print");
    let mut args = args.into_iter();
    let Some(path) = args.next() else {
        eprintln!("usage: frontend FILE.c [STANDARD]");
        std::process::exit(2);
    };
    let mut standard = "gnu11".to_owned();
    let mut include_paths = Vec::new();
    let mut rest = args.collect::<Vec<_>>().into_iter();
    let mut first = true;
    while let Some(arg) = rest.next() {
        if let Some(dir) = arg.strip_prefix("-I") {
            let dir = if dir.is_empty() {
                rest.next().unwrap_or_else(|| {
                    eprintln!("-I needs a directory");
                    std::process::exit(2);
                })
            } else {
                dir.to_owned()
            };
            include_paths.push(std::path::PathBuf::from(dir));
            continue;
        }
        if first {
            standard = arg;
            first = false;
            continue;
        }
        eprintln!("unexpected argument {arg:?}");
        std::process::exit(2);
    }
    let (dialect, std_name) = match standard.strip_prefix("gnu") {
        Some(rest) => (Dialect::Gnu, format!("c{rest}")),
        None => (Dialect::Iso, standard.clone()),
    };
    let standard = match std_name.as_str() {
        "c99" => Standard::C99,
        "c11" => Standard::C11,
        "c17" => Standard::C17,
        "c23" => Standard::C23,
        other => {
            eprintln!("unknown standard {other:?}");
            std::process::exit(2);
        }
    };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|err| {
        eprintln!("{path}: {err}");
        std::process::exit(2);
    });
    let hashes = "#".repeat(
        text.split('"')
            .skip(1)
            .map(|rest| rest.bytes().take_while(|b| *b == b'#').count() + 1)
            .max()
            .unwrap_or(0),
    );
    let literal = format!("r{hashes}\"{text}\"{hashes}");
    let stream =
        proc_macro2::TokenStream::from_str(&literal).expect("the C fits in a raw string literal");
    let mut options = Options::new(standard);
    options.dialect = dialect;
    options.include_paths = include_paths;
    let started = std::time::Instant::now();
    if expand {
        let out = cinrs_core::expand(stream, &options);
        // Iteratively, with an explicit stack: the token stream of a deeply
        // nested expression is deeply nested too, and this is the one place
        // that would otherwise turn a measurement into a stack overflow.
        fn count(ts: proc_macro2::TokenStream) -> usize {
            let mut todo = vec![ts];
            let mut total = 0;
            while let Some(stream) = todo.pop() {
                for tree in stream {
                    total += 1;
                    if let proc_macro2::TokenTree::Group(g) = tree {
                        todo.push(g.stream());
                    }
                }
            }
            total
        }
        if print {
            println!("{out}");
        }
        let n = count(out);
        let rss = std::fs::read_to_string("/proc/self/statm")
            .ok()
            .and_then(|s| {
                s.split_whitespace()
                    .nth(1)
                    .and_then(|p| p.parse::<usize>().ok())
            })
            .map(|pages| pages * 4096 / (1 << 20))
            .unwrap_or(0);
        eprintln!("expand: {n} tokens, {:?}, rss {rss} MiB", started.elapsed());
        return;
    }
    let analysis = cinrs_core::analyze(stream, &options);
    let parsed = started.elapsed();
    let unit_id = analysis.source.unit_id();
    let mut diagnostics = analysis.diagnostics;
    let (_program, mut sema) = cinrs_core::sema::analyze(&analysis.unit, &options, unit_id);
    analysis.expansions.annotate(&mut sema);
    diagnostics.extend(sema);
    let total = started.elapsed();
    let map = &analysis.source.map;
    let mut errors = 0;
    for diag in diagnostics.sorted() {
        let (line, col) = map.line_col(diag.range.start);
        println!("{line}:{col}: {:?}: {}", diag.level, diag.message);
        if matches!(diag.level, cinrs_core::diag::Level::Error) {
            errors += 1;
        }
    }
    eprintln!("front end: {errors} error(s); lex+pp+parse {parsed:?}, +sema {total:?}");
    std::process::exit(if errors == 0 { 0 } else { 1 });
}
