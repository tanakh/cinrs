/* <mmintrin.h> — MMX, which cinrs does not have.
 *
 * `__m64` and the MMX intrinsics are not in `core::arch`'s stable surface:
 * they were removed from the standard library because MMX shares its register
 * file with x87, needs an `emms` between the two, and has been superseded on
 * every processor since the Pentium 4 by the SSE2 forms of the same
 * operations. There is therefore nothing to map a call onto.
 *
 * Every MMX intrinsic has a 128-bit counterpart one letter apart —
 * `_mm_add_pi16` is `_mm_add_epi16`, `_mm_packs_pu16` is `_mm_packus_epi16` —
 * so a port is usually mechanical. <emmintrin.h> has them.
 */
#ifndef _CINRS_MMINTRIN_H
#define _CINRS_MMINTRIN_H
#error "<mmintrin.h>: cinrs has no MMX and no '__m64' — Rust's core::arch dropped them. Use the SSE2 forms in <emmintrin.h>: '_mm_add_pi16' is '_mm_add_epi16', and so on."
#endif /* _CINRS_MMINTRIN_H */
