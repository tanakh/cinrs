//! The builtins whose operands have to be more than well typed.

cinrs::c99! {
    int choose(int n) {
        return __builtin_choose_expr(n, 1, 2); //~ ERROR: is not a compile-time constant
    }

    int overflow(int a, int b, int c) {
        return __builtin_add_overflow(a, b, c); //~ ERROR: must be a pointer to an integer
    }

    int backwards(int c) {
        switch (c) {
        case 5 ... 1: //~ ERROR: empty case range
            return 1;
        default:
            return 0;
        }
    }

    /* `alloca`'s memory belongs to a function, and there is none here. */
    void *scratch = __builtin_alloca(8); //~ ERROR: is only allowed inside a function

    void *aligned(unsigned long n) {
        /* GCC's alignment is a constant in bits; the emulated arena is
           16-byte aligned, so 128 is as far as the promise goes. */
        return __builtin_alloca_with_align(n, 256); //~ ERROR: must be between 1 and 128 bits
    }

    int backwards_designator(void) {
        static const int table[8] = { [5 ... 1] = 3 }; //~ ERROR: empty range designator
        return table[0];
    }
}

fn main() {}
