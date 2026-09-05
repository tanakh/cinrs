//! A recursive-descent parser for the whole C99 grammar.
//!
//! # The lexer hack
//!
//! C cannot be parsed without knowing which identifiers are `typedef` names:
//! `T * x;` is a declaration when `T` names a type and a multiplication
//! otherwise, and `(T)-1` is a cast rather than a subtraction. The parser
//! therefore keeps a stack of scopes recording, for every
//! identifier it declares, whether it was introduced by `typedef` or as an
//! ordinary object. Lookups walk the stack from the innermost scope out, so an
//! ordinary declaration properly shadows an outer `typedef`.
//!
//! # Declarator resolution
//!
//! Declarators are turned into a [`Type`] tree as they are parsed, using the
//! classic "parse the suffixes first, then recurse into the parenthesised
//! declarator" trick. That is what makes `int (*fp[3])(void)` come out as
//! *array of 3 pointer to function(void) returning int* rather than as an
//! opaque chain that sema would have to interpret again.
//!
//! # Standards
//!
//! The grammar is C23's, and the [`Standard`] the unit was compiled with
//! gates the parts of it a block's own revision does not have. Which way a
//! construct is gated
//! depends on how C spelled it: the C11 keywords all start with an
//! underscore, which C99 reserves, so the lexer recognises them everywhere
//! and the parser reports "'_Static_assert' requires C11 or later" instead of
//! a syntax error; the C23 keywords are ordinary identifiers before C23 — the
//! bundled `<stdbool.h>` writes `#define bool _Bool` — so they are gated
//! where the *name* turns out not to mean anything, here and in
//! [sema](crate::sema).
//!
//! # Error recovery
//!
//! A syntax error aborts the current external declaration (`Err(Bail)`
//! unwinds to the top level), which then synchronises on the next `;` or `}`
//! at nesting depth zero and keeps going. That way one compilation reports one
//! error per broken declaration instead of stopping at the first.

use std::collections::HashMap;

use crate::ast::*;
use crate::capture::SourceRange;
use crate::diag::Diagnostics;
use crate::gnu;
use crate::ir::VA_LIST_NAMES;
use crate::lex::{Keyword, Punct, StrKind, StrLit, TokenKind};
use crate::pp::{Origin, PackMap, Token};
use crate::{Gating, Options, Standard};

/// The spelling of `_Noreturn` that every standard accepts.
///
/// `_Noreturn` itself is a C11 keyword, and a `c99!` block that includes the
/// bundled `<stdlib.h>` must not be told that its `exit` declaration needs a
/// newer standard. The headers therefore write this name, which is a
/// declaration specifier in every mode and means exactly what `_Noreturn`
/// means.
pub const NORETURN_BUILTIN: &str = "__cinrs_noreturn";

/// An integer constant expression the parser synthesises.
fn int_expr(value: u128, range: SourceRange) -> Expr {
    Expr {
        kind: ExprKind::Int(crate::lex::IntLit {
            value,
            base: crate::lex::NumBase::Decimal,
            unsigned: false,
            long: crate::lex::LongKind::None,
            text: value.to_string(),
        }),
        range,
    }
}

/// Signals that the current external declaration cannot be parsed further.
#[derive(Debug)]
#[must_use]
pub struct Bail;

type PResult<T> = Result<T, Bail>;

/// Whether an identifier names a type or an object.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SymKind {
    Typedef,
    Ordinary,
}

/// One lexical scope's contribution to the lexer hack.
#[derive(Default)]
struct Scope {
    syms: HashMap<String, SymKind>,
}

/// Parses a token list into a [`TranslationUnit`].
///
/// The tokens are the [preprocessor](crate::pp)'s, so there are no directives
/// left to see and every range already points where a diagnostic should land —
/// at the invocation, for a token a macro produced.
///
/// `unit_range` is the range the whole translation unit covers — normally
/// [`crate::Source::root_range`]. The token list must end with
/// [`TokenKind::Eof`].
/// Diagnostics are pushed into `diags`; the returned tree is a best effort and
/// may contain [`TypeKind::Error`]/[`StmtKind::Error`] placeholders.
pub fn parse(
    tokens: &[Token],
    unit_range: SourceRange,
    packing: &PackMap,
    options: &Options,
    diags: &mut Diagnostics,
) -> TranslationUnit {
    // The preprocessor always ends its output with EOF, but the parser indexes
    // on that promise, so make it true rather than trust it.
    let patched: Vec<Token>;
    let tokens: &[Token] = if tokens.last().is_some_and(Token::is_eof) {
        tokens
    } else {
        patched = tokens
            .iter()
            .cloned()
            .chain(std::iter::once(Token {
                kind: TokenKind::Eof,
                range: SourceRange::at(unit_range.end),
                origin: Origin::Source,
            }))
            .collect();
        &patched
    };
    let last_range = tokens.first().map_or(unit_range, |t| t.range);
    // `__builtin_va_list` names a type wherever it appears, which the parser
    // has to know before it can tell `__builtin_va_list *p;` from a
    // multiplication. It is the one name the compiler owns; `va_list` itself
    // is an ordinary identifier that the bundled `<stdarg.h>` `typedef`s to
    // it, exactly as GCC's own header does.
    let mut builtins = Scope::default();
    for name in VA_LIST_NAMES {
        builtins.syms.insert((*name).to_owned(), SymKind::Typedef);
    }
    let mut parser = Parser {
        tokens,
        pos: 0,
        diags,
        scopes: vec![builtins],
        standard: options.standard,
        gating: options.gating(),
        packing,
        last_range,
        depth: 0,
        records: Vec::new(),
        enums: Vec::new(),
        typeofs: Vec::new(),
    };
    parser.parse_translation_unit(unit_range)
}

/// How deeply one construct may nest before the parser gives up.
///
/// Recursive descent turns nesting in the input into stack frames, and a
/// procedural macro that overflows the stack takes the whole compiler down
/// with no useful message. Pathologically nested input becomes a diagnostic
/// instead.
///
/// What it bounds is the *nesting* of the tree and never the length of
/// anything. `a + b + c + …`, `a, b, c, …` and `a && b && …` are
/// left-associative, so each operand is a sibling rather than a level, and
/// the parser, [`crate::sema`] and [`crate::codegen`] all walk such a chain
/// iteratively: its length is bounded by memory alone, which is what lets a
/// logical source line hold the 4095 characters C23 5.2.5.2p1 asks for. Three
/// constructs are the other way round and *are* charged here, because each
/// operator is one more level of a tree every pass has to walk:
///
/// * the right-associative `a ? b : c ? d : e` and `a = b = c`
///   ([`Parser::parse_conditional_expr`],
///   [`Parser::parse_assignment_expr`]);
/// * a run of postfix operators, `p->a->b->c` and `a[i][j][k]`
///   ([`Parser::parse_postfix_suffixes`]), where each one is a place inside
///   the last.
///
/// 200 is three times the 63 levels of nesting C23 5.2.5.2p1 asks for and
/// close to Clang's own `-fbracket-depth` default of 256. Measured on an
/// unoptimised build against the 8 MiB `rustc` gives macro expansion, code
/// generation survives about 5000 levels of a conditional chain and about 450
/// of a `->` chain, which is the tightest of them; the margin is therefore
/// twofold at worst and twentyfold at best.
const MAX_RECURSION_DEPTH: u32 = 200;

/// How many labels one statement may carry.
///
/// A label chain is parsed iteratively, so it costs the parser nothing — but
/// each label is still a level of the tree that sema, the CFG lowering and
/// code generation walk recursively, and something has to bound that. C23
/// 5.2.5.2p1 asks for 1023 `case` labels in one `switch`; this is four times
/// that, and a chain longer than it is a diagnostic rather than a crash.
const MAX_LABEL_CHAIN: usize = 4096;

/// One label of a chain, held while the statement it labels is parsed.
enum PendingLabel {
    /// `name:`, with the range of its `:`.
    Ident { label: Ident, colon: SourceRange },
    /// `case value:`, and GNU's `case low ... high:`.
    Case { value: Expr, upper: Option<Expr> },
    /// `default:`
    Default,
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    diags: &'a mut Diagnostics,
    scopes: Vec<Scope>,
    /// Which revision's grammar to accept; see [`Parser::require_standard`].
    standard: Standard,
    /// How that revision gates a newer one's features.
    gating: Gating,
    /// What `#pragma pack` was asking for, by token position.
    packing: &'a PackMap,
    /// Range of the most recently consumed token, used to close node ranges.
    last_range: SourceRange,
    /// Current recursion depth; reset at every external declaration.
    depth: u32,
    /// The `struct`/`union` specifiers seen so far; see [`RecordSpecId`].
    records: Vec<RecordSpec>,
    /// The `enum` specifiers seen so far.
    enums: Vec<EnumSpec>,
    /// The `typeof` operands seen so far.
    typeofs: Vec<TypeofOperand>,
}

// ---------------------------------------------------------------------------
// token helpers
// ---------------------------------------------------------------------------

impl Parser<'_> {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn nth(&self, n: usize) -> &Token {
        let i = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[i]
    }

    fn cur_range(&self) -> SourceRange {
        self.peek().range
    }

    fn describe_cur(&self) -> String {
        self.peek().kind.describe()
    }

    fn at_eof(&self) -> bool {
        self.peek().is_eof()
    }

    fn at_punct(&self, p: Punct) -> bool {
        self.peek().is_punct(p)
    }

    fn at_keyword(&self, k: Keyword) -> bool {
        self.peek().is_keyword(k)
    }

    fn advance(&mut self) {
        self.last_range = self.tokens[self.pos].range;
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn bump_range(&mut self) -> SourceRange {
        let range = self.cur_range();
        self.advance();
        range
    }

    fn eat_punct(&mut self, p: Punct) -> Option<SourceRange> {
        self.at_punct(p).then(|| self.bump_range())
    }

    fn eat_keyword(&mut self, k: Keyword) -> Option<SourceRange> {
        self.at_keyword(k).then(|| self.bump_range())
    }

    fn eat_ident(&mut self) -> Option<Ident> {
        let name = self.peek().ident()?.to_owned();
        let range = self.bump_range();
        Some(Ident { name, range })
    }

    fn error(&mut self, range: SourceRange, message: impl Into<String>) {
        self.diags.error(range, message);
    }

    fn error_bail(&mut self, range: SourceRange, message: impl Into<String>) -> Bail {
        self.diags.error(range, message);
        Bail
    }

    fn expect_punct(&mut self, p: Punct, ctx: &str) -> PResult<SourceRange> {
        if self.at_punct(p) {
            return Ok(self.bump_range());
        }
        let range = self.cur_range();
        let found = self.describe_cur();
        Err(self.error_bail(
            range,
            format!("expected '{}'{ctx}, found {found}", p.as_str()),
        ))
    }

    fn expect_ident(&mut self, ctx: &str) -> PResult<Ident> {
        if let Some(id) = self.eat_ident() {
            return Ok(id);
        }
        let range = self.cur_range();
        let found = self.describe_cur();
        Err(self.error_bail(range, format!("expected identifier{ctx}, found {found}")))
    }

    /// Range from `start` up to and including the last consumed token.
    fn span_to_here(&self, start: SourceRange) -> SourceRange {
        start.join(self.last_range)
    }

    /// Enters one level of recursion.
    ///
    /// The counter is only decremented on the success path; an error unwinds
    /// all the way to the top level, which resets it.
    fn enter(&mut self) -> PResult<()> {
        self.depth += 1;
        if self.depth > MAX_RECURSION_DEPTH {
            let range = self.cur_range();
            return Err(self.error_bail(range, "this construct nests too deeply"));
        }
        Ok(())
    }

    /// Leaves one level of recursion.
    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Reports a construct the block's own standard does not have.
    ///
    /// Parsing continues either way: the shape of the code is known, and
    /// carrying on means one diagnostic that says exactly what to change
    /// instead of a cascade of syntax errors after it.
    fn require_standard(&mut self, needed: Standard, what: &str, range: SourceRange) {
        if let Some(message) = self.gating.requires(what, needed) {
            self.error(range, message);
        }
    }

    /// Reports a keyword the block's own standard does not have.
    fn require_keyword(&mut self, k: Keyword, range: SourceRange) {
        let needed = k.since();
        self.require_standard(needed, &format!("'{}'", k.as_str()), range);
    }

    /// The gate message for the identifier at the current position, if a
    /// newer revision would have made it a keyword.
    fn newer_keyword_here(&self) -> Option<String> {
        self.gating.newer_keyword(self.peek().ident()?)
    }
}

// ---------------------------------------------------------------------------
// scopes / the lexer hack
// ---------------------------------------------------------------------------

