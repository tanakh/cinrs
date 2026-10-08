//! The `cinrs` C front end.
//!
//! This crate holds everything the `c89!`, `c99!`, `c11!`, `c17!` and `c23!`
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
//! A function that jumps takes two more steps on the way: sema hands its body
//! to [`cfg`](mod@cfg), which turns it into basic blocks, and
//! [`reloop`](mod@reloop) reads those back into loops, `if`s and `match`es for
//! codegen to emit. Most `goto`s take neither — [`regions`](mod@regions) is
//! the line between an outward jump, which stays a labelled block or a
//! labelled loop over the statements themselves, and one that does not fit.
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
//! from — the renamed `extern` *objects*, the mangled function-local
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
//! the `include_str!` items that make Cargo rebuild when one changes. C23's
//! `#embed` takes the same route with [`Analysis::embedded_files`] and
//! `include_bytes!`, since a resource is bytes rather than text.
//!
//! # Where `include_c99!` fits
//!
//! [`expand_include`] is [`expand`] with a `.c` file in place of the token
//! stream: the file becomes the map's root — see
//! [`capture::capture_c_file`] — and every pass after that is the same one.
//! Since nothing in the `.rs` file corresponds to a position in the `.c`, that
//! root resolves every range to the span of the macro invocation and carries
//! its own `path:line:column` into the message, which is exactly what a header
//! already does.
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
pub mod clock;
pub mod codegen;
pub mod complex;
pub mod diag;
pub mod dump;
pub mod gnu;
pub mod include;
pub mod ir;
pub mod lex;
mod locate;
pub mod parse;
pub mod pp;
pub mod regions;
pub mod reloop;
pub mod sema;
pub mod target;
pub mod x86;

use std::path::{Path, PathBuf};

use proc_macro2::{Ident, Literal, Span, TokenStream, TokenTree};
use quote::quote;

pub use ast::TranslationUnit;
pub use capture::{FileId, InputMode, Origin, Pos, Source, SourceMap, SourceRange, Subspan};
pub use diag::{Diagnostic, Diagnostics, Level};
pub use ir::{Program, Ty};
pub use pp::Token;
pub use target::{Arch, Env, Os, TargetModel, TargetSource, UnknownTarget};

/// The environment variable that names the target the expansion is for.
///
/// A procedural macro cannot ask `rustc` what it is compiling for, so the
/// crate being built says it, from its own build script:
///
/// ```text
/// println!("cargo:rustc-env=CINRS_TARGET={}", std::env::var("TARGET").unwrap());
/// ```
///
/// `cargo:rustc-env` reaches the very `rustc` process that runs the macro, and
/// Cargo makes the value part of the crate's fingerprint, so changing the
/// `--target` rebuilds. See [`target`] for the whole order of precedence.
pub const TARGET_ENV_VAR: &str = "CINRS_TARGET";

