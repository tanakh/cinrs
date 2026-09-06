# The conformance suites

`cinrs` is measured against three public corpora. They are three different
questions, which is why there are three of them and not one:

| suite | corpus | what it asks | cases | passing |
| --- | --- | --- | ---: | ---: |
| [c-testsuite](c-testsuite.md) | `third_party/c-testsuite/tests/single-exec` | does a small whole program run and print the right thing? | 220 | **97.7 %** (`c99!`) |
| [GCC torture](gcc-torture.md) | `third_party/gcc/…/gcc.c-torture/execute` | does a corner case somebody once filed a bug about still work? | 1769 | **82.7 %** (`gnu89!`), 77.3 % (`gnu11!`) |
| [Clang C](clang-c-tests.md) | `third_party/llvm-project/clang/test/C` | is exactly the right *line* diagnosed, or accepted? | 276 | **61.1 %** of the 203 run |

The first two run programs and check the answer; only the third measures what
`cinrs` **refuses**, which is half of what a front end is for. Between them
they are about 2,265 cases and about five minutes.

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

The list itself is one id per line with a note, and a marker in front of the id
saying what kind of claim the line makes:

| marker | claim |
| --- | --- |
| *(none)* | `cinrs` gets this one wrong. |
| `?` | it passes or fails depending on the *toolchain* — the Rust 1.99 variadic gap, mostly — and is guarded neither way. |
| `!` | this entry point makes the case invalid, or refusing it is conforming behaviour, so the refusal is the *right* answer. Guard mode asserts the case still fails to compile; one that builds and runs is a failure. The note may open with `error: "<substring>"`, which the diagnostic then has to contain. |

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
  thoroughly, among the torture suite's 1,769. Forty files against 2,265 is not
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
