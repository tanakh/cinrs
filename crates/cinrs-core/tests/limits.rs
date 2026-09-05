//! The translation limits of C23 5.2.5.2p1, measured.
//!
//! Every case here is an input that reaches one of the minimum limits the
//! standard asks an implementation to accept. What is being tested is not only
//! that the front end accepts them — the other test files do plenty of that —
//! but *what they cost*: a construct whose representation grows faster than the
//! source does turns a program somebody might really write into a compiler that
//! never finishes. A `struct` nested 63 deep used to need more memory than the
//! machine had, because the parser copied each specifier's member list into
//! every type derived from it and so doubled the tree at every level.
//!
//! # How it is measured
//!
//! A counting [`GlobalAlloc`] wrapper records the total number of bytes handed
//! out — allocation *traffic*, not peak usage, which is the number that
//! explodes first and the one that does not depend on when the allocator hands
//! memory back. Since the counter is global and the front end does its work on
//! a thread of its own, the measured cases take a mutex and run one at a time.
//!
//! The budgets are deliberately loose: they are here to catch a return of
//! exponential growth, not to pin down the current numbers. What the current
//! numbers are is printed, so `cargo test -p cinrs-core --test limits --
//! --nocapture` shows the whole table.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use cinrs_core::{Analysis, Options, Standard, TranslationUnit, analyze, sema};

// ---------------------------------------------------------------------------
// measuring
// ---------------------------------------------------------------------------

/// Total bytes handed out since [`ALLOCATED`] was last reset.
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

/// A `System` allocator that counts what it hands out.
struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // Only the growth is new traffic; a `Vec` that doubles a dozen times
        // should read as the size it ended up at, not as a dozen copies.
        ALLOCATED.fetch_add(new_size.saturating_sub(layout.size()), Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Held while a case is measured, so that no two measurements interleave.
static MEASURING: Mutex<()> = Mutex::new(());

/// What running the front end over one input cost.
struct Cost {
    bytes: usize,
    elapsed: Duration,
    errors: Vec<String>,
}

impl Cost {
    /// Fails unless the case was accepted and stayed inside its budget.
    fn accepted(&self, what: &str) -> &Self {
        assert!(self.errors.is_empty(), "{what}: {:#?}", self.errors);
        self.within_budget(what)
    }

    /// Fails unless the case stayed inside its budget.
    fn within_budget(&self, what: &str) -> &Self {
        println!(
            "{what:.<44} {:>7.1} MiB {:>8.1} ms  {} error(s)",
            self.bytes as f64 / (1 << 20) as f64,
            self.elapsed.as_secs_f64() * 1e3,
            self.errors.len(),
        );
        assert!(
            self.bytes < BYTE_BUDGET,
            "{what} allocated {} bytes, over the {BYTE_BUDGET} byte budget",
            self.bytes
        );
        assert!(
            self.elapsed < TIME_BUDGET,
            "{what} took {:?}, over the {TIME_BUDGET:?} budget",
            self.elapsed
        );
        self
    }

    /// Fails unless the case was refused, with a diagnostic mentioning `what`.
    fn refused(&self, what: &str, message: &str) -> &Self {
        assert_eq!(self.errors.len(), 1, "{what}: {:#?}", self.errors);
        assert!(
            self.errors[0].contains(message),
            "{what}: {:?} does not mention {message:?}",
            self.errors[0]
        );
        self.within_budget(what)
    }
}

/// The most any one of these inputs may allocate.
///
/// The largest of them — four thousand external declarations — is a few
/// megabytes; the nested `struct` at the head of this file allocated gigabytes
/// before it was fixed, and would blow this at fourteen levels.
const BYTE_BUDGET: usize = 64 << 20;

/// The longest any one of them may take, in an unoptimised build.
const TIME_BUDGET: Duration = Duration::from_secs(2);

/// Takes the lock every measured case runs under.
///
/// A test holds it for its whole body, so that neither the allocation counter
/// nor the printed table is interleaved with another case's.
fn measuring() -> MutexGuard<'static, ()> {
    MEASURING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs the lexer, preprocessor, parser and semantic analysis over `source`.
///
/// Sema goes onto a thread with a large stack, which is exactly what
/// [`cinrs_core::expand`] does with it: the recursive passes must not run on
/// the small stack `rustc` gives macro expansion.
fn measure(_guard: &MutexGuard<'static, ()>, source: &str) -> Cost {
    let options = {
        let mut options = Options::new(Standard::C23);
        options.include_paths = vec![PathBuf::from("tests/include")];
        options
    };
    // The raw string literal is how a file's worth of C reaches the front end
    // outside a procedural macro; see `cinrs_core::capture`.
    let literal = format!("r####\"{source}\"####");
    let stream = proc_macro2::TokenStream::from_str(&literal)
        .expect("the C fits in a Rust raw string literal");

    ALLOCATED.store(0, Ordering::Relaxed);
    let started = Instant::now();
    let Analysis {
        unit,
        mut diagnostics,
        source: captured,
        ..
    } = analyze(stream, &options);
    let unit_id = captured.unit_id();
    let handle = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || {
            let (_program, diags) = sema::analyze(&unit, &options, unit_id);
            diags
        })
        .expect("the analysis thread must start");
    diagnostics.extend(handle.join().expect("semantic analysis must not panic"));
    let elapsed = started.elapsed();
    let bytes = ALLOCATED.load(Ordering::Relaxed);

    Cost {
        bytes,
        elapsed,
        errors: diagnostics
            .sorted()
            .into_iter()
            .filter(|d| matches!(d.level, cinrs_core::Level::Error))
            .map(|d| d.message.clone())
            .collect(),
    }
}

