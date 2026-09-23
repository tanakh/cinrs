//@check-pass
//@compile-flags: --crate-type lib
//! Everything `cinrs` generates is `core`-only, so an expansion compiles into
//! a `#![no_std]` crate as it stands.
//!
//! The two exceptions are the variable length array and the `alloca`
//! emulations, which allocate; `no_std_alloc.rs` is those two with
//! `#pragma cinrs no_std` and an `alloc` crate to take them from. Everything
//! else is here: records and bit-fields, raw pointers, `goto` (a graph of
//! basic blocks), a `switch` with fallthrough, string literals, a
//! compound literal, a statement expression, `__builtin_*` forms,
//! `__attribute__((constructor))` and calls into the C library through the
//! bundled headers — the library is linked, but no Rust `std` is.

#![no_std]

cinrs::gnu99! {
    #include <stdio.h>
    #include <string.h>
    #include <stdarg.h>
    #include <assert.h>
    #include <stddef.h>

    struct Flags { unsigned int ready : 1; int level : 3; };
    struct Point { int x; int y; };

    static int started = 0;

    __attribute__((constructor)) static void begin(void) { started = 1; }

    int describe(char *buf, unsigned long size, const char *name) {
        return snprintf(buf, size, "%s has %d letters", name, (int)strlen(name));
    }

    int classify(int c) {
        switch (c) {
        case '0' ... '9': return 1;
        case 'a' ... 'z': return 2;
        default: return 0;
        }
    }

    int jumpy(int n) {
        int total = 0;
    again:
        total += n;
        if (--n > 0) goto again;
        return total;
    }

    /* `assert`, `__builtin_trap` and `unreachable()` are the three places a
       translation could reach for `std`; the first two call the C library's
       own `abort` and the third is `core::hint`. */
    int diagnostics(int n) {
        assert(n >= 0);
        if (n == 7) __builtin_trap();
        if (n == 8) __builtin_unreachable();
        return n + (int)offsetof(struct Point, y);
    }

    int flags_and_literals(int level) {
        struct Flags f = { 1, 0 };
        f.level = level;
        struct Point *p = &(struct Point){ 1, 2 };
        int biggest = ({ int a = p->x; int b = p->y; a > b ? a : b; });
        const char *text = "no_std";
        return (int)f.ready + f.level + biggest + __builtin_popcount(7)
             + (int)text[0] + started + classify('7') + jumpy(3)
             + diagnostics(1);
    }
}

// Nothing here uses `std` either: the expansion is the point.
pub fn call(level: core::ffi::c_int) -> core::ffi::c_int {
    unsafe { flags_and_literals(level) }
}
