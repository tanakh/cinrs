/* <x86intrin.h> — everything <immintrin.h> has.
 *
 * GCC's <x86intrin.h> adds the AMD-only families — 3DNow!, SSE4a, XOP, TBM —
 * on top of <immintrin.h>. None of those is in `core::arch`'s stable surface,
 * so here the two headers carry the same set.
 */
#ifndef _CINRS_X86INTRIN_H
#define _CINRS_X86INTRIN_H
#include <immintrin.h>
#endif /* _CINRS_X86INTRIN_H */
