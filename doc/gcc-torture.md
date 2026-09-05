# Conformance: GCC's C torture tests

`gcc/testsuite/gcc.c-torture/execute` is the oldest and largest body of "does
this compiler get C right" in existence: about 1,800 self-checking programs,
most of them a bug report distilled into twenty lines, each of which calls
`abort()` when the compiler got it wrong and returns or `exit(0)`s when it did
not. There are no expected-output files and no options to get right — **success
is exit status zero** — which makes it the cheapest large corpus there is to
run through a new front end, and the one whose failures map most directly onto
a to-do list.

The harness is [`tests/gcc_torture.rs`](../tests/gcc_torture.rs); everything it
shares with the other two suites — the modes, the expected-failure list and its
markers, the result collector, the ceilings — is in
[`tests/support/conformance.rs`](../tests/support/conformance.rs). See
[`doc/testsuites.md`](testsuites.md) for the three of them side by side.

## Fetching it

```
scripts/fetch-testsuites.sh gcc
```

GCC is far too large to vendor and too large to be a submodule of a small
crate, so the script makes a **blobless, sparse checkout** of the one directory
into `third_party/gcc`, pinned to the commit recorded at the top of the script
(`3653b8dd68b0caf1c713058854d1efde9cc14329`, 2026-09-05). That turns a
multi-gigabyte clone into a few tens of megabytes. `third_party/gcc` is in
`.gitignore`.

Without the corpus the harness prints those two lines and exits successfully,
so a fresh checkout still has a green `cargo test`. `CINRS_TESTSUITES_REQUIRED=1`
turns the skip into a failure, which is what a CI job that means to run the
suite wants. `scripts/fetch-testsuites.sh --check` says what is present and
fetches nothing.

## Licence

GCC's testsuite is GPLv3-or-later with the GCC Runtime Library Exception.
**Nothing from it is copied into this repository**: the sparse checkout holds
it, the generated `.rs` files that embed a case live under `target/`, and
`tests/gcc-torture/expected-failures.txt` holds case *names* and notes written
here and nothing of GCC's own.

## The two groups

`execute/*.c` and `execute/ieee/*.c` are run as two groups and reported
separately, exactly as GCC's own `execute.exp` and `ieee/ieee.exp` do. The
`ieee` half is the floating-point corner cases, which GCC compiles with
`-ffloat-store` on x86 to keep excess precision out; nothing here can do that,
so its rate is expected to be the lower of the two. `execute/builtins` is a
third `.exp` and is not run: every case in it is about a specific GCC builtin
being expanded a specific way.

## The prelude

These are C89-era programs, and a great many of them call `abort()` and
`exit()` with no declaration in sight, because C89 let them. Two lines go in
front of every case:

```c
#pragma cinrs include_path "…/gcc.c-torture/execute"
extern void abort(void); extern void exit(int);
```

The declarations are compatible with the ones the cases write for themselves
and with the bundled `<stdlib.h>`, so a case that has its own is unaffected;
**580 of the 1,769** need them. `CINRS_GCC_TORTURE_PRELUDE=0` leaves them out,
which is how that number was measured. The `include_path` is there because
about thirty cases `#include` a sibling `.c` or `.h` out of the corpus
directory, which is not where the generated `.rs` file lives.

Because the prelude is *prepended*, a line of the upstream file is two lines
further down in the generated one. That is the one thing this harness gives up
that the c-testsuite one keeps.

## The DejaGnu directives

GCC's own runner reads `{ dg-… }` directives out of the source. What is
honoured is `dg-do run`/`compile`, `dg-skip-if`, `dg-require-effective-target`
and the option lists; what is *not* honoured is recorded in the report's "the
corpus" section, which says how many cases asked upstream for a flag this
harness cannot give them:

| asked for | cases | what it would mean |
| --- | ---: | --- |
| `-std=gnu89` | 75 | there is no C89 entry point; they run through `gnu11!` |
| `-fpermissive` | 32 | GCC downgrading constraint violations to warnings |
| `-std=gnu17` | 18 | run through `gnu11!` instead |
| `stack_size` | 14 | an effective-target the runner would set |
| `-fgnu89-inline` | 12 | the C89 meaning of `inline` |
| `-fwrapv` | 11 | already what cinrs does: signed overflow wraps |
| `ieee` | 11 | an effective-target for the floating-point group |
| `-msse2`, `-mfpmath=sse` | 9 each | code generation, which Rust decides |
| `-fsignaling-nans` | 9 | signalling NaNs |
| `-Wno-psabi` | 5 | a warning |

