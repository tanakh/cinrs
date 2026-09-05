//! The GNU extensions `cinrs` knows about and cannot honour. Each is refused
//! with the reason, because ignoring one would change what the program means.

cinrs::c99! {
    int weak_function(void) __attribute__((weak)); //~ ERROR: weak linkage cannot be asked for

    void with_cleanup(void) {
        int fd __attribute__((cleanup(close_it))); //~ ERROR: it needs a drop guard
    }

    typedef int v4si __attribute__((vector_size(16))); //~ ERROR: the vector extensions need `core::simd`

    int aliased(void) __attribute__((alias("weak_function"))); //~ ERROR: write a function that forwards
}

fn main() {}
