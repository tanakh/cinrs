# ccinrs — a C compiler with GCC's command line

`ccinrs` is `cinrs` as a command: it takes the command line a build gives GCC,
translates every C file to Rust with the front end the `c99!` macros run, and
has `rustc` compile and link the result. It never runs a C compiler or a linker
of its own, so `make CC=ccinrs` builds a C project on any machine that can
`cargo build`:

```text
cargo install ccinrs
ccinrs -O2 -o hello hello.c
make CC=ccinrs
cmake -S . -B build -DCMAKE_C_COMPILER=ccinrs
```

It needs a `rustc` of 1.99 or later — `RUSTC` names one, otherwise it is the
one on `PATH` — and, for programs for the machine it runs on, the platform's C
headers and C library (`libc6-dev` on Debian and Ubuntu), which `rustc` links
anyway.

## How a program is made

Each C file is translated on its own and compiled by `rustc --crate-type lib
--emit obj` into an ordinary object file: one small crate per translation unit,
named after the file and a hash of its path, so that two `util.c` in two
directories never meet. The objects are linked by `rustc` too, into a
`#![no_main]` binary whose `main` is the C one, because they name Rust's
standard library as that one `rustc` mangles it and only `rustc` knows where it
is. `-c` stops at the object, as with GCC; a later link takes it, and a static
archive of such objects, like any other.

Every object carries the `rustc` that compiled it — release, commit and target
— and a link with another `rustc` stops in one sentence instead of a thousand
undefined symbols.

