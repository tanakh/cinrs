//! The `cinrs` C front end.
//!
//! This crate holds everything the `c99!`, `c11!`, `c17!` and `c23!`
//! procedural macros do — they are one pipeline parameterised by
//! [`Standard`] — but is built on [`proc_macro2`] alone and never touches
//! `proc_macro`. That makes the whole pipeline — input capture, lexing,
//! preprocessing, parsing, sema and code generation — unit-testable outside a
//! procedural macro context.
//!
//! The pipeline is:
//!
//! ```text
//! TokenStream ──capture──▶ (C source text, SourceMap)
//!             ──lex─────▶ Vec<lex::Token>     (byte ranges into that text)
//!             ──pp──────▶ Vec<pp::Token>      (directives gone, macros gone)
//!             ──parse───▶ TranslationUnit     (every node carries a range)
//!             ──sema────▶ ir::Program         (typed, conversions explicit)
//!             ──codegen─▶ TokenStream         (every token spanned at its C)
//! ```
//!
//! A function that jumps takes one more step on the way: sema hands its body
//! to [`cfg`](mod@cfg), which turns it into basic blocks that codegen emits as a state
//! machine. See that module for why, and for what it costs.
//!
//! Which revision is being compiled reaches every pass that has an opinion
//! about it: the [lexer](mod@lex) (which spellings are keywords, and the C23
//! constant forms), the [preprocessor](mod@pp) (`__VA_OPT__`, `#elifdef`,
//! `__STDC_VERSION__`), the [parser](mod@parse) (the C11 and C23 grammar) and
//! [sema](mod@sema) (a C23 keyword used as a name in an older block). A
//! feature from a later revision is a diagnostic naming the macro that would
//! have it; see [`Standard::requires`].
//!
//! Everything downstream reports problems as [`Diagnostic`]s carrying a
//! [`SourceRange`]; [`Diagnostics::to_token_stream`] turns those into
//! `compile_error!` invocations whose tokens are spanned at the exact C token
//! that caused them. That is the point of the whole design: an error — ours or
//! `rustc`'s — must land on the C code the user wrote. The lexer takes part in
//! that by *not* reporting: its problems ride on the tokens they were found
//! in, and the [preprocessor](mod@pp) reports only the ones whose token
//! survives, because a group skipped by `#if 0` may hold anything at all.
//! A token a macro produced carries the range of the *invocation*, which is
//! the only place the user can look; [`pp::Expansions::annotate`] adds the
//! `in expansion of macro 'X'` note that says how it got there.
//!
//! Capture also hands sema a [unit id](capture::Source::unit_id) identifying
//! the invocation, which every synthetic name the expansion needs is built
//! from — the renamed `extern` declarations, the mangled function-local
//! `static`s, the names given to anonymous tags, and the module the whole
//! expansion goes into (see [`expand`]), so that two `c99!` blocks in one Rust
//! module never collide.
//!
//! # Where `#include` fits
//!
//! A header is another file in the same offset space, so nothing downstream of
//! the preprocessor has to know it exists: a [`SourceRange`] identifies a file
//! as well as a position, and a diagnostic inside a header resolves to the
//! span of the `#include` that pulled it in, with the header's own path, line
//! and column put into the message text.
//!
//! The one seam is that the preprocessor runs on a thread where a
//! `proc_macro2::Span` cannot follow it, so it *allocates* the offsets of the
//! files it opens and hands them back — see [`pp::Preprocessed::included`] and
//! [`SourceMap::next_base`] — and [`analyze`] adds them to the map afterwards,
//! in the order they were opened. The [`include`](mod@include) module is where
//! a header name is resolved and what is bundled; the user headers that were
//! read come back as [`Analysis::user_headers`], which [`expand`] turns into
//! the `include_str!` items that make Cargo rebuild when one changes.
//!
//! # Example
//!
//! ```
//! use std::str::FromStr;
//! use cinrs_core::{expand, Options, Standard};
//!
//! let input = proc_macro2::TokenStream::from_str("int add(int a, int b) { return a + b; }")
//!     .unwrap();
//! let out = expand(input, &Options::new(Standard::C99));
//! assert!(out.to_string().contains("extern \"C\" fn add"));
//! ```

#![warn(missing_docs)]

pub mod ast;
pub mod capture;
pub mod cfg;
pub mod codegen;
pub mod diag;
pub mod dump;
pub mod include;
pub mod ir;
pub mod lex;
pub mod parse;
pub mod pp;
pub mod sema;
pub mod target;

