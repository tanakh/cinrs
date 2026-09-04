//! Compound literals — `(type-name){ initializer-list }`, C99 6.5.2.5.
//!
//! A compound literal is an *object*, not a value, and that is what every test
//! here is really about. At block scope it has automatic storage duration and
//! the lifetime of the enclosing block, so `&(struct S){1, 2}` is still a good
//! pointer at the end of that block — a temporary inside the expression could
//! not offer that, which is why the translation gives each literal a hidden
//! local of its own and stores into it where the literal was written. Storing
//! it *there* is what keeps C's evaluation order and what makes a literal
//! inside a loop a fresh object on every iteration. At file scope the object
//! has static storage duration instead, and its initialiser has to be a
//! constant expression like any other.

use cinrs::c99;

// ---------------------------------------------------------------------------
// lifetime
// ---------------------------------------------------------------------------

#[test]
fn the_address_of_a_literal_outlives_the_statement() {
    c99! {
        struct S { int a; int b; };

        int through_a_pointer(int a, int b) {
            struct S *p = &(struct S){ a, b };
            /* The object lives as long as this block does, so `p` is still
               usable — and writable — well after the statement that made it. */
            int total = p->a + p->b;
            p->a = 10;
            return total * 100 + p->a + p->b;
        }
    }

    unsafe {
        assert_eq!(through_a_pointer(1, 2), 312);
    }
}

#[test]
fn a_literal_in_a_loop_is_rebuilt_on_every_iteration() {
    c99! {
        int sum_of_triples(int n) {
            int total = 0;
            for (int i = 0; i < n; i++) {
                int *p = (int[]){ i, i * 2, i * 3 };
                total += p[0] + p[1] + p[2];
            }
            return total;
        }
    }

    unsafe {
        // 6 * (0 + 1 + 2)
        assert_eq!(sum_of_triples(3), 18);
        assert_eq!(sum_of_triples(0), 0);
    }
}

#[test]
fn the_initializer_runs_where_the_literal_was_written() {
    c99! {
        int order(void) {
            int n = 1;
            /* `n` is read by the literal *after* the assignment in front of
               it, which only holds if the initialiser is evaluated here
               rather than at the top of the block. */
            int *p = (n = 4, &(int){ n * 2 });
            return *p;
        }
    }

    unsafe {
        assert_eq!(order(), 8);
    }
}

// ---------------------------------------------------------------------------
// passing them around
// ---------------------------------------------------------------------------

#[test]
fn a_struct_literal_passes_by_value_and_by_pointer() {
    c99! {
        struct Point { int x; int y; };

        int manhattan(struct Point p) {
            return (p.x < 0 ? -p.x : p.x) + (p.y < 0 ? -p.y : p.y);
        }

        int through_pointer(const struct Point *p) {
            return p->x * 10 + p->y;
        }

        int both(void) {
            return manhattan((struct Point){ -3, 4 }) * 1000
                 + through_pointer(&(struct Point){ 5, 6 });
        }
    }

    unsafe {
        assert_eq!(both(), 7 * 1000 + 56);
    }
}

#[test]
fn an_array_literal_infers_its_length_subscripts_and_decays() {
    c99! {
        int sum(const int *values, int n) {
            int total = 0;
            for (int i = 0; i < n; i++) {
                total += values[i];
            }
            return total;
        }

        int indexed(int i) { return (int[]){ 10, 20, 30 }[i]; }
        int passed(void) { return sum((int[]){ 1, 2, 3, 4 }, 4); }
        unsigned long array_size(void) { return sizeof((int[]){ 1, 2, 3 }); }
        unsigned long sized_size(void) { return sizeof((int[5]){ 1 }); }
    }

    unsafe {
        assert_eq!(indexed(0), 10);
        assert_eq!(indexed(2), 30);
        assert_eq!(passed(), 10);
        assert_eq!(array_size(), 12);
        assert_eq!(sized_size(), 20);
    }
}

// ---------------------------------------------------------------------------
// the initialiser forms a declaration has
// ---------------------------------------------------------------------------