/// Which revision of the C standard to accept.
///
/// The variants are ordered, so a feature is gated by comparing: a construct
/// C11 introduced is accepted when `standard >= Standard::C11`. C17 is C11
/// with a different `__STDC_VERSION__` and adds nothing else.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum Standard {
    /// ISO/IEC 9899:1990, the standard everyone still calls C89.
    ///
    /// C90 is the ISO republication of ANSI X3.159-1989 with no technical
    /// change, so `c89!` and `c90!` are one entry point under two names.
    C89,
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
            Standard::C89 => "C89",
            Standard::C99 => "C99",
            Standard::C11 => "C11",
            Standard::C17 => "C17",
            Standard::C23 => "C23",
        }
    }

    /// The macro that selects this standard, as a diagnostic names it.
    pub fn macro_name(self) -> &'static str {
        self.macro_name_in(Dialect::Iso)
    }

    /// The macro that selects this standard in `dialect`.
    pub fn macro_name_in(self, dialect: Dialect) -> &'static str {
        match (dialect, self) {
            (Dialect::Iso, Standard::C89) => "c89!",
            (Dialect::Gnu, Standard::C89) => "gnu89!",
            (Dialect::Iso, Standard::C99) => "c99!",
            (Dialect::Iso, Standard::C11) => "c11!",
            (Dialect::Iso, Standard::C17) => "c17!",
            (Dialect::Iso, Standard::C23) => "c23!",
            (Dialect::Gnu, Standard::C99) => "gnu99!",
            (Dialect::Gnu, Standard::C11) => "gnu11!",
            (Dialect::Gnu, Standard::C17) => "gnu17!",
            (Dialect::Gnu, Standard::C23) => "gnu23!",
        }
    }

    /// Every name the macro that selects this standard in `dialect` may be
    /// written with, without the `!`.
    ///
    /// Two of them only for C89, which has two names and one entry point; the
    /// point of the list is that [the search](capture::Origin) for an invocation
    /// in the crate's sources knows what to look for, and `c90! { … }` has to be
    /// found as readily as `c89! { … }`.
    pub fn macro_names_in(self, dialect: Dialect) -> &'static [&'static str] {
        match (dialect, self) {
            (Dialect::Iso, Standard::C89) => &["c89", "c90"],
            (Dialect::Gnu, Standard::C89) => &["gnu89"],
            (Dialect::Iso, Standard::C99) => &["c99"],
            (Dialect::Iso, Standard::C11) => &["c11"],
            (Dialect::Iso, Standard::C17) => &["c17"],
            (Dialect::Iso, Standard::C23) => &["c23"],
            (Dialect::Gnu, Standard::C99) => &["gnu99"],
            (Dialect::Gnu, Standard::C11) => &["gnu11"],
            (Dialect::Gnu, Standard::C17) => &["gnu17"],
            (Dialect::Gnu, Standard::C23) => &["gnu23"],
        }
    }

    /// The same for the `include_…!` family, which is one entry point per name
    /// of [`Standard::macro_names_in`].
    pub fn include_macro_names_in(self, dialect: Dialect) -> &'static [&'static str] {
        match (dialect, self) {
            (Dialect::Iso, Standard::C89) => &["include_c89", "include_c90"],
            (Dialect::Gnu, Standard::C89) => &["include_gnu89"],
            (Dialect::Iso, Standard::C99) => &["include_c99"],
            (Dialect::Iso, Standard::C11) => &["include_c11"],
            (Dialect::Iso, Standard::C17) => &["include_c17"],
            (Dialect::Iso, Standard::C23) => &["include_c23"],
            (Dialect::Gnu, Standard::C99) => &["include_gnu99"],
            (Dialect::Gnu, Standard::C11) => &["include_gnu11"],
            (Dialect::Gnu, Standard::C17) => &["include_gnu17"],
            (Dialect::Gnu, Standard::C23) => &["include_gnu23"],
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

/// Whether the GNU extensions that need a plain spelling are switched on.
///
/// GCC draws the same line between `-std=c99` and `-std=gnu99`: everything
/// spelled with a double underscore (`__typeof__`, `__attribute__`,
/// `__builtin_*`, `__extension__`) is available either way, because those names
/// are reserved and cannot collide with a user's own; only the plain spellings
/// — `typeof`, `asm` — need the GNU dialect, and only there is a feature of a
/// *newer* revision accepted without a diagnostic (GCC takes `_Static_assert`
/// in `gnu99`).
///
/// See [`doc/gnu-extensions.md`](https://github.com/tanakh/cinrs/blob/master/doc/gnu-extensions.md)
/// in the repository for the whole catalogue.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Dialect {
    /// Strict ISO C: `c89!`, `c99!`, `c11!`, `c17!`, `c23!`.
    #[default]
    Iso,
    /// ISO C plus the GNU extensions: `gnu89!`, `gnu99!`, `gnu11!`, `gnu17!`,
    /// `gnu23!`.
    Gnu,
}

impl Dialect {
    /// Whether this is a GNU dialect.
    pub fn is_gnu(self) -> bool {
        self == Dialect::Gnu
    }
}

/// Whether the complex types are available, which is this crate's `complex`
/// feature.
///
/// The feature exists because the generated code for a `_Complex` value names
/// a *runtime* type — `::cinrs::rt::Complex`, which is `num_complex::Complex`
/// — and a build that does not want the dependency says
/// `default-features = false` on the `cinrs` crate. It is only the default of
/// [`Options::complex`]; the front end itself is compiled either way.
pub const COMPLEX_SUPPORTED: bool = cfg!(feature = "complex");

/// The GCC release cinrs presents itself as — `__GNUC__`, `__GNUC_MINOR__`
/// and `__GNUC_PATCHLEVEL__` — and why it is that one is in the preprocessor,
/// where they are defined.
pub const GCC_VERSION: (u32, u32, u32) = (14, 2, 0);

/// What every diagnostic about a complex type says when
/// [`Options::complex`] is off.
///
/// One message, in one place, because the lexer (an imaginary constant), the
/// parser (`__real__`) and sema (the type itself) all have to give it.
pub const COMPLEX_UNSUPPORTED: &str = "complex types are not supported here: '_Complex' needs the 'complex' feature of the \
     cinrs crate, which is on by default and supplies the runtime the generated code links \
     against";

/// Knobs for one macro expansion.
///
/// Not [`Copy`], because [`Options::include_paths`] owns its list; the front
/// end clones it once per invocation.
#[derive(Clone, Debug)]
pub struct Options {
    /// Which standard to accept.
    pub standard: Standard,
    /// Whether the plain-spelled GNU extensions are switched on, and whether a
    /// feature of a newer revision is accepted silently. See [`Dialect`].
    pub dialect: Dialect,
    /// Accept `$` in identifiers, like GCC's `-fdollars-in-identifiers`.
    ///
    /// **On by default**, which is where GCC and Clang both keep it: WG14
    /// DR027 lets an implementation put characters outside the basic source
    /// character set into an identifier, GCC takes `$` unconditionally, and
    /// Clang takes it with a warning that only `-pedantic-errors` promotes.
    /// A `$` that reaches the generated Rust — which has no such spelling —
    /// is written `_dollar_` there, and the C name is what the symbol still
    /// links by.
    pub dollar_in_identifiers: bool,
    /// Directories `#include` searches, after the ones the unit itself names
    /// with `#pragma cinrs include_path` and before the bundled headers.
    ///
    /// This is the programmatic way in; see [`crate::include`] for the whole
    /// search order and for the `CINRS_INCLUDE_PATH` environment variable,
    /// which comes last.
    pub include_paths: Vec<PathBuf>,
    /// Whether `#include` also searches the platform's own directories, and
    /// where in the order they go.
    ///
    /// **Off by default**, which is what keeps a unit self-contained and
    /// portable across [target models](target). [`analyze`] replaces this with
    /// what [`include::SYSTEM_ENV_VAR`] says when the caller left it at
    /// [`include::System::Off`], and the unit's own
    /// `#pragma cinrs system_include` replaces it again; see
    /// [`mod@include`] for the whole order and for the directories themselves.
    pub system_include: include::System,
    /// The data model the generated code is compiled for.
    ///
    /// Defaults to the host's, which [`analyze`] replaces with the one
    /// [`TARGET_ENV_VAR`] names and the unit's own
    /// `#pragma cinrs target` replaces again; see [`target`] for the order.
    /// Whatever it ends up being, the expansion states it — the
    /// `const _: () = { assert!(…); };` block every unit opens with is built
    /// from *this* model, so a unit expanded for one data model and compiled
    /// for another fails to compile.
    ///
    /// Setting it directly — [`Options::for_target`] is the tidy way — marks
    /// [`Options::target_source`] `Explicit`, and the environment variable is
    /// then left alone.
    pub target: TargetModel,
    /// Where [`Options::target`] came from, which the diagnostics name.
    pub target_source: TargetSource,
    /// Whether the complex types are available.
    ///
    /// Defaults to [`COMPLEX_SUPPORTED`], which is this crate's `complex`
    /// feature — the one the `cinrs` crate turns on to supply the `cinrs-rt`
    /// runtime the generated code names. With it off, `_Complex` is a
    /// diagnostic, `__STDC_NO_COMPLEX__` is predefined, and nothing generated
    /// needs more than `core`. The front end carries the support either way, so
    /// a test may set this in either direction.
    pub complex: bool,
    /// Whether every function and object with external linkage is a symbol
    /// under its C name, as `#pragma cinrs export` asks.
    ///
    /// **Off by default**: a block inside a Rust crate is usually called from
    /// Rust, and an unexported unit cannot collide with a C library at link
    /// time. The pragma turns it on for one unit, and the command-line driver
    /// turns it on for all of them, because to a C compiler every non-`static`
    /// definition is a symbol another object may name.
    pub export: bool,
    /// Macros defined and undefined on the command line, in the order given,
    /// before the first line of the unit — GCC's `-D` and `-U`.
    pub macros: Vec<CommandLineMacro>,
    /// Files read before the first line of the unit, in the order given, each
    /// as if `#include "file"` were written there — GCC's `-include`. As in
    /// GCC, one is looked for in the working directory first, and then where
    /// a quoted `#include` looks.
    pub includes: Vec<String>,
    /// Instruction sets enabled for the whole unit, in GCC's spelling and
    /// order — `avx2`, `no-avx512f` — as the command line's `-m` switches and
    /// `-march=` enable them. Only their feature macros come from here
    /// (`__AVX2__` and what it implies); the code is compiled for them by
    /// whoever runs `rustc`, with `-C target-feature`. A `#pragma GCC target`
    /// adds to them rather than replacing them, as in GCC. x86 only.
    pub target_features: Vec<String>,
    /// Whether every function the unit defines, declares or points at has the
    /// `extern "C-unwind"` ABI rather than `extern "C"`; see [`Unwind`].
    ///
    /// **[`Unwind::Auto`] by default**: `extern "C"`, except in a unit that
    /// uses `setjmp` or `longjmp` itself or says `#pragma cinrs unwind`. The
    /// command-line driver asks for [`Unwind::Always`], since any file may be
    /// between a `longjmp` and its `setjmp`, and for [`Unwind::Never`] under
    /// `-fno-cinrs-unwind`.
    pub unwind: Unwind,
    /// What a local declared without an initialiser starts out as; see
    /// [`AutoVarInit`]. **Zero** by default; `ccinrs
    /// -ftrivial-auto-var-init=uninitialized` and `#pragma cinrs
    /// auto_var_init uninitialized` (which wins for its unit) ask for
    /// uninitialised arrays.
    pub auto_var_init: AutoVarInit,
    /// Whether a function or object the unit declares *outside* the
    /// platform's headers and cinrs's bundled ones is taken to be compiled by
    /// cinrs too, with cinrs's ABI.
    ///
    /// It decides two things. A `long double` in such a function's prototype
    /// is cinrs's `long double` — a `double` — rather than the platform's x87
    /// or quad type, so a call to it is not refused (see
    /// [`crate::sema`]'s `long double` boundary). And a thread-local object
    /// with external linkage is reached through the accessor function a
    /// cinrs unit that defines one exports (see [`ir::Storage::ThreadLocal`]).
    ///
    /// **Off by default**: a `c99!` block may call a C library a C compiler
    /// built, through that library's own header, which is not one of the
    /// platform's. `ccinrs` turns it on, because what it links is what it
    /// compiled — every file of the program — and the platform's libraries,
    /// whose declarations are in the platform's headers.
    pub own_declarations_are_cinrs: bool,
    /// Whether a weak *definition* — `__attribute__((weak))` on a function or
    /// an object the unit defines — is a real one, which another definition
    /// of the symbol overrides at link time.
    ///
    /// Rust's `#[linkage]` is unstable, so it is made with the assembler: the
    /// body goes under a private name, a `global_asm!` makes the C symbol a
    /// weak alias of it (`.weak f` / `.set f, body`), and every use in the
    /// unit goes through the symbol, so that an override is what it reaches.
    /// That needs an ELF object, and an object that is not merged with
    /// another before it is assembled: under link-time optimisation a strong
    /// definition in another file can land in the same module as the alias,
    /// which the assembler refuses. And `rustc`'s own export list for a
    /// `cdylib` names `#[no_mangle]` items only, so a symbol only assembly
    /// defines would not be exported.
    ///
    /// **Off by default**, where the definition is an ordinary one with a
    /// warning: a `c99!` block cannot see whether its crate is built with LTO
    /// or as a `cdylib`. `ccinrs` turns it on for an ELF target without
    /// `-flto`, where it writes the export list itself.
    pub weak_definitions: bool,
    /// Whether an array *with external linkage* gets the x86-64 psABI's
    /// alignment too.
    ///
    /// The psABI (3.1.2) gives every array variable of 16 bytes or more —
    /// automatic, static or global — 16-byte alignment, and SIMD code relies
    /// on it: `_mm_load_ps(table)`. cinrs gives it to every such array of the
    /// unit on an x86-64 System V target, through the wrapper an `_Alignas`
    /// object is generated in. The one exception is an array Rust code
    /// reads, a `pub static` of the unit, whose Rust type the wrapper would
    /// change from `[T; N]` to `__cinrs_align_16<[T; N]>`.
    ///
    /// **Off by default**, so a block's public arrays keep their Rust type;
    /// `ccinrs`, whose objects nothing in Rust reads, turns it on.
    pub abi_align_public_arrays: bool,
    /// Which front end the C came through, which is what a diagnostic names
    /// when it says how to choose another standard or dialect: `gnu11!`, or
    /// `-std=gnu11`. [`FrontEnd::Macros`] by default; `ccinrs` sets
    /// [`FrontEnd::CommandLine`].
    pub front_end: FrontEnd,
    /// Whether a strict [`Dialect::Iso`] unit takes GCC's leniencies — the
    /// constraint violations GCC only warns about, which a GNU dialect takes
    /// anyway: a stray `;` at file scope, an enumerator outside `int`,
    /// `sizeof (void)`, a cast to a union type and the rest of
    /// `doc/gnu-extensions.md`'s leniency table — and `#embed` before C23.
    /// What makes the dialect ISO stays: `asm` and `typeof` are not keywords,
    /// `__STRICT_ANSI__` is defined, trigraphs are read and the GNU-only
    /// library functions are not builtins.
    ///
    /// GCC's `-std=c11` differs from `-std=gnu11` in exactly those things and
    /// warns about the rest, which only `-pedantic-errors` makes errors.
    /// **Off by default**, so `c11!` keeps refusing them; `ccinrs` turns it
    /// on for every `-std=` unless `-pedantic-errors` is given.
    pub gnu_leniencies: bool,
    /// The moment of translation `__DATE__` and `__TIME__` say, and
    /// `__TIMESTAMP__` for text that is no file of its own. `None`, the
    /// default, reads the environment as GCC does: `SOURCE_DATE_EPOCH` in
    /// UTC when it is set, and the clock in local time otherwise; see
    /// [`clock`]. A test sets it rather than the process's environment.
    pub translation_time: Option<clock::LocalTime>,
}

/// What a local declared without an initialiser holds before the program
/// writes it — GCC's `-ftrivial-auto-var-init`.
///
/// C leaves it indeterminate. Rust may not read uninitialised memory at all,
/// so cinrs zero-fills every such local unless told otherwise, which is
/// GCC's own hardening option `-ftrivial-auto-var-init=zero`. For a scalar
/// the store is free — LLVM deletes it wherever the program writes the
/// variable first — but a large array is cleared on every entry to its
/// block, which is what makes libuv's event loop, whose `uv__io_poll` declares
/// 15 KB of `epoll_event`s, run at a fraction of GCC's speed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutoVarInit {
    /// Every local is zero-filled.
    #[default]
    Zero,
    /// A local **array** with no initialiser is left uninitialised: its
    /// storage is a `MaybeUninit`, reached only through a raw pointer to it,
    /// and reading an element nothing wrote is undefined behaviour — in the
    /// generated Rust as in C, where its value is indeterminate. Scalars,
    /// structures and unions are still zeroed; see `doc/translation.md`.
    Uninitialized,
}

