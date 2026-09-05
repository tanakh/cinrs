//! The memory orders an operation may not be performed with.
//!
//! Rust *panics* at run time on `load(Release)` and `store(Acquire)`, and C
//! makes both undefined (7.17.7.1p2, 7.17.7.2p3); a diagnostic here is the
//! only answer that helps. The two rules about a compare-and-exchange's
//! failure order are 7.17.7.4p2's: it may not be a release, and it may not be
//! stronger than the success order.

cinrs::c11! {
    #include <stdatomic.h>

    int bad_load(int *p) {
        return __atomic_load_n(p, __ATOMIC_RELEASE);
        //~^ ERROR: may not be performed with 'memory_order_release'
    }

    int bad_load_acq_rel(int *p) {
        return __atomic_load_n(p, __ATOMIC_ACQ_REL);
        //~^ ERROR: may not be performed with 'memory_order_acq_rel'
    }

    void bad_store(int *p) {
        __atomic_store_n(p, 1, __ATOMIC_ACQUIRE);
        //~^ ERROR: may not be performed with 'memory_order_acquire'
    }

    void bad_store_acq_rel(int *p) {
        __atomic_store_n(p, 1, __ATOMIC_ACQ_REL);
        //~^ ERROR: may not be performed with 'memory_order_acq_rel'
    }

    void bad_clear(char *p) {
        __atomic_clear(p, __ATOMIC_ACQUIRE);
        //~^ ERROR: may not be performed with 'memory_order_acquire'
    }

    int failure_is_a_release(int *p, int *expected) {
        return __atomic_compare_exchange_n(p, expected, 1, 0,
                                           __ATOMIC_SEQ_CST, __ATOMIC_RELEASE);
        //~^ ERROR: the failure memory order of '__atomic_compare_exchange_n' may not be 'memory_order_release'
    }

    int failure_is_stronger(int *p, int *expected) {
        return __atomic_compare_exchange_n(p, expected, 1, 0,
                                           __ATOMIC_RELAXED, __ATOMIC_SEQ_CST);
        //~^ ERROR: may not be stronger than the success order
    }

    int header_failure_is_stronger(atomic_int *p, int *expected) {
        return atomic_compare_exchange_strong_explicit(p, expected, 1,
                                                       memory_order_acquire,
                                                       memory_order_seq_cst);
        //~^ ERROR: may not be stronger than the success order
    }

    int no_such_order(int *p) {
        return __atomic_load_n(p, 42); //~ ERROR: has no memory order 42
    }
}

fn main() {}
