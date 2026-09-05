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

// ---------------------------------------------------------------------------
// <uchar.h>, and the u8/u/U literals
// ---------------------------------------------------------------------------

/// C11's Unicode literals (N1326, N1488) and the types they have.
///
/// Rust's own lexer reserves `u8"…"`, `u"…"` and `U"…"` as prefixes of its
/// own, so these cannot be written as raw tokens — the string-literal form of
/// the macro is the one that carries them, which is what every test here uses.
#[test]
fn unicode_string_literals_and_their_types() {
    c11! { r##"
        #include <uchar.h>
        #include <string.h>

        static const char utf8[] = u8"héllo";
        static const char16_t utf16[] = u"hé\U0001F600";
        static const char32_t utf32[] = U"hé\U0001F600";

        /* `sizeof` counts the elements, terminating NUL and all: the smiley
           needs a surrogate pair in UTF-16 and one code unit in UTF-32. */
        unsigned long utf8_size(void) { return sizeof u8"héllo"; }
        unsigned long utf16_size(void) { return sizeof u"hé\U0001F600"; }
        unsigned long utf32_size(void) { return sizeof U"hé\U0001F600"; }
        unsigned long char16_size(void) { return sizeof(char16_t); }
        unsigned long char32_size(void) { return sizeof(char32_t); }

        int utf8_at(int i) { return (unsigned char) utf8[i]; }
        unsigned long utf16_at(int i) { return utf16[i]; }
        unsigned long utf32_at(int i) { return utf32[i]; }

        /* An unprefixed literal takes the prefix of the one next to it, and
           its own bytes are re-decoded when it does (6.4.5p5). */
        static const char32_t joined[] = U"a" "é" U"b";
        unsigned long joined_at(int i) { return joined[i]; }
        unsigned long joined_size(void) { return sizeof joined; }

        /* The character constants, each with the type of one element of the
           string literal that shares its prefix. */
        int narrow_char(void) { return 'x'; }
        unsigned long u16_char_size(void) { return sizeof u'x'; }
        unsigned long u32_char_size(void) { return sizeof U'x'; }
        int u16_char(void) { return u'é'; }
        int u32_char(void) { return U'\U0001F600'; }

        /* `_Generic` sees the underlying integer types, which is what
           `char16_t` and `char32_t` are typedefs of. */
        int kind_of_u(void) { return _Generic(u'x', unsigned short: 16, unsigned int: 32, default: 0); }
        int kind_of_U(void) { return _Generic(U'x', unsigned short: 16, unsigned int: 32, default: 0); }

        /* The library declarations are there and link. */
        int converts(void) {
            char buffer[8];
            mbstate_t state;
            memset(&state, 0, sizeof state);
            return (int) c32rtomb(buffer, U'A', &state) == 1 && buffer[0] == 'A';
        }
    "## }

    unsafe {
        // "héllo" is six bytes in UTF-8, seven with the NUL.
        assert_eq!(utf8_size(), 7);
        assert_eq!(utf8_at(0), 0x68);
        assert_eq!((utf8_at(1), utf8_at(2)), (0xc3, 0xa9));
        assert_eq!((char16_size(), char32_size()), (2, 4));
        // 'h', 'é', a surrogate pair and the NUL: five units of two bytes.
        assert_eq!(utf16_size(), 10);
        assert_eq!(
            (
                utf16_at(0),
                utf16_at(1),
                utf16_at(2),
                utf16_at(3),
                utf16_at(4)
            ),
            (0x68, 0xe9, 0xd83d, 0xde00, 0)
        );
        // The same three characters and the NUL, four units of four bytes.
        assert_eq!(utf32_size(), 16);
        assert_eq!(
            (utf32_at(0), utf32_at(1), utf32_at(2), utf32_at(3)),
            (0x68, 0xe9, 0x1_f600, 0)
        );
        assert_eq!(joined_size(), 16);
        assert_eq!(
            (joined_at(0), joined_at(1), joined_at(2), joined_at(3)),
            (0x61, 0xe9, 0x62, 0)
        );
        assert_eq!(narrow_char(), 0x78);
        assert_eq!((u16_char_size(), u32_char_size()), (2, 4));
        assert_eq!(u16_char(), 0xe9);
        assert_eq!(u32_char(), 0x1_f600);
        assert_eq!((kind_of_u(), kind_of_U()), (16, 32));
        assert_eq!(converts(), 1);
    }
}

#[test]
fn the_c11_headers_are_bundled() {
    c11! {
        #include <assert.h>
        #include <stdalign.h>
        #include <stdbool.h>
        #include <stdnoreturn.h>
        #include <uchar.h>

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
