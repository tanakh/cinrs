//! What C's scopes refuse: WG14 DR011 and DR103.
//!
//! The other half of `tests/scopes.rs`. An identifier with linkage names one
//! object, and a redeclaration of it in an inner block gives the *name* the
//! composite type for the length of that block only (C99 6.2.7p4) — so a
//! `sizeof` after the block sees the outer declaration's incomplete type
//! again. A `struct` tag written in a parameter list has that list's scope
//! (6.2.1p4), so two prototypes that each declare `struct S` declare two
//! unrelated types, and the declarations of the function they are parameters
//! of conflict.
//!
//! Both are exactly what GCC warns about when it says a tag "will not be
//! visible outside of this definition or declaration": nothing after the
//! closing parenthesis can name it, so nothing after it can pass one either.

cinrs::c11! {
    void dr011(void) {
        extern int i[];
        {
            extern int i[10];
            (void)sizeof(i);
        }
        /* The composite is gone with the block, and the outer declaration's
         * type is incomplete again. */
        (void)sizeof(i); //~ ERROR: invalid application of 'sizeof' to an incomplete type
    }
}

cinrs::c11! {
    /* Two prototypes, each declaring its own `struct S` — two types, so the
     * two declarations of `dr103` are not compatible. */
    void dr103(struct S s);
    void dr103(struct S { int a; } s); //~ ERROR: conflicting types for 'dr103'
}

cinrs::c11! {
    /* A tag a parameter list declared is not in scope after it. */
    void takes(struct Hidden *p);
    struct Hidden after; //~ ERROR: variable 'after' has incomplete type 'struct Hidden'
}

cinrs::c11! {
    /* And the tag the file scope goes on to define is a different type, so
     * the pointer the prototype takes is not the one a caller has. */
    void consume(struct Later *p);
    struct Later { int x; };
    void call(struct Later *p) { consume(p); } //~ ERROR: of incompatible type 'struct Later *'
}

fn main() {}
