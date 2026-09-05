//@check-pass
//@compile-flags: --crate-type lib
//! `#pragma cinrs no_std` in a `#![no_std]` crate that has an `alloc`.
//!
//! A variable length array and `alloca` are the only two constructs whose
//! expansion needs more than `core`: the elements live in a `Vec`. The pragma
//! says to take it from `alloc` rather than from `std`, and the crate has to
//! provide `extern crate alloc;` itself, since a procedural macro cannot add
//! one — an expansion is items, not crate-level directives.

#![no_std]

extern crate alloc;

cinrs::gnu99! {
    #pragma cinrs no_std
    #include <alloca.h>

    long sum(int n) {
        int a[n];
        int *scratch = alloca(n * sizeof(int));
        for (int i = 0; i < n; i++) {
            a[i] = i;
            scratch[i] = i * 2;
        }
        long total = 0;
        for (int i = 0; i < n; i++) {
            total += a[i] + scratch[i];
        }
        return total + (long)sizeof a;
    }

    /* The same thing in a function that jumps, where every local — the hidden
       storage included — is bound at the top of the state machine. */
    long jumpy(int n) {
        long total = 0;
        char buf[n];
    again:
        buf[--n] = (char)n;
        total += buf[n];
        if (n > 0) goto again;
        return total + (long)sizeof buf;
    }
}

pub fn call(n: core::ffi::c_int) -> core::ffi::c_long {
    unsafe { sum(n) + jumpy(n) }
}
