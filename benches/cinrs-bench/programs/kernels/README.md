# Kernels written for this suite

Every `.c` file in this directory was written for `cinrs-bench` and is licensed
`MIT OR Apache-2.0`, the same as the crate. Nothing here is vendored, so there
is no `SOURCES.md`: the provenance is this repository.

Each one is a whole C program that

* takes its size from `argv`, so the harness can tune it without editing the
  source,
* is deterministic — fixed seeds, no clocks, no addresses printed,
* prints one line ending in a checksum, so that the `gcc`, `clang` and `cinrs`
  builds can be compared byte for byte, and
* isolates **one** thing, named in the comment at the top of the file.

They come in three kinds.

## Ordinary code, as a baseline

`sieve`, `recursion`, `nqueens`, `matmul`, `sort`, `binsearch`, `fft`, `crc32`,
`sha256`, `life`, `levenshtein`, `hashtable`, `chase`. Loops any code
generator has to get right. If these are not close to `gcc -O2` then nothing
else in the report matters.

`chase` and `binsearch` are deliberately memory-bound and `libc_str` is
deliberately a call into libc: they are controls, where all three builds should
land on the same time because the compiler is barely involved.

## Pairs, where the difference is the measurement

| pair | the difference |
| --- | --- |
| `matmul` / `matmul_vla` | `a[i * n + k]` written out, against the C99 parameter form `double a[n][n]` whose row stride is a run-time value. They print the same line. |
| `vla` / `vla_hoisted` | a variable length array made afresh every iteration, against one `malloc` outside the loop. Rust cannot move the stack pointer by a run-time amount, so a VLA is a `Vec`; this pair is what that costs. They print the same line. |
| `interp_switch` / `interp_goto` | the same bytecode machine dispatched through a `switch` with a fallthrough case, and through GCC's computed `goto`. They print the same line. |
| `libc_str` / `hand_str` | the platform's `strlen`/`strcmp`/`memcpy`, against the same routines written out in C so the compiler has to optimise them itself. |
| `statemachine` / `statemachine_structured` | the same lexer written as a dozen labels and `goto`s, and written with `while` and `switch`. `cinrs` lowers a function that jumps into a `loop`/`match` over basic blocks; the difference between these two rows is what that lowering costs, and it is the largest single number in the report. They print the same line. |

## The constructs `cinrs` lowers unusually

* `bitfields` — a bit-field has no address, so it is not a field of the
  generated Rust `struct` at all: a run of them shares one `[u8; K]` and each
  member is a pair of inherent methods. Every read and write in this kernel is
  therefore a call that has to be inlined back into a shift and a mask.
* `vla` — heap-emulated variable length arrays, as above.
* `statemachine` — a function that jumps is lowered into a `loop`/`match` over
  basic blocks; this kernel is already a state machine, so it ends up as one
  inside another.
* `interp_goto` — labels as values, which fall out of that same lowering.
* `structval` — small `struct`s by value, across both halves of the x86-64
  System V classification (two doubles in registers, three longs in memory).
* `complexmandel` — `double _Complex`, which is the one type the expansion
  names a crate for (`cinrs::rt::Complex`, i.e. `num_complex`).
* `wrapping` — C's modular unsigned arithmetic, which becomes `wrapping_add`
  and friends.
* `divide` — Rust's `/` and `%` check for a zero divisor, and signed division
  also has to rule out `INT_MIN / -1`; C's do neither.

## A note on undefined behaviour

None of these kernels relies on signed overflow, and the accumulators that are
*meant* to wrap are all `unsigned`. That is not fastidiousness: `gcc` and
`clang` are entitled to assume signed overflow does not happen and `cinrs` is
not, so a kernel that overflowed a `long` would make the three builds disagree
about the answer and the harness would report a mismatch that was the
benchmark's fault rather than the compiler's.
