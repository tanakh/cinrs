/* <signal.h> — signal handling (C99 7.14).
 *
 * The six signal numbers C requires are the *platform's*, not C's: nothing in
 * the standard fixes them, and a program that passes `SIGFPE` to the real
 * `signal` has to pass the number that library expects. So the numbers below
 * are the three families' own — Linux's, the BSD/Apple one, and the small set
 * the Microsoft C runtime has — chosen by the model cinrs was told to
 * translate for, exactly as `<time.h>` chooses `time_t`.
 *
 * The POSIX signals beyond the standard six are here for the two Unix
 * families, because a program that says `signal(SIGPIPE, SIG_IGN)` is doing
 * something ordinary; `sigaction`, `sigset_t` and the real-time signals are
 * not, because `struct sigaction` and `sigset_t` have layouts this header
 * would have to guess at, and guessing wrong would corrupt memory rather than
 * fail to compile.
 *
 * `SIG_DFL`, `SIG_IGN` and `SIG_ERR` are the three small integers every
 * platform uses for them, cast to the handler type through `__INTPTR_TYPE__`
 * so that the conversion is between an integer and a pointer of the same
 * width rather than a narrowing one.
 */
#ifndef _CINRS_SIGNAL_H
#define _CINRS_SIGNAL_H

/* The only type C requires here: an integer object that can be accessed as an
 * atomic entity even in the presence of asynchronous interrupts. It is `int`
 * on all three platforms. */
typedef int sig_atomic_t;

#define SIG_DFL ((void (*)(int))(__INTPTR_TYPE__)0)
#define SIG_IGN ((void (*)(int))(__INTPTR_TYPE__)1)
#define SIG_ERR ((void (*)(int))(__INTPTR_TYPE__)-1)

#if defined(_WIN32)
/* The Microsoft C runtime has only these, and numbers `SIGABRT` 22 rather
 * than 6; 6 is `SIGABRT_COMPAT`, which is what a program compiled elsewhere
 * would have sent. */
#define SIGINT 2
#define SIGILL 4
#define SIGABRT_COMPAT 6
#define SIGFPE 8
#define SIGSEGV 11
#define SIGTERM 15
#define SIGBREAK 21
#define SIGABRT 22
#define NSIG 23
#elif defined(__APPLE__)
/* The 4.4BSD numbering, which Apple, FreeBSD and NetBSD share. */
#define SIGHUP 1
#define SIGINT 2
#define SIGQUIT 3
#define SIGILL 4
#define SIGTRAP 5
#define SIGABRT 6
#define SIGEMT 7
#define SIGFPE 8
#define SIGKILL 9
#define SIGBUS 10
#define SIGSEGV 11
#define SIGSYS 12
#define SIGPIPE 13
#define SIGALRM 14
#define SIGTERM 15
#define SIGURG 16
#define SIGSTOP 17
#define SIGTSTP 18
#define SIGCONT 19
#define SIGCHLD 20
#define SIGTTIN 21
#define SIGTTOU 22
#define SIGIO 23
#define SIGXCPU 24
#define SIGXFSZ 25
#define SIGVTALRM 26
#define SIGPROF 27
#define SIGWINCH 28
#define SIGINFO 29
#define SIGUSR1 30
#define SIGUSR2 31
#define NSIG 32
#else
/* Linux's, which is the System V numbering with the two user signals where
 * BSD puts `SIGBUS` and `SIGSYS`. */
#define SIGHUP 1
#define SIGINT 2
#define SIGQUIT 3
#define SIGILL 4
#define SIGTRAP 5
#define SIGABRT 6
#define SIGIOT 6
#define SIGBUS 7
#define SIGFPE 8
#define SIGKILL 9
#define SIGUSR1 10
#define SIGSEGV 11
#define SIGUSR2 12
#define SIGPIPE 13
#define SIGALRM 14
#define SIGTERM 15
#define SIGSTKFLT 16
#define SIGCHLD 17
#define SIGCONT 18
#define SIGSTOP 19
#define SIGTSTP 20
#define SIGTTIN 21
#define SIGTTOU 22
#define SIGURG 23
#define SIGXCPU 24
#define SIGXFSZ 25
#define SIGVTALRM 26
#define SIGPROF 27
#define SIGWINCH 28
#define SIGIO 29
#define SIGPOLL 29
#define SIGPWR 30
#define SIGSYS 31
#define NSIG 65
#endif

void (*signal(int sig, void (*func)(int)))(int);
int raise(int sig);

#endif /* _CINRS_SIGNAL_H */
