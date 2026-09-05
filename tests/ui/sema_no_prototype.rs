//! Function declarators with no prototype.
//!
//! Before C23 `int f()` says nothing about the parameters, so a call with
//! arguments is fine; what is *not* fine is a second declaration whose
//! parameters the default argument promotions would change, since a call
//! through the empty list could never reach them (C99 6.7.5.3p15).

cinrs::c99! {
    int narrow();
    int narrow(char c); //~ ERROR: conflicting types

    int widened(float x);
    int widened(); //~ ERROR: conflicting types

    int takes_a_char(char c);
    int use_pointer(void) {
        int (*fp)() = takes_a_char; //~ ERROR: cannot initialize
        return fp(0);
    }
}

// C23 removed the form: there `int f()` is `int f(void)`, and an argument is
// one too many.
cinrs::c23! {
    int nothing();
    int call(void) { return nothing(1); } //~ ERROR: too many arguments
}

fn main() {}
