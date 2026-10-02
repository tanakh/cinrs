/* <stdarg.h> — variable arguments (C99 7.15, C23 7.16).
 *
 * `__builtin_va_list` is the one name cinrs knows on its own; it becomes
 * Rust's `core::ffi::VaList`. Everything the standard spells is defined from
 * it here, exactly as GCC's own <stdarg.h> does, so `va_list` and friends are
 * ordinary identifiers in a translation unit that does not include this
 * header.
 *
 * C23's `va_start` takes the list alone — what follows it is never evaluated,
 * so a function with nothing before its `...` can call it too — and is GCC
 * 15's `__builtin_c23_va_start`. Before C23 it names the last parameter.
 */
#ifndef _CINRS_STDARG_H
#define _CINRS_STDARG_H

typedef __builtin_va_list va_list;
typedef __builtin_va_list __gnuc_va_list;

#if __STDC_VERSION__ >= 202311L
#define va_start(...) __builtin_c23_va_start(__VA_ARGS__)
#else
#define va_start(ap, parmN) __builtin_va_start(ap, parmN)
#endif
#define va_arg(ap, type) __builtin_va_arg(ap, type)
#define va_end(ap) __builtin_va_end(ap)
#define va_copy(dest, src) __builtin_va_copy(dest, src)
#define __va_copy(dest, src) __builtin_va_copy(dest, src)

#endif /* _CINRS_STDARG_H */
