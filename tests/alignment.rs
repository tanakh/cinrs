//! Over-aligned *objects*: `_Alignas(N)` and `__attribute__((aligned(N)))` on
//! a declaration rather than on a member.
//!
//! Rust has no way to over-align a binding, so such an object is generated
//! inside a `#[repr(C, align(N))]` wrapper and every access goes through its
//! one field — which is also how the Rust half of these tests reads one. What
//! the tests check is the only thing that matters to the C program: the
//! address really is a multiple of what the declaration asked for, whatever
//! storage duration the object has.

use cinrs::{c11, c99};

/// The alignment a pointer actually has, as C's `(uintptr_t) p % N` would
/// compute it.
fn misalignment<T>(p: *const T, align: usize) -> usize {
    (p as usize) % align
}

// ---------------------------------------------------------------------------
// the four storage durations
// ---------------------------------------------------------------------------

#[test]
fn an_automatic_object_is_aligned_where_it_asked_to_be() {
    c11! {
        #include <stdint.h>

        /* The classic: a buffer a cache line wide, on the stack. */
        int aligned_automatic(void) {
            _Alignas(64) char buf[256];
            _Alignas(32) int counters[4];
            buf[0] = 1;
            counters[0] = 2;
            return ((uintptr_t) &buf % 64 == 0) && ((uintptr_t) counters % 32 == 0)
                && buf[0] + counters[0] == 3;
        }

        /* A record, whose own alignment is raised past its members'. */
        struct Pair { int a; int b; };

        int aligned_record(void) {
            _Alignas(128) struct Pair p;
            p.a = 3;
            p.b = 4;
            return ((uintptr_t) &p % 128 == 0) && p.a + p.b == 7;
        }
    }

    assert_eq!(unsafe { aligned_automatic() }, 1);
    assert_eq!(unsafe { aligned_record() }, 1);
}

#[test]
fn a_block_scope_static_is_aligned_where_it_asked_to_be() {
    c11! {
        #include <stdint.h>

        int block_static(void) {
            static _Alignas(16) float v[4] = { 1, 2, 3, 4 };
            static _Alignas(256) char big[3];
            big[0] = 5;
            return ((uintptr_t) v % 16 == 0) && ((uintptr_t) big % 256 == 0)
                && v[3] == 4 && big[0] == 5;
        }
    }

    assert_eq!(unsafe { block_static() }, 1);
}

#[test]
fn a_file_scope_object_is_aligned_where_it_asked_to_be() {
    c11! {
        #include <stdint.h>

        _Alignas(64) char shared[100] = { 7 };
        static _Alignas(32) long private_table[8] = { 1, 2, 3 };

        int file_scope(void) {
            return ((uintptr_t) shared % 64 == 0) && ((uintptr_t) private_table % 32 == 0)
                && shared[0] == 7 && private_table[1] == 2;
        }
    }

    assert_eq!(unsafe { file_scope() }, 1);
    // A Rust caller reaches an over-aligned object through the wrapper's one
    // field, which is what the crate documentation says of it.
    unsafe {
        assert_eq!(shared.0[0], 7);
        assert_eq!(misalignment((&raw const shared.0).cast::<u8>(), 64), 0);
    }
}

#[test]
fn a_thread_local_object_is_aligned_where_it_asked_to_be() {
    c11! {
        #include <stdint.h>

        _Thread_local _Alignas(64) int per_thread[4];

        int thread_local_aligned(void) {
            per_thread[2] = 9;
            return ((uintptr_t) per_thread % 64 == 0) && per_thread[2] == 9;
        }
    }

    assert_eq!(unsafe { thread_local_aligned() }, 1);
    // Every thread's copy, not only the first one's.
    let other = std::thread::spawn(|| unsafe { thread_local_aligned() });
    assert_eq!(other.join().unwrap(), 1);
}

// ---------------------------------------------------------------------------
// the rules
// ---------------------------------------------------------------------------

#[test]
fn the_strictest_specifier_wins_and_alignof_says_so() {
    c11! {
        /* C11 6.7.5p6: when several alignment specifiers appear the strictest
           of them is the one that holds. GCC folds `aligned` in with them. */
        int strictest(void) {
            _Alignas(4) _Alignas(64) _Alignas(16) char a[3];
            return _Alignof a;
        }

        /* `_Alignas(T)` asks for the alignment of a type. */
        int alignas_a_type(void) {
            _Alignas(double) char b[3];
            return _Alignof b;
        }

        /* An alignment no stricter than the type's own leaves the object
           exactly as it was. */
        int no_stricter(void) {
            _Alignas(4) int x = 1;
            return _Alignof x + x;
        }

        _Alignas(128) int over;
        int alignof_a_file_scope_object(void) { return _Alignof over; }
    }

    unsafe {
        assert_eq!(strictest(), 64);
        assert_eq!(alignas_a_type(), align_of::<f64>() as i32);
        assert_eq!(no_stricter(), 5);
        assert_eq!(alignof_a_file_scope_object(), 128);
    }
}

