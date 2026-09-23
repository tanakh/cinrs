//! The maintainer tool that writes the x86 intrinsics table and the bundled
//! headers' prototype lists, and the check that the two committed files agree.
//!
//! `cinrs` maps a call to an Intel intrinsic straight onto the function of the
//! same name in `core::arch::x86_64` (or `core::arch::x86`), so the two have to
//! agree about every name, every parameter type and every operand that is an
//! immediate. Rather than transcribe eight hundred signatures by hand, they are
//! *read out of the standard library's own source*: `library/stdarch` is what
//! `core::arch` is built from, it is installed by `rustup component add
//! rust-src`, and the declaration `cinrs` bundles is a translation of the
//! signature `rustc` will type check the call against.
//!
//! Two tests live here:
//!
//! * [`regenerate`] is `#[ignore]`d — it is a maintainer tool, run by hand with
//!   `cargo test -p cinrs-core --test x86_intrinsics -- --ignored --nocapture`
//!   when the minimum supported Rust version moves. It rewrites the generated
//!   regions of `include/*intrin.h` and the whole of `src/x86/table.rs`, and
//!   prints what it left out and why.
//! * [`header_and_table_agree`] runs every time and needs no `rust-src`: it
//!   reads the two committed files and checks that they declare exactly the
//!   same names with exactly the same arities and immediate positions. A hand
//!   edit to one of them and not the other is what it is there to catch.
//!
//! The source of truth is the **oldest** toolchain the crate supports, not the
//! newest: an intrinsic that only exists in a later release must not be
//! declared, or a unit that compiles here would not compile on the minimum
//! supported Rust version. `CINRS_STDARCH` points the generator at a checkout;
//! failing that it asks `rustc` for its sysroot.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// what is covered
// ---------------------------------------------------------------------------

/// The `core_arch` source files that are read, with the bundled header each
/// one's prototypes go into and the marker that names the region.
///
/// The header layout is GCC's and Clang's: `<xmmintrin.h>` is SSE,
/// `<emmintrin.h>` includes it and adds SSE2, and so on up to `<nmmintrin.h>`;
/// `<immintrin.h>` includes the whole chain and adds everything 256-bit and
/// everything scalar. Code that was written for one of the older compilers
/// includes the header its instruction set arrived in, which is why they are
/// separate files rather than one.
const FAMILIES: &[Family] = &[
    Family::new("sse", "xmmintrin.h", "sse"),
    Family::new("sse2", "emmintrin.h", "sse2"),
    Family::new("sse3", "pmmintrin.h", "sse3"),
    Family::new("ssse3", "tmmintrin.h", "ssse3"),
    Family::new("sse41", "smmintrin.h", "sse4.1"),
    Family::new("sse42", "nmmintrin.h", "sse4.2"),
    Family::new("aes", "wmmintrin.h", "aes"),
    Family::new("pclmulqdq", "wmmintrin.h", "pclmulqdq"),
    Family::new("avx", "immintrin.h", "avx"),
    Family::new("avx2", "immintrin.h", "avx2"),
    Family::new("fma", "immintrin.h", "fma"),
    // `sha.rs` also holds SHA512, SM3 and SM4, which GCC gives headers of
    // their own; `split` sends them there (see [`route`]).
    Family::new("sha", "immintrin.h", "sha").split(None),
    Family::new("bmi1", "immintrin.h", "bmi1").also("bmi"),
    Family::new("bmi2", "immintrin.h", "bmi2"),
    Family::new("abm", "immintrin.h", "lzcnt"),
    // AVX-512 and what arrived with it. GCC splits each AVX-512 family's
    // 128- and 256-bit forms — the ones that also need AVX512VL — into a
    // second header, and `stdarch` keeps AVX-VNNI and AVX-IFMA in the AVX-512
    // files; `split` and [`route`] put every prototype where GCC has it. These
    // headers do not exist until the generator first writes them, from the
    // skeleton [`skeleton`] builds; after that only their regions are
    // rewritten, like every other header's.
    //
    // Left out, and listed in the report as unstable: AVX512VP2INTERSECT (the
    // whole module is `#[unstable]` in `core::arch`, so its header is declared
    // empty), and the AVX512-FP16 and AVX512-BF16 intrinsics that take or
    // return an `f16` or `bf16` scalar, which Rust has not stabilised.
    Family::new("avx512f", "avx512fintrin.h", "avx512f").split(Some("avx512vlintrin.h")),
    Family::new("avx512bw", "avx512bwintrin.h", "avx512bw").split(Some("avx512vlbwintrin.h")),
    Family::new("avx512cd", "avx512cdintrin.h", "avx512cd").split(Some("avx512vlintrin.h")),
    Family::new("avx512dq", "avx512dqintrin.h", "avx512dq").split(Some("avx512vldqintrin.h")),
    Family::new("avx512vbmi", "avx512vbmiintrin.h", "avx512vbmi")
        .split(Some("avx512vbmivlintrin.h")),
    Family::new("avx512vbmi2", "avx512vbmi2intrin.h", "avx512vbmi2")
        .split(Some("avx512vbmi2vlintrin.h")),
    Family::new("avx512vnni", "avx512vnniintrin.h", "avx512vnni")
        .split(Some("avx512vnnivlintrin.h")),
    Family::new("avx512bitalg", "avx512bitalgintrin.h", "avx512bitalg")
        .split(Some("avx512bitalgvlintrin.h")),
    Family::new(
        "avx512vpopcntdq",
        "avx512vpopcntdqintrin.h",
        "avx512vpopcntdq",
    )
    .split(Some("avx512vpopcntdqvlintrin.h")),
    Family::new("avx512ifma", "avx512ifmaintrin.h", "avx512ifma")
        .split(Some("avx512ifmavlintrin.h")),
    Family::new("avx512bf16", "avx512bf16intrin.h", "avx512bf16")
        .split(Some("avx512bf16vlintrin.h")),
    Family::new("avx512fp16", "avx512fp16intrin.h", "avx512fp16")
        .split(Some("avx512fp16vlintrin.h")),
    Family::new(
        "avx512vp2intersect",
        "avx512vp2intersectintrin.h",
        "avx512vp2intersect",
    ),
    Family::new("gfni", "gfniintrin.h", "gfni"),
    Family::new("vaes", "vaesintrin.h", "vaes"),
    Family::new("vpclmulqdq", "vpclmulqdqintrin.h", "vpclmulqdq"),
    Family::new("f16c", "f16cintrin.h", "f16c"),
];

