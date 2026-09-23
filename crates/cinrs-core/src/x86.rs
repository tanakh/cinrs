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
//! AVX-512 is here as the rest is: `core::arch` stabilised it in Rust 1.89
//! (AVX512-FP16 in 1.94), and the `__m512*`, `__m…bh` and `__m…h` vectors and
//! the `__mmask*` integers come from the same generated headers. A feature
//! such as `"gfni,avx512bw,avx512vl"` is a comma-separated list, which
//! `#[target_feature(enable = …)]` takes as it is.
//!
//! What is deliberately not here: the intrinsics `core::arch` still keeps
//! unstable — AVX512-VP2INTERSECT, and the FP16 and BF16 ones that take or
//! return a scalar `f16` or `bf16` — and MMX and `__m64`, which the standard
//! library dropped. See `doc/features.md`.

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
/// `is_x86_feature_detected!` knows; an unstable one — `sse4a`, `tbm`, `rtm` —
/// would turn a `target` attribute into a nightly-only build, so it is refused
/// with the reason instead. The list is checked against `rustc` by
/// `tests/simd.rs`. Every AVX-512 name, and each instruction set that arrived
/// with it, is spelled the same by GCC 15 (in `target` and in
/// `__builtin_cpu_supports` alike) and by Rust 1.89 and later.
pub const TARGET_FEATURES: &[(&str, &[&str])] = &[
    // GCC's ABM is LZCNT plus POPCNT; LLVM splits them.
    ("abm", &["lzcnt", "popcnt"]),
    ("adx", &["adx"]),
    ("aes", &["aes"]),
    ("avx", &["avx"]),
    ("avx2", &["avx2"]),
    ("avx512bf16", &["avx512bf16"]),
    ("avx512bitalg", &["avx512bitalg"]),
    ("avx512bw", &["avx512bw"]),
    ("avx512cd", &["avx512cd"]),
    ("avx512dq", &["avx512dq"]),
    ("avx512f", &["avx512f"]),
    ("avx512fp16", &["avx512fp16"]),
    ("avx512ifma", &["avx512ifma"]),
    ("avx512vbmi", &["avx512vbmi"]),
    ("avx512vbmi2", &["avx512vbmi2"]),
    ("avx512vl", &["avx512vl"]),
    ("avx512vnni", &["avx512vnni"]),
    // No intrinsics (they are unstable in `core::arch`), but the instruction
    // set is a stable target feature and a fair question to ask the CPU.
    ("avx512vp2intersect", &["avx512vp2intersect"]),
    ("avx512vpopcntdq", &["avx512vpopcntdq"]),
    ("avxifma", &["avxifma"]),
    ("avxneconvert", &["avxneconvert"]),
    ("avxvnni", &["avxvnni"]),
    ("avxvnniint16", &["avxvnniint16"]),
    ("avxvnniint8", &["avxvnniint8"]),
    ("bmi", &["bmi1"]),
    ("bmi2", &["bmi2"]),
    ("cx16", &["cmpxchg16b"]),
    ("f16c", &["f16c"]),
    ("fma", &["fma"]),
    ("fxsr", &["fxsr"]),
    ("gfni", &["gfni"]),
    ("kl", &["kl"]),
    ("lzcnt", &["lzcnt"]),
    ("movbe", &["movbe"]),
    ("pclmul", &["pclmulqdq"]),
    ("popcnt", &["popcnt"]),
    ("rdrnd", &["rdrand"]),
    ("rdseed", &["rdseed"]),
    ("sha", &["sha"]),
    ("sha512", &["sha512"]),
    ("sm3", &["sm3"]),
    ("sm4", &["sm4"]),
    ("sse", &["sse"]),
    ("sse2", &["sse2"]),
    ("sse3", &["sse3"]),
    // GCC's `-msse4` is both halves of SSE4.
    ("sse4", &["sse4.1", "sse4.2"]),
    ("sse4.1", &["sse4.1"]),
    ("sse4.2", &["sse4.2"]),
    ("ssse3", &["ssse3"]),
    ("vaes", &["vaes"]),
    ("vpclmulqdq", &["vpclmulqdq"]),
    ("widekl", &["widekl"]),
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
}

