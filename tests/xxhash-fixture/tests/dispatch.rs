//! `xxh_x86dispatch.c` through `cinrs`: which kernel it picks, and whether its
//! answers are upstream's table's and the scalar unit's.

mod common;

use cinrs_xxhash_fixture::dispatch::{
    XXH_featureTest, XXH3_64bits_dispatch, XXH3_64bits_withSeed_dispatch, XXH3_128bits_dispatch,
    XXH3_128bits_withSeed_dispatch,
};
use common::{ptr, sanity_buffer, table, units};

#[test]
fn which_variant() {
    let id = unsafe { XXH_featureTest() };
    let name = match id {
        0 => "scalar",
        1 => "SSE2",
        2 => "AVX2",
        3 => "AVX-512",
        _ => "?",
    };
    eprintln!("XXH_featureTest() = {id}: the dispatcher picks {name}");
    eprintln!(
        "host: avx512f={} avx2={}",
        std::is_x86_feature_detected!("avx512f"),
        std::is_x86_feature_detected!("avx2")
    );
}

#[test]
fn dispatch_matches_table_and_scalar() {
    let buf = sanity_buffer();
    let mut bad = 0usize;
    for r in table("XSUM_XXH3_testdata") {
        let d = &buf[..r[0] as usize];
        let got = unsafe { XXH3_64bits_withSeed_dispatch(ptr(d), d.len() as _, r[1]) };
        if got != r[2] {
            bad += 1;
        }
        if r[1] == 0 && unsafe { XXH3_64bits_dispatch(ptr(d), d.len() as _) } != r[2] {
            bad += 1;
        }
    }
    for r in table("XSUM_XXH128_testdata") {
        let d = &buf[..r[0] as usize];
        let h = unsafe { XXH3_128bits_withSeed_dispatch(ptr(d), d.len() as _, r[1]) };
        if (h.low64, h.high64) != (r[2], r[3]) {
            bad += 1;
        }
        if r[1] == 0 {
            let h = unsafe { XXH3_128bits_dispatch(ptr(d), d.len() as _) };
            if (h.low64, h.high64) != (r[2], r[3]) {
                bad += 1;
            }
        }
    }
    let scalar = &units()[0];
    for len in 0..=4096usize {
        let d = &buf[..len];
        if unsafe { XXH3_64bits_dispatch(ptr(d), d.len() as _) } != (scalar.xxh3_64)(d) {
            bad += 1;
        }
    }
    assert_eq!(bad, 0, "dispatcher mismatches");
}
