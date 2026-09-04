//! A syntax error must point at the exact token that could not be parsed.

cinrs::c99! {
    int add(int a, int b) {
        return a + ; //~ ERROR: expected expression, found ';'
    }
}

fn main() {}
