//! A header that includes itself and has no guard to stop it.

cinrs::c99! {
    #include "include/loop.h" //~ ERROR: #include nested too deeply
    int x;
}

fn main() {}
