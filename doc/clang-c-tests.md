# Conformance: Clang's C standard-conformance tests

`clang/test/C` is Clang's record of *its own* answer to the C status page: one
file per WG14 paper (`C99/n617.c`, `C23/n3042.c`) or defect report
(`drs/dr0xx.c`), each a `lit` test whose `// RUN:` line compiles it with a
particular `-std=` and whose `// expected-error {{…}}` comments say exactly
which lines must be diagnosed.

Because the file names *are* paper numbers, the result maps row by row onto
[`doc/c-status.md`](c-status.md) — which is what this harness is for. It turns
"believed to work" into "checked", and it is the only one of the three suites
that measures what `cinrs` **refuses**: the other two are whole programs that
either run or do not.

The harness is [`tests/clang_c.rs`](../tests/clang_c.rs); everything it shares
with the other two suites is in
[`tests/support/conformance.rs`](../tests/support/conformance.rs), and
[`doc/testsuites.md`](testsuites.md) has the three of them side by side.

## Fetching it

```
scripts/fetch-testsuites.sh llvm
```

The same arrangement the GCC corpus has: a **blobless, sparse checkout** of the
one directory into `third_party/llvm-project`, pinned to
`3efd20d463e1b1210d2ff07cd79b68d6fa215f24` (2026-09-04) and in `.gitignore`.
Without it the harness prints that line and exits successfully;
`CINRS_TESTSUITES_REQUIRED=1` turns the skip into a failure.

## Licence

LLVM is Apache-2.0 with LLVM exceptions. **Nothing from it is copied into this
repository**: the sparse checkout holds it, the generated `.rs` files live
under `target/`, and `tests/clang-c/expected-failures.txt` holds revision
*names* and notes written here.

## The format, and what one file becomes

Four directories are run — `C99`, `C11`, `C23` and `drs` — in that order.
`C2y` is left out: it is the *next* revision, still being drafted, and nothing
in `cinrs` claims to implement any of it.

**One RUN line is one revision.** A file may be compiled several ways —
`drs/dr0xx.c` has six RUN lines, one per revision of C — and each is a
revision of its own, with its own result and its own line in the
expected-failure list. A file with a single RUN line is named `C99/n617.c`; one
with several is `drs/dr0xx.c:3`, counting from zero.

### Mapping a RUN line onto an entry point

| `-std=` on the RUN line | entry point |
| --- | --- |
| `c89`, `c90`, `iso9899:1990` / `c99` / `c11` / `c17`, `c18`, `iso9899:2017` / `c23`, `c2x` | `c89!` / `c99!` / `c11!` / `c17!` / `c23!` |
| `gnu89`, `gnu90` / `gnu99` / `gnu11` / `gnu17`, `gnu18` / `gnu23`, `gnu2x` | `gnu89!` / `gnu99!` / `gnu11!` / `gnu17!` / `gnu23!` |
| none at all | `gnu17!`, which is what `clang -cc1` defaults to for C |
| `iso9899:199409` | **skipped** — see below |

**C95 has no entry point.** `-std=iso9899:199409` is Amendment 1 — C89 plus
`<iso646.h>`, `<wctype.h>` and a `__STDC_VERSION__` of `199409L` — and `c89!`
would answer the amendment's own questions wrongly, so it is skipped rather
than guessed at. The C89 revisions themselves are run: `c89!` gates what C99
added and `gnu89!` accepts it, which is the line these tests are drawn along.
Where a `-std=c89` revision uses a C99 feature that Clang takes as an
*extension* — a `//` comment, `_Bool`, a declaration in a `for` clause — the
strict entry point refuses it and the revision is a listed, deliberate
refusal, exactly as `_Static_assert` in a `c99!` block already was.

