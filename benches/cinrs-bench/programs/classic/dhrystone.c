/* Dhrystone 2.1 as one translation unit.
 *
 * Reinhold P. Weicker's benchmark is distributed as three files — `dhry.h`,
 * `dhry_1.c` and `dhry_2.c` — and is normally linked from two objects. A
 * `cinrs` macro invocation is *one* translation unit, so this file makes one
 * out of the two: the vendored sources are included verbatim, in order, and
 * nothing else is added. `dhrystone/dhry.h` is a two-line include guard in
 * front of the header Weicker wrote, which has none of its own; see the note
 * there and `SOURCES.md`.
 *
 * Compiled with `-std=gnu89 -DTIME`: the benchmark is K&R C — implicit `int`,
 * implicit function declarations, old-style definitions — and `-DTIME` is
 * what defines `Too_Small_Time`, without which the source does not compile at
 * all. The number of runs is read from standard input.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0 (this file only; the included
 * sources are Weicker's, see `SOURCES.md`)
 */

#include "dhrystone/dhry_1.c"
#include "dhrystone/dhry_2.c"
