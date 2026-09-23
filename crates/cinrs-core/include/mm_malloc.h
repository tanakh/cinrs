/* <mm_malloc.h> — `_mm_malloc` and `_mm_free`, aligned allocation for SIMD.
 *
 * Not a standard C header: it is GCC's and Clang's, and their <xmmintrin.h>
 * includes it, which is how a program that includes only <xmmintrin.h> gets
 * `exit`, `atoi` and the rest of <stdlib.h> — real programs rely on that.
 *
 * GCC's copy calls `posix_memalign`, which the Microsoft runtime does not
 * have, and `aligned_alloc` is missing there too; so this one is portable C
 * over `malloc`: it over-allocates, rounds the address up, and keeps the
 * pointer `malloc` returned in the word just below the block, where `_mm_free`
 * reads it back. Only `_mm_free` may free what `_mm_malloc` returned.
 *
 * The alignment rules are GCC's: 1, 2 and 4 mean "as a pointer is aligned";
 * any other alignment that is not a power of two, or that is zero, gives a
 * null pointer, as `posix_memalign` would refuse it. `_mm_malloc(0, a)` returns
 * a block `_mm_free` accepts, and `_mm_free(NULL)` does nothing.
 */
#ifndef _CINRS_MM_MALLOC_H
#define _CINRS_MM_MALLOC_H

#include <stdlib.h>
#include <stdint.h>

static __inline void *_mm_malloc(size_t __size, size_t __align) {
    char *__raw;
    size_t __skip;
    if (__align == 1 || __align == 2 || __align == 4)
        __align = sizeof(void *);
    if (__align == 0 || (__align & (__align - 1)) != 0 || __align < sizeof(void *))
        return NULL;
    if (__size > (size_t)-1 - __align - sizeof(void *))
        return NULL;
    __raw = (char *)malloc(__size + __align - 1 + sizeof(void *));
    if (__raw == NULL)
        return NULL;
    /* The first address at least a pointer's width in that is aligned. */
    __skip = sizeof(void *);
    __skip += (__align - ((uintptr_t)(__raw + __skip) & (__align - 1))) & (__align - 1);
    ((void **)(__raw + __skip))[-1] = __raw;
    return __raw + __skip;
}

static __inline void _mm_free(void *__ptr) {
    if (__ptr != NULL)
        free(((void **)__ptr)[-1]);
}

#endif /* _CINRS_MM_MALLOC_H */
