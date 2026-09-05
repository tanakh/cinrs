//@check-pass
//@compile-flags: --crate-type lib
//! Atomics need no `std`: `core::sync::atomic` is `core`, so `_Atomic`,
//! `<stdatomic.h>` and all three builtin families compile into a `#![no_std]`
//! crate exactly as they stand.

#![no_std]

cinrs::c11! {
    #include <stdatomic.h>

    _Atomic int counter;
    atomic_flag lock = ATOMIC_FLAG_INIT;
    _Atomic(int *) cursor;
    _Atomic double total;

    int bump(void) { return ++counter; }
    int take(void) { return !atomic_flag_test_and_set(&lock); }
    void give(void) { atomic_flag_clear(&lock); }

    int cas(int *expected, int desired) {
        return atomic_compare_exchange_strong_explicit(&counter, expected, desired,
                                                       memory_order_acq_rel,
                                                       memory_order_acquire);
    }
    int *step(void) { return atomic_fetch_add(&cursor, 1); }
    void accumulate(double v) { total += v; }

    int builtins(int *p, char *byte) {
        __atomic_thread_fence(__ATOMIC_SEQ_CST);
        __sync_synchronize();
        int old = __atomic_fetch_nand(p, 3, __ATOMIC_SEQ_CST);
        old += __sync_val_compare_and_swap(p, old, 0);
        old += __atomic_test_and_set(byte, __ATOMIC_ACQUIRE);
        __atomic_clear(byte, __ATOMIC_RELEASE);
        return old + atomic_is_lock_free(&counter);
    }
}

pub fn call(p: *mut core::ffi::c_int, byte: *mut core::ffi::c_char) -> core::ffi::c_int {
    unsafe {
        let mut expected = 0;
        bump() + take() + cas(&mut expected, 1) + builtins(p, byte) + step() as core::ffi::c_int
    }
}

pub fn more(v: core::ffi::c_double) {
    unsafe {
        give();
        accumulate(v);
    }
}
