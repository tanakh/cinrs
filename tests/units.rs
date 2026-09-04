//! One invocation is one translation unit — and one Rust module.
//!
//! Every expansion goes into a private module of its own, glob re-exported
//! into the module the invocation is written in. That is what lets two blocks
//! in one Rust module both `#include` the same header: each generates the
//! `struct Point` its own code refers to, and two `struct Point` items in two
//! modules are not a redefinition. The price is that *using* the shared name
//! from Rust is ambiguous, which `#pragma cinrs module` gives a way to say.
//!
//! `#pragma cinrs export` is the other half: a unit that asks for it defines
//! real C symbols, so another unit can call its functions and read its objects
//! by name instead of only sharing types with it.

use cinrs::c99;

// ---------------------------------------------------------------------------
// two blocks in one module
// ---------------------------------------------------------------------------

c99! {
    #pragma cinrs module "first"
    /* Exported, so that the unit in `header_client` below can call what this
     * one defines rather than only sharing its types. */
    #pragma cinrs export
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

c99! {
    #pragma cinrs module "second"
    #include "include/point.h"

    /* The same header, so the same `struct Point` — a second time, in a
     * module of its own. */
    int point_quadrant(struct Point p) {
        if (p.x >= 0 && p.y >= 0) return 1;
        if (p.x < 0 && p.y >= 0) return 2;
        if (p.x < 0) return 3;
        return 4;
    }
}

#[test]
fn two_blocks_in_one_module_may_share_a_header() {
    // `Point` alone would be ambiguous — it is glob re-exported twice — so
    // each block's module names the one it means. The two are the same
    // `#[repr(C)]` layout, which is what makes passing one to the other work.
    let p = first::Point { x: -3, y: 4 };
    let q = second::Point { x: -3, y: 4 };
    unsafe {
        assert_eq!(point_manhattan(p), 7);
        assert_eq!(point_quadrant(q), 2);
        let moved = point_translate(p, 10, -10);
        assert_eq!((moved.x, moved.y), (7, -6));
    }
}

#[test]
fn a_block_in_a_function_body_is_a_module_in_a_block() {
    // Items — modules included — are allowed inside a block, so an invocation
    // in statement position works exactly as one at module scope does. Two of
    // them in one body is the interesting case: two modules, two glob
    // re-exports, one scope.
    c99! {
        /* No name: the module is the generated one, which nothing outside
         * needs to say — this unit keeps its `struct Point` to itself. */
        #include "include/point.h"

        int inner_x(int x, int y) {
            struct Point p;
            p.x = x;
            p.y = y;
            return p.x;
        }
    }

    c99! {
        #pragma cinrs module "other"
        #include "include/point.h"

        int inner_y(struct Point p) { return p.y; }
    }

    // Each unit's `struct Point` is a type of its own, so a value crosses from
    // Rust into the unit whose module it was built from.
    let p = other::Point { x: 6, y: 7 };
    unsafe {
        assert_eq!(inner_x(6, 7), 6);
        assert_eq!(inner_y(p), 7);
    }
}

// ---------------------------------------------------------------------------
// C linkage between units
// ---------------------------------------------------------------------------

/// A unit whose functions and objects are real C symbols.
mod exporter {
    use cinrs::c99;

    c99! {
        #pragma cinrs export

        int cinrs_test_counter = 40;

        /* `static` keeps its internal linkage even here: it is not part of
         * what the unit exports, and nothing outside can name it. */
        static int bonus(void) { return 2; }

        int cinrs_test_helper(int n) { return n + bonus(); }
    }

    #[test]
    fn an_exported_unit_is_still_an_ordinary_rust_module() {
        unsafe {
            assert_eq!(cinrs_test_helper(0), 2);
        }
    }
}

/// Another unit, which only *declares* what the first one defines.
mod importer {
    use cinrs::c99;

    c99! {
        /* A prototype with no definition becomes an `extern "C"` declaration,
         * which the linker resolves against the symbol the other block
         * exported. Without `#pragma cinrs export` over there, this would not
         * link at all. */
        int cinrs_test_helper(int n);
        extern int cinrs_test_counter;

        int answer(void) { return cinrs_test_helper(cinrs_test_counter); }
    }

    #[test]
    fn one_unit_links_against_another() {
        assert_eq!(unsafe { answer() }, 42);
    }
}

/// A third unit, sharing a header with the exporting one and calling into it.
mod header_client {
    use cinrs::c99;

    c99! {
        /* point.h declares `point_manhattan`; this unit does not define it,
         * so the declaration is linked rather than generated. */
        #include "include/point.h"
        #pragma cinrs module "client"

        int distance_from_origin(int x, int y) {
            struct Point p;
            p.x = x;
            p.y = y;
            return point_manhattan(p);
        }
    }

    #[test]
    fn a_shared_header_can_be_a_shared_implementation_too() {
        assert_eq!(unsafe { distance_from_origin(-3, 4) }, 7);
    }
}
