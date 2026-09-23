//@compile-flags: --crate-type lib
//! `_Pragma`'s operand is macro-replaced before it is destringized, as in GCC
//! and Clang, so CRoaring's way of opening a target region out of a macro —
//! `_Pragma(STRINGIFY(GCC target(T)))` — is a pragma. An operand that is still
//! not one string literal after replacement is one error, and none of it is
//! left behind for the parser to trip over.

cinrs::gnu11! {
    #include <immintrin.h>

    #define STRINGIFY_IMPLEMENTATION_(a) #a
    #define STRINGIFY(a) STRINGIFY_IMPLEMENTATION_(a)
    #define CROARING_TARGET_REGION(T) _Pragma("GCC push_options") _Pragma(STRINGIFY(GCC target(T)))
    #define CROARING_UNTARGET_REGION _Pragma("GCC pop_options")
    #define CROARING_TARGET_AVX2 CROARING_TARGET_REGION("avx2,bmi,pclmul,lzcnt,popcnt")

    CROARING_TARGET_AVX2
    static inline int twice(int x) { return x * 2; }
    int region(void) { return twice(21); }
    CROARING_UNTARGET_REGION
}

cinrs::gnu11! {
    #define NOT_A_STRING(a) a
    _Pragma(NOT_A_STRING(GCC target("avx2"))) //~ ERROR: '_Pragma' takes one string literal
    int after(void) { return 1; }
}
