//! A flexible array member is an array of no elements at the end of a struct,
//! which the program is expected to over-allocate for. Anywhere else it has no
//! meaning, and there is nothing an initialiser could put in it.

cinrs::c99! {
    struct NotLast {
        int data[];  //~ ERROR: must be the last member
        int len;
    };

    union InAUnion {
        int n;
        int data[]; //~ ERROR: not allowed in a union
    };

    struct Buffer { int len; int data[]; };

    struct Buffer filled = { 2, { 1, 2 } }; //~ ERROR: cannot be initialized
}

fn main() {}
