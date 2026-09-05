//! `restrict` may only qualify a pointer to an object type (C99 6.7.3p2), and
//! the two declaration rules C23 relaxed are still errors in an earlier block.

cinrs::c99! {
    int *restrict fine;
    int restrict not_a_pointer; //~ ERROR: 'restrict' requires a pointer to an object type ('int' is invalid)
    void (*restrict to_a_function)(void); //~ ERROR: 'restrict' qualifies a pointer to an object type

    typedef int *int_ptr;
    int_ptr restrict through_a_typedef;

    void array_parameter(int a[restrict]);

    struct S {
        int restrict member; //~ ERROR: 'restrict' requires a pointer to an object type ('int' is invalid)
    };
}

cinrs::c17! {
    int second(int, int b) { //~ ERROR: omitting a parameter name in a function definition requires C23
        return b;
    }

    enum too_wide {
        fits = 1,
        does_not = 4294967296 //~ ERROR: enumerator value 4294967296 is outside the range of 'int'
    };
}

fn main() {}
