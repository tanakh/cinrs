//! Integration tests that *run* translated C using `setjmp` and `longjmp`.
//!
//! A `longjmp` is a Rust unwind here, and the function that called `setjmp`
//! catches it and re-enters its body at the code after the `setjmp`; see
//! `cinrs_core::sema`'s non-local jumps. These are the shapes C allows a
//! `setjmp` in, the jumps that cross other frames — translated ones and the C
//! library's `qsort` — and what the locals hold afterwards.

use cinrs::{c99, gnu99};

#[test]
fn if_setjmp_then_longjmp() {
    c99! {
        #include <setjmp.h>

        static jmp_buf env;

        static void fail(int code) { longjmp(env, code); }

        int attempt(int code) {
            if (setjmp(env) == 0) {
                fail(code);
                return -1;
            } else {
                return 100;
            }
        }

        /* The value comes back through the comparison it was written in. */
        int value_of(int code) {
            int r = setjmp(env);
            if (r == 0)
                fail(code);
            return r;
        }

        /* `longjmp(env, 0)` delivers 1, as C says. */
        int zero_is_one(void) {
            int r;
            r = setjmp(env);
            if (r == 0)
                longjmp(env, 0);
            return r;
        }
    }

    unsafe {
        assert_eq!(attempt(3), 100);
        assert_eq!(value_of(3), 3);
        assert_eq!(value_of(-7), -7);
        assert_eq!(zero_is_one(), 1);
    }
}

#[test]
fn every_place_c_allows_a_setjmp() {
    gnu99! {
        #include <setjmp.h>

        static jmp_buf env;

        int places(void) {
            int log = 0;
            /* The whole controlling expression. */
            if (setjmp(env)) log += 1; else longjmp(env, 1);
            /* Negated. */
            if (!setjmp(env)) longjmp(env, 2); else log += 10;
            /* Compared with a constant, on either side. */
            if (2 < setjmp(env)) log += 100; else longjmp(env, 3);
            /* A switch. */
            switch (setjmp(env)) {
            case 0: longjmp(env, 5);
            case 5: log += 1000; break;
            default: log = -1;
            }
            /* An iteration statement, called again on every pass. */
            int passes = 0;
            while (setjmp(env) < 3) {
                passes++;
                longjmp(env, passes);
            }
            log += passes * 10000;
            /* An expression statement, cast to void and not. */
            int again = 0;
            setjmp(env);
            if (again++ < 2) longjmp(env, 1);
            (void) setjmp(env);
            if (again++ < 5) longjmp(env, 1);
            log += again * 100000;
            /* Beyond C17, as GCC takes it: assigned inside the condition. */
            int rc;
            if ((rc = setjmp(env)) == 0) longjmp(env, 4);
            log += rc * 1000000;
            return log;
        }
    }

    unsafe {
        assert_eq!(
            places(),
            1 + 10 + 100 + 1000 + 3 * 10000 + 6 * 100000 + 4 * 1000000
        );
    }
}

#[test]
fn nested_setjmps_in_one_function() {
    c99! {
        #include <setjmp.h>

        static jmp_buf outer, inner;
        static int trail;

        static void deep(int which) {
            trail = trail * 10 + which;
            if (which == 1) longjmp(inner, 1);
            longjmp(outer, 2);
        }

        int nested(void) {
            trail = 0;
            if (setjmp(outer) == 0) {
                if (setjmp(inner) == 0) {
                    deep(1);
                } else {
                    trail = trail * 10 + 7;
                    deep(2);
                }
                trail = -1;
            } else {
                trail = trail * 10 + 9;
            }
            return trail;
        }

        /* A setjmp in a loop: the continuation is a second way into the loop. */
        int in_a_loop(int n) {
            int total = 0;
            for (int i = 0; i < n; i++) {
                int r = setjmp(inner);
                total += r;
                if (r == 0) longjmp(inner, i + 1);
            }
            return total;
        }
    }

    unsafe {
        assert_eq!(nested(), 1729);
        assert_eq!(in_a_loop(4), 1 + 2 + 3 + 4);
        assert_eq!(in_a_loop(0), 0);
    }
}

