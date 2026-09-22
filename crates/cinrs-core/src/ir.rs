//! The typed intermediate representation.
//!
//! [`sema`](crate::sema) turns the untyped [`ast`](crate::ast) into this tree
//! and [`codegen`](crate::codegen) turns this tree into Rust tokens. The two
//! halves are deliberately separated by a data structure rather than by a
//! traversal, for three reasons:
//!
//! * **Types never leak into the AST.** The AST records what the user wrote;
//!   the IR records what it *means*. Every implicit conversion C performs —
//!   integer promotions, the usual arithmetic conversions, array-to-pointer
//!   decay, the conversion on assignment, initialisation, argument passing and
//!   `return` — is an explicit node here, so codegen never has to re-derive a
//!   type or guess where an `as` belongs.
//!
//! * **Sema is pure.** Nothing in this module refers to `proc_macro2`; every
//!   node carries a [`SourceRange`] into the captured C text, which codegen
//!   resolves to a span. That is what lets sema run on a thread with a large
//!   stack while codegen — which needs the (non-`Send`) source map — runs on
//!   the caller's.
//!
//! * **Control flow is already resolved.** `break` and `continue` name the
//!   loop or switch they leave ([`BreakTarget`], [`LoopId`]), a `goto` names
//!   the label it jumps to ([`LabelId`]), and a `switch` arrives as an ordered
//!   list of groups rather than as a statement tree with labels buried in it.
//!   Codegen can therefore be a straight transliteration.
//!
//!   A function that jumps is the exception: `goto`, and a `case` label the
//!   groups cannot express, send the whole body through [`crate::cfg`] instead,
//!   and its [`Body`] is a graph of basic blocks rather than a statement list.
//!
//! # Interned types
//!
//! [`Ty`] stays a small `Copy` value even though C's types are trees: the
//! scalar types are variants of their own, and every derived type (pointer,
//! array, function) is an index into the [`Types`] arena that hash-conses them.
//! Two `int *` written in different places therefore compare equal with a
//! single integer comparison, which is what the assignment rules and the switch
//! over "what kind of thing is this" in codegen are written against.
//!
//! `struct`, `union` and `enum` are *nominal*, exactly as C says: each tag
//! definition allocates a [`RecordId`] or [`EnumId`] of its own, and two
//! structurally identical tags are different types. The tables also hold the
//! computed [`Layout`], so `sizeof` folds to a constant everywhere.
//!
//! # Places
//!
//! Anything assignable is a [`Place`]: a named object, `*p`, `a[i]`, `s.f`, a
//! string literal, or the temporary that holds a `struct` returned by value.
//! The abstraction is what compound assignment and `++`/`--` are written
//! against, so the "evaluate the operand exactly once" rule is expressed
//! structurally: `p[i()] += 1` is one [`ExprKind::CompoundAssign`] holding one
//! place, not a re-evaluated expression. Codegen lowers a place into a *setup*
//! (statements that must run first, where the pointer arithmetic lands) plus an
//! *access* that can be evaluated as often as needed.

use std::collections::HashMap;

use crate::capture::SourceRange;
use crate::target::TargetModel;

// ---------------------------------------------------------------------------
// types
// ---------------------------------------------------------------------------

/// The spelling of the `va_list` type the compiler owns, which means
/// [`Ty::VaList`].
///
/// It is the only one: the bundled `<stdarg.h>` writes
/// `typedef __builtin_va_list va_list;`, the way GCC's own header does, so a
/// translation unit that does not include it may use `va_list`, `va_end` and
/// the rest as ordinary identifiers of its own. The name is seeded into the
/// parser's and sema's outermost scopes, where a program may repeat the
/// `typedef` but not give the name a different meaning.
pub const VA_LIST_NAMES: &[&str] = &["__builtin_va_list"];

/// The `typedef` names the compiler owns for the two 128-bit integer types,
/// with the [`Ty`] each one means.
///
/// GCC predefines `__int128_t` and `__uint128_t` in every mode alongside the
/// `__int128` keyword, and a great deal of code spells them that way. Like
/// [`VA_LIST_NAMES`] they are seeded into the parser's and sema's outermost
/// scopes, so `__int128_t *p;` is a declaration rather than a multiplication.
pub const INT128_TYPEDEF_NAMES: &[(&str, Ty)] =
    &[("__int128_t", Ty::Int128), ("__uint128_t", Ty::UInt128)];

/// The `typedef` names the compiler owns for the x86 vector types, with the
/// [`Ty`] each one means.
///
/// `__m128` and its five relatives are the compiler's types, not the library's:
/// GCC writes them in `<xmmintrin.h>` as `__attribute__((vector_size(16)))`,
/// which cinrs has no equivalent of, so the bundled header writes
/// `typedef __cinrs_m128 __m128;` over a name seeded into the parser's and
/// sema's outermost scopes — exactly the arrangement [`VA_LIST_NAMES`] uses
/// for `va_list`. A translation unit that does not include the header may
/// therefore still use `__m128` as an identifier of its own.
pub const X86_VECTOR_TYPEDEF_NAMES: &[(&str, Ty)] = &[
    ("__cinrs_m128", Ty::Vector(VecTy::M128)),
    ("__cinrs_m128d", Ty::Vector(VecTy::M128d)),
    ("__cinrs_m128i", Ty::Vector(VecTy::M128i)),
    ("__cinrs_m256", Ty::Vector(VecTy::M256)),
    ("__cinrs_m256d", Ty::Vector(VecTy::M256d)),
    ("__cinrs_m256i", Ty::Vector(VecTy::M256i)),
];

/// The builtin C23's `unreachable()` stands for.
///
/// The bundled `<stddef.h>` writes `#define unreachable() __builtin_unreachable()`,
/// the way GCC's does, so a unit that does not include it may use the name
/// `unreachable` for whatever it likes.
pub const UNREACHABLE_BUILTIN: &str = "__builtin_unreachable";

/// The names Rust cannot spell even as raw identifiers.
///
/// A C identifier that is one of them is generated with an underscore
/// appended, which both code generation and the naming of the bit-field
/// accessors have to agree on.
pub const NEVER_RAW: &[&str] = &["self", "Self", "super", "crate", "_"];

/// The Rust identifier a C name is generated as, spelled out.
///
/// Only the names [`NEVER_RAW`] lists change; a name that collides with an
/// ordinary keyword becomes a raw identifier, which is the same identifier.
///
/// This is the rule as it stands before the translation unit is taken into
/// account — enough to tell two accessor names of one record apart, which is
/// all [`crate::sema`] needs it for. The spelling the generated code actually
/// carries is [`crate::codegen`]'s, which also keeps a changed spelling clear
/// of every other name the unit uses.
pub fn rust_name_of(name: &str) -> String {
    if NEVER_RAW.contains(&name) {
        return format!("{name}_");
    }
    name.to_owned()
}

/// A pointer type in the [`Types`] arena.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PointerId(pub u32);

/// An array type in the [`Types`] arena.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ArrayId(pub u32);

/// A function type in the [`Types`] arena.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FuncTyId(pub u32);

/// A `struct` or `union` definition in the [`Types`] arena.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RecordId(pub u32);

/// An `enum` definition in the [`Types`] arena.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EnumId(pub u32);

/// An `_Atomic` type in the [`Types`] arena.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AtomicId(pub u32);

/// A resolved C type.
///
/// `long double` is mapped onto [`Ty::Double`] when the type is resolved,
/// because there is no portable Rust type with the layout of an x87 extended
/// double; the mapping is documented rather than diagnosed, since a procedural
/// macro has no stable way to raise a warning.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Ty {
    /// `void`
    Void,
    /// `_Bool`
    Bool,
    /// Plain `char`, whose signedness is the target's business and which is a
    /// distinct type from both `signed char` and `unsigned char`.
    Char,
    /// `signed char`
    SChar,
    /// `unsigned char`
    UChar,
    /// `short`
    Short,
    /// `unsigned short`
    UShort,
    /// `int`
    Int,
    /// `unsigned int`
    UInt,
    /// `long`
    Long,
    /// `unsigned long`
    ULong,
    /// `long long`
    LongLong,
    /// `unsigned long long`
    ULongLong,
    /// GNU's `__int128` (also spelled `__int128_t`), which ranks above
    /// `long long` and is generated as Rust's `i128`.
    Int128,
    /// `unsigned __int128` (also spelled `__uint128_t`), generated as `u128`.
    UInt128,
    /// `float`
    Float,
    /// `double` (and `long double`)
    Double,
    /// `float _Complex`, generated as `cinrs_rt::Complex<f32>`.
    ///
    /// C calls the complex types *floating* types and therefore arithmetic
    /// ones, but almost nothing in this crate wants them where a `float` or a
    /// `double` goes: [`Ty::is_floating`] is deliberately the *real* floating
    /// types only, and [`Ty::is_complex`] is the question to ask about these.
    ComplexFloat,
    /// `double _Complex` (and `long double _Complex`), generated as
    /// `cinrs_rt::Complex<f64>`.
    ComplexDouble,
    /// A pointer, including a pointer to a function.
    Pointer(PointerId),
    /// An array of a known length.
    Array(ArrayId),
    /// A function type. Only ever reached through a pointer or as the type of
    /// a function designator.
    Func(FuncTyId),
    /// A `struct` or `union`, complete or not.
    Record(RecordId),
    /// A file-scope `enum` with a tag, which becomes a named `c_int` alias.
    /// Every other `enum` is simply [`Ty::Int`].
    Enum(EnumId),
    /// `va_list` (and its `__builtin_va_list` / `__gnuc_va_list` spellings),
    /// which becomes [`core::ffi::VaList`].
    ///
    /// The type is opaque: it has no size, nothing may point at it, and it may
    /// only be a local variable or a parameter — see [`crate::sema`] for why.
    VaList,
    /// One of x86's vector types: `__m128`, `__m128i`, `__m128d`, `__m256`,
    /// `__m256i` or `__m256d`.
    ///
    /// Opaque, exactly as C sees it: there is no arithmetic on one, no
    /// conversion to or from one, and no way to reach a lane except through an
    /// intrinsic or through a union. What it *is* is an object of a known size
    /// and alignment — sixteen or thirty-two bytes, aligned to itself — which
    /// is what makes it a member, an element, a parameter, a return value and
    /// the thing a `union { __m128i v; int i[4]; }` punnes. Code generation
    /// writes `::core::arch::x86_64::__m128i`, whose layout is the same.
    Vector(VecTy),
    /// `_Atomic T`, for a scalar `T` (C11 6.7.2.4).
    ///
    /// It is the type of an *object*, never of a value: reading an atomic
    /// lvalue is an atomic load whose result has the underlying type, so
    /// [`ExprKind::Load`] of an atomic place is typed [`Types::unatomic`] of
    /// it and nothing downstream of the load ever meets this variant. Where it
    /// does appear is a declared object, a member, a pointee and `sizeof` —
    /// which is why it is a type rather than a flag on the declaration:
    /// `_Atomic int *` and `int *` are different types, and a store through
    /// the first one is atomic.
    ///
    /// The alignment is the size (see [`Types::size_align`]), which is what
    /// makes `_Atomic long long` eight-byte aligned on a target whose plain
    /// `long long` is not.
    Atomic(AtomicId),
    /// The type of something whose declaration was already reported as wrong.
    ///
    /// It exists so that one bad declaration produces one diagnostic: an object
    /// declared `int a[n]` still enters the symbol table, and every later use of
    /// it is checked against a type that silences further complaints instead of
    /// "use of undeclared identifier".
    Error,
}

/// Which x86 vector type a [`Ty::Vector`] is.
///
/// The name is the same in C and in `core::arch`, which is the whole point:
/// the generated Rust says `::core::arch::x86_64::__m128i` and Rust's type has
/// the size, the alignment and the calling convention C's has.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum VecTy {
    /// `__m128`: four `float` lanes.
    M128,
    /// `__m128i`: sixteen bytes of integer lanes, whatever width.
    M128i,
    /// `__m128d`: two `double` lanes.
    M128d,
    /// `__m256`: eight `float` lanes.
    M256,
    /// `__m256i`: thirty-two bytes of integer lanes.
    M256i,
    /// `__m256d`: four `double` lanes.
    M256d,
}

impl VecTy {
    /// The name, which C and `core::arch` spell the same way.
    pub fn name(self) -> &'static str {
        match self {
            VecTy::M128 => "__m128",
            VecTy::M128i => "__m128i",
            VecTy::M128d => "__m128d",
            VecTy::M256 => "__m256",
            VecTy::M256i => "__m256i",
            VecTy::M256d => "__m256d",
        }
    }

    /// `sizeof`, which is also `_Alignof`: a vector type is aligned to its own
    /// width on every x86 ABI, and so is Rust's.
    pub fn bytes(self) -> u64 {
        match self {
            VecTy::M128 | VecTy::M128i | VecTy::M128d => 16,
            VecTy::M256 | VecTy::M256i | VecTy::M256d => 32,
        }
    }
}

/// A pointer type: what it points at, and whether that is `const`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PointerType {
    /// The pointee type.
    pub pointee: Ty,
    /// Whether the pointee is `const`-qualified, which decides between
    /// `*const T` and `*mut T`.
    pub konst: bool,
}

/// One dimension of a [variably modified](Types::is_vm) array type.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum VmDim {
    /// A constant bound, outside a variable one: the `3` of `int a[3][n]`.
    Fixed(u64),
    /// A run-time bound, held by a hidden `size_t` object.
    Len(ObjectId),
    /// A run-time bound the type does not carry — `int a[*]`, or a
    /// prototype's, which is never evaluated (C99 6.7.5.3p7).
    Unknown,
}

/// An array type.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ArrayType {
    /// The element type.
    pub elem: Ty,
    /// The number of elements, zero for a [variable length
    /// array](ArrayType::vla), whose length is only known at run time.
    pub len: u64,
    /// Whether the element type is `const`-qualified, which is what decides
    /// the constness of the pointer the array decays to.
    pub elem_const: bool,
    /// Whether this is a variable length array (C99 6.7.5.2), whose bound was
    /// not an integer constant expression.
    ///
    /// The bound itself is not a number here but a *run-time object*, named by
    /// [`ArrayType::vla_len`], which is why `sizeof` of one is an expression
    /// rather than a constant. See [`Stmt::Vla`].
    pub vla: bool,
    /// The hidden `size_t` object holding this dimension's length, for a
    /// [variable length array](ArrayType::vla) whose declaration evaluated its
    /// bound.
    ///
    /// `None` says the length is not available: `int a[*]`, and the parameter
    /// types of a prototype, whose bounds are never evaluated because C99
    /// 6.7.5.3p7 leaves them out of the type. Two variably modified types are
    /// compatible whatever their bounds (6.7.5.2p6), so this is deliberately
    /// *not* part of what [`crate::sema`] compares — only of what the
    /// generated code computes with.
    pub vla_len: Option<ObjectId>,
    /// Whether the bound was left out — `int j[]`, C99 6.2.5p22's *incomplete*
    /// array type.
    ///
    /// It is what `extern int j[];` declares, and what a file-scope `int j[];`
    /// with no initialiser is until the end of the translation unit completes
    /// it to one element (6.9.2p5). `sizeof` of one is an error, but it may be
    /// pointed at, subscripted and decayed like any other array, and it is
    /// *compatible* with every completed array of the same element type
    /// (6.2.7p1), which is what lets `extern int j[]; int j[3];` declare one
    /// object.
    pub incomplete: bool,
}

/// A function type.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct FuncType {
    /// The return type.
    pub ret: Ty,
    /// The parameter types, after the adjustments C applies to them.
    pub params: Vec<Ty>,
    /// Whether the prototype ended with `, ...`.
    pub variadic: bool,
    /// Whether a parameter type list was given at all.
    ///
    /// `int f(void)` and `int f(int)` have a prototype; `int f()` — before C23,
    /// which removed the form — does not, and says nothing about the number or
    /// the types of the parameters. An unprototyped type has no parameters and
    /// is never variadic, so `ret` is all that distinguishes two of them; the
    /// difference from `int (void)` is what a call site has to know, since an
    /// argument passed to one gets the default argument promotions and is then
    /// passed as if the prototype had been written that way (C99 6.5.2.2p6).
    pub prototyped: bool,
}

/// `struct` or `union`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
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

/// The size and alignment of a complete type, in bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layout {
    /// `sizeof` the type.
    pub size: u64,
    /// `_Alignof` the type.
    pub align: u64,
}

/// What makes a member a bit-field (C99 6.7.2.1).
///
/// A bit-field has no address of its own, so it is not a Rust field: a maximal
/// run of them shares one `[u8; K]` storage field, and reading or writing one
/// goes through a pair of generated accessors. Everything code generation needs
/// to emit those — where the bits are and how wide they are — lives here.
#[derive(Clone, Debug)]
pub struct BitField {
    /// The declared width, in bits.
    pub width: u32,
    /// The bit offset from the start of the record, counting from the least
    /// significant bit of byte 0 (the little-endian bit order every ABI this
    /// crate targets uses).
    pub bit_offset: u64,
    /// Whether reading the field sign-extends.
    ///
    /// Usually the signedness of the declared type; an `enum` bit-field follows
    /// the enumeration's underlying type instead, which GCC and Clang make
    /// unsigned when no enumerator is negative.
    pub signed: bool,
    /// The name of the `[u8; K]` field the bits live in.
    pub storage: String,
    /// The byte offset of that field within the record.
    pub storage_offset: u64,
    /// The name of the generated getter.
    pub getter: String,
    /// The name of the generated setter.
    pub setter: String,
}

impl BitField {
    /// The bit offset of the field within its storage array.
    pub fn offset_in_storage(&self) -> u64 {
        self.bit_offset - self.storage_offset * 8
    }
}

/// One member of a `struct` or `union`.
#[derive(Clone, Debug)]
pub struct Field {
    /// The member name as written in C, or the synthetic `__cinrs_anonN` an
    /// anonymous member is generated under.
    pub name: String,
    /// Whether this is an anonymous `struct`/`union` member (C11 6.7.2.1p13),
    /// whose own members are reached through it as if they were the enclosing
    /// record's.
    pub anonymous: bool,
    /// The member type.
    pub ty: Ty,
    /// Whether the member's type is `const`-qualified.
    pub is_const: bool,
    /// The byte offset from the start of the record (always 0 in a union).
    ///
    /// For a bit-field this is the byte the field's first bit falls in; the
    /// exact position is in [`Field::bits`].
    pub offset: u64,
    /// Set when the member was declared with a width.
    pub bits: Option<BitField>,
    /// Whether this is the flexible array member `int data[];` — an array of
    /// no elements that the object is expected to be over-allocated for.
    pub flexible: bool,
    /// Where the member was declared.
    pub range: SourceRange,
}

