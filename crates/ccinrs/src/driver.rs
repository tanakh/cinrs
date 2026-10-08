//! From a command line to a program.
//!
//! Each C file is translated by cinrs-core, the Rust it becomes is compiled by
//! `rustc` into an ordinary object file — one small crate per translation unit,
//! `--crate-type lib --emit obj` — and the objects are linked by `rustc` too,
//! into a `#![no_main]` binary whose `main` is the C one. `rustc` doing the
//! link is not a convenience: the objects name the standard library and the
//! allocator shim the way that `rustc` mangles them, and only it knows where
//! those live. `ccinrs` never runs a C compiler or a linker of its own.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

use cinrs_core::include::System;
use cinrs_core::{Arch, Env, Options, Os, TargetModel, TargetSource, Unwind};

use crate::args::{Input, Invocation, LinkerArg, Lto, Query, Stage};
use crate::runtime::{self, Runtime};
use crate::rustc::Rustc;
use crate::{deps, diag, elf, preprocess, print};

/// Why a run stopped.
pub enum Failure {
    /// Everything there is to say has been printed: the C had errors.
    Reported,
    /// One message to print.
    Message(String),
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::Message(message)
    }
}

/// Runs one command line.
pub fn run(inv: &Invocation) -> Result<(), Failure> {
    if inv.warnings {
        for note in &inv.notes {
            eprintln!("ccinrs: warning: {note}");
        }
    }
    if let Some(query) = &inv.query {
        return answer(inv, query);
    }
    if inv.inputs.is_empty() {
        // `gcc -v` on its own says what it is, and that is all.
        if inv.verbose {
            eprint!("{}", verbose_text());
            return Ok(());
        }
        return Err("no input files".to_owned().into());
    }
    let rustc = Rustc::locate()?;
    if inv.verbose {
        eprintln!(
            "ccinrs {} (cinrs-core), rustc {} for {}",
            env!("CARGO_PKG_VERSION"),
            rustc.release,
            rustc.host
        );
    }
    let target = Target::choose(inv, &rustc)?;
    check_target(inv, &target.model, &target.triple)?;
    let work = WorkDir::create(inv.save_temps)?;
    let features = command_line_features(inv, &rustc, &target)?;
    let run = Run {
        inv,
        options: options(inv, &target, features),
        stamp: stamp(&rustc, &target),
        rustc,
        target,
        work,
        runtime: Runtime::new(),
    };

    match inv.stage {
        Stage::Preprocess => return run.preprocess(),
        Stage::SyntaxOnly => return run.check_syntax(),
        Stage::Rust => return run.emit_rust(),
        Stage::Compile => return run.compile_only(),
        Stage::Link => {}
    }
    let output = inv
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from(default_output()));
    check_output(&output)?;
    let mut objects = Vec::new();
    let mut linked = ObjectInfo::default();
    let mut failed = false;
    for (index, input) in inv.inputs.iter().enumerate() {
        match input {
            Input::C(path) => {
                let object = run.work.path(&format!("{}.o", run.work.stem(index, path)));
                let deps = run.deps_beside(path, None);
                match run.compile(index, path, &object, deps)? {
                    Some(info) => {
                        objects.push(LinkInput {
                            path: object,
                            crate_name: info.crates.first().cloned(),
                            whole: false,
                        });
                        linked.add(info);
                    }
                    None => failed = true,
                }
            }
            Input::Linker(path) => {
                let mut info = check_stamp(path, &run.stamp)?;
                // An archive's members are linked when something needs them,
                // and a shared library's symbols are its own, so neither is
                // what a shared library made here exports — unless the
                // archive is linked whole, when every member is in, as
                // libtool puts a convenience library into a shared one.
                let whole = is_archive(path) && inv.whole_archives.contains(&index);
                if (is_archive(path) && !whole) || is_shared_library(path) {
                    info.symbols.clear();
                }
                // An `-flto` object is an rlib, which is an archive itself,
                // and an archive of archives is nothing a linker reads — as
                // GCC's own LTO objects need `gcc-ar` to go into one.
                if is_archive(path) && !info.crates.is_empty() {
                    return Err(format!(
                        "{} holds objects compiled with -flto, which cannot be linked from an \
                         archive; link the objects themselves",
                        path.display()
                    )
                    .into());
                }
                let crate_name = info.crates.first().cloned();
                linked.add(info);
                // A shared library is named to the linker as it was on the
                // command line, which is what a program without a soname
                // to go by records as its dependency (GCC's `DT_NEEDED`);
                // the rest are brought into the work directory.
                let path = if is_shared_library(path) {
                    path.clone()
                } else {
                    run.work.link_input(index, path)?
                };
                objects.push(LinkInput {
                    path,
                    crate_name,
                    whole,
                });
            }
        }
    }
    if failed {
        return Err(Failure::Reported);
    }
    run.link(&objects, &linked, &output)
}

/// One file a link takes, in the work directory.
struct LinkInput {
    path: PathBuf,
    /// The crate an object compiled with `-flto` is — an rlib, which the link
    /// loads as one; see [`Run::compile`].
    crate_name: Option<String>,
    /// An archive linked whole: `-Wl,--whole-archive` around it.
    whole: bool,
}

/// What a link has to know about the objects in it, which each object made
/// here carries in itself; see [`check_stamp`].
#[derive(Default)]
struct ObjectInfo {
    /// Whether one of them calls the runtime.
    uses_runtime: bool,
    /// The C symbols they define, which a shared library exports.
    symbols: Vec<String>,
    /// The crates the `-flto` ones are.
    crates: Vec<String>,
    /// Whether one of them was compiled here *without* `-flto`: machine code
    /// that calls into Rust's standard library by symbol.
    native: bool,
    /// Whether one of the `-flto` ones is bitcode for the linker's
    /// link-time optimisation; see [`Target::linker_lto`].
    bitcode: bool,
}

impl ObjectInfo {
    fn add(&mut self, other: ObjectInfo) {
        self.uses_runtime |= other.uses_runtime;
        self.symbols.extend(other.symbols);
        self.crates.extend(other.crates);
        self.native |= other.native;
        self.bitcode |= other.bitcode;
    }
}

/// Answers `--version`, `-dumpmachine`, `-print-search-dirs` and the like.
fn answer(inv: &Invocation, query: &Query) -> Result<(), Failure> {
    let (major, minor, patch) = cinrs_core::GCC_VERSION;
    let triple = || -> Result<String, String> {
        match &inv.target {
            Some(triple) => Ok(triple.clone()),
            None => Ok(Rustc::locate()?.host),
        }
    };
    match query {
        Query::Version => print!("{}", version_text()),
        // What `__GNUC__` says, which is what a build that asks compares.
        Query::DumpVersion => println!("{major}"),
        Query::DumpFullVersion => println!("{major}.{minor}.{patch}"),
        Query::DumpMachine => println!("{}", triple()?),
        Query::Help => print!("{HELP}"),
        Query::Name(name) => println!("{name}"),
        // GCC's answer on a 64-bit Debian or Ubuntu, where the libraries are
        // in `lib/<multiarch>` beside `lib`.
        Query::MultiOsDirectory => println!("../lib"),
        Query::Multiarch => println!("{}", multiarch(&triple()?)),
        Query::Literal(text) => println!("{text}"),
        // The directories a link searches for the C library's libraries:
        // the platform's, for a program for this machine, whose linker is
        // the platform's; nothing for another.
        Query::SearchDirs => {
            let install = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| format!("{}/", dir.display())))
                .unwrap_or_default();
            let mut libraries = Vec::new();
            if inv.target.is_none() {
                let arch = multiarch(&triple()?);
                if !arch.is_empty() {
                    libraries.push(format!("/lib/{arch}/"));
                    libraries.push(format!("/usr/lib/{arch}/"));
                }
                libraries.push("/lib/".to_owned());
                libraries.push("/usr/lib/".to_owned());
                libraries.retain(|dir| Path::new(dir).is_dir());
            }
            println!("install: {install}");
            println!("programs: ={install}");
            println!("libraries: ={}", libraries.join(":"));
        }
    }
    Ok(())
}

/// Debian's name for a Linux target's library directory — `x86_64-linux-gnu`,
/// `i386-linux-gnu`, `aarch64-linux-musl` — or nothing for another system.
fn multiarch(triple: &str) -> String {
    let parts: Vec<&str> = triple.split('-').collect();
    let (Some(arch), Some(env)) = (parts.first(), parts.last()) else {
        return String::new();
    };
    if !parts.contains(&"linux") {
        return String::new();
    }
    let arch = match *arch {
        "i386" | "i486" | "i586" | "i686" => "i386",
        other => other,
    };
    format!("{arch}-linux-{env}")
}

/// What `--version` prints: ccinrs's own version, and the `rustc` it would
/// compile with, or why there is none.
fn version_text() -> String {
    let rustc = match Rustc::locate() {
        Ok(rustc) => format!(
            "rustc {} ({}) for {}",
            rustc.release, rustc.commit, rustc.host
        ),
        Err(error) => format!("no rustc: {error}"),
    };
    let (major, minor, patch) = cinrs_core::GCC_VERSION;
    format!(
        "ccinrs {} — C to Rust with cinrs, compiled and linked by rustc\n{rustc}\n\
         The C it compiles sees GCC {major}.{minor}.{patch} (__GNUC__ is {major}).\n",
        env!("CARGO_PKG_VERSION"),
    )
}

/// What `-v` on its own prints: [`version_text`], and then the line GCC's own
/// `-v` ends with, `gcc version 14.2.0 (…)`, which build scripts read to
/// decide whether a compiler takes GCC's options — zlib's `configure` gives a
/// shared library a soname only then. It says the version cinrs presents
/// itself as, as `__GNUC__` does, and that this is not GCC.
fn verbose_text() -> String {
    let (major, minor, patch) = cinrs_core::GCC_VERSION;
    format!(
        "{}gcc version {major}.{minor}.{patch} compatible (ccinrs {}, not GCC)\n",
        version_text(),
        env!("CARGO_PKG_VERSION"),
    )
}

