//@check-pass
//! A variadic definition, end to end: nothing is reported, and the generated
//! function is callable from both C and Rust.

cinrs::c99! {
    #include <stdarg.h>

    int vsnprintf(char *buf, unsigned long size, const char *fmt, va_list ap);

    int sum(int n, ...) {
        va_list ap;
        int total = 0;
        va_start(ap, n);
        for (int i = 0; i < n; i++) {
            total += va_arg(ap, int);
        }
        va_end(ap);
        return total;
    }

    int format(char *buf, unsigned long size, const char *fmt, ...) {
        va_list ap;
        int written;
        va_start(ap, fmt);
        written = vsnprintf(buf, size, fmt, ap);
        va_end(ap);
        return written;
    }

    int use_sum(void) {
        return sum(3, 1, 2, 3);
    }
}

fn main() {
    let total: ::core::ffi::c_int = unsafe { sum(2, 40, 2) };
    assert_eq!(total, 42);
    assert_eq!(unsafe { use_sum() }, 6);
}
