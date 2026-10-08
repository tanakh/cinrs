//! The command line, read the way GCC reads it.
//!
//! GCC's options are not the shape a general-purpose argument parser expects:
//! `-std=c11` is one option, not `-s -t -d=c11`; `-Idir` and `-I dir` are the
//! same thing; `-Wl,-rpath,/x` carries a list; and a build system passes
//! dozens of `-f…` and `-W…` flags that tune a compiler `ccinrs` is not. So
//! this is a table of what is understood, read by hand, with three answers
//! for the rest:
//!
//! * a warning option (`-W…`) or a code-generation knob (`-f…`) that cannot
//!   change what the program means is accepted and ignored — silently when it
//!   is a common one, with a warning otherwise;
//! * one that would change what the program means — `-fshort-enums`,
//!   `-fpack-struct`, `-fopenmp` — is an error, because ignoring it would be a
//!   silent miscompile;
//! * anything else is an error naming the option, as GCC's own "unrecognized
//!   command-line option" is.

use std::path::PathBuf;

use cinrs_core::{AutoVarInit, CommandLineMacro, Dialect, Standard};

use crate::gcc_warnings;

/// What to stop after. The earlier stage wins when a command line names two,
/// as in GCC: `-E` over `-S` over `-c`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// `-E` (and `-M`, `-MM`): the preprocessed text, or the dependencies.
    Preprocess,
    /// `-fsyntax-only`: every C file read and checked, its diagnostics
    /// printed, and nothing written.
    SyntaxOnly,
    /// `-S`: one `.rs` file per C file — Rust being what `ccinrs` compiles C
    /// to, as assembly is what GCC does.
    Rust,
    /// `-c`: one object file per C file, and no link.
    Compile,
    /// Compile every C file and link the program: the default.
    Link,
}

/// `-M` and its relatives: a Makefile rule saying which files an object
/// depends on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Deps {
    /// `-M` or `-MM`: the rule is the output, in place of the preprocessed
    /// text, and nothing is compiled.
    pub only: bool,
    /// `-MD` or `-MMD`: the rule is written to a `.d` file beside what the
    /// command line makes.
    pub beside: bool,
    /// Whether the platform's headers are listed: `-M` and `-MD` list them,
    /// `-MM` and `-MMD` do not.
    pub system: bool,
    /// `-MF`: the file the rule goes to.
    pub file: Option<PathBuf>,
    /// `-MT` and `-MQ` (quoted for `make` already), in order; the object's
    /// name when there are none.
    pub targets: Vec<String>,
    /// `-MP`: an empty rule for every header, so that a header deleted does
    /// not stop `make`.
    pub phony: bool,
}

/// Which link-time optimisation `-flto` asks for: `rustc`'s `-C lto=fat`,
/// or with `-flto=thin`, Clang's spelling, `-C lto=thin`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lto {
    Fat,
    Thin,
}

/// Something the command line says about the link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkerArg {
    /// An option of the C driver's own: `-rdynamic`.
    Driver(String),
    /// One word for the linker itself: an item of `-Wl,a,b`, or what follows
    /// `-Xlinker`. libtool writes `-Wl,--version-script -Wl,file`, one word
    /// at a time, so the words are what a link reads, not the options.
    Linker(String),
}

/// What a command line asks that is not a compilation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Query {
    /// `--version`.
    Version,
    /// `-dumpversion`: the GCC release cinrs presents itself as.
    DumpVersion,
    /// `-dumpfullversion`.
    DumpFullVersion,
    /// `-dumpmachine`: the target triple.
    DumpMachine,
    /// `--help`.
    Help,
    /// `-print-search-dirs`, whose `libraries:` line libtool reads.
    SearchDirs,
    /// `-print-prog-name=…`, `-print-file-name=…`: GCC answers with the path
    /// of its own program or file of that name, or with the name itself when
    /// it has none — and `ccinrs` has none.
    Name(String),
    /// `-print-multi-os-directory`.
    MultiOsDirectory,
    /// `-print-multiarch`: Debian's name for the target's library directory.
    Multiarch,
    /// `-print-sysroot`, `-print-multi-directory`, `-print-multi-lib`,
    /// `-dumpspecs`: what GCC says when there is nothing to say, as a line.
    Literal(&'static str),
}

/// One file named on the command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// A C translation unit.
    C(PathBuf),
    /// Something the linker takes as it is: an object, an archive, a shared
    /// library.
    Linker(PathBuf),
}