impl Parser<'_> {
    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str, kind: SymKind) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.syms.insert(name.to_owned(), kind);
        }
    }

    /// Whether `name` currently names a type.
    fn is_typedef_name(&self, name: &str) -> bool {
        for scope in self.scopes.iter().rev() {
            if let Some(kind) = scope.syms.get(name) {
                return *kind == SymKind::Typedef;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// C23 attributes and `_Static_assert`
// ---------------------------------------------------------------------------

impl Parser<'_> {
    /// Whether an attribute specifier sequence starts here.
    ///
    /// Both spellings count: C23's `[[…]]` and GNU's `__attribute__((…))`,
    /// which mean the same things and are parsed by the same code.
    fn at_attributes(&self) -> bool {
        (self.at_punct(Punct::LBracket) && self.nth(1).is_punct(Punct::LBracket))
            || self.at_keyword(Keyword::Attribute)
    }

    /// Consumes every attribute specifier here, keeping what is acted on.
    ///
    /// An attribute the front end does not know is dropped, which C23
    /// 6.7.13.1p3 explicitly allows and which is what GCC does with a warning
    /// this crate has no way to raise; one it knows but cannot honour —
    /// `weak`, `cleanup`, `vector_size` — is refused, because ignoring it
    /// would change what the program means.
    fn parse_attributes(&mut self) -> PResult<Attributes> {
        let mut attrs = Attributes::default();
        loop {
            if self.at_keyword(Keyword::Attribute) {
                let start = self.bump_range();
                self.expect_punct(Punct::LParen, " after '__attribute__'")?;
                self.expect_punct(Punct::LParen, " after '__attribute__('")?;
                self.parse_attribute_list(&mut attrs, Punct::RParen)?;
                self.expect_punct(Punct::RParen, " to close '__attribute__'")?;
                self.expect_punct(Punct::RParen, " to close '__attribute__'")?;
                let _ = start;
                continue;
            }
            if self.at_punct(Punct::LBracket) && self.nth(1).is_punct(Punct::LBracket) {
                let start = self.cur_range();
                self.require_standard(Standard::C23, "an attribute specifier", start);
                self.advance(); // `[`
                self.advance(); // `[`
                self.parse_attribute_list(&mut attrs, Punct::RBracket)?;
                self.expect_punct(Punct::RBracket, " to close an attribute specifier")?;
                self.expect_punct(Punct::RBracket, " to close an attribute specifier")?;
                continue;
            }
            return Ok(attrs);
        }
    }

    /// `name (args)? , name (args)? , …`, up to `close`.
    fn parse_attribute_list(&mut self, attrs: &mut Attributes, close: Punct) -> PResult<()> {
        loop {
            if self.at_punct(close) || self.at_eof() {
                return Ok(());
            }
            // An empty element is legal in GNU's list: `__attribute__((,))`.
            if self.eat_punct(Punct::Comma).is_some() {
                continue;
            }
            self.parse_one_attribute(attrs, close)?;
            if self.eat_punct(Punct::Comma).is_none() {
                return Ok(());
            }
        }
    }

    /// One attribute, with its argument clause if it has one.
    fn parse_one_attribute(&mut self, attrs: &mut Attributes, close: Punct) -> PResult<()> {
        let start = self.cur_range();
        // The name may be a keyword — `__attribute__((const))`, `[[noreturn]]`
        // — and C23 allows a `vendor::` prefix, which is skipped.
        let mut name = match &self.peek().kind {
            TokenKind::Ident(name) => name.clone(),
            TokenKind::Keyword(k) => k.as_str().to_owned(),
            _ => {
                let found = self.describe_cur();
                return Err(self.error_bail(start, format!("expected an attribute, found {found}")));
            }
        };
        self.advance();
        if self.at_punct(Punct::Colon) && self.nth(1).is_punct(Punct::Colon) {
            self.advance();
            self.advance();
            let prefix = std::mem::take(&mut name);
            name = match &self.peek().kind {
                TokenKind::Ident(name) => name.clone(),
                TokenKind::Keyword(k) => k.as_str().to_owned(),
                _ => {
                    let found = self.describe_cur();
                    return Err(
                        self.error_bail(start, format!("expected an attribute, found {found}"))
                    );
                }
            };
            self.advance();
            // Only the GNU namespace names attributes this front end knows;
            // anything else is another vendor's and is ignored.
            if prefix != "gnu" && prefix != "clang" {
                self.skip_attribute_args()?;
                return Ok(());
            }
        }

        let known = gnu::attribute(&name);
        // Only three attributes have arguments this front end reads; every
        // other clause may hold anything at all — `format(printf, 1, 2)` names
        // a *mode* rather than a value — and is skipped as balanced tokens.
        match known {
            Some(gnu::Attribute::Aligned) => {
                let alignment = if self.at_punct(Punct::LParen) {
                    self.advance();
                    let expr = self.parse_conditional_expr()?;
                    self.expect_punct(Punct::RParen, " after the alignment")?;
                    AlignmentKind::Expr(expr)
                } else {
                    // Bare `aligned` asks for the biggest alignment any type on
                    // the target needs, which is 16 on every ABI here.
                    AlignmentKind::Expr(int_expr(16, start))
                };
                let range = self.span_to_here(start);
                attrs.aligned = Some(Alignment {
                    kind: alignment,
                    range,
                });
                return Ok(());
            }
            Some(gnu::Attribute::Deprecated) => {
                let message = self.attribute_string()?;
                let range = self.span_to_here(start);
                attrs.deprecated = Some(Spanned::new(message, range));
                return Ok(());
            }
            Some(gnu::Attribute::Section) => {
                let name = self.attribute_string()?;
                let range = self.span_to_here(start);
                match name {
                    Some(name) => attrs.section = Some(Spanned::new(name, range)),
                    None => self.error(range, "'section' takes one string literal"),
                }
                return Ok(());
            }
            _ => {}
        }
        self.skip_attribute_args()?;
        let range = self.span_to_here(start);
        match known {
            Some(gnu::Attribute::Noreturn) => attrs.noreturn = attrs.noreturn.or(Some(range)),
            Some(gnu::Attribute::AlwaysInline) => {
                attrs.always_inline = attrs.always_inline.or(Some(range));
            }
            Some(gnu::Attribute::NoInline) => attrs.noinline = attrs.noinline.or(Some(range)),
            Some(gnu::Attribute::Cold) => attrs.cold = attrs.cold.or(Some(range)),
            // GCC's `hot` is the opposite of `cold`, and the two cancel.
            Some(gnu::Attribute::Hot) => attrs.cold = None,
            Some(gnu::Attribute::Packed) => attrs.packed = attrs.packed.or(Some(range)),
            Some(gnu::Attribute::Constructor) => {
                attrs.constructor = attrs.constructor.or(Some(range));
            }
            Some(gnu::Attribute::Destructor) => {
                attrs.destructor = attrs.destructor.or(Some(range));
            }
            // A statement attribute with nothing to say here: a `switch` group
            // falls through in the generated Rust either way.
            Some(gnu::Attribute::Fallthrough) | Some(gnu::Attribute::Ignored) => {}
            Some(gnu::Attribute::Unsupported) => {
                let reason = gnu::unsupported_reason(&name).unwrap_or("is not supported");
                self.error(range, format!("'{name}' {reason}"));
            }
            // Everything above was handled; an unknown attribute is ignored,
            // as C23 requires.
            _ => {}
        }
        let _ = close;
        Ok(())
    }

    /// The single string literal an attribute's argument clause holds, if it
    /// has one at all.
    fn attribute_string(&mut self) -> PResult<Option<String>> {
        if !self.at_punct(Punct::LParen) {
            return Ok(None);
        }
        self.advance();
        let mut text = None;
        if let TokenKind::Str(lit) = self.peek().kind.clone() {
            let range = self.cur_range();
            let literal = self.parse_string_literal(lit, range);
            if let ExprKind::Str(lit) = literal.kind {
                text = String::from_utf8(lit.values.iter().map(|v| *v as u8).collect()).ok();
            }
        }
        // Anything else — a priority, an unknown option — is skipped.
        let mut depth = 1i32;
        while depth > 0 && !self.at_eof() {
            if self.at_punct(Punct::LParen) {
                depth += 1;
            } else if self.at_punct(Punct::RParen) {
                depth -= 1;
                if depth == 0 {
                    self.advance();
                    break;
                }
            }
            self.advance();
        }
        Ok(text)
    }

    /// Skips a balanced argument clause without looking inside it.
    fn skip_attribute_args(&mut self) -> PResult<()> {
        if !self.at_punct(Punct::LParen) {
            return Ok(());
        }
        let start = self.cur_range();
        let mut depth = 0i32;
        while !self.at_eof() {
            if self.at_punct(Punct::LParen) {
                depth += 1;
            } else if self.at_punct(Punct::RParen) {
                depth -= 1;
                if depth == 0 {
                    self.advance();
                    return Ok(());
                }
            }
            self.advance();
        }
        Err(self.error_bail(start, "unterminated attribute argument list"))
    }

    /// Whether a `_Static_assert` declaration starts here.
    fn at_static_assert(&self) -> bool {
        matches!(
            self.peek().keyword(),
            Some(Keyword::StaticAssert | Keyword::StaticAssertName)
        )
    }

    /// `_Static_assert ( constant-expression , "message" ) ;`, whose message
    /// C23 makes optional.
    fn parse_static_assert(&mut self) -> PResult<StaticAssert> {
        let start = self.cur_range();
        let keyword = self.peek().keyword().expect("the caller checked");
        self.require_keyword(keyword, start);
        self.advance();
        let name = keyword.as_str();
        self.expect_punct(Punct::LParen, &format!(" after '{name}'"))?;
        let cond = self.parse_conditional_expr()?;
        let mut message = None;
        if self.eat_punct(Punct::Comma).is_some() {
            let range = self.cur_range();
            let TokenKind::Str(first) = self.peek().kind.clone() else {
                let found = self.describe_cur();
                return Err(self.error_bail(
                    range,
                    format!("expected a string literal as the message of '{name}', found {found}"),
                ));
            };
            let literal = self.parse_string_literal(first, range);
            if let ExprKind::Str(lit) = literal.kind {
                message = Some(lit.text);
            }
        } else {
            self.require_standard(
                Standard::C23,
                &format!("'{name}' without a message"),
                self.span_to_here(start),
            );
        }
        self.expect_punct(Punct::RParen, &format!(" to close '{name}'"))?;
        self.expect_punct(Punct::Semi, &format!(" after '{name}'"))?;
        Ok(StaticAssert {
            cond,
            message,
            range: self.span_to_here(start),
        })
    }
}

// ---------------------------------------------------------------------------
// top level
// ---------------------------------------------------------------------------

impl Parser<'_> {
    fn parse_translation_unit(&mut self, range: SourceRange) -> TranslationUnit {
        let mut items = Vec::new();
        while !self.at_eof() {
            let before = self.pos;
            self.depth = 0;
            match self.parse_external_decl() {
                Ok(item) => items.push(item),
                Err(Bail) => self.recover_top_level(before),
            }
            if self.pos == before {
                self.advance();
            }
        }
        TranslationUnit {
            items,
            records: std::mem::take(&mut self.records),
            enums: std::mem::take(&mut self.enums),
            typeofs: std::mem::take(&mut self.typeofs),
            range,
        }
    }

    /// How deeply nested in brackets the current position is, relative to the
    /// token at `start`.
    fn depth_from(&self, start: usize) -> i32 {
        let mut depth = 0i32;
        for tok in &self.tokens[start.min(self.pos)..self.pos] {
            match &tok.kind {
                TokenKind::Punct(Punct::LBrace | Punct::LParen | Punct::LBracket) => depth += 1,
                TokenKind::Punct(Punct::RBrace | Punct::RParen | Punct::RBracket) => depth -= 1,
                _ => {}
            }
        }
        depth.max(0)
    }

    /// Skips forward to just past the `;` or `}` that ends the external
    /// declaration that started at token `decl_start`.
    ///
    /// Starting from the nesting depth the error was found at (rather than
    /// from zero) is what keeps a single broken statement inside a function
    /// body from producing a cascade of errors for the rest of the body.
    fn recover_top_level(&mut self, decl_start: usize) {
        let mut depth = self.depth_from(decl_start);
        while !self.at_eof() {
            match &self.peek().kind {
                TokenKind::Punct(Punct::LBrace | Punct::LParen | Punct::LBracket) => {
                    depth += 1;
                    self.advance();
                }
                TokenKind::Punct(Punct::RBrace | Punct::RParen | Punct::RBracket) => {
                    depth -= 1;
                    self.advance();
                    if depth <= 0 {
                        // `struct S { int x };` — swallow the `;` that closes
                        // the declaration so that it is not mistaken for the
                        // start of the next one.
                        self.eat_punct(Punct::Semi);
                        return;
                    }
                }
                TokenKind::Punct(Punct::Semi) => {
                    self.advance();
                    if depth <= 0 {
                        return;
                    }
                }
                _ => self.advance(),
            }
        }
    }

    fn parse_external_decl(&mut self) -> PResult<ExternalDecl> {
        let start = self.cur_range();
        // `__extension__` marks what follows as a GNU extension and asks for
        // the pedantic warnings to be held back; there are none to hold back.
        while self.eat_keyword(Keyword::Extension).is_some() {}
        let attrs = self.parse_attributes()?;
        if self.at_static_assert() {
            return Ok(ExternalDecl::StaticAssert(self.parse_static_assert()?));
        }
        let mut specs = self.parse_decl_specifiers(true)?;
        specs.attrs.merge(attrs);
        specs.noreturn = specs.noreturn.or(specs.attrs.noreturn);

        if let Some(semi) = self.eat_punct(Punct::Semi) {
            return Ok(ExternalDecl::Decl(Decl {
                specifiers: specs,
                declarators: Vec::new(),
                range: start.join(semi),
            }));
        }

        let mut first = self.parse_declarator(specs.base.clone(), false)?;
        self.parse_declarator_tail(&mut first)?;

        let looks_like_definition = matches!(first.ty.kind, TypeKind::Function(_))
            && (self.at_punct(Punct::LBrace) || self.starts_declaration());
        if looks_like_definition && !specs.is_typedef() {
            return self.finish_function_def(specs, first, start);
        }

        let decl = self.finish_declaration(specs, Some(first), start)?;
        Ok(ExternalDecl::Decl(decl))
    }

    /// `__asm__("symbol")` and `__attribute__((…))`, which may follow any
    /// declarator and in that order.
    fn parse_declarator_tail(&mut self, declarator: &mut DeclaratorResult) -> PResult<()> {
        loop {
            if self.at_keyword(Keyword::Asm) {
                let start = self.cur_range();
                self.advance();
                self.expect_punct(Punct::LParen, " after 'asm'")?;
                let range = self.cur_range();
                let TokenKind::Str(lit) = self.peek().kind.clone() else {
                    let found = self.describe_cur();
                    return Err(self.error_bail(
                        range,
                        format!("expected the symbol name as a string literal, found {found}"),
                    ));
                };
                let literal = self.parse_string_literal(lit, range);
                self.expect_punct(Punct::RParen, " after the symbol name")?;
                if let ExprKind::Str(lit) = literal.kind
                    && let Ok(name) =
                        String::from_utf8(lit.values.iter().map(|v| *v as u8).collect())
                {
                    declarator.asm_label = Some(Spanned::new(name, self.span_to_here(start)));
                }
                continue;
            }
            if self.at_attributes() {
                let attrs = self.parse_attributes()?;
                declarator.attrs.merge(attrs);
                continue;
            }
            return Ok(());
        }
    }

    fn finish_function_def(
        &mut self,
        specs: DeclSpecifiers,
        declarator: DeclaratorResult,
        start: SourceRange,
    ) -> PResult<ExternalDecl> {
        let Some(name) = declarator.name.clone() else {
            return Err(self.error_bail(declarator.range, "function definition requires a name"));
        };
        self.declare(&name.name, SymKind::Ordinary);

        // Parameters (and old-style parameter declarations) share a scope with
        // the body's outermost block.
        self.push_scope();
        if let TypeKind::Function(ft) = &declarator.ty.kind {
            for param in &ft.params {
                if let Some(pname) = &param.name {
                    self.scopes
                        .last_mut()
                        .expect("scope stack is never empty")
                        .syms
                        .insert(pname.name.clone(), SymKind::Ordinary);
                }
            }
            for kr in &ft.kr_names {
                self.scopes
                    .last_mut()
                    .expect("scope stack is never empty")
                    .syms
                    .insert(kr.name.clone(), SymKind::Ordinary);
            }
        }

        let mut kr_decls = Vec::new();
        while self.starts_declaration() {
            match self.parse_declaration() {
                Ok(decl) => kr_decls.push(decl),
                Err(bail) => {
                    self.pop_scope();
                    return Err(bail);
                }
            }
        }

        let body = match self.parse_compound_stmt() {
            Ok(body) => body,
            Err(bail) => {
                self.pop_scope();
                return Err(bail);
            }
        };
        self.pop_scope();

        Ok(ExternalDecl::Function(FunctionDef {
            specifiers: specs,
            name,
            ty: declarator.ty,
            kr_decls,
            attrs: declarator.attrs,
            asm_label: declarator.asm_label,
            body,
            range: self.span_to_here(start),
        }))
    }

    /// Parses `declarator (= initializer)? (, declarator (= initializer)?)* ;`.
    fn finish_declaration(
        &mut self,
        specs: DeclSpecifiers,
        first: Option<DeclaratorResult>,
        start: SourceRange,
    ) -> PResult<Decl> {
        let is_typedef = specs.is_typedef();
        let mut declarators = Vec::new();
        let mut pending = first;
        loop {
            let mut declarator = match pending.take() {
                Some(d) => d,
                None => {
                    let mut d = self.parse_declarator(specs.base.clone(), false)?;
                    self.parse_declarator_tail(&mut d)?;
                    d
                }
            };
            if let Some(name) = &declarator.name {
                let kind = if is_typedef {
                    SymKind::Typedef
                } else {
                    SymKind::Ordinary
                };
                self.declare(&name.name.clone(), kind);
            }
            let init = if self.eat_punct(Punct::Assign).is_some() {
                Some(self.parse_initializer()?)
            } else {
                None
            };
            // GCC lets the attributes come after the initialiser too.
            if self.at_attributes() {
                let attrs = self.parse_attributes()?;
                declarator.attrs.merge(attrs);
            }
            let range = self.span_to_here(declarator.range);
            declarators.push(InitDeclarator {
                name: declarator.name,
                ty: declarator.ty,
                init,
                attrs: declarator.attrs,
                asm_label: declarator.asm_label,
                range,
            });
            if self.eat_punct(Punct::Comma).is_none() {
                break;
            }
        }
        let semi = self.expect_punct(Punct::Semi, " after declaration")?;
        Ok(Decl {
            specifiers: specs,
            declarators,
            range: start.join(semi),
        })
    }

    fn parse_declaration(&mut self) -> PResult<Decl> {
        let start = self.cur_range();
        while self.eat_keyword(Keyword::Extension).is_some() {}
        let attrs = self.parse_attributes()?;
        let mut specs = self.parse_decl_specifiers(true)?;
        specs.attrs.merge(attrs);
        specs.noreturn = specs.noreturn.or(specs.attrs.noreturn);
        if let Some(semi) = self.eat_punct(Punct::Semi) {
            return Ok(Decl {
                specifiers: specs,
                declarators: Vec::new(),
                range: start.join(semi),
            });
        }
        self.finish_declaration(specs, None, start)
    }
}

// ---------------------------------------------------------------------------
// declaration specifiers
// ---------------------------------------------------------------------------

/// Counters for the combinable type-specifier keywords.
#[derive(Default)]
struct SpecCounts {
    void: u32,
    char: u32,
    short: u32,
    int: u32,
    long: u32,
    float: u32,
    double: u32,
    signed: u32,
    unsigned: u32,
    bool: u32,
    complex: u32,
    imaginary: u32,
}

impl SpecCounts {
    fn any(&self) -> bool {
        self.void
            + self.char
            + self.short
            + self.int
            + self.long
            + self.float
            + self.double
            + self.signed
            + self.unsigned
            + self.bool
            + self.complex
            + self.imaginary
            > 0
    }
}

impl Parser<'_> {
    /// Whether the current token can begin a declaration.
    fn starts_declaration(&self) -> bool {
        self.starts_decl_specifier(0)
    }

    /// Whether the token `n` positions ahead can begin a declaration
    /// specifier (or, for a type name, a specifier-qualifier list).
    fn starts_decl_specifier(&self, n: usize) -> bool {
        let tok = self.nth(n);
        if let Some(k) = tok.keyword() {
            return matches!(
                k,
                Keyword::Typedef
                    | Keyword::Extern
                    | Keyword::Static
                    | Keyword::Auto
                    | Keyword::Register
                    | Keyword::Const
                    | Keyword::Volatile
                    | Keyword::Restrict
                    | Keyword::Inline
                    | Keyword::Void
                    | Keyword::Char
                    | Keyword::Short
                    | Keyword::Int
                    | Keyword::Long
                    | Keyword::Float
                    | Keyword::Double
                    | Keyword::Signed
                    | Keyword::Unsigned
                    | Keyword::Bool
                    | Keyword::Complex
                    | Keyword::Imaginary
                    | Keyword::Struct
                    | Keyword::Union
                    | Keyword::Enum
                    | Keyword::Alignas
                    | Keyword::AlignasName
                    | Keyword::Atomic
                    | Keyword::BitInt
                    | Keyword::Noreturn
                    | Keyword::ThreadLocal
                    | Keyword::ThreadLocalName
                    | Keyword::Constexpr
                    | Keyword::Typeof
                    | Keyword::TypeofUnqual
                    | Keyword::BoolName
                    | Keyword::Attribute
                    | Keyword::Extension
                    | Keyword::TypeofGnu
                    | Keyword::TypeofUnqualGnu
                    | Keyword::AutoType
                    | Keyword::ThreadGnu
            );
        }
        match tok.ident() {
            Some(NORETURN_BUILTIN) => true,
            // A label such as `done:` must not look like a declaration even
            // when it happens to share a name with a `typedef`.
            Some(name) => self.is_typedef_name(name) && !self.nth(n + 1).is_punct(Punct::Colon),
            None => false,
        }
    }

    fn eat_type_qualifier(&mut self) -> Option<TypeQualifiers> {
        let q = match self.peek().keyword()? {
            Keyword::Const => TypeQualifiers {
                is_const: true,
                ..TypeQualifiers::NONE
            },
            Keyword::Volatile => TypeQualifiers {
                is_volatile: true,
                ..TypeQualifiers::NONE
            },
            Keyword::Restrict => TypeQualifiers {
                is_restrict: true,
                ..TypeQualifiers::NONE
            },
            _ => return None,
        };
        self.advance();
        Some(q)
    }

    fn parse_type_qualifiers(&mut self) -> TypeQualifiers {
        let mut quals = TypeQualifiers::NONE;
        while let Some(q) = self.eat_type_qualifier() {
            quals = quals.merge(q);
        }
        quals
    }

    /// Parses a declaration-specifier list (or, with `allow_storage == false`,
    /// a specifier-qualifier list).
    fn parse_decl_specifiers(&mut self, allow_storage: bool) -> PResult<DeclSpecifiers> {
        let start = self.cur_range();
        let mut storage: Option<Spanned<StorageClass>> = None;
        let mut inline = false;
        let mut noreturn: Option<SourceRange> = None;
        let mut alignas: Option<Alignment> = None;
        let mut attributes = Attributes::default();
        let mut quals = TypeQualifiers::NONE;
        let mut counts = SpecCounts::default();
        let mut tag: Option<Type> = None;
        let mut typedef_name: Option<Ident> = None;
        let mut auto_type: Option<SourceRange> = None;
        let mut consumed_any = false;

        loop {
            let has_type = counts.any() || tag.is_some() || typedef_name.is_some();
            // C23 allows an attribute specifier sequence among the specifiers,
            // and GNU's `__attribute__((…))` goes in the same places.
            if self.at_attributes() {
                let attrs = self.parse_attributes()?;
                noreturn = noreturn.or(attrs.noreturn);
                attributes.merge(attrs);
                consumed_any = true;
                continue;
            }
            if self.at_keyword(Keyword::Extension) {
                self.advance();
                consumed_any = true;
                continue;
            }
            if let Some(k) = self.peek().keyword() {
                let storage_class = match k {
                    Keyword::Typedef => Some(StorageClass::Typedef),
                    Keyword::Extern => Some(StorageClass::Extern),
                    Keyword::Static => Some(StorageClass::Static),
                    Keyword::Auto => Some(StorageClass::Auto),
                    Keyword::Register => Some(StorageClass::Register),
                    Keyword::ThreadLocal | Keyword::ThreadLocalName | Keyword::ThreadGnu => {
                        Some(StorageClass::ThreadLocal)
                    }
                    Keyword::Constexpr => Some(StorageClass::Constexpr),
                    _ => None,
                };
                if let Some(sc) = storage_class {
                    let range = self.bump_range();
                    self.require_keyword(k, range);
                    consumed_any = true;
                    if !allow_storage {
                        self.error(
                            range,
                            format!("storage class '{}' is not allowed here", sc.as_str()),
                        );
                    } else if let Some(prev) = &storage {
                        self.error(
                            range,
                            format!(
                                "cannot combine storage class '{}' with '{}'",
                                sc.as_str(),
                                prev.node.as_str()
                            ),
                        );
                    } else {
                        storage = Some(Spanned::new(sc, range));
                    }
                    continue;
                }
                if let Some(q) = self.eat_type_qualifier() {
                    quals = quals.merge(q);
                    consumed_any = true;
                    continue;
                }
                if k == Keyword::Inline {
                    self.advance();
                    inline = true;
                    consumed_any = true;
                    continue;
                }
                if k == Keyword::Noreturn {
                    let range = self.bump_range();
                    self.require_keyword(k, range);
                    noreturn = noreturn.or(Some(range));
                    consumed_any = true;
                    continue;
                }
                if matches!(k, Keyword::Alignas | Keyword::AlignasName) {
                    let spec = self.parse_alignment_specifier(k)?;
                    if alignas.is_none() {
                        alignas = Some(spec);
                    }
                    consumed_any = true;
                    continue;
                }
                // GNU's `__auto_type`, which is C23's `auto` under another
                // name and needs no entry point of its own.
                if k == Keyword::AutoType {
                    let range = self.bump_range();
                    auto_type = auto_type.or(Some(range));
                    consumed_any = true;
                    continue;
                }
                if matches!(
                    k,
                    Keyword::Typeof
                        | Keyword::TypeofUnqual
                        | Keyword::TypeofGnu
                        | Keyword::TypeofUnqualGnu
                ) {
                    let ty = self.parse_typeof_specifier(k)?;
                    if tag.is_some() || has_type {
                        self.error(ty.range, "two or more data types in declaration specifiers");
                    } else {
                        tag = Some(ty);
                    }
                    consumed_any = true;
                    continue;
                }
                // `_Atomic` and `_BitInt` are parsed so that the diagnostic is
                // about them rather than about the tokens that follow.
                if matches!(k, Keyword::Atomic | Keyword::BitInt) {
                    let range = self.bump_range();
                    self.error(range, format!("'{}' is not supported yet", k.as_str()));
                    if self.at_punct(Punct::LParen) {
                        self.advance();
                        if k == Keyword::Atomic {
                            let inner = self.parse_type_name()?;
                            if tag.is_none() && !has_type {
                                tag = Some(inner.ty);
                            }
                        } else {
                            let _ = self.parse_conditional_expr()?;
                        }
                        self.expect_punct(Punct::RParen, " after the operand")?;
                    }
                    if k == Keyword::BitInt {
                        // Recover as `int`, so that the declaration does not
                        // also complain about a missing type specifier.
                        counts.int += 1;
                    }
                    consumed_any = true;
                    continue;
                }
                let counter = match k {
                    Keyword::Void => Some(&mut counts.void),
                    Keyword::Char => Some(&mut counts.char),
                    Keyword::Short => Some(&mut counts.short),
                    Keyword::Int => Some(&mut counts.int),
                    Keyword::Long => Some(&mut counts.long),
                    Keyword::Float => Some(&mut counts.float),
                    Keyword::Double => Some(&mut counts.double),
                    Keyword::Signed => Some(&mut counts.signed),
                    Keyword::Unsigned => Some(&mut counts.unsigned),
                    Keyword::Bool | Keyword::BoolName => Some(&mut counts.bool),
                    Keyword::Complex => Some(&mut counts.complex),
                    Keyword::Imaginary => Some(&mut counts.imaginary),
                    _ => None,
                };
                if let Some(c) = counter {
                    *c += 1;
                    self.advance();
                    consumed_any = true;
                    continue;
                }
                if matches!(k, Keyword::Struct | Keyword::Union) {
                    let ty = self.parse_record_specifier()?;
                    if tag.is_some() || has_type {
                        self.error(ty.range, "two or more data types in declaration specifiers");
                    } else {
                        tag = Some(ty);
                    }
                    consumed_any = true;
                    continue;
                }
                if k == Keyword::Enum {
                    let ty = self.parse_enum_specifier()?;
                    if tag.is_some() || has_type {
                        self.error(ty.range, "two or more data types in declaration specifiers");
                    } else {
                        tag = Some(ty);
                    }
                    consumed_any = true;
                    continue;
                }
                break;
            }

            // The spelling of `_Noreturn` that every standard accepts, which
            // is what the bundled headers mark `exit` and `abort` with: they
            // are read by `c99!` blocks too, where `_Noreturn` itself would be
            // an error.
            if self.peek().ident() == Some(NORETURN_BUILTIN) {
                let range = self.bump_range();
                noreturn = noreturn.or(Some(range));
                consumed_any = true;
                continue;
            }

            // A `typedef` name is a type specifier only while we do not have
            // one yet; otherwise it is the declarator's identifier.
            let is_typedef_use = match self.peek().ident() {
                Some(name) => !has_type && self.is_typedef_name(name),
                None => false,
            };
            if is_typedef_use {
                let id = self.eat_ident().expect("checked above");
                typedef_name = Some(id);
                consumed_any = true;
                continue;
            }
            break;
        }

        if !consumed_any {
            let range = self.cur_range();
            // A name a newer revision would have made a keyword is almost
            // always that keyword rather than a declaration nobody finished
            // writing, and saying so is the difference between a fix and a
            // puzzle.
            if let Some(message) = self.newer_keyword_here() {
                return Err(self.error_bail(range, message));
            }
            let found = self.describe_cur();
            return Err(self.error_bail(range, format!("expected a declaration, found {found}")));
        }

        let specs_range = self.span_to_here(start);
        // C23's `auto x = e;` and GNU's `__auto_type x = e;`: a declaration
        // with no type specifier at all takes its type from the initialiser.
        let no_type = !counts.any() && tag.is_none() && typedef_name.is_none();
        let inferred = no_type
            && (auto_type.is_some()
                || (self.standard >= Standard::C23
                    && matches!(
                        storage,
                        Some(Spanned {
                            node: StorageClass::Auto,
                            ..
                        })
                    )));
        let base = if inferred {
            Type::new(TypeKind::Auto, quals, specs_range)
        } else {
            self.build_base_type(&counts, tag, typedef_name, quals, specs_range)
        };
        // `__attribute__((aligned(N)))` on a declaration says exactly what
        // `_Alignas(N)` says, so the two go through one path.
        let alignas = alignas.or_else(|| attributes.aligned.clone());

        Ok(DeclSpecifiers {
            storage,
            inline,
            noreturn,
            alignas,
            attrs: attributes,
            base,
            range: specs_range,
        })
    }

    /// `_Alignas ( constant-expression )` or `_Alignas ( type-name )`.
    fn parse_alignment_specifier(&mut self, keyword: Keyword) -> PResult<Alignment> {
        let start = self.cur_range();
        self.require_keyword(keyword, start);
        self.advance();
        let name = keyword.as_str();
        self.expect_punct(Punct::LParen, &format!(" after '{name}'"))?;
        let kind = if self.starts_declaration() {
            AlignmentKind::Type(Box::new(self.parse_type_name()?))
        } else {
            AlignmentKind::Expr(self.parse_conditional_expr()?)
        };
        self.expect_punct(Punct::RParen, &format!(" after the operand of '{name}'"))?;
        Ok(Alignment {
            kind,
            range: self.span_to_here(start),
        })
    }

    /// `typeof ( expression )` or `typeof ( type-name )`.
    ///
    /// `typeof_unqual` parses the same way and means the same thing here: the
    /// only qualifier this crate keeps in a type is the `const` of a pointee,
    /// which is not a top-level qualifier and which neither form strips.
    fn parse_typeof_specifier(&mut self, keyword: Keyword) -> PResult<Type> {
        let start = self.cur_range();
        self.require_keyword(keyword, start);
        self.advance();
        let name = keyword.as_str();
        self.expect_punct(Punct::LParen, &format!(" after '{name}'"))?;
        let operand = if self.starts_declaration() {
            TypeofOperand::Type(self.parse_type_name()?)
        } else {
            TypeofOperand::Expr(self.parse_expr()?)
        };
        self.expect_punct(Punct::RParen, &format!(" after the operand of '{name}'"))?;
        let range = self.span_to_here(start);
        let id = self.add_typeof(operand);
        Ok(Type::plain(TypeKind::Typeof(id), range))
    }

    fn build_base_type(
        &mut self,
        counts: &SpecCounts,
        tag: Option<Type>,
        typedef_name: Option<Ident>,
        quals: TypeQualifiers,
        range: SourceRange,
    ) -> Type {
        if let Some(mut ty) = tag {
            if counts.any() || typedef_name.is_some() {
                self.error(range, "two or more data types in declaration specifiers");
            }
            ty.qualifiers = ty.qualifiers.merge(quals);
            return ty;
        }
        if let Some(name) = typedef_name {
            if counts.any() {
                self.error(range, "two or more data types in declaration specifiers");
            }
            return Type::new(TypeKind::Typedef(name), quals, range);
        }
        if !counts.any() {
            self.error(
                range,
                "type specifier missing; C99 does not support implicit 'int'",
            );
            return Type::new(
                TypeKind::Int {
                    sign: Sign::Signed,
                    size: IntSize::Int,
                },
                quals,
                range,
            );
        }

        let sign = if counts.unsigned > 0 {
            Some(Sign::Unsigned)
        } else if counts.signed > 0 {
            Some(Sign::Signed)
        } else {
            None
        };
        if counts.signed > 0 && counts.unsigned > 0 {
            self.error(range, "cannot combine 'signed' with 'unsigned'");
        }

        let kind = if counts.void > 0 {
            if counts.void > 1 || counts.any_besides(&["void"]) {
                self.error(range, "cannot combine 'void' with other type specifiers");
            }
            TypeKind::Void
        } else if counts.bool > 0 {
            if counts.any_besides(&["_Bool"]) {
                self.error(range, "cannot combine '_Bool' with other type specifiers");
            }
            TypeKind::Bool
        } else if counts.char > 0 {
            if counts.any_besides(&["char", "signed", "unsigned"]) {
                self.error(range, "cannot combine 'char' with other type specifiers");
            }
            TypeKind::Char(sign)
        } else if counts.float > 0 || counts.double > 0 {
            let size = if counts.float > 0 {
                if counts.double > 0 {
                    self.error(range, "cannot combine 'float' with 'double'");
                }
                if counts.long > 0 {
                    self.error(range, "cannot combine 'long' with 'float'");
                }
                FloatSize::Float
            } else if counts.long > 0 {
                FloatSize::LongDouble
            } else {
                FloatSize::Double
            };
            if sign.is_some() {
                self.error(
                    range,
                    "cannot combine 'signed' or 'unsigned' with a floating type",
                );
            }
            if counts.complex > 0 {
                TypeKind::Complex(size)
            } else if counts.imaginary > 0 {
                TypeKind::Imaginary(size)
            } else {
                TypeKind::Float(size)
            }
        } else if counts.complex > 0 || counts.imaginary > 0 {
            // `__complex__ x;` on its own means `double _Complex` in GNU C,
            // and saying so gets the honest "complex types are not supported"
            // rather than a complaint about a missing type specifier.
            if counts.complex > 0 {
                TypeKind::Complex(FloatSize::Double)
            } else {
                TypeKind::Imaginary(FloatSize::Double)
            }
        } else {
            let size = if counts.short > 0 {
                if counts.long > 0 {
                    self.error(range, "cannot combine 'short' with 'long'");
                }
                IntSize::Short
            } else {
                match counts.long {
                    0 => IntSize::Int,
                    1 => IntSize::Long,
                    2 => IntSize::LongLong,
                    _ => {
                        self.error(range, "'long long long' is too long for cinrs");
                        IntSize::LongLong
                    }
                }
            };
            TypeKind::Int {
                sign: sign.unwrap_or(Sign::Signed),
                size,
            }
        };

        Type::new(kind, quals, range)
    }
}

