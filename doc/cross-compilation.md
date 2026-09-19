# Cross-compilation

`sizeof`, `_Alignof`, member offsets, bit-field storage, the type an integer
constant gets, the value of an `#if`, the predefined macros and therefore which
branch each bundled header takes are all worked out while the macro is
expanding — from a model of the machine the code will *run* on. A procedural
macro cannot ask `rustc` what that machine is, so the crate being built says
so, from its own build script:

```rust
// build.rs
fn main() {
    println!(
        "cargo:rustc-env=CINRS_TARGET={}",
        std::env::var("TARGET").expect("Cargo sets TARGET for a build script")
    );
}
```

`cargo:rustc-env` reaches the very `rustc` process that runs the macro, and
Cargo makes the value part of the crate's fingerprint, so changing `--target`
rebuilds. That is the whole recipe: with it, `cargo build --target
i686-unknown-linux-gnu` translates the C for a 32-bit machine, and
`--target x86_64-pc-windows-msvc` for one where `long` is four bytes and
`wchar_t` is two.

A single unit can override it:

```c
#pragma cinrs target "aarch64-unknown-linux-gnu"
```

which has to be written in that unit's own text, before any `#include` or
`#if` — the model is settled before the first directive is read, so a pragma
after one would be a lie, and cinrs says so instead of half-applying it.

With neither, the model is the machine the macro itself was compiled for.

## The supported families

| Data model | Targets |
| --- | --- |
| **LP64** — 64-bit `long` and pointers | `x86_64-*`, `aarch64-*`, `riscv64*`, `powerpc64*`, `s390x-*`, `loongarch64-*`, `mips64*`, `sparc64-*` on Linux, Darwin, the BSDs or bare metal |
| **LLP64** — 64-bit pointers, 32-bit `long`, 16-bit `wchar_t` | `*-windows-msvc`, `*-windows-gnu`, `*-uwp-windows-*` at 64 bits |
| **ILP32** — 32 bits throughout | `i686-*`, `i586-*`, `armv7*`, `thumb*`, `riscv32*`, `wasm32-*`, `mips-*`, `powerpc-*`, `sparc-*`, and `x86_64-*-gnux32` |

Plain `char`'s signedness follows `core::ffi::c_char`: unsigned on AArch64,
Arm, PowerPC, RISC-V and s390x, **except** on Windows and Apple's platforms,
which make it signed; signed everywhere else, wasm32 and LoongArch included.
`long long` and `double` are eight-byte aligned everywhere but 32-bit x86 off
Windows, where the i386 System V ABI makes them four — which changes how a
`struct` is laid out, and which `rustc` splits the same way.

Three things are refused rather than guessed at: `__int128` on a 32-bit
architecture (as GCC refuses it, leaving `__SIZEOF_INT128__` undefined so a
program can guard on it), a **bit-field on a big-endian target** (cinrs
allocates from the least significant end, which is not how a big-endian ABI
does it), and an architecture whose data model is none of the three —
`avr`, with its 16-bit `int`, is named in the diagnostic. A triple the table
does not know is an error listing the families that are in it.

[`doc/c-status.md`](c-status.md) has the full table, family by family.

## The assertion that guards it

Every expansion opens with a `const _: () = { assert!(…); };` block stating
the model it was translated for — one assertion per width, one for plain
`char`'s signedness, one for the alignment of `long long` and `double`, and,
in a unit that uses `__int128`, one for its alignment. They are written over
the `core::ffi` aliases, which follow the *real* target, so a build that forgot
the build script fails like this:

```text
error[E0080]: evaluation panicked: cinrs: 'long' is 8 bytes in the data model
this unit was translated for, and is not on this target. Translated for LP64
(x86_64-linux, signed 'char', 32-bit 'wchar_t'), chosen from the host,
CINRS_TARGET being unset; set CINRS_TARGET from a build script
(cargo:rustc-env=CINRS_TARGET=$TARGET) or write #pragma cinrs target.
```

with the caret on the C, rather than compiling into a program whose every
`sizeof` is wrong.

## The Microsoft library's inline `printf`

The C library is still the platform's, and the one thing cinrs has to do about
*which* library that is concerns the `printf` family. In the Universal CRT — the
Microsoft C library from Visual Studio 2015 on — `printf` and `scanf` and their
relatives are **inline functions in `<stdio.h>`**, written over
`__stdio_common_vfprintf` and its siblings, so the import library exports no
`printf` at all and a declaration of one is `LNK2019: unresolved external symbol
printf` at link time. Microsoft ships `legacy_stdio_definitions.lib` with
out-of-line definitions for exactly this case, and it is what Rust's own `libc`
crate links for the same declarations.

