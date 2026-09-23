//! SQLite 3.53.4, compiled from C by `cinrs`.
//!
//! One `include_gnu11!` over the amalgamation — 269,649 lines of C in one file,
//! the largest single C translation unit anyone ships. The
//! amalgamation itself is not in this repository: `scripts/check-sqlite.sh`
//! downloads it into `target/sqlite/`, verifies it against the SHA3-256
//! sqlite.org publishes, and then builds this crate.
//!
//! `gnu11` rather than `c11` because SQLite uses GNU extensions —
//! `__builtin_expect`, `__attribute__((noinline))`, `__builtin_add_overflow` —
//! and because a GNU entry point is what `gcc` would be given for it.
//!
//! The two configurations are two features and two wrapper units, because
//! `#pragma cinrs system_include` and `#define SQLITE_THREADSAFE 0` both have to
//! be in the unit's own text and the amalgamation is read unedited. See the
//! comments in `src/sqlite_threadsafe.c` and `src/sqlite_nothreads.c`.
//!
//! **`--features native-gcc` / `native-clang`** are for
//! `scripts/check-sqlite.sh --bench` only. No C is translated then: the
//! `sqlite` module is `sqlite3.h` as a header-only unit, so the declarations
//! and types are still cinrs's, and the functions come from the same
//! amalgamation compiled by `gcc -O2` or `clang -O2` with
//! `SQLITE_THREADSAFE=1` into `target/sqlite/native/` (`build.rs` says where).
//! They take precedence over `threadsafe`/`nothreads`. (`system-sqlite` is
//! different: it links the *platform's* `libsqlite3` beside the translated C,
//! for the smoke test's one-query comparison.)

#[cfg(all(
    feature = "threadsafe",
    not(feature = "nothreads"),
    not(feature = "native-gcc"),
    not(feature = "native-clang")
))]
pub mod sqlite {
    //! `SQLITE_THREADSAFE=1`, SQLite's own default.
    cinrs::include_gnu11!("sqlite_threadsafe.c");
}

#[cfg(all(
    feature = "nothreads",
    not(feature = "native-gcc"),
    not(feature = "native-clang")
))]
pub mod sqlite {
    //! `SQLITE_THREADSAFE=0`.
    cinrs::include_gnu11!("sqlite_nothreads.c");
}

#[cfg(feature = "native-gcc")]
pub mod sqlite {
    //! The API from `sqlite3.h`, the functions from `libsqlite3_gcc.a`.
    cinrs::gnu11! {
        #pragma cinrs link "sqlite3_gcc"
        #include "../../../target/sqlite/sqlite3.h"
    }
}

#[cfg(all(feature = "native-clang", not(feature = "native-gcc")))]
pub mod sqlite {
    //! The API from `sqlite3.h`, the functions from `libsqlite3_clang.a`.
    cinrs::gnu11! {
        #pragma cinrs link "sqlite3_clang"
        #include "../../../target/sqlite/sqlite3.h"
    }
}

/// Which configuration this build is, for the smoke test to print.
pub const CONFIGURATION: &str = if cfg!(feature = "native-gcc") {
    "native gcc -O2, SQLITE_THREADSAFE=1"
} else if cfg!(feature = "native-clang") {
    "native clang -O2, SQLITE_THREADSAFE=1"
} else if cfg!(feature = "nothreads") {
    "SQLITE_THREADSAFE=0"
} else {
    "SQLITE_THREADSAFE=1"
};
