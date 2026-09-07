//@compile-flags: --crate-type lib
//! What a safe function may not do: read through a pointer.
//!
//! Pointers are raw pointers in the translation and indexing is pointer
//! arithmetic, so all three of these are `*p` in the generated Rust — which is
//! exactly what Rust asks for an `unsafe` block around. The third is the one
//! worth knowing about: `a[i]` is `*(a + i)` in C, so the elements of an array
//! are reached through a pointer even when the array is a local of the
//! function itself.
//!
//! There is no message of this crate's here on purpose: `rustc`'s own is the
//! accurate one, and the caret is on the C that wrote the dereference.

cinrs::c99! {
    #pragma cinrs safe first nth sum

    int first(const int *p) {
        return *p; //~ ERROR: dereference of raw pointer is unsafe
    }

    int nth(const int *p, int i) {
        //~v ERROR: call to unsafe function
        return p[i]; //~ ERROR: dereference of raw pointer is unsafe
    }

    int sum(int n) {
        int a[3];
        //~v ERROR: call to unsafe function
        a[0] = n; //~ ERROR: dereference of raw pointer is unsafe
        //~v ERROR: call to unsafe function
        return a[0]; //~ ERROR: dereference of raw pointer is unsafe
    }
}
