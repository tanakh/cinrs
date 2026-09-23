//! BLAKE3 1.8.7's C implementation, compiled from C by `cinrs`.
//!
//! The C is not in this repository: `scripts/check-blake3.sh` downloads the
//! release archive into `target/blake3/`, verifies its SHA-256, and then builds
//! this crate. The upstream files are read unedited.
//!
//! **One unit per C file, as upstream's build has it.** The seven files are one
//! program, but they cannot be one translation unit: the four SIMD files each
//! define `static inline` helpers under the *same* names (`loadu`, `storeu`,
//! `addv`, `rot16`, `round_fn`, …), which is fine for seven object files and a
//! redefinition in one. So each file is its own `gnu11!` block in a `mod` of
//! its own, and `#pragma cinrs export` gives every function with external
//! linkage a real C symbol, so that `blake3_dispatch.c`'s calls to
//! `blake3_hash_many_avx2` and the rest, and `blake3.c`'s calls into the
//! dispatcher, resolve at link time exactly as the object files would.
//!
//! **One instruction set per SIMD file.** Upstream compiles `blake3_sse41.c`
//! with `-msse4.1`, `blake3_avx2.c` with `-mavx2` and `blake3_avx512.c` with
//! `-mavx512f -mavx512vl`. A procedural macro cannot see compiler flags, and
//! the files do not say what they need themselves, so each block writes the
//! pragma GCC would take in place of the flag — `#pragma GCC target("…")`
//! before the `#include` — which applies to every function *defined* after it,
//! the `static inline` helpers included. That is what makes the intrinsics
//! callable (each becomes `#[target_feature(enable = "…")]`) and what lets a
//! helper take or return a `__m256i`/`__m512i` by value. SSE2 is the x86-64
//! baseline and needs nothing; the pragma is written for symmetry.
//! Being one unit per block, no `push_options`/`pop_options` is needed.
//!
//! `gnu11` because the files use GNU C: `__attribute__((always_inline))`
//! (which becomes `#[inline]` beside a target feature, since rustc refuses
//! `#[inline(always)]` there), `__builtin_clzll`, and `__asm__ __volatile__`
//! for `cpuid`/`xgetbv` — whose `"=b"` operand cinrs carries through rbx with
//! an `xchg` around the template.

pub mod blake3 {
    //! `blake3.c`: the public API (`blake3_hasher_*`), the tree logic.
    cinrs::gnu11! {
        #pragma cinrs export
        #include "../../../target/blake3/c/blake3.c"
    }
}

pub mod dispatch {
    //! `blake3_dispatch.c`: `cpuid`/`xgetbv` detection and the choice of
    //! implementation at run time.
    cinrs::gnu11! {
        #pragma cinrs export
        #include "../../../target/blake3/c/blake3_dispatch.c"
    }
}

pub mod portable {
    //! `blake3_portable.c`: the plain-C compression function.
    cinrs::gnu11! {
        #pragma cinrs export
        #include "../../../target/blake3/c/blake3_portable.c"
    }
}

pub mod sse2 {
    //! `blake3_sse2.c`, upstream built with `-msse2` (the x86-64 baseline).
    cinrs::gnu11! {
        #pragma cinrs export
        #pragma GCC target("sse2")
        #include "../../../target/blake3/c/blake3_sse2.c"
    }
}

pub mod sse41 {
    //! `blake3_sse41.c`, upstream built with `-msse4.1`.
    cinrs::gnu11! {
        #pragma cinrs export
        #pragma GCC target("sse4.1")
        #include "../../../target/blake3/c/blake3_sse41.c"
    }
}

pub mod avx2 {
    //! `blake3_avx2.c`, upstream built with `-mavx2`.
    cinrs::gnu11! {
        #pragma cinrs export
        #pragma GCC target("avx2")
        #include "../../../target/blake3/c/blake3_avx2.c"
    }
}

pub mod avx512 {
    //! `blake3_avx512.c`, upstream built with `-mavx512f -mavx512vl`.
    cinrs::gnu11! {
        #pragma cinrs export
        #pragma GCC target("avx512f,avx512vl")
        #include "../../../target/blake3/c/blake3_avx512.c"
    }
}
