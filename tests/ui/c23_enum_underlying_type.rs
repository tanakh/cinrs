//! Every declaration of one enumeration has to give it the same underlying
//! type (C23, N3030).
//!
//! An `enum E : short;` fixes the type; a later `enum E : long { … }` fixes a
//! different one, and the two cannot both be right. The absence of a fixed
//! type is a choice as well, so `enum E { … };` followed by `enum E : int;`
//! is the same constraint violated the other way round. GCC and Clang refuse
//! all four shapes below; Clang's `test/C/C23/n3030.c` is where they are
//! written down.
//!
//! What is *accepted* — the same type repeated, and a plain `enum E x;` that
//! asks nothing — is in `tests/c23.rs`.

cinrs::c23! {
    enum A : short;
    enum A { a1 }; //~ ERROR: previously declared with the fixed underlying type 'short'
}

cinrs::c23! {
    enum B : short;
    enum B : int { b1 }; //~ ERROR: is redeclared with the underlying type 'int'
}

cinrs::c23! {
    enum C { c1 };
    enum C : int; //~ ERROR: previously declared without a fixed underlying type
}

cinrs::c23! {
    enum D : short;
    enum D : long; //~ ERROR: is redeclared with the underlying type 'long'
}

cinrs::c23! {
    /* And inside a block, where the tag is the block's. */
    int f(void) {
        enum E : short;
        enum E : long { e1 }; //~ ERROR: is redeclared with the underlying type 'long'
        return 0;
    }
}

fn main() {}