/// Which ABI a unit's functions have: `extern "C"` or `extern "C-unwind"`.
///
/// A `longjmp` is a Rust unwind here (see the non-local jumps of
/// [`crate::sema`]), and an unwind that leaves an `extern "C"` function is
/// undefined behaviour, so every frame between a `longjmp` and its `setjmp`
/// has to be `C-unwind`. The calling convention is the same; what changes is
/// the *type* of a function pointer, which Rust code handing a callback to the
/// unit then has to match — and `nounwind` on every call, which costs 0.3 % of
/// the instructions SQLite's speedtest1 runs (see `doc/ccinrs.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Unwind {
    /// `extern "C"`, unless the unit calls `setjmp` or `longjmp` itself or
    /// says `#pragma cinrs unwind`. What a `c99!` block gets.
    #[default]
    Auto,
    /// `extern "C-unwind"` for every unit, whatever it uses: what `ccinrs`
    /// gives every file.
    Always,
    /// `extern "C"` for every unit, and `setjmp`, `longjmp` and `#pragma cinrs
    /// unwind` refused where they are written: `ccinrs -fno-cinrs-unwind`.
    Never,
}

impl Unwind {
    /// Whether `program` is generated with `extern "C-unwind"`.
    pub fn resolve(self, program: &Program) -> bool {
        match self {
            Unwind::Auto => !program.nonlocal_jumps.is_empty(),
            Unwind::Always => true,
            Unwind::Never => false,
        }
    }
}

/// One `-D` or `-U` of a command line; see [`Options::macros`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandLineMacro {
    /// `-D` with its operand as GCC takes it: `NAME` defines `NAME` as `1`,
    /// `NAME=VALUE` as `VALUE`, and `F(x)=VALUE` a function-like macro.
    Define(String),
    /// `-U NAME`.
    Undefine(String),
}

impl Default for Options {
    fn default() -> Self {
        Self::new(Standard::default())
    }
}

impl Options {
    /// Default options for `standard`, in the strict ISO dialect.
    pub fn new(standard: Standard) -> Self {
        Self::with_dialect(standard, Dialect::Iso)
    }

    /// Default options for `standard` in the GNU dialect — what `gnu99!` and
    /// friends use.
    pub fn gnu(standard: Standard) -> Self {
        Self::with_dialect(standard, Dialect::Gnu)
    }

    /// Default options for `standard` in `dialect`.
    pub fn with_dialect(standard: Standard, dialect: Dialect) -> Self {
        Self {
            standard,
            dialect,
            dollar_in_identifiers: true,
            include_paths: Vec::new(),
            system_include: include::System::Off,
            target: TargetModel::host(),
            target_source: TargetSource::Host,
            complex: COMPLEX_SUPPORTED,
            export: false,
            macros: Vec::new(),
            includes: Vec::new(),
            target_features: Vec::new(),
            unwind: Unwind::Auto,
            auto_var_init: AutoVarInit::Zero,
            own_declarations_are_cinrs: false,
            weak_definitions: false,
            abi_align_public_arrays: false,
            front_end: FrontEnd::Macros,
            gnu_leniencies: false,
            translation_time: None,
        }
    }

    /// These options with the complex types switched on or off; see
    /// [`Options::complex`].
    pub fn with_complex(mut self, complex: bool) -> Self {
        self.complex = complex;
        self
    }

