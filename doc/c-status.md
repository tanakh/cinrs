# C standard support status

Modelled on Clang's [C status page](https://clang.llvm.org/c_status.html): one
row per feature (with its WG14 paper number where Clang lists one) and cinrs's
status. Rows that are pure wording changes, library-only, or about optimisation
freedom are kept so the table can be checked against Clang's, and marked N/A.

Statuses:

* **Yes** — implemented and tested.
* **Partial** — implemented with a documented gap (the note says which).
* **Accepted** — parsed and ignored where the standard allows an implementation
  to ignore it (attributes, hints, pragmas).
* **No** — not implemented; a clear diagnostic is produced where the construct
  can be recognised.
* **Unverified** — believed to work but not covered by a test yet; treat as a
  to-do for the test suite.
* **N/A** — no implementation work is involved (wording, library semantics we
  inherit from libc/Rust, or freedom we do not exercise).

Entry points: `c99!`, `c11!`, `c17!`, `c23!`. A feature of a newer standard used
in an older entry point is rejected with `requires C11/C23 or later`. The GNU
entry points — `gnu99!`, `gnu11!`, `gnu17!`, `gnu23!` — accept those features
instead, exactly as `gcc -std=gnu99` does, and add the GNU extensions on top;
[`doc/gnu-extensions.md`](gnu-extensions.md) is the catalogue for both.

Two of the rows below are answered by the *entry point* rather than by the
front end: `__STDC_NO_ATOMICS__`, `__STDC_NO_THREADS__`, `__STDC_NO_VLA__` and
`__STDC_NO_COMPLEX__` are all predefined, so the four features they name are
conforming omissions rather than gaps.

## C99

