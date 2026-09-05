//! An implicit declaration is a real declaration: `extern int f();`, with no
//! prototype. A later one has to be *compatible* with it (C99 6.7.5.3p15), and
//! a `double` return type is not — the call above has already been generated
//! as an `int` one.
//!
//! Written with `gnu89!` rather than `c89!` for a reason worth knowing: an
//! error annotation is written as a line comment, and inside a strict `c89!`
//! block a line comment is itself a diagnostic. The rule under test — a call
//! to an undeclared function declares it — is the same in both.

cinrs::gnu89! {
    int early(void)
    {
        return measure(1);
    }

    double measure(n)   //~ ERROR: conflicting types for 'measure'
        int n;
    {
        return n * 1.5;
    }
}

fn main() {}
