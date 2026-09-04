//! User headers: `#include "…"`, the search order, the two ways of writing an
//! include guard, and the pragmas that configure them.
//!
//! The headers live in `tests/include/`. A quoted include searches the
//! directory of the file the directive is written in first, which for the
//! macro's own text is the directory of *this* `.rs` file — so `"include/…"`
//! is how this file names them, and a header of its own includes its
//! neighbours by their bare names.

use cinrs::c99;

// ---------------------------------------------------------------------------
// a quoted header, and the nested one it includes
// ---------------------------------------------------------------------------

#[test]
fn a_quoted_header_is_found_next_to_the_rs_file() {
    c99! {
        #include "include/point.h"

        int point_manhattan(struct Point p) {
            return (p.x < 0 ? -p.x : p.x) + (p.y < 0 ? -p.y : p.y);
        }

        struct Point point_translate(struct Point p, int dx, int dy) {
            struct Point out;
            out.x = p.x + dx;
            out.y = p.y + dy;
            return out;
        }

        int origin_is_zero(void) {
            return POINT_ORIGIN_X == 0 && POINT_ORIGIN_Y == 0;
        }

        /* From vec.h, which point.h includes by its bare name — relative to
         * point.h's own directory rather than to this file's. */
        int scaled(int v) { return vec_scaled(v); }
    }

    let p = Point { x: -3, y: 4 };
    unsafe {
        assert_eq!(point_manhattan(p), 7);
        let moved = point_translate(p, 10, -10);
        assert_eq!((moved.x, moved.y), (7, -6));
        assert_eq!(origin_is_zero(), 1);
        assert_eq!(scaled(5), 15);
    }
}

// ---------------------------------------------------------------------------
// include guards
// ---------------------------------------------------------------------------

#[test]
fn a_header_included_twice_is_read_once() {
    c99! {
        /* point.h has an `#ifndef` guard and pulls in vec.h, which has
         * `#pragma once`; including both again defines nothing twice. */
        #include "include/point.h"
        #include "include/vec.h"
        #include "include/point.h"

        int point_manhattan(struct Point p) { return p.x + p.y; }
        struct Point point_translate(struct Point p, int dx, int dy) {
            struct Point out;
            out.x = p.x + dx;
            out.y = p.y + dy;
            return out;
        }
        int scale(void) { return VEC_SCALE; }
    }

    unsafe {
        assert_eq!(scale(), 3);
        assert_eq!(point_manhattan(Point { x: 1, y: 2 }), 3);
    }
}

// ---------------------------------------------------------------------------
// one header, two translation units
// ---------------------------------------------------------------------------

/// A header shared between two `c99!` blocks, each a translation unit of its
/// own.
///
/// Two things follow from that, and both are on show here. Each block
/// generates the items the header describes — the `struct Point`, the
/// declarations — so two blocks that include it cannot be written in one Rust
/// module, where two items of a name would collide; a module each is enough.
/// And a function one block *defines* is a Rust item rather than an exported C
/// symbol, so the other block cannot link to it: what they share is the
/// header's types, macros and prototypes.
///
/// The header is named relative to this `.rs` file rather than to the module:
/// what a quoted include searches is the directory of the file the directive
/// is *written* in, and every module in this file is written in this file.
mod shared {
    pub mod producer {
        cinrs::c99! {
            #include "include/point.h"

            int point_manhattan(struct Point p) {
                return (p.x < 0 ? -p.x : p.x) + (p.y < 0 ? -p.y : p.y);
            }

            struct Point point_translate(struct Point p, int dx, int dy) {
                struct Point out;
                out.x = p.x + dx;
                out.y = p.y + dy;
                return out;
            }
        }
    }

    pub mod consumer {
        cinrs::c99! {
            /* The same header, read a second time in a second unit: the type
             * and the macros it defines are what this one builds on. */
            #include "include/point.h"

            struct Point scaled_point(int x, int y) {
                struct Point p;
                p.x = vec_scaled(x + POINT_ORIGIN_X);
                p.y = vec_scaled(y + POINT_ORIGIN_Y);
                return p;
            }
        }
    }
}