## Memory and thread safety, and how to run the suite

**This suite must be run under a memory cap.** It is a thousand `rustc`
processes and a thousand programs that a front end *under development* wrote,
and either half can ask for all of memory: a wrong array bound is a
`[u8; 2^61]` in the expansion, a miscompiled loop is a `malloc` that never
stops. `ui_test` bounds neither, and its default parallelism is one test per
core, so on a many-core machine the first mistake takes the machine down rather
than the test. That is not hypothetical — it happened twice while this harness
was being written.

Four ceilings are built in, all keyed to `CINRS_MEMORY_LIMIT_MB` (8192 by
default; `0` switches all four off):

1. an **RSS watchdog thread** in the harness process, which reads
   `/proc/self/statm` every 250 ms and aborts with a message when the resident
   set passes the limit;
2. **`ulimit -v` and `timeout(1)` on every compiler** the harness spawns —
   `sh -c 'limit=$0; …; ulimit -v "$limit"; exec "$@"' <kb> timeout -k 5 300
   rustc …`, with the limit clamped to the inherited hard limit so that
   nothing is written to the compiler's stderr;
3. a **clock and a memory gauge inside every generated program**: the watchdog
   thread exits 124 after `CINRS_GCC_TORTURE_TIMEOUT` seconds (20 by default)
   and exits 137 — reported as `runtime: out of memory` — when its own
   resident set passes the limit;
4. a **cap on the default parallelism** at eight threads
   (`CINRS_TEST_THREADS`, or `-- --test-threads=N`), because what actually
   exhausted the machine was thirty-two `rustc` processes at once rather than
   any single one of them.

They are checked by [`tests/harness_safety.rs`](../tests/harness_safety.rs),
which provokes each one.

Run the suite like this, and not otherwise:

```
( ulimit -v 8000000; timeout -k 30 3600 \
      cargo test -q --test gcc_torture -- --test-threads=2 )
```

The outer `ulimit` is belt and braces for the ceilings above; the outer
`timeout` bounds the whole run; `--test-threads=2` keeps the peak down further
than the built-in cap does. Measured that way, a full run takes **3 m 30 s**
and peaks at **298 MiB** of resident set.

## Baseline

Measured on `rustc 1.97.1` (stable), x86_64-unknown-linux-gnu, `gnu11!`, at the
pinned corpus revision.

| group | run | passed | rate |
| --- | ---: | ---: | ---: |
| `execute` | 1691 | 1195 | **70.7 %** |
| `execute/ieee` | 78 | 35 | 44.9 % |
| **total** | **1769** | **1230** | **69.5 %** |

539 failed and 7 were not generated at all: five want the effective target
`run_expensive_tests`, one holds a carriage return (which a Rust raw string
literal may not), and one is not valid UTF-8.

### The failures, by cause

524 of the 539 are refused at compile time, in 60 distinct causes. The ones
worth a line each:

| cases | cause | e.g. |
| ---: | --- | --- |
| 89 | `expected a declaration, found identifier` — old-style (K&R) definitions again, seen from the parser | `execute/20000717-3` |
| 61 | inline assembly is not supported | `execute/20001009-2` |
| 42 | old-style (K&R) function definitions are not supported | `execute/20000112-1` |
| 41 | `vector_size` / `__vector_size__`: the vector extensions need an unstable Rust feature | `execute/20050316-1` |
| 39 | implicit declaration of a function (C99 removed it; these are C89 programs) | `execute/20000412-3` |
| 38 | variadic function *definitions*, which need Rust 1.99's `c_variadic` | `execute/20030914-2` |
| 23 | `expected ';' after declaration, found '{'` — K&R again | `execute/20000822-1` |
| 21 | `_Complex` | `execute/20010605-2` |
| 17 | a builtin that needs a type the unit has not declared | `execute/20020406-1` |
| 15 | `va_arg` with a struct type | `execute/920625-1` |
| 13 | an `ieee` case calling `__builtin_issignaling` and friends | `ieee/bfloat16-builtin-issignaling-1` |
| 11 | `expected expression` — assorted parse gaps | `execute/20040302-1` |
| 9 | `__attribute__((mode(…)))` | `execute/20020108-1` |
| 8 | `va_list` in a context that needs Rust 1.99 | `execute/20000519-1` |
| 6 | an initialiser whose type does not convert | `execute/20020920-1` |
| 6 | `va_list` somewhere other than a local or a parameter | `execute/stdarg-1` |
| 5 | a `#include` of a corpus file the harness does not put on the path | `execute/pr105777` |
| 5 | a struct member with a variably modified type | `execute/20020412-1` |

