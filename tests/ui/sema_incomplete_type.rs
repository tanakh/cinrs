//! `sizeof` needs to know how big something is, and a tag nobody defined is
//! not something it can measure.

cinrs::c99! {
    struct Opaque;

    unsigned long size_of_opaque(void) {
        return sizeof(struct Opaque); //~ ERROR: invalid application of 'sizeof' to an incomplete type 'struct Opaque'
    }

    unsigned long size_of_unknown(void) {
        return sizeof(struct NeverMentioned); //~ ERROR: invalid application of 'sizeof' to an incomplete type 'struct NeverMentioned'
    }

    int declare_an_opaque_object(void) {
        struct Opaque value; //~ ERROR: variable 'value' has incomplete type 'struct Opaque'
        return 0;
    }
}

fn main() {}
