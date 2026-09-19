//! The example at the top of the README, as a program you can run:
//!
//! ```text
//! cargo run --example readme
//! ```
//!
//! It is built with the rest of the examples by `cargo test`, which is what
//! keeps the README honest.

use cinrs::c99;

c99! {
    #include <stdio.h>

    typedef struct { double x, y; } Vec2;

    double dot(Vec2 a, Vec2 b) {
        return a.x * b.x + a.y * b.y;
    }

    void greet(const char *name) {
        printf("hello, %s\n", name);
    }

    __attribute__((cinrs_safe)) int fact(int n) {
        return n == 0 ? 1 : n * fact(n - 1);
    }
}

fn main() {
    // C functions are foreign functions: calling one is `unsafe`…
    let d = unsafe { dot(Vec2 { x: 1.0, y: 2.0 }, Vec2 { x: 3.0, y: 4.0 }) };
    unsafe { greet(c"cinrs".as_ptr()) };

    // …unless the C says `cinrs_safe`, and then `rustc` checks the body.
    println!("dot = {d}, fact(10) = {}", fact(10));
}
