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

#[cfg(all(feature = "threadsafe", not(feature = "nothreads")))]
pub mod sqlite {
    //! `SQLITE_THREADSAFE=1`, SQLite's own default.
    cinrs::include_gnu11!("sqlite_threadsafe.c");
}

#[cfg(feature = "nothreads")]
pub mod sqlite {
    //! `SQLITE_THREADSAFE=0`.
    cinrs::include_gnu11!("sqlite_nothreads.c");
}

/// Which configuration this build is, for the smoke test to print.
pub const CONFIGURATION: &str = if cfg!(feature = "nothreads") {
    "SQLITE_THREADSAFE=0"
} else {
    "SQLITE_THREADSAFE=1"
};