    /// These options with `target` set, and marked as the caller's choice so
    /// that [`TARGET_ENV_VAR`] does not override it.
    ///
    /// What a test that wants the ILP32 or LLP64 rules uses; a unit still
    /// overrides it with `#pragma cinrs target`.
    pub fn for_target(mut self, target: TargetModel) -> Self {
        self.target = target;
        self.target_source = TargetSource::Explicit;
        self
    }

    /// The macro that selects this entry point, as a diagnostic names it.
    pub fn macro_name(&self) -> &'static str {
        self.standard.macro_name_in(self.dialect)
    }

    /// The [origin](Origin) a real expansion of this entry point has: the names
    /// the invocation may be written with, and the crate directory Cargo names.
    ///
    /// This is what the procedural macros hand to [`expand_with`], and the only
    /// thing that lets capture find the invocation in the crate's sources when
    /// the host reports no positions. [`expand`] and [`analyze`] deliberately do
    /// not use it; see [`Origin`].
    pub fn origin(&self) -> Origin {
        Origin::new(self.standard.macro_names_in(self.dialect))
    }

    /// The same for the `include_…!` entry point of this standard and dialect.
    pub fn include_origin(&self) -> Origin {
        Origin::new(self.standard.include_macro_names_in(self.dialect))
    }

    /// How a pass gates the features of a newer revision.
    pub fn gating(&self) -> Gating {
        Gating {
            standard: self.standard,
            dialect: self.dialect,
            front_end: self.front_end,
            leniencies: self.gnu_leniencies,
        }
    }
}

/// Which front end the C came through, which is what a diagnostic that says
/// how to choose another standard or dialect has to name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FrontEnd {
    /// The `c99!` family of macros: "write gnu11! for the same leniency".
    #[default]
    Macros,
    /// A command line, `ccinrs`'s: "compile with -std=gnu11".
    CommandLine,
}

/// The pair every pass needs to answer "may this block write that?".
///
/// A GNU dialect answers yes to everything a later revision added, exactly as
/// GCC's `-std=gnu99` does; the strict entry points keep the diagnostic that
/// names the macro to write instead.
#[derive(Clone, Copy, Debug, Default)]
pub struct Gating {
    /// The revision the block is written in.
    pub standard: Standard,
    /// Whether the GNU extensions are switched on.
    pub dialect: Dialect,
    /// How the diagnostics name a standard and a dialect.
    pub front_end: FrontEnd,
    /// Whether a strict dialect takes GCC's leniencies all the same; see
    /// [`Options::gnu_leniencies`].
    pub leniencies: bool,
}

impl Gating {
    /// Whether this unit takes the constraint violations GCC only warns
    /// about: in a GNU dialect, and in a strict one that
    /// [`Options::gnu_leniencies`] says behaves as GCC's `-std=c11` does.
    pub fn lenient(self) -> bool {
        self.dialect.is_gnu() || self.leniencies
    }

    /// The message a feature from a newer revision gets here, or `None` when it
    /// is accepted.
    pub fn requires(self, what: &str, needed: Standard) -> Option<String> {
        if self.dialect.is_gnu() || self.standard >= needed {
            return None;
        }
        Some(format!(
            "{what} requires {} or later ({})",
            needed.as_str(),
            self.here()
        ))
    }

    /// How the front end selects this standard in `dialect`: `gnu11!`, or
    /// `-std=gnu11` on a command line.
    pub fn spelled(self, dialect: Dialect) -> String {
        let name = self.standard.macro_name_in(dialect);
        match self.front_end {
            FrontEnd::Macros => name.to_owned(),
            FrontEnd::CommandLine => format!("-std={}", name.trim_end_matches('!')),
        }
    }

    /// What a diagnostic says the code is being compiled as: "this block is
    /// c99!", or "this file is compiled with -std=c99".
    pub fn here(self) -> String {
        match self.front_end {
            FrontEnd::Macros => format!("this block is {}", self.spelled(self.dialect)),
            FrontEnd::CommandLine => {
                format!("this file is compiled with {}", self.spelled(self.dialect))
            }
        }
    }

    /// The note on a GNU leniency a strict standard refuses, which says how to
    /// have it.
    pub fn leniency_note(self) -> String {
        let gnu = self.spelled(Dialect::Gnu);
        match self.front_end {
            FrontEnd::Macros => {
                format!("GCC accepts this with a warning; write {gnu} for the same leniency")
            }
            // A command line takes the leniency in every `-std=` unless it
            // says `-pedantic-errors`, which is GCC's way of refusing it too.
            FrontEnd::CommandLine => format!(
                "GCC accepts this with a warning, and so does ccinrs without -pedantic-errors \
                 or with {gnu}"
            ),
        }
    }

    /// Whether a declaration with no type specifier at all means `int`
    /// (C89 6.5.2).
    ///
    /// C99 removed implicit `int` (N635) and GCC diagnoses it in every later
    /// mode, `-std=gnu99` included, so this is one of the three places where
    /// `gnu89!` is *older* than `gnu99!` rather than a superset of it.
    pub fn implicit_int(self) -> bool {
        self.standard < Standard::C99
    }

    /// Whether a call to a function nobody declared declares `extern int f();`
    /// at file scope (C89 6.3.2.2).
    ///
    /// C99 removed the rule (N636); see [`Gating::implicit_int`] for why the
    /// GNU dialect does not bring it back.
    pub fn implicit_function_declarations(self) -> bool {
        self.standard < Standard::C99
    }

    /// Whether an old-style (K&R) function definition may be written.
    ///
    /// Obsolescent from C89 onwards and *removed* by C23 (N2432), so every
    /// entry point below `c23!` has it and the two C23 ones do not.
    pub fn old_style_definitions(self) -> bool {
        self.standard < Standard::C23
    }

