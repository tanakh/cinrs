//! A fixed, seeded xxHash workload, timed per section.
//!
//! The same Rust for every build: the C cinrs translated (the default), or
//! the same C built by `gcc -O2` / `clang -O2` (`--features native-gcc` /
//! `native-clang`), reached through cinrs's declarations from the headers.
//! `scripts/check-xxhash.sh --bench` runs it all three ways and prints one
//! table; run alone it is
//!
//! ```text
//! cargo test --release --test bench -- --ignored --nocapture
//! ```
//!
//! (`#[ignore]`d so that the check's plain `cargo test` does not time it.)
//!
//! XXH3 goes through the dispatcher (`xxh_x86dispatch.c`, `XXH3_*_dispatch`),
//! which picks AVX-512 on a machine that has it. XXH32 and XXH64 have no
//! vector kernel and are not in the dispatcher, so they come from the `sse2_`
//! unit — `xxhash.c` as upstream's default x86-64 build compiles it — as do
//! the streaming state's create, reset and digest; its updates go through
//! `XXH3_64bits_update_dispatch`.
//!
//! Each section is one *pass* of a fixed piece of work; a repetition runs the
//! pass `N` times (`N` fixed per section, so every build does identical work)
//! and the number printed is the median of 5 repetitions divided by `N`: ms per
//! pass. `bench-row:` lines are the numbers, `bench-checksum:` lines show that
//! every build computed the same thing, `bench-info:` lines are for the record.

use std::ffi::c_void;
use std::hint::black_box;
use std::time::Instant;

use cinrs_xxhash_fixture::CONFIGURATION;
use cinrs_xxhash_fixture::dispatch::{
    XXH_featureTest, XXH3_64bits_dispatch, XXH3_64bits_update_dispatch, XXH3_128bits_dispatch,
};
use cinrs_xxhash_fixture::sse2::{
    sse2_XXH3_64bits_digest, sse2_XXH3_64bits_reset, sse2_XXH3_createState,
    sse2_XXH3_freeState, sse2_XXH32, sse2_XXH64,
};

const MIB: usize = 1 << 20;

/// xorshift64*, so that the input is the same everywhere.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn bytes(&mut self, len: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(len + 8);
        while v.len() < len {
            v.extend_from_slice(&self.next().to_le_bytes());
        }
        v.truncate(len);
        v
    }
}

/// Runs `pass` `n` times per repetition, five repetitions, and prints the
/// median in ms per pass. The checksum is the first repetition's last pass.
/// `CINRS_BENCH_ONLY=g` (a list of section letters) skips the other sections,
/// for profiling one of them.
fn time(name: &str, n: usize, mut pass: impl FnMut() -> u64, sums: &mut Vec<(String, u64)>) {
    if let Ok(only) = std::env::var("CINRS_BENCH_ONLY") {
        if !only.contains(&name[..1]) {
            return;
        }
    }
    let mut samples = Vec::new();
    let mut first = None;
    for _ in 0..5 {
        let started = Instant::now();
        let mut c = 0;
        for _ in 0..n {
            c = black_box(pass());
        }
        samples.push(started.elapsed().as_secs_f64() * 1e3 / n as f64);
        first.get_or_insert(c);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = samples[2];
    println!("bench-row: {name}, {median:.3}");
    println!(
        "    ({name}: N={n}, {:.0} ms per repetition, min {:.3} max {:.3})",
        median * n as f64,
        samples[0],
        samples[4]
    );
    sums.push((name.to_owned(), first.unwrap()));
}

fn p(data: &[u8]) -> *const c_void {
    data.as_ptr().cast()
}

/// `n` keys of `len` bytes cut from a 1 MiB region that stays in cache, at
/// offsets that step by an odd amount; XXH3-64 of each, folded.
fn keys(region: &[u8], n: usize, len: usize) -> u64 {
    let mut c = 0u64;
    for i in 0..n {
        let at = (i * (len + 1)) & (MIB - 1);
        // `size_t` is C's `unsigned long` here, a `u64`.
        let h = unsafe { XXH3_64bits_dispatch(p(&region[at..]), len as _) };
        c = c.rotate_left(5) ^ h;
    }
    c
}

#[test]
#[ignore = "a benchmark: scripts/check-xxhash.sh --bench runs it"]
fn bench() {
    println!("bench-info: configuration {CONFIGURATION}");
    println!("bench-info: XXH_featureTest() = {} (3 is AVX-512)", unsafe {
        XXH_featureTest()
    });

    let mut rng = Rng(0x0C0F_FEE5_EED5_2026);
    let big = rng.bytes(64 * MIB);
    let region = rng.bytes(MIB + 512);
    let mut sums = Vec::new();

    time(
        "a XXH3_64bits 64 MiB",
        100,
        || unsafe { XXH3_64bits_dispatch(p(&big), big.len() as _) },
        &mut sums,
    );
    time(
        "b XXH3_128bits 64 MiB",
        100,
        || {
            let h = unsafe { XXH3_128bits_dispatch(p(&big), big.len() as _) };
            h.low64 ^ h.high64.rotate_left(32)
        },
        &mut sums,
    );
    time("c XXH3_64bits 1M x 32 B keys", 100, || keys(&region, 1_000_000, 32), &mut sums);
    time("d XXH3_64bits 1M x 256 B keys", 30, || keys(&region, 1_000_000, 256), &mut sums);
    time(
        "e XXH64 64 MiB",
        80,
        || unsafe { sse2_XXH64(p(&big), big.len() as _, 0) },
        &mut sums,
    );
    time(
        "f XXH32 64 MiB",
        40,
        || u64::from(unsafe { sse2_XXH32(p(&big), big.len() as _, 0) }),
        &mut sums,
    );
    let state = unsafe { sse2_XXH3_createState() };
    assert!(!state.is_null());
    time(
        "g XXH3_64bits streaming 64 MiB in 4 KiB updates",
        100,
        || unsafe {
            let _ = sse2_XXH3_64bits_reset(state);
            for piece in big.chunks(4096) {
                let _ = XXH3_64bits_update_dispatch(state.cast(), p(piece), piece.len() as _);
            }
            sse2_XXH3_64bits_digest(state)
        },
        &mut sums,
    );
    unsafe {
        let _ = sse2_XXH3_freeState(state);
    }
    // The streaming answer is the one-shot answer, in every build.
    assert_eq!(sums[6].1, sums[0].1, "streaming XXH3 differs from one-shot");

    for (name, c) in &sums {
        println!("bench-checksum: {name} = {c:#018x}");
    }
}
