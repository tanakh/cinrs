//! A conditional group that is never closed is reported at the `#if`.

cinrs::c99! {
    #if 1 //~ ERROR: unterminated conditional directive
    int x;
}

fn main() {}
