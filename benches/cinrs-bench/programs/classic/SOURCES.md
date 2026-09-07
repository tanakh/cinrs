# The classic micro-benchmarks

Three benchmarks from the 1970s and 1980s that every compiler has been measured
against at some point. Two of them are vendored; the third is written here,
and this file says why.

## Dhrystone 2.1 — vendored

* `dhrystone/dhry_weicker.h`, `dhrystone/dhry_1.c`, `dhrystone/dhry_2.c`,
  `dhrystone/README_C` — **verbatim**, from the `dhry-c` shell archive at
  <https://www.netlib.org/benchmark/dhry-c>, fetched **2026-09-07**. Only
  `dhry.h` was renamed, to `dhry_weicker.h`; the bytes are untouched.
* Author: **Reinhold P. Weicker**, Siemens AG. Version 2.1, 25 May 1988,
  published with the article "Dhrystone Benchmark: Rationale for Version 2 and
  Measurement Rules" in *SIGPLAN Notices* 23, 8 (Aug. 1988), 49-62 — the
  article is `dhrystone/RATIONALE` in the archive.
* **Licence.** The distribution carries no formal licence text. Dhrystone was
  published to be run and reported: the author's own `README_C` (kept here)
  asks recipients who take measurements to send him the results, and the
  program has been redistributed unmodified in that spirit for nearly forty
  years — it is in the source tree of, among others, Debian, coremark's
  ancestors and every embedded toolchain vendor. The three sources are here
  **unmodified**, with the author's headers and the `README_C` intact, which is
  the only condition the distribution states. Anyone repackaging this
  repository who wants a formal grant should drop the directory; nothing else
  in the suite depends on it.
* **Two files added by this suite**, both marked as such in their own headers
  and both MIT OR Apache-2.0:
  * `dhrystone/dhry.h` — a two-line include guard in front of
    `dhry_weicker.h`. Weicker's header has none, and this suite compiles
    `dhry_1.c` and `dhry_2.c` as *one* translation unit (a `cinrs` macro
    invocation is one), so it is read twice.
  * `dhrystone.c` — the amalgamation: `#include` of the two `.c` files, in
    order, and nothing else.
* **How it is built.** `-std=gnu89 -DTIME`. The benchmark is K&R C — implicit
  `int`, implicit function declarations, old-style definitions — which gcc 15
  rejects at `-std=gnu11`, and which `cinrs`'s `gnu89!` accepts for the same
  reasons gcc's `-std=gnu89` does. `-DTIME` is not optional: `Too_Small_Time`
  is defined only under `TIME` or `TIMES`, so without one of them the source
  does not compile. The number of runs is read from standard input.
* **Output.** Deterministic except for three things, which the row's
  `output_filter` drops before the three builds are compared: the two
  `Ptr_Comp:` lines, which print a pointer value cast to `int`, and the lines
  that report elapsed time and Dhrystones per second. Everything else — the
  "Final values of the variables used in the benchmark" block, which is what
  Dhrystone is really checking — is compared byte for byte.

## Whetstone — vendored

* `whetstone.c` — **verbatim**, from <https://www.netlib.org/benchmark/whetstone.c>,
  fetched **2026-09-07**. "C Converted Whetstone Double Precision Benchmark,
  Version 1.2, 22 March 1998", converted by **Rich Painter**, Painter
  Engineering, Inc., from the netlib `benchmark/whetstoned` FORTRAN original.
* **Licence.** © 1998 Painter Engineering, Inc. The header comment states:
  "Permission is granted to use, duplicate, and publish this text and program
  as long as it includes this entire comment block and limited rights
  reference." The file here includes that entire comment block, unedited,
  which is the condition met.
* **How it is built.** `-DPRINTOUT`, which turns on the `POUT` calls. Without
  it the program's only output is the rate it measured, which is a different
  number on every run and cannot be compared across builds; with it, each
  module prints its `N`, `J`, `K` and `X1..X4` as `%12.4e`, which is
  deterministic. The loop count is an argument.
* **Why it is the longest row in the suite.** Whetstone times itself with
  `time(0)`, whose resolution is one second, and *returns 1* when less than a
  second has passed. The loop count is therefore chosen so that every build,
  including the slowest, is safely over that boundary.

## LINPACK — written here, not vendored

`linpack.c` is **not** the netlib program. Two reasons:

1. **Licence.** `netlib.org/benchmark/linpackc.new` — "Translated to C by
   Bonnie Toy 5/88", modified by Will Menninger and Jack Dongarra — carries no
   licence notice of any kind. Netlib's contents are conventionally treated as
   freely available, but "conventionally" is not a grant, and the instruction
   for this suite was to write an equivalent rather than vendor something
   doubtful.
2. **It cannot be compared.** The netlib program reports MFLOPS and the number
   of repetitions it chose to reach ten CPU seconds. Both are different on
   every run, so there would be nothing left to compare byte for byte — which
   is the correctness check this whole suite is built around.

What is here instead is the benchmark's *kernel*, written from its published
structure and licensed MIT OR Apache-2.0 like the rest of the crate: `idamax`,
`dscal`, `daxpy`, `dgefa` (LU with partial pivoting, multipliers stored
negated, exactly as LINPACK does it) and `dgesl`, over a column-major matrix
generated by a fixed LCG, with the right-hand side chosen so the solution is
the vector of ones. It performs the same arithmetic the benchmark does — the
same O(n³/3) of `daxpy` down columns — and prints the residual instead of a
rate, so it is deterministic and the same on every machine.
