# cinrs: Write C code in Rust

This is a library that implements a procedural macro allowing C code to be written within Rust code.

## Example

```rust
use cinrs::c99;

c99! {
    int fact(int n) {
        if (n == 0) {
            return 1;
        } else {
            return n * fact(n - 1);
        }
    }
}

// The generated functions are `extern "C"`, so calling one is `unsafe`.
let v = unsafe { fact(10) };
println!("fact(10) = {v}");
```

Run it with `cargo run --example fact`.

## What works

* **Four standards.** `c99!`, `c11!`, `c17!` and `c23!` are the same macro for
  four revisions of the language, and `__STDC_VERSION__` follows. `c11!` adds
  `_Static_assert`, `_Generic`, `_Alignof`, `_Alignas` (on the members of a
  `struct` or `union`), `_Noreturn` and anonymous `struct`/`union` members;
  `c17!` is `c11!` with a different version macro; `c23!` adds the keywords C23
  promoted (`bool`, `true`, `false`, `nullptr`, `static_assert`, `alignof`,
  `alignas`, `thread_local`, `constexpr`, `typeof`), `[[…]]` attributes,
  `__VA_OPT__`, `#elifdef`/`#elifndef`, binary constants, digit separators,
  empty initialisers, `auto` type inference, enumerations with a fixed
  underlying type and `unreachable()`. A feature from a later revision used in
  an earlier block is a diagnostic that says which macro to write instead.
* **The C99 language.** All the arithmetic types, pointers, arrays, `struct`,
  `union`, `enum`, `typedef`, string literals, function pointers, `sizeof` with
  the real layout, casts, aggregate and designated initialisers, compound
  literals — `&(struct S){ 1, 2 }`, whose object lives as long as the block it
  is written in — file-scope,
  `static` and `extern` objects, every operator, every control structure —
  `if`, `while`, `do`/`while`, `for`, `switch` with fallthrough, `break`,
  `continue`, `return`, and `goto`, which is lowered to a state machine over
  basic blocks.
* **The C99 preprocessor.** Object-like and function-like macros with `#`,
  `##`, `__VA_ARGS__` and the standard's rescanning rules, every conditional
  directive, `#error`, `#warning`, `#pragma` and `#line`.
* **`#include`.** Standard headers (`<stdio.h>`, `<string.h>`, `<math.h>` and
  the rest) are bundled with the crate, written in plain C99 rather than read
  from the platform, and the calls link against the real C library. Your own
  headers are found next to the `.rs` file that includes them, and editing one
  rebuilds the crate.
* **Pragmas that configure the unit.**
  `#pragma cinrs include_path "…"` adds a search directory;
  `#pragma cinrs link "…"` links a library;
  `#pragma cinrs export` gives everything with external linkage a real C
  symbol, so that another block — or a C library — can call it by name (with
  C's own risk: two exported units defining one name is a duplicate symbol);
  `#pragma cinrs module "…"` names the module the expansion goes into.
* **Two input forms.** C the Rust lexer accepts is written as raw tokens; C it
  refuses (hexadecimal floating constants, `'ab'`, `L"…"`, `\` line
  continuations, C23's digit separators such as `1'000'000`) goes in a string
  literal instead. `##` cannot be written in
  raw-token form either, so a replacement list spells the pasting operator
  `a # # b`.
* **Errors that point at the C.** Every generated token carries the span of the
  C token it came from, so both `cargo` and an IDE put the caret on the C code
  — for the front end's own diagnostics and for `rustc`'s. Pointing *inside* a
  string literal needs `Literal::subspan`, which is unstable, so there the
  position is appended to the message instead; the crate's `nightly` feature
  turns it back into a caret.
* **Variadic functions.** Declaring and calling one works anywhere; *defining*
  one needs Rust 1.99's `c_variadic`, and is a clear error before that.

## Known limitations

* Not supported, each as a located error rather than a silent mistranslation:
  variable length arrays, bit-fields, `_Complex`, old-style (K&R) function
  definitions, `setjmp`/`longjmp`, `_Thread_local`, `_Atomic`, `_BitInt`,
  `#embed`, `__has_include`, and C11's `u8"…"`/`u"…"`/`U"…"` literals with
  their `char16_t`/`char32_t`.
* `_Alignas` is honoured on the members of a `struct` or `union`, by raising
  the alignment of the whole record; a member whose natural offset does not
  already satisfy the alignment it asks for — `struct { char c;
  _Alignas(16) int x; }` — is refused rather than laid out differently from
  the Rust item. On an object it is not supported at all.
* A `constexpr` object is a *constant*: its value is folded wherever the name
  is used (so it may be an array bound or a `case` label), and there is
  nothing to take the address of. Only the arithmetic types are accepted.
  `nullptr` has type `void *` rather than a `nullptr_t` of its own.
* `long double` is `double`: the extended precision, and the ABI that goes with
  it, are not there.
* `va_list` is `core::ffi::VaList`, which cannot be stored in a `struct` or
  returned; the usual uses — `va_start`, `va_arg`, `va_copy`, passing a list to
  `vprintf` — are fine.
* The platform's include directories are never searched. A real `<stdio.h>` is
  not C, so anything outside the bundled set is declared by hand or pointed at
  with an include path.
* Sizes and alignments come from a model of the target rather than from the
  target's own C compiler; the host is assumed to be LP64 when
  cross-compilation cannot be detected from Cargo's environment.
* Each invocation is one translation unit. Two blocks may share a header, but
  the types it declares are then two distinct Rust types — one per unit.

## Conformance

`cinrs` is measured against
[c-testsuite](https://github.com/c-testsuite/c-testsuite), a public database of
C compiler test cases: whole programs with the output each must produce. Of the
220 in its `single-exec` suite, **203 of the 218 that `c99!` is eligible for
pass (93.1 %)**, and 206 of 220 under `c11!` — compiled, run, and diffed
against the expected output. What is left is GCC extensions the cases lean on
(statement expressions, `__attribute__`, incomplete `enum`s, range designators)
and the constructs listed as unsupported above; one program compiles and prints
the wrong thing, and one needs a newer Rust than 1.97. The corpus is a git submodule, so
a fresh checkout skips the suite until `git submodule update --init
third_party/c-testsuite` fetches it. [`doc/c-testsuite.md`](doc/c-testsuite.md)
has the harness, how to run it in either mode, the selection rules and the
baseline with every failure and its cause.

## How it works

The macro recovers the C source text of its own invocation (by slicing the
`.rs` file, or by decoding the string literal) together with a map from byte
offsets back to `proc_macro2` spans, then runs a C front end over it — lexer,
preprocessor, parser, semantic analysis with C's conversion rules made
explicit — and emits Rust, `c2rust`-style: `#[repr(C)]` records, raw pointers,
wrapping arithmetic where C defines wrap-around, `pub unsafe extern "C" fn` for
each function. Each expansion goes into a private module of its own with a glob
re-export, so two blocks in one Rust module never collide. Every token it emits
is stamped with the span of the C it came from, which is what makes the errors
land where they should.

See the [crate documentation](https://docs.rs/cinrs) for the details.
