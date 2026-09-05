/* <stddef.h> — common definitions (C99 7.17).
 *
 * One of the bundled headers cinrs ships instead of reading the platform's
 * own: see the crate documentation for why. The widths below come from the
 * target description macros the preprocessor predefines, so the same text
 * serves every data model cinrs knows about.
 */
#ifndef _CINRS_STDDEF_H
#define _CINRS_STDDEF_H

/* `__SIZE_TYPE__` and its relatives are what the front end itself uses for
 * these types, predefined from the target model, so writing the typedefs in
 * terms of them is what keeps `sizeof` and `size_t` the same type on every
 * target: `unsigned int` on i686, `unsigned long` on LP64, `unsigned long
 * long` on 64-bit Windows. `__extension__` says "an extension, and I know it",
 * so that a `c89!` block including this one is not told off for the `long
 * long` the last of those expands to. */
__extension__ typedef __SIZE_TYPE__ size_t;
__extension__ typedef __PTRDIFF_TYPE__ ptrdiff_t;

/* `wchar_t` is `int` on the Unix platforms, `unsigned int` on Arm and
 * `unsigned short` on Windows; `__WCHAR_TYPE__` is whichever this target has,
 * and is the same type the front end gives `L'x'` and `L"…"`. */
typedef __WCHAR_TYPE__ wchar_t;

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
