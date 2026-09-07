# Conformance: c-testsuite

*One of three suites; [`doc/testsuites.md`](testsuites.md) is the overview, and
has the memory rules a run has to be given.*

[c-testsuite](https://github.com/c-testsuite/c-testsuite) is a collaborative
database of C compiler test cases. `cinrs` runs its `single-exec` suite through
any of its entry points — `c89!`, `c99!`, `c11!`, `c23!` and the GNU dialects
`gnu89!`, `gnu99!`, `gnu11!` and `gnu23!` — so that "how much of C does the front end
actually get right" is a number that a test run can hold on to rather than an
impression.

The harness is `tests/c_testsuite.rs`; the corpus is a git submodule at
`third_party/c-testsuite`, pinned to
`5c7275656d751de0e68b2d340a95b5681858ed07` (2020-03-09).

## Fetching it

```
git submodule update --init third_party/c-testsuite
```

Without it the harness prints that line and exits successfully, so a checkout
without the submodule still has a green `cargo test`. Set
`CINRS_CTESTSUITE_REQUIRED=1` to turn the skip into a failure — which is what a
CI job that means to run the suite wants.

## Licence

The corpus's own `LICENSE` covers "all testing software, but not for
individual test cases": MIT, © 2018 Andrew Chambers. `tests/LICENSE` says that
the licence of each *case* is discoverable from its `.otags` file, which names
the project the case came from (`org=`, `repository=`, `version=`, `path=`) —
tinycc, GCC's torture tests, and others. Nothing from the corpus is copied
into this repository: the submodule holds it, and the generated files that
embed a case live in `target/`, which is not checked in.

## What is in the suite

220 cases, `tests/single-exec/NNNNN.c`, each a whole program with a `main`,
plus:

* `NNNNN.c.expected` — what the program must print. The corpus's own runner
  redirects stdout *and* stderr into one file and diffs that, and requires the
  program to exit 0. 66 of the 220 print anything at all; none of them writes
  to stderr, so the harness compares `.expected` against stdout and requires
  stderr to be empty, which says the same thing in the two files `ui_test`
  knows how to compare.
* `NNNNN.c.tags` — one tag per line. The whole vocabulary in use is

  | tag | cases | meaning |
  | --- | ---: | --- |
  | `portable` | 220 | the program should work anywhere |
  | `c89` | 174 | valid C89 (and so C99 and C11) |
  | `c99` | 43 | needs C99 |
  | `c11` | 2 | needs C11 |
  | `needs-cpp` | 98 | leans on the preprocessor |
  | `needs-libc` | 63 | leans on the C library |

  The corpus also documents `arch-…` tags; no case in this revision carries
  one, and every case is `portable`. One case (`00216`) has no revision tag at
  all.

Everything else worth knowing before running them: no case reads standard
input, two spell `main(int argc, char **argv)` (only one of the two is live —
the other is inside an `#ifndef` its own `#define` has already made false), and
one (`00187`) writes a file relative to the working directory. The headers they
include are `<stdio.h>` (61 cases), `<stdlib.h>`, `<string.h>`, `<stdint.h>`,
`<stdarg.h>`, `<math.h>` and `<wchar.h>` — all bundled with `cinrs`.

## Running it

**Guard mode** is the default, and is what `cargo test` runs:

```
cargo test --test c_testsuite
```

Every selected case that is not listed in
`tests/c-testsuite/expected-failures.txt` is generated and has to pass; a
failure is reported by `ui_test` exactly as a failure in `tests/ui` is, with
the compiler's diagnostics or the stdout diff. The listed ones are generated
into a directory of their own and run as well, but only so that one which has
*started* passing is reported and its line can be deleted. That report is a
warning; `CINRS_CTESTSUITE_STRICT=1` makes it a failure. A `!` entry is the
exception: it is *asserted* rather than skipped, and the assertion failing is a
failure whatever `CINRS_CTESTSUITE_STRICT` says. See
[the markers and the category tag](#the-markers-and-the-category-tag) below.

**Report mode** runs everything, fails nothing and prints the numbers:

```
CINRS_CTESTSUITE_REPORT=1 cargo test --test c_testsuite
```

and, to write what it found back into the expected-failure list:

```
CINRS_CTESTSUITE_REPORT=1 CINRS_CTESTSUITE_UPDATE_EXPECTED=1 \
    cargo test --test c_testsuite
```

which keeps the note *and the category tag* already written against an id that
still fails, writes a new one in as `[bug]  classify me: <the one-line
classification>`, and leaves every `?` and `!` line exactly as it stands.

| variable | effect |
| --- | --- |
| `CINRS_CTESTSUITE_REQUIRED=1` | a missing corpus is a failure, not a skip |
| `CINRS_CTESTSUITE_STANDARD=c89\|c99\|c11\|c23\|gnu89\|gnu99\|gnu11\|gnu23` | which entry point to translate with, and which cases are eligible; default `c99`. `c90` is another spelling of `c89` |
| `CINRS_CTESTSUITE_FILTER=<substring>` | only the cases whose id contains it |
| `CINRS_CTESTSUITE_REPORT=1` | report mode |
| `CINRS_CTESTSUITE_STRICT=1` | a stale expected-failure entry is a failure — a broken `!` assertion is one either way |
| `CINRS_CTESTSUITE_UPDATE_EXPECTED=1` | rewrite the list from the report, keeping every `?` and `!` line |
| `CINRS_CTESTSUITE_TIMEOUT=<seconds>` | per-case timeout, 20 by default; `0` disables it |

The expected-failure list for `c99!` is
`tests/c-testsuite/expected-failures.txt`; every other entry point has one of
its own — `expected-failures-c89.txt`, `-c11.txt`, `-c23.txt`, `-gnu89.txt`,
`-gnu99.txt`, `-gnu11.txt` and `-gnu23.txt` — because a case that needs C11
fails under `c99!` and passes under `c11!` and one list cannot say both. The
format is one id per line — a marker, the id, a category tag and a note — with
`#` for a comment; see [the section below](#the-markers-and-the-category-tag).

### The markers and the category tag

A marker in front of the id says what kind of claim the line is making, and a
plain line — the only kind that records something *wrong* — carries a category
tag saying what kind of wrong. The categories are the same four in all three
suites and are explained once, in
[`doc/testsuites.md`](testsuites.md#what-correct-means-and-the-four-kinds-of-error).

| line | means |
| --- | --- |
| `NNNNN  [bug]  <note>` | `cinrs` is wrong here. |
| `NNNNN  [unimplemented]  <note>` | A feature `cinrs` means to have and has not got to yet. |
| `NNNNN  [not-planned]  <note>` | Deliberately unsupported; nothing to fix. |
| `?NNNNN  <note>` | It passes or fails depending on the *toolchain*. |
| `!NNNNN  <note>` | This entry point makes the case **invalid**, so refusing it is conforming behaviour and not an error at all. |

A `!` line may name the diagnostic the case has to be refused with, written in
front of the note as `error: "<substring>"`:

```
!00209  error: "too many arguments to function call"  conforming: C23 removed …
```

The substring is plain text, matched against everything the compiler wrote;
there is no escape, so it cannot itself contain a `"`.

| | plain | `?` | `!` |
| --- | --- | --- | --- |
| **guard** | skipped; a warning (a failure under `CINRS_CTESTSUITE_STRICT=1`) if it starts passing | run, but guarded neither way | **asserted**: the case must fail to *compile*, and the output must contain the substring where one is given. One that builds and runs, or is refused in the wrong words, is a failure whatever `CINRS_CTESTSUITE_STRICT` says |
| **report** | counted as an error of its category, listed under it | counted as a `toolchain` error when it fails here | counted as **correct**, under `rejected as the standard requires` |
| **update** | kept, tag and note and all; a *new* failure is written in as `[bug]  classify me: …` | kept as it stands | kept as it stands; never downgraded to a plain failure, and a case that stopped being refused is warned about rather than deleted |

`00140` is the `?` entry: it defines a variadic function, which needs Rust
1.99, so it passes on a new enough compiler and not on an older one. The `!`
entries are `00209` under `c23!` and `gnu23!`, `00219` under `c99!` — and 21
more under `c89!`, which is [the row](#the-c89-row-which-is-the-corpuss-own-tag-being-generous)
where a strict C89 entry point refuses what C99 added.

## Which cases are selected

Two rules, both out of the corpus's own tags:

* **It has to run on this machine.** `portable`, or an `arch-…` tag naming the
  host. A case with neither kind of tag makes no claim and is kept. Every case
  in this revision is `portable`, so nothing is excluded here today.
* **It must not need a newer revision of C than the entry point.** A `cNN`
  tag names the *oldest* revision the program is valid in — the corpus
  documents `c89` as implying `c99` and `c99` as implying `c11` — so a
  `c89`-tagged case runs everywhere and a `c11`-tagged one only from `c11!`
  up. `c89!` therefore takes the 175 cases that ask for nothing later than
  C89, `c99!` takes 218 of the 220 and leaves the two `c11`-tagged ones, and
  `c11!` and `c23!` take all 220. A GNU dialect accepts everything a later
  revision added, so `gnu89!` and `gnu99!` take all 220 as well.

Nothing is excluded by the other two tags. `needs-cpp` is fine — `cinrs` has
the whole C99 preprocessor — and so is `needs-libc`, since the bundled headers
declare the platform's real library and the calls link against it. A case that
needs something `cinrs` does not have, such as `00204`'s `va_arg` of a `struct`
the x86-64 System V ABI passes on the stack, is left in and *fails*, so that it
shows up in the count instead of being quietly filtered out of it.

## How a case becomes a Rust file

`target/c-testsuite/<standard>/NNNNN.rs`, a fresh directory per run:

```rust
//@run
//@edition: 2024

mod unit {
    cinrs::c99! { r#"<the C source, verbatim>

#pragma cinrs module "ctest"
"# }
}

fn main() {
    // chdir into target/c-testsuite-work/<standard>/NNNNN
    // spawn the watchdog
    let status = unsafe { unit::ctest::main() };
    std::process::exit(status as i32);
}
```

with `NNNNN.run.stdout` next to it, copied from `.expected`.

* The C goes in a **raw string literal** — string-literal input is the form
  that accepts every C token, and the corpus has hexadecimal floating
  constants and `'ab'` in it. The number of `#` is computed from the source, so
  the literal cannot terminate early.
* The `#pragma cinrs module "ctest"` is **appended**, because the preprocessor
  acts on it wherever it reads it and appending leaves every line number of the
  original alone — a diagnostic then points at the line the upstream file has.
  A blank line goes in front of it, so that a source ending in a backslash
  continuation splices with that and not with the pragma.
* The expansion is wrapped in a **`mod unit`** of its own. The unit exports a
  function called `main`, and glob re-exporting that into the crate root next
  to the harness's own `fn main` is a warning — which would then have to be
  blessed into a `.stderr` file for all 220 cases.
* The **signature of `main`** is read off the text, after comments and string
  literals are blanked out: `int main()` and `int main(void)` are called with
  no arguments, `int main(int argc, char **argv)` with a fake `argv` of
  `["ctest", NULL]`, matching how the corpus runs its cases. The first
  declaration wins, which is what makes `00182` — whose second `main` is inside
  a false `#ifndef` — come out right. A `main` that falls off the end returns
  0, since the translation appends a zeroed return, which is what C99 says
  `main` does.
* `//@run` expects exit status 0, which is the corpus's convention for every
  case; none of them expects a non-zero one.
* The programs are compiled with `-Cstrip=symbols`: 220 unstripped binaries
  are most of a gigabyte and the same 220 stripped are eighty megabytes.

## Timeouts and memory

A miscompiled program may never come back — or may allocate until there is
nothing left — and `ui_test` bounds neither. Four things keep that from wedging
a `cargo test`, and they are the same four every one of the three suites has;
[`doc/testsuites.md`](testsuites.md#the-safety-rules) is where they are
described, and `CINRS_MEMORY_LIMIT_MB` (8192 by default) is the one number they
all work to.

The two this suite spells with a prefix of its own:

* the generated program starts a **watchdog thread** that prints a line and
  exits 124 after `CINRS_CTESTSUITE_TIMEOUT` seconds (20 by default), which
  covers a program that loops forever, and exits 137 — reported as
  `runtime: out of memory` — when its own resident set passes the ceiling;
* the compiler is invoked through **`timeout(1)`** (five minutes, `-k 5`) and
  under a **`ulimit -v`**, which covers a macro expansion that never comes back
  or that asks for everything.

`CINRS_CTESTSUITE_TIMEOUT=0` disables the clock and `CINRS_MEMORY_LIMIT_MB=0`
the ceilings. A program killed by a signal — a stack overflow, an `abort` —
needs neither: its exit status is not zero, so it is an ordinary run failure,
reported as `runtime: killed by signal N`.

This suite is 220 short programs and takes about six seconds, so it is the one
run that does not really need the outer cap; run it with one anyway, for the
same reason the other two must be:

```
( ulimit -v 8000000; timeout -k 10 1800 \
      cargo test -q --test c_testsuite -- --test-threads=2 )
```

## Baseline

Measured on `rustc 1.98.1` (stable), x86_64-unknown-linux-gnu, at the pinned
corpus revision.

**Correct** is passed plus rejected-as-required; see
[`doc/testsuites.md`](testsuites.md#what-correct-means-and-the-four-kinds-of-error)
for why that is the number to lead with and what the four error categories
are.

| entry point | selected | correct | rate | passed | rejected | errors |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `c89!` | 175 | **173** | **98.9 %** | 152 | 21 | 2 |
| `c99!` | 218 | **214** | **98.2 %** | 213 | 1 | 4 |
| `c11!` | 220 | **216** | **98.2 %** | 216 | — | 4 |
| `c23!` | 220 | **217** | **98.6 %** | 216 | 1 | 3 |
| `gnu89!` | 220 | **217** | **98.6 %** | 217 | — | 3 |
| `gnu99!` | 220 | **217** | **98.6 %** | 217 | — | 3 |
| `gnu11!` | 220 | **217** | **98.6 %** | 217 | — | 3 |
| `gnu23!` | 220 | **217** | **98.6 %** | 216 | 1 | 3 |

**There is not a single `bug` or `not planned` error in this corpus, under any
entry point.** Every error is one of the same four cases:

| entry point | bug | unimplemented | not planned | toolchain |
| --- | ---: | ---: | ---: | ---: |
| `c89!` | 0 | 1 (`00213`) | 0 | 1 (`00140`) |
| `c99!`, `c11!` | 0 | 3 (`00204`, `00213`, `00216`) | 0 | 1 (`00140`) |
| `c23!` and the GNU dialects | 0 | 2 (`00204`, `00213`) | 0 | 1 (`00140`) |

*rejected* is the `!` category: a case this entry point is *required* to
refuse. It stays in the denominator — it is one of the cases the run looked
at — and it counts as correct, because refusing it is the right answer. The
report mode summary line says the same thing:

```
c-testsuite / single-exec through `c23!`: 217/220 correct (98.6%) — 216 passed, 1 rejected as the standard requires
  errors: 3 — bug 0, unimplemented 2, not planned 0, toolchain 1
```

Per tag, under `c23!` (a run that selects everything and has a rejection in
it):

| tag | correct | rate | unimplemented | toolchain |
| --- | ---: | ---: | ---: | ---: |
| `portable` | 217/220 | 98.6 % | 2 | 1 |
| `c89` | 172/174 | 98.9 % | 1 | 1 |
| `c99` | 42/43 | 97.7 % | 1 | — |
| `c11` | 2/2 | 100 % | — | — |
| `needs-cpp` | 97/98 | 99.0 % | 1 | — |
| `needs-libc` | 62/63 | 98.4 % | 1 | — |

### The `c89!` row, which is the corpus's own tag being generous

`c89!` is the entry point that *refuses* the most, and **21 of its 23
non-passes are cases that use something a later revision added** — twenty of
them tagged `c89`, and the twenty-first (`00216`) tagged with no revision at
all: nine mix declarations and code, four write `long long`, three end an
enumerator list with a comma, two define a variadic macro, and one each uses a
variable length array, a compound literal and `_Generic`. The tag says the
program is meant to be portable, not that it is strict C90, and running it
through the entry point that *means* strict C90 is what shows the difference —
which is the useful thing this row measures.

Every one of those 21 is an `!` line naming the diagnostic it has to be
refused with, so guard mode asserts the refusal rather than tolerating a
failure, and they count as **correct**: a strict C89 entry point that accepted
`long long` would be the news. That is why the row reads 173/175 (98.9 %)
rather than the 152/175 (86.9 %) it did when a conforming refusal was filed
next to a gap. The two errors that are left are `00140` (the Rust 1.99
variadic gap) and `00213`, which every entry point fails.

`gnu89!` is the same corpus with those gates switched off, and it passes
216/220 — exactly what `gnu99!` and `gnu11!` pass. That is the point of the
dialect: `gcc -std=gnu89` takes all of the above as extensions too.

`00140` is the one case whose result depends on the compiler: it *defines* a
variadic function, which needs Rust 1.99, so it passes on beta and nightly and
fails on 1.98.1. The tables above count it as a `toolchain` error; on 1.99 it
is a pass, and every row goes up by one — 174/175 (99.4 %) for `c89!`, 215/218
(98.6 %) for `c99!` and 217/220 (98.6 %) for the other six.

The GNU dialects select all 220 cases and pass the same ones as their ISO
counterparts: nothing in the corpus needs a *plain*-spelled GNU keyword, so
switching the dialect on buys eligibility rather than passes — except for
`gnu89!`, where what it buys is the twenty-one cases `c89!` gates.

### The errors, by category

Four errors under `c99!` and `c11!` and three under `c23!` and the GNU
dialects, all of them at compile time, and **none of them a `cinrs` bug**: two
or three are unimplemented and one is the toolchain. The earlier measurements'
failures — `00110`, `00149`, `00150`,
`00152`, `00159`, `00200`, `00207`, `00209`, `00218`, `00219` and `00220` —
are fixed and have regression tests of their own, and so are the six the GNU
extensions closed (`00095`, `00170`, `00206`, `00210` and `00214`, plus
`00209`'s incomplete `enum`). `00207` is the newest of them: it declares a
variable length array in a function that also uses `goto`, which is now
translated (`tests/vla.rs` covers the same shape).

**`[unimplemented]` (3 under `c99!` and `c11!`, 2 elsewhere).**

| case | what it needs |
| --- | --- |
| `00204` | `va_arg` of a `struct` **larger than sixteen bytes**. The case is a deliberate ABI stress test, and most of what it does now works — `va_arg` of a `struct` is [implemented](../src/lib.rs) for the one to two eightbytes the x86-64 System V ABI passes in registers. What is left is its homogeneous float aggregates of three and four `double`s and of `long double`, which are twenty-four and thirty-two bytes and go into the overflow area, where nothing in Rust's stable `va_list` can reach them. (`long double` is mapped to `double` here in any case, so the case's `%Lf` conversions would print the wrong thing even if the size rule let it through.) |
| `00213` | a `goto` out of a statement expression. Whether a function is lowered through a [control-flow graph](../crates/cinrs-core/src/cfg.rs) is decided from its *statements*, so a jump buried in an expression is refused rather than dropped. The other thing this case writes — a `?:` one of whose operands is `void` — now works; see [`doc/gnu-extensions.md`](gnu-extensions.md). |
| `00216` | the C23 empty initialiser `{}`, which the case writes as `(empty_s){}` — so it fails under `c99!` and `c11!` and **passes** under `c23!` and every GNU dialect. Its *other* gap, initialising a flexible array member, is closed: GCC allows it for an object with static storage duration and so does this now; see [`doc/gnu-extensions.md`](gnu-extensions.md). |

**`[toolchain]` (1).** `00140` defines a variadic function, which needs Rust
1.99.

**`[bug]` (0), `[not-planned]` (0).** Nothing in this corpus is either.

The cases that want a *later entry point* — `00219`'s `_Generic` under `c99!`,
and twenty more under `c89!` — are not errors at all: refusing them is what
those entry points owe the standard, and they are counted under
[rejected as the standard requires](#rejected-as-the-standard-requires) below.

### Rejected as the standard requires

Not a failure of anything, and counted as correct. Each of these is an `!`
entry naming the diagnostic it has to be refused with, so the harness
*asserts* the refusal rather than tolerating a failure — if `cinrs` ever
started accepting one, `cargo test` would fail. That is the point of the
marker: the other entries record what the compiler cannot yet do, and these
record what it must not do.

**`00209`, under `c23!` and `gnu23!`.** It calls an `int (*fp)();` with an
argument. That is C89 through C17, and `cinrs` supports it there; C23 removed
function declarators without a prototype (N2841), so `fp` takes no parameters
and the call has one argument too many. `gcc -std=c23` refuses the case in the
same words.

```
!00209  error: "too many arguments to function call"  conforming: C23 removed …
```

**`00219`, under `c99!` and `c89!`.** It uses `_Generic`, which C11 added, and
is tagged `c89` by a corpus that means "portable" rather than "strict C90".
Every entry point from `c11!` up passes it, and so does every GNU dialect.

**Twenty more, under `c89!` only** — nine mixing declarations and code, four
writing `long long`, three ending an enumerator list with a comma, two
defining a variadic macro, and one each with a variable length array, a
compound literal and (with `00219`) `_Generic`. A strict C89 entry point is
required to refuse all of them, and each line names the diagnostic:

```
!00200  error: "'long long' requires C99 or later"  conforming: `long long` is C99, …
```

## Reproducing the numbers

```
cargo test --test c_testsuite                                   # guard, c99
CINRS_CTESTSUITE_REPORT=1 cargo test --test c_testsuite          # report, c99
CINRS_CTESTSUITE_REPORT=1 CINRS_CTESTSUITE_STANDARD=c11 \
    cargo test --test c_testsuite                                # report, c11
CINRS_CTESTSUITE_REPORT=1 CINRS_CTESTSUITE_STANDARD=c23 \
    cargo test --test c_testsuite                                # report, c23
CINRS_CTESTSUITE_REPORT=1 CINRS_CTESTSUITE_STANDARD=gnu99 \
    cargo test --test c_testsuite                                # report, gnu99
```

A full run of all 220 cases — generate, compile, execute, diff — takes about
six seconds on a 32-thread machine.
