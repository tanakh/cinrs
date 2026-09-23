/* <math.h> — mathematics (C99 7.12).
 *
 * # Linking
 *
 * Nothing here asks for `-lm`. On glibc 2.35 and later the maths functions are
 * part of libc itself, and on every target Rust supports the standard library
 * already links whatever else is needed, so a call to `sqrt` from a `c99!`
 * block resolves with no help. A platform where that stops being true does not
 * need a new header — put
 *
 *     #pragma cinrs link "m"
 *
 * in the translation unit and the generated `extern` block carries
 * `#[link(name = "m")]`.
 *
 * # Classification and comparison
 *
 * C99's classification macros (7.12.3: `fpclassify`, `isfinite`, `isinf`,
 * `isnan`, `isnormal`, `signbit`) and quiet comparisons (7.12.14:
 * `isgreater` … `isunordered`) are GCC's own definitions, one builtin each,
 * and so type-generic over `float`, `double` and `long double` the way the
 * standard asks. Like the platform's header, they are left out of strict
 * C89 (`c89!`), where the names still belong to the program; `gnu89!` has
 * them.
 *
 * # What is not here
 *
 * The type-generic macros of <tgmath.h> and `long double` (which cinrs makes
 * a `double`) are absent. `NAN` and `INFINITY` are constant expressions,
 * spelt as GCC spells them.
 */
#ifndef _CINRS_MATH_H
#define _CINRS_MATH_H

/* An overflowing decimal constant is infinity, exactly as the standard says a
 * constant too large for its type becomes. */
#define HUGE_VAL 1e999
#define HUGE_VALF 1e999f
#define HUGE_VALL __builtin_huge_vall()
#define INFINITY __builtin_inff()
/* A positive quiet NaN, where `0.0f / 0.0f` has whatever sign the host's
 * division gives it. */
#define NAN __builtin_nanf("")

#if defined(__STDC_VERSION__) || !defined(__STRICT_ANSI__)

#define FP_NAN 0
#define FP_INFINITE 1
#define FP_ZERO 2
#define FP_SUBNORMAL 3
#define FP_NORMAL 4

#define fpclassify(x) \
    __builtin_fpclassify(FP_NAN, FP_INFINITE, FP_NORMAL, FP_SUBNORMAL, FP_ZERO, x)
#define isfinite(x) __builtin_isfinite(x)
#define isinf(x) __builtin_isinf_sign(x)
#define isnan(x) __builtin_isnan(x)
#define isnormal(x) __builtin_isnormal(x)
#define signbit(x) __builtin_signbit(x)

#define isgreater(x, y) __builtin_isgreater(x, y)
#define isgreaterequal(x, y) __builtin_isgreaterequal(x, y)
#define isless(x, y) __builtin_isless(x, y)
#define islessequal(x, y) __builtin_islessequal(x, y)
#define islessgreater(x, y) __builtin_islessgreater(x, y)
#define isunordered(x, y) __builtin_isunordered(x, y)

/* The functions are the platform's libm. glibc, musl and the UCRT set
 * `errno` and raise the exception; Apple's libm only raises it. */
#define MATH_ERRNO 1
#define MATH_ERREXCEPT 2
#ifdef __APPLE__
#define math_errhandling MATH_ERREXCEPT
#else
#define math_errhandling (MATH_ERRNO | MATH_ERREXCEPT)
#endif

#endif /* C99 or a GNU dialect */

#define M_E 2.7182818284590452354
#define M_LOG2E 1.4426950408889634074
#define M_LOG10E 0.43429448190325182765
#define M_LN2 0.69314718055994530942
#define M_LN10 2.30258509299404568402
#define M_PI 3.14159265358979323846
#define M_PI_2 1.57079632679489661923
#define M_PI_4 0.78539816339744830962
#define M_1_PI 0.31830988618379067154
#define M_2_PI 0.63661977236758134308
#define M_2_SQRTPI 1.12837916709551257390
#define M_SQRT2 1.41421356237309504880
#define M_SQRT1_2 0.70710678118654752440

double sin(double x);
double cos(double x);
double tan(double x);
double asin(double x);
double acos(double x);
double atan(double x);
double atan2(double y, double x);
double sinh(double x);
double cosh(double x);
double tanh(double x);
double exp(double x);
double exp2(double x);
double log(double x);
double log10(double x);
double log2(double x);
double pow(double x, double y);
double sqrt(double x);
double cbrt(double x);
double fabs(double x);
double floor(double x);
double ceil(double x);
double round(double x);
double trunc(double x);
double fmod(double x, double y);
double fmin(double x, double y);
double fmax(double x, double y);
double hypot(double x, double y);
double copysign(double x, double y);
double ldexp(double x, int exp);
double frexp(double value, int *exp);
double modf(double value, double *iptr);

float sinf(float x);
float cosf(float x);
float tanf(float x);
float asinf(float x);
float acosf(float x);
float atanf(float x);
float atan2f(float y, float x);
float sinhf(float x);
float coshf(float x);
float tanhf(float x);
float expf(float x);
float exp2f(float x);
float logf(float x);
float log10f(float x);
float log2f(float x);
float powf(float x, float y);
float sqrtf(float x);
float cbrtf(float x);
float fabsf(float x);
float floorf(float x);
float ceilf(float x);
float roundf(float x);
float truncf(float x);
float fmodf(float x, float y);
float fminf(float x, float y);
float fmaxf(float x, float y);
float hypotf(float x, float y);
float copysignf(float x, float y);
float ldexpf(float x, int exp);
float frexpf(float value, int *exp);
float modff(float value, float *iptr);

#endif /* _CINRS_MATH_H */
