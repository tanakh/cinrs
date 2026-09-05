//! C that names its types after Rust's primitives, which is most C.
//!
//! `typedef _Bool bool;` is the first line of every C23 compatibility shim,
//! `typedef unsigned int u32;` and `typedef long long i64;` are how a great
//! deal of embedded and kernel C spells its types, and `typedef unsigned long
//! usize;` is not rare either. Each of those becomes a Rust `pub type` item in
//! the module the unit expands into — so a code generator that wrote a bare
//! `bool`, `u32` or `usize` anywhere in that module would either produce a
//! cycle (`pub type bool = bool;` is `E0391`, which is what gcc.c-torture's
//! `execute/20030714-1` used to hit) or silently pick up the C type where it
//! meant the primitive: the padding of a `struct`, a bit-field accessor's
//! window, a pointer difference, the length of a variable length array.
//!
//! Every primitive the code generator writes therefore goes through
//! `::core::primitive::…`. These tests are the proof: each block redefines as
//! many primitive names as C allows *and* exercises the constructs that emit
//! them, so a regression is a compile error rather than a subtly wrong answer.
//!
//! `char`, `str` and `f32` cannot be `typedef` names — `char` is a C keyword,
//! and the other two are simply not C types — so what is checked for those is
//! the other half of the hazard: an *object* may be called `str`, and the
//! `include_str!` items the expansion emits for rebuild tracking live in the
//! same module.

use cinrs::{c11, c23, gnu11};

/// The whole set of names at once, over the constructs that emit primitives.
///
/// The `struct` has padding (`[u8; N]`) and bit-fields (a `u64` window and
/// `u8` stores); `count` uses the bit-counting builtins (`u32`/`u64`); `sum`
/// does pointer arithmetic (`isize`); `pick` casts a pointer to an integer
/// (`usize`); `nan_is_nan` needs `f64::NAN`.
#[test]
fn every_primitive_name_is_a_typedef() {
    gnu11! {
        typedef _Bool bool;
        typedef unsigned char u8;
        typedef unsigned short u16;
        typedef unsigned int u32;
        typedef unsigned long long u64;
        typedef unsigned long usize;
        typedef signed char i8;
        typedef short i16;
        typedef int i32;
        typedef long long i64;
        typedef long isize;
        typedef double f64;
        typedef float f32;
        typedef unsigned __int128 u128;
        typedef __int128 i128;

        struct packet {
            u8 tag;
            u64 payload;
            u32 flags : 5;
            u32 kind : 27;
            i32 delta : 9;
        };

        usize packet_size(void) { return sizeof(struct packet); }

        u32 roundtrip(u32 flags, i32 delta) {
            struct packet p;
            p.flags = flags;
            p.delta = delta;
            return (u32) (p.flags + (u32) (p.delta + 512));
        }

        u32 count(u64 v) {
            return (u32) (__builtin_popcountll(v) + __builtin_clzll(v | 1)
                          + __builtin_ctzll(v | 1));
        }

        i64 sum(const i32 *values, usize n) {
            i64 total = 0;
            for (usize i = 0; i < n; i++) total += values[i];
            return total;
        }

        usize pick(const void *p) { return (usize) p != 0; }

        bool nan_is_nan(void) {
            f64 x = __builtin_nan("");
            return x != x;
        }

        bool truthy(bool b) { return !b; }

        u128 widen(u64 a, u64 b) { return (u128) a * b; }

        i8 narrow(i64 v) { return (i8) v; }

        f32 half(f32 x) { return x / 2.0f; }
    }

    unsafe {
        // The `struct` laid out with its padding and bit-field storage. The
        // qualified `usize` is needed on *this* side too: the glob re-export
        // has brought the C `usize` into the block, which is the hazard seen
        // from the outside.
        assert_eq!(
            packet_size() as ::core::primitive::usize,
            core::mem::size_of::<packet>()
        );
        assert_eq!(roundtrip(17, -3), 17 + 509);
        // popcount(0xff) + clz(0xff) + ctz(0xff) = 8 + 56 + 0
        assert_eq!(count(0xff), 8 + 56);
        let values: [i32; 4] = [1, -2, 30, 400];
        assert_eq!(sum(values.as_ptr(), 4), 429);
        assert_eq!(pick(values.as_ptr().cast()), 1);
        assert!(nan_is_nan());
        assert!(truthy(false));
        assert_eq!(widen(u64::from(u32::MAX), 3), u128::from(u32::MAX) * 3);
        assert_eq!(narrow(0x1ff), -1);
        assert_eq!(half(3.0), 1.5);
    }
}

