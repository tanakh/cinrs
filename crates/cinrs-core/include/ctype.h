/* <ctype.h> — character handling (C99 7.4).
 *
 * Declared as functions, never as the table-lookup macros a real library also
 * provides: every one of these is a real symbol in the platform's C library,
 * and a call is what cinrs generates best.
 */
#ifndef _CINRS_CTYPE_H
#define _CINRS_CTYPE_H

int isalnum(int c);
int isalpha(int c);
int isblank(int c);
int iscntrl(int c);
int isdigit(int c);
int isgraph(int c);
int islower(int c);
int isprint(int c);
int ispunct(int c);
int isspace(int c);
int isupper(int c);
int isxdigit(int c);
int tolower(int c);
int toupper(int c);

#endif /* _CINRS_CTYPE_H */
