# Changelog

All notable changes to `cinrs` and its companion crates — `cinrs-core`,
`cinrs-macros` and `cinrs-rt`, which are versioned together with it — are
recorded here. The format follows [Keep a Changelog][kac], and the project
follows [Semantic Versioning][semver].

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## 0.1.0 — unreleased

First release. A procedural macro that takes a C translation unit and
translates it to Rust, with every generated token carrying the span of the C it
came from, so that `cargo` and an IDE put the caret on the C. The
[README](README.md) is the long form; this is what is in it.

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

### Toolchain

Rust **1.88** or later (verified on 1.88.0, 1.90.0 and 1.98.1). *Defining* a
variadic function needs Rust 1.99's `c_variadic`; below that it is a located
error, and declaring and calling one works on every supported version.