#[test]
fn a_longjmp_crosses_translated_frames_and_qsort() {
    c99! {
        #include <setjmp.h>
        #include <stdlib.h>

        static jmp_buf env;
        static int compares;

        static int by_value(const void *a, const void *b) {
            int x = *(const int *)a, y = *(const int *)b;
            if (++compares == 20) longjmp(env, 42);
            return (x > y) - (x < y);
        }

        /* glibc's qsort is compiled with unwind tables, so the unwind passes
           through it. */
        int out_of_qsort(void) {
            int v[64];
            for (int i = 0; i < 64; i++) v[i] = (i * 37) % 64;
            compares = 0;
            if (setjmp(env) == 0) {
                qsort(v, 64, sizeof v[0], by_value);
                return -1;
            }
            return compares;
        }

        static int recurse(int n, int (*step)(int)) {
            if (n == 0) return step(n);
            return recurse(n - 1, step) + 1;
        }

        static int jump(int n) { longjmp(env, n + 5); }

        /* Through a function pointer and a dozen frames. */
        int through_frames(void) {
            int r = setjmp(env);
            if (r == 0) {
                recurse(12, jump);
                return -1;
            }
            return r;
        }

        /* A frame in between that called setjmp itself: the unwind is not
           for it, and goes on. */
        static jmp_buf other;
        static int middle(void) {
            if (setjmp(other) == 0) {
                recurse(3, jump);
                return -1;
            }
            return -2;
        }

        int past_another_setjmp(void) {
            if (setjmp(env) == 0)
                return middle();
            return 1;
        }
    }

    unsafe {
        assert_eq!(out_of_qsort(), 20);
        assert_eq!(through_frames(), 5);
        assert_eq!(past_another_setjmp(), 1);
    }
}

#[test]
fn locals_keep_their_latest_values() {
    c99! {
        #include <setjmp.h>

        static jmp_buf env;

        static void bail(void) { longjmp(env, 1); }

        /* C only promises the `volatile` one; here both have the value they
           had when the jump left. */
        int locals(void) {
            volatile int v = 1;
            int plain = 1;
            int array[2] = { 1, 1 };
            if (setjmp(env) == 0) {
                v = 2;
                plain = 3;
                array[1] = 4;
                bail();
            }
            return v * 100 + plain * 10 + array[1];
        }

        /* A parameter is a local too. */
        int parameter(int n) {
            if (setjmp(env) == 0) {
                n = n * 2;
                bail();
            }
            return n;
        }
    }

    unsafe {
        assert_eq!(locals(), 234);
        assert_eq!(parameter(21), 42);
    }
}

/// A `cleanup` attribute's function runs when its scope is left the ordinary
/// ways, and not when a `longjmp` leaves it — GCC's rule, which registers a
/// cleanup with the unwinder only under `-fexceptions`.
#[test]
fn a_longjmp_does_not_run_cleanups() {
    gnu99! {
        #include <setjmp.h>

        static jmp_buf env;
        static int cleaned;

        static void count(int *p) { (void) p; cleaned++; }

        /* No setjmp of its own, so the guard is a drop guard. */
        static void guarded(int jump) {
            __attribute__((cleanup(count))) int x = 1;
            (void) x;
            if (jump) longjmp(env, 1);
        }

        int cleanups(int jump) {
            cleaned = 0;
            if (setjmp(env) == 0)
                guarded(jump);
            return cleaned;
        }
    }

    unsafe {
        assert_eq!(cleanups(0), 1);
        assert_eq!(cleanups(1), 0);
    }
}

