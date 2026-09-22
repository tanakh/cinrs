//! Address constants in the initialiser of an object with static storage
//! duration (C99 6.6p9).
//!
//! An address constant is a null pointer, a pointer to an lvalue designating
//! an object of static storage duration, or a function designator; it may be
//! built with `&`, with array-to-pointer or function-to-pointer conversion,
//! offset by an *integer constant expression* through `+`, `-` or a subscript,
//! and cast between pointer types. The link-time relocation a C compiler emits
//! for one is a `&raw const`/`&raw mut` place expression here, which Rust
//! evaluates in a `static` initialiser for the same reason.
//!
//! Every value is read back twice — once from Rust through the pointer, once
//! from C through a function the same unit defines — so that a wrong address
//! cannot hide behind a Rust side that happens to agree with itself. The
//! objects are file-scope *without* `static` so that Rust can see them at all:
//! a C object with internal linkage is private to the unit's module.

use cinrs::c99;

// ---------------------------------------------------------------------------
// the address of an array element
// ---------------------------------------------------------------------------

/// `&a[k]` with a literal subscript, and the same with an *expression* — C
/// asks only for an integer constant expression, which is what SQLite's
/// `&sqlite3UpperToLower[256-OP_Ne]` is.
#[test]
fn the_address_of_an_array_element() {
    c99! {
        #define BASE 4
        const unsigned char table[] = { 10, 11, 12, 13, 14, 15, 16, 17 };

        const unsigned char *at3 = &table[3];
        const unsigned char *computed = &table[BASE + 2 - 1];
        const unsigned char *decayed = table + 6;
        const unsigned char *backwards = &table[7] - 2;

        unsigned char read_at3(void) { return *at3; }
        unsigned char read_computed(void) { return *computed; }
        unsigned char read_decayed(void) { return *decayed; }
        unsigned char read_backwards(void) { return *backwards; }
    }

    unsafe {
        assert_eq!(*at3, 13);
        assert_eq!(*computed, 15);
        assert_eq!(*decayed, 16);
        assert_eq!(*backwards, 15);
        assert_eq!(read_at3(), 13);
        assert_eq!(read_computed(), 15);
        assert_eq!(read_decayed(), 16);
        assert_eq!(read_backwards(), 15);
    }
}

/// One past the end is a valid address constant (6.5.6p8); only the address
/// itself is checked, since nothing may read through it.
#[test]
fn one_past_the_end_of_an_array() {
    c99! {
        int a[4] = { 1, 2, 3, 4 };
        int *end = &a[4];
        int *also_end = a + 4;

        int distance(void) { return (int)(end - a); }
        int same(void) { return end == also_end; }
    }

    unsafe {
        assert_eq!({ end }, (&raw mut a).cast::<core::ffi::c_int>().add(4));
        assert_eq!(same(), 1);
        assert_eq!(distance(), 4);
    }
}

// ---------------------------------------------------------------------------
// the address of a member
// ---------------------------------------------------------------------------

/// `&s.m`, `&s.a[i]`, and a member of an element of an array of structures.
#[test]
fn the_address_of_a_member() {
    c99! {
        struct point { int x; int y; int v[3]; };
        struct point origin = { 1, 2, { 30, 31, 32 } };
        struct point line[3] = {
            { 10, 11, { 0, 0, 0 } },
            { 20, 21, { 0, 0, 0 } },
            { 30, 31, { 0, 0, 0 } },
        };

        int *px = &origin.x;
        int *py = &origin.y;
        int *pv = &origin.v[2];
        int *second_y = &line[1].y;
        int *third_x = &line[2].x;

        int sum(void) { return *px + *py + *pv + *second_y + *third_x; }
    }

    unsafe {
        assert_eq!(*px, 1);
        assert_eq!(*py, 2);
        assert_eq!(*pv, 32);
        assert_eq!(*second_y, 21);
        assert_eq!(*third_x, 30);
        assert_eq!(sum(), 1 + 2 + 32 + 21 + 30);
    }
}

// ---------------------------------------------------------------------------
// casts
// ---------------------------------------------------------------------------

/// A cast between pointer types keeps the address constant, and so does
/// byte-wise arithmetic on a `char *` built from one.
#[test]
fn a_cast_keeps_the_constant() {
    c99! {
        const unsigned int word = 0x04030201u;
        const unsigned char *bytes = (const unsigned char *)&word;
        const unsigned char *third = (const unsigned char *)&word + 2;
        void *anonymous = (void *)&word;

        unsigned char byte_at(int i) { return bytes[i]; }
        int is_same(void) { return anonymous == (void *)&word; }
    }

    unsafe {
        // The unit is translated for the host, so the byte order is the
        // host's; the value was chosen so that every byte differs.
        let little = *bytes == 1;
        assert!(little || *bytes == 4, "the host is big- or little-endian");
        if little {
            assert_eq!(*third, 3);
            assert_eq!(byte_at(1), 2);
        } else {
            assert_eq!(*third, 2);
            assert_eq!(byte_at(1), 3);
        }
        assert_eq!(is_same(), 1);
    }
}