/// Everything one command line asks for.
#[derive(Clone, Debug)]
pub struct Invocation {
    /// The files, in command-line order.
    pub inputs: Vec<Input>,
    /// `-o`.
    pub output: Option<PathBuf>,
    pub stage: Stage,
    /// `-std=`; `gnu17` by default, which is what the GCC 14 that cinrs
    /// presents itself as defaults to.
    pub standard: Standard,
    pub dialect: Dialect,
    /// `-I`, `-iquote`, `-isystem` and `-idirafter`, in order. cinrs has one
    /// search list rather than GCC's three, so they are searched in the order
    /// given, before the platform's directories.
    pub include_dirs: Vec<PathBuf>,
    /// Whether the platform's own headers are searched; `-nostdinc` turns it
    /// off.
    pub system_include: bool,
    /// `-D` and `-U`, in order.
    pub macros: Vec<CommandLineMacro>,
    /// `-include`, in order.
    pub includes: Vec<String>,
    /// `-dM`: under `-E`, the macros defined at the end instead of the text.
    pub dump_macros: bool,
    /// `rustc`'s `-C opt-level`: `0`, `1`, `2`, `3`, `s` or `z`.
    pub opt_level: &'static str,
    /// `rustc`'s `-C debuginfo`.
    pub debuginfo: u8,
    /// `-w` turns warnings off.
    pub warnings: bool,
    /// `-Werror`: every warning is an error, as in GCC — `#warning` included,
    /// which is what a `configure` script's test of `-Werror` writes.
    pub werror: bool,
    /// `-pedantic-errors` (or `-Werror=pedantic`): the constraint violations
    /// GCC only warns about are errors. Without it, every `-std=` takes them
    /// as GCC's does; see [`cinrs_core::Options::gnu_leniencies`].
    pub pedantic_errors: bool,
    /// `-fgnu89-inline` (`Some(true)`) or `-fno-gnu89-inline`: which rules
    /// decide whether an inline definition is an external one; see
    /// [`cinrs_core::Options::gnu89_inline`]. `None` is the `-std=`'s.
    pub gnu89_inline: Option<bool>,
    /// Whether the generated code keeps Rust's run-time checks — misaligned
    /// and null pointer dereferences, overlapping `memcpy`, and the checks
    /// cinrs itself writes in a build with debug assertions. **On by
    /// default**; `-fno-cinrs-checks` turns them off.
    pub checks: bool,
    /// Whether every function is `extern "C-unwind"`, which is what lets a
    /// `longjmp` — a Rust unwind — pass through it. **On by default**;
    /// `-fno-cinrs-unwind` makes them `extern "C"`, which saves 0.3 % of the
    /// instructions on SQLite, and refuses `setjmp` and `longjmp`.
    pub unwind: bool,
    /// What a local declared without an initialiser starts out as: GCC's
    /// `-ftrivial-auto-var-init=`. **Zero by default**, which is what Rust
    /// needs anyway; `=uninitialized` leaves local arrays uninitialised, which
    /// is GCC's own default and saves clearing a large buffer on every call.
    /// `=pattern` is taken as `=zero`, with a warning. See
    /// [`cinrs_core::AutoVarInit`].
    pub auto_var_init: AutoVarInit,
    /// `-fdollars-in-identifiers`, on by default as in GCC.
    pub dollars: bool,
    /// What `-fsigned-char` or `-funsigned-char` asked for, which the driver
    /// checks against the target: cinrs takes the platform's choice and
    /// cannot change it.
    pub char_signed: Option<bool>,
    /// The pointer width `-m32` or `-m64` asked for, checked the same way.
    pub pointer_bits: Option<u32>,
    /// `--target=<triple>`, Clang's spelling: what to compile for, when it is
    /// not the machine `rustc` runs on.
    pub target: Option<String>,
    /// `--sysroot=<dir>`: where the target's own headers are, as
    /// `<dir>/include` and `<dir>/usr/include`.
    pub sysroot: Option<PathBuf>,
    /// `-march=` (or `-mcpu=`): the processor to compile for, which is
    /// `rustc`'s `-C target-cpu` — `native` included.
    pub cpu: Option<String>,
    /// `-mavx2`, `-mno-avx512f` and the like, in order, in GCC's spelling
    /// (`avx2`, `no-avx512f`).
    pub features: Vec<String>,
    /// `-L`, in order.
    pub lib_dirs: Vec<PathBuf>,
    /// `-l`, in order (and `pthread` for `-pthread`).
    pub libs: Vec<String>,
    /// `-Wl,…`, `-Xlinker` and `-rdynamic`, in order.
    pub linker_args: Vec<LinkerArg>,
    /// `-fuse-ld=`: which linker the C compiler `rustc` links with is to run.
    pub fuse_ld: Option<String>,
    /// Which of `inputs` were named between `-Wl,--whole-archive` and
    /// `-Wl,--no-whole-archive`, as libtool links a convenience library into
    /// a shared one. `rustc` lays out the link line itself, so the two are
    /// not passed on but kept as which archives they surround.
    pub whole_archives: Vec<usize>,
    /// `-flto`: objects that carry what `rustc` needs to optimise them
    /// together, and a link that does.
    pub lto: Option<Lto>,
    /// `-s`.
    pub strip: bool,
    /// `-shared`: link a shared library rather than a program.
    pub shared: bool,
    /// `-static`: link the C library and everything else statically.
    pub static_link: bool,
    /// `-v`: print every command run.
    pub verbose: bool,
    /// `-save-temps`: keep the generated `.rs` files and the objects.
    pub save_temps: bool,
    /// `-P` turns off the line markers in `-E`'s output.
    pub line_markers: bool,
    /// `-M`, `-MD` and the rest.
    pub deps: Deps,
    /// `--version` and the like, answered instead of compiling.
    pub query: Option<Query>,
    /// Warnings about the command line itself, printed before anything runs.
    pub notes: Vec<String>,
}

impl Default for Invocation {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            output: None,
            stage: Stage::Link,
            standard: Standard::C17,
            dialect: Dialect::Gnu,
            include_dirs: Vec::new(),
            system_include: true,
            macros: Vec::new(),
            includes: Vec::new(),
            dump_macros: false,
            opt_level: "0",
            debuginfo: 0,
            warnings: true,
            werror: false,
            pedantic_errors: false,
            gnu89_inline: None,
            checks: true,
            unwind: true,
            auto_var_init: AutoVarInit::Zero,
            dollars: true,
            char_signed: None,
            pointer_bits: None,
            target: None,
            sysroot: None,
            cpu: None,
            features: Vec::new(),
            lib_dirs: Vec::new(),
            libs: Vec::new(),
            linker_args: Vec::new(),
            fuse_ld: None,
            whole_archives: Vec::new(),
            lto: None,
            strip: false,
            shared: false,
            static_link: false,
            verbose: false,
            save_temps: false,
            line_markers: true,
            deps: Deps::default(),
            query: None,
            notes: Vec::new(),
        }
    }
}

impl Invocation {
    /// Stops at `stage`, unless an earlier one is already asked for.
    fn stop_at(&mut self, stage: Stage) {
        self.stage = self.stage.min(stage);
    }
}

/// The options GCC has that this version of `ccinrs` does not, yet: an error
/// that says so rather than "unknown option".
const NOT_YET: &[&str] = &["-MG", "-dD", "-dN", "-imacros"];

