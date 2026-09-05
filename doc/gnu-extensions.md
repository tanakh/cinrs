# GNU C extensions: what exists, how common they are, and what cinrs does with them

`cinrs` implements the C standards (`c99!`, `c11!`, `c17!`, `c23!`) and, on top
of them, the GNU extensions (`gnu99!`, `gnu11!`, `gnu17!`, `gnu23!`). Real-world
C leans on those extensions heavily, so this document lists them, estimates how
often each shows up in *user* code (not in system headers, which cinrs never
reads — its bundled headers are plain C), and tracks what cinrs supports. It is
the place to update when an extension lands.

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
* **Status**: `supported`, `accepted` (parsed and ignored where ignoring is
  semantically safe), `refused` (recognised and rejected with the reason, so
  that nothing is silently mistranslated), `planned`, `not planned`,
  `impossible` (no stable Rust counterpart), or `—` (not yet decided).
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
   they are keywords only in `gnu99!`, `gnu11!`, `gnu17!` and `gnu23!`
   (`typeof` is also C23's own keyword, so `c23!` has it as well). The
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

4. **The predefined macros say which entry point it is.** `__STRICT_ANSI__` is
   defined in the strict entry points only. `__GNUC__` is `4`,
   `__GNUC_MINOR__` `2` and `__GNUC_PATCHLEVEL__` `1` in *all* of them —
   Clang's own precedent, and for the same reason: a program guards
   `__attribute__` and `__builtin_expect` with
   `#if defined(__GNUC__) && __GNUC__ >= 4`, and those work here. `__VERSION__`
   names cinrs and its version, and nothing claims to be Clang.

C11 6.10.8.3's subsetting macros — `__STDC_NO_ATOMICS__`, `__STDC_NO_THREADS__`,
`__STDC_NO_VLA__` and `__STDC_NO_COMPLEX__` — are all defined as `1` in every
entry point, which turns those four gaps into the conforming omissions the
standard provides for. `__STDC_NO_VLA__` stays defined even though
one-dimensional variable length arrays now work, because the *rest* of C99's
variably modified types do not; a program that tests the macro takes its
`malloc` path, which is always correct. See
[`doc/c-status.md`](c-status.md#c99).

## Language extensions

| Extension | Example | Frequency | Status | Notes |
| --- | --- | --- | --- | --- |
| Statement expressions | `({ int t = x; t * 2; })` | very common (macros: `min`/`max`, container_of, kernel-style) | supported | A Rust block expression. Declarations, loops, nested statement expressions and `return` all work; a *label* or a `goto` inside one is refused, because the decision to lower a function through a control-flow graph is made from its statements and a jump buried in an expression would be dropped. In a function that does use `goto`, a `break` or `continue` that leaves the statement expression is refused too — there is no Rust loop left to leave. |
| `typeof` / `__typeof__` | `typeof(x) y = x;` | very common (macros) | supported | `__typeof__` and `__typeof` everywhere; `typeof` in a GNU dialect and in `c23!`. `__typeof_unqual__` is the same thing here, since the type model carries no top-level qualifiers. |
| `?:` with omitted middle operand | `p ?: default` | common | supported | The operand is evaluated exactly once, into a temporary. |
| `__attribute__((…))` on functions, variables, types | see the attribute table below | very common | supported | Parsed in every position GCC accepts it — declaration specifiers, after a declarator, inside one (`int (__attribute__((x)) *)(void)`), on records, members, parameters, labels and statements — in both the `name` and `__name__` spellings. A subset is honoured, the rest is ignored as C23 allows, and the handful that would change the program's meaning is refused. |
| Alternate keywords | `__inline__`, `__asm__`, `__const__`, `__signed__`, `__volatile__`, `__restrict`, `__restrict__`, `__alignof__`, `__extension__` | very common (portability macros) | supported | The [preprocessor](../crates/cinrs-core/src/pp.rs) turns them into keywords on the way to the parser — *after* macro replacement, so `#define __attribute__(x)` still defines and expands a macro of that name, which is what portability headers write. |
| `__builtin_expect`, `__builtin_expect_with_probability` | `if (__builtin_expect(x, 0))` | very common (`likely`/`unlikely`) | supported | The value is the first argument, typed `long` as GCC types it; `core::hint::likely` is unstable, so the hint has nowhere to go. |
| `__builtin_unreachable`, `__builtin_trap` | | common | supported | `core::hint::unreachable_unchecked()`, and a call to the C library's `abort` — `core::intrinsics::abort` is unstable and `std` is not available to a `no_std` crate. |
| Bit-manipulation builtins | `__builtin_popcount(l,ll)`, `__builtin_clz`, `__builtin_ctz`, `__builtin_ffs`, `__builtin_parity`, `__builtin_bswap16/32/64`, `__builtin_clrsb` | common (codecs, hashing) | supported | Rust's integer methods on the unsigned type of the operand's width. `clz(0)` and `ctz(0)` are undefined in C and answer the width here, which is what Rust does. |
| Overflow-checking builtins | `__builtin_add_overflow(a, b, &r)`, `mul_overflow`, `sub_overflow`, the `_p` forms and the typed `__builtin_sadd_overflow` family | common (security-conscious code) | supported | The arithmetic happens in `i128` and the value says whether the result survived the conversion to the type it is stored in — which is exactly "compute in infinite precision, then convert", including when the operands and the result have different types. |
| `__builtin_constant_p` | `__builtin_constant_p(x)` | common (kernel macros) | supported | 1 when the operand folds, 0 otherwise. |
| `__builtin_types_compatible_p`, `__builtin_choose_expr` | generic macros | occasional | supported | Both are constants; only the operand `choose_expr` picks is type checked. |
| Library builtins | `__builtin_memcpy`, `__builtin_memset`, `__builtin_strlen`, `__builtin_abs`, `__builtin_sqrt`, `__builtin_huge_val`, `__builtin_inf`, `__builtin_nan`, … | common (via headers, sometimes direct) | supported | A call to the library function of that name, declared into the unit if the header that would have declared it was not included. `crate::gnu::LIBRARY_BUILTINS` is the list; a name outside it says which header to include. |
| `__builtin_offsetof` | | common (via `offsetof`) | supported | The implementation of `<stddef.h>`'s `offsetof`. |
| `__builtin_va_list`, `__builtin_va_start/arg/end/copy` | | common (via `<stdarg.h>`) | supported | The implementation of `<stdarg.h>`. |
| `__func__`, `__FUNCTION__`, `__PRETTY_FUNCTION__` | logging macros | common | supported | `__func__` is standard C99. All three are the function's name as a `const char[]`, so `sizeof(__func__)` is its length, and the byte string is emitted only where one is used. |
| `__COUNTER__` | unique identifiers in macros | common | supported | A fresh integer at every use. |
| Case ranges | `case 1 ... 5:` | common (interpreters, lexers) | supported | One Rust range pattern, in both the structured and the control-flow-graph lowering. An *empty* range (`case 5 ... 1:`) is refused rather than warned about: GCC's warning leaves an arm nothing can enter, and the only way to write one is by mistake. |
| Designated-initializer ranges | `[1 ... 5] = 0` | occasional | supported | The value is checked once and written into every element of the range. |
| Old designator syntax | `{ x: 1, y: 2 }` | rare (pre-C99 code) | supported | Read as `.x = 1`. |
| Zero-length arrays | `int data[0];` | common (before C99 flexible members) | supported | A `[T; 0]` member, which is what it already was. |
| Flexible array members (standard C99) | `int data[];` | common | supported | A `[T; 0]` tail member; `sizeof` leaves it out and indexing it is the pointer arithmetic it always was. Must be the last member of a `struct`; *initialising* one — which GCC allows with a warning — is refused, because the object would have to be larger than its type. |
| Empty structures | `struct e {};` | occasional | supported | A zero-sized `#[repr(C)]` item, size 0 as GCC gives it. An empty `union` is generated as an item with one `[u8; 0]` field, since Rust has no fieldless `union`. |
| Bit-fields of a type other than `_Bool`/`int`/`unsigned int` | `char flags : 3;`, `enum e kind : 8;`, `long long wide : 40;` | very common (protocols, kernels, compilers) | supported | Standard C leaves any other type implementation defined. `cinrs` follows GCC and Clang: the allocation unit is `8 * sizeof(T)`, a named field raises the record's alignment to `alignof(T)`, and the integer promotions are the standard width-restricted ones applied to the wider types too — so `unsigned long x : 31` is an `int` and `unsigned long x : 33` keeps its declared type. An `enum` bit-field follows the enumeration's underlying type, which is unsigned when no enumerator is negative; `cinrs` reads such a field back through `int`, so an `enum` field exactly as wide as `int` and holding a value with the top bit set differs from GCC (every narrower width agrees). |
| Incomplete `enum` types | `enum e; enum e *p;` | common | supported | `int` until the enumerator list is seen, and `int` afterwards too — so the two mentions never disagree. A tag declared that way therefore gets no named Rust alias of its own. |
| Arithmetic on `void *` and function pointers | `p + 1` with `void *p` | common | supported (`void *`) / not planned (function pointers) | `void *` arithmetic is byte-wise. |
| Conversion between function pointers and `void *` | `void *p = f;` | common (dlsym users, callbacks) | supported | ISO C forbids it and POSIX requires it; the two have the same size on every target here, and code generation makes the reinterpretation explicit. |
| Non-constant initializers for aggregates (standard C99) | `int a[2] = { x, y };` | — | supported | Standard since C99. |
| Subscripting non-lvalue arrays | `f().a[0]` | rare | supported | Falls out of the place lowering with a temporary. |
| Cast to a union type | `(union u) x` | rare | planned | |
| Mixed declarations and code, `//` comments, `long long`, `inline`, hex floats, variadic macros, compound literals, designated initializers | | — | supported | Standard C99. |
| Named variadic macro parameters | `#define log(fmt, args…) …` | occasional (older code) | supported | `args` is another spelling of `__VA_ARGS__`. |
| `, ## __VA_ARGS__` comma elision | `#define log(fmt, ...) printf(fmt, ## __VA_ARGS__)` | very common | supported | The comma goes when the invocation passed no variable arguments, and the arguments are macro-replaced as usual when it did. `__VA_OPT__` (C23) is the standard way. |
| Dollar signs in identifiers | `$foo` | rare | supported (flag exists, off by default) | Lexer option. |
| `\e` character escape | `"\e[0m"` | occasional (terminal colours) | supported | ESC, in every entry point, as GCC does. |
| Local labels | `__label__ retry;` | rare | supported | Accepted and dropped: every label already has function scope here, and no two may share a name. |
| Labels as values (computed goto) | `void *t[] = { &&a, &&b }; goto *t[i];` | occasional (interpreters) | planned (CFG mode only) | Label addresses become state numbers; `goto *e` becomes a dispatch on the state. Only within one function. |
| Nested functions | `int f(void) { int g(int x) { … } }` | rare | not planned | Needs closures with trampolines; nothing addressable in Rust. |
| `__int128` / `unsigned __int128` | | occasional (crypto, hashing) | planned | `i128`/`u128` — the ABI matches on x86-64. |
| `_Float128`, `__float128`, `_Float16`, decimal floats, fixed-point | | rare | not planned | No stable Rust types (`f128`/`f16` are unstable). |
| `_Complex`, `__complex__`, `__real__`, `__imag__` (standard C99) | | rare | refused | Each is recognised and reported; `__STDC_NO_COMPLEX__` says so to the program. |
| Vector extensions | `typedef int v4si __attribute__((vector_size(16)));` | occasional (SIMD) | refused | `core::simd` is unstable. |
| Inline assembly | `asm volatile("…" : "=r"(x) : "r"(y) : "memory")` | occasional (kernels, crypto) | refused | Rust has `core::arch::asm!`, but mapping GCC's operand constraints onto its own is a project rather than a feature, and half a translation of assembly is worse than none. The `asm` **label** on a declaration — `int f(void) __asm__("f_impl");` — is a different thing and is supported. |
| `asm` labels on declarations | `int f(void) __asm__("f_impl");` | common (libc shims) | supported | On a declaration the unit does not define, `#[link_name = "…"]`; on one it defines, `#[unsafe(export_name = "…")]`. |
| `__thread` | thread-local objects | occasional | impossible on stable | `#[thread_local]` is unstable; `thread_local!` changes the object model. The same diagnostic as `_Thread_local`. |
| `__sync_*` / `__atomic_*` builtins | lock-free code | occasional | planned (subset) | `core::sync::atomic` intrinsics exist for all sizes; needs `_Atomic`-free spellings only. |
| `__auto_type` | `__auto_type x = expr;` | rare | supported | C23's `auto` under another name, and available in every entry point. |
| Escaped newlines with trailing whitespace | `\ ` + newline | rare | planned | Lexer leniency. |
| `__builtin_return_address`, `__builtin_frame_address`, `__builtin_apply` | | rare | not planned | |
| `__builtin_LINE`, `__builtin_FILE`, `__builtin_FUNCTION` | | rare | supported | Predefined macros for `__LINE__`, `__FILE__` and `__func__`, so each reports the *use*. |
| `__builtin_object_size`, `__builtin_dynamic_object_size` | fortified headers | rare in user code | supported | `(size_t) -1` and `0`, the two answers GCC documents for "unknown". |
| `__builtin_alloca`, `alloca()`, `__builtin_alloca_with_align` | `char *p = alloca(n);` | occasional | supported (emulated) | Rust has no stack allocation with a size chosen at run time, so the memory comes from a per-function arena on the heap: one 16-byte aligned block per call, all of them freed when the function returns — which is `alloca`'s own lifetime, and why a pointer that outlives the call dangles here exactly as it does in C. The bundled `<alloca.h>` defines the plain name in terms of the builtin, the way every platform's own header does. `__builtin_alloca_with_align` takes its alignment in bits and accepts up to 128, which the arena already satisfies. The arena is a `Vec`, so this and variable length arrays are the only two constructs whose expansion needs more than `core`; see the crate documentation for `#pragma cinrs no_std`. |
| `__builtin_prefetch`, `__builtin_assume_aligned`, `__builtin_assume`, `__builtin_speculation_safe_value` | | occasional | supported (no-ops) | The operands are still evaluated; `assume_aligned` gives the pointer back. |
| Non-standard predefined macros | `__GNUC__`, `__GNUC_MINOR__`, `__VERSION__`, `__STRICT_ANSI__`, `__BASE_FILE__`, `__FILE_NAME__`, `__INCLUDE_LEVEL__`, `__TIMESTAMP__`, `__x86_64__`, `__linux__`, `__SIZEOF_INT__`, `__CHAR_BIT__`, `__INT_MAX__`, `__BYTE_ORDER__`, `__ORDER_LITTLE_ENDIAN__` | very common (portability `#if`s) | supported | See [Strict and GNU entry points](#strict-and-gnu-entry-points) for what `__GNUC__` commits to. `__TIMESTAMP__` is a fixed placeholder like `__DATE__`, so a build gives the same output twice. `__INT_MAX__` and the other limit macros are not predefined; `<limits.h>` has them. |

## `__attribute__` forms

Every attribute is parsed. One it does not know is dropped, which C23 6.7.13.1p3
explicitly allows and which is what GCC does with a warning this crate has no
way to raise; one it knows but cannot honour is refused, because ignoring it
would change what the program means. Both the `name` and the `__name__`
spellings work, and so does C23's `[[gnu::name]]`.

| Attribute | Frequency | Status | What it does here |
| --- | --- | --- | --- |
| `noreturn` | very common | supported | Exactly what `_Noreturn` does: a call to the function ends the statement it is in. |
| `packed` (record or member) | very common (protocols, file formats) | supported | `#[repr(C, packed)]`, with explicit padding where a member moved. On one *member* it packs that member alone. Any packing also switches the bit-field allocation-unit rule off, which is GCC's own rule and easy to miss. |
| `aligned(N)` | very common | supported | On a record, `#[repr(C, align(N))]`; on a member, the same request `_Alignas(N)` makes — the member moves to the boundary and explicit padding puts it there. On an *object* it is still refused. |
| `always_inline`, `noinline`, `cold`, `hot` | very common | supported | `#[inline(always)]`, `#[inline(never)]`, `#[cold]`; `hot` cancels `cold`. |
| `flatten` | common | accepted | Ignored. |
| `format(printf, i, j)`, `format_arg` | very common (logging APIs) | accepted | Diagnostic-only in GCC. |
| `deprecated`, `deprecated("message")` | common | supported | `#[deprecated]`, so Rust code that calls the function is warned. |
| `warn_unused_result` | common | accepted | Ignored; `#[must_use]` would make the *C* calls warn too. |
| `constructor`, `destructor` (with or without a priority) | common (plugin registration, test frameworks) | supported | A `#[used]` function pointer in `.init_array` / `.fini_array` on ELF and `__DATA,__mod_init_func` / `__mod_term_func` on Apple. The priority is parsed and ignored. A target with neither table is a `compile_error!`, because a constructor that silently never ran would be worse. |
| `section("…")` | common in libraries | supported | `#[unsafe(link_section = "…")]` on a function or on an object with static storage duration. |
| `visibility("…")` | common in libraries | accepted | Only matters with `#pragma cinrs export`. |
| `weak`, `alias("…")`, `weakref`, `ifunc` | occasional | refused | `#[linkage]` is unstable, so weak linkage cannot be asked for at all. |
| `cleanup(f)` | occasional (glib, systemd) | refused | A Drop guard calling `f(&var)` at scope exit; CFG mode needs care. Phase 2. |
| `mode(…)` | rare | refused | Write the type the mode names instead. |
| `vector_size(N)` | occasional | refused | See vector extensions. |
| `nonnull`, `returns_nonnull`, `malloc`, `pure`, `const`, `leaf`, `nothrow`, `access(…)`, `alloc_size`, `alloc_align`, `sentinel`, `returns_twice`, `no_sanitize`, `noclone`, `noipa`, `optimize(…)`, `target(…)`, `error(…)`, `warning(…)`, `designated_init`, `artificial`, `gnu_inline`, `externally_visible` | common in library headers, rare in bodies | accepted | Optimisation and diagnostic hints only. |
| `transparent_union`, `may_alias`, `nonstring` | rare | accepted | Ignored. |
| `unused`, `used`, `maybe_unused` | very common | accepted | Every generated item already carries `#[allow(dead_code)]`. |
| `fallthrough` | common | supported | Accepted and dropped: a `switch` group falls through in the generated Rust either way. |
| `nodiscard`, `unsequenced`, `reproducible` | rare | accepted | C23's, and answered by `__has_c_attribute`. |
| `stdcall`, `cdecl`, `fastcall` | rare | accepted | The generated code is `extern "C"` throughout. |

## Preprocessor extensions

| Extension | Frequency | Status | Notes |
| --- | --- | --- | --- |
| `#pragma once` | very common | supported | |
| `#pragma GCC diagnostic push/pop/ignored/warning/error` | very common | accepted | There are no warnings of ours to suppress. |
| `#pragma GCC error "…"` / `#pragma GCC warning "…"` | occasional | supported | An error and a warning, like `#error` and `#warning`. |
| `#pragma GCC poison a b c` | occasional | supported | Writing a poisoned identifier afterwards is an error. |
| `#pragma pack(N)`, `#pragma pack(push, N)`, `#pragma pack(pop)`, `#pragma pack()` | common (protocols, file formats) | supported | The value in force where a record is *defined* is the one that applies to it. Bad syntax is a diagnostic rather than a silent no-op. |
| `#pragma push_macro("X")` / `#pragma pop_macro("X")` | rare in user code | supported | MSVC-origin, in GCC since 4.4 and in Clang. c-testsuite `00206`. |
| `#pragma GCC system_header`, `#pragma GCC visibility`, `#pragma weak`, `#pragma redefine_extname`, `#pragma message`, `#pragma region` / `#pragma endregion` | occasional | accepted | Ignored, as 6.10.6 asks. `weak` is the one worth knowing about: it asks for weak linkage, which stable Rust cannot express, so it gets the same answer `__attribute__((weak))` gets — except that a *pragma* is ignored rather than refused, since it may name a symbol the unit never defines. |
| `_Pragma("…")` (standard C99) | occasional | supported | Destringized and executed as the directive it spells, so a macro can produce one. |
| `#include_next` | rare in user code (system headers) | refused | cinrs never searches the platform's include directories, so there is no next header to reach. |
| `__has_include`, `__has_include_next` | common (portability) | supported | Resolved exactly as `#include` is; `__has_include_next` searches the angled path, since there is no file below the current one. |
| `__has_attribute`, `__has_builtin`, `__has_feature`, `__has_extension`, `__has_c_attribute` | common (portability) | supported | Answered from [`crate::gnu`](../crates/cinrs-core/src/gnu.rs)'s tables, so the answer is true of *this* implementation: `__has_attribute(packed)` is 1 and `__has_attribute(cleanup)` is 0. |
| `#warning` | common | supported (accepted, no output) | Standard in C23. |
| `#ident`, `#sccs` | rare | accepted | Ignored: there is no object-file section to put the string in. |
| `#assert` / `#unassert` | rare | not planned | Removed from GCC itself. |
| `#line`, and the `# N "file" flags…` line marker GCC writes in its place | occasional (generated code: lex/yacc) | supported | Both forms redirect `__LINE__` and `__FILE__` from the next line to the end of that file, the marker's flags being read and dropped. Nothing else moves: a diagnostic still points at the token that was really written, which is what makes an error inside a `c99!` block land on the C. In the macro's own text a `#line` replaces the `.rs`-line convention from there on. c-testsuite `00152`. |
| `__BASE_FILE__`, `__FILE_NAME__`, `__INCLUDE_LEVEL__`, `__TIMESTAMP__` | rare | supported | |
| Directives inside macro arguments | `f(\n#ifdef X …)` | rare | — | GCC processes them; the standard says undefined. |
| `__VA_OPT__` in older modes | | supported in `c23!` and every GNU dialect | |
| Binary literals `0b…`, `'` digit separators in older modes | common (`0b`) | supported in `c23!` and every GNU dialect | |

## Standard features from newer standards, in older modes

GCC accepts these in `gnu99`/`gnu11` and, with a pedantic warning, in the
strict modes.

| Feature | GCC in `-std=c99` | cinrs `c99!` | cinrs `gnu99!` |
| --- | --- | --- | --- |
| `_Static_assert`, `_Alignof`, `_Alignas`, `_Generic`, `_Noreturn` (C11) | accepted (pedantic warning) | error "requires C11" | accepted |
| Anonymous struct/union members (C11) | accepted | error "requires C11" | accepted |
| `typeof`, `__typeof__` | `__typeof__` accepted; `typeof` only in `gnu*` | `__typeof__` accepted; `typeof` is C23 | both accepted |
| `0b` literals, digit separators, `__VA_OPT__`, `#elifdef`, `{}`, `[[…]]` (C23) | accepted (pedantic warning) | error "requires C23" | accepted |

## Known differences from GCC

Three of them, all deliberate, all visible only where the C program could not
observe them anyway:

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

## Status summary

Everything in the tables above marked *supported* is implemented and tested;
`tests/gnu_language.rs`, `tests/gnu_attributes.rs`, `tests/gnu_builtins.rs`,
`tests/gnu_preprocessor.rs` and `tests/dialects.rs` are where. What is left is
the *planned* rows — computed `goto`, `cleanup`, `__int128`, the `__sync_*` and
`__atomic_*` builtins, casts to a union type — plus the rows that say
`not planned` or `impossible`, and the c-testsuite report
([`doc/c-testsuite.md`](c-testsuite.md)) lists which cases each one would fix.
