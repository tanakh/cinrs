//! The C99 abstract syntax tree.
//!
//! Three design points matter for the milestones that follow:
//!
//! * **Every node carries a [`SourceRange`]**, so a diagnostic raised by sema
//!   (M0b) or a token emitted by codegen can be attributed to the exact piece
//!   of C it came from.
//!
//! * **Declarators are resolved into a type tree while parsing.** The parser
//!   never hands out a raw declarator chain; `int (*fp[3])(void)` arrives as
//!   `array of 3 pointer to function(void) returning int`. Sema therefore only
//!   ever has to walk [`Type`], which is also the shape codegen needs.
//!
//! * **The body of a tag specifier is stored once.** Resolving declarators
//!   eagerly means one specifier is named by several types — `struct S { … } a,
//!   b;` is the declaration's specifiers plus the type of each declarator — so
//!   the member list lives in [`TranslationUnit::records`] and the types carry
//!   a [`RecordSpecId`]. Copying it instead would double the tree at every
//!   level of a nested `struct`.

use crate::capture::SourceRange;
use crate::lex::{CharLit, FloatLit, IntLit, StrLit};

/// A value paired with the source range it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Spanned<T> {
    /// The value.
    pub node: T,
    /// Where it was written.
    pub range: SourceRange,
}

impl<T> Spanned<T> {
    /// Pairs `node` with `range`.
    pub fn new(node: T, range: SourceRange) -> Self {
        Self { node, range }
    }
}

/// An identifier occurrence.
#[derive(Clone, Debug, PartialEq)]
pub struct Ident {
    /// The spelling.
    pub name: String,
    /// Where it was written.
    pub range: SourceRange,
}

// ---------------------------------------------------------------------------
// types
// ---------------------------------------------------------------------------

/// `const` / `volatile` / `restrict`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypeQualifiers {
    /// `const`
    pub is_const: bool,
    /// `volatile`
    pub is_volatile: bool,
    /// `restrict`
    pub is_restrict: bool,
    /// `_Atomic`, written without a parenthesised type name — C11 6.7.3 makes
    /// it a qualifier there and a type *specifier* in `_Atomic(T)`.
    ///
    /// Unlike the other three it changes the type: `_Atomic int` resolves to
    /// [`crate::ir::Ty::Atomic`], which is a different type from `int` and has
    /// its own alignment. That is why it is not part of [`TypeQualifiers::any`],
    /// which asks about the qualifiers a type *name* carries beside its type.
    pub is_atomic: bool,
}

impl TypeQualifiers {
    /// No qualifiers at all.
    pub const NONE: Self = Self {
        is_const: false,
        is_volatile: false,
        is_restrict: false,
        is_atomic: false,
    };

    /// Whether any of the three qualifiers that leave the type alone is set.
    pub fn any(self) -> bool {
        self.is_const || self.is_volatile || self.is_restrict
    }

    /// The union of two qualifier sets.
    pub fn merge(self, other: Self) -> Self {
        Self {
            is_const: self.is_const || other.is_const,
            is_volatile: self.is_volatile || other.is_volatile,
            is_restrict: self.is_restrict || other.is_restrict,
            is_atomic: self.is_atomic || other.is_atomic,
        }
    }
}

/// Signedness of an integer type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    /// `signed` (explicitly or by default).
    Signed,
    /// `unsigned`.
    Unsigned,
}

/// Rank of a standard integer type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntSize {
    /// `short`
    Short,
    /// `int`
    Int,
    /// `long`
    Long,
    /// `long long`
    LongLong,
    /// GNU's `__int128`.
    Int128,
}

/// Rank of a standard floating type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatSize {
    /// `float`
    Float,
    /// `double`
    Double,
    /// `long double`
    LongDouble,
}

/// `struct` or `union`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordKind {
    /// `struct`
    Struct,
    /// `union`
    Union,
}

impl RecordKind {
    /// The keyword that introduces this kind of record.
    pub fn as_str(self) -> &'static str {
        match self {
            RecordKind::Struct => "struct",
            RecordKind::Union => "union",
        }
    }
}

/// The size of an array derivation.
#[derive(Clone, Debug, PartialEq)]
pub enum ArraySize {
    /// `[]` — an incomplete array type.
    Unspecified,
    /// `[*]` — a variable length array of unspecified size, only valid in a
    /// function prototype.
    Star,
    /// `[expr]`. Whether this is a constant array bound or a VLA is decided by
    /// constant evaluation in sema.
    Expr(Box<Expr>),
}

