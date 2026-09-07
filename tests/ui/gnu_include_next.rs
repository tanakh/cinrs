//! `#include_next` reaches the *next* header of a name on the search path.
//!
//! Written in the unit's own text there is no place it was found in, so the
//! search starts at the beginning exactly as `#include` does — which is what
//! GCC does too, and what makes the first directive below find the bundled
//! `<stdio.h>`. The second names a header nothing carries, and the message
//! lists what was looked in.

cinrs::c99! {
    #include_next <stdio.h>

    #include_next <nothing_of_that_name.h> //~ ERROR: file not found by #include_next

    int puts_it(const char *s) { return puts(s); }
}

fn main() {}