impl SpecCounts {
    /// Whether any counter outside `allowed` is non-zero.
    fn any_besides(&self, allowed: &[&str]) -> bool {
        let all: [(&str, u32); 12] = [
            ("void", self.void),
            ("char", self.char),
            ("short", self.short),
            ("int", self.int),
            ("long", self.long),
            ("float", self.float),
            ("double", self.double),
            ("signed", self.signed),
            ("unsigned", self.unsigned),
            ("_Bool", self.bool),
            ("_Complex", self.complex),
            ("_Imaginary", self.imaginary),
        ];
        all.iter()
            .any(|(name, count)| *count > 0 && !allowed.contains(name))
    }
}

// ---------------------------------------------------------------------------
// struct / union / enum
// ---------------------------------------------------------------------------

impl Parser<'_> {
    /// Stores a `struct`/`union` specifier and hands back its id.
    fn add_record(&mut self, spec: RecordSpec) -> RecordSpecId {
        let id = RecordSpecId(self.records.len() as u32);
        self.records.push(spec);
        id
    }

    /// Stores an `enum` specifier and hands back its id.
    fn add_enum(&mut self, spec: EnumSpec) -> EnumSpecId {
        let id = EnumSpecId(self.enums.len() as u32);
        self.enums.push(spec);
        id
    }

    /// Stores a `typeof` operand and hands back its id.
    fn add_typeof(&mut self, operand: TypeofOperand) -> TypeofId {
        let id = TypeofId(self.typeofs.len() as u32);
        self.typeofs.push(operand);
        id
    }

    /// A `struct`/`union` specifier, whose body may hold more of them.
    ///
    /// The nesting is counted: a member list is parsed by a recursive call, so
    /// a specifier nested past [`MAX_RECURSION_DEPTH`] is a diagnostic rather
    /// than a stack overflow. C23 5.2.5.2p1 asks for 63 levels.
    fn parse_record_specifier(&mut self) -> PResult<Type> {
        self.enter()?;
        let result = self.parse_record_specifier_inner();
        self.leave();
        result
    }

    fn parse_record_specifier_inner(&mut self) -> PResult<Type> {
        let start = self.cur_range();
        // What `#pragma pack` was asking for *here* is what applies to this
        // record; a pragma written after it changes nothing about it.
        let pack = self.packing.at(self.pos);
        let kind = match self.peek().keyword() {
            Some(Keyword::Struct) => RecordKind::Struct,
            Some(Keyword::Union) => RecordKind::Union,
            _ => unreachable!("caller checked the keyword"),
        };
        self.advance();
        let mut attrs = self.parse_attributes()?;
        let name = self.eat_ident();
        let mut asserts = Vec::new();
        let fields = if self.at_punct(Punct::LBrace) {
            let (fields, found) = self.parse_struct_body()?;
            asserts = found;
            Some(fields)
        } else {
            if name.is_none() {
                let range = self.cur_range();
                let found = self.describe_cur();
                return Err(self.error_bail(
                    range,
                    format!(
                        "expected identifier or '{{' after '{}', found {found}",
                        kind.as_str()
                    ),
                ));
            }
            None
        };
        // GCC lets `__attribute__((packed))` come after the member list too,
        // which is where most code writes it.
        if self.at_attributes() {
            let after = self.parse_attributes()?;
            attrs.merge(after);
        }
        let range = self.span_to_here(start);
        let id = self.add_record(RecordSpec {
            kind,
            name,
            fields,
            asserts,
            attrs,
            pack,
            range,
        });
        Ok(Type::plain(TypeKind::Record(id), range))
    }

    /// The member list of a `struct` or `union`, and the `_Static_assert`
    /// declarations written among the members.
    fn parse_struct_body(&mut self) -> PResult<(Vec<FieldDecl>, Vec<StaticAssert>)> {
        self.expect_punct(Punct::LBrace, " to open a member list")?;
        let mut fields = Vec::new();
        let mut asserts = Vec::new();
        while !self.at_punct(Punct::RBrace) && !self.at_eof() {
            let before = self.pos;
            // A stray `;` is harmless; skip it.
            if self.eat_punct(Punct::Semi).is_some() {
                continue;
            }
            if self.at_static_assert() {
                asserts.push(self.parse_static_assert()?);
                continue;
            }
            let start = self.cur_range();
            while self.eat_keyword(Keyword::Extension).is_some() {}
            let leading = self.parse_attributes()?;
            let mut specs = self.parse_decl_specifiers(false)?;
            specs.attrs.merge(leading);

            if self.at_punct(Punct::Semi) {
                // An anonymous struct/union member: C11 6.7.2.1p13.
                let range = self.span_to_here(start);
                self.require_standard(Standard::C11, "an anonymous struct or union member", range);
                fields.push(FieldDecl {
                    ty: specs.base.clone(),
                    attrs: specs.attrs.clone(),
                    specifiers: specs,
                    name: None,
                    bit_width: None,
                    range,
                });
                self.expect_punct(Punct::Semi, " after member declaration")?;
                continue;
            }

            loop {
                let (name, ty, dstart, mut attrs) = if self.at_punct(Punct::Colon) {
                    (
                        None,
                        specs.base.clone(),
                        self.cur_range(),
                        Attributes::default(),
                    )
                } else {
                    let mut d = self.parse_declarator(specs.base.clone(), true)?;
                    self.parse_declarator_tail(&mut d)?;
                    (d.name, d.ty, d.range, d.attrs)
                };
                let bit_width = if self.eat_punct(Punct::Colon).is_some() {
                    Some(self.parse_conditional_expr()?)
                } else {
                    None
                };
                // A member's attributes may follow its width.
                if self.at_attributes() {
                    let after = self.parse_attributes()?;
                    attrs.merge(after);
                }
                attrs.merge(specs.attrs.clone());
                let range = self.span_to_here(dstart);
                fields.push(FieldDecl {
                    specifiers: specs.clone(),
                    name,
                    ty,
                    bit_width,
                    attrs,
                    range,
                });
                if self.eat_punct(Punct::Comma).is_none() {
                    break;
                }
            }
            self.expect_punct(Punct::Semi, " after member declaration")?;
            if self.pos == before {
                self.advance();
            }
        }
        self.expect_punct(Punct::RBrace, " to close a member list")?;
        Ok((fields, asserts))
    }

    fn parse_enum_specifier(&mut self) -> PResult<Type> {
        let start = self.cur_range();
        self.advance(); // `enum`
        // An attribute specifier sequence is allowed here and ignored.
        let _ = self.parse_attributes()?;
        let name = self.eat_ident();
        // C23's fixed underlying type: `enum E : unsigned char { … }`.
        //
        // The `:` only introduces one when a type follows it. Among the members
        // of a record, `enum E : 3;` is an unnamed bit-field of the
        // enumeration's type — which C has had for far longer — and the width
        // is an expression, never a type.
        let underlying = if self.at_punct(Punct::Colon) && self.starts_decl_specifier(1) {
            let colon = self.bump_range();
            self.require_standard(Standard::C23, "an enum with a fixed underlying type", colon);
            let specs = self.parse_decl_specifiers(false)?;
            Some(specs.base)
        } else {
            None
        };
        let enumerators = if self.at_punct(Punct::LBrace) {
            self.advance();
            let mut list = Vec::new();
            while !self.at_punct(Punct::RBrace) && !self.at_eof() {
                let ename = self.expect_ident(" in enumerator list")?;
                self.declare(&ename.name.clone(), SymKind::Ordinary);
                // An attribute specifier sequence is allowed here and ignored.
                let _ = self.parse_attributes()?;
                let value = if self.eat_punct(Punct::Assign).is_some() {
                    Some(self.parse_conditional_expr()?)
                } else {
                    None
                };
                let range = self.span_to_here(ename.range);
                list.push(Enumerator {
                    name: ename,
                    value,
                    range,
                });
                if self.eat_punct(Punct::Comma).is_none() {
                    break;
                }
            }
            self.expect_punct(Punct::RBrace, " to close an enumerator list")?;
            Some(list)
        } else {
            if name.is_none() {
                let range = self.cur_range();
                let found = self.describe_cur();
                return Err(self.error_bail(
                    range,
                    format!("expected identifier or '{{' after 'enum', found {found}"),
                ));
            }
            None
        };
        let range = self.span_to_here(start);
        let id = self.add_enum(EnumSpec {
            name,
            enumerators,
            underlying,
            range,
        });
        Ok(Type::plain(TypeKind::Enum(id), range))
    }
}