/// A C type, as produced by resolving declaration specifiers and a declarator.
#[derive(Clone, Debug, PartialEq)]
pub struct Type {
    /// What kind of type this is.
    pub kind: TypeKind,
    /// Qualifiers applied to *this* type (not to what it points at).
    pub qualifiers: TypeQualifiers,
    /// The source range of the construct that produced this type.
    pub range: SourceRange,
}

impl Type {
    /// Builds a type.
    pub fn new(kind: TypeKind, qualifiers: TypeQualifiers, range: SourceRange) -> Self {
        Self {
            kind,
            qualifiers,
            range,
        }
    }

    /// Builds an unqualified type.
    pub fn plain(kind: TypeKind, range: SourceRange) -> Self {
        Self::new(kind, TypeQualifiers::NONE, range)
    }

    /// Whether this is the placeholder produced by error recovery.
    pub fn is_error(&self) -> bool {
        matches!(self.kind, TypeKind::Error)
    }
}

/// The shape of a [`Type`].
#[derive(Clone, Debug, PartialEq)]
pub enum TypeKind {
    /// `void`
    Void,
    /// `_Bool`
    Bool,
    /// `char`, `signed char` or `unsigned char`. `None` means plain `char`,
    /// which is a distinct type from both of the others.
    Char(Option<Sign>),
    /// `short`, `int`, `long`, `long long`, signed or unsigned.
    Int {
        /// Signedness.
        sign: Sign,
        /// Rank.
        size: IntSize,
    },
    /// `float`, `double`, `long double`.
    Float(FloatSize),
    /// `float _Complex` and friends.
    Complex(FloatSize),
    /// `float _Imaginary` and friends.
    Imaginary(FloatSize),
    /// Pointer to the given type.
    Pointer(Box<Type>),
    /// Array derivation.
    Array {
        /// Element type.
        elem: Box<Type>,
        /// The bound.
        size: ArraySize,
        /// Qualifiers written inside the brackets (`int p[const 4]`); only
        /// meaningful on a parameter.
        qualifiers: TypeQualifiers,
        /// Whether `static` was written inside the brackets.
        is_static: bool,
    },
    /// Function derivation.
    Function(Box<FunctionType>),
    /// A `struct`/`union` specifier, with or without a body.
    ///
    /// The specifier itself lives in [`TranslationUnit::records`]; see
    /// [`RecordSpecId`] for why it is an id rather than the body.
    Record(RecordSpecId),
    /// An `enum` specifier, with or without a body.
    Enum(EnumSpecId),
    /// A name introduced by `typedef`, not yet resolved.
    Typedef(Ident),
    /// `typeof(…)` / `typeof_unqual(…)` — C23.
    ///
    /// The operand lives in [`TranslationUnit::typeofs`]; see [`TypeofId`].
    Typeof {
        /// Which operand.
        id: TypeofId,
        /// Whether this is `typeof_unqual`, which takes the *unqualified*
        /// type of its operand (C23 6.7.2.5p3).
        ///
        /// Only one qualifier is part of a [`crate::ir::Ty`] at all —
        /// `_Atomic`, which changes the size, the alignment and what an access
        /// to the object does — so that is the one this strips.
        unqual: bool,
    },
    /// The type `auto x = e;` infers from the initialiser — C23.
    Auto,
    /// Produced by error recovery; sema must not report further errors on it.
    Error,
}

/// Identifies one `typeof` operand in [`TranslationUnit::typeofs`].
///
/// Stored apart for the reason a [`RecordSpecId`] is: a type name holds both
/// the specifiers it was written with and the type its declarator produced, so
/// a `typeof` written inside another one would otherwise be copied twice per
/// level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeofId(pub u32);

impl TypeofId {
    /// The index into [`TranslationUnit::typeofs`].
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// What `typeof` was applied to.
//
// The AST is built once per macro invocation and then walked; boxing the type
// name to even the variants out would only make sema noisier to write.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum TypeofOperand {
    /// `typeof(expr)`, whose operand is not evaluated.
    Expr(Expr),
    /// `typeof(type-name)`.
    Type(TypeName),
}

