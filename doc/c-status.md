# C standard support status

Modelled on Clang's [C status page](https://clang.llvm.org/c_status.html): one
row per feature (with its WG14 paper number where Clang lists one) and cinrs's
status. Rows that are pure wording changes, library-only, or about optimisation
freedom are kept so the table can be checked against Clang's, and marked N/A.

Statuses:

* 🟢 **Yes** — implemented and tested.
* 🟡 **Partial** — implemented with a documented gap (the note says which).
* 🟢 **Accepted** — parsed and ignored where the standard allows an implementation
  to ignore it (attributes, hints, pragmas).
* 🔴 **No** — not implemented; a clear diagnostic is produced where the construct
  can be recognised.
* ⚪ **Unverified** — believed to work but not covered by a test yet; treat as a
  to-do for the test suite.
* ⚪ **N/A** — no implementation work is involved (wording, library semantics we
  inherit from libc/Rust, or freedom we do not exercise).

Entry points: `c89!` (`c90!` is the same macro), `c99!`, `c11!`, `c17!`,
`c23!`. A feature of a newer standard used in an older entry point is rejected
with `requires C99/C11/C23 or later`. The GNU entry points — `gnu89!`,
`gnu99!`, `gnu11!`, `gnu17!`, `gnu23!` — accept those features instead, exactly
as `gcc -std=gnu99` does, and add the GNU extensions on top;
[`doc/gnu-extensions.md`](gnu-extensions.md) is the catalogue for both.

`c89!` is the one that gates *backwards*, and the C99 column below doubles as
its list: `//` comments, mixed declarations and code, a declaration in a `for`
clause, variable length arrays, `_Bool`, `restrict`, `inline`, `long long`,
designated initializers, compound literals, variadic macros, flexible array
members, hexadecimal floating constants, `__func__`, `_Pragma`, universal
character names, a trailing comma in an enumerator list, `static` and `[*]` in
an array parameter declarator, `_Complex` and an imaginary constant (`2.0i`)
are each `requires C99 or later (this block is c89!)`. The *library* additions are not gated — a bundled header
is a set of declarations, and `snprintf` is one of them; the C99 declarations
that need a C99 *type* carry `__extension__`, which switches the gate off for
the declaration it is written on, exactly as glibc's headers do, so
`#include <stdlib.h>` in a `c89!` block still declares `llabs`. `__inline` and
`__restrict`, being reserved spellings, work there as they do everywhere else.
What `gnu89!` keeps of C89 is only what a later revision **deleted**: implicit
`int`, implicit function declarations, and (with every entry point below
`c23!`) old-style function definitions.

One of the rows below is answered by the *entry point* rather than by the
front end: `__STDC_NO_THREADS__` is predefined, so the feature it names is a
conforming omission rather than a gap. It is the conservative half of a
feature that partly works — it stays defined although `_Thread_local` does,
because `<threads.h>` is what the macro is about. `__STDC_NO_ATOMICS__` and
`__STDC_NO_VLA__` are *not* defined: atomics and variably modified types are
implemented.

`__STDC_NO_COMPLEX__` is the one that depends on how the crate was built. The
complex types need a *runtime* type — `cinrs::rt::Complex`, which is
[`num_complex::Complex`](https://docs.rs/num-complex) — so they live behind
`cinrs`'s `complex` feature. It is **on by default**, and then the macro is not
defined and everything in the `<complex.h>` row below is there;
`default-features = false` drops the dependency, predefines the macro, and
makes `_Complex` a diagnostic that says which feature to turn on.
`__STDC_IEC_559_COMPLEX__` is never defined either way: `cinrs` implements
Annex G.5.1's arithmetic without claiming the rest of the annex.

## The target model

Almost every row below is answered against a *model* of the machine rather than
against the machine: `sizeof`, `_Alignof`, member offsets, bit-field storage,
the type an integer constant gets, whether `-1 < 1u`, the value of an `#if`,
the predefined macros and therefore which branch each bundled header takes are
all computed while the macro is expanding, from `cinrs_core::TargetModel`.

### Choosing it

A procedural macro cannot ask `rustc` what it is compiling for, so the model is
chosen, in this order:

1. **`#pragma cinrs target "<triple>"`** written in the unit itself;
2. the **`CINRS_TARGET`** environment variable, which the crate being built
   sets from its own build script —
   `println!("cargo:rustc-env=CINRS_TARGET={}", std::env::var("TARGET").unwrap());`
   — because `cargo:rustc-env` reaches the very `rustc` process that runs the
   macro;
3. otherwise the machine the macro itself was compiled for, the *host*.

The pragma is read *before* preprocessing — the predefined macros are built
from the model, so a pragma handled where it stood would come too late — which
gives it three rules, each of them a diagnostic rather than a silent
half-measure: it has to be a directive in the unit's own text (not in a header,
and not through `_Pragma`), it has to come before every `#include` and `#if`,
and two of them have to name the same triple. The one case the two halves
cannot agree on is a pragma inside a group `#if 0` goes on to skip: the scan
has already read it, so it applies, and nothing is left to complain. A `target`
pragma does not belong in a conditional.

Whichever it was, **every expansion states the model it was translated for**,
as a `const _: () = { assert!(…); };` block at the top of the unit's module:
one assertion per width, one for plain `char`'s signedness, one for the
alignment of `long long` and `double`, and — in a unit that has one — one for
`__int128`'s, all over the `core::ffi` aliases, which follow the real target. A
wrong choice, or a forgotten build script on a cross build, is therefore a
failed compile-time assertion whose message names both the model cinrs used and
the knob that chose it, rather than a program that computes the wrong thing.
`tests/cross_targets.rs` compiles a small crate for each installed target with
and without the variable and checks exactly that.

### The table

The triple is read the way `rustc` writes one — `arch-vendor-os-env`, or
`arch-os-env` — and the first component picks the architecture row. An
architecture or an operating system that is not here is an error naming the
families that are, never a guess.

| Architecture | Pointer | `long` | Endian | `__int128` | `align(long long, double)` | Macros |
| --- | --- | --- | --- | --- | --- | --- |
| `x86_64*` (incl. `gnux32`, where the pointer is 32) | 64 (32) | LP64 rule | little | yes | 8 | `__x86_64__`, `__amd64__` |
| `i386`/`i486`/`i586`/`i686` | 32 | 32 | little | no | **4 off Windows**, 8 on it | `__i386__` |
| `aarch64*` (`aarch64_be` big; `gnu_ilp32` 32-bit pointer) | 64 (32) | LP64 rule | little | yes | 8 | `__aarch64__` |
| `arm*`, `thumb*` (`armeb*` big) | 32 | 32 | little | no | 8 | `__arm__` |
| `riscv32*` / `riscv64*` | 32 / 64 | LP64 rule | little | 64-bit only | 8 | `__riscv`, `__riscv_xlen` |
| `wasm32` | 32 | 32 (64 on a Linux ABI) | little | no | 8 | `__wasm__`, `__wasm32__` |
| `powerpc` / `powerpc64` / `powerpc64le` | 32 / 64 | LP64 rule | big except `le` | 64-bit only | 8 | `__powerpc__`, `__PPC64__` |
| `s390x` | 64 | 64 | **big** | yes | 8 | `__s390x__` |
| `mips*` / `mips64*` (`*el` little) | 32 / 64 | LP64 rule | big except `el` | 64-bit only | 8 | `__mips__` |
| `sparc` / `sparc64`, `sparcv9` | 32 / 64 | LP64 rule | **big** | 64-bit only | 8 | `__sparc__` |
| `loongarch64` | 64 | 64 | little | yes | 8 | `__loongarch__` |

