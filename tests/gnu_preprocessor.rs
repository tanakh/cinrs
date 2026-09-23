//! The GNU preprocessor extensions.

use cinrs::{c99, gnu99};

// ---------------------------------------------------------------------------
// comma elision and named variadic parameters
// ---------------------------------------------------------------------------

c99! { r#"
    #include <stdio.h>

    static char log_buffer[256];

    /* The two spellings of the same GNU rule: `, ## __VA_ARGS__` deletes the
     * comma when the invocation passed no variable arguments — without it the
     * call would end in a stray comma and not compile at all. */
    #define LOG(fmt, ...)         snprintf(log_buffer, sizeof log_buffer, "[" fmt "]", ## __VA_ARGS__)
    #define LOG2(fmt, args...)         snprintf(log_buffer, sizeof log_buffer, "<" fmt ">" , ## args)

    void log_nothing(void) { LOG("plain"); }
    void log_one(int n) { LOG("n=%d", n); }
    void log_named_nothing(void) { LOG2("plain"); }
    void log_named_two(int a, int b) { LOG2("%d,%d", a, b); }

    const char *log_text(void) { return log_buffer; }
    void log_reset(void) { log_buffer[0] = 0; }
"# }

#[test]
fn comma_elision_drops_the_comma_when_there_is_nothing_after_it() {
    let text = || {
        unsafe { core::ffi::CStr::from_ptr(log_text()) }
            .to_bytes()
            .to_vec()
    };
    unsafe { log_reset() };
    unsafe { log_nothing() };
    assert_eq!(text(), b"[plain]");
    unsafe { log_reset() };
    unsafe { log_one(7) };
    assert_eq!(text(), b"[n=7]");
    unsafe { log_reset() };
    unsafe { log_named_nothing() };
    assert_eq!(text(), b"<plain>");
    unsafe { log_reset() };
    unsafe { log_named_two(1, 2) };
    assert_eq!(text(), b"<1,2>");
}

// ---------------------------------------------------------------------------
// `__COUNTER__`
// ---------------------------------------------------------------------------

c99! { r#"
    #define CONCAT_(a, b) a ## b
    #define CONCAT(a, b) CONCAT_(a, b)
    #define UNIQUE(prefix) CONCAT(prefix, __COUNTER__)

    static int UNIQUE(slot_) = 10;
    static int UNIQUE(slot_) = 20;
    static int UNIQUE(slot_) = 30;

    int counter_slots(void) { return slot_0 + slot_1 + slot_2; }
    int counter_next(void) { return __COUNTER__; }
"# }

#[test]
fn counter_hands_out_a_fresh_number_every_time() {
    assert_eq!(unsafe { counter_slots() }, 60);
    assert_eq!(unsafe { counter_next() }, 3);
}

// ---------------------------------------------------------------------------
// `__has_include` and the rest of the family
// ---------------------------------------------------------------------------

c99! {
    #if __has_include(<stdio.h>)
    int has_stdio(void) { return 1; }
    #else
    int has_stdio(void) { return 0; }
    #endif

    #if __has_include("nowhere-at-all.h")
    int has_nonsense(void) { return 1; }
    #else
    int has_nonsense(void) { return 0; }
    #endif

    #if __has_attribute(packed) && __has_attribute(__always_inline__)
    int has_attributes(void) { return 1; }
    #else
    int has_attributes(void) { return 0; }
    #endif

    /* `cleanup` is honoured, so the answer has to be yes; `vector_size` is
       known and refused, so that one is still no. */
    #if __has_attribute(cleanup)
    int has_cleanup(void) { return 1; }
    #else
    int has_cleanup(void) { return 0; }
    #endif

    #if __has_attribute(vector_size)
    int has_vector_size(void) { return 1; }
    #else
    int has_vector_size(void) { return 0; }
    #endif

    #if __has_builtin(__builtin_add_overflow)
    int has_overflow(void) { return 1; }
    #else
    int has_overflow(void) { return 0; }
    #endif

    #if __has_feature(c_static_assert) && __has_feature(c_atomic) && !__has_feature(blocks)
    int has_features(void) { return 1; }
    #else
    int has_features(void) { return 0; }
    #endif

    /* The atomic builtins answer `__has_builtin` under their own names. */
    #if __has_builtin(__atomic_fetch_add) && __has_builtin(__sync_synchronize)
    int has_atomic_builtins(void) { return 1; }
    #else
    int has_atomic_builtins(void) { return 0; }
    #endif
}

#[test]
fn the_has_family_answers_about_this_implementation() {
    assert_eq!(unsafe { has_stdio() }, 1);
    assert_eq!(unsafe { has_nonsense() }, 0);
    assert_eq!(unsafe { has_attributes() }, 1);
    assert_eq!(unsafe { has_cleanup() }, 1);
    assert_eq!(unsafe { has_vector_size() }, 0);
    assert_eq!(unsafe { has_overflow() }, 1);
    assert_eq!(unsafe { has_features() }, 1);
    assert_eq!(unsafe { has_atomic_builtins() }, 1);
}

// ---------------------------------------------------------------------------
// `#pragma push_macro` / `pop_macro`
// ---------------------------------------------------------------------------

c99! {
    #define LIMIT 10

    int before(void) { return LIMIT; }

    #pragma push_macro("LIMIT")
    #undef LIMIT
    #define LIMIT 99
    int during(void) { return LIMIT; }
    #pragma pop_macro("LIMIT")

    int after(void) { return LIMIT; }

    /* Popping a macro that was never pushed is ignored, as GCC does. */
    #pragma pop_macro("NEVER_PUSHED")
}

#[test]
fn push_macro_puts_the_old_definition_back() {
    assert_eq!(unsafe { before() }, 10);
    assert_eq!(unsafe { during() }, 99);
    assert_eq!(unsafe { after() }, 10);
}

// ---------------------------------------------------------------------------
// the pragmas that are accepted and ignored, and `_Pragma`
// ---------------------------------------------------------------------------

c99! { r#"
    #pragma GCC diagnostic push
    #pragma GCC diagnostic ignored "-Wunused"
    #pragma GCC system_header
    #pragma GCC visibility push(default)
    #pragma message("this pragma says nothing to cinrs")
    #pragma region something
    #pragma weak nothing_at_all
    #pragma endregion
    #pragma GCC diagnostic pop
    #pragma GCC visibility pop
    #pragma redefine_extname old new

    /* The standard's own three (C99 6.10.6 and 7.12.2) are ignored as well:
     * what the generated Rust does is the `OFF` state of each, so a unit that
     * asks for that gets it and one that asks for the other gets no change. */
    #pragma STDC FP_CONTRACT OFF
    #pragma STDC FENV_ACCESS OFF
    #pragma STDC CX_LIMITED_RANGE ON

    /* And a vendor's, which is what 6.10.6 is really about. */
    #pragma omp parallel for

    #define DO_PRAGMA(x) _Pragma(#x)
    DO_PRAGMA(GCC diagnostic ignored "-Wall")
    _Pragma("once")

    int quiet(void) { return 1; }
"# }

#[test]
fn the_ignorable_pragmas_are_ignored() {
    assert_eq!(unsafe { quiet() }, 1);
}

// ---------------------------------------------------------------------------
// the predefined macros
// ---------------------------------------------------------------------------

c99! {
    #if defined(__GNUC__) && __GNUC__ >= 4
    int gnuc_major(void) { return __GNUC__; }
    #else
    int gnuc_major(void) { return 0; }
    #endif

    int gnuc_minor(void) { return __GNUC_MINOR__; }
    int gnuc_patch(void) { return __GNUC_PATCHLEVEL__; }

    #ifdef __STRICT_ANSI__
    int strict(void) { return 1; }
    #else
    int strict(void) { return 0; }
    #endif

    const char *version(void) { return __VERSION__; }
    const char *base_file(void) { return __BASE_FILE__; }
    const char *file_name(void) { return __FILE_NAME__; }
    const char *timestamp(void) { return __TIMESTAMP__; }
    int include_level(void) { return __INCLUDE_LEVEL__; }

    /* Annex G is never claimed, whether or not complex arithmetic is here. */
    #ifndef __STDC_IEC_559_COMPLEX__
    int subsetting(void) { return 1; }
    #else
    int subsetting(void) { return 0; }
    #endif

    /* C11's threads are the platform's own, so this one follows the *target*:
       the macro is predefined exactly where the bundled `<threads.h>` refuses,
       which is every target whose C library it cannot lay the objects out
       for. */
    #ifdef __STDC_NO_THREADS__
    int no_threads(void) { return 1; }
    #else
    int no_threads(void) { return 0; }
    #endif

    /* Complex arithmetic is the one optional part that depends on a cargo
       feature, so the answer is checked against `cfg!` rather than pinned. */
    #ifdef __STDC_NO_COMPLEX__
    int no_complex(void) { return 1; }
    #else
    int no_complex(void) { return 0; }
    #endif

    /* Neither atomics nor variable length arrays are left out, so the macros
       that would say so are not defined — and the `__ATOMIC_*` orders and the
       `__sync_*` advertisement are, in every entry point. */
    #if !defined(__STDC_NO_VLA__)
    int has_vla(void) { return 1; }
    #else
    int has_vla(void) { return 0; }
    #endif

    #if !defined(__STDC_NO_ATOMICS__) && __ATOMIC_SEQ_CST == 5 && defined(__GCC_HAVE_SYNC_COMPARE_AND_SWAP_4)
    int has_atomics(void) { return 1; }
    #else
    int has_atomics(void) { return 0; }
    #endif
}

gnu99! {
    #ifdef __STRICT_ANSI__
    int gnu_strict(void) { return 1; }
    #else
    int gnu_strict(void) { return 0; }
    #endif
}

#[test]
fn the_predefined_macros_say_what_this_implementation_is() {
    // cinrs presents itself as GCC 14.2.0; see `doc/features.md`.
    assert_eq!(unsafe { gnuc_major() }, 14);
    assert_eq!(unsafe { gnuc_minor() }, 2);
    assert_eq!(unsafe { gnuc_patch() }, 0);
    // Only a strict entry point is `-std=c99`.
    assert_eq!(unsafe { strict() }, 1);
    assert_eq!(unsafe { gnu_strict() }, 0);
    assert_eq!(unsafe { subsetting() }, 1);
    assert_eq!(
        unsafe { no_complex() },
        i32::from(!cfg!(feature = "complex")),
        "__STDC_NO_COMPLEX__ has to follow the 'complex' feature"
    );
    assert_eq!(
        unsafe { no_threads() },
        i32::from(!cfg!(all(
            target_os = "linux",
            any(target_env = "gnu", target_env = "musl")
        ))),
        "__STDC_NO_THREADS__ has to follow whether <threads.h> declares anything"
    );
    assert_eq!(unsafe { has_vla() }, 1);
    assert_eq!(unsafe { has_atomics() }, 1);

    let text = |p| unsafe { core::ffi::CStr::from_ptr(p) }.to_bytes().to_vec();
    // GCC's number first, which is what a program parsing it reads, and then
    // who really compiled it.
    assert_eq!(
        text(unsafe { version() }),
        format!("14.2.0 (cinrs {})", env!("CARGO_PKG_VERSION")).into_bytes()
    );
    // `__BASE_FILE__` names the outermost file and `__FILE_NAME__` drops the
    // directory; both are the `.rs` file the block is written in.
    assert!(text(unsafe { base_file() }).ends_with(b"gnu_preprocessor.rs"));
    assert_eq!(text(unsafe { file_name() }), b"gnu_preprocessor.rs");
    // Fixed, like `__DATE__`, so that a build gives the same output twice.
    assert_eq!(text(unsafe { timestamp() }), b"??? ??? ?? ??:??:?? ????");
    assert_eq!(unsafe { include_level() }, 0);
}

c99! {
    #include "include/include_level.h"

    int level_in_a_header(void) { return header_level(); }
}

#[test]
fn include_level_counts_the_open_files() {
    assert_eq!(unsafe { level_in_a_header() }, 1);
}
