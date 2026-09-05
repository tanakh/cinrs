//! `#include` resolution: where a header is looked for, and what is bundled.
//!
//! # Search order
//!
//! `#include "name"` looks in
//!
//! 1. the directory of the file the directive is written in — for the macro's
//!    own text that is the directory of the invoking `.rs` file, and for a
//!    header it is the directory that header was found in;
//! 2. the configured include directories, in the order [`SearchPaths`]
//!    describes;
//! 3. the working directory, but only when the name *is* a path — when it
//!    holds a directory separator. That is what makes `#include __FILE__`
//!    work: [`Resolved::name`] is written relative to the working directory
//!    wherever it can be (a diagnostic naming an absolute path is a
//!    diagnostic that differs between two machines), so a header that
//!    includes itself by `__FILE__` is asking for
//!    `some/dir/thing.h` from a directive written *in* `some/dir`, which
//!    neither of the first two steps will find. A bare name is deliberately
//!    left out of this step, so that a `stdio.h` sitting in the working
//!    directory never shadows the bundled one;
//! 4. the [bundled headers](bundled).
//!
//! `#include <name>` skips steps 1 and 3. A name that is absolute is used as
//! it stands.
//!
//! # What is deliberately *not* searched
//!
//! The system directories — `/usr/include` and friends — are never looked in,
//! on any platform. A real `<stdio.h>` is not C: glibc's is a thicket of
//! `__attribute__`, `__extension__`, `__asm__` renaming, `_Float128` and
//! compiler builtins, and musl's and Apple's are only a little tamer. A front
//! end that read them would have to become GCC to survive them, and would
//! break every time libc changed. So `cinrs` ships its own small, plain-C99
//! declarations of the standard library instead: they declare exactly what the
//! platform's real library exports, the linker binds the calls to the real
//! implementation, and the C that uses them is ordinary C. A program that
//! needs a system header cinrs does not bundle can still point at one with
//! `#pragma cinrs include_path`, but it does so knowingly.

use std::path::{Path, PathBuf};

/// The bundled standard headers, as `(name, text)` pairs.
///
/// They are compiled into the crate rather than installed anywhere, so no part
/// of a build depends on where `cinrs` itself lives on disk.
pub const BUNDLED: &[(&str, &str)] = &[
    ("alloca.h", include_str!("../include/alloca.h")),
    ("assert.h", include_str!("../include/assert.h")),
    ("ctype.h", include_str!("../include/ctype.h")),
    ("errno.h", include_str!("../include/errno.h")),
    ("float.h", include_str!("../include/float.h")),
    ("inttypes.h", include_str!("../include/inttypes.h")),
    ("iso646.h", include_str!("../include/iso646.h")),
    ("limits.h", include_str!("../include/limits.h")),
    ("math.h", include_str!("../include/math.h")),
    ("setjmp.h", include_str!("../include/setjmp.h")),
    ("stdalign.h", include_str!("../include/stdalign.h")),
    ("stdarg.h", include_str!("../include/stdarg.h")),
    ("stdbool.h", include_str!("../include/stdbool.h")),
    ("stdckdint.h", include_str!("../include/stdckdint.h")),
    ("stddef.h", include_str!("../include/stddef.h")),
    ("stdint.h", include_str!("../include/stdint.h")),
    ("stdio.h", include_str!("../include/stdio.h")),
    ("stdlib.h", include_str!("../include/stdlib.h")),
    ("stdnoreturn.h", include_str!("../include/stdnoreturn.h")),
    ("string.h", include_str!("../include/string.h")),
    ("time.h", include_str!("../include/time.h")),
    ("uchar.h", include_str!("../include/uchar.h")),
    ("wchar.h", include_str!("../include/wchar.h")),
    ("wctype.h", include_str!("../include/wctype.h")),
];

/// The directory the bundled headers appear to live in.
///
/// It is not a directory at all — the headers are strings inside this crate —
/// but a diagnostic has to name the file it is talking about, and
/// `<cinrs>/stdio.h` says both which header it is and that it is ours. The
/// angle brackets keep it from being mistaken for a path that exists.
pub const BUNDLED_DIR: &str = "<cinrs>";

/// The text of a bundled header, by name.
pub fn bundled(name: &str) -> Option<&'static str> {
    BUNDLED
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| *text)
}

/// The display name a bundled header is known by: `<cinrs>/stdio.h`.
pub fn bundled_name(name: &str) -> String {
    format!("{BUNDLED_DIR}/{name}")
}

