//! `_Thread_local` is C11's, `thread_local` is C23's spelling of it, and
//! `__thread` is GCC's — which, being reserved, works in every entry point.

cinrs::c99! {
    _Thread_local int a; //~ ERROR: '_Thread_local' requires C11 or later
    int read_a(void) { return a; }
}

cinrs::c99! {
    thread_local int b; //~ ERROR: 'thread_local' requires C23 or later
}

cinrs::c99! {
    __thread int c;
    int read_c(void) { return c; }
}

fn main() {}
