//! C compiled for whatever machine `cargo check --target` names.
//!
//! Two independent computations of the same layout meet here, and the crate
//! compiles only if they agree:
//!
//! * the **C** side, where `cinrs` answers `sizeof` and `_Alignof` from the
//!   `TargetModel` that `CINRS_TARGET` chose, and the `_Static_assert`s below
//!   check those answers against each other and against the predefined macros;
//! * the **Rust** side, where `size_of` and `align_of` are `rustc`'s own for
//!   the real target, and the `const _: () = assert!(…)` items below check the
//!   `#[repr(C)]` item `cinrs` generated — padding and all — against the rule
//!   C's layout algorithm gives.
//!
//! On top of both sits the data-model assertion every expansion opens with,
//! which pins `cinrs`'s widths to `core::ffi`'s. Build this crate without the
//! build script's `CINRS_TARGET` for a 32-bit or a Windows target and that is
//! the assertion that fires; build it with, and everything here has to line
//! up.
//!
//! The C is in string-literal form because it uses `L'x'` and `'\xff'`, which
//! Rust's own lexer refuses.

#![no_std]

use core::ffi::{c_char, c_int, c_long, c_longlong};
use core::mem::{align_of, size_of};

cinrs::c11! { r#"
    #include <stddef.h>
    #include <limits.h>

    /* `long` is four bytes under ILP32 and under LLP64, eight under LP64 —
     * which is the split the whole exercise is about. Written as a test of the
     * predefined macros so that one file serves every target; the assertion
     * that this is the *right* answer for the real machine is the data-model
     * one, over `core::ffi::c_long`. */
    #if defined(_WIN32) || __SIZEOF_POINTER__ == 4
    _Static_assert(sizeof(long) == 4, "ILP32 and LLP64 have a 32-bit long");
    #else
    _Static_assert(sizeof(long) == 8, "LP64 has a 64-bit long");
    #endif

    _Static_assert(sizeof(void *) == __SIZEOF_POINTER__, "");
    _Static_assert(sizeof(wchar_t) == __SIZEOF_WCHAR_T__, "");
    _Static_assert(sizeof(long) == __SIZEOF_LONG__, "");
    _Static_assert(LONG_MAX == __LONG_MAX__, "<limits.h> follows the model");

    /* `wchar_t` is two bytes on Windows and four everywhere else, and `L'x'`
     * has to be the type the header names or a call would not typecheck. */
    #if defined(_WIN32)
    _Static_assert(sizeof(wchar_t) == 2, "");
    #else
    _Static_assert(sizeof(wchar_t) == 4, "");
    #endif
    _Static_assert(sizeof(L'x') == sizeof(wchar_t), "");

    /* Plain `char`'s signedness, which decides what `'\xff'` is worth. */
    #if defined(__CHAR_UNSIGNED__)
    _Static_assert('\xff' > 0, "unsigned plain char");
    #else
    _Static_assert('\xff' < 0, "signed plain char");
    #endif

    /* The layout that separates two targets of the same data model: the i386
     * System V ABI aligns `long long` to four bytes and the Microsoft one to
     * eight, so the padding after `c` is not the same on
     * `i686-unknown-linux-gnu` as on `i686-pc-windows-msvc`. */
    struct probe {
        char c;
        long long ll;
    };
    _Static_assert(sizeof(struct probe) == _Alignof(long long) + 8, "");
    _Static_assert(_Alignof(struct probe) == _Alignof(long long), "");

    struct with_long {
        char c;
        long l;
    };
    _Static_assert(sizeof(struct with_long) == 2 * sizeof(long), "");

    /* The alignment of an atomic type is its *size*, which is the one place a
     * type's alignment does not come from the ABI's table — and the reason
     * `_Atomic long long` needs a target whose eight-byte scalars are
     * eight-byte aligned. `__GCC_HAVE_SYNC_COMPARE_AND_SWAP_8` is where the
     * model says whether this one is. */
    _Static_assert(sizeof(_Atomic int) == sizeof(int), "");
    _Static_assert(_Alignof(_Atomic int) == sizeof(int), "");
    int atomic_bump(_Atomic int *p) { return ++*p; }
    void *atomic_swap(void **p, void *v) { return __atomic_exchange_n(p, v, __ATOMIC_SEQ_CST); }

    #if defined(__GCC_HAVE_SYNC_COMPARE_AND_SWAP_8)
    struct atomic_probe {
        char c;
        _Atomic long long ll;
    };
    _Static_assert(_Alignof(_Atomic long long) == 8, "an atomic type is aligned to its size");
    _Static_assert(_Alignof(struct atomic_probe) == 8, "");
    _Static_assert(sizeof(struct atomic_probe) == 16, "");
    long long atomic_add(_Atomic long long *p, long long v) { return *p += v; }
    #endif

    /* Something for the code generator to chew on, so that the model reaches
     * more than the constant folder: a bit-field window, pointer arithmetic
     * and an array sized from `sizeof`. */
    struct flags {
        unsigned kind : 3;
        unsigned rest : 13;
    };

    static char pointer_sized[sizeof(void *)];

    int probe_offset(void) { return (int) __builtin_offsetof(struct probe, ll); }
    long widen(int x) { return (long) x * 2; }
    unsigned kind_of(struct flags f) { return f.kind; }
    int sizes_agree(void) { return (int) (sizeof(pointer_sized) == sizeof(void *)); }
    long sum(const long *xs, unsigned long n) {
        long total = 0;
        for (unsigned long i = 0; i < n; i++) total += xs[i];
        return total;
    }
"# }

/// C's own layout rule, recomputed from `rustc`'s alignments for the real
/// target: `char` at 0, then the first offset at or after 1 that `long long`
/// is allowed to sit at, then eight bytes, then rounded up to the record's
/// alignment.
const fn probe_size() -> usize {
    let align = align_of::<c_longlong>();
    (align + 8).next_multiple_of(align)
}

/// The `#[repr(C)]` item `cinrs` generated has the size C says it should —
/// which it can only have if the model's `alignof(long long)` was the real
/// one, since the padding is written into the item explicitly.
const _: () = assert!(
    size_of::<probe>() == probe_size(),
    "cinrs laid `struct probe` out for a machine with a different alignment of `long long`"
);

const _: () = assert!(align_of::<probe>() == align_of::<c_longlong>());

const _: () = assert!(size_of::<with_long>() == 2 * size_of::<c_long>());

/// The widths the expansion asserts are also the ones its function signatures
/// use, so this is a second look at the same thing from the Rust side.
const _: () = assert!(size_of::<c_int>() == 4);
const _: () = assert!(size_of::<c_char>() == 1);
