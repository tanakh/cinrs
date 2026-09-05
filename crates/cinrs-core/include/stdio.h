/* <stdio.h> — input/output (C99 7.19).
 *
 * `FILE` is an incomplete type: nothing here says what the platform's really
 * looks like, and nothing needs to, since a program only ever passes `FILE *`
 * around. The stream pointers are the symbols the platform's own library
 * exports, which is why they are spelled differently on each.
 *
 * `fpos_t`, `fgetpos` and `fsetpos` are deliberately absent: glibc's `fpos_t`
 * is a structure whose layout this header would have to guess at, and getting
 * it wrong would corrupt memory rather than fail to compile. Use `ftell` and
 * `fseek`.
 *
 * `_WIN32` and `__APPLE__` are the *target's*, from the model cinrs was told
 * to translate for, so a cross build declares the streams of the machine the
 * program will run on. The Windows branch is the portable UCRT subset —
 * `__acrt_iob_func` for the three streams, which both the Microsoft library
 * and mingw-w64 export — and is the least tested of the three; see the
 * "Cross-compilation" section of the README.
 */
#ifndef _CINRS_STDIO_H
#define _CINRS_STDIO_H

#include <stdarg.h>
#include <stddef.h>

#if defined(_WIN32)
typedef struct _iobuf FILE;
#elif defined(__APPLE__)
typedef struct __sFILE FILE;
#else
typedef struct _IO_FILE FILE;
#endif

#define EOF (-1)

#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2

#if defined(_WIN32)
#define BUFSIZ 512
#define FOPEN_MAX 20
#define FILENAME_MAX 260
#define _IOFBF 0
#define _IOLBF 64
#define _IONBF 4
#elif defined(__APPLE__)
#define BUFSIZ 1024
#define FOPEN_MAX 20
#define FILENAME_MAX 1024
#define _IOFBF 0
#define _IOLBF 1
#define _IONBF 2
#else
#define BUFSIZ 8192
#define FOPEN_MAX 16
#define FILENAME_MAX 4096
#define _IOFBF 0
#define _IOLBF 1
#define _IONBF 2
#endif

#define L_tmpnam 20
#define TMP_MAX 238328

#if defined(__APPLE__)
extern FILE *__stdinp;
extern FILE *__stdoutp;
extern FILE *__stderrp;
#define stdin __stdinp
#define stdout __stdoutp
#define stderr __stderrp
#elif defined(_WIN32)
FILE *__acrt_iob_func(unsigned int index);
#define stdin (__acrt_iob_func(0))
#define stdout (__acrt_iob_func(1))
#define stderr (__acrt_iob_func(2))
#else
extern FILE *stdin;
extern FILE *stdout;
extern FILE *stderr;
#endif

int printf(const char *format, ...);
int fprintf(FILE *stream, const char *format, ...);
int sprintf(char *s, const char *format, ...);
int snprintf(char *s, size_t n, const char *format, ...);
int vprintf(const char *format, va_list arg);
int vfprintf(FILE *stream, const char *format, va_list arg);
int vsprintf(char *s, const char *format, va_list arg);
int vsnprintf(char *s, size_t n, const char *format, va_list arg);

int scanf(const char *format, ...);
int fscanf(FILE *stream, const char *format, ...);
int sscanf(const char *s, const char *format, ...);

int puts(const char *s);
int fputs(const char *s, FILE *stream);
int putchar(int c);
int fputc(int c, FILE *stream);
int putc(int c, FILE *stream);
int getchar(void);
int fgetc(FILE *stream);
int getc(FILE *stream);
int ungetc(int c, FILE *stream);
char *fgets(char *s, int n, FILE *stream);

FILE *fopen(const char *filename, const char *mode);
FILE *freopen(const char *filename, const char *mode, FILE *stream);
FILE *tmpfile(void);
char *tmpnam(char *s);
int fclose(FILE *stream);
int fflush(FILE *stream);
void setbuf(FILE *stream, char *buf);
int setvbuf(FILE *stream, char *buf, int mode, size_t size);

size_t fread(void *ptr, size_t size, size_t nmemb, FILE *stream);
size_t fwrite(const void *ptr, size_t size, size_t nmemb, FILE *stream);

int fseek(FILE *stream, long offset, int whence);
long ftell(FILE *stream);
void rewind(FILE *stream);

int remove(const char *filename);
int rename(const char *old, const char *new_name);
void perror(const char *s);

void clearerr(FILE *stream);
int feof(FILE *stream);
int ferror(FILE *stream);

#endif /* _CINRS_STDIO_H */