/// One field of the generated Rust item.
///
/// A record without bit-fields maps one C member onto one Rust field, and this
/// is simply its member list. Bit-fields break that correspondence: they share
/// storage, they may be unnamed, and the bytes they occupy do not always start
/// where `#[repr(C)]` would put the next field on its own.
#[derive(Clone, Debug)]
pub enum RustField {
    /// A C member, by its index in [`RecordDef::fields`].
    Member(usize),
    /// The bytes one maximal run of bit-fields lives in.
    Bits {
        /// The field name, `__cinrs_bitsN`.
        name: String,
        /// The byte offset of the run within the record.
        offset: u64,
        /// How many bytes it covers.
        bytes: u64,
    },
    /// Filler that puts the field after it where C puts it.
    Pad {
        /// The field name, `__cinrs_padN`.
        name: String,
        /// How many bytes it covers.
        bytes: u64,
    },
    /// A zero-sized field whose only job is to raise the item's alignment.
    ///
    /// `#[repr(C, align(N))]` says the same thing and reads better, so it is
    /// what a record normally carries. A record that is a member of a *packed*
    /// one cannot use it: Rust refuses a packed type that transitively holds a
    /// `#[repr(align)]` one (`E0588`), while C is perfectly happy to pack such
    /// a member. A `[uN; 0]` field costs no bytes, raises the alignment the
    /// same way, and is not a `repr(align)` type.
    Align {
        /// The field name, `__cinrs_alignN`.
        name: String,
        /// The alignment it carries, in bytes.
        align: u64,
    },
}

/// A `struct` or `union` tag.
#[derive(Clone, Debug)]
pub struct RecordDef {
    /// Whether this is a `struct` or a `union`.
    pub kind: RecordKind,
    /// The C tag, absent for an anonymous `struct { … }`.
    pub tag: Option<String>,
    /// The name of the generated Rust item, already made unique.
    pub rust_name: String,
    /// Whether `rust_name` is still the synthetic name given to an anonymous
    /// tag, and may therefore be replaced by the name of a `typedef` of it.
    pub anonymous: bool,
    /// The members, in declaration order. Empty while the tag is incomplete.
    ///
    /// An unnamed bit-field is *not* here: it declares no member, so nothing
    /// can name it and no initialiser reaches it. It still occupies bits, and
    /// [`RecordDef::rust_fields`] accounts for them.
    pub fields: Vec<Field>,
    /// The fields of the generated Rust item, in order.
    pub rust_fields: Vec<RustField>,
    /// Whether a member list has been seen.
    pub complete: bool,
    /// The layout, computed once the tag is complete.
    pub layout: Option<Layout>,
    /// The alignment `_Alignas` on a member raised the record to, which
    /// becomes `#[repr(C, align(N))]` on the generated item.
    pub align: Option<u64>,
    /// The maximum member alignment `__attribute__((packed))` or
    /// `#pragma pack(N)` asked for, which becomes `#[repr(C, packed(N))]`.
    pub packed: Option<u64>,
    /// The alignment the *generated Rust item* has, which is [`Layout::align`]
    /// except for a packed record — Rust refuses `packed` and `align(N)`
    /// together, so such an item is one byte aligned however strict C says the
    /// record is. Laying out a record that has one as a member reads this
    /// rather than the C alignment, so that the padding it inserts puts the
    /// member where both sides agree it goes.
    pub rust_align: u64,
    /// Whether the record ends in a flexible array member.
    pub flexible: bool,
    /// Whether an item should be generated for this tag.
    pub emit: bool,
    /// Where the tag was defined (or first mentioned).
    pub range: SourceRange,
}

/// One `enum` constant.
#[derive(Clone, Debug)]
pub struct Enumerator {
    /// The name as written in C.
    pub name: String,
    /// The name of the generated Rust `const`, already made unique.
    pub rust_name: String,
    /// The constant's type: `int`, or the fixed underlying type a C23 `enum`
    /// was given.
    pub ty: Ty,
    /// The value.
    pub value: i128,
    /// Where it was written.
    pub range: SourceRange,
}

/// A file-scope `enum` tag, which becomes a named `c_int` alias.
#[derive(Clone, Debug)]
pub struct EnumDef {
    /// Whether the enumeration's underlying type is unsigned, which is what GCC
    /// and Clang pick when no enumerator is negative.
    ///
    /// The choice is implementation defined and only observable through a
    /// bit-field of the type, which is where this is used; everywhere else an
    /// enumeration is `int`, as [`Ty::Enum`] says.
    pub unsigned: bool,
    /// The C tag, if one was written.
    pub tag: Option<String>,
    /// The name of the generated Rust type alias.
    pub rust_name: String,
    /// Whether `rust_name` is still synthetic and may be replaced by a
    /// `typedef` name.
    pub anonymous: bool,
    /// Whether an alias item should be generated.
    pub emit: bool,
    /// Where the tag was defined.
    pub range: SourceRange,
}

/// The arena of derived and tagged types.
///
/// Pointers, arrays and function types are hash-consed, so [`Ty`] equality is
/// C's type compatibility for them; `struct`, `union` and `enum` are nominal
/// and are simply appended.
#[derive(Clone, Debug, Default)]
pub struct Types {
    pointers: Vec<PointerType>,
    arrays: Vec<ArrayType>,
    funcs: Vec<FuncType>,
    records: Vec<RecordDef>,
    enums: Vec<EnumDef>,
    atomics: Vec<Ty>,
    pointer_index: HashMap<PointerType, PointerId>,
    array_index: HashMap<ArrayType, ArrayId>,
    func_index: HashMap<FuncType, FuncTyId>,
    atomic_index: HashMap<Ty, AtomicId>,
}

impl Types {
    /// An empty arena.
    pub fn new() -> Self {
        Self::default()
    }

    /// The type `pointee *`, with `konst` set when the pointee is `const`.
    pub fn pointer(&mut self, pointee: Ty, konst: bool) -> Ty {
        let key = PointerType { pointee, konst };
        if let Some(id) = self.pointer_index.get(&key) {
            return Ty::Pointer(*id);
        }
        let id = PointerId(self.pointers.len() as u32);
        self.pointers.push(key);
        self.pointer_index.insert(key, id);
        Ty::Pointer(id)
    }

    /// The type `_Atomic inner`, for a scalar `inner`.
    ///
    /// Wrapping an atomic type again gives the same type back: C11 6.7.3p5
    /// makes `_Atomic _Atomic int` the same as `_Atomic int`, exactly as a
    /// repeated `const` is.
    pub fn atomic(&mut self, inner: Ty) -> Ty {
        if matches!(inner, Ty::Atomic(_)) {
            return inner;
        }
        if let Some(id) = self.atomic_index.get(&inner) {
            return Ty::Atomic(*id);
        }
        let id = AtomicId(self.atomics.len() as u32);
        self.atomics.push(inner);
        self.atomic_index.insert(inner, id);
        Ty::Atomic(id)
    }

    /// The type inside a [`Ty::Atomic`].
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this arena.
    pub fn atomic_inner(&self, id: AtomicId) -> Ty {
        self.atomics[id.0 as usize]
    }

    /// `ty` with an `_Atomic` taken off it, which is what reading an atomic
    /// lvalue produces (C11 6.3.2.1p2: lvalue conversion drops the
    /// qualifiers).
    pub fn unatomic(&self, ty: Ty) -> Ty {
        match ty {
            Ty::Atomic(id) => self.atomic_inner(id),
            other => other,
        }
    }

    /// Whether `ty` is an `_Atomic` type.
    pub fn is_atomic(&self, ty: Ty) -> bool {
        matches!(ty, Ty::Atomic(_))
    }

    /// The type `elem[len]`.
    pub fn array(&mut self, elem: Ty, len: u64, elem_const: bool) -> Ty {
        self.array_type_of(ArrayType {
            elem,
            len,
            elem_const,
            vla: false,
            vla_len: None,
            incomplete: false,
        })
    }

    /// The type `elem[n]` for a bound that is not a constant: a variable
    /// length array, whose length lives in the object `vla_len` names.
    pub fn vla_array(&mut self, elem: Ty, elem_const: bool, vla_len: Option<ObjectId>) -> Ty {
        self.array_type_of(ArrayType {
            elem,
            len: 0,
            elem_const,
            vla: true,
            vla_len,
            incomplete: false,
        })
    }

    /// The type `elem[]` — an array whose bound was left out (6.2.5p22).
    pub fn incomplete_array(&mut self, elem: Ty, elem_const: bool) -> Ty {
        self.array_type_of(ArrayType {
            elem,
            len: 0,
            elem_const,
            vla: false,
            vla_len: None,
            incomplete: true,
        })
    }

    /// The same type with the elements of every array in it `const`.
    ///
    /// C99 6.7.3p9: "If the specification of an array type includes any type
    /// qualifiers, the element type is so-qualified, not the array type."
    /// Writing the declarator says so by itself — the qualifier in
    /// `const int a[1]` is on `int` — but the `typedef` spelling does not:
    ///
    /// ```c
    /// typedef int A[1];
    /// const A a;      /* `const int[1]`, so `&a` is `const int (*)[1]` */
    /// ```
    ///
    /// and neither does `typeof`. Anything that is not an array is returned
    /// as it stands, because every other type carries `const` on the object
    /// rather than in [`Ty`].
    ///
    /// A multidimensional array is an array *of arrays*, and the element type
    /// the qualifier lands on is the one that is not an array — exactly where
    /// the declarator spelling puts it, so that `const int a[2][3]` and
    /// `typedef int A[2][3]; const A a;` are one type.
    pub fn const_elements(&mut self, ty: Ty) -> Ty {
        let Ty::Array(id) = ty else {
            return ty;
        };
        let array = self.array_type(id);
        if array.elem.is_array() {
            let elem = self.const_elements(array.elem);
            if elem == array.elem {
                return ty;
            }
            return self.array_type_of(ArrayType { elem, ..array });
        }
        if array.elem_const {
            return ty;
        }
        self.array_type_of(ArrayType {
            elem_const: true,
            ..array
        })
    }

    fn array_type_of(&mut self, key: ArrayType) -> Ty {
        if let Some(id) = self.array_index.get(&key) {
            return Ty::Array(*id);
        }
        let id = ArrayId(self.arrays.len() as u32);
        self.arrays.push(key);
        self.array_index.insert(key, id);
        Ty::Array(id)
    }

    /// The type of a function with a prototype.
    pub fn func(&mut self, ret: Ty, params: Vec<Ty>, variadic: bool) -> Ty {
        self.func_type_of(FuncType {
            ret,
            params,
            variadic,
            prototyped: true,
        })
    }

    /// The type `ret ()`: a function whose parameters are unspecified.
    ///
    /// See [`FuncType::prototyped`]. Only the return type varies, so this needs
    /// nothing else.
    pub fn unprototyped_func(&mut self, ret: Ty) -> Ty {
        self.func_type_of(FuncType {
            ret,
            params: Vec::new(),
            variadic: false,
            prototyped: false,
        })
    }

    /// The interned type for a function type description.
    pub fn func_type_of(&mut self, key: FuncType) -> Ty {
        if let Some(id) = self.func_index.get(&key) {
            return Ty::Func(*id);
        }
        let id = FuncTyId(self.funcs.len() as u32);
        self.funcs.push(key.clone());
        self.func_index.insert(key, id);
        Ty::Func(id)
    }

    /// Adds a `struct` or `union` tag.
    pub fn add_record(&mut self, def: RecordDef) -> RecordId {
        let id = RecordId(self.records.len() as u32);
        self.records.push(def);
        id
    }

    /// Adds an `enum` tag.
    pub fn add_enum(&mut self, def: EnumDef) -> EnumId {
        let id = EnumId(self.enums.len() as u32);
        self.enums.push(def);
        id
    }

    /// The pointer type behind a [`Ty::Pointer`].
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this arena.
    pub fn pointer_type(&self, id: PointerId) -> PointerType {
        self.pointers[id.0 as usize]
    }

    /// The array type behind a [`Ty::Array`].
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this arena.
    pub fn array_type(&self, id: ArrayId) -> ArrayType {
        self.arrays[id.0 as usize]
    }

    /// The function type behind a [`Ty::Func`].
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this arena.
    pub fn func_type(&self, id: FuncTyId) -> &FuncType {
        &self.funcs[id.0 as usize]
    }

    /// A tag definition.
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this arena.
    pub fn record(&self, id: RecordId) -> &RecordDef {
        &self.records[id.0 as usize]
    }

    /// A tag definition, mutably.
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this arena.
    pub fn record_mut(&mut self, id: RecordId) -> &mut RecordDef {
        &mut self.records[id.0 as usize]
    }

    /// Every tag, in definition order.
    pub fn records(&self) -> &[RecordDef] {
        &self.records
    }

    /// Generates no item for any tag added since there were `mark` of them.
    ///
    /// What this is for is C23's repeated definition of one tag (N3037): the
    /// second member list has to be *resolved* to be compared with the first,
    /// and everything that resolving it created — the tag itself, and any
    /// anonymous member of it — is then a duplicate of something the first
    /// definition already generated. The type the program sees is the first
    /// one; these are left in the arena, unreferenced and unemitted, because
    /// removing them would move every [`RecordId`] after them.
    pub fn suppress_records_from(&mut self, mark: usize) {
        for def in &mut self.records[mark..] {
            def.emit = false;
        }
    }

    /// An `enum` definition.
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this arena.
    pub fn enum_def(&self, id: EnumId) -> &EnumDef {
        &self.enums[id.0 as usize]
    }

    /// An `enum` definition, mutably.
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this arena.
    pub fn enum_mut(&mut self, id: EnumId) -> &mut EnumDef {
        &mut self.enums[id.0 as usize]
    }

    /// Every `enum` definition, in definition order.
    pub fn enums(&self) -> &[EnumDef] {
        &self.enums
    }

    /// What a pointer points at, if `ty` is a pointer.
    pub fn pointee(&self, ty: Ty) -> Option<Ty> {
        match ty {
            Ty::Pointer(id) => Some(self.pointer_type(id).pointee),
            _ => None,
        }
    }

    /// Whether `ty` is a pointer whose pointee is `const`.
    pub fn points_to_const(&self, ty: Ty) -> bool {
        match ty {
            Ty::Pointer(id) => self.pointer_type(id).konst,
            _ => false,
        }
    }

    /// The element type of an array.
    pub fn elem(&self, ty: Ty) -> Option<Ty> {
        match ty {
            Ty::Array(id) => Some(self.array_type(id).elem),
            _ => None,
        }
    }

    /// Whether `ty` is an array whose own bound is a run-time value.
    ///
    /// `int a[n]` is one and `int a[3][n]` is not — that one is an array *of*
    /// variable length arrays, which C calls variably modified all the same.
    /// [`Types::is_vm`] is the question to ask about the type as a whole.
    pub fn is_vla(&self, ty: Ty) -> bool {
        matches!(ty, Ty::Array(id) if self.array_type(id).vla)
    }

    /// Whether `ty` is *variably modified* (C99 6.7.5.2p4): an array with a
    /// run-time bound anywhere in it.
    ///
    /// A pointer to one is variably modified too by C's definition, but what
    /// the question is asked for here is "does this type have a size only the
    /// running program knows", and a pointer's size is a constant.
    pub fn is_vm(&self, ty: Ty) -> bool {
        match ty {
            Ty::Array(id) => {
                let array = self.array_type(id);
                array.vla || self.is_vm(array.elem)
            }
            _ => false,
        }
    }

    /// The type a pointer into a [variably modified](Types::is_vm) array
    /// addresses: what is left after every dimension with a run-time size is
    /// taken off.
    ///
    /// It is the element type of the hidden `Vec` a [`Stmt::Vla`] allocates
    /// and the pointee of the generated Rust pointer — `double a[n][m]` is a
    /// `*mut c_double` over `n * m` of them, and `double a[n][3]` a
    /// `*mut [c_double; 3]` over `n`. Everything else about a variably
    /// modified type is arithmetic on top of that: see [`Types::vm_dims`].
    pub fn vm_step_ty(&self, ty: Ty) -> Ty {
        match ty {
            Ty::Array(id) if self.is_vm(ty) => self.vm_step_ty(self.array_type(id).elem),
            other => other,
        }
    }

    /// The dimensions between `ty` and its [step type](Types::vm_step_ty),
    /// outermost first.
    ///
    /// Their product is how many step-type elements the type holds, which is
    /// what `sizeof` multiplies by the element size and what pointer
    /// arithmetic on a pointer to `ty` scales by.
    pub fn vm_dims(&self, ty: Ty) -> Vec<VmDim> {
        let mut out = Vec::new();
        let mut ty = ty;
        while self.is_vm(ty) {
            let Ty::Array(id) = ty else { break };
            let array = self.array_type(id);
            out.push(match (array.vla, array.vla_len) {
                (true, Some(len)) => VmDim::Len(len),
                (true, None) => VmDim::Unknown,
                // A fixed dimension outside a variable one counts too: the
                // rows of `int a[3][n]` are `n` ints apart, and there are
                // three of them.
                (false, _) => VmDim::Fixed(array.len),
            });
            ty = array.elem;
        }
        out
    }

    /// Whether `ty` is an array whose bound was left out — `int j[]`.
    pub fn is_incomplete_array(&self, ty: Ty) -> bool {
        matches!(ty, Ty::Array(id) if self.array_type(id).incomplete)
    }

    /// `elem[1]`, for an incomplete array type: what C99 6.9.2p5 completes a
    /// tentative definition with one to at the end of the translation unit.
    pub fn complete_tentative_array(&mut self, ty: Ty) -> Option<Ty> {
        let Ty::Array(id) = ty else { return None };
        let array = self.array_type(id);
        if !array.incomplete {
            return None;
        }
        Some(self.array(array.elem, 1, array.elem_const))
    }

    /// Whether `ty` is a pointer to a function.
    pub fn is_func_pointer(&self, ty: Ty) -> bool {
        matches!(self.pointee(ty), Some(Ty::Func(_)))
    }

    /// Whether `ty` is `void *` (however qualified).
    pub fn is_void_pointer(&self, ty: Ty) -> bool {
        self.pointee(ty) == Some(Ty::Void)
    }

    /// Whether the two pointer types point at the same thing, ignoring `const`.
    pub fn same_pointee(&self, a: Ty, b: Ty) -> bool {
        match (a, b) {
            (Ty::Pointer(a), Ty::Pointer(b)) => {
                self.pointer_type(a).pointee == self.pointer_type(b).pointee
            }
            _ => false,
        }
    }

    /// The type an array or function decays to in a value context.
    pub fn decayed(&mut self, ty: Ty, konst: bool) -> Ty {
        match ty {
            Ty::Array(id) => {
                let array = self.array_type(id);
                self.pointer(array.elem, array.elem_const || konst)
            }
            Ty::Func(_) => self.pointer(ty, false),
            other => other,
        }
    }

    /// Whether the type is complete, i.e. whether `sizeof` applies to it.
    pub fn is_complete(&self, ty: Ty) -> bool {
        match ty {
            Ty::Void | Ty::Func(_) | Ty::Error => false,
            Ty::Record(id) => self.record(id).complete,
            Ty::Array(id) => {
                let array = self.array_type(id);
                !array.incomplete && self.is_complete(array.elem)
            }
            _ => true,
        }
    }

