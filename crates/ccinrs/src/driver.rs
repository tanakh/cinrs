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
use cinrs_core::{Options, TargetModel};

use crate::args::{Input, Invocation, Stage};
use crate::diag;
use crate::rustc::Rustc;

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
    if inv.inputs.is_empty() {
        return Err("no input files".to_owned().into());
    }
    if inv.stage == Stage::Compile {
        return Err("'-c' is not supported yet".to_owned().into());
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
    let target = TargetModel::host();
    check_target(inv, &target, &rustc.host)?;
    let work = WorkDir::create(inv.save_temps)?;
    let options = options(inv, target);

    let mut objects = Vec::new();
    let mut failed = false;
    for (index, input) in inv.inputs.iter().enumerate() {
        match input {
            Input::C(path) => match compile(inv, &rustc, &options, &work, index, path)? {
                Some(object) => objects.push(object),
                None => failed = true,
            },
            Input::Linker(path) => objects.push(work.link_input(index, path)?),
        }
    }
    if failed {
        return Err(Failure::Reported);
    }
    let output = inv
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from(default_output()));
    link(inv, &rustc, &work, &objects, &output)
}

/// What cinrs-core is told for every file of this command line.
fn options(inv: &Invocation, target: TargetModel) -> Options {
    let mut options = Options::with_dialect(inv.standard, inv.dialect).for_target(target);
    options.include_paths = inv.include_dirs.clone();
    options.system_include = if inv.system_include {
        // GCC's order: the platform's headers win, and what only a compiler
        // carries — <stdarg.h>, <stddef.h>, the intrinsics — is cinrs's,
        // since the platform's directories do not have them.
        System::First
    } else {
        System::Off
    };
    options.dollar_in_identifiers = inv.dollars;
    options.macros = inv.macros.clone();
    // To a C compiler every definition that is not `static` is a symbol.
    options.export = true;
    // The complex types need cinrs-rt, which `ccinrs` does not build yet.
    options.complex = false;
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

/// Translates and compiles one C file into an object, or prints why it could
/// not and answers `None`.
fn compile(
    inv: &Invocation,
    rustc: &Rustc,
    options: &Options,
    work: &WorkDir,
    index: usize,
    path: &Path,
) -> Result<Option<PathBuf>, Failure> {
    let translation = cinrs_core::translate_file(path, options)?;
    let (text, tally) = diag::render(&translation.map, &translation.diagnostics, inv.warnings);
    eprint!("{text}");
    let Some(items) = translation.items else {
        debug_assert!(tally.errors > 0);
        return Ok(None);
    };
    let stem = work.stem(index, path);
    let rust = work.path(&format!("{stem}.rs"));
    let source = format!(
        "// Generated by ccinrs from {}; not meant to be edited.\n{items}\n",
        path.display()
    );
    std::fs::write(&rust, source)
        .map_err(|error| format!("cannot write {}: {error}", rust.display()))?;
    let object = work.path(&format!("{stem}.o"));
    let mut cmd = rustc.command();
    cmd.args(["--edition", "2024", "--crate-type", "lib", "--emit", "obj"])
        .args(["--crate-name", &crate_name(&stem, path)])
        .args(codegen_flags(inv))
        .args(["-C", "codegen-units=1", "--cap-lints", "allow"])
        .arg("-o")
        .arg(&object)
        .arg(&rust);
    run_rustc(inv, cmd, || {
        format!(
            "rustc rejected the translation of {}; this is a bug in ccinrs (the Rust is in {})",
            path.display(),
            rust.display()
        )
    })?;
    Ok(Some(object))
}

/// Links the objects, in command-line order, into `output`.
fn link(
    inv: &Invocation,
    rustc: &Rustc,
    work: &WorkDir,
    objects: &[PathBuf],
    output: &Path,
) -> Result<(), Failure> {
    let stub = work.path("ccinrs_main.rs");
    std::fs::write(
        &stub,
        "// The program's `main` is the C one, in one of the objects.\n#![no_main]\n",
    )
    .map_err(|error| format!("cannot write {}: {error}", stub.display()))?;
    let mut cmd = rustc.command();
    cmd.args([
        "--edition",
        "2024",
        "--crate-type",
        "bin",
        "--crate-name",
        "ccinrs_main",
    ])
    .args(codegen_flags(inv))
    .args(["--cap-lints", "allow"]);
    // The objects go in as `-l static:+verbatim`, which puts them on the link
    // line ahead of the standard library they refer to — the order a linker
    // that reads its inputs once needs. A verbatim name is a file name in a
    // `-L` directory, which is why every object is in the work directory.
    cmd.arg("-L").arg(work.dir());
    for object in objects {
        let name = object
            .file_name()
            .expect("an object in the work directory has a name")
            .to_string_lossy();
        let kind = if is_shared_library(object) {
            "dylib"
        } else {
            "static"
        };
        cmd.arg("-l").arg(format!("{kind}:+verbatim={name}"));
    }
    for dir in &inv.lib_dirs {
        cmd.arg("-L").arg(format!("native={}", dir.display()));
    }
    for lib in &inv.libs {
        cmd.arg("-l").arg(lib);
    }
    for arg in &inv.linker_args {
        cmd.arg("-C").arg(format!("link-arg={arg}"));
    }
    if inv.strip {
        cmd.args(["-C", "strip=symbols"]);
    }
    cmd.arg("-o").arg(output).arg(&stub);
    // A link that fails is the program's own problem — an undefined symbol, a
    // missing library — so what the linker said is the message.
    run_rustc(inv, cmd, || format!("linking {} failed", output.display()))
}

/// The `-C` flags every `rustc` run gets.
fn codegen_flags(inv: &Invocation) -> Vec<String> {
    vec![
        "-C".to_owned(),
        format!("opt-level={}", inv.opt_level),
        "-C".to_owned(),
        format!("debuginfo={}", inv.debuginfo),
        "-C".to_owned(),
        format!("debug-assertions={}", if inv.checks { "on" } else { "off" }),
    ]
}

/// Runs `rustc`, echoing the command under `-v`; on failure prints what it
/// said and fails with `context`.
fn run_rustc(
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
