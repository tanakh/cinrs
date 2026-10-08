//! The generated Rust as text, laid out on the C's lines.
//!
//! cinrs-core gives every token it generates from a line of the C a span on
//! that line (see `cinrs_core::capture::capture_c_file_by_line`). Printing each
//! token on the line its span names puts the Rust for C line 12 on line 12 of
//! the `.rs` file, and `--remap-path-prefix` then makes `rustc` call that file
//! by the C file's name — so a panic in the program, which is how Rust's own
//! run-time checks stop it, reports `file.c:12:9` rather than a position in a
//! file the user never saw.

use proc_macro2::{Delimiter, Group, LineColumn, Spacing, TokenStream, TokenTree};

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
/// into one block per declaration first. What the file does not use is left
/// out; see [`used_items`].
///
/// The items are found in the tokens as code generation lays them out (see
/// [`split_items`]) rather than parsed with `syn`, which takes longer than
/// `rustc` does over a unit that includes a few large headers; a module that
/// does not split as expected is left as it is.
pub fn in_source_order(tokens: TokenStream) -> TokenStream {
    let mut out = TokenStream::new();
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut index = 0;
    while index < trees.len() {
        // `mod <name> { … }`, the unit's module.
        if is_ident(&trees[index], "mod")
            && let (Some(TokenTree::Ident(_)), Some(TokenTree::Group(body))) =
                (trees.get(index + 1), trees.get(index + 2))
            && body.delimiter() == Delimiter::Brace
        {
            out.extend(trees[index..index + 2].iter().cloned());
            let mut module = Group::new(Delimiter::Brace, module_in_source_order(body.stream()));
            module.set_span(body.span());
            out.extend([TokenTree::Group(module)]);
            index += 3;
            continue;
        }
        out.extend([trees[index].clone()]);
        index += 1;
    }
    out
}

/// The body of the unit's module, its items used, split and in order; see
/// [`in_source_order`]. A body that does not split into items as expected
/// is left as it is.
fn module_in_source_order(body: TokenStream) -> TokenStream {
    let Some((header, items)) = split_items(body.clone()) else {
        return body;
    };
    let mut items: Vec<(usize, Item)> = used_items(items)
        .into_iter()
        .map(|item| (last_line(item.tokens.clone()), item))
        .collect();
    // A `static` or a type declared inside a C function is an item of the
    // module, with lines inside the function's; it goes right after the
    // function, where it follows on from the closing brace instead of
    // dragging the function's first line down to its own.
    let mut functions: Vec<(usize, usize)> = items
        .iter()
        .filter_map(|(last, item)| item.function.map(|head| (head, *last)))
        .filter(|&(head, last)| head > 1 && head <= last)
        .collect();
    functions.sort_unstable();
    // Items that name no line — what the headers declare, what code
    // generation adds of its own — go after everything else, where they
    // displace none of the file's lines; the sort is stable, so they keep
    // their order there.
    items.sort_by_key(|&(last, ref item)| match last {
        1 => (usize::MAX, 0),
        line if item.function.is_none() => {
            let before = functions.partition_point(|&(head, _)| head <= line);
            match before.checked_sub(1).map(|index| functions[index]) {
                Some((_, last)) if line < last => (last, 1),
                _ => (line, 0),
            }
        }
        line => (line, 0),
    });
    let mut out = header;
    for (_, item) in items {
        out.extend(item.tokens);
    }
    out
}

/// One item of the unit's module.
struct Item {
    tokens: TokenStream,
    /// For a function, the line of its name.
    function: Option<usize>,
    role: Role,
}

/// What keeps an [`Item`] in the unit; see [`used_items`].
enum Role {
    /// Kept whatever names it.
    Root,
    /// Kept when something kept names it.
    Named(String),
    /// An `impl`: kept with any of the types it is for.
    Impl(Vec<String>),
}

