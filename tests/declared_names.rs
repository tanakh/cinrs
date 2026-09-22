//! What a unit **declares** and does not define, as Rust sees it.
//!
//! A function comes out under its own C name, `pub` and glob re-exported like
//! a definition, so `#include <string.h>` is all Rust needs to call `strlen`:
//! the declaration in the generated `extern` block *is* the item, pointed at
//! its symbol with `#[link_name]`. There is no `bindgen` step because a
//! header's declarations are already the binding.
//!
//! An **object** is the one exception. Rust resolves a binding pattern against
//! the value namespace first, and a `static` there is not a name a `let` may
//! shadow — `let stdout = std::io::stdout();` beside a block that includes
//! `<stdio.h>` would be `error[E0530]: let bindings cannot shadow statics` — so
//! a declared-only object keeps a hidden name and Rust reaches it the way C
//! code does in the same situation, through an accessor the block writes.
//!
//! Two units in one Rust scope that both declare a name are not a problem
//! until *Rust* uses it, which is `E0659`; a `mod` around one of them is the
//! answer, and `tests/units.rs` is where that lives.

use cinrs::c99;

// ---------------------------------------------------------------------------
// the bundled headers, called from Rust by name
// ---------------------------------------------------------------------------

/// Five C library functions from four headers, called from Rust with nothing
/// between them and it: no wrapper in the C, no `mod` around the block.
#[test]
fn what_a_bundled_header_declares_is_callable_from_rust() {
    c99! {
        #include <string.h>
        #include <stdlib.h>
        #include <ctype.h>
        #include <math.h>
    }

    unsafe {
        assert_eq!(strlen(c"hello".as_ptr()), 5);
        assert_eq!(abs(-7), 7);
        assert_eq!(atoi(c"42".as_ptr()), 42);
        assert_eq!(toupper(i32::from(b'q')), i32::from(b'Q'));
        assert_eq!(sqrt(16.0), 4.0);
    }
}

/// The same five through a `mod`, which is the form to reach for when a second
/// unit in the same scope would declare any of them too.
mod library {
    cinrs::c99! {
        #include <string.h>
        #include <stdlib.h>
        #include <ctype.h>
        #include <math.h>
    }
}

#[test]
fn a_mod_gives_the_declarations_a_path() {
    unsafe {
        assert_eq!(library::strlen(c"hello".as_ptr()), 5);
        assert_eq!(library::abs(-7), 7);
        assert_eq!(library::atoi(c"42".as_ptr()), 42);
        assert_eq!(library::toupper(i32::from(b'q')), i32::from(b'Q'));
        assert_eq!(library::sqrt(16.0), 4.0);
    }
}

// ---------------------------------------------------------------------------
// two units in one Rust scope
// ---------------------------------------------------------------------------

/// Two blocks in one Rust module, both including the same headers, both calling
/// `printf` and `strlen` from their own C. Nothing is wrong: each unit sees only
/// its own declarations, and the glob re-exports collide only where *Rust* uses
/// a name both of them export.
///
/// `tests/ui/two_units_declare_one_name.rs` is the other half — what Rust
/// naming `strlen` here would be, and the `mod` that answers it.
mod two_units {
    cinrs::c99! {
        #include <stdio.h>
        #include <string.h>

        int first_length(const char *s) {
            if (s[0] == '\0') printf("");
            return (int) strlen(s);
        }
    }

    cinrs::c99! {
        #include <stdio.h>
        #include <string.h>
        #include <ctype.h>

        int second_length(const char *s) {
            if (s[0] == '\0') printf("");
            return (int) strlen(s) * 2;
        }
    }

    #[test]
    fn nothing_is_wrong_until_rust_uses_a_shared_name() {
        unsafe {
            assert_eq!(first_length(c"abc".as_ptr()), 3);
            assert_eq!(second_length(c"abc".as_ptr()), 6);
            // `toupper` is declared by only one of the two, so Rust may name it.
            assert_eq!(toupper(i32::from(b'q')), i32::from(b'Q'));
        }
    }
}

// ---------------------------------------------------------------------------
// a user header, and the unit that implements it
// ---------------------------------------------------------------------------

