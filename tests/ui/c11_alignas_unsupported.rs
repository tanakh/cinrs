//! `_Alignas` is honoured by raising the alignment of the whole record, which
//! only works while the member's natural offset already satisfies it. Anything
//! else is refused rather than laid out differently from the Rust item.

cinrs::c11! {
    struct Late {
        char c;
        _Alignas(16) int x; //~ ERROR: not supported yet
    };

    _Alignas(16) int object; //~ ERROR: not supported yet
}

fn main() {}
