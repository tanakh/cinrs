//! `goto` needs a label, and a label may only be defined once per function.
//!
//! GNU's `&&label` needs one of *this* function's, and the computed
//! `goto *e` needs a pointer to jump through. A statement expression is an
//! expression, and how a function's jumps are lowered is read off its
//! statements, so neither a label nor a `goto` may be buried in one.

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

cinrs::gnu99! {
    int jumping_out_of_an_expression(int n) {
        int v = ({ if (n) goto out; //~ ERROR: a 'goto' cannot appear inside a statement expression
                   n + 1; });
        return v;
    out:
        return 0;
    }

    int a_label_in_an_expression(int n) {
        return ({ if (n) { skip: n++; } n; }); //~ ERROR: a label cannot appear inside a statement expression
    }

    /* Leaving an enclosing loop from inside a statement expression is fine —
       it is a Rust block, and `break` in one means what C says — until the
       function is lowered through a control-flow graph, which it cannot leave. */
    int leaving_a_loop(int n) {
        void *entry = &&spin;
        (void)entry;
        while (n) {
            n -= ({ if (n > 5) break; //~ ERROR: a 'break' inside a statement expression is not supported
                    1; });
        }
    spin:
        return n;
    }
}

fn main() {}
