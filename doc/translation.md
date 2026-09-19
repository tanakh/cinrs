# What the C becomes

A reference for the other side of the boundary: what a construct is translated
into, and what Rust code that reads or calls it has to know. [What
works](features.md) is the tour of the language; this is the shape of the
output.

Two conventions in the snippets below. Every generated item carries an
`#[allow(…)]` list (`non_camel_case_types`, `unused_parens`, `clippy::all` and
two dozen more — C is not idiomatic Rust), and it is left out here. So is the
`::core::` prefix the expansion writes in full: `c_int` below is
`::core::ffi::c_int`.

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

A function the unit only **declares** is linked rather than defined. It is
renamed apart from everything else of that name and pointed back at its symbol:

```c
int printf(const char *, ...);
```

```rust
unsafe extern "C" {
    #[link_name = "printf"]
    pub fn __cinrs_ab49b1a7_printf(_: *const c_char, ...) -> c_int;
}
```

so Rust code calls the C library through its own declaration, not through this
one. `#pragma cinrs link "name"` puts `#[link(name = "name")]` on that block —
and so does cinrs itself in one case: a unit translated for a `*-windows-msvc`
target whose block declares one of the `printf` or `scanf` family gets
`#[link(name = "legacy_stdio_definitions")]`, because the Universal CRT defines
those functions inline in `<stdio.h>` and exports no symbol for them (see
[Cross-compilation](cross-compilation.md#the-microsoft-librarys-inline-printf)).

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

## Types

| C | Rust |
| --- | --- |
| `T *`, `const T *`, `void *` | `*mut T`, `*const T`, `*mut c_void` — raw pointers, and pointer arithmetic goes through `offset`, so nothing generated holds a reference |
| `T a[N]` | `[T; N]` |
| `struct S`, `union U` | `#[repr(C)] #[derive(Copy, Clone)]` items with `pub` members |
| `enum E` | `pub type E = c_int;` plus one `pub const` per enumerator |
| `int (*)(void *)` | `Option<unsafe extern "C" fn(*mut c_void) -> c_int>`, so that a null function pointer is representable |

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
pointer by an amount chosen at run time, so the elements live in a hidden `Vec`
whose `Drop` is the end of the block — the object's lifetime — and the C object
itself is a pointer into it. The model is one hidden `size_t` object per
variable dimension, created where the *type* is declared and named after it:

```c
unsigned long sizes(int n, int m) {
    double a[n][m];
    a[1][2] = 1.0;
    return sizeof a + sizeof a[0];
}
```

```rust
let mut __cinrs_vla_len_a: c_ulong = m as c_ulong;
let mut __cinrs_vla_len1_a: c_ulong = n as c_ulong;
let mut __cinrs_vla_a: ::std::vec::Vec<c_double> =
    ::std::vec::from_elem(0.0, __cinrs_vla_len_a.wrapping_mul(__cinrs_vla_len1_a) as usize);
let mut a: *mut c_double = __cinrs_vla_a.as_mut_ptr();
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
`malloc` does rather than by running the stack out. A zero length allocates
nothing, as it does in GCC; a *negative* one is undefined behaviour in C, and
here it converts to a huge `size_t` and the allocation aborts rather than
corrupting anything.

`alloca(n)` takes one 16-byte aligned block out of a per-function arena, and the
whole arena is freed by the `return` — which is `alloca`'s own lifetime, so a
pointer to it returned to the caller dangles here exactly as it does in C.

In a function lowered into a [state machine](#control-flow-and-goto), where
every local is hoisted to the top, the hidden `Vec` is hoisted with them: it is
created empty, filled where the declaration was written, and dropped when the
function returns rather than when the block ends. A C program can only observe
that as memory it expected to have been given back sooner.

The `Vec` is `::std::vec::Vec` unless the unit writes
[`#pragma cinrs no_std`](no-std.md), which makes it `::alloc::vec::Vec`.

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

What is left over is lowered into a **state machine** over basic blocks, with
every local of the function hoisted to the top and renamed apart:

```c
int into_block(int n) {
    int total = 0;
    if (n > 0) goto inside;         /* into the loop body */
    total = 100;
    while (n < 3) { inside: total += n; n++; }
    return total;
}
```

```rust
let mut total: c_int = 0;
let mut __cinrs_state: u32 = 0;
'cfg: loop {
    match __cinrs_state {
        0 => { total = 0;
               if n > 0 { __cinrs_state = 3; } else { __cinrs_state = 1; }
               continue 'cfg; }
        1 => { total = 100; __cinrs_state = 2; continue 'cfg; }
        2 => { if n < 3 { __cinrs_state = 3; } else { __cinrs_state = 4; }
               continue 'cfg; }
        3 => { total = total.wrapping_add(n); n = n.wrapping_add(1);
               __cinrs_state = 2; continue 'cfg; }
        4 => { return total; }
        _ => ::core::unreachable!(),
    }
}
```

Five things ask for it: a jump **into** a block (a label inside a loop body, an
`if` branch or a `switch` group, named from outside it), a computed `goto`, a
`case` label that is not a direct child of its `switch` body (Duff's device),
two labels whose regions would have to overlap without nesting, and a
declaration between a jump and the label it names, which no Rust block may hold
without ending its scope early. Both forms compute exactly what the C did; only
the second is unpleasant to read, and only the functions that need it get it.
That is also the one systematic cost in the [benchmarks](benchmarks.md) — an
outward `goto` is free.

Because the locals are hoisted, a `cleanup` attribute in such a function is
emitted on each edge that leaves the scope rather than as a drop guard, and a
`break` or `continue` out of a statement expression is refused: there is no Rust
loop left to leave.

## Labels as values

GNU C's computed `goto` is why the state machine is worth having. `&&label` is
an rvalue of type `void *` whose value is the **state number** the label's block
was given, and `goto *e` is `__cinrs_state = e as usize as u32; continue 'cfg;`
— so a function that takes a label's address is always lowered through the
machine.

```c
static void *table[] = { &&push, &&add, &&halt };
goto *table[code[pc]];
```

A label address is an *address constant*, so the dispatch table may be a
block-scope `static` as above; it goes into a `void *` variable, a `?:`, or
straight into `goto *`, and a label whose address is taken keeps a block — and
therefore a number — of its own. GCC's label difference `&&a - &&b` is a
constant here too. What such a value is *not* is a real address: nothing may be
read through it, and arithmetic on one only means anything inside its own
function. The full row, with the limits, is in
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

[`core::ffi`]: https://doc.rust-lang.org/core/ffi/index.html