/// A function type derivation.
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionType {
    /// The return type.
    pub ret: Type,
    /// The declared parameters (empty for `f(void)` and for `f()`).
    pub params: Vec<ParamDecl>,
    /// Whether the parameter list ended with `, ...`.
    pub variadic: bool,
    /// Where the `...` was written, which is where a diagnostic about it
    /// belongs.
    pub ellipsis: Option<SourceRange>,
    /// Whether a prototype was given at all. `int f()` and `int f(a, b)` have
    /// no prototype; `int f(void)` and `int f(int)` do.
    pub has_prototype: bool,
    /// The identifier list of an old-style (K&R) declarator, e.g. `f(a, b)`.
    pub kr_names: Vec<Ident>,
    /// Whether [`FunctionType::params`] came from an identifier list and the
    /// declaration list of an old-style definition rather than from a
    /// prototype.
    ///
    /// Sema builds that list — see `Sema::old_style_params` — so the rest of
    /// the front end sees ordinary parameters; the flag is what remembers that
    /// the *type* still has no prototype and that the generated item takes the
    /// promoted types (C99 6.9.1p7).
    pub old_style: bool,
}

/// One parameter of a function prototype.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamDecl {
    /// The parameter's declaration specifiers.
    pub specifiers: DeclSpecifiers,
    /// The parameter name, absent in an abstract declarator.
    pub name: Option<Ident>,
    /// The parameter's type after applying its declarator.
    pub ty: Type,
    /// Where the parameter was written.
    pub range: SourceRange,
}

/// Identifies one `struct`/`union` specifier in [`TranslationUnit::records`].
///
/// A declarator is resolved into a [`Type`] as it is parsed, so the specifier
/// of `struct S { … } a, b;` is named by the type of every declarator that
/// shares it *and* by the declaration's own specifiers. Storing the member
/// list once and handing out an index keeps that from copying the body — which,
/// for a record nested inside a record, would double the data at every level.
///
/// An index rather than an `Rc` because the tree crosses the thread boundary
/// [`crate::analyze`] puts the deep passes behind, and has to stay [`Send`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RecordSpecId(pub u32);

impl RecordSpecId {
    /// The index into [`TranslationUnit::records`].
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Identifies one `enum` specifier in [`TranslationUnit::enums`].
///
/// See [`RecordSpecId`], which this is the enumeration's counterpart of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EnumSpecId(pub u32);

impl EnumSpecId {
    /// The index into [`TranslationUnit::enums`].
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A `struct` or `union` specifier, as written.
///
/// One of these lives in [`TranslationUnit::records`] per specifier in the
/// source, and [`TypeKind::Record`] names it by [`RecordSpecId`].
#[derive(Clone, Debug, PartialEq)]
pub struct RecordSpec {
    /// `struct` or `union`.
    pub kind: RecordKind,
    /// The tag, if one was written.
    pub name: Option<Ident>,
    /// The member list; `None` for a reference such as `struct S *p`.
    pub fields: Option<Vec<FieldDecl>>,
    /// The `_Static_assert` declarations written among the members (C11).
    ///
    /// They are kept apart from the members because they declare nothing: a
    /// member list with one in it lays out exactly as it would without.
    pub asserts: Vec<StaticAssert>,
    /// What `__attribute__((packed))` and friends asked of the whole record.
    pub attrs: Attributes,
    /// The member alignment `#pragma pack(N)` was asking for where the
    /// specifier was written, if any.
    pub pack: Option<u32>,
    /// Where the specifier was written.
    pub range: SourceRange,
}

/// `_Static_assert(expr, "message");` — C11 6.7.10, whose message C23 makes
/// optional.
#[derive(Clone, Debug, PartialEq)]
pub struct StaticAssert {
    /// The constant expression that must be non-zero.
    pub cond: Expr,
    /// The message, as written (quotes included), if one was given.
    pub message: Option<String>,
    /// Where the whole declaration was written.
    pub range: SourceRange,
}

/// One member of a `struct` or `union`.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldDecl {
    /// The member's declaration specifiers.
    pub specifiers: DeclSpecifiers,
    /// The member name; absent for an anonymous bit-field or an anonymous
    /// struct/union member.
    pub name: Option<Ident>,
    /// The member's type.
    pub ty: Type,
    /// The bit-field width, if any.
    pub bit_width: Option<Expr>,
    /// What `__attribute__((…))` on this member asked for.
    pub attrs: Attributes,
    /// Where the member was written.
    pub range: SourceRange,
}

/// An `enum` specifier, as written.
///
/// Stored in [`TranslationUnit::enums`] and named by [`EnumSpecId`], for the
/// same reason a [`RecordSpec`] is.
#[derive(Clone, Debug, PartialEq)]
pub struct EnumSpec {
    /// The tag, if one was written.
    pub name: Option<Ident>,
    /// The enumerator list; `None` for a reference such as `enum E e`.
    pub enumerators: Option<Vec<Enumerator>>,
    /// The fixed underlying type (`enum E : unsigned char { … }`), C23.
    pub underlying: Option<Type>,
    /// Where the specifier was written.
    pub range: SourceRange,
}

/// One `enum` constant.
#[derive(Clone, Debug, PartialEq)]
pub struct Enumerator {
    /// The constant's name.
    pub name: Ident,
    /// Its explicit value, if given.
    pub value: Option<Expr>,
    /// Where it was written.
    pub range: SourceRange,
}

// ---------------------------------------------------------------------------
// declarations
// ---------------------------------------------------------------------------

/// A storage-class specifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageClass {
    /// `typedef`
    Typedef,
    /// `extern`
    Extern,
    /// `static`
    Static,
    /// `auto`
    Auto,
    /// `register`
    Register,
    /// `constexpr` — C23.
    Constexpr,
}

