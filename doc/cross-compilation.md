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

The C library is still the platform's, and there are two things cinrs has to do
about *which* library that is. The first concerns the `printf` family; the
second, [below](#the-names-the-microsoft-library-exports-differently), the
`<time.h>` functions. In the Universal CRT — the
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

## The names the Microsoft library exports differently

The other shape of the same problem: the function *is* in `ucrt.lib`, under a
name that is not the one C gives it. Microsoft's own headers paper over the
difference with a macro — `#define time _time64` — which cinrs's bundled
headers, the same text on every platform, do not write. So on an MSVC target a
**declaration** of one of these links by the name the library really has, which
is what `#[link_name]` says:

| C name | links by |
| --- | --- |
| `time` | `_time64` |
| `difftime` | `_difftime64` |
| `mktime` | `_mktime64` |
| `localtime` | `_localtime64` |
| `gmtime` | `_gmtime64` |
| `ctime` | `_ctime64` |
| `timespec_get` | `_timespec64_get` |
| `hypotf` | `_hypotf` |

The table is `MSVC_RENAMED` in the code generator, beside `LEGACY_STDIO`, and
every row was read out of a real import library with `nm` — the Windows SDK's
`ucrt.lib` and the MSVC toolset's `msvcrt.lib`, 10.0.26100.0 and 14.44.35207.
As with the `printf` rule it is keyed on the symbol a declaration links by, it
applies to `*-windows-msvc` and `*-uwp-windows-msvc` and not to mingw-w64, and
it touches a declaration only: a C program that *defines* `time` has defined
`time`. An `__asm__("…")` label is the program's own answer and wins over the
table, which is also how a unit asks for `_time32` on purpose:

```c
/* `_time32` takes and answers a 32-bit `__time32_t`, which is a `long`. */
long now32(long *t) __asm__("_time32");
```

The `<time.h>` rows are **not** merely a link error, and that is what makes them
worth a table. The MSVC toolset's `msvcrt.lib` holds an alias-map object per
name that defines `time` as a *weak* external for `_time32`, and the rest
likewise, so a declaration of `time` does resolve — to the **32-bit** `time_t`
function. cinrs's `<time.h>` makes `time_t` eight bytes on Windows, as the UCRT
does, and the two then disagree in ways a program sees: `_mktime32`'s
`(time_t)-1` arrives as `0x0000_0000_FFFF_FFFF`, and `_gmtime32` answers a null
pointer for every date after 2038 rather than a `struct tm`. Naming `_time64`
and its siblings is a correctness fix and not only a convenience;
`tests/portability.rs` asserts both of those answers on the platform itself.

`timespec_get` and `hypotf` had no weak alias and were plain `LNK2019`s before.

### What is still not worked around

Ten functions are in the same position as `printf` but have **no** exported
equivalent at all — `nm` over `ucrt.lib`, `msvcrt.lib`, `vcruntime.lib` and
`oldnames.lib` shows no symbol of any spelling, in any library on an MSVC link
line — because Microsoft's headers make each one inline over a wider function.
A bundled header declares the C name, and a unit that calls one fails with
`LNK2019`. They are

* `<math.h>`'s **`fabsf`, `frexpf` and `ldexpf`**, inline over the `double`
  forms. The other float functions — `sinf`, `sqrtf`, `powf`, `floorf`, … — are
  exported and work, and so is `hypotf`, through the table above.
  `__builtin_fabsf` needs no library at all: cinrs lowers it to a sign-bit clear
  in `core`, exactly as it does `__builtin_fabs`. For the other two, `frexp` and
  `ldexp` *are* exported, and a `float` converts to a `double` and back exactly,
  which is what Microsoft's header does inline —
  `(float) frexp((double) x, &e)`.
* `<wchar.h>`'s **`wmemcpy`, `wmemmove`, `wmemset`, `wmemcmp`, `wmemchr`,
  `mbsinit` and `fwide`** — `wcslen`, `wcscmp`, `swprintf`, `wcstok`, `mbrtowc`
  and the rest of the header are exported. `memcpy`, `memmove`, `memset`,
  `memcmp` and `memchr` over `n * sizeof(wchar_t)` bytes are the five in plain
  C; `mbsinit(&s)` is true of a zeroed `mbstate_t`, which is the initial
  conversion state in every locale the UCRT has; `fwide` has no equivalent, and
  a UCRT stream is byte-oriented until a wide call orients it.

mingw-w64 is unaffected by any of this: `ucrtbase.dll` exports the plain names
and mingw's import libraries expose them, which is why both rules ask
`TargetModel::is_msvc` rather than merely whether the target is Windows. And on
an MSVC target a program can always ask for a symbol by hand with an `__asm__`
label.

One more MSVC rule follows from the `#[link(name = "…")]` those two put on the
generated `extern` block — and from the one
[`#pragma cinrs link`](pragmas.md#link-name) puts there. `rustc` reaches a
`static` declared in a block with such an attribute through a `dllimport`, which
is right for a symbol in another image and wrong for one defined in *this* one:
a unit that declares `extern int counter;` while another exported unit of the
same crate defines it reads rubbish, and `lld-link` says
`LNK4217: locally defined symbol imported`. Functions are unaffected. So on an
MSVC target, keep an `extern` object shared between two units of one crate in a
unit that names no library — or hand it over through an accessor function, which
is what Rust has to do for such an object anyway (see
[Objects](translation.md#objects)).

### Apple's C library

The same thing can happen on any platform, for a plainer reason: a bundled
header declares what the *standard* says the header holds, and a C library may
simply not implement all of it. Apple's is the one known case. Its SDK has no
`<uchar.h>`, and libSystem has no **`c16rtomb`, `c32rtomb`, `mbrtoc16` or
`mbrtoc32`**; it has no **`quick_exit`** or **`at_quick_exit`** either. The
types and the literals — `char16_t`, `u"…"`, `U'x'` — need no library and work;
a *call* to one of those six is `Undefined symbols for architecture arm64`
from the linker, exactly as it would be from C.

## What is run where

Nothing above says that the library on the other end agrees with the header
cinrs bundled; only a program built, linked and run on the platform does. That
is what `tests/portability.rs` is for, and this is how far each platform is
taken:

| Platform | How far |
| --- | --- |
| **`x86_64-unknown-linux-gnu`** | the development platform: the whole test suite, the three conformance corpora, and the differential tests that compile the same C with `gcc` and `clang` and compare |
| **macOS (arm64) and Windows (x86-64, MSVC)** | built, linked and **run** on every push by the `portability` job in `.github/workflows/ci.yml`: both examples, and the test files `scripts/portability-tests.txt` names — `tests/portability.rs`, which asserts the C library through the bundled headers value by value, beside thirty-five behavioural files, which is every one with no platform in it |
| **`i686-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `x86_64-apple-darwin`, `wasm32-unknown-unknown`** | compile-checked with a real `cargo check --target` — `tests/cross_targets.rs`, which also shows the data-model assertion firing when `CINRS_TARGET` is left out |
| **everything else in the table above** | the front end only: every bundled header is compiled for a model of each family, and the widths, alignments, layouts and predefined macros are asserted in-process |

A maintainer on WSL2 with Visual Studio installed on the Windows side can run
the Windows column locally, without waiting for CI:

```console
$ scripts/test-windows-from-wsl.sh
```

It finds the newest MSVC toolset and Windows SDK under `/mnt/c`, links with
`lld-link`, sets `CINRS_TARGET` — the macro runs on the *host*, so without it
every `sizeof` in the expansion would be the Linux answer — and runs the two
examples and the same `scripts/portability-tests.txt` list, one file at a time,
executing each `.exe` through WSL's interop. `--help` says what it needs, and
`--list` prints the list the CI job reads from the same file.
