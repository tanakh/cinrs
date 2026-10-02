//! `-M` and its relatives: a Makefile rule naming the files an object was
//! made from, so that `make` rebuilds it when a header changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cinrs_core::Header;
use cinrs_core::include::HeaderKind;

use crate::args::{Deps, make_quoted};

/// The column past which GCC continues a rule on the next line.
const MAX_COLUMN: usize = 75;

/// The rule for one C file, as GCC writes it: `target: file.c header.h …`,
/// continued with a backslash where it grows long, and under `-MP` an empty
/// rule for every header.
///
/// The bundled headers are part of `ccinrs` and are never listed; the
/// platform's are listed by `-M` and `-MD` only, as GCC lists them — and, as
/// GCC does, `-MM` leaves out a header of the program's own when it was read
/// by one of the platform's.
pub fn rule(
    deps: &Deps,
    default_target: &str,
    source: &Path,
    headers: &[Header],
    embedded: &[PathBuf],
) -> String {
    let targets: Vec<String> = if deps.targets.is_empty() {
        vec![make_quoted(default_target)]
    } else {
        deps.targets.clone()
    };
    let by_file: HashMap<usize, &Header> = headers
        .iter()
        .map(|header| (header.file.index(), header))
        .collect();
    let read_by_system = |header: &Header| {
        let mut from = by_file.get(&header.included_from.index());
        while let Some(parent) = from {
            if parent.kind == HeaderKind::System {
                return true;
            }
            from = by_file.get(&parent.included_from.index());
        }
        false
    };
    let mut names = vec![source.display().to_string()];
    for header in headers {
        let listed = match header.kind {
            HeaderKind::Bundled => false,
            HeaderKind::System => deps.system,
            HeaderKind::User => deps.system || !read_by_system(header),
        };
        if listed && !names.contains(&header.name) {
            names.push(header.name.clone());
        }
    }
    for path in embedded {
        let name = path.display().to_string();
        if !names.contains(&name) {
            names.push(name);
        }
    }

    let mut text = format!("{}:", targets.join(" "));
    let mut width = text.chars().count();
    for name in &names {
        let name = make_quoted(name);
        let len = name.chars().count();
        if width + 1 + len > MAX_COLUMN && width > 1 {
            text.push_str(" \\\n");
            width = 0;
        }
        text.push(' ');
        text.push_str(&name);
        width += 1 + len;
    }
    text.push('\n');
    if deps.phony {
        for name in &names[1..] {
            text.push_str(&format!("\n{}:\n", make_quoted(name)));
        }
    }
    text
}

/// The object `-M` names when `-MT` does not say: the C file's name without
/// its directory, with `.o` for its suffix.
pub fn object_name(source: &Path) -> String {
    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    format!("{stem}.o")
}
