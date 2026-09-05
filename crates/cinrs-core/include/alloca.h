/* <alloca.h> — allocation in the calling function's frame.
 *
 * Not a standard C header: it is what glibc, the BSDs and (as <malloc.h>)
 * MSVC give the function, and every one of them defines the name in terms of
 * the compiler's own builtin, because the memory has to belong to the caller's
 * frame rather than to the header's. This does the same.
 *
 * `cinrs` emulates `alloca` with a per-function arena on the heap: the memory
 * is freed when the function returns, which is exactly the lifetime C gives
 * it, so a pointer that outlives the call dangles here as it does anywhere
 * else. See the crate documentation.
 */
#ifndef _CINRS_ALLOCA_H
#define _CINRS_ALLOCA_H

#include <stddef.h>

#define alloca(size) __builtin_alloca(size)

#endif /* _CINRS_ALLOCA_H */
