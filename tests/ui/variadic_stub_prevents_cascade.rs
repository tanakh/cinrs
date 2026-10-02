//! A variadic definition whose body does not check still leaves a callable
//! item behind, `...` and all, so the Rust call below adds nothing to the one
//! real error.

cinrs::c99! {
    #include <stdarg.h>

    int sum(int n, ...) {
        va_list ap;
        va_start(ap, n);
        return missing_helper(va_arg(ap, int)); //~ ERROR: implicit declaration of function 'missing_helper'
    }
}

fn main() {
    let value: ::core::ffi::c_int = unsafe { sum(2, 1, 2) };
    let _ = value;
}
