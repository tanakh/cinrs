//! What `atomic_flag` will not do.
//!
//! C11 7.17.8 gives the type exactly two operations, and its member is not one
//! of them: an `atomic_flag` may only be tested, cleared and initialised with
//! `ATOMIC_FLAG_INIT`. Everything below is what a program that treats it as an
//! ordinary object gets, and the diagnostics come from the type rules rather
//! than from a special case — which is the point.

cinrs::c11! {
    #include <stdatomic.h>

    atomic_flag lock = ATOMIC_FLAG_INIT;

    /* It is a `struct`, so there is no conversion to a scalar… */
    int flag_as_value(void) { return lock; }
    //~^ ERROR: returning 'struct atomic_flag' from a function with incompatible result type 'int'

    /* …and no atomic operation takes one: the argument has to point at an
       `_Atomic` object, and a `struct` holding one is not itself atomic. */
    int flag_load(void) { return atomic_load(&lock); }
    //~^ ERROR: must be a pointer to an '_Atomic' type, and 'struct atomic_flag *' is not

    /* The two operations take the *address* of the flag, not the flag. */
    int by_value(void) { return atomic_flag_test_and_set(lock); }
    //~^ ERROR: member reference type 'struct atomic_flag' is not a pointer

    /* `ATOMIC_FLAG_INIT` is a braced initialiser and not an expression. */
    void assign(void) { lock = ATOMIC_FLAG_INIT; }
    //~^ ERROR: expected expression, found '{'
}

fn main() {}