/// `struct S { struct { … struct { int a; } mem; … } mem; };`, `depth` deep.
fn nested_records(depth: usize) -> String {
    format!(
        "struct S {{ {}int a;{} }};",
        "struct { ".repeat(depth),
        " } mem;".repeat(depth)
    )
}

/// `__typeof__(__typeof__(… int …))`, `depth` deep.
fn nested_typeof(depth: usize) -> String {
    let mut ty = "int".to_owned();
    for _ in 0..depth {
        ty = format!("__typeof__({ty})");
    }
    format!("{ty} x;")
}

// ---------------------------------------------------------------------------
// the bug this file was written for
// ---------------------------------------------------------------------------

#[test]
fn nested_struct_definitions_cost_what_they_are_worth() {
    let guard = measuring();
    // C23 5.2.5.2p1 asks for 63 levels; the parser's own guard allows about
    // 200. Both are checked, and so is the growth *rate* — the specifier used
    // to be copied into every type derived from it, which doubled the tree at
    // every level and needed more memory than the machine had by level 63.
    measure(&guard, &nested_records(63)).accepted("63 nested struct definitions");
    measure(&guard, &nested_records(100)).accepted("100 nested struct definitions");

    let fifty = measure(&guard, &nested_records(50));
    let hundred = measure(&guard, &nested_records(100));
    fifty.accepted("50 nested struct definitions");
    assert!(
        hundred.bytes < fifty.bytes * 3,
        "doubling the nesting multiplied the memory by {:.1}, which is not linear \
         ({} bytes at 50 levels, {} at 100)",
        hundred.bytes as f64 / fifty.bytes as f64,
        fifty.bytes,
        hundred.bytes
    );
}

#[test]
fn nested_typeof_specifiers_cost_what_they_are_worth() {
    let guard = measuring();
    // A type name holds both the specifiers it was written with and the type
    // its declarator produced, so a `typeof` inside a `typeof` was copied
    // twice per level for the same reason a `struct` was.
    let thirty = measure(&guard, &nested_typeof(30));
    let sixty = measure(&guard, &nested_typeof(60));
    thirty.accepted("30 nested typeof specifiers");
    sixty.accepted("60 nested typeof specifiers");
    assert!(
        sixty.bytes < thirty.bytes * 3,
        "doubling the nesting multiplied the memory by {:.1}",
        sixty.bytes as f64 / thirty.bytes as f64
    );
}

#[test]
fn the_tree_crosses_a_thread_boundary() {
    // Sema runs on a thread of its own, so the specifier arenas have to be
    // reachable by an id rather than by an `Rc`.
    fn assert_send<T: Send>() {}
    assert_send::<TranslationUnit>();
}

// ---------------------------------------------------------------------------
// C23 5.2.5.2p1, in the order the standard lists them
// ---------------------------------------------------------------------------

