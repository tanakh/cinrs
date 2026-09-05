/* <wctype.h> — wide character classification and mapping (C99 7.25).
 *
 * `wctype_t` and `wctrans_t` are handles: a program obtains one from `wctype`
 * or `wctrans` and hands it straight back to `iswctype` or `towctrans`, so
 * nothing here has to say what is inside one — only how wide it is, which the
 * calling convention does care about. The `#if` below spells each platform's.
 */
#ifndef _CINRS_WCTYPE_H
#define _CINRS_WCTYPE_H

#include <wchar.h>

#if defined(_WIN32)
typedef unsigned short wctype_t;
typedef int wctrans_t;
#elif defined(__APPLE__)
typedef unsigned int wctype_t;
typedef int wctrans_t;
#else
typedef unsigned long wctype_t;
typedef const int *wctrans_t;
#endif

/* -- classification (7.25.2) ------------------------------------------- */

int iswalnum(wint_t wc);
int iswalpha(wint_t wc);
int iswblank(wint_t wc);
int iswcntrl(wint_t wc);
int iswdigit(wint_t wc);
int iswgraph(wint_t wc);
int iswlower(wint_t wc);
int iswprint(wint_t wc);
int iswpunct(wint_t wc);
int iswspace(wint_t wc);
int iswupper(wint_t wc);
int iswxdigit(wint_t wc);

/* -- extensible classification (7.25.2.2) ------------------------------ */

int iswctype(wint_t wc, wctype_t desc);
wctype_t wctype(const char *property);

/* -- mapping (7.25.3) -------------------------------------------------- */

wint_t towlower(wint_t wc);
wint_t towupper(wint_t wc);

/* -- extensible mapping (7.25.3.2) ------------------------------------- */

wint_t towctrans(wint_t wc, wctrans_t desc);
wctrans_t wctrans(const char *property);

#endif /* _CINRS_WCTYPE_H */