"LP64 rule" is `long` = 64 on a 64-bit pointer **unless the system is
Windows**, which is LLP64 and keeps a 32-bit `long`.

| Operating system | Macros | `wchar_t` | `wint_t` | `time_t` | Object format |
| --- | --- | --- | --- | --- | --- |
| `linux` (incl. `android`; any libc) | `__linux__`, `__gnu_linux__`, `__unix__` | 32-bit `int`, unsigned on Arm | `unsigned int` | `long` | `__ELF__` |
| `darwin`/`macos`/`ios`/`tvos`/`watchos`/`visionos` | `__APPLE__`, `__MACH__`, `__unix__` | 32-bit `int` | `int` | `long` | Mach-O |
| `windows` (`msvc` and `gnu`) | `_WIN32`, `_WIN64` at 64 bits | **16-bit `unsigned short`** | `unsigned short` | `long long` | PE |
| `freebsd`, `netbsd`, `openbsd` | `__FreeBSD__`/`__NetBSD__`/`__OpenBSD__`, `__unix__` | 32-bit `int` | `unsigned int` | `long` | `__ELF__` |
| `wasi` | `__wasi__` | 32-bit `int` | `unsigned int` | `long` | wasm |
| `none` (and `arch-unknown-unknown`) | — | 32-bit `int` | `unsigned int` | `long` | `__ELF__` off wasm |

**The signedness of plain `char` follows `core::ffi::c_char`, not the
architecture alone.** That is the rule, because the generated code uses that
alias and a model that disagreed would fail its own assertion: unsigned on
AArch64, Arm, PowerPC, RISC-V and s390x — *except* on Windows and on Apple's
platforms, which make it signed whatever the machine is — and signed everywhere
else, LoongArch and **wasm32** included.

`short` is 16 bits, `int` 32, `long long` 64 and `double` 64 on every target
here; `long double` is `double`; `intmax_t` is 64 bits everywhere, so an ILP32
target has `__INTMAX_TYPE__` of `long long int` while LP64 has `long int`.
`size_t`, `ptrdiff_t` and `intptr_t` are the narrowest standard type as wide as
a pointer, exactly as GCC picks them.

### What a target refuses

* **`__int128` on a 32-bit architecture.** GCC has the type on the 64-bit ones
  only and refuses it rather than emulating it; so does this, and
  `__SIZEOF_INT128__` is left undefined there so that a program can guard on it.
  The x32 ABI keeps the type, the machine still being x86-64.
* **A bit-field on a big-endian target.** Where a bit-field's bits sit inside
  its storage unit is implementation defined, and cinrs allocates from the
  least significant end — which is what GCC and Clang do on a little-endian
  machine and the opposite of what they do on a big-endian one. Refusing beats
  laying one out the wrong way round, since nothing in the generated Rust would
  notice.
* **An architecture whose data model is not one of the three.** `avr` (16-bit
  `int`, 32-bit `double`) and `msp430` are named in the diagnostic rather than
  silently approximated.

### Where it does *not* reach

The C library is still the platform's. A bundled header declares the functions
and spells the types the target's library really uses — the branches above are
what `errno`, the standard streams, `mbstate_t`, `struct tm` and `FILE` are
picked with — but nothing here can check that the library on the other end
agrees. The Windows branch keeps to the portable UCRT subset
(`__acrt_iob_func` for the three streams, `_errno()` for `errno`), which both
the Microsoft library and mingw-w64 export; it is compiled for in
`tests/cross_targets.rs` and has not been *run*. The same is true of every
target but the host.

## C99

