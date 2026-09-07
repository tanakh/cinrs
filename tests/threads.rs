//! Integration tests that *run* translated C with thread-local objects.
//!
//! C11's `_Thread_local` (`thread_local` in C23, `__thread` in GNU C) gives an
//! object static storage duration and one instance per thread, which is
//! exactly what Rust's `std::thread_local!` provides. What the tests are
//! really about is that "per thread" is true of the generated code: a counter
//! incremented from two `std::thread::spawn`ed threads must come out at the
//! same value in each.
//!
//! The expansion of one of these units needs `std`, which is the one way they
//! differ from everything else this crate generates.
//!
//! The threads themselves are Rust's here, because what is under test is the
//! *object*. C11's own thread library — `<threads.h>`, where the threads are
//! made from translated C — is in `tests/c11_threads.rs`.

use std::thread;

use cinrs::{c11, c23, gnu11};

// ---------------------------------------------------------------------------
// one instance per thread
// ---------------------------------------------------------------------------

#[test]
fn a_counter_is_private_to_each_thread() {
    c11! {
        _Thread_local int counter;

        int bump(int by) {
            counter += by;
            return counter;
        }

        int read(void) { return counter; }
    }

    // The main thread has its own copy, and so does each spawned one.
    assert_eq!(unsafe { bump(1) }, 1);
    assert_eq!(unsafe { bump(1) }, 2);

    let handles: Vec<_> = (0..2)
        .map(|_| {
            thread::spawn(|| {
                let mut last = 0;
                for _ in 0..100 {
                    last = unsafe { bump(3) };
                }
                last
            })
        })
        .collect();
    for handle in handles {
        // Each thread saw only its own three hundred, not the other's.
        assert_eq!(handle.join().expect("the thread must not panic"), 300);
    }

    // …and the main thread's copy was not touched by either of them.
    assert_eq!(unsafe { read() }, 2);
}

/// A `static _Thread_local` inside a function: block scope, static storage
/// duration, one copy per thread (C11 6.7.1p3 requires the `static`).
#[test]
fn a_block_scope_thread_local_is_per_thread_too() {
    c11! {
        int calls(void) {
            static _Thread_local int n = 0;
            n++;
            return n;
        }
    }

    assert_eq!(unsafe { calls() }, 1);
    assert_eq!(unsafe { calls() }, 2);
    let other = thread::spawn(|| unsafe { calls() })
        .join()
        .expect("the thread must not panic");
    assert_eq!(other, 1);
    assert_eq!(unsafe { calls() }, 3);
}

// ---------------------------------------------------------------------------
// the address of one
// ---------------------------------------------------------------------------

/// `&x` on a thread-local object is a pointer to *this* thread's copy, valid
/// for as long as the thread is — which is what C promises.
#[test]
fn a_pointer_to_a_thread_local_reaches_this_threads_copy() {
    c11! {
        _Thread_local long value = 41;

        long *address(void) { return &value; }

        long through_pointer(void) {
            long *p = address();
            *p += 1;
            return value;
        }
    }

    assert_eq!(unsafe { through_pointer() }, 42);
    let other = thread::spawn(|| unsafe { through_pointer() })
        .join()
        .expect("the thread must not panic");
    // The other thread started from its own initialiser, not from 42.
    assert_eq!(other, 42);
    assert_eq!(unsafe { value.with(|c| *c.get()) }, 42);
}

// ---------------------------------------------------------------------------
// initialisers
// ---------------------------------------------------------------------------

/// A thread-local object's initialiser is a constant expression, and every
/// thread starts from it.
#[test]
fn each_thread_starts_from_the_initializer() {
    c11! {
        struct Point { int x; int y; };

        _Thread_local struct Point origin = { 3, 4 };
        _Thread_local int table[4] = { 1, 2, 3, 4 };
        _Thread_local const char *label = "start";
        _Thread_local double scale = 0.5;

        int sum(void) {
            int total = origin.x + origin.y;
            for (int i = 0; i < 4; i++) total += table[i];
            return total + (int) label[0] + (int) (scale * 8);
        }

        void clobber(void) {
            origin.x = 0;
            origin.y = 0;
            for (int i = 0; i < 4; i++) table[i] = 0;
            label = "";
            scale = 0.0;
        }
    }

    // 3 + 4 + (1+2+3+4) + 's' (115) + 4
    let expected = 3 + 4 + 10 + i32::from(b's') + 4;
    assert_eq!(unsafe { sum() }, expected);
    unsafe { clobber() };
    assert_eq!(unsafe { sum() }, 0);

    let other = thread::spawn(|| unsafe { sum() })
        .join()
        .expect("the thread must not panic");
    assert_eq!(other, expected);
}