// ---------------------------------------------------------------------------
// declarators
// ---------------------------------------------------------------------------

/// The outcome of parsing one declarator.
#[derive(Clone, Debug)]
pub struct DeclaratorResult {
    /// The declared name, absent for an abstract declarator.
    pub name: Option<Ident>,
    /// The type the declarator builds from the base type.
    pub ty: Type,
    /// What `__attribute__((…))` on the declarator asked for.
    pub attrs: Attributes,
    /// The symbol `__asm__("name")` renamed it to.
    pub asm_label: Option<Spanned<String>>,
    /// Where the declarator was written.
    pub range: SourceRange,
}

impl Parser<'_> {
    /// Parses a declarator, applying it to `base`.
    ///
    /// With `allow_abstract` the identifier may be omitted, which is what
    /// parameter declarations and type names need.
    fn parse_declarator(&mut self, base: Type, allow_abstract: bool) -> PResult<DeclaratorResult> {
        self.enter()?;
        let result = self.parse_declarator_inner(base, allow_abstract);
        self.leave();
        result
    }

    fn parse_declarator_inner(
        &mut self,
        base: Type,
        allow_abstract: bool,
    ) -> PResult<DeclaratorResult> {
        let start = self.cur_range();
        // GNU allows an attribute at the head of a declarator, which is where
        // a calling convention is usually written.
        let leading = self.parse_attributes()?;
        let mut ty = base;

        // `* qual* ` repeated: the leftmost `*` becomes the innermost pointer,
        // so `int * const * p` is "pointer to const pointer to int".
        while self.at_punct(Punct::Star) {
            let star = self.bump_range();
            let mut quals = self.parse_type_qualifiers();
            // GNU allows `int * __attribute__((x)) p;` and mixes the two.
            while self.at_attributes() {
                let _ = self.parse_attributes()?;
                quals = quals.merge(self.parse_type_qualifiers());
            }
            let range = self.span_to_here(star);
            ty = Type::new(TypeKind::Pointer(Box::new(ty)), quals, range);
        }

        if self.at_punct(Punct::LParen) && self.is_grouping_paren() {
            let save = self.pos;
            let balanced = self.skip_balanced_parens();
            if !balanced {
                let range = self.cur_range();
                return Err(self.error_bail(range, "unbalanced '(' in declarator"));
            }
            let rparen = self.pos - 1;
            ty = self.parse_type_suffix(ty)?;
            let after = self.pos;
            self.pos = save + 1;
            let inner = self.parse_declarator(ty, allow_abstract)?;
            if self.pos != rparen {
                let range = self.cur_range();
                let found = self.describe_cur();
                return Err(self.error_bail(
                    range,
                    format!("expected ')' after declarator, found {found}"),
                ));
            }
            self.pos = after;
            self.last_range = self.tokens[after - 1].range;
            let mut attrs = inner.attrs;
            attrs.merge(leading);
            return Ok(DeclaratorResult {
                name: inner.name,
                ty: inner.ty,
                attrs,
                asm_label: inner.asm_label,
                range: self.span_to_here(start),
            });
        }

        let name = match self.eat_ident() {
            Some(id) => Some(id),
            None if allow_abstract => None,
            None => {
                let range = self.cur_range();
                let found = self.describe_cur();
                return Err(self.error_bail(
                    range,
                    format!("expected identifier in declarator, found {found}"),
                ));
            }
        };
        // C23 allows an attribute specifier sequence after the declared name
        // (`int x [[deprecated]];`), and so does GNU.
        let mut attrs = self.parse_attributes()?;
        attrs.merge(leading);
        let ty = self.parse_type_suffix(ty)?;
        Ok(DeclaratorResult {
            name,
            ty,
            attrs,
            asm_label: None,
            range: self.span_to_here(start),
        })
    }

    /// At a `(` that begins a direct-declarator: does it group a nested
    /// declarator, or is it a parameter list?
    ///
    /// An attribute may stand at the head of either — `int (__attribute__((x))
    /// *)(void)` groups a declarator and `int (__attribute__((x)) int)` is a
    /// parameter list — so the question is asked of what follows it.
    fn is_grouping_paren(&self) -> bool {
        let after = self.after_attributes(1);
        !self.nth(after).is_punct(Punct::RParen) && !self.starts_decl_specifier(after)
    }

    /// The offset of the first token after any attribute specifiers at `n`.
    ///
    /// Used for lookahead only, so it never reports: an unbalanced clause
    /// stops at the end of the input and the caller's own parse reports it.
    fn after_attributes(&self, mut n: usize) -> usize {
        loop {
            let brackets =
                self.nth(n).is_punct(Punct::LBracket) && self.nth(n + 1).is_punct(Punct::LBracket);
            if !self.nth(n).is_keyword(Keyword::Attribute) && !brackets {
                return n;
            }
            let (open, close) = if brackets {
                (Punct::LBracket, Punct::RBracket)
            } else {
                (Punct::LParen, Punct::RParen)
            };
            let mut i = if brackets { n } else { n + 1 };
            let mut depth = 0i32;
            while !self.nth(i).is_eof() {
                if self.nth(i).is_punct(open) {
                    depth += 1;
                } else if self.nth(i).is_punct(close) {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                i += 1;
            }
            if i <= n {
                return n;
            }
            n = i;
        }
    }

    /// From the current `(`, skips to just past its matching `)`.
    fn skip_balanced_parens(&mut self) -> bool {
        let mut depth = 0i32;
        while !self.at_eof() {
            if self.at_punct(Punct::LParen) {
                depth += 1;
            } else if self.at_punct(Punct::RParen) {
                depth -= 1;
                if depth == 0 {
                    self.advance();
                    return true;
                }
            }
            self.advance();
        }
        false
    }

    /// Parses the `[...]` and `(...)` suffixes of a direct-declarator.
    ///
    /// The remaining suffixes are resolved *before* wrapping, so `int a[3][4]`
    /// becomes "array of 3 array of 4 int" rather than the other way round.
    fn parse_type_suffix(&mut self, ty: Type) -> PResult<Type> {
        if let Some(lb) = self.eat_punct(Punct::LBracket) {
            let mut is_static = false;
            let mut quals = TypeQualifiers::NONE;
            loop {
                if self.at_keyword(Keyword::Static) {
                    self.advance();
                    is_static = true;
                    continue;
                }
                match self.eat_type_qualifier() {
                    Some(q) => quals = quals.merge(q),
                    None => break,
                }
            }
            let size = if self.at_punct(Punct::RBracket) {
                ArraySize::Unspecified
            } else if self.at_punct(Punct::Star) && self.nth(1).is_punct(Punct::RBracket) {
                self.advance();
                ArraySize::Star
            } else {
                ArraySize::Expr(Box::new(self.parse_assignment_expr()?))
            };
            let rb = self.expect_punct(Punct::RBracket, " after array bound")?;
            let elem = self.parse_type_suffix(ty)?;
            return Ok(Type::plain(
                TypeKind::Array {
                    elem: Box::new(elem),
                    size,
                    qualifiers: quals,
                    is_static,
                },
                lb.join(rb),
            ));
        }

        if let Some(lp) = self.eat_punct(Punct::LParen) {
            let list = self.parse_param_list()?;
            let rp = self.expect_punct(Punct::RParen, " after parameter list")?;
            let ret = self.parse_type_suffix(ty)?;
            return Ok(Type::plain(
                TypeKind::Function(Box::new(FunctionType {
                    ret,
                    params: list.params,
                    variadic: list.ellipsis.is_some(),
                    ellipsis: list.ellipsis,
                    has_prototype: list.has_prototype,
                    kr_names: list.kr_names,
                })),
                lp.join(rp),
            ));
        }

        Ok(ty)
    }

    fn parse_param_list(&mut self) -> PResult<ParamList> {
        if self.at_punct(Punct::RParen) {
            return Ok(ParamList::default());
        }
        // `(void)` — an explicitly empty prototype.
        if self.at_keyword(Keyword::Void) && self.nth(1).is_punct(Punct::RParen) {
            self.advance();
            return Ok(ParamList {
                has_prototype: true,
                ..ParamList::default()
            });
        }
        // An old-style identifier list: `int f(a, b)`.
        let kr = match self.peek().ident() {
            Some(name) => !self.is_typedef_name(name),
            None => false,
        };
        if kr {
            let mut names = Vec::new();
            loop {
                names.push(self.expect_ident(" in parameter list")?);
                if self.eat_punct(Punct::Comma).is_none() {
                    break;
                }
            }
            return Ok(ParamList {
                kr_names: names,
                ..ParamList::default()
            });
        }

        // Parameter names are visible to later parameters' declarators, so
        // they get their own scope.
        self.push_scope();
        let result = self.parse_prototype_params();
        self.pop_scope();
        result
    }

    fn parse_prototype_params(&mut self) -> PResult<ParamList> {
        let mut params = Vec::new();
        let mut ellipsis = None;
        loop {
            if self.at_punct(Punct::Ellipsis) {
                ellipsis = Some(self.bump_range());
                break;
            }
            let start = self.cur_range();
            let specs = self.parse_decl_specifiers(true)?;
            let declarator = self.parse_declarator(specs.base.clone(), true)?;
            if let Some(name) = &declarator.name {
                self.declare(&name.name.clone(), SymKind::Ordinary);
            }
            let range = self.span_to_here(start);
            params.push(ParamDecl {
                specifiers: specs,
                name: declarator.name,
                ty: declarator.ty,
                range,
            });
            if self.eat_punct(Punct::Comma).is_none() {
                break;
            }
        }
        Ok(ParamList {
            params,
            ellipsis,
            has_prototype: true,
            kr_names: Vec::new(),
        })
    }

    fn parse_type_name(&mut self) -> PResult<TypeName> {
        let start = self.cur_range();
        let specs = self.parse_decl_specifiers(false)?;
        let declarator = self.parse_declarator(specs.base.clone(), true)?;
        if let Some(name) = &declarator.name {
            let range = name.range;
            self.error(range, "a type name must not declare an identifier");
        }
        Ok(TypeName {
            specifiers: specs,
            ty: declarator.ty,
            range: self.span_to_here(start),
        })
    }
}

