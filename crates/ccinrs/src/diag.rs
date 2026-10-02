//! Diagnostics as GCC prints them.
//!
//! Inside a procedural macro a diagnostic becomes a `compile_error!` that
//! `rustc` draws with its caret; here there is no `rustc` between the C and the
//! user, so this module does what GCC does — `file:line:column: error:
//! message`, the line itself, and a caret under the column — writing to
//! standard error.

use std::fmt::Write as _;

use cinrs_core::{Diagnostic, Diagnostics, Level, Pos, SourceMap};

/// How a run of diagnostics came out.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tally {
    pub errors: usize,
    pub warnings: usize,
}

/// Renders every diagnostic of one translation unit, errors always and
/// warnings when `warnings` is set, in source order.
pub fn render(map: &SourceMap, diags: &Diagnostics, warnings: bool) -> (String, Tally) {
    let mut out = String::new();
    let mut tally = Tally::default();
    if let Some(message) = diags.fatal_message() {
        let _ = writeln!(out, "ccinrs: fatal error: {message}");
        tally.errors += 1;
    }
    for diag in diags.sorted() {
        let level = match diag.level {
            Level::Error => {
                tally.errors += 1;
                "error"
            }
            Level::Warning if warnings => {
                tally.warnings += 1;
                "warning"
            }
            Level::Warning => continue,
        };
        one(&mut out, map, diag, level);
    }
    (out, tally)
}

/// One diagnostic and its notes.
fn one(out: &mut String, map: &SourceMap, diag: &Diagnostic, level: &str) {
    let _ = writeln!(
        out,
        "{}: {level}: {}",
        location(map, diag.range.start),
        diag.message
    );
    excerpt(out, map, diag.range.start);
    for note in &diag.notes {
        match note.range {
            Some(range) => {
                let _ = writeln!(
                    out,
                    "{}: note: {}",
                    location(map, range.start),
                    note.message
                );
            }
            None => {
                let _ = writeln!(out, "note: {}", note.message);
            }
        }
    }
}

/// `file:line:column`.
fn location(map: &SourceMap, pos: Pos) -> String {
    let file = map.file(map.file_of(pos));
    let (line, column) = map.line_col(pos);
    format!("{}:{line}:{column}", file.name())
}

/// The line `pos` is on, and a caret under it, as GCC draws them:
///
/// ```text
///     4 |     return missing(x);
///       |            ^
/// ```
fn excerpt(out: &mut String, map: &SourceMap, pos: Pos) {
    let file = map.file(map.file_of(pos));
    let (line, column) = map.line_col(pos);
    let Some(text) = file.text().lines().nth(line - 1) else {
        return;
    };
    // A byte of a file that is not UTF-8 is a private-use character in the
    // text; shown as the replacement character, which is what it is.
    let text: String = text
        .chars()
        .map(|c| {
            if cinrs_core::lex::raw_byte(c).is_some() {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect();
    let gutter = line.to_string().len().max(4);
    let _ = writeln!(out, " {line:>gutter$} | {text}");
    // The caret goes under the same characters, tabs included, so that it
    // lines up whatever the terminal makes of a tab.
    let lead: String = text
        .chars()
        .take(column.saturating_sub(1))
        .map(|c| if c == '\t' { '\t' } else { ' ' })
        .collect();
    let _ = writeln!(out, " {:>gutter$} | {lead}^", "");
}