#[test]
fn the_gnu_attribute_says_the_same_thing() {
    c99! {
        #include <stdint.h>

        /* `aligned` in every position GCC takes it: on the specifiers, after
           the declarator, and with no argument at all — which asks for the
           strictest alignment any type on the target needs. */
        __attribute__((aligned(64))) char before[16];
        char after[16] __attribute__((aligned(32)));

        int gnu_aligned(void) {
            char bare[3] __attribute__((aligned));
            /* Leading, at block scope: the sequence belongs to the declaration
               that follows it, exactly as one written at file scope does. */
            __attribute__((aligned(128))) char leading[3];
            /* `aligned` can only *increase* alignment, so a weaker request is
               not applied — which is GCC's rule and not a diagnostic. */
            int weak __attribute__((aligned(1))) = 6;
            leading[0] = 1;
            return ((uintptr_t) before % 64 == 0) && ((uintptr_t) after % 32 == 0)
                && ((uintptr_t) bare % 16 == 0) && ((uintptr_t) leading % 128 == 0)
                && leading[0] == 1 && weak == 6;
        }
    }

    assert_eq!(unsafe { gnu_aligned() }, 1);
}

// ---------------------------------------------------------------------------
// what an over-aligned object still is
// ---------------------------------------------------------------------------

#[test]
fn the_wrapper_is_invisible_to_the_c_program() {
    c11! {
        #include <stdint.h>
        #include <string.h>

        struct Node { int key; struct Node *next; };

        _Alignas(64) struct Node head = { 1, 0 };
        _Alignas(64) char scratch[24];

        /* `sizeof` is the *type's* size — the wrapper's padding is none of
           C's business — and the address is an ordinary one: it may be taken,
           passed, compared and written through. */
        int invisible(void) {
            struct Node *p = &head;
            unsigned long by_type = sizeof(struct Node);
            unsigned long by_object = sizeof head;
            p->key = 2;
            memset(scratch, 3, sizeof scratch);
            return by_type == by_object && sizeof scratch == 24
                && head.key == 2 && scratch[23] == 3
                && ((uintptr_t) &head % 64 == 0);
        }
    }

    assert_eq!(unsafe { invisible() }, 1);
    unsafe {
        assert_eq!(size_of::<Node>(), size_of::<*mut Node>() * 2);
        assert_eq!(misalignment(&raw const head.0, 64), 0);
    }
}

/// A `goto` puts the function into the [CFG lowering](cinrs), where every
/// local is hoisted to the top of the generated item — wrapper and all.
#[test]
fn a_hoisted_local_keeps_its_alignment() {
    c11! {
        #include <stdint.h>

        int hoisted(int n) {
            _Alignas(64) char buf[8];
            int ok = 0;
        again:
            buf[0] = (char) n;
            if ((uintptr_t) buf % 64 == 0) ok++;
            if (--n > 0) goto again;
            return ok;
        }
    }

    assert_eq!(unsafe { hoisted(3) }, 3);
}

/// A `switch` hoists the declarations of its body out of the `match` arms, so
/// they are a third place a binding is written.
#[test]
fn a_declaration_inside_a_switch_keeps_its_alignment() {
    c11! {
        #include <stdint.h>

        int in_a_switch(int which) {
            switch (which) {
            case 0: {
                _Alignas(64) char buf[8];
                buf[0] = 1;
                return ((uintptr_t) buf % 64 == 0) && buf[0] == 1;
            }
            default:
                return 0;
            }
        }
    }

    assert_eq!(unsafe { in_a_switch(0) }, 1);
}

/// `cleanup` hangs a drop guard on the object's own binding, which the wrapper
/// sits between.
#[test]
fn an_over_aligned_object_can_still_have_a_cleanup() {
    c99! {
        #include <stdint.h>

        static int freed = 0;
        static void note(int *p) { freed += *p; }

        int with_cleanup(void) {
            {
                __attribute__((cleanup(note), aligned(64))) int x = 5;
                if ((uintptr_t) &x % 64 != 0) return -1;
            }
            return freed;
        }
    }

    assert_eq!(unsafe { with_cleanup() }, 5);
}

/// A GNU nested function reaches an enclosing object through a pointer the
/// call site hands it, which has to be the address of the object and not of
/// its wrapper.
#[test]
fn a_nested_function_can_capture_an_over_aligned_object() {
    c99! {
        #include <stdint.h>

        int captured(void) {
            __attribute__((aligned(64))) int total = 0;
            void add(int n) { total += n; }
            add(3);
            add(4);
            return ((uintptr_t) &total % 64 == 0) ? total : -1;
        }
    }

    assert_eq!(unsafe { captured() }, 7);
}
