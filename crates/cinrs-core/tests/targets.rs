//! What the front end answers when it is translating for another machine.
//!
//! `Options::for_target` is the whole of cross-compilation as far as this
//! crate is concerned, so every question a data model decides can be asked
//! here with no cross toolchain in sight: the widths, the alignments, the type
//! an integer constant gets, the predefined macros, and which branch the
//! bundled headers take. The C says what it expects with `_Static_assert`, so
//! an answer that is wrong is an error message naming the assertion rather
//! than a value buried in a debug dump.
//!
//! # Where the expected values come from
//!
//! The `i686-unknown-linux-gnu` ones were checked against the host's own
//! `gcc -m32` on the machine this was written on — `_Static_assert`s compiled
//! with `-std=c11 -ffreestanding`, and `-dM -E` for the predefined macros.
//! Freestanding because `gcc -m32` here has no 32-bit C library headers
//! (`gcc-multilib` is not installed), which is enough: the data model, the
//! struct layouts and the `__SIZE_TYPE__` family are the compiler's answers,
//! not the library's. `sizeof(long)` 4, `_Alignof(long long)` 4,
//! `sizeof(struct { char; long long; })` 12 at offset 4, `2147483648` typed
//! `long long`, `__SIZE_TYPE__` `unsigned int`, `__INTMAX_TYPE__`
//! `long long int` and no `__SIZEOF_INT128__` all come from that run.
//!
//! The `x86_64-unknown-linux-gnu` ones come from plain `gcc`; the Windows and
//! AArch64 ones from the ABI documents and from `core::ffi`'s own `cfg`
//! cascade, which the generated code has to agree with anyway.
//! `tests/cross_targets.rs` in the top-level crate is what checks the same
//! models against a real `cargo check --target`.

use std::str::FromStr;

use cinrs_core::{Level, Options, Standard, TargetModel, analyze, sema};
use proc_macro2::TokenStream;

/// The model of `triple`, which the table must know.
fn model(triple: &str) -> TargetModel {
    TargetModel::from_triple(triple).unwrap_or_else(|e| panic!("{triple}: {e}"))
}

/// Options translating for `triple`, in C11 so that `_Static_assert` and
/// `_Generic` are available.
fn options(triple: &str) -> Options {
    Options::new(Standard::C11).for_target(model(triple))
}

/// Every error `source` draws, front end and sema alike, in source order.
fn errors_with(source: &str, options: &Options) -> Vec<String> {
    // String-literal mode accepts any C text, including what the Rust lexer
    // refuses.
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let analysis = analyze(input, options);
    let mut out: Vec<String> = analysis
        .diagnostics
        .sorted()
        .into_iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| d.message.clone())
        .collect();
    // `analysis.options` rather than `options`: a `#pragma cinrs target` in
    // the source has already changed the model by now, and sema has to see
    // the one the unit asked for.
    let (_program, diagnostics) =
        sema::analyze(&analysis.unit, &analysis.options, analysis.source.unit_id());
    out.extend(
        diagnostics
            .sorted()
            .into_iter()
            .filter(|d| d.level == Level::Error)
            .map(|d| d.message.clone()),
    );
    out
}

/// Asserts that `source` is accepted when translating for `triple`.
///
/// Every check in these tests is a `_Static_assert`, so "accepted" is the
/// whole assertion.
fn accepts(triple: &str, source: &str) {
    let found = errors_with(source, &options(triple));
    assert!(found.is_empty(), "{triple} rejected:\n{source}\n{found:#?}");
}

/// Asserts that `source` draws exactly these errors when translating for
/// `triple`.
fn rejects(triple: &str, source: &str, expected: &[&str]) {
    let found = errors_with(source, &options(triple));
    assert_eq!(found, expected, "{triple}, for:\n{source}");
}

// ---------------------------------------------------------------------------
// widths
// ---------------------------------------------------------------------------