What else is honoured: `-verify` and `-verify=<prefixes>`, `-D` (as `#define`
lines in front of the case), `-I` (as `#pragma cinrs include_path`, with `%S`
substituted) and `-triple`. The case's **own directory** goes on that path
too, ahead of any `-I`: a quoted `#include` is looked for beside the file the
directive is written in, and the file this harness compiles is a generated one
under `target/`. What is ignored, because it changes nothing about
what is *accepted*: `-fsyntax-only`, `-pedantic`, `-pedantic-errors`, every
`-W…` and `-O…`, `-emit-llvm`, `-ast-dump`, `-E`, `-w`,
`-fno-dollars-in-identifiers`.

### The oracle

`-verify` checks every diagnostic Clang emits against the annotations. `cinrs`
has no warnings to speak of — a procedural macro has no way to raise one — so
**warnings and notes are ignored**, and what is left is the set of *lines*
carrying an `expected-error`:

* a revision with at least one of them is a **reject** case: `cinrs` must
  report at least one error on each of those lines and no error anywhere else.
  That is checked with the front end **in process** — no `rustc`, no linking,
  a hundred and thirty revisions in well under a second;
* a revision with none of them (or with `expected-no-diagnostics`) is an
  **accept** case: the front end must produce no error *and* the expansion must
  compile. That is a `//@check-pass --crate-type lib` file put through
  `ui_test`. Valid C that produces invalid Rust is a `cinrs` bug, and this is
  what finds it.

An `expected-error` in this directory is a genuine constraint violation: most
of the RUN lines pass `-pedantic-errors`, which is what turns Clang's extension
warnings into errors, and the ones that do not spell the errors out anyway.
`expected-warning` is where the extensions live, and that is exactly what is
ignored.

**Which line a directive is about** is Clang's rule and not the obvious one. A
directive written on a *continuation line* of a `/* … */` comment belongs to
the line the **comment started on**, so that a run of them under one
diagnostic all point at it:

```c
void f(struct S s) {} /* expected-warning {{…}}
                         expected-error {{…}} */
```

Both of those are about the definition, and half of `drs/` is written that way
— `drs/dr0xx.c` and `drs/dr1xx.c` most of all. Reading the second as its own
line answered those files' questions on the wrong lines and showed up as
"wrong line" against `cinrs` when nothing was wrong with `cinrs`; fixing it in
[`base_line`](../tests/clang_c.rs) took the suite from 96 of 203 to 109. A
backslash continuation follows the same rule, and `@±N`, `@LINE` and `@#name`
override both.

### The outcome classes

| class | meaning | counts as |
| --- | --- | --- |
| accepted as required | Clang wants no error, there was none, and the expansion compiled | correct |
| rejected as required (lines match) | Clang wants errors on a set of lines, and that is what came out | correct |
| false rejection | Clang wants no error and `cinrs` reported one | **correct** when the line is marked `!` — this entry point is required to refuse it — and an error of its category otherwise |
| missed rejection | Clang wants errors and `cinrs` reported none: invalid code accepted | an error |
| wrong line | errors, but not on the lines asked for | an error |
| rust compile error | valid C whose expansion `rustc` refused — always a `cinrs` bug | an error |
| skipped | not run, with a reason | neither: out of the denominator |

## Skipped revisions

73 of the 276 RUN lines are skipped rather than guessed at, each with a reason
the report prints. The 28 that used to head this list — `-std=c89` and its
spellings — are now run through `c89!` and `gnu89!`:

| revisions | reason |
| ---: | --- |
| 25 | `-ffreestanding`: `cinrs` has no freestanding mode — `__STDC_HOSTED__` is 1 and the bundled headers are the hosted ones |
| 22 | a `\| FileCheck` pipeline, and the file has a `-verify` run too; there is nothing here that can check what `FileCheck` checks |
| 5 | `-fms-extensions`, a Clang-only flag |
| 5 | not a `%clang_cc1` invocation (a `%python` line, a shell pipeline) |
| 2 | `-fexperimental-late-parse-attributes` |
| 2 | `-x c++` |
| 2 | the file is not valid UTF-8 (they are about extended characters) |
| 1 each | `-fexperimental-new-constant-interpreter`, `-fms-compatibility`, `-fno-signed-char`, `-ftrigraphs`, the input being `%t.inc` rather than `%s` |
| 5 | `-triple aarch64`, `arm`, `ppc32`, `ppc64`, `sparcv9` — not the LP64 model `cinrs` assumes |

