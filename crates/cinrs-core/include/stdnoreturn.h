/* <stdnoreturn.h> — _Noreturn (C11 7.23).
 *
 * C23 deprecates this header in favour of the `[[noreturn]]` attribute, which
 * cinrs also accepts; the macro keeps working either way.
 */
#ifndef _CINRS_STDNORETURN_H
#define _CINRS_STDNORETURN_H

#define noreturn _Noreturn

#endif /* _CINRS_STDNORETURN_H */
