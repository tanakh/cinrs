//! Inline assembly while its code generation is still to come: sema maps the
//! statement onto `asm!`, and code generation says it cannot emit it yet
//! rather than dropping it. (Replaced by `inline_asm_refused.rs` once the
//! `asm!` is emitted.) The `asm` *label* on a declaration is a different
//! thing, and is supported.

cinrs::gnu99! {
    int add(int a, int b) {
        int sum;
        asm volatile ("add %1, %0" : "=r"(sum) : "r"(a), "0"(b)); //~ ERROR: inline assembly: code generation is not implemented yet
        return sum;
    }

    void barrier(void) {
        __asm__ __volatile__("" ::: "memory"); //~ ERROR: inline assembly: code generation is not implemented yet
    }
}

fn main() {}