/// `-f` options that cannot change what a program means here, accepted
/// without a word: the names themselves, or a prefix ending in `-` or `=`.
const QUIET_FLAGS: &[&str] = &[
    "PIC",
    "pic",
    "PIE",
    "pie",
    "no-PIC",
    "no-pic",
    "no-PIE",
    "no-pie",
    "strict-aliasing",
    "no-strict-aliasing",
    "wrapv",
    "no-wrapv",
    "common",
    "no-common",
    "visibility=",
    "stack-protector",
    "no-stack-protector",
    "function-sections",
    "no-function-sections",
    "data-sections",
    "no-data-sections",
    "omit-frame-pointer",
    "no-omit-frame-pointer",
    "diagnostics-color",
    "diagnostics-color=",
    "no-diagnostics-color",
    // Universal character names in identifiers, which cinrs takes anyway.
    "extended-identifiers",
    "no-extended-identifiers",
    "message-length=",
    "exceptions",
    "no-exceptions",
    "asynchronous-unwind-tables",
    "no-asynchronous-unwind-tables",
    "unwind-tables",
    "no-unwind-tables",
    "plt",
    "no-plt",
    "math-errno",
    "no-math-errno",
    "stack-clash-protection",
    "no-stack-clash-protection",
    "cf-protection",
    "semantic-interposition",
    "no-semantic-interposition",
    "fat-lto-objects",
    "no-fat-lto-objects",
    "use-linker-plugin",
    "lto-partition=",
    "ident",
    "no-ident",
    "strict-overflow",
    "no-strict-overflow",
    "delete-null-pointer-checks",
    "no-delete-null-pointer-checks",
    "builtin",
    "no-builtin",
    "inline",
    "no-inline",
    "inline-functions",
    "unroll-loops",
    "no-unroll-loops",
    "tree-vectorize",
    "no-tree-vectorize",
    "hosted",
    "freestanding",
    "jump-tables",
    "no-jump-tables",
    "merge-constants",
    "no-merge-constants",
    // Hardening a distribution's build asks for, which changes nothing a
    // program can see.
    "stack-protector-strong",
    "stack-protector-all",
    "stack-protector-explicit",
    "cf-protection=",
    "zero-call-used-regs=",
    "sanitize-recover",
    "no-sanitize-recover",
    "optimize-sibling-calls",
    "no-optimize-sibling-calls",
    "tls-model=",
    "strict-flex-arrays=",
    "debug-prefix-map=",
    "visibility-inlines-hidden",
    // Floating point kept strict, which `rustc` never does otherwise: no
    // contraction into fused multiply-adds, no fast-math assumptions, SSE's
    // precision and no other.
    "fp-contract=",
    "no-fast-math",
    "no-finite-math-only",
    "signed-zeros",
    "trapping-math",
    "no-trapping-math",
    "excess-precision=",
];

/// `-f` options that change what a program means in a way cinrs cannot
/// follow: an error, never a silent difference.
const MEANINGFUL_FLAGS: &[&str] = &[
    "short-enums",
    "short-wchar",
    "pack-struct",
    "openmp",
    "openacc",
    "ms-extensions",
    "single-precision-constant",
    "trapv",
    "no-signed-zeros",
    "zero-initialized-in-bss",
    "no-zero-initialized-in-bss",
    "sanitize=",
];

/// Whether `name` is one of `table`'s entries: the name itself, or one that
/// begins with an entry ending in `-` or `=`.
fn listed(table: &[&str], name: &str) -> bool {
    table.iter().any(|entry| {
        name == *entry
            || ((entry.ends_with('-') || entry.ends_with('=')) && name.starts_with(entry))
    })
}

