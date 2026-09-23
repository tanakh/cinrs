//! The shape of the control-flow graph a jumping function is lowered into.
//!
//! What the lowering *computes* is covered by the integration tests that run
//! it, and what it *reads* like by the code-generation snapshots. This file
//! covers the invariants that hold between the two: which functions take the
//! CFG path **at all** — most `goto`s do not, and
//! [`cinrs_core::regions`] is where that line is drawn — that the
//! cleanups leave a graph with no block that only forwards to another and none
//! that cannot be reached, and that a computed `goto` is one `switch` over the
//! labels whose address GNU's `&&label` took.
//!
//! It also holds [the table](every_shape_is_lowered_by_the_tier_it_needs) of
//! which of the four lowerings each shape of jump needs, and a snapshot of what
//! the interesting ones come out as. Bless the snapshots with
//! `INSTA_UPDATE=always cargo test -p cinrs-core --test cfg`.

use std::str::FromStr;

use cinrs_core::cfg::{Cfg, Terminator};
use cinrs_core::ir::Body;
use cinrs_core::{Options, Program, Standard, analyze, expand, sema};
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
    tier(source, name) == Tier::Source
}

/// Which of the four lowerings a function's control flow got.
///
/// They are tried in this order, and each is a fallback for what the one
/// before it cannot express.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tier {
    /// Rust's own control flow, with an outward `goto` as the `break` or the
    /// `continue` of a labelled region; see [`cinrs_core::regions`].
    Source,
    /// The graph read back into loops, `if`s and `match`es; see
    /// [`cinrs_core::reloop`].
    Relooped,
    /// The same, with a state variable for an irreducible region — a cycle
    /// with two heads, which no arrangement of Rust's blocks can enter twice.
    StateVariable,
    /// The whole function as one `match` over block numbers, which only a
    /// graph whose shapes nest deeper than `rustc` parses still needs.
    Machine,
}

/// The tier the named function was lowered by.
fn tier(source: &str, name: &str) -> Tier {
    tier_of(source, name).unwrap_or_else(|| panic!("no function named '{name}'"))
}

fn tier_of(source: &str, name: &str) -> Option<Tier> {
    let program = program(source);
    let function = program.functions.iter().find(|f| f.name == name)?;
    Some(match function.body.as_ref().expect("a definition") {
        Body::Structured(_) => Tier::Source,
        Body::Cfg(cfg) => match &cfg.shape {
            None => Tier::Machine,
            Some(plan) if plan.states > 0 => Tier::StateVariable,
            Some(_) => Tier::Relooped,
        },
    })
}

/// The named function of `source`, expanded and pretty-printed on its own.
///
/// Only the one item: the module, its `#![allow(…)]` and the data-model
/// assertions every unit opens with say nothing about the lowering, and they
/// have snapshots of their own in `codegen.rs`.
fn lowered(source: &str, name: &str) -> String {
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let mut options = Options::new(Standard::C99);
    options.c_variadic = true;
    let output = expand(input, &options);
    let text = output.to_string();
    let file: syn::File = match syn::parse2(output) {
        Ok(file) => file,
        Err(error) => panic!("the expansion must be valid Rust: {error}\n{text}"),
    };
    let items = file.items.iter().find_map(|item| match item {
        syn::Item::Mod(module) => module.content.as_ref().map(|(_, items)| items),
        _ => None,
    });
    let wanted = items
        .into_iter()
        .flatten()
        .find(|item| match item {
            syn::Item::Fn(function) => function.sig.ident == name,
            _ => false,
        })
        .unwrap_or_else(|| panic!("no generated function named '{name}'\n{text}"))
        .clone();
    prettyplease::unparse(&syn::File {
        items: vec![wanted],
        ..file
    })
}

