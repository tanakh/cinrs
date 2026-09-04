//! `goto` needs a label, and a label may only be defined once per function.

cinrs::c99! {
    int jump(int n) {
        if (n < 0) {
            goto dnoe; //~ ERROR: use of undeclared label 'dnoe'
        }
        n = n * 2;
    done:
        return n;
    }

    int twice(int n) {
    again:
        n++;
        if (n < 10) goto again;
    again: //~ ERROR: redefinition of label 'again'
        return n;
    }
}

fn main() {}
