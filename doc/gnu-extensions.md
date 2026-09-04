# GNU C extensions: what exists, how common they are, and what cinrs does with them

`cinrs` implements the C standards (`c99!`, `c11!`, `c17!`, `c23!`). Real-world C
leans on GNU extensions too, so this document lists them, estimates how often
each shows up in *user* code (not in system headers, which cinrs never reads —
its bundled headers are plain C), and tracks what cinrs supports. It is the
place to update when an extension lands.

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
  semantically safe), `planned`, `not planned`, `impossible` (no stable Rust
  counterpart), or `—` (not yet decided).
* Where an extension later became standard C, it says so; those are handled by
  the standard gating (`c11!`/`c23!`), and the row is only about using the
  feature in an *older* entry point.

## Policy proposal: strict and GNU entry points

GCC itself distinguishes `-std=c99` from `-std=gnu99`. Even in `-std=c99`,
everything spelled with a double underscore (`__typeof__`, `__attribute__`,
`__extension__`, `__asm__`, `__builtin_*`) stays available; only the plain
spellings (`typeof`, `asm`, `inline` in C89) are switched off, and features from
newer standards are merely *pedantic warnings*. `cinrs` currently gates newer
standard features as hard errors in older entry points.

Proposed rule set (not implemented yet — see the tables for each item):

1. `__`-spelled extensions are available in every entry point, exactly as in
   GCC's strict modes.
2. Plain-spelled GNU keywords (`typeof`, `asm`) need a GNU entry point:
   `gnu99!`, `gnu11!`, `gnu17!`, `gnu23!` (or `#pragma cinrs gnu` inside a
   block — one of the two, to be decided).
3. In a GNU entry point, features of *newer* standards are accepted (GCC
   accepts `_Static_assert` in `gnu99`); the strict entry points keep the
   "requires C11 or later" errors.

## Language extensions