#[test]
fn gcc_builtins() {
    gnu99! {
        static void *buf[5];

        static void thrower(void) { __builtin_longjmp(buf, 1); }

        int builtin(void) {
            int hits = 0;
            if (__builtin_setjmp(buf) == 0) {
                hits = 1;
                thrower();
                hits = -1;
            } else {
                hits += 10;
            }
            return hits;
        }
    }

    unsafe {
        assert_eq!(builtin(), 11);
    }
}

/// A `longjmp` in one unit to a `setjmp` in another: each unit keeps its own
/// registry of live frames, and the buffer names the one to ask.
mod thrower {
    cinrs::c99! {
        #pragma cinrs export
        #include <setjmp.h>

        void cinrs_setjmp_test_throw(jmp_buf *env, int value) {
            longjmp(*env, value);
        }
    }
}

mod catcher {
    cinrs::c99! {
        #include <setjmp.h>

        void cinrs_setjmp_test_throw(jmp_buf *env, int value);

        int catch_from_another_unit(int value) {
            jmp_buf env;
            int r = setjmp(env);
            if (r == 0) {
                cinrs_setjmp_test_throw(&env, value);
                return -1;
            }
            return r;
        }
    }
}

#[test]
fn a_longjmp_from_another_unit() {
    unsafe {
        assert_eq!(catcher::catch_from_another_unit(9), 9);
        assert_eq!(catcher::catch_from_another_unit(-3), -3);
    }
}

/// A unit with no `setjmp` or `longjmp` of its own that a `longjmp` passes
/// through — a callback's caller — asks for the `C-unwind` ABI with the
/// pragma.
mod walker {
    cinrs::c99! {
        #pragma cinrs unwind
        #pragma cinrs export

        int cinrs_setjmp_test_walk(const int *v, int n, int (*visit)(int)) {
            int total = 0;
            for (int i = 0; i < n; i++)
                total += visit(v[i]);
            return total;
        }
    }
}

mod visitor {
    cinrs::c99! {
        #include <setjmp.h>

        int cinrs_setjmp_test_walk(const int *v, int n, int (*visit)(int));

        static jmp_buf env;

        static int visit(int x) {
            if (x < 0) longjmp(env, x);
            return x;
        }

        int walk_until_negative(void) {
            static const int v[] = { 1, 2, 3, -4, 5 };
            int r = setjmp(env);
            if (r == 0)
                return cinrs_setjmp_test_walk(v, 5, visit);
            return r * 100;
        }
    }
}

#[test]
fn a_longjmp_through_a_unit_that_asked_for_c_unwind() {
    unsafe {
        assert_eq!(visitor::walk_until_negative(), -400);
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn sigsetjmp_saves_and_restores_the_signal_mask() {
    gnu99! {
        /* glibc's own <setjmp.h> and <signal.h>: `sigset_t` is POSIX's. */
        #pragma cinrs system_include first
        #include <setjmp.h>
        #include <signal.h>

        static sigjmp_buf env;

        static int blocked(void) {
            sigset_t now;
            sigprocmask(SIG_SETMASK, NULL, &now);
            return sigismember(&now, SIGUSR1);
        }

        /* Returns what the mask said before the jump and after it, as two
           digits. */
        int mask_after_jump(int save) {
            sigset_t set;
            int before = -1;
            sigemptyset(&set);
            sigaddset(&set, SIGUSR1);
            sigprocmask(SIG_UNBLOCK, &set, NULL);
            if (sigsetjmp(env, save) == 0) {
                sigprocmask(SIG_BLOCK, &set, NULL);
                before = blocked();
                siglongjmp(env, 1);
            }
            int after = blocked();
            sigprocmask(SIG_UNBLOCK, &set, NULL);
            return before * 10 + after;
        }
    }

    unsafe {
        // Saved: the jump puts SIGUSR1 back to unblocked.
        assert_eq!(mask_after_jump(1), 10);
        // Not saved: it stays blocked.
        assert_eq!(mask_after_jump(0), 11);
    }
}
