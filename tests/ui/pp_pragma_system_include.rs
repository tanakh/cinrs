//! `#pragma cinrs system_include` takes either nothing or `first`, and nothing
//! else.

cinrs::c99! {
    #pragma cinrs system_include maybe //~ ERROR: which takes either nothing or 'first'
    #pragma cinrs system_include first extra //~ ERROR: unexpected identifier 'extra' after

    int nothing(void) { return 0; }
}

fn main() {}
