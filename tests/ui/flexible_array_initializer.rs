//! Initialising a flexible array member makes the object larger than its
//! type, which only an object with *static* storage duration can be: its
//! storage is the compiler's to size, and the generated item gets a companion
//! type whose tail is as long as the initialiser.
//!
//! `tests/gnu_language.rs` runs the ones that are allowed.

cinrs::c99! {
    struct W { int n; int data[]; };

    /* Allowed: file scope and a block-scope `static`. */
    struct W ok = { 2, { 1, 2 } };

    int also_ok(void) {
        static struct W s = { 1, { 9 } };
        return s.data[0];
    }

    /* An automatic object is exactly its type's size, and nothing can make it
       bigger. GCC says "non-static initialization of a flexible array
       member". */
    int automatic(void) {
        struct W w = { 1, { 5 } }; //~ ERROR: non-static initialization of a flexible array member
        return w.n;
    }

    /* Nested inside another aggregate, whose own size already fixes the room
       there is. GCC's wording again. */
    struct W table[2] = { { 1, { 5 } }, { 0 } };
    //~^ ERROR: initialization of flexible array member in a nested context

    struct Outer { struct W inner; };
    struct Outer o = { { 1, { 5 } } };
    //~^ ERROR: initialization of flexible array member in a nested context

    /* A compound literal is its type name's size, whatever it is written
       inside. */
    struct W *literal = &(struct W){ 1, { 5 } };
    //~^ ERROR: initialization of flexible array member in a nested context
}

fn main() {}