/// The typedefs really do become items of those names, and Rust code can use
/// them: the point is that the generated code stops referring to them, not
/// that they stop existing.
#[test]
fn the_typedefs_are_usable_from_rust() {
    c11! {
        #pragma cinrs module "primitive_aliases"
        typedef _Bool bool;
        typedef unsigned int u32;
        typedef unsigned long usize;
        typedef double f64;

        u32 twice(u32 x) { return x * 2; }
        bool even(u32 x) { return x % 2 == 0; }
        f64 scale(f64 x, usize n) { return x * (f64) n; }
    }

    let x: primitive_aliases::u32 = 21;
    let doubled: primitive_aliases::u32 = unsafe { twice(x) };
    assert_eq!(doubled, 42);
    let flag: primitive_aliases::bool = unsafe { even(doubled) };
    assert!(flag);
    let scaled: primitive_aliases::f64 = unsafe { scale(1.5, 4) };
    assert_eq!(scaled, 6.0);
}

/// `bool` under `c23!`, where it is a keyword rather than a typedef, and under
/// `c11!` through `<stdbool.h>`, where it is a macro for `_Bool`. Neither can
/// collide, but both emit the Rust `bool`, so both belong here.
#[test]
fn bool_from_the_keyword_and_from_the_header() {
    c23! {
        bool both(bool a, bool b) { return a && b; }
    }
    c11! {
        #include <stdbool.h>
        bool either(bool a, bool b) { return a || b; }
    }
    unsafe {
        assert!(both(true, true));
        assert!(!both(true, false));
        assert!(either(false, true));
    }
}

/// A variable-length array and `alloca`, which emit `usize`, `u128` and the
/// `Vec` they live in — with `usize` and `u128` taken over by typedefs.
#[test]
fn a_vla_and_alloca_with_the_names_taken() {
    gnu11! {
        #include <alloca.h>
        typedef unsigned long usize;
        typedef unsigned __int128 u128;
        typedef unsigned char u8;

        int vla_sum(int n) {
            int values[n];
            int total = 0;
            for (int i = 0; i < n; i++) values[i] = i * i;
            for (int i = 0; i < n; i++) total += values[i];
            return total;
        }

        int alloca_sum(usize n) {
            u8 *buffer = (u8 *) alloca(n);
            int total = 0;
            for (usize i = 0; i < n; i++) buffer[i] = (u8) i;
            for (usize i = 0; i < n; i++) total += buffer[i];
            return total;
        }
    }

    unsafe {
        assert_eq!(vla_sum(5), (0..5).map(|i| i * i).sum::<i32>());
        assert_eq!(alloca_sum(10), (0..10).sum::<i32>());
    }
}

/// An object called `str`, and a header the expansion tracks for rebuilds —
/// the `const _: &str = include_str!(…)` item goes into this very module, so
/// its `str` has to be the primitive rather than anything the C introduced.
#[test]
fn an_object_named_str_next_to_a_tracked_header() {
    c11! {
        #include "include/point.h"
        static char *str = "hello";
        static struct Point origin = { POINT_ORIGIN_X, POINT_ORIGIN_Y };

        int str_length(void) {
            int n = 0;
            while (str[n]) n++;
            return n;
        }

        int origin_is_zero(void) { return origin.x == 0 && origin.y == 0; }
    }
    unsafe {
        assert_eq!(str_length(), 5);
        assert_eq!(origin_is_zero(), 1);
    }
}

/// `goto`, which turns the body into a state machine whose state variable is a
/// `u32`, in a unit that has taken `u32` for itself.
#[test]
fn a_state_machine_with_u32_taken() {
    gnu11! {
        typedef unsigned int u32;
        typedef unsigned long usize;

        u32 collatz(u32 n) {
            u32 steps = 0;
        top:
            if (n == 1) goto done;
            n = (n % 2 == 0) ? n / 2 : 3 * n + 1;
            steps++;
            goto top;
        done:
            return steps;
        }

        usize find(const u32 *xs, usize n, u32 want) {
            usize i = 0;
        again:
            if (i == n) goto missing;
            if (xs[i] == want) return i;
            i++;
            goto again;
        missing:
            return n;
        }
    }
    unsafe {
        assert_eq!(collatz(27), 111);
        let xs: [u32; 4] = [3, 5, 8, 13];
        assert_eq!(find(xs.as_ptr(), 4, 8), 2);
        assert_eq!(find(xs.as_ptr(), 4, 9), 4);
    }
}

/// An incomplete type and an empty `union`, both of which get a `[u8; 0]`
/// field, in a unit whose `u8` is something else.
#[test]
fn the_zero_length_fields_with_u8_taken() {
    gnu11! {
        typedef signed char u8;
        struct opaque;
        union empty { };

        int is_null(const struct opaque *p) { return p == 0; }
        unsigned long empty_size(void) { return sizeof(union empty); }
        u8 minus_one(void) { return (u8) -1; }
    }
    unsafe {
        assert_eq!(is_null(core::ptr::null()), 1);
        assert_eq!(empty_size(), 0);
        assert_eq!(minus_one(), -1);
    }
}
