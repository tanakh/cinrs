/* <stdarg.h> — variable arguments (C99 7.15).
 *
 * `__builtin_va_list` is the one name cinrs knows on its own; it becomes
 * Rust's `core::ffi::VaList`. Everything the standard spells is defined from
 * it here, exactly as GCC's own <stdarg.h> does, so `va_list` and friends are
 * ordinary identifiers in a translation unit that does not include this
 * header.
 *
 * Note that *defining* a variadic function needs Rust 1.99 or later, which is
 * where `c_variadic` was stabilised; declaring and calling one has always
 * worked.
 */
#ifndef _CINRS_STDARG_H
#define _CINRS_STDARG_H

typedef __builtin_va_list va_list;
typedef __builtin_va_list __gnuc_va_list;

#define va_start(ap, parmN) __builtin_va_start(ap, parmN)
#define va_arg(ap, type) __builtin_va_arg(ap, type)
#define va_end(ap) __builtin_va_end(ap)
#define va_copy(dest, src) __builtin_va_copy(dest, src)
#define __va_copy(dest, src) __builtin_va_copy(dest, src)

#endif /* _CINRS_STDARG_H */
