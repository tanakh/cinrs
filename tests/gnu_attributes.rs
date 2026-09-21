//! `__attribute__((…))` and `#pragma pack`: what is honoured, and what it does.

use cinrs::c99;

// ---------------------------------------------------------------------------
// packing
// ---------------------------------------------------------------------------

c99! {
    #include <stddef.h>

    /* A protocol header, which is the reason `packed` exists. */
    struct __attribute__((packed)) Ethernet {
        unsigned char dst[6];
        unsigned char src[6];
        unsigned short ethertype;
        unsigned int crc;
    };

    struct Mixed { char c; int x; };
    struct __attribute__((packed)) MixedPacked { char c; int x; };

    /* Per-member packing, which only unaligns the member it is on. */
    struct MemberPacked { char c; int x __attribute__((packed)); short s; };

    #pragma pack(2)
    struct Pack2 { char c; int x; short s; };
    struct Pack2Bits { char c; long long b : 40; int z : 5; };
    #pragma pack(push, 1)
    struct Pack1 { char c; int x; };
    #pragma pack(pop)
    struct Pack2Again { char c; int x; short s; };
    #pragma pack()
    struct Natural { char c; int x; short s; };

    unsigned long sizes(int which) {
        switch (which) {
        case 0: return sizeof(struct Ethernet);
        case 1: return sizeof(struct Mixed);
        case 2: return sizeof(struct MixedPacked);
        case 3: return sizeof(struct MemberPacked);
        case 4: return sizeof(struct Pack2);
        case 5: return sizeof(struct Pack1);
        case 6: return sizeof(struct Pack2Again);
        case 7: return sizeof(struct Natural);
        case 8: return sizeof(struct Pack2Bits);
        default: return 0;
        }
    }

    unsigned long offsets(int which) {
        switch (which) {
        case 0: return offsetof(struct Ethernet, ethertype);
        case 1: return offsetof(struct Ethernet, crc);
        case 2: return offsetof(struct MixedPacked, x);
        case 3: return offsetof(struct MemberPacked, x);
        case 4: return offsetof(struct MemberPacked, s);
        case 5: return offsetof(struct Pack2, x);
        case 6: return offsetof(struct Pack2, s);
        default: return 0;
        }
    }

    /* Reading and writing a packed member through a pointer, which must not
     * take a reference to it. */
    unsigned int ethertype_of(struct Ethernet *e) { return e->ethertype; }
    void set_crc(struct Ethernet *e, unsigned int crc) { e->crc = crc; }

    /* Bit-fields inside a packed struct follow GCC: the allocation-unit rule
     * is switched off, so a field starts at the next free bit. */
    struct __attribute__((packed)) PackedBits { char c; int x : 20; int y : 20; };
    unsigned long packed_bits_size(void) { return sizeof(struct PackedBits); }
    int packed_bits_roundtrip(int v) {
        struct PackedBits b;
        b.x = v;
        return b.x;
    }
}

#[test]
fn packed_records_have_the_layout_gcc_gives_them() {
    // Verified against gcc 15 on x86-64; see tests/bitfield_layout.rs for the
    // differential test that keeps the whole corpus honest.
    assert_eq!(unsafe { sizes(0) }, 6 + 6 + 2 + 4);
    assert_eq!(unsafe { sizes(1) }, 8);
    assert_eq!(unsafe { sizes(2) }, 5);
    assert_eq!(unsafe { sizes(3) }, 8);
    assert_eq!(unsafe { sizes(4) }, 8);
    assert_eq!(unsafe { sizes(5) }, 5);
    assert_eq!(unsafe { sizes(6) }, 8);
    assert_eq!(unsafe { sizes(7) }, 12);
    assert_eq!(unsafe { sizes(8) }, 8);
    assert_eq!(unsafe { offsets(0) }, 12);
    assert_eq!(unsafe { offsets(1) }, 14);
    assert_eq!(unsafe { offsets(2) }, 1);
    assert_eq!(unsafe { offsets(3) }, 1);
    assert_eq!(unsafe { offsets(4) }, 6);
    assert_eq!(unsafe { offsets(5) }, 2);
    assert_eq!(unsafe { offsets(6) }, 6);

    // The Rust item really has that layout, which is what makes the two sides
    // agree about the same bytes.
    assert_eq!(size_of::<Ethernet>(), 18);
    assert_eq!(align_of::<Ethernet>(), 1);

    let mut e = Ethernet {
        dst: [0; 6],
        src: [0; 6],
        ethertype: 0x0800,
        crc: 0,
    };
    assert_eq!(unsafe { ethertype_of(&raw mut e) }, 0x0800);
    unsafe { set_crc(&raw mut e, 0xdead_beef) };
    assert_eq!({ e.crc }, 0xdead_beef);

    assert_eq!(unsafe { packed_bits_size() }, 6);
    assert_eq!(unsafe { packed_bits_roundtrip(-1) }, -1);
    assert_eq!(unsafe { packed_bits_roundtrip(0x5_5555) }, 0x5_5555);
}

