/* <setjmp.h> — non-local jumps, as a Rust unwind.
 *
 * `longjmp` does not restore a machine context here: it unwinds, Rust's way,
 * to the function that called `setjmp`, which runs its body again from the
 * `setjmp` the buffer names. What `setjmp` writes into a buffer is cinrs's
 * own, so a `jmp_buf` is only an array of words, large enough for it and for
 * a saved signal mask. A `setjmp` may only stand where C17 7.13.1.1p4 says,
 * and every function between a `longjmp` and its `setjmp` has to be one cinrs
 * compiled, or a C function compiled with unwind tables.
 *
 * `setjmp` is a function here rather than a macro; cinrs recognises a call to
 * it, and to the other spellings below, by name.
 */
#ifndef __CINRS_SETJMP_H
#define __CINRS_SETJMP_H

typedef struct __jmp_buf_tag {
    unsigned long long __cinrs_words[25];
} jmp_buf[1];

int setjmp(jmp_buf __env);
__cinrs_noreturn void longjmp(jmp_buf __env, int __val);

#if !defined(__STRICT_ANSI__) || defined(_POSIX_C_SOURCE) || defined(_XOPEN_SOURCE)
typedef jmp_buf sigjmp_buf;
int _setjmp(jmp_buf __env);
__cinrs_noreturn void _longjmp(jmp_buf __env, int __val);
int sigsetjmp(sigjmp_buf __env, int __savemask);
__cinrs_noreturn void siglongjmp(sigjmp_buf __env, int __val);
#endif

#endif
