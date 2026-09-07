//@compile-flags: --crate-type lib
//! What a safe function may not do: read a `union` member.
//!
//! A C `union` becomes a Rust one, where reading a field is unsafe because
//! nothing says the value that was written is the one being read. That is C's
//! own type-punning, and a safe function is exactly the place it is not
//! allowed — even though the `union` may be passed and returned by value like
//! any other object.

cinrs::c99! {
    #pragma cinrs safe as_int

    union Value { int as_int; float as_float; };

    int as_int(union Value v) {
        return v.as_int; //~ ERROR: access to union field is unsafe
    }
}
