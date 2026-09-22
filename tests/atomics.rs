//! Integration tests that *run* translated C with atomics.
//!
//! Three things are being checked, and they are worth telling apart:
//!
//! * that the operations mean what C says they mean — a fetch-and-add answers
//!   the *old* value, an `add_and_fetch` the new one, a compare-and-exchange
//!   writes the value it saw back through `expected`;
//! * that they really are atomic, which only a second thread can show: every
//!   counter here is incremented from two threads at once and has to come out
//!   at exactly the sum;
//! * that `_Atomic` is a *type* — its size, its alignment inside a `struct`,
//!   and what `_Generic` sees when an lvalue of one is read.
//!
//! The generated code is `core::sync::atomic` throughout, reached with
//! `AtomicX::from_ptr` over the object's address; `std` is needed by the
//! threads in this file and by nothing in the expansion.

use std::thread;

use cinrs::{c11, c23, gnu99};

// ---------------------------------------------------------------------------
// two threads, one counter
// ---------------------------------------------------------------------------

c11! {
    #include <stdatomic.h>

    _Atomic int plain_counter;
    atomic_int header_counter;
    int racy_counter;

    /* The three spellings of the same increment. */
    void bump_object(void) { plain_counter++; }
    void bump_builtin(void) { __atomic_fetch_add(&racy_counter, 1, __ATOMIC_RELAXED); }
    void bump_sync(void) { __sync_fetch_and_add(&racy_counter, 1); }
    void bump_header(void) { atomic_fetch_add_explicit(&header_counter, 1, memory_order_relaxed); }

    int read_object(void) { return plain_counter; }
    int read_builtin(void) { return __atomic_load_n(&racy_counter, __ATOMIC_SEQ_CST); }
    int read_header(void) { return atomic_load(&header_counter); }

    void reset(void) {
        plain_counter = 0;
        racy_counter = 0;
        atomic_store(&header_counter, 0);
    }
}

