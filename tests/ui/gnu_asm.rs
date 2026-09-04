//! Inline assembly is refused rather than half translated: Rust's own
//! `core::arch::asm!` has operand constraints of its own, and mapping GCC's
//! onto them is a project rather than a feature. The `asm` *label* on a
//! declaration is a different thing, and is supported.

cinrs::gnu99! {
    int add(int a, int b) {
        int sum;
        asm volatile ("add %1, %0" : "=r"(sum) : "r"(a), "0"(b)); //~ ERROR: inline assembly is not supported
        return sum;
    }

    void barrier(void) {
        __asm__ __volatile__("" ::: "memory"); //~ ERROR: inline assembly is not supported
    }
}

fn main() {}
