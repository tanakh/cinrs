//! An unknown name is reported in C's words, at the C identifier.

cinrs::c99! {
    int use_missing(void) {
        return missing_value; //~ ERROR: use of undeclared identifier 'missing_value'
    }
}

fn main() {}
