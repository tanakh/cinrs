# Real programs: what runs, and how fast

[`doc/benchmarks.md`](benchmarks.md) measures whole *small* programs — the
Benchmarks Game, Dhrystone, kernels — built three ways and compared byte for
byte. This page is the same question asked of *libraries other people wrote*,
taken as they ship and never edited: does the C build through `cinrs`, does it
give the answers its own tests demand, and how fast is the result against the
same source compiled by `gcc -O2` and `clang -O2`?

The method is the same for every row. The upstream release is fetched at a
pinned version and hash. It is compiled by `cinrs` (a release build, `rustc
-C opt-level=3`), and separately by `gcc -O2 -std=gnu11` and `clang -O2
-std=gnu11` into static libraries, with exactly the per-file flags upstream's
own build uses (`-mavx2` for the file upstream compiles with `-mavx2`, and so
on). One Rust test drives all three through the *same* declarations — the
native builds are reached through a header-only `cinrs` unit and `#pragma
cinrs link` — runs a fixed, seeded workload, and prints a checksum, so the
three builds are shown to have done the same work before their times are
compared. Times are the median of five repetitions, in milliseconds per pass.

Neither `-march=native` nor `-ffast-math` nor link-time optimisation is given to
any side, and every call in the native columns crosses into a static library
while the `cinrs` column is Rust the optimiser can see through; see the
[benchmark page's caveats](benchmarks.md#caveats). A library's own run-time
dispatch (`cpuid`) is left to do what it does in each build, and on this host
every dispatcher picks its AVX-512 path.

For the three programs whose checks are in this repository the whole
measurement is one command — `scripts/check-blake3.sh --bench`,
`scripts/check-xxhash.sh --bench`, `scripts/check-sqlite.sh --bench` — which
builds the native libraries, runs the three ways and prints the table
([`doc/testsuites.md`](testsuites.md#against-gcc-and-clang---bench)). The
programs tried outside the repository are measured the same way from a fixture
under `target/wild/`, which is not committed; what *is* committed for those is
every fix they prompted, each with a test naming the program.

The host for every number below: AMD Ryzen 9 9950X (AVX-512), Linux 6.18 under
WSL2, gcc 15.2.0, clang 21.1.8, rustc 1.98.1 — 1.99.0-beta.6 for the programs
that define a variadic function — measured on 2026-09-24 with nothing else
running.

## SQLite 3.53.4

The amalgamation, 269,649 lines in one file, public domain, `SQLITE_THREADSAFE=1`;
what it exercises is in [`doc/testsuites.md`](testsuites.md#one-real-program-sqlite).
Needs Rust 1.99 for its twenty variadic definitions. An in-memory database,
`PRAGMA journal_mode=MEMORY`, everything through
`sqlite3_prepare_v2`/`bind`/`step`/`reset`:

| section | gcc | clang | cinrs | cinrs/gcc | cinrs/clang |
| --- | ---: | ---: | ---: | ---: | ---: |
| 200,000 inserts in one transaction | 38.9 | 39.0 | 41.2 | 1.06× | 1.06× |
| 200,000 point lookups by primary key | 63.4 | 63.9 | 67.0 | 1.06× | 1.05× |
| 200 range scans of 10,000 rows with `ORDER BY` | 303.8 | 293.3 | 305.9 | 1.01× | 1.04× |
| `GROUP BY` over 200,000 rows | 42.0 | 41.2 | 41.7 | 0.99× | 1.01× |
| 200,000 updates by primary key | 71.7 | 67.5 | 70.4 | 0.98× | 1.04× |

Everything within 6 %. The virtual machine, `sqlite3VdbeExec` — a 7,000-line
`switch` full of `goto`s that becomes a control-flow graph and is relooped —
runs at the speed the C compilers give it.

## BLAKE3 1.8.7

The reference C implementation, about 3,000 lines in seven files: SSE2,
SSE4.1, AVX2 and AVX-512 kernels each compiled under the target its file asks
for, and a `cpuid` dispatcher in inline assembly, which picks AVX-512 here
(`blake3_simd_degree()` is 16) in all three builds.

| section | gcc | clang | cinrs | cinrs/gcc | cinrs/clang |
| --- | ---: | ---: | ---: | ---: | ---: |
| 64 MiB in 1 MiB updates | 8.36 | 7.93 | 7.61 | 0.91× | 0.96× |
| 1,000,000 messages of 64 bytes | 90.7 | 70.6 | 70.6 | 0.78× | 1.00× |
| 100,000 messages of 1 KiB | 108.9 | 100.8 | 102.1 | 0.94× | 1.01× |
| `keyed_hash` over 16 MiB | 1.62 | 1.43 | 1.48 | 0.92× | 1.04× |
| `finalize_seek`, 16 MiB of output | 1.66 | 1.28 | 2.40 | 1.44× | 1.87× |

The hashing sections are at or under the native builds (`cinrs` and clang
agree; gcc is the slow one on the 64-byte messages). The one open item is the
extended-output path: producing 16 MiB through `finalize_seek` costs 1.4–1.9×,
which points at the translation of `blake3_xof_many` and the per-block output
loop rather than at the kernels the other rows share. Not profiled yet.

## xxHash 0.8.4

`xxhash.h` compiled four times (scalar, SSE2, AVX2, AVX-512 — each the way
upstream's own build compiles it) and `xxh_x86dispatch.c`, whose dispatcher
picks AVX-512 here (`XXH_featureTest()` is 3) in all three builds. XXH3 goes
through the dispatcher; XXH32 and XXH64, which it does not cover, come from
the SSE2 unit, as in upstream's default x86-64 build.

| section | gcc | clang | cinrs | cinrs/gcc | cinrs/clang |
| --- | ---: | ---: | ---: | ---: | ---: |
| `XXH3_64bits` over 64 MiB | 1.33 | 1.41 | 1.49 | 1.12× | 1.05× |
| `XXH3_128bits` over 64 MiB | 1.35 | 1.37 | 1.49 | 1.11× | 1.09× |
| `XXH3_64bits`, 1,000,000 keys of 32 bytes | 1.79 | 1.64 | 1.53 | 0.86× | 0.93× |
| `XXH3_64bits`, 1,000,000 keys of 256 bytes | 6.36 | 3.83 | 4.01 | 0.63× | 1.05× |
| `XXH64` over 64 MiB | 2.30 | 2.33 | 2.31 | 1.00× | 0.99× |
| `XXH32` over 64 MiB | 4.39 | 4.36 | 4.36 | 0.99× | 1.00× |
| `XXH3_64bits` streaming, 64 MiB in 4 KiB updates | 1.91 | 1.93 | 2.78 | 1.46× | 1.44× |

The one-shot rows are within 12 % (gcc is the outlier on the 256-byte keys, not
`cinrs`). The open item is the streaming row: the same kernels reached through
`XXH3_64bits_update` in 4 KiB pieces cost 1.45×, so the per-update path — the
dispatcher's `update` and its consume-stripes code — is where the translation
loses, not the kernels. Not profiled yet.

## CRoaring 4.7.2 (not in the repository)

The compressed-bitmap library, as its amalgamated `roaring.c` and `roaring.h`
(21,497 and 10,543 lines; Apache-2.0 OR MIT). What it asks of a compiler: a
`cpuid` dispatcher in inline assembly (`"=b"`), AVX2 and AVX-512 kernels behind
per-function-group `_Pragma(STRINGIFY(GCC target(T)))` regions, `_pext_u64` and
the other BMI2 intrinsics, `_Static_assert`, flexible array members, `_Atomic`
reference counts, `restrict`, copy-on-write, and one variadic definition
(`roaring_bitmap_of`), which is why it needs Rust 1.99.

**Correctness.** A differential test against a `BTreeSet<u32>` reference:
4,000 seeded operations (`add`, `add_many`, `add_range`, `remove_range`,
`flip`, `and`/`or`/`xor`/`andnot` in the new-bitmap, in-place and cardinality
forms, `run_optimize`, copies as copy-on-write snapshots) in four
configurations, with `internal_validate`, cardinality, `contains`,
`to_uint32_array`, `rank`, `select`, both iterators and a portable
serialise/deserialise round trip checked along the way. No mismatch, in
release, in debug (where a misaligned pointer or an overflow panics) or in the
scalar build; and the same test passes against the `gcc` library through
`cinrs`'s declarations, so the struct layouts agree. `croaring_hardware_support()`
reports AVX2 and AVX-512 on this host and the binary carries the AVX-512
kernels.

**Speed.** 200 bitmaps of about 50,000 values each in three shapes (dense
ranges, sparse randoms, mixed), total cardinality 9,993,809:

| section | gcc | clang | cinrs | cinrs/gcc | cinrs/clang |
| --- | ---: | ---: | ---: | ---: | ---: |
| construct + `run_optimize` | 239.1 | 225.3 | 237.0 | 0.99× | 1.05× |
| 2,000 `and`/`or`/`xor`/`andnot` | 156.4 | 160.4 | 151.8 | 0.97× | 0.95× |
| 2,000 `*_cardinality` | 47.5 | 43.9 | 43.1 | 0.91× | 0.98× |
| 5,000,000 `contains` | 285.5 | 322.9 | 293.8 | 1.03× | 0.91× |
| `roaring_iterate` over all | 15.0 | 15.8 | 15.2 | 1.01× | 0.96× |
| serialise + `deserialize_safe` | 7.1 | 7.5 | 6.3 | 0.90× | 0.84× |

Everything is within the run-to-run noise (the `contains` section, bound by
cache misses over a 40 MB probe array, moves by 20 % between two runs of the
same binary). Compiling the whole unit for AVX-512 instead of honouring the
per-function regions changes nothing.

**What it took.** One fix: `_Pragma`'s operand was not macro-expanded, which
refused the `STRINGIFY` idiom (64 sites, four errors each). The fixture is not
in this repository; the fix and its `tests/ui/pragma_operator.rs` are.

## lz4 1.10.0 (not in the repository)

`lib/lz4.c`, `lz4hc.c`, `lz4frame.c` and lz4's own `xxhash.c` (BSD-2-Clause),
included into one unit in that order — no `static` name collides, and
`lz4hc.c`'s `LZ4_SRC_INCLUDED` guard does the rest — and compiled unedited on
the first try, with no diagnostic and no warning.

**Correctness.** Round trips of the block API (`LZ4_compress_default`,
`LZ4_compress_HC` at levels 1, 9 and 12, `LZ4_decompress_safe` and
`_partial`), the frame API (`LZ4F_compressFrame`/`LZ4F_decompress`, with
content checksums) and the streaming ring buffer
(`LZ4_compress_fast_continue`/`LZ4_decompress_safe_continue`) over 21 inputs
(zeros, random, text-like; 0 bytes to 1 MiB), plus truncated inputs and
too-small destinations; every one of the 165 compressed outputs from the
`cinrs` build is byte-identical to the `gcc` build's, in release and in
debug.

**Speed.** 64 MiB of text-like data, ms per pass:

| section | gcc | clang | cinrs | cinrs/gcc | cinrs/clang |
| --- | ---: | ---: | ---: | ---: | ---: |
| block compress, default | 79.1 | 79.4 | 82.3 | 1.04× | 1.04× |
| block decompress, default | 13.6 | 13.0 | 13.2 | 0.97× | 1.02× |
| block compress, HC level 9 | 3309 | 3434 | 3320 | 1.00× | 0.97× |
| block decompress, HC level 9 | 9.9 | 9.6 | 9.9 | 1.00× | 1.03× |
| frame compress | 81.0 | 82.5 | 83.1 | 1.03× | 1.01× |
| frame decompress | 13.2 | 13.3 | 13.7 | 1.04× | 1.03× |

**What it took.** Nothing.

## cJSON 1.7.19 (not in the repository)

`cJSON.c` and `cJSON_Utils.c` (MIT), one unit each under `#pragma cinrs
export`; compiled unedited on the first try, with no diagnostic and no warning.
(Two units that both declare `struct cJSON` give Rust two distinct types, and
glob-importing both makes every `cJSON_*` name ambiguous, so the test names
the Utils functions explicitly — a property of one-unit-per-block rather than
a fault; see [Limitations](limitations.md).)

**Correctness.** 58 documents covering every value kind, `é` and
surrogate pairs, the edge numbers (`-0`, `1e400`, 2⁵³+1, `DBL_MAX`),
999- and 1,001-deep nesting and 17 malformed inputs, through `Print`,
`PrintUnformatted`, `PrintBuffered`, `PrintPreallocated`, `Minify`,
`ParseWithOpts`/`WithLength`, `GetErrorPtr`, the mutation API, JSON pointers,
patches, merge patches and `SortObject`, with every allocation counted through
`cJSON_InitHooks` (4,108 allocations, none live at the end of a section). The
250-line transcript from the `cinrs` build is byte-identical to the `gcc`
build's, every `%1.15g` number and error offset included.

**Speed.** A generated 10 MB document, ms per pass:

| section | gcc | clang | cinrs | cinrs/gcc | cinrs/clang |
| --- | ---: | ---: | ---: | ---: | ---: |
| parse | 58.6 | 57.8 | 57.4 | 0.98× | 0.99× |
| print, unformatted | 54.8 | 54.3 | 55.6 | 1.01× | 1.02× |
| print, formatted | 57.3 | 56.2 | 57.9 | 1.01× | 1.03× |
| print, buffered | 53.1 | 51.3 | 53.6 | 1.01× | 1.04× |
| duplicate + compare + delete | 103.6 | 102.0 | 103.1 | 1.00× | 1.01× |
| minify | 6.8 | 6.6 | 7.3 | 1.07× | 1.11× |

**What it took.** Nothing.

## libdeflate 1.26 (not in the repository)

The DEFLATE/zlib/gzip library (MIT), `lib/*.c` and `lib/x86/*` as shipped:
CRC32 in PCLMUL, VPCLMULQDQ and AVX-512 forms, Adler-32 in AVX2, AVX-512 and
AVX-VNNI forms, a BMI2 decompressor, every one behind a per-function
`__attribute__((target("…")))`, and a `cpuid`/`xgetbv` dispatcher in inline
assembly. Upstream compiles every file with plain `-O2`, and so did the native
builds here. Two units (`deflate_compress.c` and `deflate_decompress.c` each
define a `BITBUF_NBITS` of their own, so they cannot share one).

**What it took.** Two things, neither a bug in the translation of what runs:

* `common_defs.h` was `#error "gcc versions older than 4.9 are no longer
  supported"` while `cinrs` said `__GNUC__` was 4.2, as clang does, and the
  same gate switched off the VPCLMULQDQ CRC32 (gcc ≥ 10.1) and the AVX-VNNI
  Adler-32 (≥ 12.1). This program is why `cinrs` now presents itself as GCC
  14.2: the two units compile unedited, with no workaround, and pass the
  round-trip test. (The fixture first took clang's way out — defining
  `__clang__` before the include — and still has it as its default.)
* The AVX2 and AVX-512 Adler-32 templates keep their accumulators alive with
  gcc's empty barrier, `__asm__("" : "+x"(v))` on an `__m256i` and
  `"+v"` on an `__m512i`; `cinrs`'s inline assembly took `"x"` only at 128
  bits and did not know `"v"`. Those are gaps in the asm subset (`asm!` has
  `ymm_reg` and `zmm_reg`), on the list.

**Correctness.** 189 streams — zeros, random and text-like inputs from 0 bytes
to 4 MiB, levels 1, 6 and 12, raw, zlib and gzip — round-trip in release and
in debug, and **every compressed stream from the `cinrs` build is
byte-identical to the `gcc` build's**; each build decompresses the other two's
streams; the platform's zlib 1.3.1, reached through a `system_include` unit,
accepts all 126 zlib and gzip streams; `crc32` and `adler32` match bit-by-bit
references over 1,806 short cases, 1 MiB of random bytes, 1 MiB of `0xFF` and
chained uneven pieces. The dispatcher reports the same feature word in all
three builds, so the runs used the VPCLMULQDQ/AVX-512 CRC32, the AVX-512 VNNI
Adler-32 and the BMI2 decompressor.

**Speed.** ms per pass:

| section | gcc | clang | cinrs | cinrs/gcc | cinrs/clang |
| --- | ---: | ---: | ---: | ---: | ---: |
| compress level 1, 64 MiB of text | 144.1 | 154.4 | 160.6 | 1.11× | 1.04× |
| decompress the level-1 stream | 36.8 | 41.8 | 40.4 | 1.10× | 0.97× |
| compress level 6 | 573.7 | 623.5 | 652.5 | 1.14× | 1.05× |
| decompress the level-6 stream | 33.1 | 34.9 | 33.5 | 1.01× | 0.96× |
| `crc32` over 256 MiB | 7.4 | 7.6 | 6.4 | 0.86× | 0.84× |
| `adler32` over 256 MiB | 7.3 | 8.0 | 6.9 | 0.95× | 0.86× |

Within 5 % of clang throughout; the compressor is 11–14 % behind gcc, which
is ahead of clang on it too. The checksums run at memory bandwidth in all
three.

## stb_image 2.30 and stb_image_write 1.16 (not in the repository)

The two single-header libraries (public domain), one unit with both
`*_IMPLEMENTATION` macros: the SSE2 JPEG IDCT and YCbCr paths (live, because
`__SSE2__` is predefined), `aligned(16)` tables, `_Thread_local` state, and a
variadic definition (`stbiw__writef`), which is why it needs Rust 1.99.
Compiled unedited on the first try, with no diagnostic and no warning.

**Correctness.** 50 generated images (gradients, noise, an alpha
checkerboard, 1×1 and odd sizes, 1–4 channels, 8- and 16-bit) written to
memory as PNG (adaptive and each filter 0–4), BMP, TGA and TGA+RLE, JPEG at
quality 50/90/95 and HDR, and read back through `stbi_load_from_memory`,
`_load_16_` and `_loadf_`: the lossless formats exact, JPEG within the
expected error and **byte-identical to the `gcc` build's decode** (the SSE2
IDCT), HDR exact; `stbi_info`, the vertical flip and six failure reasons for
truncated and bogus files; and 728 hand-made PNGs covering every colour type
and bit depth, `tRNS`, every filter fixed and mixed per row, and Adam7
interlacing, each decoding to exactly the expected pixels. Of the 3,958
encoded files and decoded buffers, `cinrs`'s are byte-identical to clang's
throughout, and to gcc's in all but nine.

Those nine are **gcc's**: `gcc -O2` decodes an HDR image narrower than eight
pixels — stored flat, without run-length encoding — with row 0 right and the
rows after it left uninitialised (valgrind confirms the read). `gcc -O0`,
`-O1`, `-O2 -fno-thread-jumps`, clang at every level and `cinrs` all decode
it correctly, UBSan reports nothing, and the C has no undefined behaviour
anyone could find; the loop has a label the RLE branch jumps into. A likely
compiler bug, reproducible with six pixels through `stbi_write_hdr_to_func`
and `stbi_loadf_from_memory`; not `cinrs`'s.

**Speed.** A 4096×4096 RGB image (42.7 MB as PNG, 2.19 MB as JPEG q90), ms
per pass:

| section | gcc | clang | cinrs | cinrs/gcc | cinrs/clang |
| --- | ---: | ---: | ---: | ---: | ---: |
| decode PNG | 281.7 | 263.1 | 267.0 | 0.95× | 1.01× |
| decode JPEG | 113.7 | 109.1 | 109.2 | 0.96× | 1.00× |
| encode PNG | 1901.7 | 1512.4 | 1553.0 | 0.82× | 1.03× |
| encode JPEG q90 | 200.3 | 144.1 | 162.7 | 0.81× | 1.13× |

**What it took.** Nothing.

## chibicc (not in the repository)

Rui Ueyama's small C11 compiler (MIT; the `main` branch's head, 2020-12-07),
about 10,000 lines in nine files, one unit each under `#pragma cinrs export`
and `system_include first`: `noreturn`, variadic `error(...)` definitions
with `va_list`, `format` attributes, `open_memstream`, `fork`/`execvp`/`wait`,
`mkstemp`, `glob`, huge `switch`es, `goto`, unions and static tables. A
program rather than a library, so the fixture is a binary crate with
`#![no_main]`, the exported C `main` being the entry point (rustc otherwise
says the entry symbol is declared twice; see [Pragmas](pragmas.md#export)).
Compiled unedited with no diagnostic and no warning; needs Rust 1.99.

**Correctness.** chibicc's own test suite, run exactly as its Makefile runs
it (each `test/*.c` compiled by the chibicc under test, linked with gcc,
executed; then `test/driver.sh`), for a gcc-built chibicc and the
`cinrs`-built one; then **stage 2**: the `cinrs`-built chibicc compiles
chibicc's own nine sources, and that compiler is put through the suite too.

| compiler | `test/*.c` | `driver.sh` |
| --- | ---: | --- |
| chibicc built by gcc | 41/41 | ok |
| chibicc built by `cinrs`, as first found | 33/41 | ok |
| chibicc built by `cinrs`, after the fix below | 41/41 | ok |
| stage 2: chibicc compiled by the `cinrs`-built chibicc | 41/41 | ok |

The assembly the `cinrs`-built and gcc-built compilers emit for chibicc's
own sources is byte-identical.

**What it took.** One silent wrong result, of the kind this campaign is for.
`cinrs` maps `long double` to `double`, a documented limitation that is
self-consistent inside a unit — but chibicc's tokenizer calls the
platform's `strtold`, which glibc returns on the x87 stack, and the call was
bound with the `double` convention, so every floating literal the compiler
read was garbage (and `printf("%Lf", 2.5L)` printed `-nan`). The boundary
is now handled instead of trusted: a declared-only ISO C function whose only
difference from a `double` sibling is the type (`strtold`, `sinl`, `powl`, …)
is linked to the sibling, which is what "`long double` is `double`" means
here; any other external function with a `long double` in its signature, and
a `long double` handed to an external variadic function, is refused by name
with the rewrite. One thing stays as it is, by nature: chibicc itself
assumes the host's `long double` is x87 when it builds an 80-bit image
through a `union`, so a program it compiles that *uses* `long double` gets
`double`'s bits — the same documented limitation, one level down.

**Speed.** Compiling chibicc's preprocessed `parse.c` (4,332 lines) to
assembly, and all 41 test files, ms:

| compiler build | `parse.c` to `.s` | `cc1` only | 41 tests to `.s` |
| --- | ---: | ---: | ---: |
| gcc -O2 | 16.9 | 16.6 | 109 |
| clang -O2 | 17.4 | 16.5 | 112 |
| cinrs | 15.0 | 14.4 | 110 |

All four compilers (upstream's `-O0 -g` build included) emit byte-identical
assembly for the inputs; the test loop is process start-up.

## cmark 0.31.2 (not in the repository)

The CommonMark reference implementation (BSD-2-Clause), nineteen `src/*.c`
files one unit each under `#pragma cinrs export` (the renderers share
`static` names), with the two headers CMake would generate written by hand:
a 9,400-line re2c-generated scanner of `goto` state machines, the
`case_fold_switch.inc` and `entities.inc` tables, the five renderers.
Compiled unedited on the first try in 5.7 s, no diagnostic, no warning.

**Correctness.** The specification's 652 examples with the options
upstream's own test runner passes, 652/652, and every example also rendered
to XML, man, LaTeX and CommonMark; `smart_punct.txt` 16/16 and
`regression.txt` 23/23; the whole `spec.txt` through all five renderers with
and without source positions and smart punctuation; and the 24 pathological
inputs (deep nesting, thousands of backticks, the reference-collision
document built against its own hash), each under 200 ms. Every one of those
outputs is byte-identical to the `gcc` build's, in release and in the debug
build with overflow checks on.

**Speed.** ms per pass:

| section | gcc | clang | cinrs | cinrs/gcc | cinrs/clang |
| --- | ---: | ---: | ---: | ---: | ---: |
| `spec.txt` (200 KB) to HTML | 1.56 | 1.45 | 1.38 | 0.88× | 0.95× |
| the same with smart punctuation and source positions | 1.70 | 1.56 | 1.49 | 0.88× | 0.96× |
| `spec.txt` to XML | 1.55 | 1.40 | 1.28 | 0.83× | 0.91× |
| `spec.txt` to man | 2.70 | 2.50 | 2.47 | 0.91× | 0.99× |
| `spec.txt` to LaTeX | 2.53 | 2.38 | 2.39 | 0.94× | 1.00× |
| `spec.txt` to CommonMark | 2.60 | 2.41 | 2.39 | 0.92× | 0.99× |
| a synthetic 5 MB document to HTML | 145.2 | 141.0 | 138.0 | 0.95× | 0.98× |

Ahead of both native builds, most likely because the nineteen units are one
Rust crate the optimiser inlines across, where the native libraries get no
link-time optimisation.

**What it took.** Nothing.

## brotli 1.2.0 (not in the repository)

Google's compressor (MIT): `c/common`, `c/dec` and `c/enc`, thirty-five
units — the 120 KB dictionary table, the Huffman tables, `hash_*_inc.h`
included many times under different macros to stamp out the hashers, the
decoder's state machine, `platform.h`'s compiler sniffing. Its `platform.h`
includes `<sys/types.h>`, so the units need `#pragma cinrs system_include`;
with it, compiled unedited with no diagnostic and no warning in 8.4 s, and
`platform.h` takes exactly the paths `gcc -E` takes for a GCC 14 (`restrict`,
`always_inline`, the `__builtin_` forms, `memcpy` unaligned loads, x86-64).

**Correctness.** 190 inputs (zeros, random, text-like; 0 bytes to 4 MiB) at
qualities 0, 5, 9 and 11 with windows 16 and 22, one-shot and streaming in
small pieces both ways: all 358 compressed outputs byte-identical to the
`gcc` build's, and every `gcc` stream decodes in the `cinrs` build; the
tarball's test data decodes to the same bytes.

**Speed.** 16 MiB of text-like data, window 22, ms per pass:

| section | gcc | clang | cinrs | cinrs/gcc | cinrs/clang |
| --- | ---: | ---: | ---: | ---: | ---: |
| compress, quality 5 | 217.2 | 202.5 | 213.5 | 0.98× | 1.05× |
| decompress the quality-5 stream | 16.4 | 17.0 | 17.9 | 1.09× | 1.05× |
| compress, quality 9 | 569.7 | 633.5 | 584.2 | 1.03× | 0.92× |
| decompress the quality-9 stream | 12.0 | 12.2 | 13.3 | 1.11× | 1.09× |

**What it took.** Nothing that changed a byte of output; it did show one
conformance slip in the preprocessor — stringifying a macro argument kept
the space before the argument (`"((14) * 1000000) + (( 2) * 1000)"` where
GCC prints `((2) * 1000)`), which C11 6.10.3.2 says to drop — fixed with a
test.

## Kissat 4.0.4 (not in the repository)

Armin Biere's SAT solver (MIT): 38,944 lines of C99 in 93 `src/*.c` files,
one unit each under `#pragma cinrs export` and `system_include first`,
with the `NDEBUG` its `configure` chooses and a hand-written `build.h`.
Macros that generate whole families of functions (`stack.h`, `vector.h`,
`heap.h`), `__builtin_clz`/`prefetch`, `popen`, `getrusage`, `sysconf`, and
variadic message functions, which is why it needs Rust 1.99.

**What it took.** Two refusals of valid C, both fixed with ui tests
(`tests/ui/incomplete_prototypes.rs`, `tests/ui/typedef_shadowed_by_declarator.rs`):
a prototype whose return type is a never-completed `struct` (`changes
kissat_changes(struct kissat *)`, 80 sites) was refused where C requires
completeness only at a definition or a call; and `links *links =
solver->links, *l = links + idx;` — the typedef name reused as the first
declarator's own name, the shape its `all_stack(watch, watch, …)` macro
produces too, 44 sites — lost the second declarator's type, because the
specifiers' typedef was looked up again after the declarator had hidden it.
With those, all 93 files compile unedited with no diagnostic.

**Correctness.** 22 generated CNFs (random 3-SAT at ratio 4.26 with 100 to
350 variables, pigeonhole 7 and 8, 16- and 30-queens, planted and random
3-XOR systems): the `cinrs` build gives the same exit codes as the gcc and
clang builds (11 satisfiable, 11 not), the same models to the hash, and
every model satisfies every clause under an independent checker. Kissat is
deterministic for a seed, and the search statistics match to the last
conflict: 433,976 conflicts, 656,686 decisions, 20,305,786 propagations on
one instance in all three builds. Its own test program `tissat`, built from
`test/*.c` the same way, passes 1,004 of its 1,017 jobs; the 13 it cannot
run are the allocation-failure tests, which recover through
`setjmp`/`longjmp`, the documented limitation.

**Speed.** `gcc -O3` and `clang -O3` (upstream's flags), ms:

| workload | gcc | clang | cinrs | cinrs/gcc | cinrs/clang |
| --- | ---: | ---: | ---: | ---: | ---: |
| random 3-SAT, 200 variables (unsat) | 455 | 437 | 445 | 0.98× | 1.02× |
| random 3-SAT, 300 variables (sat) | 786 | 759 | 784 | 1.00× | 1.03× |
| random 3-SAT, 250 variables (unsat) | 3,798 | 3,655 | 3,784 | 1.00× | 1.04× |
| `tissat`, the whole suite | 7,675 | 7,714 | 7,738 | 1.01× | 1.00× |

## Wren 0.4.0 (not in the repository)

The scripting language's VM, `src/vm/*.c` and the two optional modules (MIT;
about 12,000 lines in one unit, the nine files' statics never colliding):
a bytecode interpreter loop written with **computed `goto`** through a
272-entry `static void *dispatchTable[]` of `&&label`s, NaN-boxed values
punned through unions, a mark-sweep collector, a single-pass compiler driven
by a table of function pointers, and three variadic definitions, which is
why it needs Rust 1.99. Upstream's `test/api/*.c` — seventeen files of
foreign-function bindings — are translated too, so only the VM changes
between the builds under test.

**Correctness.** Wren's own suite, every `test/**/*.wren` with its
`// expect:` lines, run by a Rust port of upstream's `test/main.c` and
`util/test.py`:

| VM | scripts | passed |
| --- | ---: | ---: |
| built by gcc | 857 | 857 |
| built by clang | 857 | 857 |
| built by `cinrs`, computed `goto` | 857 | 857 |
| built by `cinrs`, `switch` dispatch (`WREN_COMPUTED_GOTO=0`) | 857 | 857 |

The pass lists are identical, so there is nothing to minimise.

**What it took.** Two small things and one large one. The bundled `<math.h>`
had none of C99's classification macros — `isnan`, `isinf`, `isfinite`,
`signbit`, `fpclassify` and the comparison macros — although the builtins
behind them were there; they are defined now. Under the platform's
`<math.h>` instead, glibc (which then saw `__GNUC__` 4.2) expanded `isnan(x)` to
`sizeof (x) == sizeof (double) ? __isnan (x) : __isnanl (x)`, and the
[`long double` boundary check](#chibicc-not-in-the-repository) refused the
`__isnanl` call in the arm a `double` can never take; a use in an arm a
constant condition excludes is not reported any more. And the large one is
speed, below.

**Speed.** Upstream's `test/benchmark/*.wren`, one whole run of the VM per
script, ms, the median of five runs interleaved across the VMs (identical
output from all four):

| script | gcc | clang | cinrs, computed `goto` | cinrs, `switch` |
| --- | ---: | ---: | ---: | ---: |
| api_call | 27.5 | 25.4 | 25.5 | 27.5 |
| api_foreign_method | 146.4 | 150.7 | 150.6 | 144.1 |
| binary_trees | 120.9 | 121.2 | 127.7 | 125.4 |
| binary_trees_gc | 346.1 | 294.5 | 307.4 | 319.6 |
| delta_blue | 58.8 | 64.9 | 75.4 | 71.2 |
| fib | 121.5 | 123.1 | 119.3 | 113.1 |
| fibers | 33.8 | 33.7 | 33.7 | 33.8 |
| for | 37.9 | 35.9 | 40.0 | 39.9 |
| map_numeric | 768.7 | 746.4 | 750.7 | 746.9 |
| map_string | 88.0 | 94.2 | 92.3 | 92.2 |
| method_call | 58.8 | 52.6 | 73.2 | 67.1 |
| string_equals | 77.7 | 84.0 | 81.7 | 81.6 |

Both `cinrs` builds run at the C compilers' speed: computed `goto` —
upstream's default — at 0.89–1.28× of gcc, and 0.93–1.09× of the `switch`
build. It did not start out that way. A function that took a label's address
used to be lowered to a whole-function state machine, where every opcode went
through the central `match` twice: 1.3–5.6× of gcc, 8.6 G instructions against
gcc's 2.1 G on `fib`, with no more branch misses. It now gets GCC's own
lowering — `goto *p` is a `switch` over the labels whose address is taken,
after which the function has only ordinary jumps for the relooper — and, since
`dispatchTable` is only ever read, its `goto *dispatchTable[op]` is a `match`
on `op` itself, [the table fold](translation.md#labels-as-values): 2.37 G
instructions on `fib`, against 2.34 G for the `switch` build and 2.08 G for
gcc's.