The Rust is printed with every token on the line of the C it came from, and
`rustc` is told to call it by the C file's name, so a panic — which is how
Rust's run-time checks stop a program — names the C: `panicked at
parse.c:212:41`. (The column is the Rust's.) `-save-temps` keeps the generated
Rust and the objects; `-S` writes the Rust a file becomes, formatted to be
read, as a `.rs` that `rustc --edition 2024` builds on its own.

A program that uses `_Complex` calls cinrs's runtime, whose source `ccinrs`
carries and compiles with the same `rustc` the first time a program needs it;
it is kept in the cache directory (`CCINRS_CACHE_DIR`, or the platform's —
`~/.cache/ccinrs` on Linux), one per `rustc`, target and `ccinrs` release.

## Defaults

* **`-std=gnu17`**, as GCC 14 has it, and the predefined macros of GCC 14.2
  (`__GNUC__` is 14) together with `__CINRS__`.
* **Rust's run-time checks are on**: a misaligned or null pointer dereference,
  an out-of-bounds `memcpy` overlap and the checks cinrs writes itself stop the
  program with a panic at the C line, at every optimisation level.
  `-fno-cinrs-checks` takes them out.
* The **platform's headers** are searched, after `-I`, and before cinrs's
  bundled ones, which supply what only a compiler has (`<stdarg.h>`,
  `<stddef.h>`, the intrinsics) — GCC's order. `-nostdinc` leaves the
  platform's out. A program for another machine (`--target`) gets the bundled
  headers, and whatever `--sysroot` and `-I` point at.
* Every definition that is not `static` is a **C symbol**, as in any C
  compiler.

## Options

| | |
| --- | --- |
| `-o`, `-c`, `-S`, `-E` | the output, and where to stop (`-S` writes Rust); the earliest stage named wins |
| `-P` | `-E` without line markers |
| `-dM` | `-E` printing the macros defined at the end instead — `ccinrs -dM -E - </dev/null` lists the predefined ones |
| `-` | standard input, under `-x c` or `-E` (`<stdin>` to `__FILE__`) |
| `-include` | a file read before the first line, as if `#include "file"` were written there |
| `-I`, `-iquote`, `-isystem`, `-idirafter` | include directories, searched in the order given (cinrs has one list) |
| `-D`, `-U`, `-std=`, `-ansi`, `-x c`, `-nostdinc` | as GCC's; `-std=` takes `c89` to `c23` and `gnu89` to `gnu23` with their aliases |
| `-M`, `-MM`, `-MD`, `-MMD`, `-MF`, `-MT`, `-MQ`, `-MP` | Makefile dependency rules, as GCC writes them and under GCC's names (`-c -o obj/x.o -MD` writes `obj/x.d`); the bundled headers are not listed |
| `-O0` … `-O3`, `-Os`, `-Oz`, `-Og`, `-Ofast` | `rustc`'s `-C opt-level` (`-Ofast` is `3`, with no fast-math) |
| `-g`, `-g0` | `rustc`'s `-C debuginfo` |
| `-march=`, `-mcpu=` | `rustc`'s `-C target-cpu`, `native` included; the feature macros (`__AVX2__`, …) follow what the processor has |
| `-mavx2`, `-mno-avx512f`, … | `rustc`'s `-C target-feature`, in GCC's names, and the macros with them (x86) |
| `--target=`, `--sysroot=` | another machine; see [Targets](#targets) |
| `-l`, `-L`, `-Wl,…`, `-pthread`, `-s` | as GCC's; `-pthread` also defines `_REENTRANT` |
| `-shared`, `-static`, `-rdynamic` | a shared library (see [below](#shared-libraries)); a static program (`-C target-feature=+crt-static`); all symbols in the dynamic table |
| `-fno-cinrs-checks`, `-fcinrs-checks` | Rust's run-time checks off, on |
| `-funsigned-char`, `-fsigned-char`, `-m32`, `-m64` | checked against the target, whose answer cinrs takes |
| `--version`, `-dumpversion`, `-dumpfullversion`, `-dumpmachine`, `--help`, `-v`, `-save-temps` | `-dumpversion` is `14`, as `__GNUC__` says; `-v` on its own ends with GCC's `gcc version 14.2.0 …` line, saying it is compatible and not GCC, which is what a `configure` reads |

A `-W` option, a common `-f` code-generation option (`-fPIC`,
`-fno-strict-aliasing`, `-fvisibility=hidden`, …), `-pedantic` and `-pipe`
change nothing here and are accepted; an unknown `-W` or `-f` option is too,
with a warning. An option that would change what a program means and that
cinrs cannot follow — `-fshort-enums`, `-fpack-struct`, `-fopenmp`,
`-fsanitize=` — is an error, never a silent difference, and so is anything
unknown. Not yet: `-imacros`, `-dD`, `-MG`.

## Targets

`--target=<triple>` (Clang's spelling) compiles for another machine: the Rust
standard library for it has to be installed (`rustup target add <triple>`),
and cinrs's model of the target decides the sizes, the alignments and the
predefined macros.

**`wasm32-wasip1` and `wasm32-wasip2`** need nothing else: the C library is
wasi-libc, which Rust ships with the target, and the C `main` is named as
wasi-libc's start code calls it. A program runs under any WASI runtime
(`wasmtime prog.wasm`); a `wasip2` program is a component, whose exit status is
only success or failure. What wasi-libc leaves out — `clock()`, signals,
`fork` — is left out here too.

## Shared libraries

`-shared` links the objects — every one of them whole, as GCC does — into an
ELF shared library through `rustc --crate-type cdylib`, and exports exactly the
C symbols they define: a version script is handed to the linker beside
`rustc`'s own, which exports nothing of the Rust. `-Wl,-soname=…` and the
rest pass through, so a Makefile's or CMake's shared library builds as it is.

What to know:

* Every definition that is not `static` is exported, whatever
  `-fvisibility=hidden` or a `visibility` attribute says — unless the build
  gives its own version script (`-Wl,--version-script,…`), which then decides
  for every symbol it names, as with GCC; what it does not name is exported.
* A variable the library exports is shared with programs `ccinrs` links. A
  program another compiler links copies it into itself (a copy relocation),
  and the library goes on using its own copy, because the Rust in it refers to
  its own statics directly. Functions are unaffected.
* Each library carries its own copy of Rust's standard library.
* ELF only, for now: not macOS, Windows or WebAssembly.

## What it has built

`scripts/check-ccinrs.sh` fetches six projects at pinned releases, builds them
with their own build systems and `CC=ccinrs` — run-time checks on — and runs
their own tests; it also runs c-testsuite through the command line.

| | build | tests |
| --- | --- | --- |
| lz4 1.10.0 | `make`: the library, static and shared, and `lz4` | frame and fuzz tests, the command's seven test targets |
| cJSON 1.7.19 | `make`: both libraries, static and shared, and the test program | the 21 Unity unit tests (built by hand, see below) |
| cmark 0.31.2 | CMake | the CommonMark spec (652 examples), regressions (23), smart punctuation (16) |
| brotli 1.2.0 | CMake, shared libraries | ctest (12); the output is byte for byte `gcc`'s at every quality tried |
| zlib 1.3.1 | its own `configure` and `make`; the shared library with zlib's version script | `make test`: static, shared and 64-bit; the library exports what `gcc`'s does, with the same versions |
| expat 2.7.3 | autoconf's `configure`, libtool, `make`: static and shared | `xmlwf` (its own tests need `setjmp`) |
| c-testsuite | `ccinrs case.c` | 218 of 220; the two are on [the list](c-testsuite.md) |

SQLite's amalgamation goes through `-E` and back: the preprocessed `.i`
compiles, links and answers queries. cJSON's Unity tests are built with
Unity's switches for a compiler without `setjmp` or weak definitions
(`-DUNITY_EXCLUDE_SETJMP_H -DUNITY_NO_WEAK` and its `unity_setup.c`), which
its CMake build does not give outside MSVC; cmark's `api_test` has C++ in it,
which CMake links with `c++` — see below.

## Limitations

Everything [cinrs's own limitations](limitations.md) lists applies; the ones a
C project meets first:

* **`setjmp` and `longjmp`** are refused, and so is a **weak definition**
  (`__attribute__((weak))` on a function with a body). Lua, Tcl-style
  interpreters and test frameworks such as Unity use them by default.
* **`long double` is `double`.** A literal `printf` or `scanf` format that
  names a `long double` argument with `L` (`%Lf`, `%.2Le`) is rewritten to `l`
  for the platform's C library, which then reads the `double` it is given;
  a `long double` passed any other way to a platform function whose own is
  wider (x87's on x86-64 Linux) is an error.
* **Only `rustc` links.** An object from another compiler links with
  `ccinrs`'s, but `ccinrs`'s objects need Rust's standard library, so another
  compiler's driver — `c++` linking a C++ program, say — cannot link them.
* Developed and tested on Linux x86-64, and for `wasm32-wasip1` and `-p2`;
  macOS and Windows are not tried yet.
* A program is bigger than GCC's: it carries Rust's standard library, and
  the run-time checks.