#[test]
fn the_widths_of_the_standard_types() {
    accepts(
        "x86_64-unknown-linux-gnu",
        r#"
        _Static_assert(sizeof(long) == 8, "");
        _Static_assert(sizeof(void *) == 8, "");
        _Static_assert(sizeof(long long) == 8, "");
        _Static_assert(sizeof(int) == 4, "");
        "#,
    );
    accepts(
        "i686-unknown-linux-gnu",
        r#"
        _Static_assert(sizeof(long) == 4, "");
        _Static_assert(sizeof(void *) == 4, "");
        _Static_assert(sizeof(long long) == 8, "");
        _Static_assert(sizeof(int) == 4, "");
        "#,
    );
    // LLP64: a 64-bit pointer and a 32-bit `long`, which is the whole reason
    // the data-model assertion exists.
    accepts(
        "x86_64-pc-windows-msvc",
        r#"
        _Static_assert(sizeof(long) == 4, "");
        _Static_assert(sizeof(void *) == 8, "");
        "#,
    );
    accepts(
        "wasm32-unknown-unknown",
        r#"
        _Static_assert(sizeof(long) == 4, "");
        _Static_assert(sizeof(void *) == 4, "");
        "#,
    );
}

#[test]
fn the_width_of_wchar_t() {
    for (triple, size) in [
        ("x86_64-unknown-linux-gnu", 4),
        ("aarch64-unknown-linux-gnu", 4),
        ("aarch64-apple-darwin", 4),
        ("x86_64-pc-windows-msvc", 2),
        ("i686-pc-windows-gnu", 2),
    ] {
        accepts(
            triple,
            &format!(
                r#"
                #include <stddef.h>
                _Static_assert(sizeof(wchar_t) == {size}, "");
                _Static_assert(sizeof(L'x') == {size}, "");
                _Static_assert(sizeof(L"ab") == {}, "");
                "#,
                3 * size
            ),
        );
    }
}

/// `wchar_t`'s signedness decides what `L'\xffff'` is worth, and the header
/// and the front end have to agree about it or passing `L"…"` to a
/// `const wchar_t *` would be a type error.
#[test]
fn the_signedness_of_wchar_t() {
    accepts(
        "x86_64-unknown-linux-gnu",
        r#"
        #include <stddef.h>
        _Static_assert((wchar_t)-1 < 0, "int");
        _Static_assert(_Generic(L'x', int: 1), "");
        "#,
    );
    // AArch64's `wchar_t` is `unsigned int` (AAPCS64 §10).
    accepts(
        "aarch64-unknown-linux-gnu",
        r#"
        #include <stddef.h>
        _Static_assert((wchar_t)-1 > 0, "unsigned int");
        _Static_assert(_Generic(L'x', unsigned int: 1), "");
        "#,
    );
    // Windows: `unsigned short`, so `L"😀"` is a surrogate pair and three
    // elements with the terminator rather than two.
    accepts(
        "x86_64-pc-windows-msvc",
        r#"
        #include <stddef.h>
        _Static_assert((wchar_t)-1 > 0, "unsigned short");
        _Static_assert(sizeof(L"\U0001F600") == 6, "surrogate pair");
        "#,
    );
    accepts(
        "x86_64-unknown-linux-gnu",
        r#"_Static_assert(sizeof(L"\U0001F600") == 8, "one code point");"#,
    );
}

/// Plain `char`'s signedness is what `'\xff'` is worth; it follows
/// `core::ffi::c_char`, so Windows and Apple override the architecture.
#[test]
fn the_signedness_of_plain_char() {
    for triple in [
        "x86_64-unknown-linux-gnu",
        "i686-unknown-linux-gnu",
        "wasm32-unknown-unknown",
        "aarch64-pc-windows-msvc",
        "aarch64-apple-darwin",
    ] {
        accepts(
            triple,
            r#"
            _Static_assert('\xff' < 0, "signed");
            _Static_assert((char)-1 < 0, "signed");
            "#,
        );
    }
    for triple in [
        "aarch64-unknown-linux-gnu",
        "armv7-unknown-linux-gnueabihf",
        "riscv64gc-unknown-linux-gnu",
        "s390x-unknown-linux-gnu",
    ] {
        accepts(
            triple,
            r#"
            _Static_assert('\xff' > 0, "unsigned");
            _Static_assert((char)-1 > 0, "unsigned");
            "#,
        );
    }
}

// ---------------------------------------------------------------------------
// the type an integer constant gets
// ---------------------------------------------------------------------------

