//@compile-flags: --crate-type lib
//! `weak` on a declaration of something the unit does not define is a weak
//! reference (`tests/gnu_attributes.rs`), and on one it defines an ordinary
//! definition with a warning. What is refused is what GCC refuses — a weak
//! name with no linkage — and a weak reference baked into a `static`, whose
//! null test LLVM would fold.

cinrs::gnu99! {
    static int hidden(int) __attribute__((weak)); //~ ERROR: weak declaration of 'hidden' must be public

    int counter_user(void) {
        static int local __attribute__((weak)) = 1; //~ ERROR: weak declaration of 'local' must be public
        return local;
    }

    __attribute__((weak)) int hook(int);
    int (*table[])(int) = { hook }; //~ ERROR: takes the address of 'hook', which is declared weak

    int use_them(void) { return hidden(1); }
}
