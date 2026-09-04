//! The two plain spellings GCC keeps for its `gnu*` modes. The diagnostic
//! names the entry point that has them, and the `__`-spelled form that is
//! available everywhere.

cinrs::c99! {
    int f(void) {
        asm("nop"); //~ ERROR: requires a GNU dialect
        return 0;
    }

    typeof(int) x; //~ ERROR: requires a GNU dialect
    //~^ ERROR: expected a declaration
}

fn main() {}
