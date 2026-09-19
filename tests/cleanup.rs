//! `__attribute__((cleanup(f)))` (GCC's extension).
//!
//! `T x __attribute__((cleanup(f)));` calls `f(&x)` when `x` goes out of
//! scope, in reverse declaration order, on *every* exit from that scope —
//! falling off the end, `break`, `continue`, `return`, or a `goto` that leaves
//! several scopes at once. It is what systemd's `_cleanup_free_` and glib's
//! `g_autofree` are made of.
//!
//! The two lowerings say it differently and have to agree. In the structured
//! one a drop guard is bound right after the object, so Rust's own drop order
//! *is* C's — and a `goto` that leaves a scope is a `break` out of the Rust
//! block it is, which drops what that block holds; in the [CFG] one the locals
//! live to the end of the function, so the calls are emitted on the edges that
//! leave the scope. Every test here runs in both, which is what the second
//! half of each pair is for: an ordinary `goto` outwards keeps the structured
//! form now, so the twin uses a computed one, which nothing but the graph can
//! express.
//!
//! [CFG]: https://docs.rs/cinrs

use cinrs::gnu99;

// ---------------------------------------------------------------------------
// order
// ---------------------------------------------------------------------------

#[test]
fn guards_run_in_reverse_declaration_order() {
    gnu99! {
        #include <stdio.h>

        static int trace[16];
        static int traced;

        static void note(int *p) { trace[traced++] = *p; }

        /* Declared 1, 2, 3; run 3, 2, 1. */
        int structured(void) {
            traced = 0;
            {
                int a __attribute__((cleanup(note))) = 1;
                int b __attribute__((cleanup(note))) = 2;
                int c __attribute__((cleanup(note))) = 3;
                (void)a; (void)b; (void)c;
            }
            return trace[0] * 100 + trace[1] * 10 + trace[2] + traced * 1000;
        }

        /* The same, in a function the computed `goto` puts through the
           control-flow graph. */
        int with_goto(void) {
            void *entry = &&start;
            traced = 0;
            goto *entry;
        start:
            {
                int a __attribute__((cleanup(note))) = 1;
                int b __attribute__((cleanup(note))) = 2;
                int c __attribute__((cleanup(note))) = 3;
                (void)a; (void)b; (void)c;
            }
            return trace[0] * 100 + trace[1] * 10 + trace[2] + traced * 1000;
        }

        /* The value the function sees is the one at the moment of the call:
           the cleanup reads the object, not a copy taken at the declaration. */
        int reads_the_final_value(void) {
            traced = 0;
            {
                int a __attribute__((cleanup(note))) = 1;
                a = 41;
                a++;
            }
            return trace[0];
        }
    }

    unsafe {
        assert_eq!(structured(), 3000 + 321);
        assert_eq!(with_goto(), 3000 + 321);
        assert_eq!(reads_the_final_value(), 42);
    }
}

// ---------------------------------------------------------------------------
// leaving the scope
// ---------------------------------------------------------------------------

