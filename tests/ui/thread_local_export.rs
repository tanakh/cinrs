//@compile-flags: --crate-type lib
//! `#pragma cinrs export` gives a thread-local object with external linkage
//! the accessor another unit reaches it through (`tests/thread_local_units.rs`
//! runs one), so exporting one is fine. What is refused is a disagreement
//! C11 6.7.1p3 forbids: every declaration of an object says `_Thread_local`,
//! or none does — `extern` ones included.

cinrs::c11! {
    #pragma cinrs export

    _Thread_local int shared;

    /* Internal linkage is not exported either way. */
    static _Thread_local int mine;

    extern int plain;
    extern _Thread_local int plain; //~ ERROR: is declared '_Thread_local' in one declaration and not in another

    extern _Thread_local int tls_only;
    extern int tls_only; //~ ERROR: is declared '_Thread_local' in one declaration and not in another

    int bump(void) { return ++shared + ++mine; }
}
