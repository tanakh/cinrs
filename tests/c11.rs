//! Integration tests that *run* translated C11.
//!
//! Everything C11 added that this crate implements is here end to end:
//! `_Static_assert`, `_Generic`, `_Alignof`, `_Alignas`, `_Noreturn` and
//! anonymous `struct`/`union` members. The layout tests matter most — an
//! anonymous member is a *synthetic* field in the generated Rust type, so
//! `sizeof`, `offsetof` and the item `rustc` lays out all have to agree.

use cinrs::c11;

// ---------------------------------------------------------------------------
// _Static_assert
// ---------------------------------------------------------------------------

#[test]
fn static_assertions_hold_at_every_scope() {
    c11! {
        _Static_assert(sizeof(int) == 4, "int is 32 bits here");

        struct Checked {
            int a;
            int b;
            _Static_assert(sizeof(int) * 2 == 8, "two ints are eight bytes");
        };

        int checked(void) {
            _Static_assert(1 + 1 == 2, "arithmetic still works");
            struct Checked c;
            c.a = 1;
            c.b = 2;
            return c.a + c.b;
        }
    }

    assert_eq!(unsafe { checked() }, 3);
    assert_eq!(size_of::<Checked>(), 8);
}

// ---------------------------------------------------------------------------
// _Generic
// ---------------------------------------------------------------------------

#[test]
fn generic_selection_dispatches_on_the_type() {
    c11! {
        /* A `\` line continuation is not available in raw-token form — the
           Rust lexer refuses one — so the replacement list stays on one line. */
        #define KIND(x) _Generic((x), int: 1, double: 2, char *: 3, int *: 4, default: 0)

        int kind_of_int(void) { int v = 0; return KIND(v); }
        int kind_of_double(void) { double v = 0; return KIND(v); }
        int kind_of_string(void) { char *v = "x"; return KIND(v); }
        int kind_of_pointer(void) { int v = 0; int *p = &v; return KIND(p); }
        int kind_of_array(void) { int a[3]; return KIND(a); }
        int kind_of_other(void) { long v = 0; return KIND(v); }

        /* The unchosen associations are parsed but never checked, so an
           association whose expression would not compile for this type is
           still fine. */
        int only_the_chosen_arm(int n) {
            return _Generic(n, int: n + 1, char *: *n);
        }
    }

    unsafe {
        assert_eq!(kind_of_int(), 1);
        assert_eq!(kind_of_double(), 2);
        assert_eq!(kind_of_string(), 3);
        assert_eq!(kind_of_pointer(), 4);
        // An array is converted to a pointer to its first element before the
        // association is chosen (C11 DR 481).
        assert_eq!(kind_of_array(), 4);
        assert_eq!(kind_of_other(), 0);
        assert_eq!(only_the_chosen_arm(41), 42);
    }
}

#[test]
fn generic_associations_may_differ_only_in_their_qualifiers() {
    // C11 6.5.1.1p2 forbids two associations naming *compatible* types, and a
    // qualifier is part of the type: `int` and `const int` are not compatible,
    // so both may appear. The controlling expression has had the lvalue
    // conversion applied to it (DR 481), which drops the top-level qualifiers
    // — so it is always the unqualified association that is chosen, whether
    // the operand was `const` or not. (c-testsuite 00219.)
    c11! {
        const int qualified = 0;

        int a_f(void) { return 20; }
        int b_f(void) { return 10; }

        int through_a_const_lvalue(void) {
            return _Generic(qualified, int: a_f, const int: b_f)();
        }

        int through_a_plain_lvalue(void) {
            int plain = 0;
            /* The qualified association is written first, and still loses. */
            return _Generic(plain, const int: 1, volatile int: 2, int: 3);
        }

        int pointers(void) {
            const int *to_const = &qualified;
            int value = 0;
            int *to_plain = &value;
            return _Generic(to_const, int *: 1, const int *: 2, default: 0) * 10
                 + _Generic(to_plain, int *: 1, const int *: 2, default: 0);
        }

        int a_top_level_qualified_pointer(void) {
            const int *const doubly = &qualified;
            /* Lvalue conversion drops the *pointer's* own `const`, so neither
               `int *` nor `int * const` matches a `const int *`. */
            return _Generic(doubly, int *: 1, int *const: 2, default: 20);
        }

        int arrays_are_not_pointers(void) {
            return _Generic(qualified, char: 1, int[4]: 2, default: 5);
        }
    }

    unsafe {
        assert_eq!(through_a_const_lvalue(), 20);
        assert_eq!(through_a_plain_lvalue(), 3);
        assert_eq!(pointers(), 21);
        assert_eq!(a_top_level_qualified_pointer(), 20);
        assert_eq!(arrays_are_not_pointers(), 5);
    }
}

// ---------------------------------------------------------------------------
// anonymous members
// ---------------------------------------------------------------------------

