//! `#error` is reported at the directive, carrying the text as written.

cinrs::c99! {
    #define WIDTH 16
    #if WIDTH < 32
    #error this code needs at least 32 bits //~ ERROR: #error this code needs at least 32 bits
    #endif
    int x;
}

fn main() {}