/// Checks the invariants every finished graph has.
fn check_invariants(cfg: &Cfg) {
    let count = cfg.blocks.len();
    assert!(count > 0, "a graph has at least an entry block");
    let mut reachable = vec![false; count];
    reachable[0] = true;
    let mut stack = vec![0usize];
    // The one block that may be entered and never left is the `default` of a
    // computed `goto`'s dispatch: a value that is no label's.
    let invalid: Vec<usize> = cfg
        .blocks
        .iter()
        .filter_map(|block| match &block.term {
            Terminator::Switch { default, .. } if !cfg.labels.is_empty() => {
                Some(default.0 as usize)
            }
            _ => None,
        })
        .collect();
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
            Terminator::InvalidTarget if invalid.contains(&index) => Vec::new(),
            Terminator::InvalidTarget => panic!("block {index} is no dispatch's default"),
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

/// A scanner that enters its loop in the middle.
///
/// A jump *into* a block is the one thing no Rust label can express — a
/// labelled block is only ever entered at its top — so this is a function the
/// graph is for. See [`only_a_jump_rust_cannot_make_takes_the_cfg_path`].
const FIND: &str = "
    int find(const int *values, int n, int needle) {
        int i = 0;
        if (n < 0) goto probe;
        while (i < n) {
            i++;
        probe:
            if (values[i] == needle) return i;
        }
        return -1;
    }
";

#[test]
fn only_a_jump_rust_cannot_make_takes_the_cfg_path() {
    // A jump into a loop body, an `if` branch or a `switch` group: there is no
    // way into a Rust block but its top.
    assert!(!is_structured(FIND, "find"));
    assert!(!is_structured(
        "int f(int n) { if (n) goto in_branch; if (n > 1) { in_branch: return 1; } return 0; }",
        "f"
    ));
    assert!(!is_structured(
        "int f(int n) { if (n) goto arm; switch (n) { case 1: arm: return 1; } return 0; }",
        "f"
    ));
    // A label in a block the `goto` is not inside is the same thing.
    assert!(!is_structured(
        "int f(int n) { { goto other; } { other: return 1; } }",
        "f"
    ));
    // Two labels whose regions would have to overlap without nesting: the
    // forward jump to `late` has to cross the loop `early` heads.
    assert!(!is_structured(
        "int f(int n) { if (n) goto late; early: n++; late: if (n < 3) goto early; return n; }",
        "f"
    ));
    // A declaration between the first jump and the label it names: the region
    // is a Rust block, and `x` would go out of scope at the label.
    assert!(!is_structured(
        "int f(int n) { if (n) goto done; int x = n + 1; n = x; done: return n; }",
        "f"
    ));
    // GNU's `&&label`, and the computed `goto` that is a `switch` over them.
    assert!(!is_structured(
        "int f(int n) { void *p = &&here; goto *p; here: return n; }",
        "f"
    ));
    // A `case` label that is not a direct child of its `switch` body.
    assert!(!is_structured(
        "int f(int n) { switch (n) { if (n) { case 1: return 1; } } return 0; }",
        "f"
    ));

    // An outward `goto` is a `break` of a labelled block, and a backward one a
    // `continue` of a labelled loop, so neither needs the graph.
    assert!(is_structured(
        "int f(int n) { int t = 0; while (n) { if (n == 3) goto done; t += n--; } done: return t; }",
        "f"
    ));
    assert!(is_structured(
        "int f(int n) { retry: if (n > 10) { n /= 2; goto retry; } return n; }",
        "f"
    ));
    // Both kinds at one label, which is a block in front of a loop.
    assert!(is_structured(
        "int f(int n) { if (n < 0) goto retry; n = -n; retry: n++; if (n < 0) goto retry; return n; }",
        "f"
    ));
    // A jump out of a `switch`, which the labelled-block chain is inside.
    assert!(is_structured(
        "int f(int n) { switch (n) { case 1: goto done; default: n = 0; } n++; done: return n; }",
        "f"
    ));
    // A declaration in a block *inside* the region is as scoped as C says, so
    // it is no obstacle: leaving that block through the `break` frees what it
    // holds, which is what leaving it any other way would do.
    assert!(is_structured(
        "int f(int n) { { int a[n]; a[0] = 1; if (n) goto done; n += a[0]; } n++; done: return n; }",
        "f"
    ));
    // A label with nothing jumping to it needs nothing at all.
    assert!(is_structured("int f(int n) { done: return n; }", "f"));
    // Nor does an ordinary `switch`, however tangled.
    assert!(is_structured(
        "int f(int n) { switch (n) { case 1: case 2: return 1; default: break; } return 0; }",
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
    // The entry, the loop's test, the increment, the probe and the two
    // returns: chain merging leaves nothing else, and the `probe:` label costs
    // no block of its own because the test was merged into it.
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
             while (n < 0) {
             out:
                 total = -1;
             }
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
             while (n < 0) {
             out:
                 local = 1;
             }
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
             if (n < 0) goto zero;
             for (int i = 0; i < n; i++) {
                 if (i == 3) continue;
                 if (i == 7) break;
             zero:
                 total += i;
             }
             return total;
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

#[test]
fn a_switch_with_more_groups_than_rustc_can_nest_takes_the_graph() {
    // The structured lowering puts one labelled block inside the last, one per
    // group, and `rustc`'s own parser dies on the stack somewhere past four
    // hundred of them. C23 5.2.5.2p1 asks for 1023 `case` labels in one
    // `switch`, so a big one is lowered into the graph instead, whose `match`
    // over states is flat however many there are.
    let groups = |n: u32| {
        let body: String = (0..n)
            .map(|i| format!("case {i}: t += {i}; break;"))
            .collect();
        format!("int f(int x) {{ int t = 0; switch (x) {{ {body} }} return t; }}")
    };
    assert!(is_structured(&groups(200), "f"));
    assert!(!is_structured(&groups(1023), "f"));
    let cfg = cfg_of(&groups(1023), "f");
    check_invariants(&cfg);
    assert!(
        cfg.blocks
            .iter()
            .any(|b| matches!(b.term, Terminator::Switch { .. }))
    );

    // A thousand labels on *one* statement is a single group, which the
    // structured lowering handles as one block and one pattern.
    let labels: String = (0..1023).map(|i| format!("case {i}:")).collect();
    let one_group =
        format!("int g(int x) {{ int t = 0; switch (x) {{ {labels} t = 1; break; }} return t; }}");
    assert!(is_structured(&one_group, "g"));
}

#[test]
fn a_computed_goto_is_a_switch_over_the_labels_whose_address_is_taken() {
    // `&&label` is the label's number among them, from 1, and every `goto *`
    // jumps to one dispatch block that switches on it — so the function is
    // lowered through the graph even without a `goto`, and nothing else about
    // the graph is special.
    let source = "
        int f(int n) {
            void *table[2];
            table[0] = &&one;
            table[1] = &&two;
            goto *table[n];
        one:
            return 1;
        two:
            return 2;
        }";
    assert!(!is_structured(source, "f"));
    let cfg = cfg_of(source, "f");
    check_invariants(&cfg);
    let mut numbers: Vec<u32> = cfg.labels.values().copied().collect();
    numbers.sort_unstable();
    assert_eq!(numbers, [1, 2], "two labels, numbered from 1");
    let dispatches: Vec<&Terminator> = cfg
        .blocks
        .iter()
        .map(|b| &b.term)
        .filter(|term| matches!(term, Terminator::Switch { cases, default, .. }
            if cases.len() == 2 && matches!(cfg.blocks[default.0 as usize].term, Terminator::InvalidTarget)))
        .collect();
    assert_eq!(dispatches.len(), 1, "one dispatch: {cfg:#?}");
    assert!(cfg.shape.is_some(), "the relooper handles it");

    // Taking an address without ever jumping through it is enough on its own
    // to take the graph path, and the label's number is still its own.
    let unjumped = "
        int g(void) {
            void *p = &&here;
            if (p == 0) return 1;
            return 0;
        here:
            return 2;
        }";
    assert!(!is_structured(unjumped, "g"));
    let cfg = cfg_of(unjumped, "g");
    check_invariants(&cfg);
    assert_eq!(cfg.labels.len(), 1);
}

// ---------------------------------------------------------------------------
// the four tiers, shape by shape
// ---------------------------------------------------------------------------

/// A forward `goto` out of a loop: a labelled block, and no graph at all.
const FORWARD: &str = "
    int f(int n) {
        int t = 0;
        while (n) { if (n == 3) goto done; t += n--; }
    done:
        return t;
    }";

/// A backward `goto`: a labelled loop.
const BACKWARD: &str = "
    int f(int n) {
    retry:
        if (n > 10) { n /= 2; goto retry; }
        return n;
    }";

/// A `goto` into the body of a loop. The loop now has two heads — its own test
/// and the label — which is irreducible, and the one thing a state variable is
/// still for.
const INTO_A_LOOP: &str = "
    int f(int n, int inside) {
        int t = 0;
        if (inside) goto mid;
        while (n > 0) {
            n--;
        mid:
            t++;
        }
        return t;
    }";

/// Duff's device: the `case` labels are heads of the `do` loop the `switch`
/// wraps, so the loop has five of them.
const DUFF: &str = "
    void f(char *to, const char *from, int count) {
        int n = (count + 3) / 4;
        switch (count % 4) {
        case 0: do { *to++ = *from++;
        case 3:      *to++ = *from++;
        case 2:      *to++ = *from++;
        case 1:      *to++ = *from++;
                } while (--n > 0);
        }
    }";

/// Two loops that jump into each other: one cycle, two heads.
const INTERLEAVED: &str = "
    int f(int n) {
        int t = 0;
        if (n & 1) goto b;
    a:
        t += n;
        if (--n <= 0) return t;
    b:
        t += 2 * n;
        if (--n <= 0) return t;
        goto a;
    }";

/// `sqlite3VdbeExec` in miniature: a label inside one `case` that the others
/// jump to. Reducible — the label is simply the block after the `switch` —
/// so it is a `match` inside a `loop` and nothing else.
const SHARED_CASE_LABEL: &str = "
    int f(int n, const int *ops) {
        int rc = 0;
        int i = 0;
        for (;;) {
            switch (ops[i]) {
            case 1: rc += 1; break;
            case 2: if (n < 0) goto fail; rc += 2; break;
            case 3: rc += 3; goto fail;
            case 4: rc += 4; break;
            case 5:
                rc = 5;
            fail:
                rc = -rc;
                goto done;
            default:
                goto done;
            }
            i++;
        }
    done:
        return rc;
    }";

/// The `statemachine` benchmark's shape: a dozen labels jumping among one
/// another. Every cycle in it has one head — `start` dominates the lot, and
/// each of the scanning loops is entered at its own label — so it is
/// reducible, and comes out as nested Rust loops with no state variable.
const LEXER: &str = r#"
    int f(const char *p) {
        int words = 0, numbers = 0, strings = 0, comments = 0, punct = 0;
    start:
        if (*p == '\0') goto done;
        if (*p == ' ') { p++; goto start; }
        if (*p >= 'a' && *p <= 'z') goto in_word;
        if (*p >= '0' && *p <= '9') goto in_number;
        if (*p == '"') goto in_string;
        if (*p == '/') goto maybe_comment;
        punct++;
        p++;
        goto start;
    in_word:
        p++;
        if ((*p >= 'a' && *p <= 'z') || (*p >= '0' && *p <= '9')) goto in_word;
        words++;
        goto start;
    in_number:
        p++;
        if (*p >= '0' && *p <= '9') goto in_number;
        numbers++;
        goto start;
    in_string:
        p++;
        if (*p == '\0') goto unterminated;
        if (*p != '"') goto in_string;
        p++;
        strings++;
        goto start;
    unterminated:
        strings--;
        goto done;
    maybe_comment:
        p++;
        if (*p != '*') { punct++; goto start; }
        p++;
        goto in_comment;
    in_comment:
        if (*p == '\0') goto done;
        if (*p == '*') goto comment_star;
        p++;
        goto in_comment;
    comment_star:
        p++;
        if (*p != '/') goto in_comment;
        p++;
        comments++;
        goto start;
    done:
        return words + 10 * numbers + 100 * strings + 1000 * comments + punct;
    }"#;

/// Nested loops left through labels of their own.
const NESTED_LOOPS: &str = "
    int f(int n, int m) {
        int t = 0;
        int i, j;
        for (i = 0; i < n; i++) {
            for (j = 0; j < m; j++) {
                if (j == 2) goto continue_outer;
                if (j == 5) goto break_outer;
                t += j;
            }
        continue_outer: ;
        }
    break_outer:
        return t;
    }";

/// A loop with three ways out, to three different labels — and a declaration
/// between the loop and them, which is what a labelled block cannot hold and
/// therefore what sends this one through the graph.
const THREE_EXITS: &str = "
    int f(int n) {
        int t = 0;
        while (n > 0) {
            if (n == 3) goto one;
            if (n == 5) goto two;
            if (n == 7) goto three;
            t += n;
            n--;
        }
        int extra = n + 1;
        t += extra;
    one:
        return t - 1;
    two:
        return t - 2;
    three:
        return t - 3;
    }";

/// A `cleanup` in a block a `goto` leaves through two different edges. Both
/// run it, innermost first, and `tests/cleanup.rs` checks the order at run
/// time.
const CLEANUP_TWO_EDGES: &str = "
    static void note(int *p);
    int f(int c) {
        int a __attribute__((cleanup(note))) = 1;
        if (c < 0) goto inside;
        while (c) {
            int b __attribute__((cleanup(note))) = 2;
            if (b) { continue; }
        inside:
            goto out;
        }
        return a;
    out:
        return 0;
    }";

/// A variable length array in a loop body a `goto` leaves.
///
/// The jump in is to a label *before* the array's scope — jumping into it is
/// an error C99 6.8.6.1p1 gives and sema reports — and the jump out leaves it,
/// which is what frees the storage.
const VLA_IN_A_LOOP: &str = "
    int f(int n, int start) {
        int t = 0;
        if (start) goto mid;
        while (n > 0) {
        mid:
            {
                int a[n < 1 ? 1 : n];
                a[0] = n;
                t += a[0];
                if (t > 100) goto out;
            }
            n--;
        }
    out:
        return t;
    }";

/// A safe function that jumps: it is generated without `unsafe`, whatever the
/// lowering does.
const SAFE_GOTO: &str = "
    __attribute__((cinrs_safe)) int f(int n, int inside) {
        int t = 0;
        if (inside) goto mid;
        while (n > 0) {
            n--;
        mid:
            t += 2;
        }
        return t;
    }";

/// GNU's `&&label` and a computed `goto` through a table of them: a `switch`
/// over the two labels, which the relooper reads like any other.
const COMPUTED_GOTO: &str = "
    int f(int n) {
        static void *const table[] = { &&one, &&two };
        goto *table[n & 1];
    one:
        return 1;
    two:
        return 2;
    }";

/// A `switch` inside a `do`/`while`, with a `continue` that leaves the
/// `switch` but not the loop — and a `goto` into the `switch` to keep it off
/// the structured path.
const SWITCH_IN_DO_WHILE: &str = "
    int f(int n) {
        int t = 0;
        if (n > 100) goto odd;
        do {
            switch (n & 3) {
            case 0: t += 1; break;
            case 1:
            odd:
                t += 2;
                continue;
            default: t += 4; break;
            }
            t += 8;
        } while (--n > 0);
        return t;
    }";

#[test]
fn every_shape_is_lowered_by_the_tier_it_needs() {
    let table: &[(&str, Tier, &str)] = &[
        ("a forward goto", Tier::Source, FORWARD),
        ("a backward goto", Tier::Source, BACKWARD),
        (
            "nested loops left through labels",
            Tier::Source,
            NESTED_LOOPS,
        ),
        ("a goto into a loop body", Tier::StateVariable, INTO_A_LOOP),
        ("Duff's device", Tier::StateVariable, DUFF),
        ("two interleaved loops", Tier::StateVariable, INTERLEAVED),
        (
            "a label shared between cases",
            Tier::Relooped,
            SHARED_CASE_LABEL,
        ),
        ("a state-machine lexer", Tier::Relooped, LEXER),
        ("a loop with three exits", Tier::Relooped, THREE_EXITS),
        (
            "a cleanup left by two edges",
            Tier::Relooped,
            CLEANUP_TWO_EDGES,
        ),
        (
            "a VLA in a loop a goto leaves",
            Tier::StateVariable,
            VLA_IN_A_LOOP,
        ),
        ("a safe function that jumps", Tier::StateVariable, SAFE_GOTO),
        ("a computed goto", Tier::Relooped, COMPUTED_GOTO),
        (
            "a switch inside a do/while",
            Tier::StateVariable,
            SWITCH_IN_DO_WHILE,
        ),
    ];
    for (what, expected, source) in table {
        assert_eq!(tier(source, "f"), *expected, "{what}");
    }
}

/// The `sqlite3VdbeExec` idiom, which is the whole point of the relooper: a
/// `match` inside a `loop`, the case bodies in its arms, the shared label as
/// the code after it, and **no** state variable.
#[test]
fn a_label_shared_between_cases_is_a_match_in_a_loop() {
    let out = lowered(SHARED_CASE_LABEL, "f");
    assert!(!out.contains("__cinrs_entry"), "{out}");
    assert!(!out.contains("__cinrs_state"), "{out}");
    insta::assert_snapshot!(out);
}

/// The `statemachine` kernel's shape, which is reducible: one Rust loop per
/// cycle, each named after the C label that heads it.
#[test]
fn a_state_machine_lexer_becomes_nested_loops() {
    let out = lowered(LEXER, "f");
    assert!(!out.contains("__cinrs_entry"), "{out}");
    assert!(out.contains("'start: loop"), "{out}");
    insta::assert_snapshot!(out);
}

/// A loop with three ways out.
#[test]
fn a_loop_with_three_exits() {
    insta::assert_snapshot!(lowered(THREE_EXITS, "f"));
}

/// A jump into a loop body: the state variable is inside the loop it belongs
/// to and nothing outside that loop reads it.
#[test]
fn a_jump_into_a_loop_keeps_its_state_variable_inside_it() {
    insta::assert_snapshot!(lowered(INTO_A_LOOP, "f"));
}

/// Two loops that jump into each other.
#[test]
fn two_interleaved_loops_dispatch_on_their_heads() {
    insta::assert_snapshot!(lowered(INTERLEAVED, "f"));
}

/// Both edges out of the block run the cleanup, and `a`'s runs after `b`'s.
#[test]
fn a_cleanup_left_by_two_edges() {
    insta::assert_snapshot!(lowered(CLEANUP_TWO_EDGES, "f"));
}

/// A `switch` inside a `do`/`while` with a `continue` in it.
#[test]
fn a_switch_inside_a_do_while() {
    insta::assert_snapshot!(lowered(SWITCH_IN_DO_WHILE, "f"));
}

/// A safe function's jumps are generated without `unsafe`, whatever tier they
/// need.
#[test]
fn a_safe_function_that_jumps_has_no_unsafe_block() {
    let out = lowered(SAFE_GOTO, "f");
    assert!(!out.contains("unsafe {"), "{out}");
    insta::assert_snapshot!(out);
}

/// The table fold: a `static` table of distinct label addresses that is only
/// ever read has its labels numbered in its order, so `goto *table[e]` stores
/// `e + 1` and never reads the table. Any other use of the table — written,
/// addressed, passed on, or holding something that is not a label — keeps the
/// plain lowering, which reads it.
#[test]
fn only_a_table_that_is_only_read_is_folded() {
    let source = "
        void use(void **t);
        int folded(int i) {
            static void *table[] = { &&b, &&a };
            goto *table[i];
        a: return 1;
        b: return 2;
        }
        int written(int i) {
            static void *table[] = { &&a, &&b };
            table[1] = &&a;
            goto *table[i];
        a: return 1;
        b: return 2;
        }
        int addressed(int i) {
            static void *table[] = { &&a, &&b };
            void **p = table;
            goto *p[i];
        a: return 1;
        b: return 2;
        }
        int passed(int i) {
            static void *table[] = { &&a, &&b };
            use(table);
            goto *table[i];
        a: return 1;
        b: return 2;
        }
        int not_a_label(int i) {
            static void *table[] = { &&a, 0 };
            goto *table[i];
        a: return 1;
        b: return 2;
        }
        int repeated(int i) {
            static void *table[] = { &&a, &&a, &&b };
            goto *table[i];
        a: return 1;
        b: return 2;
        }
        /* An automatic array, as `interp_goto.c` keeps its table. */
        int automatic(int i) {
            void *table[] = { &&b, &&a };
            goto *table[i];
        a: return 1;
        b: return 2;
        }
        int automatic_written(int i) {
            void *table[] = { &&a, &&b };
            if (i > 1) table[0] = &&b;
            goto *table[i & 1];
        a: return 1;
        b: return 2;
        }";
    let folded = lowered(source, "folded");
    assert!(folded.contains("wrapping_add(1)"), "{folded}");
    let automatic = lowered(source, "automatic");
    assert!(automatic.contains("wrapping_add(1)"), "{automatic}");
    insta::assert_snapshot!("automatic", automatic);
    assert!(!folded.contains("folded_table)"), "{folded}");
    // The table's order, not the labels' order in the source: `b` is 1.
    let cfg = cfg_of(source, "folded");
    let mut numbers: Vec<u32> = cfg.labels.values().copied().collect();
    numbers.sort_unstable();
    assert_eq!(numbers, [1, 2]);
    for name in [
        "written",
        "addressed",
        "passed",
        "not_a_label",
        "repeated",
        "automatic_written",
    ] {
        let out = lowered(source, name);
        assert!(
            !out.contains("wrapping_add(1)"),
            "{name} was folded:\n{out}"
        );
        assert!(out.contains("as ::core::ffi::c_ulong;"), "{name}:\n{out}");
    }
    insta::assert_snapshot!(folded);
}

/// A computed `goto` is a `match` on the label's number, relooped like any
/// other `switch`: no state machine.
#[test]
fn a_computed_goto_is_relooped() {
    let out = lowered(COMPUTED_GOTO, "f");
    assert!(!out.contains("__cinrs_state"), "{out}");
    assert!(out.contains("match __cinrs_goto"), "{out}");
    insta::assert_snapshot!(out);
}
