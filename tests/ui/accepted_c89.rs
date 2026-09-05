//@check-pass
//! The C89 rules a later revision deleted, in the entry points that have them.
//!
//! Implicit `int`, implicit function declarations and old-style definitions
//! all work in `c89!` and `gnu89!`, and a program written the way C was
//! written in 1990 compiles as it stands.

cinrs::c89! {
    static counter;
    const limit = 10;

    bump()
    {
        counter = counter + 1;
        return counter;
    }

    int uses_the_library(n)
        int n;
    {
        /* Nothing declares `abs`: the call declares `extern int abs();`, and
           the linker resolves it. */
        return abs(n) + limit;
    }

    long narrow(c, f)
        char c;
        float f;
    {
        return c + (long)f;
    }
}

cinrs::gnu89! {
    #pragma cinrs module "gnu89_block"

    /* Everything a later revision added is accepted here as an extension —
       and the C89 rules are still C89's. */
    main(argc, argv)
        int argc;
        char **argv;
    {
        long long wide = 1LL << 40;   // a `//` comment, too
        int total = argc;
        for (int i = 0; i < 3; i++) total += i;
        if (argv == 0) return 1;
        return total + (int)(wide >> 40);
    }
}

fn main() {}
