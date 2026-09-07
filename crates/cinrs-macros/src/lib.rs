//! The procedural macro front end for [`cinrs`](https://docs.rs/cinrs).
//!
//! Deliberately a thin shim: everything interesting lives in `cinrs-core`,
//! which is built on `proc-macro2` and can therefore be tested without a
//! procedural macro context.
//!
//! # The `nightly` feature
//!
//! `cinrs-core` never touches `proc_macro`, so the one thing it cannot do by
//! itself is build a span that points *inside* a string literal —
//! `proc_macro::Literal::subspan` is unstable on every channel, and
//! `proc-macro2` does not expose it. With the `nightly` feature this crate
//! turns on `#![feature(proc_macro_span)]` and hands the front end a
//! [`Subspan`] hook that does exactly that, so a `c99! { r#"…"# }` body reports
//! its errors on the offending C token rather than on the whole literal with
//! the position spelled out in the message. Everything else is unchanged, and
//! without the feature — or wherever `subspan` declines to answer, as under
//! `rust-analyzer` — the stable behaviour comes back on its own.

#![cfg_attr(feature = "nightly", feature(proc_macro_span))]

use proc_macro::TokenStream;

use cinrs_core::{Options, Standard, Subspan};

/// Compiles a C89 (C90) translation unit written inside Rust.
///
/// The oldest entry point, and the one written for code that predates the 1999
/// standard: **implicit `int`** (`static x;`, `f() { … }`), **implicit
/// function declarations** (calling `abs` with nothing declaring it declares
/// `extern int abs();` from that point on) and **old-style (K&R) function
/// definitions** all work here, and everything C99 added is a diagnostic that
/// says so:
///
/// ```text
/// error: '//' comments require C99 or later (this block is c89!)
/// ```
///
/// What is gated: `//` comments, mixed declarations and code, a declaration in
/// a `for` clause, variable length arrays, `_Bool`, `restrict`, `inline`,
/// `long long`, designated initializers, compound literals, variadic macros,
/// flexible array members, hexadecimal floating constants, `__func__`,
/// `_Pragma`, universal character names, a trailing comma in an enumerator
/// list, `static` and `[*]` in an array parameter declarator, `_Complex` and
/// an imaginary constant (`2.0i`).
/// The *library* additions are not: a bundled header is a set of declarations,
/// and `snprintf` is one of them — with `<complex.h>` the one exception, since
/// every declaration in it names a type C89 does not have.
///
/// `__STDC_VERSION__` is **not defined** — C89 as published had no such macro
/// — while `__STDC__` is `1`, exactly as in `gcc -std=c89`.
/// [`c90!`](macro@c90) is another name for this macro, and
/// [`gnu89!`](macro@gnu89) is the GNU dialect of it.
#[proc_macro]
pub fn c89(input: TokenStream) -> TokenStream {
    expand(input, Standard::C89)
}

/// Compiles a C90 translation unit written inside Rust.
///
/// ISO/IEC 9899:1990 is ANSI X3.159-1989 republished with no technical change,
/// so this is [`c89!`](macro@c89) under the other name the language has.
#[proc_macro]
pub fn c90(input: TokenStream) -> TokenStream {
    expand(input, Standard::C89)
}

