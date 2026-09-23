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

Eight more programs are the Intel-intrinsics versions of four of the same
benchmarks — every C version in the archive that is written with `<*intrin.h>`
and does not need pthreads — vendored on the same terms and run at the same
sizes as the plain rows, so that the rows of a benchmark can be read against
each other. They were fetched on **2026-09-23** from the same archive URL; the
repository's `master` was still commit
**`40296663ed350d5fe4a6ab5e367bab61cb77c219`** (authored 2025-03-07, according
to the salsa GitLab API that day), so they come from the same commit as the
eight above.

Only `mandelbrot.gcc-6` states compiler flags, and `spectralnorm.gcc-2` and
`-7` say "`-march=native -fopenmp` for best results"; for the rest the
instruction set in the last column is read off the intrinsics each one calls.
It is what the harness row's `features` passes as `-mNAME` to `gcc` and `clang`
and as `-C target-feature=+NAME` to `rustc`; `-march=native`, `-ffast-math`
and `-O3` are not passed, so that every row is built the same way. None of
them has an `#ifdef` on `__SSE3__`, `__AVX__` or the like, so there is no
second code path that the `cinrs` build could take instead.

`mandelbrot_sse2.c` begins with a UTF-8 byte-order mark, as upstream's does;
`spectralnorm_sse41.c` and `spectralnorm_avx2.c` have CRLF line endings and
trailing blanks, and the former ends in a line of four spaces with no newline.
All of that is upstream's and kept. The copies were made with `cp` and checked
with `cmp`, since an editor would normalise it.

| file here | upstream path in the archive | why this version | intrinsics and instruction set | flags |
| --- | --- | --- | --- | --- |
| `fannkuchredux_ssse3.c` | `fannkuchredux/fannkuchredux.gcc-4.gcc` | the one C version that is intrinsics without pthreads, OpenMP or `vector_size` | `_mm_shuffle_epi8` for every flip and rotation (SSSE3, `<tmmintrin.h>`), `_mm_load_si128`/`_mm_loadu_si128`/`_mm_store_si128`/`_mm_storel_epi64` (SSE2); calls `exit` and `atoi` with no `<stdlib.h>`, which GCC's and Clang's `<xmmintrin.h>` bring in through `<mm_malloc.h>` | `-mssse3` |
| `nbody_sse.c` | `nbody/nbody.gcc-4.gcc` | two interactions at a time in a `__m128d`, single-threaded | `_mm_cvtpd_ps`, `_mm_rsqrt_ps`, `_mm_cvtps_pd` (SSE/SSE2, `<immintrin.h>`), and the GNU vector operators `*`, `+`, `-`, `/` on `__m128d` and between `__m128d` and `double`; calls `atoi` with no `<stdlib.h>` | none — the x86-64 baseline |
| `nbody_avx.c` | `nbody/nbody.gcc-9.gcc` | one body per `__m256d`, single-threaded; `-5` is the same idea with `vector_size` | `_mm256_hadd_pd`, `_mm256_permute2f128_pd`, `_mm256_blend_pd`, `_mm256_cvtpd_ps`/`_mm256_cvtps_pd`, `_mm_rsqrt_ps` (AVX, `<x86intrin.h>`); defines its own `static inline __m256d _mm256_rsqrt_pd(__m256d)`, a 256-bit vector by value, and uses `*` on `__m256d` once, in it | `-mavx` |
| `spectralnorm_sse2.c` | `spectralnorm/spectralnorm.gcc-5.gcc` | the original intrinsics version: two columns of a row per `__m128d`, A's entries computed in scalar | `_mm_setzero_pd`, `_mm_set_pd`, `_mm_div_pd`, `_mm_add_pd` (SSE2, `<emmintrin.h>`), the vector subscript `sum[0] + sum[1]`, `memalign` from `<malloc.h>`, a non-`static` C99 `inline` function; OpenMP on both matrix products | none — the x86-64 baseline |
| `spectralnorm_sse41.c` | `spectralnorm/spectralnorm.gcc-7.gcc` | a different algorithm from `-5`: A's entries are computed four at a time in `__m128i`, and the sum is read back through `((double*)&mysum)[k]` | `_mm_mullo_epi32` (SSE4.1), `_mm_set1_epi32`, `_mm_setr_epi32`, `_mm_add_epi32`, `_mm_srli_epi32`, `_mm_shuffle_epi32` with `_MM_SHUFFLE`, `_mm_cvtepi32_pd`, `_mm_load_pd`, `_mm_div_pd` (SSE2), `_mm_malloc`/`_mm_free` (`<x86intrin.h>`); OpenMP | `-msse4.1` (it asks for `-march=native`) |
| `spectralnorm_avx2.c` | `spectralnorm/spectralnorm.gcc-2.gcc` | `-7` by the same authors at twice the width — not a duplicate, since every integer operation becomes its 256-bit AVX2 form | `_mm256_mullo_epi32`, `_mm256_add_epi32`, `_mm256_srli_epi32`, `_mm256_extracti128_si256` (AVX2), `_mm256_castsi256_si128`, `_mm256_cvtepi32_pd`, `_mm256_load_pd`, `_mm256_div_pd` (AVX), `_mm_malloc`/`_mm_free`; OpenMP | `-mavx2` (it asks for `-march=native`; its header says "AVX intrinsics", but the integer half is AVX2) |
| `spectralnorm_avx.c` | `spectralnorm/spectralnorm.gcc-6.gcc` | a third algorithm: 4×4 blocks of A, a reciprocal approximation refined by a Goldschmidt step instead of a division | `_mm256_permute_pd`, `_mm256_permute2f128_pd`, `_mm256_blend_pd`, `_mm256_unpacklo_pd`/`hi`, `_mm256_hadd_pd`, `_mm256_cvtepi32_pd`, `_mm256_extractf128_pd` (AVX), `_mm_rcp_ps`, `_mm_mullo_epi32` (SSE4.1, implied by AVX), `_mm_sqrt_pd`; `static inline` functions taking and returning `__m256d` by value; VLAs with `__attribute__((aligned(32)))`; OpenMP | `-mavx` |
| `mandelbrot_sse2.c` | `mandelbrot/mandelbrot.gcc-6.gcc` | the only mandelbrot that uses the Intel vector types without `vector_size` or pthreads | **no intrinsic calls at all**: `__m128d` from `<emmintrin.h>` with the GNU vector operators, vector subscripts `v[0][1]`, compound literals `(__m128d){xy, xy+1}`, `double * __m128d`; a VLA of `__m128d`; `write` from `<unistd.h>`; non-`static` C99 `inline` functions; OpenMP | `-msse3` (its header: `-pipe -Wall -O3 -ffast-math -fno-finite-math-only -march=native -mfpmath=sse -msse3 -fopenmp`) |

The four spectral-norm versions and mandelbrot's carry `#pragma omp parallel
for`; the other eleven contain none. No OpenMP is switched on anywhere: the
native compilers are invoked **without** `-fopenmp`, and `cinrs` ignores
`#pragma omp`, so every build is serial in exactly the same way.

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
