/* Not part of the Dhrystone distribution.
 *
 * `dhry_1.c` and `dhry_2.c` each `#include "dhry.h"`, and this suite compiles
 * the two of them as one translation unit (`../dhrystone.c`), because a
 * `cinrs` macro invocation *is* one translation unit. The header Weicker
 * distributed has no include guard, so reading it twice redeclares its
 * enumerators, which no revision of C before C23 allows.
 *
 * So the vendored header is next to this one, verbatim, under the name
 * `dhry_weicker.h`, and this two-line guard stands in front of it. Nothing
 * else about the benchmark is changed.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#ifndef CINRS_BENCH_DHRY_H
#define CINRS_BENCH_DHRY_H

#include "dhry_weicker.h"

#endif /* CINRS_BENCH_DHRY_H */
