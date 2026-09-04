//! The eight entry points: four revisions, each strict or GNU.
//!
//! The line between them is GCC's own. Everything spelled with a leading
//! double underscore is available either way, because those names are
//! reserved; the plain spellings `typeof` and `asm` need a GNU dialect, and
//! only there is a construct a later revision introduced accepted without a
//! diagnostic naming the macro to write instead.

use cinrs::{c17, c23, gnu11, gnu17, gnu23, gnu99};

gnu99! {
    /* Every one of these needs a later revision than C99, and `gnu99!` takes
     * them all — exactly as `gcc -std=gnu99` does. */
    _Static_assert(sizeof(int) >= 4, "int is at least 32 bits");

    struct Tagged {
        int tag;
        union { int as_int; double as_double; };   /* C11: anonymous member */
    };

    int kind(int n) { return _Generic(n, int: 1, double: 2, default: 0); }
    unsigned long alignment(void) { return _Alignof(double); }
    int binary(void) { return 0b1010; }
    int empty_init(void) { struct Tagged t = {}; return t.tag; }

    _Noreturn void gnu99_never(void);
    void gnu99_never(void) { for (;;) { } }

    typeof(int) plain(int n) { return n; }
    int tagged_int(struct Tagged t) { return t.as_int; }
}

gnu11! {
    int gnu11_typeof(int n) { typeof(n) m = n; return m; }
    int gnu11_binary(void) { return 0b11; }
}

gnu17! {
    int gnu17_version(void) { return (int) (__STDC_VERSION__ / 100L); }
}

gnu23! {
    /* A GNU dialect of C23 still has everything C23 has. */
    constexpr int GNU23_LIMIT = 4;
    static_assert(GNU23_LIMIT == 4);
    int gnu23_limit(void) { return GNU23_LIMIT; }
    typeof(int) gnu23_typeof(int n) { return n; }
}

c23! {
    /* `typeof` is C23's own keyword, so the strict entry point has it too. */
    typeof(int) c23_typeof(int n) { return n; }
}

c17! {
    /* And here it is an ordinary identifier, which is the whole reason the
     * strict entry points cannot have it. */
    int typeof_is_a_variable(void) {
        int typeof = 3;
        int asm = 4;
        return typeof + asm;
    }
}

#[test]
fn a_gnu_dialect_accepts_what_a_later_revision_added() {
    assert_eq!(unsafe { kind(0) }, 1);
    assert_eq!(unsafe { alignment() }, align_of::<f64>() as u64);
    assert_eq!(unsafe { binary() }, 10);
    assert_eq!(unsafe { empty_init() }, 0);
    assert_eq!(unsafe { plain(7) }, 7);
    let t = Tagged {
        tag: 1,
        __cinrs_anon0: unsafe { core::mem::zeroed() },
    };
    assert_eq!(unsafe { tagged_int(t) }, 0);
}

#[test]
fn every_gnu_entry_point_exists() {
    assert_eq!(unsafe { gnu11_typeof(5) }, 5);
    assert_eq!(unsafe { gnu11_binary() }, 3);
    assert_eq!(unsafe { gnu17_version() }, 2017);
    assert_eq!(unsafe { gnu23_limit() }, 4);
    assert_eq!(unsafe { gnu23_typeof(9) }, 9);
    assert_eq!(unsafe { c23_typeof(9) }, 9);
    assert_eq!(unsafe { typeof_is_a_variable() }, 7);
}

// ---------------------------------------------------------------------------
// what both dialects share
// ---------------------------------------------------------------------------

use cinrs::c99;

c99! {
    /* The `__`-spelled extensions are available in a strict entry point, and
     * this is the point of the policy: a program that guards them with
     * `#if defined(__GNUC__)` gets them either way. */
    __typeof__(int) strict_typeof(int n) { return n; }
    int strict_stmt_expr(int n) { return ({ int t = n; t * 2; }); }
    int strict_attribute(int n) __attribute__((const));
    int strict_attribute(int n) { return n; }
    int strict_builtin(unsigned int n) { return __builtin_popcount(n); }
    unsigned long strict_alignof(void) { return __alignof__(long); }
}

#[test]
fn the_double_underscore_extensions_need_no_gnu_entry_point() {
    assert_eq!(unsafe { strict_typeof(1) }, 1);
    assert_eq!(unsafe { strict_stmt_expr(21) }, 42);
    assert_eq!(unsafe { strict_attribute(3) }, 3);
    assert_eq!(unsafe { strict_builtin(7) }, 3);
    assert_eq!(
        unsafe { strict_alignof() },
        align_of::<core::ffi::c_long>() as u64
    );
}