#[test]
fn every_exit_from_the_scope_runs_the_cleanup() {
    gnu99! {
        static int runs;
        static int last;

        static void count(int *p) { runs++; last = *p; }

        /* `return` from inside the scope. */
        int early_return(int c) {
            runs = 0;
            {
                int a __attribute__((cleanup(count))) = 7;
                if (c) {
                    return runs * 100 + a;
                }
            }
            return runs * 100 + 1;
        }

        /* GCC computes the returned value *before* the cleanups run, so a
           cleanup that changes what the expression read cannot be seen in it. */
        static void zero(int *p) { *p = 0; }

        int value_first(void) {
            int a __attribute__((cleanup(zero))) = 5;
            return a * 10;
        }

        /* The same where the cleanups are *statements* in front of the
           `return` rather than drops after it: the value goes into a
           temporary, so the call cannot change it either. */
        int value_first_with_goto(int c) {
            void *entry = &&here;
            if (c) {
                goto *entry;
            }
        here:
            {
                int a __attribute__((cleanup(zero))) = 5;
                return a * 10 + c;
            }
        }

        /* `break` out of a loop, and `continue` around it: the object is
           declared in the body, so the cleanup runs once per iteration. */
        int per_iteration(int n) {
            runs = 0;
            for (int i = 0; i < n; i++) {
                int a __attribute__((cleanup(count))) = i;
                if (i == 3) {
                    break;
                }
                if (i % 2 == 0) {
                    continue;
                }
            }
            return runs * 100 + last;
        }

        /* The same loop in a function the computed `goto` puts through the
           control-flow graph, where the object is hoisted and the call is
           emitted on each edge that leaves the body: once per iteration, on
           the bottom edge, on the `continue` and on the `break` alike. */
        int per_iteration_with_goto(int n) {
            void *entry = &&start;
            runs = 0;
            int i = 0;
            goto *entry;
        start:
            for (; i < n; i++) {
                int a __attribute__((cleanup(count))) = i;
                if (i == 3) {
                    break;
                }
                if (i % 2 == 0) {
                    continue;
                }
            }
            return runs * 100 + last;
        }

        /* A `while` whose body leaves through the bottom, a `continue` and a
           `break` in turn. */
        int while_loop(void) {
            runs = 0;
            int i = 0;
            while (i < 4) {
                int a __attribute__((cleanup(count))) = i;
                i++;
                if (a == 1) {
                    continue;
                }
                if (a == 2) {
                    break;
                }
            }
            return runs * 100 + last;
        }
    }

    unsafe {
        // The cleanup has not run yet where the `return` expression is
        // evaluated, so `runs` is 0 there and `a` is 7.
        assert_eq!(early_return(1), 7);
        assert_eq!(early_return(0), 100 + 1);
        assert_eq!(value_first(), 50);
        assert_eq!(value_first_with_goto(1), 51);
        assert_eq!(value_first_with_goto(0), 50);
        // i = 0, 1, 2, 3 with the `break` on the last: four cleanups, last 3.
        assert_eq!(per_iteration(10), 4 * 100 + 3);
        assert_eq!(per_iteration_with_goto(10), 4 * 100 + 3);
        assert_eq!(per_iteration_with_goto(2), 2 * 100 + 1);
        // i = 0, 1, 2 with the `break` on the last: three cleanups, last 2.
        assert_eq!(while_loop(), 3 * 100 + 2);
    }
}

#[test]
fn a_goto_leaving_two_scopes_runs_both_cleanups() {
    gnu99! {
        static int trace[16];
        static int traced;

        static void note(int *p) { trace[traced++] = *p; }

        /* The jump leaves two scopes at once, innermost first — and the label
           is outside both. It is an outward `goto`, so this one keeps the
           structured form: the `break` out of the two Rust blocks drops the
           guards they hold, in Rust's own order. */
        int out_of_two(void) {
            traced = 0;
            {
                int outer __attribute__((cleanup(note))) = 1;
                {
                    int inner __attribute__((cleanup(note))) = 2;
                    if (outer + inner == 3) {
                        goto done;
                    }
                    trace[traced++] = 99;
                }
            }
        done:
            return traced * 100 + trace[0] * 10 + trace[1];
        }

        /* Leaving one scope and staying in the other: only the inner cleanup
           runs at the jump, and the outer one where its own scope ends. */
        int out_of_one(void) {
            traced = 0;
            {
                int outer __attribute__((cleanup(note))) = 1;
                {
                    int inner __attribute__((cleanup(note))) = 2;
                    (void)inner;
                    goto still_inside;
                }
            still_inside:
                trace[traced++] = 3;
            }
            return traced * 1000 + trace[0] * 100 + trace[1] * 10 + trace[2];
        }

        /* A backward `goto` runs the cleanups of the scopes it leaves on the
           way, once per pass. */
        int backwards(void) {
            traced = 0;
            int i = 0;
        again:
            {
                int a __attribute__((cleanup(note))) = i;
                if (++i < 3) {
                    goto again;
                }
            }
            return traced * 100 + trace[0] * 10 + trace[2];
        }

        /* Both of those in the graph, where the calls are statements on the
           edge the jump takes rather than drops at the end of a block. */
        int out_of_two_in_the_graph(void) {
            void *target = &&done;
            traced = 0;
            {
                int outer __attribute__((cleanup(note))) = 1;
                {
                    int inner __attribute__((cleanup(note))) = 2;
                    if (outer + inner == 3) {
                        goto *target;
                    }
                    trace[traced++] = 99;
                }
            }
        done:
            return traced * 100 + trace[0] * 10 + trace[1];
        }

        int backwards_in_the_graph(void) {
            void *target = &&again;
            traced = 0;
            int i = 0;
        again:
            {
                int a __attribute__((cleanup(note))) = i;
                if (++i < 3) {
                    goto *target;
                }
            }
            return traced * 100 + trace[0] * 10 + trace[2];
        }
    }

    unsafe {
        // Inner (2) then outer (1).
        assert_eq!(out_of_two(), 2 * 100 + 2 * 10 + 1);
        assert_eq!(out_of_two_in_the_graph(), 2 * 100 + 2 * 10 + 1);
        // Inner (2), the marker (3), then the outer (1).
        assert_eq!(out_of_one(), 3 * 1000 + 2 * 100 + 3 * 10 + 1);
        // Three passes, values 0, 1, 2.
        assert_eq!(backwards(), 3 * 100 + 2);
        assert_eq!(backwards_in_the_graph(), 3 * 100 + 2);
    }
}

