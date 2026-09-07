//@compile-flags: --crate-type lib
//! A file `include_c99!` cannot find.
//!
//! The path is resolved against the directory of *this* file, which the
//! message says, and the caret is on the invocation — the only place in a
//! `.rs` file that has anything to do with the missing `.c`.

//~v ERROR: cannot find 'c/nowhere.c'
cinrs::include_c99!("c/nowhere.c");
