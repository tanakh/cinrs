//! `#include` with nothing that could be a header name.

cinrs::c99! {
    #include 42 //~ ERROR: #include expects "FILENAME" or <FILENAME>
    #include //~ ERROR: #include expects "FILENAME" or <FILENAME>
    int x;
}

fn main() {}