use std::path::{Path, PathBuf};

use proc_macro2::{Ident, Literal, Span, TokenStream, TokenTree};
use quote::quote;

pub use ast::TranslationUnit;
pub use capture::{FileId, InputMode, Pos, Source, SourceMap, SourceRange, Subspan};
pub use diag::{Diagnostic, Diagnostics, Level};
pub use ir::{Program, Ty};
pub use pp::Token;
pub use target::TargetModel;

/// Which revision of the C standard to accept.
///
/// The variants are ordered, so a feature is gated by comparing: a construct
/// C11 introduced is accepted when `standard >= Standard::C11`. C17 is C11
/// with a different `__STDC_VERSION__` and adds nothing else.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum Standard {
    /// ISO/IEC 9899:1999.
    #[default]
    C99,
    /// ISO/IEC 9899:2011.
    C11,
    /// ISO/IEC 9899:2018.
    C17,
    /// ISO/IEC 9899:2024.
    C23,
}

impl Standard {
    /// The name used in diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Standard::C99 => "C99",
            Standard::C11 => "C11",
            Standard::C17 => "C17",
            Standard::C23 => "C23",
        }
    }

    /// The macro that selects this standard, as a diagnostic names it.
    pub fn macro_name(self) -> &'static str {
        match self {
            Standard::C99 => "c99!",
            Standard::C11 => "c11!",
            Standard::C17 => "c17!",
            Standard::C23 => "c23!",
        }
    }

    /// The message a feature from a newer revision gets in this one.
    ///
    /// `what` is the subject, already quoted where it names a token:
    /// `standard.requires("'_Static_assert'", Standard::C11)` reads
    /// `'_Static_assert' requires C11 or later (this block is c99!)`. Saying
    /// which macro the block is written with is the point — the fix is to
    /// change the macro, and nothing in the C says which one it is.
    pub fn requires(self, what: &str, needed: Standard) -> String {
        format!(
            "{what} requires {} or later (this block is {})",
            needed.as_str(),
            self.macro_name()
        )
    }
}

/// Whether the compiler supports C-variadic definitions and
/// [`core::ffi::VaList`], which Rust stabilised in 1.99.
///
/// This crate is compiled by the very toolchain that will compile the code it
/// generates, so the answer is exact rather than a guess: a `c99!` invocation
/// that defines a variadic function, or that declares a `va_list` object, is
/// diagnosed as needing a newer Rust instead of producing an expansion the
/// compiler rejects with `E0658`.
#[rustversion::since(1.99)]
pub const C_VARIADIC_SUPPORTED: bool = true;
/// Whether the compiler supports C-variadic definitions and
/// [`core::ffi::VaList`], which Rust stabilised in 1.99.
#[rustversion::before(1.99)]
pub const C_VARIADIC_SUPPORTED: bool = false;

/// Knobs for one macro expansion.
///
/// Not [`Copy`], because [`Options::include_paths`] owns its list; the front
/// end clones it once per invocation.
#[derive(Clone, Debug)]
pub struct Options {
    /// Which standard to accept.
    pub standard: Standard,
    /// Accept `$` in identifiers, like GCC's `-fdollars-in-identifiers`.
    pub dollar_in_identifiers: bool,
    /// Directories `#include` searches, after the ones the unit itself names
    /// with `#pragma cinrs include_path` and before the bundled headers.
    ///
    /// This is the programmatic way in; see [`crate::include`] for the whole
    /// search order and for the `CINRS_INCLUDE_PATH` environment variable,
    /// which comes last.
    pub include_paths: Vec<PathBuf>,
    /// The data model the generated code is compiled for.
    ///
    /// Defaults to the host's; see [`TargetModel`] for what that means when
    /// cross-compiling.
    pub target: TargetModel,
    /// Whether `va_list` and variadic *definitions* may be generated.
    ///
    /// Defaults to [`C_VARIADIC_SUPPORTED`], which is exactly what the
    /// compiling toolchain can do; a test that wants to see the diagnostics an
    /// older toolchain produces — or the code a newer one would generate — can
    /// set it either way.
    pub c_variadic: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self::new(Standard::default())
    }
}

impl Options {
    /// Default options for `standard`.
    pub fn new(standard: Standard) -> Self {
        Self {
            standard,
            dollar_in_identifiers: false,
            include_paths: Vec::new(),
            target: TargetModel::host(),
            c_variadic: C_VARIADIC_SUPPORTED,
        }
    }
}

