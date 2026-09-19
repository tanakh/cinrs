# The pragmas: `#pragma cinrs`, and what else the preprocessor understands

C99 6.10.6 gives `#pragma` to the implementation: a directive whose first token
the implementation does not recognise behaves in an implementation-defined
manner, which in practice means *ignore it*. `cinrs` does exactly that — except
for `#pragma cinrs`, which is addressed to this crate, so an option it does not
know there is a mistake worth reporting rather than a hint meant for some other
compiler. A pragma this crate *does* know but which is written wrongly is
reported whatever its family: `#pragma pack(3)` and `#pragma push_macro(LIMIT)`
are diagnostics rather than silent no-ops.

The `cinrs` pragmas are how a translation unit says what it needs: the machine
it is translated for, where its headers are, what it links against, which of
its functions `rustc` is to check. They are **directives** rather than
attributes or macro arguments, which is what makes them mean the same thing in
raw-token and in string-literal input:

```rust,ignore
cinrs::c99! {
    #pragma cinrs safe gcd
    int gcd(int a, int b) { return b == 0 ? a : gcd(b, a % b); }
}
```

Everything on this page works in every entry point — `c89!` through `c23!` and
every `gnu*!` dialect — in a header as well as in the unit's own text, and
through `_Pragma("…")` as well. [`target`](#target-triple) is the one exception
to the last two, for a reason its own section gives.

## `#pragma cinrs`

| Option | What it does | How far it reaches |
| --- | --- | --- |
| [`target "<triple>"`](#target-triple) | picks the data model the unit is translated for | the whole unit, and it must come first |
| [`include_path "<dir>"`](#include_path-dir) | adds a directory to the include search path | from the directive on |
| [`system_include [first]`](#system_include-and-system_include-first) | puts the platform's own include directories on the path | from the directive on |
| [`link "<name>"`](#link-name) | `#[link(name = "…")]` on the generated `extern` block | the whole unit |
| [`export`](#export) | gives everything with external linkage a real C symbol | the whole unit |
| [`safe f g h`](#safe-f-g-h) | generates those functions without `unsafe` | the whole unit |
| [`no_std`](#no_std) | takes the `Vec` a VLA or `alloca` needs from `alloc` | the whole unit |
| [`crate "<path>"`](#crate-path) | says where the `cinrs` facade crate is | the whole unit |

An option that is not one of those eight is an error that lists them:

```text
error: unknown #pragma cinrs option 'frobnicate'; the options are 'target',
       'include_path', 'system_include', 'link', 'export', 'safe', 'no_std'
       and 'crate'
```

and `#pragma cinrs` with nothing after it is the same error, differently
worded. The four options that take a string want **one narrow string literal**:
a missing one, a token that is not a literal, a wide literal (`L"…"`) and an
empty string are each their own message. A token *after* the value is reported
too — and the option is still applied, since what the unit asked for was
clear.

### `target "<triple>"`

```c
#pragma cinrs target "i686-unknown-linux-gnu"
```

Picks the **data model** the unit is translated for: the width of `long`,
`wchar_t` and a pointer, the alignment of `long long` and `double`, whether
plain `char` is signed, the byte order, and whether `__int128` exists at all.
Every `sizeof`, every member offset and every predefined macro follows it. The
triple is a Rust target triple; the families that are supported, and what each
refuses, are tabulated in [`doc/c-status.md`](c-status.md).

It is the one pragma that cannot be handled where it stands: the predefined
macros are built from the model *before* the first directive is read, so the
whole text is scanned for this directive first, lexically. Three rules follow
from that.

* It has to be a **directive in the unit's own text**. A header's comes too
  late to have been part of the scan, and one produced by `_Pragma` is never
  seen at all; either is an error saying which model the unit is really being
  translated for.
* It has to come **before every `#include` and `#if`**, and before
  `#pragma cinrs system_include`: each of those reads the model — a bundled
  header branches on `_WIN32` and on `__SIZEOF_POINTER__` — so a pragma after
  one would be a lie, and it is reported rather than half-applied. An `#ifdef`
  or `#ifndef` decides nothing about the model and does not count, so an
  include guard above the pragma is fine.
* Because the scan is lexical, a `target` inside a group that `#if 0` skips is
  **applied all the same**. Conditionalising one does not work.

Naming the same triple twice says the same thing twice and is no mistake.
Naming two different ones is an error on the second; the first is what the unit
is translated for. A triple cinrs does not know, or knows and does not support,
is an error naming the reason.

The same choice for a whole crate is the **`CINRS_TARGET`** environment
variable, which a build script sets from Cargo's `TARGET`; a unit's pragma wins
over it, and with neither, the model is the host's. Programmatically it is
`Options::for_target`. Every expansion states the model it was translated for
as a block of `assert!`s over the `core::ffi` aliases, so a wrong answer is a
message at compile time rather than a wrong `sizeof` at run time; see
[Cross-compilation](cross-compilation.md).

### `include_path "<dir>"`

```c
#pragma cinrs include_path "vendor/include"
#include <mylib.h>
```

Adds a directory to the include search path, in the group that is looked in
**first** — after the including file's own directory, and before
`Options::include_paths`, before `CINRS_INCLUDE_PATH` and before the bundled
headers. A relative path resolves against `CARGO_MANIFEST_DIR`, the package
being compiled, so a block means the same thing however the build was invoked.

The directory is added **where the directive stands**, so it has to come before
the `#include`s it is meant to serve. Any number of them may be written, and
they are searched in the order written.

The same list for a whole crate is **`CINRS_INCLUDE_PATH`**, split the way the
platform splits `PATH`, and searched after both the pragma's directories and
`Options::include_paths`.

### `system_include` and `system_include first`

```c
#pragma cinrs system_include
#include <sys/stat.h>
```

Puts the **platform's own** include directories on the search path, which is
how a unit reaches a type whose layout only the platform knows — `struct stat`,
`DIR`, `pthread_mutex_t`, the real `FILE`. Written plain they go *after* the
bundled headers, so cinrs's own `<stdio.h>` and `<string.h>` still win and only
a header cinrs does not carry comes from the machine. Written
`system_include first` they go before, which is how a unit asks for the
platform's version of a header cinrs does carry.

The argument is a bare word rather than a string, so that it reads as the
switch it is. Like `include_path`, it takes effect where it stands and has to
come before the `#include`s it is meant to change — and because the
directories are read off the data model, it *settles* that model: a
`#pragma cinrs target` after one is an error.

Any word other than `first` is an error, and so is a second token after
`first`. The one thing that can go wrong afterwards is a cross build for a
target whose default directories cinrs does not know: the defaults are the
*host*'s, and a header laid out for another machine is worse than no header at
all, so that is an error naming `CINRS_SYSTEM_INCLUDE_PATH`.

For a whole crate there is **`CINRS_SYSTEM_INCLUDE`**: `1`, `on`, `true` or
`yes` for the plain mode, `first` for the other, and `0`, `off`, `false`, `no`
or the empty string for off; anything else is a diagnostic. A pragma in a unit
overrides the variable. **`CINRS_SYSTEM_INCLUDE_PATH`** *replaces* the
per-target default list, split like `PATH`, which is the only way to name the
directories on a target whose default cinrs does not know and the only way to
point a cross build at a sysroot. Programmatically it is
`Options::system_include`. The whole subject has a page of its own:
[`doc/system-headers.md`](system-headers.md).

### `link "<name>"`

```c
#pragma cinrs link "mylib"

extern int mylib_open(const char *path);
```

Puts `#[link(name = "mylib")]` on the generated `extern` block, for a program
that calls into a library the Rust runtime does not already link. Nothing is
needed for the C library itself.

It reaches the whole unit wherever it is written — there is one `extern` block
— and any number of libraries may be named. They are kept in the order written,
and naming the same one twice adds it once.

There is no environment variable for it; the Cargo-level equivalent is a
`build.rs` that prints `cargo:rustc-link-lib=mylib`.

### `export`

```c
#pragma cinrs export

int counter;
int bump(void) { return ++counter; }
```

Gives everything with external linkage a **real C symbol**, so that another
`c99!` block — or a C library, or a linker script — can reach it by the name
its author gave it. A definition becomes `#[unsafe(no_mangle)]`, or
`#[unsafe(export_name = "…")]` where the Rust item is not spelled like the C
name (`int match(int)` is the item `r#match` and the symbol `match`). A
`static` function or object keeps the internal linkage C gives it and is not
exported.

It takes no argument and reaches the whole unit wherever it is written. An
argument is reported — the option says nothing about *what* is exported — and
the export still happens. Two exported units that define one name are a
duplicate symbol at link time, exactly as two C files would be; that is the
risk the pragma buys.

One thing cannot be exported: a `_Thread_local` object, because there is no
stable way to give a Rust `thread_local!` a C symbol. It is a located error
rather than a silently missing symbol.

### `safe f g h`

```c
#pragma cinrs safe gcd

int gcd(int a, int b) { return b == 0 ? a : gcd(b, a % b); }
```

Generates those functions as `pub extern "C" fn` rather than
`pub unsafe extern "C" fn`, with the body *not* wrapped in an `unsafe` block,
so that `rustc` checks the whole translation and Rust calls them without
`unsafe`. What such a body may hold, and what it may not, is *Safe functions*
in the [crate documentation](https://docs.rs/cinrs/latest/cinrs/#safe-functions).

It takes **identifiers** rather than a string, so that it reads like the C it
is naming, and at least one of them; a unit that marks one function usually
marks several. It reaches the whole unit and does not care about position: the
pragma may stand before or after the definition.

A token that is not an identifier is an error, as is writing the option with no
name at all. A name the unit does not declare is an error too — the check
happens once the program and the pragmas are together, which is the only point
where what the unit defines is known. Three kinds of function are refused with
the reason, and the request is then taken back rather than half-applied: one
that is only *declared* here (nothing about a function compiled elsewhere can
be checked), one that takes `...` (Rust makes every C-variadic function
unsafe), and a GNU nested function (its body dereferences the hidden pointers
it reaches the enclosing frame through).

The same request is spelled `[[cinrs::safe]]` wherever `[[…]]` parses (`c23!`
and every GNU dialect) and `__attribute__((cinrs_safe))` in every entry point;
see [The `cinrs` attributes](#the-cinrs-attributes).

### `no_std`

```c
#pragma cinrs no_std

int sum(int n) { int a[n]; int t = 0; for (int i = 0; i < n; i++) { a[i] = i; t += a[i]; } return t; }
```

Says the expansion goes into a `#![no_std]` crate. Everything cinrs generates
names `core` alone except the storage a variable length array or `alloca`
needs, which is a `Vec`; this decides whether that `Vec` is spelled
`::std::vec::Vec` or `::alloc::vec::Vec`. A unit that uses neither construct
needs neither the pragma nor an allocator. See
[`no_std`](no-std.md).

It takes no argument and reaches the whole unit wherever it is written. An
argument is reported and the option is still honoured. A `_Thread_local`
object under it is a located error: `thread_local!` is a `std` macro and
`core` has no thread-local storage, so the pragma cannot help there.

There is no environment variable for it, and no Cargo feature: the `cinrs`
facade is `#![no_std]` already, and this is about the *generated code*.

### `crate "<path>"`

```c
#pragma cinrs crate "crate::vendor::cinrs"

_Complex double twice(_Complex double z) { return z * 2; }
```

Says where the `cinrs` facade crate is to be found. The generated code names it
only where it needs the runtime — today that is a complex type, and nothing
else — and has to name it in full, because the expansion lives in a module of
its own and cannot rely on anything being in scope there. The default is
`::cinrs`; a dependency renamed in `Cargo.toml`, or one reached through a
re-export, needs this.

It reaches the whole unit wherever it is written, and repeats are forgiving one
way only: the same path twice is harmless, two different paths are an error on
the second and the first wins.

The value is pasted into the expansion as tokens, so it is checked strictly: a
sequence of identifier segments joined by `::`, optionally starting with one,
where `crate`, `self` and `super` are allowed as segments. Anything else is
`'…' is not usable as a Rust path to a crate`.

The related switch is the crate's **`complex`** Cargo feature, which is on by
default. Without it `_Complex` is a diagnostic, the `cinrs-rt` dependency is
gone, and nothing in an expansion names the facade crate at all — so the pragma
has nothing to do.

## Naming the module

There is no pragma for this, because it is not a question about the C. Every
expansion goes into a module of its own — `__cinrs_unit_<hash>`, **private**,
followed by a **glob re-export** of it into the module the invocation was
written in — because a translation unit is a namespace, and two of them written
side by side must not collide. Rust already has the way to give that unit a path
of its own: an ordinary `mod` around the invocation, which settles its
visibility and its attributes at the same time.

```rust,ignore
pub(crate) mod packet {
    cinrs::c99! { #include "packet.h" }
}
```

`packet::Header` is then the type and `packet::decode` the function. That is
also how two units that define the *same* name are told apart — the glob
re-exports conflict only when Rust *uses* an ambiguous name, and one `mod` each
is what says which is meant. Neither a relative `#include`, which is still
looked for beside the `.rs` file, nor [`export`](#export), which still gives the
unit's symbols their C names, cares how many `mod`s the invocation sits inside.

## The pragmas the preprocessor knows

These are not cinrs's own; they are the ones a real C program already writes,
and they behave here as they do in GCC. [`doc/gnu-extensions.md`](gnu-extensions.md#preprocessor-extensions)
has the same list as a support table.

### `#pragma once`

```c
#pragma once
struct Config { int verbose; };
```

The file the directive is written in is not read again, however the next
`#include` spells its name. It is keyed on the file's identity rather than on
the spelling, so `"config.h"` and `"../inc/config.h"` are one file. An include
guard does the same thing and both work; a header may use either.

### `#pragma pack(…)`

```c
#pragma pack(push, 1)
struct header { char kind; int length; };   /* 5 bytes, not 8 */
#pragma pack(pop)
```

Caps the alignment every member of a record is laid out with, which becomes
`#[repr(C, packed(N))]` on the generated item. Five forms are accepted:

| Written | Effect |
| --- | --- |
| `#pragma pack(N)` | members are aligned to at most `N` bytes |
| `#pragma pack()` | back to the natural alignment |
| `#pragma pack(push)` | saves the value in force, which stays in force |
| `#pragma pack(push, N)` | saves it and sets `N` |
| `#pragma pack(pop)` | restores the saved value |

`N` is a power of two up to 16. The value in force where a record is
**defined** is the one that applies to it, so a pragma after the definition
changes nothing about it.

Anything else is a diagnostic rather than a silent no-op:
`#pragma pack expects '(N)', '(push, N)', '(push)', '(pop)' or '()', where N is
a power of two up to 16` — which is also what `pack(3)` and `pack(32)` get —
and `#pragma pack(pop) with nothing pushed`. The per-record and per-member
equivalent is `__attribute__((packed))`, which is `pack(1)` for the one
declaration it is written on.

### `#pragma push_macro("X")` and `#pragma pop_macro("X")`

```c
#pragma push_macro("LIMIT")
#undef LIMIT
#define LIMIT 99
#pragma pop_macro("LIMIT")
```

MSVC's, and in GCC since 4.4: a header that has to redefine a macro for a few
lines saves the old definition and puts it back. `push_macro` saves the current
definition — or the fact that there was none — and `pop_macro` restores it,
undefining the macro again if that is what was saved. They nest. A `pop_macro`
with nothing pushed is ignored, as GCC does.

The name is a **string literal** that is then read as an identifier, so
`#pragma push_macro(LIMIT)` is an error: `#pragma push_macro needs a string
literal naming a macro`.

### `#pragma GCC …`

```c
#pragma GCC poison gets sprintf
#pragma GCC warning "this build is untested"
```

* **`#pragma GCC poison a b c`** takes identifiers and means they are never to
  be written again; using one afterwards is `attempt to use the poisoned
  identifier 'a'`. A token that is not an identifier is an error.
* **`#pragma GCC error "…"`** is an error carrying that text and
  **`#pragma GCC warning "…"`** is a warning carrying it, exactly as `#error`
  and `#warning` are. The text is reported as it was written, quotes and all.
* **`#pragma GCC diagnostic push`/`pop`/`ignored`/`warning`/`error "-W…"`**,
  **`#pragma GCC system_header`** and **`#pragma GCC visibility push`/`pop`**
  are accepted and ignored: there are no warnings of this front end's to
  suppress, no distinction between a system header and any other to make once a
  file has been found, and no visibility to set.

Note that `#pragma GCC diagnostic error "-Wall"` is the *diagnostic* form and
is ignored, while `#pragma GCC error "…"` is the one that reports.

### Everything else

Silently ignored, which is what 6.10.6 asks for. That includes
`#pragma message("…")`, `#pragma weak`, `#pragma redefine_extname`,
`#pragma region` / `#pragma endregion`, `#pragma omp …`, and every other
vendor's pragma. `#pragma weak` is the one worth knowing about: it asks for weak
linkage, which stable Rust cannot express at all, so ignoring it is the same
answer `__attribute__((weak))` gets.

The standard `#pragma STDC` pragmas — `FP_CONTRACT`, `FENV_ACCESS` and
`CX_LIMITED_RANGE` — are ignored too, in either state. What the generated code
does is the `OFF` state of all three: an expression is translated operation by
operation and nothing is contracted into a fused multiply-add, nothing reads or
writes the floating-point environment, and complex multiplication and division
go through the runtime's Annex G implementations rather than the naive
formulae. So a unit that switches one *off* gets what it asked for, and one
that switches one *on* gets no change.

## The `cinrs` attributes

One thing a pragma asks for can be asked for on the function instead, which is
often where it belongs:

| Spelling | Where it works |
| --- | --- |
| `[[cinrs::safe]]` | `c23!` and every `gnu*!` dialect — wherever `[[…]]` parses |
| `__attribute__((cinrs_safe))` | every entry point, `c89!` included |
| `#pragma cinrs safe f g` | every entry point, and needs nothing written on the function itself |

`cinrs` is this crate's own attribute namespace, so — unlike `[[clang::…]]`,
which C23 6.7.13.1p3 lets an implementation ignore — a name it does not know
there is an error that lists the ones it has: today only `safe`.

## The environment variables

| Variable | What it says | The pragma that says it |
| --- | --- | --- |
| `CINRS_TARGET` | the data model, for a whole crate; usually set from a build script with `cargo:rustc-env=CINRS_TARGET=$TARGET` | [`target`](#target-triple), which wins over it |
| `CINRS_INCLUDE_PATH` | include directories for a whole crate, split like `PATH`, searched after the pragma's and `Options::include_paths` | [`include_path`](#include_path-dir) |
| `CINRS_SYSTEM_INCLUDE` | `1`/`on`/`true`/`yes`, `first`, or `0`/`off`/`false`/`no`/empty | [`system_include`](#system_include-and-system_include-first), which overrides it |
| `CINRS_SYSTEM_INCLUDE_PATH` | *replaces* the per-target default system directories, split like `PATH`; the only way to reach a sysroot or an SDK | — |
| `CARGO_MANIFEST_DIR` | set by Cargo; what a relative [`include_path`](#include_path-dir) resolves against | — |
