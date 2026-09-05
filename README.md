# cinrs: Write C code in Rust

This is a library that implements a procedural macro allowing C code to be written within Rust code.

## Example

```rust
use cinrs::c99;

c99! {
    int fact(int n) {
        if (n == 0) {
            return 1;
        } else {
            return n * fact(n - 1);
        }
    }
}

// The generated functions are `extern "C"`, so calling one is `unsafe`.
let v = unsafe { fact(10) };
println!("fact(10) = {v}");
```

Run it with `cargo run --example fact`.

## What works

* **Five standards, twice over.** `c89!` (also spelled `c90!`), `c99!`, `c11!`,
  `c17!` and `c23!` are the
  same macro for five revisions of the language, and `__STDC_VERSION__`
  follows — except in `c89!`, which leaves it undefined, because C89 as
  published had no such macro; `gnu89!`, `gnu99!`, `gnu11!`, `gnu17!` and
  `gnu23!` are the same five with the
  GNU extensions switched on. `c11!` adds
  `_Static_assert`, `_Generic`, `_Alignof`, `_Alignas` (on the members of a
  `struct` or `union`), `_Noreturn` and anonymous `struct`/`union` members;
  `c17!` is `c11!` with a different version macro; `c23!` adds the keywords C23
  promoted (`bool`, `true`, `false`, `nullptr`, `static_assert`, `alignof`,
  `alignas`, `thread_local`, `constexpr`, `typeof`), `[[…]]` attributes,
  `__VA_OPT__`, `#elifdef`/`#elifndef`, binary constants, digit separators,
  empty initialisers, `auto` type inference, enumerations with a fixed
  underlying type and `unreachable()`. A feature from a later revision used in
  an earlier block is a diagnostic that says which macro to write instead — and
  `c89!` is that rule pointed the other way, refusing everything C99 added
  (`//` comments, mixed declarations and code, `long long`, designated
  initializers, variable length arrays, `_Bool`, `restrict`, `inline`, …) with
  the same message. The
  revision also decides what `int f();` means: the parameters are *unspecified*
  before C23, so a call may pass any number of arguments and each gets the
  default argument promotions, while `c23!` and `gnu23!` read the empty list as
  `(void)` — which is exactly where the standard moved it.
* **The C of the 1980s.** Old-style (K&R) function definitions —
  `int f(a, b) int a; char *b; { … }` — work in every entry point below
  `c23!`, which is the revision that removed them. Their type has no
  prototype, so a caller applies the default argument promotions, and the
  generated item takes the promoted types and converts to the declared ones on
  entry. `c89!` and `gnu89!` add the two rules C99 deleted: **implicit `int`**
  (`static x;`, `f() { … }`) and **implicit function declarations**, where
  calling an undeclared `abs` declares `extern int abs();` and the linker
  resolves it. `gnu89!` is otherwise `gnu99!`: `gcc -std=gnu89` takes every
  later feature as an extension, and so does this.
* **The C99 language.** All the arithmetic types, pointers, arrays, `struct`,
  `union`, `enum`, bit-fields, `typedef`, string literals, function pointers,
  `sizeof` with
  the real layout, casts, aggregate and designated initialisers — designator
  *lists* included, so `{ .a.b = 1 }`, `{ .arr[2].x = 3 }` and the elements
  that carry on from where one of them landed all work — compound
  literals — `&(struct S){ 1, 2 }`, whose object lives as long as the block it
  is written in — variable length arrays, file-scope,
  `static` and `extern` objects, every operator, every control structure —
  `if`, `while`, `do`/`while`, `for`, `switch` with fallthrough, `break`,
  `continue`, `return`, and `goto`, which is lowered to a state machine over
  basic blocks.
* **Every character set C has.** Digraphs, the bundled `<iso646.h>`, and the
  nine **trigraphs**, replaced in translation phase 1 wherever the revision
  still has them — every strict entry point below `c23!`, which is where C
  removed them, and no GNU dialect, which is the line GCC draws.
  **Extended identifiers**: `int café(void)` and `int café(void)` are
  one function, in Unicode Annex #31's character set, refused when the name is
  not in Normalization Form C. And the **Unicode literals** — `u8"…"`, `u"…"`
  and `U"…"` with their `char8_t`, `char16_t` and `char32_t`, `u'x'`, `U'x'`
  and C23's `u8'x'`, surrogate pairs and all — with `<uchar.h>` bundled.
