# ccinrs on real programs

[`ccinrs`](ccinrs.md) is meant to be the C compiler a project's own build
system calls. This page records what happened when it was: 27 open-source
programs, from bzip2 to CPython, OpenSSL, FFmpeg and git, each built from its
release tarball with its own build system and
`CC=ccinrs`, its own test suite run, and the result timed against the same
source compiled by GCC. It is the long version of the table in
[ccinrs.md](ccinrs.md#what-it-has-built), which lists what
`scripts/check-ccinrs.sh` re-checks.

The projects themselves are not in this repository. Every bug the work found
was fixed in cinrs or ccinrs with a regression test that names the program
(`tests/*.rs`, `tests/ui/`, `crates/ccinrs/tests/cli.rs`), which is what the
continuous integration keeps; the projects were built again afterwards, and
the tables give the last state.

## How it was measured

* **Machine**: AMD Ryzen 9 9950X (16 cores, 32 threads), Linux 6.18 under
  WSL2; GCC 15.2.0, Clang 21.1.8, rustc 1.99.0.
* **Sources**: the official release archive of each project, checked against
  its published hash, and never edited. A setting a project offers for its
  own build (`ZSTD_NO_ASM=1`, `--disable-asm`, a `configure` cache variable)
  counts as a build choice; anything beyond that is listed as a workaround.
* **Builds**: the project's own build system — Makefiles, autoconf and
  libtool, CMake, Perl's `Configure`, nginx's and FFmpeg's own `configure` —
  with `CC=ccinrs` and the flags the project chooses, at `make -j2`. GCC
  builds the same configuration, so that the comparison is of compilers, not
  of code paths: where ccinrs cannot take a project's assembly, GCC is
  measured without it too (`no-asm`), and GCC's normal build is shown beside
  it for reference.
* **Tests**: the project's own suite, compared with GCC's run test by test.
  Where a test fails for both, it is the machine's (WSL2's networking, mostly).
* **Speed**: ccinrs's time divided by GCC's, so more than 1 is slower; for a
  throughput the inverse. The median of three to five runs pinned to one core,
  on a machine that was building other projects at the same time — differences
  within about 10 % are noise. "checks" is ccinrs as it comes, with Rust's
  run-time checks on; "no checks" is `-fno-cinrs-checks`.
* **ccinrs changed during the work.** The numbers are the last measurement of
  each project; where a later fix is known to change them, the row says so.

## The results

