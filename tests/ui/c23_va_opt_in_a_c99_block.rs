//! `__VA_OPT__` is C23's, and a `c99!` block is told so at the definition
//! rather than at the invocation.

cinrs::c99! {
    #define LOG(fmt, ...) log(fmt __VA_OPT__(,) __VA_ARGS__) //~ ERROR: requires C23 or later

    int log(const char *fmt, ...);
    int use_it(void) { return LOG("x"); }
}

fn main() {}