/// The typedefs a new header opens with, where GCC has them: the 512-bit
/// vectors, `__mmask8`, `__mmask16` and the `_MM_*_ENUM` immediates in
/// `<avx512fintrin.h>`, the wide masks in `<avx512bwintrin.h>`, and the
/// BF16 and FP16 vectors in their own headers. A mask is a plain unsigned
/// integer in C and in `core::arch` alike, and each enum is `i32` in Rust.
const PREAMBLES: &[(&str, &str)] = &[
    (
        "avx512fintrin.h",
        "typedef __cinrs_m512 __m512;\n\
         typedef __cinrs_m512i __m512i;\n\
         typedef __cinrs_m512d __m512d;\n\
         typedef unsigned char __mmask8;\n\
         typedef unsigned short __mmask16;\n\
         typedef int _MM_CMPINT_ENUM;\n\
         typedef int _MM_MANTISSA_NORM_ENUM;\n\
         typedef int _MM_MANTISSA_SIGN_ENUM;\n\
         typedef int _MM_PERM_ENUM;\n\
         \n\
         /* Intel's and GCC's own spellings, where `core::arch` capitalised\n\
         \x20* them or has no name at all: real code writes these. */\n\
         #define _MM_MANT_NORM_p5_2 _MM_MANT_NORM_P5_2\n\
         #define _MM_MANT_NORM_p5_1 _MM_MANT_NORM_P5_1\n\
         #define _MM_MANT_NORM_p75_1p5 _MM_MANT_NORM_P75_1P5\n\
         #define _MM_MANT_SIGN_src _MM_MANT_SIGN_SRC\n\
         #define _MM_MANT_SIGN_zero _MM_MANT_SIGN_ZERO\n\
         #define _MM_MANT_SIGN_nan _MM_MANT_SIGN_NAN\n\
         #define _MM_CMPINT_UNUSED 0x0003\n\
         #define _MM_CMPINT_GE _MM_CMPINT_NLT\n\
         #define _MM_CMPINT_GT _MM_CMPINT_NLE\n",
    ),
    (
        "avx512bwintrin.h",
        "typedef unsigned int __mmask32;\n\
         typedef unsigned long long __mmask64;\n",
    ),
    (
        "avx512bf16intrin.h",
        "typedef __cinrs_m128bh __m128bh;\n\
         typedef __cinrs_m256bh __m256bh;\n\
         typedef __cinrs_m512bh __m512bh;\n",
    ),
    (
        "avx512fp16intrin.h",
        "typedef __cinrs_m128h __m128h;\n\
         typedef __cinrs_m256h __m256h;\n\
         typedef __cinrs_m512h __m512h;\n",
    ),
];

