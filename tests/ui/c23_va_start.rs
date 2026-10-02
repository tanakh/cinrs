//! C23's `va_start` takes the list alone (N2975), and what it still refuses.
//!
//! Before C23 the bundled `<stdarg.h>` is the two-parameter macro, so the
//! one-argument form is a macro used with too few arguments — which is also
//! what `gcc -std=c11` says.

cinrs::c23! {
    #include <stdarg.h>

    int no_list(int n) {
        va_list ap; //~ ERROR: a 'va_list' variable can only be declared in a variadic function
        return n;
    }

    int nothing_to_start(int n, ...) {
        va_list ap;
        va_start(); //~ ERROR: 'va_start' expects at least 1 argument, have 0
        va_end(ap);
        return n;
    }

    int fixed(int n) {
        va_list *p = 0;
        va_start(*p); //~ ERROR: 'va_start' used in a function with fixed arguments
        return n;
    }
}

cinrs::c11! {
    #include <stdarg.h>

    int first(int n, ...) {
        va_list ap;
        va_start(ap); //~ ERROR: macro 'va_start' requires 2 arguments, but only 1 given
        int value = va_arg(ap, int);
        va_end(ap);
        return value;
    }
}

fn main() {}
