//! Bit-fields have no natural Rust translation and are rejected outright.

cinrs::c99! {
    struct Flags {
        unsigned int ready : 1; //~ ERROR: bit-fields are not supported yet
        unsigned int mode : 3; //~ ERROR: bit-fields are not supported yet
    };

    int use_flags(struct Flags f) {
        return 0;
    }
}

fn main() {}
