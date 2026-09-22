//! Integration tests that *run* translated C using GNU inline assembly.
//!
//! GCC's `asm` becomes `core::arch::asm!` (see `crates/cinrs-core/src/sema/
//! asm.rs` for the mapping). Every function below was also compiled, with a
//! `main` printing its results, by `gcc -O2` (gcc 15.2 on x86-64), and the
//! values asserted here are the ones gcc's build printed — so each assertion
//! says the generated `asm!` does what GCC's `asm` does, not what someone
//! expected it to. None of that needs gcc at test time.
//!
//! What must *not* compile — memory operands, the x87 and MMX constraints,
//! rbx, `%=`, `asm goto`, asm in a safe function or on another architecture —
//! is in `tests/ui/inline_asm_refused.rs`.
#![cfg(any(target_arch = "x86_64", target_arch = "x86"))]

use cinrs::gnu99;

gnu99! {
    unsigned long long rdtsc_read(void) {
        unsigned lo, hi;
        __asm__ __volatile__("rdtsc" : "=a"(lo), "=d"(hi));
        return ((unsigned long long)hi << 32) | lo;
    }
    int bsr32(unsigned x) { int r; asm("bsrl %1, %0" : "=r"(r) : "rm"(x)); return r; }
    int bsf32(unsigned x) { int r; asm("bsfl %1, %0" : "=r"(r) : "rm"(x)); return r; }
    __attribute__((target("lzcnt")))
    int lzcnt32(unsigned x) { unsigned r; asm("lzcntl %1, %0" : "=r"(r) : "r"(x)); return (int)r; }
    int swap_digits(int a, int b) { asm("xchgl %0, %1" : "+r"(a), "+r"(b)); return a * 10 + b; }
    int add_ri(int a, int b) {
        asm("addl %1, %0" : "+r"(a) : "ri"(b));
        asm("addl %1, %0" : "+r"(a) : "ri"(100));
        return a;
    }
    unsigned char byte_add(unsigned x) {
        unsigned char c = (unsigned char)x;
        asm("addb %b1, %0" : "+q"(c) : "q"(x));
        return c;
    }
    unsigned word_inc(unsigned x) { asm("incw %w0" : "+r"(x)); return x; }
    int barrier_sum(void) {
        int i, s = 0;
        for (i = 0; i < 10; i++) { s += i; asm volatile("" ::: "memory"); }
        return s;
    }
    int fences(void) { asm("mfence"); asm volatile("pause"); __asm__("nop"); return 7; }
    int shl_imm(int x) { asm("shll %1, %0" : "+r"(x) : "i"(3)); return x; }
    int times_four(int x) {
        asm("movl %1, %%eax\n\tshll $2, %%eax\n\tmovl %%eax, %0" : "=r"(x) : "r"(x) : "eax");
        return x;
    }
    int named_sub(int a, int b) { asm("subl %[b], %[a]" : [a] "+r"(a) : [b] "r"(b)); return a; }
    struct pt { int x; int y; };
    int member_out(struct pt *p, int v) {
        asm("leal 1(%1), %0" : "=r"(p->x) : "r"(v));
        asm("movl $-5, %0" : "=r"(p->y));
        return p->x * 100 + p->y;
    }
    /* The rewrite the memory-operand refusal recommends: the address in a
     * register, and the memory reference written in the template. */
    int pointer_in(int *q) {
        int r;
        asm("movl (%1), %0\n\taddl $1, (%1)" : "=&r"(r) : "r"(q) : "memory");
        return r * 100 + *q;
    }
    int pointer_out(int *q, int v) { asm("movl %1, %0" : "=r"(*q) : "r"(v)); return *q; }
    int loop_sum(int n) {
        int s = 0, i;
        for (i = 0; i < n; i++) asm("addl %1, %0" : "+r"(s) : "r"(i));
        return s;
    }
    int switch_asm(int k) {
        int r = 0;
        switch (k) {
        case 0: asm("movl $10, %0" : "=r"(r)); break;
        case 1: asm("movl $20, %0" : "=r"(r)); /* fall through */
        case 2: asm("addl $1, %0" : "+r"(r)); break;
        default: asm("movl $-1, %0" : "=r"(r));
        }
        return r;
    }
    /* A backward goto: the function is lowered through the control-flow
     * graph, which carries the asm statement as a simple statement. */
    int goto_asm(int n) {
        int i = 0, s = 0;
    again:
        asm("addl %1, %0" : "+r"(s) : "r"(i));
        if (++i < n) goto again;
        return s;
    }
    unsigned mul_wide32(unsigned a, unsigned b, unsigned *hi) {
        unsigned lo, h;
        asm("mull %3" : "=a"(lo), "=d"(h) : "0"(a), "r"(b) : "cc");
        *hi = h;
        return lo;
    }
    double sqrt_x(double v) { double r; asm("sqrtsd %1, %0" : "=x"(r) : "x"(v)); return r; }
}