/// What `--help` prints.
const HELP: &str = "\
Usage: ccinrs [options] file...

ccinrs compiles C with GCC's command line: each file becomes Rust through
cinrs, and rustc (RUSTC, or the one on PATH) compiles and links it.

  -o <file>            Write the output to <file>
  -c                   Compile to an object file, and do not link
  -S                   Write the Rust each C file becomes (<file>.rs)
  -E                   Preprocess only; -P leaves out the line markers
  -M, -MM              Write a Makefile rule of the headers instead
  -MD, -MMD            Write that rule to a .d file while compiling
  -MF <file>, -MT <target>, -MQ <target>, -MP
                       Where the rule goes, what it is for, phony headers
  -I <dir>, -D <name>[=<value>], -U <name>
                       Include directories and macros
  -std=<standard>      c89 … c23, gnu89 … gnu23 (gnu17 by default)
  -O<level>, -g        Optimisation and debug information, as rustc's
  -march=<cpu>, -m<feature>
                       The processor and instruction sets
  --target=<triple>    Compile for another machine (wasm32-wasip1, …)
  -l <lib>, -L <dir>, -Wl,<args>
                       Libraries and linker arguments
  -fno-cinrs-checks    Leave out Rust's run-time checks (on by default)
  -fno-cinrs-unwind    Make functions extern \"C\", not \"C-unwind\" (a little
                       faster; refuses setjmp and longjmp)
  -ftrivial-auto-var-init=uninitialized
                       Leave local arrays uninitialised (zero by default)
  -w                   Print no warnings
  -v                   Print the commands run
  --version, -dumpversion, -dumpmachine
";

/// One command line, with what every file of it is compiled with.
struct Run<'a> {
    inv: &'a Invocation,
    rustc: Rustc,
    target: Target,
    /// What cinrs-core is told.
    options: Options,
    work: WorkDir,
    /// What every object carries; see [`stamp`].
    stamp: String,
    runtime: Runtime,
}

/// The machine the program is for.
struct Target {
    /// Its triple, as `rustc` names it.
    triple: String,
    /// What cinrs-core makes of it: sizes, alignments, predefined macros.
    model: TargetModel,
    /// Whether it is another machine than the one `rustc` runs on.
    cross: bool,
    /// Whether the Rust target carries its own C library and start files —
    /// musl's, which Rust ships as `libc.a` and `crt1.o` beside the standard
    /// library — so that `rust-lld` links the program alone, and the machine
    /// needs no C compiler, no linker and no C library of its own.
    self_contained: bool,
    /// Whether it is WebAssembly with no system under it —
    /// `wasm32-unknown-unknown` — where what is made is a module for a host
    /// to call into; see [`Run::link`].
    bare_wasm: bool,
}

impl Target {
    /// `--target`, or the machine `rustc` runs on.
    fn choose(inv: &Invocation, rustc: &Rustc) -> Result<Self, String> {
        let (triple, model) = match &inv.target {
            Some(triple) => {
                let model = TargetModel::from_triple(triple)
                    .map_err(|unknown| unknown.message(&TargetSource::Explicit))?;
                (triple.clone(), model)
            }
            None => (rustc.host.clone(), TargetModel::host()),
        };
        // The machine `rustc` runs on has its standard library — `rustup`
        // installs it with the compiler, and `rustc` says so itself where it
        // is missing — so only another machine's is looked for, and musl's,
        // which may carry its C library: asking costs another run of `rustc`
        // for every file compiled.
        let self_contained = if triple == rustc.host && model.env != Env::Musl {
            false
        } else {
            let Some(libdir) = rustc.target_libdir(&triple) else {
                return Err(format!(
                    "the Rust standard library for '{triple}' is not installed; \
                     `rustup target add {triple}` installs it"
                ));
            };
            model.env == Env::Musl && libdir.join("self-contained/libc.a").is_file()
        };
        let bare_wasm = model.arch == Arch::Wasm32 && model.os == Os::None;
        Ok(Self {
            cross: triple != rustc.host,
            triple,
            model,
            self_contained,
            bare_wasm,
        })
    }

    /// Whether the program is linked by `rust-lld`, the one linker here that
    /// reads LLVM bitcode: on x86-64 Linux, where `rustc` links with it by
    /// default, unless `-fuse-ld` names another, and wherever the target
    /// carries its own C library.
    fn links_with_lld(&self, inv: &Invocation) -> bool {
        self.self_contained
            || (self.triple == "x86_64-unknown-linux-gnu"
                && inv.fuse_ld.as_deref().is_none_or(|ld| ld == "lld"))
    }

    /// Whether `-flto` is the linker's own link-time optimisation — the
    /// objects are LLVM bitcode, which `rust-lld` optimises as one program
    /// (ThinLTO) — rather than `rustc`'s.
    ///
    /// The linker's sees every object in the link, so an object compiled
    /// without `-flto` (an archive of a build's own `deps/`, a unit with a
    /// weak definition) links beside the optimised ones: what it calls in
    /// Rust's standard library stays defined. `rustc`'s keeps only what the
    /// crates it optimises use, and an object it does not see is left calling
    /// `core::panicking::panic` by a name nothing defines.
    fn linker_lto(&self, inv: &Invocation) -> bool {
        inv.lto.is_some() && !self.bare_wasm && self.links_with_lld(inv)
    }
}

/// What cinrs-core is told for every file of this command line.
fn options(inv: &Invocation, target: &Target, features: Vec<String>) -> Options {
    let mut options = Options::with_dialect(inv.standard, inv.dialect).for_target(target.model);
    options.include_paths = inv.include_dirs.clone();
    // `--sysroot` names where another machine's headers are, after `-I`.
    if let Some(sysroot) = &inv.sysroot {
        for dir in [sysroot.join("include"), sysroot.join("usr/include")] {
            if dir.is_dir() {
                options.include_paths.push(dir);
            }
        }
    }
    // The headers in this machine's own directories describe this machine,
    // so a program for another one uses the bundled headers — and whatever
    // `--sysroot` or `-I` points at.
    options.system_include = if inv.system_include && !target.cross {
        // GCC's order: the platform's headers win, and what only a compiler
        // carries — <stdarg.h>, <stddef.h>, the intrinsics — is cinrs's,
        // since the platform's directories do not have them.
        System::First
    } else {
        System::Off
    };
    options.dollar_in_identifiers = inv.dollars;
    options.macros = inv.macros.clone();
    options.includes = inv.includes.clone();
    options.target_features = features;
    // To a C compiler every definition that is not `static` is a symbol.
    options.export = true;
    // The complex types need the runtime, which `runtime` provides.
    options.complex = true;
    // Any file may be between a `longjmp` and its `setjmp` — a callback that
    // jumps out through a library — so every function is `extern
    // "C-unwind"`, unless `-fno-cinrs-unwind` asked for `extern "C"`, which
    // then refuses `setjmp` and `longjmp` by name.
    options.unwind = if inv.unwind {
        Unwind::Always
    } else {
        Unwind::Never
    };
    // `-ftrivial-auto-var-init=`; a unit's `#pragma cinrs auto_var_init`
    // still wins for that unit.
    options.auto_var_init = inv.auto_var_init;
    // What ccinrs links is what it compiled, and the platform's libraries:
    // a function or object the program declares outside the platform's
    // headers is another of its own files, with cinrs's `long double` and
    // cinrs's way to a thread-local object.
    options.own_declarations_are_cinrs = true;
    // A weak definition is a real one, made with the assembler, wherever the
    // object is assembled on its own: not under `rustc`'s `-flto`, where an
    // override in another file can share the module with the alias. Under
    // the linker's, a unit that makes one is compiled without it; see
    // `Run::compile`. cinrs-core settles the object format. ccinrs writes a
    // shared library's export list itself, so a symbol only the assembler
    // defines is exported too.
    options.weak_definitions = inv.lto.is_none() || target.linker_lto(inv);
    // Nothing in Rust reads the objects, so every array variable of 16 bytes
    // or more is 16-byte aligned on x86-64, as GCC's are.
    options.abi_align_public_arrays = true;
    // Nor names anything in them but what the linker sees, so a `static
    // inline` function of a header nothing calls, and a declaration nothing
    // names, need no Rust at all; see `cinrs_core::reach`.
    options.reachable_only = true;
    // A diagnostic that says how to choose another standard names `-std=`,
    // not the macro a `c99!` block is written with.
    options.front_end = cinrs_core::FrontEnd::CommandLine;
    // GCC's `-std=c11` warns about the constraint violations its
    // `-std=gnu11` takes, and only `-pedantic-errors` refuses them.
    options.gnu_leniencies = !inv.pedantic_errors;
    options.gnu89_inline = inv.gnu89_inline;
    options
}

/// `-funsigned-char` and `-m32` describe the target; cinrs takes the
/// target's own answer and cannot be told otherwise.
fn check_target(inv: &Invocation, target: &TargetModel, triple: &str) -> Result<(), String> {
    if let Some(signed) = inv.char_signed
        && signed != target.char_signed
    {
        return Err(format!(
            "'char' is {} on {triple}, and ccinrs cannot change that",
            if target.char_signed {
                "signed"
            } else {
                "unsigned"
            },
        ));
    }
    if let Some(bits) = inv.pointer_bits
        && bits != target.ptr_bits
    {
        return Err(format!(
            "'-m{bits}' asks for {bits}-bit pointers, and {triple} has {}",
            target.ptr_bits
        ));
    }
    Ok(())
}

