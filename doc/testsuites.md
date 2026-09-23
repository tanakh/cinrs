# The conformance suites

`cinrs` is measured against three public corpora. They are three different
questions, which is why there are three of them and not one:

| suite | corpus | what it asks | cases | **correct** | errors |
| --- | --- | --- | ---: | ---: | --- |
| [c-testsuite](c-testsuite.md) | `third_party/c-testsuite/tests/single-exec` | does a small whole program run and print the right thing? | 220 | **98.2 %** (`c99!`), 98.6 % (`c23!`) | 4: 3 unimplemented, 1 toolchain |
| [GCC torture](gcc-torture.md) | `third_party/gcc/…/gcc.c-torture/execute` | does a corner case somebody once filed a bug about still work? | 1776 | **89.1 %** (`gnu11!`), 88.8 % (`gnu89!`); 92.6 % / 92.2 % on `beta` | 192: 0 bug, 3 unimplemented, 127 not planned, 62 toolchain |
| [Clang C](clang-c-tests.md) | `third_party/llvm-project/clang/test/C` | is exactly the right *line* diagnosed, or accepted? | 276 | **82.3 %** of the 203 run | 36: 0 bug, 6 unimplemented, 30 not planned |

The first two run programs and check the answer; only the third measures what
`cinrs` **refuses**, which is half of what a front end is for. Between them
they are about 2,270 cases and about ten minutes.

