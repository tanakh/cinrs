//! Every input form, opened with rust-analyzer.
//!
//! `scripts/check-rust-analyzer.sh` runs `rust-analyzer diagnostics` over this
//! crate and fails if anything at all is reported about these files. That is a
//! harder question than it looks: rust-analyzer hands a procedural macro tokens
//! with **no positions** — no file, no source text, line 1 column 0 for every
//! token alike — so none of the blocks below can be read the way a `cargo build`
//! reads them, by slicing this file between the first and the last token. They
//! are read by finding *this* invocation in the crate's sources instead, which
//! is only possible because a saved file's text and its tokens agree exactly;
//! see `crates/cinrs-core/src/locate.rs`.
//!
//! Each block is here for a reason:
//!
//! * `fact` is the program from the bug report: raw tokens, no directives, and
//!   an operator (`==`) that arrives as two `Punct`s sharing one span.
//! * `area` is the hard case: `#include <…>`, `#include "…"` of a header beside
//!   this very file, a function-like `#define`, and an `#if` whose other arm is
//!   not C at all — four things that are *lines*, of which a token stream keeps
//!   nothing.
//! * `twice` is the string-literal form, which needs no positions in the first
//!   place and must keep working.
//! * `perimeter` comes from `include_c99!`, whose relative path is resolved
//!   against the directory of the `.rs` file the invocation is written in.
//! * `again::fact` is `fact` written a second time, word for word: two
//!   invocations with the same tokens is the ordinary case, and nothing in the
//!   input says which of them is being expanded.
//!
//! Finally `totals` *calls* all of them from Rust, so that a name the analysis
//! cannot resolve is an error of its own (`E0425`) rather than a silence.

use cinrs::{c99, include_c99};

c99! {
    int fact(int n) {
        return n == 0 ? 1 : n * fact(n - 1);
    }
}

c99! {
    #include <stdio.h>
    #include "shapes.h"

    #define AREA(w, h) ((w) * (h))

    #if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 199901L
    int area(const struct Rect *r) {
        return AREA(r->w, r->h);
    }
    #else
    this line is not C, and is never read
    #endif

    int report(const struct Rect *r) {
        return printf("%d\n", area(r));
    }
}

/// `fact` again, word for word, in a module of its own.
///
/// Two invocations with identical tokens are the ordinary case rather than a
/// strange one, and a host with no positions cannot tell which of them it is
/// expanding: both have to come out right. See `doc/features.md`, "Identical
/// invocations", for the whole rule.
pub mod again {
    cinrs::c99! {
        int fact(int n) {
            return n == 0 ? 1 : n * fact(n - 1);
        }
    }
}

c99! { r#"
    /* String-literal form: the text is the literal's, so a host with no
       positions has nothing to work out. */
    int twice(int x) { return x + x; }
"# }

include_c99!("c/geometry.c");

/// Every unit's own symbols, called from Rust.
pub fn totals() -> i32 {
    let rect = Rect { w: 2, h: 3 };
    unsafe { fact(4) + again::fact(4) + area(&rect) + twice(3) + perimeter(rect.w, rect.h) }
}
