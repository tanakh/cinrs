# cinrs benchmarks

How fast is the code `cinrs` generates? Each program below is one whole C program,
built three ways — `gcc -O2`, `clang -O2`, and as a `cinrs` macro invocation compiled
by `rustc -C opt-level=3` — and run 5 times per build with the median wall clock
reported. The outputs of all three builds are compared byte for byte, so a
miscompilation shows up here as loudly as a slowdown.

Regenerate with

```text
( ulimit -v 8000000; timeout -k 10 3600 \
      cargo run -p cinrs-bench --release -- --markdown doc/benchmarks.md )
```

## The machine

| | |
| --- | --- |
| CPU | AMD Ryzen 9 9950X 16-Core Processor (32 logical cores) |
| Memory | 62 GiB |
| Kernel | Linux 6.18.33.2-microsoft-standard-WSL2 |
| Rust | rustc 1.98.1 (48a229cea 2026-09-01) |
| GCC | gcc (Ubuntu 15.2.0-16ubuntu1) 15.2.0 |
| Clang | Ubuntu clang version 21.1.8 (6ubuntu1) |
| Measured | 2026-09-07 |

## Benchmarks Game

| program | input | gcc (s) | clang (s) | cinrs (s) | cinrs/gcc | cinrs/clang | output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| `fannkuch-redux` | `11` | 1.822 | 1.836 | 1.956 | 1.07× | 1.07× | same |
| `n-body` | `20000000` | 0.676 | 0.672 | 0.685 | 1.01× | 1.02× | same |
| `spectral-norm` | `5500` | 0.916 | 0.921 | 0.918 | 1.00× | 1.00× | same |
| `mandelbrot` | `4000` | 0.595 | 0.614 | 0.615 | 1.03× | 1.00× | same |
| `fasta` | `2500000` | 0.365 | 0.363 | 0.391 | 1.07× | 1.08× | same |
| `reverse-complement` | `stdin=fasta 25000000 (254 MB)` | 0.259 | 0.264 | 0.256 | 0.99× | 0.97× | same |
| `binary-trees` | `18` | 0.782 | 0.830 | 0.825 | 1.06× | 0.99× | same |
| `pidigits` | `10000` | 0.367 | 0.368 | 0.369 | 1.01× | 1.00× | same |

<details><summary>what each row exercises</summary>

* `fannkuch-redux` — permutation flips over a small array; VLAs sized once
* `n-body` — double-precision n-body integration, `sqrt` in the inner loop
* `spectral-norm` — eigenvalue by the power method; the array bounds are run-time values
* `mandelbrot` — escape-time loop over doubles, bit-packed PBM output through `putc`
* `fasta` — weighted random selection, character at a time through a line-buffered stdout
* `reverse-complement` — reads 254 MB from stdin through `fgets`; a table lookup per byte
* `binary-trees` — `malloc`/`free` churn over a recursive tree; needs the platform's <malloc.h>
* `pidigits` — spigot digits of pi through GMP; the work is in the library, not in the C

</details>

## Classic micro-benchmarks

| program | input | gcc (s) | clang (s) | cinrs (s) | cinrs/gcc | cinrs/clang | output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| `dhrystone` | `stdin=50000000` | 0.416 | 0.171 | 0.456 | 1.10× | 2.66× | same |
| `whetstone` | `400000` | 1.613 | 1.644 | 3.917 | 2.43× | 2.38× | same |
| `linpack` | `1600` | 0.379 | 0.236 | 0.242 | 0.64× | 1.03× | same |

<details><summary>what each row exercises</summary>

* `dhrystone` — Weicker 2.1: K&R C, struct assignment, `strcpy`/`strcmp`, an enum and a union
* `whetstone` — the floating-point classic: arrays, `sin`/`cos`/`exp`/`sqrt`, procedure calls
* `linpack` — LINPACK-style LU with partial pivoting over `daxpy` (written here, see SOURCES.md)

</details>

## Kernels written for this suite

