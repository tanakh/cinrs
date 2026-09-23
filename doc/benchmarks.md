# cinrs benchmarks

How fast is the code `cinrs` generates? Each program below is one whole C program,
built three ways — `gcc -O2`, `clang -O2`, and as a `cinrs` macro invocation compiled
by `rustc -C opt-level=3` — and run 5 times per build with the median wall clock
reported. The outputs of all three builds are compared byte for byte, so a
miscompilation shows up here as loudly as a slowdown.

The programs are the single-threaded C entries from [The Computer Language
Benchmarks Game](https://benchmarksgame-team.pages.debian.net/benchmarksgame/),
Dhrystone 2.1 and Whetstone, and two dozen kernels written to isolate one
construct each (bit-fields, heap-emulated variable length arrays, `goto`
lowering, `switch` against computed `goto`, `_Complex`, wrapping arithmetic,
division). That comparison makes the suite a differential test as well as a
benchmark, and `benches/cinrs-bench` is the harness: its
[README](../benches/cinrs-bench/README.md) says how to run it and how to add a
program.

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
| Measured | 2026-09-23 |

## Benchmarks Game

| program | input | gcc (s) | clang (s) | cinrs (s) | cinrs/gcc | cinrs/clang | output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| `fannkuch-redux` | `11` | 1.911 | 1.835 | 1.937 | 1.01× | 1.06× | same |
| `n-body` | `20000000` | 0.666 | 0.665 | 0.680 | 1.02× | 1.02× | same |
| `spectral-norm` | `5500` | 0.905 | 0.908 | 0.912 | 1.01× | 1.00× | same |
| `mandelbrot` | `4000` | 0.588 | 0.605 | 0.606 | 1.03× | 1.00× | same |
| `fasta` | `2500000` | 0.361 | 0.359 | 0.397 | 1.10× | 1.10× | same |
| `reverse-complement` | `stdin=fasta 25000000 (254 MB)` | 0.236 | 0.214 | 0.247 | 1.05× | 1.15× | same |
| `binary-trees` | `18` | 0.781 | 0.827 | 0.833 | 1.07× | 1.01× | same |
| `pidigits` | `10000` | 0.387 | 0.372 | 0.361 | 0.93× | 0.97× | same |
| `fannkuch-redux-ssse3` | `11` | 0.663 | 0.781 | 0.836 | 1.26× | 1.07× | same |
| `n-body-sse` | `20000000` | 0.634 | 0.600 | 0.601 | 0.95× | 1.00× | same |
| `n-body-avx` | `20000000` | 0.602 | 0.365 | 0.368 | 0.61× | 1.01× | same |
| `spectral-norm-sse2` | `5500` | 0.452 | 0.461 | 0.528 | 1.17× | 1.15× | same |
| `spectral-norm-sse41` | `5500` | 0.441 | 0.441 | 0.446 | 1.01× | 1.01× | same |
| `spectral-norm-avx2` | `5500` | 0.225 | 0.227 | 0.226 | 1.01× | 1.00× | same |
| `spectral-norm-avx` | `5500` | 0.691 | 0.542 | 0.544 | 0.79× | 1.00× | same |
| `mandelbrot-sse2` | `4000` | 0.195 | 0.111 | 0.110 | 0.56× | 0.99× | same |

<details><summary>what each row exercises</summary>

* `fannkuch-redux` — permutation flips over a small array; VLAs sized once
* `n-body` — double-precision n-body integration, `sqrt` in the inner loop
* `spectral-norm` — eigenvalue by the power method; the array bounds are run-time values
* `mandelbrot` — escape-time loop over doubles, bit-packed PBM output through `putc`
* `fasta` — weighted random selection, character at a time through a line-buffered stdout
* `reverse-complement` — reads 254 MB from stdin through `fgets`; a table lookup per byte
* `binary-trees` — `malloc`/`free` churn over a recursive tree; needs the platform's <malloc.h>
* `pidigits` — spigot digits of pi through GMP; the work is in the library, not in the C
* `fannkuch-redux-ssse3` — fannkuch-redux with each flip one `_mm_shuffle_epi8` (SSSE3)
* `n-body-sse` — n-body two pairs at a time in `__m128d`, `_mm_rsqrt_ps` and Newton steps (SSE2)
* `n-body-avx` — n-body one body per `__m256d`, `_mm256_hadd_pd` and `_mm_rsqrt_ps` (AVX)
* `spectral-norm-sse2` — spectral-norm two columns at a time, `_mm_set_pd`/`_mm_div_pd` (SSE2)
* `spectral-norm-sse41` — spectral-norm with A's entries computed in `__m128i`, `_mm_mullo_epi32` (SSE4.1)
* `spectral-norm-avx2` — the SSE4.1 row at twice the width, `_mm256_mullo_epi32` (AVX2)
* `spectral-norm-avx` — spectral-norm over 4x4 blocks of A, `_mm_rcp_ps` and a Goldschmidt step (AVX)
* `mandelbrot-sse2` — mandelbrot eight pixels in four `__m128d`, GNU vector operators and subscripts (SSE2)

</details>

## Classic micro-benchmarks

| program | input | gcc (s) | clang (s) | cinrs (s) | cinrs/gcc | cinrs/clang | output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| `dhrystone` | `stdin=50000000` | 0.409 | 0.168 | 0.450 | 1.10× | 2.68× | same |
| `whetstone` | `400000` | 1.584 | 1.628 | 1.624 | 1.03× | 1.00× | same |
| `linpack` | `1600` | 0.332 | 0.213 | 0.215 | 0.65× | 1.01× | same |

<details><summary>what each row exercises</summary>

* `dhrystone` — Weicker 2.1: K&R C, struct assignment, `strcpy`/`strcmp`, an enum and a union
* `whetstone` — the floating-point classic: arrays, `sin`/`cos`/`exp`/`sqrt`, procedure calls
* `linpack` — LINPACK-style LU with partial pivoting over `daxpy` (written here, see SOURCES.md)

</details>

## Kernels written for this suite

| program | input | gcc (s) | clang (s) | cinrs (s) | cinrs/gcc | cinrs/clang | output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| `sieve` | `40000000 4` | 0.378 | 0.351 | 0.350 | 0.93× | 1.00× | same |
| `recursion` | `32 10 3 8` | 0.542 | 0.068 | 0.069 | 0.13× | 1.01× | same |
| `nqueens` | `15` | 0.740 | 0.815 | 0.780 | 1.05× | 0.96× | same |
| `matmul` | `1024 4` | 0.500 | 0.505 | 0.506 | 1.01× | 1.00× | same |
| `matmul-vla` | `1024 4` | 0.852 | 0.507 | 0.510 | 0.60× | 1.01× | same |
| `sort` | `3000000 2` | 0.816 | 0.786 | 0.842 | 1.03× | 1.07× | same |
| `binsearch` | `2000000 8000000` | 0.822 | 0.832 | 0.749 | 0.91× | 0.90× | same |
| `fft` | `20 8` | 0.454 | 0.446 | 0.480 | 1.06× | 1.08× | same |
| `crc32` | `4000000 100` | 0.598 | 0.609 | 0.649 | 1.09× | 1.07× | same |
| `sha256` | `200000 900` | 0.428 | 0.419 | 0.391 | 0.91× | 0.93× | same |
| `prng` | `400000000` | 0.445 | 0.433 | 0.442 | 0.99× | 1.02× | same |
| `life` | `512 500` | 0.477 | 0.345 | 0.362 | 0.76× | 1.05× | same |
| `levenshtein` | `2000 200` | 0.646 | 1.069 | 1.143 | 1.77× | 1.07× | same |
| `hashtable` | `4000000 20000000` | 0.812 | 0.859 | 0.839 | 1.03× | 0.98× | same |
| `libc-str` | `4096 4000000` | 0.432 | 0.465 | 0.496 | 1.15× | 1.07× | same |
| `hand-str` | `4096 400000` | 0.661 | 0.012 | 0.011 | 0.02× | 0.97× | same |
| `bitfields` | `200000 1200` | 0.413 | 0.493 | 0.407 | 0.99× | 0.83× | same |
| `vla` | `64 10000000` | 0.454 | 0.460 | 0.570 | 1.26× | 1.24× | same |
| `vla-hoisted` | `64 10000000` | 0.445 | 0.440 | 0.436 | 0.98× | 0.99× | same |
| `interp-switch` | `150000000` | 0.761 | 0.744 | 0.822 | 1.08× | 1.11× | same |
| `interp-goto` | `150000000` | 0.570 | 0.916 | 0.873 | 1.53× | 0.95× | same |
| `structval` | `1000000000` | 0.503 | 1.540 | 1.495 | 2.98× | 0.97× | same |
| `chase` | `4000000 6000000` | 0.847 | 0.753 | 0.724 | 0.85× | 0.96× | same |
| `statemachine` | `8000000 30` | 0.429 | 0.414 | 0.431 | 1.00× | 1.04× | same |
| `statemachine-structured` | `8000000 30` | 0.430 | 0.443 | 0.442 | 1.03× | 1.00× | same |
| `complexmandel` | `1600 200` | 0.390 | 0.420 | 0.559 | 1.43× | 1.33× | same |
| `wrapping` | `120000000` | 0.344 | 0.357 | 0.354 | 1.03× | 0.99× | same |
| `divide` | `120000000` | 0.427 | 0.370 | 0.361 | 0.85× | 0.98× | same |
| `simd-dot` | `200000 4000` | 0.396 | 0.598 | 0.624 | 1.58× | 1.04× | same |

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
* `statemachine` — a lexer written as `goto`s that jump into one another, which is what `cinrs` lowers through a control-flow graph and reloops
* `statemachine-structured` — control for `statemachine`: the same lexer with `while` and `switch`, no `goto`
* `complexmandel` — `double _Complex` arithmetic, Annex G recovery and all
* `wrapping` — wrapping arithmetic at every width
* `divide` — integer and floating division by run-time divisors
* `simd-dot` — SSE2 intrinsics against the same kernels in scalar C: dot product, sum of absolute differences, memchr

</details>

## Output agreement

Every program printed the same bytes from all 3 builds.

## Compile time and memory

The `cinrs` column is one `rustc` process: the macro expands the C — lexer, preprocessor, parser, semantic analysis, code generation — and then `rustc` compiles what came out at `-C opt-level=3 -C codegen-units=1`. The native columns are one `gcc`/`clang` process each. Peak resident set is `/usr/bin/time -f %M`.

| program | C lines | gcc (s) | clang (s) | cinrs rustc (s) | cinrs peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fannkuch-redux` | 71 | 0.12 | 0.10 | 0.21 | 131 MiB |
| `n-body` | 141 | 0.12 | 0.11 | 0.21 | 133 MiB |
| `spectral-norm` | 52 | 0.13 | 0.11 | 0.21 | 132 MiB |
| `mandelbrot` | 58 | 0.11 | 0.10 | 0.19 | 131 MiB |
| `fasta` | 108 | 0.13 | 0.10 | 0.24 | 140 MiB |
| `reverse-complement` | 70 | 0.12 | 0.09 | 0.21 | 133 MiB |
| `binary-trees` | 131 | 0.16 | 0.09 | 0.22 | 135 MiB |
| `pidigits` | 65 | 0.12 | 0.11 | 0.26 | 138 MiB |
| `fannkuch-redux-ssse3` | 187 | 0.14 | 0.12 | 0.25 | 140 MiB |
| `n-body-sse` | 233 | 0.33 | 0.21 | 0.39 | 152 MiB |
| `n-body-avx` | 208 | 0.34 | 0.20 | 0.35 | 151 MiB |
| `spectral-norm-sse2` | 87 | 0.14 | 0.13 | 0.24 | 142 MiB |
| `spectral-norm-sse41` | 133 | 0.32 | 0.22 | 0.36 | 149 MiB |
| `spectral-norm-avx2` | 138 | 0.33 | 0.21 | 0.37 | 149 MiB |
| `spectral-norm-avx` | 196 | 0.38 | 0.21 | 0.38 | 153 MiB |
| `mandelbrot-sse2` | 202 | 0.17 | 0.12 | 0.28 | 144 MiB |
| `dhrystone` | 21 | 0.16 | 0.10 | 0.22 | 136 MiB |
| `whetstone` | 433 | 0.14 | 0.11 | 0.22 | 135 MiB |
| `linpack` | 172 | 0.15 | 0.12 | 0.23 | 135 MiB |
| `sieve` | 46 | 0.11 | 0.09 | 0.20 | 130 MiB |
| `recursion` | 46 | 0.13 | 0.09 | 0.19 | 130 MiB |
| `nqueens` | 38 | 0.11 | 0.09 | 0.19 | 131 MiB |
| `matmul` | 62 | 0.12 | 0.10 | 0.20 | 133 MiB |
| `matmul-vla` | 66 | 0.12 | 0.10 | 0.20 | 133 MiB |
| `sort` | 108 | 0.13 | 0.11 | 0.22 | 133 MiB |
| `binsearch` | 53 | 0.11 | 0.09 | 0.19 | 133 MiB |
| `fft` | 97 | 0.13 | 0.10 | 0.21 | 134 MiB |
| `crc32` | 54 | 0.15 | 0.10 | 0.21 | 133 MiB |
| `sha256` | 136 | 0.14 | 0.14 | 0.22 | 136 MiB |
| `prng` | 33 | 0.11 | 0.09 | 0.19 | 133 MiB |
| `life` | 56 | 0.12 | 0.11 | 0.21 | 132 MiB |
| `levenshtein` | 69 | 0.13 | 0.10 | 0.24 | 132 MiB |
| `hashtable` | 75 | 0.14 | 0.11 | 0.27 | 133 MiB |
| `libc-str` | 55 | 0.13 | 0.12 | 0.24 | 133 MiB |
| `hand-str` | 73 | 0.14 | 0.11 | 0.23 | 133 MiB |
| `bitfields` | 84 | 0.13 | 0.10 | 0.25 | 135 MiB |
| `vla` | 44 | 0.13 | 0.25 | 0.23 | 131 MiB |
| `vla-hoisted` | 44 | 0.12 | 0.12 | 0.27 | 132 MiB |
| `interp-switch` | 114 | 0.13 | 0.10 | 0.22 | 134 MiB |
| `interp-goto` | 111 | 0.13 | 0.10 | 0.26 | 134 MiB |
| `structval` | 76 | 0.12 | 0.10 | 0.23 | 134 MiB |
| `chase` | 61 | 0.13 | 0.10 | 0.24 | 133 MiB |
| `statemachine` | 109 | 0.14 | 0.12 | 0.21 | 133 MiB |
| `statemachine-structured` | 108 | 0.14 | 0.10 | 0.23 | 133 MiB |
| `complexmandel` | 44 | 0.12 | 0.10 | 0.28 | 132 MiB |
| `wrapping` | 41 | 0.13 | 0.11 | 0.23 | 132 MiB |
| `divide` | 44 | 0.13 | 0.11 | 0.22 | 132 MiB |
| `simd-dot` | 190 | 0.39 | 0.47 | 0.44 | 150 MiB |

The slowest `cinrs` compilation is `simd-dot` at **0.44 s**, and the largest is `spectral-norm-avx` at **153 MiB** — neither is close to the thresholds this report flags, which are 30 s and 2 GiB.
 Read them against the floor: `fn main() {}`, compiled with the very same flags and the same `--extern cinrs`, costs **0.14 s** and **79 MiB** on this machine. Nearly all of both columns is `rustc` starting and loading the procedural macro, not the C being translated.

"C lines" is the length of the file named in the program table. `dhrystone` is the exception: the twenty-one lines there are an amalgamation that `#include`s Weicker's two source files, about seven hundred lines between them.

## Programs not in the suite

* **k-nucleotide** — the only C version in the corpus (`knucleotide.gcc`) uses `khash.h` from samtools, which is not part of the Benchmarks Game distribution, and OpenMP for its outer loop; there is no version with a hash table of its own to vendor
* **regex-redux** — every C version needs PCRE2, and `pcre2.h` is not installed on this machine; with it, the row would be `#pragma cinrs system_include` plus `#pragma cinrs link "pcre2-8"` and nothing else

## What the numbers say

Across the 48 programs measured, the median `cinrs`/`gcc -O2` ratio is **1.02×**, and **39 of 48** are within 10 % of `gcc -O2` or faster. The extremes are `hand-str` at 0.02× and `structval` at 2.98×.

The `clang` column is what separates the two kinds of difference. `cinrs` and `clang` share a back end, so a row where `clang` is exactly as slow as `cinrs` is not saying anything about the translation at all — it is LLVM's code generator against GCC's, and every Rust program on the machine is subject to it. A row where `cinrs` is slower than **both** is the translation's own.

**Slower than both, which is `cinrs`'s own to answer for:**

* `spectral-norm-sse2` — 1.17× gcc, 1.15× clang. spectral-norm two columns at a time, `_mm_set_pd`/`_mm_div_pd` (SSE2)
* `vla` — 1.26× gcc, 1.24× clang. a variable length array made afresh every iteration (a `Vec` in the expansion)
* `complexmandel` — 1.43× gcc, 1.33× clang. `double _Complex` arithmetic, Annex G recovery and all

**Slower than `gcc` but level with `clang`, i.e. LLVM against GCC and not this crate:**

* `libc-str` — 1.15× gcc, 1.07× clang.
* `fannkuch-redux-ssse3` — 1.26× gcc, 1.07× clang.
* `interp-goto` — 1.53× gcc, 0.95× clang.
* `simd-dot` — 1.58× gcc, 1.04× clang.
* `levenshtein` — 1.77× gcc, 1.07× clang.
* `structval` — 2.98× gcc, 0.97× clang.

**Faster than `gcc -O2`:**

* `hand-str` — 0.02× gcc.
* `recursion` — 0.13× gcc.
* `mandelbrot-sse2` — 0.56× gcc.
* `matmul-vla` — 0.60× gcc.
* `n-body-avx` — 0.61× gcc.
* `linpack` — 0.65× gcc.
* `life` — 0.76× gcc.
* `spectral-norm-avx` — 0.79× gcc.
* `divide` — 0.85× gcc.
* `chase` — 0.85× gcc.

### Why, construct by construct

What the expansion does with each of these is in [What works](features.md); what it costs is here.

* **Arithmetic wraps for free.** C's unsigned arithmetic is modular, so the expansion is `wrapping_add`, `wrapping_mul` and their relatives rather than Rust's `+` and `*`. Those are `#[inline]` intrinsics that lower to the bare instruction, and `prng` — a loop that is nothing but a multiply, an add and three shifts — and `wrapping`, which does the same at every width C has, are the measurement: both land on `gcc`. If they did not, every arithmetic row in the table would be paying for it.
* **There are no bounds checks.** A C array is a raw pointer and a subscript is `.offset()`, which is plain address arithmetic with no check in it — `sieve`, `matmul`, `life`, `crc32` and `binsearch` are where that shows, and none of them has a Rust tax to pay.
* **Division has a check C does not.** Rust's `/` and `%` panic on a zero divisor, and signed division also has to rule out `INT_MIN / -1`; C's do neither. `divide` is a loop of nothing but divisions by run-time divisors, which is the worst case, and the check does not show above the latency of the divider itself.
* **A bit-field is a pair of methods.** A bit-field has no address, so it is not a field of the generated `#[repr(C)]` struct: a run of them shares one `[u8; K]` and each named member becomes `s.ttl()` and `s.set_ttl(v)`. `bitfields` parses and repacks an IP header a hundred million times over, so every one of those is a call that has to be inlined and folded back into a shift and a mask before it can keep up. It does.
* **A variable length array is a `Vec`.** Rust cannot move the stack pointer by an amount chosen at run time, so a VLA and `alloca` are emulated on the heap. `vla` makes one per iteration and `vla-hoisted` does the same work with one `malloc` outside the loop; the difference between those two rows is exactly what the emulation costs, and it is an allocation per declaration rather than a stack adjustment. A VLA declared once and used in a loop — which is what `spectral-norm` and `fannkuch-redux` do — costs nothing.
* **A `goto` keeps the shape its C had.** A jump forwards to a label later in a block it stands in becomes `break 'done`, and one backwards to a label that block begins with becomes `continue 'retry`, so the function keeps the shape its C had — hot loop included. `whetstone`, whose `main` and whose inner `PA` are built out of backward jumps, and `interp-switch`, whose dispatch loop leaves through `goto done`, are what that is worth: both were more than 40 % behind `gcc` when every jump went through a state machine over basic blocks. What no Rust label can express — a jump *into* a block, or two labels whose regions would have to overlap without nesting — goes through a control-flow graph, and a relooper reads that back into Rust's own loops and `match`es, so the hot loop is a hot loop there too. `statemachine` and `statemachine-structured` are the same lexer over the same input, one written with a dozen labels that jump into one another every which way and one with `while` and `switch`: all three compilers take the same time over both, where `cinrs` was 2.16× on the `goto` version and 1.02× on the other before the graph was relooped. What is still lowered as a state machine is a computed `goto`, whose `&&label` *is* a block's number — `interp-goto` is that row, and it is level with `clang`.
* **`switch` becomes `match`.** `interp-switch` is a bytecode dispatch loop with a fallthrough case; the fallthrough has to run the next arm's body without re-dispatching, which is what the labelled-block chain the expansion builds is for. The `goto done` that leaves the loop costs it nothing any more.
* **Structs by value go through Rust's C ABI.** `structval` passes and returns a two-`double` struct (two SSE registers on x86-64 System V) and a three-`long` one (memory). Nothing in the expansion decides that — `#[repr(C)]` and `extern "C"` hand it to `rustc`.
* **`_Complex` is `num_complex::Complex`.** `complexmandel` multiplies complex numbers in the inner loop, and C's complex multiplication is not four multiplies and two adds: Annex G.5.1 requires an infinity-recovery path. What the row measures is what that path costs when it is never taken.
* **The C library is the C library.** `libc-str`, `chase` and `pidigits` are controls — a `strlen` call, a dependent load, and a program whose work is all inside GMP. They should be the same in every column, and are.

### What is *not* being measured

* **Parallelism.** Every program here is single-threaded, and the Benchmarks Game versions were chosen to be. `#pragma omp` is ignored by `cinrs`, so the native builds are compiled without `-fopenmp` and the comparison is serial against serial.
* **`-march=native`.** Neither side gets it. Both are built for the base x86-64 target, so neither back end may use AVX-512 unless it can prove it is there.
* **Link-time optimisation.** Neither side gets that either. Each program is one translation unit and one crate, so there is nothing across units to optimise.
* **Long-running programs.** Everything here is under two seconds, which is enough for the median of five runs to be stable to about a millisecond but not enough to say anything about a program whose working set grows for an hour.