    /// The gate message for `name`, if another entry point would have made it
    /// a keyword.
    ///
    /// `nullptr`, `bool` and the rest are ordinary identifiers before C23 — the
    /// bundled `<stdbool.h>` writes `#define bool _Bool` — so a `c11!` block
    /// that uses one gets told what it would have meant. A GNU dialect says
    /// nothing about those: GCC's `gnu11` has no `bool` keyword either, so
    /// "use of undeclared identifier" is the honest answer there.
    ///
    /// `typeof` and `asm` are the two the *dialect* decides, so their message
    /// names the entry point that has them.
    pub fn newer_keyword(self, name: &str) -> Option<String> {
        if self.dialect.is_gnu() {
            return None;
        }
        let gnu = self.spelled(Dialect::Gnu);
        let here = self.here();
        match name {
            "typeof" | "typeof_unqual" if self.standard < Standard::C23 => {
                return Some(format!(
                    "'{name}' requires a GNU dialect ({gnu}) or C23 or later \
                     ({here})"
                ));
            }
            "asm" => {
                return Some(format!(
                    "'{name}' requires a GNU dialect ({gnu}); the spelling '__asm__' is \
                     available everywhere ({here})"
                ));
            }
            _ => {}
        }
        let needed = crate::lex::Keyword::from_str(name, Standard::C23)?.since();
        self.requires(&format!("'{name}'"), needed)
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
    /// Every header the unit read, in the order it read them.
    pub headers: Vec<Header>,
    /// The macro invocations the preprocessor replaced, which later passes'
    /// diagnostics are annotated from.
    pub expansions: pp::Expansions,
    /// The absolute paths of the user headers that were read, which the
    /// expansion mentions so that Cargo rebuilds when one changes.
    pub user_headers: Vec<PathBuf>,
    /// The absolute paths of the resources `#embed` read, mentioned in the
    /// expansion for the same reason.
    pub embedded_files: Vec<PathBuf>,
    /// The libraries `#pragma cinrs link` asked the expansion to be linked
    /// against, each of which becomes an `extern` block of its own.
    pub link_libraries: Vec<String>,
    /// The functions `#pragma cinrs safe` asked to be generated without
    /// `unsafe`; see [`sema::check_safe`].
    pub safe_functions: Vec<pp::SafeName>,
    /// Whether `#pragma cinrs export` asked for real C symbols.
    pub export: bool,
    /// Whether `#pragma cinrs no_std` said the expansion goes into a
    /// `#![no_std]` crate, so that the storage a variable length array or
    /// `alloca` needs comes from `alloc` rather than from `std`.
    pub no_std: bool,
    /// The Rust path `#pragma cinrs crate` gave the `cinrs` facade crate, if
    /// any; see [`ir::DEFAULT_CRATE_PATH`].
    pub crate_path: Option<String>,
    /// The options the rest of the pipeline is to run with.
    ///
    /// These are the caller's, with the target model resolved: whatever
    /// [`TARGET_ENV_VAR`] and the unit's own `#pragma cinrs target` had to say
    /// is already in [`Options::target`], so sema and code generation must use
    /// *these* rather than the ones they were handed.
    pub options: Options,
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
    embedded_files: Vec<PathBuf>,
    link_libraries: Vec<String>,
    safe_functions: Vec<pp::SafeName>,
    export: bool,
    no_std: bool,
    crate_path: Option<String>,
    /// The options with the target model resolved; see [`Analysis::options`].
    options: Options,
}

/// Lexes and preprocesses one translation unit: the half of [`front_end`]
/// that [`preprocess_file`] stops after.
fn preprocess_unit(
    ctx: &mut pp::Context,
    options: &mut Options,
    diagnostics: &mut Diagnostics,
) -> pp::Preprocessed {
    let mut raw = lex::lex_file(&ctx.text, ctx.base, &(&*options).into());
    // `#pragma cinrs target` has to be answered before anything else looks at
    // the model: the predefined macros are built from it, so it cannot be a
    // pragma like the others, handled where it stands. The scan is lexical and
    // over the unit's own text only; see [`pp::scan_target_pragma`]. A pragma
    // that really did change the model means the text has to be lexed again,
    // because how wide `wchar_t` is decides what `L'…'` may hold.
    let (target_pragmas, relex) = pp::scan_target_pragma(&raw, options, diagnostics);
    ctx.target_pragmas = target_pragmas;
    if relex {
        raw = lex::lex_file(&ctx.text, ctx.base, &(&*options).into());
    }
    pp::preprocess(&raw, ctx, options, diagnostics)
}

/// Lexes, preprocesses and parses one translation unit.
fn front_end(input: FrontEndInput) -> FrontEndOutput {
    let FrontEndInput {
        mut ctx,
        unit_range,
        mut options,
    } = input;
    let mut diagnostics = Diagnostics::new();
    let pp::Preprocessed {
        tokens,
        expansions,
        included,
        user_headers,
        embedded_files,
        link_libraries,
        safe_functions,
        export,
        no_std,
        unwind,
        auto_var_init,
        crate_path,
        pack_events,
        target_events,
        macros: _,
    } = preprocess_unit(&mut ctx, &mut options, &mut diagnostics);
    // `#pragma cinrs unwind` is the unit's own request for what the option
    // asks for the whole program; see [`Options::unwind`]. Under
    // `Unwind::Never` the preprocessor has already refused it.
    if unwind && options.unwind == Unwind::Auto {
        options.unwind = Unwind::Always;
    }
    // `#pragma cinrs auto_var_init` is the unit's own answer, and wins over
    // the option for its unit; see [`Options::auto_var_init`].
    if let Some(mode) = auto_var_init {
        options.auto_var_init = mode;
    }
    let packing = pp::PackMap::new(pack_events);
    let targets = pp::TargetOptionMap::new(target_events);
    let mut unit = parse::parse(
        &tokens,
        unit_range,
        &packing,
        &targets,
        &options,
        &mut diagnostics,
    );
    // Where the C library's declarations are; see
    // [`ast::TranslationUnit::library_headers`].
    unit.library_headers = included
        .iter()
        .filter(|file| file.kind != include::HeaderKind::User)
        .map(|file| SourceRange {
            start: file.base,
            end: file.base + file.text.len() as Pos,
        })
        .collect();
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
        embedded_files,
        link_libraries,
        safe_functions,
        export,
        no_std,
        crate_path,
        options,
    }
}

/// Runs capture, lexing, preprocessing and parsing over `input`.
///
/// This is the entry point tests use when they want to look at the
/// intermediate results; [`expand`] is the one the macro calls, and it adds
/// semantic analysis and code generation on top.
pub fn analyze(input: TokenStream, options: &Options) -> Analysis {
    analyze_with(input, options, &Origin::unknown())
}

/// Runs capture, lexing, preprocessing and parsing over `input`, with
/// everything the host says about the invocation itself.
///
/// See [`Origin`]; [`analyze`] is this with an origin that says nothing.
pub fn analyze_with(input: TokenStream, options: &Options, origin: &Origin) -> Analysis {
    let mut diagnostics = Diagnostics::new();
    // Capture must stay on this thread: it handles `proc_macro2::Span`s, which
    // are not `Send`. Everything after it works on plain byte offsets.
    let source = capture::capture_with(input, &mut diagnostics, origin);
    analyze_source(source, options, diagnostics)
}

/// Runs the front end over a [`Source`] that has already been captured.
///
/// [`analyze_with`] is this with the capture in front of it; the other caller
/// is [`expand_include`], whose source is a `.c` file rather than a token
/// stream.
fn analyze_source(mut source: Source, options: &Options, mut diagnostics: Diagnostics) -> Analysis {
    // The environment is read here rather than in `Options::new`, so that the
    // diagnostic a bad `CINRS_TARGET` deserves has a range to sit on — the
    // whole invocation, there being nothing in the C to point at.
    let mut options = options.clone();
    apply_env_target(&mut options, source.root_range(), &mut diagnostics);
    apply_env_system_include(&mut options, source.root_range(), &mut diagnostics);
    let arg = FrontEndInput {
        ctx: unit_context(&source),
        unit_range: source.root_range(),
        options,
    };
    let out = on_large_stack(arg, front_end);
    let headers = add_headers(&mut source.map, &out.included);
    diagnostics.extend(out.diagnostics);

    Analysis {
        source,
        tokens: out.tokens,
        unit: out.unit,
        diagnostics,
        headers,
        expansions: out.expansions,
        user_headers: out.user_headers,
        embedded_files: out.embedded_files,
        link_libraries: out.link_libraries,
        safe_functions: out.safe_functions,
        export: out.export,
        no_std: out.no_std,
        crate_path: out.crate_path,
        options: out.options,
    }
}

/// What the preprocessor is told about the unit's own file.
fn unit_context(source: &Source) -> pp::Context {
    let file = source.map.file(source.root);
    pp::Context {
        text: file.text().to_owned(),
        base: file.base(),
        file_name: file.rust_path().unwrap_or(pp::DEFAULT_FILE_NAME).to_owned(),
        first_line: file.first_line(),
        dir: including_directory(file.rust_path()),
        next_base: source.map.next_base(),
        // Filled in by `preprocess_unit`, which is where the scan runs.
        target_pragmas: pp::TargetPragmas::default(),
    }
}

/// Adds the headers the preprocessor read to the source map, answering what
/// each one is.
///
/// The preprocessor allocated the offsets; the map hands out the spans for
/// them. Adding the files in the order they were opened is what makes a
/// diagnostic inside a nested header point at the outermost `#include`, since
/// the file each directive is written in is already in the map.
fn add_headers(map: &mut SourceMap, included: &[pp::IncludedFile]) -> Vec<Header> {
    let mut headers = Vec::with_capacity(included.len());
    for header in included {
        let span = map.span(header.directive);
        let included_from = map.file_of(header.directive.start);
        let file = map.add_included_file(header.name.clone(), header.text.clone(), span);
        debug_assert_eq!(
            map.file(file).base(),
            header.base,
            "the preprocessor and the source map disagree about where '{}' starts",
            header.name
        );
        headers.push(Header {
            name: header.name.clone(),
            kind: header.kind,
            file,
            included_from,
        });
    }
    headers
}

