//! What C rejects around pointers: dereferencing something that is not one,
//! and mixing pointer types (or integers and pointers) without a cast.

cinrs::c99! {
    int deref_an_int(int n) {
        return *n; //~ ERROR: indirection requires pointer operand ('int' invalid)
    }

    void assign_incompatible(int *p, char *s) {
        p = s; //~ ERROR: assigning to 'int *' from incompatible type 'char *'
    }

    void assign_an_integer(int *p, int n) {
        p = n; //~ ERROR: assigning to 'int *' from incompatible type 'int'
    }

    int to_an_integer(int *p) {
        return p; //~ ERROR: returning 'int *' from a function with incompatible result type 'int'
    }
}

fn main() {}
