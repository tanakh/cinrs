/* <errno.h> — errors (C99 7.5, and the whole of POSIX.1-2017's set).
 *
 * `errno` is a macro around the platform's per-thread location, which is what
 * every modern C library makes it: reading a global would be wrong in a
 * threaded program, and Rust code calling into C is threaded by default.
 *
 * `<errno.h>` is an ISO C header and is therefore bundled — the three names C
 * itself requires are `EDOM`, `ERANGE` and `EILSEQ` — but a program that uses
 * `errno` at all uses the POSIX names, so the full POSIX.1-2017 list is here
 * for every platform family this crate models a C library for. The numbers are
 * ABI facts and are taken from the real headers: Linux's from the kernel's
 * `asm-generic/errno{,-base}.h`, which glibc and musl both use unchanged;
 * Apple's from xnu's `bsd/sys/errno.h`; Windows' from the Universal CRT's own
 * `<errno.h>`.
 *
 * `_WIN32` and `__APPLE__` here are the *target's*, predefined from the model
 * cinrs was told to translate for — `CINRS_TARGET`, `#pragma cinrs target`, or
 * the host with neither — so a cross build takes the branch of the machine the
 * program will run on rather than the one it is built on.
 *
 * Every definition is `#ifndef`-guarded, and the Linux branch spells `errno`,
 * `EWOULDBLOCK`, `ENOTSUP` and `EDEADLOCK` exactly the way glibc spells them —
 * `(*__errno_location ())` with the space glibc writes, `EWOULDBLOCK` as
 * `EAGAIN` rather than as `11`. That is not fussiness: with
 * `#pragma cinrs system_include` a platform header may pull in the platform's
 * own `<errno.h>` after this one, and a macro redefined with a different token
 * sequence is a diagnostic where one redefined identically is not. glibc's own
 * `bits/errno.h` guards its additions the same way, for the same reason.
 *
 * The Linux numbers are the `asm-generic` ones, which every architecture Rust
 * targets on Linux uses *except* MIPS and SPARC, whose kernels renumber
 * everything above 34 (`EDQUOT` is 1133 on MIPS). Rather than emit wrong
 * numbers there, those two architectures get only what ISO C requires.
 */
#ifndef _CINRS_ERRNO_H
#define _CINRS_ERRNO_H

#if defined(_WIN32)
int *_errno(void);
#ifndef errno
#define errno (*_errno())
#endif
#elif defined(__APPLE__)
int *__error(void);
#ifndef errno
#define errno (*__error())
#endif
#else
int *__errno_location(void);
#ifndef errno
#define errno (*__errno_location ())
#endif
#endif

/* The three ISO C requires, which happen to agree on every platform here. */
#ifndef EDOM
#define EDOM 33
#endif
#ifndef ERANGE
#define ERANGE 34
#endif

#if defined(_WIN32)
/* ---------------------------------------------------------------------------
 * Windows / Universal CRT.
 *
 * Two blocks: the classic 1..42 set, which is not contiguous (there is no
 * ENOTBLK 15, no 26, 35, 37 or 43..99), and the POSIX block the UCRT added at
 * 100. `EDEADLK` is 36 here and `ETXTBSY` 139 — not the Linux 35 and 26, which
 * is the trap in copying one branch to the other. `EDQUOT`, `EMULTIHOP` and
 * `ESTALE` are the three POSIX names the UCRT has no number for, so they are
 * absent rather than invented.
 * ------------------------------------------------------------------------- */