/// Runs `body` on two threads, `count` times each, and waits for both.
fn in_two_threads(count: usize, body: fn()) {
    let handles: Vec<_> = (0..2)
        .map(|_| {
            thread::spawn(move || {
                for _ in 0..count {
                    body();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("the thread must not panic");
    }
}

#[test]
fn two_threads_agree_on_a_counter() {
    const N: usize = 20_000;
    unsafe { reset() };

    in_two_threads(N, || unsafe { bump_object() });
    assert_eq!(unsafe { read_object() }, (2 * N) as i32);

    in_two_threads(N, || unsafe { bump_builtin() });
    assert_eq!(unsafe { read_builtin() }, (2 * N) as i32);

    in_two_threads(N, || unsafe { bump_sync() });
    assert_eq!(unsafe { read_builtin() }, (4 * N) as i32);

    in_two_threads(N, || unsafe { bump_header() });
    assert_eq!(unsafe { read_header() }, (2 * N) as i32);
}

// ---------------------------------------------------------------------------
// what each operation answers
// ---------------------------------------------------------------------------

c11! {
    /* `fetch_op` answers the old value and `op_fetch` the new one; that is the
     * whole difference between the two halves of the family. */
    int fetch_add(int *p, int v) { return __atomic_fetch_add(p, v, __ATOMIC_SEQ_CST); }
    int add_fetch(int *p, int v) { return __atomic_add_fetch(p, v, __ATOMIC_SEQ_CST); }
    int fetch_sub(int *p, int v) { return __atomic_fetch_sub(p, v, __ATOMIC_SEQ_CST); }
    int sub_fetch(int *p, int v) { return __atomic_sub_fetch(p, v, __ATOMIC_SEQ_CST); }
    int fetch_and(int *p, int v) { return __atomic_fetch_and(p, v, __ATOMIC_SEQ_CST); }
    int and_fetch(int *p, int v) { return __atomic_and_fetch(p, v, __ATOMIC_SEQ_CST); }
    int fetch_or(int *p, int v) { return __atomic_fetch_or(p, v, __ATOMIC_SEQ_CST); }
    int or_fetch(int *p, int v) { return __atomic_or_fetch(p, v, __ATOMIC_SEQ_CST); }
    int fetch_xor(int *p, int v) { return __atomic_fetch_xor(p, v, __ATOMIC_SEQ_CST); }
    int xor_fetch(int *p, int v) { return __atomic_xor_fetch(p, v, __ATOMIC_SEQ_CST); }
    /* Rust has no `fetch_nand` for the integers, so this one is a
     * compare-exchange loop — and still answers what C says it does. */
    int fetch_nand(int *p, int v) { return __atomic_fetch_nand(p, v, __ATOMIC_SEQ_CST); }
    int nand_fetch(int *p, int v) { return __atomic_nand_fetch(p, v, __ATOMIC_SEQ_CST); }

    int exchange(int *p, int v) { return __atomic_exchange_n(p, v, __ATOMIC_SEQ_CST); }

    /* The `_n`-less forms take pointers to the values instead. */
    int generic_load(const int *p) { int out; __atomic_load(p, &out, __ATOMIC_SEQ_CST); return out; }
    void generic_store(int *p, int v) { __atomic_store(p, &v, __ATOMIC_SEQ_CST); }
    int generic_exchange(int *p, int v) {
        int out;
        __atomic_exchange(p, &v, &out, __ATOMIC_SEQ_CST);
        return out;
    }

    /* A narrower object, where the arithmetic wraps in its own width. */
    unsigned char byte_add(unsigned char *p, int v) {
        return __atomic_add_fetch(p, v, __ATOMIC_SEQ_CST);
    }
    long long wide_add(long long *p, long long v) {
        return __atomic_fetch_add(p, v, __ATOMIC_SEQ_CST);
    }
}

#[test]
fn each_operation_answers_what_c_says() {
    let mut v = 10;
    unsafe {
        assert_eq!(fetch_add(&mut v, 5), 10);
        assert_eq!(v, 15);
        assert_eq!(add_fetch(&mut v, 5), 20);
        assert_eq!(fetch_sub(&mut v, 4), 20);
        assert_eq!(sub_fetch(&mut v, 4), 12);

        v = 0b1100;
        assert_eq!(fetch_and(&mut v, 0b1010), 0b1100);
        assert_eq!(v, 0b1000);
        assert_eq!(and_fetch(&mut v, 0b0001), 0b0000);
        assert_eq!(fetch_or(&mut v, 0b0110), 0);
        assert_eq!(or_fetch(&mut v, 0b1000), 0b1110);
        assert_eq!(fetch_xor(&mut v, 0b1111), 0b1110);
        assert_eq!(xor_fetch(&mut v, 0b0001), 0b0000);

        v = 0b1100;
        assert_eq!(fetch_nand(&mut v, 0b1010), 0b1100);
        assert_eq!(v, !(0b1100 & 0b1010));
        v = 0b1100;
        assert_eq!(nand_fetch(&mut v, 0b1010), !(0b1100 & 0b1010));

        v = 3;
        assert_eq!(exchange(&mut v, 9), 3);
        assert_eq!(v, 9);

        assert_eq!(generic_load(&v), 9);
        generic_store(&mut v, 11);
        assert_eq!(v, 11);
        assert_eq!(generic_exchange(&mut v, 12), 11);
        assert_eq!(v, 12);

        let mut small = 250u8;
        assert_eq!(byte_add(&mut small, 10), 4);
        assert_eq!(small, 4);

        let mut big = i64::MAX;
        assert_eq!(wide_add(&mut big, 1), i64::MAX);
        assert_eq!(big, i64::MIN);
    }
}

// ---------------------------------------------------------------------------
// compare-and-exchange
// ---------------------------------------------------------------------------

c11! {
    #include <stdatomic.h>

    /* The strong form, whose `expected` is written back when it fails. */
    int cas_strong(int *p, int *expected, int desired) {
        return __atomic_compare_exchange_n(p, expected, desired, 0,
                                           __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE);
    }
    /* The weak one, which may fail for no reason at all — so it is written in
     * the loop C's own documentation puts it in. */
    int weak_max(_Atomic int *slot, int candidate) {
        int seen = atomic_load(slot);
        while (candidate > seen) {
            if (__c11_atomic_compare_exchange_weak(slot, &seen, candidate,
                                                   __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST))
                return 1;
        }
        return 0;
    }

    /* The header spelling, whose third argument is the value and whose second
     * is written back. */
    int header_cas(atomic_int *slot, int *expected, int desired) {
        return atomic_compare_exchange_strong(slot, expected, desired);
    }

    /* The `__sync_*` pair: one answers the flag, the other the old value. */
    int sync_bool_cas(int *p, int old, int fresh) {
        return __sync_bool_compare_and_swap(p, old, fresh);
    }
    int sync_val_cas(int *p, int old, int fresh) {
        return __sync_val_compare_and_swap(p, old, fresh);
    }

    /* A counter built out of nothing but a CAS loop, to be run from two
     * threads. */
    atomic_int cas_counter;
    void cas_bump(void) {
        int seen = atomic_load(&cas_counter);
        while (!atomic_compare_exchange_weak(&cas_counter, &seen, seen + 1)) {
        }
    }
    int cas_total(void) { return atomic_load(&cas_counter); }
}

#[test]
fn compare_and_exchange_writes_back_what_it_saw() {
    let mut v = 7;
    let mut expected = 7;
    unsafe {
        assert_eq!(cas_strong(&mut v, &mut expected, 8), 1);
        assert_eq!(v, 8);
        // It failed, so `expected` now holds what was really there.
        assert_eq!(cas_strong(&mut v, &mut expected, 9), 0);
        assert_eq!(expected, 8);
        assert_eq!(v, 8);

        let mut slot = 0;
        assert_eq!(weak_max(&mut slot, 5), 1);
        assert_eq!(weak_max(&mut slot, 3), 0);
        assert_eq!(weak_max(&mut slot, 9), 1);
        assert_eq!(slot, 9);

        let mut header = 1;
        let mut expected = 2;
        assert_eq!(header_cas(&mut header, &mut expected, 3), 0);
        assert_eq!(expected, 1, "the observed value comes back");
        assert_eq!(header_cas(&mut header, &mut expected, 3), 1);
        assert_eq!(header, 3);

        let mut sync = 4;
        assert_eq!(sync_bool_cas(&mut sync, 4, 5), 1);
        assert_eq!(sync_bool_cas(&mut sync, 4, 6), 0);
        assert_eq!(sync_val_cas(&mut sync, 5, 6), 5);
        assert_eq!(sync, 6);
    }
}

#[test]
fn a_cas_loop_counts_correctly_from_two_threads() {
    const N: usize = 20_000;
    in_two_threads(N, || unsafe { cas_bump() });
    assert_eq!(unsafe { cas_total() }, (2 * N) as i32);
}

// ---------------------------------------------------------------------------
// spin locks
// ---------------------------------------------------------------------------

c11! {
    #include <stdatomic.h>

    /* The `__atomic_test_and_set` / `__atomic_clear` pair, which work on one
     * byte of anything. */
    char builtin_lock;
    int builtin_guarded;

    void builtin_acquire(void) {
        while (__atomic_test_and_set(&builtin_lock, __ATOMIC_ACQUIRE)) {
        }
    }
    void builtin_release(void) { __atomic_clear(&builtin_lock, __ATOMIC_RELEASE); }
    void builtin_locked_bump(void) {
        builtin_acquire();
        builtin_guarded++;      /* an ordinary, non-atomic increment */
        builtin_release();
    }
    int builtin_guarded_value(void) { return builtin_guarded; }

    /* The same thing with `atomic_flag`, which is the one type C guarantees is
     * lock free. */
    atomic_flag flag_lock = ATOMIC_FLAG_INIT;
    int flag_guarded;

    void flag_locked_bump(void) {
        while (atomic_flag_test_and_set_explicit(&flag_lock, memory_order_acquire)) {
        }
        flag_guarded++;
        atomic_flag_clear_explicit(&flag_lock, memory_order_release);
    }
    int flag_guarded_value(void) { return flag_guarded; }

    /* And with the older `__sync` lock pair, whose `lock_test_and_set` is an
     * acquire exchange and whose `lock_release` is a release store of zero. */
    int sync_lock;
    int sync_guarded;
    void sync_locked_bump(void) {
        while (__sync_lock_test_and_set(&sync_lock, 1)) {
        }
        sync_guarded++;
        __sync_lock_release(&sync_lock);
    }
    int sync_guarded_value(void) { return sync_guarded; }
}

#[test]
fn a_spin_lock_keeps_two_threads_apart() {
    const N: usize = 5_000;

    in_two_threads(N, || unsafe { builtin_locked_bump() });
    assert_eq!(unsafe { builtin_guarded_value() }, (2 * N) as i32);

    in_two_threads(N, || unsafe { flag_locked_bump() });
    assert_eq!(unsafe { flag_guarded_value() }, (2 * N) as i32);

    in_two_threads(N, || unsafe { sync_locked_bump() });
    assert_eq!(unsafe { sync_guarded_value() }, (2 * N) as i32);
}

// ---------------------------------------------------------------------------
// pointers
// ---------------------------------------------------------------------------

c11! {
    #include <stdatomic.h>

    _Atomic(int *) shared_ptr;

    int *load_ptr(void) { return atomic_load(&shared_ptr); }
    void store_ptr(int *p) { atomic_store(&shared_ptr, p); }
    int *swap_ptr(int *p) { return atomic_exchange(&shared_ptr, p); }
    int cas_ptr(int **expected, int *desired) {
        return atomic_compare_exchange_strong(&shared_ptr, expected, desired);
    }

    /* 7.17.7.5 counts in *elements*… */
    int *scaled_step(int by) { return atomic_fetch_add(&shared_ptr, by); }
    int *scaled_back(int by) { return atomic_fetch_sub(&shared_ptr, by); }
    /* …and GCC's own builtin counts in *bytes*, which is the one place the two
     * families deliberately disagree. */
    int *byte_step(int by) { return __atomic_fetch_add(&shared_ptr, by, __ATOMIC_SEQ_CST); }

    /* `++` on an atomic pointer object is C's pointer arithmetic, so it moves
     * by one element. */
    void step_one(void) { shared_ptr++; }

    /* A plain (non-`_Atomic`) pointer object through the GCC builtin. */
    void *plain_swap(void **slot, void *fresh) {
        return __atomic_exchange_n(slot, fresh, __ATOMIC_SEQ_CST);
    }
}

#[test]
fn pointer_atomics_move_by_elements_or_by_bytes() {
    let mut cells = [0i32; 16];
    let base = cells.as_mut_ptr();
    unsafe {
        store_ptr(base);
        assert_eq!(load_ptr(), base);
        assert_eq!(swap_ptr(base.add(3)), base);
        assert_eq!(load_ptr(), base.add(3));

        let mut expected = base.add(3);
        assert_eq!(cas_ptr(&mut expected, base), 1);
        assert_eq!(load_ptr(), base);
        assert_eq!(cas_ptr(&mut expected, base.add(1)), 0);
        assert_eq!(expected, base, "the observed pointer comes back");

        // Elements: two `int`s is eight bytes.
        assert_eq!(scaled_step(2), base);
        assert_eq!(load_ptr(), base.add(2));
        assert_eq!(scaled_back(1), base.add(2));
        assert_eq!(load_ptr(), base.add(1));

        // Bytes: `__atomic_fetch_add` adds exactly what it is given.
        let before = load_ptr();
        assert_eq!(byte_step(4), before);
        assert_eq!(load_ptr(), before.add(1), "four bytes is one int");
        assert_eq!(load_ptr() as usize, before as usize + 4);

        step_one();
        assert_eq!(load_ptr(), base.add(3));

        let mut slot: *mut core::ffi::c_void = core::ptr::null_mut();
        let fresh = base.cast::<core::ffi::c_void>();
        assert!(plain_swap(&mut slot, fresh).is_null());
        assert_eq!(slot, fresh);
    }
}

// ---------------------------------------------------------------------------
// floating objects
// ---------------------------------------------------------------------------

c11! {
    /* A float has no atomic of its own in Rust; the load and the store go
     * through the integer atomic of the same width and `to_bits`/`from_bits`,
     * which is bit-for-bit what the object holds. */
    float load_float(float *p) { return __atomic_load_n(p, __ATOMIC_SEQ_CST); }
    void store_float(float *p, float v) { __atomic_store_n(p, v, __ATOMIC_SEQ_CST); }
    float swap_float(float *p, float v) { return __atomic_exchange_n(p, v, __ATOMIC_SEQ_CST); }
    double load_double(double *p) { return __atomic_load_n(p, __ATOMIC_SEQ_CST); }
    void store_double(double *p, double v) { __atomic_store_n(p, v, __ATOMIC_SEQ_CST); }

    /* An `_Atomic` floating object, whose `+=` is a compare-exchange loop
     * because no processor has an atomic float add. */
    _Atomic double accumulator;
    void accumulate(double v) { accumulator += v; }
    double accumulated(void) { return accumulator; }
    void reset_accumulator(void) { accumulator = 0.0; }

    /* A second one, for the test that runs on two threads: the tests of this
     * file run in parallel, and two of them sharing an object would be a race
     * of their own. */
    _Atomic double shared_total;
    void add_to_total(double v) { shared_total += v; }
    double total(void) { return shared_total; }
}

#[test]
fn floating_objects_go_through_their_bits() {
    let mut f = 1.5f32;
    let mut d = -2.25f64;
    unsafe {
        assert_eq!(load_float(&mut f), 1.5);
        store_float(&mut f, 3.5);
        assert_eq!(f, 3.5);
        assert_eq!(swap_float(&mut f, 4.5), 3.5);
        assert_eq!(f, 4.5);

        assert_eq!(load_double(&mut d), -2.25);
        store_double(&mut d, f64::INFINITY);
        assert!(d.is_infinite());

        // A NaN survives as the very bits it was written with.
        let nan = f32::from_bits(0x7fc0_1234);
        store_float(&mut f, nan);
        assert_eq!(load_float(&mut f).to_bits(), 0x7fc0_1234);

        reset_accumulator();
        accumulate(1.5);
        accumulate(2.25);
        assert_eq!(accumulated(), 3.75);
    }
}

#[test]
fn an_atomic_float_accumulates_from_two_threads() {
    // `total += 0.5` is a compare-exchange loop — no processor has an atomic
    // floating add — and every one of the four thousand has to land.
    const N: usize = 2_000;
    in_two_threads(N, || unsafe { add_to_total(0.5) });
    assert_eq!(unsafe { total() }, (2 * N) as f64 * 0.5);
}

// ---------------------------------------------------------------------------
// the `_Atomic` object model
// ---------------------------------------------------------------------------

c11! {
    #include <stdatomic.h>

    _Atomic int object;
    _Atomic unsigned char narrow;
    _Atomic long long wide;

    /* Every read is a load, every write a store, and `+=`, `++` and `--` are
     * one read-modify-write each. */
    int read_it(void) { return object; }
    void write_it(int v) { object = v; }
    int add_to_it(int v) { return object += v; }
    int multiply_it(int v) { return object *= v; }   /* no method: a CAS loop */
    int shift_it(int v) { return object <<= v; }
    int pre_increment(void) { return ++object; }
    int post_increment(void) { return object++; }
    int pre_decrement(void) { return --object; }
    int post_decrement(void) { return object--; }
    int or_it(int v) { return object |= v; }

    /* The value of an assignment is the value stored, not what the object
     * holds when the dust settles. */
    int assignment_value(int v) { return (object = v); }

    /* `&x` is an ordinary pointer, and a store through it is atomic too. */
    _Atomic int *address_of_it(void) { return &object; }
    void store_through(_Atomic int *p, int v) { *p = v; }
    int load_through(_Atomic int *p) { return *p; }

    /* A narrower object, where the compound assignment is computed in `int`
     * and narrowed on the way back. */
    unsigned char narrow_add(int v) { narrow += v; return narrow; }

    /* Passing one by value, and `sizeof` on it, work on the underlying type. */
    int by_value(int v) { return v; }
    int pass_it(void) { return by_value(object); }
    unsigned long size_of_object(void) { return sizeof object; }
    unsigned long size_of_type(void) { return sizeof(_Atomic long long); }
    unsigned long align_of_type(void) { return _Alignof(_Atomic long long); }
    unsigned long align_of_int(void) { return _Alignof(atomic_int); }

    /* Lvalue conversion drops the `_Atomic`, so `_Generic` sees `int`. */
    int generic_of_lvalue(void) { return _Generic(object, int: 1, _Atomic int: 2, default: 3); }
    int generic_of_wide(void) { return _Generic(wide, long long: 1, default: 0); }

    /* An `_Atomic` member of an ordinary struct: the alignment of the atomic
     * type is its size, and the record layout follows. */
    struct mixed {
        char tag;
        _Atomic long long counter;
        char trailer;
    };
    unsigned long mixed_size(void) { return sizeof(struct mixed); }
    unsigned long mixed_align(void) { return _Alignof(struct mixed); }
    unsigned long counter_offset(void) { return __builtin_offsetof(struct mixed, counter); }
    long long bump_member(struct mixed *m) { return ++m->counter; }
    long long member_value(struct mixed *m) { return m->counter; }
}

#[test]
fn an_atomic_object_reads_writes_and_updates() {
    unsafe {
        write_it(0);
        assert_eq!(read_it(), 0);
        assert_eq!(add_to_it(5), 5);
        assert_eq!(multiply_it(3), 15);
        assert_eq!(shift_it(1), 30);
        assert_eq!(or_it(1), 31);
        assert_eq!(pre_increment(), 32);
        assert_eq!(post_increment(), 32);
        assert_eq!(read_it(), 33);
        assert_eq!(pre_decrement(), 32);
        assert_eq!(post_decrement(), 32);
        assert_eq!(read_it(), 31);
        assert_eq!(assignment_value(4), 4);

        let p = address_of_it();
        assert_eq!(load_through(p), 4);
        store_through(p, 6);
        assert_eq!(read_it(), 6);
        assert_eq!(pass_it(), 6);

        assert_eq!(narrow_add(300), 300u32 as u8);
    }
}

#[test]
fn the_size_and_alignment_of_an_atomic_type() {
    unsafe {
        assert_eq!(size_of_object() as usize, size_of::<core::ffi::c_int>());
        // The alignment of an atomic type is at least its size — which is what
        // `AtomicI64` has in Rust and what GCC gives `_Atomic long long` even
        // where a plain one is four-byte aligned.
        assert_eq!(size_of_type(), 8);
        assert_eq!(align_of_type(), 8);
        assert_eq!(align_of_int() as usize, align_of::<core::ffi::c_int>());

        // char, seven bytes of padding, the counter, and the trailer rounded
        // up to the record's own alignment.
        assert_eq!(counter_offset(), 8);
        assert_eq!(mixed_align(), 8);
        assert_eq!(mixed_size(), 24);

        assert_eq!(generic_of_lvalue(), 1, "lvalue conversion drops _Atomic");
        assert_eq!(generic_of_wide(), 1);
    }
}

#[test]
fn an_atomic_member_is_atomic_from_two_threads() {
    // The Rust type of the record is what the C layout says it is, so the
    // member can be handed straight to the C.
    #[repr(C, align(8))]
    struct Mixed {
        tag: u8,
        _pad: [u8; 7],
        counter: i64,
        trailer: u8,
        _tail: [u8; 7],
    }
    assert_eq!(size_of::<Mixed>(), unsafe { mixed_size() } as usize);

    static mut RECORD: Mixed = Mixed {
        tag: 0,
        _pad: [0; 7],
        counter: 0,
        trailer: 0,
        _tail: [0; 7],
    };
    const N: usize = 20_000;
    in_two_threads(N, || unsafe {
        bump_member((&raw mut RECORD).cast());
    });
    assert_eq!(
        unsafe { member_value((&raw mut RECORD).cast()) },
        (2 * N) as i64
    );
}

// ---------------------------------------------------------------------------
// fences, lock-free queries and the memory orders
// ---------------------------------------------------------------------------

c11! {
    #include <stdatomic.h>

    int published;
    int payload;

    /* The release/acquire pair the fences exist for. */
    void publish(int v) {
        payload = v;
        __atomic_thread_fence(__ATOMIC_RELEASE);
        __atomic_store_n(&published, 1, __ATOMIC_RELAXED);
    }
    int consume(void) {
        if (!__atomic_load_n(&published, __ATOMIC_RELAXED))
            return -1;
        __atomic_thread_fence(__ATOMIC_ACQUIRE);
        return payload;
    }
    void signal_fence(void) { __atomic_signal_fence(__ATOMIC_ACQ_REL); }
    /* A relaxed fence is a no-op, and Rust's `fence` panics on one rather than
     * saying so — this must not. */
    void relaxed_fence(void) {
        __atomic_thread_fence(__ATOMIC_RELAXED);
        atomic_thread_fence(memory_order_relaxed);
        atomic_signal_fence(memory_order_relaxed);
    }
    void full_barrier(void) { __sync_synchronize(); }

    int lock_free_sizes(void) {
        return __atomic_always_lock_free(1, 0) + __atomic_always_lock_free(2, 0)
             + __atomic_always_lock_free(4, 0) + __atomic_always_lock_free(8, 0)
             + __atomic_always_lock_free(16, 0) + __atomic_always_lock_free(3, 0);
    }
    int is_lock_free_object(void) {
        atomic_int x = 0;
        return atomic_is_lock_free(&x) + __atomic_is_lock_free(sizeof(int), 0);
    }
    int lock_free_macros(void) {
        return ATOMIC_INT_LOCK_FREE + ATOMIC_POINTER_LOCK_FREE + ATOMIC_BOOL_LOCK_FREE;
    }

    /* The six orders, as the macros and as the enumeration. */
    int orders(void) {
        return (__ATOMIC_RELAXED == memory_order_relaxed)
             + (__ATOMIC_CONSUME == memory_order_consume)
             + (__ATOMIC_ACQUIRE == memory_order_acquire)
             + (__ATOMIC_RELEASE == memory_order_release)
             + (__ATOMIC_ACQ_REL == memory_order_acq_rel)
             + (__ATOMIC_SEQ_CST == memory_order_seq_cst);
    }
    /* `memory_order_consume` is an acquire everywhere; what matters is that it
     * is accepted and does the stronger thing. */
    int consume_load(int *p) { return __atomic_load_n(p, __ATOMIC_CONSUME); }
    int killed(int v) { return kill_dependency(v); }
}

#[test]
fn fences_lock_free_queries_and_the_orders() {
    unsafe {
        publish(42);
        assert_eq!(consume(), 42);
        signal_fence();
        relaxed_fence();
        full_barrier();

        // 1, 2, 4 and 8 are lock free; 16 and 3 are not.
        assert_eq!(lock_free_sizes(), 4);
        assert_eq!(is_lock_free_object(), 2);
        assert_eq!(lock_free_macros(), 6);
        assert_eq!(orders(), 6);

        let mut v = 5;
        assert_eq!(consume_load(&mut v), 5);
        assert_eq!(killed(7), 7);
    }
}

// ---------------------------------------------------------------------------
// the layout of a record with an atomic member
// ---------------------------------------------------------------------------

c11! {
    /* The alignment of an atomic type is its size, which is stricter than the
     * underlying type's wherever the ABI stops short of it — so these are the
     * records where what `cinrs` computed and what `rustc` lays the generated
     * `#[repr(C)]` item out as could come apart. */
    struct AtomicByte { _Atomic char a; char b; };
    struct AtomicInt { char a; _Atomic int b; char c; };
    struct AtomicWide { char a; _Atomic long long b; };
    struct AtomicPtrHolder { char a; _Atomic(int *) b; };
    struct AtomicDouble { char a; _Atomic double b; char c; };
    struct AtomicNested { struct AtomicWide inner; char tail; };
    struct AtomicAligned { char a; _Alignas(16) _Atomic int b; };
    union AtomicUnion { _Atomic long long a; char b; };

    unsigned long atomic_sizes(int which) {
        switch (which) {
            case 0: return sizeof(struct AtomicByte);
            case 1: return sizeof(struct AtomicInt);
            case 2: return sizeof(struct AtomicWide);
            case 3: return sizeof(struct AtomicPtrHolder);
            case 4: return sizeof(struct AtomicDouble);
            case 5: return sizeof(struct AtomicNested);
            case 6: return sizeof(struct AtomicAligned);
            case 7: return sizeof(union AtomicUnion);
            default: return 0;
        }
    }
    unsigned long atomic_aligns(int which) {
        switch (which) {
            case 0: return _Alignof(struct AtomicByte);
            case 1: return _Alignof(struct AtomicInt);
            case 2: return _Alignof(struct AtomicWide);
            case 3: return _Alignof(struct AtomicPtrHolder);
            case 4: return _Alignof(struct AtomicDouble);
            case 5: return _Alignof(struct AtomicNested);
            case 6: return _Alignof(struct AtomicAligned);
            case 7: return _Alignof(union AtomicUnion);
            default: return 0;
        }
    }
    unsigned long atomic_offsets(int which) {
        switch (which) {
            case 0: return __builtin_offsetof(struct AtomicInt, b);
            case 1: return __builtin_offsetof(struct AtomicInt, c);
            case 2: return __builtin_offsetof(struct AtomicWide, b);
            case 3: return __builtin_offsetof(struct AtomicPtrHolder, b);
            case 4: return __builtin_offsetof(struct AtomicDouble, b);
            case 5: return __builtin_offsetof(struct AtomicNested, tail);
            case 6: return __builtin_offsetof(struct AtomicAligned, b);
            default: return 0;
        }
    }
}

#[test]
fn a_record_with_an_atomic_member_lays_out_the_same_on_both_sides() {
    // What the C front end folded has to be what `rustc` gives the generated
    // item — the same check `tests/aggregates.rs` makes for every other
    // record, on the one member type whose alignment C and Rust disagree
    // about unless the item says otherwise.
    let sizes = [
        size_of::<AtomicByte>(),
        size_of::<AtomicInt>(),
        size_of::<AtomicWide>(),
        size_of::<AtomicPtrHolder>(),
        size_of::<AtomicDouble>(),
        size_of::<AtomicNested>(),
        size_of::<AtomicAligned>(),
        size_of::<AtomicUnion>(),
    ];
    let aligns = [
        align_of::<AtomicByte>(),
        align_of::<AtomicInt>(),
        align_of::<AtomicWide>(),
        align_of::<AtomicPtrHolder>(),
        align_of::<AtomicDouble>(),
        align_of::<AtomicNested>(),
        align_of::<AtomicAligned>(),
        align_of::<AtomicUnion>(),
    ];
    for (index, (size, align)) in sizes.iter().zip(&aligns).enumerate() {
        assert_eq!(
            unsafe { atomic_sizes(index as i32) } as usize,
            *size,
            "sizeof, record {index}"
        );
        assert_eq!(
            unsafe { atomic_aligns(index as i32) } as usize,
            *align,
            "alignof, record {index}"
        );
    }
    let offsets = [
        core::mem::offset_of!(AtomicInt, b),
        core::mem::offset_of!(AtomicInt, c),
        core::mem::offset_of!(AtomicWide, b),
        core::mem::offset_of!(AtomicPtrHolder, b),
        core::mem::offset_of!(AtomicDouble, b),
        core::mem::offset_of!(AtomicNested, tail),
        core::mem::offset_of!(AtomicAligned, b),
    ];
    for (index, offset) in offsets.iter().enumerate() {
        assert_eq!(
            unsafe { atomic_offsets(index as i32) } as usize,
            *offset,
            "offsetof, member {index}"
        );
    }

    // And the numbers themselves, on the host: an eight-byte atomic is
    // eight-byte aligned, which is what a lock-free instruction needs.
    assert_eq!(sizes[0], 2, "an atomic char aligns like a char");
    assert_eq!(aligns[0], 1);
    assert_eq!(offsets[2], 8, "the atomic long long moves to eight");
    assert_eq!(aligns[2], 8);
    assert_eq!(sizes[2], 16);
    assert_eq!(aligns[6], 16, "_Alignas raises it further");
    assert_eq!(aligns[7], 8);
    assert_eq!(sizes[7], 8);
}

// ---------------------------------------------------------------------------
// where else an `_Atomic` object can turn up
// ---------------------------------------------------------------------------

c23! {
    _Atomic int tracked = 3;

    /* A parameter and a return type may be atomic; the value passed and the
     * value returned have the underlying type either way. */
    _Atomic int through_value(_Atomic int x) { x += 1; return x; }

    /* `const _Atomic` reads as an atomic load like any other — including
     * through a pointer, where the `*const` the pointer is generated as has
     * to become the `*mut` that `from_ptr` takes. */
    const _Atomic int frozen = 7;
    int read_frozen(void) { return frozen; }
    int read_const(const _Atomic int *p) { return *p; }
    struct frozen_holder { const _Atomic int v; };
    int read_member(const struct frozen_holder *h) { return h->v; }

    /* A cast to an atomic type is a conversion to the underlying one: the
     * result of a cast is not an lvalue, so there is nothing to qualify. */
    long cast_out(void) { return (long) tracked; }
    int cast_in(void) { return (_Atomic int) 5L; }

    /* `typeof` keeps the `_Atomic`; `typeof_unqual` takes it off (C23
     * 6.7.2.5p3). */
    typeof(tracked) same;
    typeof_unqual(tracked) plain;
    void write_both(int v) { same = v; plain = v; }
    int read_both(void) { return same + plain; }

    /* A controlling expression is an ordinary value, so a `switch` and a
     * condition work on the loaded one. */
    int classify(void) {
        switch (tracked) {
        case 1: return 10;
        case 3: return 30;
        default: return 0;
        }
    }
    int truthy(void) { return tracked ? 1 : 0; }

    /* A function with a `goto` becomes a state machine, and the atomic
     * accesses inside it come out the same. */
    int jumpy(void) {
        int total = 0;
    again:
        total += tracked;
        if (--tracked > 0) goto again;
        return total;
    }

    /* A local one, whose object is an ordinary `let mut` binding. */
    int local_counter(void) {
        _Atomic int n = 0;
        n++;
        n += 4;
        return n;
    }

    /* An *array* of atomics is an ordinary array of them — the constraint C
     * places is on `_Atomic(T[N])`, not on `_Atomic T a[N]`. */
    _Atomic int slots[4];
    int slot_bump(int i) { return ++slots[i]; }

    /* And an enumeration, which is an `int` underneath. */
    enum colour { red, green, blue };
    _Atomic enum colour shade;
    int next_colour(void) { shade = green; return shade; }

    /* The value of a chained assignment is the value stored. */
    int chained(void) { int a; a = tracked = 9; return a; }
}

#[test]
fn an_atomic_object_in_every_position() {
    unsafe {
        assert_eq!(through_value(1), 2);
        assert_eq!(read_frozen(), 7);
        let n = 4;
        assert_eq!(read_const(&n), 4);
        let holder = frozen_holder { v: 9 };
        assert_eq!(read_member(&holder), 9);
        assert_eq!(cast_out(), 3);
        assert_eq!(cast_in(), 5);

        write_both(6);
        assert_eq!(read_both(), 12);

        assert_eq!(classify(), 30);
        assert_eq!(truthy(), 1);
        assert_eq!(jumpy(), 3 + 2 + 1);
        assert_eq!(local_counter(), 5);

        assert_eq!(slot_bump(2), 1);
        assert_eq!(slot_bump(2), 2);
        assert_eq!(next_colour(), 1);
        assert_eq!(chained(), 9);
    }
}

c11! {
    #include <stdatomic.h>

    /* A `_Bool` object is an `AtomicBool`, and the operators it does have —
     * `|=`, `&=`, `^=` and `++` — are its own methods. */
    _Atomic _Bool boolean;
    int flip(void) { boolean = !boolean; return boolean; }
    int or_bool(void) { boolean |= 1; return boolean; }
    int and_bool(void) { boolean &= 0; return boolean; }
    int bump_bool(void) { return boolean++; }

    /* An operator no atomic has a method for, on an object narrower than the
     * type the arithmetic happens in. */
    _Atomic short narrow_word;
    _Atomic unsigned char narrow_byte;
    _Atomic unsigned char shared_byte;
    int shift_word(void) { narrow_word <<= 3; return narrow_word; }
    int add_byte(int v) { narrow_byte += v; return narrow_byte; }
    int add_shared(int v) { shared_byte += v; return shared_byte; }

    /* A local `atomic_flag`, whose object is an ordinary binding. */
    int local_lock(void) {
        atomic_flag f = ATOMIC_FLAG_INIT;
        int first = !atomic_flag_test_and_set(&f);
        int again = !atomic_flag_test_and_set(&f);
        atomic_flag_clear(&f);
        return first * 100 + again * 10 + !atomic_flag_test_and_set(&f);
    }
}

#[test]
fn the_narrow_and_bool_shapes() {
    unsafe {
        assert_eq!(flip(), 1);
        assert_eq!(and_bool(), 0);
        assert_eq!(or_bool(), 1);
        assert_eq!(bump_bool(), 1, "`b++` answers the old value");

        assert_eq!(shift_word(), 0);
        assert_eq!(
            add_byte(300),
            300u32 as u8 as i32,
            "narrowed on the way back"
        );
        assert_eq!(add_byte(1), (300u32 as u8).wrapping_add(1) as i32);

        assert_eq!(local_lock(), 101);
    }
}

#[test]
fn a_compare_exchange_loop_counts_correctly_from_two_threads() {
    // `shared_byte += 1` is computed in `int` and narrowed on the way back,
    // which is the shape that becomes the general compare-exchange loop
    // rather than a `fetch_add`. Two threads, a hundred each: not one of them
    // may be lost.
    const N: usize = 100;
    in_two_threads(N, || unsafe {
        add_shared(1);
    });
    assert_eq!(unsafe { add_shared(0) }, 2 * N as i32);
}

// ---------------------------------------------------------------------------
// the operands that are evaluated and ignored
// ---------------------------------------------------------------------------

c11! {
    int side_effects;
    int noisy(int v) { side_effects += v; return v; }

    /* GCC takes a memory order that is not a constant and falls back to
     * `__ATOMIC_SEQ_CST`; the expression is still evaluated, because C says
     * it is. */
    int runtime_order(int *p, int order) {
        return __atomic_fetch_add(p, 1, noisy(order));
    }

    /* The `__sync_*` builtins take any number of trailing arguments — GCC's
     * "list of variables to be protected" — which are evaluated and ignored
     * in the same way. */
    int sync_extra(int *p) { return __sync_fetch_and_add(p, 1, noisy(4), noisy(8)); }
    void sync_barrier(void) { __sync_synchronize(noisy(16)); }

    int effects(void) { return side_effects; }
    void clear_effects(void) { side_effects = 0; }
}

#[test]
fn an_operand_that_is_only_evaluated_is_still_evaluated() {
    unsafe {
        clear_effects();
        let mut v = 0;
        assert_eq!(runtime_order(&mut v, 2), 0);
        assert_eq!(v, 1);
        assert_eq!(effects(), 2, "the order expression ran");

        clear_effects();
        assert_eq!(sync_extra(&mut v), 1);
        assert_eq!(v, 2);
        assert_eq!(effects(), 12, "both trailing arguments ran");

        clear_effects();
        sync_barrier();
        assert_eq!(effects(), 16);
    }
}

// ---------------------------------------------------------------------------
// function pointers
// ---------------------------------------------------------------------------

c11! {
    #include <stdatomic.h>

    /* SQLite's `AtomicStore(&sqlite3GlobalConfig.xLog, xLog)` is this: an
     * ordinary member of function-pointer type, written through an atomic
     * store so that a reader never sees half a pointer. In Rust the value is
     * an `Option<unsafe extern "C" fn(…)>`, which is pointer-sized with the
     * null pointer as its `None`, so it goes through the same `AtomicPtr` as
     * any other pointer with a `transmute` at each end. */
    typedef int (*logger)(int);

    int twice(int n) { return n * 2; }
    int thrice(int n) { return n * 3; }

    struct config { int flags; logger xLog; };
    struct config global;

    void set_log(logger f) { __atomic_store_n(&global.xLog, f, __ATOMIC_SEQ_CST); }
    logger get_log(void) { return __atomic_load_n(&global.xLog, __ATOMIC_SEQ_CST); }
    logger swap_log(logger f) { return __atomic_exchange_n(&global.xLog, f, __ATOMIC_SEQ_CST); }

    int cas_log(logger *expected, logger desired) {
        return __atomic_compare_exchange_n(&global.xLog, expected, desired, 0,
                                           __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST);
    }

    int call_log(int n) {
        logger f = __atomic_load_n(&global.xLog, __ATOMIC_SEQ_CST);
        if (f == 0) return -1;
        return f(n);
    }

    int log_is_null(void) { return __atomic_load_n(&global.xLog, __ATOMIC_SEQ_CST) == 0; }

    /* The same object as a real `_Atomic`, through the header's generic
     * functions and through a plain assignment, which is an atomic store. */
    _Atomic(logger) hook;

    void set_hook(logger f) { atomic_store(&hook, f); }
    logger get_hook(void) { return atomic_load(&hook); }
    logger swap_hook(logger f) { return atomic_exchange(&hook, f); }
    int cas_hook(logger *e, logger d) { return atomic_compare_exchange_strong(&hook, e, d); }
    void assign_hook(logger f) { hook = f; }
    int call_hook(int n) { logger f = hook; return f == 0 ? -1 : f(n); }

    /* `__sync_*` says the same thing about the same object. */
    logger sync_swap_hook(logger f) { return __sync_lock_test_and_set(&global.xLog, f); }
    int sync_cas_hook(logger old, logger new_) {
        return __sync_bool_compare_and_swap(&global.xLog, old, new_);
    }
}

#[test]
fn a_function_pointer_is_loaded_and_stored_atomically() {
    unsafe {
        // A null function pointer is `None`, and reads back as one.
        assert_eq!(log_is_null(), 1);
        assert_eq!(call_log(10), -1);

        set_log(Some(twice));
        assert_eq!(log_is_null(), 0);
        assert_eq!(call_log(10), 20);
        assert_eq!({ get_log() }.expect("a function")(21), 42);

        // An exchange answers the old value and installs the new one.
        let old = swap_log(Some(thrice));
        assert_eq!(old.expect("the old function")(10), 20);
        assert_eq!(call_log(10), 30);

        // A compare-exchange that succeeds, then one that fails and writes
        // back what it saw.
        let mut expected: Option<unsafe extern "C" fn(core::ffi::c_int) -> core::ffi::c_int> =
            Some(thrice);
        assert_eq!(cas_log(&mut expected, Some(twice)), 1);
        assert_eq!(call_log(10), 20);

        let mut wrong = Some(thrice as unsafe extern "C" fn(core::ffi::c_int) -> core::ffi::c_int);
        assert_eq!(cas_log(&mut wrong, None), 0);
        assert_eq!(
            wrong.expect("the observed value")(10),
            20,
            "the value it saw was written back"
        );
        assert_eq!(call_log(10), 20, "and nothing was stored");

        // Storing a null pointer back, through the CAS this time.
        let mut seen = Some(twice as unsafe extern "C" fn(core::ffi::c_int) -> core::ffi::c_int);
        assert_eq!(cas_log(&mut seen, None), 1);
        assert_eq!(log_is_null(), 1);

        // `__sync_lock_test_and_set` is an exchange, and the old value is null.
        assert!(sync_swap_hook(Some(twice)).is_none());
        assert_eq!(call_log(4), 8);
        assert_eq!(sync_cas_hook(Some(twice), Some(thrice)), 1);
        assert_eq!(call_log(4), 12);
        assert_eq!(sync_cas_hook(Some(twice), None), 0);
        assert_eq!(call_log(4), 12);
    }
}

#[test]
fn an_atomic_object_of_function_pointer_type() {
    unsafe {
        assert_eq!(call_hook(10), -1);

        set_hook(Some(twice));
        assert_eq!(call_hook(10), 20);
        assert_eq!({ get_hook() }.expect("a function")(3), 6);

        let old = swap_hook(Some(thrice));
        assert_eq!(old.expect("the old function")(10), 20);
        assert_eq!(call_hook(10), 30);

        let mut expected: Option<unsafe extern "C" fn(core::ffi::c_int) -> core::ffi::c_int> =
            Some(thrice);
        assert_eq!(cas_hook(&mut expected, Some(twice)), 1);
        assert_eq!(call_hook(10), 20);

        // A plain assignment to an `_Atomic` object is an atomic store.
        assign_hook(Some(thrice));
        assert_eq!(call_hook(10), 30);
        assign_hook(None);
        assert_eq!(call_hook(10), -1);
    }
}

// ---------------------------------------------------------------------------
// the other entry points
// ---------------------------------------------------------------------------

gnu99! {
    /* Everything spelled with a leading double underscore is available in
     * every entry point, `_Atomic` and `<stdatomic.h>` are not — but a GNU
     * dialect takes a later revision's feature as an extension, so `gnu99!`
     * has both. */
    _Atomic int gnu_counter;
    int gnu_bump(void) { return ++gnu_counter; }
    int gnu_builtin(int *p) { return __atomic_add_fetch(p, 1, __ATOMIC_SEQ_CST); }
    int gnu_sync(int *p) { return __sync_add_and_fetch(p, 1); }
}

c23! {
    #include <stdatomic.h>

    /* C23 spells the version macro of the header, and `char8_t` is atomic
     * there too. */
    atomic_char8_t byte;
    int header_version(void) { return __STDC_VERSION_STDATOMIC_H__ == 202311L; }
    unsigned char bump_byte(void) { return ++byte; }
}

#[test]
fn the_other_entry_points_have_them_too() {
    unsafe {
        assert_eq!(gnu_bump(), 1);
        let mut v = 0;
        assert_eq!(gnu_builtin(&mut v), 1);
        assert_eq!(gnu_sync(&mut v), 2);

        assert_eq!(header_version(), 1);
        assert_eq!(bump_byte(), 1);
    }
}
