# cinrs-core

An implementation detail of [`cinrs`](https://crates.io/crates/cinrs) — the
crate that lets you write C inside Rust. **Use that crate.**

This one is the C front end itself: the lexer, the preprocessor, the parser,
the semantic analysis and the code generator, driven as a library over a
`proc_macro2::TokenStream`, plus the C standard and POSIX headers `cinrs`
bundles. It is published because `cinrs-macros` depends on it, and because a
tool that wants the front end without the procedural macro can use it — but
**there is no stability promise for this API**. It is versioned with `cinrs`
and changes whenever that is convenient for `cinrs`, in a patch release as
readily as in a minor one.

Documentation: <https://docs.rs/cinrs-core>. What the front end accepts, and
the whole of the language it translates, is documented in
[`cinrs`](https://docs.rs/cinrs) instead.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
