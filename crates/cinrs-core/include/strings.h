/* <strings.h> — the BSD string functions POSIX kept (POSIX.1-2008 XSH).
 *
 * Four of these are older spellings of what `<string.h>` has — `bzero` is
 * `memset(s, 0, n)`, `bcopy` is `memmove` with its arguments the other way
 * round, `index` is `strchr` and `rindex` is `strrchr` — and every Unix
 * library still exports all of them. `strcasecmp` and `ffs` have no
 * `<string.h>` counterpart at all.
 *
 * There is no Windows branch: the Microsoft runtime has no header of this
 * name and exports none of these symbols (its own are `_stricmp`,
 * `_strnicmp` and `_BitScanForward`), so declaring them for a Windows target
 * would turn a compile error into a link error.
 */
#ifndef _CINRS_STRINGS_H
#define _CINRS_STRINGS_H

#if defined(_WIN32)

/* Everything below is behind the `#else`, so that the one diagnostic a
 * Windows target gets is this one and not a cascade of undeclared types. */
#error "<strings.h> is a POSIX header; the Microsoft C runtime has no such header, and none of its functions. Use <string.h>, or _stricmp / _strnicmp."

#else

#include <stddef.h>

int strcasecmp(const char *s1, const char *s2);
int strncasecmp(const char *s1, const char *s2, size_t n);

void bzero(void *s, size_t n);
void bcopy(const void *src, void *dest, size_t n);
int bcmp(const void *s1, const void *s2, size_t n);

char *index(const char *s, int c);
char *rindex(const char *s, int c);

/* One more than the index of the least significant set bit, or 0. */
int ffs(int i);

#endif /* !_WIN32 */

#endif /* _CINRS_STRINGS_H */
