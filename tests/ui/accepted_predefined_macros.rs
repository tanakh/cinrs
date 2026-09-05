//@check-pass
//@compile-flags: --crate-type lib
//! The GCC predefined limit and type macros, and a line splice in the middle
//! of an identifier — two things a great deal of portable C depends on and
//! neither of which is spelled out in a header.
//!
//! Both are checked for their *values* in `tests/preprocessor.rs`; this is the
//! part that has to hold on every toolchain, which is that the unit compiles
//! at all. The C goes in as a string literal because a `\` line continuation
//! is not something Rust's own lexer will hand over.

cinrs::c99! { r#"
    /* `__SIZE_TYPE__` and friends are what a program written for an unknown
     * compiler uses before it has included anything. GCC's own torture suite
     * names `__INT_MAX__` in ninety-five files. */
    __SIZE_TYPE__ a_size;
    __PTRDIFF_TYPE__ a_difference;
    __INTPTR_TYPE__ an_integer_pointer;
    __UINT32_TYPE__ an_exact_width;
    __WCHAR_TYPE__ a_wide_character;

    /* Each of these is a diagnostic if the macro is undefined, because an
     * undefined identifier is 0 in an `#if` and the array bound then goes
     * negative. */
    int int_max_fits[__INT_MAX__ >= 32767 ? 1 : -1];
    int long_is_at_least_int[__LONG_MAX__ >= __INT_MAX__ ? 1 : -1];
    int size_max_is_unsigned[__SIZE_MAX__ > 0 ? 1 : -1];
    int a_byte_is_eight_bits[__CHAR_BIT__ == 8 ? 1 : -1];

    double the_largest_double = __DBL_MAX__;

    /* Translation phase 2 deletes a backslash-newline before the source is
     * split into tokens, so this is one identifier. */
    #line 10000
    int the_line_number(void) { return __LI\
NE__; }
"# }
