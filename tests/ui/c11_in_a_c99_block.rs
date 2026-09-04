//! A C11 construct in a `c99!` block says which macro to write instead. The
//! C11 keywords are all spelled with a leading underscore, which C99 reserves,
//! so they are recognised everywhere and this is a gate rather than a syntax
//! error.

cinrs::c99! {
    _Static_assert(1, "not in C99"); //~ ERROR: requires C11 or later

    int f(void) {
        return _Generic(1, int: 1, default: 0); //~ ERROR: requires C11 or later
    }
}

fn main() {}