| program | input | gcc (s) | clang (s) | cinrs (s) | cinrs/gcc | cinrs/clang | output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| `sieve` | `40000000 4` | 0.454 | 0.436 | 0.474 | 1.04× | 1.09× | same |
| `recursion` | `32 10 3 8` | 0.547 | 0.070 | 0.069 | 0.13× | 1.00× | same |
| `nqueens` | `15` | 0.752 | 0.828 | 0.793 | 1.05× | 0.96× | same |
| `matmul` | `1024 4` | 0.509 | 0.515 | 0.511 | 1.00× | 0.99× | same |
| `matmul-vla` | `1024 4` | 0.864 | 0.516 | 0.516 | 0.60× | 1.00× | same |
| `sort` | `3000000 2` | 0.851 | 0.807 | 0.845 | 0.99× | 1.05× | same |
| `binsearch` | `2000000 8000000` | 0.841 | 0.846 | 0.907 | 1.08× | 1.07× | same |
| `fft` | `20 8` | 0.472 | 0.465 | 0.480 | 1.02× | 1.03× | same |
| `crc32` | `4000000 100` | 0.588 | 0.588 | 0.586 | 1.00× | 1.00× | same |
| `sha256` | `200000 900` | 0.416 | 0.421 | 0.396 | 0.95× | 0.94× | same |
| `prng` | `400000000` | 0.450 | 0.446 | 0.449 | 1.00× | 1.01× | same |
| `life` | `512 500` | 0.477 | 0.346 | 0.368 | 0.77× | 1.06× | same |
| `levenshtein` | `2000 200` | 0.596 | 1.035 | 1.035 | 1.74× | 1.00× | same |
| `hashtable` | `4000000 20000000` | 0.881 | 0.967 | 0.869 | 0.99× | 0.90× | same |
| `libc-str` | `4096 4000000` | 0.428 | 0.431 | 0.434 | 1.01× | 1.01× | same |
| `hand-str` | `4096 400000` | 0.618 | 0.011 | 0.011 | 0.02× | 1.02× | same |
| `bitfields` | `200000 1200` | 0.379 | 0.488 | 0.378 | 1.00× | 0.78× | same |
| `vla` | `64 10000000` | 0.426 | 0.453 | 0.553 | 1.30× | 1.22× | same |
| `vla-hoisted` | `64 10000000` | 0.418 | 0.439 | 0.411 | 0.98× | 0.93× | same |
| `interp-switch` | `150000000` | 0.758 | 0.606 | 1.058 | 1.40× | 1.75× | same |
| `interp-goto` | `150000000` | 0.531 | 0.851 | 0.818 | 1.54× | 0.96× | same |
| `structval` | `1000000000` | 0.462 | 1.492 | 1.456 | 3.15× | 0.98× | same |
| `chase` | `4000000 6000000` | 0.760 | 0.736 | 0.797 | 1.05× | 1.08× | same |
| `statemachine` | `8000000 30` | 0.414 | 0.411 | 0.888 | 2.15× | 2.16× | same |
| `statemachine-structured` | `8000000 30` | 0.415 | 0.421 | 0.426 | 1.03× | 1.01× | same |
| `complexmandel` | `1600 200` | 0.380 | 0.425 | 0.567 | 1.49× | 1.33× | same |
| `wrapping` | `120000000` | 0.338 | 0.358 | 0.357 | 1.06× | 1.00× | same |
| `divide` | `120000000` | 0.426 | 0.369 | 0.370 | 0.87× | 1.00× | same |

<details><summary>what each row exercises</summary>