#[test]
fn a_switch_group_is_a_scope_like_any_other() {
    gnu99! {
        static int runs;
        static int last;

        static void count(int *p) { runs++; last = *p; }

        /* What the cleanups have done, asked after the call rather than in
           the `return` expression, which C evaluates before they run. */
        int after(void) { return runs * 1000 + last; }

        /* The object is declared in a block of its own inside the group, so
           the cleanup runs when the group's `break` leaves it. */
        int in_a_group(int c) {
            runs = 0;
            last = 0;
            switch (c) {
                case 1: {
                    int a __attribute__((cleanup(count))) = 11;
                    (void)a;
                    break;
                }
                case 2: {
                    int a __attribute__((cleanup(count))) = 22;
                    if (a > 0) {
                        return runs * 1000 + last;
                    }
                    break;
                }
                default:
                    break;
            }
            return runs * 1000 + last;
        }

        /* A declaration directly in the body of a `switch` is hoisted ahead of
           the dispatch by the structured lowering, so a `cleanup` on one puts
           the function through the control-flow graph instead — where the
           scope really does end with the body. */
        int directly_in_the_body(int c) {
            runs = 0;
            last = 0;
            switch (c) {
                int a __attribute__((cleanup(count)));
                case 1:
                    a = 1;
                    break;
                case 2:
                    a = 2;
                    break;
                default:
                    a = 0;
                    break;
            }
            return runs * 1000 + last;
        }
    }

    unsafe {
        assert_eq!(in_a_group(1), 1000 + 11);
        // The `return` inside the group computes its value before the cleanup
        // runs, so it sees nothing — and the cleanup has run by the time the
        // caller can ask.
        assert_eq!(in_a_group(2), 0);
        assert_eq!(after(), 1000 + 22);
        assert_eq!(in_a_group(3), 0);
        assert_eq!(directly_in_the_body(1), 1000 + 1);
        assert_eq!(directly_in_the_body(2), 1000 + 2);
    }
}

// ---------------------------------------------------------------------------
// the functions a cleanup names
// ---------------------------------------------------------------------------

#[test]
fn the_cleanup_free_idiom_works() {
    gnu99! {
        #include <stdlib.h>
        #include <string.h>

        /* systemd's `_cleanup_free_`: the function takes a `void *` — which is
           what `&p`, a `char **`, converts to — and frees what it points at. */
        static void freep(void *p) { free(*(void **)p); }

        int copy_and_free(const char *text) {
            char *p __attribute__((cleanup(freep))) = malloc(strlen(text) + 1);
            if (p == 0) {
                return -1;
            }
            strcpy(p, text);
            return (int)strlen(p);
        }

        /* The same with a typed wrapper, and a second allocation freed on the
           way out of an inner scope. */
        static void free_chars(char **p) { free(*p); *p = 0; }

        int two_allocations(void) {
            int total = 0;
            char *a __attribute__((cleanup(free_chars))) = malloc(4);
            if (a == 0) {
                return -1;
            }
            strcpy(a, "abc");
            {
                char *b __attribute__((cleanup(free_chars))) = malloc(3);
                if (b == 0) {
                    return -1;
                }
                strcpy(b, "de");
                total += (int)strlen(b);
            }
            return total + (int)strlen(a);
        }
    }

    unsafe {
        assert_eq!(copy_and_free(c"hello".as_ptr()), 5);
        assert_eq!(two_allocations(), 5);
    }
}