/// The words that say what an item is, the first of which in an item's
/// tokens (outside its attributes and groups) decides where it ends: after
/// its `;` for the ones whose value may hold a block — the `{ … }` of a
/// `const` or a `static` initializer, or of a `use` list — and otherwise at
/// its first `{ … }` or `;`.
const ITEM_KINDS: &[&str] = &[
    "fn",
    "struct",
    "union",
    "enum",
    "impl",
    "mod",
    "trait",
    "const",
    "static",
    "type",
    "use",
    "macro_rules",
];

/// The tokens of a module body as its inner attributes and its items, the
/// `extern` blocks split into one declaration each — or `None` where they do
/// not split as code generation lays them out.
fn split_items(body: TokenStream) -> Option<(TokenStream, Vec<Item>)> {
    let trees: Vec<TokenTree> = body.into_iter().collect();
    let mut index = 0;
    let mut header = TokenStream::new();
    // `#![…]`
    while trees.get(index).is_some_and(|tree| is_punct(tree, '#'))
        && trees.get(index + 1).is_some_and(|tree| is_punct(tree, '!'))
        && is_group(trees.get(index + 2), Delimiter::Bracket)
    {
        header.extend(trees[index..index + 3].iter().cloned());
        index += 3;
    }
    let mut items = Vec::new();
    while index < trees.len() {
        let start = index;
        let mut kind: Option<&str> = None;
        let mut end = None;
        while index < trees.len() {
            let tree = &trees[index];
            match tree {
                // An attribute, whatever it holds.
                _ if is_punct(tree, '#') && is_group(trees.get(index + 1), Delimiter::Bracket) => {
                    index += 2;
                    continue;
                }
                TokenTree::Ident(ident) if kind.is_none() => {
                    let word = ident.to_string();
                    // `const fn` is a function.
                    let const_fn = word == "const"
                        && trees.get(index + 1).is_some_and(|next| {
                            ["fn", "unsafe", "extern"]
                                .iter()
                                .any(|word| is_ident(next, word))
                        });
                    if !const_fn {
                        kind = ITEM_KINDS.iter().copied().find(|&known| known == word);
                    }
                }
                _ if is_punct(tree, ';') => {
                    end = Some(index + 1);
                }
                TokenTree::Group(group)
                    if group.delimiter() == Delimiter::Brace
                        && !matches!(kind, Some("const" | "static" | "use" | "type")) =>
                {
                    end = Some(index + 1);
                }
                _ => {}
            }
            index += 1;
            if end.is_some() {
                break;
            }
        }
        let end = end?;
        let tokens = &trees[start..end];
        // An `extern` block: one item per declaration, each in a block of its
        // own like this one.
        if kind.is_none()
            && tokens.iter().any(|tree| is_ident(tree, "extern"))
            && let Some(TokenTree::Group(block)) = tokens.last()
            && block.delimiter() == Delimiter::Brace
        {
            let prefix = &tokens[..tokens.len() - 1];
            let declarations: Vec<TokenTree> = block.stream().into_iter().collect();
            let mut from = 0;
            for (at, tree) in declarations.iter().enumerate() {
                if is_punct(tree, ';') {
                    let declaration = &declarations[from..=at];
                    let mut one =
                        Group::new(Delimiter::Brace, declaration.iter().cloned().collect());
                    one.set_span(block.span());
                    let mut stream: TokenStream = prefix.iter().cloned().collect();
                    stream.extend([TokenTree::Group(one)]);
                    items.push(Item {
                        role: declaration_role(declaration),
                        function: None,
                        tokens: stream,
                    });
                    from = at + 1;
                }
            }
            if from != declarations.len() {
                return None;
            }
            continue;
        }
        items.push(Item {
            role: item_role(tokens, kind),
            function: (kind == Some("fn"))
                .then(|| name_after(tokens, "fn").map(|ident| ident.span().start().line))
                .flatten(),
            tokens: tokens.iter().cloned().collect(),
        });
    }
    Some((header, items))
}

