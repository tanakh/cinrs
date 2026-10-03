//! `long double` at the boundary with the platform.
//!
//! `long double` is `double` here, which is self-consistent for the C a unit
//! defines and wrong for the platform's library wherever its `long double` is
//! wider: x87's eighty bits on x86-64 System V. The ISO C functions whose only
//! difference from a `double` sibling is the type (`strtold`, `powl`, …) are
//! linked to the sibling; a literal `printf` or `scanf` format that names
//! every `long double` argument with `L` has the `L`s made `l`; everything
//! else is refused where it is *used*, with the rewrite.
//!
//! The block names its target, so the diagnostics are the same wherever the
//! test is run.

cinrs::gnu11! {
    #pragma cinrs target "x86_64-unknown-linux-gnu"

    int printf(const char *fmt, ...);
    int sscanf(const char *s, const char *fmt, ...);
    long double strtold(const char *nptr, char **endptr);

    /* Declared, never used: platform headers declare dozens of these. */
    long double frexp10l(long double x, int *e);

    /* Not an ISO C function with a `double` twin. */
    long double my_scale(long double x);
    void my_fill(long double *out);

    double read_it(const char *s, const char *fmt) {
        long double x = strtold(s, 0);
        /* Named with `L` by a literal format: `%lf` is what the platform reads. */
        printf("%Lf %.3Le\n", x, x);
        sscanf(s, "%Lf", &x);
        /* A format that is not a literal, or that does not say `L`. */
        printf(fmt, x);
        //~^ ERROR: a 'long double' cannot be passed to the platform's 'printf'
        printf("%f\n", x);
        //~^ ERROR: a 'long double' cannot be passed to the platform's 'printf'
        sscanf(s, fmt, &x);
        //~^ ERROR: a 'long double *' cannot be passed to the platform's 'sscanf'
        my_fill(&x);
        //~^ ERROR: 'my_fill' takes a 'long double *'
        printf("%f\n", (double) x);
        return (double) my_scale(x);
        //~^ ERROR: 'my_scale' returns a 'long double'
    }
}

fn main() {}
