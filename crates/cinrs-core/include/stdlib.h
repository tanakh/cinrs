/* <stdlib.h> — general utilities (C99 7.20). */
#ifndef _CINRS_STDLIB_H
#define _CINRS_STDLIB_H

#include <stddef.h>

#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1

#if defined(_WIN32)
#define RAND_MAX 32767
#else
#define RAND_MAX 2147483647
#endif

#define MB_CUR_MAX 6

typedef struct {
    int quot;
    int rem;
} div_t;

typedef struct {
    long quot;
    long rem;
} ldiv_t;

/* The C99 additions use `long long`, which C89 has not; `__extension__` is
 * how a header says "an extension, and I know it", so that a `c89!` block
 * including this one is not told off for what the implementation wrote. */
__extension__ typedef struct {
    long long quot;
    long long rem;
} lldiv_t;

void *malloc(size_t size);
void *calloc(size_t nmemb, size_t size);
void *realloc(void *ptr, size_t size);
void free(void *ptr);

int abs(int j);
long labs(long j);
__extension__ long long llabs(long long j);
div_t div(int numer, int denom);
ldiv_t ldiv(long numer, long denom);
__extension__ lldiv_t lldiv(long long numer, long long denom);

int atoi(const char *nptr);
long atol(const char *nptr);
__extension__ long long atoll(const char *nptr);
double atof(const char *nptr);
long strtol(const char *nptr, char **endptr, int base);
unsigned long strtoul(const char *nptr, char **endptr, int base);
__extension__ long long strtoll(const char *nptr, char **endptr, int base);
__extension__ unsigned long long strtoull(const char *nptr, char **endptr,
                                          int base);
double strtod(const char *nptr, char **endptr);
float strtof(const char *nptr, char **endptr);

int rand(void);
void srand(unsigned int seed);

/* The functions that do not return are marked with the spelling of
 * `_Noreturn` that every standard this crate accepts understands: `_Noreturn`
 * itself is a C11 keyword, and this header is read by `c99!` blocks too. A
 * call to one of them ends the statement it is in, so a function may end with
 * `exit(1);` and never write a `return`. */
__cinrs_noreturn void exit(int status);
__cinrs_noreturn void _Exit(int status);
__cinrs_noreturn void abort(void);
int atexit(void (*func)(void));

#if __STDC_VERSION__ >= 201112L
__cinrs_noreturn void quick_exit(int status);
int at_quick_exit(void (*func)(void));
#endif

char *getenv(const char *name);
int system(const char *string);

void qsort(void *base, size_t nmemb, size_t size,
           int (*compar)(const void *, const void *));
void *bsearch(const void *key, const void *base, size_t nmemb, size_t size,
              int (*compar)(const void *, const void *));

#endif /* _CINRS_STDLIB_H */