/// The implementation: the same header, plus definitions for what it declares,
/// and `#pragma cinrs export` so that the definitions are real C symbols.
mod tally_impl {
    cinrs::c99! {
        #pragma cinrs export
        #include "include/library.h"

        int cinrs_test_tally_add(struct Tally *t, int n) {
            t->count++;
            t->total += n;
            return t->total;
        }

        int cinrs_test_tally_mean(struct Tally t) {
            return t.count == 0 ? 0 : t.total / t.count;
        }

        int yield(int n) { return n * CINRS_TEST_LIBRARY_SCALE; }
    }
}

/// The client: the header and nothing else, which is the shape of `#include
/// <zlib.h>`. Every function in it is a declaration, so every function in it
/// is an item of this module that Rust can name.
mod tally {
    cinrs::c99! {
        #include "include/library.h"
    }
}

#[test]
fn a_header_is_the_binding() {
    let mut t = tally::Tally { count: 0, total: 0 };
    unsafe {
        assert_eq!(tally::cinrs_test_tally_add(&raw mut t, 4), 4);
        assert_eq!(tally::cinrs_test_tally_add(&raw mut t, 8), 12);
        assert_eq!(tally::cinrs_test_tally_mean(t), 6);
        // A C name that is a Rust keyword is a raw identifier, on the
        // declaration side exactly as on the definition side.
        assert_eq!(tally::r#yield(5), 15);
    }
    assert_eq!((t.count, t.total), (2, 12));
}

// ---------------------------------------------------------------------------
// a local named like a declared function
// ---------------------------------------------------------------------------

/// C lets an object have the name of a function declared in the same unit, and
/// so does Rust: a function is not a pattern, so the binding shadows it rather
/// than colliding with it. The `<time.h>` case is the one to watch, because
/// `time` is also one of the names the Microsoft C runtime exports under
/// another spelling.
#[test]
fn a_local_may_carry_a_declared_functions_name() {
    c99! {
        #include <stdlib.h>
        #include <time.h>

        int shadowing_local(void) {
            int abs = 3;            /* <stdlib.h> declares `abs` */
            return abs + 1;
        }

        long shadowing_param(long time) {   /* <time.h> declares `time` */
            return time * 2;
        }

        /* And the functions themselves are still there for the C to call. */
        int calls_them(void) { return abs(-5) + (time(0) != (time_t)-1); }
    }

    unsafe {
        assert_eq!(shadowing_local(), 4);
        assert_eq!(shadowing_param(21), 42);
        assert_eq!(calls_them(), 6);
    }
}

// ---------------------------------------------------------------------------
// two C names, one symbol
// ---------------------------------------------------------------------------

/// An `__asm__` label gives a declaration a symbol of its own, so a unit may
/// reach one function under two names. Both are items now, and both work;
/// `#[link_name]` is what makes them one symbol.
#[test]
fn two_names_may_point_at_one_symbol() {
    c99! {
        #include <stdlib.h>

        int magnitude(int n) __asm__("abs");

        int both(int n) { return magnitude(n) + abs(n); }
    }

    unsafe {
        assert_eq!(both(-4), 8);
        assert_eq!(magnitude(-9), 9);
        assert_eq!(abs(-9), 9);
    }
}

/// The other direction: a function the generated code needs for *itself* must
/// not be declared twice when the program declares it too. `__builtin_trap` is
/// `abort()`, and this unit declares `abort` — one item, not two, or the
/// expansion would not compile at all.
#[test]
fn a_builtin_and_the_programs_own_declaration_are_one_item() {
    c99! {
        void abort(void);

        int never_returns(void) { __builtin_trap(); }
        int reachable(int n) { return n; }
    }

    assert_eq!(unsafe { reachable(7) }, 7);
}

// ---------------------------------------------------------------------------
// a declared object
// ---------------------------------------------------------------------------

/// An object the unit declares and does not define keeps a hidden name, so a
/// `let` of the same name in the surrounding module is an ordinary binding.
///
/// `stdout` is what the bundled `<stdio.h>` declares on a platform whose C
/// library exports it as an object; `timezone` and `daylight` are glibc's, and
/// reach a unit through `#pragma cinrs system_include`. None of them may stop a
/// `let`, which is the whole reason the name stays hidden.
mod declared_objects {
    cinrs::c99! {
        #include <stdio.h>
        #include <time.h>

        /* The C reads the objects by their C names, which is what the hidden
         * Rust name does not change. */
        int has_the_standard_streams(void) {
            return stdout != 0 && stderr != 0;
        }
    }

    #[test]
    fn a_declared_object_is_not_in_a_lets_way() {
        let stdout = 1;
        let timezone = 2;
        let daylight = 3;
        assert_eq!(stdout + timezone + daylight, 6);
        assert_eq!(unsafe { has_the_standard_streams() }, 1);
    }
}

/// The same with an object of the unit's **own**, so that the rule is tested
/// where the bundled headers happen not to declare one: `stdout` is
/// `__acrt_iob_func(1)` on Windows and `__stdoutp` on Apple.
///
/// The declaring unit includes `<stdio.h>`, which is deliberate: that header is
/// what makes an MSVC target link `legacy_stdio_definitions` for the `printf`
/// family, and `#[link(name = "…")]` is also what would make `rustc` reach every
/// `static` in the block it sits on through a `dllimport` — reading rubbish for
/// a symbol defined in *this* image, with `lld-link` saying `LNK4217: locally
/// defined symbol imported`. cinrs therefore puts every library it links on an
/// `extern` block of its own, with nothing in it; this is the test that says so
/// on the platform. See `doc/cross-compilation.md`.
mod a_declared_object_of_our_own {
    cinrs::c99! {
        #include <stdio.h>

        extern int cinrs_test_shared_object;

        int read_shared_object(void) { return cinrs_test_shared_object; }
    }

    mod object_owner {
        cinrs::c99! {
            #pragma cinrs export
            int cinrs_test_shared_object = 17;
        }
    }

    #[test]
    fn the_name_is_free_and_the_c_still_reads_the_object() {
        let cinrs_test_shared_object = 4;
        assert_eq!(cinrs_test_shared_object, 4);
        assert_eq!(unsafe { read_shared_object() }, 17);
    }
}

/// The same shared object in a unit that names a library **itself**, and the
/// other end of the rule: a data symbol a real DLL exports.
///
/// `#pragma cinrs link` is the program's own statement, and it too puts the
/// library on a block with nothing in it, so the object another unit of this
/// crate defines is an ordinary symbol rather than a `dllimport`. The price is
/// that a DLL's *data* export — the one kind of symbol an import library holds
/// as `__imp_name` alone — has to be named through that pointer, which is what
/// a C program with no `__declspec(dllimport)` does as well. Both halves are
/// MSVC-only: `doc/cross-compilation.md` has the measurements, and the link
/// attribute is a no-op on the platforms where it is not needed.
#[cfg(all(windows, target_env = "msvc"))]
mod a_unit_that_names_a_library {
    /// `gdi32.lib` exports the `DWORD` `GdiBatchLimit` as data: it holds
    /// `__imp_GdiBatchLimit` and no thunk, which is why a plain declaration of
    /// it is `LNK2019: unresolved external symbol` and the pointer is the way in.
    /// The value is the DLL's business — 20 by default, and this only reads it.
    mod gdi {
        cinrs::c99! {
            #pragma cinrs link "gdi32"

            extern int cinrs_test_pragma_object;
            extern unsigned long *cinrs_test_batch_limit __asm__("__imp_GdiBatchLimit");

            int read_pragma_object(void) { return cinrs_test_pragma_object; }
            unsigned long read_batch_limit(void) { return *cinrs_test_batch_limit; }
        }

        mod object_owner {
            cinrs::c99! {
                #pragma cinrs export
                int cinrs_test_pragma_object = 17;
            }
        }
    }

    #[test]
    fn the_object_is_this_crates_and_the_dlls_data_is_the_dlls() {
        assert_eq!(unsafe { gdi::read_pragma_object() }, 17);
        assert_ne!(unsafe { gdi::read_batch_limit() }, 0);
    }
}

/// How Rust reaches such an object: an accessor written in the block, which is
/// a two-line C function and the same thing a C program writes when it wants
/// one translation unit to hand another a `FILE *`.
#[test]
fn an_accessor_hands_a_declared_object_to_rust() {
    c99! {
        #include <stdio.h>

        FILE *cinrs_test_get_stdout(void) { return stdout; }

        int write_nothing(FILE *stream) { return fputs("", stream); }
    }

    unsafe {
        let out = cinrs_test_get_stdout();
        assert!(!out.is_null());
        assert!(write_nothing(out) >= 0);
    }
}
