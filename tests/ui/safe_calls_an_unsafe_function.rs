//@compile-flags: --crate-type lib
//! A safe function calling one of this unit's own functions that is not safe.
//!
//! `rustc` would say "call to unsafe function is unsafe", which is true and
//! says nothing about the C; this is the one place a message of this crate's
//! own is worth having, because both functions are right there in the source
//! and the fix is a word on the other one.

cinrs::c99! {
    #pragma cinrs safe safe_one

    int helper(int n) { return n + 1; }

    int safe_one(int n) {
        return helper(n); //~ ERROR: function 'helper' is not safe
    }
}