/// Reads a command line, not counting the program name.
///
/// # Errors
///
/// The message for an option that is wrong, unknown, or not supported yet.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Invocation, String> {
    let mut inv = Invocation::default();
    let mut args = args.into_iter();
    // `-x c` makes every following file C whatever its name; `-x none` goes
    // back to judging by the extension.
    let mut force_c = false;
    // Standard input is C only under `-x c`, or when it is only preprocessed.
    let mut stdin = None;
    // The `-W` and `-f` options GCC would not take, which `-Werror` refuses.
    let mut unknown: Vec<String> = Vec::new();
    // Clang's `-Werror=unknown-warning-option` and `-Wno-unknown-warning-option`.
    let mut unknown_error = false;
    let mut unknown_quiet = false;
    // Between `--whole-archive` and `--no-whole-archive`.
    let mut whole = false;
    // `.h` files, which only `-E` and `-M` take.
    let mut headers: Vec<PathBuf> = Vec::new();
    while let Some(arg) = args.next() {
        // An option whose value may be attached (`-Idir`) or the next
        // argument (`-I dir`).
        let mut value = |name: &str| -> Result<Option<String>, String> {
            let Some(rest) = arg.strip_prefix(name) else {
                return Ok(None);
            };
            if !rest.is_empty() {
                return Ok(Some(rest.to_owned()));
            }
            args.next()
                .map(Some)
                .ok_or_else(|| format!("missing argument to '{name}'"))
        };
        if let Some(path) = value("-o")? {
            inv.output = Some(PathBuf::from(path));
            continue;
        }
        // `--target=T` or `--target T`, Clang's older `-target T`, and
        // `--sysroot` the same way.
        let mut long = None;
        for name in ["--target", "-target", "--sysroot"] {
            if let Some(v) = value(name)? {
                long = Some((
                    name,
                    v.strip_prefix('=').map_or_else(|| v.clone(), str::to_owned),
                ));
                break;
            }
        }
        match long {
            Some(("--sysroot", dir)) => {
                inv.sysroot = Some(PathBuf::from(dir));
                continue;
            }
            Some((_, triple)) => {
                inv.target = Some(triple);
                continue;
            }
            None => {}
        }
        if let Some(lang) = value("-x")? {
            force_c = match lang.as_str() {
                "c" => true,
                "none" => false,
                other => {
                    return Err(format!(
                        "language '{other}' is not supported; ccinrs compiles C"
                    ));
                }
            };
            continue;
        }
        if arg == "-Xlinker"
            && let Some(word) = value("-Xlinker")?
        {
            linker_word(&mut inv, &mut whole, &word);
            continue;
        }
        if NOT_YET.contains(&arg.as_str()) {
            return Err(format!("'{arg}' is not supported yet"));
        }
        if let Some(file) = value("-MF")? {
            inv.deps.file = Some(PathBuf::from(file));
            continue;
        }
        if let Some(target) = value("-MT")? {
            inv.deps.targets.push(target);
            continue;
        }
        if let Some(target) = value("-MQ")? {
            inv.deps.targets.push(make_quoted(&target));
            continue;
        }
        // `-isystem`, `-iquote` and `-idirafter` before `-I`, of which they
        // are not spellings.
        let mut include = None;
        for name in ["-isystem", "-iquote", "-idirafter", "-I"] {
            if let Some(dir) = value(name)? {
                include = Some(dir);
                break;
            }
        }
        if let Some(dir) = include {
            inv.include_dirs.push(PathBuf::from(dir));
            continue;
        }
        if let Some(file) = value("-include")? {
            inv.includes.push(file);
            continue;
        }
        if let Some(def) = value("-D")? {
            inv.macros.push(CommandLineMacro::Define(def));
            continue;
        }
        if let Some(name) = value("-U")? {
            inv.macros.push(CommandLineMacro::Undefine(name));
            continue;
        }
        if let Some(dir) = value("-L")? {
            inv.lib_dirs.push(PathBuf::from(dir));
            continue;
        }
        if let Some(lib) = value("-l")? {
            inv.libs.push(lib);
            continue;
        }
        match arg.as_str() {
            "-c" => inv.stop_at(Stage::Compile),
            "-S" => inv.stop_at(Stage::Rust),
            "-E" => inv.stop_at(Stage::Preprocess),
            "-P" => inv.line_markers = false,
            "-C" | "-CC" => inv.notes.push(format!(
                "ignoring '{arg}': the preprocessed output keeps no comments"
            )),
            // `-M` and `-MM` are `-E` with the dependencies for output.
            "-M" | "-MM" => {
                inv.stop_at(Stage::Preprocess);
                inv.deps.only = true;
                inv.deps.system = arg == "-M";
            }
            "-MD" | "-MMD" => {
                inv.deps.beside = true;
                inv.deps.system = arg == "-MD";
            }
            "-MP" => inv.deps.phony = true,
            "--version" => inv.query = Some(Query::Version),
            "--help" => inv.query = Some(Query::Help),
            "-dumpversion" => inv.query = Some(Query::DumpVersion),
            "-dumpfullversion" => inv.query = Some(Query::DumpFullVersion),
            "-dumpmachine" => inv.query = Some(Query::DumpMachine),
            "-print-search-dirs" => inv.query = Some(Query::SearchDirs),
            "-print-multi-os-directory" => inv.query = Some(Query::MultiOsDirectory),
            "-print-multiarch" => inv.query = Some(Query::Multiarch),
            "-print-libgcc-file-name" => inv.query = Some(Query::Name("libgcc.a".to_owned())),
            "-print-sysroot" => inv.query = Some(Query::Literal("")),
            "-print-multi-directory" => inv.query = Some(Query::Literal(".")),
            "-print-multi-lib" => inv.query = Some(Query::Literal(".;")),
            "-dumpspecs" => inv.query = Some(Query::Literal("")),
            _ if arg.starts_with("-print-prog-name=") || arg.starts_with("-print-file-name=") => {
                let (_, name) = arg.split_once('=').expect("the prefix has one");
                inv.query = Some(Query::Name(name.to_owned()));
            }
            "-w" => inv.warnings = false,
            "-v" => inv.verbose = true,
            "-s" => inv.strip = true,
            "-shared" => inv.shared = true,
            "-static" => inv.static_link = true,
            // `rustc` links a position-independent executable for Linux
            // anyway, and the objects are position-independent whatever is
            // asked: `-no-pie` is a property a program cannot observe.
            "-pie" | "-no-pie" => {}
            // The C driver's spelling of `--export-dynamic`, which the C
            // compiler `rustc` links with understands as it is.
            "-rdynamic" => inv.linker_args.push(LinkerArg::Driver(arg.clone())),
            // `-pedantic` only adds warnings, and cinrs has none of those to
            // add. `-pedantic-errors` makes the constraint violations GCC
            // warns about errors: an ISO `-std=` then refuses GCC's
            // leniencies, as `c11!` does.
            "-pipe" | "-pedantic" | "-Wpedantic" => {}
            "-pedantic-errors" | "-Werror=pedantic" => inv.pedantic_errors = true,
            // Which inline definitions provide an external definition, and
            // `__GNUC_GNU_INLINE__` or `__GNUC_STDC_INLINE__`.
            "-fgnu89-inline" => inv.gnu89_inline = Some(true),
            "-fno-gnu89-inline" => inv.gnu89_inline = Some(false),
            // GCC's `-pthread` is the library and the macro.
            "-pthread" => {
                inv.libs.push("pthread".to_owned());
                inv.macros
                    .push(CommandLineMacro::Define("_REENTRANT".to_owned()));
            }
            "-nostdinc" => inv.system_include = false,
            "-ansi" => (inv.standard, inv.dialect) = (Standard::C89, Dialect::Iso),
            "-m64" => inv.pointer_bits = Some(64),
            "-m32" => inv.pointer_bits = Some(32),
            "-" => {
                stdin = Some(force_c);
                inv.inputs.push(Input::C(PathBuf::from("-")));
            }
            "-dM" => inv.dump_macros = true,
            _ if arg.starts_with("-save-temps") => inv.save_temps = true,
            _ if arg.starts_with("-std=") => {
                (inv.standard, inv.dialect) = standard(&arg["-std=".len()..])
                    .ok_or_else(|| format!("unrecognized command-line option '{arg}'"))?;
            }
            _ if arg.starts_with("-O") => {
                inv.opt_level = match &arg[2..] {
                    "0" => "0",
                    "" | "1" | "g" => "1",
                    "2" => "2",
                    "s" => "s",
                    "z" => "z",
                    "fast" => "3",
                    level if level.bytes().all(|b| b.is_ascii_digit()) => "3",
                    _ => return Err(format!("unrecognized command-line option '{arg}'")),
                };
            }
            _ if arg.starts_with("-g") => {
                inv.debuginfo = if arg == "-g0" { 0 } else { 2 };
            }
            _ if arg.starts_with("-Wl,") => {
                for word in arg["-Wl,".len()..].split(',') {
                    linker_word(&mut inv, &mut whole, word);
                }
            }
            _ if arg.starts_with("-Wa,") || arg.starts_with("-Wp,") => {
                inv.notes.push(format!(
                    "ignoring '{arg}': ccinrs runs no separate assembler or preprocessor"
                ));
            }
            // A warning option chooses what GCC would print, which changes
            // nothing here: the ones GCC knows are taken without a word. One
            // it does not know is a warning, as in Clang — and under `-Werror`
            // an error, which is what a `configure` script's test of whether
            // the compiler takes a flag looks for. An unknown `-Wno-…` is let
            // be, as GCC lets it be.
            "-Werror" => inv.werror = true,
            "-Wno-error" => inv.werror = false,
            // Clang's own switches for what it says of an unknown one:
            // jemalloc's `configure` asks for an error before it probes a
            // warning option, and would otherwise take Clang's for GCC's.
            "-Werror=unknown-warning-option" => unknown_error = true,
            "-Wno-error=unknown-warning-option" => unknown_error = false,
            "-Wno-unknown-warning-option" => unknown_quiet = true,
            "-Wunknown-warning-option" => unknown_quiet = false,
            // GCC's old spelling of `-Wextra`, which libevent's configure
            // still adds.
            "-W" => {}
            _ if arg.starts_with("-W") => {
                let name = &arg[2..];
                if !gcc_warnings::known(name) && !name.starts_with("no-") {
                    inv.notes
                        .push(format!("ignoring unknown warning option '{arg}'"));
                    unknown.push(arg.clone());
                }
            }
            _ if arg.starts_with("-f") => {
                if !flag(&mut inv, &arg)? {
                    unknown.push(arg.clone());
                }
            }
            // Tuning changes no meaning, and SSE arithmetic is what x86-64
            // does anyway.
            _ if arg.starts_with("-mtune=") || arg == "-mfpmath=sse" => {}
            _ if arg.starts_with("-march=") || arg.starts_with("-mcpu=") => {
                let (_, cpu) = arg.split_once('=').expect("the prefix has one");
                inv.cpu = Some(cpu.to_owned());
            }
            _ if arg.starts_with("-m") => {
                let name = &arg[2..];
                let set = name.strip_prefix("no-").unwrap_or(name);
                if let Some(why) = cinrs_core::x86::unsupported_feature(set) {
                    // `-mno-mmx` asks for what ccinrs does anyway.
                    if name.starts_with("no-") {
                        continue;
                    }
                    return Err(format!("'{arg}' is not supported: {why}"));
                }
                if cinrs_core::x86::feature_row(set).is_none() {
                    return Err(format!(
                        "'{arg}' is not supported: ccinrs takes -march=, -mcpu= and the \
                         instruction-set switches (-mavx2, -mno-avx512f, …)"
                    ));
                }
                inv.features.push(name.to_owned());
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unrecognized command-line option '{arg}'"));
            }
            _ => {
                let path = PathBuf::from(&arg);
                if !force_c && path.extension().is_some_and(|ext| ext == "h") {
                    headers.push(path.clone());
                }
                let input = input(path, force_c)?;
                if whole && matches!(input, Input::Linker(_)) {
                    inv.whole_archives.push(inv.inputs.len());
                }
                inv.inputs.push(input);
            }
        }
    }
    // GCC's rule: nothing says what language standard input is, so it has to
    // be said, unless all that is asked is to preprocess it.
    if stdin == Some(false) && inv.stage != Stage::Preprocess {
        return Err("-E or -x required when input is from standard input".to_owned());
    }
    if let Some(header) = headers.first()
        && inv.stage != Stage::Preprocess
    {
        return Err(format!(
            "{}: a header is not compiled on its own; ccinrs makes no precompiled headers",
            header.display()
        ));
    }
    if let Some(first) = unknown.first()
        && inv.werror
    {
        return Err(format!("unrecognized command-line option '{first}'"));
    }
    if let Some(first) = unknown.iter().find(|arg| arg.starts_with("-W"))
        && unknown_error
    {
        return Err(format!("unknown warning option '{first}'"));
    }
    if unknown_quiet {
        inv.notes
            .retain(|note| !note.starts_with("ignoring unknown warning option"));
    }
    Ok(inv)
}

