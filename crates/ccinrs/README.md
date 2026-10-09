# ccinrs

A C compiler with GCC's command line, built on [`cinrs`](https://crates.io/crates/cinrs).

```text
cargo install ccinrs
ccinrs -O2 -o hello hello.c
make CC=ccinrs
cmake -S . -B build -DCMAKE_C_COMPILER=ccinrs
```

Every C file is translated to Rust by the same front end the `c99!` family
of macros runs, and `rustc` compiles that Rust into an ordinary object file
and links the program. No C compiler, assembler or linker of its own is
involved, so `make CC=ccinrs` builds a C project with its own build system —
Makefiles, CMake, autoconf and libtool — on any machine that can
`cargo build`. SQLite, CPython, OpenSSL, FFmpeg, git, nginx, Redis and twenty
other open-source programs have been built this way and their own test
suites run; see
[ccinrs on real programs](https://github.com/tanakh/cinrs/blob/master/doc/ccinrs-real-programs.md).

## Requirements

* **`rustc` 1.99 or later**: the one `RUSTC` names, or else the one on
  `PATH`. A program links with the `rustc` that compiled its objects; an
  object from another `rustc` is refused in one sentence.
* For a program for the machine it runs on, the platform's C headers and C
  library (`libc6-dev` on Debian and Ubuntu), which `rustc` links anyway.
* For another machine, the Rust standard library for it (`rustup target add
  <triple>`), and nothing else for the targets below.

## Defaults

* **`-std=gnu17`** and GCC 14.2's predefined macros (`__GNUC__` is 14), with
  `__CINRS__` beside them. Every `-std=` takes the constraint violations GCC
  only warns about, as GCC's does, unless `-pedantic-errors` is given.
* **Rust's run-time checks are on**: a misaligned or null pointer
  dereference, an overlapping `memcpy` and the checks cinrs writes itself stop
  the program with a panic that names the C file and line. `-fno-cinrs-checks`
  takes them out.
* **Every function is `extern "C-unwind"`**, so that `setjmp` and `longjmp` —
  a Rust unwind here — work across files. `-fno-cinrs-unwind` makes them
  `extern "C"` and refuses `setjmp` and `longjmp`.
* The platform's headers are searched after `-I` and before cinrs's bundled
  ones, every definition that is not `static` is a C symbol, and a local
  declared without an initialiser is zero-filled
  (`-ftrivial-auto-var-init=uninitialized` leaves local arrays alone).

## Options

The options a build passes GCC are taken: `-o`, `-c`, `-S` (which writes the
Rust), `-E`, `-fsyntax-only`, `-I` and its relatives, `-D`, `-U`,
`-include`, `-std=`, `-M` and its relatives, `-O0` … `-O3`, `-Os`, `-Oz`,
`-g`, `-flto`, `-march=`, `-m<feature>`, `--target=`, `--sysroot=`, `-l`,
`-L`, `-Wl,…`, `-Xlinker`, `-shared`, `-static`, `-rdynamic`, `-pthread`,
`-fuse-ld=`, `-Werror`, `-w`, `-v`, `-save-temps`, `--version`,
`-dumpversion`, `-dumpmachine` and the `-print-*` questions libtool asks.
[The option table](https://github.com/tanakh/cinrs/blob/master/doc/ccinrs.md#options)
says what each one does here.

For the rest:

* A warning option GCC knows, and a code-generation option that cannot change
  what a program means (`-fPIC`, `-fno-strict-aliasing`,
  `-fvisibility=hidden`, …), is accepted without a word.
* An unknown `-W` or `-f` option is accepted with a warning — and is an error
  under `-Werror`, which is how a `configure` script finds out whether an
  option is taken.
* An option that would change what a program means and that cinrs cannot
  follow — `-fshort-enums`, `-fpack-struct`, `-fopenmp`, `-fsanitize=` — is
  an error, never a silent difference, and so is any other unknown option.
* C++, Objective-C and assembly source files are refused; `-imacros`, `-dD`,
  `-dN` and `-MG` are not supported yet.

## Platforms and targets

* **x86-64 Linux** is where `ccinrs` is developed and tested: its own tests,
  which the continuous integration runs; and `scripts/check-ccinrs.sh`, which
  builds six projects with their own build systems and runs their tests, and
  runs c-testsuite through the command line.
* **`--target=`** compiles for another machine: **musl**
  (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, …), linked by
  `rust-lld` into a static program with no C compiler, linker or C library
  installed (an AArch64 one is tested under qemu where it is installed);
  **`wasm32-wasip1` and `wasm32-wasip2`**, with Rust's wasi-libc, for any WASI
  runtime (the tests run them under wasmtime); and
  **`wasm32-unknown-unknown`**, as a module that exports the C's functions
  and imports what the C leaves undefined, for a browser or Node to call;
  there is no C library under it.
* **macOS and Windows** are not tried yet.
* `-shared` makes ELF shared libraries only.

## Known limitations

* **`long double` is `double`.** A literal `printf` or `scanf` format that
  names one with `L` is rewritten for the platform's C library; a `long
  double` passed to the platform any other way, where its own is wider, is an
  error.
* **Only `rustc` links.** An object another compiler made links with
  `ccinrs`'s, but `ccinrs`'s objects need Rust's standard library, so another
  compiler's driver — `c++` linking a C++ program — cannot link them.
* **A weak definition** is a real one on ELF only; elsewhere, and under
  `rustc`'s own `-flto`, it is an ordinary definition with a warning.
* **`setjmp` and `longjmp`** work, with limits: a `setjmp` may stand where C
  allows it and in a few more places GCC takes, a `longjmp` costs about a
  microsecond rather than GCC's tens of nanoseconds, and one out of a signal
  handler that interrupted the program's own code stops the program.
* **A thread-local object** with external linkage is shared between
  `ccinrs`'s files, but not with an object a C compiler made.
* **A `configure` probe that does something C leaves undefined** — a
  misaligned load — stops at a run-time check, and `configure` takes that for
  its answer; give it GCC's answer, or run it with `-fno-cinrs-checks`.
* **A program is bigger than GCC's**: it carries Rust's standard library, and
  the run-time checks.

Everything [cinrs's own limitations](https://github.com/tanakh/cinrs/blob/master/doc/limitations.md)
lists applies too. [`doc/ccinrs.md`](https://github.com/tanakh/cinrs/blob/master/doc/ccinrs.md)
is the whole of it: how a program is made, the defaults, every option, the
targets, link-time optimisation, shared libraries and the limitations.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
