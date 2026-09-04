//! `#include_next` reaches the *next* header of a name on the search path,
//! which only means something when the platform's own directories are on it.

cinrs::c99! {
    #include_next <stdio.h> //~ ERROR: #include_next is not supported
}

fn main() {}
