//@compile-flags: --crate-type lib
//! `safe` on a GNU nested function.
//!
//! A nested function is lifted to a file-scope item that takes a pointer to
//! each enclosing object it uses, and its body reads and writes through those
//! pointers — which is exactly the dereference a safe function may not do. It
//! could therefore never compile, so it is refused by name, with the reason.

cinrs::gnu99! {
    int report(int a, int b) {
        int tally = 0;

        //~v ERROR: 'note' is a nested function
        __attribute__((cinrs_safe)) void note(int value) { tally += value; }

        note(a);
        note(b);
        return tally;
    }
}
