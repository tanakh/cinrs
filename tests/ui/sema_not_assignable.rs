//! Assigning to something that is not an lvalue.

cinrs::c99! {
    int assign_to_constant(int x) {
        1 = x; //~ ERROR: expression is not assignable
        return x;
    }
}

fn main() {}
