//@compile-flags: --crate-type lib
//! `thread_local!` is a `std` macro, so a thread-local object is the third
//! construct an expansion cannot have without `std` — after the variable
//! length array and `alloca`, which need an allocator.

#![no_std]

extern crate alloc;

cinrs::gnu11! {
    #pragma cinrs no_std

    _Thread_local int counter; //~ ERROR: requires std; this unit says no_std

    int bump(void) { return ++counter; }
}
