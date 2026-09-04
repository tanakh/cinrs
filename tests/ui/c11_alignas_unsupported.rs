//! `_Alignas` on a *member* is honoured — the member is moved to the boundary
//! it asks for, with explicit padding in front of it so that the generated
//! `#[repr(C)]` item lands it in the same place. On an *object* there is
//! nothing to carry the alignment, so it is refused.

cinrs::c11! {
    struct Late {
        char c;
        _Alignas(16) int x;
    };

    unsigned long offset_of_x(void) { return __builtin_offsetof(struct Late, x); }
    unsigned long size_of_late(void) { return sizeof(struct Late); }

    _Alignas(16) int object; //~ ERROR: not supported yet
}

fn main() {}