/// The pieces of a parsed parameter list.
#[derive(Default)]
struct ParamList {
    params: Vec<ParamDecl>,
    /// Where `...` was written, if it was.
    ellipsis: Option<SourceRange>,
    has_prototype: bool,
    kr_names: Vec<Ident>,
}

// ---------------------------------------------------------------------------
// initialisers
// ---------------------------------------------------------------------------

impl Parser<'_> {
    fn parse_initializer(&mut self) -> PResult<Initializer> {
        self.enter()?;
        let result = self.parse_initializer_inner();
        self.leave();
        result
    }

    fn parse_initializer_inner(&mut self) -> PResult<Initializer> {
        if self.at_punct(Punct::LBrace) {
            let start = self.cur_range();
            let items = self.parse_initializer_list()?;
            return Ok(Initializer {
                kind: InitializerKind::List(items),
                range: self.span_to_here(start),
            });
        }
        let expr = self.parse_assignment_expr()?;
        Ok(Initializer {
            range: expr.range,
            kind: InitializerKind::Expr(expr),
        })
    }

    fn parse_initializer_list(&mut self) -> PResult<Vec<InitItem>> {
        let brace = self.expect_punct(Punct::LBrace, " to open an initializer list")?;
        if self.at_punct(Punct::RBrace) {
            // `= {}` zero-initialises anything; before C23 an initializer list
            // had to hold at least one initializer.
            let range = brace.join(self.cur_range());
            self.require_standard(Standard::C23, "an empty initializer", range);
        }
        let mut items = Vec::new();
        while !self.at_punct(Punct::RBrace) && !self.at_eof() {
            let start = self.cur_range();
            let mut designators = Vec::new();
            // The obsolete `name:` designator GNU still accepts, which is what
            // pre-C99 code writes for `.name =`.
            let mut old_style = false;
            if self.peek().ident().is_some() && self.nth(1).is_punct(Punct::Colon) {
                let field = self.eat_ident().expect("checked above");
                self.advance();
                designators.push(Designator::Field(field));
                old_style = true;
            }
            loop {
                if old_style {
                    break;
                }
                if self.eat_punct(Punct::Dot).is_some() {
                    let field = self.expect_ident(" after '.' in designator")?;
                    designators.push(Designator::Field(field));
                } else if self.eat_punct(Punct::LBracket).is_some() {
                    let index = self.parse_conditional_expr()?;
                    // GNU's range designator, `[low ... high] = v`.
                    if self.eat_punct(Punct::Ellipsis).is_some() {
                        let high = self.parse_conditional_expr()?;
                        self.expect_punct(Punct::RBracket, " after array designator")?;
                        designators.push(Designator::Range(index, high));
                    } else {
                        self.expect_punct(Punct::RBracket, " after array designator")?;
                        designators.push(Designator::Index(index));
                    }
                } else {
                    break;
                }
            }
            if !designators.is_empty() && !old_style {
                self.expect_punct(Punct::Assign, " after designator")?;
            }
            let init = self.parse_initializer()?;
            items.push(InitItem {
                designators,
                init,
                range: self.span_to_here(start),
            });
            if self.eat_punct(Punct::Comma).is_none() {
                break;
            }
        }
        self.expect_punct(Punct::RBrace, " to close an initializer list")?;
        Ok(items)
    }
}

