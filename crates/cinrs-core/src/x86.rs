//! The x86 SIMD intrinsics, and how a C name becomes a `core::arch` one.
//!
//! Intel's intrinsics are the API real C uses for SSE and AVX — `#include
//! <immintrin.h>`, `__m128i`, `_mm_add_epi32`, `_mm256_loadu_si256` — and
//! Rust's `core::arch::x86_64` has *the same names with the same signatures*,
//! because it was generated from the same Intel data. So `cinrs` needs no
//! vector language of its own: the bundled headers declare the prototypes, the
//! generated table in `src/x86/table.rs` says which names are intrinsics, and a
//! call to one is
//! generated as `::core::arch::x86_64::_mm_add_epi32(a, b)` rather than as a
//! call to a symbol that does not exist.
//!
//! Three things have to come out of this module for that to work.
//!
//! * **Which names.** [`lookup`] answers, over a table generated from
//!   `core::arch`'s own source by `crates/cinrs-core/tests/x86_intrinsics.rs`
//!   — the same generator that writes the headers' prototypes, so a
//!   declaration and its mapping cannot drift apart. A name is only treated
//!   this way on an x86 target, and only for a function the unit does not
//!   define itself.
//!
//! * **Which operands are immediates.** Intel requires the last operand of
//!   `_mm_slli_epi32(v, 3)`, `_mm_shuffle_epi32(v, m)` and the rest to be an
//!   integer constant expression, and `core::arch` expresses that as a `const`
//!   generic: `_mm_slli_epi32::<3>(v)`. [`Intrinsic::imm`] records where those
//!   operands are in the C argument list — `core::arch` carries exactly that
//!   in its own `#[rustc_legacy_const_generics]` — so sema can fold the
//!   argument and code generation can write the turbofish.
//!
//! * **Which target feature.** Each intrinsic is a `#[target_feature(enable =
//!   "…")]` function in `core::arch`, which is why calling one needs an
//!   `unsafe` block (which every generated body already is) or a caller that
//!   has the feature. [`Intrinsic::feature`] is that name, and
//!   [`target_features`] maps GCC's spelling of it —
//!   `__attribute__((target("avx2")))` — onto Rust's.
//!
//! What is deliberately not here: AVX-512 and the `__mmask*` types, which are
//! still unstable in `core::arch` on this crate's minimum supported Rust
//! version, and MMX and `__m64`, which the standard library dropped. See
//! `doc/features.md`.

mod table;

/// One intrinsic: what `core::arch` calls it and what a C call has to supply.
#[derive(Clone, Copy, Debug)]
pub struct Intrinsic {
    /// The name, which is the same in C and in `core::arch`.
    pub name: &'static str,
    /// The `#[target_feature]` the `core::arch` function carries, in Rust's
    /// spelling: `sse2`, `sse4.1`, `avx2`, `bmi1`, `lzcnt`, …
    pub feature: &'static str,
    /// How many arguments the C prototype takes, immediates included.
    pub arity: u8,
    /// The operands that are `const` generics in Rust, in C argument order.
    pub imm: &'static [Imm],
    /// Whether `core::arch` only has it under `x86_64` — the bundled header
    /// hides those behind `#ifdef __x86_64__`.
    pub x86_64_only: bool,
}

/// One immediate operand: where it is, and what Rust's `const` generic is.
#[derive(Clone, Copy, Debug)]
pub struct Imm {
    /// The zero-based index of the argument in the C call.
    pub index: u8,
    /// The Rust type of the `const` parameter, which decides the suffix the
    /// generated literal carries: `i32` or `u32`.
    pub rust_ty: &'static str,
}

impl Intrinsic {
    /// Whether the argument at `index` has to be an integer constant
    /// expression.
    pub fn immediate_at(&self, index: usize) -> Option<&'static Imm> {
        self.imm.iter().find(|imm| usize::from(imm.index) == index)
    }
}

/// The intrinsic of this name, if there is one.
///
/// The table is sorted, so this is a binary search; it is asked once per
/// declaration and once per call.
pub fn lookup(name: &str) -> Option<&'static Intrinsic> {
    table::INTRINSICS
        .binary_search_by(|probe| probe.name.cmp(name))
        .ok()
        .map(|index| &table::INTRINSICS[index])
}

/// How many intrinsics the table holds, for the documentation test that keeps
/// the prose honest.
pub fn count() -> usize {
    table::INTRINSICS.len()
}

/// Every intrinsic, for the tests that check the table against the headers.
pub fn all() -> &'static [Intrinsic] {
    table::INTRINSICS
}

// ---------------------------------------------------------------------------
// target features
// ---------------------------------------------------------------------------

