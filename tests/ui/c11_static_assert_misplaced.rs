//! A static assertion where a declaration is not allowed — and *one* error
//! for it.
//!
//! `_Static_assert` is a declaration, so the grammar admits it in places C
//! then forbids: a function's parameter list (6.7.5.3, where every entry has
//! to be a parameter declaration) and the declaration list of an old-style
//! definition (6.9.1p6, where every declaration has to declare one of the
//! parameters). Clang's `C11/n1330.c` writes both.
//!
//! What matters here is the *count*. Each mistake is reported once and the
//! parse carries on: a definition whose parameter list was refused still owes
//! a body, so reporting its `{` as a stray one afterwards would be a second
//! error for one mistake — and would swallow everything the rest of the file
//! had to say. The failing assertion at the bottom is the proof that the file
//! is still being read.

cinrs::c11! {
    void in_a_declaration(
        _Static_assert(1, "not a parameter") //~ ERROR: expected a declaration, found keyword '_Static_assert'
    );

    void in_a_definition(
        _Static_assert(1, "not a parameter") //~ ERROR: expected a declaration, found keyword '_Static_assert'
    ) {}

    int knr(a, b)
      int a, b;
      _Static_assert(1, "not a parameter declaration"); //~ ERROR: not allowed in the declaration list of an old-style function definition
      { return a + b; }

    /* Still reading, and still checking. */
    _Static_assert(0, "the file was read to the end"); //~ ERROR: static assertion failed: "the file was read to the end"
}

fn main() {}
