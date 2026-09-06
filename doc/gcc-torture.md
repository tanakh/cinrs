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
**580 of the 1,769** need them.

**`c89!` and `gnu89!` get them left out**, because those two entry points
implement the rule the cases are leaning on: a call to an undeclared `abort`
declares `extern int abort();` on the spot, and it becomes the same
`#[link_name]` extern the prelude's declaration would have. Under every other
entry point the two lines go in, and `CINRS_GCC_TORTURE_PRELUDE=0` or `=1`
overrides the default either way — which is how the 580 was measured. The
`include_path` is there because about thirty cases `#include` a sibling `.c`
or `.h` out of the corpus directory, which is not where the generated `.rs`
file lives.

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
| `-std=gnu89` | 75 | the whole suite can be run through `gnu89!`, and the second baseline below is that run; the default `gnu11!` gives them a later revision than they asked for |
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
than the built-in cap does. Measured that way, a full run takes **4 m 35 s**
and peaks at **298 MiB** of resident set.

## Baseline

Measured on `rustc 1.97.1` (stable), x86_64-unknown-linux-gnu, at the pinned
corpus revision, under the two entry points worth pointing at this corpus:
`gnu89!`, which is the language these programs were actually written in, and
`gnu11!`, the harness default and the closest thing here to the
`-std=gnu17 -w` GCC compiles them with.

| entry point | `execute` | `execute/ieee` | total | rate |
| --- | ---: | ---: | ---: | ---: |
| **`gnu89!`** | 1401/1691 (82.9 %) | 62/78 (79.5 %) | **1463/1769** | **82.7 %** |
| `gnu11!` | 1305/1691 (77.2 %) | 62/78 (79.5 %) | 1367/1769 | 77.3 % |

