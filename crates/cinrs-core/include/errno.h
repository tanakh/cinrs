/* <errno.h> — errors (C99 7.5).
 *
 * `errno` is a macro around the platform's per-thread location, which is what
 * every modern C library makes it: reading a global would be wrong in a
 * threaded program, and Rust code calling into C is threaded by default.
 *
 * Only the numbers are platform-specific; EDOM and ERANGE happen to agree
 * everywhere.
 */
#ifndef _CINRS_ERRNO_H
#define _CINRS_ERRNO_H

#if defined(_WIN32)
int *_errno(void);
#define errno (*_errno())
#elif defined(__APPLE__)
int *__error(void);
#define errno (*__error())
#else
int *__errno_location(void);
#define errno (*__errno_location())
#endif

#define EDOM 33
#define ERANGE 34

#if defined(__APPLE__)
#define EILSEQ 92
#define EPERM 1
#define ENOENT 2
#define EINTR 4
#define EIO 5
#define EBADF 9
#define EAGAIN 35
#define ENOMEM 12
#define EACCES 13
#define EEXIST 17
#define EINVAL 22
#define ENOSPC 28
#define EPIPE 32
#elif defined(_WIN32)
#define EILSEQ 42
#define EPERM 1
#define ENOENT 2
#define EINTR 4
#define EIO 5
#define EBADF 9
#define EAGAIN 11
#define ENOMEM 12
#define EACCES 13
#define EEXIST 17
#define EINVAL 22
#define ENOSPC 28
#define EPIPE 32
#else
#define EILSEQ 84
#define EPERM 1
#define ENOENT 2
#define EINTR 4
#define EIO 5
#define EBADF 9
#define EAGAIN 11
#define ENOMEM 12
#define EACCES 13
#define EEXIST 17
#define EINVAL 22
#define ENOSPC 28
#define EPIPE 32
#endif

#endif /* _CINRS_ERRNO_H */
