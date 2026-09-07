//! GNU's nested functions: what lambda lifting cannot do.
//!
//! A nested function is lifted to a file-scope item whose hidden leading
//! parameters are pointers to the enclosing objects it uses;
//! `tests/nested_functions.rs` is the proof that the shapes which fit through
//! that all work. These do not, and each is named rather than mistranslated:
//!
//! * the **address** of a nested function that uses the enclosing frame — the
//!   one thing GCC's trampoline is for, and the reason the lifted item's
//!   signature is not the one C gave it. A callback, an explicit `&g` and a
//!   `cleanup` attribute all ask for it;
//! * a **nonlocal `goto`**, which leaves the nested function for a label of the
//!   enclosing one;
//! * capturing a **variable length array** or a **`va_list`**, neither of which
//!   is a plain address;
//! * a storage class other than `auto`, and an `auto` declaration that no
//!   definition follows.
//!
//! A nested function that captures nothing has none of these problems: it is
//! lifted to a plain function and its address may be taken, which
//! `tests/nested_functions.rs` shows with `qsort`.

cinrs::gnu11! {
    #include <stdarg.h>

    typedef unsigned long size_t;

    int apply(int (*fn)(int), int v);
    void qsort(void *, size_t, size_t, int (*)(const void *, const void *));

    /* Passing a capturing one as a callback. */
    int as_a_callback(int n)
    {
        int base = n;
        int add(int x) { return x + base; }
        return apply(add, 1); //~ ERROR: the address of the nested function 'add' cannot be taken
    }

    /* `&g` says the same thing. */
    int explicit_address(int n)
    {
        int base = n;
        int add(int x) { return x + base; }
        int (*p)(int) = &add; //~ ERROR: the address of the nested function 'add' cannot be taken
        return p(1);
    }

    /* One that captures nothing itself but calls a sibling that does: what it
       needs from the frame is passed on, so its address is no more available
       than the sibling's. The captured object is named in the diagnostic. */
    int through_a_sibling(int n)
    {
        int base = n;
        int add(int x) { return x + base; }
        int wrapper(int x) { return add(x); }
        return apply(wrapper, 1); //~ ERROR: the address of the nested function 'wrapper' cannot be taken
    }

    /* `cleanup(f)` holds `f` in a drop guard, which is its address, so it
       wants the same kind of nested function a callback does. */
    int as_a_cleanup(int n)
    {
        int base = n;
        void release(int *p) { base += *p; }
        int a __attribute__((cleanup(release))) = 1; //~ ERROR: the address of the nested function 'release' cannot be taken
        return a;
    }

    /* GNU's nonlocal goto: the jump leaves the nested function. */
    int jumps_out(int n)
    {
        int leave(void)
        {
            if (n) goto done; //~ ERROR: 'goto done' leaves this nested function
            return 0;
        }
        leave();
        return 0;
    done:
        return 1;
    }

    /* The address of a label of the enclosing function is the same thing one
       step earlier: it is what a nonlocal jump would go through. */
    void *label_of_the_enclosing(int n)
    {
        void *where(void) { return &&outside; } //~ ERROR: '&&outside' names a label of the enclosing function
        if (n) return where();
    outside:
        return 0;
    }

    /* A variable length array of the enclosing function: its length lives in a
       hidden object of that frame, which is not captured yet. */
    int reads_a_vla(int n)
    {
        int cells[n];
        int first(void) { return cells[0]; } //~ ERROR: a nested function cannot use 'cells'
        return first();
    }

    /* A `va_list` belongs to the function whose arguments it walks; there is
       no address of one a callee may keep. */
    int reads_a_va_list(int n, ...)
    {
        va_list ap;
        va_start(ap, n);
        int take(void) { return va_arg(ap, int); } //~ ERROR: a nested function cannot use 'ap'
        int value = take();
        va_end(ap);
        return value;
    }

    /* The storage classes: `auto` is the one GNU allows, and it is the
       forward declaration. */
    int wrong_storage(int n)
    {
        static int g(int x) { return x; } //~ ERROR: 'static' is not allowed on a nested function definition
        return g(n);
    }

    /* Declared and never defined: nothing outside the block could define it,
       since a nested function has no linkage. */
    int only_declared(int n)
    {
        auto int missing(int); //~ ERROR: the nested function 'missing' is declared but never defined
        return n;
    }

    /* `auto` on a function only means anything inside a block. */
    auto int at_file_scope(int); //~ ERROR: 'auto' is not allowed on a file-scope function
}

fn main() {}
