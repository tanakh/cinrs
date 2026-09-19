# cinrs-macros

An implementation detail of [`cinrs`](https://crates.io/crates/cinrs) — the
crate that lets you write C inside Rust. **Use that crate.**

This one holds the procedural macros themselves — `c99!`, `gnu11!`,
`include_c99!` and the rest of the entry points — because a `proc-macro = true`
crate can export nothing else. `cinrs` re-exports every one of them, and the
code they generate names `::cinrs`, so invoking them from here directly is not
supported. **There is no stability promise for this API**: it is versioned with
`cinrs` and follows it.

Documentation: <https://docs.rs/cinrs>.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