/// GCC's name for an instruction set, and Rust's.
///
/// `__attribute__((target("avx2")))` and `#pragma GCC target("sse4.2,popcnt")`
/// name the instruction set the way GCC's `-m` switches do, and
/// `#[target_feature(enable = "…")]` names it the way LLVM does. Most of them
/// agree; these are the ones that matter, and the three that do not are `bmi`
/// (Rust's `bmi1`), `pclmul` (Rust's `pclmulqdq`) and `abm`, which is GCC's
/// name for LZCNT and POPCNT together.
///
/// Every Rust name here is one `rustc` accepts on its **stable** channel and
/// `is_x86_feature_detected!` knows; an unstable one — `avx512f`, `sse4a`,
/// `gfni` — would turn a `target` attribute into a nightly-only build, so it
/// is refused with the reason instead. The list is checked against `rustc` by
/// `tests/simd.rs`.
pub const TARGET_FEATURES: &[(&str, &[&str])] = &[
    // GCC's ABM is LZCNT plus POPCNT; LLVM splits them.
    ("abm", &["lzcnt", "popcnt"]),
    ("adx", &["adx"]),
    ("aes", &["aes"]),
    ("avx", &["avx"]),
    ("avx2", &["avx2"]),
    ("bmi", &["bmi1"]),
    ("bmi2", &["bmi2"]),
    ("cx16", &["cmpxchg16b"]),
    ("f16c", &["f16c"]),
    ("fma", &["fma"]),
    ("fxsr", &["fxsr"]),
    ("lzcnt", &["lzcnt"]),
    ("movbe", &["movbe"]),
    ("pclmul", &["pclmulqdq"]),
    ("popcnt", &["popcnt"]),
    ("rdrnd", &["rdrand"]),
    ("rdseed", &["rdseed"]),
    ("sha", &["sha"]),
    ("sse", &["sse"]),
    ("sse2", &["sse2"]),
    ("sse3", &["sse3"]),
    // GCC's `-msse4` is both halves of SSE4.
    ("sse4", &["sse4.1", "sse4.2"]),
    ("sse4.1", &["sse4.1"]),
    ("sse4.2", &["sse4.2"]),
    ("ssse3", &["ssse3"]),
    ("xsave", &["xsave"]),
];

/// The instruction sets that exist but that this crate cannot ask for, with
/// the reason — which is what a `target` attribute naming one is told.
pub const UNSUPPORTED_FEATURES: &[(&str, &str)] = &[
    (
        "3dnow",
        "3DNow! was removed from every compiler and from Rust's core::arch",
    ),
    (
        "mmx",
        "Rust's core::arch has no MMX and no '__m64'; the SSE2 forms in \
         <emmintrin.h> do the same work",
    ),
    (
        "sse4a",
        "the SSE4a target feature is still unstable in rustc, so asking for it \
         would make this a nightly-only build",
    ),
    (
        "fma4",
        "Rust's core::arch has no FMA4; Intel's 'fma' is the portable one",
    ),
    ("xop", "Rust's core::arch has no XOP"),
    ("tbm", "the TBM target feature is still unstable in rustc"),
    ("rtm", "the RTM target feature is still unstable in rustc"),
];

/// The Rust target features GCC's name for an instruction set means.
pub fn target_features(gcc: &str) -> Option<&'static [&'static str]> {
    feature_row(gcc).map(|index| TARGET_FEATURES[index].1)
}

/// The row of [`TARGET_FEATURES`] GCC's name for an instruction set is in.
///
/// It is what [`crate::ir::BuiltinOp::CpuSupports`] carries, because a row
/// index is one byte and a slice reference is sixteen.
pub fn feature_row(gcc: &str) -> Option<usize> {
    // GCC accepts `sse4_1` as well as `sse4.1` in a few places, and so do
    // several code bases that write the attribute.
    let name = gcc.trim();
    TARGET_FEATURES
        .iter()
        .position(|(n, _)| *n == name || n.replace('.', "_") == name)
}

/// A row index has to fit in the byte
/// [`crate::ir::BuiltinOp::CpuSupports`] carries.
const _: () = assert!(
    TARGET_FEATURES.len() <= u8::MAX as usize,
    "TARGET_FEATURES has outgrown the byte BuiltinOp::CpuSupports carries"
);

/// The Rust target features the row `index` of [`TARGET_FEATURES`] names.
pub fn detect_features(index: u8) -> &'static [&'static str] {
    TARGET_FEATURES
        .get(usize::from(index))
        .map_or(&[][..], |(_, rust)| *rust)
}

/// Why `target("…")` naming this instruction set is refused.
pub fn unsupported_feature(gcc: &str) -> Option<&'static str> {
    let name = gcc.trim();
    UNSUPPORTED_FEATURES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, why)| *why)
        .or_else(|| {
            // Everything AVX-512 is one answer, and there are forty names.
            name.starts_with("avx512").then_some(
                "AVX-512 is still unstable in rustc's core::arch on the Rust \
                 version this crate supports, so cinrs does not map it",
            )
        })
}

/// The instruction sets [`target_features`] knows, for a diagnostic that lists
/// them.
pub fn feature_names() -> Vec<&'static str> {
    TARGET_FEATURES.iter().map(|(name, _)| *name).collect()
}
