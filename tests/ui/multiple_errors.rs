//! Every broken external declaration is reported; parsing resumes after each.

cinrs::c99! {
    int a = 08; //~ ERROR: invalid digit '8' in octal constant '08'

    int b = 09; //~ ERROR: invalid digit '9' in octal constant '09'

    int good(void) { return 0; }

    int bad(void) { return 1 } //~ ERROR: expected ';' after 'return' statement, found '}'

    struct S { int x }; //~ ERROR: expected ';' after member declaration, found '}'
}

fn main() {}