#[test]
fn rdtsc_counts_up() {
    let (t0, t1) = unsafe { (rdtsc_read(), rdtsc_read()) };
    assert!(t0 != 0);
    assert!(t1 >= t0);
}

#[test]
fn bit_scans() {
    unsafe {
        assert_eq!((bsr32(0x40), bsr32(1), bsr32(0x8000_0000)), (6, 0, 31));
        assert_eq!((bsf32(0x40), bsf32(0x8000_0000), bsf32(12)), (6, 31, 2));
        if is_x86_feature_detected!("lzcnt") {
            assert_eq!((lzcnt32(1), lzcnt32(0), lzcnt32(0xffff)), (31, 32, 16));
        }
    }
}

#[test]
fn read_write_operands_and_immediates() {
    unsafe {
        assert_eq!(swap_digits(1, 2), 21);
        assert_eq!(add_ri(5, 7), 112);
        assert_eq!(shl_imm(5), 40);
        assert_eq!(shl_imm(-1), -8);
        assert_eq!(named_sub(50, 8), 42);
        assert_eq!(times_four(11), 44);
    }
}

#[test]
fn sub_register_modifiers() {
    unsafe {
        // `%b1` on a `"q"` operand: the low byte of a 32-bit register.
        assert_eq!(byte_add(0x1f0), 224);
        assert_eq!(byte_add(0x81), 2);
        // `%w0`: only the low 16 bits change.
        assert_eq!(word_inc(0x1ffff), 0x10000);
        assert_eq!(word_inc(0x1234_1234), 0x1234_1235);
    }
}

#[test]
fn barriers_and_basic_asm() {
    unsafe {
        assert_eq!(barrier_sum(), 45);
        assert_eq!(fences(), 7);
    }
}

#[test]
fn outputs_through_places_and_pointer_inputs() {
    unsafe {
        let mut p = pt { x: 0, y: 0 };
        assert_eq!(member_out(&mut p, 41), 4195);
        assert_eq!((p.x, p.y), (42, -5));
        let mut q = 9;
        assert_eq!(pointer_in(&mut q), 910);
        assert_eq!(q, 10);
        assert_eq!(pointer_out(&mut q, 77), 77);
        assert_eq!(q, 77);
    }
}

#[test]
fn asm_in_loops_switches_and_gotos() {
    unsafe {
        assert_eq!((loop_sum(10), loop_sum(0)), (45, 0));
        assert_eq!(
            (switch_asm(0), switch_asm(1), switch_asm(2), switch_asm(3)),
            (10, 21, 1, -1)
        );
        assert_eq!((goto_asm(5), goto_asm(1)), (10, 0));
    }
}

#[test]
fn explicit_register_pairs_and_sse() {
    unsafe {
        let mut hi = 0;
        assert_eq!(mul_wide32(0x8000_0001, 6, &mut hi), 6);
        assert_eq!(hi, 3);
        assert_eq!(sqrt_x(2.25), 1.5);
        // gcc printed 1.4142135623730951, which is `SQRT_2` exactly.
        assert_eq!(sqrt_x(2.0), core::f64::consts::SQRT_2);
    }
}

// ---------------------------------------------------------------------------
// x86-64 only: 64-bit operands. `long long` rather than `long`, because a
// `q`-suffixed instruction needs a 64-bit operand and Windows's `long` is 32.
// ---------------------------------------------------------------------------