#[test]
fn anonymous_members_are_part_of_the_enclosing_record() {
    c11! {
        #include <stddef.h>

        struct Value {
            int tag;
            union {
                int as_int;
                double as_double;
                char as_bytes[8];
            };
        };

        struct Value from_int(int v) {
            struct Value out = { .tag = 0, .as_int = v };
            return out;
        }

        struct Value from_double(double v) {
            struct Value out = { .tag = 1, .as_double = v };
            return out;
        }

        int read_int(struct Value v) { return v.as_int; }
        double read_double(const struct Value *v) { return v->as_double; }

        void set_int(struct Value *v, int n) {
            v->tag = 0;
            v->as_int = n;
        }

        unsigned long size(void) { return sizeof(struct Value); }
        unsigned long tag_offset(void) { return offsetof(struct Value, tag); }
        unsigned long value_offset(void) { return offsetof(struct Value, as_double); }
        unsigned long bytes_offset(void) { return offsetof(struct Value, as_bytes); }
    }

    unsafe {
        assert_eq!(read_int(from_int(7)), 7);
        let floating = from_double(1.5);
        assert_eq!(read_double(&raw const floating), 1.5);

        let mut v = from_int(1);
        set_int(&raw mut v, 9);
        assert_eq!(read_int(v), 9);

        // What C computes and what Rust lays out have to be the same thing.
        assert_eq!(size(), size_of::<Value>() as u64);
        assert_eq!(align_of::<Value>(), 8);
        assert_eq!(tag_offset(), core::mem::offset_of!(Value, tag) as u64);
        assert_eq!(
            value_offset(),
            core::mem::offset_of!(Value, __cinrs_anon0.as_double) as u64
        );
        assert_eq!(bytes_offset(), value_offset());

        // The synthetic field is an ordinary Rust field, so Rust can build one.
        let built = Value {
            tag: 0,
            __cinrs_anon0: core::mem::zeroed(),
        };
        assert_eq!(read_int(built), 0);
    }
}

#[test]
fn an_anonymous_struct_flattens_its_members() {
    c11! {
        struct Outer {
            struct {
                int x;
                int y;
            };
            int z;
        };

        struct Outer make(void) {
            struct Outer o = { .x = 1, .y = 2, .z = 3 };
            return o;
        }

        int sum(struct Outer o) { return o.x + o.y + o.z; }
    }

    unsafe {
        assert_eq!(sum(make()), 6);
        assert_eq!(size_of::<Outer>(), 12);
    }
}

// ---------------------------------------------------------------------------
// _Alignof and _Alignas
// ---------------------------------------------------------------------------

#[test]
fn alignof_and_alignas_agree_with_rust() {
    c11! {
        struct Plain { char c; double d; };
        struct Over { _Alignas(32) int first; char rest; };

        unsigned long align_of_int(void) { return _Alignof(int); }
        unsigned long align_of_double(void) { return _Alignof(double); }
        unsigned long align_of_plain(void) { return _Alignof(struct Plain); }
        unsigned long align_of_expr(void) { double d = 0; return _Alignof(d); }

        unsigned long align_of_over(void) { return _Alignof(struct Over); }
        unsigned long size_of_over(void) { return sizeof(struct Over); }
    }

    unsafe {
        assert_eq!(align_of_int() as usize, align_of::<core::ffi::c_int>());
        assert_eq!(align_of_double() as usize, align_of::<f64>());
        assert_eq!(align_of_plain() as usize, align_of::<Plain>());
        assert_eq!(align_of_expr() as usize, align_of::<f64>());

        assert_eq!(align_of_over() as usize, align_of::<Over>());
        assert_eq!(align_of::<Over>(), 32);
        assert_eq!(size_of_over() as usize, size_of::<Over>());
        assert_eq!(size_of::<Over>(), 32);
    }
}

// ---------------------------------------------------------------------------
// _Noreturn
// ---------------------------------------------------------------------------

#[test]
fn a_noreturn_call_can_end_a_value_returning_function() {
    c11! {
        #include <stdlib.h>

        /* No `return` after `abort()`: a call to a `_Noreturn` function ends
           the function, and the bundled <stdlib.h> marks `abort` as one. */
        int must_be_positive(int n) {
            if (n > 0) return n;
            abort();
        }

        /* A `_Noreturn` function of the unit's own, which ends in another
           one. */
        _Noreturn void die(int status) { exit(status); }

        int checked(int n) {
            if (n >= 0) return n;
            die(2);
        }
    }

    assert_eq!(unsafe { must_be_positive(3) }, 3);
    assert_eq!(unsafe { checked(0) }, 0);
}

// ---------------------------------------------------------------------------
// the version macro
// ---------------------------------------------------------------------------

#[test]
fn the_version_macro_says_c11() {
    c11! {
        long version(void) { return __STDC_VERSION__; }

        #if __STDC_VERSION__ >= 201112L
        int has_c11(void) { return 1; }
        #else
        int has_c11(void) { return 0; }
        #endif
    }

    unsafe {
        assert_eq!(version(), 201112);
        assert_eq!(has_c11(), 1);
    }
}

#[test]
fn the_c11_headers_are_bundled() {
    c11! {
        #include <assert.h>
        #include <stdalign.h>
        #include <stdbool.h>
        #include <stdnoreturn.h>

        /* `static_assert` is <assert.h>'s spelling of `_Static_assert`, and
           `alignof` is <stdalign.h>'s of `_Alignof`. */
        static_assert(__alignas_is_defined, "<stdalign.h> was read");
        static_assert(sizeof(bool) == 1, "<stdbool.h> was read");

        struct Wide { alignas(16) int first; };

        unsigned long wide_align(void) { return alignof(struct Wide); }
        bool truth(void) { return true; }

        noreturn void never(void);
    }

    unsafe {
        assert_eq!(wide_align(), 16);
        assert!(truth());
    }
}
