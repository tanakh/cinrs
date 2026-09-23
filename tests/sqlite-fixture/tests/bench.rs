//! A fixed, seeded SQLite workload, timed per section.
//!
//! The same Rust for every build: the amalgamation cinrs translated (the
//! default, `SQLITE_THREADSAFE=1`), or the same amalgamation built by
//! `gcc -O2` / `clang -O2` (`--features native-gcc` / `native-clang`), reached
//! through cinrs's declarations from `sqlite3.h`. `scripts/check-sqlite.sh
//! --bench` runs it all three ways and prints one table; run alone it is
//!
//! ```text
//! cargo +beta test --release --test bench -- --ignored --nocapture
//! ```
//!
//! (`#[ignore]`d so that the check's plain `cargo test` does not time it.)
//!
//! One repetition opens an in-memory database (`PRAGMA journal_mode=MEMORY`),
//! and runs the five sections in order on it, each timed on its own, all
//! through `sqlite3_prepare_v2`/`bind`/`step`/`reset`: (a) 200,000 inserts in
//! one transaction, (b) 200,000 point lookups by primary key, (c) 200 range
//! scans of 10,000 rows each with `ORDER BY` on an unindexed column, (d) an
//! aggregate `GROUP BY` over the table, run ten times, and (e) 200,000 updates
//! by primary key in one transaction. The number printed is the median of 5
//! repetitions in ms per section — for (d), per `GROUP BY` query.
//! `bench-row:` lines are the numbers, `bench-checksum:` lines show that
//! every build computed the same thing, `bench-info:` lines are for the record.

use std::ffi::{CStr, CString, c_char, c_int};
use std::time::Instant;

use cinrs_sqlite_fixture::{CONFIGURATION, sqlite::*};

const OK: c_int = 0;
const ROW: c_int = 100;
const DONE: c_int = 101;

const ROWS: i64 = 200_000;
const LOOKUPS: usize = 200_000;
const SCANS: usize = 200;
const SCAN_ROWS: i64 = 10_000;
const GROUP_BYS: usize = 10;
const UPDATES: usize = 200_000;

/// xorshift64*, so that the data and the keys are the same everywhere.
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

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Everything the workload binds, generated once.
struct Work {
    /// Per row: `a`, `g` and the 16-letter `s`.
    rows: Vec<(i64, i64, [u8; 16])>,
    lookups: Vec<i64>,
    scans: Vec<i64>,
    updates: Vec<(i64, i64)>,
}

fn work() -> Work {
    let mut rng = Rng(0x5_71_1E_5EED_2026);
    let rows = (0..ROWS)
        .map(|_| {
            let a = rng.below(1_000_000) as i64;
            let g = rng.below(100) as i64;
            let mut s = [0u8; 16];
            for b in &mut s {
                *b = b'a' + rng.below(26) as u8;
            }
            (a, g, s)
        })
        .collect();
    let lookups = (0..LOOKUPS).map(|_| 1 + rng.below(ROWS as u64) as i64).collect();
    let scans = (0..SCANS)
        .map(|_| 1 + rng.below((ROWS - SCAN_ROWS + 1) as u64) as i64)
        .collect();
    let updates = (0..UPDATES)
        .map(|_| (1 + rng.below(ROWS as u64) as i64, rng.below(1000) as i64))
        .collect();
    Work { rows, lookups, scans, updates }
}

fn cs(text: &str) -> CString {
    CString::new(text).expect("no interior NUL")
}

fn check(db: *mut sqlite3, rc: c_int, want: c_int, what: &str) {
    if rc != want {
        let msg = unsafe { CStr::from_ptr(sqlite3_errmsg(db)) }.to_string_lossy().into_owned();
        panic!("{what}: rc {rc}, wanted {want}: {msg}");
    }
}

fn exec(db: *mut sqlite3, sql: &str) {
    let text = cs(sql);
    let rc = unsafe {
        sqlite3_exec(db, text.as_ptr(), None, std::ptr::null_mut(), std::ptr::null_mut())
    };
    check(db, rc, OK, sql);
}

fn prepare(db: *mut sqlite3, sql: &str) -> *mut sqlite3_stmt {
    let text = cs(sql);
    let mut stmt: *mut sqlite3_stmt = std::ptr::null_mut();
    let rc = unsafe {
        sqlite3_prepare_v2(db, text.as_ptr(), -1, &raw mut stmt, std::ptr::null_mut())
    };
    check(db, rc, OK, sql);
    stmt
}

/// A one-row query's integer columns.
fn one_row(db: *mut sqlite3, sql: &str, columns: c_int) -> Vec<i64> {
    let stmt = prepare(db, sql);
    unsafe {
        check(db, sqlite3_step(stmt), ROW, sql);
        let v = (0..columns).map(|i| sqlite3_column_int64(stmt, i)).collect();
        check(db, sqlite3_step(stmt), DONE, sql);
        sqlite3_finalize(stmt);
        v
    }
}

