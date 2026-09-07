//@compile-flags: --crate-type lib
//! `include_c99!` takes one string literal and nothing else.
//!
//! Not a path expression, not a macro that would produce one: the file has to
//! be read while this expansion is running, so the name has to be there in the
//! source.

//~v ERROR: include_c99! takes one string literal naming a C file
cinrs::include_c99!(c / geometry.c);

//~v ERROR: include_c99! takes one string literal naming a C file
cinrs::include_c99!("a.c", "b.c");