impl Run<'_> {
    /// `-c`: one object per C file — `<stem>.o` in the working directory, as
    /// GCC names it, or `-o` for a single file — and no link.
    fn compile_only(&self) -> Result<(), Failure> {
        let mut failed = false;
        for (index, path, object) in outputs_each(self.inv, "-c", "o")? {
            let deps = self.deps_beside(path, Some(&object));
            if self.compile(index, path, &object, deps)?.is_none() {
                failed = true;
            }
        }
        if failed {
            Err(Failure::Reported)
        } else {
            Ok(())
        }
    }

    /// `-fsyntax-only`: every C file read and checked by cinrs — preprocessed,
    /// parsed and analysed — and its diagnostics printed; nothing is written
    /// and `rustc` is not run, as GCC writes nothing and generates no code.
    fn check_syntax(&self) -> Result<(), Failure> {
        unused_linker_inputs(self.inv)?;
        let mut failed = false;
        for input in &self.inv.inputs {
            let Input::C(path) = input else {
                continue;
            };
            let translation = translate(path, &self.options)?;
            let (text, tally) = diag::render(&translation.map, &translation.diagnostics, self.inv);
            eprint!("{text}");
            failed |= tally.errors > 0 || translation.items.is_none();
        }
        if failed {
            Err(Failure::Reported)
        } else {
            Ok(())
        }
    }

    /// `-S`: the Rust each C file becomes, formatted to be read and ready for
    /// `rustc --edition 2024` on its own — `<stem>.rs` in the working
    /// directory, or `-o` for a single file, `-o -` being standard output.
    fn emit_rust(&self) -> Result<(), Failure> {
        let mut failed = false;
        for (_, path, output) in outputs_each(self.inv, "-S", "rs")? {
            if output.as_os_str() != "-" {
                check_output(&output)?;
            }
            let translation = translate(path, &self.options)?;
            let (text, tally) = diag::render(&translation.map, &translation.diagnostics, self.inv);
            eprint!("{text}");
            let Some(items) = translation.items.filter(|_| tally.errors == 0) else {
                failed = true;
                continue;
            };
            let mut rust = format!(
                "//! Translated by ccinrs {} from `{}` for {}.\n//!\n\
                 //! `rustc --edition 2024` builds it as a program, whose entry point is\n\
                 //! the C `main`, and `--crate-type lib` as a library.\n\
                 #![no_main]\n\n{}",
                env!("CARGO_PKG_VERSION"),
                path.display(),
                self.target.triple,
                print::readable(items),
            );
            // The code names the runtime `::cinrs::rt`, as it would in a `c99!`
            // block; here the file is that crate, and carries the module.
            if translation.uses_runtime {
                rust.push_str("\nextern crate self as cinrs;\n\n");
                rust.push_str(&runtime::source());
            }
            if output.as_os_str() == "-" {
                print!("{rust}");
            } else {
                std::fs::write(&output, rust)
                    .map_err(|error| format!("cannot write {}: {error}", output.display()))?;
            }
            if let Some((target, file)) = self.deps_beside(path, Some(&output)) {
                write_rule(
                    &file,
                    &deps::rule(
                        &self.inv.deps,
                        &target,
                        path,
                        &translation.headers,
                        &translation.embedded,
                    ),
                )?;
            }
        }
        if failed {
            Err(Failure::Reported)
        } else {
            Ok(())
        }
    }

    /// `-E`, `-M` and `-MM`: the preprocessed text of every C file, or the
    /// rule naming what each one read, to `-o` or to standard output. Under
    /// `-MD` the rule goes to its `.d` file as well.
    fn preprocess(&self) -> Result<(), Failure> {
        let inv = self.inv;
        let mut out = Vec::new();
        let mut failed = false;
        unused_linker_inputs(inv)?;
        for input in &inv.inputs {
            let Input::C(path) = input else {
                continue;
            };
            let pre = if path.as_os_str() == STDIN {
                cinrs_core::preprocess_source(STDIN_NAME.to_owned(), read_stdin()?, &self.options)
            } else {
                cinrs_core::preprocess_file(path, &self.options)?
            };
            let (text, tally) = diag::render(&pre.map, &pre.diagnostics, inv);
            eprint!("{text}");
            if tally.errors > 0 {
                failed = true;
                continue;
            }
            let rule =
                |target: &str| deps::rule(&inv.deps, target, path, &pre.headers, &pre.embedded);
            if inv.deps.only {
                // `-M` makes the rule the output, which `-MF` sends elsewhere.
                let rule = rule(&deps::object_name(path));
                match &inv.deps.file {
                    Some(file) => write_rule(file, &rule)?,
                    None => out.extend_from_slice(rule.as_bytes()),
                }
                continue;
            }
            if inv.dump_macros {
                out.extend(preprocess::render_macros(&pre));
            } else {
                out.extend(preprocess::render(&pre, inv.line_markers));
            }
            if let Some((target, file)) = self.deps_beside(path, None) {
                write_rule(&file, &rule(&target))?;
            }
        }
        match &inv.output {
            Some(file) if file.as_os_str() != "-" => std::fs::write(file, &out)
                .map_err(|error| format!("cannot write {}: {error}", file.display()))?,
            _ => {
                use std::io::Write;
                std::io::stdout()
                    .write_all(&out)
                    .map_err(|error| format!("cannot write the output: {error}"))?;
            }
        }
        if failed {
            Err(Failure::Reported)
        } else {
            Ok(())
        }
    }

    /// Under `-MD` or `-MMD`, the target and the file of the rule for the C
    /// file at `source`, which this command line makes into `made` (an
    /// object for `-c`, a `.rs` for `-S`) or into a program. GCC's names: the
    /// `.d` beside what is made, or for a program `<program>-<stem>.d`, or
    /// `<stem>.d` in the working directory — unless `-MF` names it.
    fn deps_beside(&self, source: &Path, made: Option<&Path>) -> Option<(String, PathBuf)> {
        let deps = &self.inv.deps;
        if !deps.beside {
            return None;
        }
        let stem = source.file_stem().unwrap_or_default().to_string_lossy();
        let (target, beside) = match made {
            Some(made) => (made.display().to_string(), made.with_extension("d")),
            None => {
                let beside = match (&self.inv.output, self.inv.stage) {
                    (Some(program), Stage::Link) => {
                        PathBuf::from(format!("{}-{stem}.d", program.display()))
                    }
                    _ => PathBuf::from(format!("{stem}.d")),
                };
                (deps::object_name(source), beside)
            }
        };
        Some((target, deps.file.clone().unwrap_or(beside)))
    }
}

/// Refuses an output whose directory is not there, in GCC's words, before
/// any work goes into making it — `rustc` would say it with a temporary
/// directory's name.
fn check_output(path: &Path) -> Result<(), String> {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() && !dir.is_dir() => Err(format!(
            "cannot open output file {}: the directory '{}' does not exist",
            path.display(),
            dir.display()
        )),
        _ => Ok(()),
    }
}

/// Every symbol name and pattern the version scripts a command line gives the
/// linker (`-Wl,--version-script,FILE`, `-Wl,--version-script=FILE`) mention,
/// in a `global:` or a `local:` list alike. A script that cannot be read is
/// the linker's to report.
fn version_script_patterns(linker_args: &[LinkerArg]) -> Vec<String> {
    let mut files = Vec::new();
    let mut words = linker_args.iter().filter_map(|arg| match arg {
        LinkerArg::Linker(word) => Some(word),
        LinkerArg::Driver(_) => None,
    });
    while let Some(word) = words.next() {
        let option = word.trim_start_matches('-');
        if let Some(file) = option.strip_prefix("version-script=") {
            files.push(file.to_owned());
        } else if option == "version-script"
            && let Some(file) = words.next()
        {
            files.push(file.clone());
        }
    }
    let mut patterns = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        // Comments out, then every word that is not a keyword or the name of
        // a version node (the word before a `{`).
        let mut plain = String::new();
        let mut rest = text.as_str();
        while let Some(start) = rest.find("/*") {
            plain.push_str(&rest[..start]);
            rest = rest[start..]
                .find("*/")
                .map_or("", |end| &rest[start + end + 2..]);
        }
        plain.push_str(rest);
        let plain: String = plain
            .lines()
            .map(|line| line.split('#').next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        let spaced = plain
            .replace('{', " { ")
            .replace('}', " } ")
            .replace(';', " ; ");
        let words: Vec<&str> = spaced.split_whitespace().collect();
        for (index, word) in words.iter().enumerate() {
            let word = word.trim_end_matches(':');
            let is_node = words.get(index + 1) == Some(&"{");
            if is_node
                || matches!(
                    word,
                    "{" | "}" | ";" | "global" | "local" | "extern" | "\"C\""
                )
            {
                continue;
            }
            patterns.push(word.to_owned());
        }
    }
    patterns
}

