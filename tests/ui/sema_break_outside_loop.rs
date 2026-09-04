//! `break` needs something to break out of.

cinrs::c99! {
    void stray_break(void) {
        break; //~ ERROR: 'break' statement not in a loop or 'switch' statement
    }
}

fn main() {}