**Correct** is not the same as *passing*: a case that the entry point is
*required* to refuse, and does refuse, is correct too. The
[section below](#what-correct-means-and-the-four-kinds-of-error) says what that
means and what the four kinds of error are; between the three suites there is
now **not one case tagged `[bug]`**. What is left is what each document lists
as unimplemented or not planned, case by case.

## Results at a glance

* **[c-testsuite](https://github.com/c-testsuite/c-testsuite)** — whole
  programs with the output each must produce. Of the 220 in its `single-exec`
  suite, **214 of the 218 that `c99!` is eligible for are correct (98.2 %)**,
  216 of 220 under `c11!` and 217 of 220 under `c23!` and every GNU dialect.
  **Not one error in this corpus is a bug**: two are constructs `cinrs` has not
  implemented — a `goto` out of a statement expression, and a `va_arg` of a
  `struct` too large for the argument registers — one is C23's empty
  initialiser `{}` in a block a strict `c99!` or `c11!` refuses it in, and the
  last needs a newer Rust than 1.98. Strict
  `c89!` is 173 of the 175 it selects, because 22 of the cases it takes — the
  corpus tags them for portability rather than for strict C90 — use something
  C99 or C11 added, and a strict C89 entry point is required to refuse them.
  The corpus is a git submodule, so a fresh checkout skips the suite
  until `git submodule update --init third_party/c-testsuite` fetches it.
  [`doc/c-testsuite.md`](c-testsuite.md) has the details.
* **[GCC's C torture tests](gcc-torture.md)** — 1,776 self-checking
  programs, each a bug report distilled into twenty lines, where success is
  exit status zero. **1,577 of the 1,769 run are correct (89.1 %)** under
  `gnu11!` — 1,473 passing and 104 refused as C99 requires — and 1,570
  (88.8 %) under `gnu89!`, which is the language these C89-era programs were
  written in and refuses none of them; on `beta`, where a variadic definition
  compiles, the same runs are 1,638 (92.6 %) and 1,631 (92.2 %). **Not one of
  the 192 errors is a bug**; they are the memory operands and x87 registers of
  inline assembly, the vector extensions, the complex
  *integer* types, the corners of nested functions that need a trampoline or a
  nonlocal `goto`, the handful of `__builtin_*` forms this crate does not
  implement, the definitions and `va_list`s that need Rust 1.99, and five
  programs that built and then did the wrong thing, which the document names
  one by one.
* **[Clang's C conformance tests](clang-c-tests.md)** — one file per WG14
  paper or defect report, with `// expected-error` comments saying exactly
  which lines must be diagnosed. **167 of the 203 revisions run are correct
  (82.3 %)** — 136 answered exactly and 31 refused because the entry point
  requires it — and of the 620 `expected-error` lines the suite asks about,
  **557 are diagnosed on the right line**. This is the only suite that measures
  what `cinrs` *refuses*, which is half of what a front end is for, and **not
  one of its 36 errors is a bug** either: they are the features the document
  lists as not yet implemented, and the places where `cinrs` and Clang
  disagree on purpose — usually with GCC on `cinrs`'s side.

A fourth corpus needs no fetching, because it is already on the machine: **the
platform's own headers**. `tests/system_headers.rs` puts each of the C standard
headers and the POSIX set through the front end alone, in `gnu11!` and `c11!`,
with the platform's copies preferred over the bundled ones — **66 of the 67 go
through unchanged** against glibc 2.43, the exception being `<tgmath.h>` — and
then compiles and runs ordinary programs against those declarations, comparing
`sizeof(struct stat)` and its like against the host's own `cc`.
[`doc/system-headers.md`](system-headers.md#the-table) is the table.

Each suite has a document of its own with its baseline, its failures by cause
and how to reproduce the numbers. What follows is what they have in common.

## Fetching the corpora

```
scripts/fetch-testsuites.sh              # fetch everything that is missing
scripts/fetch-testsuites.sh --check      # say what is present, fetch nothing
scripts/fetch-testsuites.sh gcc llvm     # fetch only the named corpora
```

c-testsuite is a git submodule and is small enough to stay one. GCC and LLVM
are not: each is a **blobless, sparse checkout** of the one directory that is
wanted, pinned to a commit recorded at the top of the script, which turns a
multi-gigabyte clone into a few tens of megabytes. Both live under
`third_party/` and both are in `.gitignore`; re-running the script is a no-op
when everything is already at its pin, so it is safe in a CI step in front of
the tests.

**Nothing from any of the three is copied into this repository.** The corpora
hold the cases, the generated files that embed one live under `target/`, and
the checked-in expected-failure lists hold case *names* and notes written here.
The licences are c-testsuite: MIT for the harness, per-case upstream for the
cases; GCC: GPLv3+ with the Runtime Library Exception; LLVM: Apache-2.0 with
LLVM exceptions.

Without a corpus its harness prints how to fetch it and **exits successfully**,
so a fresh checkout still has a green `cargo test`.
`CINRS_TESTSUITES_REQUIRED=1` (and `CINRS_CTESTSUITE_REQUIRED=1` for the
submodule) turns the skip into a failure, which is what a CI job that means to
run a suite wants.

## The features the numbers are measured with

All three lists describe `cinrs` **as its default features build it**, and
`complex` is one of those, so a harness that is not that build skips itself
exactly as one without its corpus does — it says that the corpora are measured
with the default features and that this build has `complex` off, and exits
successfully, which is what keeps `cargo test -p cinrs --no-default-features`
green. The reason is that with the feature off `_Complex` is a diagnostic
again, so every case that uses one stops compiling: seven `clang/test/C`
revisions come out as false rejections against a list that says nothing of the
kind, and the other two corpora have the same latent problem. That is a
different question rather than a regression, and the answer is to run the suite
with the defaults or with `--features complex`. The check is
`conformance::skip_unless_measured_build`, beside the corpus check in
[`tests/support/conformance.rs`](../tests/support/conformance.rs), and
`CINRS_TESTSUITES_REQUIRED=1` turns this skip into a failure too.

## The safety rules

**Read this before running any of them.** A conformance run is a thousand
`rustc` processes and a thousand programs that a front end *under development*
wrote, and either half can ask for all of memory: a wrong array bound is a
`[u8; 2^61]` in the expansion, a miscompiled loop is a `malloc` that never
stops. `ui_test` bounds neither, and its default parallelism is one test per
core, so on a many-core machine the first mistake takes the machine down rather
than the test. That is not hypothetical.

Four ceilings are built into
[`tests/support/conformance.rs`](../tests/support/conformance.rs), all keyed to
one number — `CINRS_MEMORY_LIMIT_MB`, 8192 by default, `0` to switch all four
off:

1. **An RSS watchdog thread in the harness process.** Every 250 ms it reads
   `/proc/self/statm`; past the limit it prints what happened and calls
   `std::process::abort`.
2. **`ulimit -v` and `timeout(1)` on every compiler the harness spawns.** The
   compiler is run through
   `sh -c 'limit=$0; …; ulimit -v "$limit"; exec "$@"' <kb> timeout -k 5 300 rustc …`.
   The limit is clamped to the inherited hard limit and every step is silent,
   because that shell shares its stderr with the compiler and `ui_test`
   compares the compiler's stderr byte for byte.
3. **A clock and a memory gauge inside every generated program.** The watchdog
   thread exits 124 after the per-case timeout and exits 137 — reported as
   `runtime: out of memory` — when the program's own resident set passes the
   limit. It is there as well as the `ulimit` because a limit is only inherited
   by a process the harness itself started.
4. **A cap on the default parallelism**, at the smaller of this machine's
   parallelism and eight. `CINRS_TEST_THREADS=N` overrides it in the
   environment and `-- --test-threads=N` overrides both. This is the one that
   matters most: what exhausted the machine was thirty-two `rustc` processes at
   once rather than any single one of them.

Each of the four has a test that provokes it, in
[`tests/harness_safety.rs`](../tests/harness_safety.rs) — a child process that
allocates past a 64 MiB ceiling and is aborted, a `rustc` that cannot start
under an 8 MiB address-space limit, a generated program that allocates without
bound and is killed with a message, and the thread cap itself.

On top of all that, **wrap every run in an outer cap of your own**:

```
( ulimit -v 8000000; timeout -k 30 3600 \
      cargo test -q --test gcc_torture -- --test-threads=2 )
( ulimit -v 8000000; timeout -k 10 1800 \
      cargo test -q --test clang_c    -- --test-threads=2 )
( ulimit -v 8000000; timeout -k 10 1800 \
      cargo test -q --test c_testsuite -- --test-threads=2 )
```

A plain `cargo test --workspace` runs all three. When that is not what is
wanted — a quick check of everything *else* — neutralise the two big ones with
a filter that matches nothing, which is the only way to skip a corpus that is
present:

```
( ulimit -v 8000000; CINRS_GCC_TORTURE_FILTER=__none__ \
      CINRS_CLANG_C_FILTER=__none__ timeout -k 10 1800 cargo test -q --workspace )
```

## The three modes, and the expected-failure list

Every harness has the same three modes, spelled with the same suffixes on its
own environment-variable prefix (`CINRS_CTESTSUITE_`, `CINRS_GCC_TORTURE_`,
`CINRS_CLANG_C_`):

* **guard** — the default, and what `cargo test` runs. Every case *not* in the
  expected-failure list must pass; a listed case is run anyway, and one that
  has started passing is reported so the line can go.
* **`…_REPORT=1`** — run everything, fail nothing, print the tables: the rate,
  the failures grouped by cause, the skips with their reasons.
* **`…_REPORT=1 …_UPDATE_EXPECTED=1`** — rewrite the list from the report,
  keeping every `?` and `!` line as it stands.

`…_FILTER=<substring>` narrows a run to the cases whose id contains it, and
`…_STRICT=1` makes a stale list entry a failure rather than a warning.

## What "correct" means, and the four kinds of error

**A case that must fail and does fail is correct.** That is the rule the
numbers in all four documents are built on, and it is why the headline of every
report is a *correct* rate rather than a pass rate:

```
correct = passed + rejected as the standard requires
```

The second term is the `!` lines below — a `c89!` block that uses `long long`,
a `c23!` one that calls an unprototyped function pointer with an argument.
Refusing those is what the entry point owes the standard, so counting them as
shortfalls measures the wrong thing.

What is left over is an **error**, and there are exactly four kinds. Every
report breaks its error count down into them, in this order:

| category | tag | what it means |
| --- | --- | --- |
| **bug** | `[bug]` | `cinrs` is wrong here: it accepts the case and mistranslates it, refuses code it means to support, or emits Rust that will not compile. These are the work items. A failure that is not in the list at all counts as one. |
| **unimplemented** | `[unimplemented]` | A feature `cinrs` intends to have and has not got to yet — the 🟠 `planned` rows of [`doc/gnu-extensions.md`](gnu-extensions.md) and every diagnostic that says "not supported yet". |
| **not planned** | `[not-planned]` | Deliberately unsupported, with a located error rather than a mistranslation: the memory operands and x87 registers of inline assembly, which `asm!` has no operand for, the vector extensions, the trampoline and nonlocal-`goto` halves of nested functions, `setjmp`/`longjmp`, `long double` as a type distinct from `double`, the complex *integer* types, `-finstrument-functions`, programs that need an optimiser to delete dead code, `__builtin_return_address` and its relatives, a record both packed and over-aligned, a `va_list` where Rust cannot put one — and everything the tables mark 🔴 `not planned` or ⚫ `impossible`. Nothing here is a to-do. |
| **toolchain** | `?` marker | Not about `cinrs` at all: the case needs a Rust that this toolchain is older than. Today that is `c_variadic`, stable in 1.99 — a variadic *definition*, a `va_list` object, or a `va_list *`. |

A summary therefore reads

```
gcc.c-torture/execute through `gnu11!`: 1577/1769 correct (89.1%) — 1473 passed, 104 rejected as the standard requires
  errors: 192 — bug 0, unimplemented 3, not planned 127, toolchain 62
  (7 not generated) — 4 m 14 s
```

and the old numbers are still there: `passed` is the pass rate's numerator.

### The line format

One id per line: a marker, the id, a category tag and a note.

```
execute/20061220-1  [not-planned]    compile error: the constraint "m" asks for a memory operand, …
00213               [unimplemented]  unsupported: a `goto` out of a statement expression
?00140                               variadic function definition; needs Rust 1.99
!00200                               error: "'long long' requires C99 or later"  conforming: …
```

| marker | claim | tag |
| --- | --- | --- |
| *(none)* | `cinrs` gets this one wrong. | **required** — one of `[bug]`, `[unimplemented]`, `[not-planned]` |
| `?` | it passes or fails depending on the *toolchain*, and is guarded neither way. | none: the marker is the category |
| `!` | this entry point makes the case invalid, or refusing it is conforming behaviour, so the refusal is the *right* answer. Guard mode asserts the case still fails to compile; one that builds and runs is a failure. The note may open with `error: "<substring>"`, which the diagnostic then has to contain. | none: it records no error |

A plain line with no tag is a **read error** naming the file and line, so a
list cannot quietly lose its classification. Guard mode ignores tags entirely —
a tagged failure is still a failure that is expected — and an update keeps
every tag, every `?` and every `!` as it stands. A failure that appears in an
update and is *not* already listed is written in as

```
NNNNN  [bug]  classify me: compile error: …
```

which is the conservative direction: an unexamined failure is a regression
until somebody has looked at it, and the one thing an update must never do is
file a new one quietly under `not planned`. Deleting a list before regenerating
it — the way to refresh every note at once — therefore throws every tag away
too, and everything comes back as `classify me:`.

[`tests/expected_lists.rs`](../tests/expected_lists.rs) reads every list in the
repository, checks that it parses and that it is written the way an update
would write it, and needs no corpus; it is part of an ordinary `cargo test`.
`CINRS_EXPECTED_LISTS_BLESS=1 cargo test --test expected_lists` tidies a hand
edit up without running a suite.

## Three real programs: SQLite, BLAKE3 and xxHash

A suite of small programs and one large program answer different questions. The
three harnesses above ask whether each construct is translated correctly;
`scripts/check-sqlite.sh` asks whether a quarter of a million lines of somebody
else's C, written for GCC and never adjusted for this compiler, builds and works,
and `scripts/check-blake3.sh` and `scripts/check-xxhash.sh` ask the same of two
programs whose whole point is SIMD.

It downloads the **SQLite 3.53.4 amalgamation** — 9.5 MB, 269,649 lines of C in
one file, public domain — from the pinned URL in the script, verifies it
against the SHA3-256 that sqlite.org publishes, and builds `tests/sqlite-fixture`,
a crate outside the workspace holding one `include_gnu11!`. The 9 MB is not
committed; the script is what makes the check reproducible without it. Then it
opens an in-memory database, creates a table, inserts rows, reads them back
through `sqlite3_prepare_v2`/`step`/`column_int`, calls a Rust `extern "C"`
function from SQL through `sqlite3_create_function`, and closes.

Both configurations are built: `SQLITE_THREADSAFE=1`, SQLite's own default, which
on a Unix means pthreads; and `SQLITE_THREADSAFE=0`.

What it exercises that no small program does: the virtual machine
(`sqlite3VdbeExec`, a 7,000-line `switch` inside a `for(;;)` with `goto`s into and
out of it, which becomes a control-flow graph and is relooped back into a
`match` inside a `loop`), tables of function pointers
(`sqlite3_vfs`, `sqlite3_io_methods`), twenty variadic *definitions*
(`sqlite3_mprintf`, `sqlite3_snprintf`, `sqlite3_log`, `sqlite3_config` and the
rest — the one thing that needs **Rust 1.99** and `c_variadic`), `va_list`
forwarding, bit-fields in `Expr` and `Table`, `union`
initialisers, `__builtin_expect`, address constants into other objects, atomic
stores through a function pointer, and the platform's POSIX headers through
`#pragma cinrs system_include`.

It is in `scripts/ci.sh --full` only, because it needs the network, and it skips
itself with a note — successfully, like a harness without its corpus — on a
toolchain older than 1.99.

### BLAKE3

`scripts/check-blake3.sh` downloads the **BLAKE3 1.8.7** release archive — the
hash function's reference C implementation, CC0 or Apache-2.0, about 3,000
lines in seven files — verifies it against the SHA-256 pinned in the script,
unpacks `c/` and the official test vectors into `target/blake3/`, and builds
`tests/blake3-fixture/`, a crate outside the workspace with one
`gnu11!` block per upstream file. The files are not edited: each is
`#include`d under `#pragma cinrs export`, so the cross-file calls link the way
object files would, and the four SIMD files sit under the `#pragma GCC target`
(`sse2`, `sse4.1`, `avx2`, `avx512f,avx512vl`) that stands in for the `-m` flag
upstream compiles each with.

What it exercises that SQLite does not: four implementations of one kernel in
SSE2, SSE4.1, AVX2 and AVX-512 intrinsics, `__m512i` passed and returned by
value, `static inline __attribute__((always_inline))` helpers under a target
feature, a dispatcher that reads `cpuid` and `xgetbv` through GCC inline
assembly with `"=b"` and picks an implementation at run time, `_Atomic int`
through `__has_include(<stdatomic.h>)`, and `visibility("default")`. The test
hashes the 35 official vectors (inputs from 0 to 102,400 bytes) in `hash`,
`keyed_hash` and `derive_key` modes, 210 checks, and feeds one byte at a time
to `blake3_hasher_update` as well; a second test calls each SIMD implementation
directly and compares it with the portable one, as upstream's own tests do. On
a processor with AVX-512 the dispatcher chooses it — `blake3_simd_degree()`
is 16 — so the vectors run through the AVX-512 code there, and through
whatever the machine has elsewhere.

It is in `scripts/ci.sh --full` only, because it needs the network; it needs
no particular toolchain.

### xxHash

`scripts/check-xxhash.sh` downloads the **xxHash 0.8.4** release archive —
BSD-2-Clause, a 7,500-line header that is the whole library, `xxhash.c` to
instantiate it and `xxh_x86dispatch.c` to choose a vector width at run time —
verifies it against the SHA-256 pinned in the script, unpacks those four files,
the licence and upstream's `tests/sanity_test_vectors.h` into `target/xxhash/`,
and builds `tests/xxhash-fixture/`. The files are not edited. `xxhash.c` is
included four times, once per `XXH_VECTOR` — scalar, SSE2, AVX2 and AVX-512 —
each under the `#pragma GCC target` that stands in for the `-m` flag and its own
`XXH_NAMESPACE` so the four copies of the API link side by side; the fifth unit
is the dispatcher, which carries a private copy of the library
(`XXH_INLINE_ALL`), compiles the SSE2, AVX2 and AVX-512 kernels in one unit
behind `__attribute__((__target__))`, and picks one with `cpuid` and `xgetbv`.
The sanity table is upstream's own generated list of expected XXH32, XXH64,
XXH3-64 and XXH3-128 values over a pseudo-random buffer, 45,771 checks per unit,
read as text; the tests also make the four units agree on every length from 0
to 4,096 at an aligned and a misaligned start, check the streaming API in 1-,
7- and 1,000-byte pieces against the one-shot functions, and check the
dispatcher — which picks AVX-512 on a processor that has it — against the table
and the scalar unit.

It took four things, each now a general fix. `#ifdef __has_include` (and
`__has_attribute`, `__has_builtin`) had to be true, as GCC's are, and a `__has_…`
reached through a macro (`XXH_HAS_INCLUDE(x)`) answered: without them the
dispatcher compiled only SSE2. `#pragma GCC target("avx2")` had to define
`__AVX2__` and what it implies, because `xxhash.h` includes `<immintrin.h>` only
under it. The dispatcher's `cpuid` is written with GCC's assembler dialect
alternatives, `"{cpuid|cpuid}"`, of which the AT&T one is now taken. And
`typedef __attribute__((aligned(1))) uint64_t xxh_unalign64;`, the header's way
of reading unaligned input under GCC, had been a silent aligned read — a panic
in a debug build — and is now `read_unaligned`, which the debug build of the
fixture checks on every hash.

It is in `scripts/ci.sh --full` only, because it needs the network; it needs no
particular toolchain.

### SQLite's numbers, measured once

On an x86-64 laptop under WSL2, glibc 2.43, `cargo +beta` 1.99.0-beta.6, SQLite
3.53.4:

| | value |
| --- | --- |
| front end alone (lex, preprocess, parse, sema) | 0.69 s, 316 MB peak RSS |
| `rustc`, debug | 3.8 s, 584 MB peak RSS |
| `rustc`, release | 25 s, 608 MB peak RSS |
| the translated library, debug | 41 MB rlib |
| the translated library, release | 24 MB rlib |
| the linked test binary, release | 2.8 MB |
| the smoke test itself | 0.1 s |
| one small prepared query, release | 20 µs |
| the same query through the platform's `libsqlite3` | 21 µs |

The last two are the only ones that need a caveat, and they need a large one:
they are one loop in one process, the platform's library is a *different
release* built by GCC with SQLite's own recommended options, and a
`SELECT count(*), sum(n) … WHERE n > ?` over a thousand rows is a full scan
rather than anything a profile would recognise. Read the ratio as "the same
order of magnitude", not as a benchmark. The general picture — what is fast and
what is not, and why — is in [`doc/benchmarks.md`](benchmarks.md).

That ratio was **4.2×** until the control-flow graph was [relooped](
translation.md#control-flow-and-goto), and all of the difference was in one
function. Against a `gcc -O2` build of the *same* amalgamation, `callgrind` put
`sqlite3VdbeExec` at 1.97 G instructions for this query loop where GCC's ran
0.35 G — a 1,775-state `match` with 1,041 `continue 'cfg` in it, where the C is
a `for(;;)` around a 190-case `switch`. Recovering the loop and the `switch`
brought it to **0.36 G**, 1.05× GCC's, and every other function in the
amalgamation was already within ±25 %. Against that same build the whole query
loop is 0.97× now, where it was 4.24×.

## Should there be a fourth? — chibicc

[chibicc](https://github.com/rui314/chibicc)'s `test/` directory is the obvious
candidate, and the answer is **no, not as a harness**.

What is there: 49 entries and about 89 KB — some forty `.c` files
(`arith.c`, `control.c`, `function.c`, `struct.c`, `initializer.c`, `macro.c`,
`sizeof.c`, `bitfield.c`, `vla.c`, `alloca.c`, `varargs.c`, `constexpr.c`,
`generic.c`, `unicode.c`, `atomic.c`, `tls.c`, `asm.c` and the rest), each a
run of `ASSERT(expected, expression)` macros with a `main`; a `test.h` of
hand-written declarations; a `common` translation unit that defines the
`assert` they call; a `driver.sh` that tests chibicc's *command line*; and a
`thirdparty` directory that builds real projects (git, sqlite, tinycc) with it.

Against what the three suites already do:

* **The language ground is duplicated, in less depth.** Every `.c` file above
  has a counterpart among c-testsuite's 220 whole programs and, far more
  thoroughly, among the torture suite's 1,776. Forty files against 2,270 is not
  where the next conformance bug is hiding.
* **What is *not* duplicated was largely what `cinrs` documented as
  unsupported, and has since landed.** `asm.c` is inline assembly, which was a
  located error on purpose and now translates to `asm!` for the operand kinds
  `asm!` has; `tests/inline_asm.rs` covers that ground against `gcc -O2`'s
  answers. (`unicode.c` was a second until the `u8"…"`/`u"…"`/`U"…"` literals
  and extended identifiers landed, `tls.c` a third until `_Thread_local` did,
  and `atomic.c` a fourth until `_Atomic` did; `tests/c11.rs`, `tests/c23.rs`,
  `tests/identifiers.rs`, `tests/threads.rs` and `tests/atomics.rs` cover that
  ground now.)
* **It is not nearly free.** Every case has to be *linked against a second
  translation unit* (`common`, which defines `assert`), which none of the three
  harnesses does — each generates one self-contained Rust file. Adding that is
  a new shape of harness rather than another pointing of the same machine, and
  `driver.sh` and `thirdparty` are not runnable here at all.
* **The oracle is chibicc's answer, not the standard's.** `test.h` declares
  `int vsprintf();` and friends by hand, and the expected values are what
  chibicc produces; a disagreement would need adjudicating against the standard
  before it meant anything.

What *would* be worth an hour, if the itch returns: lifting `test/vla.c`,
`test/alloca.c` and `test/bitfield.c` by hand into three ordinary integration
tests next to `tests/vla.rs` and `tests/bitfields.rs`. Those three are exactly
where `cinrs`'s implementation differs most from a real compiler's — variable
length arrays and `alloca` are emulated on the heap, and a bit-field is a pair
of accessor methods rather than a field — so a second opinion on them is worth
more than another forty files of arithmetic. chibicc is MIT-licensed, so
copying with attribution is allowed; that is a choice to make deliberately
rather than a harness to add.