/// Compiles a C99 translation unit written inside Rust.
///
/// The body may be written as raw Rust tokens:
///
/// ```ignore
/// cinrs::c99! {
///     int add(int a, int b) { return a + b; }
/// }
/// ```
///
/// or, for C that the Rust lexer refuses (hex float literals, `L"…"`,
/// multi-character character constants, `\` line continuations, `##`), as a
/// single string literal:
///
/// ```ignore
/// cinrs::c99! { r#"
///     int add(int a, int b) { return a + b; }
/// "# }
/// ```
///
/// The expansion is a private module of its own plus a glob re-export of it,
/// so that two blocks in one Rust module may share a header without their
/// types colliding. Each function becomes a `pub unsafe extern "C" fn` of the
/// same name, taking and returning the [`core::ffi`] types, so the C above is
/// called as `unsafe { add(1, 2) }`; a C `static` function stays private to
/// the module, and a file-scope variable becomes a `static mut` item. A
/// `struct`, `union` or `enum` becomes a `#[repr(C)]` item Rust code can use
/// directly, and a function the unit only declares becomes an `extern "C"`
/// declaration linked against the real symbol.
///
/// The C99 preprocessor runs first — every directive, macros with `#`, `##`
/// and `__VA_ARGS__` included. `#include` finds the headers `cinrs` bundles
/// (`<stdio.h>`, `<string.h>`, `<math.h>` and the rest, written in plain C99
/// rather than read from the platform) and the user's own, in the directory of
/// the invoking `.rs` file and in whatever `#pragma cinrs include_path` adds.
/// Since Rust's lexer refuses `##` in raw-token form, a replacement list may
/// write the pasting operator as `a # # b` instead; see the
/// [crate documentation] for that, for the headers and for the predefined
/// macros.
///
/// [crate documentation]: https://docs.rs/cinrs
///
/// Errors are reported at the exact C token that caused them. In string
/// literal form, stable Rust cannot build a span pointing inside a literal, so
/// the position inside the C source is appended to the message instead — see
/// [the `nightly` feature](self#the-nightly-feature) for the way around that.
///
/// [`c11!`](macro@c11), [`c17!`](macro@c17) and [`c23!`](macro@c23) are the
/// same macro for a later revision of the language.
#[proc_macro]
pub fn c99(input: TokenStream) -> TokenStream {
    expand(input, Standard::C99)
}

/// Compiles a C11 translation unit written inside Rust.
///
/// Everything [`c99!`](macro@c99) does, plus what C11 added:
/// `_Static_assert`, `_Alignof`, `_Alignas` (on the members of a `struct` or
/// `union`), `_Generic`, `_Noreturn`, `_Thread_local`, `_Atomic` with
/// `<stdatomic.h>`, anonymous `struct`/`union` members, and
/// the Unicode literals `u8"…"`, `u"…"`, `U"…"`, `u'x'` and `U'x'` with
/// `<uchar.h>`'s `char16_t` and `char32_t`, and the `CMPLX` family in
/// `<complex.h>`. `__STDC_VERSION__` is `201112L`.
///
/// C11's threads are here too, as the platform's own: `<threads.h>` is
/// bundled for the C libraries whose objects it can lay out — glibc and musl,
/// both on Linux — and is an `#error` naming the reason elsewhere, which is
/// where `__STDC_NO_THREADS__` is predefined. An `_Atomic` `struct` is a clear
/// error rather than a silent mistranslation — it would need a lock, and there
/// is nothing in the generated Rust to be one.
#[proc_macro]
pub fn c11(input: TokenStream) -> TokenStream {
    expand(input, Standard::C11)
}

/// Compiles a C17 translation unit written inside Rust.
///
/// C17 is C11 with the defect reports applied and no new features, so this is
/// [`c11!`](macro@c11) with `__STDC_VERSION__` set to `201710L`.
#[proc_macro]
pub fn c17(input: TokenStream) -> TokenStream {
    expand(input, Standard::C17)
}

/// Compiles a C23 translation unit written inside Rust.
///
/// Everything [`c11!`](macro@c11) does, plus the C23 keywords (`bool`, `true`,
/// `false`, `nullptr`, `static_assert`, `alignof`, `alignas`, `constexpr`,
/// `typeof`), `[[…]]` attributes, `__VA_OPT__`, `#elifdef` / `#elifndef`,
/// binary constants, digit separators, empty initialisers, `auto` type
/// inference, enumerations with a fixed underlying type, improved tag
/// compatibility (a tag defined twice in one scope with the same members is
/// one type), `unreachable()`,
/// `#embed`, `char8_t` and the `u8'x'` character prefix.
/// `__STDC_VERSION__` is `202311L`.
///
/// It is also the one entry point that has **no trigraphs**, which is what
/// C23 removed: `??=` here is two question marks and an `=`.
///
/// A digit separator (`1'000'000`) needs string-literal form: Rust's own lexer
/// reads `1'000` as a literal followed by a lifetime and refuses it.
#[proc_macro]
pub fn c23(input: TokenStream) -> TokenStream {
    expand(input, Standard::C23)
}

