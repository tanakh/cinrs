//! `#else` with nothing open is reported where it is.

cinrs::c99! {
    int x;
    #else //~ ERROR: #else without #if
    int y;
}

fn main() {}
