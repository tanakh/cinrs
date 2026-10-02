//@compile-flags: --crate-type lib
//! `safe` on a variadic definition.
//!
//! Rust makes every function with a C variable argument list `unsafe`: reading
//! an argument back out of the list is a promise about what the caller passed
//! that no signature can carry. So the request is refused with that reason
//! rather than becoming an item `rustc` would reject in words about `...`.

cinrs::c99! {
    #include <stdarg.h>

    //~v ERROR: 'total' takes '...'
    __attribute__((cinrs_safe)) int total(int n, ...) {
        va_list ap;
        int sum = 0;
        va_start(ap, n);
        for (int i = 0; i < n; i++) sum += va_arg(ap, int);
        va_end(ap);
        return sum;
    }
}
