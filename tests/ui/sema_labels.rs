//! `goto` needs a label, and a label may only be defined once per function.
//!
//! GNU's `&&label` needs one of *this* function's, and the computed
//! `goto *e` needs a pointer to jump through.

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

    void *address_of_nothing(void) {
        return &&nowhere; //~ ERROR: use of undeclared label 'nowhere'
    }

    int not_a_pointer(int n) {
        goto *n; //~ ERROR: the operand of a computed 'goto' must be a pointer, not 'int'
    here:
        return 0;
    }
}

fn main() {}