| Extension | Example | Frequency | Status | Notes |
| --- | --- | --- | --- | --- |
| Statement expressions | `({ int t = x; t * 2; })` | very common (macros: `min`/`max`, container_of, kernel-style) | planned | Rust blocks are expressions: direct translation. Must keep `break`/`continue`/`goto` out of it inside the CFG lowering. |
| `typeof` / `__typeof__` | `typeof(x) y = x;` | very common (macros) | planned | `typeof` is C23 and already implemented in `c23!`; only the spelling/gating is missing. |
| `?:` with omitted middle operand | `p ?: default` | common | planned | Evaluate the condition once. |
| `__attribute__((…))` on functions, variables, types | see the attribute table below | very common | planned (parse all; honour a subset) | Parsing the syntax is the first step: unknown attributes are ignored like C23 `[[…]]`. |
| Alternate keywords | `__inline__`, `__asm__`, `__const__`, `__signed__`, `__volatile__`, `__restrict`, `__restrict__`, `__alignof__`, `__extension__` | very common (portability macros) | planned | Aliases of the standard keywords; `__extension__` is a no-op marker. |
| `__builtin_expect`, `__builtin_expect_with_probability` | `if (__builtin_expect(x, 0))` | very common (`likely`/`unlikely`) | planned | Return the first argument; Rust `core::hint::likely`/`unlikely` are unstable. |
| `__builtin_unreachable`, `__builtin_trap` | | common | planned | `core::hint::unreachable_unchecked()` / `core::intrinsics::abort` (use `std::process::abort` or `unreachable!`). |
| Bit-manipulation builtins | `__builtin_popcount(l,ll)`, `__builtin_clz`, `__builtin_ctz`, `__builtin_ffs`, `__builtin_parity`, `__builtin_bswap16/32/64`, `__builtin_clrsb` | common (codecs, hashing) | planned | Direct Rust integer methods (`count_ones`, `leading_zeros`, `swap_bytes`); mind the undefined `clz(0)`. |
| Overflow-checking builtins | `__builtin_add_overflow(a, b, &r)`, `mul_overflow`, `sub_overflow`, `*_p` forms | common (security-conscious code) | planned | `overflowing_add` + store. |
| `__builtin_constant_p` | `__builtin_constant_p(x)` | common (kernel macros) | planned | Answer from `const_eval`: 1 if the operand folds, else 0. |
| `__builtin_types_compatible_p`, `__builtin_choose_expr` | generic macros | occasional | planned | Both are sema-level constants. |
| Library builtins | `__builtin_memcpy`, `__builtin_memset`, `__builtin_strlen`, `__builtin_abs`, `__builtin_sqrt`, `__builtin_huge_val`, `__builtin_inf`, `__builtin_nan`, … | common (via headers, sometimes direct) | planned | Map to the libc call or to the Rust equivalent. |
| `__builtin_offsetof` | | common (via `offsetof`) | supported | Already the implementation of `<stddef.h>`'s `offsetof`. |
| `__builtin_va_list`, `__builtin_va_start/arg/end/copy` | | common (via `<stdarg.h>`) | supported | The implementation of `<stdarg.h>`. |
| `__FUNCTION__`, `__PRETTY_FUNCTION__` | logging macros | common | planned | `__func__` is standard C99 and needs implementing too (it is a predeclared `static const char[]`). |
| `__COUNTER__` | unique identifiers in macros | common | planned | Preprocessor counter. |
| Case ranges | `case 1 ... 5:` | common (interpreters, lexers) | planned | Expand to a Rust range pattern. |
| Designated-initializer ranges | `[1 ... 5] = 0` | occasional | planned | Expand at sema time. |
| Old designator syntax | `{ x: 1, y: 2 }` | rare (pre-C99 code) | planned | Parse as `.x = 1`. |
| Zero-length arrays | `int data[0];` | common (before C99 flexible members) | planned | Same treatment as a flexible array member: `[T; 0]` at the end. |
| Flexible array members (standard C99) | `int data[];` | common | planned | Standard, still unimplemented: `[T; 0]` tail, `sizeof` excludes it. |
| Empty structures | `struct e {};` | occasional | planned | Zero-sized `#[repr(C)] struct` (GCC: size 0; C++/MSVC: 1). c-testsuite `00216`. |
| Bit-fields of a type other than `_Bool`/`int`/`unsigned int` | `char flags : 3;`, `enum e kind : 8;`, `long long wide : 40;` | very common (protocols, kernels, compilers) | supported | Standard C leaves any other type implementation defined. `cinrs` follows GCC and Clang: the allocation unit is `8 * sizeof(T)`, a named field raises the record's alignment to `alignof(T)`, and the integer promotions are the standard width-restricted ones applied to the wider types too — so `unsigned long x : 31` is an `int` and `unsigned long x : 33` keeps its declared type. An `enum` bit-field follows the enumeration's underlying type, which is unsigned when no enumerator is negative; `cinrs` reads such a field back through `int`, so an `enum` field exactly as wide as `int` and holding a value with the top bit set differs from GCC (every narrower width agrees). |
| Incomplete `enum` types | `enum e; enum e *p;` | common | planned | Treat as `int` until (and unless) completed. |
| Arithmetic on `void *` and function pointers | `p + 1` with `void *p` | common | supported (`void *`) / — (function pointers) | `void *` arithmetic is byte-wise already. |
| Conversion between function pointers and `void *` | `void *p = f;` | common (dlsym users, callbacks) | planned | An `as`/`transmute` in codegen; C forbids it, POSIX requires it. |
| Non-constant initializers for aggregates (standard C99) | `int a[2] = { x, y };` | — | supported | Standard since C99. |
| Subscripting non-lvalue arrays | `f().a[0]` | rare | planned | Falls out of the place lowering with a temporary. |
| Cast to a union type | `(union u) x` | rare | — | |
| Mixed declarations and code, `//` comments, `long long`, `inline`, hex floats, variadic macros, compound literals, designated initializers | | — | supported | Standard C99. |
| Named variadic macro parameters | `#define log(fmt, args...) …` | occasional (older code) | planned | Preprocessor: `args` is `__VA_ARGS__`. |
| `, ## __VA_ARGS__` comma elision | `#define log(fmt, ...) printf(fmt, ## __VA_ARGS__)` | very common | planned | Preprocessor special case; `__VA_OPT__` (C23) is the standard way. |
| Dollar signs in identifiers | `$foo` | rare | supported (flag exists, off by default) | Lexer option. |
| `\e` character escape | `"\e[0m"` | occasional (terminal colours) | planned | Lexer: ESC. |
| Local labels | `__label__ retry;` | rare | planned | Trivial in CFG mode. |
| Labels as values (computed goto) | `void *t[] = { &&a, &&b }; goto *t[i];` | occasional (interpreters) | planned (CFG mode only) | Label addresses become state numbers; `goto *e` becomes a dispatch on the state. Only within one function. |
| Nested functions | `int f(void) { int g(int x) { … } }` | rare | not planned | Needs closures with trampolines; nothing addressable in Rust. |
| `__int128` / `unsigned __int128` | | occasional (crypto, hashing) | planned | `i128`/`u128` — the ABI matches on x86-64. |
| `_Float128`, `__float128`, `_Float16`, decimal floats, fixed-point | | rare | not planned | No stable Rust types (`f128`/`f16` are unstable). |
| `_Complex` (standard C99) | | rare | not planned for now | Would need a `#[repr(C)]` pair type and arithmetic lowering. |
| Vector extensions | `typedef int v4si __attribute__((vector_size(16)));` | occasional (SIMD) | not planned | `core::simd` is unstable. |
| Inline assembly | `asm volatile("…" : "=r"(x) : "r"(y) : "memory")` | occasional (kernels, crypto) | not planned (maybe later) | Rust has `core::arch::asm!` with `options(att_syntax)`, but constraint mapping is a project of its own. |
| `__thread` | thread-local objects | occasional | impossible on stable | `#[thread_local]` is unstable; `thread_local!` changes the object model. Same as `_Thread_local`. |
| `__sync_*` / `__atomic_*` builtins | lock-free code | occasional | planned (subset) | `core::sync::atomic` intrinsics exist for all sizes; needs `_Atomic`-free spellings only. |
| `__auto_type` | `__auto_type x = expr;` | rare | planned | `auto` (C23) is implemented; add the spelling. |
| Escaped newlines with trailing whitespace | `\ ` + newline | rare | planned | Lexer leniency. |
| `__builtin_return_address`, `__builtin_frame_address`, `__builtin_apply` | | rare | not planned | |
| `__builtin_LINE`, `__builtin_FILE`, `__builtin_FUNCTION` | | rare | planned | Same information as `__LINE__`/`__FILE__`/`__func__`. |
| `__builtin_object_size`, `__builtin_dynamic_object_size` | fortified headers | rare in user code | planned | Return `(size_t)-1` / 0 as the "unknown" answers. |
| `__builtin_alloca`, `alloca()` | | occasional | not planned | No stack allocation with dynamic size in Rust; VLAs have the same problem. |
| `__builtin_prefetch`, `__builtin_assume_aligned`, `__builtin_speculation_safe_value` | | occasional | planned (no-ops) | |
| Non-standard predefined macros | `__GNUC__`, `__GNUC_MINOR__`, `__x86_64__`, `__linux__`, `__SIZEOF_INT__`, `__CHAR_BIT__`, `__INT_MAX__`, `__BYTE_ORDER__`, `__ORDER_LITTLE_ENDIAN__`, `__VERSION__` | very common (portability `#if`s) | partly supported | Arch/OS/data-model/byte-order macros exist. Defining `__GNUC__` is a policy decision: code then assumes *every* extension works. |

