//! Runs the front end (capture, preprocessor, parser, sema) over one C file and
//! prints the diagnostics — a debugging aid for bisecting pathological inputs
//! without going through `rustc`.
//!
//!     cargo run -q -p cinrs-core --example frontend -- file.c [c99|c11|c17|c23|gnu99|…] [-I dir]…
//!
//! `--tiers` prints, instead of the diagnostics, how each function's control
//! flow was lowered — the count per tier and the name of every function that
//! needed more than the structured form. That is the measurement behind
//! `doc/translation.md`'s "Control flow and goto".
//!
//! Exit status 0 when the unit has no errors, 1 when it has, 2 on a usage error.

use std::str::FromStr;

use cinrs_core::{Dialect, Options, Standard};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let expand = args.iter().any(|a| a == "--expand" || a == "--print");
    let print = args.iter().any(|a| a == "--print");
    let tiers = args.iter().any(|a| a == "--tiers");
    args.retain(|a| a != "--expand" && a != "--print" && a != "--tiers");
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
        "c89" | "c90" => Standard::C89,
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
    // `analysis.options` rather than the ones handed in: `#pragma cinrs target`
    // and `CINRS_TARGET` are resolved into those, and sema has to see the model
    // the unit is really translated for.
    let (mut program, mut sema) =
        cinrs_core::sema::analyze(&analysis.unit, &analysis.options, unit_id);
    analysis.expansions.annotate(&mut sema);
    diagnostics.extend(sema);
    // The pragmas that can only be checked once the program and the pragmas are
    // together — which is where a `#pragma cinrs safe` naming nothing is found.
    program.export = analysis.export;
    program.no_std = analysis.no_std;
    let mut pragmas = cinrs_core::sema::check_pragmas(&program);
    pragmas.extend(cinrs_core::sema::check_safe(
        &mut program,
        &analysis.safe_functions,
    ));
    analysis.expansions.annotate(&mut pragmas);
    diagnostics.extend(pragmas);
    let total = started.elapsed();
    if tiers {
        report_tiers(&program);
        return;
    }
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

/// How each function's control flow was lowered.
///
/// Tier 1 is the structured form, which is every function whose jumps Rust can
/// make on its own — including the outward `goto`s `cinrs_core::regions` turns
/// into labelled blocks and loops. Tier 2 is the graph read back into loops,
/// `if`s and `match`es by `cinrs_core::reloop`, and tier 3 the same with a
/// state variable for an irreducible region. Tier 4 is the whole-function state
/// machine, which only a graph nested deeper than `rustc` parses still needs.
/// A function that takes a label's address (`&&label`) goes through the graph,
/// its computed `goto`s a `switch` over those labels; how many of the tier 2
/// and 3 functions do is printed too.
fn report_tiers(program: &cinrs_core::Program) {
    use cinrs_core::ir::Body;

    let mut structured = 0usize;
    let mut relooped = 0usize;
    let mut irreducible: Vec<(&str, u32)> = Vec::new();
    let mut machines: Vec<&str> = Vec::new();
    let mut label_values: Vec<&str> = Vec::new();
    for func in &program.functions {
        if let Some(Body::Cfg(cfg)) = &func.body
            && !cfg.labels.is_empty()
        {
            label_values.push(func.name.as_str());
        }
        match &func.body {
            None => {}
            Some(Body::Structured(_)) => structured += 1,
            Some(Body::Cfg(cfg)) => match &cfg.shape {
                Some(plan) if plan.states > 0 => {
                    irreducible.push((func.name.as_str(), plan.states));
                }
                Some(_) => relooped += 1,
                None => machines.push(func.name.as_str()),
            },
        }
    }
    let defined = structured + relooped + irreducible.len() + machines.len();
    println!("functions with a body: {defined}");
    println!("  tier 1  structured:                {structured}");
    println!("  tier 2  relooped:                  {relooped}");
    println!("  tier 3  relooped, state variable:  {}", irreducible.len());
    println!("  tier 4  whole-function machine:    {}", machines.len());
    println!(
        "  taking a label's address:          {}",
        label_values.len()
    );
    for name in &label_values {
        println!("    &&label: {name}");
    }
    for (name, states) in &irreducible {
        println!("    tier 3: {name} ({states} irreducible region(s))");
    }
    for name in &machines {
        println!("    tier 4: {name}");
    }
}
