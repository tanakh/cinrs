//! An array bound at file scope must be a constant, and an array is not
//! assignable. A bound that is not a constant *inside a function* is a
//! variable length array, which is supported; `sema_vla_errors.rs` is where
//! the rules about those live.

cinrs::c99! {
    int length;
    int table[length]; //~ ERROR: array size is not an integer constant expression

    void assign_to_an_array(void) {
        int values[3];
        int other[3];
        values = other; //~ ERROR: array type 'int[3]' is not assignable
    }
}

fn main() {}
