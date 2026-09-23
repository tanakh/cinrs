# Changelog

All notable changes to `cinrs` and its companion crates — `cinrs-core`,
`cinrs-macros` and `cinrs-rt`, which are versioned together with it — are
recorded here. The format follows [Keep a Changelog][kac], and the project
follows [Semantic Versioning][semver].

[kac]: https://keepachangelog.com/en/1.1.0/
[semver]: https://semver.org/spec/v2.0.0.html

## Unreleased

### Added

* **TS 18661-3's `_FloatN` types, as keywords.** glibc uses them as the
  compiler's own from GCC 7 on, and declares `strtof32`, `sinf64`,
  `csqrtf32x` … under `_GNU_SOURCE`. `_Float32` is `float`, `_Float64` and
  `_Float32x` are `double`, and `_Float64x` is `long double` — `double`
  here, across the platform boundary as `long double` is: `strtof64x` and the
  `f64x` twins of the ISO `l` functions link to their `double` siblings, and
  another declared-only function taking one is refused at its call. Each takes
  `_Complex`. `_Float128` and `__float128` can be *named* — prototypes,
  `typedef`s, pointers, `sizeof`, `_Generic` — but no value of them is
  accepted: an object, a cast and a call of a function whose prototype
  mentions one are refused with the reason. A `_Generic` that lists both
  `float` and `_Float32` (glibc's `__MATH_TG`) keeps the first. The
  builtins glibc's `HUGE_VAL_F32`, `SNANF64` and their relatives expand to
  (`__builtin_huge_valf32`, `__builtin_inff64x`, `__builtin_nansf32x`, …) are
  there for the four types that have one.

* **The x86 SIMD intrinsics.** `#include <immintrin.h>` and write Intel's
  intrinsics the way real C does: `__m128`, `__m128i`, `__m128d`, `__m256`,
  `__m256i` and `__m256d` are types, and 881 functions — SSE, SSE2, SSE3, SSSE3,
  SSE4.1, SSE4.2, AVX, AVX2, FMA, AES, PCLMUL, SHA, and the BMI1, BMI2, POPCNT
  and LZCNT scalar ones — are declared. There is no vector language and no new
  syntax: **the mapping is by name.** A call to `_mm_add_ps(a, b)` becomes
  `::core::arch::x86_64::_mm_add_ps(a, b)`, which works because `core::arch` was
  generated from the same Intel data Intel's own headers are. The bundled
  headers' prototypes were *read out of `core::arch`'s source* by a maintainer
  tool that writes both the header and the table driving the mapping
  (`crates/cinrs-core/tests/x86_intrinsics.rs`), so a declaration and the
  function it resolves to cannot drift apart — and a test that needs no
  toolchain checks the two committed files against each other on every run.
  `<xmmintrin.h>`, `<emmintrin.h>`, `<pmmintrin.h>`, `<tmmintrin.h>`,
  `<smmintrin.h>`, `<nmmintrin.h>`, `<wmmintrin.h>`, `<avxintrin.h>`,
  `<avx2intrin.h>`, `<x86intrin.h>` and `<popcntintrin.h>` are bundled too, in
  the layering GCC and Clang give them, so code written for an older compiler
  finds its instruction set under the header name it expects. The macros come
  with them: `_MM_SHUFFLE`, `_MM_SHUFFLE2`, `_MM_TRANSPOSE4_PS`, `_MM_HINT_*`,
  `_MM_FROUND_*`, `_CMP_*`, `_SIDD_*` and the rest, with the values GCC's
  headers give them.

  The vector types are ordinary objects of a known size and alignment —
  sixteen or thirty-two bytes, aligned to themselves — so they work as locals,
  parameters, return values, `static`s, array elements, `struct` members and the
  members of the `union { __m128i v; int32_t i[4]; }` that most code reaches a
  lane through. An **immediate operand** is a `const` generic in `core::arch`,
  so the argument is folded and written into a turbofish
  (`_mm_slli_epi32::<{ 3i32 }>(v)`); a non-constant is a diagnostic naming the
  intrinsic, which is what Intel's own compilers say too. Taking the **address**
  of an intrinsic works — GCC's are `static inline` functions, so real code does
  — through a private `extern "C"` shim per intrinsic; an intrinsic with an
  immediate operand has no address, and that is a diagnostic. **Nothing is
  exported**: those prototypes add not one item to the unit's `extern` block
  or to its glob re-export, so a `use core::arch::x86_64::*` in the surrounding
  Rust cannot clash with them.

  See [SIMD intrinsics](doc/features.md#simd-intrinsics) for the coverage per
  instruction set and the intrinsics whose `core::arch` signature C cannot
  spell; `tests/simd.rs` runs every family, with the expected values
  computed in scalar C in the same block and cross-checked against `gcc -O2`.
  What stays refused, now with a diagnostic that names the intrinsic to write
  instead: the GNU vector extensions,
  `__attribute__((vector_size))` and `__builtin_ia32_*`. What is not here:
  MMX and `__m64`, which `core::arch` dropped — `<mmintrin.h>` is an `#error`
  that names the SSE2 form of each intrinsic.

* **AVX-512, and what arrived with it.** The same mapping now covers
  AVX-512F, BW, CD, DQ and VL, the VBMI, VBMI2, VNNI, BITALG, VPOPCNTDQ, IFMA,
  BF16 and FP16 extensions, GFNI, VAES and VPCLMULQDQ at every width, AVX-VNNI
  with its INT8 and INT16 forms, AVX-IFMA, F16C, SHA-512, SM3 and SM4: 6,075
  intrinsics in all, 5,194 more than the SSE-to-AVX2 set above. The new types
  are `__m512`, `__m512i` and `__m512d` (sixty-four bytes, aligned to
  sixty-four), the bfloat16 vectors `__m128bh`/`__m256bh`/`__m512bh` and the
  half-precision `__m128h`/`__m256h`/`__m512h`; the masks `__mmask8` to
  `__mmask64` are plain unsigned integers, as in GCC, and the `_MM_*_ENUM`
  immediates are `int`, with `_MM_CMPINT_*`, `_MM_MANT_*` and `_MM_PERM_*` —
  in GCC's lower-case spellings (`_MM_MANT_SIGN_src`) as well as `core::arch`'s.
  `<immintrin.h>` pulls in thirty-five new headers under GCC's names, from
  `<avx512fintrin.h>` and `<avx512vlintrin.h>` to `<sm4intrin.h>`, and each one
  included on its own works too. A function that passes or returns a 512-bit
  vector by value needs `__attribute__((target("avx512f")))`, as a 256-bit one
  needs `avx`; rustc refuses it otherwise (`tests/ui/simd_abi_512.rs`). `target`
  and `__builtin_cpu_supports` know every one of the new names — the fourteen
  `avx512*`, `gfni`, `vaes`, `vpclmulqdq`, `avxvnni`, `avxvnniint8`,
  `avxvnniint16`, `avxifma`, `avxneconvert`, `sha512`, `sm3`, `sm4`, `kl` and
  `widekl`, spelled as GCC 15 spells them — and a feature list such as
  `gfni,avx512bw,avx512vl` is read out as the instruction sets it names in a
  diagnostic. A memory operand takes any object pointer: a pointer parameter
  is `void *` or `const void *` exactly where GCC 15.2's headers declare it so
  — 335 of them, `_mm_prefetch` and the SSE2 `_mm_loadu_si16` family included
  — and the generated call casts every pointer argument to `core::arch`'s
  type, so `_mm512_loadu_si512(p)` takes an `int *` without a cast, as in GCC.
  Still out, because `core::arch` keeps them unstable: AVX512-VP2INTERSECT,
  and the fifty FP16 and BF16 intrinsics that take or return a scalar `f16` or
  `bf16`.

* **GCC's vector operators, subscripts and brace initialisers on the Intel
  vector types.** In GCC `__m128d` is a vector of two doubles, and real code
  writes `a * b + c`, `v * 2.0`, `1.0 / v`, `-v`, `v += w`, `v[1]` and
  `(__m128d){x, y}` on it. Each is now lowered to the intrinsic that does the
  same, a call of the function the header declares, so the `target`,
  pointer and `[[cinrs::safe]]` rules apply unchanged: `+ - * /` and `& | ^`
  on the float and double vectors at every width, `+ -` and the bitwise
  operators on the integer vectors as GCC's 64-bit lanes, unary `-` as a
  sign-bit flip, the comparisons on the 128- and 256-bit float and double
  vectors typed as the same-size integer vector, a scalar on either side
  converted to the lane type and broadcast, and the compound assignments.
  `v[i]` is a lane of the lane type — an lvalue when the vector is an object
  — with a constant index out of range an error; `{a, b}` lists the lanes in
  memory order (`_mm_setr_pd`), the missing ones zero. Refused, each naming
  the intrinsic to write: integer `*`, `/`, `%`, shifts and comparisons, the
  512-bit comparisons, every operator on the bfloat16 and half-precision
  vectors, designators in a vector's braces, `~` on a float vector, the
  reversed `i[v]`, and lanes for a vector with static storage duration, which
  would be a call in a Rust `static` (the message says to assign them in a
  function). `__attribute__((vector_size))` on a type of the program's own
  stays refused.
* **`<mm_malloc.h>`**, bundled: `_mm_malloc` and `_mm_free` as `static inline`
  functions in portable C over `malloc`, with GCC's alignment rules (1, 2 and
  4 mean a pointer's; zero or a non-power of two is a null pointer), since
  GCC's calls `posix_memalign`, which the Microsoft runtime lacks.
  `<xmmintrin.h>` includes it as GCC's and Clang's do, and with it
  `<stdlib.h>` — programs call `exit` and `atoi` with only the intrinsics
  header included — so every unit that includes an intrinsics header carries
  `<stdlib.h>`'s declarations and the two functions.
* **An aligned variable length array.** `double v[n]
  __attribute__((aligned(32)))` or `_Alignas(32)` on a variable length array
  used to be refused; the heap storage is now over-allocated and the array
  starts at the first multiple of the alignment, which is what an aligned AVX
  load of it needs. An `_Alignas` weaker than the element type's is still the
  error it is on any object.
* **A byte order mark** opening a file — the unit's own text or an
  `#include`d header — is skipped, as GCC and Clang skip it, with every
  column still counted from the start of the file. One anywhere else is still
  a stray character.
* **The Benchmarks Game's SIMD programs in the benchmark suite.**
  `fannkuch-redux-ssse3`, `n-body-sse`, `n-body-avx`, `mandelbrot-sse2`,
  `spectral-norm-sse2`, `spectral-norm-sse41`, `spectral-norm-avx` and
  `spectral-norm-avx2` are vendored under
  `benches/cinrs-bench/programs/benchmarksgame/`, and all eight print, byte
  for byte, what the `gcc -O2` and `clang -O2` builds print; the timings are
  in [doc/benchmarks.md](doc/benchmarks.md). A program's row can now name the
  instruction sets it is written for, which become `-mNAME` for the native
  compilers and `-C target-feature=+NAME` for the `cinrs` build.

* **`__attribute__((target("avx2")))` and `#pragma GCC target`.** GCC's way of
  telling one function which instruction sets it may use becomes
  `#[target_feature(enable = "avx2")]` on the generated item, with GCC's name
  for an instruction set translated to LLVM's (`bmi` is `bmi1`, `pclmul` is
  `pclmulqdq`, `cx16` is `cmpxchg16b`, `sse4` is both halves of SSE4, `abm` is
  LZCNT and POPCNT together). Several names in one string, several attributes on
  one declaration, and successive pragmas all accumulate, as GCC's do; `#pragma
  GCC push_options` and `pop_options` bracket a region and an attribute on a
  function wins over the pragma. A name rustc has not stabilised, a processor
  (`target("arch=haswell")`) and turning an instruction set *off*
  (`target("no-avx")`) are each refused with the reason, because an ignored
  `target` would be a function compiled for the baseline and a program that
  faults on the first instruction it has not got. On a `[[cinrs::safe]]`
  function it is refused too: `#[target_feature]` makes a function unsafe to
  *call*, which is the opposite of what safe promises.

* **`__builtin_cpu_supports` and `__builtin_cpu_init`.** Since a procedural
  macro cannot see rustc's `-C target-feature`, only the architecture baseline is
  predefined — `__SSE__`, `__SSE2__`, `__SSE_MATH__` and `__SSE2_MATH__` on
  x86-64, and nothing above them — so `#ifdef __AVX2__` takes the other branch
  and the run-time question is the one to ask.
  `__builtin_cpu_supports("avx2")` is `std::is_x86_feature_detected!("avx2")`,
  reading the same `cpuid` leaves GCC's own builtin does and answering as an
  `int`; it takes GCC's spellings. `__builtin_cpu_init()` is accepted and does
  nothing, Rust's detection being lazy. Both need `std`, so
  `__builtin_cpu_supports` under `#pragma cinrs no_std` is a located error.
  `__builtin_cpu_is` is deliberately absent — it names a microarchitecture,
  which `std_detect` cannot answer — and `__has_builtin` says so.
* **Inline assembly.** GCC's extended and basic `asm` — `asm`, `__asm`,
  `__asm__`, with `volatile` and `inline` — becomes `::core::arch::asm!` on x86
  and x86-64, with the template handed over as AT&T and
  `options(att_syntax)`. The operands are mapped rather than guessed at: every
  one is named (`o0`, `o1`, …), `"r"`, `"q"` and `"x"` are register classes
  (`reg_byte` for an 8-bit value), `"a"` … `"D"` are that register at the
  operand's width, `"i"` and `"n"` are a folded `const`, a tied `"0"` becomes
  `inout … =>`, `=`/`=&`/`+` are `lateout`/`out`/`inout`, the `k`, `w`, `b`,
  `h` and `q` modifiers are `asm!`'s `e`, `x`, `l`, `h` and `r`, a bare `%0`
  keeps the operand's width, and register clobbers are `out("…") _` while
  `"memory"` and `"cc"` are what `asm!` assumes anyway. An output goes straight
  into its place, whose side effects happen once. What `asm!` cannot say is
  refused by name, with the rewrite: memory operands (pass `"r"(&x)` and write
  `(%0)`), an `rbx` clobber (rustc keeps rbx for LLVM), `%=`, `asm goto`, flag
  outputs, `"A"`, the x87 and MMX constraints, the range-checked immediates,
  Intel syntax, and `asm` in a `[[cinrs::safe]]` function or for another
  architecture. The `"b"` constraint itself — `"=b"(out[1])` in every
  hand-written `cpuid` — is accepted: the value travels in a scratch register
  that an `xchg` on either side of the template swaps with rbx, and a `%N`
  naming it is written as `%ebx` or `%rbx`; a second `"b"` operand, or an
  `rbx` clobber beside one, is refused. See
  [Inline assembly](doc/features.md#inline-assembly);
  `tests/inline_asm.rs` checks every operand kind against the values `gcc -O2`
  gives.
* **`<cpuid.h>`**, bundled: GCC's `__cpuid`, `__cpuid_count`, `__get_cpuid`,
  `__get_cpuid_count` and `__get_cpuid_max`, and the `bit_*` and `signature_*`
  macros, in C on inline assembly that saves and restores rbx in the template
  (`xchgq %rbx, %q1; cpuid; xchgq %rbx, %q1`) rather than naming it as an
  operand, which rustc refuses. `__get_cpuid` passes subleaf 0, which GCC's
  leaves undefined. An `#error` on a target that is not x86.

* **SQLite compiles.** `scripts/check-sqlite.sh` downloads the SQLite 3.53.4
  amalgamation — 9.5 MB, 269,649 lines of C in one file — verifies it against
  the SHA3-256 sqlite.org publishes, and builds and runs
  `tests/sqlite-fixture/`: one `include_gnu11!`, in both of SQLite's threading
  configurations, with an in-memory database, a `CREATE TABLE`, three inserts, a
  `SELECT` through `sqlite3_prepare_v2`/`step`/`column_int` and a Rust
  `extern "C"` function called from SQL. The 9 MB is not committed and the check
  is in `scripts/ci.sh --full` only, since it needs the network; it needs Rust
  1.99 as well, because the amalgamation *defines* twenty variadic functions.
  It is the largest single C translation unit anyone ships, and what it
  exercises that no small program does — and what it costs — is in
  [`doc/testsuites.md`](doc/testsuites.md).

* **BLAKE3 compiles, and picks AVX-512.** `scripts/check-blake3.sh` downloads
  the BLAKE3 1.8.7 release, verifies it against a pinned SHA-256, and builds
  and runs `tests/blake3-fixture/`: the seven upstream C files, unedited, one
  unit each — `blake3.c`, the dispatcher, the portable code, and the SSE2,
  SSE4.1, AVX2 and AVX-512 implementations under the `#pragma GCC target`
  that stands in for the `-m` flag upstream compiles each with. The
  dispatcher reads `cpuid` and `xgetbv` with inline assembly and, on a
  processor with AVX-512, chooses it (`blake3_simd_degree()` is 16); the 35
  official test vectors pass in `hash`, `keyed_hash` and `derive_key` modes,
  and each SIMD implementation agrees with the portable one. Two things it
  wrote were refused before this release and are not now: the `"=b"`
  constraint on its `cpuid`, and `always_inline` under a target feature (see
  Fixed). It is in `scripts/ci.sh --full` only, since it needs the network.

* **xxHash compiles, and picks AVX-512.** `scripts/check-xxhash.sh` downloads
  the xxHash v0.8.4 release, verifies it against a pinned SHA-256, and builds
  and runs `tests/xxhash-fixture/`: `xxhash.c` four times, unedited, with
  `XXH_VECTOR` set to scalar, SSE2, AVX2 and AVX-512 under the matching
  `#pragma GCC target`, and `xxh_x86dispatch.c` as shipped, whose `cpuid`
  dispatcher chooses AVX-512 on a processor that has it. Every unit passes all
  45,771 checks of upstream's generated sanity table, the four agree on every
  length up to 4,096, and the streaming API matches the one-shot one. Four
  things it wrote were refused or wrong before this release and are not now:
  `#ifdef __has_include`, the feature macros under `#pragma GCC target`, the
  `{att|intel}` dialect braces in its `cpuid`, and `aligned(1)` on a `typedef`
  (see Fixed). It is in `scripts/ci.sh --full` only, since it needs the
  network.

* **Atomic operations on a function pointer.** Loading, storing, exchanging and
  compare-exchanging an object of function-pointer type — `_Atomic(void (*)
  (void))`, `<stdatomic.h>`'s `atomic_store` on one, and the `__atomic_*`,
  `__sync_*` and `__c11_atomic_*` builtins — used to be refused with "a
  function pointer is an `Option<fn>` in Rust, which no atomic holds". It goes
  through the same `AtomicPtr<c_void>` as any other pointer now, with a
  `transmute` at each end: an `Option<unsafe extern "C" fn(…)>` is
  pointer-sized and uses the null pointer as its `None`, which is exactly the
  representation C gives a function pointer. Arithmetic (`__atomic_fetch_add`
  and friends) is still refused, because C has none on a function pointer
  either. See `tests/atomics.rs`.

### Changed

* **cinrs presents itself as GCC 14.2, and as `__CINRS__`.** `__GNUC__`,
  `__GNUC_MINOR__` and `__GNUC_PATCHLEVEL__` are 14, 2 and 0, up from Clang's
  4.2.1, because that is the version real programs gate on: libdeflate
  `#error`ed out on 4.2 ("gcc versions older than 4.9 are no longer
  supported") and switched off its VPCLMULQDQ CRC-32 and AVX-VNNI Adler-32,
  xxHash's dispatcher left AVX2 and AVX-512 off (`__GNUC__ > 4`), and glibc's
  `<math.h>` sent `isnan` through its slow `__MATH_TG` fallback instead of
  `__builtin_isnan`. `__VERSION__` is `"14.2.0 (cinrs <version>)"`, and the
  identity a program asks for when it wants to know who really compiled it is
  `__CINRS__` (with `__cinrs__`, both `1`) and `__CINRS_MAJOR__`,
  `__CINRS_MINOR__`, `__CINRS_PATCH__`. Newly predefined as GCC 14 has them:
  `__GNUC_STDC_INLINE__` (`__GNUC_GNU_INLINE__` in `c89!`/`gnu89!`),
  `__GCC_IEC_559` and `__GCC_IEC_559_COMPLEX` as `0` (Annexes F and G are not
  claimed, and leaving them undefined would make glibc's `<stdc-predef.h>`
  claim both), `__BIGGEST_ALIGNMENT__`, `__FLT_EVAL_METHOD_TS_18661_3__`,
  `__FLOAT_WORD_ORDER__` and `__ORDER_PDP_ENDIAN__`. Left out on purpose:
  `__GCC_ASM_FLAG_OUTPUTS__` (flag outputs are refused), `__SIZEOF_FLOAT128__`,
  `__OPTIMIZE__`, `__NO_INLINE__` and `__PRAGMA_REDEFINE_EXTNAME`. glibc's
  `<tgmath.h>`, the one platform header that did not go through, now does.
* **The minimum supported Rust version is 1.98**, up from 1.88. The bundled
  intrinsics headers are generated from the oldest supported compiler's
  `core::arch`, so that nothing is declared that it lacks, and AVX-512 is
  stable there from 1.89; 1.98 is the release they were generated from. The
  0.2.0 release will need 1.99, for `c_variadic`.
* **`register int x asm("eax")` says what to write instead.** An `asm` label on
  a local variable — GCC's register variable — is still refused, and the
  message now says to write the register as a constraint of the `asm` that
  uses it: `"a"(x)`.
* **GCC's C torture tests: 1,577 of 1,769 correct (89.1 %)** under `gnu11!`,
  up from 1,516, and 1,570 (88.8 %) under `gnu89!`, up from 1,509; 1,638
  (92.6 %) and 1,631 (92.2 %) on Rust 1.99. Sixty cases that were refused on
  their inline assembly now run, and `execute/bitfld-5` with them. See
  [`doc/gcc-torture.md`](doc/gcc-torture.md).

* **A `goto` no longer costs the loop it was written in.** The functions whose
  jumps Rust cannot make directly are still lowered into a control-flow graph,
  but that graph is now read back into Rust's own `loop`s, `if`s and `match`es
  by a [relooper](doc/translation.md#control-flow-and-goto) — Emscripten's
  algorithm — instead of being emitted as one flat `loop { match state { … } }`
  over block numbers. A `switch` comes out as a `match` with the case bodies in
  its arms and fallthrough as the code after it, a loop comes out as a Rust loop
  named after the C label its head stands at, and a jump forwards is a `break`
  of a labelled block. A state variable is left only where the C really is
  irreducible — a cycle with two heads, which is what a `goto` into a loop body,
  Duff's device and two loops that jump into each other's bodies each make —
  and it is one `u32` per such region rather than one per function. A computed
  `goto` keeps the old machine, whole, because `&&label` *is* a block's number.

  What it is worth, measured on one machine:

  | | before | after |
  | --- | ---: | ---: |
  | the `statemachine` kernel (a lexer written as a dozen `goto`s) | 2.16× `gcc -O2` | 1.00× |
  | one small SQLite query loop against a `gcc -O2` build of the same amalgamation | 4.24× | 0.97× |
  | `sqlite3VdbeExec`, instructions for that loop under `callgrind` (GCC's: 0.35 G) | 1.97 G | 0.36 G |

  Over SQLite's 2,610 defined functions, 13 were whole-function state machines
  and none is now: 12 are relooped outright and `sqlite3VdbeExec` has one
  irreducible region, `abort_due_to_error`, which the progress-callback loop at
  `vdbe_return` jumps back to. Over the GCC torture corpus 44 functions needed
  the machine and 19 do — every one of them a computed `goto`. `execute/medce-1`,
  which asks the optimiser to delete a call to an undefined `link_error()`
  inside `if (0)`, now compiles and passes. [`doc/benchmarks.md`](doc/benchmarks.md)
  was regenerated after the change: `statemachine` is at 0.99×, 32 of the 40
  programs (the SIMD kernel below included) are within 10 % of `gcc -O2` or
  faster, and every output still matches.

* **The bundled headers are ISO C; POSIX comes from the platform.** `<unistd.h>`,
  `<fcntl.h>`, `<strings.h>` and `<sys/types.h>` are no longer bundled. They were
  small incomplete copies — no `access`, `fsync` or `sysconf`, no `struct flock`
  or `F_RDLCK`, no `ETIMEDOUT` — and worse, a program that switched the
  platform's own headers on with `#pragma cinrs system_include` still got the
  bundled ones, which is the opposite of what it had asked for. The story is one
  sentence now: the bundled set is the headers ISO C describes (plus
  `<alloca.h>`, which is this crate's own), and POSIX comes from the platform,
  complete and consistent with `<sys/stat.h>` and `<pthread.h>`.

  **This is breaking for a program that included one of those four without the
  pragma.** Add one line:

  ```c
  #pragma cinrs system_include
  #include <unistd.h>
  ```

  or set `CINRS_SYSTEM_INCLUDE=1` for the whole crate. A program that asks for a
  POSIX header with the switch off now gets a diagnostic that says so and names
  the pragma, rather than only the directories that were searched.

  Two things follow. **Apple's platforms get a default** at last: the SDK's
  include directory is asked of `xcrun --show-sdk-path`, once per process, so a
  macOS user switches the platform on exactly as a Linux user does instead of
  having to set `CINRS_SYSTEM_INCLUDE_PATH`. And **the bundled `<errno.h>` is
  complete**: the whole POSIX.1-2017 `E*` set, at each platform family's own
  values — glibc and musl's from the kernel's `asm-generic`, Apple's from xnu,
  Windows' from the Universal CRT, with `EDEADLK` 36 and `ETXTBSY` 139 there
  rather than the Linux 35 and 26, and Darwin's `ENOTSUP` 45 distinct from its
  `EOPNOTSUPP` 102. MIPS and SPARC on Linux get only what ISO C requires, their
  kernels renumbering everything above 34.

* **A bundled header and a platform header may now define the same type.** In
  plain `system_include` mode a unit holds both sets at once — the bundled
  `<time.h>` for `struct timespec`, glibc's `<pthread.h>` for
  `pthread_cond_timedwait` — and that used to be three errors: "redefinition of
  'struct timespec'", "redefinition of 'struct tm'" and "macro 'CLOCKS_PER_SEC'
  redefined". Every definition in the bundled `<time.h>` now tests and then
  claims the per-type guard macro glibc, musl and mingw-w64 use for the same
  type (`_STRUCT_TIMESPEC`, `__struct_tm_defined`, `__time_t_defined`,
  `__clock_t_defined`, and the `__DEFINED_*` and `_*_DEFINED` spellings), the way
  mingw-w64 and musl coexist with each other; and where a bundled header and a
  platform header define the same *macro* differently, the platform's definition
  wins without a diagnostic, since the two are descriptions of one C library.
  `tests/system_headers.rs` compares every shared layout against `cc`'s over the
  platform's headers alone, because claiming a guard is a promise about layout.

* **A block under rust-analyzer no longer searches the crate from scratch.**
  Where the host gives a procedural macro no source positions, a raw-token block
  finds its own text by walking the crate's `.rs` files and proving which
  invocation is its own — and it used to do the whole walk again for every block.
  That is a hundred directories and five thousand entries here, 25 ms a block in
  the unoptimized build a proc-macro server runs, and some thirteen seconds of
  that server's time each time an editor re-analysed this repository's own five
  hundred blocks. The search now remembers, per process: the file list for two
  seconds, since one edit provokes a burst of expansions and nothing on disk
  moves between the first and the last; and each file's text for as long as the
  file still reports the modification time and length it was read at, which the
  search asks for anyway before reading it. A block costs 0.95 ms instead of
  25 ms, a file that was just saved is read again by the very next expansion, and
  nothing about which invocation is found can change — every candidate is still
  charged against the same byte budget, and a text that is out of date fails to
  match token for token exactly as an unsaved buffer's does. `cargo build` never
  went near any of this: it has the positions. See
  `crates/cinrs-core/src/locate.rs`.

### Fixed

* **Without the `complex` feature a complex type may still be named.** A
  declared-only prototype, a `typedef`, a pointer, `sizeof` and `_Generic`
  accept `double _Complex` and its relatives, so glibc's `<complex.h>` and
  `<tgmath.h>` go through with the feature off; the diagnostic naming the
  feature moved to where a complex value would exist — an object, a cast, a
  call of such a function (by name).
* **The bundled `<math.h>` has C99's classification and comparison macros.**
  `isnan`, `isinf`, `isfinite`, `isnormal`, `signbit` and `fpclassify` (with
  `FP_NAN` … `FP_NORMAL`), and `isgreater`, `isgreaterequal`, `isless`,
  `islessequal`, `islessgreater` and `isunordered` (C99 7.12.3, 7.12.14) were
  missing, so Wren's VM stopped at "implicit declaration of function
  'isnan'". Each is now GCC's definition, one builtin per macro and so
  type-generic over `float`, `double` and `long double`; `HUGE_VALL`,
  `MATH_ERRNO`, `MATH_ERREXCEPT` and `math_errhandling` are defined too, and
  `NAN` and `INFINITY` are GCC's `__builtin_nanf("")` and `__builtin_inff()`,
  so `NAN` is a positive quiet NaN on every host. Like glibc's header, the
  macros are left out of strict `c89!`, where the names are the program's.
* **The `long double` boundary check no longer fires in code that cannot
  run.** Under `#pragma cinrs system_include first`, glibc's `isnan(x)` for
  a compiler claiming GCC 4.2 is `__MATH_TG`, a `sizeof` chain naming
  `__isnanf`, `__isnan` and `__isnanl`; the `__isnanl` arm is dead for a
  `double`, but it was refused as a call across the boundary — Wren's VM
  again. An operand a constant condition excludes — of `?:` (and GNU's
  `?:` without a middle operand), of `&&` and `||` after a constant left
  operand, or a branch of an `if` with a constant condition that has no label
  or `case` inside it — is still checked and still generated, but its uses of
  the boundary are not recorded, as GCC diagnoses nothing in dead code.
* **`long double` at the platform boundary is redirected or refused, never
  silently wrong.** `long double` is `double` here, which is self-consistent
  inside a unit but was silently wrong at a call into the platform's library,
  whose `long double` is x87's eighty bits on x86-64 System V (a 128-bit quad
  on AArch64 Linux): chibicc's tokenizer calls glibc's `strtold` under
  `#pragma cinrs system_include first`, the call was bound with the `double`
  convention, and every floating literal it read came back as garbage;
  `printf("%Lf", 2.5L)` printed `-nan`. Now a declared-only ISO C function
  whose only difference from a `double` sibling is the type — `strtold`,
  `wcstold`, every `<math.h>` and `<complex.h>` `l` form, and `nexttoward` — is
  linked to the sibling (`#[link_name = "strtod"]`), which is what "`long
  double` is `double`" means; any other declared-only function with a
  `long double` or a `long double *` in its prototype is refused where it is
  called or its address taken, and so is a `long double` or a pointer to one
  handed to a declared-only function's `...`, with the rewrite (cast to
  `double` and use `%f`). A variadic function the unit defines reads back the
  `double` it was passed and is unaffected. Nothing changes on a target whose
  `long double` is `double` already (MSVC, 32-bit Arm, Apple arm64). The
  bundled `<stdlib.h>` declares `strtold`. `tests/ui/long_double_abi.rs`

* **`"x"` and `"v"` asm operands at 256 and 512 bits.** libdeflate's Adler-32
  templates keep their AVX2 and AVX-512 accumulators live with an empty barrier,
  `__asm__("" : "+x"(v))` or `"+v"(v)` on a `__m256i` or `__m512i`, which was
  refused ("a 32-byte operand cannot live in an SSE register"), and `"v"` was
  an unknown letter. As in GCC, `"x"` is now a vector register as wide as the
  operand — `asm!`'s `xmm_reg`, `ymm_reg` or `zmm_reg` — and `"v"` maps the
  same way, since `asm!` itself opens registers 16–31 to the three classes by
  the function's target features. `%x0`, `%t0` and `%g0` (the operand's xmm,
  ymm, zmm name) are `{o0:x}`, `{o0:y}` and `{o0:z}`. A 256-bit operand in a
  function without `avx` gets rustc's own error naming the target feature.
* **`_Pragma`'s operand is macro-expanded.** CRoaring and simdjson open a
  target region from a macro with `_Pragma(STRINGIFY(GCC target(T)))`, which
  was refused with "'_Pragma' takes one string literal" and three cascade
  errors from the operand's tokens reaching the parser. The operand is now
  macro-replaced before it is destringized, written directly or produced by a
  macro, as GCC and Clang do (even the `(` may come out of a macro), and the
  result goes through the same `#pragma` path a literal operand does. An
  operand that is still not one string literal is one error, and the whole
  parenthesised operand is consumed with it.
* **`aligned(1)` on a `typedef` is honoured.** xxHash's idiom for an unaligned
  read, `typedef __attribute__((__aligned__(1))) uint64_t xxh_unalign64;
  return *(const xxh_unalign64 *) p;`, was compiled as an aligned `u64`
  dereference because the attribute was dropped on a `typedef` that was not an
  anonymous record — a silent misaligned read, undefined behaviour in Rust and
  a panic in a debug build. `aligned(N)` on a `typedef` of a scalar or a pointer
  now makes the variant GCC makes: a pointer to it carries alignment N, so an
  access through it is a `read_unaligned`/`write_unaligned`; `_Alignof` is N; a
  member of the type is N-aligned; and a stronger N over-aligns an object or a
  member declared with it. The shapes whose layout cannot follow yet (a member
  aligned to more than one byte but less than its type, an array member, a
  weaker alignment on a record, array or `_Atomic` `typedef`) are refused
  rather than dropped. `may_alias` stays accepted and ignored.
* **`always_inline` under a target feature is `#[inline]`.** A
  `static inline __attribute__((always_inline))` helper in a function set
  compiled under `#pragma GCC target("avx2")` — BLAKE3's `INLINE` helpers, and
  the shape of every SIMD kernel written for one `-m` flag — became
  `#[inline(always)]` beside `#[target_feature]`, which rustc refuses
  (rust-lang/rust#145574). A function with a target feature now gets
  `#[inline]`; without one, `#[inline(always)]` as before.
* **`{0.0, -0.0}` keeps its sign.** An initialiser whose values were all zero
  was emitted as a zero fill, and `-0.0 == 0.0` counted it as one, so
  `double d[2] = {0.0, -0.0}` lost the sign of its second element. A zero fill
  is now only for values whose bits are all zero.
* **Assigning to a lane of a vector value is refused.** `(a * b)[0] = 1`
  wrote into the hidden local that holds the product; it is now "expression is
  not assignable", as `f().x = 1` is, and as GCC's "lvalue required" says.
* **A wide bit-field keeps its width under a cast, `_Generic` and
  `__builtin_choose_expr`.** A value computed in the width of a bit-field wider
  than `int` — `s.b - 8` with `unsigned long long b : 40` — wraps at forty bits,
  as GCC's does, and the width travels on the expression that computed it. An
  explicit cast, `_Generic` and `__builtin_choose_expr` each rebuilt that
  expression without it, so `(unsigned long long) (s.b - 8)` with `s.b == 2`
  wrapped at sixty-four bits and gave `0xfffffffffffffffa` where GCC gives
  `0xfffffffffa`. GCC's `execute/bitfld-5` is the case; the inline-assembly
  refusal in front of it had been hiding it.
* **`__USER_LABEL_PREFIX__` is predefined.** GCC defines it as *nothing* on ELF
  and as `_` on Mach-O and 32-bit COFF, and glibc builds every large-file
  redirection out of it:

  ```c
  #define __ASMNAME(cname) __ASMNAME2 (__USER_LABEL_PREFIX__, cname)
  #define __ASMNAME2(prefix, cname) __STRING (prefix) cname
  extern int open64 (…) __asm__ (__ASMNAME ("open64"));
  ```

  With the macro undefined, `__STRING` stringified the token itself and the
  symbol came out as `__USER_LABEL_PREFIX__open64` — which compiled and then
  failed to *link*, the worst kind of wrong. It bit every program that reached
  glibc's `<fcntl.h>` or `<sys/stat.h>` with `#pragma cinrs system_include`.

* **`_Float32` and its relatives may be defined by a `typedef`.** TS 18661-3
  lets an implementation either make `_Float32` a keyword or leave it to the
  library, and glibc does the second: with `_GNU_SOURCE` its
  `bits/floatn-common.h` writes `typedef float _Float32;` for a compiler that
  has no such keyword, and `<math.h>` then declares the `f32`/`f64`/`f32x`
  function families in terms of it. Those names were refused outright, which
  cost **1,671 errors on one `#include <math.h>`** under `_GNU_SOURCE` and made
  `#pragma cinrs system_include first` unusable for any program that defines it —
  SQLite does. A name a `typedef` has defined is now an ordinary `typedef` name;
  only the *keyword* use, with nothing having defined it, is still refused with
  the reason. `doc/system-headers.md`'s table is re-measured with `_GNU_SOURCE`
  as well as without: sixty-six of the sixty-seven platform headers either way.

* **An address constant may be offset by an *expression*.** C99 6.6p9 lets the
  integer a static initialiser's address constant is offset by be any integer
  constant expression, not only a literal; only a literal was recognised, so
  `static const unsigned char *p = &table[256 - OP_Ne];` — and the
  `((void *)(intptr_t)(flags | MASK))` a table of descriptors is full of — were
  refused as "initializer is not a compile-time constant expression". The
  integer parts of such an initialiser are now folded before the check, which
  also keeps the arithmetic out of the generated `static`. See
  `tests/address_constants.rs`.

## 0.1.0 — 2026-09-22

First release. A procedural macro that takes a C translation unit and
translates it to Rust, with every generated token carrying the span of the C it
came from, so that `cargo` and an IDE put the caret on the C. The
[README](README.md) and the documents under `doc/` are the long form; this is
what is in it.

### The language

* **Ten entry points.** `c89!` (`c90!`), `c99!`, `c11!`, `c17!` and `c23!`, and
  the five `gnu…!` dialects with the GNU extensions switched on. A construct
  from a later revision is a diagnostic naming the macro to write instead, and
  `c89!` is that rule pointed the other way. `include_c99!("file.c")` and one
  such macro per entry point read a C file instead of a block.
* **C99 and after**: the arithmetic types, pointers, arrays, `struct`, `union`,
  `enum`, bit-fields, `typedef`, function pointers, aggregate and designated
  initialisers, compound literals, variably modified types and `alloca`, the
  full preprocessor, `#include` with bundled standard headers, C23's `#embed`,
  C11's `_Atomic`, `<stdatomic.h>` and `<threads.h>`, C23's keywords,
  `[[…]]` attributes and `<stdckdint.h>`, `__int128`, thread-local objects,
  complex numbers, trigraphs, digraphs and the Unicode literals.
* **K&R C**: old-style function definitions in every entry point below `c23!`,
  and implicit `int` and implicit function declarations in `c89!`/`gnu89!`.
* **The GNU extensions**: statement expressions, `typeof`, `packed`/`aligned`,
  `cleanup`, `#pragma pack`, case ranges, flexible array members, labels as
  values and computed `goto`, nested functions, `constructor`/`destructor`,
  the `__builtin_*` family, and the rest — catalogued in
  [`doc/gnu-extensions.md`](https://github.com/tanakh/cinrs/blob/master/doc/gnu-extensions.md).
  What has no honest
  translation (inline assembly, the vector extensions, `setjmp`/`longjmp`) is a
  located error rather than a mistranslation.
* **Safe functions.** `[[cinrs::safe]]`, `__attribute__((cinrs_safe))` and
  `#pragma cinrs safe f g` generate a function whose body is not wrapped in
  `unsafe`, so `rustc` checks the translation and Rust calls it without
  `unsafe`.
* **A model of the target**, so that `sizeof`, `_Alignof`, offsets, bit-field
  storage and `#if` mean what they will mean on the machine the code runs on:
  LP64, LLP64 and ILP32, chosen from `CINRS_TARGET`, `#pragma cinrs target` or
  the host, and guarded by a compile-time assertion in every expansion.
* **A header is a binding.** A function a unit declares and does not define —
  everything `#include <zlib.h>` brings in — is callable from Rust under its C
  name, and so are the header's types; a Rust `mod` around the invocation
  gives them a path. A macro needs a line of C written in the same block.
* **Works under rust-analyzer**, which hands a procedural macro no source
  positions: a raw-token block recovers its own text from the crate's sources,
  so the editor shows the same diagnostics — and the same carets — as `cargo`.

### Conformance and speed, as measured for this release

* c-testsuite: **214 of the 218 cases `c99!` is eligible for are correct
  (98.2 %)**, 217 of 220 under `c23!`.
* GCC's C torture tests: **1,515 of the 1,769 run are correct (85.6 %)** under
  `gnu11!`, and 1,573 (88.9 %) on a toolchain with `c_variadic`.
* Clang's C conformance tests: **167 of the 203 revisions run are correct
  (82.3 %)**, and 557 of the 620 `expected-error` lines land on the right line.
* **Not one case in any of the three is tagged `[bug]`.**
* 39 whole C programs built as `gcc -O2`, `clang -O2` and a `cinrs` block:
  median `cinrs`/`gcc -O2` ratio **1.01×**, 30 of 39 within 10 % of `gcc -O2`
  or faster, every program's output identical across the three builds.

### Features

* `complex` (default) — C's complex types, which is the one thing the generated
  code needs a crate for. Off: the `cinrs-rt` dependency goes away,
  `__STDC_NO_COMPLEX__` is predefined and `_Complex` is a diagnostic.
* `nightly` — diagnostics pointing *inside* a string-literal body, which needs
  `proc_macro::Literal::subspan` and therefore a nightly compiler.

### Toolchain and platforms

Rust **1.88** or later (verified on 1.88.0, 1.90.0 and 1.98.1). *Defining* a
variadic function needs Rust 1.99's `c_variadic`; below that it is a located
error, and declaring and calling one works on every supported version.

Developed and fully tested on x86-64 Linux. On macOS (arm64) and Windows
(x86-64, MSVC) the examples and the portable tests are built, linked and run in
CI — on MSVC the `printf` family links `legacy_stdio_definitions` and the
`<time.h>` functions link their 64-bit UCRT names, automatically. Other targets
are compile-checked.