/// C99 6.4.4.1p5: a decimal constant takes the first of `int`, `long`,
/// `long long` that holds it. `2147483648` therefore lands on `long` under
/// LP64 and on `long long` under ILP32 and LLP64 — and `gcc -m32` agrees.
#[test]
fn the_type_of_an_integer_constant() {
    accepts(
        "x86_64-unknown-linux-gnu",
        r#"_Static_assert(_Generic(2147483648, long: 1, long long: 2) == 1, "");"#,
    );
    for triple in ["i686-unknown-linux-gnu", "x86_64-pc-windows-msvc"] {
        accepts(
            triple,
            r#"_Static_assert(_Generic(2147483648, long: 1, long long: 2) == 2, "");"#,
        );
    }
}

/// `sizeof` yields `size_t`, which is the narrowest unsigned type as wide as a
/// pointer — `unsigned int` on i686, `unsigned long` on LP64 and `unsigned
/// long long` on 64-bit Windows, exactly as `__SIZE_TYPE__` says and as
/// `gcc -m32 -dM -E` prints.
#[test]
fn the_type_of_size_t() {
    let asked = r#"
        #include <stddef.h>
        _Static_assert(
            _Generic((size_t)0, unsigned int: 1, unsigned long: 2, unsigned long long: 3) == WANT,
            "size_t");
        _Static_assert(_Generic(sizeof(int), unsigned int: 1, unsigned long: 2,
                                unsigned long long: 3) == WANT, "sizeof");
    "#;
    for (triple, want) in [
        ("x86_64-unknown-linux-gnu", 2),
        ("i686-unknown-linux-gnu", 1),
        ("wasm32-unknown-unknown", 1),
        ("x86_64-pc-windows-msvc", 3),
    ] {
        accepts(triple, &asked.replace("WANT", &want.to_string()));
    }
}

/// `intmax_t` does *not* follow the pointer: an ILP32 target still has a
/// 64-bit one, and GCC spells it `long long int` there.
#[test]
fn intmax_is_always_sixty_four_bits() {
    for triple in [
        "x86_64-unknown-linux-gnu",
        "i686-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "wasm32-unknown-unknown",
    ] {
        accepts(
            triple,
            r#"
            #include <stdint.h>
            _Static_assert(sizeof(intmax_t) == 8, "");
            _Static_assert(__SIZEOF_INTMAX__ == 8, "");
            #if __INTMAX_MAX__ != 9223372036854775807
            #error intmax_t is not 64 bits
            #endif
            "#,
        );
    }
}

// ---------------------------------------------------------------------------
// layout
// ---------------------------------------------------------------------------

/// The i386 System V ABI aligns `long long` and `double` to four bytes and the
/// Microsoft one to eight, so two targets of the same *data model* lay the
/// same `struct` out differently. Checked against `gcc -m32`.
#[test]
fn the_alignment_of_the_wide_scalars() {
    accepts(
        "i686-unknown-linux-gnu",
        r#"
        struct S { char c; long long ll; };
        struct D { char c; double d; };
        struct L { char c; long l; };
        _Static_assert(_Alignof(long long) == 4, "");
        _Static_assert(_Alignof(double) == 4, "");
        _Static_assert(_Alignof(struct S) == 4, "");
        _Static_assert(sizeof(struct S) == 12, "");
        _Static_assert(sizeof(struct D) == 12, "");
        _Static_assert(sizeof(struct L) == 8, "");
        "#,
    );
    accepts(
        "i686-pc-windows-msvc",
        r#"
        struct S { char c; long long ll; };
        struct D { char c; double d; };
        _Static_assert(_Alignof(long long) == 8, "");
        _Static_assert(_Alignof(double) == 8, "");
        _Static_assert(_Alignof(struct S) == 8, "");
        _Static_assert(sizeof(struct S) == 16, "");
        _Static_assert(sizeof(struct D) == 16, "");
        "#,
    );
    accepts(
        "x86_64-unknown-linux-gnu",
        r#"
        struct S { char c; long long ll; };
        struct L { char c; long l; };
        _Static_assert(sizeof(struct S) == 16, "");
        _Static_assert(sizeof(struct L) == 16, "");
        "#,
    );
    // Arm keeps eight-byte alignment even at 32 bits (AAPCS), so an ILP32
    // target is not automatically an i386 one.
    accepts(
        "armv7-unknown-linux-gnueabihf",
        r#"
        struct S { char c; long long ll; };
        _Static_assert(_Alignof(long long) == 8, "");
        _Static_assert(sizeof(struct S) == 16, "");
        "#,
    );
}