/// A version script with each wildcard of a `global:` list — libjpeg-turbo's
/// `LIBJPEG_6.2 { global: *; };` — replaced by the C symbols it matches, or
/// `None` when the script has no such wildcard.
///
/// In a library GCC links, `*` is the C symbols; in one `rustc` links it is
/// Rust's standard library and ccinrs's marks as well, some two thousand
/// symbols nothing should bind to. Everything else is left as written: the
/// names, the `local:` lists, the nodes and their order, `extern "C++"`
/// blocks, the comments.
fn narrow_wildcards(text: &str, symbols: &[String]) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    let mut global = false;
    let mut extern_depth = 0usize;
    let mut extern_next = false;
    let mut drop_semicolon = false;
    let mut i = 0;
    while i < bytes.len() {
        let rest = &text[i..];
        let end = if rest.starts_with("/*") {
            rest.find("*/").map_or(rest.len(), |at| at + 2)
        } else if rest.starts_with('#') {
            rest.find('\n').unwrap_or(rest.len())
        } else if let Some(quoted) = rest.strip_prefix('"') {
            quoted.find('"').map_or(rest.len(), |at| at + 2)
        } else if rest.starts_with(|c: char| c.is_whitespace() || "{};:".contains(c)) {
            rest.chars().next().map_or(1, char::len_utf8)
        } else {
            rest.find(|c: char| c.is_whitespace() || "{};:\"#".contains(c) || c == '/')
                .filter(|&at| at > 0)
                .unwrap_or(rest.len())
        };
        let token = &rest[..end];
        i += end;
        match token {
            ";" if drop_semicolon => {
                drop_semicolon = false;
                continue;
            }
            "{" if extern_next => {
                extern_next = false;
                extern_depth += 1;
            }
            "}" if extern_depth > 0 => extern_depth -= 1,
            "}" => global = false,
            "global" => global = true,
            "local" => global = false,
            "extern" => extern_next = true,
            _ if global
                && extern_depth == 0
                && token.contains(['*', '?', '['])
                && !token.starts_with("/*") =>
            {
                let names: Vec<&str> = symbols
                    .iter()
                    .filter(|symbol| glob_matches(token, symbol))
                    .map(String::as_str)
                    .collect();
                changed = true;
                if names.is_empty() {
                    drop_semicolon = true;
                } else {
                    out.push_str(&names.join(";\n    "));
                }
                continue;
            }
            _ => {}
        }
        out.push_str(token);
    }
    changed.then_some(out)
}

/// Whether `name` matches a version script's pattern, whose `*` and `?` are
/// a shell's.
fn glob_matches(pattern: &str, name: &str) -> bool {
    fn matches(p: &[u8], n: &[u8]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some(b'*'), _) => matches(&p[1..], n) || (!n.is_empty() && matches(p, &n[1..])),
            (Some(b'?'), Some(_)) => matches(&p[1..], &n[1..]),
            (Some(a), Some(b)) if a == b => matches(&p[1..], &n[1..]),
            _ => false,
        }
    }
    matches(pattern.as_bytes(), name.as_bytes())
}

/// What a panic does in a program or a library `ccinrs` links: Rust's own
/// message, and then `abort`.
///
/// A panic is a run-time check that failed, and in a C program nothing can
/// catch it. Every function is `extern "C-unwind"` — a `longjmp` is an unwind
/// — so without this a panic would unwind looking for a handler, find none
/// below `main`, and end with the unwinder's "failed to initiate panic, error
/// 5". The hook stops it where it happened instead, as a failed `assert` does.
/// A `longjmp` is unaffected: `resume_unwind` runs no hook.
///
/// It is installed by a constructor — `.init_array` on ELF, `__mod_init_func`
/// on Mach-O — because the C `main` is the program's, and a shared library
/// has none; one library loaded beside another wraps the hook it finds, which
/// aborts all the same.
const PANIC_HOOK: &str = r#"
#[cfg(any(target_os = "linux", target_os = "android", target_os = "freebsd",
          target_os = "netbsd", target_os = "openbsd", target_os = "dragonfly",
          target_vendor = "apple"))]
#[used]
#[cfg_attr(target_vendor = "apple", unsafe(link_section = "__DATA,__mod_init_func"))]
#[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = ".init_array"))]
static CCINRS_PANIC_HOOK: extern "C" fn() = {
    extern "C" fn install() {
        let report = ::std::panic::take_hook();
        ::std::panic::set_hook(::std::boxed::Box::new(move |info| {
            report(info);
            ::std::process::abort();
        }));
    }
    install
};
"#;

/// `malloc`, `calloc`, `realloc` and `free` for WebAssembly with no C library,
/// as Rust in the module the objects are linked into: Rust's own allocator,
/// each block with its size in a header of `max_align_t`'s sixteen bytes in
/// front of it, which `free` and `realloc` read back. A host allocates
/// through them too, which is how it hands the C a buffer. One the C defines
/// itself is the C's, and is left out.
fn bare_wasm_allocator(defined: &[String]) -> String {
    const HELPERS: &str = "
// `malloc` and its family: WebAssembly with no system under it has no C
// library, and these are Rust's allocator. A block's size is kept in the
// sixteen bytes in front of it.
const HEADER: usize = 16;

unsafe fn ccinrs_allocate(size: usize, zeroed: bool) -> *mut u8 {
    let Some(layout) = size
        .checked_add(HEADER)
        .and_then(|total| std::alloc::Layout::from_size_align(total, HEADER).ok())
    else {
        return core::ptr::null_mut();
    };
    unsafe {
        let base = if zeroed {
            std::alloc::alloc_zeroed(layout)
        } else {
            std::alloc::alloc(layout)
        };
        if base.is_null() {
            return base;
        }
        base.cast::<usize>().write(size);
        base.add(HEADER)
    }
}

unsafe fn ccinrs_block(ptr: *mut u8) -> (*mut u8, std::alloc::Layout) {
    unsafe {
        let base = ptr.sub(HEADER);
        let size = base.cast::<usize>().read();
        (base, std::alloc::Layout::from_size_align_unchecked(size + HEADER, HEADER))
    }
}
";
    const FUNCTIONS: [(&str, &str); 4] = [
        (
            "malloc",
            "
#[unsafe(no_mangle)]
pub unsafe extern \"C\" fn malloc(size: usize) -> *mut u8 {
    unsafe { ccinrs_allocate(size, false) }
}
",
        ),
        (
            "calloc",
            "
#[unsafe(no_mangle)]
pub unsafe extern \"C\" fn calloc(count: usize, size: usize) -> *mut u8 {
    match count.checked_mul(size) {
        Some(total) => unsafe { ccinrs_allocate(total, true) },
        None => core::ptr::null_mut(),
    }
}
",
        ),
        (
            "realloc",
            "
#[unsafe(no_mangle)]
pub unsafe extern \"C\" fn realloc(ptr: *mut u8, size: usize) -> *mut u8 {
    if ptr.is_null() {
        return unsafe { ccinrs_allocate(size, false) };
    }
    let Some(total) = size.checked_add(HEADER) else {
        return core::ptr::null_mut();
    };
    unsafe {
        let (base, layout) = ccinrs_block(ptr);
        let base = std::alloc::realloc(base, layout, total);
        if base.is_null() {
            return base;
        }
        base.cast::<usize>().write(size);
        base.add(HEADER)
    }
}
",
        ),
        (
            "free",
            "
#[unsafe(no_mangle)]
pub unsafe extern \"C\" fn free(ptr: *mut u8) {
    if !ptr.is_null() {
        unsafe {
            let (base, layout) = ccinrs_block(ptr);
            std::alloc::dealloc(base, layout);
        }
    }
}
",
        ),
    ];
    let mut text = HELPERS.to_owned();
    for (name, function) in FUNCTIONS {
        if !defined.iter().any(|symbol| symbol == name) {
            text.push_str(function);
        }
    }
    text
}

/// The libraries a glibc program links by name that musl keeps in libc.a.
const MUSL_LIBC_PARTS: &[&str] = &[
    "c", "m", "pthread", "dl", "rt", "util", "crypt", "xnet", "resolv",
];

/// The input name that means standard input, as GCC takes it.
const STDIN: &str = "-";

/// What the diagnostics and `__FILE__` call standard input, as in GCC.
const STDIN_NAME: &str = "<stdin>";

/// Translates the C file at `path`, or standard input for `-`.
fn translate(path: &Path, options: &Options) -> Result<cinrs_core::FileTranslation, String> {
    if path.as_os_str() == STDIN {
        return Ok(cinrs_core::translate_source(
            STDIN_NAME.to_owned(),
            read_stdin()?,
            options,
        ));
    }
    cinrs_core::translate_file(path, options)
}

/// Standard input, read to its end as a C file is read.
fn read_stdin() -> Result<String, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read standard input: {error}"))?;
    Ok(cinrs_core::lex::decode_source(bytes))
}

/// Writes a dependency rule.
fn write_rule(file: &Path, rule: &str) -> Result<(), String> {
    std::fs::write(file, rule).map_err(|error| format!("cannot write {}: {error}", file.display()))
}

/// The files for the linker on a command line that stops before the link,
/// reported as unused, as GCC reports them — and one that does not exist is an
/// error, as in GCC 15: CMake's test of whether `/W4` is a compiler option
/// compiles a file with it, and an MSVC option must not pass for one.
fn unused_linker_inputs(inv: &Invocation) -> Result<(), String> {
    let mut missing = None;
    for input in &inv.inputs {
        if let Input::Linker(path) = input {
            if inv.warnings {
                eprintln!(
                    "ccinrs: warning: {}: linker input file unused because linking not done",
                    path.display()
                );
            }
            if missing.is_none() && std::fs::metadata(path).is_err() {
                missing = Some(path);
            }
        }
    }
    match missing {
        Some(path) => Err(format!(
            "{}: linker input file not found: No such file or directory",
            path.display()
        )),
        None => Ok(()),
    }
}

/// The C files of a command line that stops before the link (`flag` is `-c`
/// or `-S`), each with its index and the file it becomes: `-o` for a single
/// one, `<stem>.<extension>` in the working directory otherwise, as GCC names
/// them. A file for the linker is reported as unused, as GCC reports it.
fn outputs_each<'a>(
    inv: &'a Invocation,
    flag: &str,
    extension: &str,
) -> Result<Vec<(usize, &'a Path, PathBuf)>, String> {
    let sources: Vec<(usize, &Path)> = inv
        .inputs
        .iter()
        .enumerate()
        .filter_map(|(index, input)| match input {
            Input::C(path) => Some((index, path.as_path())),
            Input::Linker(_) => None,
        })
        .collect();
    if inv.output.is_some() && sources.len() > 1 {
        return Err(format!(
            "cannot specify '-o' with '{flag}' and more than one C file"
        ));
    }
    unused_linker_inputs(inv)?;
    Ok(sources
        .into_iter()
        .map(|(index, path)| {
            let output = inv.output.clone().unwrap_or_else(|| {
                let stem = path.file_stem().unwrap_or_default();
                PathBuf::from(stem).with_extension(extension)
            });
            (index, path, output)
        })
        .collect())
}

