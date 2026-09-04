//! `_Generic` has to have an association for the controlling expression's
//! type, or a `default:`.

cinrs::c11! {
    int classify(double d) {
        return _Generic(d, int: 1, char *: 2); //~ ERROR: no association
    }

    int duplicated(int n) {
        return _Generic(n, int: 1, signed int: 2, default: 0); //~ ERROR: two associations
    }
}

fn main() {}
