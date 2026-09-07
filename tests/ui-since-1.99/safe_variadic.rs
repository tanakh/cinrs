//@compile-flags: --crate-type lib
//! `safe` on a variadic definition.
//!
//! Rust makes every function with a C variable argument list `unsafe`: reading
//! an argument back out of the list is a promise about what the caller passed
//! that no signature can carry. So the request is refused with that reason
//! rather than becoming an item `rustc` would reject in words about `...`.
//!
//! The test lives in this suite because a variadic *definition* needs Rust
//! 1.99, and an older toolchain says so as well — one message on a toolchain
//! that can compile the shape at all.

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