// ---------------------------------------------------------------------------
// statements
// ---------------------------------------------------------------------------

impl Parser<'_> {
    fn parse_compound_stmt(&mut self) -> PResult<Block> {
        let start = self.expect_punct(Punct::LBrace, " to open a block")?;
        self.push_scope();
        // GNU's `__label__ a, b;` declares labels local to the block. Every
        // label already has function scope here and no two may share a name,
        // so the declaration is accepted and changes nothing.
        let mut local_labels = Vec::new();
        while self.at_keyword(Keyword::Label) {
            self.advance();
            loop {
                match self.expect_ident(" in a '__label__' declaration") {
                    Ok(name) => local_labels.push(name),
                    Err(bail) => {
                        self.pop_scope();
                        return Err(bail);
                    }
                }
                if self.eat_punct(Punct::Comma).is_none() {
                    break;
                }
            }
            if let Err(bail) = self.expect_punct(Punct::Semi, " after '__label__'") {
                self.pop_scope();
                return Err(bail);
            }
        }
        let mut items = Vec::new();
        while !self.at_punct(Punct::RBrace) && !self.at_eof() {
            let before = self.pos;
            let item = if self.at_static_assert() {
                self.parse_static_assert().map(BlockItem::StaticAssert)
            } else {
                // An attribute sequence here belongs either to a declaration
                // or to a statement, and only what follows says which; taking
                // it first is what lets `[[fallthrough]];` be a statement.
                match self.parse_attributes() {
                    Ok(attrs) => {
                        if self.starts_declaration() {
                            self.parse_declaration().map(|mut decl| {
                                decl.specifiers.noreturn =
                                    decl.specifiers.noreturn.or(attrs.noreturn);
                                BlockItem::Decl(decl)
                            })
                        } else {
                            self.parse_stmt().map(BlockItem::Stmt)
                        }
                    }
                    Err(bail) => Err(bail),
                }
            };
            match item {
                Ok(item) => items.push(item),
                Err(bail) => {
                    self.pop_scope();
                    return Err(bail);
                }
            }
            if self.pos == before {
                self.advance();
            }
        }
        self.pop_scope();
        let end = self.expect_punct(Punct::RBrace, " to close a block")?;
        Ok(Block {
            items,
            local_labels,
            range: start.join(end),
        })
    }

    fn parse_stmt(&mut self) -> PResult<Stmt> {
        self.enter()?;
        let result = self.parse_stmt_inner();
        self.leave();
        result
    }

    /// Parses a statement, taking the labels in front of it iteratively.
    ///
    /// `case 0: case 1: … case 1022: break;` is one statement under 1023
    /// labels, and C23 5.2.5.2p1 asks for exactly that many. Recursing per
    /// label would spend a stack frame — and a level of the recursion guard —
    /// on each of them, so the run is collected into a list and folded into
    /// the tree afterwards. What the tree looks like does not change.
    fn parse_stmt_inner(&mut self) -> PResult<Stmt> {
        let mut labels: Vec<(PendingLabel, SourceRange)> = Vec::new();
        let start = loop {
            let start = self.cur_range();
            // A statement may carry attributes of its own: `[[fallthrough]];`,
            // `__attribute__((fallthrough));`, `[[likely]] if (…)`. They are
            // consumed and dropped — a `switch` group falls through either way.
            let _ = self.parse_attributes()?;
            // `__extension__ stmt` asks for the pedantic warnings to be held
            // back; there are none.
            while self.eat_keyword(Keyword::Extension).is_some() {}

            // `label:`
            let label = if self.peek().ident().is_some() && self.nth(1).is_punct(Punct::Colon) {
                let label = self.eat_ident().expect("checked above");
                let colon = self.bump_range();
                PendingLabel::Ident { label, colon }
            } else if self.at_keyword(Keyword::Case) {
                self.advance();
                let value = self.parse_conditional_expr()?;
                // GNU's `case low ... high:`, which is one label for every
                // value in the range.
                let upper = if self.eat_punct(Punct::Ellipsis).is_some() {
                    Some(self.parse_conditional_expr()?)
                } else {
                    None
                };
                self.expect_punct(Punct::Colon, " after 'case' label")?;
                PendingLabel::Case { value, upper }
            } else if self.at_keyword(Keyword::Default) {
                self.advance();
                self.expect_punct(Punct::Colon, " after 'default' label")?;
                PendingLabel::Default
            } else {
                break start;
            };
            labels.push((label, start));
            if labels.len() > MAX_LABEL_CHAIN {
                let range = self.cur_range();
                return Err(self.error_bail(
                    range,
                    format!("more than {MAX_LABEL_CHAIN} labels on one statement"),
                ));
            }
        };

        if !labels.is_empty() {
            return self.finish_labeled_stmt(labels);
        }
        self.parse_unlabeled_stmt(start)
    }

    /// A statement with the labels — and the attributes — already taken.
    ///
    /// `start` is where the statement began, attributes included, which is
    /// where its range starts.
    fn parse_unlabeled_stmt(&mut self, start: SourceRange) -> PResult<Stmt> {
        // Inline assembly, which has no honest translation. Recognising the
        // whole statement is what turns it into one clear diagnostic.
        if self.at_keyword(Keyword::Asm) {
            return self.parse_asm_stmt();
        }

        if self.at_punct(Punct::LBrace) {
            let block = self.parse_compound_stmt()?;
            return Ok(Stmt {
                range: block.range,
                kind: StmtKind::Compound(block),
            });
        }

        if let Some(k) = self.peek().keyword() {
            match k {
                Keyword::If => return self.parse_if_stmt(),
                Keyword::Switch => {
                    self.advance();
                    self.expect_punct(Punct::LParen, " after 'switch'")?;
                    let cond = self.parse_expr()?;
                    self.expect_punct(Punct::RParen, " after switch condition")?;
                    let body = self.parse_stmt()?;
                    return Ok(Stmt {
                        kind: StmtKind::Switch {
                            cond,
                            body: Box::new(body),
                        },
                        range: self.span_to_here(start),
                    });
                }
                Keyword::While => {
                    self.advance();
                    self.expect_punct(Punct::LParen, " after 'while'")?;
                    let cond = self.parse_expr()?;
                    self.expect_punct(Punct::RParen, " after loop condition")?;
                    let body = self.parse_stmt()?;
                    return Ok(Stmt {
                        kind: StmtKind::While {
                            cond,
                            body: Box::new(body),
                        },
                        range: self.span_to_here(start),
                    });
                }
                Keyword::Do => {
                    self.advance();
                    let body = self.parse_stmt()?;
                    self.expect_keyword(Keyword::While, " after 'do' body")?;
                    self.expect_punct(Punct::LParen, " after 'while'")?;
                    let cond = self.parse_expr()?;
                    self.expect_punct(Punct::RParen, " after loop condition")?;
                    self.expect_punct(Punct::Semi, " after 'do' statement")?;
                    return Ok(Stmt {
                        kind: StmtKind::DoWhile {
                            body: Box::new(body),
                            cond,
                        },
                        range: self.span_to_here(start),
                    });
                }
                Keyword::For => return self.parse_for_stmt(),
                Keyword::Goto => {
                    self.advance();
                    let label = self.expect_ident(" after 'goto'")?;
                    self.expect_punct(Punct::Semi, " after 'goto' statement")?;
                    return Ok(Stmt {
                        kind: StmtKind::Goto(label),
                        range: self.span_to_here(start),
                    });
                }
                Keyword::Continue => {
                    self.advance();
                    self.expect_punct(Punct::Semi, " after 'continue'")?;
                    return Ok(Stmt {
                        kind: StmtKind::Continue,
                        range: self.span_to_here(start),
                    });
                }
                Keyword::Break => {
                    self.advance();
                    self.expect_punct(Punct::Semi, " after 'break'")?;
                    return Ok(Stmt {
                        kind: StmtKind::Break,
                        range: self.span_to_here(start),
                    });
                }
                Keyword::Return => {
                    self.advance();
                    let value = if self.at_punct(Punct::Semi) {
                        None
                    } else {
                        Some(self.parse_expr()?)
                    };
                    self.expect_punct(Punct::Semi, " after 'return' statement")?;
                    return Ok(Stmt {
                        kind: StmtKind::Return(value),
                        range: self.span_to_here(start),
                    });
                }
                _ => {}
            }
        }

        if let Some(semi) = self.eat_punct(Punct::Semi) {
            return Ok(Stmt {
                kind: StmtKind::Expr(None),
                range: semi,
            });
        }

        let expr = self.parse_expr()?;
        self.expect_punct(Punct::Semi, " after expression")?;
        Ok(Stmt {
            kind: StmtKind::Expr(Some(expr)),
            range: self.span_to_here(start),
        })
    }

    /// Parses what a run of labels labels, and folds the run into the tree.
    ///
    /// The list is never empty; see [`Parser::parse_stmt_inner`].
    fn finish_labeled_stmt(&mut self, labels: Vec<(PendingLabel, SourceRange)>) -> PResult<Stmt> {
        // C23 lets a label stand before a declaration and at the very end of a
        // compound statement; before that it had to label a statement. Either
        // way the label itself labels nothing, so it takes a null statement and
        // whatever follows is parsed on its own.
        let trailing = match labels.last().expect("a label chain is never empty") {
            (PendingLabel::Ident { label, colon }, _) => {
                let what = if self.at_punct(Punct::RBrace) {
                    Some("a label at the end of a compound statement")
                } else if self.starts_declaration() || self.at_static_assert() {
                    Some("a label before a declaration")
                } else {
                    None
                };
                what.map(|what| (what, label.range.join(*colon), *colon))
            }
            _ => None,
        };
        let mut stmt = match trailing {
            Some((what, at, colon)) => {
                self.require_standard(Standard::C23, what, at);
                Stmt {
                    kind: StmtKind::Expr(None),
                    range: colon,
                }
            }
            None => {
                let start = self.cur_range();
                self.parse_unlabeled_stmt(start)?
            }
        };
        let end = self.last_range;
        for (label, start) in labels.into_iter().rev() {
            let body = Box::new(stmt);
            let kind = match label {
                PendingLabel::Ident { label, .. } => StmtKind::Labeled { label, body },
                PendingLabel::Case { value, upper } => StmtKind::Case { value, upper, body },
                PendingLabel::Default => StmtKind::Default { body },
            };
            stmt = Stmt {
                kind,
                range: start.join(end),
            };
        }
        Ok(stmt)
    }

    /// `asm [qualifiers] ( … ) ;` — an inline assembly statement.
    ///
    /// Rust has `core::arch::asm!`, but its operand constraints are a language
    /// of their own and mapping GCC's onto them is a project rather than a
    /// feature; half a translation of assembly would be worse than none. The
    /// whole statement is consumed so that the diagnostic is about the `asm`
    /// rather than about the tokens inside it.
    fn parse_asm_stmt(&mut self) -> PResult<Stmt> {
        let start = self.cur_range();
        self.advance();
        // `volatile`, `inline` and `goto` may qualify it.
        while matches!(
            self.peek().keyword(),
            Some(Keyword::Volatile | Keyword::Const | Keyword::Inline | Keyword::Goto)
        ) {
            self.advance();
        }
        if self.at_punct(Punct::LParen) {
            self.skip_attribute_args()?;
        }
        self.eat_punct(Punct::Semi);
        let range = self.span_to_here(start);
        self.error(range, "inline assembly is not supported");
        Ok(Stmt {
            kind: StmtKind::Error,
            range,
        })
    }

    fn parse_if_stmt(&mut self) -> PResult<Stmt> {
        let start = self.cur_range();
        self.advance(); // `if`
        self.expect_punct(Punct::LParen, " after 'if'")?;
        let cond = self.parse_expr()?;
        self.expect_punct(Punct::RParen, " after if condition")?;
        let then_branch = Box::new(self.parse_stmt()?);
        let else_branch = if self.eat_keyword(Keyword::Else).is_some() {
            Some(Box::new(self.parse_stmt()?))
        } else {
            None
        };
        Ok(Stmt {
            kind: StmtKind::If {
                cond,
                then_branch,
                else_branch,
            },
            range: self.span_to_here(start),
        })
    }

    fn parse_for_stmt(&mut self) -> PResult<Stmt> {
        let start = self.cur_range();
        self.advance(); // `for`
        self.expect_punct(Punct::LParen, " after 'for'")?;
        // C99 allows a declaration here; it scopes to the loop.
        self.push_scope();
        let result = (|parser: &mut Self| {
            let init = if parser.at_punct(Punct::Semi) {
                parser.advance();
                ForInit::None
            } else if parser.starts_declaration() {
                ForInit::Decl(Box::new(parser.parse_declaration()?))
            } else {
                let expr = parser.parse_expr()?;
                parser.expect_punct(Punct::Semi, " after 'for' initializer")?;
                ForInit::Expr(expr)
            };
            let cond = if parser.at_punct(Punct::Semi) {
                None
            } else {
                Some(parser.parse_expr()?)
            };
            parser.expect_punct(Punct::Semi, " after 'for' condition")?;
            let step = if parser.at_punct(Punct::RParen) {
                None
            } else {
                Some(parser.parse_expr()?)
            };
            parser.expect_punct(Punct::RParen, " after 'for' clauses")?;
            let body = parser.parse_stmt()?;
            Ok(StmtKind::For {
                init,
                cond,
                step,
                body: Box::new(body),
            })
        })(self);
        self.pop_scope();
        Ok(Stmt {
            kind: result?,
            range: self.span_to_here(start),
        })
    }

    fn expect_keyword(&mut self, k: Keyword, ctx: &str) -> PResult<SourceRange> {
        if self.at_keyword(k) {
            return Ok(self.bump_range());
        }
        let range = self.cur_range();
        let found = self.describe_cur();
        Err(self.error_bail(
            range,
            format!("expected '{}'{ctx}, found {found}", k.as_str()),
        ))
    }
}