/// How the header name was spelled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// `#include "name"`, which searches the including file's directory first.
    Quoted,
    /// `#include <name>`, which does not.
    Angled,
}

/// The directory a file's own `#include "…"` searches first.
#[derive(Clone, Debug, Default)]
pub enum Origin {
    /// A directory on disk. Empty means the current directory, which is what
    /// the parent of a bare `lib.rs` comes out as.
    Dir(PathBuf),
    /// The bundled set: one bundled header including another finds it there.
    Bundled,
    /// Nowhere — the compiler would not say where the including file is.
    #[default]
    Unknown,
}

/// The include directories a unit searches, in the order it searches them.
///
/// The three lists are kept apart so that the order is a decision rather than
/// an accident: what the unit itself asks for wins over what the build asked
/// for, which wins over what the environment asked for.
#[derive(Clone, Debug, Default)]
pub struct SearchPaths {
    /// Directories from `#pragma cinrs include_path`, in the order written.
    pragma: Vec<PathBuf>,
    /// Directories from [`crate::Options::include_paths`].
    options: Vec<PathBuf>,
    /// Directories from the `CINRS_INCLUDE_PATH` environment variable.
    env: Vec<PathBuf>,
}

/// The environment variable holding a global list of include directories.
///
/// Split the way the platform splits `PATH`: with `:` on Unix and `;` on
/// Windows, through [`std::env::split_paths`].
pub const ENV_VAR: &str = "CINRS_INCLUDE_PATH";

/// The environment variable Cargo sets to the package's own directory, which
/// is what a relative `#pragma cinrs include_path` is resolved against.
pub const MANIFEST_DIR_VAR: &str = "CARGO_MANIFEST_DIR";

impl SearchPaths {
    /// The directories configured before preprocessing starts.
    pub fn new(options: &[PathBuf]) -> Self {
        Self {
            pragma: Vec::new(),
            options: options.to_vec(),
            env: std::env::var_os(ENV_VAR)
                .map(|value| std::env::split_paths(&value).collect())
                .unwrap_or_default(),
        }
    }

    /// Adds a directory named by `#pragma cinrs include_path`.
    ///
    /// A relative path is resolved against `CARGO_MANIFEST_DIR` — the package
    /// being compiled, which the procedural macro reads from the environment
    /// of the `rustc` process Cargo started — so that a `c99!` block means the
    /// same thing however the build was invoked. Without that variable (a unit
    /// test, a hand-rolled `rustc`) the path is left as it stands, and is then
    /// relative to the working directory.
    pub fn add_pragma(&mut self, dir: &str) {
        let path = Path::new(dir);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            match std::env::var_os(MANIFEST_DIR_VAR) {
                Some(root) => Path::new(&root).join(path),
                None => path.to_path_buf(),
            }
        };
        if !self.pragma.contains(&resolved) {
            self.pragma.push(resolved);
        }
    }

    /// Every configured directory, in search order.
    fn dirs(&self) -> impl Iterator<Item = &PathBuf> {
        self.pragma
            .iter()
            .chain(self.options.iter())
            .chain(self.env.iter())
    }
}

/// A header that was found.
#[derive(Clone, Debug)]
pub struct Resolved {
    /// The name diagnostics call it: a path for a file on disk, and
    /// `<cinrs>/stdio.h` for a bundled header.
    pub name: String,
    /// Its contents.
    pub text: String,
    /// Where its own `#include "…"` looks first.
    pub origin: Origin,
    /// What identifies it for `#pragma once` and the include-guard
    /// optimisation: the canonical path of the file, or the bundled name.
    pub key: String,
    /// The absolute path of the file, for rebuild tracking. `None` for a
    /// bundled header, which cannot change without the crate changing.
    pub path: Option<PathBuf>,
}

/// Why a header could not be included.
#[derive(Clone, Debug)]
pub enum Error {
    /// Nothing of that name is anywhere the unit searches.
    NotFound {
        /// The directories that were looked in, in order, as a reader can
        /// check them.
        searched: Vec<String>,
    },
    /// A file of that name is there, but could not be read: no permission, not
    /// UTF-8, gone between the test and the read.
    Unreadable {
        /// The file that could not be read.
        path: String,
        /// What the operating system said about it.
        error: String,
    },
}