* `sieve` — byte stores down a strided inner loop
* `recursion` — call overhead: recursive fib, tak and Ackermann
* `nqueens` — bitmask backtracking, no memory traffic
* `matmul` — double matmul over a flat array, `a[i * n + k]`
* `matmul-vla` — the same, through the C99 parameter form `double a[n][n]`
* `sort` — quicksort and heapsort over `int`
* `binsearch` — unpredictable branches and cache misses
* `fft` — iterative radix-2 FFT, double precision
* `crc32` — table-driven CRC-32: a serial dependency chain
* `sha256` — SHA-256 written out: 32-bit rotates and adds
* `prng` — LCG and xorshift: `wrapping_mul` and `wrapping_add` and nothing else
* `life` — nine-neighbour stencil over a byte grid
* `levenshtein` — edit-distance DP: three `min`s in the inner loop
* `hashtable` — open addressing with linear probing over a 16-byte struct
* `libc-str` — control: `strlen`/`strcmp`/`memcpy`/`memcmp`, all of them libc's
* `hand-str` — the same routines written in C, so the compiler has to optimise them
* `bitfields` — bit-fields: every read and write is an accessor method in the expansion
* `vla` — a variable length array made afresh every iteration (a `Vec` in the expansion)
* `vla-hoisted` — control for `vla`: the same work with one `malloc` outside the loop
* `interp-switch` — bytecode dispatch through a `switch`, with a fallthrough case
* `interp-goto` — the same machine through GCC's computed `goto`
* `structval` — small structs passed and returned by value
* `chase` — pointer chasing: a dependent load per step
* `statemachine` — a lexer written as `goto`s, which is what `cinrs` lowers into a state machine
* `statemachine-structured` — control for `statemachine`: the same lexer with `while` and `switch`, no `goto`
* `complexmandel` — `double _Complex` arithmetic, Annex G recovery and all
* `wrapping` — wrapping arithmetic at every width
* `divide` — integer and floating division by run-time divisors

</details>

## Output agreement

Every program printed the same bytes from all 3 builds.

## Compile time and memory

The `cinrs` column is one `rustc` process: the macro expands the C — lexer, preprocessor, parser, semantic analysis, code generation — and then `rustc` compiles what came out at `-C opt-level=3 -C codegen-units=1`. The native columns are one `gcc`/`clang` process each. Peak resident set is `/usr/bin/time -f %M`.

| program | C lines | gcc (s) | clang (s) | cinrs rustc (s) | cinrs peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fannkuch-redux` | 71 | 0.11 | 0.12 | 0.21 | 162 MiB |
| `n-body` | 141 | 0.13 | 0.11 | 0.20 | 163 MiB |
| `spectral-norm` | 52 | 0.12 | 0.11 | 0.20 | 163 MiB |
| `mandelbrot` | 58 | 0.10 | 0.09 | 0.19 | 162 MiB |
| `fasta` | 108 | 0.11 | 0.10 | 0.26 | 171 MiB |
| `reverse-complement` | 70 | 0.11 | 0.09 | 0.20 | 163 MiB |
| `binary-trees` | 131 | 0.14 | 0.10 | 0.22 | 167 MiB |
| `pidigits` | 65 | 0.11 | 0.10 | 0.24 | 168 MiB |
| `dhrystone` | 21 | 0.13 | 0.11 | 0.22 | 166 MiB |
| `whetstone` | 433 | 0.13 | 0.12 | 0.23 | 166 MiB |
| `linpack` | 172 | 0.15 | 0.14 | 0.25 | 165 MiB |
| `sieve` | 46 | 0.11 | 0.10 | 0.20 | 163 MiB |
| `recursion` | 46 | 0.13 | 0.09 | 0.20 | 162 MiB |
| `nqueens` | 38 | 0.10 | 0.09 | 0.19 | 162 MiB |
| `matmul` | 62 | 0.11 | 0.10 | 0.20 | 163 MiB |
| `matmul-vla` | 66 | 0.11 | 0.10 | 0.20 | 164 MiB |
| `sort` | 108 | 0.12 | 0.11 | 0.21 | 164 MiB |
| `binsearch` | 53 | 0.11 | 0.09 | 0.19 | 163 MiB |
| `fft` | 97 | 0.12 | 0.11 | 0.20 | 165 MiB |
| `crc32` | 54 | 0.10 | 0.10 | 0.20 | 164 MiB |
| `sha256` | 136 | 0.13 | 0.10 | 0.21 | 165 MiB |
| `prng` | 33 | 0.09 | 0.08 | 0.19 | 163 MiB |
| `life` | 56 | 0.13 | 0.10 | 0.20 | 164 MiB |
| `levenshtein` | 69 | 0.11 | 0.10 | 0.19 | 164 MiB |
| `hashtable` | 75 | 0.11 | 0.10 | 0.20 | 163 MiB |
| `libc-str` | 55 | 0.11 | 0.09 | 0.20 | 164 MiB |
| `hand-str` | 73 | 0.11 | 0.10 | 0.19 | 164 MiB |
| `bitfields` | 84 | 0.11 | 0.09 | 0.20 | 166 MiB |
| `vla` | 44 | 0.10 | 0.10 | 0.21 | 162 MiB |
| `vla-hoisted` | 44 | 0.10 | 0.10 | 0.19 | 162 MiB |
| `interp-switch` | 114 | 0.10 | 0.10 | 0.18 | 164 MiB |
| `interp-goto` | 111 | 0.10 | 0.09 | 0.20 | 164 MiB |
| `structval` | 76 | 0.10 | 0.09 | 0.17 | 162 MiB |
| `chase` | 61 | 0.10 | 0.10 | 0.19 | 163 MiB |
| `statemachine` | 107 | 0.11 | 0.09 | 0.19 | 165 MiB |
| `statemachine-structured` | 108 | 0.11 | 0.10 | 0.21 | 164 MiB |
| `complexmandel` | 44 | 0.11 | 0.09 | 0.19 | 164 MiB |
| `wrapping` | 41 | 0.10 | 0.09 | 0.20 | 163 MiB |
| `divide` | 44 | 0.10 | 0.09 | 0.21 | 163 MiB |

The slowest `cinrs` compilation is `fasta` at **0.26 s**, and the largest is `fasta` at **171 MiB** — neither is close to the thresholds this report flags, which are 30 s and 2 GiB.
 Read them against the floor: `fn main() {}`, compiled with the very same flags and the same `--extern cinrs`, costs **0.11 s** and **115 MiB** on this machine. Nearly all of both columns is `rustc` starting and loading the procedural macro, not the C being translated.

"C lines" is the length of the file named in the program table. `dhrystone` is the exception: the twenty-one lines there are an amalgamation that `#include`s Weicker's two source files, about seven hundred lines between them.

