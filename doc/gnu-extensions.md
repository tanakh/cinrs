# GNU C extensions: what exists, how common they are, and what cinrs does with them

`cinrs` implements the C standards (`c89!`/`c90!`, `c99!`, `c11!`, `c17!`,
`c23!`) and, on top of them, the GNU extensions (`gnu89!`, `gnu99!`, `gnu11!`,
`gnu17!`, `gnu23!`). Real-world
C leans on those extensions heavily, so this document lists them, estimates how
often each shows up in *user* code, and tracks what cinrs supports. It is the
place to update when an extension lands.

The frequencies are for user code rather than for headers: cinrs's own bundled
headers are plain C, and the platform's — which `#pragma cinrs system_include`
makes readable, see [`doc/system-headers.md`](system-headers.md) — are a
different population entirely, where `__attribute__`, `__extension__`,
`__asm__` labels and `#include_next` are everywhere. What those headers need is
the reason several rows below say "supported" rather than "refused".

The catalogue follows the chapter "Extensions to the C Language Family" of the
GCC manual, plus the preprocessor extensions from the CPP manual and the
pragmas. Clang implements nearly all of them, so "GNU" below means "GCC and
Clang".

## How to read the tables

* **Frequency** is an estimate for application/library code found in the wild
  (kernels, embedded firmware, interpreters, codecs, CLI tools):
  *very common* — you will meet it in most non-trivial code bases;
  *common* — regularly, especially in macros and portability layers;
  *occasional* — in specific domains (interpreters, kernels, SIMD);
  *rare* — almost never outside compiler test suites.
* **Status**: 🟢 `supported`, 🟢 `accepted` (parsed and ignored where ignoring
  is semantically safe), 🟡 `partial` (the common shapes work and the rest is
  refused with the reason), 🟡 `refused` (recognised and rejected with the
  reason, so that nothing is silently mistranslated), 🟠 `planned`,
  🔴 `not planned`, ⚫ `impossible` (no stable Rust counterpart), or
  ⚪ `—` (not yet decided).
* Where an extension later became standard C, it says so; those are handled by
  the standard gating (`c11!`/`c23!`), and the row is only about using the
  feature in an *older* entry point.

## Strict and GNU entry points

GCC distinguishes `-std=c99` from `-std=gnu99`, and `cinrs` draws the line in
exactly the same place.

1. **Everything spelled with a leading double underscore is available in every
   entry point** — `__typeof__`, `__attribute__`, `__extension__`, `__asm__`,
   `__inline__`, `__restrict`, `__builtin_*`, `__auto_type`, `__label__`. Those
   names are reserved to the implementation, so nothing a program may legally
   call its own is taken away by claiming them, and that is why GCC's strict
   modes keep them too. A `c99!` block gets statement expressions,
   `__attribute__((packed))` and `__builtin_popcount` without asking.