// ---------------------------------------------------------------------------
// the preprocessor
// ---------------------------------------------------------------------------

#[test]
fn the_data_model_macros() {
    accepts(
        "i686-unknown-linux-gnu",
        r#"
        #if __SIZEOF_LONG__ != 4 || __SIZEOF_POINTER__ != 4 || __SIZEOF_INT__ != 4
        #error not ILP32
        #endif
        #if !defined(__ILP32__) || !defined(_ILP32) || defined(__LP64__)
        #error the data-model macros are wrong
        #endif
        #if defined(__SIZEOF_INT128__)
        #error a 32-bit target has no __int128
        #endif
        #if !defined(__i386__) || !defined(__linux__) || !defined(__ELF__) || defined(_WIN32)
        #error the identity macros are wrong
        #endif
        #if defined(__CHAR_UNSIGNED__)
        #error x86 has a signed plain char
        #endif
        #if __BYTE_ORDER__ != __ORDER_LITTLE_ENDIAN__
        #error little-endian
        #endif
        "#,
    );
    accepts(
        "x86_64-pc-windows-msvc",
        r#"
        #if !defined(_WIN32) || !defined(_WIN64) || !defined(__x86_64__)
        #error the identity macros are wrong
        #endif
        #if defined(__linux__) || defined(__unix__) || defined(__ELF__) || defined(__APPLE__)
        #error a Windows target is none of those
        #endif
        #if defined(__LP64__) || defined(__ILP32__)
        #error LLP64 is neither
        #endif
        #if __SIZEOF_WCHAR_T__ != 2 || __SIZEOF_WINT_T__ != 2
        #error Windows has a 16-bit wchar_t
        #endif
        "#,
    );
    accepts(
        "aarch64-unknown-linux-gnu",
        r#"
        #if !defined(__aarch64__) || !defined(__LP64__) || !defined(_LP64)
        #error the identity macros are wrong
        #endif
        #if !defined(__CHAR_UNSIGNED__)
        #error AArch64 has an unsigned plain char
        #endif
        #if !defined(__WCHAR_UNSIGNED__)
        #error AArch64 has an unsigned wchar_t
        #endif
        #if !defined(__SIZEOF_INT128__)
        #error a 64-bit target has __int128
        #endif
        "#,
    );
    accepts(
        "aarch64-apple-darwin",
        r#"
        #if !defined(__APPLE__) || !defined(__MACH__) || !defined(__unix__)
        #error the identity macros are wrong
        #endif
        #if defined(__CHAR_UNSIGNED__)
        #error Apple makes plain char signed even on Arm
        #endif
        #if defined(__ELF__)
        #error Mach-O
        #endif
        "#,
    );
    accepts(
        "s390x-unknown-linux-gnu",
        r#"
        #if __BYTE_ORDER__ != __ORDER_BIG_ENDIAN__
        #error s390x is big-endian
        #endif
        #if !defined(__s390x__)
        #error the identity macros are wrong
        #endif
        "#,
    );
    accepts(
        "wasm32-unknown-unknown",
        r#"
        #if !defined(__wasm__) || !defined(__wasm32__)
        #error the identity macros are wrong
        #endif
        #if defined(__linux__) || defined(__ELF__) || defined(_WIN32)
        #error a freestanding wasm target is none of those
        #endif
        #if defined(__CHAR_UNSIGNED__)
        #error wasm has a signed plain char, like core::ffi::c_char
        #endif
        "#,
    );
}

