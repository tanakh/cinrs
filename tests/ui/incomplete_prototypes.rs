//! A function *declaration* may name a type that is never completed, as its
//! return type or a parameter's (C11 6.7.6.3); only a definition (6.9.1p3)
//! and a call (6.5.2.2p1) need it complete. Kissat's `kimits.h` declares
//! `changes kissat_changes (struct kissat *);` and `struct changes` is defined
//! nowhere; glibc's headers do the same.

cinrs::c99! {
    struct kissat;
    typedef struct changes changes;
    changes kissat_changes (struct kissat *);
    _Bool kissat_changed (changes before, changes after);

    struct opaque;
    void takes (struct opaque value);
    struct opaque (*returns) (void);

    int prototypes_only (void) { return returns == 0; }
}

cinrs::c99! {
    struct later;
    struct later made (void);
    void consumed (struct later value);

    struct later made (void) { for (;;); } //~ ERROR: function cannot return an incomplete type 'struct later'
    void consumed (struct later value) { } //~ ERROR: parameter has incomplete type 'struct later'

    void call (void) {
        made (); //~ ERROR: calling 'made' with incomplete return type 'struct later'
    }
}

fn main() {}