/// An intrinsic's feature string as prose: `'sse2'` is "the 'sse2'
/// instruction set", and the comma-separated `'gfni,avx512bw,avx512vl'` is
/// "the 'gfni', 'avx512bw' and 'avx512vl' instruction sets".
pub fn describe_features(feature: &str) -> String {
    let names: Vec<String> = feature
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| format!("'{name}'"))
        .collect();
    match names.as_slice() {
        [] => "an instruction set".to_owned(),
        [one] => format!("the {one} instruction set"),
        [init @ .., last] => format!("the {} and {last} instruction sets", init.join(", ")),
    }
}

/// The macros `#pragma GCC target("…")` defines, per GCC name: the name's own
/// macros, and the names it implies, whose macros it defines too.
///
/// Read off GCC 15.2 once, as `gcc -m<name> -dM -E -x c /dev/null` against the
/// same command without the switch, for every name in [`TARGET_FEATURES`] (and
/// `crc32`, which `sse4.2` implies). What that prints besides the feature
/// macros is left out: `__BIGGEST_ALIGNMENT__` (a value, which the pragma does
/// not change in GCC either), `__FP_FAST_FMA*` and `__FLT_EVAL_METHOD*` (which
/// describe code generation cinrs does not do), and `cx16`'s
/// `__GCC_HAVE_SYNC_COMPARE_AND_SWAP_16` (an atomics claim). The implications
/// are GCC's, which are not always the obvious ones: `sse4.2` brings
/// `__POPCNT__` and `__CRC32__`, `avx` brings `__XSAVE__`, `avx512fp16` needs
/// only `avx512bw`, and `avx512vp2intersect` brings `avx512dq`.
pub const TARGET_MACROS: &[(&str, &[&str], &[&str])] = &[
    ("abm", &["__ABM__"], &["lzcnt", "popcnt"]),
    ("adx", &["__ADX__"], &[]),
    ("aes", &["__AES__"], &["sse2"]),
    ("avx", &["__AVX__"], &["sse4.2", "xsave"]),
    ("avx2", &["__AVX2__"], &["avx"]),
    ("avx512bf16", &["__AVX512BF16__"], &["avx512bw"]),
    ("avx512bitalg", &["__AVX512BITALG__"], &["avx512bw"]),
    ("avx512bw", &["__AVX512BW__"], &["avx512f"]),
    ("avx512cd", &["__AVX512CD__"], &["avx512f"]),
    ("avx512dq", &["__AVX512DQ__"], &["avx512f"]),
    ("avx512f", &["__AVX512F__", "__EVEX512__"], &["avx2"]),
    ("avx512fp16", &["__AVX512FP16__"], &["avx512bw"]),
    ("avx512ifma", &["__AVX512IFMA__"], &["avx512f"]),
    ("avx512vbmi", &["__AVX512VBMI__"], &["avx512bw"]),
    ("avx512vbmi2", &["__AVX512VBMI2__"], &["avx512bw"]),
    ("avx512vl", &["__AVX512VL__", "__EVEX256__"], &["avx512f"]),
    ("avx512vnni", &["__AVX512VNNI__"], &["avx512f"]),
    (
        "avx512vp2intersect",
        &["__AVX512VP2INTERSECT__"],
        &["avx512dq"],
    ),
    ("avx512vpopcntdq", &["__AVX512VPOPCNTDQ__"], &["avx512f"]),
    ("avxifma", &["__AVXIFMA__"], &["avx2"]),
    ("avxneconvert", &["__AVXNECONVERT__"], &["avx2"]),
    ("avxvnni", &["__AVXVNNI__"], &["avx2"]),
    ("avxvnniint16", &["__AVXVNNIINT16__"], &["avx2"]),
    ("avxvnniint8", &["__AVXVNNIINT8__"], &["avx2"]),
    ("bmi", &["__BMI__"], &[]),
    ("bmi2", &["__BMI2__"], &[]),
    ("crc32", &["__CRC32__"], &[]),
    ("cx16", &[], &[]),
    ("f16c", &["__F16C__"], &["avx"]),
    ("fma", &["__FMA__"], &["avx"]),
    ("fxsr", &["__FXSR__"], &[]),
    ("gfni", &["__GFNI__"], &["sse2"]),
    ("kl", &["__KL__"], &[]),
    ("lzcnt", &["__LZCNT__"], &[]),
    ("movbe", &["__MOVBE__"], &[]),
    ("pclmul", &["__PCLMUL__"], &["sse2"]),
    ("popcnt", &["__POPCNT__"], &[]),
    ("rdrnd", &["__RDRND__"], &[]),
    ("rdseed", &["__RDSEED__"], &[]),
    ("sha", &["__SHA__"], &["sse2"]),
    ("sha512", &["__SHA512__"], &["avx"]),
    ("sm3", &["__SM3__"], &["avx"]),
    ("sm4", &["__SM4__"], &["avx"]),
    ("sse", &["__SSE__"], &[]),
    ("sse2", &["__SSE2__"], &["sse"]),
    ("sse3", &["__SSE3__"], &["sse2"]),
    ("sse4", &[], &["sse4.2"]),
    ("sse4.1", &["__SSE4_1__"], &["ssse3"]),
    ("sse4.2", &["__SSE4_2__"], &["sse4.1", "popcnt", "crc32"]),
    ("ssse3", &["__SSSE3__"], &["sse3"]),
    ("vaes", &["__VAES__"], &["avx"]),
    ("vpclmulqdq", &["__VPCLMULQDQ__"], &["avx", "pclmul"]),
    ("widekl", &["__WIDEKL__"], &["kl"]),
    ("xsave", &["__XSAVE__"], &[]),
];