impl Run<'_> {
    /// Translates and compiles one C file into `object`, answering whether
    /// the object calls the runtime — or prints why it could not and answers
    /// `None`. `deps` is the target and the file of a dependency rule to
    /// write once it has compiled; see [`Run::deps_beside`].
    fn compile(
        &self,
        index: usize,
        path: &Path,
        object: &Path,
        deps: Option<(String, PathBuf)>,
    ) -> Result<Option<ObjectInfo>, Failure> {
        let inv = self.inv;
        check_output(object)?;
        let translation = translate(path, &self.options)?;
        let (text, tally) = diag::render(&translation.map, &translation.diagnostics, inv);
        eprint!("{text}");
        // An error, or a warning under `-Werror`.
        let Some(items) = translation.items.filter(|_| tally.errors == 0) else {
            debug_assert!(tally.errors > 0);
            return Ok(None);
        };
        // Under the linker's `-flto`, a unit that makes a real weak
        // definition is compiled without it: the definition is an alias the
        // assembler makes, and the optimised program is free to put two
        // units' in one module — two definitions of one symbol, and an error
        // where the linker would have taken either. The linker links the
        // object beside the optimised ones, so nothing changes for the
        // program but how much is inlined across this file; `-v` says so.
        let lto = inv
            .lto
            .filter(|_| !(translation.weak_definitions && self.target.linker_lto(inv)));
        if inv.verbose && inv.lto.is_some() && lto.is_none() {
            eprintln!(
                "ccinrs: note: {} makes a weak definition, so it is compiled without -flto and \
                 linked beside the optimised files",
                path.display()
            );
        }
        let bitcode = lto.is_some() && self.target.linker_lto(inv);
        let stem = self.work.stem(index, path);
        let rust = self.work.path(&format!("{stem}.rs"));
        // The Rust is laid out on the C's lines, so nothing may come before
        // it; see `print`. The stamp goes into the object as bytes no
        // optimisation removes, so that a link can tell which `rustc` made it,
        // and so do the mark of an object that needs the runtime and the
        // symbols it defines, so that a link can tell it needs the one and a
        // shared library what to export; see `check_stamp`.
        let mut source = format!(
            "{}// Generated by ccinrs from {}; not meant to be edited.\n{}",
            print::by_line(print::in_source_order(items)),
            path.display(),
            used_bytes("CCINRS_OBJECT", &self.stamp),
        );
        if translation.uses_runtime {
            source.push_str(&used_bytes("CCINRS_RUNTIME", RUNTIME_MARK));
        }
        if !translation.symbols.is_empty() {
            source.push_str(&used_bytes(
                "CCINRS_SYMBOLS",
                &format!("{SYMBOLS_PREFIX}{}\0", translation.symbols.join(" ")),
            ));
        }
        // Under `-flto` the object is an rlib — the crate's code, and the
        // LLVM bitcode `rustc` optimises across crates — which a link loads
        // as a crate, by the name it carries.
        let made = if object.starts_with(self.work.dir()) {
            index.to_string()
        } else {
            std::path::absolute(object)
                .unwrap_or_else(|_| object.to_path_buf())
                .display()
                .to_string()
        };
        let name = crate_name(&stem, path, &made);
        if lto.is_some() {
            source.push_str(&used_bytes(
                "CCINRS_CRATE",
                &format!("{CRATE_PREFIX}{name}\0"),
            ));
        }
        if bitcode {
            source.push_str(&used_bytes("CCINRS_BITCODE", BITCODE_MARK));
        }
        std::fs::write(&rust, source)
            .map_err(|error| format!("cannot write {}: {error}", rust.display()))?;
        let mut cmd = self.rustc.command();
        cmd.args(["--edition", "2024"]);
        if lto.is_some() {
            cmd.args(["--crate-type", "rlib"]);
        } else {
            cmd.args(["--crate-type", "lib", "--emit", "obj"]);
        }
        // The crate's object is the bitcode alone, for the linker.
        if bitcode {
            cmd.args(["-C", "linker-plugin-lto"]);
        }
        cmd.args(["--crate-name", &name])
            .args(codegen_flags(inv))
            .args(["-C", "codegen-units=1", "--cap-lints", "allow"])
            // What `rustc` says about the `.rs` — a panic's location among it
            // — names the C file, whose lines the `.rs` keeps.
            .arg(format!(
                "--remap-path-prefix={}={}",
                rust.display(),
                path.display()
            ));
        if translation.uses_runtime {
            cmd.args(runtime::extern_args(self.runtime()?));
        }
        let written = if object.starts_with(self.work.dir()) {
            object.to_path_buf()
        } else {
            self.work.path(&format!("{stem}.o"))
        };
        cmd.arg("-o").arg(&written).arg(&rust);
        run_rustc(inv, cmd, || {
            format!(
                "rustc rejected the translation of {}; this is a bug in ccinrs (the Rust is in {})",
                path.display(),
                rust.display()
            )
        })?;
        if written != object {
            deliver(&written, object)?;
        }
        if let Some((target, file)) = deps {
            let rule = deps::rule(
                &inv.deps,
                &target,
                path,
                &translation.headers,
                &translation.embedded,
            );
            write_rule(&file, &rule)?;
        }
        Ok(Some(ObjectInfo {
            uses_runtime: translation.uses_runtime,
            symbols: translation.symbols,
            crates: lto.map(|_| name).into_iter().collect(),
            native: lto.is_none(),
            bitcode,
        }))
    }

    /// `-shared` makes an ELF shared library: what decides what it exports is
    /// a version script, which only an ELF linker reads. And not for musl,
    /// whose C library Rust ships only as an archive to link in.
    fn check_shared(&self) -> Result<(), String> {
        let elf = matches!(
            self.target.model.os,
            Os::Linux | Os::FreeBsd | Os::NetBsd | Os::OpenBsd
        ) && self.target.model.arch != Arch::Wasm32;
        if !elf {
            return Err(format!(
                "'-shared' makes ELF shared libraries, and {} is not an ELF target",
                self.target.triple
            ));
        }
        if self.target.self_contained {
            return Err(format!(
                "'-shared' needs a shared C library to link against, and Rust's {} \
                 has musl only as an archive (libc.a)",
                self.target.triple
            ));
        }
        Ok(())
    }

    /// The command line's linker words, each version script with a wildcard
    /// in a `global:` list replaced by a narrowed copy in the work directory.
    fn narrowed_scripts(
        &self,
        args: &[LinkerArg],
        symbols: &[String],
    ) -> Result<Vec<LinkerArg>, String> {
        let mut out = Vec::with_capacity(args.len());
        let mut script_next = false;
        for arg in args {
            let LinkerArg::Linker(word) = arg else {
                out.push(arg.clone());
                continue;
            };
            let option = word.trim_start_matches('-');
            // The script's file, and what comes before it in the word.
            let file = if std::mem::take(&mut script_next) {
                Some(("", word.as_str()))
            } else if let Some(file) = option.strip_prefix("version-script=") {
                Some((&word[..word.len() - file.len()], file))
            } else {
                script_next = option == "version-script";
                None
            };
            let narrowed = file.and_then(|(lead, file)| {
                let text = std::fs::read_to_string(file).ok()?;
                Some((lead, narrow_wildcards(&text, symbols)?))
            });
            match narrowed {
                Some((lead, text)) => {
                    let path = self.work.path(&format!("script-{}.map", out.len()));
                    std::fs::write(&path, text)
                        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
                    out.push(LinkerArg::Linker(format!("{lead}{}", path.display())));
                }
                None => out.push(arg.clone()),
            }
        }
        Ok(out)
    }

    /// One linker argument of the command line, as `rustc` is to pass it on.
    /// Through a C compiler, in its spelling — `-Wl,word`, or `-Xlinker word`
    /// for a word with a comma in it; to `rust-lld` itself (musl), the word
    /// alone, and `-rdynamic` as `--export-dynamic`.
    fn linker_arg(&self, arg: &LinkerArg) -> Vec<String> {
        match arg {
            LinkerArg::Driver(option) if self.target.self_contained && option == "-rdynamic" => {
                vec!["--export-dynamic".to_owned()]
            }
            LinkerArg::Driver(option) => vec![option.clone()],
            LinkerArg::Linker(word) if self.target.self_contained => vec![word.clone()],
            LinkerArg::Linker(word) if word.contains(',') => {
                vec!["-Xlinker".to_owned(), word.clone()]
            }
            LinkerArg::Linker(word) => vec![format!("-Wl,{word}")],
        }
    }

    /// The compiled runtime, found or compiled on first asking.
    fn runtime(&self) -> Result<&Path, Failure> {
        self.runtime
            .library(self.inv, &self.rustc, &self.target.triple, self.work.dir())
    }
}

/// `#[used] static NAME: [u8; N] = *b"…";`, which keeps `bytes` in the object
/// whatever the optimisation.
fn used_bytes(name: &str, bytes: &str) -> String {
    format!(
        "#[used]\nstatic {name}: [u8; {}] = *b\"{}\";\n",
        bytes.len(),
        bytes
            .bytes()
            .flat_map(std::ascii::escape_default)
            .map(char::from)
            .collect::<String>(),
    )
}

/// What an object that calls the runtime carries besides its stamp.
const RUNTIME_MARK: &str = "ccinrs runtime wanted\0";

/// What an `-flto` object made of bitcode for the linker carries; see
/// [`Target::linker_lto`].
const BITCODE_MARK: &str = "ccinrs bitcode for the linker\0";

