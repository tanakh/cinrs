//! `_Alignas` on an object is honoured — the binding is generated inside a
//! `#[repr(C, align(N))]` wrapper and every access goes through its one field
//! — but C11 6.7.5 hedges it about with constraints, and these are them.
//!
//! `tests/alignment.rs` is where the ones that hold are run.

cinrs::c11! {
    struct Late {
        char c;
        _Alignas(16) int x;
    };

    unsigned long offset_of_x(void) { return __builtin_offsetof(struct Late, x); }

    /* 6.7.5p4: the alignment asked for may not be weaker than the one the type
       already has. GCC's `aligned` attribute is not a diagnostic there — it
       "can only increase alignment" — but `_Alignas` is. */
    _Alignas(2) int weaker; //~ ERROR: weaker than the alignment 4 that 'int' already has

    /* 6.7.5p3: the operand must be a power of two. */
    _Alignas(3) char not_a_power; //~ ERROR: is not a power of two

    /* 6.7.5p2 names five declarations an alignment specifier may not appear
       in: a `typedef`, a bit-field, a function, a parameter, and an object
       declared `register`. GCC's `aligned` attribute *is* allowed on the
       first and the third, and each of those has an answer of its own. */
    _Alignas(16) typedef int aligned_int; //~ ERROR: not allowed on a 'typedef'

    _Alignas(16) void a_function(void); //~ ERROR: not allowed on a function

    struct Bits { _Alignas(16) int b : 3; }; //~ ERROR: cannot be applied to a bit-field

    void takes_one(_Alignas(16) int x); //~ ERROR: not allowed on a parameter

    int in_a_register(void) {
        register _Alignas(16) int x = 0; //~ ERROR: not allowed on an object declared 'register'
        return x;
    }
}

fn main() {}
