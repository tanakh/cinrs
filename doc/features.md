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
irreducible, a cycle with two heads, and a computed `goto` is the one thing
still lowered as a state machine over block numbers.

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
four `__STDC_NO_*` subsetting macros, `__cinrs__`, and `__FILE__` and `__LINE__`
— which name the **`.rs` file** and the line in it, so that they point where the
user is looking. `__DATE__`, `__TIME__` and `__TIMESTAMP__` are fixed
placeholders, because a build has to give the same output twice. On top of those
comes the GCC family and the target description macros (`__GNUC__`,
`__STRICT_ANSI__`, `__x86_64__`, `__linux__`, `__LP64__`, `__SIZEOF_INT__`,
`__BYTE_ORDER__`, the limits and the library types), every one of them read off
the [target model](c-status.md#the-target-model) rather than the host, and
nothing claims to be Clang; the catalogue is
[`doc/gnu-extensions.md`](gnu-extensions.md#preprocessor-extensions).
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

One header that is not C's is bundled beside them: `<alloca.h>`, because
`alloca` is generated by this crate rather than called in any library, so there
is nothing for the platform's copy to add.

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
`goto *e`, whose value is the state number the label stands for in the state
machine such a function is lowered into, which is the one lowering that still
needs one — `asm` labels, `constructor`/`destructor`,
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
and the features of later revisions, need `gnu99!`. Inline *assembly* is a clear
error rather than a guess, and so is every other extension with no honest
translation. [`doc/gnu-extensions.md`](gnu-extensions.md) is the catalogue, row
by row.

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
`__STDC_NO_COMPLEX__` and makes `_Complex` a diagnostic naming the feature.
`cinrs-rt` is itself `#![no_std]`, so having it on costs a `#![no_std]` crate
nothing.
