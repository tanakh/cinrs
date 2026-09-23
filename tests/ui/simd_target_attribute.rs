//@compile-flags: --crate-type lib
//! `__attribute__((target("…")))`, and what it cannot ask for.
//!
//! The attribute becomes `#[target_feature(enable = "…")]`, which enables an
//! instruction set and nothing else: it cannot select a processor, it cannot
//! turn an instruction set *off*, and it cannot name one rustc has not
//! stabilised. Every one of those is refused rather than ignored, because an
//! ignored `target` is a function compiled for the baseline and a program that
//! crashes on the first instruction it does not have.

cinrs::c11! {
    #include <immintrin.h>

    __attribute__((target("avx2"))) int good(void) { return 1; }
    /* GCC's own spellings for two instruction sets at once, and its name for
     * LZCNT and POPCNT together. */
    __attribute__((target("sse4.2,popcnt"))) int also_good(void) { return 2; }
    __attribute__((target("abm"))) int abm_is_two_of_them(void) { return 3; }

    __attribute__((target("sse4a"))) int too_new(void) { return 4; } //~ ERROR: SSE4a target feature is still unstable

    __attribute__((target("arch=haswell"))) int a_processor(void) { return 5; } //~ ERROR: selects a processor

    __attribute__((target("no-sse2"))) int turned_off(void) { return 6; } //~ ERROR: turns an instruction set off

    __attribute__((target("frobnicate"))) int made_up(void) { return 7; } //~ ERROR: unknown instruction set 'frobnicate'

    __attribute__((target("mmx"))) int no_mmx(void) { return 8; } //~ ERROR: Rust's core::arch has no MMX

    int bad_cpu_question(void) {
        return __builtin_cpu_supports("frobnicate"); //~ ERROR: unknown instruction set 'frobnicate'
    }

    int a_processor_question(void) {
        return __builtin_cpu_is("haswell"); //~ ERROR: '__builtin_cpu_is' is not a builtin this crate implements
    }
}
