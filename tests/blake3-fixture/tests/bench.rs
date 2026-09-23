//! A fixed, seeded BLAKE3 workload, timed per section.
//!
//! The same Rust for every build: the C cinrs translated (the default), or
//! the same C built by `gcc -O2` / `clang -O2` (`--features native-gcc` /
//! `native-clang`), reached through cinrs's declarations from `blake3.h`.
//! `scripts/check-blake3.sh --bench` runs it all three ways and prints one
//! table; run alone it is
//!
//! ```text
//! cargo test --release --test bench -- --ignored --nocapture
//! ```
//!
//! (`#[ignore]`d so that the check's plain `cargo test` does not time it.)
//!
//! Each section is one *pass* of a fixed piece of work; a repetition runs the
//! pass `N` times (`N` fixed per section, so every build does identical work)
//! and the number printed is the median of 5 repetitions divided by `N`: ms per
//! pass. `bench-row:` lines are the numbers, `bench-checksum:` lines show that
//! every build computed the same thing, `bench-info:` lines are for the record.

use std::hint::black_box;
use std::mem::MaybeUninit;
use std::time::Instant;

use cinrs_blake3_fixture::CONFIGURATION;
use cinrs_blake3_fixture::blake3::{
    blake3_hasher, blake3_hasher_finalize, blake3_hasher_finalize_seek, blake3_hasher_init,
    blake3_hasher_init_keyed, blake3_hasher_update,
};
use cinrs_blake3_fixture::dispatch::blake3_simd_degree;

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
fn time(name: &str, n: usize, mut pass: impl FnMut() -> u64, sums: &mut Vec<(String, u64)>) {
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

/// The first eight bytes of a digest, as a number.
fn word(out: &[u8]) -> u64 {
    u64::from_le_bytes(out[..8].try_into().unwrap())
}

struct Hasher(Box<MaybeUninit<blake3_hasher>>);

impl Hasher {
    fn new() -> Self {
        let mut h = Hasher(Box::new(MaybeUninit::uninit()));
        unsafe { blake3_hasher_init(h.ptr()) };
        h
    }

    fn keyed(key: &[u8; 32]) -> Self {
        let mut h = Hasher(Box::new(MaybeUninit::uninit()));
        unsafe { blake3_hasher_init_keyed(h.ptr(), key.as_ptr()) };
        h
    }

    fn ptr(&mut self) -> *mut blake3_hasher {
        self.0.as_mut_ptr()
    }

    fn reset(&mut self) {
        unsafe { blake3_hasher_init(self.ptr()) };
    }

    fn update(&mut self, data: &[u8]) {
        // `size_t` is C's `unsigned long` here, a `u64`.
        unsafe { blake3_hasher_update(self.ptr(), data.as_ptr().cast(), data.len() as _) };
    }

    fn finalize(&mut self, out: &mut [u8]) {
        unsafe { blake3_hasher_finalize(self.ptr(), out.as_mut_ptr(), out.len() as _) };
    }

    fn finalize_seek(&mut self, seek: u64, out: &mut [u8]) {
        unsafe { blake3_hasher_finalize_seek(self.ptr(), seek, out.as_mut_ptr(), out.len() as _) };
    }
}

#[test]
#[ignore = "a benchmark: scripts/check-blake3.sh --bench runs it"]
fn bench() {
    println!("bench-info: configuration {CONFIGURATION}");
    println!("bench-info: blake3_simd_degree() = {}", unsafe { blake3_simd_degree() });

    let mut rng = Rng(0x0B1A_4E35_EED5_2026);
    let big = rng.bytes(64 * MIB);
    // Small messages are cut from a 1 MiB region that stays in cache, at
    // offsets that step by an odd amount so that no two neighbours coincide.
    let small = rng.bytes(MIB + 1024);
    let key: [u8; 32] = rng.bytes(32).try_into().unwrap();
    let mut sums = Vec::new();
    let mut out = [0u8; 32];

    // (a) one hasher over 64 MiB, fed in 1 MiB updates, then finalize.
    let mut h = Hasher::new();
    time(
        "a 64 MiB in 1 MiB updates",
        20,
        || {
            h.reset();
            for piece in big.chunks(MIB) {
                h.update(piece);
            }
            h.finalize(&mut out);
            word(&out)
        },
        &mut sums,
    );

    // (b) a million 64-byte messages, each init + update + finalize.
    time(
        "b 1M x 64 B messages",
        3,
        || {
            let mut c = 0u64;
            for i in 0..1_000_000usize {
                let at = (i * 67) & (MIB - 1);
                h.reset();
                h.update(&small[at..at + 64]);
                h.finalize(&mut out);
                c = c.rotate_left(5) ^ word(&out);
            }
            c
        },
        &mut sums,
    );

    // (c) a hundred thousand 1 KiB messages (one chunk each).
    time(
        "c 100k x 1 KiB messages",
        3,
        || {
            let mut c = 0u64;
            for i in 0..100_000usize {
                let at = (i * 1031) & (MIB - 1);
                h.reset();
                h.update(&small[at..at + 1024]);
                h.finalize(&mut out);
                c = c.rotate_left(5) ^ word(&out);
            }
            c
        },
        &mut sums,
    );

    // (d) keyed_hash over 16 MiB, one update.
    time(
        "d keyed_hash 16 MiB",
        100,
        || {
            let mut k = Hasher::keyed(&key);
            k.update(&big[..16 * MIB]);
            k.finalize(&mut out);
            word(&out)
        },
        &mut sums,
    );

    // (e) 16 MiB of extended output from finalize_seek, one call.
    let mut xof = vec![0u8; 16 * MIB];
    time(
        "e finalize_seek 16 MiB XOF",
        100,
        || {
            h.reset();
            h.update(&small[..1024]);
            h.finalize_seek(0, &mut xof);
            let mut c = 0u64;
            for w in xof.chunks_exact(4096) {
                c = c.rotate_left(5) ^ word(w);
            }
            c
        },
        &mut sums,
    );

    for (name, c) in &sums {
        println!("bench-checksum: {name} = {c:#018x}");
    }
}
