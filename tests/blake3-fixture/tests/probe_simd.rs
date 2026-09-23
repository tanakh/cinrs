//! Each SIMD implementation, called directly, against the portable one.
//!
//! The dispatcher only ever runs the widest implementation this machine has,
//! so the test vectors exercise that one alone. This test reaches the SSE2,
//! SSE4.1, AVX2 and AVX-512 functions by their exported names instead, on the
//! instruction sets this machine has, and checks that they agree with the
//! portable function on the same inputs — which is what upstream's own
//! `test.c` does.

use cinrs_blake3_fixture::portable::{
    blake3_compress_in_place_portable, blake3_compress_xof_portable, blake3_hash_many_portable,
};
use cinrs_blake3_fixture::{avx2, avx512, sse2, sse41};

type HashMany = unsafe extern "C" fn(*const *const u8, u64, u64, *const u32, u64, bool, u8, u8, u8, *mut u8);
type CompressInPlace = unsafe extern "C" fn(*mut u32, *const u8, u8, u64, u8);
type CompressXof = unsafe extern "C" fn(*const u32, *const u8, u8, u64, u8, *mut u8);

const KEY: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];

fn bytes(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2654435761).wrapping_add(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

fn hash_many(f: HashMany, inputs: &[Vec<u8>], blocks: usize, counter: u64, inc: bool) -> Vec<u8> {
    let ptrs: Vec<*const u8> = inputs.iter().map(|v| v.as_ptr()).collect();
    let mut out = vec![0u8; inputs.len() * 32];
    unsafe {
        f(
            ptrs.as_ptr(),
            inputs.len() as u64,
            blocks as u64,
            KEY.as_ptr(),
            counter,
            inc,
            0,
            1, // CHUNK_START
            2, // CHUNK_END
            out.as_mut_ptr(),
        )
    };
    out
}

fn check_hash_many(name: &str, f: HashMany) {
    let portable: HashMany = blake3_hash_many_portable;
    for blocks in [1usize, 2, 16] {
        for n in 1..=17usize {
            let inputs: Vec<Vec<u8>> = (0..n).map(|i| bytes(blocks * 64, (i * 31 + blocks) as u32)).collect();
            for (counter, inc) in [(0u64, true), (u64::from(u32::MAX) - 2, true), (7, false)] {
                let want = hash_many(portable, &inputs, blocks, counter, inc);
                let got = hash_many(f, &inputs, blocks, counter, inc);
                assert_eq!(got, want, "{name}: blocks={blocks} n={n} counter={counter} inc={inc}");
            }
        }
    }
    eprintln!("{name}: hash_many agrees with portable");
}

fn check_compress(name: &str, in_place: CompressInPlace, xof: CompressXof) {
    for (i, block_len) in [0u8, 1, 33, 64].into_iter().enumerate() {
        let block = bytes(64, 1000 + i as u32);
        for counter in [0u64, 1, u64::from(u32::MAX), u64::MAX - 1] {
            for flags in [0u8, 1, 2, 3, 8, 11] {
                let mut want = KEY;
                let mut got = KEY;
                unsafe {
                    blake3_compress_in_place_portable(want.as_mut_ptr(), block.as_ptr(), block_len, counter, flags);
                    in_place(got.as_mut_ptr(), block.as_ptr(), block_len, counter, flags);
                }
                assert_eq!(got, want, "{name} in_place: len={block_len} counter={counter} flags={flags}");
                let mut want = [0u8; 64];
                let mut got = [0u8; 64];
                unsafe {
                    blake3_compress_xof_portable(KEY.as_ptr(), block.as_ptr(), block_len, counter, flags, want.as_mut_ptr());
                    xof(KEY.as_ptr(), block.as_ptr(), block_len, counter, flags, got.as_mut_ptr());
                }
                assert_eq!(got, want, "{name} xof: len={block_len} counter={counter} flags={flags}");
            }
        }
    }
    eprintln!("{name}: compress_in_place and compress_xof agree with portable");
}

#[test]
fn sse2_agrees_with_portable() {
    check_compress("sse2", sse2::blake3_compress_in_place_sse2, sse2::blake3_compress_xof_sse2);
    check_hash_many("sse2", sse2::blake3_hash_many_sse2);
}

#[test]
fn sse41_agrees_with_portable() {
    if !std::is_x86_feature_detected!("sse4.1") {
        eprintln!("sse4.1: not on this machine, skipped");
        return;
    }
    check_compress("sse41", sse41::blake3_compress_in_place_sse41, sse41::blake3_compress_xof_sse41);
    check_hash_many("sse41", sse41::blake3_hash_many_sse41);
}

#[test]
fn avx2_agrees_with_portable() {
    if !std::is_x86_feature_detected!("avx2") {
        eprintln!("avx2: not on this machine, skipped");
        return;
    }
    check_hash_many("avx2", avx2::blake3_hash_many_avx2);
}

#[test]
fn avx512_agrees_with_portable() {
    if !(std::is_x86_feature_detected!("avx512f") && std::is_x86_feature_detected!("avx512vl")) {
        eprintln!("avx512: not on this machine, skipped");
        return;
    }
    check_compress("avx512", avx512::blake3_compress_in_place_avx512, avx512::blake3_compress_xof_avx512);
    check_hash_many("avx512", avx512::blake3_hash_many_avx512);
    // blake3_xof_many_avx512: `outblocks` consecutive XOF blocks from one
    // compression input, the counter stepping by one.
    let block = bytes(64, 77);
    for outblocks in [1usize, 2, 3, 4, 5, 8, 9, 16, 17, 33] {
        let mut want = vec![0u8; 64 * outblocks];
        for i in 0..outblocks {
            unsafe {
                blake3_compress_xof_portable(KEY.as_ptr(), block.as_ptr(), 64, 5 + i as u64, 11, want[64 * i..].as_mut_ptr())
            };
        }
        let mut got = vec![0u8; 64 * outblocks];
        unsafe {
            avx512::blake3_xof_many_avx512(KEY.as_ptr(), block.as_ptr(), 64, 5, 11, got.as_mut_ptr(), outblocks as u64)
        };
        assert_eq!(got, want, "avx512 xof_many: outblocks={outblocks}");
    }
    eprintln!("avx512: xof_many agrees with portable");
}
