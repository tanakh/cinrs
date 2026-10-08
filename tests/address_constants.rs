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

/// A function pointer whose constant is *not* a function: libwebp's
/// `WEBP_DSP_INIT` keeps `static volatile VP8CPUInfo last = (VP8CPUInfo)&last;`
/// as a sentinel no real function can equal, and `SIG_IGN` is
/// `(void (*)(int)) 1`. Rust's constant evaluation refuses an `Option<fn>`
/// holding either, so the item holds a data pointer and every use reads it as
/// the function pointer C declared; it is compared, assigned a real function
/// and called through like any other.
#[test]
fn a_function_pointer_holding_an_objects_address() {
    cinrs::gnu99! {
        typedef int (*unary)(int);

        static int plus_one(int n) { return n + 1; }
        static int data;
        static unary from_data = (unary)&data;
        static unary from_integer = (unary)1;
        /* glibc's `SIG_ERR` is `((__sighandler_t) -1)`: all ones, as an
           address, for a function pointer and a data pointer alike. */
        static unary minus_one = (unary)-1;
        static unary minus_two = (unary)-2;
        static unary large = (unary)0xffffffff00000000ull;
        static void *all_ones = (void *)-1;

        int negative_ones(void) {
            return (minus_one == (unary)-1) + 10 * (minus_two == (unary)-2)
                 + 100 * (large == (unary)0xffffffff00000000ull)
                 + 1000 * (all_ones == (void *)-1)
                 + 10000 * ((unsigned long)all_ones == (unsigned long)-1);
        }

        int sentinel(int n) {
            static volatile unary last = (unary)&last;
            int r = 0;
            if (last == (unary)&last) r += 1;          /* the sentinel itself */
            if (last != plus_one) r += 10;
            last = plus_one;                           /* a real function now */
            if (last == plus_one) r += 100;
            return r * 1000 + last(n);
        }
        int others(void) {
            int r = (from_data == (unary)&data) + 10 * (from_integer == (unary)1);
            from_integer = plus_one;
            return r * 100 + from_integer(1);
        }
    }

    unsafe {
        assert_eq!(sentinel(41), 111_042);
        assert_eq!(others(), 1102);
        assert_eq!(negative_ones(), 11_111);
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

/// A conditional whose condition is an integer constant is the address its
/// chosen arm is, which GCC folds: CPython's `_Py_LATIN1_CHR(ch)` picks one
/// of two tables by `ch < 128` and stands in static keyword tables.
#[test]
fn a_constant_condition_chooses_an_address() {
    cinrs::c11! {
        static struct { char ascii[128]; char latin1[128]; } strings;

        #define LATIN1_CHR(CH) ((CH) < 128 ? &strings.ascii[(CH)] : &strings.latin1[(CH) - 128])

        static char *const letter = LATIN1_CHR('p');
        static char *const table[] = { LATIN1_CHR('n'), LATIN1_CHR(200) };

        int chosen(void) {
            return (letter == &strings.ascii['p'])
                + (table[0] == &strings.ascii['n']) * 2
                + (table[1] == &strings.latin1[72]) * 4;
        }
    }

    assert_eq!(unsafe { chosen() }, 7);
}

/// The same with string literals, nested, which is a conversion of the
/// conditional's `char *` to the object's `const char *` around it: OpenSSL's
/// `test/bioprinttest.c` names the size of `long` this way.
#[test]
fn a_constant_condition_chooses_a_string_literal() {
    cinrs::c11! {
        static const char *s = sizeof(long) == 4 ? "four" : sizeof(long) == 8 ? "eight" : "";
        static const void *other = 1 > 2 ? "no" : (const void *)"yes";

        int chosen_length(void) {
            int n = 0;
            while (s[n]) n++;
            return n * 10 + (((const char *)other)[0] == 'y');
        }
    }

    let expected = if core::mem::size_of::<core::ffi::c_long>() == 8 {
        51
    } else {
        41
    };
    assert_eq!(unsafe { chosen_length() }, expected);
}

/// An array whose list decides its length names itself in that list: its
/// scope begins after its declarator (C17 6.2.1p7), before the initialiser.
/// FFmpeg's matroskadec.c builds a tree of syntax elements this way.
#[test]
fn an_array_of_unknown_length_names_itself_in_its_list() {
    cinrs::c11! {
        struct node { int v; const struct node *child; };
        static const struct node tree[] = { { 1, tree }, { 2, 0 }, { 3, &tree[1] } };
        const char *const names[] = { "a", "b", (const char *) names };

        int self_references(void) {
            static const void *self[] = { 0, self, &self[2] };
            return (tree[0].child == tree)
                + (tree[2].child->v == 2) * 2
                + (sizeof tree / sizeof tree[0] == 3) * 4
                + (names[2] == (const char *) names) * 8
                + (self[1] == self && self[2] == &self[2]) * 16;
        }
    }

    assert_eq!(unsafe { self_references() }, 31);
}

/// An integer as wide as a pointer, initialised with an address converted to
/// it: FFmpeg's `static atomic_uintptr_t av_log_callback = (uintptr_t)
/// av_log_default_callback;`. GCC takes it as the relocated address; the item
/// holds it as a pointer, and the program reads and writes the integer.
#[test]
fn an_integer_holds_an_address() {
    cinrs::c11! {
        #include <stdint.h>
        #include <stdatomic.h>

        static int called;
        static void callback(void) { called++; }
        static int table[4];

        /* `long` where it is as wide as a pointer, as it is on LP64; on
         * Windows it is narrower, and an address does not fit one (GCC
         * refuses it there too). */
        #if __SIZEOF_LONG__ == __SIZEOF_POINTER__
        typedef long word;
        #else
        typedef long long word;
        #endif

        static uintptr_t as_integer = (uintptr_t) callback;
        static atomic_uintptr_t as_atomic = (uintptr_t) callback;
        word into_table = (word) &table[2];
        static const intptr_t offset = (intptr_t) (table + 1);
        static uintptr_t not_an_address = (uintptr_t) (void *) 16;

        int addresses(void) {
            void (*f)(void) = (void (*)(void)) atomic_load(&as_atomic);
            f();
            int ok = (as_integer == (uintptr_t) callback)
                + (into_table == (word) &table[2]) * 2
                + (offset == (intptr_t) &table[1]) * 4
                + (not_an_address == 16) * 8
                + called * 16;
            as_integer += 1;
            atomic_store(&as_atomic, 7);
            return ok + (as_integer == (uintptr_t) callback + 1) * 32
                + (atomic_load(&as_atomic) == 7) * 64;
        }
    }

    assert_eq!(unsafe { addresses() }, 127);
}

/// The same as a member of a table: git's parse-options tables, `{ .defval =
/// (intptr_t) "all" }`, and nginx's variable tables, `(uintptr_t)
/// ngx_ssl_get_protocol`. The item is a `MaybeUninit` of the table, built
/// with zero there and the addresses written over it.
#[test]
fn a_table_holds_addresses_in_integer_members() {
    cinrs::c11! {
        #include <stdint.h>

        struct option { const char *long_name; intptr_t defval; int flags; };
        static struct option options[] = {
            { .long_name = "untracked-files", .defval = (intptr_t) "all" },
            { .long_name = "ignored", .defval = (intptr_t) "" },
            { .long_name = "count", .defval = 3, .flags = 1 },
        };

        typedef int (*getter_t)(void);
        static int get_protocol(void) { return 7; }
        struct variable { const char *name; uintptr_t data; };
        struct module { int id; struct variable vars[2]; };
        const struct module ssl = { 1, { { "ssl_protocol", (uintptr_t) get_protocol }, { "none", 0 } } };

        /* A pointer-sized `long` where there is one (see above). */
        #if __SIZEOF_LONG__ == __SIZEOF_POINTER__
        typedef long word_n;
        #else
        typedef long long word_n;
        #endif
        union word { word_n n; void *p; };
        static union word words[2] = { { .n = (word_n) &options[1] }, { .n = 5 } };

        int tables(void) {
            return (((const char *) options[0].defval)[0] == 'a')
                + (((const char *) options[1].defval)[0] == 0) * 2
                + (options[2].defval == 3 && options[2].flags == 1) * 4
                + (((getter_t) ssl.vars[0].data)() == 7) * 8
                + (ssl.vars[1].data == 0 && ssl.id == 1) * 16
                + (words[0].n == (word_n) &options[1] && words[1].n == 5) * 32;
        }
    }

    assert_eq!(unsafe { tables() }, 63);
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
