//! The two rules C99 removed, used in a block that is not C89.
//!
//! Implicit `int` (N635) and the implicit declaration of a function (N636)
//! are C89's, and `c89!` and `gnu89!` have them. Everywhere else — the GNU
//! dialects included, exactly as in GCC 14, which errors on both in every mode
//! from `-std=c99` up — they are what they have been since 1999: errors.

cinrs::c99! {
    static counter;     //~ ERROR: type specifier missing; C99 does not support implicit 'int'

    bump(void)          //~ ERROR: type specifier missing; C99 does not support implicit 'int'
    {
        return counter;
    }
}

cinrs::gnu99! {
    int magnitude(int n)
    {
        return abs(n);  //~ ERROR: implicit declaration of function 'abs' is invalid in C99
    }
}

fn main() {}
