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
    /// `float`
    Float,
    /// `double` (and `long double`)
    Double,
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
    /// The type of something whose declaration was already reported as wrong.
    ///
    /// It exists so that one bad declaration produces one diagnostic: an object
    /// declared `int a[n]` still enters the symbol table, and every later use of
    /// it is checked against a type that silences further complaints instead of
    /// "use of undeclared identifier".
    Error,
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

/// An array type.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ArrayType {
    /// The element type.
    pub elem: Ty,
    /// The number of elements.
    pub len: u64,
    /// Whether the element type is `const`-qualified, which is what decides
    /// the constness of the pointer the array decays to.
    pub elem_const: bool,
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
    pointer_index: HashMap<PointerType, PointerId>,
    array_index: HashMap<ArrayType, ArrayId>,
    func_index: HashMap<FuncType, FuncTyId>,
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

    /// The type `elem[len]`.
    pub fn array(&mut self, elem: Ty, len: u64, elem_const: bool) -> Ty {
        let key = ArrayType {
            elem,
            len,
            elem_const,
        };
        if let Some(id) = self.array_index.get(&key) {
            return Ty::Array(*id);
        }
        let id = ArrayId(self.arrays.len() as u32);
        self.arrays.push(key);
        self.array_index.insert(key, id);
        Ty::Array(id)
    }

    /// The type of a function.
    pub fn func(&mut self, ret: Ty, params: Vec<Ty>, variadic: bool) -> Ty {
        let key = FuncType {
            ret,
            params,
            variadic,
        };
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
            Ty::Array(id) => self.is_complete(self.array_type(id).elem),
            _ => true,
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
                Layout {
                    size: elem.size.saturating_mul(array.len),
                    align: elem.align,
                }
            }
            Ty::Record(id) => self.record(id).layout?,
            Ty::Enum(_) => {
                let size = u64::from(target.int_bits).div_ceil(8);
                Layout { size, align: size }
            }
            scalar => {
                let size = scalar.size_bytes(target);
                Layout { size, align: size }
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
        if params.is_empty() {
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
            Ty::Float => "float",
            Ty::Double => "double",
            Ty::Pointer(_) => "pointer",
            Ty::Array(_) => "array",
            Ty::Func(_) => "function",
            Ty::Record(_) => "struct",
            Ty::Enum(_) => "enum",
            Ty::VaList => "va_list",
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
                | Ty::Enum(_)
        )
    }

    /// Whether this is `float` or `double`.
    pub fn is_floating(self) -> bool {
        matches!(self, Ty::Float | Ty::Double)
    }

    /// Whether this is an arithmetic type.
    pub fn is_arithmetic(self) -> bool {
        self.is_integer() || self.is_floating()
    }

    /// Whether this is a scalar, i.e. something C can compare against zero.
    pub fn is_scalar(self) -> bool {
        self.is_arithmetic() || self.is_pointer()
    }

    /// Whether values of this type are signed.
    pub fn is_signed(self, target: &TargetModel) -> bool {
        match self {
            Ty::Char => target.char_signed,
            Ty::SChar | Ty::Short | Ty::Int | Ty::Long | Ty::LongLong | Ty::Enum(_) => true,
            Ty::Float | Ty::Double => true,
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
            Ty::Float => 32,
            Ty::Double => 64,
            Ty::Pointer(_) => target.ptr_bits,
            Ty::Array(_) | Ty::Func(_) | Ty::Record(_) | Ty::VaList | Ty::Error => 0,
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
            Ty::Float => 7,
            Ty::Double => 8,
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
            other => other,
        }
    }

    /// The smallest value this integer type can hold.
    pub fn min_value(self, target: &TargetModel) -> i128 {
        if !self.is_signed(target) {
            return 0;
        }
        -(1i128 << (self.bits(target) - 1))
    }

    /// The largest value this integer type can hold.
    pub fn max_value(self, target: &TargetModel) -> i128 {
        if self == Ty::Bool {
            return 1;
        }
        let bits = self.bits(target);
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
    pub fn usual_arithmetic(lhs: Ty, rhs: Ty, target: &TargetModel) -> Ty {
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
    pub fn size_ty(target: &TargetModel) -> Ty {
        if Ty::ULong.bits(target) == target.ptr_bits {
            Ty::ULong
        } else {
            Ty::UInt
        }
    }

    /// `ptrdiff_t` for `target`.
    pub fn ptrdiff_ty(target: &TargetModel) -> Ty {
        if Ty::Long.bits(target) == target.ptr_bits {
            Ty::Long
        } else {
            Ty::Int
        }
    }

    /// `wchar_t` for `target`, which is `int` on every platform this supports.
    pub fn wchar_ty() -> Ty {
        Ty::Int
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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
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
    /// An object defined outside the translation unit, declared in the
    /// expansion's `extern` block.
    Extern {
        /// The name the symbol has.
        item_name: String,
    },
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
    /// The body, present once a definition has been type checked.
    pub body: Option<Body>,
    /// Where the function's name was written, at its definition if there is one
    /// and at its first declaration otherwise.
    pub range: SourceRange,
}

impl Function {
    /// Whether this function is only declared here and linked from elsewhere.
    pub fn is_extern(&self) -> bool {
        self.body.is_none()
    }
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
    /// The elements, without the terminating NUL: bytes for a narrow literal,
    /// `wchar_t` values for a wide one.
    pub values: Vec<u32>,
    /// Whether the literal was written `L"…"`.
    pub wide: bool,
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
    /// The file-scope `typedef`s, in declaration order.
    pub typedefs: Vec<TypedefItem>,
    /// The `enum` constants that become Rust `const` items, in order.
    pub enum_constants: Vec<Enumerator>,
    /// Every string literal, indexed by [`StrId`].
    pub strings: Vec<StrData>,
    /// The libraries the `extern` block must be linked against, named by
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
}

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

    /// The Rust name standing for an externally linked C symbol.
    ///
    /// The whole expansion already lives in a module of its own, so the rename
    /// is not what keeps two `c99!` blocks apart any more; what it keeps apart
    /// is the *glob re-export* and the module the invocation is written in. A
    /// unit that declares `int abs(int);` would otherwise re-export a bare
    /// `abs` into the user's namespace, which is not something the user asked
    /// for. A module of its own for the `extern` block would have been
    /// prettier still, but a module cannot see the `struct` items of the block
    /// it is written in, which an `extern` declaration taking a `struct`
    /// needs.
    pub fn extern_name(&self, symbol: &str) -> String {
        format!("__cinrs_{:08x}_{symbol}", self.unit_id as u32)
    }

    /// Whether anything at all has to go into the `extern` block.
    pub fn has_externs(&self) -> bool {
        !self.externs.is_empty() || self.functions.iter().any(Function::is_extern)
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
    /// Where it was written.
    pub range: SourceRange,
}

impl Expr {
    /// Builds an expression.
    pub fn new(kind: ExprKind, ty: Ty, range: SourceRange) -> Self {
        Self { kind, ty, range }
    }

    /// An integer constant of type `ty`.
    pub fn int(value: i128, ty: Ty, range: SourceRange) -> Self {
        Self::new(ExprKind::Int(value), ty, range)
    }
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
    },
    /// `offsetof(T, member)`, whose type is `size_t`.
    ///
    /// Left unevaluated so that code generation can ask Rust for the offset of
    /// the field in the item it generated; see [`crate::sema`].
    OffsetOf {
        /// The `struct` or `union`.
        record: RecordId,
        /// The field indices to follow, one per level: more than one when the
        /// member is reached through an anonymous member.
        path: Vec<usize>,
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
        /// The value that enters here, or `None` for `default:`.
        value: Option<i128>,
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
    /// `goto label;`
    Goto {
        /// The label jumped to.
        id: LabelId,
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

/// One run of statements in a `switch`, together with the values that enter it.
#[derive(Clone, Debug)]
pub struct SwitchGroup {
    /// The `case` values that jump here, already converted to the type of the
    /// controlling expression.
    pub values: Vec<i128>,
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

fn stmt_always_terminates(stmt: &Stmt, functions: &[Function]) -> bool {
    let terminates = |stmt: &Stmt| stmt_always_terminates(stmt, functions);
    match stmt {
        Stmt::Return { .. } => true,
        Stmt::Expr(expr) => expr_never_returns(expr, functions),
        Stmt::Block(items) => always_terminates(items, functions),
        Stmt::Label { body, .. } => terminates(body),
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

/// Whether any `break` inside `stmt` leaves `target`.
///
/// Every `break` names what it leaves, so this never has to reason about
/// nesting: a `break` belonging to an inner loop simply names that loop.
fn breaks_to(stmt: &Stmt, target: BreakTarget) -> bool {
    let breaks = |stmt: &Stmt| breaks_to(stmt, target);
    match stmt {
        Stmt::Break { target: found, .. } => *found == target,
        Stmt::Block(items) => items.iter().any(breaks),
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
