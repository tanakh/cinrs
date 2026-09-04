//! An error `rustc` raises about the *generated* code must name the C function
//! and point at the Rust call site — the C code is fine here, the call is not.

cinrs::c99! {
    int fact(int n) {
        if (n == 0) {
            return 1;
        } else {
            return n * fact(n - 1);
        }
    }
}

fn main() {
    let _ = unsafe { fact("x") }; //~ ERROR: mismatched types
}
