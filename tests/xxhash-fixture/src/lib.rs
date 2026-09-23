//! xxHash 0.8.4, compiled from C by `cinrs`.
//!
//! The C is not in this repository: `scripts/check-xxhash.sh` downloads the
//! release archive into `target/xxhash/`, verifies its SHA-256, and then builds
//! this crate. The upstream files are read unedited.
//!
//! **One unit per vector width.** `xxhash.c` is `xxhash.h` with
//! `XXH_STATIC_LINKING_ONLY` and `XXH_IMPLEMENTATION` defined; which XXH3
//! kernel the header compiles is chosen by `XXH_VECTOR` (`XXH_SCALAR`,
//! `XXH_SSE2`, `XXH_AVX2`, `XXH_AVX512`, which the header itself defines, so
//! the macro can name them before they exist), which upstream's build leaves
//! to `-mavx2`/`-mavx512f` and the `__AVX2__`/`__AVX512F__` those define. Here
//! each block sets it outright, before the include, and writes the pragma GCC
//! would take in place of the flag — `#pragma GCC target("…")` — so that every
//! function defined after it may use the instruction set. `XXH_NAMESPACE`
//! prefixes every public symbol (`scalar_XXH32`, `avx2_XXH3_64bits`, …), which
//! is what upstream offers for linking several copies of xxHash into one
//! program, so the four exported APIs do not collide at link time.
//!
//! **The dispatcher** is `xxh_x86dispatch.c`, which defines `XXH_INLINE_ALL`
//! and includes `xxhash.h` itself — a private copy of the whole library — and
//! then compiles the SSE2, AVX2 and AVX-512 kernels in that one unit behind
//! `__attribute__((__target__("…")))`, choosing among them at run time with
//! `cpuid`. It exports `XXH3_64bits_dispatch` and the rest, and
//! `XXH_featureTest`.
//!
//! **Nothing is configured around cinrs.** In particular xxHash reads its input
//! the way it does under GCC (`XXH_FORCE_MEMORY_ACCESS` 1): through
//! `typedef __attribute__((__aligned__(1))) … xxh_u64 xxh_unalign64;`
//! (xxhash.h:2661, 3382), which cinrs turns into `read_unaligned` — the debug
//! build, which panics on a misaligned dereference, is what checks it.

pub mod scalar {
    //! `xxhash.c` with `XXH_VECTOR=XXH_SCALAR`.
    cinrs::gnu11! {
        #pragma cinrs export
        #define XXH_NAMESPACE scalar_
        #define XXH_VECTOR XXH_SCALAR
        #include "../../../target/xxhash/xxhash.c"
    }
}

pub mod sse2 {
    //! `xxhash.c` with `XXH_VECTOR=XXH_SSE2` (the x86-64 baseline).
    cinrs::gnu11! {
        #pragma cinrs export
        #pragma GCC target("sse2")
        #define XXH_NAMESPACE sse2_
        #define XXH_VECTOR XXH_SSE2
        #include "../../../target/xxhash/xxhash.c"
    }
}

pub mod avx2 {
    //! `xxhash.c` with `XXH_VECTOR=XXH_AVX2`, upstream built with `-mavx2`.
    cinrs::gnu11! {
        #pragma cinrs export
        // The pragma also defines `__AVX2__` (and `__AVX__`, …), as GCC's
        // does, which is what makes xxhash.h:3881 include <immintrin.h>.
        #pragma GCC target("avx2")
        #define XXH_NAMESPACE avx2_
        #define XXH_VECTOR XXH_AVX2
        #include "../../../target/xxhash/xxhash.c"
    }
}

pub mod avx512 {
    //! `xxhash.c` with `XXH_VECTOR=XXH_AVX512`, upstream built with
    //! `-mavx512f`.
    cinrs::gnu11! {
        #pragma cinrs export
        #pragma GCC target("avx512f")
        #define XXH_NAMESPACE avx512_
        #define XXH_VECTOR XXH_AVX512
        #include "../../../target/xxhash/xxhash.c"
    }
}

pub mod dispatch {
    //! `xxh_x86dispatch.c`: its own copy of the library, the three kernels
    //! behind `__attribute__((__target__))`, and `cpuid` — whose template is
    //! `"{cpuid|cpuid}"`, GCC's dialect alternatives, of which cinrs takes the
    //! AT&T one. `XXH_DISPATCH_AVX2` and `XXH_DISPATCH_AVX512` come out as 1
    //! on their own: `__GNUC__` is 4.2 here, but `#ifdef __has_include` is
    //! true and `__has_include(<avx2intrin.h>)` answers yes.
    cinrs::gnu11! {
        #pragma cinrs export
        #include "../../../target/xxhash/xxh_x86dispatch.c"
    }
}
