# The conformance suites

`cinrs` is measured against three public corpora. They are three different
questions, which is why there are three of them and not one:

| suite | corpus | what it asks | cases | **correct** | errors |
| --- | --- | --- | ---: | ---: | --- |
| [c-testsuite](c-testsuite.md) | `third_party/c-testsuite/tests/single-exec` | does a small whole program run and print the right thing? | 220 | **98.2 %** (`c99!`) | 4: 3 unimplemented, 1 toolchain |
| [GCC torture](gcc-torture.md) | `third_party/gcc/…/gcc.c-torture/execute` | does a corner case somebody once filed a bug about still work? | 1776 | **84.8 %** (`gnu11!`), 84.4 % (`gnu89!`) | 269: 1 bug, 36 unimplemented, 185 not planned, 47 toolchain |
| [Clang C](clang-c-tests.md) | `third_party/llvm-project/clang/test/C` | is exactly the right *line* diagnosed, or accepted? | 276 | **77.8 %** of the 203 run | 45: 22 bug, 7 unimplemented, 16 not planned |

The first two run programs and check the answer; only the third measures what
`cinrs` **refuses**, which is half of what a front end is for. Between them
they are about 2,270 cases and about ten minutes.

**Correct** is not the same as *passing*: a case that the entry point is
*required* to refuse, and does refuse, is correct too. The
[section below](#what-correct-means-and-the-four-kinds-of-error) says what that
means and what the four kinds of error are; between the three suites there are
**23 tagged `[bug]`** — one in the torture corpus and 22 revisions, in 8 root
causes, in Clang's — and they are named one by one in the three documents.

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
| **not planned** | `[not-planned]` | Deliberately unsupported, with a located error rather than a mistranslation: inline assembly, the vector extensions, the trampoline and nonlocal-`goto` halves of nested functions, `setjmp`/`longjmp`, `long double` as a type distinct from `double`, the complex *integer* types, `-finstrument-functions`, programs that need an optimiser to delete dead code, `__builtin_return_address` and its relatives, a record both packed and over-aligned, a `va_list` where Rust cannot put one — and everything the tables mark 🔴 `not planned` or ⚫ `impossible`. Nothing here is a to-do. |
| **toolchain** | `?` marker | Not about `cinrs` at all: the case needs a Rust that this toolchain is older than. Today that is `c_variadic`, stable in 1.99 — a variadic *definition* or a `va_list` object. |

A summary therefore reads

```
gcc.c-torture/execute through `gnu11!`: 1500/1769 correct (84.8%) — 1396 passed, 104 rejected as the standard requires
  errors: 269 — bug 1, unimplemented 36, not planned 185, toolchain 47
  (7 not generated) — 4 m 19 s
```

and the old numbers are still there: `passed` is the pass rate's numerator.

### The line format

One id per line: a marker, the id, a category tag and a note.

```
execute/20001009-2  [not-planned]    compile error: inline assembly is not supported
00204               [unimplemented]  unsupported: `va_arg` with a struct type
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
* **What is *not* duplicated is largely what `cinrs` documents as
  unsupported.** `asm.c` is inline assembly — a located error on purpose. It
  would become an `!` entry on day one and teach nothing. (`unicode.c` was a
  second until the `u8"…"`/`u"…"`/`U"…"` literals and extended identifiers
  landed, `tls.c` a third until `_Thread_local` did, and `atomic.c` a fourth
  until `_Atomic` did; `tests/c11.rs`, `tests/c23.rs`,
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