#define EILSEQ 42
#define EPERM 1
#define ENOENT 2
#define ESRCH 3
#define EINTR 4
#define EIO 5
#define ENXIO 6
#define E2BIG 7
#define ENOEXEC 8
#define EBADF 9
#define ECHILD 10
#define EAGAIN 11
#define ENOMEM 12
#define EACCES 13
#define EFAULT 14
#define EBUSY 16
#define EEXIST 17
#define EXDEV 18
#define ENODEV 19
#define ENOTDIR 20
#define EISDIR 21
#define EINVAL 22
#define ENFILE 23
#define EMFILE 24
#define ENOTTY 25
#define EFBIG 27
#define ENOSPC 28
#define ESPIPE 29
#define EROFS 30
#define EMLINK 31
#define EPIPE 32
#define EDEADLK 36
#define EDEADLOCK EDEADLK
#define ENAMETOOLONG 38
#define ENOLCK 39
#define ENOSYS 40
#define ENOTEMPTY 41
#define EADDRINUSE 100
#define EADDRNOTAVAIL 101
#define EAFNOSUPPORT 102
#define EALREADY 103
#define EBADMSG 104
#define ECANCELED 105
#define ECONNABORTED 106
#define ECONNREFUSED 107
#define ECONNRESET 108
#define EDESTADDRREQ 109
#define EHOSTUNREACH 110
#define EIDRM 111
#define EINPROGRESS 112
#define EISCONN 113
#define ELOOP 114
#define EMSGSIZE 115
#define ENETDOWN 116
#define ENETRESET 117
#define ENETUNREACH 118
#define ENOBUFS 119
#define ENODATA 120
#define ENOLINK 121
#define ENOMSG 122
#define ENOPROTOOPT 123
#define ENOSR 124
#define ENOSTR 125
#define ENOTCONN 126
#define ENOTRECOVERABLE 127
#define ENOTSOCK 128
#define ENOTSUP 129
#define EOPNOTSUPP 130
#define EOTHER 131
#define EOVERFLOW 132
#define EOWNERDEAD 133
#define EPROTO 134
#define EPROTONOSUPPORT 135
#define EPROTOTYPE 136
#define ETIME 137
#define ETIMEDOUT 138
#define ETXTBSY 139
#define EWOULDBLOCK 140

#elif defined(__APPLE__)
/* ---------------------------------------------------------------------------
 * Apple / Darwin (xnu `bsd/sys/errno.h`).
 *
 * The 4.4BSD numbering: 1..34 as everywhere, then the socket errors at 35..59
 * where Linux has its own set, then 60..88, then the POSIX/STREAMS block at
 * 89..106. `EAGAIN` is 35 and `EWOULDBLOCK` is the same number, as on Linux;
 * `ENOTSUP` (45) and `EOPNOTSUPP` (102) are *different* numbers, which they are
 * not on Linux. Darwin's header makes `EOPNOTSUPP` an alias of `ENOTSUP` only
 * under the pre-10.5 `!__DARWIN_UNIX03` ABI, which no supported target uses.
 * ------------------------------------------------------------------------- */
#define EILSEQ 92
#define EPERM 1
#define ENOENT 2
#define ESRCH 3
#define EINTR 4
#define EIO 5
#define ENXIO 6
#define E2BIG 7
#define ENOEXEC 8
#define EBADF 9
#define ECHILD 10
#define EDEADLK 11
#define ENOMEM 12
#define EACCES 13
#define EFAULT 14
#define EBUSY 16
#define EEXIST 17
#define EXDEV 18
#define ENODEV 19
#define ENOTDIR 20
#define EISDIR 21
#define EINVAL 22
#define ENFILE 23
#define EMFILE 24
#define ENOTTY 25
#define ETXTBSY 26
#define EFBIG 27
#define ENOSPC 28
#define ESPIPE 29
#define EROFS 30
#define EMLINK 31
#define EPIPE 32
#define EAGAIN 35
#define EWOULDBLOCK EAGAIN
#define EINPROGRESS 36
#define EALREADY 37
#define ENOTSOCK 38
#define EDESTADDRREQ 39
#define EMSGSIZE 40
#define EPROTOTYPE 41
#define ENOPROTOOPT 42
#define EPROTONOSUPPORT 43
#define ENOTSUP 45
#define EAFNOSUPPORT 47
#define EADDRINUSE 48
#define EADDRNOTAVAIL 49
#define ENETDOWN 50
#define ENETUNREACH 51
#define ENETRESET 52
#define ECONNABORTED 53
#define ECONNRESET 54
#define ENOBUFS 55
#define EISCONN 56
#define ENOTCONN 57
#define ETIMEDOUT 60
#define ECONNREFUSED 61
#define ELOOP 62
#define ENAMETOOLONG 63
#define EHOSTUNREACH 65
#define ENOTEMPTY 66
#define EDQUOT 69
#define ESTALE 70
#define ENOLCK 77
#define ENOSYS 78
#define EOVERFLOW 84
#define ECANCELED 89
#define EIDRM 90
#define ENOMSG 91
#define EBADMSG 94
#define EMULTIHOP 95
#define ENODATA 96
#define ENOLINK 97
#define ENOSR 98
#define ENOSTR 99
#define EPROTO 100
#define ETIME 101
#define EOPNOTSUPP 102
#define ENOTRECOVERABLE 104
#define EOWNERDEAD 105