## Programs not in the suite

* **k-nucleotide** — the only C version in the corpus (`knucleotide.gcc`) uses `khash.h` from samtools, which is not part of the Benchmarks Game distribution, and OpenMP for its outer loop; there is no version with a hash table of its own to vendor
* **regex-redux** — every C version needs PCRE2, and `pcre2.h` is not installed on this machine; with it, the row would be `#pragma cinrs system_include` plus `#pragma cinrs link "pcre2-8"` and nothing else

## What the numbers say

Across the 39 programs measured, the median `cinrs`/`gcc -O2` ratio is **1.02×**, and **31 of 39** are within 10 % of `gcc -O2` or faster. The extremes are `hand-str` at 0.02× and `structval` at 3.15×.

The `clang` column is what separates the two kinds of difference. `cinrs` and `clang` share a back end, so a row where `clang` is exactly as slow as `cinrs` is not saying anything about the translation at all — it is LLVM's code generator against GCC's, and every Rust program on the machine is subject to it. A row where `cinrs` is slower than **both** is the translation's own.

**Slower than both, which is `cinrs`'s own to answer for:**

* `vla` — 1.30× gcc, 1.22× clang. a variable length array made afresh every iteration (a `Vec` in the expansion)
* `interp-switch` — 1.40× gcc, 1.75× clang. bytecode dispatch through a `switch`, with a fallthrough case
* `complexmandel` — 1.49× gcc, 1.33× clang. `double _Complex` arithmetic, Annex G recovery and all
* `statemachine` — 2.15× gcc, 2.16× clang. a lexer written as `goto`s, which is what `cinrs` lowers into a state machine
* `whetstone` — 2.43× gcc, 2.38× clang. the floating-point classic: arrays, `sin`/`cos`/`exp`/`sqrt`, procedure calls

**Slower than `gcc` but level with `clang`, i.e. LLVM against GCC and not this crate:**

* `interp-goto` — 1.54× gcc, 0.96× clang.
* `levenshtein` — 1.74× gcc, 1.00× clang.
* `structval` — 3.15× gcc, 0.98× clang.

**Faster than `gcc -O2`:**

* `hand-str` — 0.02× gcc.
* `recursion` — 0.13× gcc.
* `matmul-vla` — 0.60× gcc.
* `linpack` — 0.64× gcc.
* `life` — 0.77× gcc.
* `divide` — 0.87× gcc.

### Why, construct by construct

What the expansion does with each of these is in the crate's README; what it costs is here.

