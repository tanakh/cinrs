//! What the complex types do *not* have.
//!
//! The complex numbers are not ordered, so the relational operators mean
//! nothing on them; `%`, the bitwise operators and the shifts want integers;
//! a complex *integer* type is a GNU extension of its own that nothing here
//! could be; `_Imaginary` is a type no compiler implements; and a complex
//! value is a pair, which rules out a bit-field and an `_Atomic` object.
//! (`va_arg` of one is *not* on the list: a pair is read back the way any
//! other aggregate is — see `tests/complex.rs`.)

cinrs::c11! {
    #include <complex.h>

    int ordered(double _Complex a, double _Complex b) { return a < b; } //~ ERROR: are not ordered
    int ordered2(double _Complex a, double b) { return a >= b; } //~ ERROR: are not ordered
    double _Complex remainder_(double _Complex a, double _Complex b) { return a % b; } //~ ERROR: requires integer operands
    double _Complex masked(double _Complex a, int b) { return a & b; } //~ ERROR: requires integer operands
    double _Complex shifted(double _Complex a, int b) { return a << b; } //~ ERROR: requires integer operands

    /* GCC's complex integer types. */
    _Complex int ci; //~ ERROR: complex integer type
    __complex__ long cl; //~ ERROR: complex integer type

    /* The imaginary types, which no compiler implements. */
    double _Imaginary imaginary_object; //~ ERROR: imaginary types are not supported

    /* A pair has no lock-free atomic and cannot be a bit-field. */
    _Atomic double _Complex shared; //~ ERROR: is not supported yet
    struct Packed { double _Complex z : 3; }; //~ ERROR: only the integer types may be given a width

    /* `__imag__` of a real lvalue is a zero, and a zero has no address. */
    void assign_imag(double x) { __imag__ x = 1.0; } //~ ERROR: is a zero, and a zero is not assignable

    /* `__builtin_complex` wants two operands of one real floating type. */
    double _Complex mixed(void) { return __builtin_complex(1.0, 2.0f); } //~ ERROR: different types
    double _Complex integral(void) { return __builtin_complex(1, 2); } //~ ERROR: real floating type
}

cinrs::c89! {
    /* `_Complex` is C99, and the strict C89 entry point says which macro has
       it. So does the header. */
    double c89_complex(void) { double _Complex z = 0; return 0; } //~ ERROR: '_Complex' requires C99
}

fn main() {}