* **Variable length arrays and `alloca`.** `int a[n];` with a bound that is not
  a constant does what C99 says: the bound is evaluated once, at the
  declaration; the object lives to the end of the block and is made afresh on
  every pass through a loop; `sizeof a` is a run-time value. `alloca` — the
  bundled `<alloca.h>`, or `__builtin_alloca` — gives memory that lives until
  the *function* returns. Both are emulated on the heap, since Rust cannot move
  the stack pointer by an amount chosen at run time, so the storage is not the
  stack and the two of them are the only constructs whose expansion needs more
  than `core`. What a C program can observe — the elements, the lifetimes, the
  run-time `sizeof` — is unchanged. One dimension is what is supported:
  `int a[n][3]` is fine, and `int a[3][n]`, `int (*p)[n]` and a `typedef` of a
  VLA are located errors, as is a `goto` or a `case` that would jump into the
  scope of one.
* **The C99 preprocessor.** Object-like and function-like macros with `#`,
  `##`, `__VA_ARGS__` and the standard's rescanning rules, every conditional
  directive, `#error`, `#warning`, `#pragma`, and `#line` — which redirects
  `__LINE__` and `__FILE__` and nothing else, so a diagnostic still points at
  the C token that was really written.
* **`#include`, and C23's `#embed`.** Standard headers (`<stdio.h>`,
  `<string.h>`, `<math.h>`, `<wchar.h>`, `<uchar.h>`, `<iso646.h>` and the
  rest) are bundled with the crate, written in plain C99
  rather than read from the platform, and the calls link against the real C
  library. Your own headers are found next to the `.rs` file that includes
  them, and editing one rebuilds the crate. `#embed "logo.png"` puts the bytes
  of a file into the program — with `limit`, `prefix`, `suffix`, `if_empty`
  and `__has_embed` — and editing *that* rebuilds the crate too.
* **The GNU extensions.** Statement expressions (`({ … })`), `typeof`,
  `__attribute__((packed))` and `aligned` with the layout GCC gives them,
  `#pragma pack`, `case 1 ... 5:`, range designators, flexible array members,
  `asm` labels, `constructor`/`destructor`, `__func__`, the `__builtin_*`
  family — bit counting, checked overflow, `__builtin_expect`,
  `__builtin_types_compatible_p` — `, ## __VA_ARGS__`, `__COUNTER__`,
  `__has_include`, `__has_attribute` and the rest. Everything spelled with a
  leading double underscore works in `c99!` too, exactly as it does in GCC's
  own `-std=c99`; only the plain spellings `typeof` and `asm`, and the features
  of later revisions, need `gnu99!`. Inline *assembly* is a clear error rather
  than a guess, and so is every other extension with no honest translation.
  [`doc/gnu-extensions.md`](doc/gnu-extensions.md) is the catalogue, row by
  row.
* **`__int128`.** GCC's 128-bit integers, in every entry point, as Rust's
  `i128` and `u128` — whose x86-64 ABI has matched `__int128`'s since Rust
  1.77. Sixteen bytes, ranked above `long long`, so `(__int128) a * b` really
  is a 128-bit multiplication; `__int128_t` and `__uint128_t` are predefined
  names for the same two types and `__SIZEOF_INT128__` is `16`. Bit-fields of
  them work, wider than sixty-four bits included. C has no 128-bit *literal*
  and neither does this: `((__int128) 1) << 100` is the idiom.
  Two gaps: `__builtin_add_overflow` and its relatives compute their check one
  width up from the operands, and there is nothing above 128 bits, so a
  128-bit *operand* is refused (a 128-bit *result* is fine); and
  `va_arg(ap, __int128)` needs a `VaArgSafe` implementation Rust still keeps
  unstable, though *passing* one through `...` works.
* **Thread-local objects.** `_Thread_local`, C23's `thread_local` and GNU's
  `__thread` become a `std::thread_local!` holding an `UnsafeCell`, so a C
  counter really is one per thread and `&x` is a pointer to *this* thread's
  copy. C's own placement rules apply: file scope, or a block-scope `static`,
  with a constant initialiser. An `extern` thread-local object and exporting
  one under `#pragma cinrs export` are refused — both would need Rust's
  unstable `#[thread_local]`.