/// The pointer parameters GCC declares `void *` (`false`) or `void const *`
/// (`true`), as `(C argument index, const, names)`.
///
/// `core::arch` types every memory operand (`*const __m512i`, `*const i32`,
/// `*mut u8`), where Intel's and GCC's prototypes say `void *` for many of
/// them — and real code passes an `int *` to `_mm512_loadu_si512` without a
/// cast, which since GCC 14 a typed parameter would turn into an error. So
/// the bundled header spells these parameters exactly as GCC does, and code
/// generation casts every pointer argument of an intrinsic call with `as
/// *const _` / `as *mut _`, which rustc infers from `core::arch`'s own type.
///
/// Derived once, from the `static __inline` definitions in GCC 15.2's
/// `/usr/lib/gcc/x86_64-linux-gnu/15/include/*intrin.h`, for every name in
/// this table; checked in so regeneration needs no gcc. An intrinsic GCC types
/// (`_mm256_loadu_si256 (__m256i_u const *)`, the AVX2 gathers' `int const *`)
/// is not here, and neither are the eight `_mm512_*i32lo{gather,scatter}_*`
/// that GCC does not have; those keep `core::arch`'s type. [`regenerate`]
/// panics if a name here is not declared or its parameter is not a pointer.
const GCC_VOID_POINTERS: &[(usize, bool, &[&str])] = &[
    (
        0,
        true,
        &[
            "_mm256_load_epi32",
            "_mm256_load_epi64",
            "_mm256_loadu_epi16",
            "_mm256_loadu_epi32",
            "_mm256_loadu_epi64",
            "_mm256_loadu_epi8",
            "_mm512_load_epi32",
            "_mm512_load_epi64",
            "_mm512_load_pd",
            "_mm512_load_ps",
            "_mm512_load_si512",
            "_mm512_loadu_epi16",
            "_mm512_loadu_epi32",
            "_mm512_loadu_epi64",
            "_mm512_loadu_epi8",
            "_mm512_loadu_pd",
            "_mm512_loadu_ps",
            "_mm512_loadu_si512",
            "_mm_clflush",
            "_mm_load_epi32",
            "_mm_load_epi64",
            "_mm_loadu_epi16",
            "_mm_loadu_epi32",
            "_mm_loadu_epi64",
            "_mm_loadu_epi8",
            "_mm_loadu_si16",
            "_mm_loadu_si32",
            "_mm_loadu_si64",
            "_mm_prefetch",
        ],
    ),
    (
        1,
        true,
        &[
            "_mm256_maskz_expandloadu_epi16",
            "_mm256_maskz_expandloadu_epi32",
            "_mm256_maskz_expandloadu_epi64",
            "_mm256_maskz_expandloadu_epi8",
            "_mm256_maskz_expandloadu_pd",
            "_mm256_maskz_expandloadu_ps",
            "_mm256_maskz_load_epi32",
            "_mm256_maskz_load_epi64",
            "_mm256_maskz_load_pd",
            "_mm256_maskz_load_ps",
            "_mm256_maskz_loadu_epi16",
            "_mm256_maskz_loadu_epi32",
            "_mm256_maskz_loadu_epi64",
            "_mm256_maskz_loadu_epi8",
            "_mm256_maskz_loadu_pd",
            "_mm256_maskz_loadu_ps",
            "_mm512_i32gather_epi32",
            "_mm512_i32gather_epi64",
            "_mm512_i32gather_pd",
            "_mm512_i32gather_ps",
            "_mm512_i64gather_epi32",
            "_mm512_i64gather_epi64",
            "_mm512_i64gather_pd",
            "_mm512_i64gather_ps",
            "_mm512_maskz_expandloadu_epi16",
            "_mm512_maskz_expandloadu_epi32",
            "_mm512_maskz_expandloadu_epi64",
            "_mm512_maskz_expandloadu_epi8",
            "_mm512_maskz_expandloadu_pd",
            "_mm512_maskz_expandloadu_ps",
            "_mm512_maskz_load_epi32",
            "_mm512_maskz_load_epi64",
            "_mm512_maskz_load_pd",
            "_mm512_maskz_load_ps",
            "_mm512_maskz_loadu_epi16",
            "_mm512_maskz_loadu_epi32",
            "_mm512_maskz_loadu_epi64",
            "_mm512_maskz_loadu_epi8",
            "_mm512_maskz_loadu_pd",
            "_mm512_maskz_loadu_ps",
            "_mm_maskz_expandloadu_epi16",
            "_mm_maskz_expandloadu_epi32",
            "_mm_maskz_expandloadu_epi64",
            "_mm_maskz_expandloadu_epi8",
            "_mm_maskz_expandloadu_pd",
            "_mm_maskz_expandloadu_ps",
            "_mm_maskz_load_epi32",
            "_mm_maskz_load_epi64",
            "_mm_maskz_load_pd",
            "_mm_maskz_load_ps",
            "_mm_maskz_loadu_epi16",
            "_mm_maskz_loadu_epi32",
            "_mm_maskz_loadu_epi64",
            "_mm_maskz_loadu_epi8",
            "_mm_maskz_loadu_pd",
            "_mm_maskz_loadu_ps",
        ],
    ),
    (
        2,
        true,
        &[
            "_mm256_mask_expandloadu_epi16",
            "_mm256_mask_expandloadu_epi32",
            "_mm256_mask_expandloadu_epi64",
            "_mm256_mask_expandloadu_epi8",
            "_mm256_mask_expandloadu_pd",
            "_mm256_mask_expandloadu_ps",
            "_mm256_mask_load_epi32",
            "_mm256_mask_load_epi64",
            "_mm256_mask_load_pd",
            "_mm256_mask_load_ps",
            "_mm256_mask_loadu_epi16",
            "_mm256_mask_loadu_epi32",
            "_mm256_mask_loadu_epi64",
            "_mm256_mask_loadu_epi8",
            "_mm256_mask_loadu_pd",
            "_mm256_mask_loadu_ps",
            "_mm512_mask_expandloadu_epi16",
            "_mm512_mask_expandloadu_epi32",
            "_mm512_mask_expandloadu_epi64",
            "_mm512_mask_expandloadu_epi8",
            "_mm512_mask_expandloadu_pd",
            "_mm512_mask_expandloadu_ps",
            "_mm512_mask_load_epi32",
            "_mm512_mask_load_epi64",
            "_mm512_mask_load_pd",
            "_mm512_mask_load_ps",
            "_mm512_mask_loadu_epi16",
            "_mm512_mask_loadu_epi32",
            "_mm512_mask_loadu_epi64",
            "_mm512_mask_loadu_epi8",
            "_mm512_mask_loadu_pd",
            "_mm512_mask_loadu_ps",
            "_mm_mask_expandloadu_epi16",
            "_mm_mask_expandloadu_epi32",
            "_mm_mask_expandloadu_epi64",
            "_mm_mask_expandloadu_epi8",
            "_mm_mask_expandloadu_pd",
            "_mm_mask_expandloadu_ps",
            "_mm_mask_load_epi32",
            "_mm_mask_load_epi64",
            "_mm_mask_load_pd",
            "_mm_mask_load_ps",
            "_mm_mask_loadu_epi16",
            "_mm_mask_loadu_epi32",
            "_mm_mask_loadu_epi64",
            "_mm_mask_loadu_epi8",
            "_mm_mask_loadu_pd",
            "_mm_mask_loadu_ps",
        ],
    ),
    (
        3,
        true,
        &[
            "_mm256_mmask_i32gather_epi32",
            "_mm256_mmask_i32gather_epi64",
            "_mm256_mmask_i32gather_pd",
            "_mm256_mmask_i32gather_ps",
            "_mm256_mmask_i64gather_epi32",
            "_mm256_mmask_i64gather_epi64",
            "_mm256_mmask_i64gather_pd",
            "_mm256_mmask_i64gather_ps",
            "_mm512_mask_i32gather_epi32",
            "_mm512_mask_i32gather_epi64",
            "_mm512_mask_i32gather_pd",
            "_mm512_mask_i32gather_ps",
            "_mm512_mask_i64gather_epi32",
            "_mm512_mask_i64gather_epi64",
            "_mm512_mask_i64gather_pd",
            "_mm512_mask_i64gather_ps",
            "_mm_mmask_i32gather_epi32",
            "_mm_mmask_i32gather_epi64",
            "_mm_mmask_i32gather_pd",
            "_mm_mmask_i32gather_ps",
            "_mm_mmask_i64gather_epi32",
            "_mm_mmask_i64gather_epi64",
            "_mm_mmask_i64gather_pd",
            "_mm_mmask_i64gather_ps",
        ],
    ),
    (
        0,
        false,
        &[
            "_mm256_i32scatter_epi32",
            "_mm256_i32scatter_epi64",
            "_mm256_i32scatter_pd",
            "_mm256_i32scatter_ps",
            "_mm256_i64scatter_epi32",
            "_mm256_i64scatter_epi64",
            "_mm256_i64scatter_pd",
            "_mm256_i64scatter_ps",
            "_mm256_mask_compressstoreu_epi16",
            "_mm256_mask_compressstoreu_epi32",
            "_mm256_mask_compressstoreu_epi64",
            "_mm256_mask_compressstoreu_epi8",
            "_mm256_mask_compressstoreu_pd",
            "_mm256_mask_compressstoreu_ps",
            "_mm256_mask_cvtepi16_storeu_epi8",
            "_mm256_mask_cvtepi32_storeu_epi16",
            "_mm256_mask_cvtepi32_storeu_epi8",
            "_mm256_mask_cvtepi64_storeu_epi16",
            "_mm256_mask_cvtepi64_storeu_epi32",
            "_mm256_mask_cvtepi64_storeu_epi8",
            "_mm256_mask_cvtsepi16_storeu_epi8",
            "_mm256_mask_cvtsepi32_storeu_epi16",
            "_mm256_mask_cvtsepi32_storeu_epi8",
            "_mm256_mask_cvtsepi64_storeu_epi16",
            "_mm256_mask_cvtsepi64_storeu_epi32",
            "_mm256_mask_cvtsepi64_storeu_epi8",
            "_mm256_mask_cvtusepi16_storeu_epi8",
            "_mm256_mask_cvtusepi32_storeu_epi16",
            "_mm256_mask_cvtusepi32_storeu_epi8",
            "_mm256_mask_cvtusepi64_storeu_epi16",
            "_mm256_mask_cvtusepi64_storeu_epi32",
            "_mm256_mask_cvtusepi64_storeu_epi8",
            "_mm256_mask_i32scatter_epi32",
            "_mm256_mask_i32scatter_epi64",
            "_mm256_mask_i32scatter_pd",
            "_mm256_mask_i32scatter_ps",
            "_mm256_mask_i64scatter_epi32",
            "_mm256_mask_i64scatter_epi64",
            "_mm256_mask_i64scatter_pd",
            "_mm256_mask_i64scatter_ps",
            "_mm256_mask_store_epi32",
            "_mm256_mask_store_epi64",
            "_mm256_mask_store_pd",
            "_mm256_mask_store_ps",
            "_mm256_mask_storeu_epi16",
            "_mm256_mask_storeu_epi32",
            "_mm256_mask_storeu_epi64",
            "_mm256_mask_storeu_epi8",
            "_mm256_mask_storeu_pd",
            "_mm256_mask_storeu_ps",
            "_mm256_store_epi32",
            "_mm256_store_epi64",
            "_mm256_storeu_epi16",
            "_mm256_storeu_epi32",
            "_mm256_storeu_epi64",
            "_mm256_storeu_epi8",
            "_mm512_i32scatter_epi32",
            "_mm512_i32scatter_epi64",
            "_mm512_i32scatter_pd",
            "_mm512_i32scatter_ps",
            "_mm512_i64scatter_epi32",
            "_mm512_i64scatter_epi64",
            "_mm512_i64scatter_pd",
            "_mm512_i64scatter_ps",
            "_mm512_mask_compressstoreu_epi16",
            "_mm512_mask_compressstoreu_epi32",
            "_mm512_mask_compressstoreu_epi64",
            "_mm512_mask_compressstoreu_epi8",
            "_mm512_mask_compressstoreu_pd",
            "_mm512_mask_compressstoreu_ps",
            "_mm512_mask_cvtepi16_storeu_epi8",
            "_mm512_mask_cvtepi32_storeu_epi16",
            "_mm512_mask_cvtepi32_storeu_epi8",
            "_mm512_mask_cvtepi64_storeu_epi16",
            "_mm512_mask_cvtepi64_storeu_epi32",
            "_mm512_mask_cvtepi64_storeu_epi8",
            "_mm512_mask_cvtsepi16_storeu_epi8",
            "_mm512_mask_cvtsepi32_storeu_epi16",
            "_mm512_mask_cvtsepi32_storeu_epi8",
            "_mm512_mask_cvtsepi64_storeu_epi16",
            "_mm512_mask_cvtsepi64_storeu_epi32",
            "_mm512_mask_cvtsepi64_storeu_epi8",
            "_mm512_mask_cvtusepi16_storeu_epi8",
            "_mm512_mask_cvtusepi32_storeu_epi16",
            "_mm512_mask_cvtusepi32_storeu_epi8",
            "_mm512_mask_cvtusepi64_storeu_epi16",
            "_mm512_mask_cvtusepi64_storeu_epi32",
            "_mm512_mask_cvtusepi64_storeu_epi8",
            "_mm512_mask_i32scatter_epi32",
            "_mm512_mask_i32scatter_epi64",
            "_mm512_mask_i32scatter_pd",
            "_mm512_mask_i32scatter_ps",
            "_mm512_mask_i64scatter_epi32",
            "_mm512_mask_i64scatter_epi64",
            "_mm512_mask_i64scatter_pd",
            "_mm512_mask_i64scatter_ps",
            "_mm512_mask_store_epi32",
            "_mm512_mask_store_epi64",
            "_mm512_mask_store_pd",
            "_mm512_mask_store_ps",
            "_mm512_mask_storeu_epi16",
            "_mm512_mask_storeu_epi32",
            "_mm512_mask_storeu_epi64",
            "_mm512_mask_storeu_epi8",
            "_mm512_mask_storeu_pd",
            "_mm512_mask_storeu_ps",
            "_mm512_store_epi32",
            "_mm512_store_epi64",
            "_mm512_store_pd",
            "_mm512_store_ps",
            "_mm512_store_si512",
            "_mm512_storeu_epi16",
            "_mm512_storeu_epi32",
            "_mm512_storeu_epi64",
            "_mm512_storeu_epi8",
            "_mm512_storeu_pd",
            "_mm512_storeu_ps",
            "_mm512_storeu_si512",
            "_mm512_stream_load_si512",
            "_mm_i32scatter_epi32",
            "_mm_i32scatter_epi64",
            "_mm_i32scatter_pd",
            "_mm_i32scatter_ps",
            "_mm_i64scatter_epi32",
            "_mm_i64scatter_epi64",
            "_mm_i64scatter_pd",
            "_mm_i64scatter_ps",
            "_mm_mask_compressstoreu_epi16",
            "_mm_mask_compressstoreu_epi32",
            "_mm_mask_compressstoreu_epi64",
            "_mm_mask_compressstoreu_epi8",
            "_mm_mask_compressstoreu_pd",
            "_mm_mask_compressstoreu_ps",
            "_mm_mask_cvtepi16_storeu_epi8",
            "_mm_mask_cvtepi32_storeu_epi16",
            "_mm_mask_cvtepi32_storeu_epi8",
            "_mm_mask_cvtepi64_storeu_epi16",
            "_mm_mask_cvtepi64_storeu_epi32",
            "_mm_mask_cvtepi64_storeu_epi8",
            "_mm_mask_cvtsepi16_storeu_epi8",
            "_mm_mask_cvtsepi32_storeu_epi16",
            "_mm_mask_cvtsepi32_storeu_epi8",
            "_mm_mask_cvtsepi64_storeu_epi16",
            "_mm_mask_cvtsepi64_storeu_epi32",
            "_mm_mask_cvtsepi64_storeu_epi8",
            "_mm_mask_cvtusepi16_storeu_epi8",
            "_mm_mask_cvtusepi32_storeu_epi16",
            "_mm_mask_cvtusepi32_storeu_epi8",
            "_mm_mask_cvtusepi64_storeu_epi16",
            "_mm_mask_cvtusepi64_storeu_epi32",
            "_mm_mask_cvtusepi64_storeu_epi8",
            "_mm_mask_i32scatter_epi32",
            "_mm_mask_i32scatter_epi64",
            "_mm_mask_i32scatter_pd",
            "_mm_mask_i32scatter_ps",
            "_mm_mask_i64scatter_epi32",
            "_mm_mask_i64scatter_epi64",
            "_mm_mask_i64scatter_pd",
            "_mm_mask_i64scatter_ps",
            "_mm_mask_store_epi32",
            "_mm_mask_store_epi64",
            "_mm_mask_store_pd",
            "_mm_mask_store_ps",
            "_mm_mask_storeu_epi16",
            "_mm_mask_storeu_epi32",
            "_mm_mask_storeu_epi64",
            "_mm_mask_storeu_epi8",
            "_mm_mask_storeu_pd",
            "_mm_mask_storeu_ps",
            "_mm_store_epi32",
            "_mm_store_epi64",
            "_mm_storeu_epi16",
            "_mm_storeu_epi32",
            "_mm_storeu_epi64",
            "_mm_storeu_epi8",
            "_mm_storeu_si16",
            "_mm_storeu_si32",
            "_mm_storeu_si64",
        ],
    ),
];