| Feature | Paper | cinrs | Notes |
| --- | --- | --- | --- |
| Restricted character set support via digraphs and `<iso646.h>` | | 🟢 Yes | Digraphs are lexed, `<iso646.h>` is bundled, and the nine **trigraphs** are replaced in translation phase 1 — before line splicing, so `??/` at the end of a line splices it, and inside string literals, so `"??!"` is `"|"`. A punctuator may be spelled with them, one or both halves: `??!??!` is `||` and `??'=` is `^=`. They are on in `c89!`, `c99!`, `c11!` and `c17!` and off in `c23!` (N2940 removed them) and in every GNU dialect, which is the line `gcc -std=c99` and `clang` both draw. |
| More precise aliasing rules via effective type | | ⚪ N/A | |
| Restricted pointers (`restrict`) | N448 | 🟢 Accepted | Parsed and ignored, as the standard permits — Rust's own aliasing rules are stricter than the promise, so there is nothing to pass on. The one *constraint* it carries is checked: 6.7.3p2 lets it qualify only a pointer to an object type, so `int restrict i` and `void (*restrict fp)(void)` are diagnosed, while `int *restrict p`, `int_ptr restrict q` through a `typedef` of a pointer, and `void f(int a[restrict])` are not. Clang's `C99/n448.c` is that test. |
| Variable length arrays | N683 | 🟢 Yes | Every variably modified type at block scope: `T a[n];`, `double a[n][m]`, `int a[3][n]` and `int a[n][3]`, `int (*p)[n]`, `typedef int T[n];`, and the parameter forms `void f(int n, int m, double a[n][m])`, `double (*a)[m]` and `int a[*][*]` in a prototype. Each variable dimension gets a hidden `size_t` object, created where the *type* is declared and named by the type itself: a `typedef` evaluates its bound once, at the `typedef` (6.7.7p4), and the objects declared with it share it; a definition's parameter evaluates its bounds on entry, in declaration order (6.9.1p10). `sizeof a`, `sizeof a[0]`, `sizeof *p` and `sizeof(int[n][m])` are products of those; `a[i][j]`, `p + 1` and `p - q` scale by them. The object's lifetime is the block, a declaration inside a loop allocates afresh on every pass, and a jump into the scope of one is diagnosed (6.8.6.1p1, 6.8.4.2p2). Emulated on the heap — the whole object, however many dimensions, is one `Vec` — so the storage is not the stack; that and the `alloc` dependency are the only differences a program can observe. The one gap: a bound written in a *type name* other than `sizeof`'s — `(double (*)[m])q` — has no object to live in, so an expression that needs the length is refused ("the length of this variably modified type is not available here"). `__STDC_NO_VLA__` is **not** predefined; see the [N1460 row](#c11). |
| Flexible array members | | 🟢 Yes | A `[T; 0]` tail member; `sizeof` leaves it out and indexing it is pointer arithmetic. Initialising one — which GCC allows with a warning — is refused. |
| Incomplete array types (6.2.5p22, 6.9.2p5) | | 🟢 Yes | `int j[];` is a *type*, not a mistake, in the two places C allows an object to have one: an `extern` declaration, whose object is defined in another unit, and a file-scope tentative definition, which the end of the translation unit completes to **one element**. A later declaration with a bound completes it sooner (`extern int j[]; int j[3];` is one object of three), the composite type is what the object keeps (6.2.7p4), an incomplete array is *compatible* with every completed one of the same element type — so `__builtin_types_compatible_p(int[5], int[])` is 1 — and `sizeof` of one is a constraint violation until it is completed, exactly as GCC has it. `typedef int A[]; A a = { 1, 2 };` takes its length from the initialiser as the `int a[]` spelling does. |
| `static` and type qualifiers in parameter array declarators | | 🟢 Accepted | Parsed; no effect on codegen. The *bound* of an array parameter is not part of its type either (6.7.5.3p7), so `void f(int n, int a[n])`, `int a[*]` and `int a[static n]` all declare an `int *` — which is what makes `sizeof a` there the size of a pointer. In a *definition* the bound is still evaluated on entry, in declaration order, because C99 6.9.1p10 says so and `void f(int n, int a[n++])` can tell: the value is thrown away, and a bound that plainly has no effect is left out of the generated code. |
| Complex and imaginary support in `<complex.h>` | N620, N638, N657, N693, N694, N809 | 🟡 Partial | **Complex, yes; imaginary, no** — which is what every compiler does, and what 7.3.1p3 allows: `_Imaginary` is refused with the reason, and `_Imaginary_I` is absent. `float _Complex` and `double _Complex` are [`cinrs::rt::Complex<f32>`](https://docs.rs/num-complex) and `Complex<f64>`, two components side by side, so `sizeof` is 8 and 16 and the alignment the component's; `long double _Complex` is `double _Complex`, the same mapping `long double` has. `+ - * /` (with Annex G.5.1's infinity recovery, and componentwise where one operand is real, exactly as GCC computes them), unary `+ -`, `==`/`!=`, compound assignment, `++`/`--`, the conversions of 6.3.1.6 and 6.3.1.7, `_Bool` by 6.3.1.2, `sizeof`, `_Alignof`, `_Generic`, static initialisers and complex members and arrays all work; the relational operators, `%`, the bitwise operators and the shifts are refused with the reason, since C gives them real operands only. GNU's `__real__`, `__imag__` (lvalues when the operand is one), `~z` for the conjugate and the imaginary suffixes `2.0i`/`2.0j` are all there. `<complex.h>` bundles `complex`, `I`, `_Complex_I`, `CMPLX` and the function declarations, which link against the platform's library by value. The gaps: no complex *integer* types (`_Complex int` is a GNU extension of its own), no `va_arg` of a complex type, no `_Atomic` or bit-field of one, no `printf` conversion, and `__STDC_IEC_559_COMPLEX__` is not claimed. Needs the `complex` feature, which is on by default; see the note above. `tests/complex.rs` |
| Type-generic math macros in `<tgmath.h>` | N693 | 🔴 No | Would be forty `_Generic`s over every arithmetic type. `<math.h>` and `<complex.h>` declare the `f` and `l` forms, which is what the macros dispatch to. |
| The `long long int` type | N601 | 🟢 Yes | |
| Increase minimum translation limits | N590 | 🟢 Yes | Every minimum in C23 5.2.5.2p1 is measured by `crates/cinrs-core/tests/limits.rs` and `tests/limits.rs`, allocation traffic and all. A left-associative chain — `a, b, c, …`, `a + b + …`, `a && b && …` — is bounded by nothing but memory, which is what lets a logical source line hold the 4095 characters the clause asks for; 4000 operands are accepted and run. *Nesting* is bounded at 200 levels, three times the 63 the clause asks for and close to Clang's own `-fbracket-depth` default of 256, and the right-associative `a ? b : c ? d : e` and `a = b = c`, and a run of postfix operators, count against it because each operator really is a level. |
| Additional floating-point characteristics in `<float.h>` | | 🟡 Partial | The common `FLT_*`/`DBL_*` macros; `FLT_EVAL_METHOD`, `DECIMAL_DIG` unverified. |
| Remove implicit `int` | N635, N692, N722 | 🟢 Yes | Error from `c99!` up, GNU dialects included, as in GCC 14. `c89!` and `gnu89!` have the rule this paper removed: a declaration with no type specifier — `static x;`, `f() { … }`, `const limit = 10;`, a K&R parameter no declaration list entry names — declares an `int`. |
| Reliable integer division | N617 | 🟢 Yes | Truncation toward zero (Rust `/`, `%`). |
| Universal character names (`\u` and `\U`) | | 🟢 Yes | In character constants, string literals and identifiers. In a literal the name is encoded the way the literal's own prefix stores its elements — UTF-8 bytes for a narrow or `u8` one, a surrogate pair in `u"…"` where the character needs one — and in an identifier it simply *is* the character, so `café` and `café` are one name. `\N{…}`, C23's *named* universal character, is not implemented. |
| Extended identifiers | N717 | 🟢 Yes | An identifier may hold any character Unicode Annex #31 calls `XID_Start`/`XID_Continue`, written either as a universal character name (C99's own spelling, and a `c89!` diagnostic) or as the character itself, which GCC and Clang have taken since GCC 10 and which every entry point here takes. The character set is C23's (N2836) in every revision: C99's Annex D and C11's are approximations of the same intent, and refusing a character a later annex added would be refusing it for no reason a user could act on. An identifier that is **not in Normalization Form C** is refused: `rustc` silently normalises the identifiers a procedural macro hands it, so two C names that differ only by normalization would otherwise become one Rust item. `$` is still opt-in. |
| Hexadecimal floating-point constants | N308 | 🟢 Yes | Raw-token input cannot carry them (Rust's lexer rejects them); use string-literal input. |
| Compound literals | N716 | 🟢 Yes | Block-scope and file-scope storage. |
| Designated initializers | N494 | 🟢 Yes | Designator *lists* included: `.a.b = 1`, `.arr[2].x = 3`, `[1].y = 2`, one that reaches into a `union` member or through a C11 anonymous member, and GNU's `[low ... high]` at any position in the list. 6.7.8p17's "next subobject" is followed exactly — `{ .i[0].p[1] = 5, 6, 7, 8 }` carries on into `i[1].p[0]`, `i[1].p[1]` and then out of `i` — and so is the elided-brace rule below a designator (`{ .arr = 1, 2, 3 }`). Two designators into the same member merge rather than replace. |
| `//` comments | N644 | 🟢 Yes | |
| Extended integer types and library functions in `<inttypes.h>` and `<stdint.h>` | | 🟢 Yes | Bundled headers. |
| Remove implicit function declaration | N636 | 🟢 Yes | Error from `c99!` up, GNU dialects included. In `c89!` and `gnu89!` a call to an undeclared `f` declares `extern int f();` at file scope from that point on — no prototype, so the arguments get the default argument promotions — and it becomes an `extern` declaration like any other, so `abs(-3)` in a program that includes nothing links against the C library. A later declaration must be compatible with it or it is the ordinary "conflicting types". A `__builtin_` name is never declared this way: it belongs to the implementation, and a diagnostic beats a link error. |
| Preprocessor arithmetic done in `intmax_t`/`uintmax_t` | N736 | 🟢 Yes | |
| Mixed declarations and code; new block scopes for selection and iteration statements | N740 | 🟢 Yes | `if`, `switch`, `while` and `do` are each a block of their own (6.8.4p3, 6.8.5p5), so a tag declared in a controlling expression — `if (sizeof(enum { a, b }))` — is scoped to the statement and does not leak into the enclosing block, which is what C89 did. Clang's `C99/block-scopes.c` is that test. |
| Integer constant type rules | N629 | 🟢 Yes | |
| Integer promotion rules | N725 | 🟢 Yes | |
| Macros with a variable number of arguments | N707 | 🟢 Yes | |
| IEC 60559 support | | 🟡 Partial | Arithmetic is IEEE (Rust `f32`/`f64`); `<fenv.h>` and `__STDC_IEC_559__` are absent. |
| Trailing comma allowed in `enum` declaration | | ⚪ Unverified | |
| Inline functions | N741 | 🟢 Yes | `#[inline]`; C99's external-definition rules are not modelled. |
| Boolean type in `<stdbool.h>` | N815 | 🟢 Yes | |
| Idempotent type qualifiers | N505 | ⚪ Unverified | |
| Empty macro arguments | N570 | 🟢 Yes | |
| Additional predefined macro names | | 🟡 Partial | `__STDC_VERSION__`, `__STDC_HOSTED__`, the four `__STDC_NO_*` subsetting macros, `__STDC_UTF_16__` and `__STDC_UTF_32__` (C11 7.28p2: `char16_t` and `char32_t` really are UTF-16 and UTF-32), and the three `__STDC_EMBED_*` answers `__has_embed` gives; `__STDC_ISO_10646__` and `__STDC_IEC_559__` are absent. Beyond the standard's own, the GCC family a great deal of portable C is written against is defined from the target model: the limits (`__SCHAR_MAX__` … `__LONG_LONG_MAX__`, `__SIZE_MAX__`, `__INTMAX_MAX__`, `__WCHAR_MAX__`), the widths (`__INT_WIDTH__` and the rest), the types (`__SIZE_TYPE__`, `__PTRDIFF_TYPE__`, `__INTPTR_TYPE__`, `__WCHAR_TYPE__`, the `__INTn_TYPE__` and `__INT_LEASTn_*` families) and the floating characteristics (`__FLT_MAX__`, `__DBL_EPSILON__`, …). Every one of them, and the architecture and system macros beside them, comes from the [target model](#the-target-model) rather than from the host, so `CINRS_TARGET` changes them together. `__SIZEOF_INT128__` is `16` on a 64-bit architecture and undefined on a 32-bit one, matching where `__int128` exists. Absent on purpose: `__OPTIMIZE__`, and the `__INT8_C`-style function-like macros. |
| `_Pragma` preprocessing operator | N634 | 🟢 Yes | Destringized and executed as the directive it spells, so a macro can produce one. |
| Standard pragmas (`STDC FP_CONTRACT`, …) | N631, N696 | 🟢 Accepted | Ignored. |
| `__func__` predefined identifier | N611 | 🟢 Yes | A `const char[]` in every function body, so `sizeof(__func__)` is the name's length; GCC's `__FUNCTION__` and `__PRETTY_FUNCTION__` are the same thing. |
| `va_copy` macro | N671 | 🟢 Yes | `<stdarg.h>`. |
| Remove deprecation of aliased array parameters | | ⚪ N/A | |
| Conversion of array to pointer not limited to lvalues | N835 | ⚪ Unverified | |
| Relaxed constraints on aggregate and union initialization | N782 | 🟢 Yes | Non-constant initializers for automatic aggregates. |
| Relaxed restrictions on portable header names | N772 | ⚪ N/A | |
| `return` without an expression not permitted in a function that returns a value | | 🟡 Partial | Accepted (returns zero) instead of diagnosed, as GCC does without `-pedantic-errors`. |

Also standard C99 but absent from Clang's list:

| Feature | cinrs | Notes |
| --- | --- | --- |
| Bit-fields (also C89) | 🟢 Yes | `_Bool`, `int` and `unsigned int` as the standard requires; `char`, `short`, `long`, `long long`, their signed and unsigned forms and `enum` as the GCC extension. The layout follows GCC and Clang, and is checked against the host compiler by `tests/bitfield_layout.rs`. A member has no address, so it becomes a pair of accessors on a shared `[u8; K]`; see the crate docs. Bits are allocated from the *least significant* end, which is what GCC and Clang do on a little-endian machine and the opposite of what they do on a big-endian one — so a bit-field is a located error on a big-endian [target](#the-target-model) rather than one laid out the wrong way round. The signedness of a bit-field of enumeration type is the enumeration's own: unsigned where no enumerator is negative, which is what GCC and Clang pick and the only place the choice is observable. |
| Function declarators without a prototype (6.7.5.3p14, 6.5.2.2p6) | 🟢 Yes | In `c89!`, `c99!`, `c11!`, `c17!` and the matching `gnu*!` dialects, `int f();` and `int (*fp)();` declare a function whose parameters are *unspecified*: a call may pass any number of arguments, each gets the default argument promotions, and the callee is invoked through the signature they make. A *definition* written `int f() { … }` takes no parameters, as 6.9.1p7 says, and calls to it through the unprototyped type are still legal. Two declarations of one function are compatible when the prototyped one is not variadic and no parameter type is changed by the promotions (6.7.5.3p15), which is also what `_Generic`, `__builtin_types_compatible_p` and assignment between function pointers use. `c23!` and `gnu23!` follow N2841 instead. |
| Old-style (K&R) function definitions (obsolescent) | 🟢 Yes | `int f(a, b) int a; char *b; { … }` in every entry point below `c23!`, which is where C removed it. The identifier list and the declaration list become the parameter list (6.9.1p6); a name the declaration list leaves out is an `int`, which is implicit `int` and therefore `c89!` and `gnu89!` only. The definition's type has **no prototype** (6.9.1p7), so it is compatible with `int f();` and a caller applies the default argument promotions — which is why the generated item takes the *promoted* types and converts to the declared ones on entry: `int f(c) char c;` is `fn f(c: c_int)` with `let c: c_char = c as c_char;` in front of the body. `register` is allowed on a parameter; a declaration-list entry that names something other than a parameter, names one twice, or carries an initialiser is diagnosed. `int f();` is *not* one of these — see the row above. |
| `#line` and GCC's `# N "file" flags…` line marker (6.10.4) | 🟢 Yes | Both forms, the macro-expanded one included, per file. Only `__LINE__`, `__FILE__` and `__FILE_NAME__` move: a diagnostic still points at the token that was really written, which is the whole point of the crate. `__BASE_FILE__` names the file the unit started in and is unaffected. A number outside 1…2147483647 is an error, which is what `-pedantic-errors` makes it. |
| Translation phase 2 (line splicing, 5.1.1.2p1) | 🟢 Yes | A backslash-newline is deleted *before* the source is split into tokens, so one may sit in the middle of an identifier: `__LI\<newline>NE__` is the one identifier `__LINE__`, which is what Clang's `drs/dr464.c` and `C99/n590.c` require. A splice needs string-literal input (see the README), because Rust's own lexer will not hand a line continuation over in raw-token form. A splice inside a *number* or a *punctuator* — `1\<newline>2`, `+\<newline>=` — is still a token boundary, which no real program depends on. |
| `#include` of a computed header name, `#include __FILE__` | 🟢 Yes | The name a header is known by is written relative to the working directory, so a header that includes itself by `__FILE__` asks for `some/dir/thing.h` from a directive written inside `some/dir`. A quoted include whose name *is* a path — it holds a directory separator — is therefore also looked for from the working directory, after the including file's own directory and the search path and before the bundled headers. A bare name never is, so nothing lying about can shadow the bundled `<stdio.h>`. |
| `<wchar.h>` and `<wctype.h>` | 🟢 Yes | Bundled. `wchar_t` is `int` (from `<stddef.h>`) on every target, which is what the front end gives `L'x'` and `L"…"`; `mbstate_t` is spelled the way each platform's library lays it out. `wcstold` is left out with `long double`. |
| `<iso646.h>` | 🟢 Yes | Bundled: the eleven macros (`and`, `and_eq`, `bitand`, `bitor`, `compl`, `not`, `not_eq`, `or`, `or_eq`, `xor`, `xor_eq`) and nothing else, since nothing in the language knows about them. |
| `<signal.h>` | 🟢 Yes | Bundled: `sig_atomic_t`, `SIG_DFL`/`SIG_IGN`/`SIG_ERR`, `signal` and `raise`, and the signal *numbers*, which are the platform's rather than C's — Linux's, the BSD/Apple set, or the six the Microsoft C runtime has, chosen by the [target model](#the-target-model) exactly as `<time.h>` chooses `time_t`. The POSIX signals beyond the standard six are there for the two Unix families, since `signal(SIGPIPE, SIG_IGN)` is ordinary. `sigaction` and `sigset_t` are not: their layouts differ between the families and a header that guessed at one would corrupt memory rather than fail to compile. |
| POSIX headers: `<sys/types.h>`, `<unistd.h>`, `<fcntl.h>`, `<strings.h>` | ⚪ N/A (bundled) | Not C, and bundled anyway because a small program reaches for them and none of the four needs a type with a layout: the system `typedef` names (`ssize_t`, `off_t`, `pid_t`, `uid_t`, `mode_t`, `dev_t`, `ino_t`, `nlink_t`, `blksize_t`, `blkcnt_t`, `suseconds_t`, `id_t`, `key_t`, `caddr_t` …), `read`/`write`/`close`/`lseek`/`getpid`/`sleep`/`_exit`, `open` with the `O_*` flags, and the BSD string functions. Every type and every flag value comes from the [target model](#the-target-model), read off glibc, Apple's library and the Microsoft runtime. The last three are POSIX and say so with an `#error` when the target is Windows. `struct stat`, `fd_set`, `sigset_t` and the `pthread_*` types are deliberately absent, for the layout reason above. |
| `setjmp`/`longjmp` | 🔴 No | `<setjmp.h>` is bundled only to `#error`. |
| `long double` | 🟡 Partial | Mapped to `double`; the ABI of `long double` arguments is therefore wrong. |
| `va_list` as a struct member or file-scope object | 🔴 No | `core::ffi::VaList` carries a lifetime. |
| Variadic function *definitions* | 🟢 Yes (Rust ≥ 1.99) | Declaring and calling variadic functions works on any toolchain. |

## C11

| Feature | Paper | cinrs | Notes |
| --- | --- | --- | --- |
| A finer-grained specification for sequencing | N1252 | ⚪ N/A | |
| Clarification of expressions | N1282 | ⚪ N/A | |
| Extending the lifetime of temporary objects | N1285 | ⚪ N/A | |
| Requiring `signed char` to have no padding bits | N1310 | 🟢 Yes | |
| Initializing static or external variables | N1311 | 🟢 Yes | |
| Conversion between pointers and floating types | N1316 | ⚪ Unverified | Should be rejected. |
| Adding TR 19769 (`<uchar.h>`, `char16_t`, `char32_t`) | N1326 | 🟢 Yes | `<uchar.h>` is bundled: `char16_t` and `char32_t` are `uint_least16_t` and `uint_least32_t` — `unsigned short` and `unsigned int` on every target modelled here — and `mbrtoc16`, `c16rtomb`, `mbrtoc32` and `c32rtomb` are declared and link. Neither name is a keyword in C, so both are ordinary typedefs, exactly as they are in a real library's header. Being typedefs, they are indistinguishable from their underlying types to `_Generic` and to `__builtin_types_compatible_p`, which is the same limitation `wchar_t` has. |
| Static assertions | N1330 | 🟢 Yes | File, block and struct scope. |
| Parallel memory sequencing model | N1349 | ⚪ N/A | |
| `_Bool` bit-fields | N1356 | 🟢 Yes | Width 1, read as `bool`, promoted to `int`. |
| Technical corrigendum for C1X | N1359 | ⚪ N/A | |
| Benign typedef redefinition | N1360 | ⚪ Unverified | |
| Thread-local storage | N1364 | 🟢 Yes | `_Thread_local` at file scope and on a block-scope `static` (6.7.1p3 requires `static` or `extern` there), with a constant initialiser; C23's `thread_local` and GNU's `__thread` are the same thing. The object becomes a `std::thread_local!` item holding an `UnsafeCell<T>`, and every access goes through the `*mut T` its `with` hands out, which is valid for as long as the current thread's copy is — C's own guarantee about the address. Two shapes are refused: `extern _Thread_local`, which would need Rust's unstable `#[thread_local]` on an `extern` item, and exporting one under `#pragma cinrs export`, since a `thread_local!` has no stable way to be given a C symbol. Because `thread_local!` is a `std` macro, this is the third construct whose expansion needs more than `core`, after variable length arrays and `alloca`; `#pragma cinrs no_std` refuses it with that reason. `<threads.h>` and the rest of C11's threads are still absent, and `__STDC_NO_THREADS__` says so. |
| Constant expressions | N1365 | ⚪ N/A | |
| Contractions and expression evaluation methods | N1367 | ⚪ N/A | |
| Floating-point to int/`_Bool` conversions | N1391 | 🟢 Yes | |
| Wide function returns | N1396 | ⚪ N/A | |
| Alignment (`_Alignas`, `_Alignof`, `<stdalign.h>`, `aligned_alloc`) | N1397, N1447 | 🟡 Partial | `_Alignof` yes; `_Alignas` on a *member* moves it to the boundary it asks for, with explicit padding in the generated item, and on an *object* is refused; `aligned_alloc` unverified. |
| Anonymous member-structures and unions | N1406 | 🟢 Yes | |
| Completeness of types | N1439 | ⚪ N/A | |
| Generic macro facility (`_Generic`) | N1441 | 🟢 Yes | |
| Dependency ordering for C memory model | N1444 | ⚪ N/A | `memory_order_consume` is accepted and performed as an acquire, which is what every compiler does with it and what 7.17.3 allows; Rust has no `Ordering::Consume` to map it to. |
| Subsetting the standard (`__STDC_NO_ATOMICS__`, `__STDC_NO_THREADS__`, `__STDC_NO_VLA__`, `__STDC_NO_COMPLEX__`) | N1460 | 🟢 Yes | One of the four is predefined as `1` in every entry point, which makes the absent feature a conforming omission, and a second follows a cargo feature. `__STDC_NO_ATOMICS__` is **not** predefined: `_Atomic` and `<stdatomic.h>` are implemented ([N1485/N1526](#c11)), so saying they are absent would be false. Neither is `__STDC_NO_VLA__` any more: variable length arrays and the variably modified types built on them are translated ([N683](#c99)) — `int a[n]`, `double a[n][m]`, `int (*p)[n]`, `typedef int T[n];` and the parameter forms — and what is left of the feature is one corner of a *type name*, which no program guards `__STDC_NO_VLA__` for. `__STDC_NO_COMPLEX__` is defined only when the `complex` feature is off, which is not the default; see the [N693 row](#c99) and the note at the top. `__STDC_NO_THREADS__` is the conservative half of a feature that partly works: it stays defined although `_Thread_local` does ([N1364](#c11)), because the macro is about `<threads.h>` and 7.26, which are not there at all. It comes off when the rest of that feature lands. |
| Assumed types in F.9.2 | N1468 | ⚪ N/A | |
| Supporting the `noreturn` property (`_Noreturn`, `<stdnoreturn.h>`) | N1478 | 🟢 Yes | |
| Updates to the memory model | N1480 | ⚪ N/A | |
| Explicit initializers for atomics | N1482 | 🟢 Yes | `_Atomic int x = 1;` and `atomic_init(&x, 1)` are both plain writes, which is what 7.17.2.1p2 says initialising an atomic object is; `ATOMIC_VAR_INIT` is defined (deprecated by C17, removed by C23) and is the identity. |
| Atomics (`_Atomic`, `<stdatomic.h>`) | N1485, N1526 | 🟡 Partial | `_Atomic` as a qualifier and as the `_Atomic(T)` specifier, over the scalar types: `_Bool`, the 1-, 2-, 4- and 8-byte integers and enumerations, `float` and `double`, and object pointers. Every read of such an object is a sequentially consistent load, every write a store, and `+=`, `++` and `--` are each one read-modify-write (6.5.16.2p3); the alignment of an atomic type is its size, so `_Alignof(_Atomic long long)` is 8. `<stdatomic.h>` is bundled — the `atomic_*` typedefs, `atomic_flag`, `memory_order`, the fences, `atomic_is_lock_free`, the `ATOMIC_*_LOCK_FREE` macros (all `2`) and the generic functions, written over Clang's `__c11_atomic_*` builtins, which scale pointer arithmetic as 7.17.7.5 requires. What is refused: an `_Atomic` `struct` or `union`, which is legal C and would need a lock nothing in the generated Rust can be; `_Atomic __int128`, for want of a stable `AtomicU128`; and an atomic function pointer, which Rust models as an `Option<fn>`. GCC's `__atomic_*` and `__sync_*` builtins are here too, in every entry point. |
| UTF-8 string literals (`u8"…"`) | N1488 | 🟢 Yes | The elements are the UTF-8 bytes of the source, of type `char` here and `char8_t` from C23 on (see [N2653](#c23)). Rust's own lexer reserves the prefix, so a `u8"…"` has to be written in the string-literal input form; the same is true of `u"…"` and `U"…"`. |
| Optimizing away infinite loops | N1509 | ⚪ N/A | |
| Conditional normative status for Annex G | N1514 | ⚪ N/A | The paper is what made Annex G conditional on `__STDC_IEC_559_COMPLEX__`, and `cinrs` does not define it: the *arithmetic* of G.5.1 is implemented — an infinite operand gives an infinite product or quotient where the naive formula gives NaN + iNaN — but nothing else of the annex is claimed. See the [N693 row](#c99). |
| Creation of complex value (`CMPLX`) | N1464 | 🟢 Yes | `CMPLX`, `CMPLXF` and `CMPLXL` are in `<complex.h>`, built on `__builtin_complex` as GCC's and Clang's are. It is the only way to write a complex value whose imaginary part is an infinity or a NaN: `x + y * I` multiplies, and `inf * 0` is a NaN. `clang/test/C/C11/n1464.c` is one of the conformance suite's cases and passes. |
| Extended identifier characters | N1518 | 🟢 Yes | See the [N717 row](#c99): one character set — C23's — serves every entry point. |
| Atomic bit-fields implementation defined | N1530 | ⚪ N/A | An `_Atomic` bit-field is refused: a bit-field has no address, and every atomic operation here is built on one. |
| Alignment and struct/union type compatibility | N1532 | ⚪ N/A | |
| Clarification for wide evaluation | N1531 | ⚪ N/A | |

## C17

C17 contains no new language features; it folds in defect-report resolutions.
`c17!` is `c11!` with `__STDC_VERSION__ == 201710L`.

## C23

| Feature | Paper | cinrs | Notes |
| --- | --- | --- | --- |
| Evaluation formats | N2186 | ⚪ N/A | |
| Harmonizing `static_assert` with C++ | N2665 | 🟢 Yes | Message optional. |
| `nodiscard` attribute | N2267, N2448 | 🟢 Accepted | |
| `maybe_unused` attribute | N2270 | 🟢 Accepted | |
| TS 18661 integration (`_FloatN`, decimal floating types) | N2314, N2341, N2359, N2546, N2640, N2755, N2931, N2754 | 🔴 No | No stable Rust types. |
| Preprocessor line numbers unspecified | N2322 | ⚪ N/A | |
| `deprecated` attribute | N2334 | 🟢 Yes | `#[deprecated]`, with the message it was given, so Rust code that calls the function is warned. |
| Attributes (`[[…]]` syntax) | N2335, N2554 | 🟢 Yes | Unknown attributes ignored, as required. |
| Defining new types in `offsetof` | N2350 | 🟢 Yes | `offsetof` is `__builtin_offsetof`, and the front end folds it to an **integer constant** out of the layout it computed for the record — so it may be an array bound, a `case` label or the initialiser of a file-scope object, which is what C99 6.6 asks and what `execute/strlen-7` writes. The member designator is 7.17p3's in full: `a`, `a.b`, `a[2].b`, and a member of an anonymous member. That the folded value agrees with the generated `#[repr(C)]` item's own layout is checked against Rust's `core::mem::offset_of!` in `tests/execute.rs` and in the differential corpus of `tests/bitfield_layout.rs`. |
| `fallthrough` attribute | N2408 | 🟢 Yes | |
| Two's complement sign representation | N2412 | 🟢 Yes | |
| Adding the `u8` character prefix | N2418 | 🟢 Yes | `u8'x'` has type `char8_t` and holds one UTF-8 code unit; a character that needs more than one is a diagnostic, as is more than one character. `u'x'` and `U'x'` are C11's and work from `c11!` up. |
| Remove support for function definitions with identifier lists | N2432 | 🟢 Yes | `c23!` and `gnu23!` refuse one with `old-style function definitions were removed in C23`; every earlier entry point accepts it, as the revision it implements does. |
| Annex F.8 update | N2384 | ⚪ N/A | |
| Allowing unnamed parameters in function definitions | N2480 | 🟢 Yes | Nothing in the body can reach the parameter, but the caller still passes one, so the generated item takes an argument in that position under a name of its own. A GNU dialect accepts it in every revision, as GCC and Clang do (`-Wmissing-parameter-name`); a strict entry point below `c23!` refuses it. |
| Free positioning of labels inside compound statements | N2508 | 🟢 Yes | Every label, not only the named ones: `switch (x) { case 1: }`, `case 1: static_assert(1, "");` and `label: int x;` are all accepted in `c23!` and in the GNU dialects, and the label takes a null statement. |
| `return` with a `void` expression in a `void` function | N2734 | 🟢 Yes | `void f(void) { return g(); }` where `g` returns nothing — how a wrapper forwards a call — is accepted in `c23!`; the expression is evaluated and there is no value to return. Every earlier revision makes it a 6.8.6.4p1 constraint violation, which the strict entry points keep and the GNU dialects (which also take a *non*-`void` expression, as GCC does) do not. |
| Querying attribute support (`__has_c_attribute`) | N2553 | 🟢 Yes | `202311L` for the attributes this crate honours, 0 otherwise. `__has_include`, `__has_attribute`, `__has_builtin`, `__has_feature` and `__has_extension` are answered from the same tables. |
| Binary literals | N2549 | 🟢 Yes | |
| Allow duplicate attributes | N2557 | 🟢 Accepted | |
| Character encoding of diagnostic text | N2563 | ⚪ N/A | |
| What we think we reserve | N2572 | ⚪ N/A | |
| Remove mixed wide string literal concatenation | N2594 | ⚪ Unverified | |
| Update to IEC 60559:2020 | N2600 | ⚪ N/A | |
| Compatibility of pointers to arrays with qualifiers | N2607 | 🟡 Partial | `float (*)[n]` converts to `const float (*)[n]`, so passing `float x[n][n]` to a `const float x[n][n]` parameter is accepted — in *every* entry point, as GCC and Clang do (GCC warns only under `-pedantic` before C23). What is not modelled is the other half of 6.7.3p9: `const A a`, where `A` is a `typedef` of an array, puts the qualifier on the object rather than on the elements, so `&a` is not the `const int (*)[1]` the paper says it is. |
| Format specifier and argument type relationship | N2562 | ⚪ N/A | |
| Digit separators | N2626 | 🟢 Yes | String-literal input only (Rust's lexer rejects `1'000`). |
| Missing `+(x)` in table | N2641 | ⚪ N/A | |
| `#elifdef` and `#elifndef` | N2645 | 🟢 Yes | |
| `[[maybe_unused]]` for labels | N2662 | ⚪ Unverified | |
| Zeros compare equal / Negative values / 5.2.4.2.2 cleanup | N2670, N2671, N2672, N2806, N2879 | ⚪ N/A | |
| Towards integer safety (`<stdckdint.h>`) | N2683 | 🟢 Yes | The header is bundled and defines `ckd_add`, `ckd_sub`, `ckd_mul` and `__STDC_VERSION_STDCKDINT_H__` on top of `__builtin_add_overflow` and its two relatives, which is the spelling the standard settled on. The arithmetic is in infinite precision and the answer is whether the result fit the type `*r` has, so the operands' types decide nothing. |
| Adding fundamental type for N-bit integers (`_BitInt`) | N2763, N2775, N2969, N3035 | 🔴 No | |
| `#warning` directive | N2686 | 🟢 Yes | Accepted; no output (a proc macro cannot warn). |
| Sterile characters / Numerically equal | N2688, N2716, N2847 | ⚪ N/A | |
| `char16_t`/`char32_t` string literals are UTF-16/UTF-32 | N2728 | 🟢 Yes | `u"…"` holds UTF-16 code units, surrogate pairs and all — `sizeof(u"\U0001F600")` is 6 — and `U"…"` holds UTF-32 ones. `__STDC_UTF_16__` and `__STDC_UTF_32__` are predefined to say so. A *numeric* escape is a code unit rather than a character and is not re-encoded, so `u"\xd83d"` is that one unit. |
| IEC 60559 binding | N2749 | ⚪ N/A | |
| Annex F overflow and underflow | N2747 | ⚪ N/A | |
| Remove UB from incomplete types in function parameters | N2770 | ⚪ N/A | |
| Variably-modified types | N2778, N2992 | 🟢 Yes | The array, pointer, `typedef` and parameter forms all carry their bounds in the *type*, in hidden `size_t` objects the declaration binds. See the [C99 row](#c99). |
| Types do not have types | N2781 | ⚪ N/A | |
| Allow 16-bit `ptrdiff_t` | N2808 | ⚪ N/A | |
| CFP freestanding requirements | N2823 | ⚪ N/A | |
| Types and sizes / Clarifying integer terms | N2838, N2837 | ⚪ N/A | |
| Max exponent macros | N2843, N2882 | ⚪ N/A | |
| Expression transformations | N2846 | ⚪ N/A | |
| Contradiction about `INFINITY` macro | N2848 | 🟢 Yes | `<math.h>` defines `INFINITY` and `NAN`. |
| Require exact-width integer type interfaces | N2872, N2888 | 🟢 Yes | |
| `@`, `$`, and `` ` `` in the source/execution character set | N2701 | 🟡 Partial | `$` in identifiers is opt-in; `@` and `` ` `` only inside literals and comments. |
| The `noreturn` attribute | N2764 | 🟢 Yes | |
| `*_HAS_SUBNORM == 0` | N2797 | ⚪ N/A | |
| Disambiguate the storage class of some compound literals | N2819 | 🟢 Yes | |
| `unreachable()` | N2826 | 🟢 Yes | `core::hint::unreachable_unchecked()`. |
| Unicode sequences more than 21 bits are a constraint violation | N2828 | ⚪ Unverified | |
| Identifier syntax using Unicode Standard Annex 31 | N2836, N2939 | 🟢 Yes | `XID_Start`/`XID_Continue`, and NFC required; see the [N717 row](#c99). `\N{NAME}`, the *named* universal character C23 also added, is not implemented. |
| No function declarators without prototypes (`f()` means `f(void)`) | N2841 | 🟢 Yes | In `c23!` and `gnu23!` only, which is where the change belongs: an empty parameter list is `(void)`, and a call with an argument is "too many arguments", exactly as `gcc -std=c23` says. Every earlier entry point keeps C99 6.7.5.3p14 — see the [C99 table](#c99). |
| `char8_t` | N2653 | 🟢 Yes | A typedef of `unsigned char` in `<uchar.h>`, and — in `c23!` and `gnu23!` only — the element type of a `u8"…"` string and the type of a `u8'x'` constant; before C23 a `u8"…"` is still a `char[]`. `mbrtoc8` and `c8rtomb` are declared there too. |
| Consistent, warningless and intuitive initialization with `{}` | N2900, N3011 | 🟢 Yes | |
| Not-so-magic: `typeof`, `typeof_unqual` | N2927, N2930 | 🟢 Yes | `typeof_unqual` equals `typeof` (no top-level qualifiers in the type model). |
| Revise spelling of keywords (`bool`, `static_assert`, `alignof`, `alignas`, `thread_local`) | N2934 | 🟢 Yes | All are keywords, and each means what the underscored spelling means; see the [N1364 row](#c11) for what `thread_local` comes to. |
| Make `false` and `true` first-class language features | N2935 | 🟢 Yes | |
| Properly define blocks as part of the grammar | N2937 | ⚪ N/A | |
| Annex H (interchange and extended types) | N2601, N2844 | 🔴 No | |
| Indeterminate values and trap representations | N2861 | ⚪ N/A | |
| Remove `ATOMIC_VAR_INIT` | N2886 | 🔴 No | The bundled `<stdatomic.h>` defines it in every revision, C23 included: it is the identity, a great deal of code writes it, and refusing it in `c23!` would break that code for nothing. |
| Remove trigraphs | N2940 | 🟢 Yes | `c23!` and `gnu23!` have no trigraphs, so `??=` there is two question marks and an `=`; every strict entry point below C23 replaces them, which is what the revision removed. See the [C99 row](#c99). |
| Improved normal enumerations (values wider than `int`) | N3029 | 🟢 Yes | An enumerator whose value will not fit widens the *enumeration*, and every enumerator then has the widened type — which is what `_Generic` selects on. The type is the narrowest of `int`, `unsigned int`, `long`, `unsigned long`, `long long`, `unsigned long long` that holds every value, which is Clang's choice too; C23 leaves it implementation-defined. Such an enumeration *is* that integer type here rather than a `Ty::Enum`, and a tagged one at file scope gets a Rust alias for it. `c23!` and the GNU dialects widen; the strict entry points below C23 keep 6.7.2.2p2's constraint violation. |
| Relax requirements for `va_start` (single-argument form) | N2975 | 🔴 No | `va_start(ap)` is rejected; planned for `c23!`. |
| Enhanced enumerations (fixed underlying type) | N3030 | 🟢 Yes | |
| Freestanding C and IEC 60559 scope reduction | N2951 | ⚪ N/A | |
| Unsequenced functions (`[[unsequenced]]`, `[[reproducible]]`) | N2956 | 🟢 Accepted | |
| Comma omission and deletion (`__VA_OPT__`) | N3033 | 🟢 Yes | Including 6.10.5.2p1's constraint: the token sequence inside `__VA_OPT__( … )` may neither begin nor end with `##`, for the same reason a replacement list may not. |
| Underspecified object definitions | N3006 | 🟡 Partial | Follows from `auto`/`constexpr` below. |
| Type inference for object declarations (`auto`) | N3007 | 🟢 Yes | Several declarators are allowed (the standard requires one). |
| `constexpr` for object definitions | N3018 | 🟡 Partial | Arithmetic objects only; they become constants. |
| Storage class specifiers for compound literals | N3038 | 🔴 No | |
| Identifier primary expressions | N3034 | ⚪ N/A | |
| Introduce the `nullptr` constant | N3042 | 🟡 Partial | `nullptr` is a null `void *`; `nullptr_t` is a typedef of it. |
| Memory layout of unions | N2929 | ⚪ N/A | |
| Improved tag compatibility | N3037 | ⚪ Unverified | |
| `#embed` | N3017 | 🟢 Yes | The bytes of a file become a comma-separated list of `unsigned char` values, with all four standard parameters: `limit(N)`, `prefix(…)`, `suffix(…)` (both left out of an *empty* expansion) and `if_empty(…)`. `__has_embed` answers `__STDC_EMBED_NOT_FOUND__`, `__STDC_EMBED_FOUND__` or `__STDC_EMBED_EMPTY__`, and a parameter this implementation does not have makes it "not found", as 6.10.1p5 asks. The search is `#include`'s with the two steps that are about *headers* left out: there are no bundled resources, and the quoted form looks next to the including file and then along the include path, where the angled form takes the include path alone — GCC's separate `--embed-dir` has no counterpart here. A resource that is read is mentioned in the expansion with `include_bytes!`, so editing it rebuilds the crate. `gnu*!` accepts the directive as GCC 15 does; a strict entry point below `c23!` refuses it with the usual gate. The bytes become preprocessing tokens like any others, so a resource is bounded by the unit's own expansion budget — two tokens a byte against four million — which puts the ceiling near two megabytes; a larger file belongs in Rust's own `include_bytes!`. |
| `__has_include` | N2799 | 🟢 Yes | Resolved exactly as `#include` is. GNU's `__has_include_next` too. |

## C2y

Nothing from C2y is implemented as C2y; `c2y!` does not exist. One of the
features Clang lists is already here under another name: case ranges (N3370)
are GNU's `case 1 ... 5:`, which every entry point accepts. `_Countof` (N3369),
named loops (N3355) and `if` declarations (N3356) would be the natural first
candidates when a `c2y!` entry point is added.
