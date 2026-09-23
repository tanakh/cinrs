//! A declarator whose name is the `typedef` its declaration specifiers named.
//!
//! The specifiers are one for every declarator, and the variable's scope
//! begins just after its own declarator (C11 6.2.1p7): in Kissat's
//! `links *links = solver->links, *l = links + idx;` the `*l` is still a
//! `links *`, and the `links` in its initialiser is the variable. In the first
//! declarator's *own* initialiser the variable is already in scope too, so
//! `(links *)` there is no cast — GCC says "expected expression" as well.

cinrs::c99! {
    typedef struct links { unsigned prev, next; } links;
    struct solver { links *links; };

    unsigned inline_queue (struct solver *solver, unsigned idx) {
        links *links = solver->links, *l = links + idx;
        return l->prev;
    }

    /* Kissat's stack.h `all_stack (T, E, S)` with `T == E`, expanded. */
    typedef struct watch { unsigned lit; } watch;

    unsigned sum_watches (watch *begin, unsigned n) {
        unsigned sum = 0;
        for (watch watch, *watch_PTR = begin, *const watch_END = begin + n;
             watch_PTR != watch_END; watch_PTR++)
            watch = *watch_PTR, sum += watch.lit;
        return sum;
    }
}

cinrs::c99! {
    typedef struct links links;
    void *p;

    void cast_in_own_initializer (void) {
        links *links = (links *) p; //~ ERROR: expected expression, found ')'
    }
}

fn main() {}