fn is_punct(tree: &TokenTree, c: char) -> bool {
    matches!(tree, TokenTree::Punct(punct) if punct.as_char() == c)
}

fn is_ident(tree: &TokenTree, word: &str) -> bool {
    matches!(tree, TokenTree::Ident(ident) if ident == word)
}

fn is_group(tree: Option<&TokenTree>, delimiter: Delimiter) -> bool {
    matches!(tree, Some(TokenTree::Group(group)) if group.delimiter() == delimiter)
}

/// Whether one of the outer attributes in `tokens` puts the item in front of
/// the linker: `no_mangle`, `export_name`, `link_section` or `used`, also
/// inside `unsafe(…)`.
fn exported(tokens: &[TokenTree]) -> bool {
    fn says(attribute: TokenStream) -> bool {
        attribute.into_iter().any(|tree| match tree {
            TokenTree::Ident(ident) => ["no_mangle", "export_name", "link_section", "used"]
                .iter()
                .any(|word| ident == word),
            TokenTree::Group(group) => says(group.stream()),
            _ => false,
        })
    }
    tokens.windows(2).any(|pair| match pair {
        [hash, TokenTree::Group(attribute)]
            if is_punct(hash, '#') && attribute.delimiter() == Delimiter::Bracket =>
        {
            says(attribute.stream())
        }
        _ => false,
    })
}

/// The identifier after the first `keyword` among `tokens`, outside groups;
/// `mut` is passed over (`static mut`).
fn name_after<'a>(tokens: &'a [TokenTree], keyword: &str) -> Option<&'a proc_macro2::Ident> {
    let at = tokens.iter().position(|tree| is_ident(tree, keyword))?;
    tokens[at + 1..].iter().find_map(|tree| match tree {
        TokenTree::Ident(ident) if ident != "mut" => Some(ident),
        _ => None,
    })
}

/// The [`Role`] of an item of `kind` made of `tokens`.
fn item_role(tokens: &[TokenTree], kind: Option<&str>) -> Role {
    let named = |keyword: &str| match name_after(tokens, keyword) {
        Some(ident) => Role::Named(ident.to_string()),
        None => Role::Root,
    };
    match kind {
        _ if exported(tokens) => Role::Root,
        Some("const") => match name_after(tokens, "const") {
            // A check of the target the unit is compiled for.
            Some(ident) if ident == "_" => Role::Root,
            Some(ident) => Role::Named(ident.to_string()),
            None => Role::Root,
        },
        Some(keyword @ ("fn" | "struct" | "union" | "enum" | "type" | "static")) => named(keyword),
        Some("macro_rules") => match tokens.iter().position(|tree| is_punct(tree, '!')) {
            Some(at) => match tokens.get(at + 1) {
                Some(TokenTree::Ident(ident)) => Role::Named(ident.to_string()),
                _ => Role::Root,
            },
            None => Role::Root,
        },
        Some("impl") => {
            // What follows `for`, or `impl` when there is none, up to the body.
            let after = tokens
                .iter()
                .position(|tree| is_ident(tree, "for"))
                .or_else(|| tokens.iter().position(|tree| is_ident(tree, "impl")))
                .map_or(0, |at| at + 1);
            let mut names = Vec::new();
            for tree in &tokens[after..] {
                if is_group(Some(tree), Delimiter::Brace) {
                    break;
                }
                names_in(std::iter::once(tree.clone()).collect(), &mut names);
            }
            Role::Impl(names)
        }
        _ => Role::Root,
    }
}

/// The [`Role`] of one declaration of an `extern` block: named by what its
/// first `fn`, `static` or `type` says — the first, for a `static` whose type
/// is a function pointer has a `fn` of its own.
fn declaration_role(declaration: &[TokenTree]) -> Role {
    if exported(declaration) {
        return Role::Root;
    }
    let keyword = declaration.iter().find_map(|tree| match tree {
        TokenTree::Ident(ident) if ident == "fn" || ident == "static" || ident == "type" => {
            Some(ident.to_string())
        }
        _ => None,
    });
    match keyword.and_then(|keyword| name_after(declaration, &keyword)) {
        Some(ident) => Role::Named(ident.to_string()),
        None => Role::Root,
    }
}

