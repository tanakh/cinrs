# Known limitations

* Not supported, each as a located error rather than a silent mistranslation:
  `setjmp`/`longjmp`, `_BitInt`, `_Imaginary`, a complex *integer* type
  (`_Complex int`, which is a GNU extension), an `_Atomic` *aggregate* (legal
  C, and there is nothing in the generated Rust to be the lock it needs), and
  C23's *named* universal character `\N{LATIN SMALL LETTER E WITH ACUTE}`. On
  the GNU side: the vector extensions — for which the
  [Intel SIMD intrinsics](features.md#simd-intrinsics) are the answer, and the
  diagnostic says so — and the parts of inline assembly listed below.
  Two of C11's four `__STDC_NO_*` macros depend on how the expansion was
  configured, which is the standard's own way of saying that a part is left
  out. `__STDC_NO_THREADS__` follows the *target*: `<threads.h>` declares the
  platform's own threads, so it is bundled for the C libraries whose objects
  cinrs can lay out — glibc and musl, both on Linux — and refuses on the rest,
  which is where the macro is predefined. `__STDC_NO_COMPLEX__` follows the
  [`complex` feature](features.md#complex-numbers) and is normally *not*
  defined; neither are `__STDC_NO_ATOMICS__` and `__STDC_NO_VLA__`, atomics and
  variably modified types being here. The one
  corner of the latter that is left is a bound written in a *type name* —
  `(double (*)[m])p` — where there is no declaration to keep the length in, so
  an expression that needs it is refused.
* A bit-field has no address, so it is not a field of the generated Rust
  `struct`: a run of them shares one `pub __cinrs_bitsN: [u8; K]`, and each
  named member becomes a pair of inherent methods — `s.level()` reads it and
  `s.set_level(v)` writes it, in the member's declared C type. The C itself is
  unchanged (`s.level = 3`, `p->flags |= 1`, `switch (s.kind)`); the accessors
  are what *Rust* code on the other side uses.
* `_Alignas` and `__attribute__((aligned(N)))` are honoured on the members of a
  `struct` or `union`: the member moves to the boundary it asks for and the
  generated Rust item gets explicit padding so that both sides agree about
  where it went. On an **object** — automatic, `static`, at file scope or
  `_Thread_local` — Rust can over-align a *type* and nothing else, so the
  binding is generated inside a one-field wrapper that carries the alignment
  (`#[repr(C, align(64))] struct __cinrs_align_64<T>(pub T);`) and every use of
  it goes through the field: **Rust code reads `buf.0`**. The C program sees
  none of it — `sizeof buf` is the object's own size. One platform cannot
  deliver it for a *thread-local* object: on macOS, dyld allocates a thread's
  thread-local storage with `malloc`, which aligns to 16 bytes and honours no
  stricter request, so `_Thread_local _Alignas(64)` gets 16 there — from a C
  compiler as much as from `cinrs`. GCC's `aligned(N)` on a `typedef` of a
  scalar or a pointer is honoured in both directions — `aligned(1)` makes every
  access through a pointer to it unaligned — but a *weaker* one is refused where
  the layout cannot follow yet: a member whose `typedef` is aligned to more
  than one byte but less than its type, an array member of such a `typedef`,
  and a `typedef` of a record, an array or an `_Atomic` type.
* A `constexpr` object is a *constant*: its value is folded wherever the name
  is used (so it may be an array bound or a `case` label), and there is
  nothing to take the address of. Only the arithmetic types are accepted.
  `nullptr` has type `void *` rather than a `nullptr_t` of its own.
* `long double` is `double`: the extended precision, and the ABI that goes with
  it, are not there. That is self-consistent for everything a unit defines, and
  the boundary with the platform's library — whose `long double` is x87's
  eighty bits on x86-64 System V and i386, and a 128-bit quad on AArch64 Linux
  and most other 64-bit targets — is handled rather than trusted. A
  declared-only ISO C function whose only difference from a `double` sibling is
  the type (`strtold`, `wcstold`, the `<math.h>` and `<complex.h>` `l` forms,
  and `nexttoward`) is linked to the sibling, `#[link_name = "strtod"]`; any
  other declared-only function with a `long double` or a `long double *` in its
  prototype is refused where it is called or its address taken, and so is a
  `long double` or a `long double *` handed to a declared-only function's `...`
  (`printf("%Lf", x)`, `sscanf("%Lf", &x)`): cast to `double` and use `%f`.
  Where the platform's `long double` is `double` too (MSVC, 32-bit Arm, Apple
  arm64) nothing needs either. A `long double` function defined in *another*
  `cinrs` unit is refused the same way, since a unit cannot tell it from the
  platform's.
* The [SIMD intrinsics](features.md#simd-intrinsics) are x86's and x86-64's, and
  what `core::arch` has on this crate's minimum supported Rust version — SSE
  through AVX-512, with the AVX-512 intrinsics Rust still keeps unstable
  (AVX512-VP2INTERSECT, and the FP16 and BF16 ones that take a scalar `f16` or
  `bf16`) left out — no MMX or `__m64`, and nothing for NEON or any other
  architecture's vectors, where `<immintrin.h>` is an `#error` naming the
  reason. Three things a program written for GCC will notice.
  **Only the x86-64 baseline is predefined**: `__SSE__` and `__SSE2__` are
  there and nothing above them, because a procedural macro cannot see rustc's
  `-C target-feature`, so `#ifdef __AVX2__` and `#ifdef __AVX512F__` take the
  other branch whatever the machine and `__builtin_cpu_supports("avx2")` is the
  question to ask instead. `#pragma GCC target("avx2")` does define them, and
  what they imply, for the rest of the file, as GCC's does — which is how a
  header that selects its SIMD path with `#ifdef __AVX2__` sees it under the
  pragma.
  **A function that passes or returns a 256-bit vector by value needs
  `__attribute__((target("avx")))`, and a 512-bit one `target("avx512f")`** —
  that is the ABI's rule rather than this crate's, and rustc refuses the
  definition and the call without it, where `gcc -Wpsabi` warns about the same
  C; a 128-bit vector, and a `__mmask*`, needs nothing.
  And **a `[[cinrs::safe]]` function cannot call an intrinsic**, because every
  `core::arch` intrinsic is a `#[target_feature]` function and Rust makes those
  unsafe to call — which is refused with the instruction set named, and holds
  for GCC's vector operators too (`a * b` on `__m128d` is `_mm_mul_pd`); the
  operators with no single instruction behind them — integer multiplies,
  shifts and comparisons on the 64-bit lanes GCC reads `__m128i` as, and the
  512-bit comparisons — are refused with the intrinsic to write. A memory
  operand is `void *` or `const void *` exactly where GCC 15.2's headers have
  it, and typed where GCC types it, so a call needs the casts it needs with GCC
  and no others; sixty-two names that `core::arch` deprecates, keeps unstable or
  takes a Rust reference for are absent, and are listed in
  [What works](features.md#simd-intrinsics).
* [Inline assembly](features.md#inline-assembly) is x86's and x86-64's, and
  only what Rust's `asm!` can say; the rest is refused by name, with the
  rewrite. **No memory operands** (`"m"`, `"+m"`, `"o"`, …): pass the address
  in a register, `"r"(&x)`, and write `(%0)` in the template. **No `rbx`
  clobber**, since rustc keeps rbx for LLVM: give the value a `"=b"` operand,
  which cinrs carries in and out of rbx with an `xchg` around the template. **No flag outputs** (`"=@ccz"`): `setz %b0` with `"=q"`. **No `%=`**:
  a GNU as local label, `1:` … `1b`. **No `asm goto`** in this release: branch
  in C on a value the `asm` sets. **No x87 or MMX operands**, no `"A"` pair
  (use `"=a"` and `"=d"`), no range-checked immediates (`"I"`: write `"i"`),
  **no Intel syntax** (`.intel_syntax`, `{att|intel}`), and no register
  variables (`register int x asm("eax")`: write `"a"(x)`). `asm!` is unsafe, so
  a `[[cinrs::safe]]` function cannot contain one. A register-or-memory
  constraint (`"rm"`, `"g"`) always gets the register, which may change the
  instruction GCC would have chosen but not the meaning.
* `va_list` is `core::ffi::VaList`, which cannot be stored in a `struct` or
  returned; the usual uses — `va_start`, `va_arg`, `va_copy`, passing a list to
  `vprintf` — are fine, and so is a **`va_list *`** parameter or local, which
  is what lets a helper advance the caller's list. `va_arg` of a `struct`, a
  `union` or a complex value is rebuilt from the eightbytes the ABI passed it
  in, so it works for records of **at most sixteen bytes on x86-64 System V**
  and is a located error anywhere else.
* The platform's include directories are not searched *by default*, which is
  what keeps a unit self-contained and portable across target models. A program
  that needs `struct stat` or the real `FILE` asks for them with
  `#pragma cinrs system_include`; see [System headers](system-headers.md) for
  what that costs, and
  [`doc/system-headers.md`](system-headers.md#the-gap-tgmathh) for the one
  header of the standard set glibc will not hand over (`<tgmath.h>`).
* `setjmp` and `longjmp` are refused where they are *called*, whichever header
  declared them: they resume a saved machine context, and the state a
  `longjmp` would return into is the generated Rust's. Declaring them, and
  declaring a `jmp_buf`, are fine — half of POSIX pulls `<setjmp.h>` in.
* The extended floating types — `__float128`, `_Float128`, `_Float16` and the
  rest of TS 18661-3's set — are refused with the reason rather than mapped
  onto `double`.
* Sizes and alignments come from a model of the target rather than from the
  target's own C compiler. Cross-compiling needs one line in a build script;
  see [Cross-compilation](cross-compilation.md), and note that without it
  a cross build is a failed compile-time assertion rather than a program that
  computes the wrong thing.
* Each invocation is one translation unit. Two blocks may share a header, but
  the types it declares are then two distinct Rust types — one per unit.
* **In plain `system_include`, the POSIX additions to an ISO C header are not
  visible.** `<time.h>`, `<signal.h>`, `<stdio.h>`, `<stdlib.h>`, `<string.h>`
  and `<locale.h>` are ISO C headers that POSIX *adds to* — `nanosleep` and
  `clock_gettime` in `<time.h>`, `sigaction` in `<signal.h>`, `fileno` and
  `popen` in `<stdio.h>`, `setenv` and `mkstemp` in `<stdlib.h>`, `strdup` in
  `<string.h>`. The bundled copy declares the ISO C part, and in the plain mode
  the bundled copy is the one that wins, so a call to one of the additions is
  "implicit declaration of function". `#pragma cinrs system_include first` takes
  every header from the platform and has all of POSIX; see
  [The switch](system-headers.md#the-switch). (A *whole* POSIX header —
  `<unistd.h>`, `<fcntl.h>`, `<pthread.h>` — is unaffected: the bundled set has
  none of those, so the platform's is what either mode finds.)
* **A macro is not exported to Rust.** A function a unit declares becomes an
  item Rust can call and a `struct` becomes a type Rust can build, but an
  object-like macro (`Z_OK`, `SEEK_SET`, `PATH_MAX`) becomes no `pub const`, and
  a function-like one (`deflateInit`, `isascii`) becomes no function: a macro is
  not a declaration and has no type. Both are one line of C away, inside the
  block — `enum { MY_OK = Z_OK };` is one `pub const` per enumerator, and
  `int wrap(T *p) { return macro(p); }` is a function — which is what
  [Calling a C library from Rust](features.md#calling-a-c-library-from-rust)
  shows. Rust asking for the macro's name directly is `E0425`, "cannot find
  value", rather than anything subtler.
* **An object a unit only declares is not nameable from Rust.** A declared
  function comes out under its C name; a declared object deliberately does not,
  because a glob-imported `static` is a name a Rust `let` may not shadow
  (`error[E0530]: let bindings cannot shadow statics`) and `<stdio.h>`'s
  `stdout`, glibc's `timezone` and `<unistd.h>`'s `optarg` would then be in the
  way of ordinary Rust in the surrounding module. The item keeps a hidden
  `__cinrs_<unit>_<symbol>` name, the C in the unit is unaffected, and Rust
  reaches the object the way one C translation unit hands another a `FILE *`:
  through an accessor written in the block,
  `FILE *get_stdout(void) { return stdout; }`. See
  [Objects](translation.md#objects).
* **In an editor, a raw-token block with a `#define` or an `#if` needs the file
  to be saved.** rust-analyzer hands a procedural macro tokens with no source
  positions at all, so a block's text is recovered by finding the invocation in
  the crate's `.rs` files and matching it token for token — see
  [Input forms](features.md#where-a-raw-token-blocks-text-comes-from). While a
  buffer differs from what is on disk there is no match, and a text rebuilt from
  the tokens alone has no lines to hang a directive on: such a block reports, once
  and on the `#`, that it cannot be read, and reads normally again the moment the
  file is saved. Everything else — any C without directives, `#include`, `#ifdef`,
  `#endif` — works in an unsaved buffer too, and a block written as a
  [string literal](features.md#input-forms) (`c99! { r#"…"# }`) never needs a
  position in the first place. A `cargo build` is unaffected: `rustc` gives the
  positions and the file on disk is what it compiles.
