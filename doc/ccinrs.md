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
anyway. For musl it needs nothing else at all; see [Targets](#targets).

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
* **Every `-std=` takes GCC's leniencies** — the constraint violations GCC
  only warns about, which [the leniency
  table](gnu-extensions.md#the-leniencies-constraint-violations-gcc-only-warns-about)
  lists: a stray `;` at file scope, an enumerator outside `int`, `sizeof
  (void)` and the rest — and `#embed` before C23, as GCC 15 does. An ISO one
  (`-std=c11`) keeps what makes it ISO in GCC: `asm` and `typeof` are not
  keywords, `__STRICT_ANSI__` is defined and trigraphs are read.
  `-pedantic-errors` (or `-Werror=pedantic`) refuses the leniencies under an
  ISO `-std=`, which is what `c11!` does; a GNU one takes them regardless.
* **`__DATE__` and `__TIME__` are the moment of translation**, as GCC's are:
  `SOURCE_DATE_EPOCH` in UTC when it is set (GCC's error, at the first use,
  when it is not a number of seconds up to the end of 9999), and now in the
  local zone — `TZ`, or `/etc/localtime` — otherwise. `__TIMESTAMP__` is the
  file's modification time in the local zone, and the moment of translation
  for standard input.
* **Every function is `extern "C-unwind"`**, so that a `longjmp` — a Rust
  unwind here — can pass through any file; see [setjmp and
  longjmp](#setjmp-and-longjmp). `-fno-cinrs-unwind` makes them `extern "C"`.
* **Rust's run-time checks are on**: a misaligned or null pointer dereference,
  an out-of-bounds `memcpy` overlap and the checks cinrs writes itself stop the
  program with a panic at the C line, at every optimisation level — Rust's
  message, and then `abort`, which a hook the link installs makes immediate
  rather than an unwind looking for a handler.
  `-fno-cinrs-checks` takes them out. With them on, LLVM's inliner is told to
  reach further (`-inlinehint-threshold=1000`,
  `-inline-cold-callsite-threshold=225`), so that a small `inline` function
  the checks swell is still inlined, where most of its checks then prove
  redundant: mbedtls's ChaCha20 runs at the speed it has without them.
* The **platform's headers** are searched, after `-I`, and before cinrs's
  bundled ones, which supply what only a compiler has (`<stdarg.h>`,
  `<stddef.h>`, the intrinsics) — GCC's order. `-nostdinc` leaves the
  platform's out. A program for another machine (`--target`) gets the bundled
  headers, and whatever `--sysroot` and `-I` point at.
* Every definition that is not `static` is a **C symbol**, as in any C
  compiler.
* **A local declared without an initialiser is zero-filled**, which is GCC's
  `-ftrivial-auto-var-init=zero` — Rust may not read uninitialised memory.
  `-ftrivial-auto-var-init=uninitialized` leaves local arrays uninitialised,
  as GCC does by default; see [the options](#options).

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
| `-flto`, `-flto=thin` | link-time optimisation: `rust-lld`'s on x86-64 Linux (`-C linker-plugin-lto`), `rustc`'s `-C lto=fat` (`thin`) elsewhere; see [below](#link-time-optimisation) |
| `-march=`, `-mcpu=` | `rustc`'s `-C target-cpu`, `native` included; the feature macros (`__AVX2__`, …) follow what the processor has |
| `-mavx2`, `-mno-avx512f`, … | `rustc`'s `-C target-feature`, in GCC's names, and the macros with them (x86) |
| `--target=`, `--sysroot=` | another machine; see [Targets](#targets) |
| `-l`, `-l:file`, `-L`, `-Wl,…`, `-Xlinker`, `-pthread`, `-s`, `-pie`, `-no-pie` | as GCC's; `-pthread` also defines `_REENTRANT`; the program is position-independent either way |
| `-fuse-ld=bfd`, `gold`, `mold`, `lld` | the linker the C compiler runs for `rustc` — whose own default on x86-64 Linux is LLD — for a program; a shared library is always linked by LLD, the one linker that takes `rustc`'s version script beside the one for the C symbols |
| `-Werror`, `-Wno-error`, `-w` | every warning an error, as GCC has it — `#warning` included; no warnings |
| `-pedantic-errors`, `-Werror=pedantic` | an ISO `-std=` refuses the constraint violations GCC only warns about, with the error and a note saying so; see [Defaults](#defaults). `-pedantic` alone changes nothing |
| `-shared`, `-static`, `-rdynamic` | a shared library (see [below](#shared-libraries)); a static program (`-C target-feature=+crt-static`); all symbols in the dynamic table |
| `-fno-cinrs-checks`, `-fcinrs-checks` | Rust's run-time checks off, on |
| `-fno-cinrs-unwind`, `-fcinrs-unwind` | every function `extern "C"` rather than `extern "C-unwind"`, the default: 0.3 % fewer instructions on SQLite's speedtest1 (under 0.1 % with `-fno-cinrs-checks`), and `setjmp` and `longjmp` refused where they are written (see [setjmp and longjmp](#setjmp-and-longjmp)) |
| `-ftrivial-auto-var-init=zero`, `=uninitialized`, `=pattern` | what a local declared without an initialiser starts out as. `zero` is the default; `uninitialized` leaves a local **array** uninitialised — a `MaybeUninit`, reached through a raw pointer — so a large buffer is not cleared on every call, and makes reading an element nothing wrote undefined behaviour, as C does (libuv's event loop in miniature: 1.04 s → 0.006 s, gcc 0.009 s); scalars, structures and unions stay zero. `pattern` is taken as `zero` with a warning: GCC's `0xFE` bytes make a `_Bool` Rust may not have. A file's `#pragma cinrs auto_var_init` wins for that file. See [Locals declared without an initialiser](translation.md#locals-declared-without-an-initialiser) |
| `-funsigned-char`, `-fsigned-char`, `-m32`, `-m64` | checked against the target, whose answer cinrs takes |
| `--version`, `-dumpversion`, `-dumpfullversion`, `-dumpmachine`, `--help`, `-v`, `-save-temps` | `-dumpversion` is `14`, as `__GNUC__` says; `-v` on its own ends with GCC's `gcc version 14.2.0 …` line, saying it is compatible and not GCC, which is what a `configure` reads |
| `-print-search-dirs`, `-print-multiarch`, `-print-multi-os-directory`, `-print-prog-name=`, `-print-file-name=`, … | what libtool asks: the platform's library directories (Debian's layout), and a name handed back as GCC hands back one it has no file for |

A warning option GCC knows (`-Wall`, `-Wformat=2`, `-Wno-unused`, …), a
common `-f` code-generation option (`-fPIC`, `-fno-strict-aliasing`,
`-fvisibility=hidden`, …), `-pedantic`, `-Wpedantic` and `-pipe` change nothing here and
are accepted. An unknown `-f` option is too, with a warning, and so is an
unknown `-W` one — except under `-Werror`, where either is an error, as it is
in GCC: that is how a `configure` script finds out whether an option is
taken. An option that would change what a program means and that
cinrs cannot follow — `-fshort-enums`, `-fpack-struct`, `-fopenmp`,
`-fsanitize=` — is an error, never a silent difference, and so is anything
unknown. Not yet: `-imacros`, `-dD`, `-MG`.

## Targets

`--target=<triple>` (Clang's spelling) compiles for another machine: the Rust
standard library for it has to be installed (`rustup target add <triple>`),
and cinrs's model of the target decides the sizes, the alignments and the
predefined macros.

**musl** (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, …):
Rust ships musl's `libc.a` and its start files with the target, and `ccinrs`
links them with `rust-lld` itself, so a program builds — static — on a
machine with no C compiler, no linker and no C library: `rustup target add`
is all it takes, for the machine's own architecture or another. `-lm`,
`-lpthread`, `-ldl` and the like are part of musl's `libc.a` and are dropped.
The headers are cinrs's bundled ISO C ones; for POSIX, point `-isystem` at
musl's own (`/usr/include/x86_64-linux-musl` from Debian's `musl-dev`). There
is no `-shared`, Rust shipping musl only as an archive.

**`wasm32-wasip1` and `wasm32-wasip2`** need nothing else: the C library is
wasi-libc, which Rust ships with the target, and the C `main` is named as
wasi-libc's start code calls it. A program runs under any WASI runtime
(`wasmtime prog.wasm`); a `wasip2` program is a component, whose exit status is
only success or failure. What wasi-libc leaves out — `clock()`, signals,
`fork` — is left out here too.

**`wasm32-unknown-unknown`** has no system and no C library under it, so what
`ccinrs` makes of the objects is a module for a host — a browser, Node — to
call into, `-shared` or not:

* it exports every C function and object that is not `static` (`main`
  included, under its own name), and its memory;
* `malloc`, `calloc`, `realloc` and `free` are Rust's allocator, exported too,
  which is how the host hands the C a buffer; `memcpy`, `memmove`, `memset`,
  `memcmp` and `strlen` are the ones Rust's own code needs and its
  compiler-builtins supply;
* everything else the C calls and does not define — `printf`, `strcmp`,
  `qsort` — is an import from the `env` module, for the host to provide.
  There is no C library here, and `ccinrs` carries none; see
  [Limitations](#limitations).

```js
const { exports } = await WebAssembly.instantiate(bytes, { env: { host_log: console.log } });
const p = exports.malloc(16);
new Int32Array(exports.memory.buffer, p, 4).set([1, 2, 3, 4]);
exports.sum(p, 4);
```

A panic — a run-time check that failed — traps.

## Link-time optimisation

Each C file is a crate of its own, so without `-flto` nothing is inlined
from one file into another. With it, an object is an rlib which a link loads
as a crate, and the link optimises the C as one program. A call in a loop to
a function in another file becomes the loop's own code, as with GCC's LTO.

**Who optimises** depends on the linker. On x86-64 Linux, where `rustc` links
with its own `rust-lld`, and on musl, the rlib's object is LLVM bitcode and
`rust-lld` does the optimisation (`-C linker-plugin-lto`; ThinLTO, for `-flto`
and `-flto=thin` alike), as GCC's linker plugin does for it. The linker sees
every object in the link, so an object or an archive compiled without
`-flto` links beside the optimised ones — Redis links its `deps/` archives
into a server built with `-flto` — and the files that make a [weak
definition](#limitations) or a `common` one are compiled without it, an alias
the assembler makes having no business in a merged module: Redis's
`redismodule.h` makes three of its 125 files such. `-v` says which; nothing
else does, since nothing changes but how much is inlined across those files.
An object compiled with `-flto` is bitcode only, so `-fuse-ld` naming another
linker is ignored, with a warning, at a link that has one.

Elsewhere — another target, or `-fuse-ld` naming GNU ld, gold or mold at the
compile — `rustc` does it (`-C lto=fat`, or `thin`), over the C, cinrs's
runtime and Rust's standard library alike, and an object compiled with
`-flto` holds machine code too, so a link without `-flto` takes it as
ordinary code. A link with `-flto` that has an object or an archive compiled
without it is then made without link-time optimisation, every object as the
ordinary code it also holds: `rustc` keeps only the standard library's symbols
its own modules use, and the other machine code calls more of them by name.
And a weak definition is an ordinary one there, with the warning.

What an object compiled with `-flto` cannot do is go into an archive: an rlib
is an archive itself, and `ar` would make an archive of archives — the link
says so — as GCC's own LTO objects need `gcc-ar`.

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
* ELF only, for now: not macOS, Windows or WASI; and not musl.
  (`wasm32-unknown-unknown` makes a module whatever is asked; see
  [Targets](#targets).)

## What it has built

[ccinrs on real programs](ccinrs-real-programs.md) is the long version: 27
open-source programs — SQLite, CPython, OpenSSL, FFmpeg, git, nginx, Redis
and twenty more — built with their own build systems, their test suites run
and compared with GCC's test by test, and their speed measured.

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

## setjmp and longjmp

A `longjmp` is a Rust unwind that the function which called `setjmp`
catches, re-entering its body after that `setjmp`; [What the C
becomes](translation.md#non-local-jumps) is the translation and [cinrs's
limits](limitations.md#setjmp-and-longjmp) the whole list. For a C project:

* **What builds.** libpng's default configuration (`make check` 36/36),
  libjpeg-turbo with TurboJPEG (ctest 664/664), xz's tuktest unit tests
  (22/22) and Lua 5.4 with its test suite, each with gcc's output.
* **Every function is `extern "C-unwind"`**, since any file may be between a
  `longjmp` and its `setjmp`. It costs 0.3 % of the instructions on SQLite's
  speedtest1 with the run-time checks and under 0.1 % without them (calls
  that may unwind hold back some inlining and code motion; before the
  inliner's thresholds were raised for the checks it was 2 %), and nothing
  measurable on the [benchmark kernels](benchmarks.md), whose code is the
  same byte for byte.
  `-fno-cinrs-unwind` takes it back, and then `setjmp` and `longjmp` are
  located errors that name the option.
* **A `setjmp` may only stand where C17 7.13.1.1p4 allows it**, plus
  `r = setjmp(buf);`, `int r = setjmp(buf);`,
  `if ((r = setjmp(buf)) == 0)` and `if (!r || !setjmp(buf))`; anywhere else
  is a located error.
* **A `longjmp` costs about 1.2 µs**, plus about 130 ns per frame it crosses,
  not GCC's tens of nanoseconds: Lua's `error`/`pcall` loop is several times
  slower than gcc's build, its ordinary code is not.
* **Locals keep their latest values**, `volatile` or not, and a `cleanup`
  attribute does not run when a `longjmp` leaves its scope, as in GCC.
* **What cannot be done stops the program with a message** of cinrs's own,
  before anything is unwound: a `longjmp` out of a signal handler that
  interrupted the program's own code (a `SIGALRM` in a loop, a fault — one
  raised inside a library call is fine), onto a buffer whose `setjmp` has
  returned or ran on another thread or stack, or through a frame without
  unwind tables. A C library compiled by another compiler and crossed by a
  `longjmp` needs unwind tables, which GCC gives by default on x86-64.

## Limitations

Everything [cinrs's own limitations](limitations.md) lists applies; the ones a
C project meets first:

* **A weak definition** (`__attribute__((weak))` on a function with a body,
  or on an object the file defines) is a real one on ELF — another file's
  definition overrides it, the defining file's own calls included, and a
  shared library exports it weak — made with an assembler alias of a private
  body; so is a `common` tentative definition. Under **`-flto`** the file is
  compiled without it where the linker does the optimisation (x86-64 Linux;
  see [above](#link-time-optimisation)). Where `rustc` does — another file's
  override could be merged into the same module as that alias — and on macOS
  and Windows, it is an ordinary definition with a warning, and a second
  definition elsewhere is a duplicate symbol rather than an override.
* **`setjmp` and `longjmp`** work with limits; see [above](#setjmp-and-longjmp).
* **`long double` is `double`.** A literal `printf` or `scanf` format that
  names a `long double` argument with `L` (`%Lf`, `%.2Le`) is rewritten to `l`
  for the platform's C library, which then reads the `double` it is given;
  a `long double` passed any other way to a platform function whose own is
  wider (x87's on x86-64 Linux) is an error. A function the platform's headers
  do not declare is one of the program's own files, compiled by `ccinrs`, and
  takes the same `double` — Redis's `ld2string` and `string2ld`, declared in
  its own `util.h`, are called as they are defined.
* **A thread-local object with external linkage** is a Rust `thread_local!`,
  which has no C symbol; the file that defines one exports an accessor,
  `name.cinrs_tls`, returning the calling thread's copy, and a file that
  declares it `extern __thread` reaches it through that — so Redis's
  `__thread sds thread_reusable_qb` works across its files. An object file a
  C compiler made cannot reach such a variable, nor `ccinrs`'s files one a C
  compiler defined; a missing definition is an undefined `name.cinrs_tls`.
* **Only `rustc` links.** An object from another compiler links with
  `ccinrs`'s, but `ccinrs`'s objects need Rust's standard library, so another
  compiler's driver — `c++` linking a C++ program, say — cannot link them.
* **`wasm32-unknown-unknown` has no C library.** Beyond the `malloc` family
  and what Rust's compiler-builtins supply, every function of the standard
  library a program calls is an import the host has to provide, or the module
  does not instantiate. `ccinrs` does not carry a C library of its own — a
  partial one would be the wrong half for most programs, and a whole one is
  not what it is for; a third-party C library for WebAssembly may be linked
  like any other. For the standard library, `wasm32-wasip1` is the target.
* Developed and tested on Linux x86-64 (and aarch64 musl under qemu), and for
  `wasm32-wasip1` and `-p2`; macOS and Windows are not tried yet.
* A program is bigger than GCC's: it carries Rust's standard library, and
  the run-time checks.
