# Conformance: c-testsuite

[c-testsuite](https://github.com/c-testsuite/c-testsuite) is a collaborative
database of C compiler test cases. `cinrs` runs its `single-exec` suite through
any of its entry points — `c99!`, `c11!`, `c23!` and the GNU dialects
`gnu99!`, `gnu11!` and `gnu23!` — so that "how much of C does the front end
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
`<stdarg.h>`, `<math.h>` and `<wchar.h>` — all bundled with `cinrs` except the
last.

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
warning; `CINRS_CTESTSUITE_STRICT=1` makes it a failure.

**Report mode** runs everything, fails nothing and prints the numbers:

```
CINRS_CTESTSUITE_REPORT=1 cargo test --test c_testsuite
```

and, to write what it found back into the expected-failure list:

```
CINRS_CTESTSUITE_REPORT=1 CINRS_CTESTSUITE_UPDATE_EXPECTED=1 \
    cargo test --test c_testsuite
```

which keeps the note already written against an id that still fails, and adds
the one-line classification as the note for a new one.

| variable | effect |
| --- | --- |
| `CINRS_CTESTSUITE_REQUIRED=1` | a missing corpus is a failure, not a skip |
| `CINRS_CTESTSUITE_STANDARD=c99\|c11\|c23\|gnu99\|gnu11\|gnu23` | which entry point to translate with, and which cases are eligible; default `c99` |
| `CINRS_CTESTSUITE_FILTER=<substring>` | only the cases whose id contains it |
| `CINRS_CTESTSUITE_REPORT=1` | report mode |
| `CINRS_CTESTSUITE_STRICT=1` | a stale expected-failure entry is a failure |
| `CINRS_CTESTSUITE_UPDATE_EXPECTED=1` | rewrite the list from the report |
| `CINRS_CTESTSUITE_TIMEOUT=<seconds>` | per-case timeout, 20 by default; `0` disables it |

The expected-failure list for `c99!` is
`tests/c-testsuite/expected-failures.txt`; every other entry point has one of
its own — `expected-failures-c11.txt`, `-c23.txt`, `-gnu99.txt`, `-gnu11.txt`
and `-gnu23.txt` — because a case that needs C11 fails under `c99!` and passes
under `c11!` and one list cannot say both. The format is one id per line with a
note after it and `#` for a comment; an id written `?NNNNN` passes or fails
depending on the *toolchain* and is guarded neither way — `00140` defines a
variadic function, which needs Rust 1.99.

## Which cases are selected

Two rules, both out of the corpus's own tags:

* **It has to run on this machine.** `portable`, or an `arch-…` tag naming the
  host. A case with neither kind of tag makes no claim and is kept. Every case
  in this revision is `portable`, so nothing is excluded here today.
* **It must not need a newer revision of C than the entry point.** `c89` and
  `c99` both mean "C99 is enough" (the corpus documents `c89` as implying
  `c99`, and `c99` as implying `c11`), so `c99!` takes 218 of the 220 and
  leaves the two `c11`-tagged ones; `c11!` and `c23!` take all 220. A GNU
  dialect accepts everything a later revision added, so `gnu99!` takes all 220
  as well.

Nothing is excluded by the other two tags. `needs-cpp` is fine — `cinrs` has
the whole C99 preprocessor — and so is `needs-libc`, since the bundled headers
declare the platform's real library and the calls link against it. A case that
needs something `cinrs` does not have, such as `00220`'s `<wchar.h>`, is left
in and *fails*, so that it shows up in the count instead of being quietly
filtered out of it.

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

## Timeouts

A miscompiled program may never come back, and `ui_test` has no per-test
timeout. Two things keep that from wedging a `cargo test`:

* the generated program starts a **watchdog thread** that prints a line and
  exits 124 after `CINRS_CTESTSUITE_TIMEOUT` seconds (20 by default), which
  covers a program that loops forever;
* the compiler is invoked through **`timeout(1)`** where there is one (five
  minutes, `-k 5`), which covers a macro expansion that never comes back.

`CINRS_CTESTSUITE_TIMEOUT=0` disables both. A program killed by a signal — a
stack overflow, an `abort` — needs neither: its exit status is not zero, so it
is an ordinary run failure, reported as `runtime: killed by signal N`.

## Baseline

Measured on `rustc 1.97.1` (stable), x86_64-unknown-linux-gnu, at the pinned
corpus revision.

| entry point | selected | passed | rate |
| --- | ---: | ---: | ---: |
| `c99!` | 218 | 209 | **95.9 %** |
| `c11!` | 220 | 212 | **96.4 %** |
| `c23!` | 220 | 212 | **96.4 %** |
| `gnu99!` | 220 | 212 | **96.4 %** |
| `gnu11!` | 220 | 212 | **96.4 %** |
| `gnu23!` | 220 | 212 | **96.4 %** |

Per tag, under `c11!` (the run that selects everything):

| tag | passed | rate |
| --- | ---: | ---: |
| `portable` | 212/220 | 96.4 % |
| `c89` | 169/174 | 97.1 % |
| `c99` | 41/43 | 95.3 % |
| `c11` | 2/2 | 100 % |
| `needs-cpp` | 93/98 | 94.9 % |
| `needs-libc` | 59/63 | 93.7 % |

`00140` is the one case whose result depends on the compiler: it *defines* a
variadic function, which needs Rust 1.99, so it passes on beta and nightly and
fails on 1.97.1. The tables above count it as a failure; on 1.99 the rows are
210/218 (96.3 %) for `c99!` and 213/220 (96.8 %) for every entry point that
selects all 220.

The GNU dialects select all 220 cases and pass the same 212 as `c11!`: nothing
in the corpus needs a *plain*-spelled GNU keyword, so switching the dialect on
buys eligibility rather than passes.

### The failures, by cause

Eight cases fail under `c11!`, all of them to compile, and `c99!` adds `00219`,
which wants a later entry point. None of them is a `cinrs` bug: the seven
earlier measurements found — `00110`, `00149`, `00150`, `00159`, `00200`,
`00218` and `00219` — are fixed and have regression tests of their own, and so
are the six the GNU extensions closed (`00095`, `00170`, `00206`, `00210` and
`00214`, plus `00209`'s incomplete `enum`).

**Something `cinrs` has not implemented (6).**

| case | what it needs |
| --- | --- |
| `00204` | `va_arg` with a struct type |
| `00207` | a variable length array |
| `00209` | a function declarator with *no prototype* — `int (*fp)();` called with an argument. C23 removed the form and `cinrs` applies that rule in every mode, so `int f()` is `int f(void)` and the call has one argument too many. |
| `00213` | a `goto` out of a statement expression, and a `?:` one of whose operands is `void`. The `goto` is the hard half: whether a function is lowered through a [control-flow graph](../crates/cinrs-core/src/cfg.rs) is decided from its *statements*, so a jump buried in an expression is refused rather than dropped. |
| `00216` | initialising a flexible array member, which GCC allows with a warning by over-allocating the object — the Rust item would have to have a different type from the one `sizeof` reports. Under `c99!` the case also needs the C23 empty initialiser `{}`, which `gnu99!` and `c23!` accept. |
| `00220` | `<wchar.h>`, which is not among the bundled headers |

**Wanted a later entry point (1, `c99!` only).** `00219` uses `_Generic` and is
tagged `c89`, so `c99!` refuses it and says to write `c11!`. Under every other
entry point — including `gnu99!`, which accepts what C11 added — it passes.

**A documented deviation (1).** `00152` writes `#line line` with `line` a macro
expanding to `1000` and then checks `__LINE__`. `cinrs` makes `__LINE__` the
line of the *`.rs` file*, so that it points where the user is looking, and
`#line` does not redirect it; the case's `#error` fires.

## Reproducing the numbers

```
cargo test --test c_testsuite                                   # guard, c99
CINRS_CTESTSUITE_REPORT=1 cargo test --test c_testsuite          # report, c99
CINRS_CTESTSUITE_REPORT=1 CINRS_CTESTSUITE_STANDARD=c11 \
    cargo test --test c_testsuite                                # report, c11
CINRS_CTESTSUITE_REPORT=1 CINRS_CTESTSUITE_STANDARD=gnu99 \
    cargo test --test c_testsuite                                # report, gnu99
```

A full run of all 220 cases — generate, compile, execute, diff — takes about
six seconds on a 32-thread machine.