#[test]
fn designators_strings_unions_and_nesting() {
    c99! {
        struct Pair { int a; int b; };
        struct Holder { int *p; };
        union U { int i; char bytes[4]; };

        int designated(void) {
            struct Pair *p = &(struct Pair){ .b = 2, .a = 1 };
            return p->a * 10 + p->b;
        }

        int from_a_string(int i) { return (char[]){ "abc" }[i]; }

        int sized_string(void) {
            char *s = (char[4]){ "ab" };
            return s[0] * 100 + s[1] + s[3];
        }

        int a_union(void) {
            union U u = (union U){ .bytes = { 1, 2, 3, 4 } };
            return u.bytes[2];
        }

        int nested(void) {
            struct Holder h = { .p = &(int){ 5 } };
            return *h.p;
        }

        int a_const_literal(void) {
            const int *p = &(const int){ 7 };
            return *p;
        }

        int partly_initialized(void) {
            struct Pair *p = &(struct Pair){ 9 };
            return p->a * 10 + p->b;
        }
    }

    unsafe {
        assert_eq!(designated(), 12);
        assert_eq!(from_a_string(0), 97);
        assert_eq!(from_a_string(3), 0);
        assert_eq!(sized_string(), 97 * 100 + 98);
        assert_eq!(a_union(), 3);
        assert_eq!(nested(), 5);
        assert_eq!(a_const_literal(), 7);
        // Whatever the initialiser leaves out is zero, as in a declaration.
        assert_eq!(partly_initialized(), 90);
    }
}

#[test]
fn a_literal_is_an_lvalue_and_can_be_assigned_to() {
    c99! {
        struct S { int a; int b; };

        int through_a_pointer(void) {
            struct S *p = &(struct S){ 1, 2 };
            (*p).a = 5;
            return p->a * 10 + p->b;
        }

        int directly(void) {
            /* Legal, if pointless: the object is thrown away straight after. */
            return (struct S){ 1, 2 }.a = 5;
        }
    }

    unsafe {
        assert_eq!(through_a_pointer(), 52);
        assert_eq!(directly(), 5);
    }
}

// ---------------------------------------------------------------------------
// where they can appear
// ---------------------------------------------------------------------------

#[test]
fn file_scope_literals_have_static_storage() {
    c99! {
        struct S { int a; int b; };
        struct Holder { int *p; };

        struct S *gs = &(struct S){ 1, 2 };
        int *gp = &(int){ 42 };
        struct Holder gh = { .p = &(int){ 9 } };

        int read_all(void) {
            return gs->a * 100 + gs->b * 10 + *gp / 42 + *gh.p;
        }

        void bump(void) { gs->b++; }
    }

    unsafe {
        assert_eq!(read_all(), 100 + 20 + 1 + 9);
        // The object really is one object with static storage duration.
        bump();
        assert_eq!(read_all(), 100 + 30 + 1 + 9);
    }
}

#[test]
fn a_literal_written_by_a_macro() {
    c99! {
        struct point { int x; int y; };
        #define POINT(x, y) ((struct point){ (x), (y) })

        int diagonal(int n) {
            struct point p = POINT(n, n * 2);
            return p.x + p.y;
        }
    }

    unsafe {
        assert_eq!(diagonal(3), 9);
    }
}

#[test]
fn a_literal_controls_an_if_and_a_switch() {
    c99! {
        struct Flag { int on; };

        int decide(int n) {
            int out = 0;
            if ((struct Flag){ n }.on) {
                out += 1;
            }
            switch ((int[]){ 0, n, 0 }[1]) {
                case 1: out += 10; break;
                case 2: out += 20; break;
                default: out += 100;
            }
            return out;
        }
    }

    unsafe {
        assert_eq!(decide(1), 11);
        assert_eq!(decide(2), 21);
        assert_eq!(decide(0), 100);
    }
}

#[test]
fn a_literal_in_a_function_that_jumps() {
    // `goto` sends the whole body through the control-flow-graph lowering,
    // where every local — the literal's hidden object included — is defined at
    // the top of the function and the store stays where it was written.
    c99! {
        int jumping(int n) {
            int total = 0;
            int i = 0;
        again:
            if (i >= n) {
                goto done;
            }
            total += (int[]){ i, i + 1 }[1];
            i++;
            goto again;
        done:
            return total;
        }
    }

    unsafe {
        // (0+1) + (1+1) + (2+1)
        assert_eq!(jumping(3), 6);
        assert_eq!(jumping(0), 0);
    }
}

#[test]
fn a_literal_inside_a_switch_body_and_a_nested_block() {
    c99! {
        struct S { int a; };

        int nested(int n) {
            int out = 0;
            switch (n) {
                case 1: {
                    struct S *p = &(struct S){ 7 };
                    out = p->a;
                    break;
                }
                default:
                    out = (&(struct S){ 3 })->a;
            }
            {
                int *q = &(int){ out * 2 };
                out = *q;
            }
            return out;
        }
    }

    unsafe {
        assert_eq!(nested(1), 14);
        assert_eq!(nested(9), 6);
    }
}