/// The row of [`TARGET_MACROS`] for a GCC name, `sse4_1` spelled either way.
fn macro_row(
    gcc: &str,
) -> Option<&'static (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
)> {
    let name = gcc.trim();
    TARGET_MACROS
        .iter()
        .find(|(n, _, _)| *n == name || n.replace('.', "_") == name)
}

/// Every macro `target(gcc)` defines: its own and those of everything it
/// implies, transitively.
fn implied_macros(gcc: &str, out: &mut std::collections::BTreeSet<&'static str>) {
    if let Some((_, own, implies)) = macro_row(gcc) {
        out.extend(own.iter().copied());
        for name in *implies {
            implied_macros(name, out);
        }
    }
}

/// The feature macros in force after `#pragma GCC target` has named `names`,
/// in order, starting from `baseline` (`__SSE__` and `__SSE2__` on x86-64).
///
/// A name adds its macros and everything it implies. `no-X` takes away X's own
/// macros and those of every name that implies X, as GCC's `-mno-X` does — so
/// `no-avx2` removes `__AVX2__` and the AVX-512 family and keeps `__AVX__`. A
/// name this table does not know (`arch=haswell`, a misspelling) defines
/// nothing; sema is where it is refused.
pub fn target_macros(
    baseline: &[&'static str],
    names: &[&str],
) -> std::collections::BTreeSet<&'static str> {
    let mut set: std::collections::BTreeSet<&'static str> = baseline.iter().copied().collect();
    for name in names {
        let name = name.trim();
        if let Some(off) = name.strip_prefix("no-") {
            let Some((off_name, _, _)) = macro_row(off) else {
                continue;
            };
            for (row, own, _) in TARGET_MACROS {
                let mut closure = std::collections::BTreeSet::new();
                implies_name(row, off_name, &mut closure);
                if !closure.is_empty() {
                    for m in *own {
                        set.remove(m);
                    }
                }
            }
        } else {
            implied_macros(name, &mut set);
        }
    }
    set
}

/// Records `target` in `found` if `row` is `target` or implies it.
fn implies_name(row: &str, target: &str, found: &mut std::collections::BTreeSet<&'static str>) {
    let Some((name, _, implies)) = macro_row(row) else {
        return;
    };
    if *name == target {
        found.insert(name);
        return;
    }
    for next in *implies {
        implies_name(next, target, found);
    }
}

/// The instruction sets [`target_features`] knows, for a diagnostic that lists
/// them.
pub fn feature_names() -> Vec<&'static str> {
    TARGET_FEATURES.iter().map(|(name, _)| *name).collect()
}
