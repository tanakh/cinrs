//! xxHash through `cinrs`: upstream's sanity table, the four vector widths
//! against each other, and streaming against one-shot.

mod common;

use common::{Unit, runnable_units, sanity_buffer, table};

fn hex128((lo, hi): (u64, u64)) -> String {
    format!("{{{lo:#018x}, {hi:#018x}}}")
}

/// `tests/sanity_test_vectors.h`, for each unit this machine can run: the
/// same checks `tests/sanity_test.c` makes of the one-shot functions, on the
/// same buffer. XXH32 and XXH64 are the same code in every unit (only XXH3 has
/// vector kernels), but each unit's copy is compiled under its own target.
#[test]
fn sanity_table() {
    let buf = sanity_buffer();
    let t32 = table("XSUM_XXH32_testdata");
    let t64 = table("XSUM_XXH64_testdata");
    let t3 = table("XSUM_XXH3_testdata");
    let t128 = table("XSUM_XXH128_testdata");
    eprintln!(
        "table rows: XXH32 {}, XXH64 {}, XXH3-64 {}, XXH3-128 {}",
        t32.len(),
        t64.len(),
        t3.len(),
        t128.len()
    );
    assert!(t32.len() > 1000 && t64.len() > 1000 && t3.len() > 1000 && t128.len() > 1000);
    let mut failures = 0usize;
    for u in runnable_units() {
        let mut checks = 0usize;
        let mut bad = 0usize;
        let mut fail = |what: &str, len: u64, seed: u64, want: String, got: String| {
            if bad < 10 {
                eprintln!("MISMATCH {} {what} len={len} seed={seed:#x}: want {want}, got {got}", u.name);
            }
            bad += 1;
        };
        for r in &t32 {
            let (len, seed, want) = (r[0], r[1], r[2]);
            let got = (u.xxh32)(&buf[..len as usize], seed as u32) as u64;
            checks += 1;
            if got != want {
                fail("XXH32", len, seed, format!("{want:#x}"), format!("{got:#x}"));
            }
        }
        for r in &t64 {
            let (len, seed, want) = (r[0], r[1], r[2]);
            let got = (u.xxh64)(&buf[..len as usize], seed);
            checks += 1;
            if got != want {
                fail("XXH64", len, seed, format!("{want:#x}"), format!("{got:#x}"));
            }
        }
        for r in &t3 {
            let (len, seed, want) = (r[0], r[1], r[2]);
            let data = &buf[..len as usize];
            let got = (u.xxh3_64_seed)(data, seed);
            checks += 1;
            if got != want {
                fail("XXH3_64bits_withSeed", len, seed, format!("{want:#x}"), format!("{got:#x}"));
            }
            if seed == 0 {
                let got = (u.xxh3_64)(data);
                checks += 1;
                if got != want {
                    fail("XXH3_64bits", len, seed, format!("{want:#x}"), format!("{got:#x}"));
                }
            }
        }
        for r in &t128 {
            let (len, seed, want) = (r[0], r[1], (r[2], r[3]));
            let data = &buf[..len as usize];
            let got = (u.xxh3_128_seed)(data, seed);
            checks += 1;
            if got != want {
                fail("XXH3_128bits_withSeed", len, seed, hex128(want), hex128(got));
            }
            if seed == 0 {
                let got = (u.xxh3_128)(data);
                checks += 1;
                if got != want {
                    fail("XXH3_128bits", len, seed, hex128(want), hex128(got));
                }
            }
        }
        eprintln!("{}: {} of {checks} sanity checks passed", u.name, checks - bad);
        failures += bad;
    }
    assert_eq!(failures, 0, "sanity table mismatches; see above");
}

/// A buffer of pseudo-random bytes unrelated to the sanity buffer (xorshift64).
fn prng(len: usize) -> Vec<u8> {
    let mut x: u64 = 0x0123_4567_89ab_cdef;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 32) as u8
        })
        .collect()
}

/// Every length from 0 to 4096 — so every XXH3 boundary: 16/17, 128/129,
/// 240/241, the 1024-byte stripe blocks and the 64-byte stripes inside them —
/// at an aligned and a misaligned start, with and without a seed: the units
/// must all agree with the scalar one.
#[test]
fn vector_widths_agree() {
    let units = runnable_units();
    let buf = prng(4096 + 64);
    let seed = 0x9E37_79B1_85EB_CA8D;
    let mut bad = 0usize;
    for offset in [0usize, 1] {
        for len in 0..=4096usize {
            let data = &buf[offset..offset + len];
            let reference: &Unit = &units[0];
            let want = (
                (reference.xxh3_64)(data),
                (reference.xxh3_64_seed)(data, seed),
                (reference.xxh3_128)(data),
                (reference.xxh3_128_seed)(data, seed),
            );
            for u in &units[1..] {
                let got = (
                    (u.xxh3_64)(data),
                    (u.xxh3_64_seed)(data, seed),
                    (u.xxh3_128)(data),
                    (u.xxh3_128_seed)(data, seed),
                );
                if got != want {
                    if bad < 10 {
                        eprintln!(
                            "MISMATCH {} vs {} offset={offset} len={len}: {want:x?} vs {got:x?}",
                            u.name, reference.name
                        );
                    }
                    bad += 1;
                }
            }
        }
    }
    eprintln!(
        "{} units agree over lengths 0..=4096 at offsets 0 and 1 ({} mismatches)",
        units.len(),
        bad
    );
    assert_eq!(bad, 0);
}

/// `XXH3_createState` / `reset_withSeed` / `update` / `digest` in pieces of 1,
/// 7 and 1000 bytes equals the one-shot function, 64- and 128-bit, for each
/// unit, across the internal buffer's 256-byte and 1024-byte boundaries.
#[test]
fn streaming_matches_one_shot() {
    let buf = prng(20_000);
    let lens = [
        0usize, 1, 16, 17, 128, 129, 240, 241, 255, 256, 257, 1023, 1024, 1025, 2048, 4096, 4097,
        10_000, 20_000,
    ];
    let mut bad = 0usize;
    for u in runnable_units() {
        for &len in &lens {
            let data = &buf[..len];
            for seed in [0u64, 42] {
                let one64 = (u.xxh3_64_seed)(data, seed);
                let one128 = (u.xxh3_128_seed)(data, seed);
                for piece in [1usize, 7, 1000] {
                    let s64 = (u.stream64)(data, seed, piece);
                    let s128 = (u.stream128)(data, seed, piece);
                    if s64 != one64 || s128 != one128 {
                        if bad < 10 {
                            eprintln!(
                                "MISMATCH {} len={len} seed={seed} piece={piece}: \
                                 64 {one64:#x} vs {s64:#x}, 128 {one128:x?} vs {s128:x?}",
                                u.name
                            );
                        }
                        bad += 1;
                    }
                }
            }
        }
        eprintln!("{}: streaming checked", u.name);
    }
    assert_eq!(bad, 0);
}
