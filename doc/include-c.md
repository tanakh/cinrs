# Including a C file

C that already lives in a file goes in whole, with one macro per entry point:
`include_c89!`, `include_c90!`, `include_c99!`, `include_c11!`, `include_c17!`,
`include_c23!` and the five `include_gnu…!` forms.

```rust,ignore
cinrs::include_c99!("c/geometry.c");

let p = Point { x: -3, y: 4 };
assert_eq!(point_manhattan(p), 7);
```

The file is read while the macro expands and translated exactly as the same
text inside a `c99!` block would be: it is one translation unit, so every
construct is accepted (a file is text, and the lexemes Rust's own lexer refuses
are no trouble there), `#pragma cinrs …` inside it configures the unit,
`#include "…"` in it searches **its own** directory first, and the expansion is
a module plus a glob re-export like any other invocation's. `__FILE__` and
`__LINE__` name the `.c` file and its own lines, and the file is mentioned with
`include_str!` in the expansion, so editing it rebuilds the crate.

A **relative path is resolved against the directory of the `.rs` file the macro
is written in** — the same rule `#include "…"` follows — and an absolute one is
used as it stands.

The one thing that is not as good as writing the C inline is where a diagnostic
can point. There is no C in the `.rs` file, so **every error lands on the macro
invocation**: a message of this crate's carries the position inside the file in
its text —

```text
error: c/geometry.c:12:5: use of undeclared identifier 'wrong'
 --> src/lib.rs:3:21
  |
3 | cinrs::include_c99!("c/geometry.c");
  |                     ^^^^^^^^^^^^^^
```

— and so does an error `rustc` raises about the generated code. Neither `cargo`
nor `rust-analyzer` will jump into the `.c` file: they show the location in the
message rather than under the caret. A call written in *Rust* is unaffected —
the caret is on the call, where it always was.