/// `<limits.h>`, which is written entirely in terms of those macros.
#[test]
fn limits_h_follows_the_model() {
    accepts(
        "x86_64-unknown-linux-gnu",
        r#"
        #include <limits.h>
        _Static_assert(LONG_MAX == 9223372036854775807L, "");
        _Static_assert(CHAR_MIN < 0, "");
        #if LONG_MAX != 9223372036854775807
        #error LONG_MAX in an #if
        #endif
        "#,
    );
    accepts(
        "i686-unknown-linux-gnu",
        r#"
        #include <limits.h>
        _Static_assert(LONG_MAX == 2147483647L, "");
        _Static_assert(ULONG_MAX == 4294967295UL, "");
        _Static_assert(LLONG_MAX == 9223372036854775807LL, "");
        #if LONG_MAX != 2147483647
        #error LONG_MAX in an #if
        #endif
        "#,
    );
    accepts(
        "aarch64-unknown-linux-gnu",
        r#"
        #include <limits.h>
        _Static_assert(CHAR_MIN == 0 && CHAR_MAX == 255, "unsigned plain char");
        "#,
    );
}

/// The bundled headers branch on `_WIN32` and `__APPLE__`, which are now the
/// *target's*; this is the one visible consequence a program can test.
#[test]
fn the_bundled_headers_follow_the_target() {
    accepts(
        "x86_64-pc-windows-msvc",
        r#"
        #include <time.h>
        #include <stdlib.h>
        _Static_assert(sizeof(time_t) == 8, "Windows time_t is long long");
        _Static_assert(CLOCKS_PER_SEC == 1000, "");
        _Static_assert(RAND_MAX == 32767, "");
        "#,
    );
    accepts(
        "i686-unknown-linux-gnu",
        r#"
        #include <time.h>
        #include <stdlib.h>
        _Static_assert(sizeof(time_t) == 4, "a 32-bit Unix time_t is a 32-bit long");
        _Static_assert(CLOCKS_PER_SEC == 1000000, "");
        _Static_assert(RAND_MAX == 2147483647, "");
        "#,
    );
    accepts(
        "x86_64-pc-windows-gnu",
        r#"
        #include <wchar.h>
        _Static_assert(sizeof(wint_t) == 2, "");
        _Static_assert(sizeof(mbstate_t) == 8, "the Microsoft _Mbstatet");
        "#,
    );
    accepts(
        "x86_64-apple-darwin",
        r#"
        #include <wchar.h>
        _Static_assert(sizeof(mbstate_t) == 136, "Apple's __mbstate_t");
        _Static_assert(WEOF == -1, "Apple's wint_t is an int");
        "#,
    );
}

// ---------------------------------------------------------------------------
// what a target refuses
// ---------------------------------------------------------------------------

#[test]
fn int128_is_refused_on_a_32_bit_target() {
    rejects(
        "i686-unknown-linux-gnu",
        "__int128 wide(void) { return 0; }",
        &[
            "'__int128' is not available on this target (x86, 32-bit pointers); \
             guard on '__SIZEOF_INT128__'",
        ],
    );
    rejects(
        "armv7-unknown-linux-gnueabihf",
        "unsigned __int128 wide(void) { return 0; }",
        &[
            "'unsigned __int128' is not available on this target (arm, 32-bit pointers); \
             guard on '__SIZEOF_INT128__'",
        ],
    );
    accepts(
        "aarch64-unknown-linux-gnu",
        r#"
        __int128 wide(void) { return 0; }
        _Static_assert(sizeof(__int128) == 16, "");
        "#,
    );
    // The x32 ABI has 32-bit pointers on a 64-bit machine, and keeps
    // `__int128`, exactly as GCC does.
    accepts(
        "x86_64-unknown-linux-gnux32",
        r#"
        _Static_assert(sizeof(void *) == 4, "");
        _Static_assert(sizeof(__int128) == 16, "");
        "#,
    );
}

#[test]
fn a_bit_field_is_refused_on_a_big_endian_target() {
    rejects(
        "s390x-unknown-linux-gnu",
        "struct S { unsigned a : 3; };",
        &[
            "bit-field 'a' is not supported on a big-endian target (s390x): cinrs allocates \
             bit-fields from the least significant end, which is not how a big-endian ABI \
             lays them out",
        ],
    );
    rejects(
        "powerpc64-unknown-linux-gnu",
        "struct S { int : 0; };",
        &[
            "anonymous bit-field is not supported on a big-endian target (powerpc64): cinrs \
             allocates bit-fields from the least significant end, which is not how a \
             big-endian ABI lays them out",
        ],
    );
    // The little-endian sibling of the same architecture is fine.
    accepts(
        "powerpc64le-unknown-linux-gnu",
        "struct S { unsigned a : 3; };",
    );
}

