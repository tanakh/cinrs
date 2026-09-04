//! The rules `va_list` and the `<stdarg.h>` builtins are held to.
//!
//! These diagnose the *program*, so they are identical on every toolchain:
//! what an older Rust cannot compile is only reported for a program that has
//! nothing else wrong with it.

cinrs::c99! {
    #include <stdarg.h>

    struct Buffer {
        va_list saved; //~ ERROR: va_list is only supported as a local variable or parameter
        int used;
    };

    int first_char(int n, ...) {
        va_list ap;
        va_start(ap, n);
        return va_arg(ap, char); //~ ERROR: 'char' is promoted to 'int' when passed through '...'
    }

    int vsum(int n, va_list ap) {
        va_start(ap, n); //~ ERROR: 'va_start' used in a function with fixed arguments
        return va_arg(ap, int);
    }
}

fn main() {}
