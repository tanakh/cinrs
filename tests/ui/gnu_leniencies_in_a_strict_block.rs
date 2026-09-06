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

    /* A cast to a union type. */
    union u { int i; float f; };
    union u as_union(int n) { return (union u) n; } //~ ERROR: cast to a union type is a GNU extension

    /* An over-long string initialiser: 6.7.8p2 says no initializer may
     * provide a value for something outside the object, and GCC warns. */
    char three[3] = "1234"; //~ ERROR: initializer-string for char array is too long

    /* Folding the address of a member of a null pointer — the hand-written
     * `offsetof` — to an integer constant. */
    struct s { int a; int b; };
    static unsigned long b_at = (unsigned long) &((struct s *) 0)->b;
    //~^ ERROR: is not a compile-time constant expression
}

fn main() {}