// ---------------------------------------------------------------------------
// #pragma cinrs target
// ---------------------------------------------------------------------------

/// The pragma overrides whatever the options said, and does so early enough
/// for the predefined macros and the bundled headers to follow.
#[test]
fn the_pragma_picks_the_model() {
    accepts(
        "x86_64-unknown-linux-gnu",
        r#"
        #pragma cinrs target "i686-unknown-linux-gnu"
        #if __SIZEOF_LONG__ != 4 || !defined(__ILP32__)
        #error the pragma did not reach the predefined macros
        #endif
        #include <limits.h>
        _Static_assert(sizeof(long) == 4, "");
        _Static_assert(LONG_MAX == 2147483647L, "");
        "#,
    );
    accepts(
        "x86_64-unknown-linux-gnu",
        r#"
        #pragma cinrs target "x86_64-pc-windows-msvc"
        #include <stddef.h>
        _Static_assert(sizeof(long) == 4, "");
        _Static_assert(sizeof(void *) == 8, "");
        _Static_assert(sizeof(wchar_t) == 2, "the re-lex reached L'x' too");
        _Static_assert(sizeof(L'x') == 2, "");
        "#,
    );
}

#[test]
fn the_pragma_reports_a_triple_it_does_not_know() {
    rejects(
        "x86_64-unknown-linux-gnu",
        r#"#pragma cinrs target "pdp11-unknown-unix""#,
        &[
            "#pragma cinrs target: unknown architecture 'pdp11' in the target triple \
             'pdp11-unknown-unix'; the architectures cinrs models are x86, x86_64, aarch64, \
             arm/thumb, riscv32, riscv64, wasm32, powerpc, powerpc64, s390x, mips, mips64, \
             sparc, sparc64 and loongarch64, on linux (android included), darwin, windows, \
             freebsd, netbsd, openbsd, wasi or none",
        ],
    );
}

#[test]
fn the_pragma_wants_one_string_literal() {
    rejects(
        "x86_64-unknown-linux-gnu",
        "#pragma cinrs target",
        &["#pragma cinrs target needs a string literal"],
    );
    rejects(
        "x86_64-unknown-linux-gnu",
        "#pragma cinrs target 42",
        &["#pragma cinrs target needs a string literal, found integer constant"],
    );
    rejects(
        "x86_64-unknown-linux-gnu",
        r#"#pragma cinrs target """#,
        &["#pragma cinrs target was given an empty string"],
    );
}

#[test]
fn two_pragmas_must_agree() {
    rejects(
        "x86_64-unknown-linux-gnu",
        "#pragma cinrs target \"i686-unknown-linux-gnu\"\n\
         #pragma cinrs target \"wasm32-unknown-unknown\"",
        &[
            "this unit is already translated for 'i686-unknown-linux-gnu' by an earlier \
           #pragma cinrs target",
        ],
    );
    // Twice with the same triple is harmless, and says so by saying nothing.
    accepts(
        "x86_64-unknown-linux-gnu",
        "#pragma cinrs target \"i686-unknown-linux-gnu\"\n\
         #pragma cinrs target \"i686-unknown-linux-gnu\"\n\
         _Static_assert(sizeof(long) == 4, \"\");",
    );
}

/// The pragma is read before preprocessing, so anything the old model already
/// answered makes it a lie — and it says so rather than half-applying.
#[test]
fn the_pragma_must_come_first() {
    rejects(
        "x86_64-unknown-linux-gnu",
        "#include <limits.h>\n#pragma cinrs target \"i686-unknown-linux-gnu\"",
        &[
            "'#pragma cinrs target' must come before every '#include' and '#if', which were \
           already answered with the previous data model",
        ],
    );
    rejects(
        "x86_64-unknown-linux-gnu",
        "#if 1\n#endif\n#pragma cinrs target \"i686-unknown-linux-gnu\"",
        &[
            "'#pragma cinrs target' must come before every '#include' and '#if', which were \
           already answered with the previous data model",
        ],
    );
    // An `#ifdef` decides nothing about the model, so it does not count.
    accepts(
        "x86_64-unknown-linux-gnu",
        "#ifndef GUARD\n#endif\n\
         #pragma cinrs target \"i686-unknown-linux-gnu\"\n\
         _Static_assert(sizeof(long) == 4, \"\");",
    );
}

