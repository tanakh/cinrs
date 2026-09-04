//! An error inside a macro expansion lands on the invocation the user wrote,
//! and says which macro it came out of.

cinrs::c99! {
    #define SCALE(x) ((x) * factor)

    int twice(int n) {
        return SCALE(n); //~ ERROR: use of undeclared identifier 'factor'
    }
}

fn main() {}
