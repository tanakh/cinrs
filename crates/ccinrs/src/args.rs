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

use cinrs_core::{CommandLineMacro, Dialect, Standard};

/// What to stop after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Compile every C file and link the program: the default.
    Link,
    /// `-c`: one object file per C file, and no link.
    Compile,
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
    /// `rustc`'s `-C opt-level`: `0`, `1`, `2`, `3`, `s` or `z`.
    pub opt_level: &'static str,
    /// `rustc`'s `-C debuginfo`.
    pub debuginfo: u8,
    /// `-w` turns warnings off.
    pub warnings: bool,
    /// Whether the generated code keeps Rust's run-time checks — misaligned
    /// and null pointer dereferences, overlapping `memcpy`, and the checks
    /// cinrs itself writes in a build with debug assertions. **On by
    /// default**; `-fno-cinrs-checks` turns them off.
    pub checks: bool,
    /// `-fdollars-in-identifiers`, on by default as in GCC.
    pub dollars: bool,
    /// What `-fsigned-char` or `-funsigned-char` asked for, which the driver
    /// checks against the target: cinrs takes the platform's choice and
    /// cannot change it.
    pub char_signed: Option<bool>,
    /// The pointer width `-m32` or `-m64` asked for, checked the same way.
    pub pointer_bits: Option<u32>,
    /// `-L`, in order.
    pub lib_dirs: Vec<PathBuf>,
    /// `-l`, in order (and `pthread` for `-pthread`).
    pub libs: Vec<String>,
    /// `-Wl,…`, each as written, for the linker driver `rustc` runs.
    pub linker_args: Vec<String>,
    /// `-s`.
    pub strip: bool,
    /// `-v`: print every command run.
    pub verbose: bool,
    /// `-save-temps`: keep the generated `.rs` files and the objects.
    pub save_temps: bool,
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
            opt_level: "0",
            debuginfo: 0,
            warnings: true,
            checks: true,
            dollars: true,
            char_signed: None,
            pointer_bits: None,
            lib_dirs: Vec::new(),
            libs: Vec::new(),
            linker_args: Vec::new(),
            strip: false,
            verbose: false,
            save_temps: false,
            notes: Vec::new(),
        }
    }
}

/// The options GCC has that this version of `ccinrs` does not, yet: an error
/// that says so rather than "unknown option".
const NOT_YET: &[&str] = &[
    "-E",
    "-S",
    "-M",
    "-MM",
    "-MD",
    "-MMD",
    "-MF",
    "-MT",
    "-MQ",
    "-MP",
    "-include",
    "-shared",
    "-static",
    "-rdynamic",
    "--version",
    "-dumpversion",
    "-dumpmachine",
];

/// `-W` options that only choose which warnings GCC prints, accepted without
/// a word: the names themselves, or a prefix ending in `-` or `=`.
const QUIET_WARNINGS: &[&str] = &[
    "all",
    "extra",
    "error",
    "pedantic",
    "no-",
    "error=",
    "fatal-errors",
    "shadow",
    "conversion",
    "sign-",
    "strict-",
    "missing-",
    "declaration-after-statement",
    "format",
    "unused",
    "cast-",
    "pointer-",
    "write-strings",
    "undef",
    "vla",
    "implicit",
    "old-style-",
    "redundant-decls",
    "nested-externs",
    "inline",
    "init-self",
    "float-",
    "switch",
    "uninitialized",
    "maybe-uninitialized",
    "double-promotion",
    "padded",
    "null-",
    "logical-op",
    "jump-misses-init",
    "comment",
    "type-limits",
    "bad-function-cast",
    "char-subscripts",
    "parentheses",
    "return-type",
    "sequence-point",
    "unreachable-code",
    "long-long",
    "variadic-macros",
    "array-bounds",
    "stack-protector",
    "frame-larger-than=",
    "larger-than=",
    "alloca",
    "deprecated",
    "attributes",
    "empty-body",
    "int-conversion",
    "incompatible-pointer-types",
    "int-to-pointer-cast",
    "misleading-indentation",
    "nonnull",
    "shift-",
    "unknown-pragmas",
    "c++-compat",
    "c90-c99-compat",
    "c99-c11-compat",
    "traditional",
    "date-time",
    "discarded-qualifiers",
    "duplicated-",
    "multichar",
    "overflow",
    "packed",
    "pointer-sign",
    "restrict",
    "stringop-",
    "vla-larger-than=",
    "override-init",
    "suggest-attribute=",
    "psabi",
    "abi",
    "address",
    "bool-",
    "clobbered",
    "dangling-",
    "div-by-zero",
    "sizeof-",
    "tautological-",
    "zero-length-bounds",
];

