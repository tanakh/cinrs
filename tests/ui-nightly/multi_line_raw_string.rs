//! The input of `tests/ui/string_literal_mode.rs`, with the `nightly` feature
//! on: the caret sits on the C token itself, on its own line inside the raw
//! string, and the message no longer has to spell the position out.

cinrs::c99! { r#"
    int ok(void) { return 0; }
    int bad = 08; //~ ERROR: invalid digit '8' in octal constant '08'
    int worse(void) { return nope; } //~ ERROR: use of undeclared identifier 'nope'
"# }

fn main() {}