// ---------------------------------------------------------------------------
// expressions
// ---------------------------------------------------------------------------

/// Binding power of the binary operators, tightest last.
fn binary_op(kind: &TokenKind) -> Option<(BinaryOp, u8)> {
    let TokenKind::Punct(p) = kind else {
        return None;
    };
    Some(match p {
        Punct::PipePipe => (BinaryOp::LogOr, 1),
        Punct::AmpAmp => (BinaryOp::LogAnd, 2),
        Punct::Pipe => (BinaryOp::BitOr, 3),
        Punct::Caret => (BinaryOp::BitXor, 4),
        Punct::Amp => (BinaryOp::BitAnd, 5),
        Punct::EqEq => (BinaryOp::Eq, 6),
        Punct::Ne => (BinaryOp::Ne, 6),
        Punct::Lt => (BinaryOp::Lt, 7),
        Punct::Gt => (BinaryOp::Gt, 7),
        Punct::Le => (BinaryOp::Le, 7),
        Punct::Ge => (BinaryOp::Ge, 7),
        Punct::Shl => (BinaryOp::Shl, 8),
        Punct::Shr => (BinaryOp::Shr, 8),
        Punct::Plus => (BinaryOp::Add, 9),
        Punct::Minus => (BinaryOp::Sub, 9),
        Punct::Star => (BinaryOp::Mul, 10),
        Punct::Slash => (BinaryOp::Div, 10),
        Punct::Percent => (BinaryOp::Rem, 10),
        _ => return None,
    })
}

/// The compound operator of an assignment token, if it is one.
fn assign_op(kind: &TokenKind) -> Option<Option<BinaryOp>> {
    let TokenKind::Punct(p) = kind else {
        return None;
    };
    Some(match p {
        Punct::Assign => None,
        Punct::StarAssign => Some(BinaryOp::Mul),
        Punct::SlashAssign => Some(BinaryOp::Div),
        Punct::PercentAssign => Some(BinaryOp::Rem),
        Punct::PlusAssign => Some(BinaryOp::Add),
        Punct::MinusAssign => Some(BinaryOp::Sub),
        Punct::ShlAssign => Some(BinaryOp::Shl),
        Punct::ShrAssign => Some(BinaryOp::Shr),
        Punct::AmpAssign => Some(BinaryOp::BitAnd),
        Punct::CaretAssign => Some(BinaryOp::BitXor),
        Punct::PipeAssign => Some(BinaryOp::BitOr),
        _ => return None,
    })
}

