/* <sys/types.h> — the system's typedef names (POSIX.1, and a subset on
 * Windows).
 *
 * Not a C header at all: every name here is the *platform's* choice, and a
 * program that passes one of these types to a real system call has to name
 * the same underlying type its library does. So the three branches below are
 * read off the three libraries — glibc's `<bits/typesizes.h>`, Apple's
 * `<sys/_types/…>` and the Microsoft runtime's `<sys/types.h>` — and the
 * model cinrs was told to translate for picks between them, exactly as
 * `<time.h>` picks `time_t`.
 *
 * `time_t` and `clock_t` are also `<time.h>`'s, and `size_t` also
 * `<stddef.h>`'s; repeating a `typedef` is legal, so including both headers
 * in either order is quiet.
 *
 * What is *not* here is anything with a layout: `struct stat`, `fd_set`,
 * `pthread_t` and `sigset_t` are opaque or platform-shaped, and a header that
 * guessed at them would corrupt memory rather than fail to compile.
 */
#ifndef _CINRS_SYS_TYPES_H
#define _CINRS_SYS_TYPES_H

#include <stddef.h>

#if defined(_WIN32)

/* The Microsoft runtime's own `<sys/types.h>`, which is this and no more.
 * `ssize_t` is the pointer-sized signed integer the Windows SDK calls
 * `SSIZE_T`. */
typedef long _off_t;
typedef long off_t;
typedef unsigned int _dev_t;
typedef unsigned int dev_t;
typedef unsigned short _ino_t;
typedef unsigned short ino_t;
typedef unsigned short _mode_t;
typedef unsigned short mode_t;
typedef int pid_t;
typedef __INTPTR_TYPE__ ssize_t;
/* `__extension__`: `long long` is C99's, and a header may use it whatever the
 * entry point is. */
__extension__ typedef long long time_t;
typedef long clock_t;

#elif defined(__APPLE__)

typedef long ssize_t;
__extension__ typedef long long off_t;
typedef int pid_t;
typedef unsigned int uid_t;
typedef unsigned int gid_t;
typedef unsigned short mode_t;
typedef int dev_t;
__extension__ typedef unsigned long long ino_t;
typedef unsigned short nlink_t;
typedef int blksize_t;
__extension__ typedef long long blkcnt_t;
typedef long time_t;
typedef unsigned long clock_t;
typedef int suseconds_t;
typedef unsigned int useconds_t;
typedef unsigned int id_t;
typedef char *caddr_t;

#else

/* glibc and musl agree on every one of these. The `unsigned long` ones are
 * `__UWORD_TYPE`, which is `unsigned int` on a 32-bit target — where `nlink_t`
 * is `unsigned int` and `ino_t` and `dev_t` keep 64 bits, because the kernel
 * interface does. */
typedef long ssize_t;
typedef long off_t;
typedef int pid_t;
typedef unsigned int uid_t;
typedef unsigned int gid_t;
typedef unsigned int mode_t;
__extension__ typedef unsigned long long dev_t;
__extension__ typedef unsigned long long ino_t;
typedef long time_t;
typedef long clock_t;
typedef long suseconds_t;
typedef unsigned int useconds_t;
typedef unsigned int id_t;
typedef int key_t;
typedef char *caddr_t;

#if __SIZEOF_POINTER__ == 8
typedef unsigned long nlink_t;
typedef long blksize_t;
typedef long blkcnt_t;
#else
typedef unsigned int nlink_t;
typedef long blksize_t;
__extension__ typedef long long blkcnt_t;
#endif

#endif

#endif /* _CINRS_SYS_TYPES_H */