/// `-f` options that cannot change what a program means here, accepted
/// without a word; same matching as [`QUIET_WARNINGS`].
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
    "no-diagnostics-color",
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
    "lto",
    "no-lto",
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
    "gnu89-inline",
    "no-gnu89-inline",
    "hosted",
    "freestanding",
    "jump-tables",
    "no-jump-tables",
    "merge-constants",
    "no-merge-constants",
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
        if NOT_YET.contains(&arg.as_str()) || arg.starts_with("-M") {
            return Err(format!("'{arg}' is not supported yet"));
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
            "-c" => inv.stage = Stage::Compile,
            "-w" => inv.warnings = false,
            "-v" => inv.verbose = true,
            "-s" => inv.strip = true,
            "-pipe" => {}
            "-pthread" => inv.libs.push("pthread".to_owned()),
            "-nostdinc" => inv.system_include = false,
            "-ansi" => (inv.standard, inv.dialect) = (Standard::C89, Dialect::Iso),
            "-m64" => inv.pointer_bits = Some(64),
            "-m32" => inv.pointer_bits = Some(32),
            "-" => {
                return Err("reading the program from standard input is not supported".to_owned());
            }
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
            _ if arg.starts_with("-Wl,") => inv.linker_args.push(arg.clone()),
            _ if arg.starts_with("-Wa,") || arg.starts_with("-Wp,") => {
                inv.notes.push(format!(
                    "ignoring '{arg}': ccinrs runs no separate assembler or preprocessor"
                ));
            }
            _ if arg.starts_with("-W") => {
                if !listed(QUIET_WARNINGS, &arg[2..]) {
                    inv.notes
                        .push(format!("ignoring unknown warning option '{arg}'"));
                }
            }
            _ if arg.starts_with("-f") => flag(&mut inv, &arg)?,
            _ if arg.starts_with("-mtune=") => {}
            _ if arg.starts_with("-m") => {
                return Err(format!("'{arg}' is not supported yet"));
            }
            _ if arg.starts_with('-') => {
                return Err(format!("unrecognized command-line option '{arg}'"));
            }
            _ => inv.inputs.push(input(PathBuf::from(&arg), force_c)?),
        }
    }
    Ok(inv)
}

/// One `-f` option.
fn flag(inv: &mut Invocation, arg: &str) -> Result<(), String> {
    let name = &arg[2..];
    match name {
        "cinrs-checks" => inv.checks = true,
        "no-cinrs-checks" => inv.checks = false,
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
        _ => inv.notes.push(format!("ignoring unknown option '{arg}'")),
    }
    Ok(())
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
        "c" | "i" => Ok(Input::C(path)),
        "h" => Err(format!(
            "{}: a header is not compiled on its own; ccinrs makes no precompiled headers",
            path.display()
        )),
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
        let inv =
            parse_all(&["-Wall", "-Wextra", "-Wno-unused", "-fPIC", "-fwrapv", "a.c"]).unwrap();
        assert!(inv.notes.is_empty(), "{:?}", inv.notes);
        let inv = parse_all(&["-Wfrobnicate", "-ffrobnicate", "a.c"]).unwrap();
        assert_eq!(inv.notes.len(), 2);
        assert!(
            parse_all(&["-fshort-enums", "a.c"])
                .unwrap_err()
                .contains("changes what")
        );
        assert!(!parse_all(&["-fno-cinrs-checks", "a.c"]).unwrap().checks);
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