// ---------------------------------------------------------------------------
// aligned
// ---------------------------------------------------------------------------

c99! {
    #include <stddef.h>

    struct __attribute__((aligned(32))) CacheLine { int a; };
    struct Moved { char c; __attribute__((aligned(16))) int x; };

    unsigned long cache_line_size(void) { return sizeof(struct CacheLine); }
    unsigned long cache_line_align(void) { return __alignof__(struct CacheLine); }
    unsigned long moved_offset(void) { return offsetof(struct Moved, x); }
    unsigned long moved_size(void) { return sizeof(struct Moved); }
}

c99! {
    /* `aligned` written after the declarator of a `typedef` of an anonymous
     * record is a property of the type it names, and the only way to name
     * that record at all. */
    typedef struct { char c[8]; } __attribute__((aligned(8))) Octet;
    typedef struct { char c[3]; } Loose;

    unsigned long octet_size(void)  { return sizeof(Octet); }
    unsigned long octet_align(void) { return __alignof__(Octet); }
    unsigned long loose_size(void)  { return sizeof(Loose); }

    static Octet an_octet;
    int octet_is_aligned(void) { return (((unsigned long) &an_octet) & 7) == 0; }
}

#[test]
fn aligned_on_a_typedef_of_an_anonymous_record_is_honoured() {
    assert_eq!(unsafe { octet_size() }, 8);
    assert_eq!(unsafe { octet_align() }, 8);
    assert_eq!(unsafe { loose_size() }, 3);
    assert_eq!(align_of::<Octet>(), 8);
    assert_eq!(unsafe { octet_is_aligned() }, 1);
}

#[test]
fn aligned_raises_a_records_alignment_and_moves_a_member() {
    assert_eq!(unsafe { cache_line_size() }, 32);
    assert_eq!(unsafe { cache_line_align() }, 32);
    assert_eq!(align_of::<CacheLine>(), 32);
    assert_eq!(unsafe { moved_offset() }, 16);
    assert_eq!(unsafe { moved_size() }, 32);
    assert_eq!(size_of::<Moved>(), 32);
}

// ---------------------------------------------------------------------------
// reaching an underaligned member
// ---------------------------------------------------------------------------

c99! {
    /* Packing moves a member to an offset its own type is not aligned for.
     * Reading and writing it is what C is for; the generated Rust has to
     * reach it without an aligned load, which is undefined behaviour there
     * and an abort in a debug build. */
    typedef struct {
        char tag;
        int  values[3];
        struct Pair { short a; short b; } pair;
    } __attribute__((packed)) Wire;

    int  read_value(Wire *w, int i)          { return w->values[i]; }
    void write_value(Wire *w, int i, int v)  { w->values[i] = v; }
    short read_pair_b(Wire *w)               { return w->pair.b; }
    void  write_pair_b(Wire *w, short v)     { w->pair.b = v; }
    int   sum(Wire *w) {
        int total = 0, i;
        for (i = 0; i < 3; i++)
            total += w->values[i];
        return total;
    }

    unsigned long wire_size(void) { return sizeof(Wire); }

    /* An array of packed records has every element at an odd offset from the
     * one before, so the second element's members are misaligned too. */
    int nth_value(Wire *ws, int n, int i) { return ws[n].values[i]; }
}

#[test]
fn a_packed_member_is_read_and_written_through_a_pointer() {
    assert_eq!(unsafe { wire_size() }, 1 + 12 + 4);
    let mut w = Wire {
        tag: 7,
        values: [0; 3],
        pair: Pair { a: 0, b: 0 },
    };
    unsafe {
        for i in 0..3 {
            write_value(&raw mut w, i, 100 + i);
        }
        for i in 0..3 {
            assert_eq!(read_value(&raw mut w, i), 100 + i);
        }
        assert_eq!(sum(&raw mut w), 303);
        write_pair_b(&raw mut w, -2);
        assert_eq!(read_pair_b(&raw mut w), -2);

        let mut many = [w, w];
        many[1].values[2] = 9;
        assert_eq!(nth_value(many.as_mut_ptr(), 1, 2), 9);
        assert_eq!(nth_value(many.as_mut_ptr(), 0, 0), 100);
    }
}

