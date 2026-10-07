//! What a local declared without an initialiser starts out as.
//!
//! C leaves it indeterminate; cinrs zero-fills it by default — GCC's
//! `-ftrivial-auto-var-init=zero`, and what Rust needs anyway — and
//! `#pragma cinrs auto_var_init uninitialized` (`ccinrs
//! -ftrivial-auto-var-init=uninitialized`) leaves a local *array* uninitialised,
//! as GCC does by default: its storage is a `MaybeUninit`, reached through a raw
//! pointer, so a large buffer is not cleared on every call. These run the
//! shapes such an array is used in, under the pragma: filled by a callee and
//! read back, partly filled, declared in a loop, in a function lowered into a
//! control-flow graph, across a `longjmp`, as a variable length array, over-
//! aligned and inside a nested function — every element read was written
//! first, which is the program's side of the contract. Scalars, structures and
//! unions are still zero-filled, and the default zero-fills everything.

use cinrs::{c99, gnu99};

mod uninitialized {
    cinrs::gnu99! {
        #pragma cinrs auto_var_init uninitialized
        #include <setjmp.h>
        #include <string.h>

        struct ev { unsigned events; unsigned long long data; };

        /* libuv's `uv__io_poll`: a table the kernel fills up to `n`. */
        static int fill(struct ev *out, int n) {
            for (int i = 0; i < n; i++) {
                out[i].events = (unsigned)i * 3;
                out[i].data = (unsigned long long)i << 32;
            }
            return n;
        }
        long poll_once(int n) {
            struct ev events[1024];
            long s = 0;
            int got = fill(events, n);
            for (int i = 0; i < got; i++)
                s += events[i].events + (long)(events[i].data >> 32);
            return s;
        }

        /* curl's `formatf`: a work buffer written from the end. */
        int digits(long x, char *out) {
            char work[64];
            char *w = work + sizeof work;
            *--w = 0;
            do { *--w = (char)('0' + x % 10); x /= 10; } while (x);
            strcpy(out, w);
            return (int)(work + sizeof work - w) - 1;
        }

        /* Declared in a loop: a fresh, uninitialised array every pass. */
        int per_iteration(int rounds) {
            int total = 0;
            for (int r = 0; r < rounds; r++) {
                int squares[16];
                for (int i = 0; i <= r && i < 16; i++) squares[i] = i * i;
                total += squares[r < 16 ? r : 15];
            }
            return total;
        }

        /* A `goto` puts the function into a control-flow graph, where every
           local is bound at the top. */
        int with_goto(int n) {
            int seen[8];
            int i = 0;
        again:
            seen[i] = i + n;
            if (++i < 8) goto again;
            return seen[0] + seen[7];
        }

        /* `setjmp` runs the body inside `catch_unwind`, with the locals
           outside it. */
        static jmp_buf env;
        static void bail(void) { longjmp(env, 1); }
        int across_longjmp(void) {
            volatile int marks[4];
            marks[0] = 5;
            if (setjmp(env) == 0) {
                marks[1] = 7;
                bail();
            }
            return marks[0] * 10 + marks[1];
        }

        /* A variable length array, from the function's arena, which does not
           clear under the pragma either. */
        int vla_sum(int n) {
            int v[n];
            for (int i = 0; i < n; i++) v[i] = i + 1;
            int s = 0;
            for (int i = 0; i < n; i++) s += v[i];
            return s;
        }

        /* Over-aligned: the alignment wrapper inside the `MaybeUninit`. */
        int aligned_buffer(void) {
            _Alignas(64) unsigned char buf[100];
            memset(buf, 7, sizeof buf);
            return ((unsigned long)buf % 64 == 0) * 1000 + buf[99];
        }

        /* A nested function that writes the enclosing function's array. */
        int nested_fill(void) {
            int cells[3];
            void put(int i, int v) { cells[i] = v; }
            put(0, 1); put(1, 2); put(2, 3);
            return cells[0] * 100 + cells[1] * 10 + cells[2];
        }

        /* A structure is still zero-filled: a member nothing wrote reads 0,
           and the whole structure may be copied. */
        struct pair { int a, b; };
        int struct_still_zero(void) {
            struct pair p;
            p.a = 4;
            struct pair q = p;
            return q.a * 10 + q.b;
        }
    }

    #[test]
    fn an_uninitialised_array_holds_what_the_program_wrote() {
        unsafe {
            assert_eq!(poll_once(0), 0);
            // The events are 0, 3 and 6, and the data 0, 1 and 2 above bit 32.
            assert_eq!(poll_once(3), 9 + 3);
            let mut out = [0 as core::ffi::c_char; 32];
            assert_eq!(digits(90_125, out.as_mut_ptr()), 5);
            assert_eq!(core::ffi::CStr::from_ptr(out.as_ptr()).to_bytes(), b"90125");
            assert_eq!(per_iteration(5), (0..5).map(|r| r * r).sum::<i32>());
            assert_eq!(with_goto(10), 10 + 17);
            assert_eq!(across_longjmp(), 57);
            assert_eq!(vla_sum(100), 5050);
            assert_eq!(aligned_buffer(), 1007);
            assert_eq!(nested_fill(), 123);
            assert_eq!(struct_still_zero(), 40);
        }
    }
}

/// Without the pragma — every block, and `ccinrs` unless told otherwise — an
/// array nothing wrote reads as zeros, in both lowerings.
#[test]
fn by_default_every_local_is_zero() {
    c99! {
        int unwritten(int i) {
            int a[64];
            a[0] = 1;
            return a[i];
        }
        int unwritten_with_goto(int i) {
            int a[64];
            int k = 0;
        again:
            if (++k < 3) goto again;
            return a[i] + k;
        }
    }
    gnu99! {
        int unwritten_vla(int n, int i) {
            long v[n];
            v[0] = 9;
            return (int)v[i];
        }
    }
    unsafe {
        assert_eq!(unwritten(0), 1);
        assert_eq!(unwritten(63), 0);
        assert_eq!(unwritten_with_goto(40), 3);
        assert_eq!(unwritten_vla(32, 0), 9);
        assert_eq!(unwritten_vla(32, 31), 0);
    }
}