impl StorageClass {
    /// The keyword spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            StorageClass::Typedef => "typedef",
            StorageClass::Extern => "extern",
            StorageClass::Static => "static",
            StorageClass::Auto => "auto",
            StorageClass::Register => "register",
            StorageClass::Constexpr => "constexpr",
        }
    }
}

/// An `_Alignas` / `alignas` specifier (C11 6.7.5).
#[derive(Clone, Debug, PartialEq)]
pub struct Alignment {
    /// What was written inside the parentheses.
    pub kind: AlignmentKind,
    /// Where the specifier was written.
    pub range: SourceRange,
}

/// The operand of an [`Alignment`].
#[derive(Clone, Debug, PartialEq)]
pub enum AlignmentKind {
    /// `_Alignas(16)` — a constant expression.
    Expr(Expr),
    /// `_Alignas(double)` — the alignment of a type.
    Type(Box<TypeName>),
}

/// What an attribute specifier sequence asked for.
///
/// Both spellings — GNU's `__attribute__((…))` and C23's `[[…]]` — produce
/// this, because they say the same things. Everything the front end does not
/// act on is dropped while it is parsed, which C23 6.7.13.1p3 explicitly
/// allows and which GCC does with a warning this crate has no way to raise.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attributes {
    /// `noreturn` / `[[noreturn]]`, which says exactly what `_Noreturn` says.
    pub noreturn: Option<SourceRange>,
    /// `always_inline`.
    pub always_inline: Option<SourceRange>,
    /// `noinline`.
    pub noinline: Option<SourceRange>,
    /// `cold`; `hot` clears it again, as GCC's do.
    pub cold: Option<SourceRange>,
    /// `deprecated`, with the message it was given.
    pub deprecated: Option<Spanned<Option<String>>>,
    /// `packed`, on a record or on one member.
    pub packed: Option<SourceRange>,
    /// `aligned(N)`, which is the same request `_Alignas(N)` makes.
    pub aligned: Option<Alignment>,
    /// `section("…")`.
    pub section: Option<Spanned<String>>,
    /// `constructor`, whose priority is parsed and ignored.
    pub constructor: Option<SourceRange>,
    /// `destructor`, likewise.
    pub destructor: Option<SourceRange>,
}

impl Attributes {
    /// Whether nothing was asked for.
    pub fn is_empty(&self) -> bool {
        *self == Attributes::default()
    }

    /// Everything either sequence asked for.
    ///
    /// GCC lets the attributes of one declaration be spread over the
    /// specifiers, the declarator and the definition, and takes the union.
    pub fn merge(&mut self, other: Attributes) {
        self.noreturn = self.noreturn.or(other.noreturn);
        self.always_inline = self.always_inline.or(other.always_inline);
        self.noinline = self.noinline.or(other.noinline);
        self.cold = self.cold.or(other.cold);
        self.deprecated = self.deprecated.take().or(other.deprecated);
        self.packed = self.packed.or(other.packed);
        self.aligned = self.aligned.take().or(other.aligned);
        self.section = self.section.take().or(other.section);
        self.constructor = self.constructor.or(other.constructor);
        self.destructor = self.destructor.or(other.destructor);
    }
}