/// One word for the linker, from `-Wl,` or `-Xlinker`.
fn linker_word(inv: &mut Invocation, whole: &mut bool, word: &str) {
    match word.trim_start_matches('-') {
        "whole-archive" => *whole = true,
        "no-whole-archive" => *whole = false,
        _ => inv.linker_args.push(LinkerArg::Linker(word.to_owned())),
    }
}

/// One `-f` option; `false` for one ccinrs does not know, which it ignores
/// with a warning.
fn flag(inv: &mut Invocation, arg: &str) -> Result<bool, String> {
    let name = &arg[2..];
    match name {
        "lto" => inv.lto = Some(Lto::Fat),
        "lto=thin" => inv.lto = Some(Lto::Thin),
        // `-flto=auto`, `-flto=8`: how many jobs, which is `rustc`'s to say.
        _ if name.starts_with("lto=") => inv.lto = Some(Lto::Fat),
        "no-lto" => inv.lto = None,
        _ if name.starts_with("use-ld=") => inv.fuse_ld = Some(name["use-ld=".len()..].to_owned()),
        "syntax-only" => inv.stop_at(Stage::SyntaxOnly),
        "cinrs-checks" => inv.checks = true,
        "no-cinrs-checks" => inv.checks = false,
        "cinrs-unwind" => inv.unwind = true,
        "no-cinrs-unwind" => inv.unwind = false,
        // GCC's three answers to what an uninitialised local holds. `pattern`
        // fills every byte with 0xFE, which makes a `_Bool` that is neither
        // true nor false — something Rust may not have — so it is taken as
        // `zero`, the other hardening choice, and said so.
        "trivial-auto-var-init=zero" => inv.auto_var_init = AutoVarInit::Zero,
        "trivial-auto-var-init=uninitialized" => {
            inv.auto_var_init = AutoVarInit::Uninitialized;
        }
        "trivial-auto-var-init=pattern" => {
            inv.auto_var_init = AutoVarInit::Zero;
            inv.notes.push(format!(
                "'{arg}' is taken as '-ftrivial-auto-var-init=zero': GCC's pattern fills every \
                 byte with 0xFE, which makes a '_Bool' that is neither true nor false, and Rust \
                 may not have one"
            ));
        }
        _ if name.starts_with("trivial-auto-var-init=") => {
            return Err(format!(
                "unrecognized argument in option '{arg}'; valid arguments to \
                 '-ftrivial-auto-var-init=' are: pattern uninitialized zero"
            ));
        }
        // The source and the program's strings are UTF-8, which is what cinrs
        // reads and writes; Tcl's `configure` adds `-finput-charset=UTF-8`.
        // Another character set would change a string's bytes.
        _ if name.starts_with("input-charset=") || name.starts_with("exec-charset=") => {
            let (_, charset) = name.split_once('=').expect("the prefix has one");
            if !matches!(charset.to_ascii_lowercase().as_str(), "utf-8" | "utf8") {
                return Err(format!(
                    "'{arg}' changes what the program means, and ccinrs cannot follow it: \
                     it reads its source and writes its strings as UTF-8"
                ));
            }
        }
        "dollars-in-identifiers" => inv.dollars = true,
        "no-dollars-in-identifiers" => inv.dollars = false,
        "signed-char" | "no-unsigned-char" => inv.char_signed = Some(true),
        "unsigned-char" | "no-signed-char" => inv.char_signed = Some(false),
        _ if listed(QUIET_FLAGS, name) => {}
        _ if listed(MEANINGFUL_FLAGS, name) => {
            return Err(format!(
                "'{arg}' changes what the program means, and ccinrs cannot follow it"
            ));
        }
        _ => {
            inv.notes.push(format!("ignoring unknown option '{arg}'"));
            return Ok(false);
        }
    }
    Ok(true)
}