/// How the list of the symbols an object defines begins: the names follow,
/// separated by spaces, up to a NUL.
const SYMBOLS_PREFIX: &str = "ccinrs symbols: ";

/// How an `-flto` object says which crate it is: the name follows, up to a
/// NUL.
const CRATE_PREFIX: &str = "ccinrs crate: ";

/// What every object this run makes carries: the `rustc` that compiled it,
/// down to the commit, and the target.
///
/// An object names the standard library's symbols as that one build mangles
/// them, so it links only with that build's standard library. Another
/// `rustc` would fail the link with hundreds of undefined symbols that say
/// nothing about why; the stamp turns that into one sentence.
fn stamp(rustc: &Rustc, target: &Target) -> String {
    format!(
        "{STAMP_PREFIX}rustc {} ({}) for {}\0",
        rustc.release, rustc.commit, target.triple
    )
}

/// How a stamp begins; see [`stamp`].
const STAMP_PREFIX: &str = "ccinrs object: ";

/// Refuses a file that holds an object from another `rustc` or for another
/// target — looked for as bytes, whatever the object format, and in every
/// member of an archive — and answers what its objects say about themselves:
/// whether one calls the runtime, and the symbols they define. A file with
/// no stamp is not ccinrs's, and is passed on as it is.
fn check_stamp(path: &Path, stamp: &str) -> Result<ObjectInfo, String> {
    let Ok(bytes) = std::fs::read(path) else {
        // Missing or unreadable: the linker says so in its own words.
        return Ok(ObjectInfo::default());
    };
    let mark = RUNTIME_MARK.as_bytes();
    let uses_runtime = bytes.windows(mark.len()).any(|w| w == mark);
    // What the objects made here say they define, and what the others' — an
    // object from GCC or an assembler — symbol tables do.
    let mut symbols = marked_names(&bytes, SYMBOLS_PREFIX);
    symbols.extend(elf::exported_symbols(&bytes, STAMP_PREFIX.as_bytes()));
    let crates = marked_names(&bytes, CRATE_PREFIX);
    let prefix = STAMP_PREFIX.as_bytes();
    let stamped = bytes.windows(prefix.len()).any(|w| w == prefix);
    let mut rest = &bytes[..];
    while let Some(at) = rest.windows(prefix.len()).position(|w| w == prefix) {
        let found = &rest[at..];
        let end = found
            .iter()
            .position(|b| *b == 0)
            .map_or(found.len(), |i| i + 1);
        if &found[..end] != stamp.as_bytes() {
            let theirs = String::from_utf8_lossy(&found[prefix.len()..end]);
            let ours = &stamp[prefix.len()..];
            return Err(format!(
                "{} was compiled by ccinrs with {}, and this link uses {}; \
                 compile it again with this rustc",
                path.display(),
                theirs.trim_end_matches('\0'),
                ours.trim_end_matches('\0'),
            ));
        }
        rest = &found[end..];
    }
    let bitcode_mark = BITCODE_MARK.as_bytes();
    Ok(ObjectInfo {
        uses_runtime,
        symbols,
        native: stamped && crates.is_empty(),
        bitcode: !crates.is_empty() && bytes.windows(bitcode_mark.len()).any(|w| w == bitcode_mark),
        crates,
    })
}

/// The names every `prefix` mark in `bytes` lists — separated by spaces, up
/// to a NUL — each once: an `-flto` object carries its marks twice, in its
/// code and in its bitcode.
fn marked_names(bytes: &[u8], prefix: &str) -> Vec<String> {
    let prefix = prefix.as_bytes();
    let mut names: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut rest = bytes;
    while let Some(at) = rest.windows(prefix.len()).position(|w| w == prefix) {
        let list = &rest[at + prefix.len()..];
        let end = list.iter().position(|b| *b == 0).unwrap_or(list.len());
        for name in String::from_utf8_lossy(&list[..end]).split(' ') {
            if !name.is_empty() && seen.insert(name.to_owned()) {
                names.push(name.to_owned());
            }
        }
        rest = &list[end..];
    }
    names
}

impl Run<'_> {
    /// Links the objects, in command-line order, into `output` — with the
    /// runtime when one of them calls it — as a program, or under `-shared`
    /// as a shared library.
    fn link(
        &self,
        objects: &[LinkInput],
        linked: &ObjectInfo,
        output: &Path,
    ) -> Result<(), Failure> {
        let inv = self.inv;
        // WebAssembly with no system under it has no programs, only modules a
        // host calls into, so that is what is made of the objects, `-shared`
        // or not; see `bare_wasm_allocator`.
        let bare = self.target.bare_wasm;
        let library = inv.shared || bare;
        if inv.shared && !bare {
            self.check_shared()?;
        }
        // A build's own version script, with its wildcards narrowed to the C
        // symbols where it has any; see `narrow_wildcards`.
        let linker_args = if inv.shared && !bare {
            self.narrowed_scripts(&inv.linker_args, &linked.symbols)?
        } else {
            inv.linker_args.clone()
        };
        let stub = self.work.path("ccinrs_main.rs");
        let mut text = if library {
            "// A shared library of the C objects linked into it.\n"
        } else {
            "// The program's `main` is the C one, in one of the objects.\n#![no_main]\n"
        }
        .to_owned();
        if bare {
            text.push_str(&bare_wasm_allocator(&linked.symbols));
        }
        // WebAssembly's Rust aborts on a panic already.
        if self.target.model.arch != Arch::Wasm32 {
            text.push_str(PANIC_HOOK);
        }
        // A crate is linked when it is loaded, and these load the runtime and
        // each `-flto` object, under the name `rustc` finds it by.
        if linked.uses_runtime {
            text.push_str("extern crate cinrs as _;\n");
        }
        let mut crates = Vec::new();
        for object in objects {
            let Some(name) = &object.crate_name else {
                continue;
            };
            let rlib = self.work.path(&format!("lib{name}.rlib"));
            if std::fs::hard_link(&object.path, &rlib).is_err() {
                std::fs::copy(&object.path, &rlib)
                    .map_err(|error| format!("cannot write {}: {error}", rlib.display()))?;
            }
            text.push_str(&format!("extern crate {name} as _;\n"));
            crates.push(format!("{name}={}", rlib.display()));
        }
        std::fs::write(&stub, text)
            .map_err(|error| format!("cannot write {}: {error}", stub.display()))?;
        let mut cmd = self.rustc.command();
        cmd.args(["--edition", "2024", "--crate-type"])
            .arg(if library { "cdylib" } else { "bin" })
            .args(["--crate-name", "ccinrs_main"])
            .args(codegen_flags(inv))
            .args(["--cap-lints", "allow"]);
        if linked.uses_runtime {
            cmd.args(runtime::extern_args(self.runtime()?));
        }
        for extern_crate in &crates {
            cmd.arg("--extern").arg(extern_crate);
        }
        // `-flto` at the link is what optimises them all as one. Where the
        // objects are bitcode for the linker, `rust-lld` does it, with every
        // other object in view; see `Target::linker_lto`. The inlining
        // thresholds the checks raise (see `codegen_flags`) are the linker's
        // LLVM's to apply then.
        //
        // Otherwise it is `rustc`'s, which optimises the C, the runtime and
        // Rust's standard library alike — but not when an object or an
        // archive compiled here without it is in the link: `rustc` keeps only
        // the standard library's symbols its own modules use, and the machine
        // code would be left calling `core::panicking::panic` and the rest by
        // names nothing defines. The `-flto` objects hold machine code too,
        // so they link as they are.
        match inv.lto {
            _ if linked.bitcode => {
                cmd.args(["-C", "linker-plugin-lto"]);
                if inv.checks && inv.opt_level != "0" {
                    for threshold in [
                        "-inlinehint-threshold=1000",
                        "-inline-cold-callsite-threshold=225",
                    ] {
                        if self.target.self_contained {
                            cmd.args(["-C", "link-arg=-mllvm"])
                                .arg("-C")
                                .arg(format!("link-arg={threshold}"));
                        } else {
                            cmd.arg("-C")
                                .arg(format!("link-arg=-Wl,-mllvm,{threshold}"));
                        }
                    }
                }
            }
            _ if linked.native => {}
            Some(Lto::Fat) => {
                cmd.args(["-C", "lto=fat"]);
            }
            Some(Lto::Thin) => {
                cmd.args(["-C", "lto=thin"]);
            }
            None => {}
        }
        // `rustc` gives a shared library a version script that exports what
        // the Rust in it exports, which here is nothing: a second one names
        // the C symbols, and the linker takes the union. Each object goes in
        // whole, as it would into GCC's shared library, whether or not
        // anything in the link refers to it.
        let mut modifiers = "+verbatim";
        // The module exports every C symbol, and what the C calls and no
        // object defines is imported, from the `env` module the host gives.
        if bare {
            cmd.args(["-C", "link-arg=--allow-undefined"]);
            for symbol in &linked.symbols {
                cmd.arg("-C").arg(format!("link-arg=--export={symbol}"));
            }
        }
        if inv.shared && !bare {
            modifiers = "+verbatim,+whole-archive";
            // `rustc` asks the linker to refuse a version script naming a
            // symbol the library does not define; a C compiler's linker lets
            // it be, and a build's own script (zlib's `zlib.map`, given to
            // its test of whether shared libraries work) counts on that.
            cmd.args(["-C", "link-arg=-Wl,--undefined-version"]);
            // (Only an ELF target that links through a C compiler gets here;
            // see `check_shared`.)
            // A build that gives its own version script decides about every
            // symbol it names — zlib's gives some `ZLIB_1.2.0` and the like,
            // and hides `_*` — and naming one here too would take that away.
            // What it does not mention, GCC's linker exports, and so does
            // this; `rustc`'s own script would hide it.
            let patterns = version_script_patterns(&linker_args);
            let exported: Vec<&String> = linked
                .symbols
                .iter()
                .filter(|symbol| !patterns.iter().any(|p| glob_matches(p, symbol)))
                .collect();
            if !exported.is_empty() {
                let script = self.work.path("exports.map");
                let mut text = "{\n  global:\n".to_owned();
                for symbol in exported {
                    text.push_str(&format!("    {symbol};\n"));
                }
                text.push_str("};\n");
                std::fs::write(&script, text)
                    .map_err(|error| format!("cannot write {}: {error}", script.display()))?;
                cmd.arg("-C").arg(format!(
                    "link-arg=-Wl,--version-script={}",
                    script.display()
                ));
            }
        }
        if inv.static_link {
            cmd.args(["-C", "target-feature=+crt-static"]);
        }
        // The objects go in as `-l static:+verbatim`, which puts them on the
        // link line ahead of the standard library they refer to — and of the
        // runtime — which is the order a linker that reads its inputs once
        // needs. A verbatim name is a file name in a `-L` directory, which is
        // why every object is in the work directory.
        //
        // WebAssembly's linker is `wasm-ld`, to which `rustc` hands a verbatim
        // name as a plain `-l` it then cannot find; it is LLD, and LLD does not
        // care about the order, so there the objects go in as they are, by
        // path.
        cmd.arg("-L").arg(self.work.dir());
        for object in objects {
            if object.crate_name.is_some() {
                continue;
            }
            let whole = object.whole;
            let object = &object.path;
            if self.target.model.arch == Arch::Wasm32 {
                if whole {
                    cmd.args(["-C", "link-arg=--whole-archive"]);
                }
                cmd.arg("-C").arg(format!("link-arg={}", object.display()));
                if whole {
                    cmd.args(["-C", "link-arg=--no-whole-archive"]);
                }
                continue;
            }
            if is_shared_library(object) {
                cmd.arg("-C").arg(format!("link-arg={}", object.display()));
                continue;
            }
            let name = object
                .file_name()
                .expect("an object in the work directory has a name")
                .to_string_lossy();
            let spec = if is_archive(object) && whole {
                format!("static:+verbatim,+whole-archive={name}")
            } else if is_archive(object) {
                format!("static:+verbatim={name}")
            } else {
                format!("static:{modifiers}={name}")
            };
            cmd.arg("-l").arg(spec);
        }
        for dir in &inv.lib_dirs {
            cmd.arg("-L").arg(format!("native={}", dir.display()));
        }
        for lib in &inv.libs {
            // musl keeps the maths library, threads and the rest in libc.a
            // itself, and ships no separate archives for them.
            if self.target.self_contained && MUSL_LIBC_PARTS.contains(&lib.as_str()) {
                continue;
            }
            // GNU ld's `-l:file`: that file, looked for in the `-L`
            // directories, which is `rustc`'s verbatim name.
            match lib.strip_prefix(':') {
                Some(file) if file.ends_with(".a") => {
                    cmd.arg("-l").arg(format!("static:+verbatim={file}"))
                }
                Some(file) => cmd.arg("-l").arg(format!("dylib:+verbatim={file}")),
                None => cmd.arg("-l").arg(lib),
            };
        }
        for arg in &linker_args {
            for arg in self.linker_arg(arg) {
                cmd.arg("-C").arg(format!("link-arg={arg}"));
            }
        }
        if self.target.self_contained {
            cmd.args(["-C", "linker=rust-lld", "-C", "linker-flavor=ld.lld"]);
        } else if let Some(ld) = inv.fuse_ld.as_ref().filter(|ld| *ld != "lld")
            && (inv.shared || linked.bitcode)
        {
            // `rustc` gives a shared library a version script of its own, and
            // the C symbols are a second; LLD takes the union of the two, and
            // GNU ld, gold and mold refuse two anonymous version tags. An
            // object compiled with `-flto` without `-fuse-ld` is bitcode, which
            // only LLD reads.
            if inv.warnings {
                let why = if linked.bitcode {
                    "an object compiled with -flto is LLVM bitcode, which LLD reads and it \
                     does not"
                } else {
                    "a shared library is linked by LLD, which alone takes rustc's version \
                     script and ccinrs's together"
                };
                eprintln!("ccinrs: warning: ignoring '-fuse-ld={ld}': {why}");
            }
        } else if !bare && let Some(ld) = &inv.fuse_ld {
            // `rustc` links x86-64 Linux with its own `rust-lld` through the C
            // compiler, and `-C linker-features=-lld` puts the platform's
            // back, which is what a build asks for with `-fuse-ld=bfd` — and
            // needs for an option only GNU ld has. Another linker is the C
            // compiler's `-fuse-ld` after that.
            let lld_default = self.target.triple == "x86_64-unknown-linux-gnu";
            if lld_default && ld != "lld" {
                cmd.args(["-C", "linker-features=-lld"]);
            }
            if ld != "bfd" && !(lld_default && ld == "lld") {
                cmd.arg("-C").arg(format!("link-arg=-fuse-ld={ld}"));
            }
        }
        if inv.strip {
            cmd.args(["-C", "strip=symbols"]);
        }
        let name = output
            .file_name()
            .map_or_else(|| "linked".into(), |name| name.to_string_lossy());
        let written = self.work.path(&format!("out-{name}"));
        cmd.arg("-o").arg(&written).arg(&stub);
        // A link that fails is the program's own problem — an undefined
        // symbol, a missing library — so what the linker said is the message.
        run_rustc(inv, cmd, || format!("linking {} failed", output.display()))?;
        deliver(&written, output)?;
        Ok(())
    }
}

