//@compile-flags: --crate-type lib
//! `__builtin_cpu_supports` in a unit that said `no_std`.
//!
//! It becomes `std::is_x86_feature_detected!`, and `core` has no processor
//! detection at all: reading the `cpuid` leaves and caching the answer is what
//! `std_detect` is, and there is nothing in `core` to fall back on. A `no_std`
//! unit therefore has to be told which instruction sets it may use, which is
//! what the `target` attribute says.

cinrs::c11! {
    #pragma cinrs no_std
    #include <immintrin.h>

    int wide(void) {
        return __builtin_cpu_supports("avx2"); //~ ERROR: '__builtin_cpu_supports' requires std
    }
}
