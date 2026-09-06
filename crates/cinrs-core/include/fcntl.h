/* <fcntl.h> — opening a file by name (POSIX.1-2008 XSH).
 *
 * `open`, `creat` and the `O_*` flags, and nothing else: `fcntl` itself takes
 * a `struct flock` whose layout differs between the two families, and
 * `openat` and the `AT_*` constants are Linux's alone.
 *
 * The flag values are the *platform's*. The first three — `O_RDONLY`,
 * `O_WRONLY`, `O_RDWR` — are 0, 1 and 2 everywhere, and every one after that
 * differs between Linux and the BSDs, which is why they are chosen by the
 * model cinrs was told to translate for.
 *
 * `open` is variadic because its third argument, the `mode_t` a newly created
 * file is given, is only read when `O_CREAT` is in the flags — which is how
 * every real library declares it.
 */
#ifndef _CINRS_FCNTL_H
#define _CINRS_FCNTL_H

#if defined(_WIN32)

/* Everything below is behind the `#else`, so that the one diagnostic a
 * Windows target gets is this one and not a cascade of undeclared types. */
#error "This <fcntl.h> is the POSIX one; the Microsoft C runtime's has _O_-prefixed flags and an _open with a different signature. Point `#pragma cinrs include_path` at the platform's own header instead."

#else

#include <sys/types.h>

#define O_RDONLY 0
#define O_WRONLY 1
#define O_RDWR 2
#define O_ACCMODE 3

#if defined(__APPLE__)
#define O_NONBLOCK 0x0004
#define O_APPEND 0x0008
#define O_CREAT 0x0200
#define O_TRUNC 0x0400
#define O_EXCL 0x0800
#define O_NOCTTY 0x20000
#define O_SYNC 0x0080
#define O_CLOEXEC 0x1000000
#define O_DIRECTORY 0x100000
#define O_NOFOLLOW 0x0100
#else
/* Linux's, which is the asm-generic set every architecture but alpha,
 * mips, parisc, sparc and xtensa uses. */
#define O_CREAT 0100
#define O_EXCL 0200
#define O_NOCTTY 0400
#define O_TRUNC 01000
#define O_APPEND 02000
#define O_NONBLOCK 04000
#define O_SYNC 04010000
#define O_DIRECTORY 0200000
#define O_NOFOLLOW 0400000
#define O_CLOEXEC 02000000
#endif

#define O_NDELAY O_NONBLOCK

int open(const char *path, int flags, ...);
int creat(const char *path, mode_t mode);

#endif /* !_WIN32 */

#endif /* _CINRS_FCNTL_H */
