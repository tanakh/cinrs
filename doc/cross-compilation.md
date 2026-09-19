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

The C library is still the platform's, and nothing here can check that the
library on the other end agrees with the header cinrs bundled. The Windows
branch keeps to the portable UCRT subset — `__acrt_iob_func` for `stdin` and
its two siblings, `_errno()` for `errno` — which both the Microsoft library
and mingw-w64 export; it is compiled for by the test suite and has not been
*run*. The same is true of every target but the host.
