# `no_std`

Everything generated is `core`-only: `core::ffi` types, `#[repr(C)]` items, raw
pointers, byte strings, `core::hint::unreachable_unchecked` for `unreachable()`,
`core::mem::offset_of!` for `offsetof`, `core::sync::atomic` for `_Atomic` and
the atomic builtins, a `#[used]` function pointer in `.init_array` for a
`constructor`, and the C library's own `abort` for `__builtin_trap` and
`assert`. `goto` is a labelled block, a recovered loop or a state machine, and
all three are `core` too. The C library is still *linked*, because the C code calls it — that
is a link-time dependency of the program rather than a Rust one.

A complex type is the one thing that names another crate — `cinrs::rt`, the
re-export of `cinrs-rt` — and `cinrs-rt` is itself `#![no_std]`, so it changes
nothing here. `default-features = false` drops it, and `_Complex` with it.

Three constructs are the exception. Two of them are variable length arrays and
`alloca`, whose storage is a `Vec`. Nothing in the C says which kind of crate
the expansion is going into, so that `Vec` is `::std::vec::Vec` unless the unit
says otherwise:

```c
#pragma cinrs no_std
```

which makes it `::alloc::vec::Vec` instead. The crate then has to contain
`extern crate alloc;` itself — an expansion is items, and a crate-level
directive is not one of them. Without the pragma, a variable length array in a
`#![no_std]` crate is `rustc`'s own "cannot find `std`", with the caret on the
declaration that needed it.

The third is a **thread-local object**, and the pragma does not help there:
`thread_local!` is a `std` macro and `core` has no thread-local storage at all,
so `_Thread_local` under `#pragma cinrs no_std` is a located error saying
exactly that.
