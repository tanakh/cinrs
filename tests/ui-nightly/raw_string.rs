//! A raw string on one line: the offsets in the C text are the offsets in the
//! literal shifted past `r#"`, and the caret lands on `08`.

cinrs::c99! { r#"int ok(void) { return 0; } int bad = 08;"# } //~ ERROR: invalid digit '8' in octal constant '08'

fn main() {}
