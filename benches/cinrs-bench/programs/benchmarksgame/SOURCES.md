# The Benchmarks Game sources vendored here

Every `.c` file in this directory is a **verbatim** copy of a program from
[The Computer Language Benchmarks Game][bg], renamed from the upstream
`<name>.gcc*` to `<name>.c` and otherwise byte for byte what upstream ships,
contributor credit and all. Nothing in them is edited: the include path and the
`#define`s the harness needs are added *around* the file when it is wrapped,
never inside it, and the Rust `mod` that gives the unit a path is outside the
string literal altogether.

[bg]: https://benchmarksgame-team.pages.debian.net/benchmarksgame/

## Licence

Revised BSD, in `LICENSE` next to this file — the copy of `LICENSE.md` from the
upstream repository. Copyright © 2004-2008 Brent Fulgham, 2005-2024 Isaac Gouy;
each program also carries the name of its contributor in its own header
comment, which is the notice clause 1 of the licence requires to be retained,
and which is intact in every file here.

## Where they came from

Fetched on **2026-09-07** from

    https://salsa.debian.org/benchmarksgame-team/benchmarksgame

at commit **`40296663ed350d5fe4a6ab5e367bab61cb77c219`** (master, authored
2025-03-07), through the source archive the site publishes at
`public/download/benchmarksgame-sourcecode.zip`, which is where the repository
keeps the programs themselves. `LICENSE` is `LICENSE.md` at the same commit.

| file here | upstream path in the archive | why this version |
| --- | --- | --- |
| `fannkuchredux.c` | `fannkuchredux/fannkuchredux.gcc-8.gcc` | the shortest of the seven C versions (71 lines); the rest use pthreads, OpenMP or `vector_size` |
| `nbody.c` | `nbody/nbody.gcc` | the original single-threaded version; `nbody.gcc-4`, `-5` and `-9` are SIMD intrinsics |
| `spectralnorm.c` | `spectralnorm/spectralnorm.gcc-8.gcc` | 52 lines, no OpenMP and no intrinsics — the only one of the seven that is neither |
| `mandelbrot.c` | `mandelbrot/mandelbrot.gcc-8.gcc` | the only version with no SIMD; the other eight use `vector_size` or pthreads |
| `fasta.c` | `fasta/fasta.gcc-8.gcc` | the shortest (108 lines) and single-threaded |
| `revcomp.c` | `revcomp/revcomp.gcc-4.gcc` | 70 lines, single-threaded; `-2`, `-7` and `-9` use pthreads and `sched_setaffinity` |
| `binarytrees.c` | `binarytrees/binarytrees.gcc` | the plain `malloc`/`free` version; `-2` and `-3` need the Apache Portable Runtime, `-5` needs pthreads |
| `pidigits.c` | `pidigits/pidigits.gcc` | the shorter of the two GMP versions |

### The SIMD-intrinsics versions

Three more programs are the Intel-intrinsics versions of two of the same
benchmarks, vendored on the same terms and run at the same sizes as the plain
rows, so that the two rows of a benchmark can be read against each other. They
were fetched on **2026-09-23** from the same archive URL; the repository's
`master` was still commit **`40296663ed350d5fe4a6ab5e367bab61cb77c219`**
(authored 2025-03-07, according to the salsa GitLab API that day), so they come
from the same commit as the eight above.

None of the three headers states the compiler flags it was written for, so the
instruction set in the last column is read off the intrinsics each one calls;
it is what the harness row's `features` passes as `-mNAME` to `gcc` and `clang`
and as `-C target-feature=+NAME` to `rustc`. None of them has an `#ifdef` on
`__SSE3__`, `__AVX__` or the like, so there is no second code path that the
`cinrs` build could take instead.

| file here | upstream path in the archive | why this version | intrinsics and instruction set | flags |
| --- | --- | --- | --- | --- |
| `fannkuchredux_ssse3.c` | `fannkuchredux/fannkuchredux.gcc-4.gcc` | the one C version that is intrinsics without pthreads, OpenMP or `vector_size` | `_mm_shuffle_epi8` for every flip and rotation (SSSE3, `<tmmintrin.h>`), `_mm_load_si128`/`_mm_loadu_si128`/`_mm_store_si128`/`_mm_storel_epi64` (SSE2); calls `exit` and `atoi` with no `<stdlib.h>`, which GCC's and Clang's `<xmmintrin.h>` bring in through `<mm_malloc.h>` | `-mssse3` |
| `nbody_sse.c` | `nbody/nbody.gcc-4.gcc` | two interactions at a time in a `__m128d`, single-threaded | `_mm_cvtpd_ps`, `_mm_rsqrt_ps`, `_mm_cvtps_pd` (SSE/SSE2, `<immintrin.h>`), and the GNU vector operators `*`, `+`, `-`, `/` on `__m128d` and between `__m128d` and `double`; calls `atoi` with no `<stdlib.h>` | none — the x86-64 baseline |
| `nbody_avx.c` | `nbody/nbody.gcc-9.gcc` | one body per `__m256d`, single-threaded; `-5` is the same idea with `vector_size` | `_mm256_hadd_pd`, `_mm256_permute2f128_pd`, `_mm256_blend_pd`, `_mm256_cvtpd_ps`/`_mm256_cvtps_pd`, `_mm_rsqrt_ps` (AVX, `<x86intrin.h>`); defines its own `static inline __m256d _mm256_rsqrt_pd(__m256d)`, a 256-bit vector by value, and uses `*` on `__m256d` once, in it | `-mavx` |

None of these eleven contains a `#pragma omp`, so no OpenMP is being switched off:
the native builds and the `cinrs` build are serial in exactly the same way. The
native compilers are invoked **without** `-fopenmp` regardless, so that a
version which did have one would still be compared serial against serial —
`cinrs` ignores `#pragma omp`.

## The two that are not here

* **k-nucleotide.** The corpus has exactly one C version, `knucleotide.gcc`,
  and it `#include`s `khash.h` — Attractive Chaos's hash table from samtools,
  which the Benchmarks Game does not distribute — and uses `#pragma omp` for
  its outer loop. There is no version with a hash table of its own to vendor,
  so the benchmark is left out rather than reconstructed. `hashtable.c` in
  `../kernels/` measures the same thing (open addressing over a large table)
  in a program this suite can vouch for.
* **regex-redux.** All four C versions need PCRE2, and `pcre2.h` is not
  installed on the machine these results were measured on. Adding it back is
  one row in `src/main.rs` with `system_include: true` and
  `libs: &["pcre2-8"]`, which is exactly what `pidigits` does for GMP.

## Inputs

The sizes in `src/main.rs` are the site's own, scaled so that a `gcc -O2` run
takes roughly half a second on the machine in `doc/benchmarks.md` — smaller
than the site's, which are tuned for a much longer measurement. The one input
that is a file is reverse-complement's, which is the output of `fasta
10000000` (about 100 MB), generated once into `target/cinrs-bench/data/` with
the `gcc` build of `fasta.c` — which is how the site feeds it too.