    /// The name of a `const`-qualified member of `ty`, if it has one.
    ///
    /// C11 6.3.2.1p1 makes a structure or union with such a member — "any
    /// member (including, recursively, any member or element of all contained
    /// aggregates or unions)" — something other than a modifiable lvalue, so
    /// the whole object cannot be assigned to even though nothing about the
    /// object itself was declared `const`. WG14 DR131 is that rule, and
    /// `drs/dr1xx.c` is where it is checked.
    ///
    /// The walk terminates: a record cannot contain itself by value.
    pub fn const_member(&self, ty: Ty) -> Option<&str> {
        match ty {
            Ty::Record(id) => self.record(id).fields.iter().find_map(|field| {
                if field.is_const || self.has_const_elements(field.ty) {
                    Some(field.name.as_str())
                } else {
                    self.const_member(field.ty)
                }
            }),
            Ty::Array(id) => self.const_member(self.array_type(id).elem),
            _ => None,
        }
    }

    /// Whether `ty` is an array whose elements are `const`-qualified.
    ///
    /// An array type is never itself qualified — 6.7.3p9 puts the qualifiers
    /// on the elements — so `const int a[3];` as a member is a `const` member
    /// with `is_const` clear.
    fn has_const_elements(&self, ty: Ty) -> bool {
        match ty {
            Ty::Array(id) => {
                let array = self.array_type(id);
                array.elem_const || self.has_const_elements(array.elem)
            }
            _ => false,
        }
    }

    /// The size and alignment of `ty`, or `None` when it is incomplete.
    pub fn size_align(&self, ty: Ty, target: &TargetModel) -> Option<Layout> {
        Some(match ty {
            // `va_list` has a layout, but not one this crate can know: it is
            // whatever the target's ABI made of it.
            Ty::Void | Ty::Func(_) | Ty::Error | Ty::VaList => return None,
            Ty::Pointer(_) => {
                let size = u64::from(target.ptr_bits).div_ceil(8);
                Layout { size, align: size }
            }
            Ty::Array(id) => {
                let array = self.array_type(id);
                let elem = self.size_align(array.elem, target)?;
                // A variable length array has no size the front end can know:
                // `sizeof` of one is a run-time value, computed from the
                // object's own hidden length. Answering `None` here is what
                // keeps a path that forgot about that loud rather than silently
                // wrong.
                // An incomplete array has no size either, and `sizeof` of one
                // is a constraint violation until a later declaration in the
                // same unit completes it.
                if array.vla || array.incomplete {
                    return None;
                }
                Layout {
                    size: elem.size.saturating_mul(array.len),
                    align: elem.align,
                }
            }
            Ty::Record(id) => self.record(id).layout?,
            // C11 6.2.5p27 lets an atomic type have a different size and
            // alignment from its underlying one, and every implementation
            // makes the alignment at least the size: `_Atomic long long` is
            // eight-byte aligned on i386, where a plain `long long` is
            // four-byte aligned, because that is what a lock-free 64-bit
            // instruction needs. Rust's `AtomicU64` says the same thing.
            Ty::Atomic(id) => {
                let inner = self.size_align(self.atomic_inner(id), target)?;
                Layout {
                    size: inner.size,
                    align: inner.size.max(inner.align).max(1),
                }
            }
            Ty::Enum(_) => {
                let size = u64::from(target.int_bits).div_ceil(8);
                Layout { size, align: size }
            }
            // A complex type is two of its component type laid out side by
            // side (C99 6.2.5p13), so it is twice as big and no more strictly
            // aligned — which is what `#[repr(C)] struct Complex<T>` gives
            // and what every ABI this crate targets says.
            Ty::ComplexFloat | Ty::ComplexDouble => {
                let component = ty.complex_component().size_bytes(target);
                Layout {
                    size: component * 2,
                    align: component.min(target.max_scalar_align).max(1),
                }
            }
            // The one scalar whose alignment is not its size on every target;
            // see [`TargetModel::int128_align`].
            Ty::Int128 | Ty::UInt128 => Layout {
                size: 16,
                align: target.int128_align,
            },
            // A vector type is aligned to its own width — sixteen bytes for a
            // `__m128i`, thirty-two for a `__m256i` — which is both what the
            // x86 ABIs say and what `core::arch`'s types have. The scalar rule
            // below would clamp it to [`TargetModel::max_scalar_align`], which
            // is eight, and a `_mm_load_si128` from an object aligned to eight
            // faults.
            Ty::Vector(vec) => Layout {
                size: vec.bytes(),
                align: vec.bytes(),
            },
            scalar => {
                let size = scalar.size_bytes(target);
                // A scalar is aligned to its own width, up to whatever the ABI
                // stops at: the i386 System V ABI aligns `long long` and
                // `double` to four bytes rather than eight, and `rustc` gives
                // `u64` and `f64` the same alignment there. See
                // [`TargetModel::max_scalar_align`].
                Layout {
                    size,
                    align: size.min(target.max_scalar_align).max(1),
                }
            }
        })
    }

    /// `sizeof ty`, or `None` when it is incomplete.
    pub fn size_of(&self, ty: Ty, target: &TargetModel) -> Option<u64> {
        self.size_align(ty, target).map(|l| l.size)
    }

    /// The C spelling of a type, as it should appear in a diagnostic.
    pub fn name(&self, ty: Ty) -> String {
        match ty {
            Ty::Pointer(id) => {
                let p = self.pointer_type(id);
                if let Ty::Func(f) = p.pointee {
                    return self.func_name(f, "(*)");
                }
                let prefix = if p.konst { "const " } else { "" };
                format!("{prefix}{} *", self.name(p.pointee))
            }
            Ty::Array(id) => {
                let a = self.array_type(id);
                let prefix = if a.elem_const { "const " } else { "" };
                if a.vla {
                    // C's own spelling for an array whose bound is not a
                    // constant expression; the bound belongs to the object, so
                    // there is nothing else honest to print.
                    return format!("{prefix}{}[*]", self.name(a.elem));
                }
                if a.incomplete {
                    return format!("{prefix}{}[]", self.name(a.elem));
                }
                format!("{prefix}{}[{}]", self.name(a.elem), a.len)
            }
            Ty::Func(id) => self.func_name(id, ""),
            Ty::Record(id) => {
                let record = self.record(id);
                match &record.tag {
                    Some(tag) => format!("{} {tag}", record.kind.as_str()),
                    None => format!("{} {}", record.kind.as_str(), record.rust_name),
                }
            }
            Ty::Enum(id) => {
                let def = self.enum_def(id);
                match &def.tag {
                    Some(tag) => format!("enum {tag}"),
                    None => format!("enum {}", def.rust_name),
                }
            }
            Ty::Atomic(id) => format!("_Atomic({})", self.name(self.atomic_inner(id))),
            Ty::Error => "<error>".to_owned(),
            scalar => scalar.scalar_name().to_owned(),
        }
    }

    fn func_name(&self, id: FuncTyId, middle: &str) -> String {
        let f = self.func_type(id);
        let mut params: Vec<String> = f.params.iter().map(|p| self.name(*p)).collect();
        if f.variadic {
            params.push("...".to_owned());
        }
        // `int ()` and `int (void)` are two types, and a diagnostic that says
        // which one it means is the whole point of the distinction.
        if params.is_empty() && f.prototyped {
            params.push("void".to_owned());
        }
        format!("{} {middle}({})", self.name(f.ret), params.join(", "))
    }
}

impl Ty {
    /// The C spelling of a scalar type.
    ///
    /// Derived and tagged types need the arena; use [`Types::name`] for a type
    /// that may be one of those.
    pub fn scalar_name(self) -> &'static str {
        match self {
            Ty::Void => "void",
            Ty::Bool => "_Bool",
            Ty::Char => "char",
            Ty::SChar => "signed char",
            Ty::UChar => "unsigned char",
            Ty::Short => "short",
            Ty::UShort => "unsigned short",
            Ty::Int => "int",
            Ty::UInt => "unsigned int",
            Ty::Long => "long",
            Ty::ULong => "unsigned long",
            Ty::LongLong => "long long",
            Ty::ULongLong => "unsigned long long",
            Ty::Int128 => "__int128",
            Ty::UInt128 => "unsigned __int128",
            Ty::Float => "float",
            Ty::Double => "double",
            Ty::ComplexFloat => "float _Complex",
            Ty::ComplexDouble => "double _Complex",
            Ty::Pointer(_) => "pointer",
            Ty::Array(_) => "array",
            Ty::Func(_) => "function",
            Ty::Record(_) => "struct",
            Ty::Enum(_) => "enum",
            Ty::VaList => "va_list",
            Ty::Vector(vec) => vec.name(),
            Ty::Atomic(_) => "_Atomic",
            Ty::Error => "<error>",
        }
    }

    /// Whether this is `void`.
    pub fn is_void(self) -> bool {
        self == Ty::Void
    }

    /// Whether this is `_Bool`.
    pub fn is_bool(self) -> bool {
        self == Ty::Bool
    }

    /// Whether this is a pointer.
    pub fn is_pointer(self) -> bool {
        matches!(self, Ty::Pointer(_))
    }

    /// Whether this is an array.
    pub fn is_array(self) -> bool {
        matches!(self, Ty::Array(_))
    }

    /// Whether this is a `struct` or `union`.
    pub fn is_record(self) -> bool {
        matches!(self, Ty::Record(_))
    }

    /// Whether this is a function type.
    pub fn is_func(self) -> bool {
        matches!(self, Ty::Func(_))
    }

    /// Whether this is a named `enum` type.
    pub fn is_enum(self) -> bool {
        matches!(self, Ty::Enum(_))
    }

    /// Whether this is `va_list`.
    pub fn is_va_list(self) -> bool {
        self == Ty::VaList
    }

    /// Whether this is one of x86's vector types.
    ///
    /// They are neither arithmetic nor scalar — nothing C does to a number can
    /// be done to one — so every operator asks this before complaining, and
    /// says "use an intrinsic" rather than the generic "invalid operands".
    pub fn is_vector(self) -> bool {
        matches!(self, Ty::Vector(_))
    }

    /// Whether this stands for something already reported as ill formed.
    pub fn is_error(self) -> bool {
        self == Ty::Error
    }

    /// Whether this is an integer type (`_Bool` and `enum` included, as C
    /// requires).
    pub fn is_integer(self) -> bool {
        matches!(
            self,
            Ty::Bool
                | Ty::Char
                | Ty::SChar
                | Ty::UChar
                | Ty::Short
                | Ty::UShort
                | Ty::Int
                | Ty::UInt
                | Ty::Long
                | Ty::ULong
                | Ty::LongLong
                | Ty::ULongLong
                | Ty::Int128
                | Ty::UInt128
                | Ty::Enum(_)
        )
    }

    /// Whether this is one of the two 128-bit integer types.
    ///
    /// They are the only integers whose values do not all fit in the `i128` a
    /// constant is carried in, so the places that fold, print or emit one have
    /// to know; see [`Ty::wrap`].
    pub fn is_int128(self) -> bool {
        matches!(self, Ty::Int128 | Ty::UInt128)
    }

    /// Whether this is `float` or `double` — one of C's *real* floating types.
    ///
    /// The complex types are floating types too as far as the standard's
    /// wording goes; [`Ty::is_complex`] is the question about those, and
    /// keeping them out of this one is what stops every existing floating-point
    /// path from silently treating a `Complex<f64>` as an `f64`.
    pub fn is_floating(self) -> bool {
        matches!(self, Ty::Float | Ty::Double)
    }

    /// Whether this is one of the complex types.
    pub fn is_complex(self) -> bool {
        matches!(self, Ty::ComplexFloat | Ty::ComplexDouble)
    }

    /// The *corresponding real type* (C99 6.2.5p14) — the type of each part of
    /// a complex value, and the type itself for everything else.
    pub fn complex_component(self) -> Ty {
        match self {
            Ty::ComplexFloat => Ty::Float,
            Ty::ComplexDouble => Ty::Double,
            other => other,
        }
    }

    /// The complex type whose parts have this real type (C99 6.2.5p13).
    ///
    /// Anything that is not a real floating type gets `double _Complex`, which
    /// is what the usual arithmetic conversions give an integer operand.
    pub fn complex_of(self) -> Ty {
        match self {
            Ty::Float => Ty::ComplexFloat,
            Ty::ComplexFloat => Ty::ComplexFloat,
            _ => Ty::ComplexDouble,
        }
    }

    /// Whether this is an arithmetic type: an integer, a real floating type, or
    /// a complex one (C99 6.2.5p18).
    pub fn is_arithmetic(self) -> bool {
        self.is_integer() || self.is_floating() || self.is_complex()
    }

    /// Whether this is a scalar, i.e. something C can compare against zero.
    ///
    /// An [`Ty::Atomic`] is *not* one: it is the type of an object, and a
    /// value read out of one has the underlying type. Everything that asks
    /// this question about a declared type therefore has to take the
    /// `_Atomic` off first, with [`Types::unatomic`].
    pub fn is_scalar(self) -> bool {
        self.is_arithmetic() || self.is_pointer()
    }

    /// Whether this is `_Atomic T`.
    pub fn is_atomic(self) -> bool {
        matches!(self, Ty::Atomic(_))
    }

    /// Whether values of this type are signed.
    pub fn is_signed(self, target: &TargetModel) -> bool {
        match self {
            Ty::Char => target.char_signed,
            Ty::SChar | Ty::Short | Ty::Int | Ty::Long | Ty::LongLong | Ty::Enum(_) => true,
            Ty::Int128 => true,
            Ty::Float | Ty::Double | Ty::ComplexFloat | Ty::ComplexDouble => true,
            _ => false,
        }
    }

    /// The width of this type in bits.
    pub fn bits(self, target: &TargetModel) -> u32 {
        match self {
            Ty::Void => 0,
            Ty::Bool => 1,
            Ty::Char | Ty::SChar | Ty::UChar => 8,
            Ty::Short | Ty::UShort => target.short_bits,
            Ty::Int | Ty::UInt | Ty::Enum(_) => target.int_bits,
            Ty::Long | Ty::ULong => target.long_bits,
            Ty::LongLong | Ty::ULongLong => target.long_long_bits,
            // Not a knob: GCC's `__int128` is 128 bits wherever it exists.
            Ty::Int128 | Ty::UInt128 => 128,
            Ty::Float => 32,
            Ty::Double => 64,
            Ty::ComplexFloat => 64,
            Ty::ComplexDouble => 128,
            Ty::Pointer(_) => target.ptr_bits,
            // Not a number of *value* bits — a vector type has no value this
            // crate reasons about — but the width the object occupies, which
            // is what `sizeof` asks for through [`Ty::size_bytes`].
            Ty::Vector(vec) => (vec.bytes() * 8) as u32,
            // An atomic type's width is its underlying one's, which needs the
            // arena; nothing asks this about one, because every value has
            // already had the `_Atomic` taken off it.
            Ty::Array(_) | Ty::Func(_) | Ty::Record(_) | Ty::VaList | Ty::Atomic(_) | Ty::Error => {
                0
            }
        }
    }

    /// `sizeof` this scalar type, in bytes.
    ///
    /// Aggregates need the arena; use [`Types::size_of`] for a type that may be
    /// one.
    pub fn size_bytes(self, target: &TargetModel) -> u64 {
        match self {
            Ty::Void => 1, // GCC's extension; C says this is an error.
            Ty::Bool => 1,
            _ => u64::from(self.bits(target)).div_ceil(8),
        }
    }

    /// The conversion rank of an integer type (C99 6.3.1.1).
    ///
    /// Only the ordering matters; the absolute values are arbitrary.
    pub fn rank(self) -> u32 {
        match self {
            Ty::Bool => 1,
            Ty::Char | Ty::SChar | Ty::UChar => 2,
            Ty::Short | Ty::UShort => 3,
            Ty::Int | Ty::UInt | Ty::Enum(_) => 4,
            Ty::Long | Ty::ULong => 5,
            Ty::LongLong | Ty::ULongLong => 6,
            // GCC ranks `__int128` above every standard integer type, which is
            // what makes `(__int128)x * y` compute in 128 bits.
            Ty::Int128 | Ty::UInt128 => 7,
            Ty::Float => 8,
            Ty::Double => 9,
            // The complex types have no *conversion* rank of their own: C
            // ranks their corresponding real types and makes the result
            // complex, which is what `usual_arithmetic` does before this is
            // ever consulted.
            _ => 0,
        }
    }

    /// The unsigned type of the same rank.
    pub fn to_unsigned(self) -> Ty {
        match self {
            Ty::Char | Ty::SChar => Ty::UChar,
            Ty::Short => Ty::UShort,
            Ty::Int | Ty::Enum(_) => Ty::UInt,
            Ty::Long => Ty::ULong,
            Ty::LongLong => Ty::ULongLong,
            Ty::Int128 => Ty::UInt128,
            other => other,
        }
    }

    /// The smallest value this integer type can hold.
    pub fn min_value(self, target: &TargetModel) -> i128 {
        if !self.is_signed(target) {
            return 0;
        }
        let bits = self.bits(target);
        if bits >= 128 {
            return i128::MIN;
        }
        -(1i128 << (bits - 1))
    }

    /// The largest value this integer type can hold.
    ///
    /// `unsigned __int128` is the one type whose largest value does not fit in
    /// the `i128` this returns, and it is clamped to [`i128::MAX`]. Nothing
    /// reads it: the only comparison of two maxima is the last step of the
    /// [usual arithmetic conversions](Ty::usual_arithmetic), which is reached
    /// only when the *unsigned* operand has the lower rank — and no integer
    /// type ranks above `unsigned __int128`.
    pub fn max_value(self, target: &TargetModel) -> i128 {
        if self == Ty::Bool {
            return 1;
        }
        let bits = self.bits(target);
        if bits >= 128 {
            return i128::MAX;
        }
        if self.is_signed(target) {
            (1i128 << (bits - 1)) - 1
        } else {
            (1i128 << bits) - 1
        }
    }

    /// Whether `value` fits in this integer type without conversion.
    pub fn can_represent(self, value: i128, target: &TargetModel) -> bool {
        value >= self.min_value(target) && value <= self.max_value(target)
    }

    /// Converts an integer value to this type the way C's conversions do:
    /// modulo 2^N for unsigned types, and the same (implementation-defined)
    /// wrap-around for signed ones.
    ///
    /// # How a 128-bit constant is carried
    ///
    /// A folded constant is an `i128`, which holds every value of every type
    /// this models except those of `unsigned __int128` above `i128::MAX`. Such
    /// a value is carried as its **two's-complement bit pattern**, which is
    /// what this returns unchanged for a 128-bit type: for every narrower type
    /// the bit pattern and the mathematical value coincide, so the invariant is
    /// "the value, except that an `unsigned __int128` is reinterpreted". The
    /// places where the difference shows — division, remainder, a right shift,
    /// a comparison and the literal that is finally emitted — dispatch on
    /// [`Ty::is_signed`] instead of on the sign of the `i128`.
    pub fn wrap(self, value: i128, target: &TargetModel) -> i128 {
        if self == Ty::Bool {
            return i128::from(value != 0);
        }
        let bits = self.bits(target);
        if bits == 0 || bits >= 128 {
            return value;
        }
        let masked = (value as u128) & (u128::MAX >> (128 - bits));
        if self.is_signed(target) && masked >> (bits - 1) != 0 {
            (masked | (u128::MAX << bits)) as i128
        } else {
            masked as i128
        }
    }

    /// The integer promotions (C99 6.3.1.1p2).
    ///
    /// Anything of lower rank than `int` becomes `int` when `int` can hold
    /// every one of its values and `unsigned int` otherwise; an `enum` becomes
    /// `int`; everything else is unchanged.
    pub fn promote(self, target: &TargetModel) -> Ty {
        if self.is_enum() {
            return Ty::Int;
        }
        if !self.is_integer() || self.rank() >= Ty::Int.rank() {
            return self;
        }
        if Ty::Int.can_represent(self.min_value(target), target)
            && Ty::Int.can_represent(self.max_value(target), target)
        {
            Ty::Int
        } else {
            Ty::UInt
        }
    }

    /// The integer promotions applied to a bit-field (C99 6.3.1.1p2, "as
    /// restricted by the width").
    ///
    /// The value of a bit-field of width `width` ranges over `width` bits
    /// rather than over the whole declared type, so `unsigned x : 31` promotes
    /// to `int` — every value fits — while `unsigned x : 32` promotes to
    /// `unsigned int`. The standard only defines the promotions for a type
    /// whose rank is at most `int`'s, which is the only case standard C allows
    /// a bit-field to have; GCC and Clang apply the same width-restricted rule
    /// to the wider types they accept as an extension, so `unsigned long x : 31`
    /// is an `int` too and `unsigned long x : 33` keeps its declared type. This
    /// follows them.
    ///
    /// `signed` is the signedness of the *field*, which is the declared type's
    /// except for an `enum` whose underlying type the implementation made
    /// unsigned.
    pub fn promote_bit_field(self, width: u32, signed: bool, target: &TargetModel) -> Ty {
        if !self.is_integer() || width == 0 || width > 127 {
            return self.promote(target);
        }
        let (min, max) = if signed {
            (-(1i128 << (width - 1)), (1i128 << (width - 1)) - 1)
        } else {
            (0, (1i128 << width) - 1)
        };
        for candidate in [Ty::Int, Ty::UInt] {
            if candidate.can_represent(min, target) && candidate.can_represent(max, target) {
                return candidate;
            }
        }
        self
    }

    /// The default argument promotions, applied to the variable part of a
    /// variadic call: `float` becomes `double`, and the integer promotions do
    /// the rest.
    pub fn promote_argument(self, target: &TargetModel) -> Ty {
        if self == Ty::Float {
            return Ty::Double;
        }
        self.promote(target)
    }

    /// The usual arithmetic conversions (C99 6.3.1.8): the common type two
    /// arithmetic operands are converted to.
    ///
    /// With a complex operand the standard's rule is in two steps: the *common
    /// real type* is worked out from the two operands' corresponding real
    /// types, and the result is the complex type belonging to it if either
    /// operand was complex. `float _Complex + long` is therefore
    /// `float _Complex`, not `double _Complex`.
    pub fn usual_arithmetic(lhs: Ty, rhs: Ty, target: &TargetModel) -> Ty {
        if lhs.is_complex() || rhs.is_complex() {
            let real =
                Ty::usual_arithmetic(lhs.complex_component(), rhs.complex_component(), target);
            return real.complex_of();
        }
        if lhs == Ty::Double || rhs == Ty::Double {
            return Ty::Double;
        }
        if lhs == Ty::Float || rhs == Ty::Float {
            return Ty::Float;
        }
        let lhs = lhs.promote(target);
        let rhs = rhs.promote(target);
        if lhs == rhs {
            return lhs;
        }
        let lhs_signed = lhs.is_signed(target);
        if lhs_signed == rhs.is_signed(target) {
            return if lhs.rank() >= rhs.rank() { lhs } else { rhs };
        }
        let (unsigned, signed) = if lhs_signed { (rhs, lhs) } else { (lhs, rhs) };
        if unsigned.rank() >= signed.rank() {
            unsigned
        } else if signed.max_value(target) >= unsigned.max_value(target) {
            signed
        } else {
            signed.to_unsigned()
        }
    }

    /// `size_t` for `target`.
    ///
    /// The *narrowest* unsigned type as wide as a pointer, which is how GCC
    /// picks it and therefore what `__SIZE_TYPE__` says: `unsigned int` on
    /// i686, `unsigned long` on LP64, `unsigned long long` on 64-bit Windows,
    /// where `long` is only 32 bits.
    pub fn size_ty(target: &TargetModel) -> Ty {
        if target.int_bits >= target.ptr_bits {
            Ty::UInt
        } else if target.long_bits >= target.ptr_bits {
            Ty::ULong
        } else {
            Ty::ULongLong
        }
    }

    /// `ptrdiff_t` for `target`, chosen the same way as [`Ty::size_ty`].
    pub fn ptrdiff_ty(target: &TargetModel) -> Ty {
        if target.int_bits >= target.ptr_bits {
            Ty::Int
        } else if target.long_bits >= target.ptr_bits {
            Ty::Long
        } else {
            Ty::LongLong
        }
    }

    /// `wchar_t` for `target`: `unsigned short` on Windows, `unsigned int` on
    /// Arm outside Apple's platforms, and `int` everywhere else.
    ///
    /// This is the type `L'x'` and `L"…"` get, and what the bundled
    /// `<stddef.h>` typedefs from `__WCHAR_TYPE__`; the two have to agree, or
    /// a call passing `L"…"` to a `const wchar_t *` would be a type error.
    pub fn wchar_ty(target: &TargetModel) -> Ty {
        match (target.wchar_bits, target.wchar_signed) {
            (16, true) => Ty::Short,
            (16, false) => Ty::UShort,
            (_, true) => Ty::Int,
            (_, false) => Ty::UInt,
        }
    }

    /// `char16_t` (C11 7.28), which is `uint_least16_t` — `unsigned short` on
    /// every target this models, and what the bundled `<uchar.h>` typedefs it
    /// to.
    pub fn char16_ty() -> Ty {
        Ty::UShort
    }

    /// `char32_t` (C11 7.28), which is `uint_least32_t` — `unsigned int`.
    pub fn char32_ty() -> Ty {
        Ty::UInt
    }
}

