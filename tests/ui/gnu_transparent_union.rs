//@compile-flags: --crate-type lib
//! A transparent union takes an argument of any member's type, and nothing
//! else: GCC says "incompatible type for argument 1", and so does this, naming
//! the union whose members it searched.

cinrs::gnu99! {
    typedef union {
        int *ip;
        long *lp;
    } num_arg __attribute__((__transparent_union__));

    struct pair { int a, b; };

    static int deref(num_arg a) { return a.ip ? *a.ip : -1; }

    int wrong(double d, struct pair p, short *s) {
        int n = deref(d); //~ ERROR: passing 'double' to parameter 1 of 'deref', of the transparent union type 'union num_arg'
        n += deref(p); //~ ERROR: passing 'struct pair' to parameter 1 of 'deref', of the transparent union type 'union num_arg'
        n += deref(s); //~ ERROR: passing 'short *' to parameter 1 of 'deref', of the transparent union type 'union num_arg'
        return n;
    }

    /* A function type with the union for a parameter is compatible with one
       that has a member's type there, and with nothing else. */
    static int by_short(short *p) { return *p; }
    int (*not_a_member)(short *) = deref; //~ ERROR: cannot initialize 'not_a_member', of type 'int (*)(short *)'
    int (*nor_back)(num_arg) = by_short; //~ ERROR: cannot initialize 'nor_back', of type 'int (*)(union num_arg)'
}
