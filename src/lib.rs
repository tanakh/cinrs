//! Write C code inside Rust.
//!
//! `cinrs` provides procedural macros that accept a C translation unit and
//! translate it to Rust. Functions defined inside the macro can be called
//! directly from Rust; arguments and return values use the C types from
//! [`core::ffi`].
//!
//! ```
//! cinrs::c99! {
//!     __attribute__((cinrs_safe)) int fact(int n) {
//!         if (n == 0) {
//!             return 1;
//!         } else {
//!             return n * fact(n - 1);
//!         }
//!     }
//! }
//!
//! // `cinrs_safe` is what makes the call ordinary Rust; see [Safe
//! // functions](#safe-functions). Without it a C function is a foreign
//! // function like any other and a call to it is `unsafe`, which is the
//! // default.
//! assert_eq!(fact(10), 3_628_800);
//! ```
//!
//! Each macro invocation is one translation unit, and a C file that already
//! exists is one too — see [Including a C file](#including-a-c-file):
//!
//! ```text
//! cinrs::include_c99!("vendor/parser.c");
//! ```
//!
//! # Standards and dialects
//!
//! [`c89!`], [`c99!`], [`c11!`], [`c17!`] and [`c23!`] are the same macro for
//! five revisions of the language; `__STDC_VERSION__` is `199901L`,
//! `201112L`, `201710L` and `202311L` from C99 on, and *undefined* in
//! [`c89!`], which is what C89 as published had. The bundled headers follow
//! it. [`c90!`] is another name for [`c89!`]: ISO/IEC 9899:1990 is ANSI
//! X3.159-1989 republished with no technical change.
//!
//! A construct a later revision introduced is a diagnostic in an earlier
//! block, and the diagnostic says which macro to write instead:
//!
//! ```text
//! error: '_Static_assert' requires C11 or later (this block is c99!)
//! ```
//!
//! [`gnu89!`], [`gnu99!`], [`gnu11!`], [`gnu17!`] and [`gnu23!`] are those
//! five with the GNU extensions switched on; see
//! [GNU extensions](#gnu-extensions) for what that changes and what it does
//! not.
//!
//! ## What `c89!` refuses
//!
//! The gate runs backwards in the oldest entry point: everything C99 added is
//! `requires C99 or later (this block is c89!)`. That is `//` comments, mixed
//! declarations and code, a declaration in a `for` clause, variable length
//! arrays, `_Bool`, `restrict`, `inline`, `long long`, designated
//! initializers, compound literals, variadic macros, flexible array members,
//! hexadecimal floating constants, `__func__`, `_Pragma`, universal character
//! names, a trailing comma in an enumerator list, `static` and `[*]` in an
//! array parameter declarator, and `_Complex`.
//!
//! The library half of C99 is *not* gated: a bundled header is a set of
//! declarations, and `snprintf` is one of them. The few declarations that
//! need a C99 *type* — `llabs`, `strtoll`, `int64_t` where it is a
//! `long long` — carry `__extension__`, which switches the gate off for the
//! declaration it is written on, exactly as GCC's own headers do; a program
//! may use it for the same purpose. Neither are the reserved
//! spellings gated: `__inline` and `__restrict` work in `c89!` exactly as they
//! do in `gcc -std=c89`, and for the same reason
//! ([GNU extensions](#gnu-extensions)).
//!
//! [`gnu89!`] switches every one of those back on, as `gcc -std=gnu89` does.
//! What it keeps of C89 is only what a later revision *deleted*: the two
//! rules below.
//!
//! ## Implicit `int` and implicit function declarations
//!
//! In [`c89!`] and [`gnu89!`] — and nowhere else, because C99 removed both
//! (N635, N636) and GCC 14 errors on them in every later mode, GNU dialects
//! included — a declaration with no type specifier declares an `int`, and a
//! call to a function nothing has declared declares one:
//!
//! ```
//! cinrs::c89! {
//!     static cinrs_doc_counter;          /* an `int` */
//!
//!     cinrs_doc_bump()                   /* returning an `int` */
//!     {
//!         cinrs_doc_counter = cinrs_doc_counter + 1;
//!         /* Nothing declares `abs`: this declares `extern int abs();` at
//!            file scope, and the linker resolves it. */
//!         return abs(cinrs_doc_counter);
//!     }
//! }
//!
//! assert_eq!(unsafe { cinrs_doc_bump() }, 1);
//! ```
//!
//! The implicit declaration has no prototype, so the arguments of the call
//! that made it get the default argument promotions, and a later declaration
//! of the same name has to be *compatible* with `int f()` or it is the
//! ordinary "conflicting types". A `__builtin_` name is never declared this
//! way: it belongs to the implementation, so a diagnostic naming it is more
//! use than a link error.
//!
//! ## Old-style (K&R) function definitions
//!
//! `int f(a, b) int a; char *b; { … }` is valid, if obsolescent, C99, and
//! works in **every entry point below [`c23!`]** — C23 is the revision that
//! removed the form (N2432), and there it is a diagnostic that says so.
//!
//! ```
//! cinrs::c99! {
//!     int cinrs_doc_knr(c, n)
//!         char c;
//!         int n;
//!     {
//!         return c * n;
//!     }
//! }
//!
//! assert_eq!(unsafe { cinrs_doc_knr(3, 4) }, 12);
//! ```
//!
//! The identifier list and the declaration list become the parameter list
//! (6.9.1p6); a name the declaration list leaves out is an `int`, which is
//! implicit `int` and therefore [`c89!`] and [`gnu89!`] alone. The resulting
//! function type has **no prototype** (6.9.1p7), so it is compatible with
//! `int f();` and a caller applies the default argument promotions — and that
//! is what the generated item takes: `char c` above is a `c_int` parameter
//! converted on entry, `let c: c_char = c as c_char;`, exactly as C says the
//! callee receives a promoted value and stores it in the declared type.
//! `register` is allowed on a parameter; a declaration-list entry naming
//! something that is not in the identifier list, naming one twice, or
//! carrying an initialiser is a diagnostic, and so is an identifier list on a
//! declaration that is not a definition.
//!
//! ## What `c11!` adds
//!
//! `_Static_assert` at file scope, at block scope and among the members of a
//! `struct`; `_Generic`; `_Alignof`; `_Alignas` on the members of a `struct`
//! or `union`; `_Noreturn`; [`_Atomic` and `<stdatomic.h>`](#atomics); and
//! anonymous `struct`/`union` members, whose own
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
//! improved tag compatibility — a tag defined twice in one scope with the same
//! members declares one type, and two of one name from two scopes are
//! compatible (N3037) — and `unreachable()`.
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
//! `_BitInt` and C23's *named* universal character
//! `\N{LATIN SMALL LETTER E WITH ACUTE}`: each is a located error rather
//! than a silent mistranslation. `_Thread_local` *is* there — see
//! [Thread-local objects](#thread-local-objects) — and so are
//! [atomics](#atomics) and C11's thread library, whose `<threads.h>` is
//! bundled for the two C libraries whose objects it can lay out (glibc and
//! musl, both on Linux) and refuses on the platforms where the library has no
//! such header. Of C11's four subsetting macros, `__STDC_NO_THREADS__` is
//! predefined exactly where that header refuses and `__STDC_NO_COMPLEX__`
//! where the `complex` feature is off — which is the standard's own way of
//! saying that a part is left out; `__STDC_NO_ATOMICS__` and
//! `__STDC_NO_VLA__` are never predefined, because those parts are here — see
//! [atomics](#atomics) and [Variably modified types and
//! `alloca`](#variably-modified-types-and-alloca). Three things are
//! simplifications rather than omissions:
//!
//! * An alignment specifier — `_Alignas(N)` or
//!   `__attribute__((aligned(N)))` — is honoured on the members of a `struct`
//!   or `union`: the member moves to the boundary it asks for, and the
//!   generated item gets `#[repr(C, align(N))]` and explicit padding so that
//!   Rust puts it in the same place. On an *object* it is not supported at
//!   all.
//! * A `constexpr` object is a constant: its value is folded wherever the
//!   name is used, so it may be an array bound or a `case` label, and there
//!   is nothing to take the address of. Only the arithmetic types are
//!   accepted.
//! * `nullptr` has type `void *` rather than a `nullptr_t` of its own, and
//!   `<stddef.h>`'s `nullptr_t` is a `typedef` for `void *`. The difference
//!   only shows where C distinguishes them, such as in `_Generic`.
//!
//! # GNU extensions
//!
//! Real C is written for GCC, so `cinrs` implements the GNU extensions as
//! well, and draws the line between "always" and "only in a GNU block" exactly
//! where GCC draws it.
//!
//! **Everything spelled with a leading double underscore works in every entry
//! point**, including [`c99!`]: `__typeof__`, `__attribute__`, `__extension__`,
//! `__inline__`, `__restrict`, `__alignof__`, `__auto_type`, `__label__` and
//! the whole `__builtin_*` family. Those names are reserved to the
//! implementation, so nothing a program may legally call its own is taken away
//! — which is why GCC's own `-std=c99` keeps them too.
//!
//! ```
//! cinrs::c99! {
//!     /* The kernel's `max`, which evaluates each operand exactly once. */
//!     #define max(a, b) ({ __typeof__(a) _a = (a); __typeof__(b) _b = (b); _a > _b ? _a : _b; })
//!     #define unlikely(x) __builtin_expect(!!(x), 0)
//!
//!     struct __attribute__((packed)) Header { unsigned char kind; unsigned int length; };
//!
//!     int biggest(int a, int b) { return max(a, b); }
//!     int bits(unsigned int n) { return __builtin_popcount(n); }
//!     unsigned long header_size(void) { return sizeof(struct Header); }
//!     const char *whoami(void) { return __func__; }
//!
//!     int classify(int c) {
//!         switch (c) {
//!         case '0' ... '9': return 1;
//!         case 'a' ... 'z': return 2;
//!         default: return unlikely(c < 0) ? -1 : 0;
//!         }
//!     }
//! }
//!
//! assert_eq!(unsafe { biggest(3, 9) }, 9);
//! assert_eq!(unsafe { bits(0b1011) }, 3);
//! assert_eq!(unsafe { header_size() }, 5);
//! assert_eq!(unsafe { classify('7' as i32) }, 1);
//! ```
//!
//! **The plain spellings need a GNU entry point.** `typeof` and `asm` are
//! ordinary identifiers in ISO C, so they are keywords only in [`gnu99!`],
//! [`gnu11!`], [`gnu17!`] and [`gnu23!`] — `typeof` is C23's own keyword too,
//! so [`c23!`] has it as well. A GNU block also **accepts what a later
//! revision added** without the gate: `_Static_assert`, `_Generic`, `0b`
//! literals and the rest, exactly as `gcc -std=gnu99` does.
//!
//! ```
//! cinrs::gnu99! {
//!     _Static_assert(sizeof(int) == 4, "C11 in a C99 block");
//!     typeof(int) identity(int n) { return n; }
//! }
//!
//! assert_eq!(unsafe { identity(7) }, 7);
//! ```
//!
//! `__GNUC__` is `4`, `__GNUC_MINOR__` `2` and `__GNUC_PATCHLEVEL__` `1` in
//! every entry point — Clang's own precedent, and for the same reason: a
//! program guards `__attribute__` and `__builtin_expect` behind
//! `#if defined(__GNUC__) && __GNUC__ >= 4`, and those work here.
//! `__STRICT_ANSI__` is defined in the strict entry points only.
//!
//! [`doc/gnu-extensions.md`][gnu-extensions] in the repository is the
//! catalogue: every extension, how common it is, and whether it is supported,
//! accepted and ignored, refused
//! with a reason, or still to come. The short version of what is *refused* —
//! recognised and reported rather than mistranslated — is inline assembly,
//! `alias`, `weakref`, `vector_size` and
//! `__complex__`. `__attribute__((weak))` is half off that list: it is refused
//! on a *definition*, where Rust's unstable `#[linkage]` would be needed, and
//! accepted and ignored on a declaration of something defined elsewhere, which
//! is what lets glibc's own headers be read. `#include_next` is off it
//! altogether — see [System headers](#system-headers).
//! Neither `alloca` nor `cleanup` is on
//! that list any more: see [Variably modified types and
//! `alloca`](#variably-modified-types-and-alloca) and
//! [`cleanup`](#the-cleanup-attribute); nor are [nested
//! functions](#nested-functions), which are lifted out rather than refused,
//! and nor is
//! `__attribute__((mode(M)))`, which now names the width of the type it is
//! written on — `QI`, `HI`, `SI`, `DI`, `TI`, `byte`, `word`, `pointer`, `SF`
//! and `DF`, with the modes that name a type this crate does not have refused
//! individually.
//!
//! The `__builtin_*` family reaches as far as `<math.h>`: the classification
//! and quiet-comparison builtins `__builtin_isnan`, `isinf`, `isfinite`,
//! `isnormal`, `signbit`, `fpclassify`, `isunordered`, `isgreater` and their
//! relatives are answered in `core` with no maths library involved, and
//! `__builtin_fabs` and `__builtin_copysign` are the bit manipulations they
//! are defined as. A `__builtin_X` for a library function X is a call to X,
//! declared on the spot with the prototype GCC knows for it, so
//! `__builtin_printf("%d\n", n)` works without `<stdio.h>` — and a
//! `long double` maths builtin is the `double` one, `long double` being
//! `double` here.
//!
//! `unreachable()` becomes [`core::hint::unreachable_unchecked`], which is
//! exactly the promise C attaches to it: reaching it is undefined behaviour.
//! It is the one place the expansion trusts the C program with undefined
//! behaviour, because the program asked for it by name.
//!
//! ## The `cleanup` attribute
//!
//! `T x __attribute__((cleanup(f)));` calls `f(&x)` when `x` goes out of
//! scope, in reverse declaration order, on *every* way out of that scope —
//! falling off the end, `break`, `continue`, `return`, or a `goto` that leaves
//! several scopes at once. It is what systemd's `_cleanup_free_` and glib's
//! `g_autofree` are made of, and it is the one GNU extension whose meaning is
//! a Rust feature outright:
//!
//! ```
//! cinrs::gnu99! {
//!     #include <stdlib.h>
//!     #include <string.h>
//!
//!     static void freep(void *p) { free(*(void **)p); }
//!
//!     int length_of_a_copy(const char *text) {
//!         char *copy __attribute__((cleanup(freep))) = malloc(strlen(text) + 1);
//!         if (copy == 0) {
//!             return -1;                  /* freed here … */
//!         }
//!         strcpy(copy, text);
//!         return (int)strlen(copy);       /* … and here */
//!     }
//! }
//!
//! assert_eq!(unsafe { length_of_a_copy(c"cinrs".as_ptr()) }, 5);
//! ```
//!
//! In the ordinary lowering the attribute becomes a **drop guard** bound right
//! after the object, so that Rust's own drop order — reverse declaration
//! order, on every path out of the block, including a `return` from inside a
//! statement expression — *is* C's. In a function lowered through a
//! [control-flow graph](#control-flow) there are no scopes left to drop in:
//! every local is hoisted to the top, so the call is emitted on each edge that
//! leaves the scope instead, which is also what runs it once per pass through
//! a loop body. `return expr;` computes its value before the cleanups run, as
//! GCC does.
//!
//! The function must take one argument, a pointer to the variable's type
//! (`void *` included, which is how the idiom above works), and the attribute
//! belongs on a variable with automatic storage duration: on a parameter, a
//! `static`, a thread-local or a file-scope object GCC drops it with a
//! warning, and `cinrs` refuses it with the reason rather than changing what
//! the program does. A `goto` *into* the scope of one is allowed, and the
//! cleanup still runs when the scope ends — which is GCC's behaviour too.
//!
//! ## Nested functions
//!
//! GNU C lets a function be *defined* inside another one, where it sees the
//! enclosing function's locals. GCC compiles that with a **static chain** — a
//! hidden pointer to the enclosing frame — and, when the nested function's
//! address is taken, with a **trampoline** written onto the stack. Rust has
//! neither, so `cinrs` **lambda-lifts** instead:
//!
//! ```
//! cinrs::gnu99! {
//!     int report(int a, int b) {
//!         int tally = 0;
//!
//!         void note(int value) { tally += value; }
//!
//!         note(a);
//!         note(b);
//!         return tally;
//!     }
//! }
//!
//! assert_eq!(unsafe { report(20, 22) }, 42);
//! ```
//!
//! `note` becomes a file-scope item of its own, `__cinrs_report_note`, private
//! to the expansion — never a C symbol, even under `#pragma cinrs export`.
//! Each object of the enclosing function it uses becomes a hidden pointer
//! parameter in front of the declared ones, named after the variable it
//! carries, and every use of the variable inside the body becomes a
//! dereference of it:
//!
//! ```text
//! unsafe extern "C" fn __cinrs_report_note(__env_tally: *mut c_int, mut value: c_int) {
//!     (*__env_tally) = (*__env_tally).wrapping_add(value);
//! }
//! ```
//!
//! so the call site reads `__cinrs_report_note(&raw mut tally, a)`. The object
//! is therefore **shared, not copied** — a store in the nested function is
//! visible in the enclosing one the moment it returns, which is the whole
//! reason the extension exists — and `&x`, `x++`, `a[i]`, `p.f` and
//! `sizeof x` all keep meaning what they meant, the hidden pointer being a
//! *place* rather than a value. Recursion passes the same environment through;
//! a function nested two levels down receives an outermost local through the
//! middle one, which takes the pointer whether it mentions the variable or
//! not; a nested function that calls a capturing sibling gets what the sibling
//! needs; and `auto int g(int);`, GNU's forward declaration, works, so two
//! nested functions may call each other.
//!
//! A nested function that uses **nothing** of the enclosing frame is lifted to
//! a plain function, and **its address may be taken** — it can be handed to
//! `qsort` like any other callback, since the item's signature is the one C
//! gave it.
//!
//! Four things are refused, each by name rather than mistranslated: **the
//! address of a nested function that does use the enclosing frame** (`&g`, a
//! decay, a callback), which is exactly what GCC's trampoline is for and which
//! the diagnostic names the variables in the way of; a **nonlocal `goto`**,
//! which jumps from the nested body to a label of the enclosing function; and
//! capturing a **variable length array** or a **`va_list`**, neither of which
//! is a plain address — those two say "not supported yet".
//!
//! ## `__int128`
//!
//! GCC's 128-bit integers are there, in every entry point — the double
//! underscore is what makes that safe — and become Rust's `i128` and `u128`,
//! whose x86-64 ABI has matched `__int128`'s since Rust 1.77. Sixteen bytes,
//! aligned the way the compiling toolchain aligns an `i128`, and ranked above
//! `long long`, so `(__int128) a * b` really is a 128-bit multiplication:
//!
//! ```
//! cinrs::c99! {
//!     /* The high half of a 64x64 product — the reason the type exists. */
//!     unsigned long long mul_high(unsigned long long a, unsigned long long b) {
//!         unsigned __int128 product = (unsigned __int128) a * b;
//!         return (unsigned long long) (product >> 64);
//!     }
//!
//!     /* C has no 128-bit *literal*, so a wide constant is built. */
//!     __int128 bit_100(void) { return ((__int128) 1) << 100; }
//! }
//!
//! assert_eq!(unsafe { mul_high(u64::MAX, u64::MAX) }, u64::MAX - 1);
//! assert_eq!(unsafe { bit_100() }, 1i128 << 100);
//! ```
//!
//! `__int128_t` and `__uint128_t` are predefined `typedef` names for the same
//! two types, as they are in GCC, and `__SIZEOF_INT128__` is `16`. Bit-fields
//! of them work — wider than sixty-four bits included, where the accessors
//! read and write through a `u128` window — and so do `_Generic`, the
//! conversions to and from every other scalar, and passing and returning one
//! across the ABI, `...` included.
//!
//! Three things are not there, each a located error rather than a
//! mistranslation. `1 << 100` is still a shift of an `int` by more
//! than its width, because the type of a shift is the type of its left
//! operand and widening the result afterwards does not go back and redo it —
//! which is C's rule and GCC's behaviour, diagnosed here wherever the shift is
//! in a constant expression. `__builtin_add_overflow` and its relatives
//! refuse a 128-bit *operand*: they compute the check one width up from their
//! operands, and there is nothing above 128 bits to compute it in. (A 128-bit
//! *result* type is fine — `__builtin_mul_overflow(a, b, &wide)` with narrower
//! operands says exactly what GCC says.) And `va_arg(ap, __int128)` reads an
//! argument back out of a list, which needs a `VaArgSafe` implementation that
//! Rust still keeps behind the unstable `c_variadic_int128` feature; *passing*
//! a 128-bit value through `...` is unaffected, so a program can read it back
//! as two `unsigned long long` halves — or as a one-member `struct`, which is
//! read [eightbyte by eightbyte](#va_arg-of-a-struct-or-a-union) and so needs
//! no 128-bit `next_arg` at all.
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
//! taking a reference to a `static mut` is an error in edition 2024. `unsafe`
//! is the default, and [Safe functions](#safe-functions) is how a function
//! stops being foreign.
//!
//! The five names Rust cannot write even as raw identifiers — `self`, `Self`,
//! `super`, `crate` and `_` — get an underscore appended instead
//! (`int self(void)` is called as `self_`), and a `$`, which C takes as an
//! identifier character and Rust has no spelling for at all, is written
//! `_dollar_` (`a$b` is `a_dollar_b`); the C name is still what the symbol
//! links by. If the translation unit already uses the result for something
//! else, the spelling grows another `_` until it is free — a unit with both
//! `self` and `self_` calls them `self__` and `self_` — and one C name is then
//! that one Rust name everywhere it appears, as an item, as a member, in a
//! designator and in a bit-field accessor.
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
//! ## Functions declared without a prototype
//!
//! Before C23, `int f();` and `int (*fp)();` say nothing about the parameters
//! (C99 6.7.5.3p14). A call through such a type may pass any number of
//! arguments; each one gets the *default argument promotions* — `float` widens
//! to `double`, the small integer types to `int`, arrays and functions decay —
//! and the callee is then invoked as though its prototype had been made of
//! those promoted types (6.5.2.2p6).
//!
//! The generated Rust type of such a function is `unsafe extern "C" fn() -> R`,
//! with no parameters, because that is all the declaration said. Each call site
//! with at least one argument writes the reinterpretation out:
//!
//! ```text
//! ::core::mem::transmute::<unsafe extern "C" fn() -> R,
//!                          unsafe extern "C" fn(T1, …, Tn) -> R>(f)(a1, …, an)
//! ```
//!
//! (through a function pointer, the `Option` comes off first). That is exactly
//! the contract C's own ABI relies on: the program is defined only if the
//! function really does take parameters of those types, and undefined
//! otherwise — which is the risk the author took by leaving the prototype out.
//! A call with no arguments needs no cast at all.
//!
//! A *definition* written `int f() { … }` takes no parameters, as C99 6.9.1p7
//! says, and the generated item has none; its type still has no prototype, so a
//! call to it with an argument is legal C that the callee simply never looks
//! at. Two declarations of one function are compatible when the prototyped one
//! is not variadic and no parameter type is changed by the promotions
//! (6.7.5.3p15) — so `int f(); int f(int);` is one function and
//! `int f(); int f(char);` is a diagnostic, exactly as GCC has it. That same
//! rule answers `_Generic` and `__builtin_types_compatible_p`, and is what lets
//! `int (*fp)() = g;` take a `g` of any promoted prototype.
//!
//! [`c23!`] and [`gnu23!`] follow C23's N2841 instead: there `int f()` is
//! `int f(void)`, and an argument is one too many.
//!
//! ```
//! cinrs::c99! {
//!     int cinrs_doc_scale();                      /* no prototype */
//!     int cinrs_doc_scale(int n) { return n * 3; }
//!
//!     int nine(void) {
//!         short three = 3;                        /* promoted to `int` */
//!         return cinrs_doc_scale(three);
//!     }
//! }
//!
//! assert_eq!(unsafe { nine() }, 9);
//! ```
//!
//! ## Bit-fields
//!
//! A bit-field has no address of its own — it may share a byte with its
//! neighbours, and it need not start on one — so it cannot be a field of the
//! generated `#[repr(C)]` item. A maximal run of consecutive bit-fields
//! becomes one `pub __cinrs_bitsN: [u8; K]` covering the bytes the run
//! occupies, with explicit `pub __cinrs_padN: [u8; M]` wherever `#[repr(C)]`
//! would otherwise place the next member too early and
//! `#[repr(C, align(N))]` where the fields' own type made the record stricter
//! than any field of it. Each *named* member becomes a pair of inherent
//! methods instead, in plain inline integer code:
//!
//! ```
//! cinrs::c99! {
//!     struct Flags {
//!         unsigned int ready : 1;
//!         int          level : 3;
//!         unsigned int       : 0;   /* start the next field on a new unit */
//!         unsigned int mask  : 30;
//!     };
//!
//!     void arm(struct Flags *f, int level) {
//!         f->ready = 1;
//!         f->level = level;
//!         f->mask += 2;
//!     }
//! }
//!
//! let mut f = Flags { __cinrs_bits0: [0; 8] };
//! unsafe { arm(&raw mut f, -3) };
//! assert_eq!((f.ready(), f.level(), f.mask()), (1, -3, 2));
//!
//! f.set_level(9);          // stores the low three bits …
//! assert_eq!(f.level(), 1); // … and reading one back sign-extends them
//! ```
//!
//! The getter is the member's own name and the setter is `set_` in front of
//! it, both taking and returning the member's declared C type — so an `enum`
//! field reads as the `enum`'s alias and a `_Bool` field as a `bool`. A member
//! whose name is a Rust keyword becomes a raw identifier (`n.r#match()`), one
//! Rust cannot spell at all takes the underscore [What is
//! generated](#what-is-generated) describes (`n.self_()` and `n.set_self()`),
//! and where two names would collide — a member `x` next to a member `set_x` —
//! every getter is claimed first, in declaration order, so a member's own name
//! always reads it and the setter that finds its name taken grows `_2`, `_3`,
//! …. The accessors take `&self` and `&mut self`, so a bit-field of a
//! *file-scope* object is reached through a raw pointer, exactly as the
//! generated code does:
//! `unsafe { (*(&raw mut STATE)).set_ready(1) }`.
//!
//! Inside the C nothing changes: `s.level = 3`, `p->flags |= 1`,
//! `switch (s.kind)`, `++s.count`, a designated initialiser and a compound
//! literal all work as they do for an ordinary member, `sizeof` and
//! `offsetof` see the layout GCC and Clang give the record, and the integer
//! promotions follow C99 6.3.1.1p2's width-restricted rule — `unsigned x : 31`
//! takes part in arithmetic as an `int`, and `unsigned x : 32` as an
//! `unsigned int`. Taking the address of a bit-field, `sizeof` of one and
//! `offsetof` of one are the three things C forbids, and each is a located
//! error.
//!
//! Beyond `_Bool`, `int` and `unsigned int`, which the standard requires, the
//! other integer types and `enum` are accepted as the GCC and Clang extension
//! they are; [`doc/gnu-extensions.md`][gnu-extensions] in the repository
//! records what that commits the layout to, and the one corner where an `enum`
//! field differs.
//!
//! ## Compound literals
//!
//! `(T){ … }` is an *object*, not a value, and C gives one written inside a
//! block the lifetime of that block — so `&(struct S){1, 2}` is a pointer that
//! is still good after the statement that made it. It becomes a hidden binding
//! at the top of the block, with the value stored into it where the literal
//! was written: side effects in the initialiser happen in C's order, and a
//! literal inside a loop is a fresh object on every iteration.
//!
//! ```
//! cinrs::c99! {
//!     struct S { int a; int b; };
//!
//!     int sum(void) {
//!         struct S *p = &(struct S){ 1, 2 };
//!         return p->a + p->b + (int[]){ 10, 20, 30 }[2];
//!     }
//! }
//!
//! assert_eq!(unsafe { sum() }, 33);
//! ```
//!
//! At file scope the object has static storage duration instead and becomes a
//! `static mut` item of its own, so its initialiser has to be a constant
//! expression like any other.
//!
//! ## Over-aligned objects
//!
//! `_Alignas(N)` and `__attribute__((aligned(N)))` are honoured on an
//! *object* — automatic, `static`, at file scope or `_Thread_local` — as well
//! as on a type or a member. Rust has no way to over-align a *binding*, only a
//! type, so the object is generated inside a one-field wrapper that carries
//! the alignment, one per distinct alignment in the unit:
//!
//! ```text
//! #[repr(C, align(64))] #[derive(Copy, Clone)]
//! pub struct __cinrs_align_64<T>(pub T);
//!
//! pub static mut buf: __cinrs_align_64<[c_char; 256]> = __cinrs_align_64([0; 256]);
//! ```
//!
//! Every use of the object in the generated code goes through the field —
//! `&buf` is `&raw mut buf.0` — and **Rust code that reaches such an object
//! reads `buf.0`** for the same reason. Nothing about the C program changes:
//! `sizeof buf` is the array's size, the wrapper is invisible to it, and
//! `_Alignof buf` answers what the declaration asked for.
//!
//! ```
//! cinrs::c11! {
//!     #include <stdint.h>
//!
//!     _Alignas(64) char buf[256];
//!
//!     int aligned(void) { return (uintptr_t) &buf % 64 == 0; }
//! }
//!
//! assert_eq!(unsafe { aligned() }, 1);
//! assert_eq!(unsafe { (&raw const buf.0) as usize % 64 }, 0);
//! ```
//!
//! The strictest of several specifiers wins, `_Alignas(T)` asks for a type's
//! alignment, and bare `aligned` for the target's `max_align_t`. An `_Alignas`
//! *weaker* than the type's own alignment is the constraint violation C11
//! 6.7.5p4 makes it; GCC's `aligned` "can only increase alignment", so a
//! weaker one there is quietly not applied. The five declarations C11 6.7.5p2
//! forbids the specifier in — a `typedef`, a bit-field, a function, a
//! parameter and an object declared `register` — are each refused where they
//! are written, and so is a variable length array, whose storage is allocated
//! at run time.
//!
//! ## Initialising a flexible array member
//!
//! C99 forbids it, because the object would have to be larger than its type.
//! GNU C allows it for an object with **static storage duration**, whose
//! storage the compiler can make that large, and so does this: the item is
//! given a *companion* type with the record's leading layout and a tail as
//! long as the initialiser, and the C object is the record at its address.
//!
//! ```text
//! #[repr(C)] pub struct __cinrs_W_3 { pub n: c_int, pub data: [c_int; 3] }
//! pub static mut w: __cinrs_W_3 = __cinrs_W_3 { n: 3, data: [1, 2, 3] };
//! // every use of `w` in the C program is  (*(&raw mut w).cast::<W>())
//! ```
//!
//! `sizeof w` is still `sizeof(struct W)`, which is what GCC says too, and
//! `#pragma cinrs export` exports the storage — the full size — under the C
//! name. **Rust code reads the companion**, whose fields are the record's with
//! the tail sized by the initialiser:
//!
//! ```
//! cinrs::c99! {
//!     struct W { int n; int data[]; };
//!
//!     struct W w = { 3, { 1, 2, 3 } };
//!
//!     int last(void) { return w.data[w.n - 1]; }
//! }
//!
//! assert_eq!(unsafe { last() }, 3);
//! assert_eq!(unsafe { (&raw const w).read().data }, [1, 2, 3]);
//! ```
//!
//! An **automatic** object cannot be made larger than its type, and neither
//! can a record nested inside another aggregate — an array of them, a member,
//! or a compound literal — whose own size already fixes the room there is.
//! Both are refused, in GCC's own words.
//!
//! ## Variably modified types and `alloca`
//!
//! `T a[n];` where `n` is not a constant is an ordinary C99 declaration, and
//! it works here: the bound is evaluated once, where the declaration stands;
//! the object lives until the end of the block; `sizeof a` is a value computed
//! at run time; and a declaration inside a loop makes a fresh object on every
//! pass. `alloca` — `<alloca.h>`, or `__builtin_alloca` spelled directly —
//! works too, with the lifetime C gives it: until the *function* returns.
//!
//! ```
//! cinrs::c99! {
//!     #include <stdio.h>
//!     #include <string.h>
//!
//!     int describe(const char *name, int size) {
//!         char buf[size];                 /* size is not a constant */
//!         int written = snprintf(buf, sizeof buf, "%s has %d letters",
//!                                name, (int)strlen(name));
//!         return written * 100 + (int)sizeof buf;
//!     }
//! }
//!
//! assert_eq!(unsafe { describe(c"cinrs".as_ptr(), 32) }, 19 * 100 + 32);
//! ```
//!
//! **The storage is the heap, not the stack.** Rust has no way to move the
//! stack pointer by an amount chosen at run time, so the elements live in a
//! hidden `Vec` whose `Drop` is the end of the block — the object's lifetime —
//! and the C object itself is a pointer into it. Every observation a C program
//! can make is the one C promises: the elements, the lifetime, the fresh
//! object each pass through a loop, `sizeof` at run time, and `&a` with type
//! `T (*)[n]`. What changes is where the bytes are, and that a very large one
//! fails the way a `malloc` does rather than by running the stack out. A zero
//! length allocates nothing, as it does in GCC; a *negative* one is undefined
//! behaviour in C, and here it converts to a huge `size_t` and the allocation
//! aborts rather than corrupting anything.
//!
//! `alloca(n)` takes one 16-byte aligned block out of a per-function arena,
//! and the whole arena is freed by the `return`. That is `alloca`'s own
//! lifetime — its memory belongs to the function, not to the block the call
//! was written in — so a pointer to it returned to the caller dangles here
//! exactly as it does in C, and nothing about the emulation makes a defined
//! program behave differently.
//!
//! In a function lowered into a [state machine](#goto), where every local is
//! hoisted to the top, the hidden `Vec` is hoisted with them: it is created
//! empty, filled where the declaration was written, and dropped when the
//! function returns rather than when the block ends. A C program can only
//! observe that as memory it expected to have been given back sooner.
//!
//! ### More than one dimension
//!
//! A *variably modified* type is any type with a run-time bound in it, and
//! they all work: `double a[n][m]`, `int a[3][n]` and `int a[n][3]`,
//! `int (*p)[n]`, `typedef int T[n];`, and the parameter form
//! `void f(int n, int m, double a[n][m])` that C adjusts to
//! `double (*a)[m]`.
//!
//! The model is one hidden `size_t` object per variable dimension, created
//! where the *type* is declared and named by the type itself. A `typedef`
//! evaluates its bound once, at the `typedef` (6.7.7p4), and every object
//! declared with it shares that length; a definition's parameter evaluates its
//! bounds on entry, in declaration order (6.9.1p10), so assigning to `m`
//! inside the body cannot change the shape of `a`. Everything else is
//! arithmetic over those objects: one heap buffer of `n * m` elements holds
//! `double a[n][m]`, `a[i][j]` is `*(base + i * m + j)`, `a[i]` decays to a
//! `double (*)[m]` — the same pointer with a different C type — `p + 1` moves
//! by a whole row, and `sizeof a`, `sizeof a[0]` and `sizeof *p` are products
//! of the bounds.
//!
//! ```
//! cinrs::c99! {
//!     /* Called from Rust with a flat buffer, and indexed as a matrix. */
//!     void scale(int n, int m, double a[n][m], double by) {
//!         for (int i = 0; i < n; i++)
//!             for (int j = 0; j < m; j++)
//!                 a[i][j] *= by;
//!     }
//! }
//!
//! let mut buf = [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0];
//! unsafe { scale(2, 3, buf.as_mut_ptr(), 10.0) };
//! assert_eq!(buf, [10.0, 20.0, 30.0, 40.0, 50.0, 60.0]);
//! ```
//!
//! What is refused is what C itself forbids: a variably modified type at file
//! scope or among the members of a `struct`, an object of one declared
//! `static` or `extern` or with an initialiser, `[*]` outside a prototype, and
//! a `goto` or a `case` that would jump into the scope of one, which C99
//! 6.8.6.1p1 and 6.8.4.2p2 both forbid because the bound would not have been
//! evaluated. The one gap of this crate's own is a bound written in a *type
//! name* — `(double (*)[m])p`, where there is no declaration to keep the
//! length in — which is refused where the length would be needed;
//! `sizeof(int[n][m])`, whose bounds C evaluates on the spot (6.5.3.4p2),
//! works.
//!
//! Variably modified types and `alloca` are the only constructs whose
//! expansion needs more than `core`; see [`no_std`](#no_std) for what that
//! means in a crate without an allocator.
//!
//! # Safe functions
//!
//! A C function is a foreign function, and calling one is `unsafe` for the
//! same reason calling any other is. A function the unit marks **safe** is
//! generated without it — as a plain `pub extern "C" fn` whose body is *not*
//! wrapped in an `unsafe` block — so `rustc` checks the whole translation, and
//! Rust calls it like any other Rust function:
//!
//! ```
//! cinrs::c99! {
//!     __attribute__((cinrs_safe)) int gcd(int a, int b) {
//!         while (b != 0) {
//!             int t = a % b;
//!             a = b;
//!             b = t;
//!         }
//!         return a < 0 ? -a : a;
//!     }
//! }
//!
//! assert_eq!(gcd(48, -18), 6);
//! ```
//!
//! There are three spellings, and they mean the same thing:
//!
//! * `[[cinrs::safe]]`, the C23 attribute in this crate's own vendor
//!   namespace, wherever `[[…]]` parses — [`c23!`], [`gnu23!`] and the other
//!   GNU dialects, which take the C23 syntax as GCC does. In a strict
//!   pre-C23 entry point an attribute specifier is gated, so write one of the
//!   other two there;
//! * `__attribute__((cinrs_safe))`, which works in **every** entry point,
//!   [`c89!`] included, for the reason every other double-underscore spelling
//!   does: the name is reserved to the implementation;
//! * `#pragma cinrs safe f g h`, a list of function names, which works in
//!   every entry point *and* in [string-literal
//!   form](#input-forms) and needs nothing written on the function itself. A
//!   name the unit does not declare is a diagnostic.
//!
//! A safe function may also be `static` (private to the module, as C says), be
//! `inline`, and be exported — `#pragma cinrs export` gives it
//! `#[unsafe(no_mangle)] pub extern "C" fn`.
//!
//! ## What a safe body may hold
//!
//! Whatever passes Rust's own checks, which is more C than it sounds: integer
//! and floating arithmetic and comparisons, all the control flow (`if`,
//! `while`, `do`, `for`, `switch` with fallthrough, and `goto`, whose labelled
//! blocks and [state machine](#goto) are safe code too), locals and parameters, `struct`,
//! `union` and `enum` values passed and returned **by value**, `_Bool`,
//! bit-field accessors on a local, the [complex](#complex-numbers) arithmetic,
//! pointer *values* (holding one, comparing it, returning it), string literals,
//! and calls to other safe functions of the same unit.
//!
//! Division is Rust's, which is C99's: it truncates towards zero. What C
//! leaves undefined — a zero divisor, `INT_MIN / -1` — Rust panics on, and a
//! panic cannot cross an `extern "C"` frame, so the program aborts. That is a
//! run-time property rather than something `safe` promises.
//!
//! ## What `rustc` refuses
//!
//! Everything else, with the caret on the C that asked for it. Reading or
//! writing **through a pointer** — `*p`, `p[i]`, `p->f` — since pointers are
//! raw pointers, and that includes the elements of an **array**, local or not,
//! because `a[i]` is `*(a + i)` in C and is translated as one; a
//! **file-scope object**, which is a `static mut`; a
//! **`union` member**, whose read is unsafe in Rust; a call to a function the
//! unit only **declares**, such as anything from the C library; a
//! **`_Thread_local` object**, every access to which is a dereference of the
//! `*mut T` its cell hands out; the **atomics**, which are reached through
//! `AtomicX::from_ptr`; a call **through a function pointer**; the elements of
//! a **variable length array** or of `alloca`'s storage, which are reached
//! through a pointer like any others; reading an argument out of a
//! **`va_list`**; a cast that becomes a `transmute`; and C23's
//! `unreachable()`, which is
//! [`core::hint::unreachable_unchecked`] — the one construct that asks for
//! undefined behaviour by name.
//!
//! The messages are `rustc`'s, because `rustc` is what did the checking; one
//! case has a message of this crate's instead, since C is what a reader is
//! looking at:
//!
//! ```text
//! error: function 'helper' is not safe; mark it [[cinrs::safe]] or call it
//! from a non-safe function
//! ```
//!
//! ## What is refused outright
//!
//! Three shapes could never be safe, and say so where the request was written
//! rather than turning into a puzzle from `rustc`: a function this unit only
//! **declares** (there is no body to check); a **variadic** definition (Rust
//! makes every function with a C variable argument list `unsafe`); and a GNU
//! **nested function**, which reaches the enclosing frame through [hidden
//! pointer parameters](#nested-functions) that its body dereferences. So is
//! the attribute on a `typedef` or an object of function pointer type, neither
//! of which has a body either.
//!
//! One corner is worth knowing: the accessors generated for the bit-fields of
//! a **`union`** are safe methods that read the storage inside an `unsafe`
//! block of their own, so a safe function may read one where reading an
//! ordinary member of that `union` would be refused.
//!
//! Nothing about the C changes: a call *from* C to a safe function is what it
//! always was, and a function that is not safe may call one that is.
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
//! # `no_std`
//!
//! **Everything generated is [`core`]-only**, with three exceptions. A C
//! function becomes a `pub unsafe extern "C" fn` over [`core::ffi`] types;
//! records are `#[repr(C)]` items; pointers are raw pointers; a string literal
//! is a byte string; `goto` is a labelled block or a state machine; `unreachable()` is
//! [`core::hint::unreachable_unchecked`]; `offsetof` is
//! [`core::mem::offset_of!`]; `__builtin_trap` and `assert` call the C
//! library's `abort`; [atomics](#atomics) are [`core::sync::atomic`]; a
//! `constructor` is a `#[used]` function pointer in
//! `.init_array`. None of that touches `std`. The C *library* is still linked,
//! because the C code calls it — that is a link-time dependency of the
//! program, not a Rust one, and it is what the bundled headers declare.
//!
//! Two of the exceptions are [variably modified types and
//! `alloca`](#variably-modified-types-and-alloca), whose storage is a `Vec`.
//! Nothing in the C says which kind of crate the expansion is going into, so
//! that `Vec` is spelled `::std::vec::Vec` unless the unit says otherwise:
//!
//! ```
//! # #[cfg(any())]
//! # mod example {
//! #![no_std]
//! extern crate alloc;         // the macro cannot add this for you
//!
//! cinrs::c99! {
//!     #pragma cinrs no_std
//!     long sum(int n) {
//!         int a[n];
//!         for (int i = 0; i < n; i++) a[i] = i;
//!         long total = 0;
//!         for (int i = 0; i < n; i++) total += a[i];
//!         return total;
//!     }
//! }
//! # }
//! ```
//!
//! `#pragma cinrs no_std` makes the expansion say `::alloc::vec::Vec`
//! instead. The `extern crate alloc;` has to be yours: an expansion is items,
//! and a crate-level directive is not one of them.
//!
//! A unit that uses neither construct needs neither the pragma nor an
//! allocator. One that uses them in a `#![no_std]` crate *without* the pragma
//! gets `rustc`'s own `E0433` — "cannot find `std`" — with the caret on the
//! declaration that needed it; the fix is the two lines above.
//!
//! The third exception is [`_Thread_local`](#thread-local-objects), and the
//! pragma does not help there: `thread_local!` is a `std` macro and `core` has
//! no thread-local storage at all, so a unit that declares a thread-local
//! object under `#pragma cinrs no_std` is a located error saying so.
//!
//! [Complex numbers](#complex-numbers) are the one construct whose expansion
//! names a crate rather than `core` — `cinrs::rt`, the re-export of
//! `cinrs-rt` —
//! and `cinrs-rt` is `#![no_std]` too, so it changes nothing here. Switching
//! the `complex` feature off (`default-features = false`) drops the dependency
//! and `_Complex` with it.
//!
//! # Complex numbers
//!
//! `float _Complex` is `cinrs::rt::Complex<f32>` and
//! `double _Complex` is `Complex<f64>` — which is
//! [`num_complex::Complex`](https://docs.rs/num-complex), a `#[repr(C)]` pair
//! that the numeric half of crates.io already speaks. That is the point of
//! choosing it: a complex value crosses between C and Rust with no conversion
//! and no wrapper type of this crate's own.
//!
// The one example in these docs that cannot compile without the feature it is
// about; `doc` attributes rather than `//!` lines so that it is left out of a
// `default-features = false` build, where `cinrs::rt` does not exist and the C
// below is a diagnostic.
#![cfg_attr(
    feature = "complex",
    doc = "```",
    doc = "use cinrs::rt::Complex;",
    doc = "",
    doc = "cinrs::c99! {",
    doc = "    #include <complex.h>",
    doc = "",
    doc = "    double _Complex root(double _Complex z) { return csqrt(z); }",
    doc = "    double _Complex scale(double _Complex z, double x) { return z * x; }",
    doc = "    double magnitude(double _Complex z) { return cabs(z); }",
    doc = "    double _Complex unit = 1.0 + 2.0i;",
    doc = "}",
    doc = "",
    doc = "assert_eq!(unsafe { root(Complex::new(-1.0, 0.0)) }, Complex::new(0.0, 1.0));",
    doc = "assert_eq!(unsafe { scale(Complex::new(1.0, 2.0), 3.0) }, Complex::new(3.0, 6.0));",
    doc = "assert_eq!(unsafe { magnitude(Complex::new(3.0, -4.0)) }, 5.0);",
    doc = "assert_eq!(unsafe { unit }, Complex::new(1.0, 2.0));",
    doc = "```"
)]
//!
//! `long double _Complex` is `double _Complex`, the mapping `long double`
//! already has and the same ABI caveat: on a target whose `long double` is
//! wider, a value passed to or from a library compiled by the platform's C
//! compiler is the wrong size. The complex types themselves are two components
//! side by side, so `sizeof(double _Complex)` is 16 and its alignment is
//! `double`'s — which is what every ABI this crate targets says, and what the
//! [data-model assertion](#the-assertion-that-guards-it) checks for a unit
//! that has one.
//!
//! ## What the operators do
//!
//! `+ - * /`, unary `+ -`, `==`/`!=`, compound assignment and `++`/`--` (which
//! step the *real* part, as GCC has them). The relational operators, `%`, the
//! bitwise operators and the shifts are refused with the reason: C gives them
//! real operands only, and the complex numbers are not ordered.
//!
//! Two things about the arithmetic are worth knowing, because both are
//! observable and neither is what a first reading of the standard suggests.
//!
//! * **An infinity survives.** C99 Annex G.5.1 — which GCC implements by
//!   default — requires a product or a quotient with an infinite operand to be
//!   infinite, even where the schoolbook formula produces NaN + iNaN out of an
//!   `∞ − ∞`. `(∞ + 0i) · (3 − 4i)` is `∞ − ∞i`, not a NaN.
//! * **A real operand stays real.** `3.0 * z` is computed componentwise rather
//!   than as `(3 + 0i) · z`, which the sign of a zero can tell apart. That is
//!   what GCC and Clang both do, and `cinrs` follows them.
//!
//! Both live in `cinrs::rt::complex`, so an expansion holds a
//! call rather than a copy of the algorithm; `tests/complex.rs` checks them
//! against the host's own C compiler over a hundred and thirty thousand
//! operand pairs.
//!
//! ## The rest of it
//!
//! `<complex.h>` is bundled: `complex`, `I`, `_Complex_I`, C11's `CMPLX`
//! family, and the function declarations, which link against the platform's
//! library and pass complex values *by value* (a `Complex<f64>` and a
//! `double _Complex` are the same two eightbytes on x86-64 System V; i386 and
//! Windows are untested for that). `creal`, `cimag`, `conj` and `cproj` are
//! the compiler's own builtins rather than calls. GNU's `__real__` and
//! `__imag__` are there and are **lvalues** when their operand is one, so
//! `__imag__ z = 1.0;` assigns; `~z` is the conjugate; and the imaginary
//! suffixes `2.0i`, `1.0if` and `3.0jl` make a pure imaginary constant.
//!
//! What is refused, each with the reason: `_Imaginary` (no compiler implements
//! it, and C99 7.3.1p3 makes it optional), a complex *integer* type
//! (`_Complex int`, a GNU extension of its own — `3i` says to write `3.0i`),
//! an `_Atomic` or a bit-field of one, `<tgmath.h>`, and any `printf`
//! conversion for one, C having none. `va_arg(ap, double _Complex)` *is*
//! there: a pair is read back the way [any other
//! aggregate](#va_arg-of-a-struct-or-a-union) is.
//! `__STDC_IEC_559_COMPLEX__` is never defined: the arithmetic of Annex G.5.1
//! is implemented, the rest of the annex is not claimed.
//!
//! ## The `complex` feature
//!
//! All of the above needs the **`complex` feature**, which is **on by
//! default**. It is a feature at all because this is the one thing `cinrs`
//! generates that names a crate rather than `core`; with
//! `default-features = false` the `cinrs-rt` dependency goes,
//! `__STDC_NO_COMPLEX__` is predefined — C11 6.10.8.3's way of saying an
//! implementation has no complex arithmetic — and `_Complex` becomes a
//! diagnostic that names the feature to turn on.
//!
//! The generated code spells the runtime's path in full, as `::cinrs::rt`. A
//! crate that renamed its dependency, or that reaches `cinrs` through a
//! re-export, says so:
//!
//! ```text
//! #pragma cinrs crate "crate::vendor::cinrs"
//! ```
//!
//! Nothing else in an expansion mentions the path, so a unit without a complex
//! type never needs the pragma.
//!
//! # Thread-local objects
//!
//! C11's `_Thread_local` — `thread_local` in [`c23!`], `__thread` in GNU C and
//! therefore in every entry point — gives an object static storage duration
//! and one instance per thread. That is exactly what Rust's
//! `std::thread_local!` provides, so the object becomes one, holding an
//! [`UnsafeCell`](core::cell::UnsafeCell) because C code assigns to it:
//!
//! ```
//! cinrs::c11! {
//!     _Thread_local int counter;
//!
//!     int bump(int by) {
//!         counter += by;
//!         return counter;
//!     }
//! }
//!
//! assert_eq!(unsafe { bump(1) }, 1);
//! assert_eq!(unsafe { bump(2) }, 3);
//!
//! // Another thread has a counter of its own, starting from the initialiser.
//! let other = std::thread::spawn(|| unsafe { bump(10) }).join().unwrap();
//! assert_eq!(other, 10);
//! assert_eq!(unsafe { bump(0) }, 3);
//!
//! // Rust reads the object through the generated item, which is `pub` when
//! // the C object has external linkage.
//! assert_eq!(counter.with(|cell| unsafe { *cell.get() }), 3);
//! ```
//!
//! Every C access goes through the `*mut T` the cell hands out, which is valid
//! for as long as *this thread's* copy of the object is — which is precisely
//! what C promises about the address of one, so `&counter` is that pointer and
//! nothing about the object model changes.
//!
//! Where it may be written is C's rule (6.7.1p3): at file scope on its own or
//! beside `static`, and at block scope only on a `static`, since an object
//! with automatic storage duration is per *call* rather than per thread. On a
//! parameter or a function it is an error, and so is a non-constant
//! initialiser, as it is for any object with static storage duration.
//!
//! The initialiser goes inside `thread_local!`'s `const { … }` block wherever
//! Rust allows it — the cheap form, with no lazy-initialisation flag. One
//! thing keeps it out: an initialiser whose value is the address of another
//! item, such as `_Thread_local int *p = &global;`, since a Rust constant may
//! not refer to a `static`. Such an object takes the lazy form instead, which
//! is a difference in when the initialiser runs and in nothing a C program can
//! observe.
//!
//! Two shapes are refused rather than mistranslated. `extern _Thread_local int
//! x;` — a TLS symbol another object file defines — would need Rust's
//! `#[thread_local]` attribute on an `extern` item, which is unstable; and
//! `#pragma cinrs export` cannot give a `thread_local!` item a C symbol,
//! because there is no stable way to.
//!
//! # C11 threads
//!
//! `<threads.h>` (C11 7.26) is bundled, and the threads it makes are the C
//! library's own: `thrd_create` is the library's `thrd_create`, and the
//! objects the header declares are laid out the way that library lays them
//! out, so a `mtx_t` an expansion puts on the stack is a `pthread_mutex_t` the
//! library can lock.
//!
//! ```
//! # #[cfg(all(target_os = "linux", any(target_env = "gnu", target_env = "musl")))]
//! # fn main() {
//! cinrs::c11! {
//!     #include <threads.h>
//!
//!     static mtx_t lock;
//!     static long counter;
//!
//!     static int bump(void *times) {
//!         long i;
//!         for (i = 0; i < *(long *)times; i++) {
//!             mtx_lock(&lock);
//!             counter++;
//!             mtx_unlock(&lock);
//!         }
//!         return 0;
//!     }
//!
//!     long counted(long times) {
//!         thrd_t a, b;
//!         counter = 0;
//!         mtx_init(&lock, mtx_plain);
//!         thrd_create(&a, bump, &times);
//!         thrd_create(&b, bump, &times);
//!         thrd_join(a, 0);
//!         thrd_join(b, 0);
//!         mtx_destroy(&lock);
//!         return counter;
//!     }
//! }
//!
//! assert_eq!(unsafe { counted(1000) }, 2000);
//! # }
//! # #[cfg(not(all(target_os = "linux", any(target_env = "gnu", target_env = "musl"))))]
//! # fn main() {}
//! ```
//!
//! Two of the types — `mtx_t` and `cnd_t` — are opaque blocks of bytes whose
//! size belongs to the C library rather than to C, so the header models the
//! two libraries whose layouts it knows: **glibc** (whose `thrd_*` functions
//! arrived in 2.28) and **musl**, both on Linux. Everywhere else it is an
//! `#error` naming the reason — Apple's libSystem and the Microsoft UCRT have
//! no `<threads.h>` at all, and the BSDs, bionic and uClibc lay the objects
//! out their own way — and `__STDC_NO_THREADS__` is predefined there, which is
//! C11 6.10.8.3's way of saying so and what lets a portable program take the
//! other branch instead of hitting the `#error`.
//!
//! `struct timespec` comes from `<time.h>`, where C puts it, beside
//! `timespec_get` and `TIME_UTC`; `thread_local` is defined as
//! `_Thread_local` up to C17 and left alone in [`c23!`], where it is a
//! keyword. One function is declared but should not be called from translated
//! C: `thrd_exit` is `pthread_exit`, which ends the thread by forcing an
//! unwind through the frames above it, and Rust aborts rather than let a
//! foreign unwind cross the generated `extern "C"` frame. Return from the
//! thread function instead.
//!
//! # Atomics
//!
//! C11's atomics are here, and so are the two builtin families that came
//! before them. All of it is [`core::sync::atomic`], so all of it works in a
//! `#![no_std]` crate:
//!
//! ```
//! cinrs::c11! {
//!     #include <stdatomic.h>
//!
//!     _Atomic int counter;
//!     atomic_flag lock = ATOMIC_FLAG_INIT;
//!
//!     int bump(void) { return ++counter; }
//!     int take(void) { return !atomic_flag_test_and_set(&lock); }
//!     void give(void) { atomic_flag_clear(&lock); }
//!     int add(int *p, int n) { return __atomic_fetch_add(p, n, __ATOMIC_RELAXED); }
//!     int older(int *p, int n) { return __sync_add_and_fetch(p, n); }
//! }
//!
//! assert_eq!(unsafe { bump() }, 1);
//! assert_eq!(unsafe { take() }, 1);
//! assert_eq!(unsafe { take() }, 0);
//! unsafe { give() };
//!
//! let mut n = 0;
//! assert_eq!(unsafe { add(&mut n, 5) }, 0);
//! assert_eq!(unsafe { older(&mut n, 5) }, 10);
//! ```
//!
//! ## The `_Atomic` object model
//!
//! `_Atomic T` — the qualifier, and the `_Atomic(T)` specifier form — is a
//! *type*, not a flag on a declaration: `_Atomic int *` and `int *` are
//! different types, and a store through the first one is atomic. The object
//! itself is generated as a plain `T`, and every access to it goes through
//! `AtomicX::from_ptr` over its address:
//!
//! * every read is a sequentially consistent **load**;
//! * every write, and every plain assignment, is a sequentially consistent
//!   **store** whose value is the value assigned;
//! * `x += v`, `x |= v`, `x++` and `--x` are each one **read-modify-write**,
//!   as C11 6.5.16.2p3 requires — never a load followed by a store. The five
//!   operators an atomic has a method for become that method; the rest
//!   (`*=`, `<<=`, a floating object, a pointer that moves by elements) become
//!   the compare-exchange loop the method would have been;
//! * the *initialiser* of a declaration is a plain write, which is what
//!   7.17.2.1p2 says it is.
//!
//! Reading one gives a value of the underlying type — lvalue conversion drops
//! the `_Atomic` (6.3.2.1p2) — so `_Generic(x, int: …)` matches an
//! `_Atomic int` lvalue, passing one by value passes a `T`, and nothing
//! downstream of the read has to know about atomics at all.
//!
//! The **alignment of an atomic type is its size**: `_Alignof(_Atomic long
//! long)` is 8 wherever `sizeof` is, which is what a lock-free instruction
//! needs and what GCC does too. A `struct` with an `_Atomic` member gets the
//! `#[repr(C, align(N))]` and the padding that keeps the two sides agreeing
//! about where the member went.
//!
//! Which types: `_Bool`, the 1-, 2-, 4- and 8-byte integers (`enum`s
//! included), `float` and `double` — through the integer atomic of the same
//! width and `to_bits`/`from_bits`, so the bits are exactly what the object
//! holds — and object pointers, which are an `AtomicPtr`. An `_Atomic`
//! `struct` or `union` is legal C and is **refused**: it would need a lock,
//! and there is nothing in the generated Rust to be one. So is `_Atomic
//! __int128`, for want of a stable `AtomicU128`, and an atomic function
//! pointer, which Rust models as an `Option<fn>` rather than as a pointer.
//! `_Atomic` on an array or a function type is a constraint violation and is
//! reported as one.
//!
//! ## The builtins
//!
//! Three families, every one of them spelled with a leading double underscore
//! and therefore available in **every entry point**, `c89!` included:
//!
//! * **`__atomic_*`** (GCC 4.7) takes the memory order as an argument:
//!   `load_n`/`load`, `store_n`/`store`, `exchange_n`/`exchange`,
//!   `compare_exchange_n`/`compare_exchange`, `fetch_add` … `fetch_nand` and
//!   `add_fetch` … `nand_fetch`, `test_and_set`, `clear`, `thread_fence`,
//!   `signal_fence`, `always_lock_free` and `is_lock_free`. The `_n`-less
//!   forms take pointers to the values instead of the values.
//! * **`__sync_*`** (GCC 4.1) is the older family, and every one of them is
//!   sequentially consistent: `fetch_and_add` and its relatives,
//!   `add_and_fetch` and its, `bool_compare_and_swap`,
//!   `val_compare_and_swap`, `lock_test_and_set` (an acquire exchange),
//!   `lock_release` (a release store of zero) and `synchronize`.
//! * **`__c11_atomic_*`** is Clang's, and is what the bundled
//!   `<stdatomic.h>` is written in terms of, exactly as Clang's own header is.
//!
//! The memory order must be an integer constant expression — one of the
//! `__ATOMIC_RELAXED` … `__ATOMIC_SEQ_CST` macros, which are predefined
//! everywhere, or a `memory_order_…` constant, which are the same values.
//! `__ATOMIC_CONSUME` is an acquire, as it is in every compiler. An order that
//! is *not* a constant falls back to `__ATOMIC_SEQ_CST`, as GCC's own
//! documentation says it does, and the expression is still evaluated; a
//! `weak` flag that is not a constant is taken as *strong*, which a weak
//! compare-exchange is always allowed to be. An order
//! the operation may not have — a load that releases, a store that acquires, a
//! compare-exchange whose failure order is stronger than its success order —
//! is a diagnostic here, where Rust would panic at run time and C leaves it
//! undefined.
//!
//! **One deliberate difference between the families.** On a *pointer* object,
//! `__atomic_fetch_add(&p, 4, …)` moves `p` on by four **bytes**, whatever it
//! points at, which is what GCC's builtin does; `atomic_fetch_add(&p, 4)` from
//! `<stdatomic.h>` moves it by four **elements**, which is what C11 7.17.7.5
//! requires. The header goes through `__c11_atomic_fetch_add`, which is the
//! one that scales.
//!
//! `<stdatomic.h>` is bundled: the `atomic_bool` … `atomic_uintmax_t`
//! typedefs, `atomic_flag` with `ATOMIC_FLAG_INIT`, the `memory_order`
//! enumeration, `ATOMIC_VAR_INIT`, `atomic_init`, `kill_dependency`, the
//! fences, `atomic_is_lock_free`, the `ATOMIC_*_LOCK_FREE` macros (all `2`)
//! and the generic functions. Neither `__STDC_NO_ATOMICS__` nor — on a target
//! whose C library the bundled [`<threads.h>`](#c11-threads) models —
//! `__STDC_NO_THREADS__` is predefined.
//!
//! # Control flow
//!
//! `if`, `while`, `do`/`while`, `for`, `break`, `continue` and `switch` — with
//! fallthrough — become Rust's own control flow, so the expansion reads like
//! the C it came from.
//!
//! ## goto
//!
//! Most `goto`s go **outwards**, and those keep Rust's own control flow too.
//! A jump *forwards* to a label later in a block it is inside becomes a
//! `break` out of a labelled block that ends where the label stands; a jump
//! *backwards* to a label that block begins with becomes a `continue` of a
//! labelled loop that starts there. The blocks are named after the C labels,
//! so `goto done` reads as `break 'done` and `goto retry` as `continue
//! 'retry`, and they nest.
//!
//! ```
//! cinrs::c99! {
//!     int find_pair(const int *values, int n, int target) {
//!         int found = -1;
//!         for (int i = 0; i < n; i++) {
//!             for (int j = i + 1; j < n; j++) {
//!                 if (values[i] + values[j] == target) {
//!                     found = i * 100 + j;
//!                     goto done;                  /* break 'done */
//!                 }
//!             }
//!         }
//!     done:
//!         return found;
//!     }
//! }
//!
//! let values = [1, 2, 3, 9];
//! assert_eq!(unsafe { find_pair(values.as_ptr(), 4, 5) }, 102);
//! ```
//!
//! A scope left on the way out is left in full: the `break` runs the drops of
//! everything the blocks it leaves hold, which is what frees a
//! [variable length array](#variable-length-arrays) and runs a
//! [`cleanup`](#attributes) attribute exactly where C says they run.
//!
//! What is left over is lowered into a **state machine** over basic blocks
//! instead, with every local of the function hoisted to the top and renamed
//! apart: a jump *into* a block — a label inside a loop body, an `if` branch
//! or a `switch` group, named from outside it — a computed `goto` (below), a
//! `case` label that is not a direct child of its `switch` body (Duff's
//! device), two labels whose regions would have to overlap without nesting,
//! which is what a hand-written state machine looks like, and a declaration
//! between a jump and the label it names, which no Rust block may hold without
//! ending its scope early. Both forms compute exactly what the C did; only the
//! second is unpleasant to read, and only the functions that need it get it.
//!
//! ## Labels as values
//!
//! GNU C's computed `goto` is supported, and it is why the state machine is
//! worth having: `&&label` is an rvalue of type `void *` whose value is the
//! *state number* the label's block was given, and `goto *e` stores that
//! number and goes round the dispatch again. A function that takes a label's
//! address is therefore always lowered through the state machine.
//!
//! ```
//! cinrs::c99! {
//!     int run(const int *code, int n) {
//!         static void *table[] = { &&push, &&add, &&halt };
//!         int stack[8], sp = 0, pc = 0;
//!         (void) n;
//!         goto *table[code[pc]];
//!     push:
//!         stack[sp++] = code[++pc];
//!         pc++;
//!         goto *table[code[pc]];
//!     add:
//!         stack[sp - 2] += stack[sp - 1];
//!         sp--;
//!         pc++;
//!         goto *table[code[pc]];
//!     halt:
//!         return stack[sp - 1];
//!     }
//! }
//!
//! // push 3, push 4, add, halt
//! let code = [0, 3, 0, 4, 1, 2];
//! assert_eq!(unsafe { run(code.as_ptr(), 6) }, 7);
//! ```
//!
//! A label address is an *address constant*, so the dispatch table may be a
//! block-scope `static` as it is above; it goes into a `void *` variable, a
//! `?:`, or straight into `goto *`, and a label whose address is taken keeps
//! a block — and therefore a number — of its own. GCC's label difference
//! `&&a - &&b` is a constant here too. What such a value is *not* is a real
//! address: nothing may be read through it, arithmetic on one only means
//! anything inside its own function, and `&&label` naming a label of an
//! **enclosing** function — GCC's nonlocal label address, which needs a frame
//! pointer a lifted [nested function](#nested-functions) does not have — is a
//! located error.
//!
//! # The preprocessor
//!
//! A full C99 preprocessor runs before the parser: object-like and
//! function-like macros with `#`, `##`, `...`/`__VA_ARGS__` and the standard's
//! rescanning rules; `#define`, `#undef`, `#if`, `#ifdef`, `#ifndef`, `#elif`,
//! `#else`, `#endif`, `#include`, `#error`, `#warning`, `#pragma` and
//! [`#line`](#line).
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
//! entry point: `199901L`, `201112L`, `201710L` or `202311L`, and undefined
//! in [`c89!`] and [`gnu89!`], which is what C89 as published had), the four
//! `__STDC_NO_*` subsetting macros, `__cinrs__`, `__FILE__` and `__LINE__` —
//! which name the *`.rs` file* and the line in it, so that they point where the
//! user is looking — plus `__DATE__`, `__TIME__` and `__TIMESTAMP__` as fixed
//! placeholders, because a build has to give the same output twice. GNU adds
//! `__GNUC__` and friends, `__VERSION__`, `__STRICT_ANSI__` (strict entry
//! points only), `__BASE_FILE__`, `__FILE_NAME__`, `__INCLUDE_LEVEL__` and
//! `__COUNTER__`. On top of those comes a short set of target description
//! macros (`__x86_64__`, `__linux__`, `__unix__`, `__LP64__`,
//! `__SIZEOF_INT__`, `__BYTE_ORDER__`, …) taken from the machine the code is
//! being compiled for. Nothing claims to be Clang.
//!
//! `__has_include`, `__has_include_next`, `__has_attribute`,
//! `__has_c_attribute`, `__has_builtin`, `__has_feature` and `__has_extension`
//! are answered from this crate's own tables, so a program that guards a
//! construct with one is told the truth about *this* implementation.
//!
//! ## `#line`
//!
//! `#line 100` and `#line 100 "generated.c"` do what C99 6.10.4 says: the line
//! after the directive is line 100 and counts up from there, and `__FILE__` is
//! the given name until the next directive or the end of that file. The
//! macro-expanded form works too, and so does the `# 100 "generated.c" 1 3 4`
//! line marker GCC writes in place of one — its flags describe an `#include`
//! that happened in whatever produced the text, so they are read and dropped.
//! The numbering is per file, so a `#line` inside a header ends with the
//! header, and in the macro's own text it replaces the `.rs`-line convention
//! above from the next line to the end of the block.
//!
//! **Only `__LINE__` and `__FILE__` move.** A diagnostic — this crate's or
//! `rustc`'s — still points at the token that was really written, in the file
//! it was really written in, because that is the position the user can look
//! at. `__FILE_NAME__` is `__FILE__` without the directory and follows it;
//! `__BASE_FILE__` names the file the translation unit started in and does
//! not.
//!
//! ```
//! cinrs::c99! {
//!     #line 100 "generated.c"
//!     int cinrs_doc_where(void) { return __LINE__; }
//!     const char *cinrs_doc_what(void) { return __FILE__; }
//! }
//!
//! assert_eq!(unsafe { cinrs_doc_where() }, 100);
//! assert_eq!(
//!     unsafe { core::ffi::CStr::from_ptr(cinrs_doc_what()) }.to_str(),
//!     Ok("generated.c"),
//! );
//! ```
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
//! `cinrs` ships its own `<assert.h>`, `<complex.h>`, `<ctype.h>`,
//! `<errno.h>`, `<float.h>`,
//! `<inttypes.h>`, `<iso646.h>`, `<limits.h>`, `<math.h>`, `<signal.h>`,
//! `<stdalign.h>`, `<stdarg.h>`, `<stdatomic.h>`,
//! `<stdbool.h>`, `<stddef.h>`, `<stdint.h>`, `<stdio.h>`, `<stdlib.h>`,
//! `<stdnoreturn.h>`, `<string.h>`, `<threads.h>`, `<time.h>`, `<uchar.h>`,
//! `<wchar.h>` and `<wctype.h>`,
//! and reads the platform's only when a unit
//! [asks for them](#system-headers). A real `<stdio.h>` is not plain C —
//! glibc's is built out of GNU extensions, compiler builtins and `__asm__`
//! renaming, and its layouts are the host's rather than the target model's.
//! The bundled ones declare what the platform's
//! C library really exports, in plain C99, and the calls link against the real
//! implementation. `<setjmp.h>` is there too, as a header that says
//! `setjmp`/`longjmp` are not supported; the *call* is refused as well,
//! whichever header declared it, since the platform's own `<setjmp.h>` is one
//! [`system_include`](#system-headers) away and declares them as ordinary
//! functions. Declaring them, and declaring a `jmp_buf`, are both fine.
//!
//! Five headers that are not C's are bundled beside them, because a small
//! program reaches for them and none of the five needs a type with a layout:
//! `<alloca.h>`, `<sys/types.h>` (the system's `typedef` names — `ssize_t`,
//! `off_t`, `pid_t`, `mode_t` and the rest, each spelled the way the target's
//! own library spells it), `<unistd.h>` (`read`, `write`, `close`, `lseek`,
//! the process identity, `sleep`, `_exit`), `<fcntl.h>` (`open` and the `O_*`
//! flags, at the target's own values) and `<strings.h>` (`strcasecmp`,
//! `bzero`, `bcopy`, `index`, `ffs`). The last three are POSIX and say so with
//! an `#error` when the target is Windows, whose C runtime has no header of
//! any of those names. Anything with a layout — `struct stat`, `sigset_t`,
//! `fd_set`, `pthread_t` — is deliberately absent: a header that guessed at
//! one of those would corrupt memory rather than fail to compile. Those come
//! from the machine's own headers instead; see [System
//! headers](#system-headers).
//!
//! `<signal.h>` is C's, and its signal *numbers* are the platform's: the six
//! the standard requires plus the POSIX ones, at Linux's or the BSD/Apple
//! values, or the small set the Microsoft C runtime has. `sigaction` and
//! `sigset_t` are absent for the layout reason above.
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
//! `<wchar.h>` and `<wctype.h>` make `wchar_t` an `int`, which is what the
//! front end gives `L'x'` and `L"…"` on every target and what the Unix
//! platforms do; a header that said otherwise would hand the library a pointer
//! of the wrong type. Wide characters are the one place `cinrs` is knowingly a
//! Unix compiler — the Microsoft library's `wchar_t` is 16 bits wide.
//! `mbstate_t` *is* spelled the way each platform's library lays it out, since
//! a program declares one and passes its address to `mbrtowc`. Writing `L"…"`
//! needs [string-literal input](#input-forms), which is where every prefixed
//! literal lives.
//!
//! `<uchar.h>` is the Unicode half of the same story: `char16_t` and
//! `char32_t` are `unsigned short` and `unsigned int` — the types the front
//! end gives `u"…"` and `U"…"` — `char8_t` joins them in C23, and
//! `mbrtoc16`/`c16rtomb`/`mbrtoc32`/`c32rtomb` are declared and link. None of
//! the three names is a keyword in C, so all of them are ordinary typedefs and
//! `_Generic` cannot tell one from its underlying type, exactly as it cannot
//! for `wchar_t`.
//!
//! ```
//! cinrs::c99! { r#"
//! #include <wchar.h>
//! int cinrs_doc_wide(void) { return (int)wcslen(L"hello") + (wcscmp(L"a", L"a") == 0); }
//! "# }
//!
//! assert_eq!(unsafe { cinrs_doc_wide() }, 6);
//! ```
//!
//! Anything else — POSIX beyond those five, a third-party library, your own
//! project's headers — is written out as C declarations by hand, pointed at
//! with an include path, or taken from the machine itself with
//! [`#pragma cinrs system_include`](#system-headers).
//!
//! ## System headers
//!
//! The platform's own directories — `/usr/include` and its like — are **not
//! searched by default**, which is what makes a `c99!` block self-contained
//! and portable across [target models](#the-data-model). One pragma turns them
//! on, for the types no bundled header can honestly declare: `struct stat`,
//! `DIR`, `pthread_mutex_t`, `regex_t`, `struct utsname`, the real `FILE`.
//!
//! ```
//! # #[cfg(all(target_os = "linux", target_env = "gnu"))]
//! # fn main() {
//! cinrs::gnu11! {
//!     #pragma cinrs system_include
//!
//!     #include <sys/utsname.h>
//!
//!     int kernel_name_length(void) {
//!         struct utsname u;
//!         int n = 0;
//!         if (uname(&u) != 0) return -1;
//!         while (u.sysname[n] != '\0') n++;
//!         return n;
//!     }
//! }
//!
//! assert!(unsafe { kernel_name_length() } > 0);
//! # }
//! # #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
//! # fn main() {}
//! ```
//!
//! With the switch on the search order becomes: the including file's own
//! directory, the configured directories, the **bundled** headers, then the
//! **platform's** — so a name cinrs carries still comes from cinrs, and only
//! what it does not carry comes from the machine.
//! `#pragma cinrs system_include first` swaps the last two, which is how a
//! program asks for the platform's `<stdio.h>` and so for a `FILE` whose
//! `sizeof` is a number. `CINRS_SYSTEM_INCLUDE=1` (or `=first`) sets the same
//! switch for a whole crate, and a pragma in a unit overrides it.
//!
//! A header *found* in one of the platform's directories resolves its own
//! `#include`s there first, whichever mode is in force, which is what keeps
//! that header set self-consistent: glibc's `<pthread.h>` gets glibc's
//! `<time.h>`, and there is one `struct timespec` rather than two.
//!
//! The directories are `CINRS_SYSTEM_INCLUDE_PATH` when it is set, and
//! otherwise, on Linux, `/usr/local/include`, the multiarch directory
//! (`/usr/include/x86_64-linux-gnu` and its like, when it exists) and
//! `/usr/include`. **GCC's and Clang's own private directories are never
//! searched**: their `limits.h`, `stdint.h`, `stddef.h` and `stdarg.h` belong
//! to that compiler, chain onward with `#include_next`, and are bundled here
//! anyway. Apple's platforms and Windows have no default — the macOS SDK path
//! is knowable only from `xcrun --show-sdk-path` — and a **cross build** has
//! none either, since those directories hold the host's headers and a
//! `struct stat` laid out for another architecture is worse than none; both are
//! an error naming `CINRS_SYSTEM_INCLUDE_PATH`.
//!
//! A system header is **not** recorded for rebuilds: it belongs to the machine
//! rather than to the crate, so upgrading libc does not rebuild every unit
//! that read one. Your own headers still are.
//!
//! What those headers *declare* is their own business, and glibc decides it
//! from the feature test macros exactly as it does for GCC: a strict entry
//! point defines `__STRICT_ANSI__`, so glibc withholds everything outside C —
//! no `sigset_t`, no `struct sigaction` — while a GNU one leaves
//! `_DEFAULT_SOURCE` on and POSIX is there. `#define _GNU_SOURCE 1` ahead of
//! the first `#include` works in either.
//!
//! GNU's `#include_next` works with GCC's semantics — the search goes on from
//! the entry *after* the one the current file was found under — because the
//! platform's headers use it; so does `__has_include_next`.
//!
//! [`doc/system-headers.md`][system-headers] in the repository has the table:
//! every standard and POSIX header, both entry points, what passes and why the
//! one that does not does not.
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
//! platform splits `PATH` and searched last —
//! `#pragma cinrs system_include` adds the platform's own directories after
//! that, and is its own [section](#system-headers).
//! `#pragma cinrs link "name"` puts
//! `#[link(name = "name")]` on the generated `extern` block, for a program
//! that calls into a library the Rust runtime does not already link. The other
//! six `cinrs` pragmas are [`target`](#the-data-model),
//! [`export`](#linking-two-blocks-together), [`safe`](#safe-functions),
//! [`no_std`](#no_std),
//! [`module`](#one-block-one-module) and
//! [`crate`](#the-complex-feature), which says where the `cinrs` crate itself
//! is for a renamed dependency. The repository's [`doc/pragmas.md`][pragmas]
//! is the reference page for all nine, and for the pragmas the preprocessor
//! itself knows — `once`, `pack`, `push_macro`, `#pragma GCC …` — and what
//! happens to one it does not.
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
//! ## Pointers to `va_list`
//!
//! `va_list *` is `*mut core::ffi::VaList<'_>`, and works as a parameter and
//! as a local — which is what lets a helper *advance the caller's list*:
//!
//! ```
//! # #[rustversion::since(1.99)]
//! # fn main() {
//! cinrs::c99! {
//!     #include <stdarg.h>
//!
//!     int two(va_list *ap) { return va_arg(*ap, int) + va_arg(*ap, int); }
//!
//!     int drive(int n, ...) {
//!         va_list ap;
//!         va_start(ap, n);
//!         int first = two(&ap);
//!         int third = va_arg(ap, int);   /* carries on where `two` stopped */
//!         va_end(ap);
//!         return first * 100 + third;
//!     }
//! }
//!
//! assert_eq!(unsafe { drive(3, 1, 2, 3) }, 303);
//! # }
//! # #[rustversion::before(1.99)]
//! # fn main() {}
//! ```
//!
//! A `va_list` local inside such a helper starts out as a copy of `*ap`, so
//! `va_copy(copy, *ap)` works there too. The lifetime `VaList` carries is
//! elided, which a function signature and a `let` can do and nothing else
//! can: a `struct` member, a file-scope object and a return type of that type
//! are each refused, with the same "only a local variable or parameter"
//! message a bare `va_list` gets there.
//!
//! ## `va_arg` of a `struct` or a `union`
//!
//! C99 allows `va_arg` at any complete object type, and Rust's `VaList` has
//! `next_arg` for the primitives only. So an aggregate is not read at its own
//! type: it is **reassembled from the registers the ABI passed it in**. On
//! x86-64 System V (AMD64 psABI 3.2.3) an argument of at most sixteen bytes is
//! split into one or two *eightbytes*, each classified INTEGER or SSE from the
//! members that overlap it — one integer member anywhere in an eightbyte makes
//! the whole of it INTEGER — and each passed in one register of that file. A
//! `va_arg` therefore becomes one `next_arg::<u64>()` per INTEGER eightbyte
//! and one `next_arg::<f64>()` per SSE one, gathered into a `[u64; N]` and read
//! back out of it:
//!
//! ```
//! # #[rustversion::since(1.99)]
//! # fn main() {
//! cinrs::c99! {
//!     #include <stdarg.h>
//!
//!     /* Eight bytes of `double` then four of `int`: SSE, then INTEGER. */
//!     struct Reading { double value; int sensor; };
//!
//!     double total(int n, ...) {
//!         va_list ap;
//!         double sum = 0;
//!         va_start(ap, n);
//!         for (int i = 0; i < n; i++) {
//!             struct Reading r = va_arg(ap, struct Reading);
//!             sum += r.value * r.sensor;
//!         }
//!         va_end(ap);
//!         return sum;
//!     }
//! }
//!
//! let a = Reading { value: 1.5, sensor: 2 };
//! let b = Reading { value: 0.25, sensor: 4 };
//! assert_eq!(unsafe { total(2, a, b) }, 4.0);
//! # }
//! # #[rustversion::before(1.99)]
//! # fn main() {}
//! ```
//!
//! A `union` follows the same rule, classified per member since they all start
//! at offset zero; a bit-field counts as the integer its storage is; a member
//! that is itself a `struct`, a `union` or an array flattens to its scalars;
//! and `__int128` is fine *inside* a record — nothing reads it at its own type,
//! only as the two integer eightbytes it is.
//!
//! Three things are refused by name rather than translated with the wrong
//! rules. A record **larger than sixteen bytes**, or one with a member the
//! packing has moved off its own type's alignment, is class MEMORY: the caller
//! pushes it into the *overflow area*, which nothing in the stable `VaList` API
//! can reach. Any [target](#the-data-model) that is not x86-64 System V —
//! the Microsoft x64 ABI passes an aggregate over eight bytes *by pointer*,
//! AArch64 has homogeneous float aggregates, i686 puts everything on the stack
//! — says so instead of guessing.
//!
//! And one edge is worth knowing rather than refusing. The eightbytes are read
//! one at a time, while the ABI decides register-versus-stack for the argument
//! *as a whole*: they agree unless a two-eightbyte record is the very argument
//! that exhausts the register save area — five integer or seven SSE eightbytes
//! into the list — where the caller pushes the whole record onto the stack and
//! reading its first eightbyte still finds a register. A record of at most
//! eight bytes is a single eightbyte and is therefore always exact. `long
//! double` is another: this crate maps it to `double` throughout, so a member
//! of that type is classified SSE, where a real `long double` would be X87 and
//! would put the whole record in memory.
//!
//! `tests/vaarg_structs.rs` checks the classification against the host's own C
//! compiler, on a generated corpus of a hundred and fifty records read through
//! four argument shapes each.
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
//! C that the Rust lexer accepts can be written as raw tokens — including an
//! [extended identifier](#standards) written as the character itself, since
//! Rust's identifiers are Unicode Annex #31's too. C that it does
//! not accept (hexadecimal floating constants such as `0x1.8p3`,
//! multi-character character constants, prefixed literals like `L"…"`,
//! `u8"…"`, `u"…"`, `U"…"` and `u8'x'` — Rust reserves those prefixes —
//! a universal character name (`é`),
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
//! The third form is a file of its own, which has no such limits either; see
//! [Including a C file](#including-a-c-file).
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
//! # Including a C file
//!
//! The third input form is a file: [`include_c99!`] and one such macro per
//! entry point — [`include_c89!`], [`include_c90!`], [`include_c11!`],
//! [`include_c17!`], [`include_c23!`] and the five `include_gnu…!` forms.
//!
//! ```text
//! cinrs::include_c99!("c/geometry.c");
//!
//! let p = Point { x: -3, y: 4 };
//! assert_eq!(point_manhattan(p), 7);
//! ```
//!
//! The file is read while the macro expands and translated exactly as the same
//! text inside a [`c99!`] block would be. It is one translation unit, so every
//! construct is accepted — a file is text, and the lexemes Rust's own lexer
//! refuses (`0x1.8p3`, `L"…"`, `\` continuations, `##`) are no trouble there —
//! `#pragma cinrs …` written inside it configures the unit, `#include "…"` in
//! it searches **its own** directory first as a header's does, and the
//! expansion is [a module plus a glob re-export](#one-block-one-module) like
//! any other invocation's. `__FILE__` and `__LINE__` name the `.c` file and its
//! own lines, and the file is mentioned with `include_str!` in the expansion,
//! so editing it rebuilds the crate exactly as editing a header does.
//!
//! A **relative path is resolved against the directory of the `.rs` file the
//! macro is written in**, which is the rule `#include "…"` already follows; an
//! absolute path is used as it stands. `Span::local_file` is how that directory
//! is found, and where the compiler will not say — input built by another
//! macro, some IDE contexts — `CARGO_MANIFEST_DIR` stands in, so a path written
//! relative to the package still resolves.
//!
//! ## Where the errors go
//!
//! There is no C in the `.rs` file, so there is no span to point into: **every
//! diagnostic lands on the macro invocation**. A message of this crate's
//! carries the position inside the file in its text, exactly as one from inside
//! an `#include`d header does:
//!
//! ```text
//! error: c/geometry.c:12:5: use of undeclared identifier 'wrong'
//!  --> src/lib.rs:3:21
//!   |
//! 3 | cinrs::include_c99!("c/geometry.c");
//!   |                     ^^^^^^^^^^^^^^
//! ```
//!
//! and so does an error `rustc` raises about the generated code. **`cargo` and
//! `rust-analyzer` show the location in the message rather than by jumping into
//! the `.c` file** — that is the price of keeping the C in a file of its own,
//! and it is the only difference from writing the same text inline. A call
//! written in *Rust* is unaffected: the caret is on the call, where it always
//! was.
//!
//! # Status
//!
//! The front end (lexer, preprocessor, parser, diagnostics) is complete for
//! C99, and most of the language is translated end to end: all the arithmetic
//! types, pointers, arrays, `struct`, `union`, `enum`, bit-fields, `typedef`,
//! string
//! literals, function pointers, `sizeof` with real layout, casts, aggregate
//! and designated initialisers — designator *lists* (`{ .a.b = 1 }`) and all
//! — compound literals, file-scope, `static` and
//! `extern` objects,
//! functions (including `static`, `inline` and variadic ones), every operator,
//! every control structure — `if`, `while`, `do`/`while`, `for`, `switch` with
//! fallthrough, `break`, `continue`, `return` and `goto` — and the whole
//! preprocessor, `#include` and the bundled standard headers included. What
//! C11 and C23 added on top of that is listed under [Standards](#standards),
//! entry point by entry point.
//!
//! On top of that come the [GNU extensions](#gnu-extensions), which real C
//! leans on: statement expressions, `typeof`, `__attribute__`, `#pragma pack`,
//! the `__builtin_*` family, case ranges, flexible array members, `alloca`,
//! [nested functions](#nested-functions), [`__int128`](#__int128),
//! [`__thread`](#thread-local-objects) and the rest.
//!
//! [Variably modified types](#variably-modified-types-and-alloca) are there —
//! `int a[n]`, `double a[n][m]`, `int (*p)[n]`, `typedef int T[n];` and the
//! parameter forms — emulated on the heap, and so are
//! [complex numbers](#complex-numbers), with Annex G.5.1's arithmetic and
//! `<complex.h>`.
//!
//! Deliberately never: `setjmp`/`longjmp`, `_BitInt`, `_Imaginary`, a complex
//! *integer* type, an `_Atomic` aggregate,
//! C23's *named* universal character `\N{…}`, inline assembly, and
//! `long double`'s extended precision (it is `double`, with the ABI that
//! implies). Each of them is a clear, located error rather than a silent
//! mistranslation.
//!
//! The other things worth knowing before reaching for this crate: everything
//! generated is `core`-only except the storage a variably modified object or
//! `alloca` needs, which is a `Vec`, a [thread-local
//! object](#thread-local-objects), which is a `std::thread_local!`, and a
//! [complex value](#complex-numbers), which is `cinrs-rt`'s (itself
//! `#![no_std]`) — see [`no_std`](#no_std); the
//! platform's include directories are not searched unless a unit asks for them
//! with [`#pragma cinrs system_include`](#system-headers), so by default
//! anything outside the bundled headers is declared by hand or pointed at with
//! an include path;
//! `va_list` is [`core::ffi::VaList`], which cannot be stored in a `struct` or
//! returned; and sizes and alignments come from a
//! [model of the target](#the-data-model) rather than from its own C
//! compiler.
//!
//! # The data model
//!
//! `sizeof`, `_Alignof`, member offsets, bit-field storage, the type an
//! integer constant gets, whether `-1 < 1u`, the value of an `#if`, the
//! predefined macros and therefore which branch each bundled header takes are
//! all worked out while the macro is expanding, from a model of the machine
//! the code will *run* on: the widths of `short`, `int`, `long`, `long long`
//! and a pointer, the signedness of plain `char`, how strictly `long long` and
//! `double` are aligned, how wide `wchar_t` is, the byte order, and whether
//! `__int128` exists at all.
//!
//! A procedural macro cannot ask `rustc` what it is compiling for — `--target`
//! is not part of a macro's world — so the model is chosen, in this order:
//!
//! 1. **`#pragma cinrs target "<triple>"`** written in the unit itself;
//! 2. the **`CINRS_TARGET`** environment variable, which the crate being built
//!    sets from its own build script;
//! 3. otherwise the machine the macro itself was compiled for, the *host*.
//!
//! ## Cross-compiling
//!
//! One line in a build script is the whole recipe:
//!
//! ```rust,ignore
//! // build.rs
//! fn main() {
//!     println!(
//!         "cargo:rustc-env=CINRS_TARGET={}",
//!         std::env::var("TARGET").expect("Cargo sets TARGET for a build script")
//!     );
//! }
//! ```
//!
//! `cargo:rustc-env` reaches the very `rustc` process that runs the macro, and
//! Cargo makes the value part of the crate's fingerprint, so changing
//! `--target` rebuilds. With it, `cargo build --target i686-unknown-linux-gnu`
//! translates the C for a 32-bit machine — a four-byte `long`, `2147483648`
//! typed as `long long`, `long long` and `double` aligned to four bytes and
//! laid out accordingly — and `--target x86_64-pc-windows-msvc` for one where
//! `long` is four bytes, `wchar_t` is two and `<stdio.h>` declares the
//! Microsoft streams.
//!
//! A unit may override it for itself:
//!
//! ```c
//! #pragma cinrs target "aarch64-unknown-linux-gnu"
//! ```
//!
//! which has to be a directive in that unit's own text and has to come before
//! any `#include` or `#if`: the model is settled before the first directive is
//! read, so a pragma after one would be a lie, and it is reported rather than
//! half-applied.
//!
//! The families, and what each one refuses — `__int128` on a 32-bit
//! architecture, a bit-field on a big-endian one — are tabulated in the
//! repository's [`doc/c-status.md`][c-status].
//!
//! ## The assertion that guards it
//!
//! Any of the three choices may still be the wrong one, so **every expansion
//! states the model it was translated for**, as a
//! `const _: () = { assert!(…); };` block at the top of the unit's module,
//! written over the [`core::ffi`] aliases, which follow the *real* target:
//!
//! ```text
//! const _: () = {
//!     assert!(::core::mem::size_of::<::core::ffi::c_long>() == 8, "cinrs: …");
//!     assert!(::core::ffi::c_char::MIN != 0, "cinrs: …");
//!     assert!(::core::mem::align_of::<::core::ffi::c_longlong>() == 8
//!             && ::core::mem::align_of::<::core::ffi::c_double>() == 8, "cinrs: …");
//!     // … and one for `short`, `int`, `long long` and a pointer.
//! };
//! ```
//!
//! A mismatch is therefore a failed compile-time assertion with the caret on
//! the C, rather than a program that computes the wrong thing — and its
//! message names both the model cinrs used and the knob that would change it:
//!
//! ```text
//! error[E0080]: evaluation panicked: cinrs: 'long' is 8 bytes in the data model
//! this unit was translated for, and is not on this target. Translated for LP64
//! (x86_64-linux, signed 'char', 32-bit 'wchar_t'), chosen from the host,
//! CINRS_TARGET being unset; set CINRS_TARGET from a build script
//! (cargo:rustc-env=CINRS_TARGET=$TARGET) or write #pragma cinrs target.
//! ```
//!
//! `__int128`'s alignment is asserted only in a unit that has one, it being
//! the only scalar whose alignment does not follow from its width.
//!
//! What the model cannot check is the C *library* on the other end: a bundled
//! header declares what the target's library is expected to export, and
//! nothing here can confirm it. Every target but the host is compiled for and
//! not run.
//!
// The documents under `doc/` are not part of the published crate, so the links
// to them are absolute: there is no `doc/` directory next to this page on
// docs.rs.
//!
//! [gnu-extensions]: https://github.com/tanakh/cinrs/blob/master/doc/gnu-extensions.md
//! [pragmas]: https://github.com/tanakh/cinrs/blob/master/doc/pragmas.md
//! [system-headers]: https://github.com/tanakh/cinrs/blob/master/doc/system-headers.md
//! [c-status]: https://github.com/tanakh/cinrs/blob/master/doc/c-status.md

#![warn(missing_docs)]
#![no_std]

pub use cinrs_macros::{c11, c17, c23, c89, c90, c99, gnu11, gnu17, gnu23, gnu89, gnu99};
pub use cinrs_macros::{
    include_c11, include_c17, include_c23, include_c89, include_c90, include_c99, include_gnu11,
    include_gnu17, include_gnu23, include_gnu89, include_gnu99,
};

/// The runtime the generated code links against; see [`cinrs_rt`].
///
/// It holds one thing — [`rt::Complex`], which is
/// [`num_complex::Complex`](https://docs.rs/num-complex), and C's arithmetic
/// on it — and exists only because a `_Complex` value has to have a type that
/// other crates already speak and because Annex G's multiplication and
/// division are too big to write out per expansion. Everything else `cinrs`
/// generates names `core` alone.
///
/// The generated code spells this path in full, so a renamed dependency needs
/// `#pragma cinrs crate "<path>"` to say where to find it.
///
/// Present only with the `complex` feature, which is on by default.
#[cfg(feature = "complex")]
pub use cinrs_rt as rt;