#[test]
fn a_cleanup_may_take_a_pointer_to_a_qualified_type_or_a_struct() {
    gnu99! {
        struct Buf { int len; int data[4]; };

        static int total;

        /* A `const T *` parameter is a pointer to a type compatible with the
           variable's, which is all GCC asks for. */
        static void add(const int *p) { total += *p; }
        static void sum_buf(struct Buf *b) { for (int i = 0; i < b->len; i++) total += b->data[i]; }

        int mixed(void) {
            total = 0;
            {
                int a __attribute__((cleanup(add))) = 4;
                struct Buf b __attribute__((cleanup(sum_buf))) = { 3, { 1, 2, 3, 0 } };
                (void)a; (void)b;
            }
            return total;
        }

        /* An array object: the function is handed the address of its first
           element, which is what `&a` is. */
        static void first(int (*a)[3]) { total += (*a)[0]; }

        int of_an_array(void) {
            total = 0;
            {
                int a[3] __attribute__((cleanup(first))) = { 9, 8, 7 };
                (void)a;
            }
            return total;
        }
    }

    unsafe {
        // The struct's cleanup runs first (declared last): 1 + 2 + 3, then 4.
        assert_eq!(mixed(), 10);
        assert_eq!(of_an_array(), 9);
    }
}

// ---------------------------------------------------------------------------
// two units, each in a module of its own
// ---------------------------------------------------------------------------

mod two_units {
    mod first {
        cinrs::gnu99! {
            static int runs;
            static void count(int *p) { runs += *p; }
            int one(void) {
                runs = 0;
                {
                    int a __attribute__((cleanup(count))) = 1;
                    (void)a;
                }
                return runs;
            }
        }
    }

    mod second {
        cinrs::gnu99! {
            static int runs;
            static void count(int *p) { runs += *p * 2; }
            int two(void) {
                runs = 0;
                {
                    int a __attribute__((cleanup(count))) = 1;
                    (void)a;
                }
                return runs;
            }
        }
    }

    /// Each unit is a module of its own, so each may generate a guard type of
    /// its own without the two colliding.
    #[test]
    fn the_guard_type_does_not_collide() {
        unsafe {
            assert_eq!(first::one(), 1);
            assert_eq!(second::two(), 2);
        }
    }
}

// ---------------------------------------------------------------------------
// interaction with the rest of the language
// ---------------------------------------------------------------------------

#[test]
fn a_cleanup_lives_beside_the_other_scope_machinery() {
    gnu99! {
        static int trace[16];
        static int traced;

        static void note(int *p) { trace[traced++] = *p; }
        static void note_ptr(int **p) { trace[traced++] = **p; }

        /* A variable length array in the same scope: the cleanup takes the
           address of the pointer the object is, and the storage outlives the
           call, exactly as C's does. */
        int beside_a_vla(int n) {
            traced = 0;
            {
                int a[n];
                a[0] = 5;
                int *p __attribute__((cleanup(note_ptr))) = a;
                (void)p;
            }
            return traced * 10 + trace[0];
        }

        /* A statement expression is a block, and a `cleanup` in one runs when
           the block ends — before the value it produced is used. */
        int in_a_statement_expression(void) {
            traced = 0;
            int v = ({ int a __attribute__((cleanup(note))) = 3; a * 2; });
            return traced * 100 + trace[0] * 10 + v;
        }

        /* Nested functions of the scope: an inner block's cleanup runs on the
           way out of it, and the outer one afterwards. */
        int nesting(void) {
            traced = 0;
            {
                int a __attribute__((cleanup(note))) = 1;
                for (int i = 0; i < 2; i++) {
                    int b __attribute__((cleanup(note))) = 10 + i;
                    (void)b;
                }
                (void)a;
            }
            return traced * 1000 + trace[0] * 100 + trace[1] + trace[2];
        }
    }

    unsafe {
        assert_eq!(beside_a_vla(4), 10 + 5);
        assert_eq!(in_a_statement_expression(), 100 + 30 + 6);
        // 10, 11, then 1.
        assert_eq!(nesting(), 3000 + 10 * 100 + 11 + 1);
    }
}
