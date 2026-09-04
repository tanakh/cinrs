/* <stddef.h> — common definitions (C99 7.17).
 *
 * One of the bundled headers cinrs ships instead of reading the platform's
 * own: see the crate documentation for why. The widths below come from the
 * target description macros the preprocessor predefines, so the same text
 * serves every data model cinrs knows about.
 */
#ifndef _CINRS_STDDEF_H
#define _CINRS_STDDEF_H

#if __SIZEOF_POINTER__ == __SIZEOF_LONG__
typedef unsigned long size_t;
typedef long ptrdiff_t;
#elif __SIZEOF_POINTER__ == __SIZEOF_LONG_LONG__
typedef unsigned long long size_t;
typedef long long ptrdiff_t;
#else
typedef unsigned int size_t;
typedef int ptrdiff_t;
#endif

/* cinrs gives a wide character constant the type `int` on every target, which
 * is what the Unix platforms do and what MSVC does not — there `wchar_t` is 16
 * bits wide. The two agree everywhere cinrs is tested. */
typedef int wchar_t;

#define NULL ((void *)0)

/* `offsetof` is a compiler builtin here: cinrs turns it into Rust's own
 * `core::mem::offset_of!`, so the answer is the offset the generated `struct`
 * really has rather than one worked out separately.
 *
 * Two things follow. A nested member designator (`a.b`, `a[0]`) is not
 * supported, and neither is using `offsetof` where C99 wants an integer
 * constant expression — as an array bound, a `case` label or the initialiser
 * of a file-scope object — because the value is Rust's to compute. Writing it
 * inside a function works everywhere. */
#define offsetof(type, member) __builtin_offsetof(type, member)

#if __STDC_VERSION__ >= 202311L
/* C23's `nullptr_t`. cinrs gives `nullptr` the type `void *` rather than a
 * type of its own, so this is what the name stands for; the difference shows
 * only in the places C distinguishes them, such as `_Generic`. */
typedef void *nullptr_t;

/* `unreachable()` promises control never gets here, and cinrs takes the
 * promise literally: it becomes `core::hint::unreachable_unchecked()`, so
 * reaching it is undefined behaviour exactly as C says it is. */
#define unreachable() __builtin_unreachable()
#endif

#endif /* _CINRS_STDDEF_H */
