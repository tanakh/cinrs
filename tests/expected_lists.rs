//! The checked-in expected-failure lists are canonical, and parse.
//!
//! The three conformance harnesses write their lists with
//! [`conformance::write_list`] and read them back with
//! [`conformance::read_list`], so the two have to agree — but a list is also
//! *hand-edited*, which is where the category tags and half the notes come
//! from, and a hand edit can put a line in a shape the writer would never
//! produce. The next `…_UPDATE_EXPECTED=1` run then reflows the file and the
//! diff is a hundred lines of whitespace with the real change buried in it.
//!
//! So this test reads every list in the repository, asserts that it parses —
//! which is where a missing `[bug]`/`[unimplemented]`/`[not-planned]` tag is
//! caught — and asserts that rendering it again reproduces the file byte for
//! byte. It needs no corpus and no `rustc`, so it runs in an ordinary
//! `cargo test` in well under a second, and it is the thing that keeps
//! "`UPDATE_EXPECTED` round-trips every list" true between updates.
//!
//! `CINRS_EXPECTED_LISTS_BLESS=1 cargo test --test expected_lists` rewrites the
//! bodies instead of comparing them, which is how a hand edit is tidied up
//! without running a suite. It leaves the header comment exactly as it is;
//! only a harness's own update mode rewrites that.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ui_test::color_eyre::eyre::{Result, bail, eyre};

#[path = "support/conformance.rs"]
mod conformance;

use conformance::{Entry, flag, read_list, render_list};

/// Where the lists live, one directory per suite.
const LIST_DIRS: &[&str] = &["tests/c-testsuite", "tests/gcc-torture", "tests/clang-c"];

/// Every `*.txt` under [`LIST_DIRS`], in a stable order.
fn lists() -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for dir in LIST_DIRS {
        let dir = Path::new(dir);
        let entries = std::fs::read_dir(dir).map_err(|err| eyre!("reading {dir:?}: {err}"))?;
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "txt") {
                out.push(path);
            }
        }
    }
    out.sort();
    if out.is_empty() {
        bail!("no expected-failure lists found; is the working directory the package root?");
    }
    Ok(out)
}

/// The comment block at the top of a list, up to the first entry.
///
/// A list is a run of `#` comments and blank lines and then a run of entries;
/// the header is everything before the first line that is neither.
fn split_header(text: &str) -> (&str, &str) {
    let mut at = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if !(trimmed.is_empty() || trimmed.starts_with('#')) {
            break;
        }
        at += line.len() + 1;
    }
    text.split_at(at.min(text.len()))
}

fn main() -> Result<()> {
    let bless = flag("CINRS_EXPECTED_LISTS_BLESS");
    let mut wrong = Vec::new();
    for path in lists()? {
        let text = std::fs::read_to_string(&path)?;
        let (header, body) = split_header(&text);
        let entries = read_list(&path)?;
        let entries: BTreeMap<&str, Entry> = entries
            .iter()
            .map(|(id, entry)| (id.as_str(), entry.clone()))
            .collect();
        let rendered = render_list(&entries);
        if rendered == body {
            println!(
                "expected-lists: {} — {} entries",
                path.display(),
                entries.len()
            );
            continue;
        }
        if bless {
            std::fs::write(&path, format!("{header}{rendered}"))?;
            println!("expected-lists: rewrote {}", path.display());
            continue;
        }
        let differs = body
            .lines()
            .zip(rendered.lines())
            .find(|(was, is)| was != is)
            .map(|(was, is)| format!("\n    it has: {was}\n    wanted: {is}"))
            .unwrap_or_default();
        wrong.push(format!("  {}{differs}", path.display()));
    }
    if !wrong.is_empty() {
        bail!(
            "{} not written the way `write_list` writes one, so the next \
             `…_UPDATE_EXPECTED=1` run would reflow it:\n{}\n\n\
             `CINRS_EXPECTED_LISTS_BLESS=1 cargo test --test expected_lists` tidies them up.",
            conformance::plural(wrong.len(), "list is", "lists are"),
            wrong.join("\n"),
        );
    }
    Ok(())
}
