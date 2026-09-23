//! What the tests share: the four `xxhash.c` units behind one set of function
//! pointers, upstream's sanity buffer, and a reader for
//! `tests/sanity_test_vectors.h`.

#![allow(dead_code)]

use cinrs_xxhash_fixture as fx;

/// One copy of the xxHash API. `size_t` is C's `unsigned long` here, a `u64`.
pub struct Unit {
    pub name: &'static str,
    /// The instruction set the unit's kernel needs, if any beyond the baseline.
    pub needs: Option<&'static str>,
    pub xxh32: fn(&[u8], u32) -> u32,
    pub xxh64: fn(&[u8], u64) -> u64,
    pub xxh3_64: fn(&[u8]) -> u64,
    pub xxh3_64_seed: fn(&[u8], u64) -> u64,
    pub xxh3_128: fn(&[u8]) -> (u64, u64),
    pub xxh3_128_seed: fn(&[u8], u64) -> (u64, u64),
    /// `XXH3_createState`, `XXH3_64bits_reset_withSeed`, `update` in pieces of
    /// the given size, `digest`, `XXH3_freeState`.
    pub stream64: fn(&[u8], u64, usize) -> u64,
    pub stream128: fn(&[u8], u64, usize) -> (u64, u64),
}

/// A pointer for `data`, null for an empty input as upstream's sanity test
/// passes it.
pub fn ptr(data: &[u8]) -> *const std::ffi::c_void {
    if data.is_empty() {
        std::ptr::null()
    } else {
        data.as_ptr().cast()
    }
}

macro_rules! unit {
    ($name:literal, $needs:expr, $m:ident,
     $xxh32:ident, $xxh64:ident, $x3:ident, $x3s:ident, $x128:ident, $x128s:ident,
     $create:ident, $free:ident,
     $r64:ident, $u64:ident, $d64:ident, $r128:ident, $u128:ident, $d128:ident) => {
        Unit {
            name: $name,
            needs: $needs,
            xxh32: |d, s| unsafe { fx::$m::$xxh32(ptr(d), d.len() as _, s) },
            xxh64: |d, s| unsafe { fx::$m::$xxh64(ptr(d), d.len() as _, s) },
            xxh3_64: |d| unsafe { fx::$m::$x3(ptr(d), d.len() as _) },
            xxh3_64_seed: |d, s| unsafe { fx::$m::$x3s(ptr(d), d.len() as _, s) },
            xxh3_128: |d| {
                let h = unsafe { fx::$m::$x128(ptr(d), d.len() as _) };
                (h.low64, h.high64)
            },
            xxh3_128_seed: |d, s| {
                let h = unsafe { fx::$m::$x128s(ptr(d), d.len() as _, s) };
                (h.low64, h.high64)
            },
            stream64: |d, s, piece| unsafe {
                let st = fx::$m::$create();
                assert!(!st.is_null());
                let _ = fx::$m::$r64(st, s);
                if d.is_empty() {
                    let _ = fx::$m::$u64(st, ptr(d), 0);
                }
                for c in d.chunks(piece.max(1)) {
                    let _ = fx::$m::$u64(st, c.as_ptr().cast(), c.len() as _);
                }
                let h = fx::$m::$d64(st);
                let _ = fx::$m::$free(st);
                h
            },
            stream128: |d, s, piece| unsafe {
                let st = fx::$m::$create();
                assert!(!st.is_null());
                let _ = fx::$m::$r128(st, s);
                if d.is_empty() {
                    let _ = fx::$m::$u128(st, ptr(d), 0);
                }
                for c in d.chunks(piece.max(1)) {
                    let _ = fx::$m::$u128(st, c.as_ptr().cast(), c.len() as _);
                }
                let h = fx::$m::$d128(st);
                let _ = fx::$m::$free(st);
                (h.low64, h.high64)
            },
        }
    };
}

