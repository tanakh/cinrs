//! In string-literal mode stable Rust cannot span into the literal, so the
//! position inside the C source is appended to the message instead.

//~v ERROR: invalid digit '8' in octal constant '08' (at line 3, column 15 of the C source)
cinrs::c99! { r#"
    int ok(void) { return 0; }
    int bad = 08;
"# }

fn main() {}