/// Moves what `rustc` wrote in the work directory to where it was asked for.
///
/// `rustc -o dir/out` writes its intermediate objects beside the output, named
/// after its stem — `dir/out.ccinrs_main.<hash>-cgu.0.rcgu.o` — and deletes
/// them when it is done, so two runs in one directory whose outputs share a
/// stem delete each other's: `make -j` linking mbedtls's `test_suite_ecp` and
/// `test_suite_ecp.generated` at once fails now and then. In the work
/// directory, which is this run's own, nothing else is. The result is renamed
/// into place, or copied, permissions and all, from another file system.
fn deliver(written: &Path, wanted: &Path) -> Result<(), String> {
    if std::fs::rename(written, wanted).is_ok() {
        return Ok(());
    }
    // A program that is running cannot be written over, but it can be
    // unlinked, as a linker does.
    let _ = std::fs::remove_file(wanted);
    std::fs::copy(written, wanted)
        .map(|_| ())
        .map_err(|error| format!("cannot write {}: {error}", wanted.display()))
}

/// The `-C` flags every `rustc` run gets.
fn codegen_flags(inv: &Invocation) -> Vec<String> {
    let mut flags = vec![
        "-C".to_owned(),
        format!("opt-level={}", inv.opt_level),
        "-C".to_owned(),
        format!("debuginfo={}", inv.debuginfo),
        "-C".to_owned(),
        format!("debug-assertions={}", if inv.checks { "on" } else { "off" }),
    ];
    // The checks are branches and calls LLVM's inliner counts against a
    // function, so a small `inline` helper that it inlines without them —
    // mbedtls's ChaCha20 quarter round — stays a call with them, and its
    // checks with it, where inlining would have proved most of them
    // redundant: 2.4 times as slow. C says nothing of how far `inline`
    // reaches; what is wanted is that the checks not change which functions
    // are inlined, so the thresholds are raised about as much as the checks
    // swell a function: `inline` ones (225 → 1000 for a hinted callee) and
    // call sites LLVM thinks seldom run (45 → 225), which is every case of
    // an interpreter's big `switch`. A program grows by under 2 %.
    if inv.checks && inv.opt_level != "0" {
        flags.extend([
            "-C".to_owned(),
            "llvm-args=-inlinehint-threshold=1000".to_owned(),
            "-C".to_owned(),
            "llvm-args=-inline-cold-callsite-threshold=225".to_owned(),
        ]);
    }
    if let Some(triple) = &inv.target {
        flags.extend(["--target".to_owned(), triple.clone()]);
    }
    if let Some(cpu) = &inv.cpu {
        flags.extend(["-C".to_owned(), format!("target-cpu={cpu}")]);
    }
    // GCC's names, as LLVM spells them: `-mbmi` is `+bmi1`, `-mno-abm` is
    // `-lzcnt,-popcnt`.
    let features: Vec<String> = inv
        .features
        .iter()
        .flat_map(|name| {
            let (sign, set) = match name.strip_prefix("no-") {
                Some(set) => ('-', set),
                None => ('+', name.as_str()),
            };
            cinrs_core::x86::target_features(set)
                .unwrap_or(&[])
                .iter()
                .map(move |feature| format!("{sign}{feature}"))
        })
        .collect();
    if !features.is_empty() {
        flags.extend([
            "-C".to_owned(),
            format!("target-feature={}", features.join(",")),
        ]);
    }
    flags
}

