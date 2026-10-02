//! The generated Rust as text, laid out on the C's lines.
//!
//! cinrs-core gives every token it generates from a line of the C a span on
//! that line (see `cinrs_core::capture::capture_c_file_by_line`). Printing each
//! token on the line its span names puts the Rust for C line 12 on line 12 of
//! the `.rs` file, and `--remap-path-prefix` then makes `rustc` call that file
//! by the C file's name — so a panic in the program, which is how Rust's own
//! run-time checks stop it, reports `file.c:12:9` rather than a position in a
//! file the user never saw.

use proc_macro2::{Delimiter, LineColumn, Spacing, TokenStream, TokenTree};
use quote::ToTokens;

/// The length past which a line is broken rather than kept in step with the
/// C; see [`Printer::place`].
const MAX_LINE: usize = 4000;

/// The unit's items in the order of the C lines they came from.
///
/// Code generation writes a unit's items kind by kind — the types, then the
/// `extern` block, then the statics, then the functions — and a printer that
/// can only move forward would put every function after the last line any
/// `static` came from. Rust does not care in what order a module's items come,
/// so they are sorted by the last line any of their tokens names (see
/// [`last_line`]), with what a C function declares inside it right after the
/// function and the items that name no line at the end; and the one
/// `extern` block, whose declarations come from all over the file, is split
/// into one block per declaration first. Anything that does not parse as
/// items is left as it is.
pub fn in_source_order(tokens: TokenStream) -> TokenStream {
    let Ok(mut file) = syn::parse2::<syn::File>(tokens.clone()) else {
        return tokens;
    };
    for item in &mut file.items {
        let syn::Item::Mod(module) = item else {
            continue;
        };
        let Some((_, items)) = &mut module.content else {
            continue;
        };
        let mut split = Vec::with_capacity(items.len());
        for item in items.drain(..) {
            match item {
                syn::Item::ForeignMod(block) => {
                    let declarations = block.items.clone();
                    let template = syn::ItemForeignMod {
                        items: Vec::new(),
                        ..block
                    };
                    for declaration in declarations {
                        let mut one = template.clone();
                        one.items.push(declaration);
                        split.push(syn::Item::ForeignMod(one));
                    }
                }
                other => split.push(other),
            }
        }
        // A `static` or a type declared inside a C function is an item of the
        // module, with lines inside the function's; it goes right after the
        // function, where it follows on from the closing brace instead of
        // dragging the function's first line down to its own.
        let mut functions: Vec<(usize, usize)> = split
            .iter()
            .filter_map(|item| match item {
                syn::Item::Fn(function) => Some((
                    function.sig.ident.span().start().line,
                    last_line(item.to_token_stream()),
                )),
                _ => None,
            })
            .filter(|&(head, last)| head > 1 && head <= last)
            .collect();
        functions.sort_unstable();
        // Items that name no line — what the headers declare, what code
        // generation adds of its own — go after everything else, where they
        // displace none of the file's lines; the sort is stable, so they keep
        // their order there.
        split.sort_by_cached_key(|item| match last_line(item.to_token_stream()) {
            1 => (usize::MAX, 0),
            line if !matches!(item, syn::Item::Fn(_)) => {
                let before = functions.partition_point(|&(head, _)| head <= line);
                match before.checked_sub(1).map(|index| functions[index]) {
                    Some((_, last)) if line < last => (last, 1),
                    _ => (line, 0),
                }
            }
            line => (line, 0),
        });
        *items = split;
    }
    file.into_token_stream()
}

/// `tokens` formatted to be read — what `-S` writes — with the items in the
/// order of the C, or as `rustc` would print them where they do not parse.
pub fn readable(tokens: TokenStream) -> String {
    let tokens = in_source_order(tokens);
    match syn::parse2::<syn::File>(tokens.clone()) {
        Ok(file) => prettyplease::unparse(&file),
        Err(_) => format!("{tokens}\n"),
    }
}

/// The last line any token of `tokens` names, which is where the item is
/// *defined*: a `struct` declared at its first `typedef` has its members at
/// its definition, and a variable declared `extern` early and defined late
/// has its initializer at the definition — the first line would put the
/// whole item there and drag the printer forward past everything between.
/// Line 1 is where a token generated with no C behind it reports itself.
fn last_line(tokens: TokenStream) -> usize {
    let mut max = 1;
    for tree in tokens {
        max = max.max(tree.span().start().line);
        if let TokenTree::Group(group) = tree {
            max = max.max(last_line(group.stream()));
        }
    }
    max
}

