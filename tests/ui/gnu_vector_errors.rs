//! GCC's vector extensions: what GCC refuses is refused, with GCC's reason,
//! and so is passing a vector by value to a function the unit does not
//! define, whose convention is a vector register rather than the struct the
//! vector is generated as.

cinrs::gnu11! {
    typedef int v4si __attribute__((vector_size(16)));
    typedef unsigned int v4su __attribute__((vector_size(16)));
    typedef float v4sf __attribute__((vector_size(16)));
    typedef char v16qi __attribute__((vector_size(16)));
    typedef short v4hi __attribute__((vector_size(8)));

    /* The elements are integers or real floating numbers, a power of two of
     * them, filling the size exactly. */
    struct s { int x; };
    typedef struct s vs __attribute__((vector_size(16))); //~ ERROR: invalid vector type
    typedef int v3si __attribute__((vector_size(12))); //~ ERROR: not a power of two
    typedef double odd __attribute__((vector_size(12))); //~ ERROR: not an integral multiple
    typedef int none __attribute__((vector_size(0))); //~ ERROR: zero vector size

    /* A scalar is broadcast only when it converts without losing anything. */
    v4si fraction(v4si a) { return a + 1.5; } //~ ERROR: cannot convert 'double' to the integer vector
    v16qi narrow(v16qi a, int i) { return a + i; } //~ ERROR: involves truncation
    v4sf inexact(v4sf a) { return a * 0.1; } //~ ERROR: involves truncation

    /* Two vectors of different types need a cast, and a floating vector has
     * no remainder or bitwise operators. */
    v4si mixed(v4si a, v4su b) { return a + b; } //~ ERROR: the two vectors have different types
    v4sf remainder(v4sf a, v4sf b) { return a % b; } //~ ERROR: the elements are floating
    v4sf complement(v4sf a) { return ~a; } //~ ERROR: wrong type argument to unary '~'

    /* `!`, `&&`, `||` and a vector condition are GCC's C++ only. */
    int not(v4si a) { return !a; } //~ ERROR: to unary operator '!'
    int cond(v4si a) { if (a) return 1; return 0; } //~ ERROR: not contextually convertible

    /* A cast keeps the bytes, so the sizes have to agree. */
    v4hi shrink(v4si a) { return (v4hi)a; } //~ ERROR: which has a different size

    /* A constant element index is checked. */
    int past_the_end(v4si a) { return a[4]; } //~ ERROR: index 4 is out of range

    /* The builtins check their operands as GCC does. */
    v4sf convert(v16qi a) { return __builtin_convertvector(a, v4sf); } //~ ERROR: should be the same
    v4si shuffle(v4si a, v4sf m) { return __builtin_shuffle(a, m); } //~ ERROR: last argument must be an integer vector

    /* A vector by value to a function compiled elsewhere would be in the
     * wrong registers; so would one in the variable part of a call. */
    v4si elsewhere(v4si a);
    int printf(const char *, ...);
    v4si call_out(v4si a) { return elsewhere(a); } //~ ERROR: passes or returns the vector type
    void print(v4si a) { printf("%d", a); } //~ ERROR: as a variadic argument
}

fn main() {}
