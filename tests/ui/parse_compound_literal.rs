//! A syntax error inside a compound literal's initialiser list points at the
//! token that is wrong, not at the literal it is written in.

cinrs::c99! {
    struct S { int a; int b; };

    struct S missing_element(void) {
        return (struct S){ 1, , 2 }; //~ ERROR: expected expression
    }
}

fn main() {}
