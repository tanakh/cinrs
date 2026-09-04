//! Thread-local storage needs Rust's `#[thread_local]`, which is unstable.

cinrs::c11! {
    _Thread_local int counter; //~ ERROR: not supported yet

    int f(void) {
        _Thread_local int local; //~ ERROR: not supported yet
        return 0;
    }
}

fn main() {}
