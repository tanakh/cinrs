//! Pasting two tokens that do not make one is reported at the invocation.
//!
//! Written `# #` because Rust's own lexer refuses `##` in raw-token mode; the
//! preprocessor reads the two as the pasting operator.

cinrs::c99! {
    #define JOIN(a, b) a # # b

    int f(void) {
        return JOIN(+, 1); //~ ERROR: pasting '+' and '1' does not give a valid token
    }
}

fn main() {}