/// Compiles a C99 translation unit with the GNU extensions switched on.
///
/// Everything [`c99!`](macro@c99) does, plus what GCC's `-std=gnu99` adds over
/// its `-std=c99`: the plain spellings `typeof` and `asm` are keywords, and a
/// construct a later revision introduced — `_Static_assert`, `_Generic`, a
/// `0b` literal — is accepted rather than being told which macro to write.
///
/// The extensions spelled with a leading double underscore — `__typeof__`,
/// `__attribute__`, `__extension__`, `__builtin_*`, `__restrict` — are
/// available in *every* entry point, exactly as they are in GCC's strict
/// modes: the names are reserved, so nothing a program may legally call its
/// own is taken away. `__STRICT_ANSI__` is defined only in the strict entry
/// points; `__GNUC__` is 4 in all of them. A GNU dialect also switches
/// **trigraphs off**, as `gcc -std=gnu99` does, so `"what??!"` there is an
/// exclamation rather than a pipe.
///
/// See `doc/gnu-extensions.md` in the repository for the whole catalogue.
#[proc_macro]
pub fn gnu99(input: TokenStream) -> TokenStream {
    expand_gnu(input, Standard::C99)
}

/// Compiles a C89 translation unit with the GNU extensions switched on.
///
/// What `gcc -std=gnu89` is: **everything a later revision added is accepted**
/// — `//` comments, mixed declarations and code, `long long`, designated
/// initializers and the rest, which C89 as an ISO document does not have and
/// GCC has always taken as extensions — *and* the three C89 rules that are not
/// a matter of extension at all, because a later revision deleted them:
/// implicit `int`, implicit function declarations, and old-style (K&R)
/// definitions (which every entry point below `c23!` has).
///
/// So `gnu89!` is [`gnu99!`](macro@gnu99) plus those three, with
/// `__STDC_VERSION__` left undefined; it is the entry point for the C of the
/// 1990s, and the one the GCC torture suite is measured with.
#[proc_macro]
pub fn gnu89(input: TokenStream) -> TokenStream {
    expand_gnu(input, Standard::C89)
}

/// Compiles a C11 translation unit with the GNU extensions switched on.
///
/// [`c11!`](macro@c11) plus what [`gnu99!`](macro@gnu99) adds.
#[proc_macro]
pub fn gnu11(input: TokenStream) -> TokenStream {
    expand_gnu(input, Standard::C11)
}

/// Compiles a C17 translation unit with the GNU extensions switched on.
///
/// [`c17!`](macro@c17) plus what [`gnu99!`](macro@gnu99) adds.
#[proc_macro]
pub fn gnu17(input: TokenStream) -> TokenStream {
    expand_gnu(input, Standard::C17)
}

/// Compiles a C23 translation unit with the GNU extensions switched on.
///
/// [`c23!`](macro@c23) plus what [`gnu99!`](macro@gnu99) adds.
#[proc_macro]
pub fn gnu23(input: TokenStream) -> TokenStream {
    expand_gnu(input, Standard::C23)
}

/// The body every strict entry point shares.
fn expand(input: TokenStream, standard: Standard) -> TokenStream {
    run(input, Options::new(standard))
}

/// The body every GNU entry point shares.
fn expand_gnu(input: TokenStream, standard: Standard) -> TokenStream {
    run(input, Options::gnu(standard))
}

fn run(input: TokenStream, options: Options) -> TokenStream {
    let subspan = subspan(&input);
    cinrs_core::expand_with(input.into(), &options, subspan).into()
}

/// A hook resolving a byte range of a string-literal body into a span.
///
/// There is one only when the whole input is a single literal, which is what
/// string-literal mode is; everything else already has a span per token.
#[cfg(feature = "nightly")]
fn subspan(input: &TokenStream) -> Option<Subspan> {
    let mut trees = input.clone().into_iter();
    let (Some(proc_macro::TokenTree::Literal(literal)), None) = (trees.next(), trees.next()) else {
        return None;
    };
    Some(Subspan::new(move |range| {
        literal.subspan(range).map(Into::into)
    }))
}

/// Without the `nightly` feature there is nothing to hook up, and
/// string-literal input keeps reporting positions in the message text.
#[cfg(not(feature = "nightly"))]
fn subspan(_input: &TokenStream) -> Option<Subspan> {
    None
}