// ---------------------------------------------------------------------------
// identifiers
// ---------------------------------------------------------------------------

/// Identifies a named object (a local, a parameter, a `static` local or a
/// file-scope variable) inside a [`Program`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ObjectId(pub u32);

/// Identifies a function inside a [`Program`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FuncId(pub u32);

/// Identifies one loop inside a function; used to build its Rust label.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct LoopId(pub u32);

/// Identifies one `switch` inside a function; used to build its Rust label.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SwitchId(pub u32);

/// Identifies one `goto` label inside a function.
///
/// C gives labels function scope and their own namespace, so every label of a
/// function is collected before its body is checked — that is what lets a
/// `goto` jump forwards.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct LabelId(pub u32);

/// Identifies a string literal inside a [`Program`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct StrId(pub u32);

// ---------------------------------------------------------------------------
// objects and functions
// ---------------------------------------------------------------------------

/// Where an object lives, and how it is generated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Storage {
    /// A `let` binding: a local variable or a parameter.
    Automatic,
    /// A `static mut` item: a file-scope variable or a function-local `static`.
    Static {
        /// The name of the generated Rust item, already made unique.
        item_name: String,
        /// Whether the item is `pub` — true for a file-scope object without
        /// `static`, which C gives external linkage and which Rust code should
        /// therefore be able to reach.
        exported: bool,
    },
    /// A `thread_local!` item: an object declared `_Thread_local`,
    /// `thread_local` or `__thread`.
    ///
    /// C gives it static storage duration and one instance per thread, which
    /// is exactly what `std::thread_local!` provides. The item holds an
    /// `UnsafeCell<T>`, and every access goes through the `*mut T` its `with`
    /// hands out — valid for as long as the current thread's copy is, which is
    /// the lifetime C promises. It is the third construct whose expansion
    /// needs more than `core`, after variable length arrays and `alloca`.
    ThreadLocal {
        /// The name of the generated Rust item, already made unique.
        item_name: String,
        /// Whether the item is `pub`; see [`Storage::Static`].
        exported: bool,
    },
    /// An object defined outside the translation unit, declared in the
    /// expansion's `extern` block.
    Extern {
        /// The name the symbol has.
        item_name: String,
    },
}

impl Storage {
    /// The name of the generated item, for the two storage classes that have
    /// one of their own.
    pub fn item_name(&self) -> Option<&str> {
        match self {
            Storage::Static { item_name, .. } | Storage::ThreadLocal { item_name, .. } => {
                Some(item_name)
            }
            Storage::Automatic | Storage::Extern { .. } => None,
        }
    }

    /// Whether this is a thread-local object.
    pub fn is_thread_local(&self) -> bool {
        matches!(self, Storage::ThreadLocal { .. })
    }
}

/// A named object.
#[derive(Clone, Debug)]
pub struct Object {
    /// The name as written in C.
    pub name: String,
    /// The object's type.
    pub ty: Ty,
    /// How the object is stored.
    pub storage: Storage,
    /// Whether the object's type is `const`-qualified.
    pub is_const: bool,
    /// Whether the declaration said `register`.
    ///
    /// The specifier is a hint about speed that this crate has nothing to do
    /// with — Rust decides where a local lives — but it has one rule with
    /// teeth: C11 6.7.1p6 says the address of such an object "cannot be
    /// computed, either explicitly (by use of the unary `&` operator as
    /// discussed in 6.5.3.2) or implicitly (by converting an array name to a
    /// pointer as discussed in 6.3.2.1)", so `sizeof` is the only operator an
    /// array declared `register` can be the operand of. WG14 DR116 is that
    /// rule; `drs/dr1xx.c` is where it is checked.
    pub is_register: bool,
    /// Set when this is the hidden `Vec` a [variable length array](Stmt::Vla)
    /// keeps its elements in, in which case [`Object::ty`] is the *element*
    /// type and the generated binding has type `Vec<T>`.
    ///
    /// It is not an object of the C program at all; it exists so that the
    /// storage is dropped when the block ends, which is the lifetime C gives
    /// the array.
    pub vla_storage: bool,
    /// The alignment `_Alignas(N)` or `__attribute__((aligned(N)))` asked for,
    /// when it is stricter than the one the type already has.
    ///
    /// Rust has no way to over-align a binding, so the object is generated
    /// inside a one-field wrapper that carries the alignment —
    /// `#[repr(C, align(N))] struct __cinrs_align_N<T>(pub T);` — and every
    /// access to it goes through the field. The C object's *type* is unchanged:
    /// `sizeof` is the type's size and the wrapper is invisible to everything
    /// but the generated binding. See [`codegen`](crate::codegen).
    ///
    /// `None` is the ordinary case, and also what a request no stricter than
    /// the natural alignment leaves behind — there is nothing for a wrapper to
    /// say.
    pub align: Option<u64>,
    /// How many elements the object's storage gives the record's [flexible
    /// array member](Field::flexible), when an initialiser filled it in.
    ///
    /// GNU C lets an object with *static* storage duration initialise the
    /// member (`static struct W w = { 3, { 1, 2, 3 } };`), which makes the
    /// object larger than its own type — something Rust has no way to say
    /// about a value of type `W`. The item is therefore given a *companion*
    /// type with the same leading layout and a tail of this length,
    /// `__cinrs_W_3`, and every use of the object is a place reached through
    /// `(*(&raw mut w).cast::<W>())`. `sizeof w` is still `sizeof(struct W)`,
    /// which is what GCC says too. See [`codegen`](crate::codegen).
    pub flexible_len: Option<u64>,
    /// The symbol `__asm__("name")` renamed the object to.
    pub asm_label: Option<String>,
    /// The section `__attribute__((section("…")))` asked for.
    pub section: Option<String>,
    /// Where the declarator was written.
    pub range: SourceRange,
}

/// A `static mut` item and its constant initialiser.
#[derive(Clone, Debug)]
pub struct StaticVar {
    /// The object the item defines.
    pub object: ObjectId,
    /// The initial value; C zero-initialises objects with static storage
    /// duration, so this is present even when the source wrote no initialiser.
    pub init: Expr,
}

/// A function's signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// The return type.
    pub ret: Ty,
    /// The parameter types.
    pub params: Vec<Ty>,
    /// Whether the prototype ended with `, ...`.
    pub variadic: bool,
    /// Whether a parameter type list was given at all; see
    /// [`FuncType::prototyped`].
    ///
    /// `int f();` before C23 declares a function whose parameters are
    /// unspecified: `params` is empty because nothing was said, not because
    /// there are none. A *definition* written that way does take no parameters
    /// — that is what the generated item has — but its type still has no
    /// prototype, so a call with arguments is legal C and reaches the callee
    /// with the default argument promotions applied.
    pub prototyped: bool,
}

/// How a function's body is lowered.
///
/// Nearly every C function maps onto Rust's own control flow, which is what
/// makes the expansion readable. A function that jumps around — a `goto`, or a
/// `case` label the enclosing `switch` cannot reach without one — cannot, and
/// is lowered into a [control-flow graph](crate::cfg) instead.
#[derive(Clone, Debug)]
pub enum Body {
    /// Rust control flow mirrors C's.
    Structured(Vec<Stmt>),
    /// A state machine over basic blocks; see [`crate::cfg`].
    Cfg(crate::cfg::Cfg),
}

/// A function declared or defined in the translation unit.
#[derive(Clone, Debug)]
pub struct Function {
    /// The name as written in C.
    pub name: String,
    /// The signature every declaration of it must agree on.
    pub sig: Signature,
    /// The parameter objects. Empty until the definition is seen.
    pub params: Vec<ObjectId>,
    /// The parameter names as first declared, for the generated `extern` block.
    pub param_names: Vec<Option<String>>,
    /// Whether the function was declared `static`, i.e. is private to the unit.
    pub is_static: bool,
    /// Whether the function was declared `inline`.
    pub is_inline: bool,
    /// Whether the function was declared `_Noreturn` (or `[[noreturn]]`), so
    /// that a call to it ends the statement it is in.
    pub noreturn: bool,
    /// What `always_inline` / `noinline` asked for.
    pub inline_hint: Option<InlineHint>,
    /// Whether `__attribute__((cold))` marked it unlikely.
    pub cold: bool,
    /// The message `__attribute__((deprecated))` gave, if it was there at all.
    pub deprecated: Option<Option<String>>,
    /// The section `__attribute__((section("…")))` asked for.
    pub section: Option<String>,
    /// The symbol `__asm__("name")` renamed the function to.
    pub asm_label: Option<String>,
    /// Whether `__attribute__((constructor))` asked for it to run before
    /// `main`, or `destructor` for after it.
    pub init_kind: Option<InitKind>,
    /// The instruction sets `__attribute__((target("…")))` or
    /// `#pragma GCC target("…")` asked for, in Rust's spelling, which the
    /// generated item carries as `#[target_feature(enable = "…")]`.
    ///
    /// Only a *definition* can carry them: an `extern` declaration has no item
    /// for the attribute to go on, and the function it names was compiled
    /// somewhere else.
    pub target_features: Vec<String>,
    /// Set when this name is an [x86 intrinsic](crate::x86) rather than a
    /// symbol: a call to it is generated as `::core::arch::x86_64::<name>`,
    /// and it is left out of the unit's `extern` block because there is
    /// nothing to link.
    pub intrinsic: Option<&'static crate::x86::Intrinsic>,
    /// Where the function was asked to be [safe](crate::sema::check_safe), if
    /// it was: `[[cinrs::safe]]`, `__attribute__((cinrs_safe))` or
    /// `#pragma cinrs safe`.
    ///
    /// A safe function is generated as `extern "C" fn` rather than
    /// `unsafe extern "C" fn`, and its body is *not* wrapped in an `unsafe`
    /// block, so `rustc` checks it. The range is where the request was
    /// written, which is what the diagnostics about it point at.
    pub safe: Option<SourceRange>,
    /// Whether the body calls `alloca`, in which case the generated item opens
    /// with the arena the emulation allocates out of.
    ///
    /// `alloca`'s memory lives until the function returns — not until the end
    /// of the block it was called in — so the arena is per function and is
    /// dropped by the `return`, which is exactly that lifetime.
    pub uses_alloca: bool,
    /// Every automatic object the body declared, in declaration order.
    ///
    /// Code generation needs the whole list — not only the ones a `let`
    /// statement is visible for — because a local declared inside a statement
    /// expression is a binding too and may need renaming apart.
    pub locals: Vec<ObjectId>,
    /// The body, present once a definition has been type checked.
    pub body: Option<Body>,
    /// The Rust item name, when it is not the C name.
    ///
    /// Only a lifted [GNU nested function](EnvParam) has one: it becomes a
    /// file-scope item, so it needs a name that cannot collide with the C
    /// function of the same name at file scope, while [`Function::name`] stays
    /// what the program called it — which is what diagnostics and `__func__`
    /// say.
    pub item_name: Option<String>,
    /// The hidden environment parameters a lifted GNU nested function takes in
    /// front of its declared ones, in the order they are passed.
    ///
    /// Empty for every ordinary function, and for a nested one that captures
    /// nothing — which is why such a nested function's address may still be
    /// taken: the generated item has exactly the signature C gave it.
    pub env: Vec<EnvParam>,
    /// Where the function's name was written, at its definition if there is one
    /// and at its first declaration otherwise.
    pub range: SourceRange,
}

/// One hidden parameter of a lifted [GNU nested
/// function](crate::sema#nested-functions).
///
/// GCC gives a nested function a *static chain* — a pointer to the enclosing
/// frame — and writes a trampoline when its address is taken. This crate
/// lambda-lifts instead: each enclosing object the body uses becomes a
/// pointer parameter of its own, the body reads and writes it through that
/// pointer, and every call site passes the address of the object it has. The
/// sharing C promises is therefore kept — a store in the nested function is
/// visible in the enclosing one — without a trampoline, at the price of not
/// being able to hand the function's address out.
#[derive(Clone, Copy, Debug)]
pub struct EnvParam {
    /// The enclosing function's object the pointer carries.
    pub owner: ObjectId,
    /// The `*mut T` (or `*const T`) parameter of *this* function that holds
    /// its address.
    pub param: ObjectId,
}

/// What `always_inline` and `noinline` ask for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InlineHint {
    /// `#[inline(always)]`
    Always,
    /// `#[inline(never)]`
    Never,
}

