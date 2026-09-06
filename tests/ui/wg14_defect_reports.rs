//! The constraint violations the WG14 defect reports in `clang/test/C/drs`
//! ask an implementation to diagnose, and which this one now does.
//!
//! Each block names its DR. The accepting half of the same rules is in
//! `tests/pointers.rs`, `tests/scopes.rs` and `tests/aggregates.rs`.

cinrs::c11! {
    /* DR047: an array's element type may not be incomplete — including as a
     * parameter, where the array itself is adjusted to a pointer and the
     * constraint on the declarator still applies (6.7.6.2p1). */
    struct dr047_t;
    struct dr047_t *by_value(struct dr047_t a[]); //~ ERROR: array has incomplete element type 'struct dr047_t'
    extern struct dr047_t incomplete_is_fine; /* an `extern` object's size is another unit's business */

    /* DR088: `struct S;` on its own declares a tag of *this* scope, so the
     * pointer below is to a different type from the file scope's. */
    struct dr088_t;
    void takes_outer(struct dr088_t *);
    void dr088(void) {
        struct dr088_t;
        takes_outer((struct dr088_t *) 0); //~ ERROR: of incompatible type 'struct dr088_t *'
    }

    /* DR116: the address of a `register` object cannot be computed, whether
     * `&` asks for it or an array decaying to a pointer does (6.7.1p6). */
    void dr116(void) {
        register int array[5];
        register int scalar;
        (void) array;       //~ ERROR: 'array' is declared 'register'
        (void) array[3];    //~ ERROR: 'array' is declared 'register'
        (void) (array + 3); //~ ERROR: 'array' is declared 'register'
        (void) &scalar;     //~ ERROR: cannot take the address of 'scalar', which is declared 'register'
        (void) sizeof array; /* the one operator that still applies */
    }

    /* DR118: an enumeration is incomplete until the `}` of its own list. */
    void dr118(void) {
        enum E {
            Val = sizeof(enum E) //~ ERROR: invalid application of 'sizeof' to an incomplete type 'enum E'
        };
    }

    /* DR131: a structure with a `const` member is not a modifiable lvalue,
     * however unqualified the object itself is (6.3.2.1p1). */
    void dr131(void) {
        struct Inner { const int i; };
        struct Outer { struct Inner inner; int j; };
        struct Outer a, b;
        a = b; //~ ERROR: cannot assign to variable 'a' with the const-qualified data member 'i'
    }

    /* DR252: an argument with no parameter to check it against still has to
     * have a value to pass. */
    void no_prototype();
    void returns_nothing(void);
    void dr252(void) {
        no_prototype(returns_nothing()); //~ ERROR: an argument of type 'void' is incomplete and has no value to pass
    }

    /* An object whose size does not fit `size_t` has no size at all. */
    unsigned long too_big(void) {
        return sizeof(int[__SIZE_MAX__ / 2]); //~ ERROR: array is too large
    }
}

fn main() {}
