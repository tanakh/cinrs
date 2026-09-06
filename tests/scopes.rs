//! Where a declaration's *type* is visible, which is not where its object is.
//!
//! C keeps two things apart that are easy to run together. An identifier with
//! linkage names **one object** across the whole translation unit; but the
//! *type* a use of that name sees is the type of the declaration it went
//! through, and a redeclaration in an inner block gives it a composite type
//! that lasts to the end of **that block** and no further (C99 6.2.7p4). The
//! same paragraph-and-a-half of the standard decides where a `struct` tag
//! written in a parameter list lives: in the parameter list, for a prototype
//! (6.2.1p4's *function prototype scope*), or in the body, for a definition.
//!
//! The two questions are WG14 **DR011** and **DR103**, and Clang's
//! `test/C/drs/dr0xx.c` and `dr1xx.c` ask them. What is *refused* is in
//! `tests/ui/scoped_types.rs`; this file is what has to keep working.

use cinrs::c11;

// ---------------------------------------------------------------------------
// DR011: the composite type of a redeclaration lasts to the end of its block
// ---------------------------------------------------------------------------

/// `extern int i[]; { extern int i[10]; sizeof i; }` is 40 inside the block.
///
/// The object is the same one throughout — there is one `i` in the program —
/// and only the type the name is seen through changes. The composite of a
/// completed array type and an incomplete one is the completed one, whichever
/// declaration came first, so the two shapes below are the same question
/// asked twice.
#[test]
fn a_redeclaration_gives_the_name_a_composite_type_inside_its_block() {
    c11! {
        extern int dr011_i[];
        extern int dr011_j[10];

        unsigned long incomplete_then_complete(void) {
            extern int dr011_i[];
            {
                /* The composite here is `int[10]`, so `sizeof` has an
                 * answer where the outer declaration gives it none. */
                extern int dr011_i[10];
                return sizeof(dr011_i);
            }
        }

        unsigned long complete_then_incomplete(void) {
            extern int dr011_j[10];
            {
                extern int dr011_j[];
                /* The composite is still `int[10]`: an incomplete array type
                 * is compatible with every completed one of the same element
                 * type, and the completed one wins (6.2.7p1). */
                return sizeof(dr011_j);
            }
        }

        /* Nothing about the *object* changed, so it is still one array and
         * still writable through either name. */
        int stores_and_reads(void) {
            extern int dr011_j[10];
            dr011_j[3] = 7;
            {
                extern int dr011_j[];
                return dr011_j[3];
            }
        }
    }

    #[unsafe(no_mangle)]
    static mut dr011_i: [core::ffi::c_int; 10] = [0; 10];
    #[unsafe(no_mangle)]
    static mut dr011_j: [core::ffi::c_int; 10] = [0; 10];

    let ints = core::mem::size_of::<core::ffi::c_int>() as u64;
    unsafe {
        assert_eq!(incomplete_then_complete(), 10 * ints);
        assert_eq!(complete_then_incomplete(), 10 * ints);
        assert_eq!(stores_and_reads(), 7);
    }
}

/// The same rule at file scope, where the "block" is the rest of the unit.
///
/// `int a[]; int a[3];` is C99 6.9.2's tentative definition completed by a
/// later declaration, and the composite is what the object ends up with — so
/// `sizeof` after it is twelve and the generated item is three elements long.
#[test]
fn a_file_scope_redeclaration_completes_the_object_for_the_rest_of_the_unit() {
    c11! {
        static int tentative[];
        static int tentative[3];

        unsigned long size(void) { return sizeof(tentative); }
        int third(void) { tentative[2] = 5; return tentative[2]; }
    }

    unsafe {
        assert_eq!(size(), 3 * core::mem::size_of::<core::ffi::c_int>() as u64);
        assert_eq!(third(), 5);
    }
}

// ---------------------------------------------------------------------------
// DR103: a tag written in a parameter list belongs to that list
// ---------------------------------------------------------------------------

/// A tag defined in a **definition's** parameter list reaches the body.
///
/// C99 6.2.1p4 gives an identifier declared "within the list of parameter
/// declarations in a function definition" block scope terminating at the end
/// of the body — not the function prototype scope a *declaration*'s list gives
/// it. So `t.a` below resolves, and the type is gone again after the closing
/// brace, where a later `struct dr103_t` is a new and unrelated tag.
#[test]
fn a_tag_defined_in_a_definitions_parameter_list_reaches_the_body() {
    c11! {
        int member(struct dr103_t { int a; int b; } t) { return t.a + t.b; }

        /* A second definition writes the tag out again, because the first
         * one's is not in scope here — and these are two different types,
         * which is only observable in that neither declaration can name the
         * other's. */
        int first(struct dr103_u { int a; } u) { return u.a; }

        /* The tag *this* file scope declares is a third one, and completing
         * it here is not a redefinition of either. */
        struct dr103_t { char c; };
        unsigned long file_scope_size(void) { return sizeof(struct dr103_t); }
    }

    unsafe {
        assert_eq!(file_scope_size(), 1);
    }
}

/// A tag first *mentioned* in a parameter list is still that list's.
///
/// `void f(struct S *p);` declares `struct S` in the prototype scope, which is
/// why GCC warns that it "will not be visible outside of this declaration".
/// Naming the type before the prototype is what makes the two one type, and
/// that is the shape every real program uses.
#[test]
fn a_tag_declared_before_the_prototype_is_the_one_the_prototype_means() {
    c11! {
        struct dr103_node;
        void set(struct dr103_node *p, int v);
        int get(struct dr103_node *p);

        struct dr103_node { int value; };

        void set(struct dr103_node *p, int v) { p->value = v; }
        int get(struct dr103_node *p) { return p->value; }

        int round_trip(int v) {
            struct dr103_node n;
            set(&n, v);
            return get(&n);
        }
    }

    unsafe {
        assert_eq!(round_trip(41), 41);
    }
}
