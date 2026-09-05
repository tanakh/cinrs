//@compile-flags: --crate-type lib
//! `#pragma cinrs export` gives everything with external linkage a real C
//! symbol; a `thread_local!` item has no stable way to be given one.

cinrs::c11! {
    #pragma cinrs export

    _Thread_local int shared; //~ ERROR: cannot be exported

    /* Internal linkage is not exported either way, so this one is fine. */
    static _Thread_local int mine;

    int bump(void) { return ++shared + ++mine; }
}