// ---------------------------------------------------------------------------
// function designators
// ---------------------------------------------------------------------------

/// A function name in a static initialiser is a function designator, which
/// converts to a pointer to it — with or without the `&`.
#[test]
fn a_function_designator() {
    c99! {
        int twice(int n) { return n * 2; }
        int thrice(int n) { return n * 3; }

        typedef int (*unary)(int);
        unary plain = twice;
        unary addressed = &thrice;
        unary table[2] = { twice, thrice };

        int call_plain(int n) { return plain(n); }
        int call_table(int i, int n) { return table[i](n); }
    }

    unsafe {
        assert_eq!({ plain }.expect("a function")(7), 14);
        assert_eq!({ addressed }.expect("a function")(7), 21);
        assert_eq!(call_plain(5), 10);
        assert_eq!(call_table(1, 5), 15);
    }
}

// ---------------------------------------------------------------------------
// a structure holding pointers, which is what SQLite's `aJsonFunc` is
// ---------------------------------------------------------------------------

/// SQLite's `static FuncDef aJsonFunc[] = { JFUNCTION(…), … }`: a table of
/// structures whose members are an integer cast to a `void *`, a function
/// pointer, a string literal and a null pointer — every kind of address
/// constant at once, and the integer one built by an expression rather than
/// written as a literal.
#[test]
fn a_table_of_records_full_of_pointers() {
    c99! {
        #define JSON_BLOB 0x10
        #define INT_TO_PTR(x) ((void *)(__PTRDIFF_TYPE__)(x))

        struct entry {
            int n;
            void *user;
            void (*call)(int *);
            const char *name;
            int *slot;
        };

        int counter = 0;

        void bump(int *p) { *p += 1; }
        void drop(int *p) { *p -= 1; }

        struct entry table[] = {
            { 1, INT_TO_PTR(0 | (0 * JSON_BLOB)), bump, "bump", &counter },
            { 2, INT_TO_PTR(0 | (1 * JSON_BLOB)), drop, "drop", 0 },
            { 3, INT_TO_PTR(7 | (1 * JSON_BLOB)), 0,    "none", 0 },
        };

        int run(int i, int start) { table[i].call(&start); return start; }
        const char *name_of(int i) { return table[i].name; }
        long user_of(int i) { return (long)table[i].user; }
        int slot_is_counter(void) { return table[0].slot == &counter; }
    }

    unsafe {
        assert_eq!(user_of(0), 0);
        assert_eq!(user_of(1), 0x10);
        assert_eq!(user_of(2), 7 | 0x10);
        assert_eq!(run(0, 41), 42);
        assert_eq!(run(1, 41), 40);
        assert!({ table[2].call }.is_none());
        assert_eq!(slot_is_counter(), 1);
        let name = core::ffi::CStr::from_ptr(name_of(1));
        assert_eq!(name.to_bytes(), b"drop");
    }
}

// ---------------------------------------------------------------------------
// forward references
// ---------------------------------------------------------------------------

/// A `static` whose initialiser points into an object *defined later* in the
/// unit. C allows it — the object is declared by then, and the address is the
/// linker's answer — and so does Rust, whose items are not ordered.
#[test]
fn a_pointer_into_an_object_defined_later() {
    c99! {
        extern const char letters[];
        const char *middle = &letters[2];
        const char *whole = letters;
        const char letters[] = "abcdef";

        struct node { int key; struct node *next; };
        extern struct node head;
        struct node tail = { 2, &head };
        struct node head = { 1, &tail };

        char read_middle(void) { return *middle; }
        int walk(void) { return head.key * 10 + head.next->key; }
    }

    unsafe {
        assert_eq!(*middle, b'c' as core::ffi::c_char);
        assert_eq!(*whole, b'a' as core::ffi::c_char);
        assert_eq!(read_middle(), b'c' as core::ffi::c_char);
        assert_eq!(walk(), 12);
        assert_eq!({ head.next }, &raw mut tail);
        assert_eq!({ tail.next }, &raw mut head);
    }
}

// ---------------------------------------------------------------------------
// block scope
// ---------------------------------------------------------------------------

/// A block-scope `static` holds one as well; its initialiser is evaluated at
/// translation time exactly as a file-scope one is.
#[test]
fn a_block_scope_static_may_hold_one_too() {
    c99! {
        static int shared[4] = { 5, 6, 7, 8 };

        int peek(void) {
            static int *const inner = &shared[2];
            return *inner;
        }
    }

    assert_eq!(unsafe { peek() }, 7);
}
