# cinrs-bench

Is a C program written inside a `c99!` block as *fast* as the same C program
compiled by a C compiler?

The rest of this repository measures whether `cinrs` gets C **right** — three
public corpora, about 2,270 cases, in `doc/testsuites.md`. This crate measures
whether it gets C **fast**. Each program below is one whole C program with a
`main`, built three ways and run:

1. `gcc -O2 -std=gnu11`
2. `clang -O2 -std=gnu11`
3. the same C inside `cinrs::gnu11! { r#"…"# }`, compiled by
   `rustc -C opt-level=3 -C codegen-units=1` against a release build of the
   crate

and then the outputs of all three are compared **byte for byte**. A
miscompilation found that way is worth more than any of the timings, so the
comparison is not optional and a mismatch fails the run.

The results, and what they mean, are in [`doc/benchmarks.md`](../../doc/benchmarks.md).

## Running it

```text
( ulimit -v 8000000; timeout -k 10 3600 \
      cargo run -p cinrs-bench --release -- --markdown doc/benchmarks.md )
```

The outer `ulimit` and `timeout` are the house rule for anything in this
repository that compiles or runs generated code; see the memory section of
[`doc/testsuites.md`](../../doc/testsuites.md). The harness *also* wraps every
child of its own in `sh -c 'ulimit -v …; exec "$@"' … timeout -k 10 …`, so a
runaway compiler or a miscompiled program cannot take the machine with it, but
the outer one is what catches the harness itself.

A full run is a few minutes. Options:

| option | |
| --- | --- |
| `--filter NAME` | only programs whose name contains `NAME` — this is how to look at one row |
| `--runs N` | timed runs per build, default 5; the median is reported |
| `--compilers LIST` | from `gcc`, `gcc-O3`, `clang`; default `gcc,clang` |
| `--timeout SECS` | per-execution ceiling, default 60. A run that hits it is killed and reported as timed out rather than waited on — a build that is *that* much slower is itself the finding |
| `--build-timeout S` | per-compilation ceiling, default 600 |
| `--mem-limit-kb KB` | address-space ceiling for every child, default 8000000; `0` inherits |
| `--markdown PATH` | write the report there as well as to stdout |
| `--list` | print the program table and stop |
| `--keep-going` | carry on past a program that fails to build |

The exit status is non-zero if any program's builds disagreed about the output,
or if any program failed to build.

## What is measured, and why those flags

Wall-clock time of the whole process, median of `--runs`, with `stdout` sent to
`/dev/null`. One extra run per build, untimed, sends `stdout` to a file, and
that is what the outputs are compared from. All three builds are started
through the same `sh` + `timeout` wrapper, so the constant few milliseconds of
process setup is in every column and cancels out of the ratio.

`gcc -O2` and `clang -O2` compile one translation unit. `rustc -C opt-level=3
-C codegen-units=1` compiles one crate, which here is also one unit: that is
Cargo's `release` profile — `opt-level = 3`, debug assertions off, overflow
checks off — with `codegen-units` turned down to one, so that the whole
program reaches LLVM at once the way the whole `.c` file reaches GCC. `-O2` is
what a C program is normally built with and is the honest thing to compare
against; `-O3` for gcc is available as a fourth column with
`--compilers gcc,gcc-O3,clang`, since LLVM's `opt-level=3` nominally
corresponds to it.

The harness also records what the `cinrs` compilation cost — wall clock and
peak resident set of the `rustc` process, from `/usr/bin/time -f "%e %M"` —
because the expansion happens inside it, and a front end that took thirty
seconds or two gigabytes on a two-hundred-line program would be worth knowing
about.

## The programs

`--list` prints the table. There are three families:

* **`programs/benchmarksgame/`** — eight programs from
  [The Computer Language Benchmarks Game][bg], vendored verbatim under the
  Revised BSD licence in `LICENSE`, with every file's provenance in
  `SOURCES.md`. The version chosen for each is the simplest single-threaded C
  one: no pthreads, no `vector_size`, no intrinsics. k-nucleotide and
  regex-redux are absent, and `SOURCES.md` says why. Next to them are the
  Intel-intrinsics versions of fannkuch-redux (SSSE3) and n-body (SSE2 and
  AVX), at the same sizes, built for the instruction set in the row's
  `features`.