/// Whether a function runs before `main` or after it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitKind {
    /// `__attribute__((constructor))`
    Constructor,
    /// `__attribute__((destructor))`
    Destructor,
}

impl Function {
    /// Whether this function is only declared here and linked from elsewhere.
    pub fn is_extern(&self) -> bool {
        self.body.is_none()
    }

    /// The name the generated Rust item has.
    pub fn item_name(&self) -> &str {
        self.item_name.as_deref().unwrap_or(&self.name)
    }

    /// Whether this is a lifted GNU nested function.
    pub fn is_nested(&self) -> bool {
        self.item_name.is_some()
    }

    /// Whether the function is generated without `unsafe`; see
    /// [`Function::safe`].
    pub fn is_safe(&self) -> bool {
        self.safe.is_some()
    }
}

/// One direct call, from the function whose body holds it to the function it
/// names.
///
/// Sema records these as it checks the calls, and
/// [`check_safe`](crate::sema::check_safe) is the one thing that reads them: a
/// [safe](Function::safe) function calling one that is not gets a diagnostic
/// worded in C rather than `rustc`'s "call to unsafe function". A call through
/// a *pointer* has no edge — there is no callee to name — and is left to
/// `rustc`, which refuses it in a safe function like any other unsafe
/// operation.
#[derive(Clone, Copy, Debug)]
pub struct CallEdge {
    /// The function the call was written in.
    pub caller: FuncId,
    /// The function it calls.
    pub callee: FuncId,
    /// Where the callee was named, which is where a diagnostic points.
    pub range: SourceRange,
}

/// A file-scope `typedef`, which becomes a Rust type alias.
#[derive(Clone, Debug)]
pub struct TypedefItem {
    /// The name of the generated alias.
    pub rust_name: String,
    /// What it stands for.
    pub ty: Ty,
    /// Where it was written.
    pub range: SourceRange,
}

/// A string literal's decoded contents.
#[derive(Clone, Debug)]
pub struct StrData {
    /// The elements, without the terminating NUL: bytes for a narrow or
    /// `u8"…"` literal, UTF-16 code units for `u"…"`, and character values for
    /// `U"…"` and `L"…"`.
    pub values: Vec<u32>,
    /// The type of one element: `char`, `char8_t`, `char16_t`, `char32_t` or
    /// `wchar_t`, whichever prefix the literal was written with.
    pub elem: Ty,
}

impl StrData {
    /// The number of elements including the terminating NUL.
    pub fn len_with_nul(&self) -> u64 {
        self.values.len() as u64 + 1
    }
}

/// Everything one translation unit generates.
#[derive(Clone, Debug, Default)]
pub struct Program {
    /// A hash of the invocation site, used to build the synthetic item names
    /// that must not collide between two `c99!` blocks in one module.
    pub unit_id: u64,
    /// Every derived and tagged type.
    pub types: Types,
    /// Every named object, indexed by [`ObjectId`].
    pub objects: Vec<Object>,
    /// The objects that become `static mut` items, in declaration order.
    pub statics: Vec<StaticVar>,
    /// The objects declared `extern`, in declaration order.
    pub externs: Vec<ObjectId>,
    /// Every function, indexed by [`FuncId`], in declaration order.
    pub functions: Vec<Function>,
    /// Every direct call the unit's bodies make; see [`CallEdge`].
    pub calls: Vec<CallEdge>,
    /// The file-scope `typedef`s, in declaration order.
    pub typedefs: Vec<TypedefItem>,
    /// The `enum` constants that become Rust `const` items, in order.
    pub enum_constants: Vec<Enumerator>,
    /// Every string literal, indexed by [`StrId`].
    pub strings: Vec<StrData>,
    /// The libraries the unit must be linked against, named by
    /// `#pragma cinrs link "…"`.
    ///
    /// Filled in after semantic analysis: it is the preprocessor that reads
    /// the pragma, and nothing about the program itself depends on it.
    pub link_libraries: Vec<String>,
    /// Whether `#pragma cinrs export` asked for every function and object with
    /// external linkage to become a real C symbol.
    ///
    /// Filled in after semantic analysis, for the same reason as
    /// [`Program::link_libraries`].
    pub export: bool,
    /// Every `__builtin_cpu_supports` the unit wrote, by where it was written.
    ///
    /// It becomes `::std::is_x86_feature_detected!`, and `core` has no CPU
    /// detection at all — so a unit that said `#pragma cinrs no_std` cannot
    /// have one. The pragma is only known after semantic analysis, which is
    /// why the sites are collected rather than checked where they are met; see
    /// [`crate::sema::check_pragmas`].
    pub cpu_supports: Vec<SourceRange>,
    /// Whether `#pragma cinrs no_std` said the expansion goes into a
    /// `#![no_std]` crate.
    ///
    /// Everything generated is `core`-only except the storage a [variable
    /// length array](Stmt::Vla) and `alloca` need, which is a `Vec`; this
    /// decides whether that `Vec` is spelled `::std::vec::Vec` or
    /// `::alloc::vec::Vec`. Filled in after semantic analysis, for the same
    /// reason as [`Program::link_libraries`].
    pub no_std: bool,
    /// The Rust path of the `cinrs` facade crate, which the generated code
    /// names when it needs the runtime: `::cinrs` unless
    /// `#pragma cinrs crate "…"` said otherwise.
    ///
    /// Only a unit that uses a complex type spells it at all. Filled in after
    /// semantic analysis, for the same reason as [`Program::link_libraries`].
    pub crate_path: String,
}

/// The Rust path of the facade crate the generated code names, when the unit
/// does not say.
///
/// A crate renamed in `Cargo.toml` — `cinrs = { package = "cinrs", … }` under
/// another name — is reached with `#pragma cinrs crate "<path>"` instead.
pub const DEFAULT_CRATE_PATH: &str = "::cinrs";

impl Program {
    /// Looks an object up.
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this program.
    pub fn object(&self, id: ObjectId) -> &Object {
        &self.objects[id.0 as usize]
    }

    /// Looks a function up.
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this program.
    pub fn function(&self, id: FuncId) -> &Function {
        &self.functions[id.0 as usize]
    }

    /// Looks a string literal up.
    ///
    /// # Panics
    ///
    /// Panics if `id` did not come from this program.
    pub fn string(&self, id: StrId) -> &StrData {
        &self.strings[id.0 as usize]
    }

    /// The hidden Rust name an externally linked **object** is declared under.
    ///
    /// Only an object. A function the unit merely declares is generated under
    /// its own C name, like everything else the unit spells, so that
    /// `#include <zlib.h>` is enough for Rust to call `crc32`; an object is
    /// not, and the difference is Rust's rule for patterns rather than a
    /// matter of taste.
    ///
    /// A glob-imported **function** cannot change the meaning of Rust code
    /// that does not mention it: a function is not a pattern, so `let read =
    /// 1;` beside a unit that declares `read` is still a new binding. A glob
    /// imported **static** can. Rust resolves a binding pattern against the
    /// value namespace first, and a `static` there is not a name a `let` may
    /// shadow:
    ///
    /// ```text
    /// error[E0530]: let bindings cannot shadow statics
    /// ```
    ///
    /// — which is what `let stdout = std::io::stdout();` would become next to a
    /// block that includes `<stdio.h>`, and `let timezone = …` next to one that
    /// includes glibc's `<time.h>`. `optarg`, `optind` and `environ` are the
    /// same story. So a declared-only object keeps a name of its own,
    /// `__cinrs_<unit>_<symbol>`, and Rust reaches it the way C code does in
    /// the same situation: through an accessor written in the block,
    /// `FILE *get_stdout(void) { return stdout; }`.
    ///
    /// The C code in the unit is unaffected either way — it refers to the
    /// object by its C name, and this is only the Rust side of that.
    ///
    /// A module of its own for the `extern` block would have avoided the
    /// question altogether, but a module cannot see the `struct` items of the
    /// block it is written in, which an `extern` declaration taking a `struct`
    /// needs.
    ///
    /// A `$` in the C name — an identifier character here, and one Rust has no
    /// spelling for — is written [`crate::codegen::DOLLAR`]; the symbol the
    /// declaration links by is a `#[link_name]` string and keeps the `$`.
    pub fn extern_object_name(&self, symbol: &str) -> String {
        format!(
            "__cinrs_{:08x}_{}",
            self.unit_id as u32,
            symbol.replace('$', crate::codegen::DOLLAR)
        )
    }

    /// Whether anything at all has to go into the `extern` block.
    ///
    /// An [x86 intrinsic](crate::x86) does not count: it is declared like any
    /// other function and generated as a call to `core::arch`, so a unit whose
    /// only declarations came from `<immintrin.h>` needs no block at all.
    pub fn has_externs(&self) -> bool {
        !self.externs.is_empty()
            || self
                .functions
                .iter()
                .any(|func| func.is_extern() && func.intrinsic.is_none())
    }
}

// ---------------------------------------------------------------------------
// constants
// ---------------------------------------------------------------------------

/// The value of an arithmetic constant expression.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ConstValue {
    /// An integer value, already reduced to the range of its type.
    Int(i128),
    /// A floating value.
    Float(f64),
    /// A complex value: the real part and the imaginary one, each already
    /// rounded to the component type.
    ///
    /// Both parts are carried as `f64` whatever the type is, exactly as a
    /// `float` constant is; the rounding to `f32` happens where the value is
    /// stored, so that `(float _Complex) 0.1` is the same number here and in
    /// the generated code.
    Complex(f64, f64),
}

// ---------------------------------------------------------------------------
// expressions
// ---------------------------------------------------------------------------

/// A binary arithmetic, bitwise or shift operator.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(missing_docs)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitXor,
    BitOr,
    Shl,
    Shr,
}

impl BinOp {
    /// The C spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::BitAnd => "&",
            BinOp::BitXor => "^",
            BinOp::BitOr => "|",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
        }
    }

    /// Whether this operator shifts, in which case its operands are promoted
    /// separately rather than converted to a common type.
    pub fn is_shift(self) -> bool {
        matches!(self, BinOp::Shl | BinOp::Shr)
    }
}

/// A relational or equality operator.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(missing_docs)]
pub enum CmpOp {
    Lt,
    Gt,
    Le,
    Ge,
    Eq,
    Ne,
}

/// `&&` or `||`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogicalOp {
    /// `&&`
    And,
    /// `||`
    Or,
}

/// A GNU builtin that becomes a fixed piece of Rust rather than a call.
///
/// The bit-manipulation ones map onto the integer methods of the same name;
/// the overflow ones do the arithmetic in `i128` and check the result against
/// the range of the type it is stored in, which is exactly the "compute in
/// infinite precision, then convert" the builtins are defined by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuiltinOp {
    /// `__builtin_popcount…`
    Popcount,
    /// `__builtin_clz…`; undefined for zero in C, and this follows Rust.
    Clz,
    /// `__builtin_ctz…`
    Ctz,
    /// `__builtin_ffs…`: one more than the index of the lowest set bit, or 0.
    Ffs,
    /// `__builtin_parity…`
    Parity,
    /// `__builtin_clrsb…`: leading redundant sign bits.
    Clrsb,
    /// `__builtin_bswap16/32/64`
    Bswap,
    /// `__builtin_{add,sub,mul}_overflow(a, b, &r)`, whose value is the flag.
    Overflow(BinOp),
    /// The `_p` forms, which only ask whether it *would* overflow.
    OverflowP(BinOp),
    /// Evaluate the operands and produce nothing: `__builtin_prefetch` and
    /// `__builtin_assume`, which promise something the generated code cannot
    /// pass on.
    Discard,
    /// `__builtin_alloca(size)`, whose one operand is the size in bytes,
    /// converted to `size_t`.
    ///
    /// The memory comes out of the arena [`Function::uses_alloca`] puts at the
    /// top of the function, so it is freed by the `return` — which is
    /// `alloca`'s own lifetime.
    Alloca,
    /// `__builtin_fabs…`: the sign bit cleared, which is what C's `fabs` is
    /// defined as and what makes it exact for a NaN and for a zero.
    Fabs,
    /// `__builtin_copysign…`: the first operand's magnitude with the second
    /// operand's sign bit.
    Copysign,
    /// One of the quiet comparison macros' builtins, whose value is an `int`.
    FloatOrder(FloatOrder),
    /// One of the classification builtins, whose value is an `int`.
    FloatClass(FloatClass),
    /// `__builtin_fpclassify(nan, inf, normal, subnormal, zero, x)`: the one
    /// of the first five operands the sixth one's class selects.
    Fpclassify,
    /// `__builtin_cpu_supports("avx2")`: whether the processor running the
    /// program has that instruction set, as an `int`.
    ///
    /// It becomes `::std::is_x86_feature_detected!("avx2")`, which is the same
    /// question asked of the same `cpuid` leaves. The payload is the row of
    /// [`crate::x86::TARGET_FEATURES`] the instruction set is in, so that
    /// [`BuiltinOp`] stays two bytes wide — it is a field of [`ExprKind`], and
    /// every expression in the program pays for whatever the largest variant
    /// is. Several Rust features for one GCC name (`abm` is LZCNT and POPCNT)
    /// are `&&`ed together; [`crate::x86::detect_features`] is the lookup.
    CpuSupports(u8),
    /// `__builtin_cproj(z)`: C99 7.3.9.5's projection onto the Riemann sphere.
    ///
    /// Everything is itself except a value with an infinite part, which becomes
    /// `+∞` with the imaginary part's sign kept on a zero — so every infinity
    /// is the *one* point at infinity.
    ComplexProj,
}

/// The `float` bit pattern of a NaN that travels through the IR as a `double`.
///
/// A NaN's payload and its sign are part of its value — `__builtin_nanf
/// ("0x123")` asks for one in particular — and [`ExprKind::Float`] carries
/// every floating constant as an `f64`, so a `float` NaN is carried as the
/// `double` whose sign, quiet bit and payload are the same. Widening with `as`
/// would not do: it may quiet a signalling NaN and is free to choose the
/// payload. This and [`widen_nan_bits`] are exact inverses.
pub fn narrow_nan_bits(bits: u64) -> u32 {
    let sign = ((bits >> 63) as u32) << 31;
    let payload = ((bits >> 29) & 0x7f_ffff) as u32;
    sign | 0x7f80_0000 | payload
}

/// The `double` bit pattern a `float` NaN is carried as; see
/// [`narrow_nan_bits`].
pub fn widen_nan_bits(bits: u32) -> u64 {
    let sign = u64::from(bits >> 31) << 63;
    let payload = u64::from(bits & 0x7f_ffff) << 29;
    sign | 0x7ff0_0000_0000_0000 | payload
}

/// A quiet floating-point comparison: `__builtin_isgreater` and its relatives.
///
/// "Quiet" is the whole point of them — C99 7.12.14 defines each as the
/// comparison it names *without* raising the invalid exception on a NaN, which
/// `<` and friends would. Rust's floating comparison operators are the quiet
/// ones, so each is written out as the operator it stands for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FloatOrder {
    /// `__builtin_isgreater`
    Greater,
    /// `__builtin_isgreaterequal`
    GreaterEqual,
    /// `__builtin_isless`
    Less,
    /// `__builtin_islessequal`
    LessEqual,
    /// `__builtin_islessgreater`: `x < y || x > y`, which is `x != y` without
    /// the NaN case.
    LessGreater,
    /// `__builtin_isunordered`: either operand is a NaN.
    Unordered,
}

/// A floating-point classification: `__builtin_isnan` and its relatives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FloatClass {
    /// `__builtin_isnan…`
    IsNan,
    /// `__builtin_isinf…`
    IsInf,
    /// `__builtin_isinf_sign`, whose value is -1, 0 or 1.
    IsInfSign,
    /// `__builtin_isfinite`
    IsFinite,
    /// `__builtin_isnormal`
    IsNormal,
    /// `__builtin_issignaling`: a NaN whose quiet bit is clear.
    IsSignaling,
    /// `__builtin_signbit…`, which is 1 for a negative zero too.
    SignBit,
}

// ---------------------------------------------------------------------------
// atomics
// ---------------------------------------------------------------------------

/// A memory order, C11 7.17.3's `memory_order` and Rust's `Ordering`.
///
/// `memory_order_consume` is not here: no compiler implements dependency
/// ordering and Rust has no `Consume`, so it arrives as [`MemOrder::Acquire`]
/// — which is what GCC and Clang also emit for it, and what C11 7.17.3p1
/// allows an implementation to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemOrder {
    /// `memory_order_relaxed`
    Relaxed,
    /// `memory_order_acquire` (and `memory_order_consume`).
    Acquire,
    /// `memory_order_release`
    Release,
    /// `memory_order_acq_rel`
    AcqRel,
    /// `memory_order_seq_cst`
    SeqCst,
}

impl MemOrder {
    /// The name of the `core::sync::atomic::Ordering` variant.
    pub fn rust_name(self) -> &'static str {
        match self {
            MemOrder::Relaxed => "Relaxed",
            MemOrder::Acquire => "Acquire",
            MemOrder::Release => "Release",
            MemOrder::AcqRel => "AcqRel",
            MemOrder::SeqCst => "SeqCst",
        }
    }

    /// The C spelling, for a diagnostic.
    pub fn c_name(self) -> &'static str {
        match self {
            MemOrder::Relaxed => "memory_order_relaxed",
            MemOrder::Acquire => "memory_order_acquire",
            MemOrder::Release => "memory_order_release",
            MemOrder::AcqRel => "memory_order_acq_rel",
            MemOrder::SeqCst => "memory_order_seq_cst",
        }
    }

    /// Whether a load may be performed with this order (C11 7.17.7.2p3).
    pub fn valid_for_load(self) -> bool {
        !matches!(self, MemOrder::Release | MemOrder::AcqRel)
    }

    /// Whether a store may be performed with this order (C11 7.17.7.1p2).
    pub fn valid_for_store(self) -> bool {
        !matches!(self, MemOrder::Acquire | MemOrder::AcqRel)
    }

    /// How strong the order is, for the rule that a compare-exchange's failure
    /// order may not be stronger than its success order.
    pub fn strength(self) -> u8 {
        match self {
            MemOrder::Relaxed => 0,
            MemOrder::Acquire | MemOrder::Release => 1,
            MemOrder::AcqRel => 2,
            MemOrder::SeqCst => 3,
        }
    }

    /// The strongest order a failed compare-exchange may use beside this one,
    /// which is what `fetch_update` and a nand loop are given.
    pub fn failure_order(self) -> MemOrder {
        match self {
            MemOrder::Release => MemOrder::Relaxed,
            MemOrder::AcqRel => MemOrder::Acquire,
            other => other,
        }
    }
}