A `| FileCheck` pipeline in a file with **no** `-verify` run anywhere is *not*
skipped: there are no annotations for anything, so the whole file is valid C
and it becomes an accept case. That is where a third of the passes come from.

Skipping for an unrecognised `-f…` or `-m…` flag is deliberate: the flag is
named in the reason, so the list of them is a to-do rather than a silent hole.

## Baseline

Measured on `rustc 1.97.1` (stable), x86_64-unknown-linux-gnu, at the pinned
corpus revision: **99 files, 276 RUN lines, 203 run, 73 skipped**, in about ten
seconds.

```
clang/test/C: 158/203 correct (77.8%) — 131 passed, 27 rejected as the standard requires
  errors: 45 — bug 22, unimplemented 7, not planned 16, toolchain 0
  (73 skipped) — 9.5 s
```

**Correct** is a revision that came out as the test asks, plus one `cinrs`
refuses *on purpose* because the entry point requires it to — a `//` comment
in a `c89!` block, `_Static_assert` in a `c99!` one. Refusing those is
conforming behaviour, so counting them as shortfalls measures the wrong thing;
[`doc/testsuites.md`](testsuites.md#what-correct-means-and-the-four-kinds-of-error)
is where the rule and the four error categories are set out.

| directory | run | correct | rate | bug | unimplemented | not planned | revisions | skipped |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `C99` | 30 | 26 | **86.7 %** | 2 | 0 | 2 | 37 | 7 |
| `C11` | 23 | 20 | **87.0 %** | 1 | 2 | 0 | 30 | 7 |
| `C23` | 45 | 32 | **71.1 %** | 5 | 4 | 4 | 69 | 24 |
| `drs` | 105 | 80 | **76.2 %** | 14 | 1 | 10 | 140 | 35 |

The C23 row is the honest one: `c23!` implements the parts of C23 the README
lists and not the rest, and this directory is one file per C23 paper.

### Why the revision number is the lowest of the three suites

**77.8 % here does not mean 22 % of the C is wrong.** Three things compress it,
and none of them is a translation error:

1. **A revision is all-or-nothing.** One error on one line, out of the forty
   annotations `drs/dr0xx.c` carries, and the whole revision is a mismatch.
   The [annotation rate](#annotations-the-other-half-of-the-picture) below is
   the same run measured per *line*, and it is much higher.
2. **A file is five revisions.** `drs/dr0xx.c`, `dr1xx.c`, `dr2xx.c`,
   `dr3xx.c` and `dr4xx.c` are compiled once per revision of C, so a single
   cause is counted five times. Collapsing the cascades leaves **41 distinct
   root causes** behind the 72 listed revisions — 21 of them conforming
   refusals — and the report prints them with their counts.
3. **27 of the 72 are conforming refusals**, which the correct rate above
   already counts as correct rather than as gaps.

Put together: 45 errors in **20 root causes**, of which 8 are bugs.

### Annotations: the other half of the picture

The report counts the `expected-error` *lines* as well as the revisions:

```
  annotations, over the 203 revisions run
      620  lines carry a required `expected-error`
      463  of them were diagnosed (74.7%)
      157  were not
      552  errors landed on a line no directive names
           162 of those are on the 27 revisions this entry point is required
           to refuse, where every later revision's feature is one of them
```

620 lines are asked about and **463 are answered on the right line**. The last
row is what an earlier entry point costs rather than a count of wrong answers:
a `c89!` revision of a C23 paper refuses every C99 and C11 construct in the
file, and Clang — which takes each as an extension and only warns — names none
of them. 162 of the 552 are on the 27 revisions that are conforming refusals
outright; the other 390 are on the 45 error revisions, where the same effect
piles up behind whichever refusal came first.

### The errors, by category

45 revisions do not come out as the test asks and are not deliberate refusals.
Every one is in `tests/clang-c/expected-failures.txt` with a category and a
one-line cause, and a note of the form ``see `<id>`` says "same reason as that
one", which is how the report collapses the cascades.

**`[bug]` — 22 revisions, 8 root causes.** These are the work items:

| root cause | revisions |
| --- | ---: |
| DR011: the composite type a block-scope `extern int i[10];` gives an object is scoped to that block, and `cinrs` scopes it to the function (`drs/dr0xx.c`) | 5 |
| `__LLONG_WIDTH__` is not predefined — `cinrs` has GCC's `__LONG_LONG_WIDTH__` spelling and not Clang's — so `drs/dr2xx.c`'s `#if`/`#elif` on it both fail and the file's own `#error` fires | 5 |
| DR103: a tag declared in a *parameter list* has the scope of that list, and `cinrs` puts it in the enclosing scope (`drs/dr1xx.c`) | 4 |
| C23 type inference is not taken when a *storage-class* specifier precedes the `auto`: `static auto c = 1UL;` (`C23/n3007.c`, `n3006.c`) | 2 |
| 6.7.3p9: `const int a[1]` qualifies the *element* type, and `cinrs` qualifies the object (`C23/n2607.c`) | 2 |
| a universal character name is validated where the token is used rather than where it is lexed, so one inside a macro argument that expands to nothing is never diagnosed (`C99/n717.c`) | 2 |
| an enumeration redeclared with a different fixed underlying type is not diagnosed (`C23/n3030.c`) | 1 |
| the parse recovery from a `_Static_assert` in a parameter list emits a second, spurious error (`C11/n1330.c`) | 1 |

**`[unimplemented]` — 7 revisions, 5 root causes.** C23 tag compatibility
(N3037, 2 revisions: `C23/n3037_1.c` and `drs/dr1xx.c:4`); the C11 and C23
identifier-character tables, which `cinrs` has as one rule for every revision
(2, `C11/n1518.c`); `\N{…}` named universal character escapes (1); an empty
initializer for a variable length array (1); a `constexpr` object of an array
type (1).

**`[not-planned]` — 16 revisions, 8 root causes.** Nothing here is a to-do:

* `drs/dr4xx.c` (5): a compound literal as the operand of `_Static_assert`.
  Clang folds it as a documented GNU extension and *warns*; ISO C does not
  make it an integer constant expression, and `cinrs` refuses it.
* `drs/dr3xx.c` (5): the file-scope compound literal of variably modified
  type on line 226, which `cinrs` diagnoses and Clang's own comment calls a
  FIXME for not diagnosing — plus the C99 features its `c89!` and `c99!`
  revisions meet first.
* `C99/n448.c` and `C99/n809.c` (2): a `_Static_assert` gate, and a
  `-verify` directive inside an `#if __STDC_VERSION__ >= 202311L` that the
  revision does not compile. Clang's `-verify` never sees a directive in a
  skipped conditional; this harness, which reads them out of the raw text,
  does.
* one each: `_BitInt`'s `wb` constants (🔴 in
  [`doc/c-status.md`](c-status.md)); the line number of a macro invocation
  spanning spliced lines, which the paper leaves unspecified and Clang's own
  comment calls a FIXME; `C23/n2900_n3011.c:1`, an empty initializer in a
  `c17!` block; and `C23/n3033.c`, a `-E … | FileCheck` test whose
  *expansions* are not a translation unit.

**Missed rejection (0).**

### The deliberate refusals (27, marked `!`)

These are not gaps, and guard mode asserts that the refusal is still there.

* *A later revision's feature in an earlier block* (23): `_Static_assert` and
  `_Alignof` in a `c99!` block, an anonymous `struct` member in `c99!`, a
  binary constant in `c17!`, a label at the end of a compound statement in
  `c11!`, an enumerator too wide for `int` in `c17!` — and, since the C89
  revisions started running, a `//` comment, `_Bool`, a declaration in a `for`
  clause, a designated initializer and `_Static_assert` in a `c89!` block.
  Clang takes each as an *extension* and warns (`-Wc99-extensions`,
  `-Wc11-extensions`, `-Wc23-extensions`), which these RUN lines silence or do
  not promote — an error only under `-pedantic-errors`. `cinrs` gates them on
  the entry point instead, and says which macro to write.
* *`_Complex` in a `c89!` block* (1, `drs/dr206.c:0`), which is the same rule
  as the row above: C99 added the type and the strict C89 entry point says so.
  The other four revisions of that file, and `C99/n809_2.c`, `n809_3.c`,
  `C11/n1460.c` and `C11/n1464.c` — `__builtin_complex`, which is the *paper*
  N1464 is about — all came out as required once the complex types landed.
* *Clang-only builtins* (3): `__builtin_bit_cast`.

### How the number has moved

The pass rate went *down* once, when the C89 revisions started running — 79 of
175 (45.1 %) became 85 of 203 (41.9 %) — and that was what one should expect:
the 28 revisions that joined were 6 more passes and 22 more mismatches, most of
them a `-std=c89` RUN line using a C99 feature that Clang takes as an extension
and `c89!` refuses on purpose. Those are the `!` entries above, and counting
them as correct is what took the headline from 64.5 % to 77.8 % without
changing a line of the compiler. Before that, trigraphs and designator lists
took it from 41.9 % to 47.3 %, the round after that to 123 of 203 — 13 of those
are the directive-line rule above, which was the harness reading Clang's
annotations wrongly rather than anything `cinrs` did, and the other 14 are the
fixes listed at the end of this document — and `_Complex` took it to 131.

`drs/dr3xx.c` used to be a harness problem and is not any more: the file writes
`#include "./abc_123.h"`, which is looked for beside the file the directive is
written in, and the file this harness compiles is a *generated* one under
`target/`. Every case's own directory now goes on the search path, exactly as
the gcc-torture harness has always done, so the header is found and the file's
five revisions have moved on to a later refusal apiece.

### What the results changed in `doc/c-status.md`

Fifteen rows have moved as a result of running this suite. The one from the
latest round:

* **The complex types** (N620, N638, N657, N694, N809) and **`CMPLX`** (N1464).
  Seven revisions were deliberate refusals resting on `__STDC_NO_COMPLEX__`
  and are now required passes: both papers' own files, `C99/n809_2.c`,
  `n809_3.c` and four of the five revisions of `drs/dr206.c`. The fifth is a
  `c89!` block, where `_Complex` is still a C99 feature in an earlier entry
  point. `C99/n809.c` itself is still a wrong-line mismatch, on the
  `_Static_assert` it uses rather than on anything about complex.

The one from the round before:

* **An enumeration's fixed underlying type may be written `_Atomic`**, and the
  underlying type is then the unqualified, non-atomic one (C23 6.7.2.2p5,
  `C23/n3030_1.c`). The file used to be a deliberate refusal, because
  `_Atomic` was; now that atomics are implemented it comes out as required.

The eight from the round before that, each with the revision that asked for it:

* **`restrict` is checked** against C99 6.7.3p2 — it may only qualify a
  pointer to an object type (`C99/n448.c`).
* **`offsetof` takes a nested member designator** and folds to an integer
  constant (`C23/n2350.c`, seven revisions).
* **An incomplete array type** is a type: `extern int j[];`, a tentative
  `int j[];` completed to one element at the end of the unit, and `sizeof` of
  one refused until it is (`drs/dr011.c`, five revisions).
* **A cast to the type the operand already has** is a no-op for a `struct` or
  `union` too, as GCC and Clang both have it (`C11/n1285.c`, five revisions).
* **Every label** may stand at the end of a compound statement and before a
  declaration, `case` and `default` included (`C23/n2508.c`).
* **An enumerator too wide for `int`** widens the enumeration (`C23/n3029.c`).
* **`<stdckdint.h>`** is bundled (`C23/n2683_2.c`).
* **A `__VA_OPT__` argument may not begin or end with `##`** (C23 6.10.5.2p1),
  which was the one *missed rejection* in the suite (`C23/n3033_2.c`).
* **A parameter of a declaration that is not a definition may have an
  incomplete type**, and a `typedef` of `void` as the only parameter is an
  empty prototype (DR157, `drs/dr157.c`, five revisions).

and the five from before:

* **`__FILE__` in an `#include`** now works. A quoted include whose name is a
  path is looked for from the working directory, which is what a header that
  includes itself by `__FILE__` needs; see `include::resolve`.
* **A line splice inside a token** now works. Translation phase 2 deletes a
  backslash-newline before the source is tokenised, so `__LI\<newline>NE__` is
  one identifier — `drs/dr464.c` is that test, and `C99/n590.c` is where it
  matters.
* **The GCC predefined limit and type macros** (`__INT_MAX__`,
  `__SIZE_TYPE__`, `__INTMAX_MAX__`, the `__INTn_TYPE__` family, the
  `__FLT_*`/`__DBL_*` values) are defined. They were not, and a program that
  tests one and finds it undefined does not fail to compile — it silently
  takes the wrong branch.
* **Selection and iteration statements are blocks** (C99 6.8.4p3, 6.8.5p5), so
  a tag declared in a controlling expression no longer leaks into the
  enclosing block. `C99/block-scopes.c` is that test.
* **A `-verify=` prefix may hold a dash of its own.** `C23/n2940.c` writes
  `no-trigraphs-error@-1`, and the annotation reader stopped at the *last*
  dash, so it answered the `trigraphs` revisions' question in the
  `no-trigraphs` ones and vice versa. Reading the whole prefix took the file
  from 0 of 10 to 10 of 10 and the suite from 41.9 % to 47.3 %; nothing else
  in the corpus writes a hyphenated prefix.

The second of those two harness bugs was
[the directive-line rule](#the-oracle): a directive on a continuation line of
a `/* … */` comment belongs to the line the comment opened on, and reading it
as its own line was answering half of `drs/` on the wrong lines.

## Memory and thread safety

The same four ceilings the other two suites have, keyed to
`CINRS_MEMORY_LIMIT_MB` — see
[`doc/gcc-torture.md`](gcc-torture.md#memory-and-thread-safety-and-how-to-run-the-suite)
for what they are and
[`tests/harness_safety.rs`](../tests/harness_safety.rs) for the tests that
provoke them. This suite is the cheapest of the three (about seventy `rustc`
invocations, and the reject half never leaves the process), but it is run the
same way:

```
( ulimit -v 8000000; timeout -k 10 1800 \
      cargo test -q --test clang_c -- --test-threads=2 )
```

Measured that way, a full guard run takes **10 s** and peaks at **351 MiB**
of resident set — most of which is `cargo` checking that the `cinrs`
dependency is up to date rather than anything the suite does.

## Reproducing the numbers

```
# guard mode — the default, and what `cargo test` runs
( ulimit -v 8000000; timeout -k 10 1800 \
      cargo test -q --test clang_c -- --test-threads=2 )

# report mode: the tables above, the skips and every mismatch
( ulimit -v 8000000; CINRS_CLANG_C_REPORT=1 timeout -k 10 1800 \
      cargo test -q --test clang_c -- --test-threads=2 )

# rewrite the expected-failure list from a report
( ulimit -v 8000000; CINRS_CLANG_C_REPORT=1 CINRS_CLANG_C_UPDATE_EXPECTED=1 \
      timeout -k 10 1800 cargo test -q --test clang_c -- --test-threads=2 )

# one paper
( ulimit -v 8000000; CINRS_CLANG_C_REPORT=1 CINRS_CLANG_C_FILTER=n590 \
      cargo test -q --test clang_c -- --test-threads=2 )
```

`CINRS_CLANG_C_STRICT=1` makes a stale expected-failure entry a failure rather
than a warning.