The remaining 42 causes have four cases or fewer each; the report prints all
sixty.

Three of the rows above — K&R definitions (89 + 42 + 23 = 154 cases), implicit
declarations (39) and the Rust 1.99 variadic gap (38 + 8) — are **half of every
compile failure between them**, and none of the three is about C the language
being hard. K&R is a parser feature; implicit declarations are a C89 rule that
`gnu89!` (which does not exist) would restore; and the variadic ones simply
pass on a newer toolchain, which is why they are marked `?` in the
expected-failure list.

### The programs that built and then did the wrong thing

Fifteen cases compiled, ran and failed. These are the interesting ones — a
miscompilation or an unsupported semantic that nothing diagnosed — and each is
a to-do item with a name. Fourteen died on `abort()`, which in this corpus
means the program's own check failed:

```
execute/20021127-1     execute/20050215-1     execute/20100430-1
execute/bcp-1          execute/bitfld-1       execute/bitfld-3
execute/eeprof-1       execute/pr32244-1      execute/pr34971
execute/pr43987        execute/pr58943        execute/pr77767
execute/scope-1        execute/strct-pack-2
```

and one returned a non-zero status:

```
execute/970217-1
```

Three groups stand out in that list and are where triage should start:
`bitfld-1`, `bitfld-3` and `strct-pack-2` are bit-field layout and packing;
`bcp-1`, `pr32244-1`, `pr34971` and `pr58943` are bit-field *arithmetic* and
narrow-type promotion; `eeprof-1` and `scope-1` are about linkage and scope
rather than about arithmetic. The report names every one of them, so
`CINRS_GCC_TORTURE_REPORT=1` is where the current list lives if this one has
gone stale.

## The expected-failure list

`tests/gcc-torture/expected-failures.txt`, one id per line, in the same format
the other two suites use — see
[`doc/c-testsuite.md`](c-testsuite.md#the-markers) for what `?` and `!` mean.
Guard mode skips every listed case, runs it anyway, and reports one that has
started passing so the line can go. 43 lines carry `?`, which here means "this
needs Rust 1.99": they pass on a newer toolchain and are guarded neither way.

## Reproducing the numbers

Every one of these is capped; see the memory section above.

```
# guard mode — the default, and what `cargo test` runs
( ulimit -v 8000000; timeout -k 30 3600 \
      cargo test -q --test gcc_torture -- --test-threads=2 )

# report mode: the table above, the causes and the run-time failures by name
( ulimit -v 8000000; CINRS_GCC_TORTURE_REPORT=1 timeout -k 30 3600 \
      cargo test -q --test gcc_torture -- --test-threads=2 )

# rewrite the expected-failure list from a report
( ulimit -v 8000000; CINRS_GCC_TORTURE_REPORT=1 \
      CINRS_GCC_TORTURE_UPDATE_EXPECTED=1 timeout -k 30 3600 \
      cargo test -q --test gcc_torture -- --test-threads=2 )

# one case, with the whole machinery
( ulimit -v 8000000; CINRS_GCC_TORTURE_REPORT=1 \
      CINRS_GCC_TORTURE_FILTER=bitfld-1 \
      cargo test -q --test gcc_torture -- --test-threads=2 )
```

`CINRS_GCC_TORTURE_STANDARD=gnu99|gnu11|gnu17|gnu23|c99|c11|c17|c23` picks the
entry point (default `gnu11`, which is closest to the `-std=gnu17 -w` GCC
compiles these with) and gives the list a file of its own.
`CINRS_GCC_TORTURE_STRICT=1` makes a stale entry a failure rather than a
warning. The full set is in the harness's own module documentation.