/// The scan reads the pragma *lexically*, so one inside a group the
/// preprocessor goes on to skip still chooses the model — and the check that
/// it came first never runs, because the preprocessor never reaches it. That
/// is the one place the two halves cannot agree, and it is documented rather
/// than papered over: a `target` pragma does not belong in a conditional.
#[test]
fn a_pragma_in_a_skipped_group_still_applies() {
    accepts(
        "x86_64-unknown-linux-gnu",
        "#if 0\n\
         #pragma cinrs target \"i686-unknown-linux-gnu\"\n\
         #endif\n\
         _Static_assert(sizeof(long) == 4, \"the skipped pragma still chose the model\");",
    );
}

/// `_Pragma("cinrs target …")` is not a directive the scan can see — it is a
/// token the preprocessor produces — so it is reported rather than obeyed.
#[test]
fn the_pragma_operator_form_is_refused() {
    rejects(
        "x86_64-unknown-linux-gnu",
        "_Pragma(\"cinrs target \\\"i686-unknown-linux-gnu\\\"\")",
        &[
            "'#pragma cinrs target' is read before preprocessing, so it has to be a \
             directive in the unit's own text: a header's comes too late, and one out of \
             '_Pragma' is never seen. This unit is being translated for the model the \
             options given to the front end named",
        ],
    );
}

/// A `target` pragma in a *header* is never seen by the scan that applies it,
/// so it would silently do nothing.
#[test]
fn the_pragma_is_refused_in_a_header() {
    let mut options = options("x86_64-unknown-linux-gnu");
    options
        .include_paths
        .push(std::path::PathBuf::from("tests/include"));
    let found = errors_with("#include <target_pragma.h>\n", &options);
    assert_eq!(
        found,
        [
            "'#pragma cinrs target' is read before preprocessing, so it has to be a \
             directive in the unit's own text: a header's comes too late, and one out of \
             '_Pragma' is never seen. This unit is being translated for the model the \
             options given to the front end named"
        ],
        "{found:#?}"
    );
}

// ---------------------------------------------------------------------------
// the data-model assertion
// ---------------------------------------------------------------------------

/// The assertion states the model *and* how it was chosen, so that a wrong
/// `CINRS_TARGET` fails with a message that says which knob to turn.
#[test]
fn the_assertion_names_the_model_and_its_source() {
    let source = "int f(void) { return 0; }";
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let expansion = cinrs_core::expand(input, &options("i686-unknown-linux-gnu")).to_string();
    assert!(
        expansion.contains("ILP32 (x86-linux, signed 'char', 32-bit 'wchar_t')"),
        "{expansion}"
    );
    assert!(
        expansion.contains("chosen from the options given to the front end"),
        "{expansion}"
    );
    assert!(
        expansion.contains("cargo:rustc-env=CINRS_TARGET=$TARGET"),
        "{expansion}"
    );

    // A unit whose model came from the pragma says so.
    let source = "#pragma cinrs target \"x86_64-pc-windows-gnu\"\nint f(void) { return 0; }";
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let expansion = cinrs_core::expand(input, &Options::new(Standard::C11)).to_string();
    assert!(
        expansion.contains("LLP64 (x86_64-windows, signed 'char', 16-bit 'wchar_t')"),
        "{expansion}"
    );
    assert!(
        expansion.contains("chosen from #pragma cinrs target \\\"x86_64-pc-windows-gnu\\\""),
        "{expansion}"
    );
    // And the widths it asserts are that target's, not the host's.
    let squeezed: String = expansion.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        squeezed.contains("size_of::<::core::ffi::c_long>()==4"),
        "{squeezed}"
    );
    assert!(
        squeezed.contains("size_of::<*const::core::ffi::c_void>()==8"),
        "{squeezed}"
    );
}
