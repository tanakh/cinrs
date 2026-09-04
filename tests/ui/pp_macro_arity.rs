//! A macro invoked with the wrong number of arguments is reported at the
//! invocation, not at the `#define`.

cinrs::c99! {
    #define MAX(a, b) ((a) > (b) ? (a) : (b))

    int pick(int n) {
        MAX(n); //~ ERROR: macro 'MAX' requires 2 arguments, but only 1 given
        return n;
    }
}

fn main() {}
