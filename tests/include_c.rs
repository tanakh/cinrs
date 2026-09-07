//! `include_c99!("…")` and its siblings: a C file translated where it stands.
//!
//! The files are in `tests/c/`. A relative path is resolved against the
//! directory of the `.rs` file the macro is written in — this one — so
//! `"c/geometry.c"` is how they are named here, and a header a file of theirs
//! includes by its bare name is found beside *that file* rather than beside
//! this one.
//!
//! What the expansion is, is unchanged: a module of the unit's items plus a
//! glob re-export, over exactly the translation the same text inside a `c99!`
//! block would get. What changes is where a diagnostic can point, and that is
//! in `tests/ui/include_c_*.rs`.

use std::ffi::CStr;

// One file, one translation unit. Its own `#pragma cinrs module "geometry"`
// names the module the expansion goes into.
cinrs::include_c99!("c/geometry.c");

// A GNU dialect has an entry point of its own, for a file written for one.
cinrs::include_gnu99!("c/tally.c");

#[test]
fn the_functions_of_an_included_file_are_callable() {
    // `Point` comes from `geometry.h`, which the *file* included; the type is
    // this unit's, and the unit named itself.
    let p = geometry::Point { x: -3, y: 4 };
    assert_eq!(point_manhattan(p), 7);
    let moved = unsafe { point_translate(p, 10, -10) };
    assert_eq!((moved.x, moved.y), (7, -6));
    assert_eq!(at_origin(geometry::Point { x: 0, y: 0 }), 1);
}

#[test]
fn a_bundled_header_works_from_an_included_file() {
    assert_eq!(unsafe { label_length(c"cinrs".as_ptr()) }, 5);
}

#[test]
fn a_pragma_inside_the_file_configures_the_unit() {
    // `#pragma cinrs safe point_manhattan at_origin` is in the file, so these
    // two need no `unsafe` at the call site — and the calls above prove it,
    // since this file has none on them.
    assert_eq!(point_manhattan(geometry::Point { x: 1, y: 2 }), 3);
}

#[test]
fn file_and_line_name_the_c_file() {
    // `__FILE__` is the `.c` file as the path was written to reach it, and
    // `__LINE__` counts that file's lines rather than the `.rs` file's.
    let name = unsafe { CStr::from_ptr(geometry_file()) };
    assert_eq!(name.to_str(), Ok("tests/c/geometry.c"));
    assert_eq!(unsafe { geometry_line() }, 46);
}

#[test]
fn a_gnu_file_gets_the_gnu_entry_point() {
    assert_eq!(unsafe { tally_max(3, 9) }, 9);
    assert_eq!(unsafe { tally_classify('7' as i32) }, 1);
    assert_eq!(unsafe { tally_classify('q' as i32) }, 2);
    assert_eq!(unsafe { tally_classify('!' as i32) }, 0);
}

#[test]
fn an_included_file_may_be_expanded_inside_a_function_body() {
    // An expansion is items, and items are allowed in a block: the module and
    // its glob re-export land in this scope rather than at the top of the
    // file.
    cinrs::include_c11!("c/inner.c");

    assert_eq!(inner_double(21), 42);
}