#[test]
fn nesting_limits() {
    let guard = measuring();
    // 127 nesting levels of blocks.
    let blocks = format!(
        "int f(void) {{ {} int x = 1; {} return 0; }}",
        "{".repeat(127),
        "}".repeat(127)
    );
    measure(&guard, &blocks).accepted("127 nested blocks");

    // 63 nesting levels of conditional inclusion.
    let conditional = format!("{}int x;\n{}", "#if 1\n".repeat(63), "#endif\n".repeat(63));
    measure(&guard, &conditional).accepted("63 nested #if groups");

    // 12 pointer, array and function declarators modifying one type.
    let derivations = "int ******x[2][2][2][2][2][2];";
    measure(&guard, derivations).accepted("12 declarator derivations");

    // 63 nesting levels of parenthesised declarators.
    let declarators = format!("int {}x{};", "(".repeat(63), ")".repeat(63));
    measure(&guard, &declarators).accepted("63 nested parenthesized declarators");

    // 63 nesting levels of parenthesised expressions.
    let expressions = format!(
        "int f(void) {{ return {}1{}; }}",
        "(".repeat(63),
        ")".repeat(63)
    );
    measure(&guard, &expressions).accepted("63 nested parenthesized expressions");

    // 63 nested `_Generic` selections and 63 nested conditional operators,
    // which the standard does not list but which nest the same way.
    let mut generic = "1".to_owned();
    for _ in 0..63 {
        generic = format!("_Generic({generic}, int: 1, default: 0)");
    }
    measure(&guard, &format!("int f(void) {{ return {generic}; }}")).accepted("63 nested _Generic");
    let conditionals = format!(
        "int f(int x) {{ return {}1{}; }}",
        "x ? ".repeat(63),
        " : x".repeat(63)
    );
    measure(&guard, &conditionals).accepted("63 nested conditional operators");

    // Braces nested as deeply as the records they initialise.
    let mut records = String::from("struct S0 { int a; };");
    for level in 1..=63 {
        records.push_str(&format!("struct S{level} {{ struct S{} a; }};", level - 1));
    }
    let braces = format!(
        "{records} struct S63 v = {}1{};",
        "{".repeat(64),
        "}".repeat(64)
    );
    measure(&guard, &braces).accepted("63 nested initializer braces");
}

#[test]
fn identifier_limits() {
    let guard = measuring();
    // 511 identifiers with block scope in one block.
    let locals: String = (0..511).map(|i| format!("int v{i} = {i};")).collect();
    measure(&guard, &format!("int f(void) {{ {locals} return v510; }}"))
        .accepted("511 block identifiers");

    // 4095 external identifiers in one translation unit.
    let externals: String = (0..4095).map(|i| format!("int g{i};")).collect();
    measure(&guard, &externals).accepted("4095 external identifiers");

    // 4095 macro identifiers defined at once.
    let macros: String = (0..4095).map(|i| format!("#define M{i} {i}\n")).collect();
    measure(&guard, &format!("{macros}int x = M0 + M4094;")).accepted("4095 macro identifiers");
}

#[test]
fn source_line_and_literal_limits() {
    let guard = measuring();
    // 4095 characters in one logical source line.
    let line: String = (0..300).map(|i| format!("int q{i} = {i}; ")).collect();
    assert!(line.len() > 4095);
    measure(&guard, &format!("int f(void) {{ {line} return q0; }}"))
        .accepted("a 4095 character line");

    // 4095 characters in a string literal, after concatenation.
    let literal = "\"0123456789\" ".repeat(410);
    measure(&guard, &format!("const char *s = {literal};"))
        .accepted("a 4095 character string literal");
}

#[test]
fn declaration_limits() {
    let guard = measuring();
    // 1023 case labels in one switch, as 1023 groups and as one.
    let groups: String = (0..1023)
        .map(|i| format!("case {i}: t += {i}; break;"))
        .collect();
    measure(
        &guard,
        &format!("int f(int x) {{ int t = 0; switch (x) {{ {groups} }} return t; }}"),
    )
    .accepted("1023 case groups");
    let labels: String = (0..1023).map(|i| format!("case {i}:")).collect();
    measure(
        &guard,
        &format!("int f(int x) {{ int t = 0; switch (x) {{ {labels} t = 1; break; }} return t; }}"),
    )
    .accepted("1023 case labels on one statement");

    // 1023 members in one structure, and 1023 enumeration constants.
    let members: String = (0..1023).map(|i| format!("int m{i};")).collect();
    measure(&guard, &format!("struct T {{ {members} }};")).accepted("1023 members");
    let enumerators: String = (0..1023).map(|i| format!("e{i},")).collect();
    measure(&guard, &format!("enum E {{ {enumerators} }};")).accepted("1023 enumeration constants");

    // 127 parameters in a definition and 127 arguments in a call, and the same
    // for a macro.
    let params: Vec<String> = (0..127).map(|i| format!("int p{i}")).collect();
    let args: Vec<String> = (0..127).map(|i| i.to_string()).collect();
    measure(
        &guard,
        &format!(
            "int f({}) {{ return p0; }} int g(void) {{ return f({}); }}",
            params.join(", "),
            args.join(", ")
        ),
    )
    .accepted("127 parameters and arguments");
    let macro_params: Vec<String> = (0..127).map(|i| format!("a{i}")).collect();
    measure(
        &guard,
        &format!(
            "#define M({}) a0\nint x = M({});",
            macro_params.join(", "),
            args.join(", ")
        ),
    )
    .accepted("127 macro parameters and arguments");

    // 32767 bytes in one object.
    measure(
        &guard,
        "struct S { unsigned char mem[32767]; };\n\
         int f(void) { struct S s; s.mem[0] = 1; return s.mem[0]; }",
    )
    .accepted("a 32767 byte object");
}