pub fn units() -> Vec<Unit> {
    vec![
        unit!("scalar", None, scalar,
            scalar_XXH32, scalar_XXH64, scalar_XXH3_64bits, scalar_XXH3_64bits_withSeed,
            scalar_XXH3_128bits, scalar_XXH3_128bits_withSeed,
            scalar_XXH3_createState, scalar_XXH3_freeState,
            scalar_XXH3_64bits_reset_withSeed, scalar_XXH3_64bits_update, scalar_XXH3_64bits_digest,
            scalar_XXH3_128bits_reset_withSeed, scalar_XXH3_128bits_update, scalar_XXH3_128bits_digest),
        unit!("sse2", None, sse2,
            sse2_XXH32, sse2_XXH64, sse2_XXH3_64bits, sse2_XXH3_64bits_withSeed,
            sse2_XXH3_128bits, sse2_XXH3_128bits_withSeed,
            sse2_XXH3_createState, sse2_XXH3_freeState,
            sse2_XXH3_64bits_reset_withSeed, sse2_XXH3_64bits_update, sse2_XXH3_64bits_digest,
            sse2_XXH3_128bits_reset_withSeed, sse2_XXH3_128bits_update, sse2_XXH3_128bits_digest),
        unit!("avx2", Some("avx2"), avx2,
            avx2_XXH32, avx2_XXH64, avx2_XXH3_64bits, avx2_XXH3_64bits_withSeed,
            avx2_XXH3_128bits, avx2_XXH3_128bits_withSeed,
            avx2_XXH3_createState, avx2_XXH3_freeState,
            avx2_XXH3_64bits_reset_withSeed, avx2_XXH3_64bits_update, avx2_XXH3_64bits_digest,
            avx2_XXH3_128bits_reset_withSeed, avx2_XXH3_128bits_update, avx2_XXH3_128bits_digest),
        unit!("avx512", Some("avx512f"), avx512,
            avx512_XXH32, avx512_XXH64, avx512_XXH3_64bits, avx512_XXH3_64bits_withSeed,
            avx512_XXH3_128bits, avx512_XXH3_128bits_withSeed,
            avx512_XXH3_createState, avx512_XXH3_freeState,
            avx512_XXH3_64bits_reset_withSeed, avx512_XXH3_64bits_update, avx512_XXH3_64bits_digest,
            avx512_XXH3_128bits_reset_withSeed, avx512_XXH3_128bits_update, avx512_XXH3_128bits_digest),
    ]
}

/// Whether this machine can run `unit`'s kernel.
pub fn runnable(unit: &Unit) -> bool {
    match unit.needs {
        None => true,
        Some("avx2") => std::is_x86_feature_detected!("avx2"),
        Some("avx512f") => std::is_x86_feature_detected!("avx512f"),
        Some(other) => panic!("unknown feature {other}"),
    }
}

/// The units this machine can run, saying which it skips.
pub fn runnable_units() -> Vec<Unit> {
    units()
        .into_iter()
        .filter(|u| {
            let ok = runnable(u);
            if !ok {
                eprintln!("skipping the {} unit: this machine has no {:?}", u.name, u.needs);
            }
            ok
        })
        .collect()
}

pub const PRIME32: u64 = 2654435761;
pub const PRIME64: u64 = 11400714785074694797;

/// `fillTestBuffer` from `tests/sanity_test.c`, `SANITY_BUFFER_SIZE` long.
pub fn sanity_buffer() -> Vec<u8> {
    let mut byte_gen = PRIME32;
    (0..4096 + 64 + 1)
        .map(|_| {
            let b = (byte_gen >> 56) as u8;
            byte_gen = byte_gen.wrapping_mul(PRIME64);
            b
        })
        .collect()
}

pub const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/xxhash/tests/sanity_test_vectors.h"
);

/// The rows of `static const … <table>[] = { … };`, each row's numbers in
/// order: `{len, seed, result}`, or `{len, seed, {low64, high64}}` for the
/// 128-bit tables.
pub fn table(name: &str) -> Vec<Vec<u64>> {
    let text = std::fs::read_to_string(VECTORS)
        .unwrap_or_else(|e| panic!("{VECTORS}: {e} (run scripts/check-xxhash.sh)"));
    let head = format!(" {name}[] = {{");
    let start = text.find(&head).unwrap_or_else(|| panic!("no table {name}")) + head.len();
    let end = start + text[start..].find("\n};").expect("the end of the table");
    let mut rows = Vec::new();
    for line in text[start..end].lines() {
        let line = line.split("/*").next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let nums: Vec<u64> = line
            .split(|c: char| c == '{' || c == '}' || c == ',' || c.is_whitespace())
            .filter(|t| !t.is_empty())
            .map(|t| {
                let t = t.trim_end_matches(['U', 'L']);
                match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                    Some(h) => u64::from_str_radix(h, 16),
                    None => t.parse(),
                }
                .unwrap_or_else(|e| panic!("{name}: {t:?}: {e}"))
            })
            .collect();
        rows.push(nums);
    }
    rows
}
