//! The handful of places GCC takes a constraint violation as a warning and
//! carries on. The GNU dialects follow it; a strict entry point keeps the
//! error and names the macro that would accept it.
//!
//! `doc/gnu-extensions.md` has the list. Pointer-sign mismatches are *not* on
//! it: those are accepted everywhere, because refusing them would refuse most
//! of the C that has ever been written.

cinrs::c11! {
    void nothing(void);
    int something(void);

    void forwards(void) {
        return something(); //~ ERROR: should not return a value
    }

    double takes_double(double a);
    int takes_int(int a);

    int compare(void) {
        double (*a)(double) = &takes_double;
        int (*b)(int) = &takes_int;
        return a == b; //~ ERROR: comparison of distinct function pointer types
    }

    unsigned long how_big(void) {
        return sizeof(void); //~ ERROR: invalid application of 'sizeof' to an incomplete type 'void'
    }

    unsigned long how_aligned(void) {
        return __alignof__(void); //~ ERROR: invalid application of '_Alignof' to an incomplete type 'void'
    }
}

cinrs::c99! {
    int declared(void);
    ; //~ ERROR: expected a declaration, found ';'
}

fn main() {}