/// Every identifier in `tokens`, and every word in a string literal among
/// them, which is how an `asm!` names a symbol.
fn names_in(tokens: TokenStream, out: &mut Vec<String>) {
    for tree in tokens {
        match tree {
            TokenTree::Ident(ident) => out.push(ident.to_string()),
            TokenTree::Group(group) => names_in(group.stream(), out),
            TokenTree::Literal(literal) => {
                let text = literal.to_string();
                if text.contains('"') {
                    let mut word = String::new();
                    for c in text.chars().chain(std::iter::once(' ')) {
                        if c == '_' || c.is_ascii_alphanumeric() {
                            word.push(c);
                        } else if !word.is_empty() {
                            if !word.as_bytes()[0].is_ascii_digit() {
                                out.push(std::mem::take(&mut word));
                            }
                            word.clear();
                        }
                    }
                }
            }
            TokenTree::Punct(_) => {}
        }
    }
}

/// The items of a unit's module that its object needs: what it defines for
/// the linker, and everything that reaches by name.
///
/// A C file brings in every declaration of every header it includes — git's
/// 327-line `abspath.c` comes to 6000 `extern` functions, 480 structures and
/// 740 `static inline` functions of OpenSSL's, glibc's and git's own, two
/// megabytes of Rust — and `rustc` parses, resolves, type-checks and
/// borrow-checks all of it, for an object made of ten functions: two thirds of
/// the time a small file takes. What the unit does not use cannot change the
/// object, so it is not written.
///
/// The roots are the items the linker sees — those with `no_mangle`,
/// `export_name`, `link_section` or `used`, and the `global_asm!` — the
/// `const _` checks of the target, and anything this does not recognise. An
/// item is kept when one that is kept names it: an identifier anywhere in its
/// tokens, or a word in one of its string literals, which is how an `asm!`
/// names a symbol. Names are compared as text, which keeps too much where a
/// field or a variable shares an item's name, and never too little. An
/// `impl` goes with the type it is for.
fn used_items(items: Vec<Item>) -> Vec<Item> {
    use std::collections::HashMap;

    let roles: Vec<&Role> = items.iter().map(|item| &item.role).collect();
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, role) in roles.iter().enumerate() {
        if let Role::Named(name) = role {
            by_name.entry(name.as_str()).or_default().push(index);
        }
    }
    let mut kept = vec![false; items.len()];
    let mut work: Vec<usize> = (0..items.len())
        .filter(|&index| matches!(roles[index], Role::Root))
        .collect();
    for &index in &work {
        kept[index] = true;
    }
    loop {
        while let Some(index) = work.pop() {
            let mut names = Vec::new();
            names_in(items[index].tokens.clone(), &mut names);
            for name in &names {
                for &named in by_name.get(name.as_str()).into_iter().flatten() {
                    if !kept[named] {
                        kept[named] = true;
                        work.push(named);
                    }
                }
            }
        }
        // An `impl` of a type that is kept goes with it, and may name more.
        for (index, role) in roles.iter().enumerate() {
            if let Role::Impl(types) = role {
                let wanted = types.iter().any(|name| {
                    by_name
                        .get(name.as_str())
                        .is_some_and(|defs| defs.iter().any(|&def| kept[def]))
                });
                if wanted && !kept[index] {
                    kept[index] = true;
                    work.push(index);
                }
            }
        }
        if work.is_empty() {
            break;
        }
    }
    items
        .into_iter()
        .zip(kept)
        .filter_map(|(item, kept)| kept.then_some(item))
        .collect()
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

    fn piece(text: &str) -> TokenStream {
        TokenStream::from_str(text).expect("lexes")
    }

    #[test]
    fn what_a_function_declares_follows_it() {
        // `table` is declared on line 3, inside `f` (lines 2 to 5), and comes
        // first from code generation, as the statics do; `g` is on line 7.
        let mut tokens = piece("\n\nstatic table : u8 = 0 ;");
        tokens.extend(piece("\n\n\n\n\n\n#[used] fn g ( ) { }"));
        tokens.extend(piece("\n#[used] fn f ( ) {\n\n\ntable }"));
        let module = quote::quote! { mod unit { #tokens } };
        assert_eq!(
            by_line(in_source_order(module)),
            "mod unit {\n# [ used ] fn f ( ) {\n\n\ntable } static table : u8 = 0 ;\n\n\
             # [ used ] fn g ( ) { } }\n"
        );
    }

    /// The names of the items `in_source_order` keeps of `items`, in order,
    /// with `*` for a root.
    fn kept(items: &str) -> String {
        // Line 1 is no line; see `last_line`.
        let items = piece(&format!("\n{items}"));
        let tokens = in_source_order(quote::quote! { mod unit { #![allow(x)] #items } });
        let mut names = Vec::new();
        let Some(TokenTree::Group(module)) = tokens.into_iter().nth(2) else {
            panic!("the module is still there");
        };
        let body: Vec<TokenTree> = module.stream().into_iter().collect();
        assert_eq!(
            body.iter()
                .take(3)
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["#", "!", "[allow (x)]"],
            "the inner attribute stays first"
        );
        let (_, items) = split_items(body.into_iter().collect()).expect("splits");
        for item in items {
            names.push(match item.role {
                Role::Root => "*".to_owned(),
                Role::Named(name) => name,
                Role::Impl(types) => format!("impl {}", types.join(" ")),
            });
        }
        names.join(" ")
    }

    #[test]
    fn what_nothing_uses_is_left_out() {
        // The roots are the exported function and the check of the target.
        // The function reaches `S`, whose `impl` reaches `helper`, and `g`
        // through `f`; nothing names `unused`, or `Unused` and its `impl`.
        assert_eq!(
            kept(
                "#[unsafe(no_mangle)] pub extern \"C\" fn root ( ) { f ( ) ; let _ : S ; }
                 fn f ( ) { g ( ) } fn g ( ) { } fn unused ( ) { f ( ) }
                 #[repr(C)] pub struct S { x : i32 } impl S { fn get ( ) { helper ( ) } }
                 fn helper ( ) { } pub struct Unused ; impl Clone for Unused { }
                 const _ : ( ) = {
                     # [cfg (not (unix))] :: core :: compile_error ! ( \"unix only\" ) ; } ;"
            ),
            "* f g S impl S helper *"
        );
    }

    #[test]
    fn an_extern_block_is_one_declaration_each() {
        // `strlen` and `hook` are called and `puts` named by an `asm!`;
        // `abort` is not, and nor is `TABLE`, whose block is its own.
        assert_eq!(
            kept(
                "unsafe extern \"C\" { pub fn strlen ( s : * const u8 ) -> usize ;
                     # [link_name = \"abort\"] pub fn abort ( ) -> ! ; pub fn puts ( ) ;
                     pub static hook : Option < unsafe extern \"C\" fn ( i32 ) -> c_int > ;
                     # [used] pub static seen : i32 ; }
                 #[unsafe(no_mangle)] fn root ( ) {
                     strlen ( p ) ; hook ( 0 ) ; asm ! ( \"call {puts}@PLT\" ) ; }
                 static TABLE : [ u8 ; 2 ] = { [ 0 , 1 ] } ;"
            ),
            "strlen puts hook * *"
        );
    }

    #[test]
    fn what_does_not_split_is_left_as_it_is() {
        let tokens = quote::quote! { mod unit { fn f ( ) { } fn } };
        assert_eq!(
            in_source_order(tokens.clone()).to_string(),
            tokens.to_string()
        );
    }
}