/// The declaration specifiers shared by all declarators of one declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclSpecifiers {
    /// The storage class, if one was written.
    pub storage: Option<Spanned<StorageClass>>,
    /// Where `_Thread_local`, C23's `thread_local` or GNU's `__thread` was
    /// written, if it was.
    ///
    /// It is not a [`StorageClass`], because C11 6.7.1p2 lets it appear
    /// *beside* one: `static _Thread_local int x;` names a thread-local object
    /// with internal linkage, and at block scope one of `static` or `extern`
    /// is required (6.7.1p3).
    pub thread_local: Option<SourceRange>,
    /// Whether `inline` was written.
    pub inline: bool,
    /// Whether the declaration was marked `_Noreturn`, `[[noreturn]]` or
    /// `__cinrs_noreturn`.
    pub noreturn: Option<SourceRange>,
    /// The alignment specifier, if one was written.
    pub alignas: Option<Alignment>,
    /// What `__attribute__((…))` and `[[…]]` asked for.
    pub attrs: Attributes,
    /// The base type built from the type specifiers and qualifiers.
    pub base: Type,
    /// Where the specifiers were written.
    pub range: SourceRange,
}

impl DeclSpecifiers {
    /// Whether this declaration introduces `typedef` names.
    pub fn is_typedef(&self) -> bool {
        matches!(
            self.storage,
            Some(Spanned {
                node: StorageClass::Typedef,
                ..
            })
        )
    }
}

/// A declaration: specifiers plus zero or more declarators.
#[derive(Clone, Debug, PartialEq)]
pub struct Decl {
    /// The shared specifiers.
    pub specifiers: DeclSpecifiers,
    /// The declared entities.
    pub declarators: Vec<InitDeclarator>,
    /// Where the declaration was written, including its `;`.
    pub range: SourceRange,
}

/// One declarator of a [`Decl`], with its optional initialiser.
#[derive(Clone, Debug, PartialEq)]
pub struct InitDeclarator {
    /// The declared name; absent only after error recovery.
    pub name: Option<Ident>,
    /// The fully resolved type.
    pub ty: Type,
    /// The initialiser, if any.
    pub init: Option<Initializer>,
    /// What `__attribute__((…))` written on *this* declarator asked for.
    pub attrs: Attributes,
    /// The symbol `__asm__("name")` renamed this declaration to.
    pub asm_label: Option<Spanned<String>>,
    /// Where the declarator was written.
    pub range: SourceRange,
}

/// An initialiser.
#[derive(Clone, Debug, PartialEq)]
pub struct Initializer {
    /// What kind of initialiser this is.
    pub kind: InitializerKind,
    /// Where it was written.
    pub range: SourceRange,
}

/// The shape of an [`Initializer`].
#[derive(Clone, Debug, PartialEq)]
pub enum InitializerKind {
    /// `= expr`
    Expr(Expr),
    /// `= { … }`
    List(Vec<InitItem>),
}

/// One element of a braced initialiser list.
#[derive(Clone, Debug, PartialEq)]
pub struct InitItem {
    /// Designators written before `=`, e.g. `.x` or `[3]`.
    pub designators: Vec<Designator>,
    /// The value.
    pub init: Initializer,
    /// Where the element was written.
    pub range: SourceRange,
}

/// A C99 designator.
#[derive(Clone, Debug, PartialEq)]
pub enum Designator {
    /// `.name`, and the pre-C99 `name:` GNU still accepts.
    Field(Ident),
    /// `[expr]`
    Index(Expr),
    /// `[low ... high]` — GNU's range designator, which fills every element of
    /// the range with the same value.
    Range(Expr, Expr),
}

/// A type name, as written in a cast, `sizeof(T)` or a compound literal.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeName {
    /// The specifier-qualifier list.
    pub specifiers: DeclSpecifiers,
    /// The type after applying the abstract declarator.
    pub ty: Type,
    /// Where it was written.
    pub range: SourceRange,
}

// ---------------------------------------------------------------------------
// statements
// ---------------------------------------------------------------------------