/// One header a unit read: what a dependency list (GCC's `-M`) is made of.
#[derive(Clone, Debug)]
pub struct Header {
    /// What diagnostics call it: the path it was found at, as the search
    /// spelled it, or `<cinrs>/stdio.h` for a bundled header.
    pub name: String,
    /// Whose header it is.
    pub kind: include::HeaderKind,
    /// Its file in the unit's [`SourceMap`].
    pub file: FileId,
    /// The file whose `#include` read it.
    pub included_from: FileId,
}

/// Resolves [`TARGET_ENV_VAR`] into `options`, reporting a triple that names
/// no machine this crate models.
///
/// Only when the caller left the model at the host's: an
/// [`Options::for_target`] is a deliberate choice, and a test that sets one
/// must not have the developer's own environment change the answer.
fn apply_env_target(options: &mut Options, range: SourceRange, diagnostics: &mut Diagnostics) {
    if options.target_source != TargetSource::Host {
        return;
    }
    let Ok(triple) = std::env::var(TARGET_ENV_VAR) else {
        return;
    };
    let triple = triple.trim().to_owned();
    if triple.is_empty() {
        return;
    }
    let source = TargetSource::Env(triple);
    match TargetModel::from_triple(source.triple().expect("Env carries its triple")) {
        Ok(model) => {
            options.target = model;
            options.target_source = source;
        }
        Err(unknown) => diagnostics.error(range, unknown.message(&source)),
    }
}