fn fold(c: u64, v: i64) -> u64 {
    c.rotate_left(7) ^ (v as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

const SECTIONS: [&str; 5] = [
    "a 200k inserts in one transaction",
    "b 200k point lookups by primary key",
    "c 200 range scans x 10k rows ORDER BY",
    "d GROUP BY over 200k rows (per query)",
    "e 200k updates by primary key",
];

/// One repetition: a fresh database, the five sections. Returns the ms per
/// section and a checksum per section.
fn repetition(w: &Work) -> ([f64; 5], [u64; 5]) {
    let mut ms = [0.0; 5];
    let mut sums = [0u64; 5];
    unsafe {
        let mut db: *mut sqlite3 = std::ptr::null_mut();
        let name = cs(":memory:");
        assert_eq!(sqlite3_open(name.as_ptr(), &raw mut db), OK);
        exec(db, "PRAGMA journal_mode=MEMORY");
        exec(
            db,
            "CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER NOT NULL, \
             g INTEGER NOT NULL, s TEXT NOT NULL)",
        );

        // (a) inserts
        let started = Instant::now();
        exec(db, "BEGIN");
        let stmt = prepare(db, "INSERT INTO t (id, a, g, s) VALUES (?1, ?2, ?3, ?4)");
        for (i, (a, g, s)) in w.rows.iter().enumerate() {
            sqlite3_bind_int64(stmt, 1, i as i64 + 1);
            sqlite3_bind_int64(stmt, 2, *a);
            sqlite3_bind_int64(stmt, 3, *g);
            // SQLITE_STATIC: the bytes outlive the statement's use of them.
            sqlite3_bind_text(stmt, 4, s.as_ptr() as *const c_char, 16, None);
            check(db, sqlite3_step(stmt), DONE, "insert");
            sqlite3_reset(stmt);
        }
        sqlite3_finalize(stmt);
        exec(db, "COMMIT");
        ms[0] = started.elapsed().as_secs_f64() * 1e3;
        let v = one_row(db, "SELECT count(*), sum(a), sum(g) FROM t", 3);
        sums[0] = v.iter().fold(0, |c, &x| fold(c, x));

        // (b) point lookups
        let started = Instant::now();
        let stmt = prepare(db, "SELECT a, length(s) FROM t WHERE id = ?1");
        let mut c = 0u64;
        for &id in &w.lookups {
            sqlite3_bind_int64(stmt, 1, id);
            check(db, sqlite3_step(stmt), ROW, "lookup");
            c = fold(c, sqlite3_column_int64(stmt, 0) + sqlite3_column_int64(stmt, 1));
            sqlite3_reset(stmt);
        }
        sqlite3_finalize(stmt);
        ms[1] = started.elapsed().as_secs_f64() * 1e3;
        sums[1] = c;

        // (c) range scans with a sort
        let started = Instant::now();
        let stmt = prepare(db, "SELECT a, id FROM t WHERE id BETWEEN ?1 AND ?2 ORDER BY a");
        let mut c = 0u64;
        for &lo in &w.scans {
            sqlite3_bind_int64(stmt, 1, lo);
            sqlite3_bind_int64(stmt, 2, lo + SCAN_ROWS - 1);
            let mut n = 0i64;
            loop {
                match sqlite3_step(stmt) {
                    ROW => {
                        c = fold(c, sqlite3_column_int64(stmt, 0));
                        n += 1;
                    }
                    rc => {
                        check(db, rc, DONE, "scan");
                        break;
                    }
                }
            }
            assert_eq!(n, SCAN_ROWS);
            sqlite3_reset(stmt);
        }
        sqlite3_finalize(stmt);
        ms[2] = started.elapsed().as_secs_f64() * 1e3;
        sums[2] = c;

        // (d) GROUP BY, ten times; ms per query
        let started = Instant::now();
        let stmt = prepare(
            db,
            "SELECT g, count(*), sum(a), max(a), sum(length(s)) FROM t GROUP BY g ORDER BY g",
        );
        let mut c = 0u64;
        for _ in 0..GROUP_BYS {
            let mut groups = 0;
            loop {
                match sqlite3_step(stmt) {
                    ROW => {
                        for i in 0..5 {
                            c = fold(c, sqlite3_column_int64(stmt, i));
                        }
                        groups += 1;
                    }
                    rc => {
                        check(db, rc, DONE, "group by");
                        break;
                    }
                }
            }
            assert_eq!(groups, 100);
            sqlite3_reset(stmt);
        }
        sqlite3_finalize(stmt);
        ms[3] = started.elapsed().as_secs_f64() * 1e3 / GROUP_BYS as f64;
        sums[3] = c;

        // (e) updates
        let started = Instant::now();
        exec(db, "BEGIN");
        let stmt = prepare(db, "UPDATE t SET a = a + ?2 WHERE id = ?1");
        for &(id, delta) in &w.updates {
            sqlite3_bind_int64(stmt, 1, id);
            sqlite3_bind_int64(stmt, 2, delta);
            check(db, sqlite3_step(stmt), DONE, "update");
            sqlite3_reset(stmt);
        }
        sqlite3_finalize(stmt);
        exec(db, "COMMIT");
        ms[4] = started.elapsed().as_secs_f64() * 1e3;
        let v = one_row(db, "SELECT count(*), sum(a), total_changes() FROM t", 3);
        sums[4] = v.iter().fold(0, |c, &x| fold(c, x));

        assert_eq!(sqlite3_close(db), OK);
    }
    (ms, sums)
}

#[test]
#[ignore = "a benchmark: scripts/check-sqlite.sh --bench runs it"]
fn bench() {
    let version = unsafe { CStr::from_ptr(sqlite3_libversion()) }.to_string_lossy().into_owned();
    println!("bench-info: configuration {CONFIGURATION}, SQLite {version}");
    let w = work();
    let mut samples: [Vec<f64>; 5] = Default::default();
    let mut first = None;
    for rep in 0..5 {
        let (ms, sums) = repetition(&w);
        println!("    (repetition {rep}: {ms:.1?} ms)");
        for (s, m) in samples.iter_mut().zip(ms) {
            s.push(m);
        }
        match first {
            None => first = Some(sums),
            Some(f) => assert_eq!(f, sums, "a repetition computed something else"),
        }
    }
    for (name, s) in SECTIONS.iter().zip(&mut samples) {
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("bench-row: {name}, {:.3}", s[2]);
    }
    for (name, c) in SECTIONS.iter().zip(first.unwrap()) {
        println!("bench-checksum: {name} = {c:#018x}");
    }
}
