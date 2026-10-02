# ccinrs

A C compiler with GCC's command line, built on [`cinrs`](https://crates.io/crates/cinrs).

```text
cargo install ccinrs
ccinrs -O2 -o hello hello.c
```

Every C file is translated to Rust by the same front end the `c99!` family
of macros runs, and `rustc` compiles that Rust and links the program — no C
compiler is involved, and anything `cargo build` can link, `ccinrs` can too.

What it accepts and what it does not is documented in
[`doc/ccinrs.md`](https://github.com/tanakh/cinrs/blob/master/doc/ccinrs.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