/// Resolves [`include::SYSTEM_ENV_VAR`] into `options`, reporting a value that
/// is neither a boolean nor `first`.
///
/// Only when the caller left the switch off, for [`apply_env_target`]'s
/// reason: an [`Options::system_include`] the caller set is a deliberate
/// choice, and a test that sets one must not have the developer's own
/// environment change the answer.
fn apply_env_system_include(
    options: &mut Options,
    range: SourceRange,
    diagnostics: &mut Diagnostics,
) {
    if options.system_include != include::System::Off {
        return;
    }
    let Ok(value) = std::env::var(include::SYSTEM_ENV_VAR) else {
        return;
    };
    match include::System::from_env_value(&value) {
        Some(mode) => options.system_include = mode,
        None => diagnostics.error(
            range,
            format!(
                "{}={value:?} is not one of '1', 'first' or '0'",
                include::SYSTEM_ENV_VAR
            ),
        ),
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

/// Expands one `include_c99!("path")` invocation: the same translation, over a
/// `.c` file rather than over C written inside the `.rs`.
///
/// The file is read and translated exactly as [string-literal
/// input](capture::InputMode::StringLiteral) would be — every C construct is
/// accepted, `#pragma cinrs …` inside it configures the unit, and its own
/// directory is what its `#include "…"` searches first, as a header's is.
///
/// # Where a relative path is resolved
///
/// Against the **directory of the `.rs` file the macro is written in**, which
/// is what `#include "…"` in a `c99!` block already does and what the author
/// is looking at. `Span::local_file` is how that directory is found; where the
/// compiler will not say (input built by another macro, some IDE contexts), the
/// invocation is looked for in the crate's own sources — see [`Origin`] — and
/// `CARGO_MANIFEST_DIR` stands in when even that finds nothing, so that a path
/// written relative to the package still resolves. An absolute path is used as
/// it stands.
///
/// # Diagnostics
///
/// There is no C in the `.rs` file, so there is no span to point into: every
/// diagnostic — this crate's and `rustc`'s about the generated code — lands on
/// the macro invocation, and a message of ours carries `path:line:column` in
/// front of it, exactly as one inside an `#include`d header does. The file is
/// named in the expansion with `include_str!` as well, so that editing it
/// rebuilds the crate.
pub fn expand_include(input: TokenStream, options: &Options) -> TokenStream {
    let macro_name = format!("include_{}", options.macro_name());
    let mut trees = input.into_iter();
    let (first, second) = (trees.next(), trees.next());
    let span = first.as_ref().map_or_else(Span::call_site, TokenTree::span);
    let name = match (&first, &second) {
        (Some(TokenTree::Literal(literal)), None) => capture::string_literal_value(literal),
        _ => None,
    };
    let Some(name) = name else {
        return diag::compile_error_at(
            span,
            &format!(
                "{macro_name} takes one string literal naming a C file, as in \
                 {macro_name}(\"vendor/parser.c\")"
            ),
        );
    };

    let path = include_path(&name, span, &first, &options.include_origin());
    let found = match include::read_source(&path) {
        Ok(found) => found,
        Err(include::Error::Unreadable { path, error }) => {
            return diag::compile_error_at(span, &format!("cannot read '{path}': {error}"));
        }
        Err(include::Error::NotFound { searched }) => {
            let looked = searched.join(", ");
            return diag::compile_error_at(
                span,
                &format!(
                    "{macro_name} cannot find '{name}': there is no file at {looked}. A relative \
                     path is resolved against the directory of the .rs file this macro is \
                     written in"
                ),
            );
        }
    };

    // The file is the unit's own text, so it is tracked like a header: editing
    // it has to rebuild the crate that names it.
    let tracked: Vec<PathBuf> = found.path.into_iter().collect();
    let source = capture::capture_c_file(found.name, found.text, span);
    let analysis = analyze_source(source, options, Diagnostics::new());
    generate_unit(analysis, &tracked)
}

/// Where `include_c99!("name")` looks for its file.
///
/// The directory of the `.rs` file the invocation is written in, which is what
/// `Span::local_file` reports and what a quoted `#include` in a `c99!` block
/// already searches first.
///
/// Where the compiler will not say where that is, the invocation is looked for
/// in the crate's own sources: an `include_…!` whose argument is this very
/// literal, in a directory that does hold the file it names — which is the test
/// that tells two invocations of the same name apart, and the only thing this
/// path can be checked against. Failing that `CARGO_MANIFEST_DIR` stands in, and
/// failing even that the path is used as written, against the working directory,
/// which is a unit test rather than a build.
fn include_path(name: &str, span: Span, input: &Option<TokenTree>, origin: &Origin) -> PathBuf {
    let path = Path::new(name);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    if let Some(dir) = span
        .local_file()
        .and_then(|rs| rs.parent().map(Path::to_path_buf))
    {
        return dir.join(path);
    }
    if let Some(tree) = input
        && let Some(dir) =
            capture::invocation_directory(tree, origin, |dir| dir.join(path).is_file())
    {
        return dir.join(path);
    }
    match std::env::var_os(include::MANIFEST_DIR_VAR) {
        Some(root) => Path::new(&root).join(path),
        None => path.to_path_buf(),
    }
}

/// Expands one `c99!`-style invocation.
///
/// The result is a private module holding the unit's items plus a glob
/// re-export of it, so that two invocations in one Rust module are two
/// namespaces and cannot collide; a unit that declares nothing expands to
/// nothing at all.
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
    expand_with(input, options, &Origin::unknown())
}

/// Expands one `c99!`-style invocation, with everything the host says about the
/// invocation itself.
///
/// This is what the procedural macros call, with [`Options::origin`]; see
/// [`Origin`] for what it adds and why [`expand`] — which is this with an origin
/// that says nothing — does not.
pub fn expand_with(input: TokenStream, options: &Options, origin: &Origin) -> TokenStream {
    generate_unit(analyze_with(input, options, origin), &[])
}

/// Semantic analysis and code generation over a finished [`Analysis`].
///
/// `extra_tracking` names files the expansion must depend on that the
/// preprocessor did not read itself — the `.c` file [`expand_include`] was
/// pointed at, which is the unit's own text rather than a header of it.
fn generate_unit(analysis: Analysis, extra_tracking: &[PathBuf]) -> TokenStream {
    // Emitted whether or not the unit compiled: a header that is being fixed
    // is exactly the one whose next edit has to trigger a rebuild.
    let mut tracked = extra_tracking.to_vec();
    tracked.extend(analysis.user_headers.iter().cloned());
    let mut out = rebuild_tracking(&tracked, &analysis.embedded_files);
    let Lowered {
        source,
        program,
        diagnostics,
        options,
    } = lower(analysis);
    if diagnostics.has_errors() {
        out.extend(diagnostics.to_token_stream(&source.map));
        out.extend(codegen::generate_stubs(&program, &source.map, &options));
    } else {
        out.extend(codegen::generate(&program, &source.map, &options));
    }
    in_module(out, source.unit_id())
}

/// One `.c` file translated outside a procedural macro: what the `ccinrs`
/// command-line driver does with each file it is given.
pub struct FileTranslation {
    /// The generated Rust — the unit's private module and its glob re-export,
    /// as [`expand`] makes them — or `None` when the unit has an error, which
    /// [`FileTranslation::diagnostics`] then holds.
    pub items: Option<TokenStream>,
    /// Every error and warning, in the positions [`FileTranslation::map`]
    /// resolves.
    pub diagnostics: Diagnostics,
    /// The unit's source map: the file itself and every header it read.
    pub map: SourceMap,
    /// Whether the items call the runtime, `::cinrs::rt` — which they do for
    /// the complex types, and for nothing else.
    pub uses_runtime: bool,
    /// The C symbols the items define — every function and object with
    /// external linkage, under [`Options::export`] — which is what a shared
    /// library made of the unit exports.
    pub symbols: Vec<String>,
    /// Every header the unit read, in the order it read them.
    pub headers: Vec<Header>,
    /// The absolute paths of the resources `#embed` read.
    pub embedded: Vec<PathBuf>,
    /// Whether the items make a real weak definition — `weak`, or `common`,
    /// under [`Options::weak_definitions`] — which is an assembler alias: a
    /// driver merging units for link-time optimisation keeps this one apart.
    pub weak_definitions: bool,
}

/// Translates the C file at `path`.
///
/// The file is read and analysed as [`expand_include`] reads one — its own
/// directory is where its `#include "…"` looks first, and diagnostics and
/// `__FILE__` name it by `path` as given — but nothing becomes a
/// `compile_error!`, and nothing is emitted to make a Cargo build depend on
/// the headers it read: the caller renders the diagnostics itself.
///
/// Every generated token carries the line of the C it came from, in its
/// span's `start().line` — see [`capture::capture_c_file_by_line`] — so a
/// caller that prints the Rust line for line can make `rustc`'s positions,
/// and a panic's, name the C.
///
/// # Errors
///
/// A file that cannot be read, with the reason.
pub fn translate_file(path: &Path, options: &Options) -> Result<FileTranslation, String> {
    let (name, text) = read_unit(path)?;
    Ok(translate_source(name, text, options))
}

/// The name and the text of the C file at `path`, which is not required to
/// be UTF-8; see [`lex::decode_source`].
fn read_unit(path: &Path) -> Result<(String, String), String> {
    let found = include::read_source(path).map_err(|error| match error {
        include::Error::Unreadable { path, error } => format!("cannot read '{path}': {error}"),
        include::Error::NotFound { .. } => format!("{}: no such file", path.display()),
    })?;
    Ok((found.name, found.text))
}

/// [`translate_file`] for C that is not in a file — standard input, say.
/// `name` is what the diagnostics and `__FILE__` call it, and its `#include
/// "…"` looks in the directory the name is in: the working directory, for
/// `<stdin>`. `text` is the source as [`lex::decode_source`] makes it.
pub fn translate_source(name: String, text: String, options: &Options) -> FileTranslation {
    let source = capture::capture_c_file_by_line(name, text);
    let mut analysis = analyze_source(source, options, Diagnostics::new());
    let headers = std::mem::take(&mut analysis.headers);
    let embedded = std::mem::take(&mut analysis.embedded_files);
    let Lowered {
        source,
        program,
        diagnostics,
        options,
    } = lower(analysis);
    let mut uses_runtime = false;
    let mut symbols = Vec::new();
    let items = (!diagnostics.has_errors()).then(|| {
        let unit = codegen::generate_unit(&program, &source.map, &options);
        uses_runtime = unit.uses_runtime;
        symbols = unit.symbols;
        in_module(unit.items, source.unit_id())
    });
    FileTranslation {
        items,
        diagnostics,
        map: source.map,
        uses_runtime,
        symbols,
        headers,
        embedded,
        weak_definitions: program.has_weak_definitions(),
    }
}

/// A C file through the preprocessor alone: what GCC's `-E` prints, and
/// what its `-M` lists.
pub struct FilePreprocessing {
    /// The tokens, ending with [`lex::TokenKind::Eof`]. Each one's range is
    /// where it was written, or the macro invocation it came out of.
    pub tokens: Vec<Token>,
    /// Every error and warning, in the positions [`FilePreprocessing::map`]
    /// resolves.
    pub diagnostics: Diagnostics,
    /// The unit's source map: the file itself and every header it read.
    pub map: SourceMap,
    /// Every header the unit read, in the order it read them.
    pub headers: Vec<Header>,
    /// The absolute paths of the resources `#embed` read.
    pub embedded: Vec<PathBuf>,
    /// The pragmas that change what the tokens after them mean, as
    /// `(token index, text after "#pragma ")`: `#pragma pack` and `#pragma GCC
    /// target`, each spelled as the state it left — `pack(4)`, `pack()` —
    /// rather than as written, since a `push` or a `pop` is only meaningful
    /// against the ones before it. The other pragmas did their work in the
    /// preprocessor, or are ones cinrs does nothing with.
    pub pragmas: Vec<(usize, String)>,
    /// The macros defined when the unit ended: what GCC's `-dM` prints.
    pub macros: pp::MacroTable,
}

/// Preprocesses the C file at `path`, as [`translate_file`] reads it, and
/// stops there.
///
/// # Errors
///
/// A file that cannot be read, with the reason.
pub fn preprocess_file(path: &Path, options: &Options) -> Result<FilePreprocessing, String> {
    let (name, text) = read_unit(path)?;
    Ok(preprocess_source(name, text, options))
}

/// [`preprocess_file`] for C that is not in a file; see [`translate_source`].
pub fn preprocess_source(name: String, text: String, options: &Options) -> FilePreprocessing {
    let mut source = capture::capture_c_file_by_line(name, text);
    let mut diagnostics = Diagnostics::new();
    let mut options = options.clone();
    apply_env_target(&mut options, source.root_range(), &mut diagnostics);
    apply_env_system_include(&mut options, source.root_range(), &mut diagnostics);
    let (out, pp_diagnostics) = on_large_stack(
        (unit_context(&source), options),
        |(mut ctx, mut options)| {
            let mut diagnostics = Diagnostics::new();
            let out = preprocess_unit(&mut ctx, &mut options, &mut diagnostics);
            (out, diagnostics)
        },
    );
    let headers = add_headers(&mut source.map, &out.included);
    diagnostics.extend(pp_diagnostics);
    out.expansions.annotate(&mut diagnostics);
    let mut pragmas = Vec::new();
    for (index, alignment) in out.pack_events {
        let text = match alignment {
            Some(n) => format!("pack({n})"),
            None => "pack()".to_owned(),
        };
        pragmas.push((index, text));
    }
    for (index, names) in out.target_events {
        pragmas.push((index, "GCC reset_options".to_owned()));
        if !names.is_empty() {
            let names: Vec<&str> = names.iter().map(|(name, _)| name.as_str()).collect();
            pragmas.push((index, format!("GCC target(\"{}\")", names.join(","))));
        }
    }
    // Stable: the two pragmas of one `target` event stay in their order.
    pragmas.sort_by_key(|(index, _)| *index);
    FilePreprocessing {
        tokens: out.tokens,
        diagnostics,
        map: source.map,
        headers,
        embedded: out.embedded_files,
        pragmas,
        macros: out.macros,
    }
}

/// A unit through semantic analysis: what code generation reads.
struct Lowered {
    source: Source,
    program: Program,
    /// Every diagnostic of every pass.
    diagnostics: Diagnostics,
    /// The options with the target model resolved; see [`Analysis::options`].
    options: Options,
}

/// Semantic analysis and the pragma checks over a finished [`Analysis`].
fn lower(analysis: Analysis) -> Lowered {
    let Analysis {
        source,
        unit,
        mut diagnostics,
        expansions,
        link_libraries,
        safe_functions,
        export,
        no_std,
        crate_path,
        // The target model the environment and the unit's own pragma settled
        // on; everything after the front end has to use these rather than the
        // options the caller handed in.
        options,
        ..
    } = analysis;

    let unit_id = source.unit_id();
    let (mut program, mut sema_diagnostics) = on_large_stack(
        (unit, options.clone(), unit_id),
        |(unit, options, unit_id)| sema::analyze(&unit, &options, unit_id),
    );
    expansions.annotate(&mut sema_diagnostics);
    diagnostics.extend(sema_diagnostics);
    program.link_libraries = link_libraries;
    program.export = export || options.export;
    program.no_std = no_std;
    program.unwind = options.unwind.resolve(&program);
    if let Some(path) = crate_path {
        program.crate_path = path;
    }
    // The pragmas are the preprocessor's, so the rules that depend on one can
    // only be checked now that the program and the pragmas are together.
    // `check_safe` also *applies* one, which is why it comes before code
    // generation reads `Function::safe`.
    let mut pragma_diagnostics = sema::check_pragmas(&program);
    pragma_diagnostics.extend(sema::check_safe(&mut program, &safe_functions));
    expansions.annotate(&mut pragma_diagnostics);
    diagnostics.extend(pragma_diagnostics);
    Lowered {
        source,
        program,
        diagnostics,
        options,
    }
}

/// Wraps an expansion in a private module of its own, re-exported by a glob.
///
/// A translation unit is a namespace, and two of them written in one Rust
/// module are two namespaces: both may `#include "point.h"`, and each has to
/// generate the `struct Point` its own code refers to. Two `struct Point`
/// items side by side are `E0428`; two modules each holding one, glob
/// re-exported, are not — a glob re-export only conflicts when a name it
/// exports is *used* ambiguously, and only from Rust, which is exactly the
/// case where the user has to say which one they mean. An ordinary Rust `mod`
/// around the invocation is what they say it with, and what gives the unit's
/// items a path of their own.
///
/// The module is what makes C's own hygiene work too: a `static` function is
/// private to it, as C says it is, while everything with external linkage is
/// `pub` and glob re-exported into the module the invocation is written in, so
/// that Rust calls it by the name its author gave it.
///
/// There is no `use super::*`: C code never refers to a Rust item.
///
/// # The lint exemptions
///
/// The module's head carries the one `#![allow(…)]` the whole expansion
/// needs. A faithful translation of C trips a great many of Rust's lints, and
/// not one of them says anything about the C the user wrote: a parameter the
/// function never reads, a parenthesis C needed and Rust does not, a name
/// that is not `snake_case`, two declarations of one symbol that do not
/// match, a comparison that is always true, a statement after a `return`, an
/// `extern` signature Rust calls improper, arithmetic that overflows in a
/// branch that never runs. Clippy's lints are in the list for the same
/// reason. `unknown_lints` comes first so that a compiler that has not heard
/// of one of the newer names — `invalid_runtime_symbol_definitions`, say —
/// does not warn about the list itself.
///
/// It is an *inner* attribute, which is why it has to come before anything
/// else in the body; everything the unit generates is inside this module, and
/// lint levels are inherited, so one attribute covers every item, however
/// deeply nested, whatever the crate root denies, and for clippy as much as
/// for `rustc`.
///
/// One attribute per unit rather than one per item is what makes it
/// affordable. The list is 585 bytes and `#include <zlib.h>` generates 419
/// items, so the same exemption repeated on each of them was 83% of that
/// expansion — 245 KB of 294 KB — parsed by `rustc` and by rust-analyzer on
/// every build.
fn in_module(items: TokenStream, unit_id: u64) -> TokenStream {
    if items.is_empty() {
        // An empty translation unit expands to nothing at all, rather than to
        // an empty module and a glob re-export of it.
        return items;
    }
    let span = Span::call_site();
    let ident = Ident::new(&format!("__cinrs_unit_{:08x}", unit_id as u32), span);
    quote! {
        mod #ident {
            #![allow(
                unknown_lints,
                arithmetic_overflow,
                clashing_extern_declarations,
                dead_code,
                improper_ctypes,
                improper_ctypes_definitions,
                invalid_runtime_symbol_definitions,
                non_camel_case_types,
                non_snake_case,
                non_upper_case_globals,
                overflowing_literals,
                static_mut_refs,
                suspicious_runtime_symbol_definitions,
                unconditional_panic,
                unpredictable_function_pointer_comparisons,
                unreachable_code,
                unreachable_patterns,
                unused_assignments,
                unused_braces,
                unused_comparisons,
                unused_labels,
                unused_mut,
                unused_parens,
                unused_unsafe,
                unused_variables,
                clippy::all
            )]

            #items
        }
        // The re-export needs one of its own, which the module's cannot
        // cover: `ambiguous_glob_reexports` is what a name two units both
        // export trips, and it is not a problem until Rust code uses that
        // name — which is an error of its own, with a message that says what
        // to do.
        #[allow(unknown_lints, ambiguous_glob_reexports, unused_imports)]
        pub use #ident::*;
    }
}

