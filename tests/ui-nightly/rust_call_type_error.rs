//! `rustc`'s own errors about the generated code land inside the literal too:
//! every token the expansion emits carries the span of the C it came from, and
//! with the feature on that span points at the C token rather than at the
//! whole literal.

cinrs::c99! { r#"
    int fact(int n) {
        if (n == 0) return 1;
        return n * fact(n - 1);
    }
"# }

fn main() {
    let _ = unsafe { fact("x") }; //~ ERROR: mismatched types
}
