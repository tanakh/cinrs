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
use crate::ir::VA_LIST_NAMES;
use crate::lex::{Keyword, Punct, StrKind, StrLit, TokenKind};
use crate::pp::{Origin, Token};
use crate::{Options, Standard};

/// The spelling of `_Noreturn` that every standard accepts.
///
/// `_Noreturn` itself is a C11 keyword, and a `c99!` block that includes the
/// bundled `<stdlib.h>` must not be told that its `exit` declaration needs a
/// newer standard. The headers therefore write this name, which is a
/// declaration specifier in every mode and means exactly what `_Noreturn`
/// means.
pub const NORETURN_BUILTIN: &str = "__cinrs_noreturn";

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
        last_range,
        depth: 0,
    };
    parser.parse_translation_unit(unit_range)
}

/// How deeply the parser may recurse before giving up.
///
/// Recursive descent turns nesting in the input into stack frames, and a
/// procedural macro that overflows the stack takes the whole compiler down
/// with no useful message. Pathologically nested input becomes a diagnostic
/// instead.
const MAX_RECURSION_DEPTH: u32 = 200;

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    diags: &'a mut Diagnostics,
    scopes: Vec<Scope>,
    /// Which revision's grammar to accept; see [`Parser::require_standard`].
    standard: Standard,
    /// Range of the most recently consumed token, used to close node ranges.
    last_range: SourceRange,
    /// Current recursion depth; reset at every external declaration.
    depth: u32,
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
        if self.standard >= needed {
            return;
        }
        let message = self.standard.requires(what, needed);
        self.error(range, message);
    }

    /// Reports a keyword the block's own standard does not have.
    fn require_keyword(&mut self, k: Keyword, range: SourceRange) {
        let needed = k.since();
        self.require_standard(needed, &format!("'{}'", k.as_str()), range);
    }

    /// The gate message for the identifier at the current position, if a
    /// newer revision would have made it a keyword.
    fn newer_keyword_here(&self) -> Option<String> {
        let name = self.peek().ident()?;
        let needed = Keyword::from_str(name, Standard::C23)?.since();
        (needed > self.standard).then(|| self.standard.requires(&format!("'{name}'"), needed))
    }
}

/// What an attribute specifier sequence asked for.
///
/// C23 says an implementation may ignore any attribute it does not know, and
/// this one ignores all but `noreturn`: `[[maybe_unused]]`, `[[deprecated]]`,
/// `[[nodiscard]]` and `[[fallthrough]]` are accepted and dropped, and so is
/// anything else, standard or not.
#[derive(Clone, Copy, Default, Debug)]
struct Attributes {
    /// Where `[[noreturn]]` was written, if it was.
    noreturn: Option<SourceRange>,
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
    fn at_attributes(&self) -> bool {
        self.at_punct(Punct::LBracket) && self.nth(1).is_punct(Punct::LBracket)
    }

