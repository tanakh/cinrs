/* <unistd.h> — the POSIX system calls a small program actually reaches for
 * (POSIX.1-2008 XSH).
 *
 * Deliberately small. What is here is the handful of calls whose prototypes
 * are the same on every Unix and mention no type with a layout: read, write,
 * close, lseek, the process identity, sleeping, and `_exit`. What is not here
 * is everything that needs `struct stat`, `fd_set`, `struct timespec` or a
 * `pthread_*` type, because a header that guessed at one of those layouts
 * would corrupt memory rather than fail to compile — point
 * `#pragma cinrs include_path` at the platform's own header if that is what
 * a program needs.
 *
 * There is no Windows branch: the Microsoft runtime has no header of this
 * name, and its nearest equivalents are spelled `_read`, `_write`, `_close`
 * and `_getpid` in `<io.h>` and `<process.h>`.
 */
#ifndef _CINRS_UNISTD_H
#define _CINRS_UNISTD_H

#if defined(_WIN32)

/* Everything below is behind the `#else`, so that the one diagnostic a
 * Windows target gets is this one and not a cascade of undeclared types. */
#error "<unistd.h> is a POSIX header; the Microsoft C runtime has no such header. Its nearest equivalents are _read, _write, _close and _getpid, in <io.h> and <process.h>."

#else

#include <stddef.h>
#include <sys/types.h>

#define STDIN_FILENO 0
#define STDOUT_FILENO 1
#define STDERR_FILENO 2

#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

#ifndef NULL
#define NULL ((void *)0)
#endif

ssize_t read(int fd, void *buf, size_t count);
ssize_t write(int fd, const void *buf, size_t count);
int close(int fd);
off_t lseek(int fd, off_t offset, int whence);
int unlink(const char *pathname);
int isatty(int fd);
int dup(int oldfd);
int dup2(int oldfd, int newfd);

pid_t getpid(void);
pid_t getppid(void);
uid_t getuid(void);
uid_t geteuid(void);
gid_t getgid(void);
gid_t getegid(void);

unsigned int sleep(unsigned int seconds);
int usleep(useconds_t usec);

/* `_exit` does not run the `atexit` handlers or flush the streams, which is
 * exactly why a program calls it. */
__cinrs_noreturn void _exit(int status);

#endif /* !_WIN32 */

#endif /* _CINRS_UNISTD_H */
