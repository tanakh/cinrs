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
shares with the other two suites — the modes, the expected-failure list with its
markers and category tags, what "correct" means, the result collector, the
ceilings — is in
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
**580 of the 1,769 that are run** need them.

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
than the built-in cap does. Measured that way, a full guard run takes
**4 m 50 s** and peaks at **298 MiB** of resident set.

## Baseline

Measured on `rustc 1.98.1` (stable), x86_64-unknown-linux-gnu, at the pinned
corpus revision, under the two entry points worth pointing at this corpus:
`gnu89!`, which is the language these programs were actually written in, and
`gnu11!`, the harness default and the closest thing here to the
`-std=gnu17 -w` GCC compiles them with.

**Correct** is passed plus refused-as-the-standard-requires; the errors are
broken down into the four categories
[`doc/testsuites.md`](testsuites.md#what-correct-means-and-the-four-kinds-of-error)
defines.

```
gcc.c-torture/execute through `gnu11!`: 1516/1769 correct (85.7%) — 1412 passed, 104 rejected as the standard requires
  errors: 253 — bug 0, unimplemented 3, not planned 189, toolchain 61
  (7 not generated) — 4 m 03 s
```

| entry point | correct | rate | passed | rejected | bug | unimplemented | not planned | toolchain |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| **`gnu11!`** | **1516/1769** | **85.7 %** | 1412 | 104 | 0 | 3 | 189 | 61 |
| `gnu89!` | 1509/1769 | 85.3 % | 1509 | — | 0 | 4 | 195 | 61 |

On **`beta`**, where a variadic definition compiles, the sixty-one `toolchain`
entries pass instead: 1574/1769 (89.0 %) under `gnu11!` and 1567/1769 (88.6 %)
under `gnu89!`. That gap is the whole of the difference between the two
toolchains, which is why those lines carry `?` and are guarded neither way.

by group:

| group | `gnu11!` | `gnu89!` |
| --- | ---: | ---: |
| `execute` | 1452/1691 (85.9 %) | 1445/1691 (85.5 %) |
| `execute/ieee` | 64/78 (82.1 %) | 64/78 (82.1 %) |

**There is no `[bug]` left in this corpus**, under either entry point.
Everything that does not pass is a feature not implemented yet, a feature
deliberately not planned, or the Rust 1.99 variadic gap.

The last one was `execute/20020227-1`, whose expansion `rustc` refused with
`E0793`: the case compares a `__complex__ float` member of a
`__attribute__((packed))` record against a constant, and the runtime's complex
type derives `PartialEq`, whose `eq` takes `&self` — a reference to a field of
a packed record, which is exactly what `E0793` is about. Reading such a field
*by value* is fine, so the comparison goes through a by-value
`cinrs_rt::complex::eq_f32`/`ne_f32` instead of Rust's own `==`.

### The two entry points, and why `gnu11!` now scores higher

**`gnu89!` is the language these programs were written in**, and what to reach
for when compiling C of that era: it is `gnu99!` plus the three rules a later
revision deleted — implicit `int`, implicit function declarations and (with
every entry point below `c23!`) old-style definitions — which is exactly the
set of things seventy-five of these cases ask for with `-std=gnu89` and
another few hundred simply assume. It also needs no
[prelude](#the-prelude): a call to an undeclared `abort` declares it. Under it
1509 cases build and run.

**`gnu11!` gets the same 1412 of those, refuses 104 more as the standard
requires it to, and comes out seven ahead.** Those 104 are the cases that lean
on a rule C99 deleted — implicit `int` (101: 98 on a declaration, 3 on a K&R
parameter) and an implicit function declaration (3) — and a C99-or-later entry
point is *required* to refuse them. `gcc -std=gnu11` refuses them too, and has
since GCC 14 made both constraint violations errors. Each is an `!` line in
`tests/gcc-torture/expected-failures.txt` naming the diagnostic it must be
refused with, so guard mode asserts the refusal rather than tolerating a
failure: a case that started *compiling* would be the news.

Ninety-seven of the 104 pass under `gnu89!`; the other seven get past the C89
rule there and stop at a second gap — four at a nested function that needs the
enclosing *frame* (two at a nonlocal `goto` out of one, two at the address of a
label of the enclosing function), one at a nested function that uses the
enclosing function's variable length array, and two at `<setjmp.h>`. That is
the whole of the difference: 1509 = 1412 + 97, and 1516 = 1412 + 104.

7 cases are not generated at all under either: five want the effective target
`run_expensive_tests`, one holds a carriage return (which a Rust raw string
literal may not), and one is not valid UTF-8. They are out of the denominator,
because GCC's own runner would not have run them either.

### The errors, by category and cause

Under `gnu11!` 248 of the 253 errors are refused at compile time, in 29
distinct causes, and 5 are programs that built and then did the wrong thing;
under `gnu89!`, 255 of 260 in 30. The ones worth a line each, with what the
same cause costs under `gnu89!` beside it — the 104 conforming rejections
above are *not* in this table:

| category | `gnu89!` | `gnu11!` | cause | e.g. |
| --- | ---: | ---: | --- | --- |
| not planned | 68 | 68 | inline assembly is not supported | `execute/20001009-2` |
| toolchain | 51 | 51 | variadic function *definitions*, which need Rust 1.99's `c_variadic` | `execute/20030914-2` |
| not planned | 44 | 44 | `vector_size` / `__vector_size__`: the vector extensions need an unstable Rust feature | `execute/20050316-1` |
| not planned | 19 | 19 | a `__builtin_…` this crate does not implement: `__builtin_return_address`, `frame_address`, `setjmp`, `longjmp`, `apply`, `apply_args`, `shuffle`, `va_arg_pack`, `issignaling`, and the `_FloatN` spellings `__builtin_nansf32` and its relatives | `execute/20010122-1` |
| toolchain | 10 | 10 | `va_list` — a `va_list *` included — in a context that needs Rust 1.99 | `execute/20000519-1` |
| not planned | 7 | 7 | a complex *integer* type — `_Complex int`, `__complex__ char`, `3i` — which is a GNU extension of its own with no Rust counterpart | `execute/20041124-1` |
| not planned | 6 | 6 | `va_list` somewhere other than a local or a parameter | `execute/stdarg-1` |
| not planned | 6 | 6 | a struct member with a variably modified type, which C forbids (6.7.2.1p9) and GCC takes as an extension | `execute/20020412-1` |
| not planned | 5 | 5 | a `#include` of a corpus file outside the sparse checkout (`../../gcc.dg/…`) | `execute/pr105777` |
| not planned | 5 | 5 | `__attribute__((scalar_storage_order))`, which reverses the byte order of every scalar in a record | `execute/20230630-2` |
| not planned | 7 | 4 | a **nonlocal `goto`**: a jump out of a nested function to a label of the enclosing one, which GCC reaches through the enclosing frame | `execute/nestfunc-5` |
| not planned | 5 | 5 | a program that built and then did the wrong thing — see [below](#the-programs-that-built-and-then-did-the-wrong-thing) | `ieee/cdivchkd` |
| not planned | 3 | 3 | `__attribute__((alias))` | `execute/alias-2` |
| not planned | 3 | 3 | a record both packed and given a stricter alignment | `execute/20040308-1` |
| not planned | 2 | — | `<setjmp.h>`, which cinrs does not bundle: nothing in Rust unwinds a C `longjmp`. Under `gnu11!` both cases stop at the C89 implicit `int` first, and are conforming rejections there | `execute/pr56982` |
| not planned | 2 | 2 | `<sys/mman.h>`: cinrs bundles the ISO C headers and never searches the platform's include path | `execute/loop-2f` |
| not planned | 2 | 2 | the **address of a nested function that uses the enclosing frame**, which is what GCC's trampoline is for | `execute/20000822-1` |
| not planned | 1 | 1 | a `link_error()` nothing defines, which an optimiser is required to delete | `ieee/fp-cmp-7` |
| not planned | 2 | 1 | the address of a label of the **enclosing** function — `&&label` inside a nested one, which is the nonlocal jump one step earlier | `execute/920721-4` |
| unimplemented | 1 | 1 | a label inside a statement expression, which nothing outside it could jump to | `execute/930406-1` |
| unimplemented | 1 | 1 | an initialised flexible array member in a *nested* context: GCC takes one inside a `union` whose other member already makes the object large enough. Every other shape of the extension is [implemented](gnu-extensions.md) | `execute/pr28865` |
| unimplemented | 1 | 1 | **`va_arg` of a `struct` larger than sixteen bytes**, which the x86-64 System V ABI passes on the stack — the overflow area, which nothing in Rust's stable `va_list` reaches. Every smaller record now works; see [`doc/c-status.md`](c-status.md) | `execute/va-arg-22` |
| not planned | 1 | 1 | `sizeof(va_list)`, which the case `malloc`s by: Rust's `VaList` is a value with a lifetime rather than a size | `execute/va-arg-21` |
| not planned | 1 | 1 | an incompatible pointer argument — `execute(&p)` with `p` a `short[5]` — which C makes a constraint violation and GCC only warned about until GCC 14 | `execute/920302-1` |
| unimplemented | 1 | — | a nested function that uses the enclosing function's **variable length array** | `execute/921017-1` |
| not planned | 1 | 1 | `__attribute__((aligned))` on a *function* | `execute/align-3` |
| not planned | 1 | 1 | `__attribute__((weak))`, which needs Rust's unstable `#[linkage]` | `execute/20030125-1` |

That is every cause the report groups; the ones with a single case each are at
the bottom.
The `__builtin_…` count is the sum of two rows the report prints separately,
because a diagnostic raised inside an `#include`d corpus file carries the file
name and is grouped on its own; the `vector_size` and
address-of-a-nested-function counts are sums of two for the same reason.

The last round of work was **[relooping the control-flow
graph](translation.md#control-flow-and-goto)**, and its effect on this corpus is
one case: `execute/medce-1`, whose `if (0) { link_error(); case 1: … }` really is
an `if false` in the output now, so LLVM deletes the call and the link succeeds.
What it changed everywhere else is the *shape* of the output: of the 44
functions in this corpus whose jumps need the graph, 25 now come out as Rust
loops and `match`es and 19 keep the machine over block numbers — every one of
those 19 a computed `goto`, whose `&&label` *is* a block's number. The corpus
measures that only by continuing to pass, which is the point of running it.

The round before that was **`va_arg` of a `struct`**. It moves **12** cases
out of `[unimplemented]` — `920625-1`, `920908-1`, the seven `931004-*`,
`pr44575`, `strct-stdarg-1` and `strct-varg-1`, every one of them a `struct` of
at most sixteen bytes read back out of an argument list — and into `?`, because
what is left in their way is only the Rust 1.99 variadic gate: on `beta` they
pass, which is the whole of the 85.7 % → 89.0 % difference between the two
toolchains. Two more stopped at a second gate rather than moving: `va-arg-22`
passes records of up to thirty-one bytes, which the ABI puts on the stack, and
`va-arg-pack-1` reaches `__builtin_va_arg_pack` and is now `[not-planned]` with
the rest of the unimplemented builtins.

Before that — **labels as values**, an **alignment specifier on an
object**, **pointers to `va_list`** and an **initialised flexible array
member** — took `gnu11!` up by **14** and `gnu89!` by **14**, and no case went
the other way in either:

| cases | what landed |
| ---: | --- |
| 10 | the computed `goto`: `execute/20040302-1`, `20071210-1`, `920501-3` (`goto *(&&base + *ip++)`), `920501-4`, `920501-5`, `980526-1`, `990208-1`, `comp-goto-1`, `pr71494` and `pr70460` — the last of them on GCC's **label difference**, `static int b[] = { &&lab1 - &&lab0, … }` |
| 3 | the initialised flexible array member: `execute/20010924-1` (from a string literal), `20030109-1` (`.z = {}`) and `pr33382` |
| 1 | `_Alignas(16)` on a file-scope array: `execute/pr68532` |

Two more moved from `[unimplemented]` to `?`: `execute/pr64979` and
`20041214-1` pass a `va_list *` around and now get as far as the Rust 1.99
gate, which means they pass on a newer toolchain. Three others got past the
gate that was hiding them and stopped at a second one — `920302-1` at an
incompatible pointer argument, `va-arg-21` at `sizeof(va_list)`, and
`pr28865` at a flexible array member initialised inside a `union`.

The round before *that* took each entry point up by **one**, and it was the
corpus's last `[bug]`: `execute/20020227-1` compares a `__complex__ float`
member of a packed record, and Rust's own `==` on the runtime's complex type
borrows the field to do it. See [the baseline](#baseline).

The round before *that* — **nested functions**, lifted out of the function they
were written in — took `gnu89!` up by **15** and `gnu11!` by **14**, and
**no case went the other way** in either:

| cases | what landed |
| ---: | --- |
| 14 | the shapes that lift: `execute/20010209-1` (a non-capturing nested function called with the enclosing VLA), `20010605-1` (`inline` on one), `20030501-1`, `20040520-1`, `20090219-1`, `920612-2` (reading and writing an enclosing local), `931002-1` (the address of a *non*-capturing one), `nest-align-1`, `nestfunc-1`, `nestfunc-2`, `nestfunc-7` (a `struct` returned from one), `pr103405`, `pr22061-3` and `pr22061-4` (a parameter whose bound is a captured variable) |
| 1 | `execute/921215-1`, which `gnu89!` alone selects |

Fourteen of the corpus's nested-function cases still fail — nine under
`gnu11!`, which counts five of the fourteen as conforming rejections instead —
each for a reason the lifting cannot reach:

* the **address** of a function that uses the enclosing frame, which is what
  GCC's trampoline is for: `20000822-1`, and `nestfunc-3`, where the function
  whose address is taken needs the frame only because of a sibling it calls;
* a **nonlocal `goto`**: `nestfunc-5`, `nestfunc-6`, `pr24135`, `pr51447`, and
  under `gnu89!` also `920428-2`, `920501-7` and `comp-goto-2`;
* the **address of a label of the enclosing function**, which is the same jump
  one step earlier: `920721-4`, and under `gnu89!` also `920415-1`;
* inline assembly, which stops `20061220-1` before anything else does;
* a nested function that uses the enclosing function's **variable length
  array**: `921017-1`, under `gnu89!`;
* and a **variadic** nested function, which needs Rust 1.99 like any other
  variadic definition: `nest-stdar-1`.

The round before *those* — `_Complex` — took both entry points up by **15**:

| cases | what landed |
| ---: | --- |
| 13 | the complex types themselves: `execute/20010605-2`, `20020411-1`, `20030910-1`, `20070614-1`, `960512-1`, `complex-1`, `-2`, `-4`, `-5`, `-7`, `pr38969`, `pr42248`, `pr49644` |
| 2 | `ieee/cdivchkf` and `ieee/cdivchkld`, GCC's accuracy checks for the `float` and `long double` quotients |

Seven cases that used to fail on `_Complex` still fail, on the *complex
integer* types behind it — `_Complex int`, `__complex__ char`, `3i` — which
are a separate GNU extension. One more, `execute/20020227-1`, got past the
front end and onto the packed-member reference that the complex diagnostic had
been hiding, which is the one the round above closed. `ieee/cdivchkd` is the
one case that compiles, runs and gives an answer GCC would not; see the
section below.

The round before *those* took `gnu89!` from 1402 to 1463 and `gnu11!` from 1307
to 1367:

| cases | what landed |
| ---: | --- |
| 29 | the floating classification and comparison builtins: `__builtin_isnan`, `isinf`, `isinf_sign`, `isfinite`, `isnormal`, `issignaling`, `signbit`, `fpclassify`, `isunordered`, `isgreater(equal)`, `isless(equal)`, `islessgreater`, `fabs`, `copysign`, the `l` forms of `inf`/`huge_val`, the NaN payloads, and `__builtin_classify_type` |
| 16 | `__builtin_printf`, `sprintf` and `snprintf` declaring the function themselves, the way GCC does |
| 6 | `__attribute__((mode(M)))` — the other three cases that use it meet inline assembly or `vector_size` behind it |
| 5 | one-case code-generation bugs: a constant subscript too large for an `i32`, `x << -64`, a no-op function-pointer transmute in a `static`, `"foo" + 1` in one, and a call through a declaration that a later prototyped definition completed |
| 3 | `<signal.h>` |
| 1 | a cast to a union type |
| 1 | an over-long string initialiser, which GCC warns about and truncates |

One of the four-or-fewer rows is worth naming, because it is the only compile
failure that is not a *gap*: `ieee/fp-cmp-7` calls a `link_error()` that nothing
defines, and the whole point of the case is that an optimising compiler must
delete the call — `if (x > __builtin_inf()) link_error();` with `x` a parameter.
LLVM does not fold that far through the translation, so the call survives and
the link fails. Its twin `execute/medce-1` — `if (0) { link_error(); case 1: … }`
— used to fail the same way and now passes: with the control-flow graph
[relooped](translation.md#control-flow-and-goto) the dead call really is inside
an `if false`, which LLVM deletes.

**Read the table by its first column.** 131 of `gnu11!`'s 253 errors are the
three largest `not planned` rows — inline assembly, the vector extensions and
the `__builtin_…` forms nobody is going to write — and another 13 are the
complex-integer and `va_list`-in-a-record corners Rust has no counterpart for.
61 more are the Rust 1.99 gap, which simply passes on a newer toolchain and is
marked `?` in both lists. **Three** are the honest list of what is not
implemented yet — a label inside a statement expression, a flexible array
member initialised inside a `union`, and a `va_arg` of a `struct` too large for
the registers. None is a bug.

The 104 C89-rule cases — implicit `int`, an implicit function declaration, a
K&R parameter with no declaration — are not in the table at all, because under
`gnu11!` refusing them *is* the right answer. Under `gnu89!` they are not
diagnostics at all and 97 of them build and run.

### The programs that built and then did the wrong thing

**Five** cases compile, run and fail. These are the interesting ones — a
miscompilation, or a semantic nothing diagnosed and nothing implemented — and
there were fifteen of them until they were triaged one by one. Thirteen were
bugs and are fixed; the five that are left are not bugs in the translation, and
each is a `[not-planned]` line for the reason in its row:

| case | verdict |
| --- | --- |
| `ieee/cdivchkd` | GCC's own accuracy check for `double _Complex` division: four quotients whose operands' exponents are hundreds apart, chosen because they are where a careless implementation loses its bits. Two of the four are within what `cinrs_rt::complex::div_f64` gives; the other two need `libgcc`'s power-of-two prescaling of *both* operands, which Smith's algorithm — even with Baudin and Smith's subnormal-ratio refinement — does not do. C leaves the accuracy of complex arithmetic implementation-defined (Annex G.6), so this is a quality gap and not a conformance one. The `float` and `long double` cases beside it, `cdivchkf` and `cdivchkld`, pass: the `float` quotient is computed in the wider format, where no scaling is needed at all. |
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

The report names all five remaining cases, so `CINRS_GCC_TORTURE_REPORT=1` is
where the current list lives if this one has gone stale.

## The expected-failure list

One list per entry point: `tests/gcc-torture/expected-failures.txt` is
`gnu11!`'s, 357 entries, and `expected-failures-gnu89.txt` is `gnu89!`'s, 260.
One id per line, in the same format the other two suites use — a marker, the
id, a category tag and a note; see
[`doc/testsuites.md`](testsuites.md#what-correct-means-and-the-four-kinds-of-error)
for what the categories mean and what `?` and `!` say. Guard mode skips every
listed case, runs it anyway, and reports one that has started passing so the
line can go. 61 lines of each list carry `?`, which here means "this needs Rust
1.99": they pass on a newer toolchain and are guarded neither way, and the
harness marks them itself from the wording of the diagnostic, so a regenerated
list keeps them. 104 lines of the `gnu11!` list carry `!`.

An update *keeps* the note and the category a line already has, so that a
hand-written one survives, and writes a new failure in as
`[bug]  classify me: …` rather than guessing. When a fix changes what a
still-failing case fails on, the way to refresh every note at once is to delete
the file and regenerate it — which throws every category away too, so
everything comes back as a bug waiting to be classified.

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
