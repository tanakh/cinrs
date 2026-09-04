//! An ordinary string literal, where a byte of C text and a byte of the
//! literal are not the same thing: each `\n` is written as two. The caret has
//! to land on `08` all the same.

cinrs::c99! { "int ok(void) { return 0; }\nint bad = 08;\n\tint worse;\n" } //~ ERROR: invalid digit '8' in octal constant '08'

fn main() {}
