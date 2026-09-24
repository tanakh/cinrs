# What works

This is the tour of the language, construct by construct. What a construct is
translated *into* — the generated signatures, the bit-field accessors, the
module an expansion goes into — is [What the C becomes](translation.md).

**Contents**

* [Standards and entry points](#standards-and-entry-points)
* [K&R C, implicit int and implicit declarations](#kr-c-implicit-int-and-implicit-declarations)
* [The C99 language](#the-c99-language)
* [Character sets, extended identifiers and Unicode literals](#character-sets-extended-identifiers-and-unicode-literals)
* [Names Rust would not take](#names-rust-would-not-take)
* [Variably modified types and `alloca`](#variably-modified-types-and-alloca)
* [The preprocessor](#the-preprocessor)
* [`#include` and `#embed`](#include-and-embed)
* [Calling a C library from Rust](#calling-a-c-library-from-rust)
* [GNU extensions](#gnu-extensions)
* [`__int128`](#__int128)
* [Thread-local objects](#thread-local-objects)
* [C11 threads](#c11-threads)
* [Atomics](#atomics)
* [Pragmas](#pragmas)
* [Safe functions](#safe-functions)
* [Input forms](#input-forms)
* [Diagnostics](#diagnostics)
* [Variadic functions](#variadic-functions)
* [Complex numbers](#complex-numbers)

## Standards and entry points

`c89!` (also spelled `c90!`), `c99!`, `c11!`, `c17!` and `c23!` are the same
macro for five revisions of the language, and `__STDC_VERSION__` follows —
except in `c89!`, which leaves it undefined, because C89 as published had no
such macro; `gnu89!`, `gnu99!`, `gnu11!`, `gnu17!` and `gnu23!` are the same
five with the GNU extensions switched on.

`c11!` adds `_Static_assert`, `_Generic`, `_Alignof`, `_Alignas`, `_Noreturn`
and anonymous `struct`/`union` members; `c17!` is `c11!` with a different
version macro; `c23!` adds the keywords C23 promoted (`bool`, `true`, `false`,
`nullptr`, `static_assert`, `alignof`, `alignas`, `thread_local`, `constexpr`,
`typeof`), `[[…]]` attributes, `__VA_OPT__`, `#elifdef`/`#elifndef`, binary
constants, digit separators, empty initialisers, `auto` type inference,
enumerations with a fixed underlying type or a value too wide for `int`,
unnamed parameters in a definition, a label anywhere in a compound statement,
improved tag compatibility — a tag defined twice with the same members is one
type — `<stdckdint.h>` and `unreachable()`.

A feature from a later revision used in an earlier block is a diagnostic that
says which macro to write instead — and `c89!` is that rule pointed the other
way, refusing everything C99 added (`//` comments, mixed declarations and code,
`long long`, designated initializers, variable length arrays, `_Bool`,
`restrict`, `inline`, …) with the same message.

The revision also decides what `int f();` means: the parameters are
*unspecified* before C23, so a call may pass any number of arguments and each
gets the default argument promotions, while `c23!` and `gnu23!` read the empty
list as `(void)` — which is exactly where the standard moved it.

## K&R C, implicit int and implicit declarations

Old-style (K&R) function definitions — `int f(a, b) int a; char *b; { … }` —
work in every entry point below `c23!`, which is the revision that removed
them. Their type has no prototype, so a caller applies the default argument
promotions, and the generated item takes the promoted types and converts to the
declared ones on entry. The identifier list and the declaration list become the
parameter list (6.9.1p6), and `register` is allowed on a parameter. Four things
are diagnostics: a declaration-list entry naming something the identifier list
does not, naming one twice, carrying an initialiser, and an identifier list on a
declaration that is not a definition.

`c89!` and `gnu89!` add the two rules C99 deleted: **implicit `int`**
(`static x;`, `f() { … }`) and **implicit function declarations**, where calling
an undeclared `abs` declares `extern int abs();` and the linker resolves it. The
implicit declaration has no prototype, so a later declaration of the same name
has to be compatible with `int f()` or it is the ordinary "conflicting types"; a
`__builtin_` name is never declared this way, since it belongs to the
implementation and a diagnostic naming it is more use than a link error.
`gnu89!` is otherwise `gnu99!`: `gcc -std=gnu89` takes every later feature as an
extension, and so does this.

## The C99 language

All the arithmetic types, pointers, arrays, `struct`, `union`, `enum`,
bit-fields, `typedef`, string literals, function pointers, `sizeof` with the
real layout, casts, aggregate and designated initialisers — designator *lists*
included, so `{ .a.b = 1 }`, `{ .arr[2].x = 3 }` and the elements that carry on
from where one of them landed all work — compound literals —
`&(struct S){ 1, 2 }`, whose object lives as long as the block it is written in
— variable length arrays, file-scope, `static` and `extern` objects, every
operator, every control structure — `if`, `while`, `do`/`while`, `for`, `switch`
with fallthrough, `break`, `continue`, `return`, and `goto` — an outward one
becomes a labelled block or a labelled loop named after the C label, so
`goto done` is `break 'done` and `goto retry` is `continue 'retry`; a jump Rust
cannot make that way (into a block, or two labels whose regions would overlap)
goes through a control-flow graph, which a relooper reads back into Rust's own
loops and `match`es — a state variable is left only where the C really is
irreducible, a cycle with two heads, and a computed `goto` is a `switch` over
the labels whose address is taken, relooped like any other.

## Character sets, extended identifiers and Unicode literals

Digraphs, the bundled `<iso646.h>`, and the nine **trigraphs**, replaced in
translation phase 1 wherever the revision still has them — every strict entry
point below `c23!`, which is where C removed them, and no GNU dialect, which is
the line GCC draws.

**Extended identifiers**: `int café(void)` and `int café(void)` are one
function, in Unicode Annex #31's character set, refused when the name is not in
Normalization Form C. And the **Unicode literals** — `u8"…"`, `u"…"` and `U"…"`
with their `char8_t`, `char16_t` and `char32_t`, `u'x'`, `U'x'` and C23's
`u8'x'`, surrogate pairs and all — with `<uchar.h>` bundled.

## Names Rust would not take

A C name that is a Rust keyword becomes a raw identifier (`int match(int)` is
called as `r#match`); the five Rust cannot write even as raw ones — `self`,
`Self`, `super`, `crate` and `_` — get an underscore appended, and a `$`, which
C takes as an identifier character, is written `_dollar_`. Where the program
already uses the result for something else the spelling grows another `_` until
it is free, so a unit with both `self` and `self_` calls them `self__` and
`self_`; one C name is that one Rust name everywhere it appears, and the symbol
still links by the C name.

## Variably modified types and `alloca`

`int a[n];` with a bound that is not a constant does what C99 says: the bound is
evaluated once, at the declaration; the object lives to the end of the block and
is made afresh on every pass through a loop; `sizeof a` is a run-time value. So
does every type built on one — `double a[n][m]` and `int a[3][n]`,
`int (*p)[n]`, `typedef int T[n];`, and the parameter form
`void f(int n, int m, double a[n][m])` that adjusts to `double (*a)[m]` and
reads its bounds on entry (6.9.1p10). `a[i][j]`, `p + 1`,
`sizeof a / sizeof a[0]` and `sizeof *p` are all computed from the bounds the
declaration evaluated.

`alloca` — the bundled `<alloca.h>`, or `__builtin_alloca` — gives memory that
lives until the *function* returns. Both are emulated on the heap, since Rust
cannot move the stack pointer by an amount chosen at run time, so the storage is
not the stack and the two of them are the only constructs whose expansion needs
more than `core`. What a C program can observe — the elements, the lifetimes,
the run-time `sizeof` — is unchanged. A `goto` or a `case` that would jump into
the scope of one is a located error, as C requires.

## The preprocessor

**The C99 preprocessor.** Object-like and function-like macros with `#`, `##`,
`__VA_ARGS__` and the standard's rescanning rules, every conditional directive,
`#error`, `#warning`, `#pragma` (the catalogue is
[`doc/pragmas.md`](pragmas.md)), and `#line`. C23's `__VA_OPT__`, `#elifdef` and
`#elifndef` are there in a `c23!` block.

**Predefined macros.** `__STDC__`, `__STDC_HOSTED__`, `__STDC_VERSION__`, the
four `__STDC_NO_*` subsetting macros, and `__FILE__` and `__LINE__`
— which name the **`.rs` file** and the line in it, so that they point where the
user is looking. `__DATE__`, `__TIME__` and `__TIMESTAMP__` are fixed
placeholders, because a build has to give the same output twice. On top of those
comes the GCC family and the target description macros (`__GNUC__`,
`__STRICT_ANSI__`, `__x86_64__`, `__linux__`, `__LP64__`, `__SIZEOF_INT__`,
`__BYTE_ORDER__`, the limits and the library types), every one of them read off
the [target model](c-status.md#the-target-model) rather than the host; the
catalogue is
[`doc/gnu-extensions.md`](gnu-extensions.md#preprocessor-extensions).

**What cinrs claims to be.** GCC 14.2: `__GNUC__` is `14`, `__GNUC_MINOR__`
`2`, `__GNUC_PATCHLEVEL__` `0`, because `__GNUC__` is what the world's version
gates test — at the 4.2.1 Clang reports, libdeflate `#error`ed out ("gcc
versions older than 4.9 are no longer supported") and switched off its
VPCLMULQDQ and AVX-VNNI paths, xxHash's dispatcher left AVX2 and AVX-512 off,
and glibc's `<math.h>` took its slow `_Generic` fallback instead of
`__builtin_isnan`. The macros GCC 14 predefines alongside are defined where
cinrs does what they promise (`__GNUC_STDC_INLINE__`, or
`__GNUC_GNU_INLINE__` in `c89!`/`gnu89!`; `__GCC_HAVE_SYNC_COMPARE_AND_SWAP_n`;
`__GCC_ATOMIC_*_LOCK_FREE`; `__BIGGEST_ALIGNMENT__`), `__GCC_IEC_559` and
`__GCC_IEC_559_COMPLEX` are `0` since Annexes F and G are not claimed, and
`__GCC_ASM_FLAG_OUTPUTS__`, `__SIZEOF_FLOAT128__`, `__OPTIMIZE__` and
`__NO_INLINE__` are absent on purpose. Nothing claims to be Clang. And cinrs
says who it really is: `__CINRS__` (and `__cinrs__`) is `1`, and
`__CINRS_MAJOR__`, `__CINRS_MINOR__` and `__CINRS_PATCH__` are its version,
while `__VERSION__` says both — `"14.2.0 (cinrs 0.1.0)"` for this version.
`__has_include`, `__has_include_next`, `__has_attribute`, `__has_c_attribute`,
`__has_builtin`, `__has_feature` and `__has_extension` are answered from cinrs's
own tables, so a program that guards a construct with one is told the truth about
*this* implementation.

**`#line`.** `#line 100` and `#line 100 "generated.c"` do what C99 6.10.4 says:
the line after the directive is line 100 and counts up from there, and `__FILE__`
is the given name until the next directive or the end of that file. The
macro-expanded form works, and so does the `# 100 "generated.c" 1 3 4` line
marker GCC writes in place of one — its flags describe an `#include` that
happened in whatever produced the text, so they are read and dropped. The
numbering is per file, so a `#line` inside a header ends with the header. **Only
`__LINE__` and `__FILE__` move**: a diagnostic, this crate's or `rustc`'s, still
points at the token that was really written, in the file it was really written
in. `__FILE_NAME__` is `__FILE__` without the directory and follows it;
`__BASE_FILE__` names the file the unit started in and does not.

## `#include` and `#embed`

**`#include`, and C23's `#embed`.** The standard headers are **bundled with the
crate**, written in plain C99 rather than read from the platform, and the calls
link against the real C library: `<assert.h>`, `<complex.h>`, `<ctype.h>`,
`<errno.h>`, `<float.h>`, `<inttypes.h>`, `<iso646.h>`, `<limits.h>`,
`<math.h>`, `<signal.h>`, `<stdalign.h>`, `<stdarg.h>`, `<stdatomic.h>`,
`<stdbool.h>`, `<stdckdint.h>`, `<stddef.h>`, `<stdint.h>`, `<stdio.h>`,
`<stdlib.h>`, `<stdnoreturn.h>`, `<string.h>`, `<threads.h>`, `<time.h>`,
`<uchar.h>`, `<wchar.h>` and `<wctype.h>`. A real `<stdio.h>` is not plain C —
glibc's is built out of GNU extensions, compiler builtins and `__asm__` renaming,
and its layouts are the host's rather than the target model's. `<setjmp.h>` is
bundled as a header that says `setjmp`/`longjmp` are not supported; the *call* is
refused whichever header declared it, and declaring them and a `jmp_buf` is fine.

Two groups of headers that are not C's are bundled beside them, for the same
reason: what they declare is the *compiler's* rather than a library's, so the
platform's copy would have nothing to add. `<alloca.h>`, because `alloca` is
generated by this crate; and the Intel intrinsics headers —
`<immintrin.h>`, `<xmmintrin.h>`, `<emmintrin.h>`, `<pmmintrin.h>`,
`<tmmintrin.h>`, `<smmintrin.h>`, `<nmmintrin.h>`, `<wmmintrin.h>`,
`<avxintrin.h>`, `<avx2intrin.h>`, `<x86intrin.h>`, `<popcntintrin.h>`, the
thirty-five AVX-512-era headers under GCC's names (`<avx512fintrin.h>`,
`<avx512vlintrin.h>`, `<gfniintrin.h>`, `<sha512intrin.h>`, …) and an
`<mmintrin.h>` that says MMX is not here — because `_mm_add_epi32` is generated
too, as a call to `core::arch`. See [SIMD intrinsics](#simd-intrinsics).

**Everything else is the platform's.** `<unistd.h>`, `<fcntl.h>`,
`<sys/types.h>`, `<strings.h>`, `<pthread.h>`, `<sys/stat.h>` and the rest of
POSIX come from the platform's own directories, which
`#pragma cinrs system_include` switches on — see [The platform's own
headers](system-headers.md). Asking for one with the switch off is a diagnostic
that says exactly that. (Up to 0.1.0 the first four were bundled in small
incomplete copies; a program that switched the platform on still got the bundled
ones, which is the opposite of what it asked for.) Anything **with a layout** —
`struct stat`, `sigset_t`, `fd_set`, `pthread_t`, `struct sigaction` — was never
bundled at all: a header that guessed at one of those would corrupt memory rather
than fail to compile. `<signal.h>` is C's and stays bundled, and its signal
*numbers* are the platform's; so is `<errno.h>`, which carries the full POSIX
`E*` set at each platform's own values.

Three of them follow the entry point, as the standard says they should:
`<stdbool.h>` defines `bool`, `true` and `false` only before C23, where they
became keywords; `<assert.h>` defines `static_assert` between C11 and C23;
`<stdalign.h>` defines `alignas` and `alignof` before C23. `<stddef.h>` gains
`nullptr_t` and `unreachable()` in C23, and the `<stdlib.h>` functions that do
not come back — `exit`, `_Exit`, `abort`, `quick_exit` — are marked as such in
every entry point, so a function may end with `exit(1);` and write no `return`.
`<wchar.h>` and `<wctype.h>` make `wchar_t` an `int`, which is what the front end
gives `L'x'` on every target and what the Unix platforms do — wide characters are
the one place cinrs is knowingly a Unix compiler, the Microsoft library's
`wchar_t` being 16 bits — while `mbstate_t` *is* spelled the way each platform's
library lays it out. `<uchar.h>`'s `char16_t` and `char32_t` are `unsigned short`
and `unsigned int`, `char8_t` joins them in C23, and none of the three is a
keyword in C, so `_Generic` cannot tell one from its underlying type.

**Your own headers** are found next to the `.rs` file that includes them.
`#include "…"` looks first in the directory of the file the directive is written
in — for a block that is the `.rs` file's, for a header the directory that header
was found in — then the directories
[`#pragma cinrs include_path`](pragmas.md#include_path-dir) and
`CINRS_INCLUDE_PATH` add, and last the bundled headers; `#include <…>` skips the
first step. Every user header read is named by a `const _: &str = include_str!(…)`
in the expansion, so editing one rebuilds the crate. Include guards and
`#pragma once` both work, and a header pulled in twice is read once.

The platform's *own* headers — `/usr/include` and its like, and so `struct
stat`, `DIR` and `pthread_mutex_t` — are one pragma away; see
[System headers](system-headers.md). `#embed "logo.png"` puts the bytes of a
file into the program — with `limit`, `prefix`, `suffix`, `if_empty` and
`__has_embed` — and editing *that* rebuilds the crate too.

## Calling a C library from Rust

A header's declarations are already the binding. A function a unit declares and
does not define becomes an item under its own C name — see [What the C
becomes](translation.md#functions) — so including the library's header and
naming the library is the whole of it:

```rust,ignore
mod z {
    cinrs::c99! {
        #pragma cinrs system_include
        #pragma cinrs link "z"

        #include <zlib.h>

        /* `deflateInit` is a macro over `deflateInit_`, and a macro is not a
         * symbol. C in this very block can call it, and the wrapper is two
         * lines. */
        int z_deflate_init(z_stream *s, int level) {
            return deflateInit(s, level);
        }

        /* An object-like macro is not a Rust constant either; an enumeration
         * *is* one `pub const` per enumerator. */
        enum { ZDEMO_OK = Z_OK, ZDEMO_FINISH = Z_FINISH };
    }
}

let version = unsafe { CStr::from_ptr(z::zlibVersion()) };
let crc = unsafe { z::crc32(0, buf.as_ptr(), buf.len() as z::uInt) };
let rc = unsafe { z::compress(out.as_mut_ptr(), &raw mut len, buf.as_ptr(), n) };
assert_eq!(rc, z::ZDEMO_OK);
```

`z_stream`, `uLong` and `Bytef` come out under their C names like every other
type, so nothing has to be described twice.

Four things are worth knowing before the first `#include <…>` of somebody
else's header.

**A macro needs a line of C.** cinrs exports no macro to Rust, function-like or
object-like, and the two idioms above are the answer: a one-line wrapper
function for a function-like macro, and an `enum { NAME = MACRO }` for the
constants. Both are written inside the block, which is the point — there is no
second language and no build script.

**Naming the library.**
[`#pragma cinrs link "z"`](pragmas.md#link-name) adds `#[link(name = "z")]`, on
an `extern` block of its own. Nothing is needed for the C library itself, which
the Rust runtime has already linked.

**The header has to be findable.**
[`#pragma cinrs system_include`](pragmas.md#system_include-and-system_include-first)
puts the platform's own include directories on the path, which is where
`/usr/include/zlib.h` is; a header that ships with your crate is one
[`#pragma cinrs include_path`](pragmas.md#include_path-dir) away instead, or is
found beside the `.rs` file with no pragma at all. Reading the platform's
headers has [a page of its own](system-headers.md).

**One name, one meaning.** The `mod z` is what the example is really about. A
block's items are glob re-exported into the module the invocation is written in,
so two blocks in one Rust scope that both `#include <stdio.h>` both export
`printf` — and Rust naming `printf` is then `E0659`, "ambiguous name".
Nothing is wrong until that use, and a `mod` around one of the blocks is the
answer; `use libc::*;` beside a block is the same rule again. A Rust item you
*wrote* is not affected at all — an item in a module beats a glob import, so a
`fn strlen` of your own beside such a block is still your `strlen`. What a glob
import does beat is the **prelude**: a unit that declares `drop` or `size_of`
shadows the prelude's for the rest of the module, which shows up as a type error
rather than a silent change. And a declared-only *object* is deliberately not
nameable from Rust at all, for which see [Objects](translation.md#objects) and
[Known limitations](limitations.md).

## GNU extensions

Statement expressions (`({ … })`), `typeof`, `__attribute__((packed))` and
`aligned` with the layout GCC gives them, `__attribute__((cleanup(f)))` —
`f(&x)` on every way out of the scope, which is what systemd's `_cleanup_free_`
and glib's `g_autofree` are made of — `#pragma pack`, `case 1 ... 5:`, range
designators, flexible array members (initialised ones included, for an object
with static storage duration), **labels as values** — `&&label` and the computed
`goto *e`, lowered as GCC lowers it: `&&label` is the label's number among
those whose address is taken, and `goto *e` a `switch` on it, so an
interpreter's dispatch table becomes a `match` in a loop — `asm` labels, `constructor`/`destructor`,
`__func__`, casts to a union type, **nested functions** — lambda-lifted to a
private file-scope item that takes a pointer to each enclosing local it uses, so
a store inside one is visible outside it, and no trampoline is written onto the
stack (the address of a nested function that *does* use the enclosing frame is
the one thing a trampoline is for, and is refused by name) —
`__attribute__((mode(DI)))`, the `__builtin_*` family — bit counting, checked
overflow, `__builtin_expect`, `__builtin_types_compatible_p`, the floating
classifications (`__builtin_isnan`, `signbit`, `fpclassify`, `isunordered` and
the rest, answered in `core` with no maths library involved), and a
`__builtin_X` for a library function X that declares X itself, so
`__builtin_printf` works without `<stdio.h>` — `, ## __VA_ARGS__`,
`__COUNTER__`, `__has_include`, `__has_attribute` and the rest.

Everything spelled with a leading double underscore works in `c99!` too, exactly
as it does in GCC's own `-std=c99`; only the plain spellings `typeof` and `asm`,
and the features of later revisions, need `gnu99!`. Inline *assembly* becomes
Rust's `asm!` for the operand kinds `asm!` has — see [Inline
assembly](#inline-assembly) — and the rest of it is a clear error rather than a
guess, as is every other extension with no honest translation.
[`doc/gnu-extensions.md`](gnu-extensions.md) is the catalogue, row by row.

## `__int128`

GCC's 128-bit integers, in every entry point, as Rust's `i128` and `u128` —
whose x86-64 ABI has matched `__int128`'s since Rust 1.77. Sixteen bytes, ranked
above `long long`, so `(__int128) a * b` really is a 128-bit multiplication;
`__int128_t` and `__uint128_t` are predefined names for the same two types and
`__SIZEOF_INT128__` is `16`. Bit-fields of them work, wider than sixty-four bits
included. C has no 128-bit *literal* and neither does this:
`((__int128) 1) << 100` is the idiom.

`_Generic`, the conversions to and from every other scalar, and passing and
returning one across the ABI (through `...` included) all work.

Two gaps: `__builtin_add_overflow` and its relatives compute their check one
width up from the operands, and there is nothing above 128 bits, so a 128-bit
*operand* is refused (a 128-bit *result* is fine); and `va_arg(ap, __int128)`
needs a `VaArgSafe` implementation Rust still keeps unstable, though *passing*
one through `...` works — and a 128-bit *member* of a `struct` is fine, since
`va_arg` of a record is read eightbyte by eightbyte. And one trap that is C's
rather than this crate's: `(__int128) (1 << 100)` is still a shift of an `int`
by more than its width, because the type of a shift is the type of its left
operand, so widening the result afterwards does not go back and redo it —
diagnosed wherever such a shift is in a constant expression.

## SIMD intrinsics

`#include <immintrin.h>` and write the Intel intrinsics, which is what real C
does. `__m128`, `__m128i`, `__m128d`, `__m256`, `__m256i`, `__m256d` and
AVX-512's `__m512`, `__m512i` and `__m512d` are types; `_mm_add_epi32`,
`_mm256_loadu_si256`, `_mm_movemask_epi8`, `_mm_shuffle_epi8`, `_mm_crc32_u32`,
`_mm_fmadd_ps`, `_pdep_u64`, `_mm512_mask_add_epi32` and six thousand and
sixty-seven others are functions. There is no vector language and
no new syntax: **the mapping is by name.** A call to `_mm_add_ps(a, b)` becomes

```rust
::core::arch::x86_64::_mm_add_ps(a, b)
```

which works because `core::arch::x86_64` was generated from the same Intel data
Intel's own headers are: the same names, the same parameter types, the same
return types. The bundled headers' prototypes were *read out of `core::arch`'s
source* by `crates/cinrs-core/tests/x86_intrinsics.rs`, which writes both the
header and the table that drives the mapping, so a declaration and the function
it resolves to cannot drift apart — and a test that needs no toolchain checks
the two committed files against each other on every run. The source read is
that of Rust 1.98, the minimum supported version, so nothing is declared that
the oldest supported compiler lacks; AVX-512 has been stable in `core::arch`
since 1.89.

**What is covered**, by instruction set, with the header code written for an
older compiler would include it from:

| Instruction set | Count | Header |
| --- | --: | --- |
| SSE | 97 | `<xmmintrin.h>` |
| SSE2 | 226 | `<emmintrin.h>` |
| SSE3 | 11 | `<pmmintrin.h>` |
| SSSE3 | 16 | `<tmmintrin.h>` |
| SSE4.1 | 61 | `<smmintrin.h>` |
| SSE4.2 | 19 | `<nmmintrin.h>` |
| AES, PCLMUL | 7 | `<wmmintrin.h>` |
| AVX | 184 | `<immintrin.h>` |
| AVX2 | 193 | `<immintrin.h>` |
| FMA | 32 | `<immintrin.h>` |
| SHA | 7 | `<immintrin.h>` |
| BMI1 | 17 | `<immintrin.h>` |
| BMI2 | 6 | `<immintrin.h>` |
| POPCNT, LZCNT | 4 | `<immintrin.h>` |
| none (`_mm_pause`) | 1 | `<emmintrin.h>` |
| AVX-512F | 1,421 | `<avx512fintrin.h>` |
| AVX-512F + VL | 1,212 | `<avx512vlintrin.h>` |
| AVX-512BW | 318 | `<avx512bwintrin.h>` |
| AVX-512BW + VL | 510 | `<avx512vlbwintrin.h>` |
| AVX-512CD | 14 | `<avx512cdintrin.h>` |
| AVX-512CD + VL | 28 | `<avx512vlintrin.h>` |
| AVX-512DQ | 222 | `<avx512dqintrin.h>` |
| AVX-512DQ + VL | 177 | `<avx512vldqintrin.h>` |
| AVX-512VBMI, + VL | 10 + 20 | `<avx512vbmiintrin.h>`, `<avx512vbmivlintrin.h>` |
| AVX-512VBMI2, + VL | 50 + 100 | `<avx512vbmi2intrin.h>`, `<avx512vbmi2vlintrin.h>` |
| AVX-512VNNI, + VL | 12 + 24 | `<avx512vnniintrin.h>`, `<avx512vnnivlintrin.h>` |
| AVX-512BITALG, + VL | 8 + 16 | `<avx512bitalgintrin.h>`, `<avx512bitalgvlintrin.h>` |
| AVX-512VPOPCNTDQ, + VL | 6 + 12 | `<avx512vpopcntdqintrin.h>`, `<avx512vpopcntdqvlintrin.h>` |
| AVX-512IFMA, + VL | 6 + 12 | `<avx512ifmaintrin.h>`, `<avx512ifmavlintrin.h>` |
| AVX-512BF16, + VL | 12 + 24 | `<avx512bf16intrin.h>`, `<avx512bf16vlintrin.h>` |
| AVX-512FP16, + VL | 551 + 342 | `<avx512fp16intrin.h>`, `<avx512fp16vlintrin.h>` |
| GFNI (128-bit to 512-bit) | 27 | `<gfniintrin.h>` |
| VAES | 8 | `<vaesintrin.h>` |
| VPCLMULQDQ | 2 | `<vpclmulqdqintrin.h>` |
| AVX-VNNI | 8 | `<avxvnniintrin.h>` |
| AVX-VNNI-INT8 | 12 | `<avxvnniint8intrin.h>` |
| AVX-VNNI-INT16 | 12 | `<avxvnniint16intrin.h>` |
| AVX-IFMA | 4 | `<avxifmaintrin.h>` |
| F16C | 4 | `<f16cintrin.h>` |
| SHA-512 | 3 | `<sha512intrin.h>` |
| SM3 | 3 | `<sm3intrin.h>` |
| SM4 | 4 | `<sm4intrin.h>` |
| **total** | **6,075** | |

`<immintrin.h>` includes the whole chain, so it is the only one most programs
need; `<x86intrin.h>`, `<avxintrin.h>`, `<avx2intrin.h>` and `<popcntintrin.h>`
are the same set under other names. The thirty-five headers from
`<avx512fintrin.h>` down are GCC's names, and `<immintrin.h>` pulls every one
of them in, in the order their types need; one included on its own includes
`<immintrin.h>` first, which brings it back in its place. `<avx512vp2intersectintrin.h>`
is among them and declares nothing (see below). Six thousand prototypes are
not free to read: a unit that includes `<immintrin.h>` takes about 50 ms to
expand in a release build, against about 3 ms for `<emmintrin.h>` — SSE and
SSE2, which is what most code actually uses. Including the narrower header is
worth it in a crate with many blocks. The macros come with them —
`_MM_SHUFFLE`, `_MM_SHUFFLE2`, `_MM_TRANSPOSE4_PS`, `_MM_HINT_*`,
`_MM_FROUND_*`, `_CMP_*`, `_SIDD_*`, `_MM_ROUND_*`, and AVX-512's
`_MM_CMPINT_*`, `_MM_MANT_*` and `_MM_PERM_*` — with the values GCC's headers
give them, which for `_MM_HINT_T0` is 3 rather than the 1 Intel's
documentation prints. Where `core::arch` capitalised an enumerator Intel
spells in lower case, both spellings are defined: `_MM_MANT_SIGN_src` and
`_MM_MANT_SIGN_SRC`, `_MM_MANT_NORM_p5_2` and `_MM_MANT_NORM_P5_2`, and so on,
with GCC's `_MM_CMPINT_GE`, `_MM_CMPINT_GT` and `_MM_CMPINT_UNUSED` besides.

The vector types are ordinary objects of a known size and alignment: sixteen,
thirty-two or sixty-four bytes, aligned to themselves. They may be locals, parameters, return
values, `static`s, array elements, `struct` members and the members of a `union`
that punnes one — `union { __m128i v; int32_t i[4]; unsigned char b[16]; }` is
how most code reaches a lane, and it works because `core::arch`'s types have
exactly that layout. `sizeof` and `_Alignof` answer 16/16, 32/32 and 64/64, and a
`struct` holding one is laid out from those. AVX-512 brings three more kinds of
name. The bfloat16 vectors `__m128bh`, `__m256bh` and `__m512bh` and the
half-precision ones `__m128h`, `__m256h` and `__m512h` are vector types like the
rest, which C only moves between intrinsics: no scalar `__bf16` or `_Float16` is
needed, and none is provided. The masks `__mmask8`, `__mmask16`, `__mmask32`
and `__mmask64` are plain `unsigned char`, `unsigned short`, `unsigned int` and
`unsigned long long`, as they are in GCC and in `core::arch` alike, so a mask is
built and tested with ordinary integer arithmetic. And `_MM_CMPINT_ENUM`,
`_MM_MANTISSA_NORM_ENUM`, `_MM_MANTISSA_SIGN_ENUM` and `_MM_PERM_ENUM` are
`int`: the enumerators are the macros above, and an operand of one of these
types is an immediate like any other. A cast between a vector and an integer is
a diagnostic that says to use an intrinsic.

**GCC's vector operators work on the Intel types.** In GCC `__m128d` is a
vector of two `double`s, and real code writes `a * b + c`, `v * 2.0`,
`1.0 / v`, `-v`, `v += w`, `v[1]` and `(__m128d){x, y}` on it as often as it
writes `_mm_mul_pd`. Each of those is **lowered to the intrinsic that does the
same**, as a call of the function the included header declares — so the
`target` attribute, the pointer rule below and the `[[cinrs::safe]]` refusal
apply to an operator exactly as to the call written out. The lanes are GCC's:
`float` for `__m128`/`__m256`/`__m512`, `double` for the `…d` types, and `long
long` for the `…i` types.

| Operator | `float`/`double` vectors | integer vectors (64-bit lanes) |
| --- | --- | --- |
| `+ - * /` | `_mm*_{add,sub,mul,div}_{ps,pd}` | `+ -` only: `_mm*_{add,sub}_epi64` |
| `& \| ^` | `_mm*_{and,or,xor}_{ps,pd}` (512-bit: through `_si512` and the casts, which AVX-512F has) | `_mm*_{and,or,xor}_si{128,256,512}` |
| unary `-` | the sign bits flipped, `v ^ set1(-0.0)`, as GCC does | `sub_epi64` from zero |
| `~` | refused, as in GCC | `xor` with all ones |
| `== != < <= > >=` | 128-bit `_mm_cmp{eq,neq,lt,le,gt,ge}`, 256-bit `_mm256_cmp` with `_CMP_*_OQ` (`!=` is `_CMP_NEQ_UQ`); all ones in a lane where it holds, typed as `__m128i`/`__m256i` | refused |

A scalar operand, on either side, is converted to the lane type and broadcast
with `set1`; `+= -= *= /= &= \|= ^=` are the operator and a store, on a
variable, a member, an element or `*p` (not on a place reached through side
effects). **`v[i]`** is a lane, of the lane type — an lvalue when the vector is
an object, so `v[1] = x` and `v[0] += y` write into it, and a plain value when
it is not (`(a * b)[0]`, which cannot be assigned to). There is no bounds check,
as GCC has none, but a constant index outside the lanes is an error, and GCC's
reversed `i[v]` is not taken. **Braces** list the lanes in memory order —
`__m128d v = {a, b}`, `(__m128d){a, b}`, a member or an element inside a larger
initialiser — as `_mm*_setr_*` (`_mm_set_epi64x(b, a)` for `__m128i`), the
missing lanes zero and `{}` all zero; a designator is refused. A vector with
static storage duration cannot be given lanes that way, because building one is
a call to `core::arch` and a Rust `static` cannot make it: the message says to
leave it zero and assign the lanes in a function, or to keep the constants in a
`static const double[]` and load them with `_mm_loadu_pd`. **Refused**, each
naming the intrinsic to write: integer `*`, `/`, `%`, shifts and comparisons
(`_mm_mullo_epi32`, `_mm_slli_epi64`, `_mm_cmpeq_epi64` …, because GCC's 64-bit
lanes have no single SSE instruction), the 512-bit comparisons (a mask:
`_mm512_cmp_pd_mask`), every operator on the bfloat16 and half-precision
vectors (`_mm512_add_ph`, `_mm512_dpbf16_ps`), two vectors of different types,
and `!`, `&&`, `||` or `?:` with a vector operand.

**An immediate operand must be a constant.** Intel requires the last operand of
`_mm_slli_epi32(v, 3)`, `_mm_shuffle_epi32(v, _MM_SHUFFLE(3, 2, 1, 0))`,
`_mm_extract_epi32(v, 2)`, `_mm_blend_ps`, `_mm_alignr_epi8` and the rest to be
an integer constant expression, and `core::arch` says the same thing with a
`const` generic. So the argument is folded and written into a turbofish —
`_mm_slli_epi32::<{ 3i32 }>(v)` — and a non-constant is a diagnostic naming the
intrinsic. Valid C already has a constant there; every compiler that has these
intrinsics requires one.

**A memory operand takes any object pointer.** A pointer parameter is declared
`void *` or `const void *` exactly where GCC 15.2's headers declare it so —
335 of them, the SSE2 `_mm_loadu_si16`…`_mm_storeu_si64` and `_mm_clflush`,
`_mm_prefetch`, and AVX-512's loads, stores, gathers, scatters and
compress-stores — and keeps its type everywhere GCC types it
(`_mm256_loadu_si256(const __m256i *)`, `_mm512_stream_si512(__m512i *)`). So
`_mm512_loadu_si512(p)` with an `int *` needs no cast, as in GCC, where since
GCC 14 a mismatched typed pointer would be an error. `core::arch` types every
one of these, so the generated call casts each pointer argument of an
intrinsic with `as *const _` or `as *mut _` and lets rustc infer the rest.

**Asking for an instruction set.** SSE and SSE2 are the x86-64 baseline: cinrs
predefines `__SSE__`, `__SSE2__`, `__SSE_MATH__` and `__SSE2_MATH__` there, and
nothing above them, because a procedural macro cannot see rustc's `-C
target-feature` or `-C target-cpu`. A function that uses anything higher says so
with GCC's own attribute:

```c
__attribute__((target("avx2"))) void f(const int *p) { … }
__attribute__((target("sse4.2,popcnt"))) unsigned checksum(const char *s) { … }
```

which becomes `#[target_feature(enable = "avx2")]` on the generated item. GCC's
name for an instruction set is not always LLVM's, so there is a table:
`bmi` is Rust's `bmi1`, `pclmul` is `pclmulqdq`, `rdrnd` is `rdrand`, `cx16` is
`cmpxchg16b`, GCC's `sse4` is both halves of SSE4 and its `abm` is LZCNT and
POPCNT together; everything else — `sse`, `sse2`, `sse3`, `ssse3`, `sse4.1`,
`sse4.2`, `avx`, `avx2`, `fma`, `bmi2`, `popcnt`, `lzcnt`, `aes`, `sha`, `f16c`,
`adx`, `movbe`, `fxsr`, `xsave`, `rdseed`, the fourteen `avx512*` (from
`avx512f` to `avx512vp2intersect`), `gfni`, `vaes`, `vpclmulqdq`, `avxvnni`,
`avxvnniint8`, `avxvnniint16`, `avxifma`, `avxneconvert`, `sha512`, `sm3`,
`sm4`, `kl` and `widekl` — is spelled the same, in `target` and in
`__builtin_cpu_supports` alike (checked against GCC 15.2). A list names several
at once, `target("gfni,avx512bw,avx512vl")`, as GCC's does. A name that
rustc has not stabilised (`sse4a`, `rtm`, `tbm`), a processor
(`target("arch=haswell")`) and turning an instruction set *off*
(`target("no-avx")`) are each refused with the reason, because silently
ignoring one would give you a function compiled for the baseline and a program
that faults on the first instruction it has not got.

`#pragma GCC target("avx2")` does the same for a region: every function
*defined* after it until `#pragma GCC pop_options` or `#pragma GCC
reset_options`, with `#pragma GCC push_options` saving the set in force.
Successive `target` pragmas accumulate, as GCC's do, and an attribute written
on a function wins over the pragma.

**The pragma also defines the feature macros**, for the rest of the file, as
GCC's does: after `#pragma GCC target("avx2")`, `__AVX2__` is defined, and so is
every instruction set it implies — `__AVX__`, `__SSE4_2__`, `__SSE4_1__`,
`__SSSE3__`, `__SSE3__`, `__POPCNT__` and the rest, GCC 15.2's implications
exactly (`crates/cinrs-core/src/x86.rs`, `TARGET_MACROS`). `avx512f` brings the
AVX2 chain and `__AVX512F__`, each `avx512*` name its own macro
(`__AVX512VL__`, `__AVX512BW__`, `__AVX512FP16__`, …), `fma` `__FMA__`,
`pclmul` `__PCLMUL__`, and so on. `pop_options` takes back what the popped
`target` added, `reset_options` returns to the baseline, and a `no-avx2`
removes `__AVX2__` and everything that implies it. That is how a header that
picks its SIMD path with `#ifdef __AVX2__` — xxHash's does — sees the
instruction set a `#pragma GCC target` before its `#include` asked for. The
*attribute* on one function changes no macro, in GCC or here.

**`always_inline` helpers under a target feature.** A SIMD kernel is usually
written as small `static inline __attribute__((always_inline))` helpers under
the same `-m` flag (BLAKE3's `INLINE`, xxHash's `XXH_FORCE_INLINE`). rustc
refuses `#[inline(always)]` beside `#[target_feature]`, and plain `#[inline]`
is a hint LLVM may decline — BLAKE3's `round_fn16`, called seven times from
one kernel, stayed out of line with its state spilled around every call. So
such a helper, when it is `static` and nothing takes its address, is generated
as a Rust `#[inline(always)] unsafe fn` *without* `#[target_feature]` and
without `extern "C"`: GCC refuses to inline an `always_inline` function into a
caller without its target options, so every caller of a program GCC accepts
has the features, and once the helper is inlined there, so are the intrinsics
it calls. Vectors pass by value between Rust functions whatever the features.
A helper whose address is taken (or named by `cleanup`), one with external
linkage, and a [safe](#safe-functions) one stay `extern "C"` functions with the
feature and `#[inline]`.

**Asking the processor.** Without the pragma, only the baseline is predefined,
so `#ifdef __AVX2__` and `#ifdef __AVX512F__` take the other branch, whatever
the machine — the right question
is the run-time one, and `__builtin_cpu_supports` is it:

```c
if (__builtin_cpu_supports("avx2")) return wide(p); else return narrow(p);
```

becomes `std::is_x86_feature_detected!("avx2")`, which reads the same `cpuid`
leaves GCC's own builtin does, and answers as an `int`. It takes the GCC
spellings above. `__builtin_cpu_init()` is accepted and does nothing, because
Rust's detection is lazy and needs no initialiser. Both need `std`:
`__builtin_cpu_supports` under `#pragma cinrs no_std` is a located error, since
`core` has no processor detection at all.

**The address of an intrinsic** works, which it does in GCC because its
intrinsics are `static inline` functions. `core::arch`'s have the Rust ABI and
cannot be coerced to a C function pointer, so cinrs generates a private
`unsafe extern "C"` shim per intrinsic whose address is taken — carrying the
intrinsic's own `#[target_feature]` — and hands out the shim's address. One
shim per intrinsic however many times it is taken, and none at all for a unit
that only calls them. The exception is an intrinsic with an immediate operand,
whose address cannot be taken at all: a function pointer has nowhere to put the
constant, and that is a diagnostic naming the intrinsic.

**A `[[cinrs::safe]]` function cannot call one.** Every `core::arch` intrinsic
is a `#[target_feature]` function, and Rust makes such a function unsafe to
call from anywhere that does not carry the same instruction set — only the
program knows whether the processor running it has those instructions. A safe
function has no `unsafe` block to call one from, and carrying the attribute
itself would make *it* unsafe to call, which is the opposite of what
`[[cinrs::safe]]` promises. Both are refused, with the instruction set named.

**Nothing is exported.** An intrinsic has no symbol, so it produces no item:
`<immintrin.h>`'s six thousand and seventy-five prototypes add nothing to the
unit's `extern` block and nothing to the glob re-export, which is what keeps a
`use core::arch::x86_64::*` in the surrounding Rust from clashing with them.
The only Rust an intrinsic produces is the call.

**What is not here.**

* **The GNU vector extensions' own types.** `__attribute__((vector_size(16)))`
  on a `typedef` of the program's own, `__builtin_shuffle` and
  `__builtin_ia32_*` are refused, with a diagnostic that names the Intel
  intrinsic to write instead: an arbitrary vector type would need `core::simd`,
  which is unstable. The operators on the Intel types themselves are here; see
  above.
* **What `core::arch` still keeps unstable.** AVX512-VP2INTERSECT's six
  `_mm*_2intersect_*` — its target feature is stable, so `target(
  "avx512vp2intersect")` and `__builtin_cpu_supports("avx512vp2intersect")`
  work, but `<avx512vp2intersectintrin.h>` declares nothing. The forty-two
  AVX512-FP16 intrinsics that take or return an `f16` scalar or point at one,
  which Rust has not stabilised: `_mm*_set_ph`, `_mm*_set1_ph`, `_mm*_setr_ph`,
  `_mm*_load_ph`, `_mm*_loadu_ph`, `_mm*_store_ph`, `_mm*_storeu_ph`,
  `_mm*_reduce_{add,mul,min,max}_ph`, `_mm*_cvtsh_h` and the six `*_sh`
  loads, stores and setters — a half-precision vector is loaded as
  `_mm512_castsi512_ph(_mm512_loadu_si512(p))` instead. And AVX512-BF16's two
  scalar conversions, `_mm_cvtsbh_ss` and `_mm_cvtness_sbh`.
* **MMX and `__m64`.** The standard library dropped them; `<mmintrin.h>` is an
  `#error` that says so and names the SSE2 form of each intrinsic —
  `_mm_add_pi16` is `_mm_add_epi16`.
* **NEON, SVE, AltiVec, the RISC-V vector extension.** `core::arch` has them
  under other names; nothing in cinrs maps them, and `<immintrin.h>` on a
  target that is not x86 is an `#error` naming the reason. Guard the include
  with `#ifdef __x86_64__`.
* **Sixty-nine names the generator skips**, each for a reason it prints; sixty-two
  of them are simply absent. Ten are **deprecated** in `core::arch` and would
  warn on every use: `_mm_getcsr`, `_mm_setcsr` and the eight
  `_MM_GET_*`/`_MM_SET_*` wrappers over them, which read and write the SSE
  control word — write `asm volatile("stmxcsr (%0)" : : "r"(&csr) : "memory")`
  and its `ldmxcsr` twin instead (see [Inline assembly](#inline-assembly)).
  Fifty-one are **not stable**: the fifty in the bullet above, and
  `_MM_SHUFFLE`, which is a macro in the header anyway. And two take a Rust
  *reference* for an output: `_mulx_u32` and `_mulx_u64`, whose high half goes
  through `&mut`.

  Six more that C cannot spell come back as **macros** instead, because each is
  defined in terms of something that is here: `_mm256_broadcast_ss`,
  `_mm_broadcast_ss`, `_mm256_broadcast_sd`, `_mm256_broadcast_ps` and
  `_mm256_broadcast_pd` take a reference in `core::arch` and are the
  load-and-splat they are documented as, and `_MM_TRANSPOSE4_PS` is a macro in
  Intel's own headers too. `_mm_popcnt_u32` and `_mm_popcnt_u64` are macros for
  the same reason: `core::arch` calls them `_popcnt32` and `_popcnt64`.

`<mm_malloc.h>` is bundled too: `_mm_malloc(size, align)` and `_mm_free(p)`,
as `static inline` functions in portable C over `malloc` — GCC's call
`posix_memalign`, which the Microsoft runtime lacks — with GCC's rules: an
alignment of 1, 2 or 4 means a pointer's, zero or one that is not a power of
two gives a null pointer, and only `_mm_free` may free the block.
`<xmmintrin.h>` includes it as GCC's and Clang's do, which is also how
`<stdlib.h>` arrives with `<xmmintrin.h>` — programs that call `exit` and
`atoi` with only the intrinsics header included count on it — so every unit
that includes an intrinsics header carries `<stdlib.h>`'s declarations and the
two functions.

One rule that is neither cinrs's nor C's but the ABI's, and worth knowing
before it bites: **a function that passes or returns a 256-bit vector by value
needs `__attribute__((target("avx")))`** (or `avx2`, which implies it), **and
one that passes or returns a 512-bit vector by value needs
`target("avx512f")`** — `avx2` is not enough. Passing a `__m256` in a register
is only possible with AVX enabled, and a `__m512` only with AVX-512F, so rustc
refuses the definition and the call without it: the function "uses SIMD vector
type `__m512` which (with the chosen ABI) requires the `avx512f` target
feature", in its words — the same reason `gcc -Wpsabi` warns about the same C.
A 128-bit vector needs nothing, because SSE2 is the baseline, and a `__mmask*`
is an integer and needs nothing either. Both rules are documented in
[Limitations](limitations.md), and `tests/ui/simd_abi_512.rs` holds the 512-bit
one.

## Inline assembly

GCC's extended asm and Rust's `asm!` are the same model — an opaque template
and a list of operands, each with a constraint that says where it lives — so
for the shapes real C writes (`rdtsc`, `cpuid`, `pause`, a bit scan, an
`xchg`, a `mul` into `edx:eax`, `asm volatile("" ::: "memory")`) the
translation is mechanical, and cinrs does it: **an `asm` statement becomes
`::core::arch::asm!`.** `asm`, `__asm` and `__asm__` with `volatile`, `inline`
and their double-underscore spellings all work; the plain `asm` is `gnu99!`'s,
as it is in GCC. What `asm!` cannot say is refused, naming the constraint or
the feature and the rewrite, because an `asm!` whose meaning differed from
GCC's would be worse than none.

```c
asm("addl %2, %0" : "=r"(s) : "r"(b), "0"(a) : "rcx");
```

becomes

```rust
::core::arch::asm!(
    "addl {o0:e}, {o0:e} /* {o1:e} */",
    o0 = inout(reg) a as ::core::ffi::c_int => s,
    o1 = in(reg) b,
    out("rcx") _,
    options(att_syntax),
);
```

Every operand the template can refer to is **named** — `o` and its GCC number
— because `asm!` refuses a positional operand after an explicit-register one
and GCC puts no order on its operands. The template stays AT&T, which is what
GCC's x86 templates are, so `options(att_syntax)` is always there, and nothing
else is: `asm!` without options is volatile, reads and writes memory and
clobbers the flags, which is GCC's most conservative reading of an `asm`, so
`pure`, `nomem`, `readonly`, `preserves_flags` and `nostack` are never added.
A tied input (`"0"`) is folded into the output it is tied to, and a `%2` that
names it names that output. An operand the template never mentions — an input
there only to keep a value live — is mentioned in a trailing assembler comment,
because `asm!` calls an unused named operand an error and GCC does not.

| GCC | `asm!` |
| --- | --- |
| `"r"`, and `"g"`, `"rm"`, `"ri"` (the register is chosen) | `reg`, or `reg_byte` for an 8-bit value |
| `"=r"`, `"=&r"`, `"+r"` | `lateout`, `out`, `inout` |
| `"q"`, `"Q"` | as `"r"`; `reg_abcd` on 32-bit x86 |
| `"a"`, `"c"`, `"d"`, `"S"`, `"D"` | that register at the operand's width: `in("al")`, `in("eax")`, `in("rax")`, … |
| `"x"`, `"v"` | by the operand's width: `xmm_reg` (scalars and 128-bit vectors), `ymm_reg` (`__m256*`), `zmm_reg` (`__m512*`) |
| `"i"`, `"n"` | `const`, folded to a constant, and written `${oN}` in the template |
| `"0"` … `"9"` (tied to an output) | `inout(…) input => output` |
| `%0`, `%[name]` | `{o0}` at the operand's width — `{o0:e}` for 32 bits, `{o0:x}` for 16 — or the register itself for an explicit one |
| `%k0`, `%w0`, `%b0`, `%h0`, `%q0` | `{o0:e}`, `{o0:x}`, `{o0:l}`, `{o0:h}` (in `reg_abcd`), `{o0:r}` |
| `%x0`, `%t0`, `%g0` (a vector operand) | `{o0:x}`, `{o0:y}`, `{o0:z}`: its xmm, ymm, zmm register |
| `%%`, `%{`, `%}`, `%\|` | `%`, `{{`, `}}`, `\|` |
| clobber `"rax"`, `"ecx"`, `"xmm0"`, … | `out("rax") _` |
| clobber `"memory"`, `"cc"` | nothing: `asm!` assumes both |

Where a constraint offers a register *or* memory, the register is chosen, which
may change the instruction GCC would have picked but not what the statement
does. An explicit register cannot be named from an `asm!` template, so a `%0`
that refers to `"a"` is written as the register — `%eax`, or `%al`, `%ax`,
`%rax`, `%ah` for the modifiers. The operand types are C's own integers,
pointers, `float` and `double`, and the vector types in `"x"`; a
`_Bool`, an `__int128` or a `struct` operand is refused. As in GCC, `"x"` is a
vector register as wide as the operand: a `__m256i` is a ymm register — so
libdeflate's `__asm__("" : "+x"(v))` barrier on AVX2 accumulators maps — and
needs a function with the `avx` target feature, a `__m512i` a zmm register and
`avx512f`; without it rustc's own error says so ("register class `ymm_reg`
requires the `avx` target feature"). GCC's AVX-512 `"v"` (any register 0–31)
maps exactly as `"x"`: `asm!` lets the classes reach registers 16–31 by the
function's target features, which is the only difference between the letters. An output may be any
lvalue but a bit-field: a member, an element, `*p`. The place's own side
effects — the `i++` in `a[i++]` — happen once. **Basic asm**, with no colon,
is its template and `options(att_syntax)`: `asm("mfence")`,
`__asm__ __volatile__("pause")`.

`asm!` is `unsafe`, which every generated function body already is, so inline
assembly works in any function **but a `[[cinrs::safe]]` one**, where it is
refused by name. It needs only `core`, so it works under `#pragma cinrs
no_std`. It is **x86 and x86-64 only**: the template is one architecture's
assembly and the operand mapping is x86's registers, so an `asm` for another
target is a located error.

**What is refused, and what to write instead.**

* **A memory operand** — `"m"`, `"+m"`, `"=m"`, `"o"`, `"V"`, `"p"`. `asm!` has
  none. Pass the address in a register and write the memory reference in the
  template: `asm("incl (%0)" : : "r"(&x) : "memory")`. The template is not
  rewritten for you.
* **`rbx` as a clobber** — `rbx`, `ebx`, `bx` or `bl`. rustc keeps rbx for LLVM
  and refuses it as an `asm!` operand or clobber. The `"b"` constraint (`"b"`,
  `"=b"`, `"+b"`, `"=&b"`, and a `"0"` tied to one) *is* accepted: the value is
  carried in a scratch register that an `xchg` swaps with rbx on either side
  of the template (`xchgq %rbx, {oN:r}` on x86-64, `xchgl %ebx, {oN:e}` on
  32-bit x86) — what GCC's own `<cpuid.h>` does by hand for 32-bit PIC code —
  so the template runs with the input in rbx, the output is what it left
  there, and rbx is restored afterwards. `%0` naming the operand is written as
  `%ebx`, `%rbx`, `%bx`, `%bl` or `%bh` as the width and modifier ask. So
  `"=b"(x)` is the way to read a register an instruction writes to rbx; an
  `rbx` clobber stays refused (and one beside a `"b"` operand is an error, as
  in GCC), as does a second `"b"` operand in one statement and a one-byte
  one. `rsp` and `rbp` are refused too.
* **`%=`**, the number unique to each instance of a statement. `asm!` has no
  such number; a GNU as local label is the same thing: `1:` … `jnz 1b`.
* **`asm goto`**, and `%l0`. Not in this release: the jump to a C label has to
  go through the function's control flow. Branch in C on a value the `asm`
  sets.
* **Flag outputs** (`"=@ccz"`). `asm!` has none; set a byte register from the
  flag in the template (`setz %b0`) with `"=q"`.
* **`"A"`**, the `edx:eax` pair: use `"=a"` and `"=d"` with two variables.
* **The x87 and MMX registers** — `"f"`, `"t"`, `"u"`, `"y"` — which `asm!`
  has no operand for, and `"X"`, `"R"` and the `"Y…"` family.
* **The range-checked immediates** `"I"` … `"O"`, `"e"`, `"Z"`: write `"i"`.
* **`%c0`, `%P0`, `%a0`**, which print a constant or an address bare: write the
  operand with `"i"` and `%0`, or pass the address in a register.
* **Intel syntax**: a template that opens with `.intel_syntax`. Write the
  AT&T form. GCC's dialect alternatives `{att|intel}` *are* taken — cinrs
  always gives `asm!` `options(att_syntax)`, so the first, AT&T, alternative is
  kept and the rest dropped: `"{cpuid|cpuid}"` is `cpuid` and
  `"{movl|mov} %1, %0"` is `movl %1, %0` (GCC's result too, checked with
  15.2). That is in an extended `asm` — one with a `:`, operands or not. GCC
  passes a *basic* template to the assembler verbatim, where the braces are an
  assembler error, so a brace there is refused. `%{`, `%}` and `%|` are the
  literal characters; a nested `{`, or one that is never closed, is an error.
* **`register int x asm("eax")`**, a register variable: write the register as a
  constraint of the `asm` that uses it, `"a"(x)`.

**`<cpuid.h>`** is bundled, with the `xchg` written out by hand. The same
names mean the same things: `__cpuid(leaf, a, b, c, d)` and
`__cpuid_count(leaf, subleaf, a, b, c, d)` store the four registers into four
lvalues, `__get_cpuid_max(ext, &sig)` returns the highest leaf of a range,
`__get_cpuid` and `__get_cpuid_count` fill four pointers and return 0 for a leaf
the processor does not have, and the `bit_*` macros (`bit_SSE2`, `bit_AVX2`,
`bit_BMI2`, …) and `signature_INTEL_ebx` and friends decode the answer. It is
C on the `xchg` idiom above, nothing more. It narrows two things GCC leaves
undefined: `__get_cpuid` passes subleaf 0 in ecx, which matters for leaf 7, and
on 32-bit x86 `__get_cpuid_max` does not first test the EFLAGS ID bit for a
processor older than the Pentium. On a target that is not x86 it is an `#error`
naming the reason. `__builtin_cpu_supports` (above) is still the simpler way to
ask about one instruction set.

`tests/inline_asm.rs` runs all of this — every operand kind, the modifiers,
member and pointer outputs, `asm` in a loop, a `switch` and a function with a
`goto`, and `<cpuid.h>` against Rust's own `__cpuid` — with the values `gcc -O2`
printed for the same C.

## Thread-local objects

`_Thread_local`, C23's `thread_local` and GNU's `__thread` become a
`std::thread_local!` holding an `UnsafeCell`, so a C counter really is one per
thread and `&x` is a pointer to *this* thread's copy. C's own placement rules
apply (6.7.1p3): file scope, on its own or beside `static`; at block scope only
on a `static`, an object with automatic storage duration being per *call* rather
than per thread; on a parameter or a function it is an error, and so is a
non-constant initialiser. The initialiser goes in `thread_local!`'s `const { … }`
block — the cheap form — wherever Rust allows it, which is everywhere except an
initialiser that names the address of another item (`_Thread_local int *p =
&global;`), since a Rust constant may not refer to a `static`; that one takes the
lazy form, which nothing in C can observe. An `extern` thread-local object and
exporting one under `#pragma cinrs export` are refused — both would need Rust's
unstable `#[thread_local]`.

## C11 threads

`<threads.h>` (7.26) is bundled, and the threads it makes are the C library's
own: `thrd_create`, `mtx_*`, `cnd_*`, `tss_*` and `call_once` are that library's
functions, and `mtx_t` and `cnd_t` are laid out as its `pthread_mutex_t` and
`pthread_cond_t` — which a differential test against the host's `cc` checks. Two
libraries are modelled, glibc (2.28 and later) and musl, both on Linux; on Apple
and Windows, whose runtimes have no such header at all, and on the platforms
whose layouts cinrs does not know, the header is an `#error` naming the reason
and `__STDC_NO_THREADS__` says so — which is what lets a portable program take
the other branch rather than hit the `#error`.

`struct timespec` comes from `<time.h>`, where C puts it, beside `timespec_get`
and `TIME_UTC`, and `thread_local` is defined as `_Thread_local` up to C17 and
left alone in `c23!`, where it is a keyword. One function is declared but must
not be called from translated C: `thrd_exit` is `pthread_exit`, which ends the
thread by forcing an unwind through the frames above it, and Rust aborts rather
than let a foreign unwind cross the generated `extern "C"` frame. Return from
the thread function instead.

## Atomics

C11's `_Atomic` — the qualifier and the `_Atomic(T)` specifier — the bundled
`<stdatomic.h>`, GCC's memory-order-aware `__atomic_*` builtins, the older
sequentially consistent `__sync_*` ones and Clang's `__c11_atomic_*`, all of
them on `core::sync::atomic` reached with `AtomicX::from_ptr` over the object's
address (stable since Rust 1.75).

`_Atomic T` is a *type*, not a flag on a declaration: `_Atomic int *` and
`int *` are different types, and a store through the first is atomic. An
`_Atomic` object is a plain one of the underlying type whose every read is a
`SeqCst` load, every write a `SeqCst` store, and every `+=`, `++` and `--` a
single read-modify-write, exactly as 6.5.16.2 says — never a load followed by a
store; the five operators an atomic has a method for become that method and the
rest become the compare-exchange loop the method would have been. The
*initialiser* of a declaration is a plain write, which is what 7.17.2.1p2 says
it is. Reading one gives a value of the underlying type, lvalue conversion
having dropped the `_Atomic` (6.3.2.1p2), so `_Generic(x, int: …)` matches an
`_Atomic int` lvalue and nothing downstream of the read has to know about
atomics at all. The object's alignment is its size,
which is what makes `_Atomic long long` eight-byte aligned. The scalars are
covered — the 1-, 2-, 4- and 8-byte integers, `_Bool`, `float` and `double`
(through the integer atomic of the same width and `to_bits`), and object
pointers as an `AtomicPtr`. An `_Atomic` `struct` and a 128-bit one are refused:
neither has a lock-free counterpart, and there is nothing here to be a lock.

The one deliberate difference between the two builtin families is pointer
arithmetic: `__atomic_fetch_add` counts **bytes**, as GCC's does, and
`atomic_fetch_add` from the header counts **elements**, as C11 7.17.7.5
requires — the header goes through `__c11_atomic_fetch_add`, which is the one
that scales.

`<stdatomic.h>` is bundled whole: the `atomic_bool` … `atomic_uintmax_t`
typedefs, `atomic_flag` with `ATOMIC_FLAG_INIT`, the `memory_order` enumeration,
`ATOMIC_VAR_INIT`, `atomic_init`, `kill_dependency`, the fences,
`atomic_is_lock_free`, the `ATOMIC_*_LOCK_FREE` macros (all `2`) and the generic
functions. A memory order must be an integer constant expression — one of the
`__ATOMIC_RELAXED` … `__ATOMIC_SEQ_CST` macros, predefined everywhere, or a
`memory_order_…` constant, which are the same values; `__ATOMIC_CONSUME` is an
acquire, as in every compiler. An order that is *not* constant falls back to
`__ATOMIC_SEQ_CST` as GCC's own documentation says it does, and a non-constant
`weak` flag is taken as *strong*, which a weak compare-exchange is always
allowed to be. An order the operation may not have — a load that releases, a
store that acquires, a failure order stronger than the success one — is a
diagnostic here, where Rust would panic at run time and C leaves it undefined.

## Pragmas

`#pragma cinrs target "…"` picks the machine the unit is translated for, over
the `CINRS_TARGET` a build script sets — see
[Cross-compilation](cross-compilation.md);
`#pragma cinrs include_path "…"` adds a search directory;
`#pragma cinrs link "…"` links a library;
`#pragma cinrs export` gives everything with external linkage a real C symbol,
so that another block — or a C library — can call it by name (with C's own risk:
two exported units defining one name is a duplicate symbol);
`#pragma cinrs safe f g` generates those functions without `unsafe` and lets
`rustc` check them;
`#pragma cinrs no_std` takes the storage a variable length array or `alloca`
needs from `alloc` rather than from `std`, and refuses a thread-local object,
which needs `std` outright;
`#pragma cinrs crate "…"` says where the `cinrs` crate itself is, for a renamed
dependency — the generated code names it only for complex numbers, and
`::cinrs` is the default. `#pragma cinrs system_include` is the eighth and has a
[page](system-headers.md) of its own.

There is no pragma for naming the module an expansion goes into: it is
`__cinrs_unit_<hash>`, private and glob re-exported, and an ordinary Rust `mod`
around the invocation is what gives its items a path — `packet::Header` — and
what keeps two units that define the same name apart.

[`doc/pragmas.md`](pragmas.md) is the reference: every option's exact syntax,
how far it reaches, what is an error, and the environment variable that does the
same thing — together with the pragmas the preprocessor itself knows (`once`,
`pack`, `push_macro`, `#pragma GCC …`) and what happens to one it does not.

## Safe functions

A C function is a foreign function, so calling one is `unsafe` — unless it is
marked `[[cinrs::safe]]`, `__attribute__((cinrs_safe))` or named by
`#pragma cinrs safe f g`. Such a function is generated as a plain
`pub extern "C" fn` whose body is *not* wrapped in an `unsafe` block, so `rustc`
checks the whole translation: a raw pointer dereference, a read of a C global or
of a `union` member, a call to the C library or to a function of the unit that is
not itself safe are each an error with the caret on the C that asked for it.
What is left compiles and is a useful language — arithmetic, control flow
(`goto` included, whose labelled blocks, recovered loops and state machine are
safe code too),
locals, records and `enum`s by value, `_Bool`, the complex arithmetic, pointer
*values* (holding one, comparing it, returning it), string literals,
bit-field accessors on a local, and calls to other safe functions — and Rust
calls it as `fact(10)`. Division is Rust's, which is C99's; what C leaves
undefined there — a zero divisor, `INT_MIN / -1` — Rust panics on, and a panic
cannot cross an `extern "C"` frame, so the program aborts. That is a run-time
property rather than something `safe` promises.

Three shapes are refused where the request was written rather than left to
`rustc`: a function this unit only declares, a variadic definition, and a GNU
nested function. One corner goes the other way: the accessors generated for the
bit-fields of a **`union`** read the storage inside an `unsafe` block of their
own, so a safe function may read one where reading an ordinary member of that
`union` would be refused. Nothing about the C changes — a call *from* C to a safe
function is what it always was, and a function that is not safe may call one
that is. [`doc/pragmas.md`](pragmas.md#safe-f-g-h) is the reference.

## Input forms

C the Rust lexer accepts is written as raw tokens — `int café(void)` included,
since Rust's identifiers are UAX #31's too; C it refuses (hexadecimal floating
constants, `'ab'`, the prefixed literals `L"…"`, `u8"…"`, `u"…"`, `U"…"` and
`u8'x'`, a universal character name, `\` line continuations, C23's digit
separators such as `1'000'000`) goes in a string literal instead. `##` cannot be
written in raw-token form either, so a replacement list spells the pasting
operator `a # # b`. The third form is a **file**: `include_c99!("parser.c")` and
one such macro per entry point — see [Including a C file](include-c.md).

### Where a raw-token block's text comes from

A token stream is not text, and the preprocessor needs text: `#define` and `#if`
are *lines*, and `#error` reproduces what was written. So a raw-token block
recovers its own source text, in one of three ways, and which one it was is
invisible unless the last applies:

* **From the `.rs` file, by position.** The first token is asked which file it is
  in and where, and that file is sliced between the first and the last token —
  comments, line breaks and columns exactly as written. This is what a
  `cargo build` does, every time.
* **From the `.rs` file, by finding the invocation.** A host may hand a
  procedural macro tokens with *no positions at all*: rust-analyzer reports no
  file, no source text and line 1 column 0 for every token alike. The text is
  still on disk, and which text it is can be proved — the crate's `.rs` files
  (the directory `CARGO_MANIFEST_DIR` names) are searched for an invocation whose
  token sequence is exactly the one the macro was handed, matching token by token
  with nothing but whitespace and comments between and nothing left over. A match
  gives the same text the first way would, so everything else — `__FILE__`, a
  quoted `#include` searching beside the file, `include_str!` rebuild tracking,
  the identity of the unit — is unchanged. The search is remembered for the
  process that made it — the file list for a couple of seconds, each file's text
  for as long as the file reports the modification time and length it was read at
  — so the burst of expansions one edit provokes walks the crate once instead of
  once per block, while a file that was just saved is read again by the very next
  expansion.
* **From the tokens alone.** Where the invocation is not found either, the text
  is rebuilt from the tokens: one space between two of them, none where the host
  says they were written together, so `->`, `<<=`, `&&` and `++` stay single
  operators and `a - -b` stays what it was. This is enough for any C that is not
  a directive; the directives whose end the tokens themselves give away
  (`#include <…>`, `#include "…"`, `#ifdef X`, `#ifndef X`, `#undef X`, `#else`,
  `#endif`, `#pragma once`, C23's `#elifdef`/`#elifndef`) are written on a line
  of their own, and one whose end would have to be guessed is a single clear
  diagnostic instead — see [Limitations](limitations.md).

#### Identical invocations

Two blocks with the *same tokens* are ordinary — a test file repeats small units
— and a host that gives no positions cannot say which of them it is expanding.
The second way above therefore has a rule, and it only ever applies in an editor:
a build knows exactly where its invocation is.

Whichever copy is found, the **C is the same**: that is what matching every token
proves. What can differ is only what follows from *where* it was written — the
directory a quoted `#include "…"` is looked for beside, `__FILE__` and
`__LINE__`, the path whose edits trigger a rebuild, the name of the module the
expansion goes into — and, in a contrived case, the line structure around a
directive, since `#define A 1` on a line of its own and the same tokens run
together with what follows do not mean the same thing.

The copies are tried in sorted path order, and the one taken is **the first whose
directory holds every header the unit includes by name**; failing that, the first
of them. So two copies of a block in two directories, only one of which has the
header next to it, both work. A header that comes from an include path or from
the bundled set is next to none of the copies, which is why this is a preference
and never a refusal — and nothing is reported when copies disagree about
anything else, because a `__LINE__` that is off by a few lines in an editor is
better than a red mark under code that compiles. Two *identical* blocks in one
Rust module are the one case with no good answer: both are found as the same
copy, so both expansions carry the same generated module name. rust-analyzer says
nothing at all about that today, and the worst it could say is that a name is
defined twice — never anything stranger. `rustc` has no such trouble, since the
two invocations are in different places and it knows it; and neither has the
editor once the two blocks differ in a single token (a comment is not one: the
search compares tokens, and skips comments on both sides).

## Diagnostics

Every generated token carries the span of the C token it came from, so both
`cargo` and an IDE put the caret on the C code — for the front end's own
diagnostics and for `rustc`'s. Pointing *inside* a string literal needs
`Literal::subspan`, which is unstable, so there the position is appended to the
message instead; the crate's `nightly` feature turns it back into a caret.

An IDE is not a compiler, and rust-analyzer in particular gives a procedural
macro no positions for its tokens while still resolving the spans that come back
out on generated ones. That is why a block recovers its text [the second
way](#where-a-raw-token-blocks-text-comes-from) there: with the text in hand,
every caret lands where it does in a build. The one case it cannot cover is a
file whose buffer has not been saved, where what is on disk is not what is being
typed — then a block holding `#define` or `#if` reports that it cannot be read,
once, on the `#`, until the file is saved.

## Variadic functions

Declaring and calling one — `printf` and friends — works on any supported
toolchain; *defining* one needs Rust 1.99's `c_variadic`, and is a clear error
before that rather than an expansion the compiler would reject.

`va_list` is `core::ffi::VaList`, so a list passes straight to `vprintf`. The
names come from the bundled `<stdarg.h>`, which defines them in terms of the
`__builtin_va_*` forms the compiler owns — exactly as GCC's own header does, so a
unit that does not include it may use `va_list` and `va_end` as names of its own.
A **`va_list *`** is `*mut VaList<'_>` and works as a parameter and as a local,
which is what lets a helper advance the caller's list; a `va_list` local inside
such a helper starts out as a copy of `*ap`, so `va_copy(copy, *ap)` works there
too. What the elided lifetime rules out — a member, a file-scope object, a return
type — is in [Limitations](limitations.md).

`va_arg` at a `struct`, `union` or complex type is allowed by C99 and has no
`next_arg` in Rust's `VaList`, so the value is **reassembled from the registers
the ABI passed it in**. On x86-64 System V (AMD64 psABI 3.2.3) an argument of at
most sixteen bytes is split into one or two *eightbytes*, each classified INTEGER
or SSE from the members that overlap it — one integer member anywhere in an
eightbyte makes the whole of it INTEGER — so a `va_arg` becomes one
`next_arg::<u64>()` per INTEGER eightbyte and one `next_arg::<f64>()` per SSE
one, gathered into a `[u64; N]` and read back out. A `union` is classified per
member, since they all start at offset zero; a bit-field counts as the integer
its storage is; a nested record or array flattens to its scalars; and `__int128`
is fine *inside* a record, nothing reading it at its own type.
`tests/vaarg_structs.rs` checks the classification against the host's own C
compiler over a hundred and fifty records.

Refused by name rather than translated with the wrong rules: a record **larger
than sixteen bytes**, or one with a member the packing has moved off its own
type's alignment, which is class MEMORY and lives in the overflow area nothing in
the stable `VaList` API can reach; and any target that is not x86-64 System V —
the Microsoft x64 ABI passes an aggregate over eight bytes *by pointer*, AArch64
has homogeneous float aggregates, i686 puts everything on the stack. Two edges
are worth knowing rather than refusing. The eightbytes are read one at a time
while the ABI decides register-versus-stack for the argument *as a whole*: they
agree unless a two-eightbyte record is the very argument that exhausts the
register save area — five integer or seven SSE eightbytes in — where the caller
pushes the whole record and reading its first eightbyte still finds a register. A
record of at most eight bytes is a single eightbyte and is therefore always
exact. And `long double` is mapped to `double` throughout, so a member of that
type is classified SSE where a real `long double` would be X87 and would put the
whole record in memory.

## Complex numbers

`float _Complex` and `double _Complex` are `cinrs::rt::Complex<f32>` and
`Complex<f64>` — which is
[`num_complex::Complex`](https://docs.rs/num-complex), the type the numeric half
of crates.io already speaks, so a complex value crosses the boundary without a
conversion. `long double _Complex` is `double _Complex`, the same mapping
`long double` has and the same ABI caveat. The types themselves are two
components side by side, so `sizeof(double _Complex)` is 16 and its alignment is
`double`'s, which is what every ABI cinrs targets says and what the
[data-model assertion](cross-compilation.md#the-assertion-that-guards-it) checks
for a unit that has one.

The operators are `+ - * /`, unary `+ -`, `==`/`!=`, compound assignment and
`++`/`--`, which step the *real* part as GCC has them; the relational operators,
`%`, the bitwise operators and the shifts are refused with the reason — C gives
them real operands only, and the complex numbers are not ordered. Two things
about the arithmetic are observable and neither is what a first reading of the
standard suggests. **An infinity survives**: C99 Annex G.5.1, which GCC
implements by default, requires a product or quotient with an infinite operand to
be infinite even where the schoolbook formula produces `NaN + iNaN` out of an
`∞ − ∞`, so `(∞ + 0i) · (3 − 4i)` is `∞ − ∞i`. **A real operand stays real**:
`3.0 * z` is computed componentwise rather than as `(3 + 0i) · z`, which the sign
of a zero can tell apart — what GCC and Clang both do. Both live in
`cinrs::rt::complex`, so an expansion holds a call rather than a copy of the
algorithm, and `tests/complex.rs` checks them against the host's own C compiler
over a hundred and thirty thousand operand pairs.

`<complex.h>` is bundled: `complex`, `I`, `_Complex_I`, C11's `CMPLX` family and
the function declarations, which link against the platform's library and pass
complex values *by value*. `creal`, `cimag`, `conj` and `cproj` are the
compiler's own builtins rather than calls. GNU's `__real__` and `__imag__` are
there and are **lvalues** when their operand is one, so `__imag__ z = 1.0;`
assigns; `~z` is the conjugate; the imaginary suffixes `2.0i`, `1.0if` and
`3.0jl` make a pure imaginary constant; and `_Generic` tells the complex types
apart. What is refused, each with the reason: `_Imaginary`, a complex *integer*
type (`3i` says to write `3.0i`), an `_Atomic` or a bit-field of a complex type,
`<tgmath.h>`, and any `printf` conversion for one, C having none.
`va_arg(ap, double _Complex)` *is* there, read as any other aggregate is.
`__STDC_IEC_559_COMPLEX__` is never defined: Annex G.5.1's arithmetic is
implemented, the rest of the annex is not claimed.

This is the one thing `cinrs` generates that names a crate rather than `core`,
so it lives behind the **`complex` feature**, which is on by default.
`default-features = false` drops the `cinrs-rt` dependency, predefines
`__STDC_NO_COMPLEX__` and makes a complex *value* a diagnostic naming the
feature. A complex type may still be named — a declared-only prototype, a
`typedef`, a pointer, `sizeof`, a `_Generic` association — so a platform
`<complex.h>` or `<tgmath.h>` goes through; an object of the type, a cast to
it, an imaginary literal and a call of a function whose prototype mentions it
are the diagnostic. `cinrs-rt` is itself `#![no_std]`, so having it on costs a `#![no_std]` crate
nothing.
