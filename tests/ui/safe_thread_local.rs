//@compile-flags: --crate-type lib
//! What a safe function may not do: reach a `_Thread_local` object.
//!
//! Such an object is a `std::thread_local!` holding an `UnsafeCell`, and every
//! C access goes through the `*mut T` the cell hands out — `with` is safe, and
//! the dereference of what it produces is not. There is no safe way to read an
//! `UnsafeCell`, which is the whole point of the type, so a thread-local object
//! belongs to the non-safe part of a unit.

cinrs::c11! {
    #pragma cinrs safe bump

    _Thread_local int counter;

    int bump(int by) {
        counter += by; //~ ERROR: dereference of raw pointer is unsafe
        return counter; //~ ERROR: dereference of raw pointer is unsafe
    }
}
