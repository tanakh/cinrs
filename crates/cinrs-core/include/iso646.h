/* <iso646.h> — alternative spellings (C95 7.9, C99 7.9).
 *
 * The eleven macros the amendment added so that the operators C spells with
 * characters outside the invariant part of ISO 646 can be written with
 * letters. They are ordinary object-like macros — nothing in the language
 * knows about them — which is why this header is eleven `#define`s and
 * nothing else.
 *
 * C++ made the same eleven into keywords and deprecated the header; C did
 * not, and C23 keeps it.
 */
#ifndef _CINRS_ISO646_H
#define _CINRS_ISO646_H

#define and &&
#define and_eq &=
#define bitand &
#define bitor |
#define compl ~
#define not !
#define not_eq !=
#define or ||
#define or_eq |=
#define xor ^
#define xor_eq ^=

#endif /* _CINRS_ISO646_H */