/// What kind of Rust atomic an object is reached through.
///
/// The width comes from the C type's size, so `long` is an `AtomicI64` on an
/// LP64 target and an `AtomicI32` on an ILP32 one; the
/// [data-model check](crate::codegen) is what makes that assumption safe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtomicClass {
    /// `_Bool`, which is `AtomicBool` — a Rust `bool` may only ever hold 0 or
    /// 1, so it may not be reached through an integer atomic.
    Bool,
    /// An integer (or `enum`) of `bytes` bytes: `AtomicI8` … `AtomicU64`.
    Int {
        /// The width in bytes: 1, 2, 4 or 8.
        bytes: u64,
        /// Whether the C type is signed.
        signed: bool,
    },
    /// `float` or `double`, reached through the integer atomic of the same
    /// width and `to_bits`/`from_bits`.
    Float {
        /// The width in bytes: 4 or 8.
        bytes: u64,
    },
    /// An object pointer, which is `AtomicPtr`.
    Ptr,
    /// A *function* pointer, which is also an `AtomicPtr` — but whose C value
    /// is an `Option<unsafe extern "C" fn(…)>` in Rust rather than a raw
    /// pointer, so the two ends of every operation are a `transmute` instead
    /// of a cast.
    ///
    /// That transmute is sound because an `Option<fn>` is pointer-sized with
    /// the null pointer as its `None` niche, which is exactly the
    /// representation C gives a function pointer that may be null. Arithmetic
    /// is not: C has none on a function pointer, and neither has this.
    FnPtr,
}

impl AtomicClass {
    /// The name of the `core::sync::atomic` type.
    pub fn rust_name(self) -> &'static str {
        match self {
            AtomicClass::Bool => "AtomicBool",
            AtomicClass::Ptr | AtomicClass::FnPtr => "AtomicPtr",
            AtomicClass::Float { bytes } => match bytes {
                4 => "AtomicU32",
                _ => "AtomicU64",
            },
            AtomicClass::Int { bytes, signed } => match (bytes, signed) {
                (1, true) => "AtomicI8",
                (1, false) => "AtomicU8",
                (2, true) => "AtomicI16",
                (2, false) => "AtomicU16",
                (4, true) => "AtomicI32",
                (4, false) => "AtomicU32",
                (_, true) => "AtomicI64",
                (_, false) => "AtomicU64",
            },
        }
    }

    /// The Rust primitive the atomic holds, which is what the pointer handed
    /// to `from_ptr` points at.
    pub fn repr_name(self) -> &'static str {
        match self {
            AtomicClass::Bool => "bool",
            AtomicClass::Ptr | AtomicClass::FnPtr => "",
            AtomicClass::Float { bytes } => match bytes {
                4 => "u32",
                _ => "u64",
            },
            AtomicClass::Int { bytes, signed } => match (bytes, signed) {
                (1, true) => "i8",
                (1, false) => "u8",
                (2, true) => "i16",
                (2, false) => "u16",
                (4, true) => "i32",
                (4, false) => "u32",
                (_, true) => "i64",
                (_, false) => "u64",
            },
        }
    }
}

/// The operation an [`ExprKind::Atomic`] performs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtomicOp {
    /// An atomic load, whose value has the object's type.
    Load,
    /// An atomic store, whose value is `void`.
    Store,
    /// An atomic exchange, whose value is the old one.
    Exchange,
    /// A compare-and-exchange whose value is `_Bool`, writing the value it
    /// observed back through the `expected` pointer when it fails — C11's
    /// `atomic_compare_exchange_strong` and GCC's
    /// `__atomic_compare_exchange_n`.
    CompareExchange {
        /// Whether a spurious failure is allowed (`compare_exchange_weak`).
        weak: bool,
    },
    /// The older `__sync_bool_compare_and_swap` and
    /// `__sync_val_compare_and_swap`, whose expected value is a *value* rather
    /// than a pointer and which write nothing back.
    SyncCompareSwap {
        /// Whether the value of the expression is the old one rather than
        /// whether the swap happened.
        value_is_old: bool,
    },
    /// A read-modify-write, whose value is the old one for `fetch_op` and the
    /// new one for `op_fetch`.
    Rmw {
        /// The operation.
        op: AtomicRmw,
        /// Whether the value of the expression is the *new* one.
        returns_new: bool,
    },
    /// `__atomic_test_and_set`: an atomic exchange of "set" into a byte, whose
    /// value is what was there before, as a `_Bool`.
    TestAndSet,
    /// `__atomic_clear`: an atomic store of zero into a byte.
    Clear,
    /// A fence, which has no operand at all.
    Fence {
        /// Whether this is a `signal_fence`, which is a compiler fence only.
        signal: bool,
    },
}

/// The arithmetic a [`AtomicOp::Rmw`] performs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtomicRmw {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `&`
    And,
    /// `|`
    Or,
    /// `^`
    Xor,
    /// `~(a & b)`, which Rust has no `fetch_nand` for on the integers and
    /// which is therefore a `fetch_update`.
    Nand,
}

impl AtomicRmw {
    /// The `core::sync::atomic` method that performs it, where there is one.
    pub fn rust_method(self) -> Option<&'static str> {
        Some(match self {
            AtomicRmw::Add => "fetch_add",
            AtomicRmw::Sub => "fetch_sub",
            AtomicRmw::And => "fetch_and",
            AtomicRmw::Or => "fetch_or",
            AtomicRmw::Xor => "fetch_xor",
            AtomicRmw::Nand => return None,
        })
    }

    /// The C operator, for a diagnostic.
    pub fn c_op(self) -> &'static str {
        match self {
            AtomicRmw::Add => "+",
            AtomicRmw::Sub => "-",
            AtomicRmw::And => "&",
            AtomicRmw::Or => "|",
            AtomicRmw::Xor => "^",
            AtomicRmw::Nand => "~&",
        }
    }
}

/// One of the `__atomic_*`, `__sync_*` or `__c11_atomic_*` builtins, resolved.
///
/// The pointer is the object the operation is performed on; sema has already
/// checked that what it points at is one of the types
/// [`AtomicClass`] covers, and has resolved the memory orders — which C
/// requires to be integer constant expressions here, exactly as `<stdatomic.h>`
/// writes them.
#[derive(Clone, Debug)]
pub struct AtomicExpr {
    /// What to do.
    pub op: AtomicOp,
    /// What kind of atomic to do it through.
    pub class: AtomicClass,
    /// The C type of the object, with any `_Atomic` already taken off: the
    /// type of the value the operation produces or stores.
    pub value_ty: Ty,
    /// The object's address, absent only for a fence.
    pub ptr: Option<Expr>,
    /// The value operand — what is stored, exchanged or added.
    pub value: Option<Expr>,
    /// The expected value of a compare-and-exchange: a *pointer* to it for
    /// [`AtomicOp::CompareExchange`], which writes the observed value back
    /// through it, and the value itself for [`AtomicOp::SyncCompareSwap`].
    pub expected: Option<Expr>,
    /// The order of the operation, and of a successful compare-and-exchange.
    pub success: MemOrder,
    /// The order of a *failed* compare-and-exchange.
    pub failure: MemOrder,
}

impl AtomicExpr {
    /// Every expression the node holds, in evaluation order.
    pub fn operands(&self) -> impl Iterator<Item = &Expr> {
        self.ptr
            .iter()
            .chain(self.expected.iter())
            .chain(self.value.iter())
    }
}

/// Which Rust atomic a C object type is reached through, if any is.
///
/// The width comes from the target model, which the [data-model
/// check](crate::codegen) makes safe to rely on. `None` means there is no
/// atomic of that width or shape: a 128-bit integer, an aggregate.
pub fn atomic_class(types: &Types, ty: Ty, target: &TargetModel) -> Option<AtomicClass> {
    let ty = types.unatomic(ty);
    if ty == Ty::Bool {
        return Some(AtomicClass::Bool);
    }
    if ty.is_integer() {
        let bytes = ty.size_bytes(target);
        return matches!(bytes, 1 | 2 | 4 | 8).then_some(AtomicClass::Int {
            bytes,
            signed: ty.is_signed(target),
        });
    }
    if ty.is_floating() {
        let bytes = ty.size_bytes(target);
        return matches!(bytes, 4 | 8).then_some(AtomicClass::Float { bytes });
    }
    if ty.is_pointer() {
        return Some(if types.is_func_pointer(ty) {
            AtomicClass::FnPtr
        } else {
            AtomicClass::Ptr
        });
    }
    None
}

/// An assignable location.
#[derive(Clone, Debug)]
pub struct Place {
    /// What is being addressed.
    pub kind: PlaceKind,
    /// The type of the object addressed.
    pub ty: Ty,
    /// Whether the object is `const`-qualified, and therefore not assignable.
    pub is_const: bool,
    /// Where it was written.
    pub range: SourceRange,
}

/// The shape of a [`Place`].
#[derive(Clone, Debug)]
pub enum PlaceKind {
    /// A named object.
    Object(ObjectId),
    /// `*ptr`, where `ptr` has pointer type.
    Deref(Box<Expr>),
    /// `base[index]`, where `base` has pointer type: the same thing as
    /// `*(base + index)`, which is what C says it is.
    Index {
        /// The pointer the subscript is relative to.
        base: Box<Expr>,
        /// The subscript, of integer type.
        index: Box<Expr>,
    },
    /// `base.field`, where `field` indexes the record's member list. `p->f`
    /// arrives as a `Field` over a `Deref`.
    Field {
        /// The record the member belongs to.
        base: Box<Place>,
        /// The record's identity.
        record: RecordId,
        /// The index of the member in the record's field list.
        index: usize,
    },
    /// `__real__ z` or `__imag__ z`: one part of a complex object, which GNU C
    /// makes an lvalue whenever `z` is one, so `__imag__ z = 1.0;` assigns.
    ///
    /// The type of the place is the [corresponding real
    /// type](Ty::complex_component); `base` is the complex object, which may be
    /// a [`PlaceKind::Temporary`] when the operand was an rvalue.
    ComplexPart {
        /// The complex object the part belongs to.
        base: Box<Place>,
        /// Whether this is `__imag__` rather than `__real__`.
        imag: bool,
    },
    /// A string literal, whose type is an array of `char` (or of `wchar_t`).
    Str(StrId),
    /// A temporary holding the value of an expression, which is what makes
    /// `f().field` work for a `struct` returned by value.
    Temporary(Box<Expr>),
    /// The object a block-scope compound literal (`(T){ … }`, C99 6.5.2.5)
    /// denotes.
    ///
    /// Unlike a [`PlaceKind::Temporary`] it is a real object with automatic
    /// storage duration and the lifetime of the *enclosing block*, so its
    /// address may be taken and used for the rest of that block. `object` is a
    /// hidden local sema declares at the top of that block, zero-initialised;
    /// `init` is the value the literal was written with, and is evaluated
    /// *here* — where the literal stands — so that C's evaluation order
    /// survives and a literal inside a loop is built afresh on every
    /// iteration. A compound literal at *file* scope is an ordinary
    /// [`Storage::Static`] object instead and arrives as a
    /// [`PlaceKind::Object`].
    CompoundLiteral {
        /// The hidden local the object lives in.
        object: ObjectId,
        /// The value stored into it where the literal was written.
        init: Box<Expr>,
    },
}

/// A typed expression.
#[derive(Clone, Debug)]
pub struct Expr {
    /// What the expression computes.
    pub kind: ExprKind,
    /// The type of its value.
    pub ty: Ty,
    /// The number of bits the value is reduced to, when that is narrower than
    /// [`Expr::ty`].
    ///
    /// A bit-field wider than `int` keeps its declared type through the
    /// integer promotions (6.3.1.1p2 has nothing to say about it), but its
    /// *value* still ranges over the declared width only, and C99 6.7.2.1p10
    /// makes that width the type the arithmetic happens in: `unsigned long
    /// long b : 40` multiplies, adds and shifts in forty bits, exactly as an
    /// `unsigned int` does in thirty-two. Nothing else in the type model can
    /// say that, so the width rides along on the expression and code
    /// generation reduces the result to it.
    pub bits: Option<u32>,
    /// Where it was written.
    pub range: SourceRange,
}

impl Expr {
    /// Builds an expression.
    pub fn new(kind: ExprKind, ty: Ty, range: SourceRange) -> Self {
        Self {
            kind,
            ty,
            bits: None,
            range,
        }
    }

    /// The same expression, computed in `bits` bits; see [`Expr::bits`].
    pub fn narrowed(mut self, bits: Option<u32>) -> Self {
        self.bits = bits;
        self
    }

    /// An integer constant of type `ty`.
    pub fn int(value: i128, ty: Ty, range: SourceRange) -> Self {
        Self::new(ExprKind::Int(value), ty, range)
    }
}

/// The class of one *eightbyte* of a `struct` or `union` read out of an
/// argument list, under the x86-64 System V classification (AMD64 psABI
/// 3.2.3).
///
/// [`crate::sema`] computes it and code generation turns each entry into one
/// `next_arg` call. There is deliberately no `SseUp`: the reference
/// implementation this mirrors — `rustc_target`'s
/// `compiler/rustc_target/src/callconv/x86_64.rs` — needs that class for SIMD
/// vectors and for the floating types wider than eight bytes, and a C program
/// this crate translates has neither, `long double` being mapped to `double`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Eightbyte {
    /// An integer register: read as a `u64`.
    Int,
    /// An SSE register: read as an `f64`, whose bits are the eightbyte.
    Sse,
    /// Nothing of the object reaches this eightbyte — it is padding, which the
    /// ABI passes in no register at all, so nothing is read for it.
    None,
}

/// Who is being called.
#[derive(Clone, Debug)]
pub enum Callee {
    /// A named function.
    Direct(FuncId),
    /// An expression of function-pointer type.
    Indirect(Box<Expr>),
}