/// The GCC spelling of the pointer parameter at `index` of `name`, if GCC
/// declares it `void *` or `void const *`.
fn gcc_void_pointer(name: &str, index: usize) -> Option<&'static str> {
    GCC_VOID_POINTERS
        .iter()
        .find(|(at, _, names)| *at == index && names.contains(&name))
        .map(|(_, konst, _)| if *konst { "const void *" } else { "void *" })
}

/// One `core_arch` source file and where its declarations belong.
struct Family {
    /// The file stem under `core_arch/src/x86` and `core_arch/src/x86_64`.
    file: &'static str,
    /// The bundled header the prototypes are written into.
    header: &'static str,
    /// The `#[target_feature]` name the file is expected to carry, which is
    /// also the name the generated region is keyed by.
    ///
    /// `abm.rs` is the exception: it holds both `lzcnt` and `popcnt`, and each
    /// declaration keeps the feature its own attribute named.
    region: &'static str,
    /// A second stem to look for, because the two directories do not always
    /// agree: BMI1 is `x86/bmi1.rs` and `x86_64/bmi.rs`.
    alt: &'static str,
    /// Whether [`route`] may send a prototype somewhere other than `header`.
    split: bool,
    /// The header the forms that also need AVX512VL go into, if GCC gives
    /// them one of their own.
    vl: Option<&'static str>,
}

impl Family {
    const fn new(file: &'static str, header: &'static str, region: &'static str) -> Self {
        Self {
            file,
            header,
            region,
            alt: "",
            split: false,
            vl: None,
        }
    }

    /// The same family, with its prototypes routed by instruction set.
    const fn split(mut self, vl: Option<&'static str>) -> Self {
        self.split = true;
        self.vl = vl;
        self
    }

    /// The same family under a second file name.
    const fn also(mut self, alt: &'static str) -> Self {
        self.alt = alt;
        self
    }

    /// The file stems to look for, in order.
    fn stems(&self) -> Vec<&'static str> {
        if self.alt.is_empty() {
            vec![self.file]
        } else {
            vec![self.file, self.alt]
        }
    }
}

// ---------------------------------------------------------------------------
// the extracted signatures
// ---------------------------------------------------------------------------

/// One intrinsic, as the `core_arch` source declares it.
#[derive(Clone, Debug)]
struct Intrinsic {
    name: String,
    /// The `#[target_feature(enable = "…")]` the function carries.
    feature: String,
    /// The C parameter types, immediates included, in C argument order.
    params: Vec<String>,
    /// The C return type.
    ret: String,
    /// `(argument index, Rust const type)` for every operand that is a
    /// `const` generic in Rust and an integer constant expression in C.
    imm: Vec<(usize, String)>,
    /// Whether the declaration only exists in `core::arch::x86_64`.
    x86_64_only: bool,
}

