# The platform's own headers

`cinrs` bundles its own standard headers, written in plain C99 against the
[target model](c-status.md#the-target-model). That is what makes a `c99!` block
self-contained and portable: `sizeof(struct tm)` is the same number wherever the
macro runs, and nothing depends on which libc the developer happens to have
installed.

What the bundled set cannot give is a type whose **layout** only the platform
knows. `struct stat` is whatever the C library says it is; so are `DIR`,
`pthread_mutex_t`, `regex_t`, `struct utsname`, `sigset_t`, `fd_set` and the
real `FILE`. A bundled header that guessed at one of those would not fail to
compile — it would corrupt memory. So they are not guessed at. They are read
from the machine, when a unit asks:

```c
#pragma cinrs system_include

#include <sys/stat.h>
```

## The switch

| Written | Order |
| --- | --- |
| nothing (the default) | including file's directory → `include_path` → `Options::include_paths` → `CINRS_INCLUDE_PATH` → **bundled** |
| `#pragma cinrs system_include` | … → **bundled** → **platform** |
| `#pragma cinrs system_include first` | … → **platform** → **bundled** |

`CINRS_SYSTEM_INCLUDE=1` and `CINRS_SYSTEM_INCLUDE=first` are the same two
settings for a whole crate; `0`, `off`, `false`, `no` and the empty string turn
it off again, and anything else is a diagnostic. A pragma in a unit overrides
the variable. Programmatically it is `Options::system_include`.

The plain form is for a program that wants **one** thing the bundled set does
not carry — `<dirent.h>`, `<pthread.h>`, `<sys/mman.h>` — and keeps cinrs's
plain-C `<stdio.h>`, `<string.h>` and `<math.h>` for everything else. `first`
is for a program that wants the platform's version of a header cinrs *does*
carry, and the reason to want that is nearly always `FILE`: the bundled
`<stdio.h>` declares it as an opaque tag, which is enough to call `fopen` and
`fprintf` and not enough to take its `sizeof` or reach a member.

### A system header's own includes

A header **found** in one of the platform's directories resolves its own
`#include`s from those directories first, whichever mode is in force. Without
that rule the plain mode would hand glibc's `<pthread.h>` cinrs's `<time.h>`,
and the unit would end up with two definitions of `struct timespec` — which is
not a benign redefinition in any revision of C. With it, the platform's header
set stays self-consistent: it is used whole or not at all.

The bundled set is still reachable from inside it, at the end of the chain,
which is what supplies the headers no C library ships because they belong to
the *compiler*: `<stddef.h>`, `<stdarg.h>`, `<float.h>` and `<stdatomic.h>` are
GCC's, not glibc's, and are not in `/usr/include` at all. glibc's headers ask
for them by name and get cinrs's, which is exactly right — one `size_t`, from
one place. `<limits.h>` and `<stdint.h>` exist in both, and there the platform's
answers first and reaches cinrs's with `#include_next`, which is what that
directive is for.

### What is still a hazard

The `typedef`s the two sets share are fine: a redefinition with the same type
is legal C11 ([N1360](c-status.md#c11)), and the bundled `<sys/types.h>` and
the platform's `<sys/stat.h>` really do declare `dev_t`, `ino_t`, `mode_t`,
`nlink_t`, `off_t`, `blkcnt_t` and `blksize_t` one over the other. That is
why the bundled header spells `dev_t` and `ino_t` the way the platform does —
`unsigned long` where that is already 64 bits — since two spellings of one
width are two types and only one of them is benign.

A **tag** is another matter: `struct timespec` defined twice is an error in
every revision of C, and no rule can make it one definition. So in the plain
mode, do not include the bundled `<time.h>` — the one bundled header that
defines tags the platform's set also defines (`struct timespec`, `struct tm`)
— beside a platform header that reaches glibc's. Anything glibc pulls in for
itself is safe, because of the rule above: `<pthread.h>` and `<sys/time.h>`
both get glibc's `<time.h>` and there is only ever one. With `first` there is
nothing to collide at all, since everything comes from one set.

## The directories

`CINRS_SYSTEM_INCLUDE_PATH`, split the way the platform splits `PATH`, replaces
the default entirely. Otherwise:

| Target | Default |
| --- | --- |
| Linux | `/usr/local/include`, `/usr/include/<multiarch>` (only when it exists), `/usr/include` |
| Apple | none — set `CINRS_SYSTEM_INCLUDE_PATH="$(xcrun --show-sdk-path)/usr/include"` |
| Windows and everything else | none — set `CINRS_SYSTEM_INCLUDE_PATH` |
| any target that is not the host | none — see below |

The multiarch name is Debian's `<arch>-linux-<env>`, built from the target
model: `x86_64-linux-gnu`, `i386-linux-gnu`, `aarch64-linux-musl`,
`powerpc64le-linux-gnu`, `s390x-linux-gnu` and so on. Arm's ABI is not part of
the model, so both `arm-linux-gnueabihf` and `arm-linux-gnueabi` are offered
and whichever exists is used.

**GCC's and Clang's private directories are never searched** —
`/usr/lib/gcc/*/include`, `/usr/lib/clang/*/include`. Their `limits.h`,
`stdint.h`, `stddef.h` and `stdarg.h` belong to that compiler: they chain to
the next header of the same name with `#include_next`, and they are written in
terms of builtins only that compiler has. Every one of them is bundled here
anyway, so leaving them out costs nothing and avoids importing another
compiler's idea of itself.

**A cross build gets no default at all.** Those directories hold the headers of
the machine the compiler is *running* on. A `struct stat` laid out for another
architecture is worse than no `struct stat`: the program would compile and read
the wrong bytes. So a unit whose [target](c-status.md#the-target-model) is not
the host, with the switch on and no `CINRS_SYSTEM_INCLUDE_PATH`, is an error
saying so — point the variable at the sysroot's include directories, or leave
the switch off for that target.

## Rebuilds

A user header read by a unit is named by a `const _: &str = include_str!(…);`
in the expansion, so editing it rebuilds the crate. **A system header is
not.** It is part of the machine rather than of the crate; recording
`/usr/include/stdio.h` would rebuild every unit that ever read one whenever the
libc package was upgraded, which is not what that file identifies. Diagnostics
are unaffected — an error inside a system header carries `path:line:col:` and
puts the caret on the `#include`, exactly as for a user header.

## `#include_next`

Implemented with GCC's semantics, because the platform's headers use it: the
search is taken up again at the entry **after** the one the file writing the
directive was found under, so `/usr/include/limits.h` can wrap the *next*
`limits.h` on the path rather than itself. The quoted and the angled forms mean
the same thing, as they do in GCC — the including file's own directory is never
step one of an `#include_next`. Written in the unit's own text there is no
place it was found in, so the search starts at the beginning, exactly as
`#include` does. `__has_include_next` answers the same question and is 0 once
the chain is exhausted.

## The table

Each header below was put through the front end **alone** —
`#pragma cinrs system_include first` and one `#include`, so that the platform's
copy is used even where cinrs bundles one — under `gnu11!` and `c11!`, and
taken through lexing, preprocessing, parsing and sema. Measured against **glibc
2.43** on `x86_64-linux-gnu`; `tests/system_headers.rs` is the test, and it
fails both when one of these regresses and when the one gap closes.

`__GNUC__` is 4.2.1 here (see [Strict and GNU entry
points](gnu-extensions.md#strict-and-gnu-entry-points)), so glibc takes the
same branches it would take for that compiler: no `_Float128`, no
`__builtin_tgmath`, no fortify.

### Feature test macros

A ✓ means the header goes through the front end, not that it declares
everything it can. What it declares is glibc's own business, and glibc decides
it from the **feature test macros** — exactly as it does for GCC.

The strict entry points (`c99!`, `c11!`, `c17!`, `c23!`) define
`__STRICT_ANSI__`, as `gcc -std=c11` does, and glibc then does not turn on
`_DEFAULT_SOURCE`: `<signal.h>` declares `raise` and `signal` and *not*
`sigset_t` or `struct sigaction`, `<sys/types.h>` leaves out the BSD names, and
so on. The GNU entry points (`gnu11!` and its family) do not define it, so
`_DEFAULT_SOURCE` is on and the POSIX and BSD declarations are there. Writing

```c
#define _GNU_SOURCE 1
```

ahead of the first `#include` works in either, and is the usual answer for a
strict unit that wants POSIX.

### C standard headers

| Header | `gnu11!` | `c11!` | Notes |
| --- | --- | --- | --- |
| `<assert.h>` | ✓ | ✓ | |
| `<complex.h>` | ✓ | ✓ | Needs the `complex` feature, which is on by default; without it `_Complex` is a diagnostic and this header is one too. |
| `<ctype.h>` | ✓ | ✓ | The `__ctype_b_loc` table trick and all. |
| `<errno.h>` | ✓ | ✓ | |
| `<fenv.h>` | ✓ | ✓ | |
| `<float.h>` | ✓ | ✓ | The compiler's; not in `/usr/include`, so the bundled one answers. |
| `<inttypes.h>` | ✓ | ✓ | |
| `<iso646.h>` | ✓ | ✓ | |
| `<limits.h>` | ✓ | ✓ | Reaches the bundled `<limits.h>` through `#include_next`, which is the whole reason that directive is implemented. |
| `<locale.h>` | ✓ | ✓ | |
| `<math.h>` | ✓ | ✓ | The `__MATHCALL` macro machinery and `__MATHDECL_ALIAS`. |
| `<setjmp.h>` | ✓ | ✓ | Declaring `jmp_buf` and the functions is fine; *calling* `setjmp` or `longjmp` is refused by name — see [`doc/c-status.md`](c-status.md#c99). |
| `<signal.h>` | ✓ | ✓ | `sigset_t` and `struct sigaction`, which the bundled header does not carry — but in `c11!` only with a feature test macro; see below. |
| `<stdalign.h>` | ✓ | ✓ | |
| `<stdarg.h>` | ✓ | ✓ | Not in `/usr/include`: the bundled one answers, as it does for a real compiler. |
| `<stdatomic.h>` | ✓ | ✓ | Likewise — glibc ships none, it is GCC's, and the bundled one answers. |
| `<stdbool.h>` | ✓ | ✓ | |
| `<stddef.h>` | ✓ | ✓ | The compiler's; bundled. |
| `<stdint.h>` | ✓ | ✓ | |
| `<stdio.h>` | ✓ | ✓ | The real `struct _IO_FILE`, so `sizeof(FILE)` is a number. |
| `<stdlib.h>` | ✓ | ✓ | |
| `<stdnoreturn.h>` | ✓ | ✓ | |
| `<string.h>` | ✓ | ✓ | |
| `<tgmath.h>` | ✗ | ✗ | **The one gap.** glibc's own `#error "Unsupported combination of types for <tgmath.h>."`: on x86-64 it makes `__HAVE_FLOAT64X` unconditional but ties `__HAVE_FLOAT128` to `__GNUC_PREREQ (4, 3)`, and refuses the combination. See below. |
| `<threads.h>` | ✓ | ✓ | |
| `<time.h>` | ✓ | ✓ | |
| `<uchar.h>` | ✓ | ✓ | |
| `<wchar.h>` | ✓ | ✓ | |
| `<wctype.h>` | ✓ | ✓ | |

### POSIX headers

| Header | `gnu11!` | `c11!` | Header | `gnu11!` | `c11!` |
| --- | --- | --- | --- | --- | --- |
| `<alloca.h>` | ✓ | ✓ | `<sched.h>` | ✓ | ✓ |
| `<arpa/inet.h>` | ✓ | ✓ | `<semaphore.h>` | ✓ | ✓ |
| `<dirent.h>` | ✓ | ✓ | `<spawn.h>` | ✓ | ✓ |
| `<dlfcn.h>` | ✓ | ✓ | `<strings.h>` | ✓ | ✓ |
| `<fcntl.h>` | ✓ | ✓ | `<sys/ioctl.h>` | ✓ | ✓ |
| `<fnmatch.h>` | ✓ | ✓ | `<sys/mman.h>` | ✓ | ✓ |
| `<getopt.h>` | ✓ | ✓ | `<sys/resource.h>` | ✓ | ✓ |
| `<glob.h>` | ✓ | ✓ | `<sys/select.h>` | ✓ | ✓ |
| `<grp.h>` | ✓ | ✓ | `<sys/socket.h>` | ✓ | ✓ |
| `<iconv.h>` | ✓ | ✓ | `<sys/stat.h>` | ✓ | ✓ |
| `<langinfo.h>` | ✓ | ✓ | `<sys/time.h>` | ✓ | ✓ |
| `<libgen.h>` | ✓ | ✓ | `<sys/types.h>` | ✓ | ✓ |
| `<netdb.h>` | ✓ | ✓ | `<sys/uio.h>` | ✓ | ✓ |
| `<netinet/in.h>` | ✓ | ✓ | `<sys/un.h>` | ✓ | ✓ |
| `<poll.h>` | ✓ | ✓ | `<sys/utsname.h>` | ✓ | ✓ |
| `<pthread.h>` | ✓ | ✓ | `<sys/wait.h>` | ✓ | ✓ |
| `<pwd.h>` | ✓ | ✓ | `<syslog.h>` | ✓ | ✓ |
| `<regex.h>` | ✓ | ✓ | `<termios.h>` | ✓ | ✓ |
| | | | `<unistd.h>` | ✓ | ✓ |
| | | | `<utime.h>` | ✓ | ✓ |

Sixty-six of the sixty-seven, in both entry points.

### The gap: `<tgmath.h>`

glibc's `<tgmath.h>` opens with

```c
# if ((__HAVE_FLOAT64X && !__HAVE_FLOAT128) || (__HAVE_FLOAT128 && !__HAVE_FLOAT64X))
#  error "Unsupported combination of types for <tgmath.h>."
# endif
```

and on x86-64 `<bits/floatn.h>` sets `__HAVE_FLOAT64X` to 1 unconditionally and
`__HAVE_FLOAT128` to `__GNUC_PREREQ (4, 3)`. cinrs claims `__GNUC__` 4.2.1 —
Clang's precedent, and the version every `__attribute__` guard in the wild
tests against — so the second is 0 and the `#error` fires.

Claiming 4.3 instead was tried, and costs far more than it buys:
`<bits/floatn.h>` then declares `__float128` and `_Complex __float128`
unconditionally, which takes `<stdio.h>`, `<stdlib.h>`, `<math.h>` and
`<complex.h>` down with it — the extended floating types are
[refused](#extended-floating-types) rather than mapped onto `double`. One
header lost is the cheaper trade, and it is the one header whose entire content
is macros that need a compiler builtin (`__builtin_tgmath`, GCC ≥ 8) or a
`__builtin_classify_type` chain over types this front end does not have. The
bundled set has no `<tgmath.h>` either.

### Extended floating types

`__float128`, `_Float128`, `_Float128x`, `_Float64x`, `_Float16`, `__fp16`,
`__bf16`, `_Float32`, `_Float32x` and `_Float64` are **refused with the
reason** wherever they appear as a type specifier, rather than mapped onto
`double`. Rust has two stable floating types, `f32` and `f64`; `f16` and `f128`
are unstable, and the 80-bit `_Float64x` an x86 `long double` really is has no
Rust type at all. Mapping any of them onto `double` would compute and pass the
wrong values. Recognising the names is what turns what would be a "type
specifier missing" cascade into one clear message.

With `__GNUC__` at 4.2.1 no glibc header declares one, so this does not come up
on the platform above; it is here because it is the rule the trade above is
made against.

## What the headers forced

The sweep did not pass on the first try. What the platform's headers needed,
beyond the search rules:

* **`#include_next`**, which was refused outright before, on the grounds that
  there was nothing to reach. `/usr/include/limits.h` ends with one.
* **`__attribute__((weak))` on a declaration.** glibc's `<pthread.h>` writes it
  on `__pthread_unwind_next`. It is now refused on a *definition* only; see the
  [attribute table](gnu-extensions.md#attributes) for why
  `__has_attribute(weak)` still answers 0.
* **A clean refusal for the extended floating types**, as above.
* **A refusal on `setjmp`/`longjmp` calls by name.** The bundled `<setjmp.h>`
  is an `#error`, so before this the refusal came from the header; the
  platform's declares them as ordinary functions, and a program that called one
  would have compiled and then corrupted itself.
* **`dev_t` and `ino_t` in the bundled `<sys/types.h>`**, which were
  `unsigned long long` on a 64-bit target where glibc and musl both spell them
  `unsigned long`. Same width, different type — and a typedef redefinition is
  benign only when the type is the *same* one, so the two sets could not be
  mixed until this was right.

Everything else glibc leans on was already there:

* `__attribute__` in every position GCC accepts it and in both spellings, with
  the ones that cannot be honoured ignored as C23 6.7.13.1p3 allows —
  `__nonnull__`, `__pure__`, `__const__`, `__malloc__`, `__leaf__`,
  `__nothrow__`, `__access__`, `__alloc_size__`, `__returns_nonnull__`,
  `__format__`, `__gnu_inline__`, `__artificial__`, `__always_inline__`,
  `__visibility__`, `__warn_unused_result__`, `__deprecated__` with a message;
* `__asm__` **labels**, and the two adjacent string literals `__REDIRECT`
  builds one out of (`__asm__ ("" "tmpfile64")`) — `<stdio.h>`, `<dirent.h>`
  and `<semaphore.h>` all use it, and `_FILE_OFFSET_BITS=64` turns most of the
  other declarations in `<sys/stat.h>` and `<stdio.h>` into more of them;
* `__extension__` in odd positions, `__typeof__`, `__inline` and
  `__extern_inline`, `_Static_assert` (`<assert.h>` writes one), `_Complex`
  (`<complex.h>`), and `_Noreturn` in every spelling;
* `__has_attribute` and `__has_builtin`, which `__glibc_has_attribute` and
  `<assert.h>` ask, answered from [this crate's own
  tables](../crates/cinrs-core/src/gnu.rs) so that glibc is told the truth
  about *this* implementation rather than about GCC;
* `_Pragma`, which is what `__glibc_macro_warning` is made of.

## The programs

`tests/system_headers.rs` also compiles and runs ordinary programs against
these declarations, linking to the real library: `stat` reading `st_size`,
`opendir`/`readdir` counting a directory, two threads sharing a
`pthread_mutex_t` through `pthread_create`/`pthread_join`, `socketpair` with
`write`/`read`, `getopt` over a fake `argv`, `regcomp`/`regexec`,
`dlopen(NULL)`/`dlsym("strlen")`, `gettimeofday`, `uname`, `mmap`/`munmap`, and
`fopen`/`fprintf`/`fread`/`fclose` through the platform's `<stdio.h>` with the
`stdin`/`stdout`/`stderr` externs.

The layouts those headers describe are compared against the host's own C
compiler, which is the only oracle there is for a `struct stat`:
`sizeof(struct stat)`, `offsetof(struct stat, st_size)`,
`sizeof(pthread_mutex_t)`, `sizeof(struct dirent)`,
`offsetof(struct dirent, d_name)`, `sizeof(struct timeval)`,
`sizeof(struct utsname)` and `sizeof(FILE)`. On `x86_64-linux-gnu` with glibc
2.43 that is 144, 48, 40, 280, 19, 16, 390 and 216 — computed by cinrs's own
layout engine from the same header text, and equal to `cc`'s.

The `typedef`s the two header sets *both* declare are compared the same way —
`dev_t`, `ino_t`, `mode_t`, `nlink_t`, `off_t`, `blkcnt_t`, `size_t`, `ssize_t`
— which is what turns "the unit compiled, so the redefinitions must have been
benign" into a number. (8, 8, 4, 8, 8, 8, 8, 8.)

## Skipping

The sweep prints why and passes unless the host is Linux with glibc and
`/usr/include/stdio.h` exists. The programs are compiled while the *macro*
expands and so cannot decide anything at run time: they are `#[cfg]`-gated on a
glibc Linux target, where the headers come from the same package (`libc6-dev`)
as the object files `rustc` already needs in order to link anything at all. The
layout comparison additionally needs a C compiler on `PATH`
(`CINRS_SYSTEM_HEADER_CC`, then `CC`, then `cc`) and says so and passes without
one.