## `__attribute__` forms

Parsing every attribute is the first step (unknown ones are ignored, as GCC does
with a warning). The second column says what honouring it would mean.

| Attribute | Frequency | Status | Rust counterpart / plan |
| --- | --- | --- | --- |
| `unused`, `used` | very common | planned (ignore) | `#[allow(dead_code)]` is already emitted. |
| `noreturn` | very common | planned | Same as `_Noreturn`. |
| `packed` (struct/union/member) | very common (protocols, file formats) | planned | `#[repr(C, packed)]`; per-member packing needs the layout code to know. Reading packed fields through raw pointers must use `read_unaligned`. |
| `aligned(N)` | very common | planned | `#[repr(C, align(N))]` on types; objects need a wrapper type. |
| `always_inline`, `noinline`, `flatten`, `hot`, `cold` | very common | planned | `#[inline(always)]`, `#[inline(never)]`, `#[cold]`; `flatten` ignored. |
| `format(printf, i, j)`, `format_arg` | very common (logging APIs) | planned (ignore) | Diagnostic-only in GCC. |
| `deprecated`, `warn_unused_result`, `nodiscard`-like | common | planned | `#[deprecated]`, `#[must_use]`. |
| `constructor`, `destructor` | common (plugin registration, test frameworks) | planned | `#[used] #[link_section = ".init_array"] static F: unsafe extern "C" fn() = …` works on ELF targets; `destructor` via `.fini_array`. |
| `visibility("…")`, `section("…")` | common in libraries | planned | `section` → `#[link_section]`; `visibility` only matters with `#pragma cinrs export`. |
| `weak`, `alias("…")`, `weakref`, `ifunc` | occasional | impossible on stable | `#[linkage]` is unstable. |
| `cleanup(f)` | occasional (glib, systemd) | planned | A Drop guard calling `f(&var)` at scope exit — CFG mode needs care. |
| `nonnull`, `returns_nonnull`, `malloc`, `pure`, `const`, `leaf`, `nothrow`, `access(…)`, `alloc_size`, `sentinel` | common in library headers, rare in bodies | planned (ignore) | Optimisation/diagnostic hints only. |
| `mode(…)` | rare | not planned | |
| `transparent_union`, `may_alias` | rare | planned (ignore) / — | |
| `vector_size(N)` | occasional | not planned | See vector extensions. |
| `fallthrough` | common | planned | Same as `[[fallthrough]]`. |
| `designated_init`, `optimize(…)`, `target(…)`, `error(…)`, `warning(…)`, `noipa`, `naked` | rare | planned (ignore) | |