* **`programs/classic/`** — Dhrystone 2.1 and Whetstone, both vendored from
  netlib with their notices intact, and a LINPACK-style LU solve written for
  this suite because the netlib translation has no licence text and reports a
  rate rather than a result. `SOURCES.md` has the licence reasoning for all
  three.
* **`programs/kernels/`** — twenty-eight programs written for this suite,
  MIT OR Apache-2.0 like the crate, each self-contained, each printing a
  checksum so that the three builds can be compared. They are chosen to isolate
  one thing each: bit-fields, heap-emulated VLAs, `switch` dispatch against
  computed `goto`, `_Complex` arithmetic, wrapping arithmetic, division,
  structs by value, `goto` lowering, pointer chasing, and the ordinary loops
  (matrix multiply, FFT, sorting, sieving) that any code generator has to get
  right.

[bg]: https://benchmarksgame-team.pages.debian.net/benchmarksgame/

## Adding a program

1. Put the C in `programs/kernels/` (or vendor it, with a `SOURCES.md` entry
   and its licence header intact). It has to be **self-contained**, take its
   size from `argv` so the harness can tune it, and print a **deterministic
   checksum** — no timings, no addresses, no `rand()` without a fixed seed.
   Size it so `gcc -O2` takes roughly half a second.
2. Add a row to `PROGRAMS` in `src/main.rs`. The `program()` helper fills in the
   defaults — `gnu11!`, `main(int, char **)`, no input, no libraries — and the
   struct update syntax overrides what differs:

   ```rust
   Program {
       args: &["1024"],
       note: "what this row is for, one line",
       ..program("matmul", "kernels/matmul.c", Group::Kernel)
   },
   ```

   The fields that matter:

   | field | |
   | --- | --- |
   | `dialect` | `Gnu11` (default) or `Gnu89`, which also picks the native `-std=` |
   | `main` | `ArgcArgv` (default) or `NoArgs`, which decides what the generated `fn main` calls |
   | `args`, `stdin` | what sizes the run; `StdinSource::Fasta` is the 100 MB fasta output, generated once |
   | `defines` | `-DNAME` natively, `#define NAME 1` in front of the C for `cinrs` |
   | `libs` | `-lNAME` natively, `rustc -l NAME` for the `cinrs` build |
   | `features` | instruction sets above the x86-64 baseline: `-mNAME` natively, `rustc -C target-feature=+NAME,…` for the `cinrs` build (the program's crate only; `__AVX__` and the like stay undefined for the C) |
   | `system_include` | sets `CINRS_SYSTEM_INCLUDE=1`, so the unit may read the platform's own headers |
   | `output_filter` | lines holding these substrings are dropped before the outputs are compared — for a program that prints its own elapsed time or a pointer value, and for nothing else |

3. Run `cargo run -p cinrs-bench --release -- --filter <name>` and check that
   the row says `same` in the output column.

### How the C reaches `cinrs`

Exactly the way `tests/c_testsuite.rs` does it: the whole translation unit goes
into one raw string literal — the input form that accepts every C token,
hexadecimal floating constants and `'ab'` included — inside a `mod bench` of its
own, and a `fn main` that calls the C `main` as `bench::main`, through the glob
re-export inside that module, and exits with what it returned. Two things are
*prepended* to the C, because the preprocessor acts on them where it reads them
and they must be in force before the first `#include`: `#pragma cinrs
include_path` pointing at the C file's own directory (a string literal has no
directory of its own, and Dhrystone includes a second `.c` file next to it),
and the row's `#define`s.

The generated file is left in `target/cinrs-bench/<name>/main.rs`, which is the
first place to look when a row does not build. `cargo run -p cinrs-core
--example frontend -- <file.c> gnu11 --print` prints the Rust the front end
generates for a C file, which is the second.

## `cargo test`

The crate has four tests, all cheap, and they are what `cargo test --workspace`
runs from here: the program table names files that exist and no two rows share
a name; the generator produces the `mod bench` wrapper; the raw-string hash
count is right; the output filter drops what it is told to. A fifth builds one
small kernel with `gcc` and runs it, skipping itself when there is no `gcc` on
the `PATH`, and is capped like everything else the harness starts. Nothing here
runs a benchmark.