/// The result of running the front end over one macro invocation.
pub struct Analysis {
    /// The captured C source and its map back to Rust spans, headers and all.
    pub source: Source,
    /// The preprocessed C token list, ending with [`lex::TokenKind::Eof`].
    pub tokens: Vec<Token>,
    /// The parsed translation unit.
    pub unit: TranslationUnit,
    /// Everything that went wrong.
    pub diagnostics: Diagnostics,
    /// The macro invocations the preprocessor replaced, which later passes'
    /// diagnostics are annotated from.
    pub expansions: pp::Expansions,
    /// The absolute paths of the user headers that were read, which the
    /// expansion mentions so that Cargo rebuilds when one changes.
    pub user_headers: Vec<PathBuf>,
    /// The libraries `#pragma cinrs link` asked the `extern` block to be
    /// linked against.
    pub link_libraries: Vec<String>,
    /// Whether `#pragma cinrs export` asked for real C symbols.
    pub export: bool,
    /// The name `#pragma cinrs module` gave the generated module, if any.
    pub module: Option<String>,
}

/// Stack size for the thread the recursive passes run on.
///
/// Recursive descent turns nesting in the input into stack frames, and an
/// unoptimised build of a procedural macro — which is what a `cargo build`
/// uses — spends tens of kilobytes on each one. `rustc` runs macro expansion
/// on an 8 MiB stack, which combined with the parser's own recursion limit
/// would be uncomfortably tight, and a stack overflow inside a procedural
/// macro aborts the compiler with no useful message at all. Reserving address
/// space is free until it is touched, so the deep passes get a roomy thread of
/// their own and the recursion limit stays the only way to run out.
const WORKER_STACK_SIZE: usize = 64 << 20;

/// Runs `f` on a thread with [`WORKER_STACK_SIZE`] of stack.
///
/// The argument travels through a cell rather than being captured directly so
/// that it can be recovered and the work done in place if the thread cannot be
/// created at all.
fn on_large_stack<A, T>(arg: A, f: fn(A) -> T) -> T
where
    A: Send + 'static,
    T: Send + 'static,
{
    use std::sync::{Arc, Mutex};

    let cell = Arc::new(Mutex::new(Some(arg)));
    let worker = Arc::clone(&cell);
    let spawned = std::thread::Builder::new()
        .name("cinrs-worker".to_owned())
        .stack_size(WORKER_STACK_SIZE)
        .spawn(move || {
            let arg = worker
                .lock()
                .expect("the argument cell is never poisoned before use")
                .take()
                .expect("the argument is taken exactly once");
            f(arg)
        });
    match spawned {
        Ok(handle) => match handle.join() {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        },
        Err(_) => {
            // The thread could not be created; run here and rely on the
            // recursion limit alone.
            let arg = cell
                .lock()
                .expect("the thread never started, so nothing poisoned the cell")
                .take()
                .expect("the argument is still there");
            f(arg)
        }
    }
}

/// Everything the lexer, preprocessor and parser need, in one `Send` bundle.
struct FrontEndInput {
    /// The captured C source text.
    ctx: pp::Context,
    /// The range the whole translation unit covers.
    unit_range: SourceRange,
    options: Options,
}

/// The lexer's, preprocessor's and parser's results.
struct FrontEndOutput {
    tokens: Vec<Token>,
    unit: TranslationUnit,
    diagnostics: Diagnostics,
    expansions: pp::Expansions,
    included: Vec<pp::IncludedFile>,
    user_headers: Vec<PathBuf>,
    link_libraries: Vec<String>,
    export: bool,
    module: Option<String>,
}

/// Lexes, preprocesses and parses one translation unit.
fn front_end(input: FrontEndInput) -> FrontEndOutput {
    let FrontEndInput {
        ctx,
        unit_range,
        options,
    } = input;
    let mut diagnostics = Diagnostics::new();
    let raw = lex::lex_text(&ctx.text, ctx.base, &(&options).into());
    let pp::Preprocessed {
        tokens,
        expansions,
        included,
        user_headers,
        link_libraries,
        export,
        module,
    } = pp::preprocess(&raw, &ctx, &options, &mut diagnostics);
    let unit = parse::parse(&tokens, unit_range, &options, &mut diagnostics);
    // Annotate here rather than at the end: every diagnostic is annotated
    // exactly once, right after the pass that produced it.
    expansions.annotate(&mut diagnostics);
    FrontEndOutput {
        tokens,
        unit,
        diagnostics,
        expansions,
        included,
        user_headers,
        link_libraries,
        export,
        module,
    }
}

