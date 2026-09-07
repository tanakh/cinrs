//! The example from the README, as a program you can run:
//!
//! ```text
//! cargo run --example fact
//! ```

use cinrs::c99;

c99! {
    /* Marked safe, so the whole body goes past `rustc`'s own checks and the
     * generated item is an ordinary `extern "C" fn`. Without the attribute a
     * C function is a foreign function like any other and a call to it is
     * `unsafe`, which is the default. */
    __attribute__((cinrs_safe)) int fact(int n) {
        if (n == 0) {
            return 1;
        } else {
            return n * fact(n - 1);
        }
    }
}

fn main() {
    let v = fact(10);
    println!("fact(10) = {v}");
    assert_eq!(v, 3_628_800);
}
