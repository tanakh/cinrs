//! Does the SQLite `cinrs` compiled actually work?
//!
//! Not a unit test of anything: one session that opens an in-memory database,
//! creates a table, inserts rows, reads them back through the prepared-statement
//! API, calls a Rust `extern "C"` function from SQL, and closes. Everything goes
//! through the C API as any C program would, which is the point — the functions
//! are the ones the amalgamation defines, reached from Rust under their own
//! names.
//!
//! With `--features system-sqlite` the same query is also run against the
//! platform's own `libsqlite3`, so that there is a number to compare.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::time::Instant;

use cinrs_sqlite_fixture::{CONFIGURATION, sqlite::*};

/// `SQLITE_OK`, `SQLITE_ROW` and `SQLITE_DONE`, spelled here so that a wrong
/// value in the translation shows up as a failure rather than as agreement with
/// itself.
const OK: c_int = 0;
const ROW: c_int = 100;
const DONE: c_int = 101;
/// `SQLITE_UTF8`.
const UTF8: c_int = 1;

/// A C string that lives as long as the test.
fn cs(text: &str) -> CString {
    CString::new(text).expect("no interior NUL")
}

/// `sqlite3_exec` with no callback, panicking with SQLite's own message.
unsafe fn exec(db: *mut sqlite3, sql: &str) {
    let sql = cs(sql);
    let mut message: *mut c_char = std::ptr::null_mut();
    let rc = unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr(),
            None,
            std::ptr::null_mut(),
            &raw mut message,
        )
    };
    if rc != OK {
        let text = if message.is_null() {
            "(no message)".to_owned()
        } else {
            unsafe { CStr::from_ptr(message) }
                .to_string_lossy()
                .into_owned()
        };
        panic!("sqlite3_exec({sql:?}) failed with {rc}: {text}");
    }
}

/// The Rust function SQL calls: `rust_double(x)` returns `2 * x`.
///
/// An ordinary `extern "C"` function, registered with `sqlite3_create_function`
/// exactly as a C one would be. This is the direction that matters most —
/// translated C calling back into Rust through a function pointer it was handed
/// — because it goes through the `Option<unsafe extern "C" fn(…)>` a C function
/// pointer becomes.
unsafe extern "C" fn rust_double(
    ctx: *mut sqlite3_context,
    argc: c_int,
    argv: *mut *mut sqlite3_value,
) {
    unsafe {
        if argc != 1 {
            let message = cs("rust_double() takes one argument");
            sqlite3_result_error(ctx, message.as_ptr(), -1);
            return;
        }
        let value = sqlite3_value_int64(*argv);
        sqlite3_result_int64(ctx, value * 2);
    }
}

