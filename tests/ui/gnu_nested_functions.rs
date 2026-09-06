//! GNU's nested functions, which `cinrs` names and refuses.
//!
//! A pointer to a nested function has to reach the enclosing frame, which GCC
//! arranges by writing a trampoline onto the stack; nothing in Rust is one, so
//! the construct is refused rather than mis-parsed as a declaration with a
//! stray `{` after it — and refused by the GNU entry points too, which is what
//! `gnu11!` here shows. The refusal costs nothing else: the body is skipped
//! whole and what follows it in the enclosing function is still checked, and a
//! block-scope function *declaration*, which is ordinary C, goes on working.

cinrs::gnu11! {
    int adder(int a, int b)
    {
        int add(int x, int y) //~ ERROR: nested function definitions are a GNU extension
        {
            return x + y;
        }
        return add(a, b);
    }

    /* One that reads a local, which is the whole reason the extension exists. */
    int bumper(int n)
    {
        int step = 3;

        int bumped(int x) //~ ERROR: nested function definitions are a GNU extension
        {
            return x + step;
        }
        return bumped(n);
    }

    /* The body is skipped balanced, so the rest of the function is checked. */
    int checked(void)
    {
        int inner(int x) //~ ERROR: nested function definitions are a GNU extension
        {
            return x;
        }
        return inner(1, 2); //~ ERROR: too many arguments to function call, expected 1, have 2
    }

    /* The old-style shape, where a parameter declaration list comes between
       the declarator and the body. */
    int host(int n)
    {
        int older(x) //~ ERROR: nested function definitions are a GNU extension
            int x;
        {
            return x;
        }
        return older(n);
    }

    /* A function *declaration* inside a block is C 6.7.1p7, not an extension. */
    int outer(int n)
    {
        int twice(int);
        int old_style();
        return twice(n) + old_style(n);
    }

    int twice(int n) { return n * 2; }

    int old_style(int n) { return n; }
}

fn main() {}
