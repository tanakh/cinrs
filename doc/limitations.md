# Known limitations

* Not supported, each as a located error rather than a silent mistranslation:
  `_BitInt`, `_Imaginary`, a complex *integer* type
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
  prototype is refused where it is called or its address taken. A `long
  double` handed to the `...` of the `printf` family, or a `long double *` to
  the `scanf` family's, is fine when the format is a string literal that names
  every one of them with `L` (`printf("%.2Lf", x)`, `sscanf(s, "%Le", &x)`): the
  `L` becomes `l`, which the platform's function reads as the `double` it is
  given. Any other `long double` or `long double *` handed to a declared-only
  function's `...` — through a format built at run time, or to `%f` — is
  refused: cast to `double` and use `%f`.
  Where the platform's `long double` is `double` too (MSVC, 32-bit Arm, Apple
  arm64) nothing is refused, but the twins are still linked to the sibling:
  Microsoft's C runtime has no `powl`, `sinl` or `fabsl` symbol, only
  `<corecrt_math.h>`'s inline wrappers over `pow`, `sin` and `fabs`. A `long double` function defined in *another*
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
  rewrite. A memory operand (`"m"`, `"+m"`) is its address in a register, so
  **no bit-field** can be one and **no modifier** applies to one; `%a0` is
  refused: pass the address in a register and write `(%0)` in the template.
  **No flag outputs** (`"=@ccz"`): `setz %b0` with `"=q"`. **No `%=`**: a GNU
  as local label, `1:` … `1b`. **No `asm goto`** in this release: branch in C
  on a value the `asm` sets. **No x87 or MMX operands**, no `"A"` pair (use
  `"=a"` and `"=d"`), no range-checked immediates (`"I"`: write `"i"`), **no
  Intel syntax** (`.intel_syntax`, `{att|intel}`), and no register variables
  (`register int x asm("eax")`: write `"a"(x)`). `asm!` is unsafe, so a
  `[[cinrs::safe]]` function cannot contain one. A register-or-memory
  constraint (`"rm"`, `"g"`) always gets the register, which may change the
  instruction GCC would have chosen but not the meaning. Basic asm at **file
  scope** is `global_asm!`, so it is an error where that is unstable (wasm32,
  MIPS, SPARC), GCC 15's file-scope extended asm is refused, and a C name in
  its text links only when the definition is a real symbol (`#pragma cinrs
  export`, or `ccinrs`).
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
  what that costs. glibc's `<tgmath.h>` goes through, but its macros need
  `__builtin_tgmath`, which cinrs does not have.
* `setjmp` and `longjmp` work, as a Rust unwind, with limits of their own:
  see [setjmp and longjmp](#setjmp-and-longjmp) below.
* Of TS 18661-3's floating types, `_Float32`, `_Float64`, `_Float32x` and
  `_Float64x` are here as the types they are on this model (`float`, `double`,
  `double`, and `long double`, which is `double`). `_Float128` (and
  `__float128`) can be declared — in a prototype, a `typedef`, a pointer — but
  has no value: an object, a cast and a call through a prototype that mentions
  it are refused with the reason. `_Float16`, `__bf16` and `_Float128x` are
  refused outright. See [System headers](system-headers.md#extended-floating-types).
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

## setjmp and longjmp

A `longjmp` is a Rust unwind here, and the function that called `setjmp`
catches it and goes on after the `setjmp` the buffer names; [What the C
becomes](translation.md#non-local-jumps) shows the translation. What follows
from that:

**What works.**

* `setjmp`, `_setjmp`, `sigsetjmp` (the signal mask saved and restored),
  GCC's `__builtin_setjmp`, and their `longjmp`s — from the bundled
  `<setjmp.h>` or the platform's, through a function pointer to `longjmp`
  too (libpng hands `longjmp` itself to the library).
* A `longjmp` across any number of frames: other C files, another `c99!`
  block, and C library functions with unwind tables — glibc's `qsort`
  calling a comparator that jumps, a `raise` or a blocking system call whose
  signal handler jumps.
* libpng's error recovery, TurboJPEG, xz's tuktest, and Lua's
  `pcall`/`error` and its test suite run with their gcc output.

**Where a `setjmp` may stand.** As C17 7.13.1.1p4 says: the whole controlling
expression of an `if`, `switch`, `while`, `do` or `for`, alone, negated with
`!`, or compared with an integer constant; or a whole expression statement,
cast to `void` or not. Beyond C17, as GCC takes them: `r = setjmp(buf);`,
`int r = setjmp(buf);` and `if ((r = setjmp(buf)) == 0)`. Anywhere else — an
operand of anything else, `return setjmp(buf);`, a statement expression — is
a located error that says so: what follows the call could not be resumed.

**What differs from GCC.**

* **Locals keep their latest values.** C promises only `volatile` ones after
  a `longjmp`; here every local of the function that called `setjmp` has the
  value it had when the jump left, which is stronger and allowed.
* **A `cleanup` attribute does not run** when a `longjmp` leaves its scope —
  as in GCC, which runs one only for `-fexceptions` unwinding. The storage of
  a variable length array or an `alloca` *is* given back, as the stack is.
* **A `longjmp` costs about 1.2 µs**, plus about 130 ns for every frame it
  crosses (it walks the stack once to check that the jump can be done, and
  the unwind walks it twice), where GCC's costs tens of nanoseconds: Lua's
  error-raising loop is several times slower than with gcc. A `setjmp` costs
  what GCC's does, and so does a `pcall` that raises nothing.
* **Every function is `extern "C-unwind"`** in a unit that might be crossed:
  under `ccinrs`, every file, which costs 0.3 % of the instructions on
  SQLite (`-fno-cinrs-unwind` takes it back, and refuses `setjmp` and
  `longjmp`); in a `c99!` block, only one that calls `setjmp` or `longjmp`
  itself or says [`#pragma cinrs unwind`](pragmas.md#unwind). A Rust callback
  handed to such a unit has to be `extern "C-unwind"` too.

**What cannot be done, and stops the program with a message.**

* **A `longjmp` out of a signal handler that interrupted the program's own
  code** — a timer's `SIGALRM` in a loop, a fault. There is no call at the
  interrupted instruction to unwind from:

  ```text
  cinrs: longjmp out of a signal handler that interrupted the program's own code
  (an asynchronous signal, such as a timer's or a fault's): a longjmp is an unwind
  here, and there is no call at the interrupted instruction to unwind from; see …
  ```

  A signal that arrives inside a library call — `raise`, `kill` of the
  process itself, a blocking `read` — is fine.
* **A `longjmp` to a function that has returned, or onto another thread's or
  another stack's buffer** (a coroutine library's), which C17 7.13.2.1 leaves
  undefined: "cinrs: longjmp to a jmp_buf whose setjmp is not active on this
  thread: …", or "… to a setjmp that is not on this stack: …".
* **A frame in between that cannot be unwound** — C compiled without unwind
  tables, or a file `ccinrs -fno-cinrs-unwind` compiled: "cinrs: longjmp
  cannot unwind to its setjmp: …".

**Refused where they are written.** WebAssembly (whose Rust aborts on a
panic), a target with no operating system and `#pragma cinrs no_std` (no
`std` to catch an unwind with), a crate built with `panic = "abort"` (at
compile time), and `ccinrs -fno-cinrs-unwind`.