* **Arithmetic wraps for free.** C's unsigned arithmetic is modular, so the expansion is `wrapping_add`, `wrapping_mul` and their relatives rather than Rust's `+` and `*`. Those are `#[inline]` intrinsics that lower to the bare instruction, and `prng` — a loop that is nothing but a multiply, an add and three shifts — and `wrapping`, which does the same at every width C has, are the measurement: both land on `gcc`. If they did not, every arithmetic row in the table would be paying for it.
* **There are no bounds checks.** A C array is a raw pointer and a subscript is `.offset()`, which is plain address arithmetic with no check in it — `sieve`, `matmul`, `life`, `crc32` and `binsearch` are where that shows, and none of them has a Rust tax to pay.
* **Division has a check C does not.** Rust's `/` and `%` panic on a zero divisor, and signed division also has to rule out `INT_MIN / -1`; C's do neither. `divide` is a loop of nothing but divisions by run-time divisors, which is the worst case, and the check does not show above the latency of the divider itself.
* **A bit-field is a pair of methods.** A bit-field has no address, so it is not a field of the generated `#[repr(C)]` struct: a run of them shares one `[u8; K]` and each named member becomes `s.ttl()` and `s.set_ttl(v)`. `bitfields` parses and repacks an IP header a hundred million times over, so every one of those is a call that has to be inlined and folded back into a shift and a mask before it can keep up. It does.
* **A variable length array is a `Vec`.** Rust cannot move the stack pointer by an amount chosen at run time, so a VLA and `alloca` are emulated on the heap. `vla` makes one per iteration and `vla-hoisted` does the same work with one `malloc` outside the loop; the difference between those two rows is exactly what the emulation costs, and it is an allocation per declaration rather than a stack adjustment. A VLA declared once and used in a loop — which is what `spectral-norm` and `fannkuch-redux` do — costs nothing.
* **`goto` becomes a state machine, and that is where the time goes.** A function that jumps — *any* jump, including a `goto` out of a `switch` — is lowered into a `loop { match block { … } }` over basic blocks, and the whole function goes with it, hot loop and all. `statemachine` and `statemachine-structured` are the same lexer over the same input, one written with a dozen labels and `goto`s and one with `while` and `switch`: `gcc` and `clang` take the same time over both, and `cinrs` does not. That pair is the measurement of this lowering, and it is the largest number in the report. `whetstone`, whose `main` carries the whole benchmark and two `goto`s, and `interp-switch`, whose dispatch loop leaves through `goto done`, are the same finding in programs that were not written to show it. LLVM does thread the dispatch away — there is no indirect branch left in the generated code — but the structured shape the C had is not recovered.
* **`switch` becomes `match`.** `interp-switch` is a bytecode dispatch loop with a fallthrough case; the fallthrough has to run the next arm's body without re-dispatching. It is also a function with a `goto` in it, so the row above applies.
* **Structs by value go through Rust's C ABI.** `structval` passes and returns a two-`double` struct (two SSE registers on x86-64 System V) and a three-`long` one (memory). Nothing in the expansion decides that — `#[repr(C)]` and `extern "C"` hand it to `rustc`.
* **`_Complex` is `num_complex::Complex`.** `complexmandel` multiplies complex numbers in the inner loop, and C's complex multiplication is not four multiplies and two adds: Annex G.5.1 requires an infinity-recovery path. What the row measures is what that path costs when it is never taken.
* **The C library is the C library.** `libc-str`, `chase` and `pidigits` are controls — a `strlen` call, a dependent load, and a program whose work is all inside GMP. They should be the same in every column, and are.

### What is *not* being measured

* **Parallelism.** Every program here is single-threaded, and the Benchmarks Game versions were chosen to be. `#pragma omp` is ignored by `cinrs`, so the native builds are compiled without `-fopenmp` and the comparison is serial against serial.
* **`-march=native`.** Neither side gets it. Both are built for the base x86-64 target, so neither back end may use AVX-512 unless it can prove it is there.
* **Link-time optimisation.** Neither side gets that either. Each program is one translation unit and one crate, so there is nothing across units to optimise.
* **Long-running programs.** Everything here is under two seconds, which is enough for the median of five runs to be stable to about a millisecond but not enough to say anything about a program whose working set grows for an hour.

