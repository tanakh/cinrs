//! A lexical error must point at the constant, not at the macro call.

cinrs::c99! {
    int x = 08; //~ ERROR: invalid digit '8' in octal constant '08'
}

fn main() {}
