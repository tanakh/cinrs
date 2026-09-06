//! A *non-defining* declaration of an enumeration with a fixed underlying
//! type only stands on its own (C23, N3030).
//!
//! `enum E : short;` is the whole of what may be written: the moment the
//! declaration goes on to declare anything — an object, a parameter, a return
//! type, a `typedef` name, a member — the enumeration has to bring its list of
//! enumerators with it. Clang's `test/C/C23/n3030.c` writes every shape below.
//!
//! The neighbouring rule is in the same paper: an enumeration with a fixed
//! underlying type is *complete* where that type is written, while one without
//! is incomplete until the `}` of its list (WG14 DR118). What the two accept
//! is in `tests/c23.rs`.

cinrs::c23! {
    enum standing_alone : long; /* fine: nothing else is declared */

    enum object : long int z; //~ ERROR: is only allowed as a standalone declaration
    void parameter(enum in_a_list : long b); //~ ERROR: is only allowed as a standalone declaration
    enum returned : int a_function(); //~ ERROR: is only allowed as a standalone declaration
    typedef enum aliased : short Alias; //~ ERROR: is only allowed as a standalone declaration

    struct with_a_member {
        int x;
        enum bit_field : int : 1; //~ ERROR: is only allowed as a standalone declaration
    };

    /* Writing the list is what makes each of those legal. */
    void with_a_list(enum listed : long { one } d);
}

cinrs::c23! {
    /* A tentative definition is a definition at the end of the translation
     * unit (6.9.2p2), and an enumeration whose list was never written has
     * nothing to define. */
    enum never_completed y; //~ ERROR: which is never completed

    /* Completing it later is what the rule asks for. */
    enum completed_later w;
    enum completed_later { later };
}

fn main() {}
