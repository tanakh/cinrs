//! Two `case` labels with the same value, and two `default`s.

cinrs::c99! {
    int pick(int n) {
        switch (n) {
            case 1: return 10;
            case 1 + 0: return 20; //~ ERROR: duplicate case value '1'
            default: return 30;
            default: return 40; //~ ERROR: multiple 'default' labels in one 'switch'
        }
    }
}

fn main() {}