**`gnu89!` is what this corpus should be measured with**, and what to reach
for when compiling C of that era: it is `gnu99!` plus the three rules a later
revision deleted — implicit `int`, implicit function declarations and (with
every entry point below `c23!`) old-style definitions — which is exactly the
set of things seventy-five of these cases ask for with `-std=gnu89` and
another few hundred simply assume. It also needs no
[prelude](#the-prelude): a call to an undeclared `abort` declares it.

7 cases are not generated at all under either: five want the effective target
`run_expensive_tests`, one holds a carriage return (which a Rust raw string
literal may not), and one is not valid UTF-8.

### The failures, by cause

Under `gnu89!`, 302 of the 306 failures are refused at compile time, in 33
distinct causes. The ones worth a line each, with what the same cause costs
under `gnu11!` beside it:

| `gnu89!` | `gnu11!` | cause | e.g. |
| ---: | ---: | --- | --- |
| — | 98 | `type specifier missing` — implicit `int`, which `gnu89!` has | `execute/20000717-3` |
| 67 | 67 | inline assembly is not supported | `execute/20001009-2` |
| 44 | 44 | `vector_size` / `__vector_size__`: the vector extensions need an unstable Rust feature | `execute/20050316-1` |
| 38 | 38 | variadic function *definitions*, which need Rust 1.99's `c_variadic` | `execute/20030914-2` |
| 29 | 23 | `expected ';' after declaration, found '{'` — a nested function definition, which is a GNU extension this crate does not have | `execute/20000822-1` |
| 21 | 21 | `_Complex` | `execute/20010605-2` |
| 18 | 18 | a `__builtin_…` this crate does not implement: `__builtin_return_address`, `frame_address`, `setjmp`, `longjmp`, `apply`, `apply_args`, `shuffle`, `va_arg_pack`, and the `_FloatN` spellings `__builtin_nansf32` and its relatives | `execute/20010122-1` |
| 14 | 14 | `va_arg` with a struct type | `execute/920625-1` |
| 11 | 11 | `expected expression, found '&&'` — computed `goto` | `execute/20040302-1` |
| 8 | 8 | `va_list` in a context that needs Rust 1.99 | `execute/20000519-1` |
| 6 | 6 | `va_list` somewhere other than a local or a parameter | `execute/stdarg-1` |
| 6 | 6 | a struct member with a variably modified type, which C forbids (6.7.2.1p9) and GCC takes as an extension | `execute/20020412-1` |
| 5 | 5 | a `#include` of a corpus file outside the sparse checkout (`../../gcc.dg/…`) | `execute/pr105777` |
| 5 | 5 | `__attribute__((scalar_storage_order))`, which reverses the byte order of every scalar in a record | `execute/20230630-2` |
| 4 | 4 | an initialised flexible array member | `execute/20010924-1` |
| 3 | 3 | `__attribute__((alias))` | `execute/alias-2` |
| 3 | 3 | a record both packed and given a stricter alignment | `execute/20040308-1` |
| — | 3 | a K&R parameter with no declaration, which is implicit `int` again | `execute/930429-2` |
| — | 3 | implicit declaration of a function, which `gnu89!` has | `execute/20000412-3` |

The remaining causes have two cases or fewer each; the report prints all
thirty-three. The `__builtin_…` count is the sum of two rows the report prints
separately, because a diagnostic raised inside an `#include`d corpus file
carries the file name and is grouped on its own.

The last round of work took `gnu89!` from 1402 to 1463 and `gnu11!` from 1307
to 1367 — 61 cases and 60, and **no case went the other way** in either.
Where they came from (the `gnu89!` column; `gnu11!` gains the same set but
`execute/20031211-2`, which needs implicit `int` as well):

| cases | what landed |
| ---: | --- |
| 29 | the floating classification and comparison builtins: `__builtin_isnan`, `isinf`, `isinf_sign`, `isfinite`, `isnormal`, `issignaling`, `signbit`, `fpclassify`, `isunordered`, `isgreater(equal)`, `isless(equal)`, `islessgreater`, `fabs`, `copysign`, the `l` forms of `inf`/`huge_val`, the NaN payloads, and `__builtin_classify_type` |
| 16 | `__builtin_printf`, `sprintf` and `snprintf` declaring the function themselves, the way GCC does |
| 6 | `__attribute__((mode(M)))` — the other three cases that use it meet inline assembly or `vector_size` behind it |
| 5 | one-case code-generation bugs: a constant subscript too large for an `i32`, `x << -64`, a no-op function-pointer transmute in a `static`, `"foo" + 1` in one, and a call through a declaration that a later prototyped definition completed |
| 3 | `<signal.h>` |
| 1 | a cast to a union type |
| 1 | an over-long string initialiser, which GCC warns about and truncates |

Two of the four-or-fewer rows are worth naming, because they are the only two
compile failures that are not a *gap*: `execute/medce-1` and `ieee/fp-cmp-7`
call a `link_error()` that nothing defines, and the whole point of each case is
that an optimising compiler must delete the call — `if (0) { link_error(); case
1: … }` in one, `if (x > __builtin_inf())` with `x` folded to `1.0` in the
other. Nothing here optimises, so the call survives and the link fails. They
would pass under `-O`, and asking `rustc` for that would change what the whole
suite measures.

**What the C89 rules are worth here is the first row and the last two.** 104
cases — a quarter of every compile failure under `gnu11!` — are implicit `int`,
an implicit function declaration or a K&R parameter with no declaration, and
under `gnu89!` they are not diagnostics at all: 96 of them build and run,
which is the whole difference between the two lines of the table, and the rest
meet a second gap behind the first. The Rust 1.99 variadic gap (38 + 8) comes
next, and those simply pass on a newer toolchain, which is why they are marked
`?` in both expected-failure lists. What is left after that is the honest list
of what `cinrs` does not implement.

### The programs that built and then did the wrong thing

**Four** cases compile, run and fail. These are the interesting ones — a
miscompilation, or a semantic nothing diagnosed and nothing implemented — and
there were fifteen of them until they were triaged one by one. Thirteen were
bugs and are fixed; the four that are left are not bugs in the translation:

| case | verdict |
| --- | --- |
| `execute/20021127-1` | the case *defines* `long long llabs(long long)` as a function that aborts, and requires the compiler to expand the builtin inline rather than call it. Defining a standard library function is undefined behaviour (7.1.3p2), and `cinrs` calls what the program defined. |
| `execute/20101011-1` | installs a `SIGFPE` handler and divides by zero, requiring the *hardware* to trap. Integer division by zero is undefined in C and a panic in Rust, so the generated program aborts before the handler can run. It reached this point only once `<signal.h>` was bundled; before that it was a compile failure. |
| `execute/builtin-types-compatible-p` | requires `__builtin_types_compatible_p(long double, double)` and two distinct anonymous `enum`s to answer *no*. Both answer yes here, and both are documented mappings rather than bugs: `long double` **is** `double` (no portable Rust type has an x87 extended double's layout), and an untagged `enum` **is** `int`. Every other question in the file, the `int[5]` against `int[]` one included, is answered as GCC answers it. |
| `execute/eeprof-1` | needs `-finstrument-functions`, so that every function calls `__cyg_profile_func_enter` and `_exit` around its body. No entry point can ask for it, and the harness passes no options. |

Everything else on the list is fixed. Each of the thirteen was a real bug, and
each has a regression test of its own next to the fix:

| cases | what was wrong |
| --- | --- |
| `bitfld-1` | a cast of a bit-field to its own declared type was elided as a no-op, so `(unsigned int) x.u` took part in arithmetic as the `int` the width-restricted promotions give a bare `x.u`. `tests/bitfields.rs` |
| `bitfld-3`, `pr32244-1`, `pr34971` | a bit-field wider than `int` keeps its declared type through the promotions, but its value keeps the declared *width*, and C99 6.7.2.1p10 makes that width the precision the arithmetic happens in: `unsigned long long b : 40` multiplies, adds and shifts in forty bits. It was computing in sixty-four. `tests/bitfields.rs`, and the differential corpus in `tests/bitfield_layout.rs` now probes it against the host compiler. |
| `strct-pack-2`, `20100430-1`, `pr43987` | an object reached through a packed member — or through a pointer cast that lands on one, which is what punning a `char` buffer to a record does — was loaded and stored as if it were aligned. That is undefined behaviour in Rust and an abort in a debug build; such a place now goes through `read_unaligned` and `write_unaligned`. `tests/gnu_attributes.rs` |
| `20050215-1` | `__attribute__((aligned(N)))` written after the declarator of a `typedef` of an anonymous record was dropped. The typedef name is the only way to name such a record, so the alignment is given to the record itself. `tests/gnu_attributes.rs` |
| `970217-1`, `pr77767` | the size expressions of an array parameter of a *definition* were never evaluated, and C99 6.9.1p10 evaluates them on entry: `void f(int n, int a[n++])` increments `n`. `tests/execute.rs` |
| `scope-1` | `extern int v;` in a block bound to the block-scope `int v` that shadowed the file-scope one. A block-scope object declared without `extern` has no linkage at all, so it says nothing about what the `extern` names (6.2.2p4). `tests/execute.rs` |
| `pr58943` | `x \|= f()` was written out as `x = x \| f()`, which reads `x`, calls `f` and only then stores. C11 6.5.16.2p3 makes the read-modify-write a *single* evaluation with respect to an indeterminately sequenced call, so the right operand is now evaluated into a temporary first. `tests/execute.rs` |
| `bcp-1` | `__builtin_constant_p` said no to the address of a string literal, and to a character read out of one at a constant index. GCC says yes to both. `tests/gnu_builtins.rs` |
| `pr79286` | a constant subscript too large for an `i32` was written out as a suffixed `u64` literal, and `<*mut T>::offset` takes an `isize`. `tests/execute.rs` |
| `pr98681` | a *negative* constant shift count came out as `-64 as u32`, which `rustc` reads as the negation of a `u32` (`E0600`). Such a count is undefined in C; it is now reduced to the `u32` the shift methods take, which is what the hardware does with it. `tests/execute.rs` |
| `921110-1`, `pr53084` | two `static` initialisers that needed an `unsafe` block and did not get one: a function pointer whose C type differed from another the generated Rust cannot tell apart (the transmute between them is now elided, being a no-op), and `"foo" + 1`, whose `.offset(1)` is an unsafe call however safe the string literal's address is. |
| `pr103209` | a call through `int *h();` that a later `int *h(unsigned, int)` completed: the call was checked against the type in scope where it stands, which has no prototype, and code generation looked the *final* signature up instead and passed the arguments straight to it. The reinterpretation now compares the argument types as well as their number. |

The report names all four remaining cases, so `CINRS_GCC_TORTURE_REPORT=1` is
where the current list lives if this one has gone stale.

## The expected-failure list

One list per entry point: `tests/gcc-torture/expected-failures.txt` is
`gnu11!`'s, 402 entries, and `expected-failures-gnu89.txt` is `gnu89!`'s, 306.
One id per line, in the same format the other two suites use — see
[`doc/c-testsuite.md`](c-testsuite.md#the-markers) for what `?` and `!` mean.
Guard mode skips every listed case, runs it anyway, and reports one that has
started passing so the line can go. 46 lines of each of them
carry `?`, which here means "this needs Rust 1.99": they pass on a newer
toolchain and are guarded neither way, and the harness marks them itself from
the wording of the diagnostic, so a regenerated list keeps them.

An update *keeps* the note a line already has, so that a hand-written one
survives; when a fix changes what a still-failing case fails on, the way to
refresh every note at once is to delete the file and regenerate it, which is
what the numbers above were measured with.

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

`CINRS_GCC_TORTURE_STANDARD=gnu89|gnu99|gnu11|gnu17|gnu23|c89|c99|c11|c17|c23`
picks the entry point (default `gnu11`, which is closest to the `-std=gnu17 -w`
GCC compiles these with) and gives the list a file of its own — so the second
baseline above is

```
( ulimit -v 8000000; CINRS_GCC_TORTURE_REPORT=1 \
      CINRS_GCC_TORTURE_STANDARD=gnu89 timeout -k 30 3600 \
      cargo test -q --test gcc_torture -- --test-threads=2 )
```

and guard mode reads `tests/gcc-torture/expected-failures-gnu89.txt` when it is
given the same variable. `CINRS_GCC_TORTURE_STRICT=1` makes a stale entry a
failure rather than a warning. The full set is in the harness's own module
documentation.