#[test]
fn one_header_serves_two_translation_units() {
    unsafe {
        let scaled = shared::consumer::scaled_point(2, 3);
        assert_eq!((scaled.x, scaled.y), (6, 9));
        let moved = shared::producer::point_translate(shared::producer::Point { x: 1, y: 1 }, 2, 3);
        assert_eq!((moved.x, moved.y), (3, 4));
        assert_eq!(
            shared::producer::point_manhattan(shared::producer::Point { x: -2, y: 5 }),
            7
        );
    }
}

// ---------------------------------------------------------------------------
// #pragma cinrs
// ---------------------------------------------------------------------------

#[test]
fn an_include_path_pragma_makes_a_header_available_in_angle_brackets() {
    c99! {
        /* Relative to CARGO_MANIFEST_DIR, so it says the same thing however
         * the build was invoked. */
        #pragma cinrs include_path "tests/include"
        #include <myheader.h>

        int my_answer(void) { return MY_ANSWER; }
    }

    assert_eq!(unsafe { my_answer() }, 42);
}

#[test]
fn a_link_pragma_is_accepted_and_names_a_library() {
    c99! {
        /* Harmless here — the maths functions are in libc on this platform and
         * the Rust runtime links what is left — but this is the mechanism a
         * program that calls into its own library uses. */
        #pragma cinrs link "m"
        #include <math.h>

        double root(double x) { return sqrt(x); }
    }

    assert_eq!(unsafe { root(9.0) }, 3.0);
}

// ---------------------------------------------------------------------------
// the macro-expanded form of #include (6.10.2p4)
// ---------------------------------------------------------------------------

#[test]
fn the_header_name_may_come_out_of_a_macro() {
    c99! {
        #define QUOTED_HEADER "include/point.h"
        #define ANGLED_HEADER <stdint.h>
        #include QUOTED_HEADER
        #include ANGLED_HEADER

        int point_manhattan(struct Point p) { return p.x + p.y; }
        struct Point point_translate(struct Point p, int dx, int dy) {
            struct Point out;
            out.x = p.x + dx;
            out.y = p.y + dy;
            return out;
        }

        int32_t widths(void) { return (int32_t)sizeof(int64_t); }
    }

    unsafe {
        assert_eq!(widths(), 8);
        assert_eq!(point_manhattan(Point { x: 20, y: 22 }), 42);
    }
}

// ---------------------------------------------------------------------------
// rebuild tracking
// ---------------------------------------------------------------------------

/// Every user header read during expansion is named by an `include_str!` in
/// the expansion, so that Cargo rebuilds this test when one of them changes.
///
/// The item is what proves it; the value is the header's own text, which is
/// checked here for the one this block reads.
#[test]
fn a_user_header_is_tracked_for_rebuilds() {
    c99! {
        #include "include/vec.h"
        int scale(void) { return VEC_SCALE; }
    }

    // The same file the macro read, reached the way the macro records it.
    let tracked = include_str!("include/vec.h");
    assert!(tracked.contains("#define VEC_SCALE 3"));
    assert_eq!(unsafe { scale() }, 3);
}

// ---------------------------------------------------------------------------
// string-literal input
// ---------------------------------------------------------------------------

/// Everything above is written as raw Rust tokens; a `c99!` block written as a
/// string literal means exactly the same thing, headers, pragmas and all. That
/// is why the configuration is C syntax rather than a Rust attribute.
#[test]
fn headers_and_pragmas_work_in_string_literal_mode() {
    c99! { r#"
        #pragma cinrs include_path "tests/include"
        #include "include/point.h"
        #include <myheader.h>

        int point_manhattan(struct Point p) { return p.x + p.y; }

        struct Point point_translate(struct Point p, int dx, int dy) {
            struct Point out;
            out.x = p.x + dx;
            out.y = p.y + dy;
            return out;
        }

        int my_answer(void) { return MY_ANSWER + vec_scaled(0); }
    "# }

    unsafe {
        assert_eq!(my_answer(), 42);
        assert_eq!(point_manhattan(Point { x: 40, y: 2 }), 42);
    }
}
