//! BLAKE3's official test vectors, through the C implementation `cinrs`
//! compiled.
//!
//! `test_vectors/test_vectors.json` comes from the same release archive as the
//! C (`scripts/check-blake3.sh` unpacks it into `target/blake3/`). It is simple
//! enough JSON — one object, `"key"`, `"context_string"` and an array of
//! `{input_len, hash, keyed_hash, derive_key}` — that a few string searches
//! read it, so the fixture has no dependency but `cinrs`.
//!
//! Each output in the file is an *extended* output (131 bytes), so each case is
//! checked twice: the default 32-byte digest, and the whole extended output,
//! which goes through the XOF path (`blake3_xof_many`) as well.

use std::ffi::{CString, c_char};
use std::mem::MaybeUninit;

use cinrs_blake3_fixture::blake3::{
    blake3_hasher, blake3_hasher_finalize, blake3_hasher_init, blake3_hasher_init_derive_key,
    blake3_hasher_init_keyed, blake3_hasher_update,
};
use cinrs_blake3_fixture::dispatch::blake3_simd_degree;

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/blake3/test_vectors/test_vectors.json"
);

struct Case {
    input_len: usize,
    hash: Vec<u8>,
    keyed_hash: Vec<u8>,
    derive_key: Vec<u8>,
}

/// The string value of the first `"name": "…"` at or after `from`, and where
/// it ends.
fn string_field(text: &str, name: &str, from: usize) -> (String, usize) {
    let tag = format!("\"{name}\"");
    let at = from + text[from..].find(&tag).unwrap_or_else(|| panic!("no {tag}"));
    let open = at + tag.len() + text[at + tag.len()..].find('"').expect("a value");
    let close = open + 1 + text[open + 1..].find('"').expect("a closing quote");
    (text[open + 1..close].to_owned(), close + 1)
}

fn hex(text: &str) -> Vec<u8> {
    assert!(text.len() % 2 == 0, "odd hex length");
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn load() -> (Vec<u8>, String, Vec<Case>) {
    let text = std::fs::read_to_string(VECTORS)
        .unwrap_or_else(|e| panic!("{VECTORS}: {e} (run scripts/check-blake3.sh)"));
    let (key, _) = string_field(&text, "key", 0);
    let (context, _) = string_field(&text, "context_string", 0);
    let mut cases = Vec::new();
    let mut pos = text.find("\"cases\"").expect("\"cases\"");
    while let Some(off) = text[pos..].find("\"input_len\"") {
        let at = pos + off + "\"input_len\"".len();
        let digits: String = text[at..]
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        let input_len = digits.parse().expect("input_len");
        let (hash, next) = string_field(&text, "hash", at);
        let (keyed_hash, next) = string_field(&text, "keyed_hash", next);
        let (derive_key, next) = string_field(&text, "derive_key", next);
        cases.push(Case {
            input_len,
            hash: hex(&hash),
            keyed_hash: hex(&keyed_hash),
            derive_key: hex(&derive_key),
        });
        pos = next;
    }
    (key.into_bytes(), context, cases)
}

/// The input every case hashes: 0, 1, …, 250, 0, 1, … .
fn input(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

fn new_hasher() -> Box<MaybeUninit<blake3_hasher>> {
    Box::new(MaybeUninit::uninit())
}

/// One mode's output for `data`, `out_len` bytes long, fed in `piece`-sized
/// updates.
fn run(init: &dyn Fn(*mut blake3_hasher), data: &[u8], piece: usize, out_len: usize) -> Vec<u8> {
    let mut h = new_hasher();
    let p = h.as_mut_ptr();
    init(p);
    if data.is_empty() {
        unsafe { blake3_hasher_update(p, data.as_ptr().cast(), 0) };
    }
    for chunk in data.chunks(piece.max(1)) {
        unsafe { blake3_hasher_update(p, chunk.as_ptr().cast(), chunk.len() as _) };
    }
    let mut out = vec![0u8; out_len];
    // `size_t` is C's `unsigned long` here, a `u64`, not `usize`.
    unsafe { blake3_hasher_finalize(p, out.as_mut_ptr(), out_len as _) };
    out
}

fn check(label: &str, len: usize, got: &[u8], want: &[u8]) -> bool {
    if got == want {
        return true;
    }
    eprintln!(
        "MISMATCH {label} input_len={len}\n    want {}\n    got  {}",
        to_hex(want),
        to_hex(got)
    );
    false
}

#[test]
fn which_implementation() {
    let degree = unsafe { blake3_simd_degree() };
    let name = match degree {
        16 => "AVX-512 (avx512f + avx512vl)",
        8 => "AVX2",
        4 => "SSE4.1 or SSE2",
        _ => "portable",
    };
    eprintln!("blake3_simd_degree() = {degree}: the dispatcher chose {name}");
    eprintln!(
        "host: avx512f={} avx512vl={} avx2={} sse4.1={}",
        std::is_x86_feature_detected!("avx512f"),
        std::is_x86_feature_detected!("avx512vl"),
        std::is_x86_feature_detected!("avx2"),
        std::is_x86_feature_detected!("sse4.1"),
    );
}

#[test]
fn official_test_vectors() {
    let (key, context, cases) = load();
    assert_eq!(key.len(), 32);
    assert!(cases.len() >= 35, "only {} cases parsed", cases.len());
    let context = CString::new(context).unwrap();
    let hash_init = |p: *mut blake3_hasher| unsafe { blake3_hasher_init(p) };
    let keyed_init = |p: *mut blake3_hasher| unsafe { blake3_hasher_init_keyed(p, key.as_ptr()) };
    let derive_init = |p: *mut blake3_hasher| unsafe {
        blake3_hasher_init_derive_key(p, context.as_ptr() as *const c_char)
    };
    let mut ok = true;
    let mut passed = 0;
    for case in &cases {
        let data = input(case.input_len);
        let modes: [(&str, &dyn Fn(*mut blake3_hasher), &[u8]); 3] = [
            ("hash", &hash_init, &case.hash),
            ("keyed_hash", &keyed_init, &case.keyed_hash),
            ("derive_key", &derive_init, &case.derive_key),
        ];
        for (label, init, want) in modes {
            let short = run(init, &data, usize::MAX, 32);
            let long = run(init, &data, usize::MAX, want.len());
            let a = check(&format!("{label} (32 bytes)"), case.input_len, &short, &want[..32]);
            let b = check(&format!("{label} (extended)"), case.input_len, &long, want);
            ok &= a && b;
            passed += usize::from(a) + usize::from(b);
        }
    }
    eprintln!(
        "{passed} of {} checks passed over {} cases",
        cases.len() * 6,
        cases.len()
    );
    assert!(ok, "test vector mismatches; see above");
}

#[test]
fn one_byte_updates_match_one_call() {
    let (_, _, cases) = load();
    let hash_init = |p: *mut blake3_hasher| unsafe { blake3_hasher_init(p) };
    for len in [0usize, 1, 63, 64, 65, 1023, 1024, 1025, 2048, 4096, 8193, 31744, 102400] {
        let data = input(len);
        let whole = run(&hash_init, &data, usize::MAX, 32);
        let bytes = run(&hash_init, &data, 1, 32);
        assert_eq!(to_hex(&bytes), to_hex(&whole), "input_len={len}");
        if let Some(case) = cases.iter().find(|c| c.input_len == len) {
            assert_eq!(to_hex(&whole), to_hex(&case.hash[..32]), "input_len={len}");
        }
    }
}
