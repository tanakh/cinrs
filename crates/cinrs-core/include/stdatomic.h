/* <stdatomic.h> — atomics (C11 7.17, N1485/N1526).
 *
 * The generic functions are macros over the `__c11_atomic_*` builtins, the way
 * Clang's own header is written: those builtins take a pointer to an `_Atomic`
 * object and, on a pointer object, scale their arithmetic by the size of what
 * is pointed at — which is what 7.17.7.5 requires of `atomic_fetch_add` and
 * what GCC's `__atomic_fetch_add` (which counts in *bytes*) does not do. Both
 * families are available in every entry point; see `doc/gnu-extensions.md`.
 *
 * `__STDC_NO_ATOMICS__` is never predefined; `<threads.h>` has a macro of its
 * own, `__STDC_NO_THREADS__`, predefined only on a target without threads.
 */
#ifndef _CINRS_STDATOMIC_H
#define _CINRS_STDATOMIC_H

#include <stddef.h>
#include <stdint.h>

#define __STDC_VERSION_STDATOMIC_H__ 202311L

/* -- 7.17.1 lock-free property ----------------------------------------- */

/* Every one of them is 2, "lock free for every object of the type": the
 * `core::sync::atomic` types cinrs generates are lock free on every target
 * that has them at all. The values come from the predefined macros so that a
 * program testing either spelling gets one answer. */
#define ATOMIC_BOOL_LOCK_FREE     __GCC_ATOMIC_BOOL_LOCK_FREE
#define ATOMIC_CHAR_LOCK_FREE     __GCC_ATOMIC_CHAR_LOCK_FREE
#define ATOMIC_CHAR8_T_LOCK_FREE  __GCC_ATOMIC_CHAR8_T_LOCK_FREE
#define ATOMIC_CHAR16_T_LOCK_FREE __GCC_ATOMIC_CHAR16_T_LOCK_FREE
#define ATOMIC_CHAR32_T_LOCK_FREE __GCC_ATOMIC_CHAR32_T_LOCK_FREE
#define ATOMIC_WCHAR_T_LOCK_FREE  __GCC_ATOMIC_WCHAR_T_LOCK_FREE
#define ATOMIC_SHORT_LOCK_FREE    __GCC_ATOMIC_SHORT_LOCK_FREE
#define ATOMIC_INT_LOCK_FREE      __GCC_ATOMIC_INT_LOCK_FREE
#define ATOMIC_LONG_LOCK_FREE     __GCC_ATOMIC_LONG_LOCK_FREE
#define ATOMIC_LLONG_LOCK_FREE    __GCC_ATOMIC_LLONG_LOCK_FREE
#define ATOMIC_POINTER_LOCK_FREE  __GCC_ATOMIC_POINTER_LOCK_FREE

/* -- 7.17.2 initialization --------------------------------------------- */

/* Deprecated by C17 and removed by C23, and still defined because a great
 * deal of code writes it. Initialising an atomic object is a plain write:
 * 7.17.2.1p2 says it is not an atomic operation. */
#define ATOMIC_VAR_INIT(value) (value)
#define atomic_init(obj, value) __c11_atomic_init(obj, value)

/* -- 7.17.3 order and consistency -------------------------------------- */

/* The values are GCC's `__ATOMIC_*`, which is what lets either spelling be
 * passed to either family. */
typedef enum memory_order {
    memory_order_relaxed = __ATOMIC_RELAXED,
    memory_order_consume = __ATOMIC_CONSUME,
    memory_order_acquire = __ATOMIC_ACQUIRE,
    memory_order_release = __ATOMIC_RELEASE,
    memory_order_acq_rel = __ATOMIC_ACQ_REL,
    memory_order_seq_cst = __ATOMIC_SEQ_CST
} memory_order;

/* 7.17.3.1: no compiler tracks dependency ordering, and cinrs turns
 * `memory_order_consume` into an acquire, so this is the identity it is
 * everywhere else. */
#define kill_dependency(y) (y)

/* -- 7.17.4 fences ------------------------------------------------------ */

#define atomic_thread_fence(order) __c11_atomic_thread_fence(order)
#define atomic_signal_fence(order) __c11_atomic_signal_fence(order)

/* -- 7.17.5 lock-free property ------------------------------------------ */

#define atomic_is_lock_free(obj) __c11_atomic_is_lock_free(sizeof(*(obj)))

/* -- 7.17.6 atomic integer types ---------------------------------------- */

typedef _Atomic _Bool atomic_bool;
typedef _Atomic char atomic_char;
typedef _Atomic signed char atomic_schar;
typedef _Atomic unsigned char atomic_uchar;
typedef _Atomic short atomic_short;
typedef _Atomic unsigned short atomic_ushort;
typedef _Atomic int atomic_int;
typedef _Atomic unsigned int atomic_uint;
typedef _Atomic long atomic_long;
typedef _Atomic unsigned long atomic_ulong;
__extension__ typedef _Atomic long long atomic_llong;
__extension__ typedef _Atomic unsigned long long atomic_ullong;
typedef _Atomic unsigned short atomic_char16_t;
typedef _Atomic unsigned int atomic_char32_t;
typedef _Atomic __WCHAR_TYPE__ atomic_wchar_t;

#if __STDC_VERSION__ >= 202311L
typedef _Atomic unsigned char atomic_char8_t;
#endif

