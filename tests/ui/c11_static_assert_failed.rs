//! A failing static assertion is reported at the assertion, with the message
//! the program wrote — in both the C11 and the C23 spelling.

cinrs::c11! {
    _Static_assert(sizeof(int) == 8, "int is not 64 bits"); //~ ERROR: static assertion failed

    struct Sized {
        int x;
        _Static_assert(sizeof(int) == 8, "not in a struct either"); //~ ERROR: static assertion failed
    };

    int f(void) {
        _Static_assert(1 == 2, "not in a block either"); //~ ERROR: static assertion failed
        return 0;
    }
}

cinrs::c23! {
    // C23 makes the message optional.
    static_assert(2 + 2 == 5); //~ ERROR: static assertion failed
}

fn main() {}