/// `tokens`, each on the line its span names wherever the order allows. A
/// token whose line has already passed, or that names none, follows on the
/// current line.
pub fn by_line(tokens: TokenStream) -> String {
    let mut printer = Printer {
        out: String::new(),
        line: 1,
        column: 0,
        glued: false,
    };
    printer.stream(tokens);
    printer.out.push('\n');
    printer.out
}

struct Printer {
    out: String,
    /// The line being written, from 1.
    line: usize,
    /// Characters written on it.
    column: usize,
    /// Whether the last token was a joint punctuation character, which the
    /// next must follow directly: `:` `:` is `::` only without a space or a
    /// line break between them.
    glued: bool,
}

impl Printer {
    fn stream(&mut self, tokens: TokenStream) {
        for tree in tokens {
            self.tree(tree);
        }
    }

    fn tree(&mut self, tree: TokenTree) {
        match tree {
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::None => ("", ""),
                };
                if !open.is_empty() {
                    self.place(group.span_open().start(), open);
                }
                self.stream(group.stream());
                if !close.is_empty() {
                    self.place(group.span_close().start(), close);
                }
            }
            TokenTree::Punct(punct) => {
                self.place(punct.span().start(), &punct.as_char().to_string());
                self.glued = punct.spacing() == Spacing::Joint;
            }
            TokenTree::Ident(ident) => self.place(ident.span().start(), &ident.to_string()),
            TokenTree::Literal(literal) => {
                self.place(literal.span().start(), &literal.to_string());
            }
        }
    }

    /// Writes one token's `text` where `at` says, as nearly as the order of
    /// the tokens lets it.
    fn place(&mut self, at: LineColumn, text: &str) {
        if self.glued {
            // Part of one multi-character operator: nothing in between.
        } else if self.column > MAX_LINE && at.line <= self.line {
            // A line this long is not worth keeping in step for: `rustc`
            // counts columns from the start of the line for every position it
            // records, and SQLite's tables made lines of megabytes that took
            // minutes. Breaking it moves the next few C lines down by as many
            // lines, until a later line of the C catches the output up.
            self.out.push('\n');
            self.line += 1;
            self.column = 0;
        } else if at.line > self.line {
            while self.line < at.line {
                self.out.push('\n');
                self.line += 1;
            }
            self.out.extend(std::iter::repeat_n(' ', at.column));
            self.column = at.column;
        } else if self.column > 0 {
            self.out.push(' ');
            self.column += 1;
        }
        self.glued = false;
        self.out.push_str(text);
        // A literal may span lines (a raw string the C never wrote does not,
        // but nothing here depends on that).
        match text.rfind('\n') {
            Some(last) => {
                self.line += text.matches('\n').count();
                self.column = text[last + 1..].chars().count();
            }
            None => self.column += text.chars().count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn tokens_land_on_their_lines() {
        // A parse puts every token on its own line of this source, which is
        // what a span from the C looks like to the printer.
        let tokens = TokenStream::from_str("fn f ( ) {\n\n  let x = a :: b ;\n\n\n    x . y ( ) }")
            .expect("lexes");
        let text = by_line(tokens);
        assert_eq!(
            text,
            "fn f ( ) {\n\n  let x = a :: b ;\n\n\n    x . y ( ) }\n"
        );
    }

    #[test]
    fn a_line_already_passed_follows_on() {
        let late = TokenStream::from_str("\n\nlate").expect("lexes");
        let early = TokenStream::from_str("early").expect("lexes");
        let mut tokens = late;
        tokens.extend(early);
        assert_eq!(by_line(tokens), "\n\nlate early\n");
    }

    #[test]
    fn what_a_function_declares_follows_it() {
        // `table` is declared on line 3, inside `f` (lines 2 to 5), and comes
        // first from code generation, as the statics do; `g` is on line 7.
        let piece = |text: &str| TokenStream::from_str(text).expect("lexes");
        let mut tokens = piece("\n\nstatic table : u8 = 0 ;");
        tokens.extend(piece("\n\n\n\n\n\nfn g ( ) { }"));
        tokens.extend(piece("\nfn f ( ) {\n\n\n}"));
        let module = quote::quote! { mod unit { #tokens } };
        assert_eq!(
            by_line(in_source_order(module)),
            "mod unit {\nfn f ( ) {\n\n\n} static table : u8 = 0 ;\n\nfn g ( ) { } }\n"
        );
    }
}