    /// Consumes `[[…]] [[…]] …`, keeping only what the front end acts on.
    ///
    /// The contents are skipped as balanced token soup rather than parsed:
    /// C23 lets an implementation ignore any attribute it does not recognise,
    /// and an attribute argument clause may hold anything at all.
    fn parse_attributes(&mut self) -> PResult<Attributes> {
        let mut attrs = Attributes::default();
        while self.at_attributes() {
            let start = self.cur_range();
            self.require_standard(Standard::C23, "an attribute specifier", start);
            self.advance(); // `[`
            self.advance(); // `[`
            let mut depth = 2i32;
            loop {
                if self.at_eof() {
                    return Err(self.error_bail(start, "unterminated attribute specifier"));
                }
                match &self.peek().kind {
                    TokenKind::Punct(Punct::LBracket | Punct::LParen | Punct::LBrace) => {
                        depth += 1;
                    }
                    TokenKind::Punct(Punct::RBracket | Punct::RParen | Punct::RBrace) => {
                        depth -= 1;
                    }
                    _ => {}
                }
                // `[[noreturn]]` is the one attribute with a meaning here; it
                // says exactly what `_Noreturn` says.
                let is_noreturn = depth == 2
                    && (self.peek().ident() == Some("noreturn")
                        || self.peek().is_keyword(Keyword::Noreturn));
                let range = self.bump_range();
                if is_noreturn && attrs.noreturn.is_none() {
                    attrs.noreturn = Some(range);
                }
                if depth == 0 {
                    break;
                }
            }
        }
        Ok(attrs)
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
        TranslationUnit { items, range }
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
        let attrs = self.parse_attributes()?;
        if self.at_static_assert() {
            return Ok(ExternalDecl::StaticAssert(self.parse_static_assert()?));
        }
        let mut specs = self.parse_decl_specifiers(true)?;
        specs.noreturn = specs.noreturn.or(attrs.noreturn);

        if let Some(semi) = self.eat_punct(Punct::Semi) {
            return Ok(ExternalDecl::Decl(Decl {
                specifiers: specs,
                declarators: Vec::new(),
                range: start.join(semi),
            }));
        }

        let first = self.parse_declarator(specs.base.clone(), false)?;

        let looks_like_definition = matches!(first.ty.kind, TypeKind::Function(_))
            && (self.at_punct(Punct::LBrace) || self.starts_declaration());
        if looks_like_definition && !specs.is_typedef() {
            return self.finish_function_def(specs, first, start);
        }

        let decl = self.finish_declaration(specs, Some(first), start)?;
        Ok(ExternalDecl::Decl(decl))
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
            let declarator = match pending.take() {
                Some(d) => d,
                None => self.parse_declarator(specs.base.clone(), false)?,
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
            let range = self.span_to_here(declarator.range);
            declarators.push(InitDeclarator {
                name: declarator.name,
                ty: declarator.ty,
                init,
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
        let attrs = self.parse_attributes()?;
        let mut specs = self.parse_decl_specifiers(true)?;
        specs.noreturn = specs.noreturn.or(attrs.noreturn);
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
        let mut quals = TypeQualifiers::NONE;
        let mut counts = SpecCounts::default();
        let mut tag: Option<Type> = None;
        let mut typedef_name: Option<Ident> = None;
        let mut consumed_any = false;

        loop {
            let has_type = counts.any() || tag.is_some() || typedef_name.is_some();
            // C23 allows an attribute specifier sequence among the specifiers.
            if self.at_attributes() {
                let attrs = self.parse_attributes()?;
                noreturn = noreturn.or(attrs.noreturn);
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
                    Keyword::ThreadLocal | Keyword::ThreadLocalName => {
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
                if matches!(k, Keyword::Typeof | Keyword::TypeofUnqual) {
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
        // C23's `auto x = e;`: a declaration with `auto` and no type
        // specifier at all takes its type from the initialiser.
        let inferred = self.standard >= Standard::C23
            && !counts.any()
            && tag.is_none()
            && typedef_name.is_none()
            && matches!(
                storage,
                Some(Spanned {
                    node: StorageClass::Auto,
                    ..
                })
            );
        let base = if inferred {
            Type::new(TypeKind::Auto, quals, specs_range)
        } else {
            self.build_base_type(&counts, tag, typedef_name, quals, specs_range)
        };

        Ok(DeclSpecifiers {
            storage,
            inline,
            noreturn,
            alignas,
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
        Ok(Type::plain(TypeKind::Typeof(Box::new(operand)), range))
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
        } else {
            if counts.complex > 0 || counts.imaginary > 0 {
                self.error(
                    range,
                    "'_Complex' and '_Imaginary' require a floating type specifier",
                );
            }
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
    fn parse_record_specifier(&mut self) -> PResult<Type> {
        let start = self.cur_range();
        let kind = match self.peek().keyword() {
            Some(Keyword::Struct) => RecordKind::Struct,
            Some(Keyword::Union) => RecordKind::Union,
            _ => unreachable!("caller checked the keyword"),
        };
        self.advance();
        // An attribute specifier sequence is allowed here and ignored.
        let _ = self.parse_attributes()?;
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
        let range = self.span_to_here(start);
        Ok(Type::plain(
            TypeKind::Record(Box::new(RecordType {
                kind,
                name,
                fields,
                asserts,
                range,
            })),
            range,
        ))
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
            // An attribute specifier sequence is allowed here and ignored.
            let _ = self.parse_attributes()?;
            let specs = self.parse_decl_specifiers(false)?;

            if self.at_punct(Punct::Semi) {
                // An anonymous struct/union member: C11 6.7.2.1p13.
                let range = self.span_to_here(start);
                self.require_standard(Standard::C11, "an anonymous struct or union member", range);
                fields.push(FieldDecl {
                    ty: specs.base.clone(),
                    specifiers: specs,
                    name: None,
                    bit_width: None,
                    range,
                });
                self.expect_punct(Punct::Semi, " after member declaration")?;
                continue;
            }

            loop {
                let (name, ty, dstart) = if self.at_punct(Punct::Colon) {
                    (None, specs.base.clone(), self.cur_range())
                } else {
                    let d = self.parse_declarator(specs.base.clone(), true)?;
                    (d.name, d.ty, d.range)
                };
                let bit_width = if self.eat_punct(Punct::Colon).is_some() {
                    Some(self.parse_conditional_expr()?)
                } else {
                    None
                };
                let range = self.span_to_here(dstart);
                fields.push(FieldDecl {
                    specifiers: specs.clone(),
                    name,
                    ty,
                    bit_width,
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
        let underlying = if self.at_punct(Punct::Colon) {
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
        Ok(Type::plain(
            TypeKind::Enum(Box::new(EnumType {
                name,
                enumerators,
                underlying,
                range,
            })),
            range,
        ))
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
        let mut ty = base;

        // `* qual* ` repeated: the leftmost `*` becomes the innermost pointer,
        // so `int * const * p` is "pointer to const pointer to int".
        while self.at_punct(Punct::Star) {
            let star = self.bump_range();
            let quals = self.parse_type_qualifiers();
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
            return Ok(DeclaratorResult {
                name: inner.name,
                ty: inner.ty,
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
        // (`int x [[deprecated]];`), which is ignored like every other.
        let _ = self.parse_attributes()?;
        let ty = self.parse_type_suffix(ty)?;
        Ok(DeclaratorResult {
            name,
            ty,
            range: self.span_to_here(start),
        })
    }

    /// At a `(` that begins a direct-declarator: does it group a nested
    /// declarator, or is it a parameter list?
    fn is_grouping_paren(&self) -> bool {
        !self.nth(1).is_punct(Punct::RParen) && !self.starts_decl_specifier(1)
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
            loop {
                if self.eat_punct(Punct::Dot).is_some() {
                    let field = self.expect_ident(" after '.' in designator")?;
                    designators.push(Designator::Field(field));
                } else if self.eat_punct(Punct::LBracket).is_some() {
                    let index = self.parse_conditional_expr()?;
                    self.expect_punct(Punct::RBracket, " after array designator")?;
                    designators.push(Designator::Index(index));
                } else {
                    break;
                }
            }
            if !designators.is_empty() {
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
            range: start.join(end),
        })
    }

    fn parse_stmt(&mut self) -> PResult<Stmt> {
        self.enter()?;
        let result = self.parse_stmt_inner();
        self.leave();
        result
    }

    fn parse_stmt_inner(&mut self) -> PResult<Stmt> {
        let start = self.cur_range();
        // A statement may carry attributes of its own: `[[fallthrough]];`,
        // `[[likely]] if (…)`. They are consumed and dropped.
        let _ = self.parse_attributes()?;

        if self.at_punct(Punct::LBrace) {
            let block = self.parse_compound_stmt()?;
            return Ok(Stmt {
                range: block.range,
                kind: StmtKind::Compound(block),
            });
        }

        // `label:`
        if self.peek().ident().is_some() && self.nth(1).is_punct(Punct::Colon) {
            let label = self.eat_ident().expect("checked above");
            let colon = self.bump_range();
            // C23 lets a label stand before a declaration and at the very end
            // of a compound statement; before that it had to label a
            // statement. Either way the label itself labels nothing, so it
            // takes a null statement and whatever follows is parsed on its
            // own.
            let empty = if self.at_punct(Punct::RBrace) {
                Some("a label at the end of a compound statement")
            } else if self.starts_declaration() || self.at_static_assert() {
                Some("a label before a declaration")
            } else {
                None
            };
            let body = match empty {
                Some(what) => {
                    self.require_standard(Standard::C23, what, label.range.join(colon));
                    Stmt {
                        kind: StmtKind::Expr(None),
                        range: colon,
                    }
                }
                None => self.parse_stmt()?,
            };
            return Ok(Stmt {
                kind: StmtKind::Labeled {
                    label,
                    body: Box::new(body),
                },
                range: self.span_to_here(start),
            });
        }

        if let Some(k) = self.peek().keyword() {
            match k {
                Keyword::Case => {
                    self.advance();
                    let value = self.parse_conditional_expr()?;
                    self.expect_punct(Punct::Colon, " after 'case' label")?;
                    let body = self.parse_stmt()?;
                    return Ok(Stmt {
                        kind: StmtKind::Case {
                            value,
                            body: Box::new(body),
                        },
                        range: self.span_to_here(start),
                    });
                }
                Keyword::Default => {
                    self.advance();
                    self.expect_punct(Punct::Colon, " after 'default' label")?;
                    let body = self.parse_stmt()?;
                    return Ok(Stmt {
                        kind: StmtKind::Default {
                            body: Box::new(body),
                        },
                        range: self.span_to_here(start),
                    });
                }
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

    fn parse_assignment_expr(&mut self) -> PResult<Expr> {
        let lhs = self.parse_conditional_expr()?;
        if let Some(op) = assign_op(&self.peek().kind) {
            self.advance();
            let rhs = self.parse_assignment_expr()?;
            let range = lhs.range.join(rhs.range);
            return Ok(Expr {
                kind: ExprKind::Assign {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                range,
            });
        }
        Ok(lhs)
    }

    fn parse_conditional_expr(&mut self) -> PResult<Expr> {
        let cond = self.parse_binary_expr(1)?;
        if self.eat_punct(Punct::Question).is_none() {
            return Ok(cond);
        }
        let then_expr = self.parse_expr()?;
        self.expect_punct(Punct::Colon, " in conditional expression")?;
        let else_expr = self.parse_conditional_expr()?;
        let range = cond.range.join(else_expr.range);
        Ok(Expr {
            kind: ExprKind::Conditional {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            },
            range,
        })
    }

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

        if let Some(k @ (Keyword::Alignof | Keyword::AlignofName)) = self.peek().keyword() {
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

        self.parse_postfix_expr()
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

    fn parse_postfix_suffixes(&mut self, mut expr: Expr) -> PResult<Expr> {
        loop {
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
