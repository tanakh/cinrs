//! A header that is nowhere is reported at the directive, listing the
//! directories that were searched — in the order they were searched, and
//! relative to the working directory, so that the message is the same on every
//! machine.

cinrs::c99! {
    #include <nowhere.h> //~ ERROR: <nowhere.h> file not found
    #include "also_nowhere.h" //~ ERROR: "also_nowhere.h" file not found
    int x;
}

fn main() {}
