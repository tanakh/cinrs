//! Write C code inside Rust.
//!
//! `cinrs` provides procedural macros that accept a C translation unit and
//! translate it to Rust. Functions defined inside the macro can be called
//! directly from Rust; arguments and return values use the C types from
//! [`core::ffi`].
//!
//! ```
//! cinrs::c99! {
//!     int fact(int n) {
//!         if (n == 0) {
//!             return 1;
//!         } else {
//!             return n * fact(n - 1);
//!         }
//!     }
//! }
//!
//! // The generated functions are `extern "C"`, so calling one is `unsafe`.
//! assert_eq!(unsafe { fact(10) }, 3_628_800);
//! ```
//!
//! Each macro invocation is one translation unit.
//!
//! # Standards
//!
//! [`c99!`], [`c11!`], [`c17!`] and [`c23!`] are the same macro for four
//! revisions of the language; `__STDC_VERSION__` is `199901L`, `201112L`,
//! `201710L` and `202311L` respectively, and the bundled headers follow it.
//! A construct a later revision introduced is a diagnostic in an earlier
//! block, and the diagnostic says which macro to write instead:
//!
//! ```text
//! error: '_Static_assert' requires C11 or later (this block is c99!)
//! ```
//!
//! ## What `c11!` adds
//!
//! `_Static_assert` at file scope, at block scope and among the members of a
//! `struct`; `_Generic`; `_Alignof`; `_Alignas` on the members of a `struct`
//! or `union`; `_Noreturn`; and anonymous `struct`/`union` members, whose own
//! members are reached through the enclosing record.
//!
//! ```
//! cinrs::c11! {
//!     _Static_assert(sizeof(int) == 4, "int is 32 bits here");
//!
//!     struct Value {
//!         int tag;
//!         union { int as_int; double as_double; };
//!     };
//!
//!     int tag_of(struct Value v) { return v.tag; }
//!     int as_int(struct Value v) { return v.as_int; }
//!
//!     int kind(int n) { return _Generic(n, int: 1, double: 2, default: 0); }
//! }
//!
//! let v = Value { tag: 1, __cinrs_anon0: unsafe { core::mem::zeroed() } };
//! assert_eq!(unsafe { tag_of(v) }, 1);
//! assert_eq!(unsafe { as_int(v) }, 0);
//! assert_eq!(unsafe { kind(0) }, 1);
//! ```
//!
//! An anonymous member becomes a field named `__cinrs_anon0`,
//! `__cinrs_anon1`, … of a generated type of its own, so Rust code that
//! builds such a `struct` names it that way; the C code never does.
//!
//! `c17!` is `c11!` with a different `__STDC_VERSION__`: C17 added no
//! features.
//!
//! ## What `c23!` adds
//!
//! The keywords C23 promoted — `bool`, `true`, `false`, `nullptr`,
//! `static_assert`, `alignof`, `alignas`, `thread_local`, `constexpr`,
//! `typeof` and `typeof_unqual` — plus `[[…]]` attributes, `__VA_OPT__`,
//! `#elifdef`/`#elifndef`, binary constants, digit separators, the empty
//! initialiser `{}`, `auto` type inference, enumerations with a fixed
//! underlying type, labels before a declaration and at the end of a block,
//! and `unreachable()`.
//!
//! ```
//! cinrs::c23! {
//!     constexpr int LIMIT = 0b100;
//!
//!     static_assert(LIMIT == 4);
//!
//!     enum Level : unsigned char { LOW, HIGH };
//!
//!     [[nodiscard]] bool over([[maybe_unused]] enum Level level, int n) {
//!         auto limit = LIMIT;
//!         typeof(n) doubled = n * 2;
//!         return doubled > limit;
//!     }
//!
//!     void *nothing(void) { return nullptr; }
//! }
//!
//! assert!(unsafe { over(HIGH, 3) });
//! assert!(unsafe { nothing().is_null() });
//! ```
//!
//! Those keywords are *ordinary identifiers* before C23 — the bundled
//! `<stdbool.h>` writes `#define bool _Bool`, and a C99 program may have a
//! variable called `typeof` — so a `c11!` block that uses one is told what it
//! would have meant rather than being quietly accepted.
//!
//! A digit separator needs [string-literal form](#input-forms): Rust's own
//! lexer reads `1'000` as a literal followed by a lifetime and refuses it.
//!
//! ## What the later revisions add and this crate does not do
//!
//! `_Thread_local`/`thread_local` (Rust's `#[thread_local]` is unstable),
//! `_Atomic`, `_BitInt`, `#embed`, `__has_include`, and C11's `u8"…"`, `u"…"`
//! and `U"…"` literals with their `char16_t`/`char32_t`: each is a located
//! error rather than a silent mistranslation. Three things are simplifications rather than
//! omissions:
//!
//! * `_Alignas` is honoured on a member by raising the alignment of the
//!   *record* — `#[repr(C, align(N))]` on the generated item — which only
//!   works while the member's natural offset already satisfies it. A member
//!   that would have to move (`struct { char c; _Alignas(16) int x; }`) is
//!   refused rather than laid out differently from the Rust type Rust code
//!   sees. On an object `_Alignas` is not supported at all.
//! * A `constexpr` object is a constant: its value is folded wherever the
//!   name is used, so it may be an array bound or a `case` label, and there
//!   is nothing to take the address of. Only the arithmetic types are
//!   accepted.
//! * `nullptr` has type `void *` rather than a `nullptr_t` of its own, and
//!   `<stddef.h>`'s `nullptr_t` is a `typedef` for `void *`. The difference
//!   only shows where C distinguishes them, such as in `_Generic`.
//!
//! `unreachable()` becomes [`core::hint::unreachable_unchecked`], which is
//! exactly the promise C attaches to it: reaching it is undefined behaviour.
//! It is the one place the expansion trusts the C program with undefined
//! behaviour, because the program asked for it by name.
//!
//! # What is generated
//!
//! A C function becomes a `pub unsafe extern "C" fn` with the same name (a
//! name that is a Rust keyword becomes a raw identifier, so `int match(int)`
//! is called as `r#match`), taking and returning the [`core::ffi`] types —
//! `int` is `c_int`, `double` is `c_double`, `_Bool` is `bool`, and so on. A C
//! `static` function is generated without `pub`, and a file-scope variable
//! becomes a `static mut` item, which Rust must read by value:
//! `assert_eq!({ counter }, 1)` rather than `assert_eq!(counter, 1)`, since
//! taking a reference to a `static mut` is an error in edition 2024.
//!
//! Arithmetic follows C, not Rust: `+`, `-`, `*` and the shifts wrap instead
//! of panicking (unsigned wrap-around is defined in C, and wrapping is the
//! predictable choice for the signed overflow C leaves undefined), while `/`
//! and `%` are Rust's, which truncate towards zero and take the sign of the
//! dividend exactly as C99 says.
//!
//! Pointers are raw pointers — `T *` is `*mut T`, `const T *` is `*const T`,
//! `void *` is `*mut c_void` — and pointer arithmetic goes through `offset`,
//! so nothing in the generated code holds a reference. Arrays are `[T; N]`,
//! `struct` and `union` become `#[repr(C)]` items with `pub` members and a
//! `Copy` derive, an `enum` becomes a `c_int` alias plus one `const` per
//! enumerator, and a function pointer is
//! `Option<unsafe extern "C" fn(…) -> R>` so that a null one is
//! representable. All of those are ordinary Rust items, so Rust code can build
//! a `struct` the C code takes, read the members it sets, and pass one of its
//! own functions where C wants a callback:
//!
//! ```
//! cinrs::c99! {
//!     struct Point { int x; int y; };
//!
//!     int manhattan(struct Point p) {
//!         return (p.x < 0 ? -p.x : p.x) + (p.y < 0 ? -p.y : p.y);
//!     }
//! }
//!
//! assert_eq!(unsafe { manhattan(Point { x: 3, y: -4 }) }, 7);
//! ```
//!
//! A function the unit only declares — `int printf(const char *, ...);` — is
//! linked rather than defined: it becomes an `extern "C"` declaration, renamed
//! apart from anything else of that name, and pointed back at its symbol with
//! `#[link_name]`.
//!
//! # One block, one module
//!
//! A translation unit is a namespace, so each expansion goes into a private
//! Rust module of its own, followed by a glob re-export of it:
//!
//! ```text
//! mod __cinrs_unit_1a2b3c4d { … }
//! #[allow(ambiguous_glob_reexports, unused_imports)]
//! pub use __cinrs_unit_1a2b3c4d::*;
//! ```
//!
//! Everything with external linkage is `pub` inside the module and comes back
//! out through the glob, so Rust calls a C function by the name its author
//! gave it; a C `static` function or object stays private to the module, which
//! is exactly the linkage C gives it. Two blocks in one Rust module are two
//! modules and cannot collide, so both may `#include "point.h"` and both
//! generate the `struct Point` their own code needs.
//!
//! What that costs is that a name two blocks both export is ambiguous when
//! *Rust* uses it (`E0659`) — the C code is unaffected, since each unit sees
//! only its own. Naming the module says which one is meant:
//!
//! ```
//! cinrs::c99! {
//!     #pragma cinrs module "geometry"
//!     struct Point { int x; int y; };
//!     int point_x(struct Point p) { return p.x; }
//! }
//!
//! assert_eq!(unsafe { point_x(geometry::Point { x: 4, y: 9 }) }, 4);
//! ```
//!
//! The two units' `struct Point`s are two Rust types even when they come from
//! the same header, so a value passes to the unit whose module it was built
//! from.
//!
//! # Linking two blocks together
//!
//! A function defined in one block is a Rust item, not a C symbol, so another
//! block's `extern` declaration has nothing to link against. `#pragma cinrs
//! export` changes that for a whole unit: every function and object with
//! external linkage in it is given `#[unsafe(no_mangle)]`, and is therefore a
//! real C symbol that another `c99!` block — or a C library, or anything else
//! in the program — resolves by name.
//!
//! ```
//! mod library {
//!     cinrs::c99! {
//!         #pragma cinrs export
//!         int cinrs_doc_triple(int n) { return n * 3; }
//!     }
//! }
//!
//! mod user {
//!     cinrs::c99! {
//!         int cinrs_doc_triple(int n);
//!         int nine(void) { return cinrs_doc_triple(3); }
//!     }
//! }
//!
//! assert_eq!(unsafe { user::nine() }, 9);
//! ```
//!
//! The risk is C's own: two exported units defining the same name is a
//! duplicate symbol, and the linker says so rather than the compiler. Only
//! export the units that something else has to link against.
//!
//! # Control flow
//!
//! `if`, `while`, `do`/`while`, `for`, `break`, `continue` and `switch` — with
//! fallthrough — become Rust's own control flow, so the expansion reads like
//! the C it came from. A function that jumps cannot: one containing a `goto`,
//! or a `case` label that is not a direct child of its `switch` body (Duff's
//! device), is lowered into a state machine over basic blocks instead, with
//! every local of the function hoisted to the top and renamed apart. Both
//! forms compute exactly what the C did; only the second is unpleasant to
//! read, and only the functions that need it get it.
//!
//! # The preprocessor
//!
//! A full C99 preprocessor runs before the parser: object-like and
//! function-like macros with `#`, `##`, `...`/`__VA_ARGS__` and the standard's
//! rescanning rules; `#define`, `#undef`, `#if`, `#ifdef`, `#ifndef`, `#elif`,
//! `#else`, `#endif`, `#include`, `#error`, `#warning`, `#pragma` and `#line`.
//!
//! ```
//! cinrs::c99! {
//!     #define WIDTH 8
//!     #define MAX(a, b) ((a) > (b) ? (a) : (b))
//!
//!     #if WIDTH >= 8
//!     int capacity(void) { return MAX(WIDTH, 4); }
//!     #else
//!     int capacity(void) { return 4; }
//!     #endif
//! }
//!
//! assert_eq!(unsafe { capacity() }, 8);
//! ```
//!
//! ## Writing `##` in raw-token mode
//!
//! Rust's own lexer refuses `##` ("reserved multi-hash token"), so a `c99!`
//! block written as raw Rust tokens cannot spell the pasting operator. It can
//! spell `a # # b`, and since a `#` followed by another `#` is ill-formed C
//! anyway — `#` has to be followed by a macro parameter — the two are read as
//! the `##` operator. The rule holds in every input mode, so a macro written
//! that way means the same thing inside a string literal:
//!
//! ```
//! cinrs::c99! {
//!     #define DEFINE_ADDER(suffix, amount) int add_ # # suffix(int n) { return n + amount; }
//!     DEFINE_ADDER(ten, 10)
//! }
//!
//! assert_eq!(unsafe { add_ten(32) }, 42);
//! ```
//!
//! Two other things the Rust lexer refuses in raw-token mode are worth knowing
//! about here: a `\` at the end of a line (so a long replacement list has to
//! stay on one line) and the lexemes listed under [Input forms](#input-forms).
//! String-literal mode has none of those limits.
//!
//! `__VA_OPT__`, `#elifdef` and `#elifndef` are C23's, and are available in a
//! [`c23!`] block.
//!
//! ## Predefined macros
//!
//! `__STDC__`, `__STDC_HOSTED__` and `__STDC_VERSION__` (which follows the
//! entry point: `199901L`, `201112L`, `201710L` or `202311L`),
//! `__cinrs__`, `__FILE__` and `__LINE__` — which name the *`.rs` file* and
//! the line in it, so that they point where the user is looking — plus
//! `__DATE__` and `__TIME__` as the fixed placeholders `"??? ?? ????"` and
//! `"??:??:??"`, because a build has to give the same output twice. On top of
//! those comes a short set of target description macros (`__x86_64__`,
//! `__linux__`, `__unix__`, `__LP64__`, `__SIZEOF_INT__`, `__BYTE_ORDER__`, …)
//! taken from the machine the code is being compiled for. Nothing claims to be
//! another compiler: there is no `__GNUC__`.
//!
//! # Headers
//!
//! ```
//! cinrs::c99! {
//!     #include <stdio.h>
//!     #include <string.h>
//!
//!     int describe(char *buf, unsigned long size, const char *name) {
//!         return snprintf(buf, size, "%s has %d letters", name, (int)strlen(name));
//!     }
//! }
//!
//! let mut buf = [0u8; 32];
//! let n = unsafe { describe(buf.as_mut_ptr().cast(), 32, c"cinrs".as_ptr()) };
//! assert_eq!(&buf[..n as usize], b"cinrs has 5 letters");
//! ```
//!
//! ## The bundled standard headers
//!
//! `cinrs` ships its own `<assert.h>`, `<ctype.h>`, `<errno.h>`, `<float.h>`,
//! `<inttypes.h>`, `<limits.h>`, `<math.h>`, `<stdalign.h>`, `<stdarg.h>`,
//! `<stdbool.h>`, `<stddef.h>`, `<stdint.h>`, `<stdio.h>`, `<stdlib.h>`,
//! `<stdnoreturn.h>`, `<string.h>` and `<time.h>`, and never reads the
//! platform's. A real `<stdio.h>` is not C — glibc's is built out of GNU
//! extensions, compiler builtins and `__asm__` renaming — so a front end that
//! read it would have to be GCC. The bundled ones declare what the platform's
//! C library really exports, in plain C99, and the calls link against the real
//! implementation. `<setjmp.h>` is there too, as a header that says
//! `setjmp`/`longjmp` are not supported.
//!
//! Three of them depend on which entry point read them, exactly as the
//! standard says they should: `<stdbool.h>` defines `bool`, `true` and `false`
//! only before C23, where they became keywords; `<assert.h>` defines
//! `static_assert` between C11 and C23, for the same reason; and `<stdalign.h>`
//! defines `alignas` and `alignof` before C23. `<stddef.h>` gains `nullptr_t`
//! and `unreachable()` in C23. The functions in `<stdlib.h>` that do not come
//! back — `exit`, `_Exit`, `abort` and `quick_exit` — are marked as such
//! whichever entry point reads the header, so a function may end with
//! `exit(1);` and write no `return`.
//!
//! Anything else — POSIX, a third-party library, your own project's headers —
//! is written out as C declarations by hand, or pointed at with an include
//! path if the header itself is plain enough C to parse.
//!
//! ## Your own headers
//!
//! `#include "…"` looks first in the directory of the file the directive is
//! written in: for a `c99!` block that is the directory of the `.rs` file, and
//! for a header it is the directory that header was found in. Then come the
//! configured directories, and last the bundled headers; `#include <…>` skips
//! the first step. Directories are configured with a pragma —
//!
//! ```c
//! #pragma cinrs include_path "vendor/include"
//! ```
//!
//! — whose relative paths resolve against `CARGO_MANIFEST_DIR`, or with the
//! `CINRS_INCLUDE_PATH` environment variable, which is split the way the
//! platform splits `PATH` and searched last. `#pragma cinrs link "name"` puts
//! `#[link(name = "name")]` on the generated `extern` block, for a program
//! that calls into a library the Rust runtime does not already link. The other
//! two `cinrs` pragmas are
//! [`export`](#linking-two-blocks-together) and
//! [`module`](#one-block-one-module).
//!
//! Every user header read is named by a `const _: &str = include_str!(…);` in
//! the expansion, so editing one rebuilds the crate that includes it. Include
//! guards and `#pragma once` both work, and a header pulled in twice is read
//! once.
//!
//! Each `c99!` block is a whole translation unit, so a header included by two
//! of them generates its types twice — once in each block's own module, which
//! is why that is not a redefinition; see [One block, one
//! module](#one-block-one-module) for what it means for Rust code that uses
//! those types. What headers share between units is types, macros and
//! prototypes; sharing an *implementation* is
//! [`#pragma cinrs export`](#linking-two-blocks-together).
//!
//! # Variadic functions
//!
//! Declaring and calling one — `printf` and friends — works on any supported
//! toolchain. *Defining* one needs Rust's `c_variadic`, stable since 1.99:
//!
//! ```
//! # #[rustversion::since(1.99)]
//! # fn main() {
//! cinrs::c99! {
//!     #include <stdarg.h>
//!
//!     int sum(int n, ...) {
//!         va_list ap;
//!         int total = 0;
//!         va_start(ap, n);
//!         for (int i = 0; i < n; i++) total += va_arg(ap, int);
//!         va_end(ap);
//!         return total;
//!     }
//! }
//!
//! assert_eq!(unsafe { sum(3, 1, 2, 3) }, 6);
//! # }
//! # #[rustversion::before(1.99)]
//! # fn main() {}
//! ```
//!
//! `va_list` is [`core::ffi::VaList`], so a list can be passed straight to
//! `vprintf` and friends. The names come from the bundled `<stdarg.h>`, which
//! defines them in terms of the `__builtin_va_*` forms the compiler owns —
//! exactly as GCC's own header does, so a unit that does not include it may
//! use `va_list` and `va_end` as names of its own. On a toolchain older than
//! 1.99 a variadic *definition* is a clear error rather than an expansion the
//! compiler would reject.
//!
//! # Error reporting
//!
//! The crate goes out of its way to report every error — its own and
//! `rustc`'s — at the exact position inside the macro where the offending C
//! token was written, so that both `cargo` and an IDE point at the C code
//! rather than at the macro call.
//!
//! # Input forms
//!
//! C that the Rust lexer accepts can be written as raw tokens. C that it does
//! not accept (hexadecimal floating constants such as `0x1.8p3`,
//! multi-character character constants, prefixed literals like `L"…"`,
//! backslash line continuations, `##` — for which see
//! [the `# #` rule](#writing--in-raw-token-mode)) can be passed as a single
//! string literal — ideally a raw one:
//!
//! ```ignore
//! cinrs::c99! { r#"
//!     double x = 0x1.8p3;
//! "# }
//! ```
//!
//! ## The `nightly` feature
//!
//! Pointing a span *inside* a string literal needs
//! `proc_macro::Literal::subspan`, which is unstable on every channel
//! (`proc_macro_span`). Without it a diagnostic in string-literal form is
//! reported at the literal as a whole, with the position written into the
//! message:
//!
//! ```text
//! error: invalid digit '8' in octal constant '08' (at line 3, column 15 of the C source)
//!  --> src/main.rs:5:15
//!   |
//! 5 |   cinrs::c99! { r#"
//!   |  _______________^
//! ```
//!
//! Turning on the crate's `nightly` feature — which needs a nightly compiler,
//! since it enables `#![feature(proc_macro_span)]` inside the proc-macro crate
//! — makes the caret land on the C token instead, and drops the position from
//! the message:
//!
//! ```text
//! error: invalid digit '8' in octal constant '08'
//!  --> src/main.rs:7:15
//!   |
//! 7 |     int bad = 08;
//!   |               ^^
//! ```
//!
//! It applies to `rustc`'s own errors about the generated code as well, and it
//! changes nothing else: raw-token input is already exact, and a context where
//! `subspan` declines to answer (`rust-analyzer`, or a literal produced by
//! another macro) falls back to the message on its own.
//!
//! # Status
//!
//! The front end (lexer, preprocessor, parser, diagnostics) is complete for
//! C99, and most of the language is translated end to end: all the arithmetic
//! types, pointers, arrays, `struct`, `union`, `enum`, `typedef`, string
//! literals, function pointers, `sizeof` with real layout, casts, aggregate
//! and designated initialisers, file-scope, `static` and `extern` objects,
//! functions (including `static`, `inline` and variadic ones), every operator,
//! every control structure — `if`, `while`, `do`/`while`, `for`, `switch` with
//! fallthrough, `break`, `continue`, `return` and `goto` — and the whole
//! preprocessor, `#include` and the bundled standard headers included. What
//! C11 and C23 added on top of that is listed under [Standards](#standards),
//! entry point by entry point.
//!
//! Deliberately never: bit-fields, variable length arrays, `_Complex`,
//! old-style (K&R) definitions, `setjmp`/`longjmp`, `_Thread_local`,
//! `_Atomic`, `_BitInt`, `#embed`, `__has_include`, C11's `u8`/`u`/`U`
//! literals, and
//! `long double`'s extended precision (it is `double`, with the ABI that
//! implies). Each of them is a clear, located error rather than a silent
//! mistranslation.
//!
//! The other things worth knowing before reaching for this crate: the
//! platform's include directories are never searched, so anything outside the
//! bundled headers is declared by hand or pointed at with an include path;
//! `va_list` is [`core::ffi::VaList`], which cannot be stored in a `struct` or
//! returned; sizes and alignments come from a model of the target rather than
//! from its own C compiler, and the host is taken to be LP64 where Cargo's
//! environment does not say otherwise.

#![warn(missing_docs)]
#![no_std]

pub use cinrs_macros::{c11, c17, c23, c99};