/// An object with static storage duration and no initialiser is
/// zero-initialised, aggregates included — which is `mem::zeroed`, and which
/// a `const` block accepts.
#[test]
fn an_object_with_no_initializer_starts_out_zero() {
    c11! {
        struct Blank { int x; double y; char name[8]; };
        union Either { int i; double d; };

        _Thread_local struct Blank blank;
        _Thread_local union Either either;
        _Thread_local int table[4];
        _Thread_local char text[8] = "hi";

        int sum(void) {
            int total = blank.x + (int) blank.y + either.i;
            for (int i = 0; i < 4; i++) total += table[i];
            for (int i = 0; i < 8; i++) total += text[i];
            return total;
        }
    }

    // 'h' + 'i' and nothing else.
    let expected = i32::from(b'h') + i32::from(b'i');
    assert_eq!(unsafe { sum() }, expected);
    let other = thread::spawn(|| unsafe { sum() })
        .join()
        .expect("the thread must not panic");
    assert_eq!(other, expected);
}

// ---------------------------------------------------------------------------
// the other two spellings
// ---------------------------------------------------------------------------

#[test]
fn the_c23_spelling() {
    c23! {
        thread_local int depth = 7;
        int get_depth(void) { return depth; }
    }

    assert_eq!(unsafe { get_depth() }, 7);
}

#[test]
fn the_gnu_spelling() {
    gnu11! {
        __thread int slot = 9;
        int get_slot(void) { return slot; }
    }

    assert_eq!(unsafe { get_slot() }, 9);
}

/// GCC's `__thread` is spelled with a leading double underscore, so it is
/// reserved and available in a strict entry point too.
#[test]
fn the_gnu_spelling_in_strict_c99() {
    cinrs::c99! {
        __thread int reserved_spelling = 11;
        int get_it(void) { return reserved_spelling; }
    }

    assert_eq!(unsafe { get_it() }, 11);
}

// ---------------------------------------------------------------------------
// reaching one from Rust
// ---------------------------------------------------------------------------

/// An object with external linkage becomes a `pub` `thread_local!` item, which
/// Rust code reads through `with` and the cell's `get`.
#[test]
fn rust_reaches_the_object_through_the_generated_item() {
    c11! {
        _Thread_local int shared = 5;
        void set(int v) { shared = v; }
    }

    assert_eq!(shared.with(|c| unsafe { *c.get() }), 5);
    unsafe { set(6) };
    assert_eq!(shared.with(|c| unsafe { *c.get() }), 6);
    // Writing from the Rust side is the same pointer C uses.
    shared.with(|c| unsafe { *c.get() = 7 });
    unsafe { set(8) };
    assert_eq!(shared.with(|c| unsafe { *c.get() }), 8);

    let other = thread::spawn(|| shared.with(|c| unsafe { *c.get() }))
        .join()
        .expect("the thread must not panic");
    assert_eq!(other, 5);
}

// ---------------------------------------------------------------------------
// an initialiser that cannot be a Rust constant
// ---------------------------------------------------------------------------

/// A thread-local pointer to a file-scope object: an address constant in C,
/// but a Rust `const` may not refer to a `static`, so the item takes
/// `thread_local!`'s lazy form instead of its `const` one.
#[test]
fn an_initializer_that_names_an_item_still_works() {
    c11! {
        int anchor = 20;
        _Thread_local int *cursor = &anchor;

        int read_through(void) { return *cursor; }
        void advance(void) { cursor = 0; }
    }

    assert_eq!(unsafe { read_through() }, 20);
    unsafe { advance() };
    let other = thread::spawn(|| unsafe { read_through() })
        .join()
        .expect("the thread must not panic");
    assert_eq!(other, 20);
}