2. **The plain spellings need a GNU entry point.** `typeof` and `asm` are
   ordinary identifiers in ISO C — a C99 program may have a variable called
   `typeof`, and one of the c-testsuite cases has a variable called `asm` — so
   they are keywords only in `gnu89!`, `gnu99!`, `gnu11!`, `gnu17!` and
   `gnu23!` (`typeof` is also C23's own keyword, so `c23!` has it as well). The
   diagnostic in a strict block names both ways out:

   ```text
   error: 'typeof' requires a GNU dialect (gnu99!) or C23 or later (this block is c99!)
   ```

3. **A GNU entry point accepts what a later revision added**, exactly as
   `gcc -std=gnu99` does: `_Static_assert`, `_Generic`, `_Alignof`, `_Alignas`,
   `_Noreturn`, anonymous `struct`/`union` members, `0b` literals, digit
   separators, `__VA_OPT__`, `#elifdef`, the empty initialiser `{}` — every
   construct the strict entry points gate. The gate is simply switched off; the
   strict entry points keep it, and keep saying which macro to write instead.
   `gnu89!` is where that matters most: `gcc -std=gnu89` takes `//` comments,
   `long long`, mixed declarations and code, designated initializers and the
   rest as extensions, and so does this.

   The three rules that are *not* extensions, because a later revision
   **deleted** them, go the other way — a GNU dialect cannot switch a deleted
   rule back on, so they belong to the revision:

   | rule | where it lives | removed by |
   | --- | --- | --- |
   | implicit `int` (`static x;`, `f() { … }`) | `c89!`, `gnu89!` | C99 (N635) |
   | implicit function declarations (calling an undeclared `f`) | `c89!`, `gnu89!` | C99 (N636) |
   | old-style (K&R) definitions, `int f(a) int a; { … }` | every entry point below `c23!` | C23 (N2432) |

   That is GCC 14's own behaviour: it errors on the first two in `-std=gnu99`
   and later, and on the third in `-std=c23`.

4. **`__extension__` switches the gate off** for the declaration it is written
   on, which is what it means in GCC — "this is an extension and I know it".
   The bundled headers put it in front of their `long long` declarations, so
   that `#include <stdlib.h>` in a `c89!` block declares `llabs` rather than
   reporting the header, exactly as glibc's own headers do; a program that
   wants the same bargain may write it too.

5. **The predefined macros say which entry point it is.** `__STRICT_ANSI__` is
   defined in the strict entry points only, and `__STDC_VERSION__` is not
   defined at all in `c89!` and `gnu89!` — C89 as published had no such macro. `__GNUC__` is `4`,
   `__GNUC_MINOR__` `2` and `__GNUC_PATCHLEVEL__` `1` in *all* of them —
   Clang's own precedent, and for the same reason: a program guards
   `__attribute__` and `__builtin_expect` with
   `#if defined(__GNUC__) && __GNUC__ >= 4`, and those work here. `__VERSION__`
   names cinrs and its version, and nothing claims to be Clang.

Two of C11 6.10.8.3's subsetting macros are defined as `1` where the part they
name is really absent, which turns each gap into the conforming omission the
standard provides for — and are absent otherwise. `__STDC_NO_THREADS__`
follows the *target model*: the bundled `<threads.h>` declares the platform's
own threads, so it exists on the targets whose C library cinrs can lay the
objects out for — glibc and musl, both on Linux — and refuses on the rest,
where the macro is defined. `__STDC_NO_COMPLEX__` follows the `complex` cargo
feature, which is on by default, so it too is *not* normally defined. The
other two, `__STDC_NO_ATOMICS__` and `__STDC_NO_VLA__`, are never defined:
atomics and the variably modified types — `int a[n]`, `double a[n][m]`,
`int (*p)[n]`, `typedef int T[n];` and the parameter forms — are implemented.
See [`doc/c-status.md`](c-status.md#c99).

## Language extensions

| Extension | Example | Frequency | Status | Notes |
| --- | --- | --- | --- | --- |
| Statement expressions | `({ int t = x; t * 2; })` | very common (macros: `min`/`max`, container_of, kernel-style) | 🟢 supported | A Rust block expression. Declarations, loops, nested statement expressions and `return` all work; a *label* or a `goto` inside one is refused, because how a function's jumps are lowered is decided from its statements and a jump buried in an expression would be dropped. In a function whose jumps need the [control-flow graph](../crates/cinrs-core/src/cfg.rs), a `break` or `continue` that leaves the statement expression is refused too — there is no Rust loop left to leave. |
| `typeof` / `__typeof__` | `typeof(x) y = x;` | very common (macros) | 🟢 supported | `__typeof__` and `__typeof` everywhere; `typeof` in a GNU dialect and in `c23!`. `__typeof_unqual__` is the same thing here, since the type model carries no top-level qualifiers. |
| `?:` with omitted middle operand | `p ?: default` | common | 🟢 supported | The operand is evaluated exactly once, into a temporary. |
| `?:` with one `void` operand | `x ? (void) 0 : f()` | occasional (macros that act only sometimes) | 🟢 supported | ISO C wants both operands to be `void` or neither; GCC takes one of each in every mode it has — `-std=c99` included, where it is only a pedantic warning — so this does too, in every entry point. The other operand's value is discarded and the conditional has type `void`, which makes it a statement wherever it is written and `()` wherever a value is expected. |
| `__attribute__((…))` on functions, variables, types | see the attribute table below | very common | 🟢 supported | Parsed in every position GCC accepts it — declaration specifiers, after a declarator, inside one (`int (__attribute__((x)) *)(void)`), on records, members, parameters, labels and statements — in both the `name` and `__name__` spellings. A subset is honoured, the rest is ignored as C23 allows, and the handful that would change the program's meaning is refused. |
| Alternate keywords | `__inline__`, `__asm__`, `__const__`, `__signed__`, `__volatile__`, `__restrict`, `__restrict__`, `__alignof__`, `__extension__` | very common (portability macros) | 🟢 supported | The [preprocessor](../crates/cinrs-core/src/pp.rs) turns them into keywords on the way to the parser — *after* macro replacement, so `#define __attribute__(x)` still defines and expands a macro of that name, which is what portability headers write. `__inline__` and `__restrict__` are keywords of their own rather than the plain spellings, because `c89!` gates `inline` and `restrict` and must not gate these. `__extension__` also switches that gate off for the declaration it is written on; see [above](#strict-and-gnu-entry-points). |
| `__builtin_expect`, `__builtin_expect_with_probability` | `if (__builtin_expect(x, 0))` | very common (`likely`/`unlikely`) | 🟢 supported | The value is the first argument, typed `long` as GCC types it; `core::hint::likely` is unstable, so the hint has nowhere to go. |
| `__builtin_unreachable`, `__builtin_trap` | | common | 🟢 supported | `core::hint::unreachable_unchecked()`, and a call to the C library's `abort` — `core::intrinsics::abort` is unstable and `std` is not available to a `no_std` crate. |
| Bit-manipulation builtins | `__builtin_popcount(l,ll)`, `__builtin_clz`, `__builtin_ctz`, `__builtin_ffs`, `__builtin_parity`, `__builtin_bswap16/32/64`, `__builtin_clrsb` | common (codecs, hashing) | 🟢 supported | Rust's integer methods on the unsigned type of the operand's width. `clz(0)` and `ctz(0)` are undefined in C and answer the width here, which is what Rust does. |
| Overflow-checking builtins | `__builtin_add_overflow(a, b, &r)`, `mul_overflow`, `sub_overflow`, the `_p` forms and the typed `__builtin_sadd_overflow` family | common (security-conscious code) | 🟢 supported | The arithmetic happens in `i128` and the value says whether the result survived the conversion to the type it is stored in — which is exactly "compute in infinite precision, then convert", including when the operands and the result have different types. |
| `__builtin_constant_p` | `__builtin_constant_p(x)` | common (kernel macros) | 🟢 supported | 1 when the operand folds, 0 otherwise — and 1 for the address of a string literal and for a character read out of one at a constant index, both of which GCC calls constant. The address of an *object* is not: the linker decides it. |
| `__builtin_types_compatible_p`, `__builtin_choose_expr` | generic macros | occasional | 🟢 supported | Both are constants; only the operand `choose_expr` picks is type checked. |
| Library builtins | `__builtin_memcpy`, `__builtin_memset`, `__builtin_strlen`, `__builtin_abs`, `__builtin_sqrt`, `__builtin_printf`, … | common (via headers, sometimes direct) | 🟢 supported | A call to the library function of that name. GCC knows the prototype of every function it has a builtin for and declares it on the spot rather than making the program include the header first, and so does this: the prototype is the one the bundled header writes, so a `#include` that arrives later redeclares it compatibly. `crate::gnu::LIBRARY_BUILTINS` is the list and `Sema::library_signature` the prototypes — `<string.h>`, `<stdlib.h>`, `<ctype.h>`, all of `<math.h>` and the three of `<stdio.h>` that can be written without `FILE` (`printf`, `sprintf`, `snprintf`, and `puts`/`putchar`), plus the GNU spellings `mempcpy`, `stpcpy`, `stpncpy`, `bzero`, `bcopy`, `bcmp`, `index`, `rindex`, `strcasecmp` and `strdup`. The three that need a type only a header can introduce — `fprintf`, `fputs`, `fputc`, which take a `FILE *` — say which header to include, and so does a name outside the list. A **`long double`** maths builtin is the `double` one: `__builtin_sqrtl` calls `sqrt`, because calling the platform's `sqrtl` would hand it an eighty-bit value the generated Rust cannot make. |
| Floating classification and comparison builtins | `__builtin_isnan`, `isinf`, `isinf_sign`, `isfinite`, `isnormal`, `issignaling`, `signbit(f/l)`, `fpclassify`, `isunordered`, `isgreater(equal)`, `isless(equal)`, `islessgreater` | common (via `<math.h>`'s macros) | 🟢 supported | Answered in `core`, with no maths library involved: the classifications are `f64::is_nan` and its relatives, `signbit` is `is_sign_negative` (so a negative zero answers 1), `issignaling` is a NaN with the quiet bit clear, and `fpclassify` is a `match` on `f64::classify`. The six comparisons are the Rust operators of the same name, which are the *quiet* ones C99 7.12.14 asks for; the operands go through the usual arithmetic conversions and each is evaluated once. A non-floating argument is refused, as GCC refuses it. |
| `__builtin_fabs(f/l)`, `__builtin_copysign(f/l)` | | common | 🟢 supported | Bit manipulation rather than a call: the sign bit cleared, and the sign bit taken from the other operand. Exact for a zero and for a NaN, which is what makes them different from `x < 0 ? -x : x`. |
| `__builtin_inf(f/l)`, `__builtin_huge_val(f/l)`, `__builtin_nan(f/l)`, `__builtin_nans(f/l)` | `__builtin_nan("0x123")` | common (via `<math.h>`) | 🟢 supported | Constants, so each may initialise an object with static storage duration. The **payload** the string names is honoured — read the way `strtoull` reads it, in base 0, so `"0x123"` and `"291"` are the same NaN — and an empty string is the default: no payload for a quiet NaN, and the leading payload bit for a signalling one, which is what GCC produces. A NaN whose bits are not the default quiet one is written out as `f64::from_bits(…)` (or `f32::from_bits`), because `f64::NAN` is one particular NaN and a conversion between the two widths is free to lose both the payload and the sign. |
| `__builtin_classify_type` | `__builtin_classify_type(x) == 8` | rare (outside GCC's own testsuite) | 🟢 supported | GCC's number for the class of the operand's type, and an integer constant expression, so it may be an array bound. The operand is not evaluated and is classified by the type the *default argument promotions* give it, which is why `char`, `_Bool` and an enumeration all answer `1` and an array and a function both answer `5`. The codes are GCC's: 0 `void`, 1 integer, 5 pointer, 8 real, 10 function, 12 `struct`, 13 `union`, 14 array. |
| `__builtin_offsetof` | `offsetof(struct S, a[2].b)` | common (via `offsetof`) | 🟢 supported | The implementation of `<stddef.h>`'s `offsetof`, folded to an integer constant out of the layout sema computed — so it may be an array bound or a `case` label, and the member designator takes `.member` and `[expr]` steps and reaches through anonymous members. |
| `__builtin_va_list`, `__builtin_va_start/arg/end/copy` | | common (via `<stdarg.h>`) | 🟢 supported | The implementation of `<stdarg.h>`. `__builtin_va_arg` takes an aggregate type as well as a scalar: a `struct`, a `union` or a complex value of at most sixteen bytes is rebuilt from the eightbytes the x86-64 System V ABI passed it in, which is the only ABI it is supported on. See [`doc/c-status.md`](c-status.md) for the classification and what it refuses. |
| `__func__`, `__FUNCTION__`, `__PRETTY_FUNCTION__` | logging macros | common | 🟢 supported | `__func__` is standard C99. All three are the function's name as a `const char[]`, so `sizeof(__func__)` is its length, and the byte string is emitted only where one is used. |
| `__COUNTER__` | unique identifiers in macros | common | 🟢 supported | A fresh integer at every use. |
| Case ranges | `case 1 ... 5:` | common (interpreters, lexers) | 🟢 supported | One Rust range pattern, in both the structured and the control-flow-graph lowering. An *empty* range (`case 5 ... 1:`) is refused rather than warned about: GCC's warning leaves an arm nothing can enter, and the only way to write one is by mistake. |
| Designated-initializer ranges | `[1 ... 5] = 0` | occasional | 🟢 supported | The value is checked once and written into every element of the range. |
| Old designator syntax | `{ x: 1, y: 2 }` | rare (pre-C99 code) | 🟢 supported | Read as `.x = 1`. |
| Zero-length arrays | `int data[0];` | common (before C99 flexible members) | 🟢 supported | A `[T; 0]` member, which is what it already was. |
| Flexible array members (standard C99) | `int data[];` | common | 🟢 supported | A `[T; 0]` tail member; `sizeof` leaves it out and indexing it is the pointer arithmetic it always was. Must be the last member of a `struct`. |
| Initialising a flexible array member | `static struct W w = { 3, { 1, 2, 3 } };` | occasional | 🟢 supported | GCC allows it — with a warning — for an object with **static storage duration**, whose storage it can make as large as the initialiser asks, and this does the same. The object's item is given a *companion* type with the record's leading layout and a tail as long as the initialiser (`#[repr(C)] struct __cinrs_W_3 { n: c_int, data: [c_int; 3] }`), the C object is the record at its address, and every use of it goes through `(*(&raw mut w).cast::<W>())`. `sizeof w` is still `sizeof(struct W)`, which is what GCC says too, and `#pragma cinrs export` exports the storage — the full size — under the C name. Rust code reads the *companion*: `w.data[2]`. The braces may be left out (`{ 3, 1, 2, 3 }`), a `char` tail takes a string literal, and the length may be inferred from an aggregate element type. Refused where GCC refuses it, in GCC's own words: on an **automatic** object ("non-static initialization of a flexible array member") and on a record **nested inside another aggregate** — an array of them, a member, or a compound literal — ("initialization of flexible array member in a nested context"), which is the one place GCC is laxer, since it takes `union B { struct A a; char b[N]; } b = { { 1, "…" } };`. `tests/gnu_language.rs`, `tests/ui/flexible_array_initializer.rs` |
| Empty structures | `struct e {};` | occasional | 🟢 supported | A zero-sized `#[repr(C)]` item, size 0 as GCC gives it. An empty `union` is generated as an item with one `[u8; 0]` field, since Rust has no fieldless `union`. |
| Bit-fields of a type other than `_Bool`/`int`/`unsigned int` | `char flags : 3;`, `enum e kind : 8;`, `long long wide : 40;` | very common (protocols, kernels, compilers) | 🟢 supported | Standard C leaves any other type implementation defined. `cinrs` follows GCC and Clang: the allocation unit is `8 * sizeof(T)`, a named field raises the record's alignment to `alignof(T)`, and the integer promotions are the standard width-restricted ones applied to the wider types too — so `unsigned long x : 31` is an `int` and `unsigned long x : 33` keeps its declared type. A field the promotions do not reach also keeps its declared *width*: `unsigned long long b : 40` multiplies, adds, negates, complements and shifts in forty bits (6.7.2.1p10), and two such fields of different widths meet in the wider of the two. `tests/bitfield_layout.rs` probes that against the host compiler for every such field of its corpus. Signed overflow inside such a field wraps in the field's width, where GCC — for which the overflow is undefined — computes in the whole declared type. An `enum` bit-field follows the enumeration's underlying type, which is unsigned when no enumerator is negative; `cinrs` reads such a field back through `int`, so an `enum` field exactly as wide as `int` and holding a value with the top bit set differs from GCC (every narrower width agrees). |
| Incomplete `enum` types | `enum e; enum e *p;` | common | 🟢 supported | `int` until the enumerator list is seen, and `int` afterwards too — so the two mentions never disagree. A tag declared that way therefore gets no named Rust alias of its own, and is the one enumeration C23's widening (N3029) leaves alone. |
| Incomplete array types | `extern int j[];`, `int j[];` | common | 🟢 supported | C's own (6.2.5p22, 6.9.2p5) rather than an extension, and listed here because the two spellings turn up in every header pair: an `extern` declaration keeps the incomplete type, a file-scope tentative definition is completed to one element at the end of the unit, and a later `int j[3];` completes it sooner. `sizeof` of one is an error until it is completed, which is what GCC says too. |
| Arithmetic on `void *` and function pointers | `p + 1` with `void *p` | common | 🟢 supported (`void *`) / 🔴 not planned (function pointers) | `void *` arithmetic is byte-wise, which is `sizeof (void) == 1`; the GNU dialects answer that to `sizeof` and `__alignof__` as well. |
| Conversion between function pointers and `void *` | `void *p = f;` | common (dlsym users, callbacks) | 🟢 supported | ISO C forbids it and POSIX requires it; the two have the same size on every target here, and code generation makes the reinterpretation explicit. |
| Non-constant initializers for aggregates (standard C99) | `int a[2] = { x, y };` | — | 🟢 supported | Standard since C99. |
| Subscripting non-lvalue arrays | `f().a[0]` | rare | 🟢 supported | Falls out of the place lowering with a temporary. |
| Cast to a union type | `(union u) x` | rare | 🟢 supported | The value becomes the member whose type it has, and the result is an *rvalue* — which is the one thing that separates it from the compound literal `(union u){ x }`. A value no member has a type for is refused with that reason, and a strict entry point refuses the whole form and names the GNU entry point that has it. |
| Cast to the type the operand already has | `((struct X) x).a` | rare | 🟢 supported | 6.5.4p2's "scalar type" is about a *conversion*, and a cast to a compatible `struct` or `union` type has none to make; GCC and Clang both accept it silently. The result is a value with C11 6.2.4p8's temporary lifetime, which is what taking `.a` of it needs. |
| Mixed declarations and code, `//` comments, `long long`, `inline`, hex floats, variadic macros, compound literals, designated initializers | | — | 🟢 supported | Standard C99. |
| Named variadic macro parameters | `#define log(fmt, args…) …` | occasional (older code) | 🟢 supported | `args` is another spelling of `__VA_ARGS__`. |
| `, ## __VA_ARGS__` comma elision | `#define log(fmt, ...) printf(fmt, ## __VA_ARGS__)` | very common | 🟢 supported | The comma goes when the invocation passed no variable arguments, and the arguments are macro-replaced as usual when it did. `__VA_OPT__` (C23) is the standard way. |
| Dollar signs in identifiers | `$foo` | rare | 🟢 supported (flag exists, off by default) | Lexer option. |
| Extended characters written directly in an identifier | `int café(void);` | occasional | 🟢 supported | GCC has taken UTF-8 in identifiers since GCC 10 and Clang longer; C only ever spelled them as universal character names. Both spellings work here, in every entry point, and name the same identifier. See the [N717 row](c-status.md#c99) for the character set and for why a name that is not in Normalization Form C is refused. |
| Trigraphs off | `"what??!"` stays an exclamation | — | 🟢 supported | `gcc -std=gnu99` and Clang's GNU modes switch translation phase 1 off, and so do `gnu89!` … `gnu23!`. The strict entry points below `c23!` have them; see the [C99 row](c-status.md#c99). |
| `#embed` before C23 | `#embed "logo.png"` | new | 🟢 supported | GCC 15 takes the directive in every `-std=`, and a GNU dialect here does the same; a strict entry point below `c23!` refuses it with the usual gate. See the [N3017 row](c-status.md#c23). |
| `\e` character escape | `"\e[0m"` | occasional (terminal colours) | 🟢 supported | ESC, in every entry point, as GCC does. |
| Local labels | `__label__ retry;` | rare | 🟢 supported | Accepted and dropped: every label already has function scope here, and no two may share a name. |
| Labels as values (computed goto) | `void *t[] = { &&a, &&b }; goto *t[i];` | occasional (interpreters) | 🟢 supported | `&&label` is an rvalue of type `void *` whose value is the **state number** the label's block was given by the [CFG lowering](../crates/cinrs-core/src/cfg.rs), which a function that takes one is therefore always lowered through; `goto *e` is `__cinrs_state = e as usize as u32; continue 'cfg;`. It is an *address constant*, so a dispatch table may be a block-scope `static void *t[] = { &&a, &&b };`, and the ordinary uses all work: a `void *` variable, a `?:` between two of them, a `goto *` straight through one, and ordinary `goto`, `switch` and loops in the same function — an ordinary `goto` that would otherwise have become a [labelled block](../crates/cinrs-core/src/regions.rs) is a jump to a state here like any other. A label whose address is taken keeps a block of its own — nothing threads past it, nothing merges it away and nothing drops it for being unreachable — so its number is stable. GCC's **label difference**, `&&a - &&b`, is a constant here too and folds to the difference of the two numbers, which makes `static int off[] = { &&a - &&b, … }; goto *(&&b + off[i]);` work. Two limits: the value is a state number rather than a real address, so nothing may be read *through* it and arithmetic on one only means anything within the same function; and `&&label` naming a label of an **enclosing** function is GCC's nonlocal label address, which needs the frame pointer a lifted [nested function](#nested-functions) does not have — that is a located error naming the construct, and a label address that reaches another function any other way is simply the wrong number, as GCC's own documentation says of it. `tests/goto.rs` |
| Nested functions | `int f(void) { int g(int x) { … } }` | rare | 🟡 partial (lambda lifting) | **Supported by lifting**, in every entry point. GCC gives a nested function a *static chain* — a hidden pointer to the enclosing frame — and writes a **trampoline** onto the stack when its address is taken; nothing in Rust is one. So the definition is lambda-lifted instead: it becomes a file-scope item `__cinrs_<enclosing>_<g>`, private to the unit (never a C symbol, even under `#pragma cinrs export`), and every object of the enclosing function the body uses becomes a hidden `*mut T` parameter in front of the declared ones, named `__env_x` after the variable it carries. Each use of `x` inside the body becomes the place `*__env_x`, so assignment, `&x`, `x++`, subscripting, member access and `sizeof x` all keep meaning what they meant — and a store in `g` is visible in `f` the moment it returns, which is the whole point of the extension. A call `g(a)` becomes `__cinrs_f_g(&raw mut x, a)`. Recursion passes the same environment through; a **sibling or inner** function that uses an object of an outer level receives it through the middle one, which takes the pointer whether it mentions the variable or not; and a function that only *calls* a capturing sibling captures what the sibling does. A nested function that captures **nothing** is lifted to a plain function, so **its address may be taken** and it can be handed to `qsort`. `auto int g(int);` is GNU's forward declaration and works, mutual recursion included. `__func__` inside the body is the nested function's own name, a `static` local of it is its own item, and whether the *enclosing* function is lowered through a [CFG](../crates/cinrs-core/src/cfg.rs) says nothing about the nested one — each decides for itself. Five things are refused, by name: **the address of a capturing nested function** (`&g`, a decay, a callback), which is exactly what needs the trampoline and which names the variables in the way; a **nonlocal `goto`** (a jump from the nested body to a label of the enclosing function), which GCC reaches through the enclosing frame; **`&&label` naming a label of the enclosing function**, which is that jump one step earlier; and capturing a **variable length array** or a **`va_list`**, neither of which is a plain address — those two say "not supported yet". Clang implements nested functions not at all, so this is ground `c2rust` cannot cover. `tests/nested_functions.rs`, `tests/ui/gnu_nested_functions.rs` |
| `__int128` / `unsigned __int128` | `__int128 p = (__int128) a * b;` | occasional (crypto, hashing) | 🟢 supported | `i128`/`u128`, which have had `__int128`'s x86-64 ABI since Rust 1.77. Sixteen bytes, aligned the way the compiling toolchain aligns an `i128`, and ranked above `long long`, so the usual arithmetic conversions widen to it. `__int128_t` and `__uint128_t` are predefined `typedef` names for the same two types, as they are in GCC, and `__SIZEOF_INT128__` is `16`. Every entry point has it, the double underscore being what makes that safe — but only on a **64-bit architecture**: GCC refuses the type on a 32-bit one rather than emulating it, and so does this, leaving `__SIZEOF_INT128__` undefined there so that a portable program takes the other branch. The x32 ABI keeps it, the machine still being x86-64. See the [target model](c-status.md#the-target-model). C has no 128-bit *literal* and neither does this: `((__int128) 1) << 100` is the idiom, and `1 << 100` shifts an `int`. Bit-fields of it work, wider than sixty-four bits included. Two gaps, each a located error: `__builtin_add_overflow` and its relatives compute their check one width up from their operands and so have nowhere to go, which makes a 128-bit *operand* an error (a 128-bit *result* type is fine); and `va_arg(ap, __int128)` needs a `VaArgSafe` implementation Rust keeps behind the unstable `c_variadic_int128` feature, though *passing* one through `...` is unaffected — and a 128-bit *member* of a `struct` is fine, since [`va_arg` of a record](c-status.md) is read eightbyte by eightbyte and never at the member's own type. See `tests/int128.rs`. |
| `_Float128`, `__float128`, `_Float16`, decimal floats, fixed-point | | rare | 🔴 not planned | No stable Rust types (`f128`/`f16` are unstable). |
| Floating-constant suffixes beyond C's | `1.5q`, `1.5d`, `1.5w`, `1.5f128`, `0.5dd`, `2.0i` | rare | 🟢 supported / 🟡 refused | `d`, `w` (`__float80`), `q` (`__float128`) and the `_FloatN`/`_FloatNx` spellings `f64`, `f64x`, `f32x` and `f128` all name a format wider than `double`, and every one of them **is** `double` here — the same mapping `long double` has, and so the same loss of precision. `f32` is `float`. Each is accepted in a GNU dialect; spelled without underscores, they are refused in a strict entry point, which names the GNU one that has them. The **imaginary** suffixes `i` and `j` are supported, in either case and in either order beside `f` and `l`: `2.0i` is the pure imaginary `(0, 2)` of type `double _Complex`, `2.0if` is a `float _Complex` and `2.0il` a `long double _Complex` (which is a `double _Complex` here). They are C99's `_Complex` rather than a width, so they need no GNU dialect — only `c99!` or later, and the `complex` feature. Refused everywhere, with the reason: the decimal suffixes `df`, `dd` and `dl` (radix-10, and no Rust type is), the imaginary suffix on an *integer* constant (`3i` is GCC's `_Complex int`; the message says to write `3.0i`), and `f16`/`bf16` (Rust's `f16` is unstable, and widening the constant would change what the program computes). `tests/ui/gnu_float_suffixes.rs` |
| `_Complex`, `__complex__` (standard C99) | `double _Complex z = 1.0 + 2.0i;` | rare | 🟢 supported | `float _Complex` and `double _Complex` are [`cinrs::rt::Complex<f32>`](https://docs.rs/num-complex) and `Complex<f64>`; `long double _Complex` is `double _Complex`, the mapping `long double` already has. Needs the `complex` feature of the `cinrs` crate, which is on by default and is what supplies that runtime; without it `_Complex` is a diagnostic naming the feature, and `__STDC_NO_COMPLEX__` is predefined. `__complex__` and `__complex` are the reserved spellings and work in every entry point; a complex *integer* type (`_Complex int`, `__complex__ char`) is a GNU extension of its own and is refused with the reason. See the [`<complex.h>` row](c-status.md#c99) for what the arithmetic does. `tests/complex.rs` |
| `__real__`, `__imag__` | `__imag__ z = 1.0;` | rare | 🟢 supported | The two halves of a complex value, and **lvalues** whenever the operand is one — which is what lets a program assign to half of an object. On a *real* operand, which GCC also allows, `__real__ x` is `x` and `__imag__ x` is a zero of `x`'s type (and not an lvalue). The plain spellings `__real` and `__imag` work too. |
| `~z` as conjugation | `double _Complex w = ~z;` | rare | 🟢 supported | GNU gives `~` a second meaning on a complex operand: the conjugate, which is `conj(z)`. Available in every entry point, as it is in GCC's strict modes — the operator is undefined for complex operands in ISO C rather than reserved. `__builtin_conj`, `__builtin_creal`, `__builtin_cimag` and `__builtin_cproj` are here too, with their `f` and `l` forms, and so is `__builtin_complex`, which C11 blessed as `CMPLX`. |
| Vector extensions | `typedef int v4si __attribute__((vector_size(16)));` | occasional (SIMD) | 🟡 refused | `core::simd` is unstable. |
| Inline assembly | `asm volatile("…" : "=r"(x) : "r"(y) : "memory")` | occasional (kernels, crypto) | 🟡 refused | Rust has `core::arch::asm!`, but mapping GCC's operand constraints onto its own is a project rather than a feature, and half a translation of assembly is worse than none. The `asm` **label** on a declaration — `int f(void) __asm__("f_impl");` — is a different thing and is supported. |
| `asm` labels on declarations | `int f(void) __asm__("f_impl");` | common (libc shims) | 🟢 supported | On a declaration the unit does not define, `#[link_name = "…"]`; on one it defines, `#[unsafe(export_name = "…")]`. |
| `__thread` | `__thread int counter;` | occasional | 🟢 supported | GCC's spelling of `_Thread_local`, and — being reserved — available in every entry point. The object becomes a `std::thread_local!` holding an `UnsafeCell<T>`, and every C access goes through the `*mut T` its `with` hands out, which is valid for as long as this thread's copy is. Rust's own `#[thread_local]` is still unstable, so an `extern` thread-local object — one another object file defines — is a located error; so is exporting one under `#pragma cinrs export`. `thread_local!` lives in `std`, which makes this the third construct an expansion cannot have under `#pragma cinrs no_std`, after variable length arrays and `alloca`. See the [N1364 row](c-status.md#c11) and `tests/threads.rs`. |
| `__atomic_*` builtins | `__atomic_fetch_add(&n, 1, __ATOMIC_RELAXED)` | occasional (lock-free code) | 🟢 supported | The whole memory-order-aware family: `load_n`/`load`, `store_n`/`store`, `exchange_n`/`exchange`, `compare_exchange_n`/`compare_exchange`, `fetch_add` … `fetch_nand`, `add_fetch` … `nand_fetch`, `test_and_set`, `clear`, `thread_fence`, `signal_fence`, `always_lock_free` and `is_lock_free`. Each becomes a `core::sync::atomic` type reached with `AtomicX::from_ptr` over the object's address — `AtomicI8`…`AtomicU64` for the 1-, 2-, 4- and 8-byte integers, `AtomicBool` for a `_Bool`, the integer atomic of the same width plus `to_bits`/`from_bits` for a `float` or `double`, and `AtomicPtr` for an object pointer. Rust has no integer `fetch_nand`, so the two nand forms are the compare-exchange loop it would have been. The order must be an integer constant expression — the `__ATOMIC_*` macros are predefined in every entry point — and a non-constant one falls back to `__ATOMIC_SEQ_CST` as GCC documents; an order the operation may not have (a load that releases, a store that acquires, a failure order stronger than the success one) is a diagnostic rather than the run-time panic Rust would give. Arithmetic on a *pointer* object counts **bytes**, which is GCC's behaviour and the one place this family and `<stdatomic.h>` differ. Refused: a 128-bit object (no stable `AtomicU128`), a function pointer, and arithmetic on a floating object, which GCC also rejects. See `tests/atomics.rs`. |
| `__sync_*` builtins | `__sync_fetch_and_add(&n, 1)` | occasional (older lock-free code) | 🟢 supported | The older family, every one of them sequentially consistent: `fetch_and_add`/`add_and_fetch` and their four relatives, `bool_compare_and_swap`, `val_compare_and_swap`, `lock_test_and_set` (an acquire exchange), `lock_release` (a release store of zero) and `synchronize`. The trailing arguments GCC allows — the list of variables the barrier covers — are evaluated and ignored. `__GCC_HAVE_SYNC_COMPARE_AND_SWAP_1/2/4/8` are predefined. |
| `__c11_atomic_*` builtins | `__c11_atomic_load(&x, __ATOMIC_SEQ_CST)` | rare in user code | 🟢 supported | Clang's family, which the bundled `<stdatomic.h>` is written over exactly as Clang's own header is. They require the object to be `_Atomic`, and their arithmetic on a pointer object is **scaled** by the pointee's size, as C11 7.17.7.5 requires — which is the reason both families exist here. |
| `__auto_type` | `__auto_type x = expr;` | rare | 🟢 supported | C23's `auto` under another name, and available in every entry point. |
| Escaped newlines with trailing whitespace | `\ ` + newline | rare | 🟠 planned | Lexer leniency. |
| `__builtin_return_address`, `__builtin_frame_address`, `__builtin_apply` | | rare | 🔴 not planned | |
| `__builtin_LINE`, `__builtin_FILE`, `__builtin_FUNCTION` | | rare | 🟢 supported | Predefined macros for `__LINE__`, `__FILE__` and `__func__`, so each reports the *use*. |
| `__builtin_object_size`, `__builtin_dynamic_object_size` | fortified headers | rare in user code | 🟢 supported | `(size_t) -1` and `0`, the two answers GCC documents for "unknown". |
| `__builtin_alloca`, `alloca()`, `__builtin_alloca_with_align` | `char *p = alloca(n);` | occasional | 🟢 supported (emulated) | Rust has no stack allocation with a size chosen at run time, so the memory comes from a per-function arena on the heap: one 16-byte aligned block per call, all of them freed when the function returns — which is `alloca`'s own lifetime, and why a pointer that outlives the call dangles here exactly as it does in C. The bundled `<alloca.h>` defines the plain name in terms of the builtin, the way every platform's own header does. `__builtin_alloca_with_align` takes its alignment in bits and accepts up to 128, which the arena already satisfies. The arena is a `Vec`, so this and variable length arrays are the only two constructs whose expansion needs more than `core`; see the crate documentation for `#pragma cinrs no_std`. |
| `__builtin_prefetch`, `__builtin_assume_aligned`, `__builtin_assume`, `__builtin_speculation_safe_value` | | occasional | 🟢 supported (no-ops) | The operands are still evaluated; `assume_aligned` gives the pointer back. |
| Non-standard predefined macros | `__GNUC__`, `__GNUC_MINOR__`, `__VERSION__`, `__STRICT_ANSI__`, `__BASE_FILE__`, `__FILE_NAME__`, `__INCLUDE_LEVEL__`, `__TIMESTAMP__`, `__x86_64__`, `__linux__`, `__SIZEOF_INT__`, `__CHAR_BIT__`, `__INT_MAX__`, `__BYTE_ORDER__`, `__ORDER_LITTLE_ENDIAN__` | very common (portability `#if`s) | 🟢 supported | See [Strict and GNU entry points](#strict-and-gnu-entry-points) for what `__GNUC__` commits to. `__TIMESTAMP__` is a fixed placeholder like `__DATE__`, so a build gives the same output twice. **Every macro that describes the machine comes from the [target model](c-status.md#the-target-model), not from the host**, so `CINRS_TARGET` or `#pragma cinrs target` changes all of them together: the architecture (`__x86_64__`, `__i386__`, `__aarch64__`, `__arm__`, `__riscv` with `__riscv_xlen`, `__wasm32__`, `__powerpc64__`, `__s390x__`, `__mips__`, `__sparc__`, `__loongarch__`), the system (`__linux__`, `__gnu_linux__`, `__unix__`, `__APPLE__`, `__MACH__`, `_WIN32`, `_WIN64`, `__FreeBSD__`, `__NetBSD__`, `__OpenBSD__`, `__wasi__`), the object format (`__ELF__`), the data model (`__LP64__`/`_LP64`, `__ILP32__`/`_ILP32`, `__CHAR_UNSIGNED__`, `__WCHAR_UNSIGNED__`, `__CHAR_BIT__` and the `__SIZEOF_*__` family), the byte order (`__BYTE_ORDER__` against `__ORDER_LITTLE_ENDIAN__`/`__ORDER_BIG_ENDIAN__`), and the limits and library types (`__INT_MAX__` … `__LONG_LONG_MAX__`, the `__*_WIDTH__` set, `__SIZE_TYPE__`, `__PTRDIFF_TYPE__`, `__INTPTR_TYPE__`, `__INTMAX_TYPE__`, `__WCHAR_TYPE__`, `__WINT_TYPE__`, the `__INTn_TYPE__` and `__INT_LEASTn_*` families). `__SIZEOF_INT128__` is defined only where `__int128` exists, which is the 64-bit architectures; `intmax_t` is 64 bits on every target, so `__INTMAX_TYPE__` is `long long int` under ILP32 and LLP64 and `long int` under LP64, exactly as GCC has it. `__INT_MAX__` and the rest of the limit family *are* predefined here (GCC's own `<limits.h>` is written in terms of them); the bundled `<limits.h>` and `<stdint.h>` then repeat them under their standard names. Not claimed: `_MSC_VER` and `__MINGW32__`, since neither the Microsoft nor the mingw extensions are implemented. |

## `__attribute__` forms

Every attribute is parsed. One it does not know is dropped, which C23 6.7.13.1p3
explicitly allows and which is what GCC does with a warning this crate has no
way to raise; one it knows but cannot honour is refused, because ignoring it
would change what the program means. Both the `name` and the `__name__`
spellings work, and so does C23's `[[gnu::name]]`.

| Attribute | Frequency | Status | What it does here |
| --- | --- | --- | --- |
| `noreturn` | very common | 🟢 supported | Exactly what `_Noreturn` does: a call to the function ends the statement it is in. |
| `packed` (record or member) | very common (protocols, file formats) | 🟢 supported | `#[repr(C, packed)]`, with explicit padding where a member moved. On one *member* it packs that member alone. Any packing also switches the bit-field allocation-unit rule off, which is GCC's own rule and easy to miss. A member the packing left underaligned is read and written through `read_unaligned`/`write_unaligned`, since a plain `*p` on an underaligned pointer is undefined behaviour in Rust and an abort in a debug build; so is one reached through a pointer cast that lands on an underaligned address, which is what punning a `char` buffer to a record does. |
| `aligned(N)` | very common | 🟢 supported | On a record, `#[repr(C, align(N))]`; on a member, the same request `_Alignas(N)` makes — the member moves to the boundary and explicit padding puts it there. On an **object**, the binding is generated inside a `#[repr(C, align(N))]` wrapper of its own and every use of it goes through the wrapper's one field; see the [`_Alignas` row](c-status.md#c11). Bare `aligned` with no argument asks for the target's `max_align_t`, which is 16 on every ABI here. GCC's rule that the attribute "can only increase alignment" is kept: a weaker request on an object is *not applied*, and is not a diagnostic either — where an `_Alignas` weaker than the type's own alignment is the constraint violation C11 6.7.5p4 makes it. On a `typedef` of an *anonymous* record — `typedef struct { … } T __attribute__((aligned(16)));` — the record itself takes the alignment, which says the same thing, since the typedef name is the only way to name it; on a `typedef` of anything else, including a tagged record, it is dropped, because GCC makes it a property of the typedef alone and this type model has no room for such a variant. |
| `always_inline`, `noinline`, `cold`, `hot` | very common | 🟢 supported | `#[inline(always)]`, `#[inline(never)]`, `#[cold]`; `hot` cancels `cold`. |
| `flatten` | common | 🟢 accepted | Ignored. |
| `format(printf, i, j)`, `format_arg` | very common (logging APIs) | 🟢 accepted | Diagnostic-only in GCC. |
| `deprecated`, `deprecated("message")` | common | 🟢 supported | `#[deprecated]`, so Rust code that calls the function is warned. |
| `warn_unused_result` | common | 🟢 accepted | Ignored; `#[must_use]` would make the *C* calls warn too. |
| `constructor`, `destructor` (with or without a priority) | common (plugin registration, test frameworks) | 🟢 supported | A `#[used]` function pointer in `.init_array` / `.fini_array` on ELF and `__DATA,__mod_init_func` / `__mod_term_func` on Apple. The priority is parsed and ignored. A target with neither table is a `compile_error!`, because a constructor that silently never ran would be worse. |
| `section("…")` | common in libraries | 🟢 supported | `#[unsafe(link_section = "…")]` on a function or on an object with static storage duration. |
| `visibility("…")` | common in libraries | 🟢 accepted | Only matters with `#pragma cinrs export`. |
| `weak` | occasional | 🟡 partial | Two answers, because the attribute asks two different things. On a **definition** — a function with a body, or an object this unit reserves storage for — it is refused: Rust's `#[linkage]` is unstable, so there is no way to let the linker replace the symbol. On a **declaration** of something defined elsewhere it is accepted and ignored, because the only thing lost is that the reference can go unresolved and its address be null, and refusing it would make glibc's own `<pthread.h>` unreadable over one internal function nobody calls. `__has_attribute(weak)` still answers **0**, which is the honest answer to the question a program really asks with it — "can I test a weak reference for null?" — so a guarded program takes the portable branch and an unguarded declaration still compiles. `tests/ui/gnu_unsupported.rs` |
| `alias("…")`, `weakref`, `ifunc` | occasional | 🟡 refused | `#[linkage]` is unstable and there is no stable way to give one symbol a second name; write a function that forwards instead. |
| `cleanup(f)` | occasional (glib, systemd) | 🟢 supported | `f(&x)` on every way out of the scope `x` was declared in, in reverse declaration order — the `_cleanup_free_` idiom. The structured lowering binds a drop guard right after the object, so Rust's own drop order *is* C's — and a `goto` that leaves the scope is a `break` out of the Rust block it is, which drops what that block holds; the [CFG](../crates/cinrs-core/src/cfg.rs) one has no scopes to drop in and emits the call on each edge that leaves one, which is what runs it once per pass through a loop body. `return expr;` computes its value first, as GCC does. The function must take one argument, a pointer to the variable's type (`void *` included); on a parameter, a `static`, a thread-local or a file-scope object GCC drops the attribute with a warning and cinrs refuses it with the reason. A `goto` *into* the scope is allowed and the cleanup still runs, which is GCC's behaviour. `tests/cleanup.rs` |
| `mode(M)` | rare (portability headers, GCC's own testsuite) | 🟢 supported | The declared type is replaced by the one the machine mode names: the *width* comes from the mode and the *signedness* from the type that was written, so `typedef unsigned int u8 __attribute__((mode(QI)))` is the one-byte unsigned integer. `QI`/`HI`/`SI`/`DI`/`TI` are 1, 2, 4, 8 and 16 bytes, `byte` is 1, `word`, `pointer` and `unwind_word` are as wide as `size_t`, `SF` and `DF` are `float` and `double`, and `SC` and `DC` are `float _Complex` and `double _Complex`; the `__M__` spelling of each works too. Honoured on a `typedef`, on a member and on an object. Refused with the reason, rather than rounded to something else: `TI` where the target model has no `__int128`, the extended and quad floating formats (`XF`, `TF`, `KF`, `IF`, `HF`, `BF`) and the complex modes over them (`XC`, `TC`, `KC`, `HC`), the vector modes (`V4SI`, …), and a mode on a type that is not arithmetic. `tests/ui/gnu_mode_errors.rs` |
| `scalar_storage_order("…")` | rare (file formats, network structs) | 🟡 refused | It reverses the byte order of *every scalar* in the record, and nothing in the generated Rust could carry that. Ignoring it would silently change what the program reads, which is why it is named here rather than dropped as an unknown attribute; `execute/20230630-2` and its four relatives are the cases that noticed. |
| `vector_size(N)` | occasional | 🟡 refused | See vector extensions. |
| `nonnull`, `returns_nonnull`, `malloc`, `pure`, `const`, `leaf`, `nothrow`, `access(…)`, `alloc_size`, `alloc_align`, `sentinel`, `returns_twice`, `no_sanitize`, `noclone`, `noipa`, `optimize(…)`, `target(…)`, `error(…)`, `warning(…)`, `designated_init`, `artificial`, `gnu_inline`, `externally_visible` | common in library headers, rare in bodies | 🟢 accepted | Optimisation and diagnostic hints only. |
| `transparent_union`, `may_alias`, `nonstring` | rare | 🟢 accepted | Ignored. |
| `unused`, `used`, `maybe_unused` | very common | 🟢 accepted | Every generated item already carries `#[allow(dead_code)]`. |
| `fallthrough` | common | 🟢 supported | Accepted and dropped: a `switch` group falls through in the generated Rust either way. |
| `nodiscard`, `unsequenced`, `reproducible` | rare | 🟢 accepted | C23's, and answered by `__has_c_attribute`. |
| `stdcall`, `cdecl`, `fastcall` | rare | 🟢 accepted | The generated code is `extern "C"` throughout. |

## Preprocessor extensions

The pragma rows below are summarised here and spelled out — exact syntax, scope,
what is an error — in [`doc/pragmas.md`](pragmas.md), together with this crate's
own `#pragma cinrs`.

| Extension | Frequency | Status | Notes |
| --- | --- | --- | --- |
| `#pragma once` | very common | 🟢 supported | |
| `#pragma GCC diagnostic push/pop/ignored/warning/error` | very common | 🟢 accepted | There are no warnings of ours to suppress. |
| `#pragma GCC error "…"` / `#pragma GCC warning "…"` | occasional | 🟢 supported | An error and a warning, like `#error` and `#warning`. |
| `#pragma GCC poison a b c` | occasional | 🟢 supported | Writing a poisoned identifier afterwards is an error. |
| `#pragma pack(N)`, `#pragma pack(push, N)`, `#pragma pack(pop)`, `#pragma pack()` | common (protocols, file formats) | 🟢 supported | The value in force where a record is *defined* is the one that applies to it. Bad syntax is a diagnostic rather than a silent no-op. |
| `#pragma push_macro("X")` / `#pragma pop_macro("X")` | rare in user code | 🟢 supported | MSVC-origin, in GCC since 4.4 and in Clang. c-testsuite `00206`. |
| `#pragma GCC system_header`, `#pragma GCC visibility`, `#pragma weak`, `#pragma redefine_extname`, `#pragma message`, `#pragma region` / `#pragma endregion` | occasional | 🟢 accepted | Ignored, as 6.10.6 asks. `#pragma weak` is the one worth knowing about: it asks for weak linkage, which stable Rust cannot express, so it gets the same answer the [attribute](#attributes) gets on a declaration — dropped, the symbol then having to be there at link time. |
| `_Pragma("…")` (standard C99) | occasional | 🟢 supported | Destringized and executed as the directive it spells, so a macro can produce one. |
| `#include_next` | rare in user code (system headers) | 🟢 supported | GCC's semantics: the search is taken up again at the entry **after** the one the file writing the directive was found under, so a `<limits.h>` can wrap the next `<limits.h>` on the path rather than itself. The quoted and the angled forms mean the same thing, as they do in GCC — the including file's own directory is never step one. Written in the unit's own text there is no place it was found in, so the search starts at the beginning, exactly as `#include` does. Reaching the end of the chain is a "file not found by #include_next" naming what was looked in. This is what makes the platform's own headers readable at all; see [System headers](system-headers.md). `tests/user_headers.rs`, `tests/ui/gnu_include_next.rs` |
| `__has_include`, `__has_include_next` | common (portability) | 🟢 supported | Resolved exactly as `#include` and `#include_next` are, so `__has_include_next` answers the question the next directive would: 0 once nothing of that name is left after the place this file was found in. |
| `__has_attribute`, `__has_builtin`, `__has_feature`, `__has_extension`, `__has_c_attribute` | common (portability) | 🟢 supported | Answered from [`crate::gnu`](../crates/cinrs-core/src/gnu.rs)'s tables, so the answer is true of *this* implementation: `__has_attribute(packed)` and `__has_attribute(cleanup)` are 1, and `__has_attribute(vector_size)` is 0. |
| `#warning` | common | 🟢 supported (accepted, no output) | Standard in C23. |
| `#ident`, `#sccs` | rare | 🟢 accepted | Ignored: there is no object-file section to put the string in. |
| `#assert` / `#unassert` | rare | 🔴 not planned | Removed from GCC itself. |
| `#line`, and the `# N "file" flags…` line marker GCC writes in its place | occasional (generated code: lex/yacc) | 🟢 supported | Both forms redirect `__LINE__` and `__FILE__` from the next line to the end of that file, the marker's flags being read and dropped. Nothing else moves: a diagnostic still points at the token that was really written, which is what makes an error inside a `c99!` block land on the C. In the macro's own text a `#line` replaces the `.rs`-line convention from there on. c-testsuite `00152`. |
| `__BASE_FILE__`, `__FILE_NAME__`, `__INCLUDE_LEVEL__`, `__TIMESTAMP__` | rare | 🟢 supported | |
| Directives inside macro arguments | `f(\n#ifdef X …)` | rare | ⚪ — | GCC processes them; the standard says undefined. |
| `__VA_OPT__` in older modes | | 🟢 supported in `c23!` and every GNU dialect | |
| Binary literals `0b…`, `'` digit separators in older modes | common (`0b`) | 🟢 supported in `c23!` and every GNU dialect | |

## Standard features from newer standards, in older modes

GCC accepts these in `gnu99`/`gnu11` and, with a pedantic warning, in the
strict modes.

| Feature | GCC in `-std=c99` | cinrs `c99!` | cinrs `gnu99!` |
| --- | --- | --- | --- |
| `_Static_assert`, `_Alignof`, `_Alignas`, `_Generic`, `_Noreturn` (C11) | accepted (pedantic warning) | 🟡 error "requires C11" | 🟢 accepted |
| Anonymous struct/union members (C11) | accepted | 🟡 error "requires C11" | 🟢 accepted |
| `typeof`, `__typeof__` | `__typeof__` accepted; `typeof` only in `gnu*` | 🟢 `__typeof__` accepted; `typeof` is C23 | 🟢 both accepted |
| `0b` literals, digit separators, `__VA_OPT__`, `#elifdef`, `{}`, `[[…]]` (C23) | accepted (pedantic warning) | 🟡 error "requires C23" | 🟢 accepted |

## The leniencies: constraint violations GCC only warns about

The rows above are *features* a newer revision added. These are the other
half — places where ISO C says the program is ill-formed and no compiler
anybody uses has ever refused it. A GNU entry point follows GCC; a strict one
keeps the error and adds a note naming the macro that would take it:

```text
error: void function 'f' should not return a value
       note: GCC accepts this with a warning; write gnu11! for the same leniency
```

The **bold** rows are taken in *every* entry point rather than only the GNU
ones. The line between the two is what the two compilers do by default and
what the program then means: where GCC and Clang both accept a construct and
both produce the same value from it, refusing it would be refusing valid C for
a severity — a warning — that a procedural macro has no way to raise. Where
they disagree, or where accepting would mean *choosing* a meaning (a macro
redefined with a different replacement list, say, where the last definition
wins and only the compiler knows which that is), the diagnostic stays.

| Leniency | Example | ISO C | cinrs |
| --- | --- | --- | --- |
| **Pointer targets that differ only in signedness** | `strlen((unsigned char *) s)`, `long *p = ulp;`, `unsigned char *p = charp;` | 6.5.16.1p1 constraint violation; GCC and Clang warn (`-Wpointer-sign`) and only `-pedantic-errors` promotes it | 🟢 **accepted in every entry point**, silently. Plain `char` counts as differing in sign from both `signed char` and `unsigned char`, which is the rule both compilers use. There is too much real C behind this one for a dialect switch to be the honest answer |
| **`return expr;` in a `void` function, where `expr` has type `void`** | `void f(void) { return g(); }` | 6.8.6.4p1 constraint violation until C23, which allows it; GCC and Clang both answer it with a warning that only `-pedantic-errors` promotes (WG14 DR113) | 🟢 **accepted in every entry point**; the expression is evaluated and there is no value to drop. A `return` with a *value* is still a constraint violation outside the GNU dialects, which take any expression and drop it |
| Comparing two function pointers of incompatible types | `int (*a)(int); long (*b)(void); a == b` | 6.5.9p2; GCC warns (`-Wcompare-distinct-pointer-types`) and compares the addresses | 🟢 GNU dialects. A *compatible* pair needs no leniency: `double (*)()` and `double (*)(double)` are compatible (6.7.6.3p15) and compare everywhere, and `void *` against a function pointer follows the conversion rule above it in the table |
| `sizeof (void)`, `__alignof__ (void)` | `p + 1` with `void *p` | `void` is an incomplete type that can never be completed (6.2.5p19) | 🟢 GNU dialects, both 1 — which is what makes the `void *` arithmetic above mean anything |
| A stray `;` at file scope | `int f(void) { … };` | 6.9p1 has no empty external declaration, and C23 did not add one | 🟢 GNU dialects |
| An enumerator that will not fit `int` | `enum e { big = ULLONG_MAX };` | 6.7.2.2p2 constraint violation until C23 (N3029), which widens the enumeration instead | 🟢 `c23!` and the GNU dialects widen; the strict pre-C23 entry points keep the error. The widened type is the narrowest of `int`, `unsigned int`, `long`, … that holds every value, and every enumerator of the enumeration has it |
| A parameter of a *definition* with no name | `int f(int, int b) { … }` | C23 (N2480) allows it; before that 6.9.1p5 required a name | 🟢 `c23!` and the GNU dialects |
| An undeclared `alloca` | `void *p = alloca(n);` | no ISO header declares it | 🟢 GNU dialects, where the call is `__builtin_alloca` and so returns `void *`, exactly as GCC's `gnu` modes do. A strict entry point leaves it to the C89 implicit-declaration rule, which types it `int()` |
| **An over-long string initialiser, and a braced *scalar* with more than one value** | `char a[3] = "1234";`, `int n = { 1, 2 };` | 6.7.9p2 constraint violation — no initializer may provide a value for something outside the object — and 6.7.9p11 for the scalar. GCC and Clang both warn and drop the excess (WG14 DR114) | 🟢 **accepted in every entry point**, with the excess dropped, which is the value both compilers produce. `execute/pr86714` is the array case and its whole point is that the excess is not part of the value; an *aggregate* with too many initialisers is still refused, since the shape of the braces is the whole of what says which member each value belongs to |
| **A comma operator in a static initialiser** | `int i = (1, 2);` | 6.6p3 keeps the comma operator out of a constant expression, and 6.7.9p4 asks a static initialiser to be one (WG14 DR032/DR035) | 🟢 **accepted in every entry point** when the left operand is itself constant: GCC and Clang both take it, and only `-pedantic-errors` refuses it. Where C asks for an *integer constant expression* — an enumerator, an array bound, a `case` label, `_Static_assert` — the comma is still refused, which is what GCC does there too |
| **Folding the address of a member of a constant pointer** | `#define offsetof(T, m) ((size_t) &((T *) 0)->m)` | 6.6 has no such constant expression; GCC folds it, calls the folding an extension, and rejects it under `-pedantic-errors` | 🟢 GNU dialects, wherever an integer constant expression is required — an array bound, a `case` label, a static initialiser. This is the `offsetof` every C program wrote before `<stddef.h>` had one, and it takes `.member` and `[constant]` steps. The address of a *named object* is still not one: the linker decides it |
| A cast to a union type | `(union u) x` | not in ISO C at all | 🟢 GNU dialects; see the language table above |

## Known differences from GCC

Five of them, all deliberate, all but the last two visible only where the C
program could not observe them anyway:

* **A record with one packed member has a Rust item that is one byte aligned.**
  Rust refuses `#[repr(C, packed, align(N))]` outright (`E0587`), so an item
  that has to place one member at an unaligned offset cannot also carry the
  alignment of the others. Every offset and `sizeof` still agrees with GCC —
  which is everything the C can see — and `tests/bitfield_layout.rs` checks
  that the alignment the item does have still divides the one C computed.
* **A record inside a packed one swaps `#[repr(C, align(N))]` for a zero-sized
  field of that alignment**, for the same `E0587`: a packed type may not
  transitively hold a `repr(align)` one, and C is perfectly happy to pack such
  a member. The field is `__cinrs_alignN: [uK; 0]`, and Rust code building such
  a record has to write it out. An alignment above 8 is refused instead, since
  no Rust integer has one portably.
* **A forward-declared `enum` is `int` even after it is completed**, so that the
  two mentions of the tag agree; the tag then has no named Rust alias.
* **`long double` *is* `double`**, so `__builtin_types_compatible_p(long
  double, double)` answers yes where GCC answers no. No portable Rust type has
  the layout of an x87 extended double, and pretending otherwise would be
  worse than saying so.
* **An untagged `enum` *is* `int`**, so two of them are compatible where GCC
  makes them two distinct types. A tagged one at file scope gets a Rust alias
  of its own and is a type of its own; an anonymous one has no name to give
  such an item. `execute/builtin-types-compatible-p` is the case that asks
  both of these questions and the only one in the corpus that does.

## Status summary

Everything in the tables above marked *supported* is implemented and tested;
`tests/gnu_language.rs`, `tests/gnu_attributes.rs`, `tests/gnu_builtins.rs`,
`tests/gnu_preprocessor.rs`, `tests/int128.rs`, `tests/threads.rs`,
`tests/atomics.rs`, `tests/nested_functions.rs`, `tests/goto.rs`,
`tests/alignment.rs` and
`tests/dialects.rs` are where, and the leniencies have
`tests/ui/gnu_leniencies_in_a_strict_block.rs` for the other half of each
row — that a strict entry point still refuses it. What is left is the one
remaining *planned* row — an escaped newline with
trailing whitespace — plus the rows that say
`not planned` or `impossible`, and the c-testsuite report
([`doc/c-testsuite.md`](c-testsuite.md)) lists which cases each one would fix.