/// The shape of an [`Expr`].
#[derive(Clone, Debug)]
pub enum ExprKind {
    /// An integer constant, already reduced to the range of its type.
    Int(i128),
    /// A floating constant.
    Float(f64),
    /// A complex value built from its two parts, which have the
    /// [corresponding real type](Ty::complex_component).
    ///
    /// It is what `__builtin_complex(x, y)` — and therefore `CMPLX` — makes,
    /// what an imaginary constant such as `2.0i` is, and what a folded complex
    /// constant comes back as.
    ComplexOf {
        /// The real part.
        re: Box<Expr>,
        /// The imaginary part.
        im: Box<Expr>,
    },
    /// The all-bits-zero value of the expression's type: `0`, `0.0`, `false`,
    /// a null pointer, or a zeroed aggregate.
    Zeroed,
    /// Reading a place.
    Load(Place),
    /// The address of a place. The expression's type says what pointer type is
    /// wanted, which is what turns an array place into a pointer to its first
    /// element.
    AddrOf(Place),
    /// The address of a function, whose type is a pointer to it.
    FuncAddr(FuncId),
    /// GNU's `&&label`: the address of a label of the enclosing function, of
    /// type `void *`.
    ///
    /// A function that takes one is lowered through a [control-flow
    /// graph](crate::cfg), and the value is the *state number* the label's
    /// block was given, cast to a pointer — which is what makes
    /// `goto *e` a store to the state variable. It is an *address constant*,
    /// so a `static void *table[] = { &&a, &&b };` holds a table of them.
    LabelAddr(LabelId),
    /// `place = value`, whose value is the value stored.
    Assign {
        /// The assigned-to location.
        place: Place,
        /// The value, already converted to the place's type.
        value: Box<Expr>,
    },
    /// `place op= value`, whose value is the value stored.
    ///
    /// For a pointer place `compute` is the pointer type and `value` keeps its
    /// integer type: `p += n` is pointer arithmetic, not an addition.
    CompoundAssign {
        /// The assigned-to location, evaluated exactly once.
        place: Place,
        /// The operator.
        op: BinOp,
        /// The right operand, already converted for `compute`.
        value: Box<Expr>,
        /// The type the operation is carried out in, before the result is
        /// converted back to the place's type.
        compute: Ty,
    },
    /// `++place`, `place++`, `--place` or `place--`.
    IncDec {
        /// The affected location.
        place: Place,
        /// Whether this decrements.
        dec: bool,
        /// Whether the value is the one from before the update.
        postfix: bool,
    },
    /// Arithmetic negation, on an already promoted operand.
    Neg(Box<Expr>),
    /// `~x`, on an already promoted operand.
    BitNot(Box<Expr>),
    /// A binary operation. Both operands already have the result type, except
    /// for shifts, whose operands are promoted separately.
    Binary {
        /// The operator.
        op: BinOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// `ptr + index` or `ptr - index`: pointer arithmetic in units of the
    /// pointee, which is what `<*mut T>::offset` does.
    PtrOffset {
        /// The pointer, which is also the type of the result.
        ptr: Box<Expr>,
        /// The offset, of integer type.
        index: Box<Expr>,
        /// Whether the offset is subtracted.
        sub: bool,
    },
    /// `lhs - rhs` between two pointers, whose value has type `ptrdiff_t`.
    PtrDiff {
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// A comparison. Both operands already have a common type; the result has
    /// type `int` and is 0 or 1.
    Compare {
        /// The operator.
        op: CmpOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// `&&` or `||`: each operand is tested against zero, the right one only if
    /// the left does not already decide the result. The result has type `int`
    /// and is 0 or 1.
    Logical {
        /// The operator.
        op: LogicalOp,
        /// Left operand.
        lhs: Box<Expr>,
        /// Right operand.
        rhs: Box<Expr>,
    },
    /// A conversion to the expression's own type.
    Cast(Box<Expr>),
    /// `cond ? then_expr : else_expr`, with both arms already converted to the
    /// expression's type.
    Cond {
        /// The controlling expression.
        cond: Box<Expr>,
        /// The value when the condition is true.
        then_expr: Box<Expr>,
        /// The value otherwise.
        else_expr: Box<Expr>,
    },
    /// GNU's `a ?: b`: `a` if it is non-zero and `b` otherwise, with `a`
    /// evaluated exactly once. Both operands already have the result type.
    CondDefault {
        /// The value that is both the condition and the first result.
        value: Box<Expr>,
        /// The value when it is zero.
        else_expr: Box<Expr>,
    },
    /// GNU's statement expression, `({ …; e; })`.
    StmtExpr {
        /// The statements, in order.
        stmts: Vec<Stmt>,
        /// The value of the last expression statement, if there was one.
        value: Option<Box<Expr>>,
    },
    /// A builtin lowered to a fixed piece of Rust; see [`BuiltinOp`].
    Builtin {
        /// Which builtin.
        op: BuiltinOp,
        /// Its operands, already converted.
        args: Vec<Expr>,
    },
    /// One of the atomic builtins; see [`AtomicExpr`].
    ///
    /// Boxed because it is much the largest thing an expression can hold and
    /// every other node would grow to its size.
    Atomic(Box<AtomicExpr>),
    /// `lhs, rhs`: `lhs` is evaluated for its side effects only.
    Comma {
        /// Evaluated and discarded.
        lhs: Box<Expr>,
        /// The result.
        rhs: Box<Expr>,
    },
    /// A call, with every argument already converted to its parameter's type
    /// (or promoted, for the variable part of a variadic call).
    Call {
        /// What is called.
        callee: Callee,
        /// The arguments.
        args: Vec<Expr>,
    },
    /// A `struct` value: one expression per member, in declaration order.
    RecordLit {
        /// The record's identity.
        record: RecordId,
        /// The member values.
        fields: Vec<Expr>,
    },
    /// A `union` value, which initialises exactly one member.
    UnionLit {
        /// The record's identity.
        record: RecordId,
        /// The index of the initialised member.
        index: usize,
        /// Its value.
        value: Box<Expr>,
    },
    /// An array value: one expression per element.
    ArrayLit(Vec<Expr>),
    /// An array value whose elements are all the same: `[value; len]`.
    ArrayRepeat {
        /// The repeated element.
        value: Box<Expr>,
        /// How many times it is repeated.
        len: u64,
    },
    /// A fresh copy of the argument list the function was called with.
    ///
    /// It is what `va_start` stores and what a `va_list` local starts out as;
    /// the list it copies is the `...` parameter of a variadic definition, or
    /// the function's own `va_list` parameter. Its type is [`Ty::VaList`].
    VaListPristine,
    /// `va_arg(ap, T)`: reads the next argument and advances `ap`. The
    /// expression's own type is `T`.
    VaArg {
        /// The list to read from and advance.
        ap: Place,
        /// How a `struct` or `union` is taken apart to be read: one entry per
        /// [eightbyte](Eightbyte) of it, in order. `None` for every other
        /// type, which is read in one `next_arg` at the type itself.
        record: Option<Vec<Eightbyte>>,
    },
    /// C23's `unreachable()`, which promises control never gets here.
    Unreachable,
    /// `va_end(ap)`, whose type is `void`.
    ///
    /// Rust ends a list when it goes out of scope, so this does nothing; it is
    /// a node of its own so that the expansion does not have to pretend the
    /// call happened.
    VaEnd,
}

// ---------------------------------------------------------------------------
// statements
// ---------------------------------------------------------------------------

/// What a `break` leaves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakTarget {
    /// The innermost enclosing loop.
    Loop(LoopId),
    /// The innermost enclosing `switch`.
    Switch(SwitchId),
}

/// A typed statement.
#[derive(Clone, Debug)]
pub enum Stmt {
    /// The null statement.
    Nop,
    /// An expression evaluated for its side effects.
    Expr(Expr),
    /// A local variable definition. C leaves an uninitialised local
    /// indeterminate; the initialiser here is a zero of the right type in that
    /// case, so that the generated Rust never reads uninitialised memory.
    Let {
        /// The object being defined.
        object: ObjectId,
        /// Its initial value, already converted to the object's type.
        init: Expr,
        /// Whether the source wrote an initialiser at all.
        ///
        /// It decides what happens when the definition has to be hoisted out
        /// of the block it was written in — out of a `switch` body, or to the
        /// top of a function lowered into a [control-flow graph](crate::cfg).
        /// The hoisted definition zero-initialises; only an initialiser the
        /// program actually wrote has to run again where it was written.
        explicit: bool,
    },
    /// The definition of a variable length array (C99 6.7.5.2).
    ///
    /// It is a [`Stmt::Let`] with two bindings instead of one, because the
    /// object needs storage whose size is only known here; see [`VlaDef`].
    Vla(Box<VlaDef>),
    /// `T x __attribute__((cleanup(f)));` — the registration of the call that
    /// runs when `x` goes out of scope. See [`CleanupDef`].
    Cleanup(Box<CleanupDef>),
    /// A compound statement.
    Block(Vec<Stmt>),
    /// `if (cond) then_branch else else_branch`
    If {
        /// The controlling expression.
        cond: Expr,
        /// Taken when `cond` is non-zero.
        then_branch: Box<Stmt>,
        /// Taken otherwise.
        else_branch: Option<Box<Stmt>>,
    },
    /// `while (cond) body`
    While {
        /// This loop's identity.
        id: LoopId,
        /// The controlling expression.
        cond: Expr,
        /// The loop body.
        body: Box<Stmt>,
        /// Where the statement was written.
        range: SourceRange,
    },
    /// `do body while (cond);`
    DoWhile {
        /// This loop's identity.
        id: LoopId,
        /// The loop body.
        body: Box<Stmt>,
        /// The controlling expression.
        cond: Expr,
        /// Where the statement was written.
        range: SourceRange,
    },
    /// `for (init; cond; step) body`
    For {
        /// This loop's identity.
        id: LoopId,
        /// The init clause, which may declare variables scoped to the loop.
        init: Vec<Stmt>,
        /// The controlling expression; absent means "always true".
        cond: Option<Expr>,
        /// The iteration expression.
        step: Option<Expr>,
        /// The loop body.
        body: Box<Stmt>,
        /// Where the statement was written.
        range: SourceRange,
    },
    /// `switch (scrutinee) { … }`
    Switch(Box<Switch>),
    /// `switch (scrutinee) body`, with the body left as a statement tree.
    ///
    /// Only produced in [CFG mode](crate::cfg), where the labels stay where
    /// they were written — a `case` inside a nested statement (Duff's device)
    /// is simply another edge into the loop the CFG builds.
    SwitchTree(Box<SwitchTree>),
    /// `case value:` or `default:` inside a [`Stmt::SwitchTree`].
    Case {
        /// The `switch` the label belongs to.
        switch: SwitchId,
        /// The values that enter here, or `None` for `default:`.
        value: Option<CaseRange>,
        /// The labelled statement.
        body: Box<Stmt>,
        /// Where the label was written.
        range: SourceRange,
    },
    /// `label: body` — a `goto` target.
    Label {
        /// The label's identity.
        id: LabelId,
        /// The labelled statement.
        body: Box<Stmt>,
        /// Where the label was written.
        range: SourceRange,
    },
    /// A labelled region a `goto` leaves, in the structured lowering.
    ///
    /// Only produced by [`regions`](crate::regions), which is where the shape
    /// and the two kinds are described. A [`Stmt::Goto`] inside one names its
    /// label and becomes `break` or `continue` accordingly.
    Region(Box<Region>),
    /// `goto label;`
    Goto {
        /// The label jumped to.
        id: LabelId,
        /// Where the statement was written.
        range: SourceRange,
    },
    /// GNU's computed `goto *e;`, whose operand is a [label
    /// address](ExprKind::LabelAddr).
    ///
    /// Only produced in [CFG mode](crate::cfg), which is the only mode a
    /// function containing one is lowered in.
    GotoPtr {
        /// The pointer jumped through.
        target: Expr,
        /// Where the statement was written.
        range: SourceRange,
    },
    /// `break;`
    Break {
        /// What the `break` leaves.
        target: BreakTarget,
        /// Where the statement was written.
        range: SourceRange,
    },
    /// `continue;`
    Continue {
        /// The loop the `continue` restarts.
        id: LoopId,
        /// Where the statement was written.
        range: SourceRange,
    },
    /// `return;` or `return expr;`
    Return {
        /// The returned value.
        value: Option<Expr>,
        /// Where the statement was written.
        range: SourceRange,
    },
}

impl Stmt {
    /// Whether this is a [`cleanup`](CleanupDef) registration.
    pub fn is_cleanup(&self) -> bool {
        matches!(self, Stmt::Cleanup(_))
    }
}

/// A variably modified object's definition: `T a[n];`, `T a[n][m];`.
///
/// C99 6.7.5.2 gives the object automatic storage duration, a size fixed when
/// the declaration is reached, and the lifetime of the block it is written in;
/// each bound is evaluated exactly once, where the declaration stands, and
/// lives in a hidden `size_t` object the [type](ArrayType::vla_len) points at.
/// This crate emulates the storage on the heap — the elements live in a hidden
/// `Vec` whose `Drop` is that lifetime — so the one definition becomes a
/// [`Stmt::Let`] per bound followed by two more bindings:
///
/// ```text
/// let __cinrs_vla_len_a: size_t = <n>;                     // one per bound
/// let mut __cinrs_vla_a: Vec<T> = vec![<zero>; count];     // storage
/// let mut a: *mut T = __cinrs_vla_a.as_mut_ptr();          // object
/// ```
///
/// From there the object *is* a pointer to the first element: decay is the
/// identity, `a[i]` is pointer indexing scaled by the run-time size of a row,
/// and `sizeof a` is the product of the bounds times the element size — see
/// [`Types::vm_step_ty`] for what "element" means once more than one dimension
/// is variable.
#[derive(Clone, Debug)]
pub struct VlaDef {
    /// The object the C program declared, whose type is the array type and
    /// whose generated binding is a pointer to the first element.
    pub object: ObjectId,
    /// The hidden `Vec` the elements live in; see [`Object::vla_storage`].
    pub storage: ObjectId,
    /// The number of elements to allocate: the product of every dimension,
    /// read out of the hidden bound objects, in units of the storage's
    /// element type.
    pub count: Expr,
    /// Where the declarator was written.
    pub range: SourceRange,
}

/// A `cleanup` attribute's registration: `T x __attribute__((cleanup(f)));`.
///
/// GCC calls `f(&x)` on *every* exit from the scope `x` was declared in, in
/// reverse declaration order. The two lowerings say that in different ways:
///
/// * the structured one binds a drop guard right after the object, so that
///   Rust's own drop order — reverse declaration order, on every path out of
///   the block, `return` from inside a statement expression included — is C's;
/// * the [CFG](crate::cfg) one has no scopes left to drop in, so it emits
///   [`CleanupDef::call`] on each edge that leaves the scope.
#[derive(Clone, Debug)]
pub struct CleanupDef {
    /// The variable whose address the function is given.
    pub object: ObjectId,
    /// The function called with it.
    pub func: FuncId,
    /// The type of that function's one parameter, which is the pointer type
    /// the address is converted to.
    pub param: Ty,
    /// `f(&x)`, ready to be emitted where a scope is left.
    pub call: Expr,
    /// Where the declarator was written.
    pub range: SourceRange,
}

/// A `switch` statement, flattened into the groups its labels delimit.
///
/// The body of a `switch` is a single statement that execution *jumps into*,
/// which is why it cannot be a tree here: the labels split it into a sequence
/// of groups that fall through into one another. See [`codegen`](crate::codegen)
/// for how the sequence becomes Rust.
#[derive(Clone, Debug)]
pub struct Switch {
    /// This switch's identity.
    pub id: SwitchId,
    /// The controlling expression, after the integer promotions.
    pub scrutinee: Expr,
    /// Objects declared directly in the switch body.
    ///
    /// C keeps them alive for the whole body even though execution may jump
    /// past their declaration, so they are defined ahead of the dispatch and
    /// zero-initialised; whatever initialiser the source wrote stays where it
    /// was written, as an assignment.
    pub hoisted: Vec<ObjectId>,
    /// Statements between the `{` and the first label. C can never reach them.
    pub prelude: Vec<Stmt>,
    /// The groups, in source order.
    pub groups: Vec<SwitchGroup>,
    /// The index in `groups` that `default:` labels, if any.
    pub default_group: Option<usize>,
    /// Where the statement was written.
    pub range: SourceRange,
}

/// A labelled region a `goto` leaves or restarts.
///
/// The statements a C label divides are wrapped in one of these when every
/// `goto` to that label is a jump Rust can make on its own; see
/// [`regions`](crate::regions) for which jumps those are and how the
/// boundaries are chosen. Code generation emits a labelled block or a labelled
/// loop, named after the C label:
///
/// ```text
/// 'done: { … break 'done; … }        'retry: loop { … continue 'retry; … break 'retry; }
/// ```
#[derive(Clone, Debug)]
pub struct Region {
    /// The label the region belongs to, which the `goto`s inside it name.
    pub label: LabelId,
    /// The label's name in C, which the generated Rust label is built from.
    pub name: String,
    /// Whether a `goto` to it leaves the region or restarts it.
    pub kind: RegionKind,
    /// The statements inside.
    pub body: Vec<Stmt>,
    /// Whether control can reach the end of `body`, so that a
    /// [loop](RegionKind::Loop) needs a `break` there to leave it. A loop
    /// without one is a Rust `loop` that never finishes, which is what makes a
    /// function ending in it need no `return`.
    pub falls_out: bool,
    /// Where the label was written.
    pub range: SourceRange,
}

/// Which way a [`Region`]'s label is entered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionKind {
    /// The label stands *after* the region: a `goto` to it is `break 'l`, and
    /// the statements the label introduces follow the block.
    Block,
    /// The label stands at the *start* of the region: a `goto` to it is
    /// `continue 'l`, and control leaves by falling off the end.
    Loop,
}

/// A `switch` whose body has been left as a statement tree.
///
/// The [CFG lowering](crate::cfg) walks the tree and turns every [`Stmt::Case`]
/// it finds into an edge from this statement's dispatch, wherever in the tree
/// it sits. That is what makes Duff's device work, and it is why sema does not
/// need to flatten the body into groups in that mode.
#[derive(Clone, Debug)]
pub struct SwitchTree {
    /// This switch's identity, which `break` and the labels refer to.
    pub id: SwitchId,
    /// The controlling expression, after the integer promotions.
    pub scrutinee: Expr,
    /// The body, with its labels still in place.
    pub body: Box<Stmt>,
    /// Where the statement was written.
    pub range: SourceRange,
}

/// The values one `case` label matches.
///
/// A plain `case k:` is the range `k..=k`; GNU's `case low ... high:` is the
/// whole interval, which code generation emits as one Rust range pattern
/// rather than as one arm per value — `case 0 ... 1000000:` is a perfectly
/// ordinary thing to write.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseRange {
    /// The lowest value, already converted to the controlling type.
    pub low: i128,
    /// The highest, which equals `low` for a plain label.
    pub high: i128,
}

impl CaseRange {
    /// The range one value makes.
    pub fn single(value: i128) -> Self {
        Self {
            low: value,
            high: value,
        }
    }

    /// Whether this is a plain `case k:`.
    pub fn is_single(self) -> bool {
        self.low == self.high
    }

    /// Whether two labels would both match some value.
    pub fn overlaps(self, other: CaseRange) -> bool {
        self.low <= other.high && other.low <= self.high
    }
}

/// One run of statements in a `switch`, together with the values that enter it.
#[derive(Clone, Debug)]
pub struct SwitchGroup {
    /// The `case` values that jump here, already converted to the type of the
    /// controlling expression.
    pub values: Vec<CaseRange>,
    /// The statements, which fall through into the next group.
    pub body: Vec<Stmt>,
}

// ---------------------------------------------------------------------------
// reachability
// ---------------------------------------------------------------------------

/// Whether control can never fall off the end of `stmts`.
///
/// Used to decide whether a non-`void` function needs a synthesised
/// `return`. Deliberately conservative: saying "no" only ever costs a
/// `return` statement the program does not reach, while saying "yes" wrongly
/// would produce Rust that does not compile.
///
/// `functions` is the program's function table, which is what a call to a
/// `_Noreturn` function is recognised through; code generation makes that
/// divergence visible to Rust by following such a call with
/// `::core::unreachable!()`.
pub fn always_terminates(stmts: &[Stmt], functions: &[Function]) -> bool {
    stmts
        .last()
        .is_some_and(|stmt| stmt_always_terminates(stmt, functions))
}

/// Whether evaluating `expr` never returns.
///
/// Only a direct call to a `_Noreturn` function counts: a call through a
/// pointer has no declaration to read the specifier from.
pub fn expr_never_returns(expr: &Expr, functions: &[Function]) -> bool {
    match &expr.kind {
        ExprKind::Call {
            callee: Callee::Direct(id),
            ..
        } => functions
            .get(id.0 as usize)
            .is_some_and(|func| func.noreturn),
        ExprKind::Unreachable => true,
        // `(void)abort();` and `f(), abort();` end just as surely.
        ExprKind::Cast(inner) => expr_never_returns(inner, functions),
        ExprKind::Comma { rhs, .. } => expr_never_returns(rhs, functions),
        _ => false,
    }
}

/// Whether evaluating `expr` can call a function.
///
/// C11 6.5.16.2p3 is why this is worth asking: a compound assignment is, "with
/// respect to an indeterminately-sequenced function call, a single evaluation",
/// so the read-modify-write of `x |= f()` may not be split around the call to
/// `f` the way `x = x | f()` would be. Code generation evaluates such a right
/// operand into a temporary first, and asks this to know when it has to.
pub fn calls_a_function(expr: &Expr) -> bool {
    let any = |list: &[Expr]| list.iter().any(calls_a_function);
    match &expr.kind {
        // A statement expression is a block, which can hold anything.
        ExprKind::Call { .. } | ExprKind::StmtExpr { .. } => true,
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Zeroed
        | ExprKind::FuncAddr(_)
        | ExprKind::LabelAddr(_)
        | ExprKind::VaListPristine
        | ExprKind::Unreachable
        | ExprKind::VaEnd => false,
        ExprKind::Load(place) | ExprKind::AddrOf(place) => place_calls_a_function(place),
        ExprKind::VaArg { ap, .. } => place_calls_a_function(ap),
        ExprKind::Assign { place, value } | ExprKind::CompoundAssign { place, value, .. } => {
            place_calls_a_function(place) || calls_a_function(value)
        }
        ExprKind::IncDec { place, .. } => place_calls_a_function(place),
        ExprKind::Neg(inner) | ExprKind::BitNot(inner) | ExprKind::Cast(inner) => {
            calls_a_function(inner)
        }
        ExprKind::Binary { lhs, rhs, .. }
        | ExprKind::Compare { lhs, rhs, .. }
        | ExprKind::Logical { lhs, rhs, .. }
        | ExprKind::PtrDiff { lhs, rhs }
        | ExprKind::Comma { lhs, rhs } => calls_a_function(lhs) || calls_a_function(rhs),
        ExprKind::ComplexOf { re, im } => calls_a_function(re) || calls_a_function(im),
        ExprKind::PtrOffset { ptr, index, .. } => calls_a_function(ptr) || calls_a_function(index),
        ExprKind::Cond {
            cond,
            then_expr,
            else_expr,
        } => calls_a_function(cond) || calls_a_function(then_expr) || calls_a_function(else_expr),
        ExprKind::CondDefault { value, else_expr } => {
            calls_a_function(value) || calls_a_function(else_expr)
        }
        ExprKind::Builtin { args, .. } => any(args),
        ExprKind::Atomic(atomic) => atomic.operands().any(calls_a_function),
        ExprKind::RecordLit { fields, .. } => any(fields),
        ExprKind::UnionLit { value, .. } => calls_a_function(value),
        ExprKind::ArrayLit(items) => any(items),
        ExprKind::ArrayRepeat { value, .. } => calls_a_function(value),
    }
}

/// [`calls_a_function`], for the expressions inside a place.
fn place_calls_a_function(place: &Place) -> bool {
    match &place.kind {
        PlaceKind::Object(_) | PlaceKind::Str(_) => false,
        PlaceKind::Deref(ptr) => calls_a_function(ptr),
        PlaceKind::Index { base, index } => calls_a_function(base) || calls_a_function(index),
        PlaceKind::Field { base, .. } | PlaceKind::ComplexPart { base, .. } => {
            place_calls_a_function(base)
        }
        PlaceKind::Temporary(expr) => calls_a_function(expr),
        PlaceKind::CompoundLiteral { init, .. } => calls_a_function(init),
    }
}