/// `const _: &str = ::core::include_str!("…");` for every user header read,
/// and `include_bytes!` for every resource `#embed` read.
///
/// A procedural macro that reads a file has to tell the build system so, or
/// editing that file will not rebuild anything that included it. `include_str!`
/// is how a stable macro says it: `rustc` records the file as a dependency of
/// the crate, and Cargo re-runs the compilation when its timestamp moves. The
/// path is absolute so that it resolves the same from whichever module the
/// invocation is written in, and the item is anonymous (`const _`) so that any
/// number of them can coexist. An embedded resource is not text, so it takes
/// the byte-string form of the same trick.
///
/// Bundled headers are left out: they cannot change without the crate that
/// carries them changing, which Cargo already knows about.
///
/// `str` and `u8` take the [`core::primitive`] path for the same reason every
/// primitive the code generator writes does: these items go into the unit's
/// own module, where `typedef unsigned char u8;` may have put an alias of that
/// name.
fn rebuild_tracking(headers: &[PathBuf], embedded: &[PathBuf]) -> TokenStream {
    let span = Span::call_site();
    let mut out = TokenStream::new();
    for header in headers {
        let mut literal = Literal::string(&header.to_string_lossy());
        literal.set_span(span);
        let literal = TokenTree::Literal(literal);
        out.extend(quote! {
            const _: &::core::primitive::str = ::core::include_str!(#literal);
        });
    }
    for resource in embedded {
        let mut literal = Literal::string(&resource.to_string_lossy());
        literal.set_span(span);
        let literal = TokenTree::Literal(literal);
        out.extend(quote! {
            const _: &[::core::primitive::u8] = ::core::include_bytes!(#literal);
        });
    }
    out
}
