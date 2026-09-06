/* <complex.h> — complex arithmetic (C99 7.3).
 *
 * # What is here
 *
 * The `complex`, `_Complex_I` and `I` macros, C11's `CMPLX` family (N1464),
 * and the function declarations. `creal`, `cimag`, `conj` and `cproj` are the
 * compiler's own builtins — they are field accesses and a sign flip, and
 * calling into libm for one would be absurd — and everything else is an
 * ordinary declaration that links against the platform's library, exactly as
 * <math.h> does.
 *
 * # Linking
 *
 * Nothing here asks for `-lm`; see <math.h> for why, and for the
 * `#pragma cinrs link "m"` to write if a platform ever needs it.
 *
 * # What is not here
 *
 * `<tgmath.h>`, whose type-generic macros would need a `_Generic` over every
 * arithmetic type for each of forty names. `_Imaginary` and `_Imaginary_I`:
 * no compiler implements the imaginary types and C99 7.3.1p3 makes them
 * optional, which is why `__STDC_IEC_559_COMPLEX__` is not defined either.
 * The `long double` forms are declared and are the `double` ones, since
 * `long double` **is** `double` in cinrs — the same mapping, and the same loss
 * of precision, that <math.h> documents.
 */
#ifndef _CINRS_COMPLEX_H
#define _CINRS_COMPLEX_H

#ifdef __STDC_NO_COMPLEX__
#error "<complex.h> needs the 'complex' feature of the cinrs crate. It is on by \
default; this build switched it off (default-features = false), which is what \
predefines __STDC_NO_COMPLEX__."
#elif defined(__STRICT_ANSI__) && !defined(__STDC_VERSION__)
#error "<complex.h> and '_Complex' are C99; this block is c89!. Write c99! or \
later, or gnu89! for the GNU dialect of C89, which has them."
#else

#define __STDC_VERSION_COMPLEX_H__ 202311L

#define complex _Complex

/* C99 7.3.1p4: `_Complex_I` has type `const float _Complex` and a value whose
 * imaginary part is one. GCC's own header writes exactly this. */
#define _Complex_I (__extension__ 1.0iF)

/* 7.3.1p6 lets a program `#undef I` and use the name for something else, which
 * is why it is a macro of its own rather than a second spelling. */
#define I _Complex_I

/* C11 7.3.9.3 (N1464): build a complex value from its two parts. `x + y * I`
 * cannot do it — `y * I` is a multiplication, so an infinite `y` gives a NaN
 * real part — and this is the builtin both GCC and Clang provide for it. */
#define CMPLX(x, y) __builtin_complex((double) (x), (double) (y))
#define CMPLXF(x, y) __builtin_complex((float) (x), (float) (y))
#define CMPLXL(x, y) __builtin_complex((double) (x), (double) (y))

/* -- 7.3.9 manipulation functions --------------------------------------- */

/* Each of these is a couple of instructions, so each is the builtin rather
 * than a call: `creal` is a field, `conj` flips a sign bit, and `cproj` is a
 * test and a `copysign`. */
#define creal(z) __builtin_creal(z)
#define crealf(z) __builtin_crealf(z)
#define creall(z) __builtin_creall(z)
#define cimag(z) __builtin_cimag(z)
#define cimagf(z) __builtin_cimagf(z)
#define cimagl(z) __builtin_cimagl(z)
#define conj(z) __builtin_conj(z)
#define conjf(z) __builtin_conjf(z)
#define conjl(z) __builtin_conjl(z)
#define cproj(z) __builtin_cproj(z)
#define cprojf(z) __builtin_cprojf(z)
#define cprojl(z) __builtin_cprojl(z)

/* -- 7.3.8 absolute value and argument ----------------------------------- */

double cabs(double _Complex z);
float cabsf(float _Complex z);
double cabsl(double _Complex z);
double carg(double _Complex z);
float cargf(float _Complex z);
double cargl(double _Complex z);

/* -- 7.3.5 trigonometric functions --------------------------------------- */

double _Complex cacos(double _Complex z);
float _Complex cacosf(float _Complex z);
double _Complex cacosl(double _Complex z);
double _Complex casin(double _Complex z);
float _Complex casinf(float _Complex z);
double _Complex casinl(double _Complex z);
double _Complex catan(double _Complex z);
float _Complex catanf(float _Complex z);
double _Complex catanl(double _Complex z);
double _Complex ccos(double _Complex z);
float _Complex ccosf(float _Complex z);
double _Complex ccosl(double _Complex z);
double _Complex csin(double _Complex z);
float _Complex csinf(float _Complex z);
double _Complex csinl(double _Complex z);
double _Complex ctan(double _Complex z);
float _Complex ctanf(float _Complex z);
double _Complex ctanl(double _Complex z);

/* -- 7.3.6 hyperbolic functions ------------------------------------------ */

double _Complex cacosh(double _Complex z);
float _Complex cacoshf(float _Complex z);
double _Complex cacoshl(double _Complex z);
double _Complex casinh(double _Complex z);
float _Complex casinhf(float _Complex z);
double _Complex casinhl(double _Complex z);
double _Complex catanh(double _Complex z);
float _Complex catanhf(float _Complex z);
double _Complex catanhl(double _Complex z);
double _Complex ccosh(double _Complex z);
float _Complex ccoshf(float _Complex z);
double _Complex ccoshl(double _Complex z);
double _Complex csinh(double _Complex z);
float _Complex csinhf(float _Complex z);
double _Complex csinhl(double _Complex z);
double _Complex ctanh(double _Complex z);
float _Complex ctanhf(float _Complex z);
double _Complex ctanhl(double _Complex z);

/* -- 7.3.7 exponential and logarithmic functions ------------------------- */

double _Complex cexp(double _Complex z);
float _Complex cexpf(float _Complex z);
double _Complex cexpl(double _Complex z);
double _Complex clog(double _Complex z);
float _Complex clogf(float _Complex z);
double _Complex clogl(double _Complex z);

/* -- 7.3.8 power and absolute-value functions ---------------------------- */

double _Complex cpow(double _Complex x, double _Complex y);
float _Complex cpowf(float _Complex x, float _Complex y);
double _Complex cpowl(double _Complex x, double _Complex y);
double _Complex csqrt(double _Complex z);
float _Complex csqrtf(float _Complex z);
double _Complex csqrtl(double _Complex z);

#endif /* __STDC_NO_COMPLEX__ */
#endif /* _CINRS_COMPLEX_H */