#[test]
fn a_whole_session_through_the_c_api() {
    unsafe {
        // -- the version is the one the header said ------------------------
        let version = CStr::from_ptr(sqlite3_libversion())
            .to_str()
            .expect("ASCII");
        assert_eq!(
            version, "3.53.4",
            "sqlite3_libversion() must be SQLITE_VERSION"
        );
        assert_eq!(sqlite3_libversion_number(), 3_053_004);
        // `sqlite3_sourceid()` is a date and the 64 hex digits of the check-in
        // hash: "2026-07-24 19:02:57 bf7c7f30…".
        let source = CStr::from_ptr(sqlite3_sourceid()).to_str().expect("ASCII");
        let hash = source.rsplit(' ').next().unwrap_or_default();
        assert_eq!(hash.len(), 64, "the source id ends in a hash: {source:?}");
        assert!(
            hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "the source id ends in a hash: {source:?}"
        );

        // -- the threading mode is the one this build asked for -------------
        // `sqlite3_threadsafe()` is non-zero only when SQLITE_THREADSAFE is.
        let threadsafe = sqlite3_threadsafe() != 0;
        assert_eq!(
            threadsafe,
            CONFIGURATION == "SQLITE_THREADSAFE=1",
            "sqlite3_threadsafe() must agree with the build: {CONFIGURATION}"
        );

        // -- open ----------------------------------------------------------
        let mut db: *mut sqlite3 = std::ptr::null_mut();
        let name = cs(":memory:");
        let rc = sqlite3_open(name.as_ptr(), &raw mut db);
        assert_eq!(rc, OK, "sqlite3_open(\":memory:\")");
        assert!(!db.is_null());

        // -- create and insert ---------------------------------------------
        exec(db, "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, n INTEGER)");
        exec(db, "INSERT INTO t (name, n) VALUES ('one', 1)");
        exec(db, "INSERT INTO t (name, n) VALUES ('two', 2)");
        exec(db, "INSERT INTO t (name, n) VALUES ('three', 3)");
        assert_eq!(sqlite3_changes(db), 1, "the last statement inserted one row");
        assert_eq!(sqlite3_last_insert_rowid(db), 3);

        // -- select, through prepare/step/column ---------------------------
        let sql = cs("SELECT id, name, n FROM t ORDER BY id");
        let mut stmt: *mut sqlite3_stmt = std::ptr::null_mut();
        let rc = sqlite3_prepare_v2(db, sql.as_ptr(), -1, &raw mut stmt, std::ptr::null_mut());
        assert_eq!(rc, OK, "sqlite3_prepare_v2");
        assert_eq!(sqlite3_column_count(stmt), 3);

        let mut rows = Vec::new();
        loop {
            let rc = sqlite3_step(stmt);
            if rc == DONE {
                break;
            }
            assert_eq!(rc, ROW, "sqlite3_step");
            let id = sqlite3_column_int(stmt, 0);
            let text = sqlite3_column_text(stmt, 1);
            assert!(!text.is_null());
            let name = CStr::from_ptr(text.cast::<c_char>())
                .to_str()
                .expect("UTF-8")
                .to_owned();
            let n = sqlite3_column_int(stmt, 2);
            rows.push((id, name, n));
        }
        assert_eq!(sqlite3_finalize(stmt), OK);
        assert_eq!(
            rows,
            vec![
                (1, "one".to_owned(), 1),
                (2, "two".to_owned(), 2),
                (3, "three".to_owned(), 3),
            ]
        );

        // -- a Rust function called from SQL -------------------------------
        let fname = cs("rust_double");
        let rc = sqlite3_create_function(
            db,
            fname.as_ptr(),
            1,
            UTF8,
            std::ptr::null_mut(),
            Some(rust_double),
            None,
            None,
        );
        assert_eq!(rc, OK, "sqlite3_create_function");

        let sql = cs("SELECT sum(rust_double(n)) FROM t");
        let mut stmt: *mut sqlite3_stmt = std::ptr::null_mut();
        assert_eq!(
            sqlite3_prepare_v2(db, sql.as_ptr(), -1, &raw mut stmt, std::ptr::null_mut()),
            OK
        );
        assert_eq!(sqlite3_step(stmt), ROW);
        assert_eq!(
            sqlite3_column_int(stmt, 0),
            12,
            "2*(1+2+3), computed by a Rust callback SQLite called"
        );
        assert_eq!(sqlite3_finalize(stmt), OK);

        // -- and one that takes and returns text, through sqlite3_mprintf --
        // `sqlite3_mprintf` is a *variadic definition*, which is the one thing
        // in the amalgamation that needs Rust 1.99.
        let fmt = cs("%d rows, %s");
        let word = cs("ok");
        let message = sqlite3_mprintf(fmt.as_ptr(), 3, word.as_ptr());
        assert!(!message.is_null());
        assert_eq!(
            CStr::from_ptr(message).to_str().expect("UTF-8"),
            "3 rows, ok",
            "sqlite3_mprintf is a variadic definition and has to work"
        );
        sqlite3_free(message.cast::<c_void>());

        // -- close ---------------------------------------------------------
        assert_eq!(sqlite3_close(db), OK, "sqlite3_close");
    }
}

/// How long a small prepared query takes, as a first number.
///
/// Not a benchmark — one loop, one process — but the shape of the answer: the
/// same statement prepared, stepped and reset a thousand times over a table of a
/// thousand rows. With `--features system-sqlite` the platform's own library
/// runs the identical loop through the identical API.
#[test]
fn a_small_query_loop_has_a_number() {
    let cinrs_ns = unsafe { time_the_loop_through_cinrs() };
    println!("cinrs SQLite ({CONFIGURATION}): {} ns/query", cinrs_ns);
    #[cfg(feature = "system-sqlite")]
    {
        let system_ns = unsafe { system::time_the_loop() };
        println!("platform libsqlite3: {} ns/query", system_ns);
        println!(
            "ratio: cinrs / platform = {:.2}",
            cinrs_ns as f64 / system_ns as f64
        );
    }
    #[cfg(not(feature = "system-sqlite"))]
    println!(
        "the platform's libsqlite3 was not linked; run with --features system-sqlite to compare"
    );
}

/// The loop, over the SQLite this crate compiled.
unsafe fn time_the_loop_through_cinrs() -> u64 {
    const ROWS: i32 = 1000;
    const QUERIES: u32 = 1000;
    unsafe {
        let mut db: *mut sqlite3 = std::ptr::null_mut();
        let name = cs(":memory:");
        assert_eq!(sqlite3_open(name.as_ptr(), &raw mut db), OK);
        exec(db, "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER)");
        exec(db, "BEGIN");
        let insert = cs("INSERT INTO t (n) VALUES (?1)");
        let mut stmt: *mut sqlite3_stmt = std::ptr::null_mut();
        assert_eq!(
            sqlite3_prepare_v2(db, insert.as_ptr(), -1, &raw mut stmt, std::ptr::null_mut()),
            OK
        );
        for i in 0..ROWS {
            assert_eq!(sqlite3_bind_int(stmt, 1, i), OK);
            assert_eq!(sqlite3_step(stmt), DONE);
            assert_eq!(sqlite3_reset(stmt), OK);
        }
        assert_eq!(sqlite3_finalize(stmt), OK);
        exec(db, "COMMIT");

        let query = cs("SELECT count(*), sum(n) FROM t WHERE n > ?1");
        let mut stmt: *mut sqlite3_stmt = std::ptr::null_mut();
        assert_eq!(
            sqlite3_prepare_v2(db, query.as_ptr(), -1, &raw mut stmt, std::ptr::null_mut()),
            OK
        );
        let started = Instant::now();
        let mut total = 0i64;
        for i in 0..QUERIES {
            assert_eq!(sqlite3_bind_int(stmt, 1, (i % 100) as c_int), OK);
            assert_eq!(sqlite3_step(stmt), ROW);
            total += sqlite3_column_int64(stmt, 1);
            assert_eq!(sqlite3_reset(stmt), OK);
        }
        let elapsed = started.elapsed();
        assert!(total > 0, "the query answered something");
        assert_eq!(sqlite3_finalize(stmt), OK);
        assert_eq!(sqlite3_close(db), OK);
        (elapsed.as_nanos() / u128::from(QUERIES)) as u64
    }
}

