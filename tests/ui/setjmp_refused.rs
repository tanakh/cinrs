//! `setjmp` where C17 7.13.1.1p4 does not allow one.
//!
//! A `longjmp` comes back to the `setjmp` by re-entering the function's
//! graph at the code that follows it, so that code has to be something the
//! graph can resume: the rest of a controlling expression that compares the
//! result with a constant, or the rest of an expression statement. An
//! operand of anything else is refused where the call was written, and so is
//! a `setjmp` inside a statement expression.

cinrs::gnu99! {
    #include <setjmp.h>

    static jmp_buf env;
    int g(int);

    int operands(int x) {
        int y = 1 + setjmp(env); //~ ERROR: 'setjmp' cannot be called here
        if (setjmp(env) == x) //~ ERROR: 'setjmp' cannot be called here
            return 1;
        g(setjmp(env)); //~ ERROR: 'setjmp' cannot be called here
        if (setjmp(env) && x) //~ ERROR: 'setjmp' cannot be called here
            return 2;
        return setjmp(env); //~ ERROR: 'setjmp' cannot be called here
    }

    int in_a_statement_expression(void) {
        return ({ int r = 0; if (setjmp(env)) r = 1; r; }); //~ ERROR: inside a statement expression is not supported
    }

    /* Everything here is allowed. */
    int allowed(void) {
        int r = setjmp(env);
        if (setjmp(env)) r++;
        if (!setjmp(env)) r++;
        if (setjmp(env) != 0) r++;
        while (setjmp(env) < 3) r++;
        switch (setjmp(env)) { case 1: r++; }
        setjmp(env);
        (void) setjmp(env);
        r = setjmp(env);
        return r;
    }
}

fn main() {}
