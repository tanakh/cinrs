//@check-pass
//! An `extern` declaration may name an object of incomplete type.
//!
//! WG14 DR047: 6.7p7 asks for a complete type only where the identifier has no
//! linkage, and 6.2.5p22 says the size of an object defined in another unit is
//! none of this one's business — only its address is ever used here. What this
//! checks is that the *expansion* compiles: the `extern` block has to name a
//! Rust type for the object, and an incomplete tag still has one.

cinrs::c11! {
    struct Incomplete;
    extern struct Incomplete es;
    struct Incomplete *address(void) { return &es; }

    union AlsoIncomplete;
    extern union AlsoIncomplete eu;
    union AlsoIncomplete *union_address(void) { return &eu; }

    /* And one that *is* completed later, which is the ordinary case. */
    struct Later;
    extern struct Later later_obj;
    struct Later { int x; };
    int read_later(void) { return later_obj.x; }
}

fn main() {}
