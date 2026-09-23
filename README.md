# cinrs — write C inside Rust

[![CI](https://github.com/tanakh/cinrs/actions/workflows/ci.yml/badge.svg)](https://github.com/tanakh/cinrs/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/cinrs.svg)](https://crates.io/crates/cinrs)
[![docs.rs](https://docs.rs/cinrs/badge.svg)](https://docs.rs/cinrs)

`cinrs` is a procedural macro that takes C source written directly in a Rust
file — C89 to C23, with the GNU extensions — translates it to Rust while the
crate compiles, and hands you what it defines as ordinary Rust items.

```rust
use cinrs::c99;

c99! {
    #include <stdio.h>

    typedef struct { double x, y; } Vec2;

    double dot(Vec2 a, Vec2 b) {
        return a.x * b.x + a.y * b.y;
    }

    void greet(const char *name) {
        printf("hello, %s\n", name);
    }

    __attribute__((cinrs_safe)) int fact(int n) {
        return n == 0 ? 1 : n * fact(n - 1);
    }
}

fn main() {
    // C functions are foreign functions: calling one is `unsafe`…
    let d = unsafe { dot(Vec2 { x: 1.0, y: 2.0 }, Vec2 { x: 3.0, y: 4.0 }) };
    unsafe { greet(c"cinrs".as_ptr()) };

    // …unless the C says `cinrs_safe`, and then `rustc` checks the body.
    println!("dot = {d}, fact(10) = {}", fact(10));
}
```

That is `examples/readme.rs`: `cargo run --example readme`.

## Why

* **No C toolchain in the build.** The macro *is* the compiler: no `build.rs`,
  no `cc`, no `bindgen`, nothing to install on the build machine. The standard
  headers are bundled, and a call such as `printf` links against the platform's
  C library like any other `extern "C"` declaration.
* **What the C defines is Rust.** A `struct` is a `#[repr(C)]` type you can
  construct, a function is an `extern "C" fn` you can call, a global is a
  `static`. Nothing is declared twice, and nothing crosses an FFI boundary that
  the optimiser cannot see through.
* **Errors point at the C.** Every generated token carries the span of the C
  token it came from, so `cargo` and rust-analyzer put the caret where the
  mistake is — for this crate's diagnostics and for `rustc`'s own:

  ```text
  error: use of undeclared identifier 'j'
   --> src/main.rs:7:25
    |
  7 |             total += xs[j];
    |                         ^
  ```

* **Real C, not a subset.** `goto` (computed ones too), `switch` with
  fallthrough, bit-fields with GCC's layout, variable length arrays, variadic
  functions, `_Complex`, `_Atomic`, `<threads.h>`, K&R definitions, the whole
  preprocessor with `#include` and `#embed` — and the GNU extensions real code
  uses: statement expressions, `typeof`, `__attribute__((cleanup))`, nested
  functions, `__int128`, `__builtin_*`, and inline assembly, for the operand
  kinds `asm!` has. What has no honest translation (`setjmp`) is a located
  error, never a guess.
* **The SIMD intrinsics, by name.** `#include <immintrin.h>` and write
  `_mm_add_epi32(a, b)`: 6,075 of Intel's intrinsics, SSE through AVX2 and
  AVX-512 with FMA, AES, GFNI, VAES, SHA and the BMI scalar ones, mapped
  straight onto `core::arch::x86_64`, whose
  signatures the bundled headers were generated from. `__m128i` punnes through a
  `union` like any 16-byte type, GCC's `a * b`, `v[i]` and `{a, b}` on the vector
  types are the intrinsics that do the same, an immediate operand becomes `core::arch`'s
  `const` generic, and `__attribute__((target("avx2")))` becomes
  `#[target_feature]`. BLAKE3's C implementation — four SIMD kernels and a
  `cpuid` dispatcher in inline assembly — builds unedited and picks AVX-512.
* **Measured, not claimed.** 98 % of [c-testsuite], 89–93 % of [GCC's torture
  tests][gcc-torture] and 82 % of [Clang's C conformance tests][clang-c-tests]
  — about 2,270 cases, every failure listed by name with its reason, and **not
  one of them a known bug**. See [Conformance and speed](#conformance-and-speed).
* **As fast as a C compiler.** Over 48 whole programs the median run time is
  **1.02×** that of `gcc -O2`, with byte-identical output — the SIMD-intrinsics
  entries of the Benchmarks Game included.
* **Safety you can opt into.** Mark a function `[[cinrs::safe]]` (or
  `__attribute__((cinrs_safe))`) and it is generated *without* `unsafe`, so
  `rustc` checks the translation and Rust calls it as `fact(10)`.
* **`core`-only output.** The expansion names nothing but `core` — a variable
  length array needs `alloc`, and a `_Thread_local` object needs `std` — so it
  works in a `#![no_std]` crate.

## Installation

```text
cargo add cinrs
```

Rust **1.98** or later. One thing needs a newer compiler: *defining* a variadic
function needs Rust 1.99's `c_variadic` and is a clear error before that —
declaring and calling one, `printf` included, works everywhere.

**Linux, macOS and Windows.** Linux (x86-64) is where `cinrs` is developed and
where the whole test suite and the conformance corpora run; macOS (arm64) and
Windows (x86-64, MSVC) build, link and run the examples and the portable tests
on every push. Other targets are compile-checked; see
[Cross-compilation][cross].

Two features: `complex` (on by default) is C's complex types, as
[`num_complex::Complex`](https://docs.rs/num-complex) — `default-features =
false` drops it and its one small dependency; `nightly` moves a diagnostic about
a string-literal body onto a caret inside the literal.

## Using it

### Pick the language

| | C89 | C99 | C11 | C17 | C23 |
| --- | --- | --- | --- | --- | --- |
| ISO C | `c89!` | `c99!` | `c11!` | `c17!` | `c23!` |
| with GNU extensions | `gnu89!` | `gnu99!` | `gnu11!` | `gnu17!` | `gnu23!` |
| a whole `.c` file | `include_c89!` | `include_c99!` | `include_c11!` | `include_c17!` | `include_c23!` |

`include_gnu89!` … `include_gnu23!` exist too. A feature of a later revision
used in an earlier block is an error that names the macro to write instead.
Everything spelled with a leading double underscore (`__attribute__`,
`__typeof__`, `__builtin_*`) works in the strict macros as well, exactly as in
`gcc -std=c99`.

### Three ways to write the C

```rust,ignore
cinrs::c99! { int twice(int x) { return 2 * x; } }          // raw tokens

cinrs::c99! { r#" double eight(void) { return 0x1p3; } "# } // a string literal

cinrs::include_c99!("vendor/parser.c");                     // a file
```

Raw tokens give the best diagnostics. The few things Rust's lexer refuses —
hexadecimal floats, `'ab'`, `L"…"`, `\` line continuations, `##` — go in a
string literal, and C that already lives in a file goes in whole. Details:
[input forms][input-forms], [including a C file][include-c].

### Calling it from Rust

* Functions are `pub unsafe extern "C" fn`; [safe functions][safe-functions]
  drop the `unsafe`.
* A C name that is a Rust keyword is a raw identifier: `int match(int)` is
  called as `r#match(1)`. [More on names][names].
* A bit-field has no address, so it is a pair of methods: `h.length()` and
  `h.set_length(200)`.
* Each invocation is one translation unit, expanded into a private module that
  is glob re-exported; an ordinary Rust `mod` around the invocation gives its
  items a path (`packet::Header`). `#pragma cinrs export` gives its functions
  real C symbols, so another block — or a C library — can call them.

### Headers

The ISO C standard headers are bundled and written against a model of the
target, so a block means the same thing on every machine. Your own headers are
found next to the `.rs` file (`#pragma cinrs include_path "…"` adds
directories), and editing one rebuilds the crate. POSIX and the platform's own
headers — `<unistd.h>`, `<pthread.h>`, `struct stat`, `DIR` — are one pragma
away:

```c
#pragma cinrs system_include
#include <unistd.h>
#include <sys/stat.h>
```

See [system headers][system-headers], and [the pragma reference][pragmas] for
all eight `#pragma cinrs` options and the environment variables that go with
them.

### Calling a C library

A header's declarations are the binding: a function a header declares and the
unit does not define is callable from Rust under its own C name, and so are the
header's types.

```rust,ignore
mod z {
    cinrs::c99! {
        #pragma cinrs system_include
        #pragma cinrs link "z"
        #include <zlib.h>

        /* `deflateInit` is a macro, so it needs a line of C — written here. */
        int z_deflate_init(z_stream *s, int level) { return deflateInit(s, level); }
        enum { ZDEMO_OK = Z_OK };
    }
}
// unsafe { z::crc32(0, buf.as_ptr(), buf.len() as z::uInt) }
```

More in [calling a C library from Rust][calling-c].

### Cross-compilation

`sizeof`, layouts and `#if` are worked out while the macro expands, from a
model of the target. A procedural macro cannot ask `rustc` what the target is,
so a crate that is cross-compiled says so from its build script:

```rust,no_run
// build.rs
fn main() {
    println!("cargo:rustc-env=CINRS_TARGET={}", std::env::var("TARGET").unwrap());
}
```

Without it a cross build fails a compile-time assertion instead of computing
the wrong thing. [Cross-compilation][cross] has the supported targets.

## What is supported

[What works][features] is the tour, construct by construct.
[C standard status][c-status] is the table — every feature of C89 to C23 with
its state, in the style of Clang's `c_status` — and [GNU
extensions][gnu-extensions] is the same for GCC's.

## Limitations

The ones most likely to matter; [the full list][limitations] has the rest.

* Not supported, each as a located error: `setjmp`/`longjmp`, the memory
  operands and `asm goto` of inline assembly (which is otherwise
  `core::arch::asm!`, x86 only), the GNU vector extensions (the Intel
  intrinsics are the SIMD that is here),
  `_BitInt`, `_Imaginary`, an `_Atomic` aggregate.
* `long double` is `double`.
* The SIMD intrinsics are x86's, and only the baseline instruction set is
  predefined: a procedural macro cannot see `-C target-feature`, so `#ifdef
  __AVX2__` and `#ifdef __AVX512F__` are false and
  `__builtin_cpu_supports("avx2")` is the question to ask. No MMX.
* Variable length arrays and `alloca` live on the heap (Rust cannot move the
  stack pointer); what the C can observe is unchanged.
* `va_arg` of a `struct` works for records up to sixteen bytes on x86-64
  System V only.
* Each invocation is its own translation unit: two blocks that include one
  header get two distinct Rust types for each `struct` in it.
* The platform's include directories are not searched unless the unit asks.

## Conformance and speed

| Corpus | Correct | Entry point |
| --- | --- | --- |
| [c-testsuite] — whole programs with expected output | **214 of 218 (98.2 %)** | `c99!` |
| [GCC's C torture tests][gcc-torture] — 1,776 self-checking programs | **1,577 of 1,769 (89.1 %)**, 92.6 % on Rust 1.99 | `gnu11!` |
| [Clang's C conformance tests][clang-c-tests] — what must be *refused*, line by line | **167 of 203 (82.3 %)** | per test |
| glibc's own headers through the front end | **66 of 67** | `gnu11!`, `c11!` |

"Correct" means the case passed, or the entry point is required to refuse it
and did. Every remaining case is listed by name as unimplemented, not planned
or needing a newer toolchain — **not one is tagged as a bug**.
[The conformance suites][testsuites] says how they are run.

[Benchmarks][benchmarks]: 48 whole C programs — the single-threaded C entries
of the Benchmarks Game, including the eight written with SIMD intrinsics,
Dhrystone, Whetstone and two dozen kernels that isolate one construct each —
built as `gcc -O2`, `clang -O2` and a `cinrs` block under
`rustc -C opt-level=3`. The median `cinrs`/`gcc` ratio is **1.02×**, 39 of the
48 are within 10 % of `gcc` or faster, and every output is identical across the
three builds. A `goto` costs nothing: an outward one is a labelled `break` or
`continue`, and anything else is read back into loops and branches by a
relooper, so an interpreter loop written as a `switch` full of `goto`s — the
SQLite VDBE, say — runs at the speed `gcc` gives it.

## Documentation

| | |
| --- | --- |
| [What works][features] | the language, the preprocessor, the extensions, construct by construct |
| [What the C becomes][translation] | what is generated for each construct — signatures, accessors, the module — for Rust code on the other side |
| [Limitations][limitations] | what is refused, and what differs from a C compiler |
| [C standard status][c-status] · [GNU extensions][gnu-extensions] | feature tables |
| [Pragmas][pragmas] | `#pragma cinrs …`, the other pragmas, the attributes, the environment variables |
| [Including a C file][include-c] | `include_c99!` and its siblings |
| [System headers][system-headers] | `/usr/include` and what glibc's headers do here |
| [Cross-compilation][cross] · [`no_std`][no-std] | targets and data models; what the expansion needs |
| [Conformance][testsuites] · [Benchmarks][benchmarks] | how the numbers above are measured |
| [API documentation](https://docs.rs/cinrs) | the macro reference: `c99!` and each of its siblings |

## How it works

The macro recovers the C source text of its own invocation (by slicing the
`.rs` file, or by decoding the string literal) together with a map from byte
offsets back to `proc_macro2` spans, then runs a C front end over it — lexer,
preprocessor, parser, semantic analysis with C's conversion rules made
explicit — and emits Rust, `c2rust`-style: `#[repr(C)]` records, raw pointers,
wrapping arithmetic where C defines wrap-around, `pub unsafe extern "C" fn` for
each function. Every token it emits is stamped with the span of the C it came
from, which is what makes the errors land where they should.

`0.1` means the macro surface may still change. The companion crates
`cinrs-core`, `cinrs-macros` and `cinrs-rt` are implementation details of this
one and carry no stability promise.

## License

Licensed under either of

* [Apache License, Version 2.0](https://github.com/tanakh/cinrs/blob/master/LICENSE-APACHE)
* [MIT license](https://github.com/tanakh/cinrs/blob/master/LICENSE-MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

<!-- The documents under doc/ are not part of the published `.crate`, so these
     are absolute links: a README rendered on crates.io or docs.rs has no
     doc/ directory next to it. -->

[c-testsuite]: https://github.com/tanakh/cinrs/blob/master/doc/c-testsuite.md
[gcc-torture]: https://github.com/tanakh/cinrs/blob/master/doc/gcc-torture.md
[clang-c-tests]: https://github.com/tanakh/cinrs/blob/master/doc/clang-c-tests.md
[testsuites]: https://github.com/tanakh/cinrs/blob/master/doc/testsuites.md
[benchmarks]: https://github.com/tanakh/cinrs/blob/master/doc/benchmarks.md
[features]: https://github.com/tanakh/cinrs/blob/master/doc/features.md
[translation]: https://github.com/tanakh/cinrs/blob/master/doc/translation.md
[input-forms]: https://github.com/tanakh/cinrs/blob/master/doc/features.md#input-forms
[safe-functions]: https://github.com/tanakh/cinrs/blob/master/doc/features.md#safe-functions
[names]: https://github.com/tanakh/cinrs/blob/master/doc/features.md#names-rust-would-not-take
[calling-c]: https://github.com/tanakh/cinrs/blob/master/doc/features.md#calling-a-c-library-from-rust
[limitations]: https://github.com/tanakh/cinrs/blob/master/doc/limitations.md
[c-status]: https://github.com/tanakh/cinrs/blob/master/doc/c-status.md
[gnu-extensions]: https://github.com/tanakh/cinrs/blob/master/doc/gnu-extensions.md
[pragmas]: https://github.com/tanakh/cinrs/blob/master/doc/pragmas.md
[include-c]: https://github.com/tanakh/cinrs/blob/master/doc/include-c.md
[system-headers]: https://github.com/tanakh/cinrs/blob/master/doc/system-headers.md
[cross]: https://github.com/tanakh/cinrs/blob/master/doc/cross-compilation.md
[no-std]: https://github.com/tanakh/cinrs/blob/master/doc/no-std.md