| project | build system | what it needed | tests | speed, checks / no checks |
| --- | --- | --- | --- | --- |
| SQLite 3.53.4 | autoconf | nothing | shell and library smoke tests: output identical | speedtest1 1.26 / 1.08 |
| mbedtls 3.6.7 | make | nothing | 140 suites, 32,263 tests: all pass | 60 benchmark lines: geometric mean 1.41 / 1.01 ¹ |
| zstd 1.5.7 | make | `ZSTD_NO_ASM=1` (a `.S` file) | `make check`, fuzzer, zstreamtest, legacy, decodecorpus | compress 1.18–1.51 / 0.94–1.06, decompress 1.57–1.97 / 1.01–1.12 |
| xz 5.8.4 | autoconf | `--enable-symbol-versions=generic` (`alias`) | `make check` 22/22 | compress 1.09–1.11 / 1.00–1.06, decompress 1.06–1.07 / 1.00–1.02 |
| bzip2 1.0.8 | make | nothing | `make test` | 1.11–1.27 / 0.98–1.07 |
| PCRE2 10.49 | autoconf | nothing | `make check`, with and without the JIT | interpreter 1.19–1.30 / 1.09–1.17, JIT 1.00–1.20 / 0.99–1.09 |
| libyaml 0.2.5 | autoconf | nothing | `make check`; 40 MB of YAML through every tool, byte-identical | 1.14–1.61 / 1.05–1.29 |
| libpng 1.6.59 | autoconf | nothing | `make check` 36/36, every log identical | 0.95–1.02 / 0.88–0.99 |
| QuickJS 2026-06-04 | make | nothing | `make test` 11/11 | 1.49–2.80 / 1.12–1.92 ¹ |
| libsodium 1.0.22 | autoconf | `--disable-asm` (`.S` files) | `make check` 101/101 | 1.04–1.46 / 0.86–1.17 against GCC `--disable-asm` |
| libxml2 2.15.4 | autoconf | nothing | `make check` (runtest's 3,910, testapi, runsuite, …) | 1.11–1.25 / 1.00–1.01 |
| jansson 2.15.1 | autoconf, CMake | nothing | the suites; ctest 203/203 | 0.98–1.15 / 0.91–1.04 |
| libuv 1.53.0 | CMake | nothing | 526 of 527, as GCC | about 1.0; the event loop's `loop_count` 2.4 / 2.4 ² |
| libjpeg-turbo 3.2.0 | CMake | nothing; with NASM, its SIMD objects link in | ctest 664/664 | 1.06–1.52 / 0.63–1.01 |
| libwebp 1.6.0 | CMake | nothing | 224 round trips, byte-identical | 1.05–1.80 / 0.84–1.34 ³ |
| curl 8.22.0 | autoconf | two problems, both fixed since | 1,572 of 1,579 (GCC 1,575; one test passes `long double` to curl's printf) | 0.97–1.42 / 0.66–1.33 ² |
| libevent 2.1.13 | autoconf | nothing | regress: 356, as GCC | about 1.0 (bound by system calls) |
| Lua 5.4.9 | make | nothing | the official suite, to "final OK" | 1.0–1.1 without checks; raising errors 8.7 ⁴ |
| jq 1.8.2 | autoconf | nothing | `make check`: all pass | 0.82–1.21 / 0.74–1.06 |
| Redis 8.10.2 | make (`-C src`, its `-flto`) | nothing | its whole suite, as GCC but one ⁵ | the benchmark at GCC's rate; Lua `EVAL` 1.24 / 0.94 |
| jemalloc 5.3 (Redis's) | autoconf | nothing | 138/138 | — |
| CPython 3.14.8 | autoconf | `asm_trampoline.S` assembled by GCC, and a `configure` cache variable ⁶ | 446 test files pass (GCC 453) ⁹ | 1.70 / 1.30 |
| OpenSSL 3.5.9 | `Configure no-asm` | nothing | 345 of 347 files, the other two the async fibres ⁴ | 1.06–1.44 / 0.92–1.20 |
| Tcl 9.0.4 | autoconf | nothing | 68,225 tests, as GCC | the bytecode loop 1.5; the rest 1.0–1.13 |
| FFmpeg 9.0.2 | its `configure`, `--disable-asm` | nothing ⁶ | FATE 2,844 of 2,845, as GCC | 1.08–2.31 / 0.74–1.18; scaling 4.1 / 1.8 ⁷ |
| git 2.56.0 | make | nothing ⁸ | 33,242 tests: all pass | 1.10 / 1.01 |
| nginx 1.30.5 | its `configure` | nothing | 6,093 tests, as GCC, every module | requests per second as GCC's |

¹ Measured before later changes that help it. With the checks on, the inliner
now reaches as far as without them: mbedtls's ChaCha20 was 2.4 times as slow
because its quarter round stopped being inlined, and its reproduction now runs
at the speed it has without the checks. QuickJS's interpreter allocates a frame
with `alloca` on every call, and the chunks are now reused across calls.

² Both spend their time zero-filling uninitialised local arrays — libuv 15 KB
of `epoll_event`s on every turn of the loop, curl's `snprintf` two 128-byte
buffers per call. ccinrs zero-fills them by default; GCC's own
`-ftrivial-auto-var-init=uninitialized` leaves them alone, and libuv's loop
then runs at GCC's speed (its reproduction: 1.04 s → 0.006 s; GCC 0.009 s).

³ The lossless encoder was 2.7 times as slow in one SSE2 function from a false
dependency on `tzcnt`'s output register; `__builtin_ctz` no longer has it, and
that function runs at GCC's speed. The row is from before.

⁴ A `longjmp` is a Rust unwind ([limitations](limitations.md#setjmp-and-longjmp)):
about 1.2 µs plus 130 ns a frame, where GCC's takes nanoseconds — Lua's `error`
and `pcall` feel it, Lua's ordinary code does not — and one cannot cross to
another stack, which OpenSSL's async jobs do with `makecontext`; those abort
with cinrs's message.

⁵ `HINCRBYFLOAT` and `INCRBYFLOAT` round through `long double`, which is
`double` here: `1.23` comes back as `1.22999999999999998`.

⁶ Earlier rounds needed more: ccinrs refused under `-std=c11` and `-std=c17`
what GCC accepts there with a warning — CPython builds every file with
`-std=c11`, and FFmpeg has a stray `;` after a function — and `__DATE__` was a
placeholder CPython's `sys.version` parser rejects. ccinrs's `-std=` modes now
take what GCC's take unless `-pedantic-errors` is given, and `__DATE__` is the
date of translation (or `SOURCE_DATE_EPOCH`'s); rebuilt with that, FFmpeg needs
nothing, and CPython only its trampoline's assembly and the cache variable
described [below](#configure-and-the-run-time-checks).

⁷ `hScale8To15_c`'s inner loop indexes with `int`, and cinrs's signed
arithmetic wraps on overflow, as GCC's does under `-fwrapv`; GCC with
`-fwrapv` is just as slow on it.

⁸ git's SHA-1 code reads `uint32_t`s from unaligned addresses, which is
undefined behaviour the run-time checks stop at; its own
`-DSHA1DC_FORCE_ALIGNED_ACCESS` avoids it, and is needed only with the checks.

⁹ Of CPython's test files, those that fail here and pass with GCC are
`test_ctypes`' four `long double` tests (note 5), `test_memoryio` (the
undefined behaviour [below](#what-it-found)), the two `test_gdb` files (gdb's
CPython printers want C's debug information, and the program has Rust's),
`test_perf_profiler`, and `test_socket` and `test_ssl`, which fail now and
then with GCC as well.

`scripts/check-ccinrs.sh` covers six more — lz4, cJSON, cmark, brotli, zlib,
expat — and c-testsuite; see [ccinrs.md](ccinrs.md#what-it-has-built).

## What it found

About sixty problems, each now fixed and tested. Two were silent — a program
that compiled and gave different answers — and both are the reason the tests
are compared test by test rather than counted:

* **QuickJS**: an enumeration with no negative enumerator is `unsigned int` in
  GCC and Clang, and a three-bit bit-field of one holds 4. cinrs read it back
  as −4, and QuickJS aborted on its first closure.
* **git**: `struct object_info empty = { 0 }` is compared with `memcmp`, and
  GCC zeroes the padding as well. cinrs left it as the stack had it, and three
  of git's tests passed or failed with the stack's contents.

The rest stopped a build, and fall into a few kinds:

* **The preprocessor**: a macro redefined differently is a warning, as in GCC
  (zstd's `assert` before `<assert.h>`); a paste that makes a pp-number
  (FFmpeg's `63.1 ## .`); a directive inside a macro's arguments (CPython).
* **The language as GCC has it**: GNU vector types (glibc's `<link.h>`, Redis's
  CRC64); weak declarations and definitions (zstd's trace hooks, jemalloc);
  `transparent_union` (glibc's sockets, libuv); thread-locals shared between
  files (CPython, Redis); `ms_struct` (CPython's ctypes); a record both packed
  and aligned (`<linux/bpf.h>`); `setjmp` and `longjmp` (libpng, libjpeg-turbo,
  xz, Lua); and a dozen kinds of constant initialiser GCC folds — an address
  cast to `uintptr_t`, a constant `?:` between two addresses, a pointer into
  an `extern` array of unknown length.
* **Inline assembly**: an `ebx` clobber, GCC's numbered clobbers, memory
  operands, `%c`, immediates sign-extended from their type, and file-scope
  `asm` (zstd, xz, mbedtls).
* **The command line and the link**: what `configure` and libtool ask (the
  `-print-*` queries, `-Werror` and an unknown warning, `/dev/null` as input),
  libtool's way of passing a version script, an archive linked whole into a
  shared library, objects from another compiler in one, a relative symlink as
  an input, and two links racing in one directory under `make -j`.

And the run-time checks found real undefined behaviour in two of the projects,
without being asked: CPython's `StringIO` computes `buf + pos` after a seek to
`sys.maxsize` (its `test_memoryio` stops there), and git's SHA-1 code reads
unaligned words (note 8). No check fired anywhere else — not in the 47,263
tests of CPython's suite, nor in any other suite on this page.

## Configure and the run-time checks

Some `configure` probes find out what the machine does by doing something C
leaves undefined and seeing what happens. With the run-time checks on, the
probe program stops at the check instead, and `configure` takes that for the
answer it was testing for. CPython's "is aligned memory access required?"
probe dereferences a misaligned pointer; under ccinrs it stops, so `configure`
concludes that alignment is required, and CPython then hashes strings with FNV
instead of SipHash-1-3 — a different `hash()`, and no protection against hash
flooding — with nothing saying so.

There is no way for a compiler to tell a probe from a program. When a project's
`configure` differs between GCC and ccinrs, compare the two `config.log`s; give
the answer GCC's run found through the probe's cache variable — for CPython,
`./configure ac_cv_aligned_required=no` — or run `configure` with
`CFLAGS=-fno-cinrs-checks` and build with the checks on.

## What is left

* **Assembly files.** ccinrs runs no assembler: zstd, libsodium, OpenSSL and
  FFmpeg have switches to leave theirs out, and CPython's `asm_trampoline.S`
  needs a C compiler. Their speed in those configurations is GCC's without the
  assembly; GCC's normal builds are far faster where the assembly is the hot
  code (OpenSSL's AES-GCM by 119 times, libsodium's AEGIS by 56).
* **`__attribute__((alias))`**, which xz's default symbol versioning uses.
* **`long double` is `double`** (note 5; CPython's ctypes has four tests of it).
* **`longjmp` to another stack**, and its cost (note 4).
* **`-fvisibility=hidden` is ignored**, so a shared library exports its
  internal functions too: libcurl 1,061 symbols where GCC's has 100.
* **Build time.** Every C file is a `rustc` run, about a second each even for
  a small file: git's 563 files take nine times as long as with GCC, CPython's
  2.7 times, SQLite's one large file 1.2 times.
* **Size.** Programs are two to twenty times as large, and every shared
  library carries its own copy of Rust's standard library: CPython's 71
  extension modules take 368 MB where GCC's take 26 MB.
* **The checks' cost**: 10–30 % in most code, up to twice in interpreters'
  inner loops, where every array access in a loop no inlining sees through
  keeps its checks.
* **Without the checks**, most of the code on this page runs within 10–20 % of
  GCC's, and some of it faster (libjpeg-turbo's encoder, jq's parser,
  libsodium's BLAKE2b). The exceptions are interpreters — CPython 1.3 times,
  Tcl's bytecode loop 1.5 — and loops that rely on signed overflow being
  undefined (note 7).
