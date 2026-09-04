//! The shape of the control-flow graph a jumping function is lowered into.
//!
//! What the state machine *computes* is covered by the integration tests that
//! run it, and what it *reads* like by the code-generation snapshots. This file
//! covers the invariants that hold between the two: which functions take the
//! CFG path at all, and that the cleanups leave a graph with no block that
//! only forwards to another and none that cannot be reached.

use std::str::FromStr;

use cinrs_core::cfg::{Cfg, Terminator};
use cinrs_core::ir::Body;
use cinrs_core::{Options, Program, Standard, analyze, sema};
use proc_macro2::TokenStream;

/// Analyses `source`, which must be free of errors.
fn program(source: &str) -> Program {
    let mut options = Options::new(Standard::C99);
    options.c_variadic = true;
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let analysis = analyze(input, &options);
    assert!(
        !analysis.diagnostics.has_errors(),
        "the front end rejected it"
    );
    let (program, diagnostics) = sema::analyze(&analysis.unit, &options, analysis.source.unit_id());
    let errors: Vec<String> = diagnostics
        .sorted()
        .into_iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    program
}

/// The body of the named function, which must have been lowered into a graph.
fn cfg_of(source: &str, name: &str) -> Cfg {
    let program = program(source);
    let function = program
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no function named '{name}'"));
    match function.body.as_ref().expect("a definition") {
        Body::Cfg(cfg) => cfg.clone(),
        Body::Structured(_) => panic!("'{name}' was not lowered into a graph"),
    }
}

/// Whether the named function kept Rust's own control flow.
fn is_structured(source: &str, name: &str) -> bool {
    let program = program(source);
    let function = program
        .functions
        .iter()
        .find(|f| f.name == name)
        .expect("the function is defined");
    matches!(function.body, Some(Body::Structured(_)))
}

/// Checks the invariants every finished graph has.
fn check_invariants(cfg: &Cfg) {
    let count = cfg.blocks.len();
    assert!(count > 0, "a graph has at least an entry block");
    let mut reachable = vec![false; count];
    reachable[0] = true;
    let mut stack = vec![0usize];
    while let Some(index) = stack.pop() {
        let block = &cfg.blocks[index];
        let successors: Vec<usize> = match &block.term {
            Terminator::Jump { target, .. } => {
                assert!(
                    !block.stmts.is_empty() || target.0 as usize == index,
                    "block {index} only forwards to {}; jump threading missed it",
                    target.0
                );
                vec![target.0 as usize]
            }
            Terminator::Branch {
                then_blk, else_blk, ..
            } => vec![then_blk.0 as usize, else_blk.0 as usize],
            Terminator::Switch { cases, default, .. } => cases
                .iter()
                .map(|(_, blk)| blk.0 as usize)
                .chain([default.0 as usize])
                .collect(),
            Terminator::Return { .. } => Vec::new(),
            Terminator::Unreachable => panic!("block {index} has no terminator"),
        };
        for successor in successors {
            assert!(
                successor < count,
                "block {index} names a block that is gone"
            );
            if !reachable[successor] {
                reachable[successor] = true;
                stack.push(successor);
            }
        }
    }
    assert!(
        reachable.iter().all(|seen| *seen),
        "an unreachable block survived: {reachable:?}"
    );
}

const FIND: &str = "
    int find(const int *values, int n, int needle) {
        int i = 0;
    loop:
        if (i >= n) goto missing;
        if (values[i] == needle) goto found;
        i++;
        goto loop;
    found:
        return i;
    missing:
        return -1;
    }
";

#[test]
fn only_a_function_that_jumps_takes_the_cfg_path() {
    // A `goto` anywhere in the function is enough.
    assert!(!is_structured(FIND, "find"));
    // A label with nothing jumping to it is not.
    assert!(is_structured("int f(int n) { done: return n; }", "f"));
    // Nor is an ordinary `switch`, however tangled.
    assert!(is_structured(
        "int f(int n) { switch (n) { case 1: case 2: return 1; default: break; } return 0; }",
        "f"
    ));
    // A `case` label that is not a direct child of the body is.
    assert!(!is_structured(
        "int f(int n) { switch (n) { if (n) { case 1: return 1; } } return 0; }",
        "f"
    ));
    // Only the function that jumps changes shape.
    let source = format!("{FIND} int plain(int n) {{ return n + 1; }}");
    assert!(is_structured(&source, "plain"));
}

#[test]
fn the_graph_is_cleaned_up() {
    let cfg = cfg_of(FIND, "find");
    check_invariants(&cfg);
    // The entry, the two tests, the two returns and the increment: chain
    // merging leaves nothing else, and the `loop:` label costs no block of its
    // own because the test was merged into it.
    assert_eq!(cfg.blocks.len(), 6, "{cfg:#?}");
    // Both returns are their own block, and both are reachable.
    let returns = cfg
        .blocks
        .iter()
        .filter(|b| matches!(b.term, Terminator::Return { .. }))
        .count();
    assert_eq!(returns, 2);
}

#[test]
fn a_switch_becomes_one_dispatch() {
    let cfg = cfg_of(
        "void copy(char *to, const char *from, int count) {
             int n = (count + 3) / 4;
             switch (count % 4) {
             case 0: do { *to++ = *from++;
             case 3:      *to++ = *from++;
             case 2:      *to++ = *from++;
             case 1:      *to++ = *from++;
                     } while (--n > 0);
             }
         }",
        "copy",
    );
    check_invariants(&cfg);
    let dispatch = cfg
        .blocks
        .iter()
        .filter_map(|b| match &b.term {
            Terminator::Switch { cases, .. } => Some(cases.len()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(dispatch, [4], "one dispatch with one edge per label");
}

#[test]
fn every_local_gets_a_name_of_its_own() {
    let cfg = cfg_of(
        "int shadowing(int n) {
             int total = 0;
             { int x = 1; total += x; }
             { int x = 20; { int x = 300; total += x; } }
             if (n) goto out;
             total = -1;
         out:
             return total;
         }",
        "shadowing",
    );
    let names: Vec<&str> = cfg.locals.iter().map(|l| l.rust_name.as_str()).collect();
    assert_eq!(names, ["total", "x", "x_1", "x_2"]);
}

#[test]
fn a_function_local_static_is_not_hoisted() {
    let cfg = cfg_of(
        "int counter(int n) {
             static int calls;
             int local = 0;
             if (n) goto out;
             local = 1;
         out:
             calls++;
             return calls + local;
         }",
        "counter",
    );
    let names: Vec<&str> = cfg.locals.iter().map(|l| l.rust_name.as_str()).collect();
    assert_eq!(names, ["local"], "a `static` is an item, not a local");
}

#[test]
fn a_loop_keeps_its_edges() {
    let cfg = cfg_of(
        "int sum(int n) {
             int total = 0;
             for (int i = 0; i < n; i++) {
                 if (i == 3) continue;
                 if (i == 7) break;
                 total += i;
             }
             if (total < 0) goto zero;
             return total;
         zero:
             return 0;
         }",
        "sum",
    );
    check_invariants(&cfg);
    // `continue` and `break` are edges like any other, so the graph has no
    // loop construct left in it at all.
    assert!(
        cfg.blocks
            .iter()
            .any(|b| matches!(b.term, Terminator::Branch { .. }))
    );
}