/// Runs capture, lexing, preprocessing and parsing over `input`.
///
/// This is the entry point tests use when they want to look at the
/// intermediate results; [`expand`] is the one the macro calls, and it adds
/// semantic analysis and code generation on top.
pub fn analyze(input: TokenStream, options: &Options) -> Analysis {
    analyze_with(input, options, None)
}

/// Runs capture, lexing, preprocessing and parsing over `input`, with a hook
/// that can point inside a string literal.
///
/// See [`Subspan`]; [`analyze`] is this with no hook.
pub fn analyze_with(input: TokenStream, options: &Options, subspan: Option<Subspan>) -> Analysis {
    let mut diagnostics = Diagnostics::new();
    // Capture must stay on this thread: it handles `proc_macro2::Span`s, which
    // are not `Send`. Everything after it works on plain byte offsets.
    let mut source = capture::capture_with(input, &mut diagnostics, subspan);
    let file = source.map.file(source.root);
    let arg = FrontEndInput {
        ctx: pp::Context {
            text: file.text().to_owned(),
            base: file.base(),
            file_name: file.rust_path().unwrap_or(pp::DEFAULT_FILE_NAME).to_owned(),
            first_line: file.first_line(),
            dir: including_directory(file.rust_path()),
            next_base: source.map.next_base(),
        },
        unit_range: source.root_range(),
        options: options.clone(),
    };
    let out = on_large_stack(arg, front_end);
    // The preprocessor allocated the offsets; the map hands out the spans for
    // them. Adding the files in the order they were opened is what makes a
    // diagnostic inside a nested header point at the outermost `#include`,
    // since the file each directive is written in is already in the map.
    for header in &out.included {
        let span = source.map.span(header.directive);
        let id = source
            .map
            .add_included_file(header.name.clone(), header.text.clone(), span);
        debug_assert_eq!(
            source.map.file(id).base(),
            header.base,
            "the preprocessor and the source map disagree about where '{}' starts",
            header.name
        );
    }
    diagnostics.extend(out.diagnostics);

    Analysis {
        source,
        tokens: out.tokens,
        unit: out.unit,
        diagnostics,
        expansions: out.expansions,
        user_headers: out.user_headers,
        link_libraries: out.link_libraries,
        export: out.export,
        module: out.module,
    }
}

/// The directory an `#include "…"` in the macro's own text searches first.
///
/// `Span::local_file` gives the path of the `.rs` file exactly as `rustc` was
/// told it, which for a Cargo build is relative to the working directory
/// `rustc` runs in. Since this code *is* that `rustc` process, resolving it
/// against the working directory is a no-op — the relative path already works
/// — and taking its parent gives the directory the user is writing in, which
/// is where C looks for a quoted header. A `.rs` file at the root of the
/// working directory has `""` as its parent, which joins to a bare name and is
/// therefore still right.
fn including_directory(rust_path: Option<&str>) -> Option<PathBuf> {
    Some(Path::new(rust_path?).parent()?.to_path_buf())
}

/// Expands one `c99!`-style invocation.
///
/// The result is a private module holding the unit's items plus a glob
/// re-export of it, so that two invocations in one Rust module are two
/// namespaces and cannot collide; a unit that declares nothing expands to
/// nothing at all. `#pragma cinrs module "…"` names that module.
///
/// # Where each pass runs
///
/// Capture and code generation must run on the caller's thread, because both
/// handle `proc_macro2::Span`s and those are deliberately not `Send`.
/// Everything in between — the lexer, the parser and semantic analysis —
/// works on byte offsets alone and runs on a thread with a large stack, where
/// the parser's recursion limit is the only thing that can stop it.
///
/// That leaves code generation recursing on the caller's stack, bounded by the
/// same limit: `parse::MAX_RECURSION_DEPTH` nested constructs. Code generation
/// frames are small (it allocates token streams rather than analysing
/// anything), and a debug build of the whole of capture plus code generation
/// for an expression nested to that limit fits inside a small fraction of a
/// megabyte — see the `codegen_of_deeply_nested_input_fits_in_a_small_stack`
/// test, which runs the entire expansion on a deliberately tiny thread.
///
/// # Errors
///
/// On failure the expansion holds a `compile_error!` per diagnostic *and* a
/// stub definition for every function whose signature was well formed, so that
/// Rust code calling those functions does not add a second layer of errors on
/// top of the real one.
pub fn expand(input: TokenStream, options: &Options) -> TokenStream {
    expand_with(input, options, None)
}

