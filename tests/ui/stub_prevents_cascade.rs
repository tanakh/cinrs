//! A body that does not type check still leaves a callable definition behind,
//! so the Rust call below adds nothing to the one real error.

cinrs::c99! {
    int fact(int n) {
        return n * missing_helper(n); //~ ERROR: implicit declaration of function 'missing_helper' is invalid in C99
    }
}

fn main() {
    let value: ::core::ffi::c_int = unsafe { fact(3) };
    let _ = value;
}
