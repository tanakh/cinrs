//! What `__int128` cannot do.
//!
//! C has no integer constant wider than `unsigned long long`, so a 128-bit
//! value is *built*: `((__int128) 1) << 100`. Writing `1 << 100` shifts an
//! `int` by more than its width, which is what the first two diagnostics are
//! about — the type of a shift is the type of its left operand, and widening
//! the result afterwards does not go back and redo it.

cinrs::gnu11! {
    static __int128 too_far = 1 << 100;
    //~^ ERROR: shift count 100 is out of range for type 'int'
    //~| ERROR: initializer is not a compile-time constant expression

    /* The idiom, which is fine. */
    static __int128 built = ((__int128) 1) << 100;

    static unsigned long long huge = 18446744073709551616;
    //~^ ERROR: is too large for any integer type

    /* `__int128` is a type specifier of its own; nothing but `signed` and
       `unsigned` goes with it. */
    long __int128 mixed; //~ ERROR: cannot combine '__int128' with other type specifiers

    int overflows(void) {
        unsigned __int128 a = 1, b = 2, r;
        return __builtin_add_overflow(a, b, &r);
        //~^ ERROR: with a 128-bit operand is not supported
    }

    /* Reading a 128-bit value back out of an argument list needs a `VaArgSafe`
       impl that is still unstable, so it is refused rather than mistranslated.
       Passing one *through* `...` is fine. */
    __int128 read_one(int n, ...) {
        __builtin_va_list ap;
        __builtin_va_start(ap, n);
        __int128 v = __builtin_va_arg(ap, __int128); //~ ERROR: c_variadic_int128
        __builtin_va_end(ap);
        return v;
    }

    /* Only the *operands* are the problem: the check is computed one width up
       from them, and a 128-bit result type only changes how it is read. This
       one is fine. */
    int narrow_operands(int a, int b) {
        __int128 r;
        return __builtin_mul_overflow(a, b, &r) + (int) r;
    }
}

fn main() {}
