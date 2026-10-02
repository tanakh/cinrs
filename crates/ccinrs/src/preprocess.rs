//! `-E`: the preprocessed text, as GCC prints it.
//!
//! cinrs-core's preprocessor hands back tokens, not text, each with the range
//! it was written at — or, for one a macro produced, the range of the
//! invocation. This puts them back on their lines: a token on a later line of
//! the same file starts a new line (a line marker instead, when the gap is
//! long), a token from another file comes after a GCC line marker naming it
//! (`# 12 "dir/x.h" 1 3` — entering it, a system header), and on one line two
//! tokens are separated by a space unless they were written touching. Every
//! token a macro produced is set apart by spaces, which is always correct C
//! even where GCC's choice of spacing would have been tighter.

use std::collections::HashMap;

use cinrs_core::FilePreprocessing;
use cinrs_core::include::HeaderKind;

/// How many lines are written out as empty ones before a line marker is
/// shorter; GCC's own threshold.
const MAX_BLANK_LINES: usize = 8;

/// The text of one unit, as bytes: a string literal keeps the bytes of a file
/// that is not UTF-8 as they were.
pub fn render(pre: &FilePreprocessing, line_markers: bool) -> Vec<u8> {
    let map = &pre.map;
    let headers: HashMap<usize, &cinrs_core::Header> = pre
        .headers
        .iter()
        .map(|header| (header.file.index(), header))
        .collect();
    // Whether `file` is `ancestor` or was included, at any depth, from it.
    let within = |mut file: usize, ancestor: usize| loop {
        if file == ancestor {
            return true;
        }
        match headers.get(&file) {
            Some(header) => file = header.included_from.index(),
            None => return false,
        }
    };
    let system = |file: usize| {
        headers
            .get(&file)
            .is_some_and(|header| header.kind != HeaderKind::User)
    };

    let mut out = Vec::new();
    let mut pragmas = pre.pragmas.iter().peekable();
    // The file and line the output is at, and where the last token ended if
    // it was written in that file rather than produced by a macro. It starts
    // at the top of the unit's own file, the first in the map.
    let mut file: Option<usize> = None;
    let mut line = 1;
    if let Some(root) = map.files().first() {
        if line_markers {
            out.extend_from_slice(format!("# 1 \"{}\"\n", escaped(root.name())).as_bytes());
        }
        file = Some(root.id().index());
    }
    let mut column = 0;
    let mut last_end = None;
    // Whether the output has lost count of its lines, and the next token has
    // to say where it is in a line marker.
    let mut lost = false;
    // The headers whose tokens have begun.
    let mut started = std::collections::HashSet::new();
    for (index, token) in pre.tokens.iter().enumerate() {
        while let Some((_, text)) = pragmas.next_if(|(at, _)| *at <= index) {
            if column > 0 {
                out.push(b'\n');
            }
            out.extend_from_slice(format!("#pragma {text}\n").as_bytes());
            column = 0;
            // The pragma took a line the source did not have there.
            lost = true;
        }
        if token.is_eof() {
            break;
        }
        let pos = token.range.start;
        let here = map.file_of(pos).index();
        let (at_line, at_column) = map.line_col(pos);
        if lost || file != Some(here) || at_line > line + MAX_BLANK_LINES {
            lost = false;
            if column > 0 {
                out.push(b'\n');
            }
            // A header is entered (GCC's flag 1) the first time its tokens
            // come, and returned to (flag 2) when it is the file one that came
            // since was included from.
            let entered = headers.contains_key(&here) && started.insert(here);
            if line_markers {
                let flag = match file {
                    _ if entered => " 1",
                    Some(from) if from != here && within(from, here) => " 2",
                    _ => "",
                };
                let system = if system(here) { " 3" } else { "" };
                let name = map.file(map.file_of(pos)).name();
                out.extend_from_slice(
                    format!("# {at_line} \"{}\"{flag}{system}\n", escaped(name)).as_bytes(),
                );
            }
            file = Some(here);
            line = at_line;
            column = 0;
            last_end = None;
        }
        if at_line > line {
            out.extend(std::iter::repeat_n(b'\n', at_line - line));
            line = at_line;
            column = 0;
            last_end = None;
        }
        let written = token.origin.expansion().is_none();
        if column == 0 {
            let indent = at_column.saturating_sub(1);
            out.extend(std::iter::repeat_n(b' ', indent));
            column = indent;
        } else if !(written && last_end == Some(pos)) {
            out.push(b' ');
            column += 1;
        }
        let spelling = token.kind.spelling();
        for c in spelling.chars() {
            match cinrs_core::lex::raw_byte(c) {
                Some(byte) => out.push(byte),
                None => {
                    let mut buf = [0; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
            }
        }
        column += spelling.chars().count().max(1);
        last_end = written.then_some(token.range.end);
    }
    if column > 0 {
        out.push(b'\n');
    }
    out
}

/// A file name as a line marker spells it: a C string literal's contents.
fn escaped(name: &str) -> String {
    name.replace('\\', "\\\\").replace('"', "\\\"")
}