/// The identical loop, through the platform's own `libsqlite3`.
///
/// The declarations are written out here rather than reused from the module
/// above, so that the two really are two libraries: these symbols come from
/// `-lsqlite3` and those from the translated C.
#[cfg(feature = "system-sqlite")]
mod system {
    use std::ffi::{CStr, c_char, c_int, c_void};
    use std::time::Instant;

    use super::{DONE, OK, ROW, cs};

    #[link(name = "sqlite3")]
    unsafe extern "C" {
        fn sqlite3_libversion() -> *const c_char;
        fn sqlite3_open(filename: *const c_char, db: *mut *mut c_void) -> c_int;
        fn sqlite3_close(db: *mut c_void) -> c_int;
        fn sqlite3_exec(
            db: *mut c_void,
            sql: *const c_char,
            cb: *mut c_void,
            arg: *mut c_void,
            err: *mut *mut c_char,
        ) -> c_int;
        fn sqlite3_prepare_v2(
            db: *mut c_void,
            sql: *const c_char,
            n: c_int,
            stmt: *mut *mut c_void,
            tail: *mut *const c_char,
        ) -> c_int;
        fn sqlite3_bind_int(stmt: *mut c_void, i: c_int, v: c_int) -> c_int;
        fn sqlite3_step(stmt: *mut c_void) -> c_int;
        fn sqlite3_reset(stmt: *mut c_void) -> c_int;
        fn sqlite3_finalize(stmt: *mut c_void) -> c_int;
        fn sqlite3_column_int64(stmt: *mut c_void, i: c_int) -> i64;
    }

    unsafe fn run(db: *mut c_void, sql: &str) {
        let sql = cs(sql);
        let rc = unsafe {
            sqlite3_exec(
                db,
                sql.as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(rc, OK, "the platform's sqlite3_exec({sql:?})");
    }

    /// The version the platform's library reports, printed for the record: it is
    /// not required to be the same release as the amalgamation.
    pub(super) fn version() -> String {
        unsafe {
            CStr::from_ptr(sqlite3_libversion())
                .to_string_lossy()
                .into_owned()
        }
    }

    pub(super) unsafe fn time_the_loop() -> u64 {
        const ROWS: i32 = 1000;
        const QUERIES: u32 = 1000;
        unsafe {
            println!("platform libsqlite3 version: {}", version());
            let mut db: *mut c_void = std::ptr::null_mut();
            let name = cs(":memory:");
            assert_eq!(sqlite3_open(name.as_ptr(), &raw mut db), OK);
            run(db, "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER)");
            run(db, "BEGIN");
            let insert = cs("INSERT INTO t (n) VALUES (?1)");
            let mut stmt: *mut c_void = std::ptr::null_mut();
            assert_eq!(
                sqlite3_prepare_v2(db, insert.as_ptr(), -1, &raw mut stmt, std::ptr::null_mut()),
                OK
            );
            for i in 0..ROWS {
                assert_eq!(sqlite3_bind_int(stmt, 1, i), OK);
                assert_eq!(sqlite3_step(stmt), DONE);
                assert_eq!(sqlite3_reset(stmt), OK);
            }
            assert_eq!(sqlite3_finalize(stmt), OK);
            run(db, "COMMIT");

            let query = cs("SELECT count(*), sum(n) FROM t WHERE n > ?1");
            let mut stmt: *mut c_void = std::ptr::null_mut();
            assert_eq!(
                sqlite3_prepare_v2(db, query.as_ptr(), -1, &raw mut stmt, std::ptr::null_mut()),
                OK
            );
            let started = Instant::now();
            let mut total = 0i64;
            for i in 0..QUERIES {
                assert_eq!(sqlite3_bind_int(stmt, 1, (i % 100) as c_int), OK);
                assert_eq!(sqlite3_step(stmt), ROW);
                total += sqlite3_column_int64(stmt, 1);
                assert_eq!(sqlite3_reset(stmt), OK);
            }
            let elapsed = started.elapsed();
            assert!(total > 0);
            assert_eq!(sqlite3_finalize(stmt), OK);
            assert_eq!(sqlite3_close(db), OK);
            (elapsed.as_nanos() / u128::from(QUERIES)) as u64
        }
    }
}
