//! A compound literal is an object, and the rules of an object apply to it:
//! at file scope it has static storage duration, so its initialiser has to be
//! a constant expression, and a `const`-qualified one cannot be assigned to.

cinrs::c99! {
    int next(void);

    int *at_file_scope = &(int){ next() }; //~ ERROR: not a compile-time constant

    int write_to_a_const(void) {
        return (const int){ 1 } = 2; //~ ERROR: const-qualified
    }

    struct Incomplete;

    struct Incomplete *of_an_incomplete_type(void) {
        return &(struct Incomplete){ 0 }; //~ ERROR: complete object type
    }
}

fn main() {}
