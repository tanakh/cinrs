//@compile-flags: --crate-type lib
//! A C error inside an included file.
//!
//! There is no C in this file for a caret to point at, so the message carries
//! the position — `tests/ui/c/bad.c:3:12: ` — exactly as one inside an
//! `#include`d header does, and the caret goes on the invocation. That is the
//! honest trade of `include_c99!`: neither `cargo` nor an IDE can jump into
//! the `.c` file, and the position is in the text instead.

//~v ERROR: tests/ui/c/bad.c:3:12: implicit declaration of function 'frobnicate'
cinrs::include_c99!("c/bad.c");