| Feature | Paper | cinrs | Notes |
| --- | --- | --- | --- |
| Restricted character set support via digraphs and `<iso646.h>` | | Partial | Digraphs are lexed; `<iso646.h>` is not bundled yet. Trigraphs are not supported in any mode (removed in C23). |
| More precise aliasing rules via effective type | | N/A | |
| Restricted pointers (`restrict`) | N448 | Accepted | Parsed and ignored, as the standard permits. |
| Variable length arrays | N683 | No | No stack allocation with a dynamic size in Rust. `__STDC_NO_VLA__` is predefined, which C11 makes the conforming way to leave them out. |
| Flexible array members | | Yes | A `[T; 0]` tail member; `sizeof` leaves it out and indexing it is pointer arithmetic. Initialising one — which GCC allows with a warning — is refused. |
| `static` and type qualifiers in parameter array declarators | | Accepted | Parsed; no effect on codegen. |
| Complex and imaginary support in `<complex.h>` | N693 | No | `_Complex` is rejected. |
| Type-generic math macros in `<tgmath.h>` | N693 | No | Would be built on `_Generic`. |
| The `long long int` type | N601 | Yes | |
| Increase minimum translation limits | N590 | Yes | Nesting limits are 200 levels, above the minimums. |
| Additional floating-point characteristics in `<float.h>` | | Partial | The common `FLT_*`/`DBL_*` macros; `FLT_EVAL_METHOD`, `DECIMAL_DIG` unverified. |
| Remove implicit `int` | N635, N692, N722 | Yes | Error. |
| Reliable integer division | N617 | Yes | Truncation toward zero (Rust `/`, `%`). |
| Universal character names (`\u` and `\U`) | | Partial | In character and string constants; not in identifiers. |
| Extended identifiers | N717 | No | Identifiers are ASCII (plus `$` as an opt-in extension). |
| Hexadecimal floating-point constants | N308 | Yes | Raw-token input cannot carry them (Rust's lexer rejects them); use string-literal input. |
| Compound literals | N716 | Yes | Block-scope and file-scope storage. |
| Designated initializers | N494 | Partial | Nested member designators (`.a.b = 1`) are rejected; nested braces work. |
| `//` comments | N644 | Yes | |
| Extended integer types and library functions in `<inttypes.h>` and `<stdint.h>` | | Yes | Bundled headers. |
| Remove implicit function declaration | N636 | Yes | Error. |
| Preprocessor arithmetic done in `intmax_t`/`uintmax_t` | N736 | Yes | |
| Mixed declarations and code; new block scopes for selection and iteration statements | N740 | Yes | |
| Integer constant type rules | N629 | Yes | |
| Integer promotion rules | N725 | Yes | |
| Macros with a variable number of arguments | N707 | Yes | |
| IEC 60559 support | | Partial | Arithmetic is IEEE (Rust `f32`/`f64`); `<fenv.h>` and `__STDC_IEC_559__` are absent. |
| Trailing comma allowed in `enum` declaration | | Unverified | |
| Inline functions | N741 | Yes | `#[inline]`; C99's external-definition rules are not modelled. |
| Boolean type in `<stdbool.h>` | N815 | Yes | |
| Idempotent type qualifiers | N505 | Unverified | |
| Empty macro arguments | N570 | Yes | |
| Additional predefined macro names | | Partial | `__STDC_VERSION__`, `__STDC_HOSTED__`, and the four `__STDC_NO_*` subsetting macros; `__STDC_ISO_10646__` and `__STDC_IEC_559__` are absent. |
| `_Pragma` preprocessing operator | N634 | Yes | Destringized and executed as the directive it spells, so a macro can produce one. |
| Standard pragmas (`STDC FP_CONTRACT`, …) | N631, N696 | Accepted | Ignored. |
| `__func__` predefined identifier | N611 | Yes | A `const char[]` in every function body, so `sizeof(__func__)` is the name's length; GCC's `__FUNCTION__` and `__PRETTY_FUNCTION__` are the same thing. |
| `va_copy` macro | N671 | Yes | `<stdarg.h>`. |
| Remove deprecation of aliased array parameters | | N/A | |
| Conversion of array to pointer not limited to lvalues | N835 | Unverified | |
| Relaxed constraints on aggregate and union initialization | N782 | Yes | Non-constant initializers for automatic aggregates. |
| Relaxed restrictions on portable header names | N772 | N/A | |
| `return` without an expression not permitted in a function that returns a value | | Partial | Accepted (returns zero) instead of diagnosed, as GCC does without `-pedantic-errors`. |

Also standard C99 but absent from Clang's list:

| Feature | cinrs | Notes |
| --- | --- | --- |
| Bit-fields (also C89) | Yes | `_Bool`, `int` and `unsigned int` as the standard requires; `char`, `short`, `long`, `long long`, their signed and unsigned forms and `enum` as the GCC extension. The layout follows GCC and Clang, and is checked against the host compiler by `tests/bitfield_layout.rs`. A member has no address, so it becomes a pair of accessors on a shared `[u8; K]`; see the crate docs. |
| Old-style (K&R) function definitions (obsolescent) | No | Rejected in every mode; removed in C23. |
| `setjmp`/`longjmp` | No | `<setjmp.h>` is bundled only to `#error`. |
| `long double` | Partial | Mapped to `double`; the ABI of `long double` arguments is therefore wrong. |
| `va_list` as a struct member or file-scope object | No | `core::ffi::VaList` carries a lifetime. |
| Variadic function *definitions* | Yes (Rust ≥ 1.99) | Declaring and calling variadic functions works on any toolchain. |

## C11

| Feature | Paper | cinrs | Notes |
| --- | --- | --- | --- |
| A finer-grained specification for sequencing | N1252 | N/A | |
| Clarification of expressions | N1282 | N/A | |
| Extending the lifetime of temporary objects | N1285 | N/A | |
| Requiring `signed char` to have no padding bits | N1310 | Yes | |
| Initializing static or external variables | N1311 | Yes | |
| Conversion between pointers and floating types | N1316 | Unverified | Should be rejected. |
| Adding TR 19769 (`<uchar.h>`, `char16_t`, `char32_t`) | N1326 | No | |
| Static assertions | N1330 | Yes | File, block and struct scope. |
| Parallel memory sequencing model | N1349 | N/A | |
| `_Bool` bit-fields | N1356 | Yes | Width 1, read as `bool`, promoted to `int`. |
| Technical corrigendum for C1X | N1359 | N/A | |
| Benign typedef redefinition | N1360 | Unverified | |
| Thread-local storage | N1364 | No | `#[thread_local]` is unstable in Rust; `_Thread_local` is rejected. |
| Constant expressions | N1365 | N/A | |
| Contractions and expression evaluation methods | N1367 | N/A | |
| Floating-point to int/`_Bool` conversions | N1391 | Yes | |
| Wide function returns | N1396 | N/A | |
| Alignment (`_Alignas`, `_Alignof`, `<stdalign.h>`, `aligned_alloc`) | N1397, N1447 | Partial | `_Alignof` yes; `_Alignas` on a *member* moves it to the boundary it asks for, with explicit padding in the generated item, and on an *object* is refused; `aligned_alloc` unverified. |
| Anonymous member-structures and unions | N1406 | Yes | |
| Completeness of types | N1439 | N/A | |
| Generic macro facility (`_Generic`) | N1441 | Yes | |
| Dependency ordering for C memory model | N1444 | N/A | |
| Subsetting the standard (`__STDC_NO_ATOMICS__`, `__STDC_NO_THREADS__`, `__STDC_NO_VLA__`, `__STDC_NO_COMPLEX__`) | N1460 | Yes | All four are predefined as `1` in every entry point, which makes the absent features conforming omissions. |
| Assumed types in F.9.2 | N1468 | N/A | |
| Supporting the `noreturn` property (`_Noreturn`, `<stdnoreturn.h>`) | N1478 | Yes | |
| Updates to the memory model | N1480 | N/A | |
| Explicit initializers for atomics | N1482 | No | |
| Atomics (`_Atomic`, `<stdatomic.h>`) | N1485, N1526 | No | `_Atomic` is rejected. |
| UTF-8 string literals (`u8"…"`) | N1488 | No | |
| Optimizing away infinite loops | N1509 | N/A | |
| Conditional normative status for Annex G | N1514 | N/A | |
| Creation of complex value (`CMPLX`) | N1464 | No | |
| Extended identifier characters | N1518 | No | |
| Atomic bit-fields implementation defined | N1530 | N/A | |
| Alignment and struct/union type compatibility | N1532 | N/A | |
| Clarification for wide evaluation | N1531 | N/A | |

## C17

C17 contains no new language features; it folds in defect-report resolutions.
`c17!` is `c11!` with `__STDC_VERSION__ == 201710L`.

## C23

| Feature | Paper | cinrs | Notes |
| --- | --- | --- | --- |
| Evaluation formats | N2186 | N/A | |
| Harmonizing `static_assert` with C++ | N2665 | Yes | Message optional. |
| `nodiscard` attribute | N2267, N2448 | Accepted | |
| `maybe_unused` attribute | N2270 | Accepted | |
| TS 18661 integration (`_FloatN`, decimal floating types) | N2314, N2341, N2359, N2546, N2640, N2755, N2931, N2754 | No | No stable Rust types. |
| Preprocessor line numbers unspecified | N2322 | N/A | |
| `deprecated` attribute | N2334 | Yes | `#[deprecated]`, with the message it was given, so Rust code that calls the function is warned. |
| Attributes (`[[…]]` syntax) | N2335, N2554 | Yes | Unknown attributes ignored, as required. |
| Defining new types in `offsetof` | N2350 | Unverified | |
| `fallthrough` attribute | N2408 | Yes | |
| Two's complement sign representation | N2412 | Yes | |
| Adding the `u8` character prefix | N2418 | No | |
| Remove support for function definitions with identifier lists | N2432 | Yes | K&R definitions are rejected in every mode. |
| Annex F.8 update | N2384 | N/A | |
| Allowing unnamed parameters in function definitions | N2480 | Unverified | |
| Free positioning of labels inside compound statements | N2508 | Yes | |
| Querying attribute support (`__has_c_attribute`) | N2553 | Yes | `202311L` for the attributes this crate honours, 0 otherwise. `__has_include`, `__has_attribute`, `__has_builtin`, `__has_feature` and `__has_extension` are answered from the same tables. |
| Binary literals | N2549 | Yes | |
| Allow duplicate attributes | N2557 | Accepted | |
| Character encoding of diagnostic text | N2563 | N/A | |
| What we think we reserve | N2572 | N/A | |
| Remove mixed wide string literal concatenation | N2594 | Unverified | |
| Update to IEC 60559:2020 | N2600 | N/A | |
| Compatibility of pointers to arrays with qualifiers | N2607 | Unverified | |
| Format specifier and argument type relationship | N2562 | N/A | |
| Digit separators | N2626 | Yes | String-literal input only (Rust's lexer rejects `1'000`). |
| Missing `+(x)` in table | N2641 | N/A | |
| `#elifdef` and `#elifndef` | N2645 | Yes | |
| `[[maybe_unused]]` for labels | N2662 | Unverified | |
| Zeros compare equal / Negative values / 5.2.4.2.2 cleanup | N2670, N2671, N2672, N2806, N2879 | N/A | |
| Towards integer safety (`<stdckdint.h>`) | N2683 | No | Planned with the `__builtin_*_overflow` builtins. |
| Adding fundamental type for N-bit integers (`_BitInt`) | N2763, N2775, N2969, N3035 | No | |
| `#warning` directive | N2686 | Yes | Accepted; no output (a proc macro cannot warn). |
| Sterile characters / Numerically equal | N2688, N2716, N2847 | N/A | |
| `char16_t`/`char32_t` string literals are UTF-16/UTF-32 | N2728 | No | |
| IEC 60559 binding | N2749 | N/A | |
| Annex F overflow and underflow | N2747 | N/A | |
| Remove UB from incomplete types in function parameters | N2770 | N/A | |
| Variably-modified types | N2778, N2992 | No | With VLAs. |
| Types do not have types | N2781 | N/A | |
| Allow 16-bit `ptrdiff_t` | N2808 | N/A | |
| CFP freestanding requirements | N2823 | N/A | |
| Types and sizes / Clarifying integer terms | N2838, N2837 | N/A | |
| Max exponent macros | N2843, N2882 | N/A | |
| Expression transformations | N2846 | N/A | |
| Contradiction about `INFINITY` macro | N2848 | Yes | `<math.h>` defines `INFINITY` and `NAN`. |
| Require exact-width integer type interfaces | N2872, N2888 | Yes | |
| `@`, `$`, and `` ` `` in the source/execution character set | N2701 | Partial | `$` in identifiers is opt-in; `@` and `` ` `` only inside literals and comments. |
| The `noreturn` attribute | N2764 | Yes | |
| `*_HAS_SUBNORM == 0` | N2797 | N/A | |
| Disambiguate the storage class of some compound literals | N2819 | Yes | |
| `unreachable()` | N2826 | Yes | `core::hint::unreachable_unchecked()`. |
| Unicode sequences more than 21 bits are a constraint violation | N2828 | Unverified | |
| Identifier syntax using Unicode Standard Annex 31 | N2836, N2939 | No | ASCII identifiers only. |
| No function declarators without prototypes (`f()` means `f(void)`) | N2841 | Yes | Applied in every mode. |
| `char8_t` | N2653 | No | |
| Consistent, warningless and intuitive initialization with `{}` | N2900, N3011 | Yes | |
| Not-so-magic: `typeof`, `typeof_unqual` | N2927, N2930 | Yes | `typeof_unqual` equals `typeof` (no top-level qualifiers in the type model). |
| Revise spelling of keywords (`bool`, `static_assert`, `alignof`, `alignas`, `thread_local`) | N2934 | Partial | All are keywords; `thread_local` objects are rejected. |
| Make `false` and `true` first-class language features | N2935 | Yes | |
| Properly define blocks as part of the grammar | N2937 | N/A | |
| Annex H (interchange and extended types) | N2601, N2844 | No | |
| Indeterminate values and trap representations | N2861 | N/A | |
| Remove `ATOMIC_VAR_INIT` | N2886 | N/A | |
| Remove trigraphs | N2940 | Yes | Never supported in any mode. |
| Improved normal enumerations (values wider than `int`) | N3029 | Unverified | |
| Relax requirements for `va_start` (single-argument form) | N2975 | No | `va_start(ap)` is rejected; planned for `c23!`. |
| Enhanced enumerations (fixed underlying type) | N3030 | Yes | |
| Freestanding C and IEC 60559 scope reduction | N2951 | N/A | |
| Unsequenced functions (`[[unsequenced]]`, `[[reproducible]]`) | N2956 | Accepted | |
| Comma omission and deletion (`__VA_OPT__`) | N3033 | Yes | |
| Underspecified object definitions | N3006 | Partial | Follows from `auto`/`constexpr` below. |
| Type inference for object declarations (`auto`) | N3007 | Yes | Several declarators are allowed (the standard requires one). |
| `constexpr` for object definitions | N3018 | Partial | Arithmetic objects only; they become constants. |
| Storage class specifiers for compound literals | N3038 | No | |
| Identifier primary expressions | N3034 | N/A | |
| Introduce the `nullptr` constant | N3042 | Partial | `nullptr` is a null `void *`; `nullptr_t` is a typedef of it. |
| Memory layout of unions | N2929 | N/A | |
| Improved tag compatibility | N3037 | Unverified | |
| `#embed` | N3017 | No | |
| `__has_include` | N2799 | Yes | Resolved exactly as `#include` is. GNU's `__has_include_next` too. |

## C2y

Nothing from C2y is implemented as C2y; `c2y!` does not exist. One of the
features Clang lists is already here under another name: case ranges (N3370)
are GNU's `case 1 ... 5:`, which every entry point accepts. `_Countof` (N3369),
named loops (N3355) and `if` declarations (N3356) would be the natural first
candidates when a `c2y!` entry point is added.