/// Whether `expr` reads or takes the address of `object`.
///
/// C99 6.2.1p7 puts an identifier in scope from the end of its declarator, so
/// an initialiser may name the object it initialises: `struct list head = {
/// &head, &head }` is the idiom, and `T *p = malloc(sizeof *p)` is the one
/// everybody writes. A Rust binding cannot be named in its own initialiser, so
/// an *automatic* object whose initialiser does this is defined with a zero
/// and assigned afterwards; this is what tells the two apart. An object with
/// static storage duration needs nothing: the generated item takes its own
/// address with `&raw mut`, which reads nothing and is a constant.
pub fn mentions_object(expr: &Expr, object: ObjectId) -> bool {
    let any = |list: &[Expr]| list.iter().any(|e| mentions_object(e, object));
    match &expr.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Zeroed
        | ExprKind::FuncAddr(_)
        | ExprKind::LabelAddr(_)
        | ExprKind::VaListPristine
        | ExprKind::Unreachable
        | ExprKind::VaEnd => false,
        ExprKind::Load(place) | ExprKind::AddrOf(place) => place_mentions_object(place, object),
        ExprKind::VaArg { ap, .. } => place_mentions_object(ap, object),
        ExprKind::Assign { place, value } | ExprKind::CompoundAssign { place, value, .. } => {
            place_mentions_object(place, object) || mentions_object(value, object)
        }
        ExprKind::IncDec { place, .. } => place_mentions_object(place, object),
        ExprKind::Neg(inner) | ExprKind::BitNot(inner) | ExprKind::Cast(inner) => {
            mentions_object(inner, object)
        }
        ExprKind::Binary { lhs, rhs, .. }
        | ExprKind::Compare { lhs, rhs, .. }
        | ExprKind::Logical { lhs, rhs, .. }
        | ExprKind::PtrDiff { lhs, rhs }
        | ExprKind::Comma { lhs, rhs } => {
            mentions_object(lhs, object) || mentions_object(rhs, object)
        }
        ExprKind::ComplexOf { re, im } => {
            mentions_object(re, object) || mentions_object(im, object)
        }
        ExprKind::PtrOffset { ptr, index, .. } => {
            mentions_object(ptr, object) || mentions_object(index, object)
        }
        ExprKind::Cond {
            cond,
            then_expr,
            else_expr,
        } => {
            mentions_object(cond, object)
                || mentions_object(then_expr, object)
                || mentions_object(else_expr, object)
        }
        ExprKind::CondDefault { value, else_expr } => {
            mentions_object(value, object) || mentions_object(else_expr, object)
        }
        ExprKind::Call { callee, args } => {
            let callee = match callee {
                Callee::Direct(_) => false,
                Callee::Indirect(target) => mentions_object(target, object),
            };
            callee || any(args)
        }
        ExprKind::Builtin { args, .. } => any(args),
        ExprKind::Atomic(atomic) => atomic.operands().any(|e| mentions_object(e, object)),
        ExprKind::RecordLit { fields, .. } => any(fields),
        ExprKind::UnionLit { value, .. } => mentions_object(value, object),
        ExprKind::ArrayLit(items) => any(items),
        ExprKind::ArrayRepeat { value, .. } => mentions_object(value, object),
        // A statement expression is a block of its own; whatever it names, it
        // names through a scope this cannot walk, so it is taken to reach the
        // object rather than risk a binding read before it exists.
        ExprKind::StmtExpr { .. } => true,
    }
}

/// [`mentions_object`], for the expressions inside a place.
fn place_mentions_object(place: &Place, object: ObjectId) -> bool {
    match &place.kind {
        PlaceKind::Object(id) => *id == object,
        PlaceKind::Str(_) => false,
        PlaceKind::Deref(ptr) => mentions_object(ptr, object),
        PlaceKind::Index { base, index } => {
            mentions_object(base, object) || mentions_object(index, object)
        }
        PlaceKind::Field { base, .. } | PlaceKind::ComplexPart { base, .. } => {
            place_mentions_object(base, object)
        }
        PlaceKind::Temporary(expr) => mentions_object(expr, object),
        PlaceKind::CompoundLiteral { init, .. } => mentions_object(init, object),
    }
}

fn stmt_always_terminates(stmt: &Stmt, functions: &[Function]) -> bool {
    let terminates = |stmt: &Stmt| stmt_always_terminates(stmt, functions);
    match stmt {
        Stmt::Return { .. } => true,
        Stmt::Expr(expr) => expr_never_returns(expr, functions),
        Stmt::Block(items) => always_terminates(items, functions),
        Stmt::Label { body, .. } => terminates(body),
        // Control does not continue into the next statement: the jump is a
        // `break` or a `continue` of a region that encloses it.
        Stmt::Goto { .. } => true,
        // A block is left by falling off its end or by a `break` to its label,
        // and both continue at the label the region ends before; a loop is
        // left only by the `break` that stands at the end of its body, so one
        // that needs none never finishes.
        Stmt::Region(region) => match region.kind {
            RegionKind::Block => {
                always_terminates(&region.body, functions) && !jumps_to(&region.body, region.label)
            }
            RegionKind::Loop => !region.falls_out,
        },
        Stmt::If {
            then_branch,
            else_branch: Some(else_branch),
            ..
        } => terminates(then_branch) && terminates(else_branch),
        Stmt::While { id, cond, body, .. } => {
            is_always_true(cond) && !breaks_to(body, BreakTarget::Loop(*id))
        }
        Stmt::DoWhile { id, body, cond, .. } => {
            is_always_true(cond) && !breaks_to(body, BreakTarget::Loop(*id))
        }
        Stmt::For { id, cond, body, .. } => {
            cond.as_ref().is_none_or(is_always_true) && !breaks_to(body, BreakTarget::Loop(*id))
        }
        // Control enters a `switch` at one label and then runs through every
        // group after it, so the statement terminates exactly when it can
        // neither be skipped (there is a `default:`) nor left early (nothing
        // `break`s out of it) and the last group terminates.
        Stmt::Switch(switch) => {
            switch.default_group.is_some()
                && switch
                    .groups
                    .last()
                    .is_some_and(|group| always_terminates(&group.body, functions))
                && !switch
                    .groups
                    .iter()
                    .flat_map(|group| group.body.iter())
                    .chain(switch.prelude.iter())
                    .any(|s| breaks_to(s, BreakTarget::Switch(switch.id)))
        }
        _ => false,
    }
}

/// Whether `expr` is a constant that C treats as true.
///
/// Code generation asks the same question, so that a loop it turns into a Rust
/// `loop` — which has no exit for the type checker to see — is exactly the loop
/// [`always_terminates`] promised would not fall through.
pub fn is_always_true(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Int(v) => *v != 0,
        ExprKind::Float(v) => *v != 0.0,
        ExprKind::Cast(inner) => is_always_true(inner),
        _ => false,
    }
}

/// Whether any `goto` inside `stmts` names `label`.
///
/// Every `goto` left in a structured body is a jump to a [`Region`] enclosing
/// it, so this is what says whether control can leave a [block](
/// RegionKind::Block) other than by falling off its end.
fn jumps_to(stmts: &[Stmt], label: LabelId) -> bool {
    stmts.iter().any(|stmt| stmt_jumps_to(stmt, label))
}

fn stmt_jumps_to(stmt: &Stmt, label: LabelId) -> bool {
    let jumps = |stmt: &Stmt| stmt_jumps_to(stmt, label);
    match stmt {
        Stmt::Goto { id, .. } => *id == label,
        Stmt::Block(items) => items.iter().any(jumps),
        Stmt::Region(region) => region.body.iter().any(jumps),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => jumps(then_branch) || else_branch.as_ref().is_some_and(|s| jumps(s)),
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Label { body, .. }
        | Stmt::Case { body, .. } => jumps(body),
        Stmt::Switch(switch) => {
            switch.prelude.iter().any(jumps)
                || switch
                    .groups
                    .iter()
                    .any(|group| group.body.iter().any(jumps))
        }
        Stmt::SwitchTree(switch) => jumps(&switch.body),
        _ => false,
    }
}

/// Whether any `break` inside `stmt` leaves `target`.
///
/// Every `break` names what it leaves, so this never has to reason about
/// nesting: a `break` belonging to an inner loop simply names that loop.
fn breaks_to(stmt: &Stmt, target: BreakTarget) -> bool {
    let breaks = |stmt: &Stmt| breaks_to(stmt, target);
    match stmt {
        Stmt::Break { target: found, .. } => *found == target,
        Stmt::Block(items) => items.iter().any(breaks),
        Stmt::Region(region) => region.body.iter().any(breaks),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => breaks(then_branch) || else_branch.as_ref().is_some_and(|s| breaks(s)),
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Label { body, .. }
        | Stmt::Case { body, .. } => breaks(body),
        Stmt::Switch(switch) => {
            switch.prelude.iter().any(breaks)
                || switch
                    .groups
                    .iter()
                    .any(|g| g.body.iter().any(|s| breaks_to(s, target)))
        }
        Stmt::SwitchTree(switch) => breaks(&switch.body),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: TargetModel = TargetModel::LP64;

    #[test]
    fn small_types_promote_to_int() {
        for ty in [
            Ty::Bool,
            Ty::Char,
            Ty::SChar,
            Ty::UChar,
            Ty::Short,
            Ty::UShort,
        ] {
            assert_eq!(ty.promote(&T), Ty::Int, "{}", ty.scalar_name());
        }
        assert_eq!(Ty::Int.promote(&T), Ty::Int);
        assert_eq!(Ty::UInt.promote(&T), Ty::UInt);
        assert_eq!(Ty::Double.promote(&T), Ty::Double);
    }

    #[test]
    fn bit_fields_promote_by_their_width() {
        // 6.3.1.1p2's "as restricted by the width": `int` first, then
        // `unsigned int`, and only then the declared type. The expectations
        // were read off gcc 15 and clang 21 on x86-64.
        let p = |ty: Ty, width: u32| ty.promote_bit_field(width, ty.is_signed(&T), &T);
        assert_eq!(p(Ty::UInt, 31), Ty::Int);
        assert_eq!(p(Ty::UInt, 32), Ty::UInt);
        assert_eq!(p(Ty::Int, 32), Ty::Int);
        assert_eq!(p(Ty::Int, 3), Ty::Int);
        assert_eq!(p(Ty::Bool, 1), Ty::Int);
        assert_eq!(p(Ty::Char, 8), Ty::Int);
        assert_eq!(p(Ty::UChar, 8), Ty::Int);
        assert_eq!(p(Ty::UShort, 16), Ty::Int);
        // The types GCC accepts as an extension follow the same rule, so a
        // narrow field of a wide type is still an `int`.
        assert_eq!(p(Ty::ULong, 31), Ty::Int);
        assert_eq!(p(Ty::ULong, 32), Ty::UInt);
        assert_eq!(p(Ty::ULong, 33), Ty::ULong);
        assert_eq!(p(Ty::Long, 33), Ty::Long);
        assert_eq!(p(Ty::LongLong, 32), Ty::Int);
        assert_eq!(p(Ty::ULongLong, 40), Ty::ULongLong);
        assert_eq!(p(Ty::ULongLong, 64), Ty::ULongLong);
        // An `enum` whose underlying type the implementation made unsigned.
        assert_eq!(Ty::Int.promote_bit_field(8, false, &T), Ty::Int);
        assert_eq!(Ty::Int.promote_bit_field(32, false, &T), Ty::UInt);
    }

    #[test]
    fn unsigned_short_promotes_to_unsigned_int_on_a_16_bit_target() {
        let t = TargetModel {
            int_bits: 16,
            ..TargetModel::ILP32
        };
        assert_eq!(Ty::UShort.promote(&t), Ty::UInt);
        assert_eq!(Ty::Short.promote(&t), Ty::Int);
    }

    #[test]
    fn the_usual_arithmetic_conversions_follow_6_3_1_8() {
        let u = |a, b| Ty::usual_arithmetic(a, b, &T);
        assert_eq!(u(Ty::Int, Ty::UInt), Ty::UInt);
        assert_eq!(u(Ty::Char, Ty::Char), Ty::Int);
        assert_eq!(u(Ty::Int, Ty::Long), Ty::Long);
        // `long` is wider than `unsigned int` on LP64, so it wins.
        assert_eq!(u(Ty::UInt, Ty::Long), Ty::Long);
        // ... but not on ILP32, where the result is `unsigned long`.
        assert_eq!(
            Ty::usual_arithmetic(Ty::UInt, Ty::Long, &TargetModel::ILP32),
            Ty::ULong
        );
        // `long long` cannot hold every `unsigned long`, so both become
        // `unsigned long long` — the last clause of 6.3.1.8.
        assert_eq!(u(Ty::ULong, Ty::LongLong), Ty::ULongLong);
        assert_eq!(u(Ty::UChar, Ty::Long), Ty::Long);
        assert_eq!(u(Ty::Float, Ty::LongLong), Ty::Float);
        assert_eq!(u(Ty::Double, Ty::Float), Ty::Double);
    }

    /// GNU's `__int128` ranks above every standard integer type, which is what
    /// makes `(__int128) a * b` a 128-bit multiplication.
    #[test]
    fn int128_outranks_every_standard_integer_type() {
        let u = |a, b| Ty::usual_arithmetic(a, b, &T);
        assert_eq!(u(Ty::Int128, Ty::LongLong), Ty::Int128);
        assert_eq!(u(Ty::Int128, Ty::ULongLong), Ty::Int128);
        assert_eq!(u(Ty::UInt128, Ty::LongLong), Ty::UInt128);
        assert_eq!(u(Ty::UInt128, Ty::Int128), Ty::UInt128);
        assert_eq!(u(Ty::Int128, Ty::Int), Ty::Int128);
        // …and a floating type still outranks it.
        assert_eq!(u(Ty::Float, Ty::UInt128), Ty::Float);
        assert_eq!(u(Ty::Double, Ty::Int128), Ty::Double);
        // The promotions leave it alone, as they do every type of `int`'s rank
        // or above.
        assert_eq!(Ty::Int128.promote(&T), Ty::Int128);
        assert_eq!(Ty::UInt128.promote(&T), Ty::UInt128);
        assert_eq!(Ty::Int128.promote_argument(&T), Ty::Int128);
        assert_eq!(Ty::Int128.to_unsigned(), Ty::UInt128);
        assert!(Ty::Int128.is_signed(&T));
        assert!(!Ty::UInt128.is_signed(&T));
        assert_eq!(Ty::Int128.size_bytes(&T), 16);
        assert_eq!(Ty::UInt128.bits(&T), 128);
    }

    /// A 128-bit constant is carried as its two's-complement bit pattern, so
    /// `wrap` leaves it alone and the ends of the range come out exactly.
    #[test]
    fn int128_constants_are_carried_as_bit_patterns() {
        assert_eq!(Ty::UInt128.wrap(-1, &T), -1);
        assert_eq!(Ty::Int128.wrap(-1, &T), -1);
        assert_eq!(Ty::Int128.wrap(i128::MIN, &T), i128::MIN);
        assert_eq!(Ty::Int128.min_value(&T), i128::MIN);
        assert_eq!(Ty::Int128.max_value(&T), i128::MAX);
        assert_eq!(Ty::UInt128.min_value(&T), 0);
        // Clamped, and documented as such: the real maximum is 2^128 - 1.
        assert_eq!(Ty::UInt128.max_value(&T), i128::MAX);
        // Converting a 128-bit pattern down to a narrower type is the ordinary
        // truncation.
        assert_eq!(Ty::UInt.wrap(-1, &T), 4_294_967_295);
    }

    /// `__int128` is sixteen bytes; its *alignment* is the one thing the model
    /// decides, because it is the one scalar whose alignment is not its size
    /// on every target.
    #[test]
    fn int128_takes_its_alignment_from_the_model() {
        let types = Types::new();
        let layout = types
            .size_align(Ty::UInt128, &T)
            .expect("a scalar has a layout");
        assert_eq!(layout.size, 16);
        assert_eq!(layout.align, 16);
        let eight = TargetModel {
            int128_align: 8,
            ..TargetModel::LP64
        };
        let layout = types
            .size_align(Ty::Int128, &eight)
            .expect("a scalar has a layout");
        assert_eq!(layout.size, 16);
        assert_eq!(layout.align, 8);
    }

    #[test]
    fn conversions_wrap() {
        assert_eq!(Ty::UChar.wrap(300, &T), 44);
        assert_eq!(Ty::SChar.wrap(200, &T), -56);
        assert_eq!(Ty::UInt.wrap(-1, &T), 4_294_967_295);
        assert_eq!(Ty::Int.wrap(4_294_967_295, &T), -1);
        assert_eq!(Ty::Bool.wrap(5, &T), 1);
        assert_eq!(Ty::Bool.wrap(0, &T), 0);
        assert_eq!(Ty::ULongLong.wrap(-1, &T), u64::MAX as i128);
    }

    #[test]
    fn derived_types_are_interned() {
        let mut types = Types::new();
        let a = types.pointer(Ty::Int, false);
        let b = types.pointer(Ty::Int, false);
        let c = types.pointer(Ty::Int, true);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(types.same_pointee(a, c));
        assert_eq!(types.pointee(a), Some(Ty::Int));
        let arr = types.array(Ty::Char, 4, false);
        assert_eq!(arr, types.array(Ty::Char, 4, false));
        assert_ne!(arr, types.array(Ty::Char, 5, false));
        let f = types.func(Ty::Int, vec![a], false);
        assert_eq!(f, types.func(Ty::Int, vec![b], false));
        assert_ne!(f, types.func(Ty::Int, vec![a], true));
    }

    #[test]
    fn layouts_follow_natural_alignment() {
        let mut types = Types::new();
        let arr = types.array(Ty::Char, 5, false);
        assert_eq!(
            types.size_align(arr, &T),
            Some(Layout { size: 5, align: 1 })
        );
        let ptr = types.pointer(Ty::Void, false);
        assert_eq!(
            types.size_align(ptr, &T),
            Some(Layout { size: 8, align: 8 })
        );
        // A function type has no size at all.
        let f = types.func(Ty::Void, Vec::new(), false);
        assert_eq!(types.size_align(f, &T), None);
    }

    #[test]
    fn names_read_like_c() {
        let mut types = Types::new();
        let cchar = types.pointer(Ty::Char, true);
        assert_eq!(types.name(cchar), "const char *");
        let arr = types.array(Ty::Int, 3, false);
        assert_eq!(types.name(arr), "int[3]");
        let f = types.func(Ty::Int, vec![cchar], true);
        let fp = types.pointer(f, false);
        assert_eq!(types.name(fp), "int (*)(const char *, ...)");
    }
}