c99! {
    /* Type punning a byte buffer to a record: the buffer is one byte aligned,
     * so every member reached through the cast pointer is underaligned. */
    typedef struct { int count; double *data; } Header;

    static char buffer[64];

    void store(double *p) {
        ((Header *) buffer)->count = 3;
        ((Header *) buffer)->data = p;
    }
    double first(void) { return ((Header *) buffer)->data[0]; }
    int stored_count(void) { return ((Header *) buffer)->count; }
}

#[test]
fn a_byte_buffer_punned_to_a_record_is_reached_unaligned() {
    let mut values = [1.5f64, 2.5];
    unsafe {
        store(values.as_mut_ptr());
        assert_eq!(stored_count(), 3);
        assert_eq!(first(), 1.5);
    }
}

// ---------------------------------------------------------------------------
// function attributes
// ---------------------------------------------------------------------------

c99! {
    __attribute__((always_inline)) int hot_path(int n) { return n + 1; }
    __attribute__((noinline)) int cold_path(int n) { return n + 2; }
    __attribute__((cold)) int unlikely_path(int n) { return n + 3; }
    int pure_ish(int n) __attribute__((pure, nothrow, leaf, warn_unused_result));
    int pure_ish(int n) { return n * 2; }

    /* An attribute nobody has heard of is ignored, exactly as C23 requires. */
    __attribute__((no_such_attribute("with", 1, arguments))) int ignored(int n) {
        return n;
    }

    __attribute__((noreturn)) void never_comes_back(void);
    void never_comes_back(void) { for (;;) { } }
    int ends_with_a_noreturn_call(int n) {
        if (n) return n;
        never_comes_back();
    }

    /* What a section is called belongs to the object format: ELF and COFF take
     * a bare name, Mach-O wants "segment,section" and refuses anything else —
     * and, for a section that holds code, the two flags that say so, without
     * which `ld` warns that unwind information points at a non-code section. */
    #ifdef __APPLE__
    #define TEST_SECTION "__TEXT,__cinrs_test,regular,pure_instructions"
    #else
    #define TEST_SECTION ".cinrs_test_text"
    #endif
    __attribute__((section(TEST_SECTION))) int in_a_section(void) { return 7; }

    __attribute__((deprecated("use replacement instead"))) int obsolete(void) { return 1; }
    int replacement(void) { return 2; }
}

#[test]
fn the_function_attributes_are_honoured_or_ignored() {
    assert_eq!(unsafe { hot_path(1) }, 2);
    assert_eq!(unsafe { cold_path(1) }, 3);
    assert_eq!(unsafe { unlikely_path(1) }, 4);
    assert_eq!(unsafe { pure_ish(21) }, 42);
    assert_eq!(unsafe { ignored(5) }, 5);
    assert_eq!(unsafe { ends_with_a_noreturn_call(5) }, 5);
    assert_eq!(unsafe { in_a_section() }, 7);
    assert_eq!(unsafe { replacement() }, 2);
    #[allow(deprecated)]
    {
        assert_eq!(unsafe { obsolete() }, 1);
    }
}

// ---------------------------------------------------------------------------
// constructors and destructors
// ---------------------------------------------------------------------------

/// `constructor` and `destructor` are an initialiser table the runtime walks,
/// which is an ELF and a Mach-O idea: cinrs refuses the attributes anywhere
/// else, with a `compile_error!` naming the reason (see `init_array_guard` in
/// the code generator), so a unit using them cannot even be written on Windows.
/// The `cfg` here is that guard's own condition — `tests/ui` is where the
/// refusal itself is checked.
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
    target_vendor = "apple"
))]
mod initialiser_table {
    use cinrs::c99;

    c99! {
        static int startup_count;

        __attribute__((constructor)) static void run_first(void) { startup_count += 1; }
        __attribute__((constructor(101))) static void run_first_too(void) {
            startup_count += 10;
        }
        __attribute__((destructor)) static void run_last(void) { startup_count -= 1; }

        int startup_ran(void) { return startup_count; }
    }