#[cfg(target_arch = "x86_64")]
mod x86_64_only {
    cinrs::gnu99! {
        long long swap_long(long long a, long long b) { asm("xchgq %0, %1" : "+r"(a), "+r"(b)); return a * 10 + b; }
        unsigned long long k_on_64(unsigned long long v) { asm("notl %k0" : "+r"(v)); return v; }
        unsigned long long shl_q(unsigned long long x) { asm("shlq %1, %q0" : "+r"(x) : "i"(40)); return x; }
        unsigned long long mul_wide64(unsigned long long a, unsigned long long b, unsigned long long *hi) {
            unsigned long long lo, h;
            asm("mulq %3" : "=a"(lo), "=d"(h) : "0"(a), "r"(b) : "cc");
            *hi = h;
            return lo;
        }
    }

    #[test]
    fn sixty_four_bit_operands() {
        unsafe {
            assert_eq!(swap_long(3, 4), 43);
            // `%k0` names the 32-bit register, and writing it clears the top.
            assert_eq!(k_on_64(0xffff_ffff_0000_0000), 0xffff_ffff);
            assert_eq!(k_on_64(5), 0xffff_fffa);
            assert_eq!(shl_q(3), 0x300_0000_0000);
            let mut hi = 0;
            assert_eq!(mul_wide64(0x8000_0000_0000_0001, 6, &mut hi), 6);
            assert_eq!(hi, 3);
        }
    }
}

// ---------------------------------------------------------------------------
// <cpuid.h>
// ---------------------------------------------------------------------------

gnu99! {
    #include <cpuid.h>

    unsigned cpuid_max(unsigned *sig) { return __get_cpuid_max(0, sig); }
    /* The vendor string is %ebx, %edx, %ecx of leaf 0, in that order. */
    void vendor_words(unsigned *w) { unsigned a; __cpuid(0, a, w[0], w[2], w[1]); }
    int vendor_known(void) {
        unsigned a, b, c, d;
        __cpuid(0, a, b, c, d);
        if (b == signature_INTEL_ebx && d == signature_INTEL_edx && c == signature_INTEL_ecx)
            return 1;
        if (b == signature_AMD_ebx && d == signature_AMD_edx && c == signature_AMD_ecx)
            return 2;
        return 0;
    }
    int has_sse2(void) {
        unsigned a, b, c, d;
        if (!__get_cpuid(1, &a, &b, &c, &d)) return -1;
        return (d & bit_SSE2) != 0;
    }
    /* 1 when leaf 7's AVX2 bit and __builtin_cpu_supports agree. */
    int avx2_agrees(void) {
        unsigned a, b, c, d;
        int bit = __get_cpuid(7, &a, &b, &c, &d) && (b & bit_AVX2) != 0;
        return bit == (__builtin_cpu_supports("avx2") != 0);
    }
}

#[cfg(target_arch = "x86")]
use core::arch::x86::__cpuid as rust_cpuid;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::__cpuid as rust_cpuid;

#[test]
fn cpuid_h_reads_the_processor() {
    unsafe {
        let mut sig = 0;
        let max = cpuid_max(&mut sig);
        assert!(max >= 1, "leaf 0 reports {max}");
        let mut words = [0u32; 3];
        vendor_words(words.as_mut_ptr());
        let vendor: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert!(vendor.iter().all(|b| b.is_ascii_graphic()), "{vendor:?}");
        // The same answer Rust's own `__cpuid` gives, and `sig` is %ebx.
        let leaf0 = rust_cpuid(0);
        assert_eq!(words, [leaf0.ebx, leaf0.edx, leaf0.ecx]);
        assert_eq!((sig, max), (leaf0.ebx, leaf0.eax));
        let expected = match &vendor[..] {
            b"GenuineIntel" => 1,
            b"AuthenticAMD" => 2,
            _ => 0,
        };
        assert_eq!(vendor_known(), expected);
        if cfg!(target_arch = "x86_64") {
            assert_eq!(has_sse2(), 1);
        }
        assert_eq!(avx2_agrees(), 1);
    }
}