/// A compound statement.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// Declarations and statements, in source order (C99 allows mixing).
    pub items: Vec<BlockItem>,
    /// The labels `__label__ a, b;` declared local to this block.
    ///
    /// C gives every label function scope, and so does this crate, so the
    /// declaration is accepted and the labels behave as they would without it.
    pub local_labels: Vec<Ident>,
    /// Where the block was written, including its braces.
    pub range: SourceRange,
}

/// An element of a [`Block`].
//
// The AST is built once per macro invocation and then walked; boxing every
// large field to even out the variants would only make sema and codegen
// noisier to write.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum BlockItem {
    /// A declaration.
    Decl(Decl),
    /// A statement.
    Stmt(Stmt),
    /// `_Static_assert(…);` — C11.
    StaticAssert(StaticAssert),
}

/// A statement.
#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    /// What kind of statement this is.
    pub kind: StmtKind,
    /// Where it was written.
    pub range: SourceRange,
}

/// The shape of a [`Stmt`].
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    /// `label: stmt`
    Labeled {
        /// The label.
        label: Ident,
        /// The labelled statement.
        body: Box<Stmt>,
    },
    /// `case expr: stmt`, and GNU's `case low ... high: stmt`.
    Case {
        /// The case value, or the low end of a range.
        value: Expr,
        /// The high end of a `case low ... high:` range.
        upper: Option<Expr>,
        /// The labelled statement.
        body: Box<Stmt>,
    },
    /// `default: stmt`
    Default {
        /// The labelled statement.
        body: Box<Stmt>,
    },
    /// `{ … }`
    Compound(Block),
    /// `expr;` or the null statement `;`.
    Expr(Option<Expr>),
    /// `if (cond) then else otherwise`
    If {
        /// The controlling expression.
        cond: Expr,
        /// The `then` branch.
        then_branch: Box<Stmt>,
        /// The `else` branch, if any.
        else_branch: Option<Box<Stmt>>,
    },
    /// `switch (cond) body`
    Switch {
        /// The controlling expression.
        cond: Expr,
        /// The switch body.
        body: Box<Stmt>,
    },
    /// `while (cond) body`
    While {
        /// The controlling expression.
        cond: Expr,
        /// The loop body.
        body: Box<Stmt>,
    },
    /// `do body while (cond);`
    DoWhile {
        /// The loop body.
        body: Box<Stmt>,
        /// The controlling expression.
        cond: Expr,
    },
    /// `for (init; cond; step) body`
    For {
        /// The init clause.
        init: ForInit,
        /// The controlling expression, if any.
        cond: Option<Expr>,
        /// The iteration expression, if any.
        step: Option<Expr>,
        /// The loop body.
        body: Box<Stmt>,
    },
    /// `goto label;`
    Goto(Ident),
    /// `continue;`
    Continue,
    /// `break;`
    Break,
    /// `return expr;`
    Return(Option<Expr>),
    /// Produced by error recovery.
    Error,
}

/// The init clause of a `for` statement.
#[derive(Clone, Debug, PartialEq)]
pub enum ForInit {
    /// `for (; …)`
    None,
    /// `for (expr; …)`
    Expr(Expr),
    /// `for (int i = 0; …)` — C99.
    Decl(Box<Decl>),
}

// ---------------------------------------------------------------------------
// expressions
// ---------------------------------------------------------------------------

/// A prefix or unary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    /// `+x`
    Plus,
    /// `-x`
    Minus,
    /// `!x`
    LogNot,
    /// `~x`
    BitNot,
    /// `*p`
    Deref,
    /// `&x`
    AddrOf,
}

impl UnaryOp {
    /// The operator's spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            UnaryOp::Plus => "+",
            UnaryOp::Minus => "-",
            UnaryOp::LogNot => "!",
            UnaryOp::BitNot => "~",
            UnaryOp::Deref => "*",
            UnaryOp::AddrOf => "&",
        }
    }
}

/// A binary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
    BitAnd,
    BitXor,
    BitOr,
    LogAnd,
    LogOr,
}

