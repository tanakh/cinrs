//! Where `_Thread_local` may and may not appear.
//!
//! C11 6.7.1 puts it on objects with static storage duration, which at block
//! scope means one of `static` or `extern` has to be there too; the rest is
//! what this crate cannot generate, each with the reason.

cinrs::c11! {
    void f(void) {
        _Thread_local int local; //~ ERROR: needs 'static' or 'extern'
    }

    void g(_Thread_local int p); //~ ERROR: not allowed on a parameter

    _Thread_local void h(void); //~ ERROR: not allowed on a function

    extern _Thread_local int elsewhere; //~ ERROR: `#[thread_local]`, which is unstable

    _Thread_local int seed = 1;
    _Thread_local int *link = &seed; //~ ERROR: not a compile-time constant expression

    void combined(void) {
        register _Thread_local int r; //~ ERROR: cannot be combined with 'auto' or 'register'
    }

    /* 6.7.1p3: the specifier goes on every declaration of the object. */
    _Thread_local int both;
    int both = 1; //~ ERROR: has to be on every declaration of an object

    /* A `thread_local!` item has no linker symbol of its own. */
    _Thread_local int placed __attribute__((section(".mine")));
    //~^ ERROR: has no linker symbol to name or to place in a section
    _Thread_local int renamed __asm__("other_name");
    //~^ ERROR: has no linker symbol to name or to place in a section
}

fn main() {}
