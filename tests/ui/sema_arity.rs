//! A call with the wrong number of arguments points at the call, and says
//! where the function was declared.

cinrs::c99! {
    int add(int a, int b) {
        return a + b;
    }

    int use_add(void) {
        return add(1); //~ ERROR: too few arguments to function call, expected 2, have 1
    }
}

fn main() {}