/// Looks a header name up.
pub fn resolve(
    name: &str,
    form: Form,
    origin: &Origin,
    paths: &SearchPaths,
) -> Result<Resolved, Error> {
    let mut searched: Vec<String> = Vec::new();

    // An absolute name is not searched for: it either is the file or it is
    // nothing.
    if Path::new(name).is_absolute() {
        return match read_file(Path::new(name))? {
            Some(found) => Ok(found),
            None => Err(Error::NotFound {
                searched: vec![display_path(Path::new(name))],
            }),
        };
    }

    if form == Form::Quoted {
        match origin {
            Origin::Dir(dir) => {
                if let Some(found) = read_file(&dir.join(name))? {
                    return Ok(found);
                }
                searched.push(display_dir(dir));
            }
            Origin::Bundled => {
                if let Some(found) = read_bundled(name) {
                    return Ok(found);
                }
                searched.push(BUNDLED_DIR.to_owned());
            }
            Origin::Unknown => {}
        }
    }

    for dir in paths.dirs() {
        if let Some(found) = read_file(&dir.join(name))? {
            return Ok(found);
        }
        let shown = display_dir(dir);
        if !searched.contains(&shown) {
            searched.push(shown);
        }
    }

    // A name that is itself a path, taken from the working directory. This is
    // what `#include __FILE__` needs: the name a header is known by is
    // relative to the working directory, so a header that includes itself
    // asks for `some/dir/thing.h` from inside `some/dir`, which neither the
    // origin nor a `-I` will resolve. A bare header name is not looked for
    // here, so nothing in the working directory can shadow a bundled header.
    if form == Form::Quoted && is_path(name) {
        if let Some(found) = read_file(Path::new(name))? {
            return Ok(found);
        }
        let shown = display_dir(Path::new(""));
        if !searched.contains(&shown) {
            searched.push(shown);
        }
    }

    if let Some(found) = read_bundled(name) {
        return Ok(found);
    }
    if !searched.iter().any(|d| d == BUNDLED_DIR) {
        searched.push(BUNDLED_DIR.to_owned());
    }
    Err(Error::NotFound { searched })
}

/// A resource `#embed` found.
#[derive(Clone, Debug)]
pub struct Embedded {
    /// The name diagnostics call it.
    pub name: String,
    /// Its contents, byte for byte.
    pub bytes: Vec<u8>,
    /// Its absolute path, for rebuild tracking.
    pub path: PathBuf,
}

/// Looks an `#embed` resource up (C23 6.10.3).
///
/// The search is `#include`'s with the two steps that are about *headers* left
/// out: there are no bundled resources — a picture is not a declaration — and
/// nothing is read from the working directory by a bare name. What is left is
/// the directory of the file the directive is written in, for the quoted form,
/// and then the include path.
pub fn resolve_embed(
    name: &str,
    form: Form,
    origin: &Origin,
    paths: &SearchPaths,
) -> Result<Embedded, Error> {
    let mut searched: Vec<String> = Vec::new();

    if Path::new(name).is_absolute() {
        return match read_bytes(Path::new(name))? {
            Some(found) => Ok(found),
            None => Err(Error::NotFound {
                searched: vec![display_path(Path::new(name))],
            }),
        };
    }

    if form == Form::Quoted {
        match origin {
            Origin::Dir(dir) => {
                if let Some(found) = read_bytes(&dir.join(name))? {
                    return Ok(found);
                }
                searched.push(display_dir(dir));
            }
            // A bundled header's `#embed` has nowhere of its own to look.
            Origin::Bundled | Origin::Unknown => {}
        }
    }

    for dir in paths.dirs() {
        if let Some(found) = read_bytes(&dir.join(name))? {
            return Ok(found);
        }
        let shown = display_dir(dir);
        if !searched.contains(&shown) {
            searched.push(shown);
        }
    }

    // As for a header, a name that is itself a path is looked for from the
    // working directory, so that a resource named relative to it can be found
    // from a directive written elsewhere.
    if form == Form::Quoted && is_path(name) {
        if let Some(found) = read_bytes(Path::new(name))? {
            return Ok(found);
        }
        let shown = display_dir(Path::new(""));
        if !searched.contains(&shown) {
            searched.push(shown);
        }
    }

    Err(Error::NotFound { searched })
}

/// Reads a candidate resource, with [`read_file`]'s convention: `Ok(None)`
/// means there is no such file and the search goes on.
fn read_bytes(path: &Path) -> Result<Option<Embedded>, Error> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {}
        _ => return Ok(None),
    }
    let bytes = std::fs::read(path).map_err(|error| Error::Unreadable {
        path: display_path(path),
        error: error.to_string(),
    })?;
    Ok(Some(Embedded {
        name: display_path(path),
        bytes,
        path: std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()),
    }))
}