    #[test]
    fn a_constructor_has_already_run_by_the_time_a_test_does() {
        // Both constructors ran before `main`, so the counter is 11 before the
        // first statement of any test.
        assert_eq!(unsafe { startup_ran() }, 11);
    }
}

// ---------------------------------------------------------------------------
// asm labels
// ---------------------------------------------------------------------------

c99! {
    #include <stddef.h>

    /* An `asm` label renames a declaration's symbol, which is how a program
     * reaches a libc function under a name of its own. The generated `extern`
     * block declares `my_strlen` under that C name and points it at `strlen`
     * with `#[link_name]`, so the two names are two items and one symbol.
     * `strlen` answers a `size_t`, and saying `unsigned long` instead would be
     * four bytes too few on Windows. */
    size_t my_strlen(const char *s) __asm__("strlen");
    int my_abs(int n) __asm__("abs");

    size_t length_of(const char *s) { return my_strlen(s); }
    int magnitude(int n) { return my_abs(n); }
}

#[test]
fn an_asm_label_points_a_declaration_at_another_symbol() {
    assert_eq!(unsafe { length_of(c"hello".as_ptr()) }, 5);
    assert_eq!(unsafe { magnitude(-7) }, 7);
}

// ---------------------------------------------------------------------------
// fallthrough, unused and the statement attributes
// ---------------------------------------------------------------------------

c99! {
    int fallthrough_sum(int n) {
        int total = 0;
        switch (n) {
        case 3:
            total += 3;
            __attribute__((fallthrough));
        case 2:
            total += 2;
            __attribute__((fallthrough));
        case 1:
            total += 1;
            break;
        }
        return total;
    }

    int unused_things(int used) {
        int spare __attribute__((unused)) = 0;
        return used;
    }
}

#[test]
fn statement_and_variable_attributes_are_accepted() {
    assert_eq!(unsafe { fallthrough_sum(3) }, 6);
    assert_eq!(unsafe { fallthrough_sum(1) }, 1);
    assert_eq!(unsafe { unused_things(4) }, 4);
}

// ---------------------------------------------------------------------------
// the portability idioms
// ---------------------------------------------------------------------------

mod disabled {
    // `#define __attribute__(x)` is what every portability header writes for a
    // compiler that has no attributes, and it has to keep working: the
    // spelling is an ordinary identifier until the tokens reach the parser, so
    // a macro of that name defines and expands like any other.
    cinrs::c99! {
        #define __attribute__(x)
        struct Ignored { char c; int x; } __attribute__((packed));
        unsigned long size(void) { return sizeof(struct Ignored); }
    }

    #[test]
    fn a_macro_named_attribute_wins() {
        assert_eq!(unsafe { size() }, 8);
    }
}

mod declarators {
    cinrs::c99! {
        /* An attribute after a `typedef`'d record's member list. */
        typedef struct { char c; int x; } __attribute__((packed)) Packed;
        unsigned long packed_size(void) { return sizeof(Packed); }

        /* And one *inside* a declarator, which is where GCC puts a calling
         * convention — the position that used to garble the whole type. */
        typedef int (__attribute__((stdcall)) *Fn)(int);
        static int twice(int n) { return n * 2; }
        int through_a_pointer(int n) { Fn f = twice; return f(n); }
    }
}

#[test]
fn attributes_in_every_position_gcc_accepts_them() {
    assert_eq!(unsafe { declarators::packed_size() }, 5);
    assert_eq!(unsafe { declarators::through_a_pointer(21) }, 42);
}

mod exported {
    /// An `asm` label on a *definition* names the symbol the item takes, which
    /// only means something for a unit that asks for real C symbols at all.
    mod library {
        cinrs::c99! {
            #pragma cinrs export
            int cinrs_gnu_test_add(int a, int b) __asm__("cinrs_gnu_test_plus");
            int cinrs_gnu_test_add(int a, int b) { return a + b; }
        }
    }

    mod user {
        cinrs::c99! {
            int cinrs_gnu_test_plus(int a, int b);
            int use_it(int a, int b) { return cinrs_gnu_test_plus(a, b); }
        }
    }

    #[test]
    fn an_asm_label_renames_an_exported_definition() {
        assert_eq!(unsafe { library::cinrs_gnu_test_add(1, 2) }, 3);
        assert_eq!(unsafe { user::use_it(20, 22) }, 42);
    }
}
