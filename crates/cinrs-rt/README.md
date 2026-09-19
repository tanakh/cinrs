# cinrs-rt

An implementation detail of [`cinrs`](https://crates.io/crates/cinrs) — the
crate that lets you write C inside Rust. **Use that crate**, which re-exports
this one as `cinrs::rt` under its default `complex` feature.

There is exactly one thing in here: C's complex arithmetic. `float _Complex`
and `double _Complex` are [`num_complex::Complex`]`<f32>` and `Complex<f64>`,
and the multiplication and division C99 Annex G.5.1 describes — with the
infinity recovery the naive formulas get wrong — live here rather than being
written into every macro expansion. The crate is `#![no_std]`, so having it
costs a `#![no_std]` crate nothing. **There is no stability promise for this
API**: it is versioned with `cinrs` and follows it.

Documentation: <https://docs.rs/cinrs-rt>.

[`num_complex::Complex`]: https://docs.rs/num-complex

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