So a unit translated for an MSVC target whose generated `extern` block declares
any of

```text
printf  fprintf  sprintf  snprintf  vprintf  vfprintf  vsprintf  vsnprintf
scanf   fscanf   sscanf   vscanf    vfscanf  vsscanf
```

or any of the twelve wide forms (`wprintf`, `fwprintf`, `swprintf`, … ,
`vswscanf`) carries `#[link(name = "legacy_stdio_definitions")]` exactly as
`#pragma cinrs link "legacy_stdio_definitions"` would have — and a unit that
writes that pragma itself gets the attribute once rather than twice. The rule
lives in the code generator rather than in `<stdio.h>`, because a program may
declare `int printf(const char *, ...);` itself, or reach the function through
`__builtin_printf`, and never include a header: it is keyed on the symbols the
block links by. It applies to `*-windows-msvc` and `*-uwp-windows-msvc`, and
**not** to mingw-w64 — `*-windows-gnu` and `*-windows-gnullvm` — which has its
own out-of-line definitions and no such library to link. Nothing else about the
C library needs asking for: the Rust runtime already links it.

The list is the library's own contents, read out of a real
`legacy_stdio_definitions.lib` (MSVC 14.44, x64 and x86 alike) rather than
inferred — `snprintf` and `vsnprintf` included, which is worth knowing because
they are C99 additions that never had an out-of-line form before the UCRT.

The Windows branch of the bundled headers keeps to the portable UCRT subset
otherwise — `__acrt_iob_func` for `stdin` and its two siblings, `_errno()` for
`errno` — which both the Microsoft library and mingw-w64 export.

Three groups of functions **are** in the same position as `printf` and are *not*
worked around: a bundled header declares the C name, and no library on an MSVC
link line has it, so a unit that calls one fails with `LNK2019`. They are

* `<math.h>`'s `fabsf`, `frexpf`, `ldexpf` and `hypotf`, which Microsoft's
  `<math.h>` makes inline over the `double` forms — the other float functions
  (`sinf`, `sqrtf`, `powf`, `floorf`, …) are exported and work;
* `<wchar.h>`'s `wmemcpy`, `wmemmove`, `wmemset`, `wmemcmp` and `wmemchr`, plus
  `mbsinit` and `fwide`, inline for the same reason — `wcslen` and the rest of
  the header are exported;
* the whole of `<time.h>` but `clock`, `asctime` and `strftime`. The Windows
  SDK's `ucrt.lib` exports `_time64`, `_difftime64`, `_mktime64`, `_localtime64`,
  `_gmtime64`, `_ctime64` and `_timespec64_get`, and none of `time`, `difftime`,
  `mktime`, `localtime`, `gmtime`, `ctime` or `timespec_get`, which Microsoft's
  own `<time.h>` renames onto the first set with a macro.

mingw-w64 is unaffected by all three: `ucrtbase.dll` exports the plain names and
mingw's import libraries expose them. On an MSVC target a program can ask for
the real symbol by hand, which is what an `__asm__` label is for —
`long long now(long long *t) __asm__("_time64");` — and `tests/portability.rs`
leaves the `<time.h>` calls out on Windows, with the reason written where it
does so.

## What is run where

Nothing above says that the library on the other end agrees with the header
cinrs bundled; only a program built, linked and run on the platform does. That
is what `tests/portability.rs` is for, and this is how far each platform is
taken:

| Platform | How far |
| --- | --- |
| **`x86_64-unknown-linux-gnu`** | the development platform: the whole test suite, the three conformance corpora, and the differential tests that compile the same C with `gcc` and `clang` and compare |
| **macOS (arm64) and Windows (x86-64, MSVC)** | built, linked and **run** on every push by the `portability` job in `.github/workflows/ci.yml`: both examples, and `tests/portability.rs` — the C library through the bundled headers, every value asserted from Rust — beside six behavioural files |
| **`i686-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `x86_64-apple-darwin`, `wasm32-unknown-unknown`** | compile-checked with a real `cargo check --target` — `tests/cross_targets.rs`, which also shows the data-model assertion firing when `CINRS_TARGET` is left out |
| **everything else in the table above** | the front end only: every bundled header is compiled for a model of each family, and the widths, alignments, layouts and predefined macros are asserted in-process |