## Preprocessor extensions

| Extension | Frequency | Status | Notes |
| --- | --- | --- | --- |
| `#pragma once` | very common | supported | |
| `#pragma GCC diagnostic push/pop/ignored/warning/error` | very common | planned (ignore) | No warnings to suppress on our side. |
| `#pragma pack(N)`, `#pragma pack(push, N)`, `#pragma pack(pop)` | common (protocols, file formats) | planned | Same layout machinery as `packed`. |
| `#pragma push_macro("X")` / `#pragma pop_macro("X")` | rare in user code | planned (cheap) | MSVC-origin, in GCC since 4.4 and in Clang. Mostly used in headers that must temporarily undefine a macro. c-testsuite `00206`. |
| `#pragma GCC poison`, `#pragma GCC system_header`, `#pragma GCC visibility`, `#pragma weak`, `#pragma redefine_extname`, `#pragma message`, `#pragma region` | occasional | planned (ignore) | |
| `#include_next` | rare in user code (system headers) | not planned | cinrs never searches system directories. |
| `__has_include`, `__has_include_next` | common (portability) | planned | `__has_include` is C23; needs the same resolution as `#include`. |
| `__has_attribute`, `__has_builtin`, `__has_feature`, `__has_extension`, `__has_c_attribute` | common (portability) | planned | Answer from the tables above; unknown → 0. |
| `#warning` | common | supported (accepted, no output) | Standard in C23. |
| `#ident`, `#sccs`, `#assert`/`#unassert` | rare | planned (ignore) / not planned | |
| `#line` | occasional (generated code: lex/yacc) | planned | Must redirect `__LINE__`/`__FILE__` for the rest of the file. c-testsuite `00152`. |
| `__BASE_FILE__`, `__FILE_NAME__`, `__INCLUDE_LEVEL__`, `__TIMESTAMP__` | rare | planned | |
| Directives inside macro arguments | `f(\n#ifdef X …)` | rare | — | GCC processes them; the standard says undefined. |
| `__VA_OPT__` in older modes | | supported only in `c23!` | GNU accepts it everywhere. |
| Binary literals `0b…`, `'` digit separators in older modes | common (`0b`) | supported only in `c23!` | GNU accepts `0b` everywhere (as C23 does). |

## Standard features from newer standards, in older modes

GCC accepts these in `gnu99`/`gnu11` and, with a pedantic warning, in the
strict modes. `cinrs` rejects them in older entry points today.

| Feature | GCC in `-std=c99` | cinrs `c99!` |
| --- | --- | --- |
| `_Static_assert`, `_Alignof`, `_Alignas`, `_Generic`, `_Noreturn` (C11) | accepted (pedantic warning) | error "requires C11" |
| Anonymous struct/union members (C11) | accepted | error "requires C11" |
| `typeof`, `__typeof__` | `__typeof__` accepted; `typeof` only in `gnu*` | C23 only |
| `0b` literals, `__VA_OPT__`, `#elifdef`, `[[…]]` (C23) | accepted (pedantic warning) | C23 only |

## Status summary

Implemented GNU extensions today: `__builtin_offsetof`, `__builtin_va_*`,
`void *` arithmetic, `#pragma once`, empty variadic macro arguments
(`LOG("x")`), `$` in identifiers (opt-in).
Everything marked *planned* above is open work; the c-testsuite report
(`doc/c-testsuite.md`) lists which cases each one would fix.