typedef _Atomic int_least8_t atomic_int_least8_t;
typedef _Atomic uint_least8_t atomic_uint_least8_t;
typedef _Atomic int_least16_t atomic_int_least16_t;
typedef _Atomic uint_least16_t atomic_uint_least16_t;
typedef _Atomic int_least32_t atomic_int_least32_t;
typedef _Atomic uint_least32_t atomic_uint_least32_t;
typedef _Atomic int_least64_t atomic_int_least64_t;
typedef _Atomic uint_least64_t atomic_uint_least64_t;
typedef _Atomic int_fast8_t atomic_int_fast8_t;
typedef _Atomic uint_fast8_t atomic_uint_fast8_t;
typedef _Atomic int_fast16_t atomic_int_fast16_t;
typedef _Atomic uint_fast16_t atomic_uint_fast16_t;
typedef _Atomic int_fast32_t atomic_int_fast32_t;
typedef _Atomic uint_fast32_t atomic_uint_fast32_t;
typedef _Atomic int_fast64_t atomic_int_fast64_t;
typedef _Atomic uint_fast64_t atomic_uint_fast64_t;
typedef _Atomic intptr_t atomic_intptr_t;
typedef _Atomic uintptr_t atomic_uintptr_t;
typedef _Atomic size_t atomic_size_t;
typedef _Atomic ptrdiff_t atomic_ptrdiff_t;
typedef _Atomic intmax_t atomic_intmax_t;
typedef _Atomic uintmax_t atomic_uintmax_t;

/* -- 7.17.7 operations on atomic types ---------------------------------- */

#define atomic_store_explicit(obj, desired, order) \
    __c11_atomic_store(obj, desired, order)
#define atomic_store(obj, desired) \
    __c11_atomic_store(obj, desired, __ATOMIC_SEQ_CST)

#define atomic_load_explicit(obj, order) __c11_atomic_load(obj, order)
#define atomic_load(obj) __c11_atomic_load(obj, __ATOMIC_SEQ_CST)

#define atomic_exchange_explicit(obj, desired, order) \
    __c11_atomic_exchange(obj, desired, order)
#define atomic_exchange(obj, desired) \
    __c11_atomic_exchange(obj, desired, __ATOMIC_SEQ_CST)

#define atomic_compare_exchange_strong_explicit(obj, expected, desired, succ, fail) \
    __c11_atomic_compare_exchange_strong(obj, expected, desired, succ, fail)
#define atomic_compare_exchange_strong(obj, expected, desired) \
    __c11_atomic_compare_exchange_strong(obj, expected, desired, \
                                         __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST)
#define atomic_compare_exchange_weak_explicit(obj, expected, desired, succ, fail) \
    __c11_atomic_compare_exchange_weak(obj, expected, desired, succ, fail)
#define atomic_compare_exchange_weak(obj, expected, desired) \
    __c11_atomic_compare_exchange_weak(obj, expected, desired, \
                                       __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST)

/* On a pointer object these count in *elements*, as 7.17.7.5 requires; the
 * `__c11_atomic_*` builtins are the ones that scale. */
#define atomic_fetch_add_explicit(obj, arg, order) __c11_atomic_fetch_add(obj, arg, order)
#define atomic_fetch_add(obj, arg) __c11_atomic_fetch_add(obj, arg, __ATOMIC_SEQ_CST)
#define atomic_fetch_sub_explicit(obj, arg, order) __c11_atomic_fetch_sub(obj, arg, order)
#define atomic_fetch_sub(obj, arg) __c11_atomic_fetch_sub(obj, arg, __ATOMIC_SEQ_CST)
#define atomic_fetch_or_explicit(obj, arg, order) __c11_atomic_fetch_or(obj, arg, order)
#define atomic_fetch_or(obj, arg) __c11_atomic_fetch_or(obj, arg, __ATOMIC_SEQ_CST)
#define atomic_fetch_xor_explicit(obj, arg, order) __c11_atomic_fetch_xor(obj, arg, order)
#define atomic_fetch_xor(obj, arg) __c11_atomic_fetch_xor(obj, arg, __ATOMIC_SEQ_CST)
#define atomic_fetch_and_explicit(obj, arg, order) __c11_atomic_fetch_and(obj, arg, order)
#define atomic_fetch_and(obj, arg) __c11_atomic_fetch_and(obj, arg, __ATOMIC_SEQ_CST)

/* -- 7.17.8 atomic flag type and operations ----------------------------- */

/* The one type the standard guarantees is lock free. The member is named the
 * way Clang's header names it — a program may not touch it, and giving it a
 * reserved name is what says so. */
typedef struct atomic_flag {
    atomic_bool _Value;
} atomic_flag;

#define ATOMIC_FLAG_INIT { 0 }

#define atomic_flag_test_and_set_explicit(obj, order) \
    __c11_atomic_exchange(&(obj)->_Value, 1, order)
#define atomic_flag_test_and_set(obj) \
    __c11_atomic_exchange(&(obj)->_Value, 1, __ATOMIC_SEQ_CST)
#define atomic_flag_clear_explicit(obj, order) \
    __c11_atomic_store(&(obj)->_Value, 0, order)
#define atomic_flag_clear(obj) \
    __c11_atomic_store(&(obj)->_Value, 0, __ATOMIC_SEQ_CST)

#endif /* _CINRS_STDATOMIC_H */