/// Why a declaration was left out, for the report the generator prints.
#[derive(Debug)]
struct Skipped {
    name: String,
    reason: String,
}

// ---------------------------------------------------------------------------
// reading the standard library's source
// ---------------------------------------------------------------------------

/// The `core_arch/src` directory, from `CINRS_STDARCH` or from the sysroot.
fn core_arch_src() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CINRS_STDARCH") {
        return Some(PathBuf::from(dir));
    }
    let out = Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
        .arg("--print")
        .arg("sysroot")
        .output()
        .ok()?;
    let sysroot = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    let dir = sysroot.join("lib/rustlib/src/rust/library/stdarch/crates/core_arch/src");
    dir.is_dir().then_some(dir)
}

/// The attribute and signature of every `pub fn` in one source file.
///
/// The format `stdarch` is written in is regular enough to read line by line:
/// an item is a run of doc comments and attributes followed by a `pub fn` whose
/// signature may be wrapped over several lines. Anything else — a `mod`, a
/// `use`, an `unsafe extern` block of LLVM declarations — resets the run.
fn scan(text: &str) -> Vec<(Vec<String>, String)> {
    let mut out = Vec::new();
    let mut attrs: Vec<String> = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if trimmed.starts_with("#[") || trimmed.starts_with("#!") {
            let mut attr = trimmed.to_owned();
            // `#[deprecated(since = "…", note = "…")]` is written over three
            // lines; join until the brackets balance.
            while depth(&attr, '[', ']') > 0 {
                match lines.next() {
                    Some(next) => {
                        attr.push(' ');
                        attr.push_str(next.trim());
                    }
                    None => break,
                }
            }
            attrs.push(attr);
            continue;
        }
        // Since 1.9x most of `core::arch` is `const fn`, and a reader that
        // only knew `pub fn` found a third of it (see the count check in
        // [`regenerate`]).
        if [
            "pub fn ",
            "pub unsafe fn ",
            "pub const fn ",
            "pub const unsafe fn ",
        ]
        .iter()
        .any(|p| trimmed.starts_with(p))
        {
            let mut sig = trimmed.to_owned();
            while depth(&sig, '(', ')') > 0 || !sig.contains('{') {
                match lines.next() {
                    Some(next) => {
                        sig.push(' ');
                        sig.push_str(next.trim());
                    }
                    None => break,
                }
            }
            out.push((std::mem::take(&mut attrs), sig));
            continue;
        }
        attrs.clear();
    }
    out
}

/// How far `text` is left unclosed, counting `open` against `close`.
///
/// String literals inside an attribute never hold a bracket in this source, so
/// a plain count is enough.
fn depth(text: &str, open: char, close: char) -> i32 {
    text.chars().fold(0, |d, c| {
        if c == open {
            d + 1
        } else if c == close {
            d - 1
        } else {
            d
        }
    })
}