impl BinaryOp {
    /// The operator's spelling.
    pub fn as_str(self) -> &'static str {
        use BinaryOp::*;
        match self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Rem => "%",
            Shl => "<<",
            Shr => ">>",
            Lt => "<",
            Gt => ">",
            Le => "<=",
            Ge => ">=",
            Eq => "==",
            Ne => "!=",
            BitAnd => "&",
            BitXor => "^",
            BitOr => "|",
            LogAnd => "&&",
            LogOr => "||",
        }
    }
}

/// `++` or `--`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncDec {
    /// `++`
    Inc,
    /// `--`
    Dec,
}

impl IncDec {
    /// The operator's spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            IncDec::Inc => "++",
            IncDec::Dec => "--",
        }
    }
}

/// An expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    /// What kind of expression this is.
    pub kind: ExprKind,
    /// Where it was written.
    pub range: SourceRange,
}

/// The shape of an [`Expr`].
#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    /// An identifier reference.
    Ident(Ident),
    /// An integer constant.
    Int(IntLit),
    /// A floating constant.
    Float(FloatLit),
    /// A character constant.
    Char(CharLit),
    /// A string literal, after adjacent-literal concatenation.
    Str(StrLit),
    /// A unary operator application.
    Unary {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: Box<Expr>,
    },
    /// A binary operator application.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// An assignment; `op` is `None` for `=` and `Some(op)` for `op=`.
    Assign {
        /// The compound operator, if any.
        op: Option<BinaryOp>,
        /// The assigned-to operand.
        lhs: Box<Expr>,
        /// The assigned value.
        rhs: Box<Expr>,
    },
    /// `cond ? then_expr : else_expr`, and GNU's `cond ?: else_expr`.
    Conditional {
        /// The condition.
        cond: Box<Expr>,
        /// The value when the condition holds; `None` for `a ?: b`, where it
        /// is the condition's own value and the condition is evaluated once.
        then_expr: Option<Box<Expr>>,
        /// The value otherwise.
        else_expr: Box<Expr>,
    },
    /// `lhs, rhs`
    Comma {
        /// Evaluated and discarded.
        lhs: Box<Expr>,
        /// The result.
        rhs: Box<Expr>,
    },
    /// `callee(args…)`
    Call {
        /// The called expression.
        callee: Box<Expr>,
        /// The arguments.
        args: Vec<Expr>,
    },
    /// `base.field` or `base->field`
    Member {
        /// The object or pointer.
        base: Box<Expr>,
        /// Whether `->` was used.
        arrow: bool,
        /// The member name.
        field: Ident,
    },
    /// `base[index]`
    Index {
        /// The array or pointer.
        base: Box<Expr>,
        /// The subscript.
        index: Box<Expr>,
    },
    /// `x++` / `x--`
    PostIncDec {
        /// Which operator.
        op: IncDec,
        /// The operand.
        operand: Box<Expr>,
    },
    /// `++x` / `--x`
    PreIncDec {
        /// Which operator.
        op: IncDec,
        /// The operand.
        operand: Box<Expr>,
    },
    /// `(T)expr`
    Cast {
        /// The target type.
        ty: Box<TypeName>,
        /// The operand.
        expr: Box<Expr>,
    },
    /// `sizeof expr`
    SizeofExpr(Box<Expr>),
    /// `sizeof(T)`
    SizeofType(Box<TypeName>),
    /// `_Alignof expr` — a GNU extension C never standardised.
    AlignofExpr(Box<Expr>),
    /// `_Alignof(T)` / `alignof(T)` — C11.
    AlignofType(Box<TypeName>),
    /// `_Generic(controlling, T: value, …)` — C11.
    Generic {
        /// The controlling expression, which is never evaluated.
        controlling: Box<Expr>,
        /// The associations, in the order written.
        assocs: Vec<GenericAssoc>,
    },
    /// `true` / `false` — C23.
    Bool(bool),
    /// `nullptr` — C23.
    Nullptr,
    /// `va_arg(ap, T)`.
    ///
    /// It looks like a call but takes a type name, so — like `sizeof` — the
    /// parser has to know about it.
    VaArg {
        /// The argument list read from.
        ap: Box<Expr>,
        /// The type of the argument being read.
        ty: Box<TypeName>,
    },
    /// `__builtin_offsetof(T, member)`, which `<stddef.h>`'s `offsetof` is.
    ///
    /// Another one that takes a type name where an expression would go.
    OffsetOf {
        /// The `struct` or `union` type.
        ty: Box<TypeName>,
        /// The first step of the member designator, which C requires to be a
        /// member name.
        member: Ident,
        /// The steps after it — `offsetof(struct S, a.b)` and
        /// `offsetof(struct S, a[2].b)` are both member designators
        /// (C99 7.17p3). A [`Designator::Range`] never appears here.
        path: Vec<Designator>,
    },
    /// `(T){ … }` — a C99 compound literal.
    CompoundLiteral {
        /// The literal's type.
        ty: Box<TypeName>,
        /// The initialiser elements.
        init: Vec<InitItem>,
    },
    /// `({ … })` — GNU's statement expression, whose value is the value of the
    /// last expression statement in the block.
    StmtExpr(Box<Block>),
    /// `__builtin_types_compatible_p(T1, T2)`, an integer constant.
    TypesCompatible {
        /// The first type.
        lhs: Box<TypeName>,
        /// The second type.
        rhs: Box<TypeName>,
    },
    /// `__builtin_choose_expr(c, a, b)`.
    ///
    /// Only the operand the constant condition picks is type checked, which is
    /// the whole point of the builtin.
    ChooseExpr {
        /// The controlling constant expression.
        cond: Box<Expr>,
        /// Chosen when it is non-zero.
        then_expr: Box<Expr>,
        /// Chosen otherwise.
        else_expr: Box<Expr>,
    },
    /// `__real__ e` / `__imag__ e`, which need complex arithmetic.
    ComplexPart {
        /// Whether this is `__real__`.
        real: bool,
        /// The operand.
        operand: Box<Expr>,
    },
    /// Produced by error recovery.
    Error,
}

