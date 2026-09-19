# What works

**Contents**

* [Standards and entry points](#standards-and-entry-points)
* [K&R C, implicit int and implicit declarations](#kr-c-implicit-int-and-implicit-declarations)
* [The C99 language](#the-c99-language)
* [Character sets, extended identifiers and Unicode literals](#character-sets-extended-identifiers-and-unicode-literals)
* [Names Rust would not take](#names-rust-would-not-take)
* [Variably modified types and `alloca`](#variably-modified-types-and-alloca)
* [The preprocessor](#the-preprocessor)
* [`#include` and `#embed`](#include-and-embed)
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
declared ones on entry.

`c89!` and `gnu89!` add the two rules C99 deleted: **implicit `int`**
(`static x;`, `f() { … }`) and **implicit function declarations**, where calling
an undeclared `abs` declares `extern int abs();` and the linker resolves it.
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
cannot make at all (into a block, or through a computed `goto`) puts the
function through a state machine over basic blocks instead.

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
[`doc/pragmas.md`](pragmas.md)), and `#line` — which redirects `__LINE__` and
`__FILE__` and nothing else, so a diagnostic still points at the C token that
was really written.

## `#include` and `#embed`

**`#include`, and C23's `#embed`.** Standard headers (`<stdio.h>`,
`<string.h>`, `<math.h>`, `<signal.h>`, `<wchar.h>`, `<uchar.h>`,
`<iso646.h>`, C11's `<stdatomic.h>` and `<threads.h>`, C23's `<stdckdint.h>`
and the rest) are bundled with the crate, written in plain C99 rather than read
from the platform, and the calls link against the real C library. So are five
POSIX ones a small program actually reaches for — `<sys/types.h>`,
`<unistd.h>`, `<fcntl.h>`, `<strings.h>` and `<alloca.h>` — whose types and
constants come from the same target model everything else does. Your own headers
are found next to the `.rs` file that includes them, and editing one rebuilds
the crate.

The platform's *own* headers — `/usr/include` and its like, and so `struct
stat`, `DIR` and `pthread_mutex_t` — are one pragma away; see
[System headers](system-headers.md). `#embed "logo.png"` puts the bytes of a
file into the program — with `limit`, `prefix`, `suffix`, `if_empty` and
`__has_embed` — and editing *that* rebuilds the crate too.

## GNU extensions

Statement expressions (`({ … })`), `typeof`, `__attribute__((packed))` and
`aligned` with the layout GCC gives them, `__attribute__((cleanup(f)))` —
`f(&x)` on every way out of the scope, which is what systemd's `_cleanup_free_`
and glib's `g_autofree` are made of — `#pragma pack`, `case 1 ... 5:`, range
designators, flexible array members (initialised ones included, for an object
with static storage duration), **labels as values** — `&&label` and the computed
`goto *e`, whose value is the state number the label stands for in the machine
such a function is lowered into — `asm` labels, `constructor`/`destructor`,
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

Two gaps: `__builtin_add_overflow` and its relatives compute their check one
width up from the operands, and there is nothing above 128 bits, so a 128-bit
*operand* is refused (a 128-bit *result* is fine); and `va_arg(ap, __int128)`
needs a `VaArgSafe` implementation Rust still keeps unstable, though *passing*
one through `...` works — and a 128-bit *member* of a `struct` is fine, since
`va_arg` of a record is read eightbyte by eightbyte.

## Thread-local objects

`_Thread_local`, C23's `thread_local` and GNU's `__thread` become a
`std::thread_local!` holding an `UnsafeCell`, so a C counter really is one per
thread and `&x` is a pointer to *this* thread's copy. C's own placement rules
apply: file scope, or a block-scope `static`, with a constant initialiser. An
`extern` thread-local object and exporting one under `#pragma cinrs export` are
refused — both would need Rust's unstable `#[thread_local]`.

## C11 threads

`<threads.h>` (7.26) is bundled, and the threads it makes are the C library's
own: `thrd_create`, `mtx_*`, `cnd_*`, `tss_*` and `call_once` are that library's
functions, and `mtx_t` and `cnd_t` are laid out as its `pthread_mutex_t` and
`pthread_cond_t` — which a differential test against the host's `cc` checks. Two
libraries are modelled, glibc (2.28 and later) and musl, both on Linux; on Apple
and Windows, whose runtimes have no such header at all, and on the platforms
whose layouts cinrs does not know, the header is an `#error` naming the reason
and `__STDC_NO_THREADS__` says so.

## Atomics

C11's `_Atomic` — the qualifier and the `_Atomic(T)` specifier — the bundled
`<stdatomic.h>`, GCC's memory-order-aware `__atomic_*` builtins, the older
sequentially consistent `__sync_*` ones and Clang's `__c11_atomic_*`, all of
them on `core::sync::atomic` reached with `AtomicX::from_ptr` over the object's
address (stable since Rust 1.75).

An `_Atomic` object is a plain one of the underlying type whose every read is a
`SeqCst` load, every write a `SeqCst` store, and every `+=`, `++` and `--` a
single read-modify-write, exactly as 6.5.16.2 says; its alignment is its size,
which is what makes `_Atomic long long` eight-byte aligned. The scalars are
covered — the 1-, 2-, 4- and 8-byte integers, `_Bool`, `float` and `double`
(through the integer atomic of the same width and `to_bits`), and object
pointers as an `AtomicPtr`. An `_Atomic` `struct` and a 128-bit one are refused:
neither has a lock-free counterpart, and there is nothing here to be a lock.

The one deliberate difference between the two builtin families is pointer
arithmetic: `__atomic_fetch_add` counts **bytes**, as GCC's does, and
`atomic_fetch_add` from the header counts **elements**, as C11 7.17.7.5
requires.

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
(`goto` included), locals, records and `enum`s by value, bit-fields, and calls
to other safe functions — and Rust calls it as `fact(10)`.

## Input forms

C the Rust lexer accepts is written as raw tokens — `int café(void)` included,
since Rust's identifiers are UAX #31's too; C it refuses (hexadecimal floating
constants, `'ab'`, the prefixed literals `L"…"`, `u8"…"`, `u"…"`, `U"…"` and
`u8'x'`, a universal character name, `\` line continuations, C23's digit
separators such as `1'000'000`) goes in a string literal instead. `##` cannot be
written in raw-token form either, so a replacement list spells the pasting
operator `a # # b`. The third form is a **file**: `include_c99!("parser.c")` and
one such macro per entry point — see [Including a C file](include-c.md).

## Diagnostics

Every generated token carries the span of the C token it came from, so both
`cargo` and an IDE put the caret on the C code — for the front end's own
diagnostics and for `rustc`'s. Pointing *inside* a string literal needs
`Literal::subspan`, which is unstable, so there the position is appended to the
message instead; the crate's `nightly` feature turns it back into a caret.

## Variadic functions

Declaring and calling one works anywhere; *defining* one needs Rust 1.99's
`c_variadic`, and is a clear error before that.

## Complex numbers

`float _Complex` and `double _Complex` are `cinrs::rt::Complex<f32>` and
`Complex<f64>` — which is
[`num_complex::Complex`](https://docs.rs/num-complex), the type the numeric half
of crates.io already speaks, so a complex value crosses the boundary without a
conversion. `long double _Complex` is `double _Complex`, the same mapping
`long double` has and the same ABI caveat. The arithmetic is C's, Annex G.5.1's
infinity recovery included, and is checked against the host's own C compiler over
a hundred and thirty thousand operand pairs. GNU's `__real__`, `__imag__`, `~z`
and the `2.0i` suffix are there, and so are `<complex.h>`, `CMPLX` and
`_Generic` over the complex types.

This is the one thing `cinrs` generates that names a crate rather than `core`,
so it lives behind the **`complex` feature**, which is on by default.
`default-features = false` drops the `cinrs-rt` dependency, predefines
`__STDC_NO_COMPLEX__` and makes `_Complex` a diagnostic naming the feature.
`cinrs-rt` is itself `#![no_std]`, so having it on costs a `#![no_std]` crate
nothing.