/// What a file on the command line is, from its name — or C, under `-x c`.
fn input(path: PathBuf, force_c: bool) -> Result<Input, String> {
    if force_c {
        return Ok(Input::C(path));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_owned();
    match ext.as_str() {
        // A header is C to `-E` and `-M` — curl's tests preprocess `curl.h`
        // — and to nothing else; see `parse`.
        "c" | "i" | "h" => Ok(Input::C(path)),
        "cc" | "cpp" | "cxx" | "C" | "c++" | "m" | "mm" => Err(format!(
            "{}: ccinrs compiles C, not this language",
            path.display()
        )),
        "s" | "S" | "asm" => Err(format!(
            "{}: assembly source is not supported; ccinrs runs no assembler",
            path.display()
        )),
        // Objects, archives, shared libraries, and anything else: GCC hands
        // a file it does not recognise to the linker, and so does this.
        _ => Ok(Input::Linker(path)),
    }
}

/// A name as `make` reads it in a rule: `$` doubled, and a space or a `#`
/// escaped with a backslash — what `-MQ` does to its target, and what a
/// dependency's name gets.
pub fn make_quoted(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '$' => out.push_str("$$"),
            ' ' | '\t' | '#' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// A `-std=` value.
fn standard(name: &str) -> Option<(Standard, Dialect)> {
    Some(match name {
        "c89" | "c90" | "iso9899:1990" | "iso9899:199409" => (Standard::C89, Dialect::Iso),
        "gnu89" | "gnu90" => (Standard::C89, Dialect::Gnu),
        "c99" | "c9x" | "iso9899:1999" | "iso9899:199x" => (Standard::C99, Dialect::Iso),
        "gnu99" | "gnu9x" => (Standard::C99, Dialect::Gnu),
        "c11" | "c1x" | "iso9899:2011" => (Standard::C11, Dialect::Iso),
        "gnu11" | "gnu1x" => (Standard::C11, Dialect::Gnu),
        "c17" | "c18" | "iso9899:2017" | "iso9899:2018" => (Standard::C17, Dialect::Iso),
        "gnu17" | "gnu18" => (Standard::C17, Dialect::Gnu),
        "c23" | "c2x" | "iso9899:2024" => (Standard::C23, Dialect::Iso),
        "gnu23" | "gnu2x" => (Standard::C23, Dialect::Gnu),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(args: &[&str]) -> Result<Invocation, String> {
        parse(args.iter().map(|a| (*a).to_owned()))
    }

    #[test]
    fn attached_and_separate_values_are_the_same() {
        let a = parse_all(&["-Iinc", "-DX=1", "-o", "prog", "-lm", "-L", "lib", "a.c"]).unwrap();
        let b = parse_all(&[
            "-I", "inc", "-D", "X=1", "-oprog", "-l", "m", "-Llib", "a.c",
        ])
        .unwrap();
        for inv in [a, b] {
            assert_eq!(inv.include_dirs, [PathBuf::from("inc")]);
            assert_eq!(inv.macros, [CommandLineMacro::Define("X=1".to_owned())]);
            assert_eq!(inv.output, Some(PathBuf::from("prog")));
            assert_eq!(inv.libs, ["m"]);
            assert_eq!(inv.lib_dirs, [PathBuf::from("lib")]);
            assert_eq!(inv.inputs, [Input::C(PathBuf::from("a.c"))]);
        }
    }

    #[test]
    fn the_defaults_are_gccs() {
        let inv = parse_all(&["a.c"]).unwrap();
        assert_eq!((inv.standard, inv.dialect), (Standard::C17, Dialect::Gnu));
        assert_eq!(inv.opt_level, "0");
        assert!(inv.checks && inv.system_include && inv.warnings);
        assert_eq!(inv.stage, Stage::Link);
    }

    #[test]
    fn the_earliest_stage_wins() {
        assert_eq!(parse_all(&["-S", "-c", "a.c"]).unwrap().stage, Stage::Rust);
        assert_eq!(
            parse_all(&["-c", "-E", "a.c"]).unwrap().stage,
            Stage::Preprocess
        );
        let inv = parse_all(&["-MM", "-c", "a.c"]).unwrap();
        assert_eq!(inv.stage, Stage::Preprocess);
        assert!(inv.deps.only && !inv.deps.system);
        assert_eq!(
            parse_all(&["-c", "-fsyntax-only", "a.c"]).unwrap().stage,
            Stage::SyntaxOnly
        );
    }

    #[test]
    fn link_time_optimisation() {
        assert_eq!(parse_all(&["a.c"]).unwrap().lto, None);
        assert_eq!(parse_all(&["-flto", "a.c"]).unwrap().lto, Some(Lto::Fat));
        assert_eq!(
            parse_all(&["-flto=auto", "a.c"]).unwrap().lto,
            Some(Lto::Fat)
        );
        assert_eq!(
            parse_all(&["-flto=thin", "a.c"]).unwrap().lto,
            Some(Lto::Thin)
        );
        assert_eq!(parse_all(&["-flto", "-fno-lto", "a.c"]).unwrap().lto, None);
        let inv = parse_all(&["-flto", "-ffat-lto-objects", "-fuse-linker-plugin", "a.c"]).unwrap();
        assert!(inv.notes.is_empty(), "{:?}", inv.notes);
    }

    #[test]
    fn standard_input_and_forced_includes() {
        let inv = parse_all(&["-E", "-dM", "-"]).unwrap();
        assert!(inv.dump_macros);
        assert_eq!(inv.inputs, [Input::C(PathBuf::from("-"))]);
        assert!(parse_all(&["-x", "c", "-c", "-"]).is_ok());
        assert_eq!(
            parse_all(&["-c", "-"]).unwrap_err(),
            "-E or -x required when input is from standard input"
        );
        let inv = parse_all(&["-include", "config.h", "-include", "b.h", "a.c"]).unwrap();
        assert_eq!(inv.includes, ["config.h", "b.h"]);
    }

    #[test]
    fn dependency_options() {
        let inv = parse_all(&[
            "-MD",
            "-MP",
            "-MF",
            "x.d",
            "-MT",
            "a.o",
            "-MQ",
            "$(OBJ) b.o",
            "-c",
            "a.c",
        ])
        .unwrap();
        assert_eq!(
            inv.deps,
            Deps {
                only: false,
                beside: true,
                system: true,
                file: Some(PathBuf::from("x.d")),
                targets: vec!["a.o".to_owned(), "$$(OBJ)\\ b.o".to_owned()],
                phony: true,
            }
        );
        assert_eq!(inv.stage, Stage::Compile);
        let inv = parse_all(&["-MMD", "-MFy.d", "a.c"]).unwrap();
        assert!(inv.deps.beside && !inv.deps.system);
        assert_eq!(inv.deps.file, Some(PathBuf::from("y.d")));
    }

    #[test]
    fn processors_and_instruction_sets() {
        let inv = parse_all(&[
            "-march=haswell",
            "-mtune=generic",
            "-mno-avx2",
            "-msse4_1",
            "-mno-mmx",
            "a.c",
        ])
        .unwrap();
        assert_eq!(inv.cpu.as_deref(), Some("haswell"));
        assert_eq!(inv.features, ["no-avx2", "sse4_1"]);
        let error = parse_all(&["-mmmx", "a.c"]).unwrap_err();
        assert!(
            error.starts_with("'-mmmx' is not supported: Rust's core::arch has no MMX"),
            "{error}"
        );
    }

    #[test]
    fn standards_and_levels() {
        let inv = parse_all(&["-std=c99", "-O2", "-g", "a.c"]).unwrap();
        assert_eq!((inv.standard, inv.dialect), (Standard::C99, Dialect::Iso));
        assert_eq!((inv.opt_level, inv.debuginfo), ("2", 2));
        let inv = parse_all(&["-std=gnu2x", "-O", "-Os", "a.c"]).unwrap();
        assert_eq!((inv.standard, inv.dialect), (Standard::C23, Dialect::Gnu));
        assert_eq!(inv.opt_level, "s");
        assert!(parse_all(&["-std=c2y", "a.c"]).is_err());
    }

    #[test]
    fn warnings_and_flags() {
        let inv = parse_all(&[
            "-Wall",
            "-Wextra",
            "-Wno-unused",
            "-Wformat=2",
            "-W",
            "-Wswitch-enum",
            "-pedantic",
            "-fPIC",
            "-fwrapv",
            "-fstack-protector-strong",
            "-ftls-model=global-dynamic",
            "-ffp-contract=off",
            "-fdiagnostics-color=auto",
            "-finput-charset=UTF-8",
            "-fexec-charset=utf-8",
            "-fextended-identifiers",
            "-pie",
            "a.c",
        ])
        .unwrap();
        assert!(inv.notes.is_empty(), "{:?}", inv.notes);
        let inv = parse_all(&["-shared", "-static", "-rdynamic", "a.c"]).unwrap();
        assert!(inv.shared && inv.static_link);
        assert_eq!(inv.linker_args, [LinkerArg::Driver("-rdynamic".to_owned())]);
        // libtool's way of passing a version script, and GCC's other two.
        let inv = parse_all(&[
            "-Wl,--version-script",
            "-Wl,lib.map",
            "-Wl,-rpath,/opt/lib",
            "-Xlinker",
            "--defsym=a,b",
            "a.c",
        ])
        .unwrap();
        let linker = |word: &str| LinkerArg::Linker(word.to_owned());
        assert_eq!(
            inv.linker_args,
            [
                linker("--version-script"),
                linker("lib.map"),
                linker("-rpath"),
                linker("/opt/lib"),
                linker("--defsym=a,b"),
            ]
        );
        // What libtool links a convenience library with: the two words are
        // kept as which archives they surround.
        let inv = parse_all(&[
            "a.o",
            "-Wl,--whole-archive",
            "libx.a",
            "liby.a",
            "-Wl,--no-whole-archive",
            "libz.a",
        ])
        .unwrap();
        assert_eq!(inv.whole_archives, [1, 2]);
        assert!(inv.linker_args.is_empty());
        let inv = parse_all(&["-Wfrobnicate", "-ffrobnicate", "a.c"]).unwrap();
        assert_eq!(inv.notes.len(), 2);
        // What a `configure` script's probe of a flag looks for: an unknown
        // warning is an error under `-Werror`, and a known one is not.
        assert_eq!(
            parse_all(&["-Werror", "-Wfrobnicate", "a.c"]).unwrap_err(),
            "unrecognized command-line option '-Wfrobnicate'"
        );
        let inv = parse_all(&[
            "-Werror",
            "-Wduplicated-cond",
            "-Werror=format-security",
            "-Wno-frobnicate",
            "-Wstrict-aliasing=2",
            "a.c",
        ])
        .unwrap();
        assert!(inv.notes.is_empty(), "{:?}", inv.notes);
        assert!(parse_all(&["-Werror", "-Wno-error", "-Wfrobnicate", "a.c"]).is_ok());
        assert_eq!(
            parse_all(&["-ffrobnicate", "-Werror", "a.c"]).unwrap_err(),
            "unrecognized command-line option '-ffrobnicate'"
        );
        assert!(parse_all(&["-Werror", "a.c"]).unwrap().werror);
        // Clang's switches for an unknown warning option, as jemalloc's
        // configure uses them.
        assert_eq!(
            parse_all(&[
                "-Werror=unknown-warning-option",
                "-Wshorten-64-to-32",
                "a.c"
            ])
            .unwrap_err(),
            "unknown warning option '-Wshorten-64-to-32'"
        );
        let inv = parse_all(&["-Wno-unknown-warning-option", "-Wshorten-64-to-32", "a.c"]).unwrap();
        assert!(inv.notes.is_empty(), "{:?}", inv.notes);
        assert!(!inv.werror);
        assert_eq!(
            parse_all(&["-fuse-ld=bfd", "a.c"])
                .unwrap()
                .fuse_ld
                .as_deref(),
            Some("bfd")
        );
        assert!(
            parse_all(&["-fshort-enums", "a.c"])
                .unwrap_err()
                .contains("changes what")
        );
        assert!(
            parse_all(&["-finput-charset=ISO-8859-1", "a.c"])
                .unwrap_err()
                .contains("UTF-8")
        );
        assert!(!parse_all(&["-fno-cinrs-checks", "a.c"]).unwrap().checks);
        assert!(parse_all(&["a.c"]).unwrap().unwind);
        assert!(!parse_all(&["-fno-cinrs-unwind", "a.c"]).unwrap().unwind);
        assert!(
            parse_all(&["-fno-cinrs-unwind", "-fcinrs-unwind", "a.c"])
                .unwrap()
                .unwind
        );
        assert_eq!(
            parse_all(&["a.c"]).unwrap().auto_var_init,
            AutoVarInit::Zero
        );
        assert_eq!(
            parse_all(&["-ftrivial-auto-var-init=uninitialized", "a.c"])
                .unwrap()
                .auto_var_init,
            AutoVarInit::Uninitialized
        );
        let pattern = parse_all(&["-ftrivial-auto-var-init=pattern", "a.c"]).unwrap();
        assert_eq!(pattern.auto_var_init, AutoVarInit::Zero);
        assert_eq!(pattern.notes.len(), 1);
        assert!(parse_all(&["-ftrivial-auto-var-init=maybe", "a.c"]).is_err());
        assert_eq!(
            parse_all(&["-funsigned-char", "a.c"]).unwrap().char_signed,
            Some(false)
        );
        assert!(
            parse_all(&["--frobnicate", "a.c"])
                .unwrap_err()
                .contains("unrecognized")
        );
    }

    #[test]
    fn questions_a_build_asks() {
        let ask = |arg: &str| parse_all(&[arg]).unwrap().query;
        assert_eq!(ask("-print-search-dirs"), Some(Query::SearchDirs));
        assert_eq!(ask("-print-multiarch"), Some(Query::Multiarch));
        assert_eq!(
            ask("-print-prog-name=ld"),
            Some(Query::Name("ld".to_owned()))
        );
        assert_eq!(
            ask("-print-file-name=libc.so"),
            Some(Query::Name("libc.so".to_owned()))
        );
        assert_eq!(ask("-print-multi-directory"), Some(Query::Literal(".")));
    }

    #[test]
    fn files_by_extension() {
        let inv = parse_all(&["a.c", "b.o", "libc.a", "-x", "c", "d.txt"]).unwrap();
        assert_eq!(
            inv.inputs,
            [
                Input::C("a.c".into()),
                Input::Linker("b.o".into()),
                Input::Linker("libc.a".into()),
                Input::C("d.txt".into()),
            ]
        );
        assert!(parse_all(&["a.cpp"]).is_err());
        assert!(parse_all(&["a.S"]).is_err());
    }
}