/// The `"…"` an attribute such as `#[target_feature(enable = "sse2")]` holds.
fn quoted(attr: &str) -> Option<&str> {
    let start = attr.find('"')? + 1;
    let rest = &attr[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// The Rust type spelled `rust`, as C spells it — or `None` when C cannot.
///
/// The integer widths follow Intel's own prototypes rather than a literal
/// reading of the Rust type: Intel writes `char` for the eight-bit lanes of
/// `_mm_set_epi8` and `__int64` for the sixty-four-bit ones, and real code
/// passes plain `int` expressions to both, so the parameter has to be the type
/// that code was written against.
fn c_type(rust: &str) -> Option<String> {
    let rust = rust.trim();
    if let Some(pointee) = rust.strip_prefix("*const ") {
        return Some(format!("const {} *", c_type(pointee)?));
    }
    if let Some(pointee) = rust.strip_prefix("*mut ") {
        return Some(format!("{} *", c_type(pointee)?));
    }
    Some(
        match rust {
            "()" | "" => "void",
            "i8" => "char",
            "u8" => "unsigned char",
            "i16" => "short",
            "u16" => "unsigned short",
            "i32" => "int",
            "u32" => "unsigned int",
            "i64" => "long long",
            "u64" => "unsigned long long",
            "f32" => "float",
            "f64" => "double",
            "__m128"
            | "__m128i"
            | "__m128d"
            | "__m256"
            | "__m256i"
            | "__m256d"
            | "__m512"
            | "__m512i"
            | "__m512d"
            | "__m128bh"
            | "__m256bh"
            | "__m512bh"
            | "__m128h"
            | "__m256h"
            | "__m512h"
            | "__mmask8"
            | "__mmask16"
            | "__mmask32"
            | "__mmask64"
            | "_MM_CMPINT_ENUM"
            | "_MM_MANTISSA_NORM_ENUM"
            | "_MM_MANTISSA_SIGN_ENUM"
            | "_MM_PERM_ENUM" => rust,
            _ => return None,
        }
        .to_owned(),
    )
}

/// Splits a parameter or generic list on the commas that are at depth zero.
fn split_top(list: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in list.chars() {
        match c {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

/// Turns one scanned item into an [`Intrinsic`], or says why it was left out.
fn convert(attrs: &[String], sig: &str, x86_64_only: bool) -> Result<Intrinsic, Skipped> {
    let after_fn = sig.split_once("fn ").map_or(sig, |(_, rest)| rest);
    let name_end = after_fn
        .find(['<', '('])
        .unwrap_or_else(|| after_fn.len().min(1));
    let name = after_fn[..name_end].trim().to_owned();
    let skip = |reason: &str| {
        Err(Skipped {
            name: name.clone(),
            reason: reason.to_owned(),
        })
    };

    if !attrs.iter().any(|a| a.starts_with("#[stable(")) {
        return skip("not stable in core::arch");
    }
    if attrs.iter().any(|a| a.starts_with("#[deprecated")) {
        return skip("deprecated in core::arch");
    }
    // `_mm_pause` is the one intrinsic here that needs no instruction set at
    // all — `pause` is a `rep; nop` on everything back to the 386 — and it
    // carries no `#[target_feature]`. An empty feature name says so.
    let feature = attrs
        .iter()
        .find(|a| a.starts_with("#[target_feature("))
        .and_then(|a| quoted(a))
        .unwrap_or("");

    // The generic list, and after it the parameter list and the return type.
    let rest = &after_fn[name_end..];
    let (generics, rest) = match rest.strip_prefix('<') {
        Some(inner) => {
            let end = close(inner, '<', '>');
            (&inner[..end], &inner[end + 1..])
        }
        None => ("", rest),
    };
    let Some(open) = rest.find('(') else {
        return skip("unreadable signature");
    };
    let params_text = &rest[open + 1..];
    let end = close(params_text, '(', ')');
    let (params_text, tail) = (&params_text[..end], &params_text[end + 1..]);
    let ret_text = match tail.split_once("->") {
        Some((_, r)) => r.split('{').next().unwrap_or("").trim(),
        None => "()",
    };

    let Some(ret) = c_type(ret_text) else {
        return skip(&format!("return type '{ret_text}' has no C spelling"));
    };
    let mut params = Vec::new();
    for param in split_top(params_text) {
        let Some((_, ty)) = param.split_once(':') else {
            return skip("unreadable parameter");
        };
        match c_type(ty) {
            Some(c) => params.push(c),
            None => return skip(&format!("parameter type '{}' has no C spelling", ty.trim())),
        }
    }

    // `#[rustc_legacy_const_generics(1)]` is `core::arch`'s own record of where
    // the operand went in Intel's C prototype: the const generics, in the order
    // they are declared, belong at these argument indices. It is what makes the
    // turbofish reconstructible from a C call.
    let mut imm = Vec::new();
    if !generics.trim().is_empty() {
        let Some(legacy) = attrs
            .iter()
            .find(|a| a.starts_with("#[rustc_legacy_const_generics("))
        else {
            return skip("a const generic with no #[rustc_legacy_const_generics]");
        };
        let inner = &legacy["#[rustc_legacy_const_generics(".len()..];
        let indices: Vec<usize> = inner[..close(inner, '(', ')')]
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect();
        let consts: Vec<String> = split_top(generics)
            .iter()
            .filter_map(|g| g.trim().strip_prefix("const ").map(str::to_owned))
            .collect();
        if indices.len() != consts.len() {
            return skip("a generic that is not a const operand");
        }
        for (index, generic) in indices.iter().zip(&consts) {
            let Some((_, ty)) = generic.split_once(':') else {
                return skip("unreadable const generic");
            };
            // Intel writes the immediate as `const int`; the C prototype has
            // to have a parameter there for the argument to be checked at all.
            // The `_MM_*_ENUM` immediates are `i32` aliases in `core::arch`,
            // and the table records the type the generated literal's suffix
            // is taken from.
            let (spelling, rust_ty) = match ty.trim() {
                "i32" => ("const int", "i32"),
                "u32" => ("const unsigned int", "u32"),
                "_MM_CMPINT_ENUM"
                | "_MM_MANTISSA_NORM_ENUM"
                | "_MM_MANTISSA_SIGN_ENUM"
                | "_MM_PERM_ENUM" => ("const int", "i32"),
                other => return skip(&format!("a const operand of type '{other}'")),
            };
            imm.push((*index, rust_ty.to_owned()));
            if *index > params.len() {
                return skip("an immediate past the end of the argument list");
            }
            params.insert(*index, spelling.to_owned());
        }
    }

    // A memory operand GCC declares `void *` is declared so here too; see
    // [`GCC_VOID_POINTERS`].
    for (index, param) in params.iter_mut().enumerate() {
        if let Some(spelling) = gcc_void_pointer(&name, index) {
            assert!(
                param.ends_with('*'),
                "GCC_VOID_POINTERS says argument {index} of '{name}' is a pointer, but core::arch \
                 makes it '{param}'"
            );
            *param = spelling.to_owned();
        }
    }

    Ok(Intrinsic {
        name,
        feature: feature.to_owned(),
        params,
        ret,
        imm,
        x86_64_only,
    })
}

/// The index of the bracket that closes the one `text` opens after.
fn close(text: &str, open: char, close: char) -> usize {
    let mut depth = 1i32;
    for (index, c) in text.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return index;
            }
        }
    }
    text.len()
}

/// The `pub const _CMP_EQ_OQ: i32 = 0x00;` declarations of one source file,
/// which the header repeats as `#define`s.
fn constants(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("pub const _") else {
            continue;
        };
        let Some((name, rest)) = rest.split_once(':') else {
            continue;
        };
        let Some((ty, value)) = rest.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_end_matches(';').trim();
        // `!0` is Rust's; nothing this header repeats needs it.
        if value.starts_with('!') {
            continue;
        }
        let suffix = if ty.trim() == "u32" { "u" } else { "" };
        // A value written in terms of other constants keeps that spelling; the
        // header defines them in order, so the macro expands the same way.
        // A literal is rewritten in hexadecimal, because `stdarch` writes some
        // of them in Rust's `0b0000_0001` form, which C has no spelling for.
        let value = if value.contains('|') || value.contains("<<") {
            format!("({value})")
        } else {
            let digits = value.replace('_', "");
            let number = digits
                .strip_prefix("0x")
                .and_then(|d| u64::from_str_radix(d, 16).ok())
                .or_else(|| {
                    digits
                        .strip_prefix("0b")
                        .and_then(|d| u64::from_str_radix(d, 2).ok())
                })
                .or_else(|| digits.parse().ok());
            match number {
                Some(n) => format!("0x{n:04x}{suffix}"),
                None => format!("{value}{suffix}"),
            }
        };
        out.push((format!("_{}", name.trim()), value));
    }
    out
}

// ---------------------------------------------------------------------------
// writing the generated regions
// ---------------------------------------------------------------------------

/// The opening line of a generated region.
fn region_begin(kind: &str) -> String {
    format!("/* @generated {kind} — see crates/cinrs-core/tests/x86_intrinsics.rs */")
}

/// The closing line of a generated region.
const REGION_END: &str = "/* @generated end */";

/// Replaces the body of the region `kind` in `text` with `body`.
fn splice(text: &str, kind: &str, body: &str) -> String {
    let begin = region_begin(kind);
    let Some(start) = text.find(&begin) else {
        panic!("no '{begin}' region to fill");
    };
    let after = start + begin.len();
    let end = text[after..]
        .find(REGION_END)
        .unwrap_or_else(|| panic!("region '{kind}' is not closed"))
        + after;
    format!("{}\n{body}{}", &text[..after], &text[end..])
}

/// The C declaration of one intrinsic.
fn declaration(intr: &Intrinsic) -> String {
    let params = if intr.params.is_empty() {
        "void".to_owned()
    } else {
        intr.params.join(", ")
    };
    // `const T *` reads as `const T *` and `T *` as `T *`; the space before the
    // star is already in the type.
    format!("{} {}({});", intr.ret, intr.name, params)
}

/// The header and region one intrinsic of `family` is declared in.
///
/// Only a `split` family routes. An intrinsic whose first instruction set is
/// not the family's and not an AVX-512 one — AVX-VNNI in `avx512vnni.rs`,
/// SHA512 in `sha.rs` — goes to the header GCC names after it; one that also
/// needs AVX512VL goes to the family's `vl` header when GCC has one; anything
/// else stays in the family's own header.
fn route(family: &Family, feature: &str) -> (String, String) {
    let own = || (family.header.to_owned(), family.region.to_owned());
    if !family.split {
        return own();
    }
    let first = feature.split(',').next().unwrap_or("");
    if !first.is_empty() && first != family.region && !first.starts_with("avx512") {
        return (format!("{first}intrin.h"), first.to_owned());
    }
    match family.vl {
        Some(vl) if feature.split(',').any(|f| f == "avx512vl") => {
            (vl.to_owned(), format!("{}-vl", family.region))
        }
        _ => own(),
    }
}

/// What marks a header [`skeleton`] wrote, and that is therefore rewritten
/// whole on every run.
const SKELETON_MARK: &str = "Written by crates/cinrs-core/tests/x86_intrinsics.rs";

/// The text of a header the generator owns: the comment, the guard, the
/// architecture check, the typedefs of [`PREAMBLES`], and one empty region
/// per instruction set, which [`regenerate`] then fills.
///
/// `<immintrin.h>` includes every one of these, in the order the typedefs
/// need, and a unit that includes one of them on its own is sent to
/// `<immintrin.h>` first, which brings this file back in at its place — so
/// `__mmask8` is always declared before `<avx512vlintrin.h>` uses it.
fn skeleton(header: &str, regions: &BTreeMap<String, Vec<String>>, constants: bool) -> String {
    let guard = format!("_CINRS_{}", header.replace('.', "_").to_ascii_uppercase());
    let sets: Vec<&str> = regions.keys().map(String::as_str).collect();
    let mut out = format!(
        "/* <{header}> — the {} intrinsics, as GCC arranges them.\n\
         \x20*\n\
         \x20* Written by crates/cinrs-core/tests/x86_intrinsics.rs; the regions below\n\
         \x20* are regenerated from `core::arch`'s source, and an empty one is an\n\
         \x20* instruction set whose intrinsics are all still unstable there. Needs the\n\
         \x20* instruction set in `__attribute__((target(\"…\")))` on the function that\n\
         \x20* calls them, and a function that passes or returns a 512-bit vector by\n\
         \x20* value needs `target(\"avx512f\")`; see <immintrin.h>.\n\
         \x20*/\n\
         #ifndef {guard}\n\
         #if !defined(__i386__) && !defined(__x86_64__)\n\
         #error \"the Intel intrinsics headers are x86 only; this unit is being translated for \
         another architecture. Guard the #include with #ifdef __x86_64__, or see \
         doc/features.md, 'SIMD intrinsics'.\"\n\
         #elif !defined(_CINRS_IMMINTRIN_H)\n\
         /* On its own: <immintrin.h> includes this file back, in its place. */\n\
         #include <immintrin.h>\n\
         #else\n\
         #define {guard}\n\n",
        sets.join(", ").replace("-vl", "+VL"),
    );
    if let Some((_, typedefs)) = PREAMBLES.iter().find(|(h, _)| *h == header) {
        out.push_str(typedefs);
        out.push('\n');
    }
    if constants {
        out.push_str(&format!("{}\n{REGION_END}\n\n", region_begin("constants")));
    }
    for region in regions.keys() {
        out.push_str(&format!("{}\n{REGION_END}\n\n", region_begin(region)));
    }
    out.push_str(&format!("#endif\n#endif /* {guard} */\n"));
    out
}

// ---------------------------------------------------------------------------
// the tests
// ---------------------------------------------------------------------------

/// Rewrites the generated regions of the bundled headers and the whole of
/// `src/x86/table.rs` from the installed `stdarch` source.
#[test]
#[ignore = "maintainer tool: rewrites the bundled headers and src/x86/table.rs"]
fn regenerate() {
    let Some(src) = core_arch_src() else {
        panic!(
            "no stdarch source: run `rustup component add rust-src`, or set CINRS_STDARCH to a \
             core_arch/src directory"
        );
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    let mut all: Vec<Intrinsic> = Vec::new();
    let mut skipped: Vec<Skipped> = Vec::new();
    // header -> region -> lines
    let mut regions: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    let mut consts: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();

    for family in FAMILIES {
        for (dir, x86_64_only) in [("x86", false), ("x86_64", true)] {
            let Some(text) = family.stems().iter().find_map(|stem| {
                std::fs::read_to_string(src.join(dir).join(format!("{stem}.rs"))).ok()
            }) else {
                continue;
            };
            if !x86_64_only {
                let found = constants(&text);
                if !found.is_empty() {
                    consts
                        .entry(family.header.to_owned())
                        .or_default()
                        .extend(found);
                }
            }
            let mut found: Vec<Intrinsic> = Vec::new();
            for (attrs, sig) in scan(&text) {
                match convert(&attrs, &sig, x86_64_only) {
                    Ok(intr) => found.push(intr),
                    Err(why) => skipped.push(why),
                }
            }
            found.sort_by(|a, b| a.name.cmp(&b.name));
            // The family's own region exists even when nothing is declared in
            // it, so that a header whose every intrinsic is unstable is still
            // written (empty) rather than missing.
            regions
                .entry(family.header.to_owned())
                .or_default()
                .entry(family.region.to_owned())
                .or_default();
            let mut routed: BTreeMap<(String, String), Vec<&Intrinsic>> = BTreeMap::new();
            for intr in &found {
                routed
                    .entry(route(family, &intr.feature))
                    .or_default()
                    .push(intr);
            }
            for ((header, region), intrs) in routed {
                let lines = regions
                    .entry(header)
                    .or_default()
                    .entry(region)
                    .or_default();
                if x86_64_only {
                    lines.push("#ifdef __x86_64__".to_owned());
                }
                for intr in intrs {
                    lines.push(declaration(intr));
                }
                if x86_64_only {
                    lines.push("#endif".to_owned());
                }
            }
            all.extend(found);
        }
    }

    // A reader that stops recognising `core::arch`'s declarations would
    // otherwise shrink the table without a word: the count may only grow.
    let previous = std::fs::read_to_string(root.join("src/x86/table.rs"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.trim().starts_with("Intrinsic { name: \""))
        .count();
    let mut names: Vec<&str> = all.iter().map(|intr| intr.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert!(
        names.len() >= previous,
        "the generator found {} intrinsics where the committed table has {previous}: it no \
         longer reads this stdarch's declarations (a new `pub … fn` form?)",
        names.len()
    );

    // Every name of the GCC-derived table is one this run declared, so a typo
    // or an intrinsic that went away cannot leave a stale row behind.
    for (_, _, void_names) in GCC_VOID_POINTERS {
        for name in *void_names {
            assert!(
                names.binary_search(name).is_ok(),
                "GCC_VOID_POINTERS names '{name}', which is not declared"
            );
        }
    }

    for (header, by_region) in &regions {
        let path = root.join("include").join(header);
        // A header the generator wrote from a skeleton has no hand-written
        // text, so it is rewritten whole, and a change to [`skeleton`] reaches
        // every such header; the hand-written ones only have their regions
        // replaced.
        let owned =
            std::fs::read_to_string(&path).map_or(true, |text| text.contains(SKELETON_MARK));
        if owned {
            std::fs::write(
                &path,
                skeleton(header, by_region, consts.contains_key(header)),
            )
            .unwrap();
        }
        let mut text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for (region, lines) in by_region {
            text = splice(&text, region, &format!("{}\n", lines.join("\n")));
        }
        if let Some(values) = consts.get(header) {
            let body: String = values
                .iter()
                .map(|(name, value)| format!("#define {name} {value}\n"))
                .collect();
            text = splice(&text, "constants", &body);
        }
        std::fs::write(&path, text).unwrap();
    }

    all.sort_by(|a, b| a.name.cmp(&b.name));
    all.dedup_by(|a, b| a.name == b.name);
    std::fs::write(root.join("src/x86/table.rs"), table_source(&all)).unwrap();

    skipped.sort_by(|a, b| (&a.reason, &a.name).cmp(&(&b.reason, &b.name)));
    let mut by_reason: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for s in &skipped {
        by_reason
            .entry(s.reason.as_str())
            .or_default()
            .push(s.name.as_str());
    }
    println!("{} intrinsics declared", all.len());
    for (reason, names) in &by_reason {
        println!(
            "  skipped ({reason}): {} — {}",
            names.len(),
            names.join(" ")
        );
    }
}

/// The text of `src/x86/table.rs`.
fn table_source(all: &[Intrinsic]) -> String {
    let mut out = String::new();
    out.push_str(
        "//! The x86 intrinsics `cinrs` maps onto `core::arch`, by name.\n\
         //!\n\
         //! **Generated** by `crates/cinrs-core/tests/x86_intrinsics.rs` from the\n\
         //! `stdarch` source the standard library is built from; do not edit. The\n\
         //! bundled `<immintrin.h>` and its companions declare exactly these names,\n\
         //! and the same generator writes both, so a prototype and its mapping cannot\n\
         //! disagree.\n\
         //!\n\
         //! Sorted by name: [`super::lookup`] is a binary search.\n\n\
         use super::{Imm, Intrinsic};\n\n\
         /// Every intrinsic this crate knows, sorted by name.\n\
         ///\n\
         /// One line per entry, which `rustfmt` would otherwise turn into six\n\
         /// thousand: the table is read as a table, and `header_and_table_agree`\n\
         /// parses these lines.\n\
         #[rustfmt::skip]\n\
         pub(super) static INTRINSICS: &[Intrinsic] = &[\n",
    );
    for intr in all {
        let imm: String = intr
            .imm
            .iter()
            .map(|(index, ty)| format!("Imm {{ index: {index}, rust_ty: \"{ty}\" }}, "))
            .collect();
        let imm = if imm.is_empty() {
            "&[]".to_owned()
        } else {
            format!("&[{}]", imm.trim_end().trim_end_matches(','))
        };
        let _ = writeln!(
            out,
            "    Intrinsic {{ name: \"{}\", feature: \"{}\", arity: {}, imm: {imm}, \
             x86_64_only: {} }},",
            intr.name,
            intr.feature,
            intr.params.len(),
            intr.x86_64_only,
        );
    }
    out.push_str("];\n");
    out
}

/// The committed header and the committed table declare the same names.
///
/// This is the test that runs in CI: it needs no `rust-src` and no network, and
/// it is what catches a hand edit to one of the two generated files.
#[test]
fn header_and_table_agree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let table = std::fs::read_to_string(root.join("src/x86/table.rs")).unwrap();

    let mut from_table: BTreeMap<String, (usize, Vec<usize>)> = BTreeMap::new();
    for line in table.lines() {
        let Some(rest) = line.trim().strip_prefix("Intrinsic { name: \"") else {
            continue;
        };
        let (name, rest) = rest.split_once('"').unwrap();
        let arity: usize = rest
            .split_once("arity: ")
            .and_then(|(_, r)| r.split(',').next())
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let imm: Vec<usize> = rest
            .match_indices("index: ")
            .map(|(at, _)| {
                rest[at + "index: ".len()..]
                    .split(',')
                    .next()
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap()
            })
            .collect();
        from_table.insert(name.to_owned(), (arity, imm));
    }
    assert_eq!(
        from_table.len(),
        cinrs_core::x86::count(),
        "the file and the compiled-in table disagree"
    );
    // `doc/features.md` prints the count per instruction set and the total, and
    // a regeneration that changes either leaves that table stale. The number is
    // here so that the prose cannot quietly go out of date.
    assert_eq!(
        from_table.len(),
        6075,
        "the intrinsics table changed size: update the table in doc/features.md, \
         \"SIMD intrinsics\", and this number"
    );
    let mut by_feature: BTreeMap<&str, usize> = BTreeMap::new();
    for intr in cinrs_core::x86::all() {
        *by_feature.entry(intr.feature).or_default() += 1;
    }
    assert_eq!(
        by_feature,
        BTreeMap::from([
            ("", 1),
            ("aes", 6),
            ("avx", 184),
            ("avx2", 193),
            ("avx512bf16,avx512f", 12),
            ("avx512bf16,avx512vl", 24),
            ("avx512bitalg", 8),
            ("avx512bitalg,avx512vl", 16),
            ("avx512bw", 318),
            ("avx512bw,avx512vl", 510),
            ("avx512cd", 14),
            ("avx512cd,avx512vl", 28),
            ("avx512dq", 222),
            ("avx512dq,avx512vl", 177),
            ("avx512f", 1421),
            ("avx512f,avx512vl", 1212),
            ("avx512fp16", 551),
            ("avx512fp16,avx512vl", 342),
            ("avx512ifma", 6),
            ("avx512ifma,avx512vl", 12),
            ("avx512vbmi", 10),
            ("avx512vbmi,avx512vl", 20),
            ("avx512vbmi2", 50),
            ("avx512vbmi2,avx512vl", 100),
            ("avx512vnni", 12),
            ("avx512vnni,avx512vl", 24),
            ("avx512vpopcntdq", 6),
            ("avx512vpopcntdq,avx512vl", 12),
            ("avxifma", 4),
            ("avxvnni", 8),
            ("avxvnniint16", 12),
            ("avxvnniint8", 12),
            ("bmi1", 17),
            ("bmi2", 6),
            ("f16c", 4),
            ("fma", 32),
            ("gfni", 3),
            ("gfni,avx", 3),
            ("gfni,avx512bw,avx512f", 6),
            ("gfni,avx512bw,avx512vl", 12),
            ("gfni,avx512f", 3),
            ("lzcnt", 2),
            ("pclmulqdq", 1),
            ("popcnt", 2),
            ("sha", 7),
            ("sha512,avx", 3),
            ("sm3,avx", 3),
            ("sm4,avx", 4),
            ("sse", 97),
            ("sse2", 226),
            ("sse3", 11),
            ("sse4.1", 61),
            ("sse4.2", 19),
            ("ssse3", 16),
            ("vaes", 4),
            ("vaes,avx512f", 4),
            ("vpclmulqdq", 1),
            ("vpclmulqdq,avx512f", 1),
        ]),
        "the per-instruction-set counts in doc/features.md are these"
    );

    let mut from_headers: BTreeMap<String, (usize, Vec<usize>)> = BTreeMap::new();
    // Every bundled header with a generated region, so that a header the
    // generator adds is checked without being listed here.
    let mut headers: Vec<PathBuf> = std::fs::read_dir(root.join("include"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "h"))
        .collect();
    headers.sort();
    for header in &headers {
        let text = std::fs::read_to_string(header).unwrap();
        let mut generated = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("/* @generated") {
                generated = !line.contains("end");
                continue;
            }
            if !generated || !line.ends_with(");") {
                continue;
            }
            let Some(open) = line.find('(') else { continue };
            let name = line[..open].rsplit([' ', '*']).next().unwrap().to_owned();
            let args = &line[open + 1..line.len() - 2];
            let params: Vec<&str> = if args.trim() == "void" {
                Vec::new()
            } else {
                args.split(',').collect()
            };
            let imm = params
                .iter()
                .enumerate()
                // An immediate is spelled `const int`; `const int *` is a
                // pointer to constant memory, which is a runtime operand.
                .filter(|(_, p)| matches!(p.trim(), "const int" | "const unsigned int"))
                .map(|(index, _)| index)
                .collect();
            from_headers.insert(name, (params.len(), imm));
        }
    }

    let only_table: Vec<&String> = from_table
        .keys()
        .filter(|name| !from_headers.contains_key(*name))
        .collect();
    let only_header: Vec<&String> = from_headers
        .keys()
        .filter(|name| !from_table.contains_key(*name))
        .collect();
    assert!(
        only_table.is_empty() && only_header.is_empty(),
        "the table and the headers disagree; in the table only: {only_table:?}; in the headers \
         only: {only_header:?}"
    );
    for (name, entry) in &from_table {
        assert_eq!(
            Some(entry),
            from_headers.get(name),
            "'{name}' has a different arity or immediate position in the header"
        );
    }
}
