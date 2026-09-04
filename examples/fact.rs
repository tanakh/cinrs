//! The example from the README, as a program you can run:
//!
//! ```text
//! cargo run --example fact
//! ```

use cinrs::c99;

c99! {
    int fact(int n) {
        if (n == 0) {
            return 1;
        } else {
            return n * fact(n - 1);
        }
    }
}

fn main() {
    // `c99!` defines `extern "C"` functions, so a call is `unsafe` — the C
    // code is trusted exactly as much as any other foreign function.
    let v = unsafe { fact(10) };
    println!("fact(10) = {v}");
    assert_eq!(v, 3_628_800);
}
