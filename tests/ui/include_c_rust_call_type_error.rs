//! An error `rustc` raises about a *call* to a function that came from an
//! included file.
//!
//! The C is elsewhere, so the definition `rustc` points at is the macro
//! invocation — but the call is right here, and that is where the caret that
//! matters lands, exactly as it does for a `c99!` block.

cinrs::include_c99!("c/adder.c");

fn main() {
    let _ = unsafe { add(1, "two") }; //~ ERROR: mismatched types
}