#elif defined(__mips__) || defined(__mips)
/* ---------------------------------------------------------------------------
 * MIPS on Linux: the kernel renumbers everything above 34 — `EDEADLK` is 45,
 * `ENOSYS` 89, `EDQUOT` famously 1133 — so only what ISO C requires is defined
 * here. Guessing a POSIX number would be worse than not having it: a program
 * comparing a real `errno` against the wrong constant takes the wrong branch
 * and says nothing. A program that needs them on MIPS wants the platform's own
 * header, through `#pragma cinrs system_include first`.
 * ------------------------------------------------------------------------- */
#define EILSEQ 88

#elif defined(__sparc__) || defined(__sparc)
/* SPARC on Linux, for the same reason: its numbering above 34 is the BSD-ish
 * one — `EDEADLK` 78, `ENOSYS` 90, `EILSEQ` 122 — and none of it is
 * `asm-generic`'s. */
#define EILSEQ 122

#else
/* ---------------------------------------------------------------------------
 * Linux, glibc and musl alike: the kernel's `asm-generic` numbering, which is
 * what every architecture Rust targets on Linux uses but MIPS and SPARC.
 *
 * The three aliases are written as glibc writes them rather than as numbers, so
 * that the platform's own `<errno.h>` — which a system header may pull in after
 * this one — redefines them *identically*.
 * ------------------------------------------------------------------------- */
#define EILSEQ 84
#define EPERM 1
#define ENOENT 2
#define ESRCH 3
#define EINTR 4
#define EIO 5
#define ENXIO 6
#define E2BIG 7
#define ENOEXEC 8
#define EBADF 9
#define ECHILD 10
#define EAGAIN 11
#define EWOULDBLOCK EAGAIN
#define ENOMEM 12
#define EACCES 13
#define EFAULT 14
#define EBUSY 16
#define EEXIST 17
#define EXDEV 18
#define ENODEV 19
#define ENOTDIR 20
#define EISDIR 21
#define EINVAL 22
#define ENFILE 23
#define EMFILE 24
#define ENOTTY 25
#define ETXTBSY 26
#define EFBIG 27
#define ENOSPC 28
#define ESPIPE 29
#define EROFS 30
#define EMLINK 31
#define EPIPE 32
#define EDEADLK 35
#define EDEADLOCK EDEADLK
#define ENAMETOOLONG 36
#define ENOLCK 37
#define ENOSYS 38
#define ENOTEMPTY 39
#define ELOOP 40
#define ENOMSG 42
#define EIDRM 43
#define ENOSTR 60
#define ENODATA 61
#define ETIME 62
#define ENOSR 63
#define ENOLINK 67
#define EPROTO 71
#define EMULTIHOP 72
#define EBADMSG 74
#define EOVERFLOW 75
#define ENOTSOCK 88
#define EDESTADDRREQ 89
#define EMSGSIZE 90
#define EPROTOTYPE 91
#define ENOPROTOOPT 92
#define EPROTONOSUPPORT 93
#define EOPNOTSUPP 95
#define ENOTSUP EOPNOTSUPP
#define EAFNOSUPPORT 97
#define EADDRINUSE 98
#define EADDRNOTAVAIL 99
#define ENETDOWN 100
#define ENETUNREACH 101
#define ENETRESET 102
#define ECONNABORTED 103
#define ECONNRESET 104
#define ENOBUFS 105
#define EISCONN 106
#define ENOTCONN 107
#define ETIMEDOUT 110
#define ECONNREFUSED 111
#define EHOSTUNREACH 113
#define EALREADY 114
#define EINPROGRESS 115
#define ESTALE 116
#define EDQUOT 122
#define ECANCELED 125
#define EOWNERDEAD 130
#define ENOTRECOVERABLE 131
#endif

#endif /* _CINRS_ERRNO_H */
