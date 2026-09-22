# Changelog

All notable changes to `cinrs` and its companion crates — `cinrs-core`,
`cinrs-macros` and `cinrs-rt`, which are versioned together with it — are
recorded here. The format follows [Keep a Changelog][kac], and the project
follows [Semantic Versioning][semver].

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## Unreleased

### Added

* **The x86 SIMD intrinsics.** `#include <immintrin.h>` and write Intel's
  intrinsics the way real C does: `__m128`, `__m128i`, `__m128d`, `__m256`,
  `__m256i` and `__m256d` are types, and 881 functions — SSE, SSE2, SSE3, SSSE3,
  SSE4.1, SSE4.2, AVX, AVX2, FMA, AES, PCLMUL, SHA, and the BMI1, BMI2, POPCNT
  and LZCNT scalar ones — are declared. There is no vector language and no new
  syntax: **the mapping is by name.** A call to `_mm_add_ps(a, b)` becomes
  `::core::arch::x86_64::_mm_add_ps(a, b)`, which works because `core::arch` was
  generated from the same Intel data Intel's own headers are. The bundled
  headers' prototypes were *read out of `core::arch`'s source* by a maintainer
  tool that writes both the header and the table driving the mapping
  (`crates/cinrs-core/tests/x86_intrinsics.rs`), so a declaration and the
  function it resolves to cannot drift apart — and a test that needs no
  toolchain checks the two committed files against each other on every run.
  `<xmmintrin.h>`, `<emmintrin.h>`, `<pmmintrin.h>`, `<tmmintrin.h>`,
  `<smmintrin.h>`, `<nmmintrin.h>`, `<wmmintrin.h>`, `<avxintrin.h>`,
  `<avx2intrin.h>`, `<x86intrin.h>` and `<popcntintrin.h>` are bundled too, in
  the layering GCC and Clang give them, so code written for an older compiler
  finds its instruction set under the header name it expects. The macros come
  with them: `_MM_SHUFFLE`, `_MM_SHUFFLE2`, `_MM_TRANSPOSE4_PS`, `_MM_HINT_*`,
  `_MM_FROUND_*`, `_CMP_*`, `_SIDD_*` and the rest, with the values GCC's
  headers give them.

  The vector types are ordinary objects of a known size and alignment —
  sixteen or thirty-two bytes, aligned to themselves — so they work as locals,
  parameters, return values, `static`s, array elements, `struct` members and the
  members of the `union { __m128i v; int32_t i[4]; }` that most code reaches a
  lane through. An **immediate operand** is a `const` generic in `core::arch`,
  so the argument is folded and written into a turbofish
  (`_mm_slli_epi32::<{ 3i32 }>(v)`); a non-constant is a diagnostic naming the
  intrinsic, which is what Intel's own compilers say too. Taking the **address**
  of an intrinsic works — GCC's are `static inline` functions, so real code does
  — through a private `extern "C"` shim per intrinsic; an intrinsic with an
  immediate operand has no address, and that is a diagnostic. **Nothing is
  exported**: those 881 prototypes add not one item to the unit's `extern` block
  or to its glob re-export, so a `use core::arch::x86_64::*` in the surrounding
  Rust cannot clash with them.

  See [SIMD intrinsics](doc/features.md#simd-intrinsics) for the coverage per
  instruction set and the nineteen intrinsics whose `core::arch` signature C
  cannot spell; `tests/simd.rs` runs every family, with the expected values
  computed in scalar C in the same block and cross-checked against `gcc -O2`.
  What stays refused, now with a diagnostic that names the intrinsic to write
  instead: the GNU vector extensions,
  `__attribute__((vector_size))` and `__builtin_ia32_*`. What is not here:
  AVX-512 (every one of its target features is still unstable in rustc on this
  crate's minimum supported version), and MMX and `__m64`, which `core::arch`
  dropped — `<mmintrin.h>` is an `#error` that names the SSE2 form of each
  intrinsic.

* **`__attribute__((target("avx2")))` and `#pragma GCC target`.** GCC's way of
  telling one function which instruction sets it may use becomes
  `#[target_feature(enable = "avx2")]` on the generated item, with GCC's name
  for an instruction set translated to LLVM's (`bmi` is `bmi1`, `pclmul` is
  `pclmulqdq`, `cx16` is `cmpxchg16b`, `sse4` is both halves of SSE4, `abm` is
  LZCNT and POPCNT together). Several names in one string, several attributes on
  one declaration, and successive pragmas all accumulate, as GCC's do; `#pragma
  GCC push_options` and `pop_options` bracket a region and an attribute on a
  function wins over the pragma. A name rustc has not stabilised, a processor
  (`target("arch=haswell")`) and turning an instruction set *off*
  (`target("no-avx")`) are each refused with the reason, because an ignored
  `target` would be a function compiled for the baseline and a program that
  faults on the first instruction it has not got. On a `[[cinrs::safe]]`
  function it is refused too: `#[target_feature]` makes a function unsafe to
  *call*, which is the opposite of what safe promises.

* **`__builtin_cpu_supports` and `__builtin_cpu_init`.** Since a procedural
  macro cannot see rustc's `-C target-feature`, only the architecture baseline is
  predefined — `__SSE__`, `__SSE2__`, `__SSE_MATH__` and `__SSE2_MATH__` on
  x86-64, and nothing above them — so `#ifdef __AVX2__` takes the other branch
  and the run-time question is the one to ask.
  `__builtin_cpu_supports("avx2")` is `std::is_x86_feature_detected!("avx2")`,
  reading the same `cpuid` leaves GCC's own builtin does and answering as an
  `int`; it takes GCC's spellings. `__builtin_cpu_init()` is accepted and does
  nothing, Rust's detection being lazy. Both need `std`, so
  `__builtin_cpu_supports` under `#pragma cinrs no_std` is a located error.
  `__builtin_cpu_is` is deliberately absent — it names a microarchitecture,
  which `std_detect` cannot answer — and `__has_builtin` says so.

### Changed

* **A `goto` no longer costs the loop it was written in.** The functions whose
  jumps Rust cannot make directly are still lowered into a control-flow graph,
  but that graph is now read back into Rust's own `loop`s, `if`s and `match`es
  by a [relooper](doc/translation.md#control-flow-and-goto) — Emscripten's
  algorithm — instead of being emitted as one flat `loop { match state { … } }`
  over block numbers. A `switch` comes out as a `match` with the case bodies in
  its arms and fallthrough as the code after it, a loop comes out as a Rust loop
  named after the C label its head stands at, and a jump forwards is a `break`
  of a labelled block. A state variable is left only where the C really is
  irreducible — a cycle with two heads, which is what a `goto` into a loop body,
  Duff's device and two loops that jump into each other's bodies each make —
  and it is one `u32` per such region rather than one per function. A computed
  `goto` keeps the old machine, whole, because `&&label` *is* a block's number.

  What it is worth, measured on one machine:

  | | before | after |
  | --- | ---: | ---: |
  | the `statemachine` kernel (a lexer written as a dozen `goto`s) | 2.16× `gcc -O2` | 1.00× |
  | one small SQLite query loop against a `gcc -O2` build of the same amalgamation | 4.24× | 0.97× |
  | `sqlite3VdbeExec`, instructions for that loop under `callgrind` (GCC's: 0.35 G) | 1.97 G | 0.36 G |

  Over SQLite's 2,610 defined functions, 13 were whole-function state machines
  and none is now: 12 are relooped outright and `sqlite3VdbeExec` has one
  irreducible region, `abort_due_to_error`, which the progress-callback loop at
  `vdbe_return` jumps back to. Over the GCC torture corpus 44 functions needed
  the machine and 19 do — every one of them a computed `goto`. `execute/medce-1`,
  which asks the optimiser to delete a call to an undefined `link_error()`
  inside `if (0)`, now compiles and passes. [`doc/benchmarks.md`](doc/benchmarks.md)
  was regenerated after the change: `statemachine` is at 0.99×, 32 of the 40
  programs (the SIMD kernel below included) are within 10 % of `gcc -O2` or
  faster, and every output still matches.

* **The bundled headers are ISO C; POSIX comes from the platform.** `<unistd.h>`,
  `<fcntl.h>`, `<strings.h>` and `<sys/types.h>` are no longer bundled. They were
  small incomplete copies — no `access`, `fsync` or `sysconf`, no `struct flock`
  or `F_RDLCK`, no `ETIMEDOUT` — and worse, a program that switched the
  platform's own headers on with `#pragma cinrs system_include` still got the
  bundled ones, which is the opposite of what it had asked for. The story is one
  sentence now: the bundled set is the headers ISO C describes (plus
  `<alloca.h>`, which is this crate's own), and POSIX comes from the platform,
  complete and consistent with `<sys/stat.h>` and `<pthread.h>`.

  **This is breaking for a program that included one of those four without the
  pragma.** Add one line:

  ```c
  #pragma cinrs system_include
  #include <unistd.h>
  ```

  or set `CINRS_SYSTEM_INCLUDE=1` for the whole crate. A program that asks for a
  POSIX header with the switch off now gets a diagnostic that says so and names
  the pragma, rather than only the directories that were searched.

  Two things follow. **Apple's platforms get a default** at last: the SDK's
  include directory is asked of `xcrun --show-sdk-path`, once per process, so a
  macOS user switches the platform on exactly as a Linux user does instead of
  having to set `CINRS_SYSTEM_INCLUDE_PATH`. And **the bundled `<errno.h>` is
  complete**: the whole POSIX.1-2017 `E*` set, at each platform family's own
  values — glibc and musl's from the kernel's `asm-generic`, Apple's from xnu,
  Windows' from the Universal CRT, with `EDEADLK` 36 and `ETXTBSY` 139 there
  rather than the Linux 35 and 26, and Darwin's `ENOTSUP` 45 distinct from its
  `EOPNOTSUPP` 102. MIPS and SPARC on Linux get only what ISO C requires, their
  kernels renumbering everything above 34.

* **A bundled header and a platform header may now define the same type.** In
  plain `system_include` mode a unit holds both sets at once — the bundled
  `<time.h>` for `struct timespec`, glibc's `<pthread.h>` for
  `pthread_cond_timedwait` — and that used to be three errors: "redefinition of
  'struct timespec'", "redefinition of 'struct tm'" and "macro 'CLOCKS_PER_SEC'
  redefined". Every definition in the bundled `<time.h>` now tests and then
  claims the per-type guard macro glibc, musl and mingw-w64 use for the same
  type (`_STRUCT_TIMESPEC`, `__struct_tm_defined`, `__time_t_defined`,
  `__clock_t_defined`, and the `__DEFINED_*` and `_*_DEFINED` spellings), the way
  mingw-w64 and musl coexist with each other; and where a bundled header and a
  platform header define the same *macro* differently, the platform's definition
  wins without a diagnostic, since the two are descriptions of one C library.
  `tests/system_headers.rs` compares every shared layout against `cc`'s over the
  platform's headers alone, because claiming a guard is a promise about layout.

* **A block under rust-analyzer no longer searches the crate from scratch.**
  Where the host gives a procedural macro no source positions, a raw-token block
  finds its own text by walking the crate's `.rs` files and proving which
  invocation is its own — and it used to do the whole walk again for every block.
  That is a hundred directories and five thousand entries here, 25 ms a block in
  the unoptimized build a proc-macro server runs, and some thirteen seconds of
  that server's time each time an editor re-analysed this repository's own five
  hundred blocks. The search now remembers, per process: the file list for two
  seconds, since one edit provokes a burst of expansions and nothing on disk
  moves between the first and the last; and each file's text for as long as the
  file still reports the modification time and length it was read at, which the
  search asks for anyway before reading it. A block costs 0.95 ms instead of
  25 ms, a file that was just saved is read again by the very next expansion, and
  nothing about which invocation is found can change — every candidate is still
  charged against the same byte budget, and a text that is out of date fails to
  match token for token exactly as an unsaved buffer's does. `cargo build` never
  went near any of this: it has the positions. See
  `crates/cinrs-core/src/locate.rs`.

### Added

* **SQLite compiles.** `scripts/check-sqlite.sh` downloads the SQLite 3.53.4
  amalgamation — 9.5 MB, 269,649 lines of C in one file — verifies it against
  the SHA3-256 sqlite.org publishes, and builds and runs
  `tests/sqlite-fixture/`: one `include_gnu11!`, in both of SQLite's threading
  configurations, with an in-memory database, a `CREATE TABLE`, three inserts, a
  `SELECT` through `sqlite3_prepare_v2`/`step`/`column_int` and a Rust
  `extern "C"` function called from SQL. The 9 MB is not committed and the check
  is in `scripts/ci.sh --full` only, since it needs the network; it needs Rust
  1.99 as well, because the amalgamation *defines* twenty variadic functions.
  It is the largest single C translation unit anyone ships, and what it
  exercises that no small program does — and what it costs — is in
  [`doc/testsuites.md`](doc/testsuites.md).

* **Atomic operations on a function pointer.** Loading, storing, exchanging and
  compare-exchanging an object of function-pointer type — `_Atomic(void (*)
  (void))`, `<stdatomic.h>`'s `atomic_store` on one, and the `__atomic_*`,
  `__sync_*` and `__c11_atomic_*` builtins — used to be refused with "a
  function pointer is an `Option<fn>` in Rust, which no atomic holds". It goes
  through the same `AtomicPtr<c_void>` as any other pointer now, with a
  `transmute` at each end: an `Option<unsafe extern "C" fn(…)>` is
  pointer-sized and uses the null pointer as its `None`, which is exactly the
  representation C gives a function pointer. Arithmetic (`__atomic_fetch_add`
  and friends) is still refused, because C has none on a function pointer
  either. See `tests/atomics.rs`.

### Fixed

* **`__USER_LABEL_PREFIX__` is predefined.** GCC defines it as *nothing* on ELF
  and as `_` on Mach-O and 32-bit COFF, and glibc builds every large-file
  redirection out of it:

  ```c
  #define __ASMNAME(cname) __ASMNAME2 (__USER_LABEL_PREFIX__, cname)
  #define __ASMNAME2(prefix, cname) __STRING (prefix) cname
  extern int open64 (…) __asm__ (__ASMNAME ("open64"));
  ```

  With the macro undefined, `__STRING` stringified the token itself and the
  symbol came out as `__USER_LABEL_PREFIX__open64` — which compiled and then
  failed to *link*, the worst kind of wrong. It bit every program that reached
  glibc's `<fcntl.h>` or `<sys/stat.h>` with `#pragma cinrs system_include`.

* **`_Float32` and its relatives may be defined by a `typedef`.** TS 18661-3
  lets an implementation either make `_Float32` a keyword or leave it to the
  library, and glibc does the second: with `_GNU_SOURCE` its
  `bits/floatn-common.h` writes `typedef float _Float32;` for a compiler that
  has no such keyword, and `<math.h>` then declares the `f32`/`f64`/`f32x`
  function families in terms of it. Those names were refused outright, which
  cost **1,671 errors on one `#include <math.h>`** under `_GNU_SOURCE` and made
  `#pragma cinrs system_include first` unusable for any program that defines it —
  SQLite does. A name a `typedef` has defined is now an ordinary `typedef` name;
  only the *keyword* use, with nothing having defined it, is still refused with
  the reason. `doc/system-headers.md`'s table is re-measured with `_GNU_SOURCE`
  as well as without: sixty-six of the sixty-seven platform headers either way.

* **An address constant may be offset by an *expression*.** C99 6.6p9 lets the
  integer a static initialiser's address constant is offset by be any integer
  constant expression, not only a literal; only a literal was recognised, so
  `static const unsigned char *p = &table[256 - OP_Ne];` — and the
  `((void *)(intptr_t)(flags | MASK))` a table of descriptors is full of — were
  refused as "initializer is not a compile-time constant expression". The
  integer parts of such an initialiser are now folded before the check, which
  also keeps the arithmetic out of the generated `static`. See
  `tests/address_constants.rs`.

## 0.1.0 — 2026-09-22

First release. A procedural macro that takes a C translation unit and
translates it to Rust, with every generated token carrying the span of the C it
came from, so that `cargo` and an IDE put the caret on the C. The
[README](README.md) and the documents under `doc/` are the long form; this is
what is in it.

### The language

* **Ten entry points.** `c89!` (`c90!`), `c99!`, `c11!`, `c17!` and `c23!`, and
  the five `gnu…!` dialects with the GNU extensions switched on. A construct
  from a later revision is a diagnostic naming the macro to write instead, and
  `c89!` is that rule pointed the other way. `include_c99!("file.c")` and one
  such macro per entry point read a C file instead of a block.
* **C99 and after**: the arithmetic types, pointers, arrays, `struct`, `union`,
  `enum`, bit-fields, `typedef`, function pointers, aggregate and designated
  initialisers, compound literals, variably modified types and `alloca`, the
  full preprocessor, `#include` with bundled standard headers, C23's `#embed`,
  C11's `_Atomic`, `<stdatomic.h>` and `<threads.h>`, C23's keywords,
  `[[…]]` attributes and `<stdckdint.h>`, `__int128`, thread-local objects,
  complex numbers, trigraphs, digraphs and the Unicode literals.
* **K&R C**: old-style function definitions in every entry point below `c23!`,
  and implicit `int` and implicit function declarations in `c89!`/`gnu89!`.
* **The GNU extensions**: statement expressions, `typeof`, `packed`/`aligned`,
  `cleanup`, `#pragma pack`, case ranges, flexible array members, labels as
  values and computed `goto`, nested functions, `constructor`/`destructor`,
  the `__builtin_*` family, and the rest — catalogued in
  [`doc/gnu-extensions.md`](https://github.com/tanakh/cinrs/blob/master/doc/gnu-extensions.md).
  What has no honest
  translation (inline assembly, the vector extensions, `setjmp`/`longjmp`) is a
  located error rather than a mistranslation.
* **Safe functions.** `[[cinrs::safe]]`, `__attribute__((cinrs_safe))` and
  `#pragma cinrs safe f g` generate a function whose body is not wrapped in
  `unsafe`, so `rustc` checks the translation and Rust calls it without
  `unsafe`.
* **A model of the target**, so that `sizeof`, `_Alignof`, offsets, bit-field
  storage and `#if` mean what they will mean on the machine the code runs on:
  LP64, LLP64 and ILP32, chosen from `CINRS_TARGET`, `#pragma cinrs target` or
  the host, and guarded by a compile-time assertion in every expansion.
* **A header is a binding.** A function a unit declares and does not define —
  everything `#include <zlib.h>` brings in — is callable from Rust under its C
  name, and so are the header's types; a Rust `mod` around the invocation
  gives them a path. A macro needs a line of C written in the same block.
* **Works under rust-analyzer**, which hands a procedural macro no source
  positions: a raw-token block recovers its own text from the crate's sources,
  so the editor shows the same diagnostics — and the same carets — as `cargo`.

### Conformance and speed, as measured for this release

* c-testsuite: **214 of the 218 cases `c99!` is eligible for are correct
  (98.2 %)**, 217 of 220 under `c23!`.
* GCC's C torture tests: **1,515 of the 1,769 run are correct (85.6 %)** under
  `gnu11!`, and 1,573 (88.9 %) on a toolchain with `c_variadic`.
* Clang's C conformance tests: **167 of the 203 revisions run are correct
  (82.3 %)**, and 557 of the 620 `expected-error` lines land on the right line.
* **Not one case in any of the three is tagged `[bug]`.**
* 39 whole C programs built as `gcc -O2`, `clang -O2` and a `cinrs` block:
  median `cinrs`/`gcc -O2` ratio **1.01×**, 30 of 39 within 10 % of `gcc -O2`
  or faster, every program's output identical across the three builds.

### Features

* `complex` (default) — C's complex types, which is the one thing the generated
  code needs a crate for. Off: the `cinrs-rt` dependency goes away,
  `__STDC_NO_COMPLEX__` is predefined and `_Complex` is a diagnostic.
* `nightly` — diagnostics pointing *inside* a string-literal body, which needs
  `proc_macro::Literal::subspan` and therefore a nightly compiler.

### Toolchain and platforms

Rust **1.88** or later (verified on 1.88.0, 1.90.0 and 1.98.1). *Defining* a
variadic function needs Rust 1.99's `c_variadic`; below that it is a located
error, and declaring and calling one works on every supported version.

Developed and fully tested on x86-64 Linux. On macOS (arm64) and Windows
(x86-64, MSVC) the examples and the portable tests are built, linked and run in
CI — on MSVC the `printf` family links `legacy_stdio_definitions` and the
`<time.h>` functions link their 64-bit UCRT names, automatically. Other targets
are compile-checked.