/// The instruction sets the unit is compiled for, in GCC's names, for the
/// feature macros: what `-march=` brings — asked of `rustc`, which knows each
/// processor — and then the `-m` switches, in order.
fn command_line_features(
    inv: &Invocation,
    rustc: &Rustc,
    target: &Target,
) -> Result<Vec<String>, String> {
    let x86 = matches!(target.model.arch, Arch::X86 | Arch::X86_64);
    if !inv.features.is_empty() && !x86 {
        return Err(format!(
            "the instruction-set switches (-m{}) are x86's, and the target is {}",
            inv.features[0], target.triple
        ));
    }
    let mut names = Vec::new();
    if let Some(cpu) = &inv.cpu
        && x86
    {
        let mut cmd = rustc.command();
        cmd.args(["--print", "cfg", "-C", &format!("target-cpu={cpu}")]);
        if let Some(triple) = &inv.target {
            cmd.args(["--target", triple]);
        }
        let output = cmd
            .output()
            .map_err(|error| format!("cannot run rustc: {error}"))?;
        let said = String::from_utf8_lossy(&output.stderr);
        if said.contains("is not a recognized processor") {
            return Err(format!(
                "'-march={cpu}': rustc does not know that processor; \
                 `rustc --print target-cpus` lists those it does"
            ));
        }
        if !output.status.success() {
            return Err(format!("'-march={cpu}': {}", said.trim()));
        }
        let enabled: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.strip_prefix("target_feature=\""))
            .map(|rest| rest.trim_end_matches('"').to_owned())
            .collect();
        // GCC's names for two instruction sets at once — `abm`, `sse4` — are
        // what a processor implies by its halves, not something it has.
        for (gcc, rust) in cinrs_core::x86::TARGET_FEATURES {
            if let [feature] = rust
                && enabled.iter().any(|have| have == feature)
            {
                names.push((*gcc).to_owned());
            }
        }
    }
    names.extend(inv.features.iter().cloned());
    Ok(names)
}

/// Runs `rustc`, echoing the command under `-v`; on failure prints what it
/// said and fails with `context`.
pub fn run_rustc(
    inv: &Invocation,
    mut cmd: Command,
    context: impl FnOnce() -> String,
) -> Result<(), Failure> {
    if inv.verbose {
        eprintln!("{}", shown(&cmd));
    }
    let output = cmd
        .output()
        .map_err(|error| format!("cannot run {}: {error}", shown(&cmd)))?;
    if output.status.success() {
        return Ok(());
    }
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    Err(Failure::Message(context()))
}

/// A command as a shell would take it, for `-v` and for messages.
fn shown(cmd: &Command) -> String {
    std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|part| part.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The crate name of one translation unit's object: its file stem, made an
/// identifier, and a hash of where it came from and what it became, so that
/// two `util.c` in two directories are two crates and their private symbols
/// never meet — and so are one file compiled twice into two objects (with
/// two `-D`s, or libtool's test of `nm`, which links two objects of
/// `conftest.c`). `made` is the object's own path, or for one in the work
/// directory, which is a new name each run, the input's place on the command
/// line.
fn crate_name(stem: &str, source: &Path, made: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::path::absolute(source)
        .unwrap_or_else(|_| source.to_path_buf())
        .hash(&mut hasher);
    made.hash(&mut hasher);
    let ident: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("ccinrs_{ident}_{:08x}", hasher.finish() as u32)
}

/// Whether the linker takes `path` as a shared library.
fn is_shared_library(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    name.ends_with(".so")
        || name.contains(".so.")
        || name.ends_with(".dylib")
        || name.ends_with(".dll")
}

/// Whether the linker takes `path` as an archive, whose members it links only
/// when something needs them.
fn is_archive(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == "a" || ext == "lib")
}

/// `a.out`, or `a.exe` where executables have that extension.
fn default_output() -> &'static str {
    if cfg!(windows) { "a.exe" } else { "a.out" }
}

/// The directory a run keeps its generated `.rs` files and objects in.
struct WorkDir {
    dir: PathBuf,
    keep: bool,
}

impl WorkDir {
    fn create(keep: bool) -> Result<Self, String> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let dir = std::env::temp_dir().join(format!("ccinrs-{}-{nanos:08x}", std::process::id()));
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
        if keep {
            eprintln!(
                "ccinrs: keeping the generated Rust and the objects in {}",
                dir.display()
            );
        }
        Ok(Self { dir, keep })
    }

    fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// A file name for input `index`: unique within the run, and still
    /// recognisably the file it came from.
    fn stem(&self, index: usize, path: &Path) -> String {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        format!("{index}-{stem}")
    }

    /// Brings a file the linker takes as it is — an object, an archive, a
    /// shared library — into the work directory, where a verbatim `-l` can
    /// name it: a hard link where the file system allows one, a copy where it
    /// does not.
    fn link_input(&self, index: usize, path: &Path) -> Result<PathBuf, String> {
        let name = path
            .file_name()
            .ok_or_else(|| format!("{}: not a file", path.display()))?;
        let here = self.path(&format!("{index}-{}", name.to_string_lossy()));
        // What a symbolic link names, not the link: a hard link to a relative
        // one would name nothing from the work directory — Redis's
        // `deps/xxhash/libxxhash.a`, jemalloc's `libjemalloc.so`.
        let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if std::fs::hard_link(&target, &here).is_err() {
            std::fs::copy(&target, &here)
                .map_err(|error| format!("{}: {error}", path.display()))?;
        }
        Ok(here)
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `-std=` takes GCC's leniencies, as GCC's do, until
    /// `-pedantic-errors` (or `-Werror=pedantic`) says otherwise; `-pedantic`
    /// alone changes nothing. The dialect is the `-std=`'s either way.
    #[test]
    fn the_leniencies_follow_pedantic_errors() {
        let triple = "x86_64-unknown-linux-gnu";
        let target = Target {
            triple: triple.to_owned(),
            model: TargetModel::from_triple(triple).expect("a known triple"),
            cross: false,
            self_contained: false,
            bare_wasm: false,
        };
        let options_for = |args: &[&str]| {
            let inv = crate::args::parse(args.iter().map(|a| (*a).to_owned())).expect("parses");
            options(&inv, &target, Vec::new())
        };
        for std in ["-std=c89", "-std=c99", "-std=c11", "-std=c17", "-std=gnu11"] {
            let lenient = |args: &[&str]| {
                let mut all = vec![std];
                all.extend_from_slice(args);
                all.push("a.c");
                options_for(&all).gnu_leniencies
            };
            assert!(lenient(&[]), "{std}");
            assert!(lenient(&["-pedantic"]), "{std}");
            assert!(!lenient(&["-pedantic-errors"]), "{std}");
            assert!(!lenient(&["-Werror=pedantic"]), "{std}");
        }
        let options = options_for(&["-std=c11", "a.c"]);
        assert_eq!(options.dialect, cinrs_core::Dialect::Iso);
        assert!(options.gating().lenient());
        let options = options_for(&["-std=c11", "-pedantic-errors", "a.c"]);
        assert!(!options.gating().lenient());
        let gnu = options_for(&["-std=gnu11", "-pedantic-errors", "a.c"]);
        assert!(gnu.gating().lenient());
        // `-f[no-]gnu89-inline` overrides the `-std=`'s inline rules.
        assert!(options_for(&["-std=gnu89", "a.c"]).uses_gnu89_inline());
        assert!(!options_for(&["-std=c17", "a.c"]).uses_gnu89_inline());
        assert!(options_for(&["-std=c17", "-fgnu89-inline", "a.c"]).uses_gnu89_inline());
        assert!(!options_for(&["-std=gnu89", "-fno-gnu89-inline", "a.c"]).uses_gnu89_inline());
    }

    /// libjpeg-turbo's `global: *`, a prefix pattern, one that matches
    /// nothing, and everything a narrowing leaves alone: a `local:` list, an
    /// `extern "C++"` block, comments.
    #[test]
    fn a_version_scripts_wildcards_are_the_c_symbols() {
        let symbols = [
            "jpeg_read".to_owned(),
            "jpeg_write".to_owned(),
            "tj_init".to_owned(),
        ];
        let script = "LIBJPEG_6.2 { global: *; };\n";
        assert_eq!(
            narrow_wildcards(script, &symbols).unwrap(),
            "LIBJPEG_6.2 { global: jpeg_read;\n    jpeg_write;\n    tj_init; };\n"
        );
        let script = "/* api */ V_1 {\n  global:\n    jpeg_*; # the library\n    zz*;\n    \
                      extern \"C++\" { ns::*; };\n  local:\n    *;\n};\n";
        assert_eq!(
            narrow_wildcards(script, &symbols).unwrap(),
            "/* api */ V_1 {\n  global:\n    jpeg_read;\n    jpeg_write; # the library\n    \n    \
             extern \"C++\" { ns::*; };\n  local:\n    *;\n};\n"
        );
        assert_eq!(
            narrow_wildcards("V { global: tj_init; local: *; };", &symbols),
            None
        );
    }

    /// zlib's way of saying it: a soname and a script in one `-Wl,`, the
    /// script's own nodes and locals, a wildcard, a comment.
    #[test]
    fn what_a_version_script_mentions() {
        let dir = std::env::temp_dir().join(format!("ccinrs-vs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the directory");
        let script = dir.join("z.map");
        std::fs::write(
            &script,
            "ZLIB_1.2.0 {\n  global:\n    compressBound;\n  local:\n    zcalloc; /* not ours */\n    _*;\n};\n\
             ZLIB_1.2.2 {\n    adler32_combine;\n} ZLIB_1.2.0;\n",
        )
        .expect("the script");
        let args = crate::args::parse([
            format!(
                "-Wl,-soname,libz.so.1,--version-script,{}",
                script.display()
            ),
            "z.c".to_owned(),
        ])
        .expect("the command line")
        .linker_args;
        let patterns = version_script_patterns(&args);
        assert_eq!(
            patterns,
            [
                "compressBound",
                "zcalloc",
                "_*",
                "adler32_combine",
                "ZLIB_1.2.0"
            ]
        );
        let mentioned = |name: &str| patterns.iter().any(|p| glob_matches(p, name));
        assert!(mentioned("compressBound") && mentioned("_tr_init") && mentioned("zcalloc"));
        assert!(!mentioned("zlibVersion") && !mentioned("deflate"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn globs() {
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("gz?ead*", "gzread_internal"));
        assert!(!glob_matches("gz?ead", "gzreads"));
        assert!(glob_matches("exact", "exact") && !glob_matches("exact", "exactly"));
    }
}