impl Parser<'_> {
    /// `expression` — including the comma operator.
    pub(crate) fn parse_expr(&mut self) -> PResult<Expr> {
        self.enter()?;
        let result = self.parse_expr_inner();
        self.leave();
        result
    }

    /// `a, b, c, …` — the comma operator, which is left-associative.
    ///
    /// Taken in a loop, so the length of the chain costs the parser no stack;
    /// sema and code generation walk it iteratively too, so it costs them
    /// none either and nothing but memory bounds it. See
    /// [`MAX_RECURSION_DEPTH`].
    fn parse_expr_inner(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_assignment_expr()?;
        while self.eat_punct(Punct::Comma).is_some() {
            let rhs = self.parse_assignment_expr()?;
            let range = lhs.range.join(rhs.range);
            lhs = Expr {
                kind: ExprKind::Comma {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                range,
            };
        }
        Ok(lhs)
    }

    /// `assignment-expression`, which is right associative.
    ///
    /// `a = b = c` is taken in a loop and folded from the right afterwards, so
    /// the parser itself never recurses down the chain — but what it folds is
    /// `Assign(a, Assign(b, c))`, one level of *nesting* per operator, and
    /// nesting is what [`MAX_RECURSION_DEPTH`] is for. Each operator is
    /// therefore charged to the same counter `((((…))))` is charged to, and
    /// released again however the chain ends.
    fn parse_assignment_expr(&mut self) -> PResult<Expr> {
        let mut charged = 0u32;
        let result = self.assignment_chain(&mut charged);
        for _ in 0..charged {
            self.leave();
        }
        result
    }

    /// [`Parser::parse_assignment_expr`], reporting the nesting it charged.
    fn assignment_chain(&mut self, charged: &mut u32) -> PResult<Expr> {
        let mut pending: Vec<(Expr, Option<BinaryOp>)> = Vec::new();
        let mut value = loop {
            let lhs = self.parse_conditional_expr()?;
            let Some(op) = assign_op(&self.peek().kind) else {
                break lhs;
            };
            self.advance();
            self.enter()?;
            *charged += 1;
            pending.push((lhs, op));
        };
        for (lhs, op) in pending.into_iter().rev() {
            let range = lhs.range.join(value.range);
            value = Expr {
                kind: ExprKind::Assign {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(value),
                },
                range,
            };
        }
        Ok(value)
    }

    /// `conditional-expression`, whose `else` operand is another one.
    ///
    /// `a ? b : c ? d : e` is taken in a loop and folded from the right, for
    /// the reason [`Parser::parse_assignment_expr`] is — and, for the same
    /// reason, each operator is charged to [`MAX_RECURSION_DEPTH`]: the tree
    /// it builds nests one level per operator, and every pass after this one
    /// has to walk it.
    fn parse_conditional_expr(&mut self) -> PResult<Expr> {
        let mut charged = 0u32;
        let result = self.conditional_chain(&mut charged);
        for _ in 0..charged {
            self.leave();
        }
        result
    }

    /// [`Parser::parse_conditional_expr`], reporting the nesting it charged.
    fn conditional_chain(&mut self, charged: &mut u32) -> PResult<Expr> {
        #[allow(clippy::type_complexity)]
        let mut pending: Vec<(Expr, Option<Box<Expr>>)> = Vec::new();
        let mut value = loop {
            let cond = self.parse_binary_expr(1)?;
            if self.eat_punct(Punct::Question).is_none() {
                break cond;
            }
            // GNU's `a ?: b`: the middle operand is the condition itself, and
            // the condition is evaluated exactly once.
            let then_expr = if self.at_punct(Punct::Colon) {
                None
            } else {
                Some(Box::new(self.parse_expr()?))
            };
            self.expect_punct(Punct::Colon, " in conditional expression")?;
            self.enter()?;
            *charged += 1;
            pending.push((cond, then_expr));
        };
        for (cond, then_expr) in pending.into_iter().rev() {
            let range = cond.range.join(value.range);
            value = Expr {
                kind: ExprKind::Conditional {
                    cond: Box::new(cond),
                    then_expr,
                    else_expr: Box::new(value),
                },
                range,
            };
        }
        Ok(value)
    }

    /// The binary operators, by precedence climbing.
    ///
    /// Every level is left-associative, so a run of one operator is a loop
    /// rather than recursion and its length costs no stack — here or in the
    /// passes after it; the recursion is over the ten *precedence levels*.
    /// See [`MAX_RECURSION_DEPTH`].
    fn parse_binary_expr(&mut self, min_prec: u8) -> PResult<Expr> {
        let mut lhs = self.parse_cast_expr()?;
        while let Some((op, prec)) = binary_op(&self.peek().kind) {
            if prec < min_prec {
                break;
            }
            self.advance();
            let rhs = self.parse_binary_expr(prec + 1)?;
            let range = lhs.range.join(rhs.range);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                range,
            };
        }
        Ok(lhs)
    }

    /// Whether a `(` at the current position introduces a type name — that is,
    /// whether this is a cast or a compound literal rather than a
    /// parenthesised expression.
    fn at_paren_type_name(&self) -> bool {
        self.at_punct(Punct::LParen) && self.starts_decl_specifier(1)
    }

    fn parse_cast_expr(&mut self) -> PResult<Expr> {
        self.enter()?;
        let result = self.parse_cast_expr_inner();
        self.leave();
        result
    }

    fn parse_cast_expr_inner(&mut self) -> PResult<Expr> {
        if !self.at_paren_type_name() {
            return self.parse_unary_expr();
        }
        let start = self.cur_range();
        self.advance(); // `(`
        let ty = self.parse_type_name()?;
        self.expect_punct(Punct::RParen, " after type name")?;
        if self.at_punct(Punct::LBrace) {
            // `(T){ ... }` is a compound literal, i.e. a postfix expression.
            let items = self.parse_initializer_list()?;
            let expr = Expr {
                kind: ExprKind::CompoundLiteral {
                    ty: Box::new(ty),
                    init: items,
                },
                range: self.span_to_here(start),
            };
            return self.parse_postfix_suffixes(expr);
        }
        let expr = self.parse_cast_expr()?;
        let range = start.join(expr.range);
        Ok(Expr {
            kind: ExprKind::Cast {
                ty: Box::new(ty),
                expr: Box::new(expr),
            },
            range,
        })
    }

    fn parse_unary_expr(&mut self) -> PResult<Expr> {
        let start = self.cur_range();

        if let Some(p) = match &self.peek().kind {
            TokenKind::Punct(p) => Some(*p),
            _ => None,
        } {
            let unary = match p {
                Punct::Amp => Some(UnaryOp::AddrOf),
                Punct::Star => Some(UnaryOp::Deref),
                Punct::Plus => Some(UnaryOp::Plus),
                Punct::Minus => Some(UnaryOp::Minus),
                Punct::Tilde => Some(UnaryOp::BitNot),
                Punct::Bang => Some(UnaryOp::LogNot),
                _ => None,
            };
            if let Some(op) = unary {
                self.advance();
                let operand = self.parse_cast_expr()?;
                let range = start.join(operand.range);
                return Ok(Expr {
                    kind: ExprKind::Unary {
                        op,
                        operand: Box::new(operand),
                    },
                    range,
                });
            }
            if matches!(p, Punct::PlusPlus | Punct::MinusMinus) {
                self.advance();
                let op = if p == Punct::PlusPlus {
                    IncDec::Inc
                } else {
                    IncDec::Dec
                };
                let operand = self.parse_unary_expr()?;
                let range = start.join(operand.range);
                return Ok(Expr {
                    kind: ExprKind::PreIncDec {
                        op,
                        operand: Box::new(operand),
                    },
                    range,
                });
            }
        }

        if self.at_keyword(Keyword::Sizeof) {
            self.advance();
            if self.at_paren_type_name() {
                self.advance(); // `(`
                let ty = self.parse_type_name()?;
                self.expect_punct(Punct::RParen, " after type name")?;
                if self.at_punct(Punct::LBrace) {
                    // `sizeof (T){ ... }` measures a compound literal.
                    let items = self.parse_initializer_list()?;
                    let literal = Expr {
                        kind: ExprKind::CompoundLiteral {
                            ty: Box::new(ty),
                            init: items,
                        },
                        range: self.span_to_here(start),
                    };
                    let operand = self.parse_postfix_suffixes(literal)?;
                    let range = start.join(operand.range);
                    return Ok(Expr {
                        kind: ExprKind::SizeofExpr(Box::new(operand)),
                        range,
                    });
                }
                return Ok(Expr {
                    kind: ExprKind::SizeofType(Box::new(ty)),
                    range: self.span_to_here(start),
                });
            }
            let operand = self.parse_unary_expr()?;
            let range = start.join(operand.range);
            return Ok(Expr {
                kind: ExprKind::SizeofExpr(Box::new(operand)),
                range,
            });
        }

        // `__extension__ expr` holds back the pedantic warnings there are none
        // of, and `__real__`/`__imag__` need complex arithmetic.
        if self.eat_keyword(Keyword::Extension).is_some() {
            return self.parse_unary_expr();
        }
        if let Some(k @ (Keyword::RealGnu | Keyword::ImagGnu)) = self.peek().keyword() {
            self.advance();
            let operand = self.parse_cast_expr()?;
            let range = start.join(operand.range);
            self.error(
                range,
                format!(
                    "'{}' is not supported; complex types are not supported",
                    k.as_str()
                ),
            );
            return Ok(Expr {
                kind: ExprKind::ComplexPart {
                    real: k == Keyword::RealGnu,
                    operand: Box::new(operand),
                },
                range,
            });
        }

        if let Some(k @ (Keyword::Alignof | Keyword::AlignofName | Keyword::AlignofGnu)) =
            self.peek().keyword()
        {
            self.require_keyword(k, start);
            self.advance();
            if self.at_paren_type_name() {
                self.advance(); // `(`
                let ty = self.parse_type_name()?;
                self.expect_punct(Punct::RParen, " after type name")?;
                return Ok(Expr {
                    kind: ExprKind::AlignofType(Box::new(ty)),
                    range: self.span_to_here(start),
                });
            }
            // `_Alignof expr` is GCC's extension, which C never standardised;
            // accepting it costs nothing and `_Alignof(x)` is what people
            // write when `x` is an object.
            let operand = self.parse_unary_expr()?;
            let range = start.join(operand.range);
            return Ok(Expr {
                kind: ExprKind::AlignofExpr(Box::new(operand)),
                range,
            });
        }

        if self.at_va_arg() {
            return self.parse_va_arg();
        }
        if self.at_builtin("__builtin_offsetof") {
            return self.parse_offsetof();
        }
        if self.at_builtin("__builtin_types_compatible_p") {
            return self.parse_types_compatible();
        }
        if self.at_builtin("__builtin_choose_expr") {
            return self.parse_choose_expr();
        }

        self.parse_postfix_expr()
    }

    /// `__builtin_types_compatible_p(T1, T2)`, whose operands are type names.
    fn parse_types_compatible(&mut self) -> PResult<Expr> {
        let start = self.cur_range();
        self.advance(); // the name
        self.advance(); // `(`
        let lhs = self.parse_type_name()?;
        self.expect_punct(
            Punct::Comma,
            " after the first type of '__builtin_types_compatible_p'",
        )?;
        let rhs = self.parse_type_name()?;
        self.expect_punct(
            Punct::RParen,
            " after the second type of '__builtin_types_compatible_p'",
        )?;
        let expr = Expr {
            kind: ExprKind::TypesCompatible {
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            range: self.span_to_here(start),
        };
        self.parse_postfix_suffixes(expr)
    }

    /// `__builtin_choose_expr(c, a, b)`, whose unchosen operand is never even
    /// type checked — which is why it needs the parser's help.
    fn parse_choose_expr(&mut self) -> PResult<Expr> {
        let start = self.cur_range();
        self.advance(); // the name
        self.advance(); // `(`
        let cond = self.parse_assignment_expr()?;
        self.expect_punct(
            Punct::Comma,
            " after the condition of '__builtin_choose_expr'",
        )?;
        let then_expr = self.parse_assignment_expr()?;
        self.expect_punct(Punct::Comma, " in '__builtin_choose_expr'")?;
        let else_expr = self.parse_assignment_expr()?;
        self.expect_punct(Punct::RParen, " to close '__builtin_choose_expr'")?;
        let expr = Expr {
            kind: ExprKind::ChooseExpr {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            },
            range: self.span_to_here(start),
        };
        self.parse_postfix_suffixes(expr)
    }

    /// Whether the next tokens invoke the named builtin.
    ///
    /// The `__builtin_` names are reserved, so no declaration can turn one
    /// back into an ordinary identifier; without the check the type name each
    /// of them takes would not parse as an expression.
    fn at_builtin(&self, name: &str) -> bool {
        self.peek().ident() == Some(name) && self.nth(1).is_punct(Punct::LParen)
    }

    /// Whether the next tokens are a `va_arg(…)` invocation.
    fn at_va_arg(&self) -> bool {
        self.at_builtin("__builtin_va_arg")
    }

    /// `__builtin_offsetof(T, member)`, the special form `offsetof` is.
    ///
    /// A nested member designator — `offsetof(struct S, a.b)`, or one with a
    /// subscript — is refused rather than mistranslated: Rust's
    /// `offset_of!` does accept a path, but a `[…]` in one has no meaning
    /// there, and half of the syntax is worse than none.
    fn parse_offsetof(&mut self) -> PResult<Expr> {
        let start = self.cur_range();
        self.advance(); // `__builtin_offsetof`
        self.advance(); // `(`
        let ty = self.parse_type_name()?;
        self.expect_punct(Punct::Comma, " after the type of 'offsetof'")?;
        let member = self.expect_ident(" as the member of 'offsetof'")?;
        if self.at_punct(Punct::Dot) || self.at_punct(Punct::LBracket) {
            let range = self.cur_range();
            return Err(self.error_bail(
                range,
                "a nested member designator is not supported in 'offsetof'",
            ));
        }
        self.expect_punct(Punct::RParen, " after the member of 'offsetof'")?;
        let expr = Expr {
            kind: ExprKind::OffsetOf {
                ty: Box::new(ty),
                member,
            },
            range: self.span_to_here(start),
        };
        self.parse_postfix_suffixes(expr)
    }

    /// `va_arg(ap, T)`, whose second argument is a type name.
    fn parse_va_arg(&mut self) -> PResult<Expr> {
        let start = self.cur_range();
        self.advance(); // `va_arg`
        self.advance(); // `(`
        let ap = self.parse_assignment_expr()?;
        self.expect_punct(Punct::Comma, " after the argument list of 'va_arg'")?;
        let ty = self.parse_type_name()?;
        self.expect_punct(Punct::RParen, " after the type of 'va_arg'")?;
        let expr = Expr {
            kind: ExprKind::VaArg {
                ap: Box::new(ap),
                ty: Box::new(ty),
            },
            range: self.span_to_here(start),
        };
        self.parse_postfix_suffixes(expr)
    }

    fn parse_postfix_expr(&mut self) -> PResult<Expr> {
        let primary = self.parse_primary_expr()?;
        self.parse_postfix_suffixes(primary)
    }

    /// The `[…]`, `(…)`, `.x`, `->x`, `++` and `--` that follow an operand.
    ///
    /// A run of them is left-associative in the source and *nested* in the
    /// tree — `p->a->b` is a member of a member — and code generation walks
    /// that nesting recursively, so each suffix taken is charged to
    /// [`MAX_RECURSION_DEPTH`]. It is the tightest of the three constructs
    /// charged there: a `->` chain is what overflows first, at about 450.
    fn parse_postfix_suffixes(&mut self, expr: Expr) -> PResult<Expr> {
        let mut charged = 0u32;
        let result = self.postfix_suffixes(expr, &mut charged);
        for _ in 0..charged {
            self.leave();
        }
        result
    }

    /// [`Parser::parse_postfix_suffixes`], reporting the nesting it charged.
    fn postfix_suffixes(&mut self, mut expr: Expr, charged: &mut u32) -> PResult<Expr> {
        let mut suffixes = 0usize;
        loop {
            // Nothing is charged for an operand with no suffix at all, so
            // that the 63 levels of parenthesised expression C23 5.2.5.2p1
            // asks for keep the whole budget to themselves.
            if suffixes > 0 {
                self.enter()?;
                *charged += 1;
            }
            suffixes += 1;
            if self.eat_punct(Punct::LBracket).is_some() {
                let index = self.parse_expr()?;
                let rb = self.expect_punct(Punct::RBracket, " after subscript")?;
                expr = Expr {
                    range: expr.range.join(rb),
                    kind: ExprKind::Index {
                        base: Box::new(expr),
                        index: Box::new(index),
                    },
                };
                continue;
            }
            if self.eat_punct(Punct::LParen).is_some() {
                let mut args = Vec::new();
                if !self.at_punct(Punct::RParen) {
                    loop {
                        args.push(self.parse_assignment_expr()?);
                        if self.eat_punct(Punct::Comma).is_none() {
                            break;
                        }
                    }
                }
                let rp = self.expect_punct(Punct::RParen, " after argument list")?;
                expr = Expr {
                    range: expr.range.join(rp),
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                };
                continue;
            }
            let arrow = if self.at_punct(Punct::Dot) {
                false
            } else if self.at_punct(Punct::Arrow) {
                true
            } else if self.at_punct(Punct::PlusPlus) || self.at_punct(Punct::MinusMinus) {
                let op = if self.at_punct(Punct::PlusPlus) {
                    IncDec::Inc
                } else {
                    IncDec::Dec
                };
                let range = expr.range.join(self.bump_range());
                expr = Expr {
                    kind: ExprKind::PostIncDec {
                        op,
                        operand: Box::new(expr),
                    },
                    range,
                };
                continue;
            } else {
                break;
            };
            self.advance();
            let field = self.expect_ident(if arrow { " after '->'" } else { " after '.'" })?;
            expr = Expr {
                range: expr.range.join(field.range),
                kind: ExprKind::Member {
                    base: Box::new(expr),
                    arrow,
                    field,
                },
            };
        }
        Ok(expr)
    }

    /// `_Generic ( controlling-expression , type : value , … )` — C11 6.5.1.1.
    ///
    /// Every association is parsed; only the chosen one is checked, which is
    /// what makes `_Generic` usable for a type the other arms would refuse.
    fn parse_generic_selection(&mut self) -> PResult<Expr> {
        let start = self.cur_range();
        self.require_keyword(Keyword::Generic, start);
        self.advance();
        self.expect_punct(Punct::LParen, " after '_Generic'")?;
        let controlling = self.parse_assignment_expr()?;
        let mut assocs = Vec::new();
        while self.eat_punct(Punct::Comma).is_some() {
            let astart = self.cur_range();
            let ty = if self.eat_keyword(Keyword::Default).is_some() {
                None
            } else {
                Some(self.parse_type_name()?)
            };
            self.expect_punct(Punct::Colon, " after the type of a '_Generic' association")?;
            let value = self.parse_assignment_expr()?;
            assocs.push(GenericAssoc {
                ty,
                value,
                range: self.span_to_here(astart),
            });
        }
        let rparen = self.expect_punct(Punct::RParen, " to close '_Generic'")?;
        if assocs.is_empty() {
            self.error(
                start.join(rparen),
                "'_Generic' requires at least one association",
            );
        }
        Ok(Expr {
            kind: ExprKind::Generic {
                controlling: Box::new(controlling),
                assocs,
            },
            range: start.join(rparen),
        })
    }

    fn parse_primary_expr(&mut self) -> PResult<Expr> {
        let range = self.cur_range();
        match self.peek().kind.clone() {
            TokenKind::Keyword(Keyword::Generic) => self.parse_generic_selection(),
            TokenKind::Keyword(k @ (Keyword::True | Keyword::False)) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Bool(k == Keyword::True),
                    range,
                })
            }
            TokenKind::Keyword(Keyword::Nullptr) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Nullptr,
                    range,
                })
            }
            TokenKind::Ident(name) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Ident(Ident { name, range }),
                    range,
                })
            }
            TokenKind::Int(lit) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Int(lit),
                    range,
                })
            }
            TokenKind::Float(lit) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Float(lit),
                    range,
                })
            }
            TokenKind::Char(lit) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Char(lit),
                    range,
                })
            }
            TokenKind::Str(first) => Ok(self.parse_string_literal(first, range)),
            TokenKind::Punct(Punct::LParen) => {
                self.advance();
                // GNU's statement expression, `({ … })`: a compound statement
                // where an expression goes, whose value is the value of the
                // last expression statement in it.
                if self.at_punct(Punct::LBrace) {
                    let block = self.parse_compound_stmt()?;
                    let rp =
                        self.expect_punct(Punct::RParen, " to close a statement expression")?;
                    return Ok(Expr {
                        kind: ExprKind::StmtExpr(Box::new(block)),
                        range: range.join(rp),
                    });
                }
                let inner = self.parse_expr()?;
                let rp = self.expect_punct(Punct::RParen, " after parenthesized expression")?;
                Ok(Expr {
                    kind: inner.kind,
                    range: range.join(rp),
                })
            }
            _ => {
                let found = self.describe_cur();
                Err(self.error_bail(range, format!("expected expression, found {found}")))
            }
        }
    }

    /// Concatenates adjacent string literals, as translation phase 6 does.
    fn parse_string_literal(&mut self, first: StrLit, first_range: SourceRange) -> Expr {
        self.advance();
        let mut kind = first.kind;
        let mut values = first.values;
        let mut text = first.text;
        let mut range = first_range;
        while let TokenKind::Str(next) = self.peek().kind.clone() {
            if next.kind == StrKind::Wide {
                kind = StrKind::Wide;
            }
            values.extend_from_slice(&next.values);
            text.push(' ');
            text.push_str(&next.text);
            range = range.join(self.cur_range());
            self.advance();
        }
        Expr {
            kind: ExprKind::Str(StrLit { kind, values, text }),
            range,
        }
    }
}
