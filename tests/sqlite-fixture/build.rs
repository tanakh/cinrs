//! Where `--features native-gcc` / `native-clang` find the libraries that
//! `scripts/check-sqlite.sh --bench` builds: `target/sqlite/native/`.
//!
//! `#pragma cinrs link "sqlite3_gcc"` in `src/lib.rs` names the library; this
//! only says where it is. Without either feature it prints nothing, so the
//! ordinary build is what it was.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let gcc = std::env::var_os("CARGO_FEATURE_NATIVE_GCC").is_some();
    let clang = std::env::var_os("CARGO_FEATURE_NATIVE_CLANG").is_some();
    if !(gcc || clang) {
        return;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sqlite/native");
    println!("cargo:rustc-link-search=native={}", dir.display());
    for (on, name) in [(gcc, "gcc"), (clang, "clang")] {
        if on {
            println!("cargo:rerun-if-changed={}/libsqlite3_{name}.a", dir.display());
        }
    }
}
