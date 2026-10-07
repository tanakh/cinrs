//@compile-flags: --crate-type lib
//! What GCC's inline assembly has that Rust's `asm!` cannot say.
//!
//! Each refusal names the constraint or the feature and, where there is one,
//! the rewrite: `asm!` has no x87 or MMX registers, no register pair, no flag
//! outputs and no `%=`; a memory operand is its address, so a bit-field cannot
//! be one; rustc keeps rsp and rbp; and, as in GCC, rbx cannot be both a "b"
//! operand and a clobber.
//!
//! Everything else about extended asm maps (`sema/asm.rs`).

cinrs::gnu99! {
    #pragma cinrs target "x86_64-unknown-linux-gnu"
    struct S { int b : 3; };
    struct Pair { int a, b; };

    void refused(int x, double d, unsigned long long u, struct S *s, struct Pair p, char c) {
        asm("incl %0" : "+m"(s->b)); //~ ERROR: a bit-field cannot be a memory operand ("+m")
        asm("incl %k0" : "+m"(x)); //~ ERROR: the operand modifier '%k' cannot apply to a memory ("m") operand
        asm("" : : "m"(x + 1)); //~ ERROR: the constraint "m" asks for memory, and this operand is not an lvalue
        asm("" : "=m"(x) : "0"(1)); //~ ERROR: operand 0 is a memory operand
        asm("" : : "p"(&x)); //~ ERROR: the constraint "p" (an operand that is an address) is not supported
        asm("rdtsc" : "=A"(u)); //~ ERROR: the constraint "A" (the edx:eax pair) is not supported
        asm("" : "=f"(d)); //~ ERROR: the constraint "f" (an x87 stack register) is not supported
        asm("" : : "y"(x)); //~ ERROR: the constraint "y" (an MMX register) is not supported
        asm("" : : "X"(x)); //~ ERROR: the constraint "X" (any operand at all) is not supported
        asm("" : : "I"(x)); //~ ERROR: the constraint "I" (a range-checked immediate) is not supported
        asm("" : : "R"(x)); //~ ERROR: the constraint "R" (a legacy register) is not supported
        asm("" : : "Yz"(x)); //~ ERROR: the constraint "Yz" is not supported
        asm("cpuid" : "=b"(x) : : "rbx"); //~ ERROR: the clobber "rbx" is also operand 0 ("b") of this 'asm' statement
        asm("" : : "a"(0), "b"(x) : "bl"); //~ ERROR: the clobber "bl" is also operand 1 ("b") of this 'asm' statement
        asm("sete %0" : "=@ccz"(c)); //~ ERROR: the flag output "=@ccz" is not supported
        asm("1: jmp 1b%=" : :); //~ ERROR: '%=' is not supported
        asm("movl %c0, %%eax" : : "r"(x)); //~ ERROR: the operand modifier '%c' prints a constant without its '$'
        asm("movl %P0, %%eax" : : "a"(x)); //~ ERROR: the operand modifier '%P' prints a constant without its '$'
        asm("movl %a0, %%eax" : : "r"(&x)); //~ ERROR: the operand modifier '%a' is not supported
        asm("" : : "i"(x)); //~ ERROR: this operand is not an integer constant expression
        asm("" : "=r"(s->b)); //~ ERROR: a bit-field cannot be an 'asm' output
        asm("" : : : "rsp"); //~ ERROR: the clobber "rsp" is not supported
        asm("" : : : "7"); //~ ERROR: the clobber "7" (GCC's register sp) is not supported
        asm("" : : : "76"); //~ ERROR: the register "76" (GCC's r16, one of APX's r16 to r31) cannot be clobbered for this target
        asm("" : : : "92"); //~ ERROR: unknown register name "92" in the clobber list
        asm("{movl|mov %%eax, %%ebx" : :); //~ ERROR: a '{' with no '}' after it in an 'asm' template
        asm(".intel_syntax noprefix\n mov eax, ebx"); //~ ERROR: a template that switches to Intel syntax
        asm("" : : "r"(p)); //~ ERROR: an 'asm' operand has to have integer, floating or pointer type
        asm("incl %k0" : "+r"(c)); //~ ERROR: the operand modifier '%k' cannot apply to an 8-bit operand
        asm("mov %1, %0" : "=r"(x)); //~ ERROR: '%1' names operand 1 that this 'asm' statement does not have
        register int r asm("eax") = 1; //~ ERROR: an 'asm' label on a local variable is not supported
        (void)r;
    }

    int jumps(int x) {
        asm goto ("jmp %l0" : : : : out); //~ ERROR: 'asm goto' is not supported yet
        return 0;
    out:
        return 1;
    }
}

cinrs::c23! {
    #pragma cinrs target "x86_64-unknown-linux-gnu"
    [[cinrs::safe]] void fence(void) {
        __asm__ __volatile__("mfence"); //~ ERROR: inline assembly cannot be written in the safe function 'fence'
    }
}

// A 256-bit "x" operand is a ymm register, which needs AVX: in a function
// without it, rustc's own error says so.
cinrs::gnu99! {
    #pragma cinrs target "x86_64-unknown-linux-gnu"
    #include <immintrin.h>
    void keep(__m256i v) {
        __asm__("" : "+x"(v)); //~ ERROR: register class `ymm_reg` requires the `avx` target feature
    }
}

cinrs::gnu99! {
    #pragma cinrs target "aarch64-unknown-linux-gnu"
    void pause(void) {
        asm volatile ("yield"); //~ ERROR: inline assembly is only supported on x86 and x86-64
    }
}
