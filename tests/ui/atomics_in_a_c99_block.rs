//! `_Atomic` is C11's, and a `c99!` block says which macro to write instead.
//!
//! The *builtins* are a different matter: everything spelled with a leading
//! double underscore is available in every entry point, so `__atomic_*` and
//! `__sync_*` work here — which is exactly the line GCC draws between
//! `-std=c99` and `-std=gnu99`.

cinrs::c99! {
    _Atomic int counter; //~ ERROR: '_Atomic' requires C11 or later

    _Atomic(long) big; //~ ERROR: '_Atomic' requires C11 or later

    int *_Atomic qualified; //~ ERROR: '_Atomic' requires C11 or later

    /* The builtins are fine here. */
    int bump(int *p) { return __atomic_add_fetch(p, 1, __ATOMIC_SEQ_CST); }
    int older(int *p) { return __sync_fetch_and_add(p, 1); }
}

fn main() {}
