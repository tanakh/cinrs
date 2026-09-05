//! C23 removed old-style (K&R) function definitions (N2432), so the one entry
//! point that does not have them says so. Every earlier one accepts the form:
//! it is obsolescent C99, not invalid C99.

cinrs::c23! {
    int add(a, b)   //~ ERROR: old-style function definitions were removed in C23
        int a;
        int b;
    {
        return a + b;
    }
}

fn main() {}
