//@check-pass
//! A translation unit the front end accepts: nothing is reported and the
//! expansion is (for now) empty.

cinrs::c99! {
    typedef unsigned long size_t;

    struct Point { int x, y; };

    int fact(int n) {
        if (n == 0) {
            return 1;
        } else {
            return n * fact(n - 1);
        }
    }
}

fn main() {}
