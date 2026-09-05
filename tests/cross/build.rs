//! Tells the procedural macro what machine the crate is being built for.
//!
//! This is the whole of the recipe a real crate needs, and the one the README
//! prints: `TARGET` is the triple Cargo is building for, and `cargo:rustc-env`
//! puts a variable into the environment of the very `rustc` process that will
//! run `c11!` — which is the only channel a procedural macro has, since
//! nothing in its own world says what `--target` was given.
//!
//! `CINRS_CROSS_EXPORT=0` switches it off, so that the harness can compile the
//! same crate the way a project that *forgot* the build script would and watch
//! the data-model assertion fail. A real crate has no such knob.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CINRS_CROSS_EXPORT");
    if std::env::var("CINRS_CROSS_EXPORT").as_deref() == Ok("0") {
        return;
    }
    let target = std::env::var("TARGET").expect("Cargo sets TARGET for a build script");
    println!("cargo:rustc-env=CINRS_TARGET={target}");
}