/// Whether a header name names a directory as well as a file.
///
/// Both separators count on every platform: a `#include "sub/thing.h"` is
/// written with a forward slash in portable C whatever the host does with
/// them.
fn is_path(name: &str) -> bool {
    name.contains('/') || name.contains('\\')
}

fn read_bundled(name: &str) -> Option<Resolved> {
    let text = bundled(name)?;
    let display = bundled_name(name);
    Some(Resolved {
        text: text.to_owned(),
        origin: Origin::Bundled,
        key: display.clone(),
        name: display,
        path: None,
    })
}

/// Reads a candidate path: `Ok(None)` when there is no such file, which means
/// the search goes on, and an error when there is one and it cannot be used,
/// which means it does not.
fn read_file(path: &Path) -> Result<Option<Resolved>, Error> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {}
        // A directory of that name, or nothing at all: keep looking.
        _ => return Ok(None),
    }
    let text = std::fs::read_to_string(path).map_err(|error| Error::Unreadable {
        path: display_path(path),
        error: error.to_string(),
    })?;
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let key = std::fs::canonicalize(path)
        .unwrap_or_else(|_| absolute.clone())
        .display()
        .to_string();
    Ok(Some(Resolved {
        name: display_path(path),
        text,
        origin: Origin::Dir(path.parent().unwrap_or(Path::new("")).to_path_buf()),
        key,
        path: Some(absolute),
    }))
}

/// How a path is written in a diagnostic.
///
/// Relative wherever it can be — a message naming
/// `/home/someone/crate/include/foo.h` is a message that differs between two
/// machines, which makes it useless in a test and noisy everywhere else — so a
/// path inside the working directory is shown relative to it.
pub fn display_path(path: &Path) -> String {
    let relative = std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(&cwd).ok().map(Path::to_path_buf));
    relative
        .unwrap_or_else(|| path.to_path_buf())
        .display()
        .to_string()
}

/// How a directory is written in the list a "not found" message shows.
fn display_dir(dir: &Path) -> String {
    let shown = display_path(dir);
    if shown.is_empty() {
        // `Path::parent` of a bare file name; the working directory.
        ".".to_owned()
    } else {
        shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_header_is_listed_once_and_sorted() {
        let mut names: Vec<&str> = BUNDLED.iter().map(|(n, _)| *n).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "a header is listed twice");
        let listed: Vec<&str> = BUNDLED.iter().map(|(n, _)| *n).collect();
        assert_eq!(listed, names, "keep the table sorted by name");
    }

    #[test]
    fn a_bundled_header_is_found_without_touching_the_disk() {
        let found = resolve(
            "stddef.h",
            Form::Angled,
            &Origin::Unknown,
            &SearchPaths::default(),
        )
        .expect("stddef.h is bundled");
        assert_eq!(found.name, "<cinrs>/stddef.h");
        assert!(found.path.is_none());
        assert!(found.text.contains("size_t"));
    }

    #[test]
    fn a_quoted_name_that_is_a_path_is_taken_from_the_working_directory() {
        // What `#include __FILE__` in a header comes to: the name is a path
        // relative to the working directory, and the directive is written in
        // the directory that path leads to, so neither the origin nor a `-I`
        // resolves it.
        // `cargo test` runs with the package directory as the working
        // directory, and `include/` is where the bundled headers live as
        // files.
        let found = resolve(
            "include/stddef.h",
            Form::Quoted,
            &Origin::Dir(PathBuf::from("include")),
            &SearchPaths::default(),
        )
        .expect("the file is there, relative to the package directory");
        assert!(found.text.contains("size_t"));
        assert!(
            found.path.is_some(),
            "it is a real file, not the bundled one"
        );
    }

    #[test]
    fn a_bare_name_in_the_working_directory_does_not_shadow_a_bundled_header() {
        // `Cargo.toml` is in the working directory and is not a header, but
        // the point is the rule: a name with no separator is never looked for
        // there, so the step cannot take a `stdio.h` somebody left lying
        // about in preference to ours.
        assert!(!is_path("stdio.h"));
        assert!(is_path("sub/stdio.h"));
        assert!(is_path("sub\\stdio.h"));
    }

    #[test]
    fn a_missing_header_lists_where_it_looked() {
        let error = resolve(
            "nowhere.h",
            Form::Quoted,
            &Origin::Dir(PathBuf::from("src")),
            &SearchPaths::default(),
        )
        .expect_err("nothing is called nowhere.h");
        match error {
            Error::NotFound { searched } => assert_eq!(searched, ["src", "<cinrs>"]),
            other => panic!("expected a not-found error, got {other:?}"),
        }
    }
}