#[test]
fn fifteen_nested_includes() {
    let guard = measuring();
    // `tests/include/self_nesting.h` includes itself until `__COUNTER__`
    // reaches fifteen.
    measure(&guard, "#include \"self_nesting.h\"\nint x;").accepted("15 nested #includes");
}

// ---------------------------------------------------------------------------
// what is bounded on purpose
// ---------------------------------------------------------------------------

#[test]
fn a_left_associative_chain_is_not_bounded() {
    let guard = measuring();
    // C23 5.2.5.2p1 asks for 4095 characters in a logical source line, which
    // in the shortest thing worth writing — `x,` — is about two thousand
    // comma operands. A left-associative chain is a run of *siblings*, not
    // nesting: the parser takes it in a loop, and sema and code generation
    // walk the spine iteratively, so nothing but memory bounds its length.
    // 1500 of each is comfortably past what the standard asks for.
    let commas = |n: usize| {
        format!(
            "int f(void) {{ int x = 0; return ({}); }}",
            vec!["x"; n].join(", ")
        )
    };
    measure(&guard, &commas(1500)).accepted("a 1500 operand comma expression");
    measure(&guard, &commas(4000)).accepted("a 4000 operand comma expression");

    let sum = |n: usize| format!("int f(int x) {{ return {}; }}", vec!["x"; n].join(" + "));
    measure(&guard, &sum(1500)).accepted("a 1500 term sum");
    measure(&guard, &sum(4000)).accepted("a 4000 term sum");

    let ands = |n: usize| format!("int f(int x) {{ return {}; }}", vec!["x"; n].join(" && "));
    measure(&guard, &ands(1500)).accepted("a 1500 term conjunction");

    // The whole of the 4095-character logical source line C23 asks for, as
    // `n590.c` of Clang's own conformance suite writes it.
    let line: String = (0..4095 / 2).map(|n| format!("{},", n % 10)).collect();
    measure(
        &guard,
        &format!("int f(void) {{ (void)({line}0); return 0; }}"),
    )
    .accepted("a 4095 character logical source line");
}

#[test]
fn a_right_associative_chain_is_bounded_as_nesting() {
    let guard = measuring();
    // `a ? b : c ? d : e`, `a = b = c` and `p->a->b->c` each add a *level* to
    // the tree per operator, and code generation walks nesting recursively on
    // the 8 MiB `rustc` gives macro expansion. They are therefore charged to
    // `parse::MAX_RECURSION_DEPTH`, which is 200 — three times the 63 levels
    // of nesting C23 5.2.5.2p1 asks for — and going past it is a diagnostic
    // rather than a stack overflow with no message.
    let ternaries = |n: usize| format!("int f(int x) {{ return {}x; }}", "x ? x : ".repeat(n));
    measure(&guard, &ternaries(150)).accepted("150 nested conditional operators");
    measure(&guard, &ternaries(600))
        .refused("600 nested conditional operators", "nests too deeply");

    let assignments = |n: usize| format!("int f(int *p) {{ return {}*p; }}", "*p = ".repeat(n));
    measure(&guard, &assignments(150)).accepted("150 nested assignments");
    measure(&guard, &assignments(600)).refused("600 nested assignments", "nests too deeply");

    let arrows = |n: usize| {
        format!(
            "struct n {{ struct n *next; }};\n\
             struct n *f(struct n *p) {{ return p{}; }}",
            "->next".repeat(n)
        )
    };
    measure(&guard, &arrows(150)).accepted("150 postfix operators");
    measure(&guard, &arrows(600)).refused("600 postfix operators", "nests too deeply");
}

#[test]
fn nesting_past_the_guard_is_a_diagnostic() {
    let guard = measuring();
    // Nesting is bounded by `parse::MAX_RECURSION_DEPTH`, and a construct
    // past it is refused before it can overflow anything.
    measure(&guard, &nested_records(300))
        .refused("300 nested struct definitions", "nests too deeply");
    let parens = format!(
        "int f(void) {{ return {}1{}; }}",
        "(".repeat(5_000),
        ")".repeat(5_000)
    );
    measure(&guard, &parens).refused("5000 nested parentheses", "nests too deeply");
}
