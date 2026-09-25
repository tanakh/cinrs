# What the C becomes

A reference for the other side of the boundary: what a construct is translated
into, and what Rust code that reads or calls it has to know. [What
works](features.md) is the tour of the language; this is the shape of the
output.

Two conventions in the snippets below. The unit's module opens with an
`#![allow(…)]` list (`non_camel_case_types`, `unused_parens`, `clippy::all`
and two dozen more — C is not idiomatic Rust) that every item inside it
inherits; it is shown once, under [The shape of an
expansion](#the-shape-of-an-expansion), and left out of every snippet after
that. So is the `::core::` prefix the expansion writes in full: `c_int` below
is `::core::ffi::c_int`.

**Contents**

* [The shape of an expansion](#the-shape-of-an-expansion)
* [Functions](#functions)
* [Objects](#objects)
* [Types](#types)
* [Arithmetic](#arithmetic)
* [Functions declared without a prototype](#functions-declared-without-a-prototype)
* [Bit-fields](#bit-fields)
* [Compound literals](#compound-literals)
* [Over-aligned objects](#over-aligned-objects)
* [An initialised flexible array member](#an-initialised-flexible-array-member)
* [Variably modified types and `alloca`](#variably-modified-types-and-alloca)
* [Thread-local objects](#thread-local-objects)
* [Control flow and `goto`](#control-flow-and-goto)
* [Labels as values](#labels-as-values)
* [One block, one module](#one-block-one-module)
* [Linking two blocks together](#linking-two-blocks-together)

## The shape of an expansion

```rust
mod __cinrs_unit_ab49b1a7 {
    #![allow(unknown_lints, non_camel_case_types, unused_parens, /* … */ clippy::all)]
    const _: () = { /* the data-model assertions */ };
    // the unit's items
}
#[allow(unknown_lints, ambiguous_glob_reexports, unused_imports)]
pub use __cinrs_unit_ab49b1a7::*;
```

A private module named after a hash of the unit, and a glob re-export of it —
see [One block, one module](#one-block-one-module). The `const _` block states
the [target model](cross-compilation.md#the-assertion-that-guards-it) the unit
was translated for, so a wrong model is a compile error rather than a wrong
`sizeof`.

The lint exemptions are an *inner* attribute on the module and therefore
written once, however many items the unit has: lint levels are inherited, so
one list covers everything inside — `rustc`'s lints and clippy's alike — and a
crate that denies warnings at its root sees nothing from the generated code.
Written once rather than on each item, it costs nothing to speak of: for
`#include <zlib.h>`, which generates 419 items, the list on every item would be
83% of the expansion.

## Functions

```c
int add(int a, int b) { return a + b; }
static double half(double x) { return x / 2; }
```

```rust
pub unsafe extern "C" fn add(mut a: c_int, mut b: c_int) -> c_int {
    unsafe { return a.wrapping_add(b); }
}
unsafe extern "C" fn half(mut x: c_double) -> c_double {
    unsafe { return x / 2.0; }
}
```

The same name, the [`core::ffi`] types (`int` is `c_int`, `double` is
`c_double`, `_Bool` is `bool`), and a whole-body `unsafe` block. A C `static`
function is generated without `pub`, which is the internal linkage C gives it.
Every parameter is `mut`, because C lets a function assign to one.

A function the unit marks [safe](features.md#safe-functions) is
`pub extern "C" fn` with no `unsafe` block, and Rust calls it as `add(1, 2)`.

A function the unit only **declares** is linked rather than defined, and the
declaration *is* the item — under the C name, `pub`, and pointed at its symbol:

```c
int printf(const char *, ...);
```

```rust
unsafe extern "C" {
    #[link_name = "printf"]
    pub fn printf(_: *const c_char, ...) -> c_int;
}
```

So the glob re-export carries it out of the unit's module and Rust calls it as
`printf(…)`: `#include <zlib.h>` is the whole binding, and [Calling a C library
from Rust](features.md#calling-a-c-library-from-rust) is the tour of what that
is like. The name goes through the same mapping every other C name does — a
keyword is a raw identifier (`int yield(int);` is `r#yield`), and `$` and the
five unspellable names get their usual spellings — so one C name is one Rust
name here too, and `#[link_name]` is what carries the symbol whenever the two
differ.

`#pragma cinrs link "name"` adds `#[link(name = "name")] unsafe extern "C" {}`
beside that block — a block of its own, with nothing in it, because the attribute
would otherwise make `rustc` reach every `static` in the block it sits on through
a `dllimport` on a Windows target (see [Where a `#[link]`
goes](cross-compilation.md#where-a-link-goes-and-why-a-block-of-its-own)). cinrs
adds one itself in a single case: a unit translated for a `*-windows-msvc` target
whose block declares one of the `printf` or `scanf` family gets
`#[link(name = "legacy_stdio_definitions")]`, because the Universal CRT defines
those functions inline in `<stdio.h>` and exports no symbol for them (see
[Cross-compilation](cross-compilation.md#the-microsoft-librarys-inline-printf)).
The same page has the `<time.h>` names the Microsoft runtime exports under
another spelling: `time` is still `pub fn time`, with
`#[link_name = "_time64"]`.

A C program is free to name a local or a parameter after a function it declared
(`int index`, `long time`, `char *basename`), and so is Rust: a function is not
a pattern, so the binding shadows the item rather than colliding with it.

## Objects

```c
int counter = 1;
char buf[4];
```

```rust
pub static mut counter: c_int = 1;
pub static mut buf: [c_char; 4] = [0; 4];
```

A file-scope object is a `static mut`, so **Rust reads it by value**:
`assert_eq!({ counter }, 1)` rather than `assert_eq!(counter, 1)`, since taking
a reference to a `static mut` is an error in edition 2024. A C `static` object,
like a `static` function, loses the `pub`.

An object the unit only **declares** is the one thing that does *not* come out
under its C name:

```c
extern FILE *stdout;
```

```rust
unsafe extern "C" {
    #[link_name = "stdout"]
    pub static mut __cinrs_ab49b1a7_stdout: *mut FILE;
}
```

The reason is Rust's rule for patterns rather than a matter of taste. A
glob-imported *function* cannot change the meaning of Rust code that does not
mention it, because a function is not a pattern: `let read = 1;` beside a unit
that declares `read` is still a new binding. A glob-imported `static` can — Rust
resolves a binding pattern against the value namespace first, and a `static`
there is not a name a `let` may shadow:

```text
error[E0530]: let bindings cannot shadow statics
```

which is what `let stdout = std::io::stdout();` would become next to a block
that includes `<stdio.h>`. glibc's `<time.h>` declares `timezone` and
`daylight`, `<unistd.h>` declares `optarg` and `optind`, and `environ` is there
too; a hidden name keeps all of them out of the way of ordinary Rust.

The C in the unit is unaffected either way — it reads the object by its C name —
and Rust reaches such an object the way one C translation unit hands another a
`FILE *`, through an accessor written in the block:

```c
FILE *get_stdout(void) { return stdout; }
```

## Types

| C | Rust |
| --- | --- |
| `T *`, `const T *`, `void *` | `*mut T`, `*const T`, `*mut c_void` — raw pointers, and pointer arithmetic goes through `offset`, so nothing generated holds a reference |
| `T a[N]` | `[T; N]` |
| `struct S`, `union U` | `#[repr(C)] #[derive(Copy, Clone)]` items with `pub` members |
| `enum E` | `pub type E = c_int;` plus one `pub const` per enumerator |
| `int (*)(void *)` | `Option<unsafe extern "C" fn(*mut c_void) -> c_int>`, so that a null function pointer is representable |
| `__m128`, `__m128i`, `__m256d`, … | `::core::arch::x86_64::__m128` and its relatives — `core`'s own types, sixteen or thirty-two bytes and aligned to themselves, which is what lets a `union` punne one |

```c
struct Point { int x; int y; };
int manhattan(struct Point p);
```

```rust
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Point { pub x: c_int, pub y: c_int }
```

All of them are ordinary Rust items, so Rust code builds a `struct` the C takes
(`manhattan(Point { x: 3, y: -4 })`), reads the members the C sets, and passes
one of its own `extern "C" fn`s where the C wants a callback. A call *through* a
function pointer is `(f.expect("null function pointer"))(a)`, which is where a
null one panics.

An **anonymous** `struct` or `union` member (C11) becomes a field
`__cinrs_anon0`, `__cinrs_anon1`, … of a generated type of its own, so Rust
builds such a record as `Value { tag: 1, __cinrs_anon0: … }` while the C still
writes `v.as_int`.

For the C names Rust cannot spell — a keyword, `self`, `$` — see [Names Rust
would not take](features.md#names-rust-would-not-take). One C name is one Rust
name everywhere it appears: as an item, as a member, in a designator and in a
bit-field accessor.

## SIMD intrinsics

A call to one of the Intel intrinsics is **not** a call to a symbol: there is no
`_mm_add_ps` anywhere to link against. It becomes the `core::arch` function of
the same name, which is where the instruction really is:

```c
#include <immintrin.h>

int sum4(const int *p) {
    __m128i v = _mm_loadu_si128((const __m128i *) p);
    __m128i s = _mm_add_epi32(v, _mm_shuffle_epi32(v, _MM_SHUFFLE(1, 0, 3, 2)));
    return _mm_cvtsi128_si32(_mm_slli_epi32(s, 3));
}

__attribute__((target("avx2,fma"))) float wide(const float *p) { … }
```

```rust
pub type __m128i = ::core::arch::x86_64::__m128i;      // and its five relatives

pub unsafe extern "C" fn sum4(mut p: *const c_int) -> c_int {
    unsafe {
        let mut v: ::core::arch::x86_64::__m128i =
            ::core::arch::x86_64::_mm_loadu_si128(p as *const ::core::arch::x86_64::__m128i);
        let mut s: ::core::arch::x86_64::__m128i = ::core::arch::x86_64::_mm_add_epi32(
            v,
            ::core::arch::x86_64::_mm_shuffle_epi32::<{ 78i32 }>(v),
        );
        return ::core::arch::x86_64::_mm_cvtsi128_si32(
            ::core::arch::x86_64::_mm_slli_epi32::<{ 3i32 }>(s),
        );
    }
}

#[target_feature(enable = "avx2")]
#[target_feature(enable = "fma")]
pub unsafe extern "C" fn wide(mut p: *const c_float) -> c_float { … }
```

Three things to read out of that.

The **turbofish**. `_MM_SHUFFLE(1, 0, 3, 2)` folded to 78 and
`_mm_slli_epi32(s, 3)`'s `3` moved out of the argument list, because those
operands are immediates of the instruction and `core::arch` carries them as
`const` generics. Sema folds each one and diagnoses a non-constant; the literal
is written with the Rust type of the `const` parameter, in a block, so that a
negative or unsigned one is still a const argument Rust parses.

The **`#[target_feature]`**, one attribute per instruction set the `target`
attribute asked for, in LLVM's spelling rather than GCC's. Because the body of
every generated function is one `unsafe` block, an intrinsic can be called
without it: an `unsafe` block satisfies Rust's rule on its own, and the
attribute is what a function that means to be *given* the instruction set says.

And what is **not** there: no `extern "C"` block for the intrinsics.
`<immintrin.h>` declared six thousand and seventy-five of them in this unit and
not one produced an item — no `pub fn _mm_add_ps`, nothing in the glob
re-export, nothing to link. The `pub type __m128i = …` aliases are there because
the header wrote `typedef`s, exactly as it does in C; so are `<stdlib.h>`'s
declarations and `<mm_malloc.h>`'s two `static inline` functions, which
`<xmmintrin.h>` includes as GCC's does.

GCC's vector operators are the same call: `a * b + c * 2.0` on `__m128d` is
written `_mm_add_pd(_mm_mul_pd(a, b), _mm_mul_pd(c, _mm_set1_pd(2.0)))`, `v[i]`
is `*((double *)&v + i)`, and `(__m128d){a, b}` is `_mm_setr_pd(a, b)` —
sema lowers each to the intrinsic the header declares, so nothing about them is
new to code generation.

The one intrinsic that produces an item is one whose **address** is taken, which
GCC allows because its intrinsics are `static inline` functions. `core::arch`'s
have the Rust ABI, so the unit gets a private shim of the C signature:

```rust
#[inline]
#[target_feature(enable = "sse2")]
unsafe extern "C" fn __cinrs_intrinsic__mm_add_epi32(
    __cinrs_arg0: ::core::arch::x86_64::__m128i,
    __cinrs_arg1: ::core::arch::x86_64::__m128i,
) -> ::core::arch::x86_64::__m128i {
    unsafe { ::core::arch::x86_64::_mm_add_epi32(__cinrs_arg0, __cinrs_arg1) }
}
```

and `&_mm_add_epi32` is `Some(__cinrs_intrinsic__mm_add_epi32 as unsafe extern
"C" fn(…) -> …)`. One shim per intrinsic however many times its address is
taken, private to the unit's module, and hygienic — nothing the C declares can
name it. An intrinsic with an immediate operand has no address at all, because a
function pointer has nowhere to put the constant, and that is a diagnostic.

`__builtin_cpu_supports("avx2")` becomes
`((::std::is_x86_feature_detected!("avx2")) as c_int)`, and
`__builtin_cpu_init()` becomes `{}`.

On a 32-bit x86 target every path above reads `::core::arch::x86` instead; the
names in it are the same.

## Inline assembly

An `asm` statement becomes one `::core::arch::asm!`, with the constraints
already mapped by sema (the table is in [What
works](features.md#inline-assembly)); what code generation adds is evaluating
the operands and putting the outputs where C said:

```c
struct P { int x; long y; } __attribute__((packed));

void f(int *a, int i, struct P *q) {
    asm("incl %0" : "+r"(a[i++]));
    asm("movl $7, %0" : "=r"(*&q->x));
}
```

```rust
{
    let __cinrs_tmp1 = a.offset(({ let __cinrs_tmp0 = i; i = i.wrapping_add(1); __cinrs_tmp0 }) as isize);
    ::core::arch::asm!("incl {o0:e}", o0 = inout(reg) (*__cinrs_tmp1), options(att_syntax));
}
{
    let __cinrs_tmp2 = (&raw mut (*q).x);
    let __cinrs_asm0: ::core::ffi::c_int;
    ::core::arch::asm!("movl $7, {o0:e}", o0 = lateout(reg) __cinrs_asm0, options(att_syntax));
    (&raw mut (*__cinrs_tmp2)).write_unaligned(__cinrs_asm0);
}
```

An output is **written straight into its place**. A place is lowered into a
setup, which runs once before the statement — the `i++` above — and an access
that may be evaluated more than once, and the access is an ordinary Rust place
expression `asm!` can write to: a local, `(*p).f`, an element. The one
exception is a place that may be underaligned — a dereference of a pointer
built out of a packed member, like `*&q->x` — which `asm!` would store with an
aligned instruction. That output goes through a typed temporary, stored back
with `write_unaligned` after the statement, and for a `"+r"` operand read with
`read_unaligned` before it. (A member of a packed record written directly,
`q->x`, is a place Rust already stores unaligned, and needs no temporary.)

Three things in the **template** are there because `asm!` and GCC print
operands differently:

* a bare `%0` on a `reg` operand carries the operand's **width** — `{o0:e}`
  for a 32-bit value, `{o0:x}` for 16 — because `asm!` prints the whole 64-bit
  register unless told otherwise, and GCC prints the register the value is in;
* an `"i"` operand is a `const`, which `asm!` substitutes as a bare number, so
  AT&T's `$` is written in front of it: `${o1}`, with `o1 = const 3i64`;
* an operand the template never names is mentioned in a comment at its end,
  `/* {o1:e} */`, because `asm!` refuses a named operand nobody uses.

Inputs are written with their type (`5 as c_int`), never as a bare literal,
because an `asm!` operand has no expected type and an unsuffixed literal would
be an `i32` whatever the C said. Register clobbers follow the operands as
`out("rcx") _`, and `options(att_syntax)` ends every statement.

## Arithmetic

`+`, `-`, `*` and the shifts **wrap** rather than panic: unsigned wrap-around
is defined in C, and wrapping is the predictable choice for the signed overflow
C leaves undefined. `/` and `%` are Rust's, which truncate towards zero and
take the sign of the dividend, exactly as C99 says — so what C leaves undefined
there (a zero divisor, `INT_MIN / -1`) is a Rust panic, and a panic cannot
cross an `extern "C"` frame, so the program aborts.

## Functions declared without a prototype

Before C23, `int f();` says nothing about the parameters (C99 6.7.5.3p14): a
call may pass any number of arguments, each gets the *default argument
promotions*, and the callee is invoked as though its prototype were made of
those promoted types (6.5.2.2p6). The generated Rust type is therefore
`unsafe extern "C" fn() -> R`, with no parameters — that is all the declaration
said — and each call site with at least one argument writes the
reinterpretation out:

```c
int scale();
int nine(void) { short three = 3; return scale(three); }
```

```rust
return (::core::mem::transmute::<
    unsafe extern "C" fn() -> c_int,
    unsafe extern "C" fn(c_int) -> c_int,
>(__cinrs_ab49b1a7_scale as unsafe extern "C" fn() -> c_int))(three as c_int);
```

(through a function pointer the `Option` comes off first; a call with no
arguments needs no cast at all). That is exactly the contract C's own ABI
relies on: the program is defined only if the function really does take
parameters of those types.

A *definition* written `int f() { … }` takes no parameters, as 6.9.1p7 says,
and the generated item has none. An **old-style (K&R) definition** has no
prototype either, so its item takes the *promoted* types and converts on entry:
`int f(c, n) char c; int n;` becomes `fn f(c: c_int, n: c_int)` with
`let c: c_char = c as c_char;` at the top, which is what C says the callee
receives.

`c23!` and `gnu23!` follow N2841 instead: there `int f()` is `int f(void)`.

## Bit-fields

A bit-field has no address of its own, so it cannot be a field of the generated
`#[repr(C)]` item. A maximal run of consecutive bit-fields becomes one
`pub __cinrs_bitsN: [u8; K]` covering the bytes the run occupies, with explicit
`pub __cinrs_padN: [u8; M]` wherever `#[repr(C)]` would otherwise place the next
member too early, and `#[repr(C, align(N))]` where the fields' own type made the
record stricter than any field of it.

```c
struct Flags {
    unsigned int ready : 1;
    int          level : 3;
    unsigned int       : 0;   /* start the next field on a new unit */
    unsigned int mask  : 30;
};
```

```rust
#[repr(C, align(4))]
#[derive(Copy, Clone)]
pub struct Flags { pub __cinrs_bits0: [u8; 8] }

impl Flags {
    #[inline]
    pub fn level(&self) -> c_int {
        let raw: u64 = self.__cinrs_bits0[0] as u64;
        let value: u64 = (raw >> 1) & 0x7;
        (((value << 61) as i64) >> 61) as c_int   // sign-extended
    }
    #[inline]
    pub fn set_level(&mut self, value: c_int) { /* read, mask, write back */ }
    // … and a pair for `ready` and for `mask`
}
```

Each **named** member is a pair of inherent methods in plain inline integer
code: the getter is the member's own name and the setter is `set_` in front of
it, both taking and returning the member's declared C type — so an `enum` field
reads as the `enum`'s alias and a `_Bool` field as a `bool`. So Rust builds the
record as `Flags { __cinrs_bits0: [0; 8] }` and then uses `f.level()` and
`f.set_level(-3)`. Storing a value too wide for the field keeps the low bits and
reads back sign-extended, as C does.

A member whose name is a Rust keyword becomes a raw identifier (`n.r#match()`),
one Rust cannot spell at all takes the underscore [Names Rust would not
take](features.md#names-rust-would-not-take) describes (`n.self_()`,
`n.set_self()`), and where two names would collide — a member `x` next to a
member `set_x` — every getter is claimed first, in declaration order, so a
member's own name always reads it and the setter that finds its name taken grows
`_2`, `_3`, …

The accessors take `&self` and `&mut self`, so a bit-field of a **file-scope**
object is reached through a raw pointer, exactly as the generated code does it:
`unsafe { (*(&raw mut STATE)).set_ready(1) }`. The accessors generated for the
bit-fields of a **`union`** read the storage inside an `unsafe` block of their
own, so they are safe methods — which is why a [safe
function](features.md#safe-functions) may read one where reading an ordinary
member of that `union` would be refused.

Inside the C nothing changes: `s.level = 3`, `p->flags |= 1`,
`switch (s.kind)`, `++s.count`, a designated initialiser and a compound literal
all work as they do for an ordinary member, and `sizeof` and `offsetof` see the
layout GCC and Clang give the record. The integer promotions follow C99
6.3.1.1p2's width-restricted rule — `unsigned x : 31` takes part in arithmetic
as an `int`, `unsigned x : 32` as an `unsigned int`. Taking the address of a
bit-field, `sizeof` of one and `offsetof` of one are the three things C forbids,
and each is a located error. Which types a bit-field may have, and what the
layout commits to, is
[`doc/c-status.md`](c-status.md#c99).

## Compound literals

`(T){ … }` is an *object*, not a value, and C gives one written inside a block
the lifetime of that block — so `&(struct S){1, 2}` is still good after the
statement that made it. It becomes a hidden binding at the top of the block,
with the value stored into it where the literal was written:

```c
int sum(void) {
    struct S *p = &(struct S){ 1, 2 };
    return p->a + p->b + (int[]){ 10, 20, 30 }[2];
}
```

```rust
let mut __cinrs_3bbb7421_literal0: S = ::core::mem::zeroed::<S>();
let mut __cinrs_3bbb7421_literal1: [c_int; 3] = [0; 3];
let mut p: *mut S = { __cinrs_3bbb7421_literal0 = S { a: 1, b: 2 };
                      (&raw mut __cinrs_3bbb7421_literal0) };
// … (&raw mut __cinrs_3bbb7421_literal1).cast::<c_int>().offset(2)
```

So side effects in the initialiser happen in C's order, and a literal inside a
loop is a fresh object on every iteration. At file scope the object has static
storage duration instead and becomes a `static mut` item of its own, whose
initialiser has to be a constant expression like any other.

## Over-aligned objects

`_Alignas(N)` and `__attribute__((aligned(N)))` are honoured on an *object* —
automatic, `static`, at file scope or `_Thread_local` — as well as on a type or
a member. Rust can over-align a *type* and nothing else, so the object is
generated inside a one-field wrapper that carries the alignment, one per
distinct alignment in the unit:

```c
_Alignas(64) char buf[256];
```

```rust
#[repr(C, align(64))]
#[derive(Copy, Clone)]
pub struct __cinrs_align_64<T>(pub T);

pub static mut buf: __cinrs_align_64<[c_char; 256]> = __cinrs_align_64([0; 256]);
```

Every use of the object in the generated code goes through the field — `&buf` is
`&raw mut buf.0` — and **Rust code that reaches such an object reads `buf.0`**.
Nothing about the C program changes: `sizeof buf` is the array's size, the
wrapper is invisible to it, and `_Alignof buf` answers what the declaration
asked for.

The strictest of several specifiers wins, `_Alignas(T)` asks for a type's
alignment, and bare `aligned` for the target's `max_align_t`. An `_Alignas`
*weaker* than the type's own alignment is the constraint violation C11 6.7.5p4
makes it; GCC's `aligned` "can only increase alignment", so a weaker one there
is quietly not applied. The five declarations C11 6.7.5p2 forbids the specifier
in — a `typedef`, a bit-field, a function, a parameter and an object declared
`register` — are each refused where they are written, and so is a variable
length array, whose storage is allocated at run time.

## An initialised flexible array member

C99 forbids it, because the object would have to be larger than its type. GNU C
allows it for an object with **static storage duration**, whose storage the
compiler can make that large, and so does this: the item is given a *companion*
type with the record's leading layout and a tail as long as the initialiser, and
the C object is the record at its address.

```c
struct W { int n; int data[]; };
struct W w = { 3, { 1, 2, 3 } };
int last(void) { return w.data[w.n - 1]; }
```

```rust
#[repr(C)] #[derive(Copy, Clone)]
pub struct W { pub n: c_int, pub data: [c_int; 0] }
#[repr(C)] #[derive(Copy, Clone)]
pub struct __cinrs_W_3 { pub n: c_int, pub data: [c_int; 3] }

pub static mut w: __cinrs_W_3 = __cinrs_W_3 { n: 3, data: [1, 2, 3] };
// every use of `w` in the C is  (*(&raw mut w).cast::<W>())
```

`sizeof w` is still `sizeof(struct W)`, which is what GCC says too, and
[`#pragma cinrs export`](pragmas.md#export) exports the storage — the full size
— under the C name. **Rust code reads the companion**, whose fields are the
record's with the tail sized by the initialiser:
`(&raw const w).read().data == [1, 2, 3]`.

An **automatic** object cannot be made larger than its type, and neither can a
record nested inside another aggregate — an array of them, a member, or a
compound literal — whose own size already fixes the room there is. Both are
refused, in GCC's own words.

## Variably modified types and `alloca`

**The storage is the heap, not the stack.** Rust has no way to move the stack
pointer by an amount chosen at run time, so a function that declares a variable
length array or calls `alloca` opens with a stack of its own: a bump arena on
the heap, dropped by the `return`. The elements are bumped off it, a hidden
frame guard whose `Drop` at the end of the block — the object's lifetime —
moves the arena back down to where it was, and the C object itself is a pointer
to the first element. The model is one hidden `size_t` object per variable
dimension, created where the *type* is declared and named after it:

```c
unsigned long sizes(int n, int m) {
    double a[n][m];
    a[1][2] = 1.0;
    return sizeof a + sizeof a[0];
}
```

```rust
let __cinrs_vla = __cinrs_vla_arena::new();          // the function's first line
let mut __cinrs_vla_len_a: c_ulong = m as c_ulong;
let mut __cinrs_vla_len1_a: c_ulong = n as c_ulong;
let __cinrs_vla_frame_a = __cinrs_vla.frame();
let mut a: *mut c_double =
    __cinrs_vla.alloc::<c_double>(__cinrs_vla_len_a.wrapping_mul(__cinrs_vla_len1_a) as usize);
// a[1][2]      is  a.offset(1 * __cinrs_vla_len_a).offset(2)
// sizeof a     is  __cinrs_vla_len_a * __cinrs_vla_len1_a * 8
// sizeof a[0]  is  __cinrs_vla_len_a * 8
```

Everything else is arithmetic over those objects: `a[i]` decays to a
`double (*)[m]` — the same pointer with a different C type — `p + 1` moves by a
whole row, and `sizeof a`, `sizeof a[0]` and `sizeof *p` are products of the
bounds. Every observation a C program can make is the one C promises; see [What
works](features.md#variably-modified-types-and-alloca) for that half. What
changes is where the bytes are, and that a very large one fails the way a
`malloc` does rather than by running the stack out. A zero length takes no
bytes, as it does in GCC, and still has an address; a *negative* one is
undefined behaviour in C, and here it converts to a huge `size_t` and the
allocation panics or aborts rather than corrupting anything. The elements are
zeroed: C leaves them indeterminate, but reading uninitialised memory through a
raw pointer is undefined in Rust too.

The arena is a type of the unit's own module, `__cinrs_vla_arena`, emitted
once for a unit that needs it. Its storage is a list of chunks that never move
— `Vec<u128>`s, so each starts 16-byte aligned — and a position in them. No
chunk is allocated until the first array is, and what an allocation costs from
then on is a bump and a `memset`: a declaration in a loop takes back the place
the previous iteration gave up. Only when the current chunk is full is there a
call to the allocator, for a new chunk at least twice as large — or the next
one along, if an earlier pass left one big enough. A recursive call has an
arena of its own.

A variable length array asked to be over-aligned —
`double v[n] __attribute__((aligned(32)))` — pads the arena's position up to the
next multiple of the alignment *by address*, so any alignment works in a
16-byte-aligned chunk: `__cinrs_vla.alloc_aligned::<c_double>(n as usize, 32)`.

`alloca(n)` bumps 16-byte aligned bytes off the same arena —
`__cinrs_vla.alloca(n as usize)` — and raises a *floor* past them that no frame
moves the arena below, so they are not given back at the end of a block that
also had an array in it: they live until the `return` frees the whole arena.
That is GCC's own rule — a variable length array's space is freed at the end of
its scope "unless you also use `alloca` in this scope" — and `alloca`'s own
lifetime, so a pointer to it returned to the caller dangles here exactly as it
does in C.

In a function lowered through the [control-flow graph](#control-flow-and-goto),
where every local is hoisted to the top, there are no blocks for a frame to be
dropped at. The function keeps one arena mark per array instead, in
`__cinrs_vla_marks`, and the declaration becomes
`__cinrs_vla.redefine(&mut __cinrs_vla_marks, 0); a = __cinrs_vla.alloc::<…>(…);`.
Reaching the declaration again moves the arena back down to where it was the
last time — which also gives back every array declared after it on that path,
since jumping back over this declaration ended their lifetimes too — before
allocating afresh, so a backward `goto` over a declaration reuses its space.
An array a `goto` left the scope of is given back the next time its
declaration is reached, or when the function returns, rather than when the
block ends; a C program can only observe that as memory it expected to have
been given back sooner, and there is never more of it than one array per
declaration.

The chunks are `::std::vec::Vec`s unless the unit writes
[`#pragma cinrs no_std`](no-std.md), which makes them `::alloc::vec::Vec`s.

## Thread-local objects

```c
_Thread_local int counter;
int bump(int by) { counter += by; return counter; }
```

```rust
::std::thread_local! {
    pub static counter: ::core::cell::UnsafeCell<c_int> =
        const { ::core::cell::UnsafeCell::new(0) };
}
pub unsafe extern "C" fn bump(mut by: c_int) -> c_int {
    unsafe {
        let __cinrs_tmp0 = counter.with(|c| ::core::cell::UnsafeCell::get(c));
        (*__cinrs_tmp0) = (*__cinrs_tmp0).wrapping_add(by);
        // …
    }
}
```

Every C access goes through the `*mut T` the cell hands out, which is valid for
as long as *this thread's* copy of the object is — precisely what C promises
about the address of one, so `&counter` is that pointer and nothing about the
object model changes. **Rust reads it through the generated item**, which is
`pub` when the C object has external linkage:
`counter.with(|cell| unsafe { *cell.get() })`.

The initialiser goes inside `thread_local!`'s `const { … }` block wherever Rust
allows it — the cheap form, with no lazy-initialisation flag. One thing keeps it
out: an initialiser whose value is the address of another item, such as
`_Thread_local int *p = &global;`, since a Rust constant may not refer to a
`static`. Such an object takes the lazy form instead, which is a difference in
when the initialiser runs and in nothing a C program can observe. The rest —
where C allows the specifier, and what is refused — is [What
works](features.md#thread-local-objects).

## Control flow and `goto`

`if`, `while`, `do`/`while`, `for`, `break`, `continue` and `switch` — with
fallthrough — become Rust's own control flow, so the expansion reads like the C
it came from. Most `goto`s do too.

A jump **forwards**, to a label later in a block it is inside, is a `break` out
of a labelled block that ends where the label stands:

```c
    for (int i = 0; i < n; i++)
        if (v[i] == t) { found = i; goto done; }
done:
    return found;
```

```rust
'done: {
    let mut i: c_int = 0;
    'l0: loop {
        if !(i < n) { break 'l0; }
        'l0_body: { if (*v.offset(i as isize)) == t { found = i; break 'done; } }
        i = i.wrapping_add(1);
    }
}
return found;
```

A jump **backwards**, to a label its block begins with, is a `continue` of a
labelled loop that starts there:

```c
retry:
    tries++;
    if (tries < n) goto retry;
```

```rust
'retry: loop {
    tries = tries.wrapping_add(1);
    if tries < n { continue 'retry; }
    break 'retry;
}
```

The blocks are named after the C labels, so `goto done` reads as `break 'done`
and `goto retry` as `continue 'retry`, and they nest. A scope left on the way
out is left in full: the `break` runs the drops of everything the blocks it
leaves hold, which is what frees a [variable length
array](#variably-modified-types-and-alloca) and runs a
[`cleanup`](gnu-extensions.md#__attribute__-forms) attribute exactly where C says
they run.

Five things do not fit those two shapes: a jump **into** a block (a label inside
a loop body, an `if` branch or a `switch` group, named from outside it), a
computed `goto`, a `case` label that is not a direct child of its `switch` body
(Duff's device), two labels whose regions would have to overlap without nesting,
and a declaration between a jump and the label it names, which no Rust block may
hold without ending its scope early. Such a function is lowered into a
**control-flow graph** of basic blocks, with every local hoisted to the top and
renamed apart — and the graph is then read back into Rust's own loops and
branches. There are three tiers in all.

### Tier 2: the graph, relooped

Most of what reaches the graph is *reducible*: every cycle in it has one head.
That is exactly the shape Rust's `loop`, `break` and `continue` describe, and
[the relooper](https://github.com/tanakh/cinrs/blob/master/crates/cinrs-core/src/reloop.rs)
— Emscripten's algorithm — recovers it. A `switch` whose cases jump to a label
inside one of them, which is what `sqlite3VdbeExec` and every other interpreter
loop is made of, comes out as a `match` inside a `loop`:

```c
for (;;) {
    switch (ops[i]) {
    case 1: rc += 1; break;
    case 2: if (n < 0) goto fail; rc += 2; break;
    case 3:
        rc = 3;
    fail:
        rc = -rc;
        goto done;
    }
    i++;
}
done:
```

```rust
'r0: loop {
    match (*ops.offset(i as isize)) {
        1 => { rc = rc.wrapping_add(1); }
        2 => { if n < 0 { break 'r0; } rc = rc.wrapping_add(2); }
        3 => { rc = 3; break 'r0; }
        _ => {}
    }
    i = i.wrapping_add(1);
}
rc = rc.wrapping_neg();
return rc;
```

There is no state variable at all: a loop is a Rust loop, a jump forwards is
`break` of a labelled block that ends where its target begins, and a jump
backwards is `continue`. Loops are named after the C label their head stands at
(`'retry`, `'start`) and `'rN` where there is none; the labelled blocks the
forward jumps break out of are `'bN`. What LLVM sees is the graph GCC sees.

### Tier 3: a state variable, for an irreducible region only

A `goto` into the middle of a loop, Duff's device and two loops that jump into
each other's bodies all make a cycle with **two heads**, and no arrangement of
Rust's blocks can enter one twice. The relooper keeps both heads and puts a
`match` on a state variable at the top of the loop; every jump to a head writes
it first:

```c
int into_block(int n, int inside) {
    int total = 0;
    if (inside) goto mid;           /* into the loop body */
    while (n > 0) { n--; mid: total += n; }
    return total;
}
```

```rust
let mut total: c_int = 0;
let mut __cinrs_entry0: u32 = 0;
total = 0;
if inside != 0 { __cinrs_entry0 = 1; } else { __cinrs_entry0 = 0; }
'mid: loop {
    if __cinrs_entry0 == 0 {
        if !(n > 0) { break 'mid; }
        n = n.wrapping_sub(1);
        __cinrs_entry0 = 1;
    } else {
        total = total.wrapping_add(n);
        __cinrs_entry0 = 0;
    }
}
return total;
```

Two heads is an `if`; more than two — Duff's device has five — is a `match`.

One `u32` per such region, not one per function, and nothing outside the region
reads it. SQLite's amalgamation has exactly one: `sqlite3VdbeExec`'s
`abort_due_to_error`, which the progress-callback loop at `vdbe_return` jumps
back to.

### Tier 4: the whole-function machine

The relooper checks what it builds, and a graph whose shapes would nest deeper
than `rustc`'s own parser goes — a `switch` with more `case` groups falling one
into the next than it will nest — falls back to one `match` over block numbers,
which is flat however many arms it has:

```rust
let mut __cinrs_state: u32 = 0;
'cfg: loop {
    match __cinrs_state {
        0 => { total = 0; __cinrs_state = 1; continue 'cfg; }
        1 => { return total; }
        _ => ::core::unreachable!(),
    }
}
```

A [computed `goto`](#labels-as-values) does not need it.

All four compute exactly what the C did, and `cinrs` picks the first that can
express the function. Over SQLite's 2,610 defined functions the split is 2,597
structured, 12 relooped, one with a state variable and none on the machine.

Because the locals of a graph-lowered function are hoisted, a `cleanup`
attribute in one is emitted on each edge that leaves the scope rather than as a
drop guard, and a `break` or `continue` out of a statement expression is
refused: there is no Rust loop left to leave.

## Labels as values

GNU C's computed `goto` is lowered the way GCC lowers it: as a `switch` over
the labels whose address the function takes. Those labels are numbered from 1,
and `&&label` is an rvalue of type `void *` whose value is that **number** —
`2usize as *mut c_void` — so it is never a null pointer. Every `goto *e` of the
function stores `e` in one hidden integer local and goes to one shared
dispatch, a `match` on it with an arm per label and a `default` for a value
that is no label's — the undefined behaviour C had. A function that takes
a label's address goes through the graph, and the graph then holds nothing but
ordinary jumps, so [the relooper](#tier-2-the-graph-relooped) reads it like any
other. An interpreter's dispatch table becomes the loop its `switch`-based twin
would have been:

```c
static void *dispatch[] = { &&op_push, &&op_add, &&op_halt };
#define DISPATCH() goto *dispatch[*ip++]
    DISPATCH();
op_push: stack[sp++] = *ip++; DISPATCH();
op_add:  sp--; stack[sp - 1] += stack[sp]; DISPATCH();
op_halt: return stack[sp - 1];
```

```rust
static mut dispatch: [*mut c_void; 3] =
    [1usize as *mut c_void, 2usize as *mut c_void, 3usize as *mut c_void];

__cinrs_goto = (*ip++ as c_ulong).wrapping_add(1);
'dispatch: loop {
    match __cinrs_goto {
        1 => { /* op_push */ __cinrs_goto = (*ip++ as c_ulong).wrapping_add(1); }
        2 => { /* op_add */  __cinrs_goto = (*ip++ as c_ulong).wrapping_add(1); }
        3 => { return stack[sp - 1]; }
        _ => {
            if ::core::cfg!(debug_assertions) {
                ::core::unreachable!()
            } else {
                unsafe { ::core::hint::unreachable_unchecked() }
            }
        }
    }
}
```

The dispatch is shared rather than copied to every `goto *` (GCC calls this
*factoring* the computed gotos), which keeps an interpreter with a hundred
handlers one `match` rather than a hundred.

Two things make that `match` the one a `switch` would have given. The table is
**folded**: `dispatch` is only ever read, so its labels are numbered in its
own order, element `e` is label `e + 1`, and `goto *dispatch[e]` stores
`e + 1` without loading anything — the `match` is then on the opcode itself.
The conditions are that the table is a `static` or an automatic array the
function defines (an automatic one only once), that its initialiser is
nothing but the addresses of distinct labels of the function, and that the
body only ever reads it as `table[e]` — an automatic table is then filled and
never read, and the compiler drops it: a table that is written,
has its address taken, is passed on, holds anything but a label or could be
named by a nested function is read on every jump, which is always correct. And
the `default` — a target that is no label — panics in a build with debug
assertions and is `unreachable_unchecked` otherwise, which is what GCC assumes
and what lets the `match` be a bare jump table. Wren's interpreter runs its
benchmarks at 0.93–1.09× of its own `switch` build this way; see
[`doc/real-programs.md`](real-programs.md#wren-040-not-in-the-repository).

A label address is an *address constant*, so the dispatch table may be a
block-scope `static` as above; it goes into a `void *` variable, a `?:`, or
straight into `goto *`. GCC's label difference `&&a - &&b` is a constant here
too, the difference of the two numbers. What such a value is *not* is a real
address: nothing may be read through it, and arithmetic on one only means
anything inside its own function. The full row, with the limits, is in
[`doc/gnu-extensions.md`](gnu-extensions.md#language-extensions).

## One block, one module

A translation unit is a namespace, so each expansion goes into a private module
of its own followed by a glob re-export. Everything with external linkage is
`pub` inside the module and comes back out through the glob, so Rust calls a C
function by the name its author gave it; a C `static` function or object stays
private to the module, which is exactly the linkage C gives it.

Two blocks in one Rust module are two modules and cannot collide, so both may
`#include "point.h"` and both generate the `struct Point` their own code needs.
What that costs is that a name two blocks both export is ambiguous when *Rust*
uses it (`E0659`) — the C code is unaffected, since each unit sees only its own
— and that the two `struct Point`s are two Rust types, so a value passes to the
unit whose module it was built from.

A *declared* function is one of those names, which is what makes this worth
knowing rather than a curiosity: two blocks in one Rust scope that both
`#include <stdio.h>` both export `printf`, and Rust naming `printf` is `E0659`
whichever of them defined anything. Nothing is wrong until then — each block's C
compiles and calls its own — and the answer is the same `mod` as for the two
`struct Point`s. `use libc::*;` beside a block is the same rule again.

An ordinary Rust `mod` around the invocation is what gives a unit a path and
tells two of them apart:

```rust
mod geometry {
    cinrs::c99! {
        struct Point { int x; int y; };
        int point_x(struct Point p) { return p.x; }
    }
}

// geometry::point_x(geometry::Point { x: 4, y: 9 })
```

A relative `#include` is still looked for beside the `.rs` file, and
`#pragma cinrs export` still gives the unit's symbols their C names, however
deep the `mod`s go. There is no pragma for naming the module; see
[`doc/pragmas.md`](pragmas.md#naming-the-module).

## Linking two blocks together

A function defined in one block is a Rust item, not a C symbol, so another
block's `extern` declaration has nothing to link against.
[`#pragma cinrs export`](pragmas.md#export) changes that for a whole unit: every
function and object with external linkage in it gets `#[unsafe(no_mangle)]` and
is therefore a real C symbol that another block — or a C library, or anything
else in the program — resolves by name.

```rust
mod library {
    cinrs::c99! {
        #pragma cinrs export
        int triple(int n) { return n * 3; }
    }
}

mod user {
    cinrs::c99! {
        int triple(int n);
        int nine(void) { return triple(3); }
    }
}
```

`library`'s `triple` becomes `#[unsafe(no_mangle)] pub unsafe extern "C" fn
triple`, and `user`'s declaration becomes the `#[link_name = "triple"]` extern
above. The risk is C's own: two exported units defining the same name is a
duplicate symbol, and the linker says so rather than the compiler. Only export
the units something else has to link against.

The `mod`s are not decoration here. Both units export a `triple` to Rust — the
definition from one and the declaration from the other — so with the two
invocations written in *one* Rust scope, Rust naming `triple` is the `E0659`
above rather than a call to the definition. `user::triple` and
`library::triple` are two ways of reaching one symbol, and which one a Rust
caller writes is up to it.

[`core::ffi`]: https://doc.rust-lang.org/core/ffi/index.html
