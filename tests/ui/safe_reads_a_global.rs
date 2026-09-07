//@compile-flags: --crate-type lib
//! What a safe function may not do: touch a file-scope object.
//!
//! A C object with static storage duration becomes a `static mut` item, which
//! Rust reads and writes only in an `unsafe` block — the object is shared
//! between threads and nothing here can prove otherwise. C's own answer to
//! that is `_Thread_local` or a parameter.

cinrs::c99! {
    #pragma cinrs safe bump

    int counter;

    int bump(int by) {
        counter += by; //~ ERROR: use of mutable static is unsafe
        return counter; //~ ERROR: use of mutable static is unsafe
    }
}
