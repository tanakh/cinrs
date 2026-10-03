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
use cinrs_core::{Arch, Env, Options, Os, TargetModel, TargetSource};

use crate::args::{Input, Invocation, Lto, Query, Stage};
use crate::runtime::{self, Runtime};
use crate::rustc::Rustc;
use crate::{deps, diag, preprocess, print};

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
    if let Some(query) = inv.query {
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
                // what a shared library made here exports.
                if is_archive(path) || is_shared_library(path) {
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
                objects.push(LinkInput {
                    path: run.work.link_input(index, path)?,
                    crate_name,
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
}

impl ObjectInfo {
    fn add(&mut self, other: ObjectInfo) {
        self.uses_runtime |= other.uses_runtime;
        self.symbols.extend(other.symbols);
        self.crates.extend(other.crates);
    }
}

/// Answers `--version`, `-dumpmachine` and the like.
fn answer(inv: &Invocation, query: Query) -> Result<(), Failure> {
    let (major, minor, patch) = cinrs_core::GCC_VERSION;
    match query {
        Query::Version => print!("{}", version_text()),
        // What `__GNUC__` says, which is what a build that asks compares.
        Query::DumpVersion => println!("{major}"),
        Query::DumpFullVersion => println!("{major}.{minor}.{patch}"),
        Query::DumpMachine => match &inv.target {
            Some(triple) => println!("{triple}"),
            None => println!("{}", Rustc::locate()?.host),
        },
        Query::Help => print!("{HELP}"),
    }
    Ok(())
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
        let Some(libdir) = rustc.target_libdir(&triple) else {
            return Err(format!(
                "the Rust standard library for '{triple}' is not installed; \
                 `rustup target add {triple}` installs it"
            ));
        };
        let self_contained =
            model.env == Env::Musl && libdir.join("self-contained/libc.a").is_file();
        Ok(Self {
            cross: triple != rustc.host,
            triple,
            model,
            self_contained,
        })
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
            let (text, _) = diag::render(
                &translation.map,
                &translation.diagnostics,
                self.inv.warnings,
            );
            eprint!("{text}");
            let Some(items) = translation.items else {
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
        for input in &inv.inputs {
            let path = match input {
                Input::C(path) => path,
                Input::Linker(path) => {
                    if inv.warnings {
                        eprintln!(
                            "ccinrs: warning: {}: linker input file unused because linking not done",
                            path.display()
                        );
                    }
                    continue;
                }
            };
            let pre = if path.as_os_str() == STDIN {
                cinrs_core::preprocess_source(STDIN_NAME.to_owned(), read_stdin()?, &self.options)
            } else {
                cinrs_core::preprocess_file(path, &self.options)?
            };
            let (text, tally) = diag::render(&pre.map, &pre.diagnostics, inv.warnings);
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
fn version_script_patterns(linker_args: &[String]) -> Vec<String> {
    let mut files = Vec::new();
    for arg in linker_args {
        let Some(list) = arg.strip_prefix("-Wl,") else {
            continue;
        };
        let mut items = list.split(',');
        while let Some(item) = items.next() {
            let item = item.trim_start_matches('-');
            if let Some(file) = item.strip_prefix("version-script=") {
                files.push(file.to_owned());
            } else if item == "version-script"
                && let Some(file) = items.next()
            {
                files.push(file.to_owned());
            }
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
    if inv.warnings {
        for input in &inv.inputs {
            if let Input::Linker(path) = input {
                eprintln!(
                    "ccinrs: warning: {}: linker input file unused because linking not done",
                    path.display()
                );
            }
        }
    }
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
        let (text, tally) = diag::render(&translation.map, &translation.diagnostics, inv.warnings);
        eprint!("{text}");
        let Some(items) = translation.items else {
            debug_assert!(tally.errors > 0);
            return Ok(None);
        };
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
        let name = crate_name(&stem, path);
        if inv.lto.is_some() {
            source.push_str(&used_bytes(
                "CCINRS_CRATE",
                &format!("{CRATE_PREFIX}{name}\0"),
            ));
        }
        std::fs::write(&rust, source)
            .map_err(|error| format!("cannot write {}: {error}", rust.display()))?;
        let mut cmd = self.rustc.command();
        cmd.args(["--edition", "2024"]);
        if inv.lto.is_some() {
            cmd.args(["--crate-type", "rlib"]);
        } else {
            cmd.args(["--crate-type", "lib", "--emit", "obj"]);
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
        cmd.arg("-o").arg(object).arg(&rust);
        run_rustc(inv, cmd, || {
            format!(
                "rustc rejected the translation of {}; this is a bug in ccinrs (the Rust is in {})",
                path.display(),
                rust.display()
            )
        })?;
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
            crates: inv.lto.map(|_| name).into_iter().collect(),
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

    /// One linker argument of the command line, as `rustc` is to pass it on.
    /// Through a C compiler, as it is; to `rust-lld` itself (musl), the
    /// driver's spellings undone — `-Wl,a,b` is `a` and `b`, and `-rdynamic`
    /// is `--export-dynamic`.
    fn linker_arg(&self, arg: &str) -> Vec<String> {
        if !self.target.self_contained {
            return vec![arg.to_owned()];
        }
        if let Some(list) = arg.strip_prefix("-Wl,") {
            return list.split(',').map(str::to_owned).collect();
        }
        if arg == "-rdynamic" {
            return vec!["--export-dynamic".to_owned()];
        }
        vec![arg.to_owned()]
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
    let symbols = marked_names(&bytes, SYMBOLS_PREFIX);
    let crates = marked_names(&bytes, CRATE_PREFIX);
    let prefix = STAMP_PREFIX.as_bytes();
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
    Ok(ObjectInfo {
        uses_runtime,
        symbols,
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
        if inv.shared {
            self.check_shared()?;
        }
        let stub = self.work.path("ccinrs_main.rs");
        let mut text = if inv.shared {
            "// A shared library of the C objects linked into it.\n"
        } else {
            "// The program's `main` is the C one, in one of the objects.\n#![no_main]\n"
        }
        .to_owned();
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
            .arg(if inv.shared { "cdylib" } else { "bin" })
            .args(["--crate-name", "ccinrs_main"])
            .args(codegen_flags(inv))
            .args(["--cap-lints", "allow"]);
        if linked.uses_runtime {
            cmd.args(runtime::extern_args(self.runtime()?));
        }
        for extern_crate in &crates {
            cmd.arg("--extern").arg(extern_crate);
        }
        // `-flto` at the link is what optimises them all as one — the C, the
        // runtime and Rust's standard library alike.
        match inv.lto {
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
        if inv.shared {
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
            let patterns = version_script_patterns(&inv.linker_args);
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
            let object = &object.path;
            if self.target.model.arch == Arch::Wasm32 {
                cmd.arg("-C").arg(format!("link-arg={}", object.display()));
                continue;
            }
            let name = object
                .file_name()
                .expect("an object in the work directory has a name")
                .to_string_lossy();
            let spec = if is_shared_library(object) {
                format!("dylib:+verbatim={name}")
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
            cmd.arg("-l").arg(lib);
        }
        for arg in &inv.linker_args {
            for arg in self.linker_arg(arg) {
                cmd.arg("-C").arg(format!("link-arg={arg}"));
            }
        }
        if self.target.self_contained {
            cmd.args(["-C", "linker=rust-lld", "-C", "linker-flavor=ld.lld"]);
        }
        if inv.strip {
            cmd.args(["-C", "strip=symbols"]);
        }
        cmd.arg("-o").arg(output).arg(&stub);
        // A link that fails is the program's own problem — an undefined
        // symbol, a missing library — so what the linker said is the message.
        run_rustc(inv, cmd, || format!("linking {} failed", output.display()))
    }
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
/// identifier, and a hash of where it came from, so that two `util.c` in two
/// directories are two crates and their private symbols never meet.
fn crate_name(stem: &str, source: &Path) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::path::absolute(source)
        .unwrap_or_else(|_| source.to_path_buf())
        .hash(&mut hasher);
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
        if std::fs::hard_link(path, &here).is_err() {
            std::fs::copy(path, &here).map_err(|error| format!("{}: {error}", path.display()))?;
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
        let args = [format!(
            "-Wl,-soname,libz.so.1,--version-script,{}",
            script.display()
        )];
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
