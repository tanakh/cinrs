//! Two definitions of one tag in one scope, and what C23 made of them
//! (6.7.2.3p1, WG14 N3037).
//!
//! Identical tag types have always been compatible *across* translation units;
//! N3037 made them compatible within one. So a second definition whose members
//! agree with the first's declares the same type — `tests/c23.rs` is where
//! that half is run — and one whose members do not is the constraint violation
//! it always was. The difference is what these diagnostics name: a
//! "redefinition of 'struct S'" with two lists that are letter for letter the
//! same in every place but one is a riddle, and the point of the message is to
//! say which member it is.
//!
//! The last block is the other half of the rule: an entry point before C23 has
//! no such compatibility, so the plain redefinition error stands there even
//! when the two lists agree. GCC 15 draws the line in the same place — N3037
//! is in `-std=c23` and `-std=gnu23` and in neither of the older dialects.

cinrs::c23! {
    struct Named { int x; };
    struct Named { int y; }; //~ ERROR: the member at position 1 is named 'y' here
}

cinrs::c23! {
    struct Typed { int x; };
    struct Typed { float x; }; //~ ERROR: member 'x' has type 'float' here and 'int'
}

cinrs::c23! {
    struct Counted { int x; };
    struct Counted { int x; int y; }; //~ ERROR: has one member and this one has 2 members
}

cinrs::c23! {
    struct Bits { unsigned b : 3; };
    struct Bits { unsigned b : 4; }; //~ ERROR: member 'b' is 4 bits wide here and 3
}

cinrs::c23! {
    struct Aligned { _Alignas(8) int x; };
    struct Aligned { int x; }; //~ ERROR: the alignment differs
}

cinrs::c23! {
    union Ordered { int x; float y; };
    union Ordered { float y; int x; }; //~ ERROR: the member at position 1 is named 'y' here
}

cinrs::c23! {
    enum Values { V = 1 };
    enum Values { V = 2 }; //~ ERROR: enumerator 'V' is 2 here and 1
}

cinrs::c23! {
    enum Names { N };
    enum Names { M }; //~ ERROR: has no enumerator named 'M'
}

cinrs::c23! {
    /* The rule is about one *scope*: this is the same tag defined twice in one
     * block, not an inner tag shadowing an outer one. */
    int f(void) {
        struct Inner { int x; };
        struct Inner { int x; long y; }; //~ ERROR: has one member and this one has 2 members
        return 0;
    }
}

cinrs::c17! {
    /* Before C23 a tag has its content defined at most once, whatever the two
     * definitions say. */
    struct Same { int x; };
    struct Same { int x; }; //~ ERROR: redefinition of 'struct Same'
}

cinrs::gnu11! {
    /* And a GNU dialect of an earlier revision is an earlier revision. */
    enum Twice { T };
    enum Twice { T }; //~ ERROR: redefinition of 'enum Twice'
}

fn main() {}