/// One association of a `_Generic` selection.
#[derive(Clone, Debug, PartialEq)]
pub struct GenericAssoc {
    /// The type this association is chosen for; `None` for `default:`.
    pub ty: Option<TypeName>,
    /// The value the selection has when this association is chosen.
    pub value: Expr,
    /// Where the association was written.
    pub range: SourceRange,
}

// ---------------------------------------------------------------------------
// top level
// ---------------------------------------------------------------------------

/// A function definition.
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionDef {
    /// The declaration specifiers.
    pub specifiers: DeclSpecifiers,
    /// The function's name.
    pub name: Ident,
    /// The function's type; always [`TypeKind::Function`] unless recovery
    /// kicked in.
    pub ty: Type,
    /// Old-style parameter declarations (`int f(a) int a; { … }`). Empty for a
    /// prototype-style definition; sema decides whether to accept them.
    pub kr_decls: Vec<Decl>,
    /// What `__attribute__((…))` written after the declarator asked for.
    pub attrs: Attributes,
    /// The symbol `__asm__("name")` renamed the definition to.
    pub asm_label: Option<Spanned<String>>,
    /// The body.
    pub body: Block,
    /// Where the definition was written.
    pub range: SourceRange,
}

/// A top-level item.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum ExternalDecl {
    /// A function definition.
    Function(FunctionDef),
    /// A declaration.
    Decl(Decl),
    /// `_Static_assert(…);` — C11.
    StaticAssert(StaticAssert),
}

/// A whole translation unit — the contents of one `c99!` invocation.
#[derive(Clone, Debug, PartialEq)]
pub struct TranslationUnit {
    /// The top-level items.
    pub items: Vec<ExternalDecl>,
    /// Every `struct`/`union` specifier written in the unit, in source order.
    ///
    /// [`TypeKind::Record`] indexes this; see [`RecordSpecId`].
    pub records: Vec<RecordSpec>,
    /// Every `enum` specifier written in the unit, in source order.
    pub enums: Vec<EnumSpec>,
    /// Every `typeof` operand written in the unit, in source order.
    pub typeofs: Vec<TypeofOperand>,
    /// The range covering the whole input.
    pub range: SourceRange,
}

impl TranslationUnit {
    /// The `struct`/`union` specifier `id` names.
    pub fn record(&self, id: RecordSpecId) -> &RecordSpec {
        &self.records[id.index()]
    }

    /// The `enum` specifier `id` names.
    pub fn enum_spec(&self, id: EnumSpecId) -> &EnumSpec {
        &self.enums[id.index()]
    }

    /// The `typeof` operand `id` names.
    pub fn typeof_operand(&self, id: TypeofId) -> &TypeofOperand {
        &self.typeofs[id.index()]
    }
}