/// Expands one `c99!`-style invocation, with a hook that can point inside a
/// string literal.
///
/// See [`Subspan`]; [`expand`] is this with no hook.
pub fn expand_with(input: TokenStream, options: &Options, subspan: Option<Subspan>) -> TokenStream {
    let Analysis {
        source,
        unit,
        mut diagnostics,
        expansions,
        user_headers,
        link_libraries,
        export,
        module,
        ..
    } = analyze_with(input, options, subspan);

    let unit_id = source.unit_id();
    let (mut program, mut sema_diagnostics) = on_large_stack(
        (unit, options.clone(), unit_id),
        |(unit, options, unit_id)| sema::analyze(&unit, &options, unit_id),
    );
    expansions.annotate(&mut sema_diagnostics);
    diagnostics.extend(sema_diagnostics);
    program.link_libraries = link_libraries;
    program.export = export;

    // Emitted whether or not the unit compiled: a header that is being fixed
    // is exactly the one whose next edit has to trigger a rebuild.
    let mut out = rebuild_tracking(&user_headers);
    if diagnostics.has_errors() {
        out.extend(diagnostics.to_token_stream(&source.map));
        out.extend(codegen::generate_stubs(&program, &source.map, options));
    } else {
        out.extend(codegen::generate(&program, &source.map, options));
    }
    in_module(out, module.as_deref(), unit_id)
}

/// Wraps an expansion in a module of its own, re-exported by a glob.
///
/// A translation unit is a namespace, and two of them written in one Rust
/// module are two namespaces: both may `#include "point.h"`, and each has to
/// generate the `struct Point` its own code refers to. Two `struct Point`
/// items side by side are `E0428`; two modules each holding one, glob
/// re-exported, are not — a glob re-export only conflicts when a name it
/// exports is *used* ambiguously, and only from Rust, which is exactly the
/// case where the user has to say which one they mean. `#pragma cinrs module`
/// gives the module a name for them to say it with.
///
/// The module is what makes C's own hygiene work too: a `static` function is
/// private to it, as C says it is, while everything with external linkage is
/// `pub` and glob re-exported into the module the invocation is written in, so
/// that Rust calls it by the name its author gave it.
///
/// There is no `use super::*`: C code never refers to a Rust item.
fn in_module(items: TokenStream, name: Option<&str>, unit_id: u64) -> TokenStream {
    if items.is_empty() {
        // An empty translation unit expands to nothing at all, rather than to
        // an empty module and a glob re-export of it.
        return items;
    }
    let span = Span::call_site();
    // The name was checked when the pragma was read; checking it again is what
    // keeps a mistake there from becoming a panic inside a procedural macro.
    let (vis, ident) = match name.filter(|name| codegen::is_module_name(name)) {
        Some(name) => (quote! { pub }, Ident::new(name, span)),
        None => (
            TokenStream::new(),
            Ident::new(&format!("__cinrs_unit_{:08x}", unit_id as u32), span),
        ),
    };
    // `ambiguous_glob_reexports` is what a name two units both export trips,
    // and it is not a problem until Rust code uses that name — which is an
    // error of its own, with a message that says what to do. `unknown_lints`
    // comes first so that an older compiler may not have heard of it.
    quote! {
        #vis mod #ident {
            #items
        }
        #[allow(unknown_lints, ambiguous_glob_reexports, unused_imports)]
        pub use #ident::*;
    }
}

/// `const _: &str = ::core::include_str!("…");` for every user header read.
///
/// A procedural macro that reads a file has to tell the build system so, or
/// editing that file will not rebuild anything that included it. `include_str!`
/// is how a stable macro says it: `rustc` records the file as a dependency of
/// the crate, and Cargo re-runs the compilation when its timestamp moves. The
/// path is absolute so that it resolves the same from whichever module the
/// invocation is written in, and the item is anonymous (`const _`) so that any
/// number of them can coexist.
///
/// Bundled headers are left out: they cannot change without the crate that
/// carries them changing, which Cargo already knows about.
fn rebuild_tracking(headers: &[PathBuf]) -> TokenStream {
    let span = Span::call_site();
    let mut out = TokenStream::new();
    for header in headers {
        let mut literal = Literal::string(&header.to_string_lossy());
        literal.set_span(span);
        let literal = TokenTree::Literal(literal);
        out.extend(quote! { const _: &str = ::core::include_str!(#literal); });
    }
    out
}