* **Pragmas that configure the unit.**
  `#pragma cinrs target "…"` picks the machine the unit is translated for, over
  the `CINRS_TARGET` a build script sets — see
  [Cross-compilation](#cross-compilation);
  `#pragma cinrs include_path "…"` adds a search directory;
  `#pragma cinrs link "…"` links a library;
  `#pragma cinrs export` gives everything with external linkage a real C
  symbol, so that another block — or a C library — can call it by name (with
  C's own risk: two exported units defining one name is a duplicate symbol);
  `#pragma cinrs no_std` takes the storage a variable length array or `alloca`
  needs from `alloc` rather than from `std`, and refuses a thread-local object,
  which needs `std` outright;
  `#pragma cinrs module "…"` names the module the expansion goes into.
* **Two input forms.** C the Rust lexer accepts is written as raw tokens —
  `int café(void)` included, since Rust's identifiers are UAX #31's too; C it
  refuses (hexadecimal floating constants, `'ab'`, the prefixed literals
  `L"…"`, `u8"…"`, `u"…"`, `U"…"` and `u8'x'`, a universal character name,
  `\` line
  continuations, C23's digit separators such as `1'000'000`) goes in a string
  literal instead. `##` cannot be written in
  raw-token form either, so a replacement list spells the pasting operator
  `a # # b`.
* **Errors that point at the C.** Every generated token carries the span of the
  C token it came from, so both `cargo` and an IDE put the caret on the C code
  — for the front end's own diagnostics and for `rustc`'s. Pointing *inside* a
  string literal needs `Literal::subspan`, which is unstable, so there the
  position is appended to the message instead; the crate's `nightly` feature
  turns it back into a caret.
* **Variadic functions.** Declaring and calling one works anywhere; *defining*
  one needs Rust 1.99's `c_variadic`, and is a clear error before that.

## Known limitations

* Not supported, each as a located error rather than a silent mistranslation:
  the variably modified types other than a one-dimensional array
  (`int a[n][m]`, `int (*p)[n]`, a `typedef` of a VLA), `_Complex`,
  `setjmp`/`longjmp`, `_Atomic`, `_BitInt`, and C23's *named* universal
  character `\N{LATIN SMALL LETTER E WITH ACUTE}`. On the GNU side: inline
  assembly, computed `goto`, `cleanup`, the vector extensions and the
  `__sync_*`/`__atomic_*` builtins. C11's four
  `__STDC_NO_*` macros are predefined, which is the standard's own way of
  saying that atomics, threads, VLAs and complex arithmetic are left out;
  `__STDC_NO_VLA__` stays defined although one-dimensional VLAs work, so that a
  program which tests it keeps taking its `malloc` path, and
  `__STDC_NO_THREADS__` stays defined although `_Thread_local` works, because
  `<threads.h>` does not.
* A bit-field has no address, so it is not a field of the generated Rust
  `struct`: a run of them shares one `pub __cinrs_bitsN: [u8; K]`, and each
  named member becomes a pair of inherent methods — `s.level()` reads it and
  `s.set_level(v)` writes it, in the member's declared C type. The C itself is
  unchanged (`s.level = 3`, `p->flags |= 1`, `switch (s.kind)`); the accessors
  are what *Rust* code on the other side uses.
* `_Alignas` and `__attribute__((aligned(N)))` are honoured on the members of a
  `struct` or `union`: the member moves to the boundary it asks for and the
  generated Rust item gets explicit padding so that both sides agree about
  where it went. On an *object* an alignment specifier is not supported at
  all.
* A `constexpr` object is a *constant*: its value is folded wherever the name
  is used (so it may be an array bound or a `case` label), and there is
  nothing to take the address of. Only the arithmetic types are accepted.
  `nullptr` has type `void *` rather than a `nullptr_t` of its own.
* `long double` is `double`: the extended precision, and the ABI that goes with
  it, are not there.
* `va_list` is `core::ffi::VaList`, which cannot be stored in a `struct` or
  returned; the usual uses — `va_start`, `va_arg`, `va_copy`, passing a list to
  `vprintf` — are fine.
* The platform's include directories are never searched. A real `<stdio.h>` is
  not C, so anything outside the bundled set is declared by hand or pointed at
  with an include path.
* Sizes and alignments come from a model of the target rather than from the
  target's own C compiler. Cross-compiling needs one line in a build script;
  see [Cross-compilation](#cross-compilation) below, and note that without it
  a cross build is a failed compile-time assertion rather than a program that
  computes the wrong thing.
* Each invocation is one translation unit. Two blocks may share a header, but
  the types it declares are then two distinct Rust types — one per unit.

## Cross-compilation

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

### The supported families

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

`doc/c-status.md` has the full table, family by family.

### The assertion that guards it

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

## `no_std`

Everything generated is `core`-only: `core::ffi` types, `#[repr(C)]` items, raw
pointers, byte strings, `core::hint::unreachable_unchecked` for `unreachable()`,
`core::mem::offset_of!` for `offsetof`, and the C library's own `abort` for
`__builtin_trap` and `assert`. The C library is still *linked*, because the C
code calls it — that is a link-time dependency of the program rather than a
Rust one.

Three constructs are the exception. Two of them are variable length arrays and
`alloca`, whose storage is a `Vec`. Nothing in the C says which kind of crate
the expansion is going into, so that `Vec` is `::std::vec::Vec` unless the unit
says otherwise:

```c
#pragma cinrs no_std
```

which makes it `::alloc::vec::Vec` instead. The crate then has to contain
`extern crate alloc;` itself — an expansion is items, and a crate-level
directive is not one of them. Without the pragma, a variable length array in a
`#![no_std]` crate is `rustc`'s own "cannot find `std`", with the caret on the
declaration that needed it.

The third is a **thread-local object**, and the pragma does not help there:
`thread_local!` is a `std` macro and `core` has no thread-local storage at all,
so `_Thread_local` under `#pragma cinrs no_std` is a located error saying
exactly that.

## Conformance

`cinrs` is measured against three public corpora — about 2,265 cases in about
four and a half minutes. [`doc/testsuites.md`](doc/testsuites.md) is the
overview: how to fetch them, the three modes each harness has, the
expected-failure lists and their markers, and **the memory ceilings a run has
to be given**, which are not optional.

* **[c-testsuite](https://github.com/c-testsuite/c-testsuite)** — whole
  programs with the output each must produce. Of the 220 in its `single-exec`
  suite, **213 of the 218 that `c99!` is eligible for pass (97.7 %)**, and 216
  of 220 under `c11!`, `gnu89!`, `gnu99!` and `gnu11!`. What is left is one
  construct
  listed as unsupported above — `va_arg` with a struct — plus two corners GCC
  has and this does not: a `goto` out of a statement expression, and
  initialising a flexible array member. One needs a newer Rust than 1.97, and
  the two C23 entry points give up one more case that C23 itself made invalid.
  Strict `c89!` is the outlier at 152 of the 175 it selects, because
  twenty-one cases the corpus tags `c89` use something C99 added.
  The corpus is a git submodule, so a fresh checkout skips the suite until
  `git submodule update --init third_party/c-testsuite` fetches it.
  [`doc/c-testsuite.md`](doc/c-testsuite.md) has the details.
* **[GCC's C torture tests](doc/gcc-torture.md)** — 1,769 self-checking
  programs, each a bug report distilled into twenty lines, where success is
  exit status zero. **1,369 pass (77.4 %)** under `gnu89!`, which is the
  language these C89-era programs were written in, and 1,279 (72.3 %) under
  `gnu11!`. What is left is inline assembly, the vector extensions, the
  `__builtin_*` forms this crate does not implement, `_Complex`, nested
  functions, and the variadic definitions that need Rust 1.99 — plus two
  programs that built and then did the wrong thing, which the document names
  one by one.
* **[Clang's C conformance tests](doc/clang-c-tests.md)** — one file per WG14
  paper or defect report, with `// expected-error` comments saying exactly
  which lines must be diagnosed. **96 of the 203 revisions run come out as
  required (47.3 %)**, and this is the only suite that measures what `cinrs`
  *refuses*, which is half of what a front end is for.

The last two are fetched by `scripts/fetch-testsuites.sh`, not checked in, and
each harness skips itself with a note when its corpus is missing.

## How it works

The macro recovers the C source text of its own invocation (by slicing the
`.rs` file, or by decoding the string literal) together with a map from byte
offsets back to `proc_macro2` spans, then runs a C front end over it — lexer,
preprocessor, parser, semantic analysis with C's conversion rules made
explicit — and emits Rust, `c2rust`-style: `#[repr(C)]` records, raw pointers,
wrapping arithmetic where C defines wrap-around, `pub unsafe extern "C" fn` for
each function. Each expansion goes into a private module of its own with a glob
re-export, so two blocks in one Rust module never collide. Every token it emits
is stamped with the span of the C it came from, which is what makes the errors
land where they should.

See the [crate documentation](https://docs.rs/cinrs) for the details.
