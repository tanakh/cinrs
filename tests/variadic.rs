//! Integration tests that *run* translated C defining variadic functions.
//!
//! Defining one needs Rust's `c_variadic`, stable since 1.99, so the whole file
//! is skipped on an older toolchain — where the crate refuses the definition
//! with a diagnostic saying exactly that, rather than emitting code the
//! compiler would reject. Declaring and calling a variadic function needs
//! nothing new and is covered by the other integration tests.

/// Everything here needs `core::ffi::VaList` and `...` in a definition.
#[rustversion::since(1.99)]
mod definitions {
    use cinrs::c99;

    #[test]
    fn sum_of_ints() {
        c99! {
            #include <stdarg.h>

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

            /* Called from C, where the argument list is built by the caller. */
            int sum_of_1_to_4(void) {
                return sum(4, 1, 2, 3, 4);
            }
        }

        unsafe {
            assert_eq!(sum_of_1_to_4(), 10);
            assert_eq!(sum(0), 0);
            // ... and from Rust, with Rust's own variadic call syntax.
            assert_eq!(sum(3, 1, 2, 3), 6);
            assert_eq!(sum(2, -5, 5), 0);
        }
    }

    #[test]
    fn arguments_of_every_kind() {
        c99! {
            #include <stdarg.h>

            /* One list, read at four different types. */
            long mixed(int first, ...) {
                va_list ap;
                va_start(ap, first);
                int i = va_arg(ap, int);
                double d = va_arg(ap, double);
                long l = va_arg(ap, long);
                int *p = va_arg(ap, int *);
                va_end(ap);
                return (long) first + i + (long) d + l + *p;
            }
        }

        let mut value: core::ffi::c_int = 1000;
        let total = unsafe { mixed(1, 2, 3.75, 4i64, &raw mut value) };
        assert_eq!(total, 1 + 2 + 3 + 4 + 1000);
    }

    #[test]
    fn default_argument_promotions_are_applied_at_the_call() {
        c99! {
            #include <stdarg.h>

            double add_doubles(int n, ...) {
                va_list ap;
                double total = 0;
                va_start(ap, n);
                for (int i = 0; i < n; i++) total += va_arg(ap, double);
                va_end(ap);
                return total;
            }

            int add_ints(int n, ...) {
                va_list ap;
                int total = 0;
                va_start(ap, n);
                for (int i = 0; i < n; i++) total += va_arg(ap, int);
                va_end(ap);
                return total;
            }

            /* A `float` argument is passed as a `double`, and a `char` or a
               `short` as an `int`; the callee reads the promoted types. */
            double promoted_float(void) {
                float f = 1.5f;
                return add_doubles(2, f, 2.25);
            }

            int promoted_small_integers(void) {
                char c = 3;
                short s = 4;
                unsigned char u = 200;
                return add_ints(3, c, s, u);
            }
        }

        unsafe {
            assert_eq!(promoted_float(), 3.75);
            assert_eq!(promoted_small_integers(), 207);
        }
    }

    #[test]
    fn va_copy_reads_the_list_twice() {
        c99! {
            #include <stdarg.h>

            int first_and_third(int n, ...) {
                va_list ap;
                va_list copy;
                int first;
                int third;
                va_start(ap, n);
                va_copy(copy, ap);
                first = va_arg(ap, int);
                third = va_arg(copy, int);
                third = va_arg(copy, int);
                third = va_arg(copy, int);
                va_end(copy);
                va_end(ap);
                return first * 100 + third;
            }
        }

        unsafe {
            assert_eq!(first_and_third(3, 7, 8, 9), 709);
        }
    }

    #[test]
    fn a_va_list_parameter_takes_over() {
        c99! {
            #include <stdarg.h>

            /* The `vfoo` half of the usual pair: it takes the list by value. */
            int vsum(int n, va_list ap) {
                int total = 0;
                for (int i = 0; i < n; i++) total += va_arg(ap, int);
                return total;
            }

            /* A helper of its own may copy the parameter. */
            int vsum_twice(int n, va_list ap) {
                va_list copy;
                va_copy(copy, ap);
                int once = vsum(n, ap);
                int again = vsum(n, copy);
                va_end(copy);
                return once + again;
            }

            int sum(int n, ...) {
                va_list ap;
                va_start(ap, n);
                int total = vsum(n, ap);
                va_end(ap);
                return total;
            }

            int sum_twice(int n, ...) {
                va_list ap;
                va_start(ap, n);
                int total = vsum_twice(n, ap);
                va_end(ap);
                return total;
            }
        }

        unsafe {
            assert_eq!(sum(3, 10, 20, 30), 60);
            assert_eq!(sum_twice(2, 1, 2), 6);
        }
    }

    #[test]
    fn forwarding_to_libc() {
        c99! {
            #include <stdarg.h>

            int vsnprintf(char *buf, unsigned long size, const char *fmt, va_list ap);

            /* The whole reason `va_list` has to be passable by value. */
            int format(char *buf, unsigned long size, const char *fmt, ...) {
                va_list ap;
                int written;
                va_start(ap, fmt);
                written = vsnprintf(buf, size, fmt, ap);
                va_end(ap);
                return written;
            }
        }

        let mut buffer = [0u8; 64];
        let written = unsafe {
            format(
                buffer.as_mut_ptr().cast(),
                buffer.len() as core::ffi::c_ulong,
                c"%d %s %.2f".as_ptr(),
                42,
                c"cinrs".as_ptr(),
                1.5f64,
            )
        };
        assert_eq!(written, 13);
        assert_eq!(&buffer[..13], b"42 cinrs 1.50");
    }

    #[test]
    fn a_pointer_to_a_struct_can_be_read_back() {
        c99! {
            #include <stdarg.h>

            struct Point { int x; int y; };

            int sum_points(int n, ...) {
                va_list ap;
                int total = 0;
                va_start(ap, n);
                for (int i = 0; i < n; i++) {
                    struct Point *p = va_arg(ap, struct Point *);
                    total += p->x + p->y;
                }
                va_end(ap);
                return total;
            }
        }

        let mut a = Point { x: 1, y: 2 };
        let mut b = Point { x: 30, y: 40 };
        let total = unsafe { sum_points(2, &raw mut a, &raw mut b) };
        assert_eq!(total, 73);
    }

    #[test]
    fn va_start_may_run_again_after_va_end() {
        c99! {
            #include <stdarg.h>

            /* C allows a list to be started, ended and started again; the
               second `va_start` rewinds to the first variable argument. */
            int first_twice(int n, ...) {
                va_list ap;
                int once;
                int again;
                va_start(ap, n);
                once = va_arg(ap, int);
                va_end(ap);
                va_start(ap, n);
                again = va_arg(ap, int);
                va_end(ap);
                return once * 100 + again;
            }
        }

        unsafe {
            assert_eq!(first_twice(2, 7, 9), 707);
        }
    }

    #[test]
    fn a_variadic_function_may_also_need_a_control_flow_graph() {
        c99! {
            #include <stdarg.h>

            /* `goto` hoists every local to the top of the function, including
               the `va_list` — which cannot be zero-initialised. */
            int sum_with_goto(int n, ...) {
                va_list ap;
                int total = 0;
                int i = 0;
                va_start(ap, n);
            again:
                if (i >= n) goto done;
                total += va_arg(ap, int);
                i++;
                goto again;
            done:
                va_end(ap);
                return total;
            }
        }

        unsafe {
            assert_eq!(sum_with_goto(4, 1, 2, 3, 4), 10);
            assert_eq!(sum_with_goto(0), 0);
        }
    }
}
