//! What C23's `auto` refuses, and what Clang's extension to it refuses too.
//!
//! Clang's `test/C/C23/n3007.c` is the file these come from. The accepting
//! half — `auto n = 3;`, `auto *p = &n;`, `static auto c = 1UL;` — is in
//! `tests/c23.rs`.

cinrs::c23! {
    /* An underspecified declaration's own name is in scope for its
     * initialiser (6.2.1p7) and there is no type for it to have there, so
     * naming it is a constraint violation rather than a use of whatever an
     * enclosing scope had. GCC calls it "underspecified 'b' referenced in its
     * initializer". */
    double shadowed(void) {
        double b = 9;
        {
            auto b = b * b; //~ ERROR: 'b' is declared with a type deduced from this initializer and cannot appear in it
            return b;
        }
    }

    /* `auto` has no place in a parameter, and a `typedef` is the one
     * declaration whose type does not resolve where it is written, so the
     * mistake is reported where it stands. */
    typedef void (*handler)(auto); //~ ERROR: 'auto' is not allowed in a function prototype

    /* Inference through a pointer declarator needs the initialiser to have
     * the pointer to deduce from. */
    int no_pointer_to_peel(void) {
        int a = 1;
        auto *p = a; //~ ERROR: has an incompatible initializer of type 'int'
        return *p;
    }
}

cinrs::c23! {
    /* 6.7.1p2 pairs `constexpr` with `auto`, `register` and `static`, and
     * with nothing else: a constant has no linkage to give another unit and
     * no storage to give a thread a copy of. */
    extern constexpr int elsewhere; //~ ERROR: cannot combine storage class 'constexpr' with 'extern'
    _Thread_local constexpr int per_thread = 1; //~ ERROR: '_Thread_local' cannot be combined with 'constexpr'
    typedef constexpr int Alias; //~ ERROR: cannot combine storage class 'constexpr' with 'typedef'
}

fn main() {}
