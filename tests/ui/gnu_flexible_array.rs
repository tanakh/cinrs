//! A flexible array member is an array of no elements at the end of a struct,
//! which the program is expected to over-allocate for. Anywhere else it has no
//! meaning.
//!
//! Initialising one is GNU's extension and is allowed for an object with
//! static storage duration; `tests/ui/flexible_array_initializer.rs` is where
//! the cases that are not go.

cinrs::c99! {
    struct NotLast {
        int data[];  //~ ERROR: must be the last member
        int len;
    };

    union InAUnion {
        int n;
        int data[]; //~ ERROR: not allowed in a union
    };
}

fn main() {}
