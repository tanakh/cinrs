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

* `common_defs.h` is `#error "gcc versions older than 4.9 are no longer
  supported"`, because `cinrs` says `__GNUC__` is 4.2, as clang does; and the
  same version gate switches off the VPCLMULQDQ CRC32 (gcc ≥ 10.1) and the
  AVX-VNNI Adler-32 (≥ 12.1). The fixture took clang's way out — defining
  `__clang__` and its version before the include, which passes the gate and
  selects exactly the paths the native clang build selects. What `cinrs`
  should claim to be is an open question this program sharpened.
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
