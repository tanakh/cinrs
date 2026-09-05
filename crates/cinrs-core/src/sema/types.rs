//! Resolving [`ast::Type`] into [`ir::Ty`], and laying records out.
//!
//! # Layout
//!
//! C leaves the layout of a `struct` to the implementation, but every ABI this
//! crate targets uses the same rule, and it is the rule Rust's `#[repr(C)]`
//! follows: each member is placed at the next offset that satisfies its own
//! alignment, the record's alignment is the strictest of its members', and its
//! size is rounded up to that. A `union` puts every member at offset zero and
//! is as large as its largest member, rounded up in the same way. Computing it
//! here — rather than deferring to `size_of` in the generated code — is what
//! makes `sizeof` a constant that array bounds, `case` labels and static
//! initialisers can be written in terms of.
//!
//! ## Bit-fields
//!
//! A bit-field is placed by a running *bit* offset instead. For a field of
//! type `T` and width `W > 0`, with `unit = 8 * sizeof(T)`: if the field would
//! straddle a unit boundary at the current offset it is first moved up to the
//! next multiple of `unit`; then it takes bits `off ..< off + W`, numbered from
//! the least significant bit of byte 0. An unnamed field of width 0 rounds the
//! offset up to the next multiple of `unit` and takes no storage. An ordinary
//! member after bit-fields goes at `round_up(ceil(off / 8), alignof(T))`, and
//! the record's size is `ceil(off / 8)` rounded up to its alignment. A *named*
//! bit-field raises the record's alignment to `alignof(T)`; an unnamed one —
//! `:0` included — does not raise it at all, but does still occupy the bits it
//! names, so it can grow a `union`.
//!
//! That is what GCC and Clang do on this host; the rules were read off them
//! rather than out of the standard, which leaves all of it implementation
//! defined. `doc/gnu-extensions.md` records which parts are extensions.
//!
//! Since a bit-field has no address, it is not a field of the generated Rust
//! item: a maximal run of consecutive bit-fields shares one `[u8; K]` field
//! covering the bytes from the run's first bit to its last, and explicit
//! `[u8; M]` padding is inserted wherever `#[repr(C)]` would otherwise place
//! the field after a run too early. [`ir::RecordDef::rust_fields`] is that
//! list, and it is what code generation emits.

use std::collections::HashSet;

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{
    self, BitField, EnumDef, Field, Layout, RecordDef, RecordId, RecordKind, RustField, Ty,
};

use super::{Entry, Sema, TagEntry, TypeError};

/// A member as written, before the layout decides where it goes.
struct Member {
    /// The name, absent for an unnamed bit-field.
    name: Option<String>,
    /// Whether this is an anonymous `struct`/`union` member.
    anonymous: bool,
    ty: Ty,
    is_const: bool,
    /// The width and signedness of a bit-field.
    bits: Option<(u32, bool)>,
    /// What `_Alignas` or `__attribute__((aligned(N)))` asked for.
    align_request: Option<u64>,
    /// Whether `__attribute__((packed))` applies to this member, from the
    /// member itself or from the record.
    packed: bool,
    /// Whether this is the flexible array member.
    flexible: bool,
    range: SourceRange,
}

/// What packing a record asked for.
///
/// `Some(n)` is a maximum member alignment in bytes: `__attribute__((packed))`
/// is `Some(1)` and `#pragma pack(N)` is `Some(N)`. `None` means natural
/// alignment throughout.
///
/// The distinction matters twice. A member's alignment becomes
/// `min(natural, n)`, and so does the record's; and — the part that is easy to
/// miss — *any* packing switches off the bit-field allocation-unit rule, so a
/// field is placed at the next free bit however wide its type is. That is
/// GCC's own condition (`maximum_field_alignment == 0` in `stor-layout.cc`),
/// and it is what makes `#pragma pack(16)` change a layout it looks like it
/// should leave alone.
type Packing = Option<u64>;

/// What an array declarator's bound turned out to be.
enum ArrayLen {
    /// An integer constant expression.
    Fixed(u64),
    /// Anything else: a variable length array, whose bound has been left in
    /// [`Sema::vla_bound`].
    Variable,
    /// No bound at all — `int j[]`, an incomplete array type (6.2.5p22).
    Unspecified,
}

/// Where a member ends up.
enum Spot {
    /// A byte offset from the start of the record.
    Byte(u64),
    /// The first bit position of a bit-field.
    Bits { start: u64 },
}

/// One maximal run of consecutive bit-fields, which share a storage field.
struct Run {
    start: u64,
    end: u64,
    last: usize,
}

/// Everything laying a record out produces.
struct LaidOut {
    fields: Vec<Field>,
    rust_fields: Vec<RustField>,
    layout: Layout,
    /// The alignment the generated Rust item really has, which a packed record
    /// leaves at one; see [`ir::RecordDef::rust_align`].
    rust_align: u64,
    /// The alignment the generated item needs `#[repr(C, align(N))]` for.
    align_attr: Option<u64>,
    /// The maximum field alignment the item needs `#[repr(C, packed(N))]` for.
    packed_attr: Option<u64>,
}

impl Sema<'_> {
    /// Resolves an AST type into a [`Ty`], or explains why it cannot.
    pub(super) fn resolve_ty(&mut self, ty: &ast::Type) -> Result<Ty, TypeError> {
        let resolved = self.resolve_unqualified_ty(ty)?;
        self.check_restrict(ty, resolved)?;
        if ty.qualifiers.is_atomic {
            return self.make_atomic(resolved, ty.range);
        }
        Ok(resolved)
    }

    /// `_Atomic T` (C11 6.7.2.4), with the types C does not allow it on.
    ///
    /// The standard forbids an array or a function type outright (6.7.2.4p3);
    /// everything else it allows, and this crate narrows that to the types
    /// `core::sync::atomic` has an atomic for. A `struct` is the one that has
    /// to be turned away rather than being simply invalid C: it is legal, and
    /// a lock-free representation of one is exactly what Rust does not offer.
    pub(super) fn make_atomic(&mut self, inner: Ty, range: SourceRange) -> Result<Ty, TypeError> {
        if inner.is_error() {
            return Ok(inner);
        }
        if inner.is_array() || self.types().is_vla(inner) {
            return Err(TypeError::at(
                range,
                format!(
                    "'_Atomic' may not be applied to the array type '{}' (C11 6.7.2.4p3)",
                    self.tyname(inner)
                ),
            ));
        }
        if inner.is_func() {
            return Err(TypeError::at(
                range,
                format!(
                    "'_Atomic' may not be applied to the function type '{}' (C11 6.7.2.4p3)",
                    self.tyname(inner)
                ),
            ));
        }
        if inner.is_void() {
            return Err(TypeError::at(range, "'_Atomic void' is not a type"));
        }
        if !self.types().is_complete(inner) {
            return Err(TypeError::at(
                range,
                format!(
                    "'_Atomic' requires a complete type, and '{}' is incomplete",
                    self.tyname(inner)
                ),
            ));
        }
        let Some(_) = ir::atomic_class(self.types(), inner, &self.target) else {
            let reason = if inner.is_int128() {
                "there is no stable 128-bit atomic in `core::sync::atomic`"
            } else if self.types().is_func_pointer(inner) {
                "a function pointer is an `Option<fn>` in Rust, which no atomic holds"
            } else {
                "only the scalar types have a lock-free atomic in `core::sync::atomic`, and \
                 nothing in the generated Rust could stand for a lock"
            };
            return Err(TypeError::at(
                range,
                format!(
                    "'_Atomic {}' is not supported yet: {reason}",
                    self.tyname(inner)
                ),
            ));
        };
        // The alignment of an atomic type is its size, and the *object* is
        // generated as a plain one of the underlying type; on an ABI that
        // aligns an eight-byte scalar to four there is no way to give it the
        // eight bytes `AtomicU64::from_ptr` requires.
        let size = self.size_of(inner).unwrap_or(1);
        if size > self.target.max_scalar_align {
            return Err(TypeError::at(
                range,
                format!(
                    "'_Atomic {}' is not supported on this target: the object is {size} bytes \
                     and this ABI aligns it to {}, which a lock-free atomic of that width \
                     cannot be built on",
                    self.tyname(inner),
                    self.target.max_scalar_align
                ),
            ));
        }
        Ok(self.program.types.atomic(inner))
    }

    /// C99 6.7.3p2 for `restrict`.
    ///
    /// "shall only qualify a pointer to an object type" — so `int restrict i`
    /// and `void (*restrict fp)(void)` are constraint violations, while
    /// `int *restrict p`, `int_ptr restrict q` (a `typedef` of a pointer) and
    /// `void f(int a[restrict])` are all fine, the last because the parameter
    /// *is* a pointer. `restrict` says nothing to the generated Rust either
    /// way — Rust's own aliasing rules are stricter than the promise — so this
    /// is a diagnostic and nothing else.
    fn check_restrict(&mut self, ty: &ast::Type, resolved: Ty) -> Result<(), TypeError> {
        if !ty.qualifiers.is_restrict || resolved.is_error() {
            return Ok(());
        }
        if self.types().is_func_pointer(resolved) {
            return Err(TypeError::at(
                ty.range,
                format!(
                    "'restrict' qualifies a pointer to an object type, and '{}' points to a \
                     function",
                    self.tyname(resolved)
                ),
            ));
        }
        if !resolved.is_pointer() {
            return Err(TypeError::at(
                ty.range,
                format!(
                    "'restrict' requires a pointer to an object type ('{}' is invalid)",
                    self.tyname(resolved)
                ),
            ));
        }
        Ok(())
    }

    fn resolve_unqualified_ty(&mut self, ty: &ast::Type) -> Result<Ty, TypeError> {
        let range = ty.range;
        match &ty.kind {
            ast::TypeKind::Void => Ok(Ty::Void),
            ast::TypeKind::Bool => Ok(Ty::Bool),
            ast::TypeKind::Char(None) => Ok(Ty::Char),
            ast::TypeKind::Char(Some(ast::Sign::Signed)) => Ok(Ty::SChar),
            ast::TypeKind::Char(Some(ast::Sign::Unsigned)) => Ok(Ty::UChar),
            ast::TypeKind::Int { sign, size } => Ok(match (sign, size) {
                (ast::Sign::Signed, ast::IntSize::Short) => Ty::Short,
                (ast::Sign::Signed, ast::IntSize::Int) => Ty::Int,
                (ast::Sign::Signed, ast::IntSize::Long) => Ty::Long,
                (ast::Sign::Signed, ast::IntSize::LongLong) => Ty::LongLong,
                (ast::Sign::Unsigned, ast::IntSize::Short) => Ty::UShort,
                (ast::Sign::Unsigned, ast::IntSize::Int) => Ty::UInt,
                (ast::Sign::Unsigned, ast::IntSize::Long) => Ty::ULong,
                (ast::Sign::Unsigned, ast::IntSize::LongLong) => Ty::ULongLong,
                // GCC has `__int128` on the 64-bit architectures only and
                // refuses it outright on a 32-bit one rather than emulating
                // it; a program that has to work on both guards with
                // `#ifdef __SIZEOF_INT128__`, which is undefined there.
                (sign, ast::IntSize::Int128) if !self.target.has_int128 => {
                    return Err(TypeError::at(
                        range,
                        format!(
                            "'{}__int128' is not available on this target ({}, \
                             {}-bit pointers); guard on '__SIZEOF_INT128__'",
                            if *sign == ast::Sign::Unsigned {
                                "unsigned "
                            } else {
                                ""
                            },
                            self.target.arch.as_str(),
                            self.target.ptr_bits
                        ),
                    ));
                }
                (ast::Sign::Signed, ast::IntSize::Int128) => Ty::Int128,
                (ast::Sign::Unsigned, ast::IntSize::Int128) => Ty::UInt128,
            }),
            // `long double` has no portable Rust equivalent; it is mapped onto
            // `double`, which is what every other C-to-Rust translator does.
            ast::TypeKind::Float(ast::FloatSize::Float) => Ok(Ty::Float),
            ast::TypeKind::Float(_) => Ok(Ty::Double),
            ast::TypeKind::Complex(_) => {
                Err(TypeError::at(range, "complex types are not supported"))
            }
            ast::TypeKind::Imaginary(_) => {
                Err(TypeError::at(range, "imaginary types are not supported"))
            }
            ast::TypeKind::Pointer(inner) => {
                let pointee = self.resolve_ty(inner)?;
                // `int (*p)[n]` is a pointer to a variably modified type: the
                // pointer arithmetic on it would have to scale by a run-time
                // size, which is exactly the part of C99's VM machinery this
                // release leaves out.
                if self.types().is_vla(pointee) {
                    return Err(TypeError::at(range, super::VM_UNSUPPORTED));
                }
                if pointee.is_va_list() {
                    // Some code passes `va_list *` around to work with the
                    // array form of `va_list`; Rust's is a value, so there is
                    // nothing honest to point at.
                    return Err(TypeError::at(
                        range,
                        "pointers to va_list are not supported yet",
                    ));
                }
                Ok(self.ptr_to(pointee, inner.qualifiers.is_const))
            }
            ast::TypeKind::Array { elem, size, .. } => {
                let element = self.resolve_ty(elem)?;
                self.check_element_type(element, elem.range)?;
                let konst = elem.qualifiers.is_const;
                Ok(match self.array_len(size, range)? {
                    ArrayLen::Fixed(len) => self.program.types.array(element, len, konst),
                    ArrayLen::Variable => self.program.types.vla_array(element, konst),
                    ArrayLen::Unspecified => self.program.types.incomplete_array(element, konst),
                })
            }
            ast::TypeKind::Function(func) => {
                let ret = self.resolve_ty(&func.ret)?;
                if ret.is_array() {
                    return Err(TypeError::at(
                        func.ret.range,
                        "a function cannot return an array type",
                    ));
                }
                // `int ()` — before C23, which removed the form — is a
                // function type whose parameters are unspecified rather than
                // one that takes none; see [`ir::FuncType::prototyped`].
                if !self.is_prototyped(func) {
                    return Ok(self.program.types.unprototyped_func(ret));
                }
                let mut params = Vec::with_capacity(func.params.len());
                for param in &func.params {
                    let ty = self.resolve_param_ty(&param.ty)?;
                    if ty.is_void() {
                        return Err(TypeError::at(
                            param.range,
                            "parameter has incomplete type 'void'",
                        ));
                    }
                    params.push(ty);
                }
                Ok(self.program.types.func(ret, params, func.variadic))
            }
            ast::TypeKind::Record(id) => self.record_ty(*id),
            ast::TypeKind::Enum(id) => self.enum_ty(*id),
            // C23's `typeof`. The operand of the expression form is not
            // evaluated, and it does *not* decay: `typeof(a)` of an array is
            // the array type, which is the whole point of the operator.
            //
            // `typeof_unqual` is the same thing with the qualifiers taken off
            // (6.7.2.5p3), and `_Atomic` is the only one a resolved type here
            // carries — so `typeof(x)` of an `_Atomic int` object is an
            // `_Atomic int` and `typeof_unqual(x)` is an `int`.
            ast::TypeKind::Typeof { id, unqual } => {
                let resolved = match self.typeof_operand(*id) {
                    ast::TypeofOperand::Expr(expr) => {
                        let ty = if self.is_lvalue_form(expr) {
                            self.lvalue(expr).map(|place| place.ty)
                        } else {
                            self.expr(expr).map(|value| value.ty)
                        };
                        ty.ok_or_else(|| TypeError::silent(range))?
                    }
                    ast::TypeofOperand::Type(name) => self.resolve_ty(&name.ty)?,
                };
                Ok(if *unqual {
                    self.types().unatomic(resolved)
                } else {
                    resolved
                })
            }
            // `auto` is resolved against the initialiser, in `decl`; reaching
            // here means it was written somewhere an initialiser cannot be.
            ast::TypeKind::Auto => Err(TypeError::at(
                range,
                "'auto' is only allowed on a declaration with an initializer",
            )),
            ast::TypeKind::Typedef(name) => match self.lookup(&name.name) {
                Some(Entry::Typedef(entry)) => match &entry.resolved {
                    // Naming `va_list` is not what needs `core::ffi::VaList`:
                    // the `typedef` <stdarg.h> writes and the declaration of a
                    // `vprintf` nobody calls both name it, and neither
                    // generates anything. The gate is on declaring an *object*
                    // of the type; see `Sema::gate_va_list`.
                    Ok(ty) => Ok(*ty),
                    Err(message) => Err(TypeError {
                        range: name.range,
                        message: message.clone(),
                        note: Some((entry.range, format!("'{}' is declared", name.name))),
                    }),
                },
                _ => Err(TypeError::at(
                    name.range,
                    format!("unknown type name '{}'", name.name),
                )),
            },
            // Only produced by parser recovery, which has reported already.
            ast::TypeKind::Error => Err(TypeError::silent(range)),
        }
    }

    /// Resolves the type of a parameter, applying the adjustments C makes to
    /// one: an array parameter is a pointer, and a function parameter is a
    /// pointer to a function.
    pub(super) fn resolve_param_ty(&mut self, ty: &ast::Type) -> Result<Ty, TypeError> {
        let outer = std::mem::replace(&mut self.in_param_type, true);
        let resolved = self.resolve_param_ty_inner(ty);
        self.in_param_type = outer;
        resolved
    }

    fn resolve_param_ty_inner(&mut self, ty: &ast::Type) -> Result<Ty, TypeError> {
        if let ast::TypeKind::Array { elem, .. } = &ty.kind {
            // The bound of an array parameter is not part of its type at all:
            // `void f(int n, int a[n])`, `int a[*]` and `int a[static n]` all
            // declare an `int *`, exactly as `int a[]` does. It is not
            // evaluated here either — a *definition* evaluates it on entry,
            // which is `Sema::parameter_size_effects`, and a declaration that
            // is not one never evaluates it at all.
            let element = self.resolve_ty(elem)?;
            // ... but the *element* type still has to be one this crate can
            // point at, and `int a[3][n]` would be a pointer to a variable
            // length array.
            if self.types().is_vla(element) {
                return Err(TypeError::at(ty.range, super::VM_UNSUPPORTED));
            }
            return Ok(self.ptr_to(element, elem.qualifiers.is_const));
        }
        let resolved = self.resolve_ty(ty)?;
        if resolved.is_func() {
            return Ok(self.ptr_to(resolved, false));
        }
        // The adjustment is made on the *type*, not on the spelling, so a
        // parameter that reaches an array through a `typedef` is a pointer
        // too: `typedef int A[4]; void f(A a);` takes an `int *`, and the
        // declaration has to agree with `void f(int *a);`.
        if let Ty::Array(id) = resolved {
            let array = self.types().array_type(id);
            return Ok(self.ptr_to(array.elem, array.elem_const));
        }
        Ok(resolved)
    }

    /// Evaluates an array bound.
    ///
    /// A bound that is not an integer constant expression makes the type a
    /// [variable length array](ir::ArrayType::vla). The expression is checked
    /// and converted to `size_t` here and left in [`Sema::vla_bound`] for
    /// whoever is resolving the type: the declaration that creates the object
    /// evaluates it exactly once, where it was written (C99 6.7.5.2p5), and
    /// every other context refuses it.
    fn array_len(
        &mut self,
        size: &ast::ArraySize,
        range: SourceRange,
    ) -> Result<ArrayLen, TypeError> {
        let expr = match size {
            // `int j[]` is an *incomplete* array type (6.2.5p22), not an
            // error: `extern int j[];` declares one, a file-scope `int j[];`
            // is a tentative definition the end of the translation unit
            // completes to one element (6.9.2p5), and `int (*p)[]` and
            // `__builtin_types_compatible_p(int[5], int[])` are ordinary uses
            // of the type. What may *not* have one is an object with a size —
            // `object_ty_of` is where that is said.
            ast::ArraySize::Unspecified => return Ok(ArrayLen::Unspecified),
            // `int a[*]` says "variably modified, bound unspecified", and is
            // only allowed in a declaration that is not a definition — where
            // the parameter is a pointer and the bound never mattered. Reaching
            // here means it was written somewhere else.
            ast::ArraySize::Star => {
                return Err(TypeError::at(
                    range,
                    "'[*]' is only allowed in a function prototype",
                ));
            }
            ast::ArraySize::Expr(expr) => expr,
        };
        let Some(value) = self.expr(expr) else {
            return Err(TypeError::silent(range));
        };
        if !value.ty.is_integer() {
            return Err(TypeError::at(
                expr.range,
                format!(
                    "size of array has non-integer type '{}'",
                    self.tyname(value.ty)
                ),
            ));
        }
        let Some(ir::ConstValue::Int(len)) = self.const_eval(&value) else {
            // Inside a parameter's type the bound is allowed to be anything,
            // prototype at file scope or not (6.7.5.3p7) — but it is the
            // *element* type here, so the parameter is a pointer to a variable
            // length array, which is the part of C99's variably modified
            // machinery this release leaves out. `execute/pr22061-1`'s
            // `char a[2][N]` is that.
            if self.in_param_type {
                return Err(TypeError::at(expr.range, super::VM_UNSUPPORTED));
            }
            // A bound that is not constant is a variable length array inside a
            // function, and simply invalid at file scope, where there is no
            // moment at which the bound could be evaluated.
            if self.at_file_scope() {
                return Err(TypeError::at(
                    expr.range,
                    "array size is not an integer constant expression",
                ));
            }
            let size_ty = self.size_ty();
            self.vla_bound = Some(self.convert(value, size_ty));
            return Ok(ArrayLen::Variable);
        };
        if len < 0 {
            return Err(TypeError::at(expr.range, "array size is negative"));
        }
        u64::try_from(len)
            .map(ArrayLen::Fixed)
            .map_err(|_| TypeError::at(expr.range, "array size is too large"))
    }

    /// Rejects the element types an array cannot have.
    fn check_element_type(&mut self, elem: Ty, range: SourceRange) -> Result<(), TypeError> {
        if elem.is_func() {
            return Err(TypeError::at(range, "an array of functions is not allowed"));
        }
        // `int a[3][n]` and `int a[n][m]`: an array *of* a variable length
        // array is variably modified in more than one dimension, and indexing
        // it would have to scale by a run-time size.
        if self.types().is_vla(elem) {
            return Err(TypeError::at(range, super::VM_UNSUPPORTED));
        }
        if elem.is_va_list() {
            return Err(TypeError::at(range, super::VA_LIST_PLACEMENT));
        }
        if !self.types().is_complete(elem) {
            return Err(TypeError::at(
                range,
                format!("array has incomplete element type '{}'", self.tyname(elem)),
            ));
        }
        Ok(())
    }

    /// Resolves a type, reporting the reason it could not be resolved.
    pub(super) fn ty_of(&mut self, ty: &ast::Type) -> Option<Ty> {
        match self.resolve_ty(ty) {
            Ok(ty) => Some(ty),
            Err(err) => {
                if !err.message.is_empty() {
                    match err.note {
                        Some((range, note)) => {
                            self.error_note(err.range, err.message, range, note);
                        }
                        None => self.error(err.range, err.message),
                    }
                }
                None
            }
        }
    }

    /// Resolves a type that must name an object type, with
    /// `incomplete_array` saying whether `T x[]` is allowed here.
    ///
    /// It is in exactly two places (C99 6.9.2p3, 6.2.5p22): an `extern`
    /// declaration, whose object is defined in another unit and whose size is
    /// therefore none of this one's business, and a file-scope *tentative*
    /// definition, which the end of the translation unit completes to one
    /// element. Everywhere else an object needs a complete type.
    pub(super) fn declared_object_ty_of(
        &mut self,
        ty: &ast::Type,
        name: &str,
        incomplete_array: bool,
    ) -> Option<Ty> {
        let resolved = self.ty_of(ty)?;
        if resolved.is_void() {
            self.error(
                ty.range,
                format!("variable '{name}' has incomplete type 'void'"),
            );
            return None;
        }
        if incomplete_array && self.types().is_incomplete_array(resolved) {
            return Some(resolved);
        }
        if !self.types().is_complete(resolved) {
            self.error(
                ty.range,
                format!(
                    "variable '{name}' has incomplete type '{}'",
                    self.tyname(resolved)
                ),
            );
            return None;
        }
        Some(resolved)
    }

    // -- struct and union ---------------------------------------------------

    fn record_ty(&mut self, spec_id: ast::RecordSpecId) -> Result<Ty, TypeError> {
        // The type of every declarator that shares a specifier names that one
        // specifier, so resolving it twice would be defining the tag twice.
        if let Some(id) = self.record_by_spec[spec_id.index()] {
            return Ok(Ty::Record(id));
        }
        let spec = self.record_spec(spec_id);
        let kind = match spec.kind {
            ast::RecordKind::Struct => RecordKind::Struct,
            ast::RecordKind::Union => RecordKind::Union,
        };

        let Some(fields) = &spec.fields else {
            // A reference: `struct S *p`. C declares the tag if it is new.
            let name = spec.name.as_ref().expect("the parser requires a tag here");
            if let Some(TagEntry::Record(id)) = self.lookup_tag(&name.name) {
                if self.types().record(id).kind != kind {
                    return Err(TypeError::at(
                        spec.range,
                        format!(
                            "'{}' defined as the wrong kind of tag; it is a {} here",
                            name.name,
                            kind.as_str()
                        ),
                    ));
                }
                self.record_by_spec[spec_id.index()] = Some(id);
                return Ok(Ty::Record(id));
            }
            if let Some(TagEntry::Enum { .. }) = self.lookup_tag(&name.name) {
                return Err(TypeError::at(
                    spec.range,
                    format!("'{}' is already declared as an enum tag", name.name),
                ));
            }
            let id = self.declare_record(kind, Some(name.name.clone()), spec.range);
            self.insert_tag(&name.name, TagEntry::Record(id));
            self.record_by_spec[spec_id.index()] = Some(id);
            return Ok(Ty::Record(id));
        };

        // A definition. An incomplete tag declared in this very scope is what
        // it completes; anything else it introduces.
        let id = match spec.name.as_ref().and_then(|n| self.tag_here(&n.name)) {
            Some(TagEntry::Record(id)) if self.types().record(id).kind != kind => {
                let previous = self.types().record(id).range;
                return Err(TypeError {
                    range: spec.range,
                    message: format!(
                        "'{}' defined as the wrong kind of tag",
                        spec.name.as_ref().expect("matched a named tag").name
                    ),
                    note: Some((previous, "previously defined".to_owned())),
                });
            }
            Some(TagEntry::Record(id)) if !self.types().record(id).complete => id,
            Some(TagEntry::Record(id)) => {
                let previous = self.types().record(id).range;
                return Err(TypeError {
                    range: spec.range,
                    message: format!(
                        "redefinition of '{} {}'",
                        kind.as_str(),
                        spec.name.as_ref().expect("matched a named tag").name
                    ),
                    note: Some((previous, "previous definition is".to_owned())),
                });
            }
            Some(TagEntry::Enum { .. }) => {
                return Err(TypeError::at(
                    spec.range,
                    format!(
                        "'{}' is already declared as an enum tag",
                        spec.name.as_ref().expect("matched a named tag").name
                    ),
                ));
            }
            None => {
                let id = self.declare_record(
                    kind,
                    spec.name.as_ref().map(|n| n.name.clone()),
                    spec.range,
                );
                if let Some(name) = &spec.name {
                    self.insert_tag(&name.name, TagEntry::Record(id));
                }
                id
            }
        };
        // Registered before the members are resolved, so that a member of type
        // `struct S *` inside `struct S` finds the tag it is inside.
        self.record_by_spec[spec_id.index()] = Some(id);
        self.define_record(id, spec, fields);
        for assert in &spec.asserts {
            self.static_assert(assert);
        }
        Ok(Ty::Record(id))
    }

    /// Creates an incomplete tag and reserves the Rust name it will use.
    fn declare_record(
        &mut self,
        kind: RecordKind,
        tag: Option<String>,
        range: SourceRange,
    ) -> RecordId {
        let (rust_name, anonymous) = match &tag {
            Some(tag) => {
                // A tag shares no namespace with ordinary identifiers in C, but
                // it does in Rust; a taken name becomes `struct_S`.
                let name = if self.try_reserve_item_name(tag) {
                    tag.clone()
                } else {
                    self.reserve_item_name(&format!("{}_{tag}", kind.as_str()))
                };
                (name, false)
            }
            None => (self.anonymous_name(kind.as_str()), true),
        };
        self.program.types.add_record(RecordDef {
            kind,
            tag,
            rust_name,
            anonymous,
            fields: Vec::new(),
            rust_fields: Vec::new(),
            complete: false,
            layout: None,
            align: None,
            packed: None,
            rust_align: 1,
            flexible: false,
            emit: true,
            range,
        })
    }

    /// Resolves a member list and computes the record's layout.
    fn define_record(&mut self, id: RecordId, spec: &ast::RecordSpec, fields: &[ast::FieldDecl]) {
        let kind = self.types().record(id).kind;
        // What the record asks of every member: `packed` is a maximum
        // alignment of one byte, and `#pragma pack(N)` one of N.
        let packing: Packing = if spec.attrs.packed.is_some() {
            Some(1)
        } else {
            spec.pack.map(u64::from)
        };
        let record_align = self.alignment_of(spec.attrs.aligned.as_ref());
        let mut members: Vec<Member> = Vec::with_capacity(fields.len());
        let mut anonymous = 0u32;
        let last = fields.len().saturating_sub(1);
        for (position, field) in fields.iter().enumerate() {
            // A flexible array member — `int data[];` as the last member —
            // is a `[T; 0]` tail the object is expected to be over-allocated
            // for. `int data[0];` is GNU's older spelling of the same thing
            // and needs nothing special: it already has a length.
            let flexible = matches!(
                field.ty.kind,
                ast::TypeKind::Array {
                    size: ast::ArraySize::Unspecified,
                    ..
                }
            );
            let ty = if flexible {
                match self.flexible_member_ty(field, kind, position == last) {
                    Some(ty) => ty,
                    None => continue,
                }
            } else {
                match self.ty_of(&field.ty) {
                    Some(ty) => ty,
                    None => continue,
                }
            };
            // A member's size is part of the record's layout, so it has to be
            // known when the tag is defined; C99 6.7.2.1p8 says the same thing
            // by requiring a complete type that is not variably modified.
            if self.types().is_vla(ty) {
                self.error(
                    field.range,
                    format!(
                        "a member of a {} cannot have a variably modified type",
                        kind.as_str()
                    ),
                );
                continue;
            }
            // Rust's `VaList` borrows the caller's frame; a member would have
            // to name that lifetime, and the record would stop being a plain
            // `#[repr(C)]` type.
            if self.reject_va_list(ty, field.range) {
                continue;
            }
            let requested = field
                .attrs
                .aligned
                .as_ref()
                .or(field.specifiers.alignas.as_ref());
            let align_request = self.alignment_of(requested);
            let align_range = requested.map(|a| a.range);
            // Only the member's *own* `packed` makes it one-byte aligned; a
            // `#pragma pack(N)` caps every member at N instead, which
            // `packing` says on its own.
            let packed = field.attrs.packed.is_some();

            if let Some(width) = &field.bit_width {
                if let Some(range) = align_range
                    && field.specifiers.alignas.is_some()
                {
                    // Which is what GCC says too: `_Alignas` may not be
                    // applied to a bit-field. Its own `aligned` attribute may,
                    // and moves the field to that boundary.
                    self.error(range, "'_Alignas' cannot be applied to a bit-field");
                }
                let Some(bits) = self.bit_field_width(field, ty, width) else {
                    continue;
                };
                let name = match &field.name {
                    Some(name) => {
                        if let Some(previous) = self.find_member(&members, &name.name) {
                            self.error_note(
                                name.range,
                                format!("duplicate member '{}'", name.name),
                                previous,
                                "previous declaration is",
                            );
                            continue;
                        }
                        Some(name.name.clone())
                    }
                    None => None,
                };
                let range = field.name.as_ref().map_or(field.range, |n| n.range);
                members.push(Member {
                    name,
                    anonymous: false,
                    ty,
                    is_const: field.ty.qualifiers.is_const,
                    bits: Some(bits),
                    align_request: field.attrs.aligned.as_ref().and(align_request),
                    packed,
                    flexible: false,
                    range,
                });
                continue;
            }

            let Some(name) = &field.name else {
                // An anonymous member (C11 6.7.2.1p13): its own members are
                // reached through the enclosing record, and the generated Rust
                // struct holds it under a synthetic name.
                let Ty::Record(inner) = ty else {
                    self.error(
                        field.range,
                        "a member declaration must declare a member; only a struct or \
                         union member may be unnamed",
                    );
                    continue;
                };
                if !self.types().record(inner).complete {
                    self.error(
                        field.range,
                        format!("anonymous member has incomplete type '{}'", self.tyname(ty)),
                    );
                    continue;
                }
                if let Some(clash) = self.first_clashing_name(&members, inner) {
                    self.error(
                        field.range,
                        format!(
                            "member '{clash}' of this anonymous member is already a member \
                             of the enclosing {}",
                            kind.as_str()
                        ),
                    );
                    continue;
                }
                members.push(Member {
                    name: Some(format!("__cinrs_anon{anonymous}")),
                    anonymous: true,
                    ty,
                    is_const: field.ty.qualifiers.is_const,
                    bits: None,
                    align_request,
                    packed,
                    flexible: false,
                    range: field.range,
                });
                anonymous += 1;
                continue;
            };
            if ty.is_void() || !self.types().is_complete(ty) {
                self.error(
                    field.range,
                    format!(
                        "member '{}' has incomplete type '{}'",
                        name.name,
                        self.tyname(ty)
                    ),
                );
                continue;
            }
            if let Some(previous) = self.find_member(&members, &name.name) {
                self.error_note(
                    name.range,
                    format!("duplicate member '{}'", name.name),
                    previous,
                    "previous declaration is",
                );
                continue;
            }
            members.push(Member {
                name: Some(name.name.clone()),
                anonymous: false,
                ty,
                is_const: field.ty.qualifiers.is_const,
                bits: None,
                align_request,
                packed,
                flexible,
                range: name.range,
            });
        }

        let flexible = members.last().is_some_and(|m| m.flexible);
        let mut laid_out = self.lay_out(kind, &members, packing);
        // `__attribute__((aligned(N)))` on the record raises its alignment,
        // and the size with it.
        if let Some(want) = record_align
            && want > laid_out.layout.align
        {
            laid_out.layout.align = want;
            laid_out.layout.size = round_up(laid_out.layout.size, want);
            laid_out.align_attr = Some(want);
        }
        // Rust refuses a packed type that transitively holds a
        // `#[repr(align)]` one, and C is perfectly happy to pack such a
        // member; the inner records swap the attribute for a zero-sized field
        // that says the same thing, which is not a `repr(align)` type.
        if laid_out.packed_attr.is_some() {
            for member in &members {
                self.demote_alignment(member.ty, spec.range);
            }
        }
        if laid_out.align_attr.is_some() && laid_out.packed_attr.is_some() {
            // Rust refuses `#[repr(C, packed, align(N))]` outright (`E0587`),
            // and there is no second way to say it.
            self.error(
                spec.range,
                "a record cannot be both packed and given a stricter alignment; Rust has \
                 no representation for that combination",
            );
        }
        self.check_atomic_members(&laid_out);
        let record = self.program.types.record_mut(id);
        record.fields = laid_out.fields;
        record.rust_fields = laid_out.rust_fields;
        record.complete = true;
        record.layout = Some(laid_out.layout);
        record.align = laid_out.align_attr;
        record.packed = laid_out.packed_attr;
        record.rust_align = laid_out.rust_align;
        record.flexible = flexible;
    }

    /// Refuses an `_Atomic` member that packing has left under-aligned.
    ///
    /// Every atomic operation here is `AtomicX::from_ptr` over the member's
    /// address, and that pointer has to be aligned for the atomic — which is
    /// the member's own size. `__attribute__((packed))` and `#pragma pack(N)`
    /// can put it anywhere; GCC answers such a member with a call into
    /// `libatomic`, which takes a lock, and there is nothing here that could.
    fn check_atomic_members(&mut self, laid_out: &LaidOut) {
        for field in &laid_out.fields {
            let Ty::Atomic(_) = field.ty else { continue };
            let Some(want) = self.types().size_align(field.ty, &self.target) else {
                continue;
            };
            if field.offset.is_multiple_of(want.align)
                && laid_out.layout.align.is_multiple_of(want.align)
            {
                continue;
            }
            self.error(
                field.range,
                format!(
                    "packing puts the '_Atomic' member '{}' at offset {} in a record aligned \
                     to {}, and an atomic operation on it needs {}-byte alignment",
                    field.name, field.offset, laid_out.layout.align, want.align
                ),
            );
        }
    }

    /// Honours `typedef struct { … } T __attribute__((aligned(N)));`.
    ///
    /// GCC makes the attribute a property of the *typedef*: `T` is a variant
    /// of the record whose alignment is stricter, while the record itself
    /// keeps its own. There is no room for such a variant in this type model,
    /// so the alignment is given to the record — which says exactly the same
    /// thing when the record is anonymous, since the typedef name is then the
    /// only way to name it at all. A tagged record is left alone: raising
    /// `struct S` because one typedef of it asked would change the layout
    /// everywhere the tag is used.
    pub(super) fn align_typedef_record(&mut self, ty: Ty, want: u64, range: SourceRange) {
        let Ty::Record(id) = ty else {
            return;
        };
        let def = self.types().record(id);
        let Some(layout) = def.layout else {
            return;
        };
        if def.tag.is_some() || !def.complete || want <= layout.align {
            return;
        }
        if def.packed.is_some() {
            // Rust refuses `#[repr(C, packed, align(N))]` (`E0587`), the same
            // way it does when both are written on the record itself.
            self.error(
                range,
                "a record cannot be both packed and given a stricter alignment; Rust has \
                 no representation for that combination",
            );
            return;
        }
        let record = self.program.types.record_mut(id);
        record.layout = Some(ir::Layout {
            size: round_up(layout.size, want),
            align: want,
        });
        record.align = Some(want);
        record.rust_align = want;
    }

    /// Replaces `#[repr(C, align(N))]` with a zero-sized field of that
    /// alignment, wherever a record reached from `ty` carries one.
    ///
    /// See [`ir::RustField::Align`] for why. Only a record inside a *packed*
    /// one is ever demoted, so the ordinary item keeps the attribute, which is
    /// both shorter and one field fewer for Rust code to write out.
    fn demote_alignment(&mut self, ty: Ty, range: SourceRange) {
        let id = match ty {
            Ty::Record(id) => id,
            Ty::Array(id) => {
                let elem = self.types().array_type(id).elem;
                return self.demote_alignment(elem, range);
            }
            _ => return,
        };
        let members: Vec<Ty> = self
            .types()
            .record(id)
            .fields
            .iter()
            .map(|field| field.ty)
            .collect();
        if let Some(align) = self.types().record(id).align {
            if align > MAX_MARKER_ALIGN {
                self.error(
                    range,
                    format!(
                        "a packed record cannot hold '{}', whose alignment of {align} needs                          '#[repr(align)]' — which Rust does not allow inside a packed type",
                        self.tyname(ty)
                    ),
                );
            } else {
                let name = format!("__cinrs_align{}", self.types().record(id).rust_fields.len());
                let record = self.program.types.record_mut(id);
                record.align = None;
                record
                    .rust_fields
                    .push(ir::RustField::Align { name, align });
            }
        }
        for member in members {
            self.demote_alignment(member, range);
        }
    }

    /// The type of a flexible array member, `int data[];`.
    ///
    /// It is an array of no elements: `sizeof` the record leaves it out, and
    /// indexing it is pointer arithmetic past the end of the object, which is
    /// what the C program is over-allocating for. C99 6.7.2.1p16 asks for it
    /// to be the last member of a `struct` with at least one other member;
    /// GCC is more relaxed, and so is this — only "last member" is required,
    /// because nothing else can be laid out at all.
    fn flexible_member_ty(
        &mut self,
        field: &ast::FieldDecl,
        kind: RecordKind,
        last: bool,
    ) -> Option<Ty> {
        let ast::TypeKind::Array { elem, .. } = &field.ty.kind else {
            unreachable!("the caller matched an array of unspecified size");
        };
        // C99 6.7.2.1p16; before that the idiom was `int data[1];` and a
        // deliberate over-allocation.
        self.require_standard(crate::Standard::C99, "a flexible array member", field.range);
        if !last {
            self.error(
                field.range,
                "a flexible array member must be the last member of the struct",
            );
            return None;
        }
        if kind == RecordKind::Union {
            self.error(
                field.range,
                "a flexible array member is not allowed in a union",
            );
            return None;
        }
        let element = self.ty_of(elem)?;
        if self.types().is_vla(element) {
            self.error(field.range, super::VM_UNSUPPORTED);
            return None;
        }
        if element.is_func() || !self.types().is_complete(element) {
            self.error(
                field.range,
                format!(
                    "a flexible array member has incomplete element type '{}'",
                    self.tyname(element)
                ),
            );
            return None;
        }
        Some(
            self.program
                .types
                .array(element, 0, elem.qualifiers.is_const),
        )
    }

    /// Checks the width of a bit-field, and works out whether reading it
    /// sign-extends.
    ///
    /// The diagnostics follow GCC's, which is what someone porting the code
    /// will have seen first.
    fn bit_field_width(
        &mut self,
        field: &ast::FieldDecl,
        ty: Ty,
        width: &ast::Expr,
    ) -> Option<(u32, bool)> {
        let named = field.name.as_ref().map(|n| n.name.clone());
        let what = match &named {
            Some(name) => format!("bit-field '{name}'"),
            None => "anonymous bit-field".to_owned(),
        };
        // Where a bit-field's bits sit inside its storage unit is entirely
        // implementation defined, and this one allocates from the least
        // significant end, which is what GCC and Clang do on a little-endian
        // machine and the opposite of what they do on a big-endian one. Rather
        // than lay one out the wrong way round — nothing in the generated Rust
        // would notice, and a `union` or a `memcpy` would read rubbish — a
        // big-endian target refuses the construct.
        if self.target.big_endian {
            self.error(
                field.range,
                format!(
                    "{what} is not supported on a big-endian target ({}): cinrs allocates \
                     bit-fields from the least significant end, which is not how a \
                     big-endian ABI lays them out",
                    self.target.arch.as_str()
                ),
            );
            return None;
        }
        if !ty.is_integer() {
            self.error(
                field.range,
                format!(
                    "{what} has invalid type '{}'; only the integer types may be given \
                     a width",
                    self.tyname(ty)
                ),
            );
            return None;
        }
        let value = self.expr(width)?;
        if !value.ty.is_integer() {
            self.error(
                width.range,
                format!(
                    "the width of {what} has non-integer type '{}'",
                    self.tyname(value.ty)
                ),
            );
            return None;
        }
        let Some(ir::ConstValue::Int(bits)) = self.const_eval(&value) else {
            self.error(
                width.range,
                format!("the width of {what} is not an integer constant expression"),
            );
            return None;
        };
        if bits < 0 {
            self.error(width.range, format!("negative width in {what}"));
            return None;
        }
        let limit = i128::from(ty.bits(&self.target));
        if bits > limit {
            let plural = if limit == 1 { "" } else { "s" };
            self.error(
                width.range,
                format!(
                    "width {bits} of {what} exceeds the {limit} bit{plural} of its type '{}'",
                    self.tyname(ty)
                ),
            );
            return None;
        }
        if bits == 0 && named.is_some() {
            self.error(
                width.range,
                format!("zero width for {what}; only an unnamed bit-field may be `: 0`"),
            );
            return None;
        }
        Some((bits as u32, self.bit_field_signed(&field.ty, ty)))
    }

    /// Whether reading a bit-field of this type sign-extends.
    ///
    /// It is the signedness of the declared type, except for an enumeration:
    /// there the implementation picks the underlying type, and GCC and Clang
    /// make it unsigned when no enumerator is negative — which is observable
    /// exactly here and nowhere else. `enum E { A, B, C, D } f : 2;` therefore
    /// holds `D`, where a *signed* two-bit field would read it back as `-1`.
    ///
    /// The answer is taken from the resolved type rather than from what was
    /// written, so that a `typedef` of the enumeration gets it too — which is
    /// how the C in gcc.c-torture's `execute/20030714-1` spells it.
    /// `self.enum_unsigned` is consulted first all the same: a member declared
    /// with an `enum` specifier that is being defined right here is answered
    /// there before the [`ir::EnumDef`] is complete.
    fn bit_field_signed(&self, written: &ast::Type, ty: Ty) -> bool {
        if let ast::TypeKind::Enum(id) = &written.kind
            && let Some(unsigned) = self.enum_unsigned[id.index()]
        {
            return !unsigned;
        }
        if let Ty::Enum(id) = ty {
            return !self.types().enum_def(id).unsigned;
        }
        ty.is_signed(&self.target)
    }

    /// Where a member of this name was declared, looking through anonymous
    /// members, if it is there at all.
    fn find_member(&self, members: &[Member], name: &str) -> Option<SourceRange> {
        for member in members {
            if !member.anonymous {
                if member.name.as_deref() == Some(name) {
                    return Some(member.range);
                }
                continue;
            }
            if let Ty::Record(inner) = member.ty
                && let Some(found) = self.find_field(&self.types().record(inner).fields, name)
            {
                return Some(found);
            }
        }
        None
    }

    /// The same question asked of a record that is already laid out.
    fn find_field(&self, fields: &[Field], name: &str) -> Option<SourceRange> {
        for field in fields {
            if !field.anonymous {
                if field.name == name {
                    return Some(field.range);
                }
                continue;
            }
            if let Ty::Record(inner) = field.ty
                && let Some(found) = self.find_field(&self.types().record(inner).fields, name)
            {
                return Some(found);
            }
        }
        None
    }

    /// The first member of an anonymous member that the enclosing record
    /// already has, if any.
    fn first_clashing_name(&self, members: &[Member], anonymous: RecordId) -> Option<String> {
        for field in &self.types().record(anonymous).fields {
            if field.anonymous {
                if let Ty::Record(inner) = field.ty
                    && let Some(found) = self.first_clashing_name(members, inner)
                {
                    return Some(found);
                }
                continue;
            }
            if self.find_member(members, &field.name).is_some() {
                return Some(field.name.clone());
            }
        }
        None
    }

    /// The alignment an `_Alignas` specifier asks for, reporting the operands
    /// that cannot be one.
    pub(super) fn alignment_of(&mut self, spec: Option<&ast::Alignment>) -> Option<u64> {
        let spec = spec?;
        let value = match &spec.kind {
            ast::AlignmentKind::Type(name) => {
                let ty = self.ty_of(&name.ty)?;
                self.types().size_align(ty, &self.target)?.align
            }
            ast::AlignmentKind::Expr(expr) => {
                let value = self.expr(expr)?;
                if !value.ty.is_integer() {
                    self.error(expr.range, "the alignment must be an integer constant");
                    return None;
                }
                match self.const_eval_at(&value, "the alignment")? {
                    ir::ConstValue::Int(v) if v >= 0 => u64::try_from(v).ok()?,
                    _ => {
                        self.error(expr.range, "the alignment must not be negative");
                        return None;
                    }
                }
            }
        };
        // `_Alignas(0)` is explicitly no alignment at all.
        if value == 0 {
            return None;
        }
        if !value.is_power_of_two() {
            self.error(
                spec.range,
                format!("the requested alignment {value} is not a power of two"),
            );
            return None;
        }
        Some(value)
    }

    /// Places the members and builds both views of the record: the C member
    /// list, and the fields of the Rust item that has to have the same layout.
    ///
    /// The two views are reconciled by *choosing the Rust fields*, not by
    /// hoping they agree: explicit `[u8; M]` padding goes wherever `#[repr(C)]`
    /// would otherwise put a field too early — before a bit-field run, before
    /// a member an `aligned(N)` moved along, after a trailing `:0` — and
    /// `#[repr(C, align(N))]` says an alignment no Rust field of the item
    /// carries. Packing goes the other way: `#[repr(C, packed(N))]` is exactly
    /// `#pragma pack(N)`, so the field alignments Rust uses are the ones the C
    /// layout used.
    ///
    /// `packing` is the maximum member alignment; see [`Packing`] for the two
    /// things it changes.
    fn lay_out(&self, kind: RecordKind, members: &[Member], packing: Packing) -> LaidOut {
        let target = &self.target;
        let mut align = 1u64;
        let mut raised: Option<u64> = None;

        // The alignment a member is really placed by: its own, capped by the
        // packing and then raised by whatever it asked for.
        let effective = |member: &Member, item: Layout| -> u64 {
            let mut want = if member.packed {
                1
            } else {
                match packing {
                    Some(max) => item.align.min(max),
                    None => item.align,
                }
            };
            if let Some(request) = member.align_request {
                want = want.max(request);
            }
            want.max(1)
        };

        // What the *generated Rust item* will have to say about itself, which
        // is decided before either pass: `#[repr(C, packed(N))]` is needed as
        // soon as one member field would otherwise be placed by an alignment
        // stricter than the layout used, whether that came from the record's
        // own packing or from a `packed` on the member alone.
        let uncapped = members
            .iter()
            .filter(|m| m.bits.is_none())
            .map(|m| self.rust_align_of(m.ty))
            .max()
            .unwrap_or(1);
        let member_packed = members
            .iter()
            .any(|m| m.packed && m.bits.is_none() && self.rust_align_of(m.ty) > 1);
        let rust_packing: Option<u64> = match packing {
            Some(max) if max < uncapped => Some(max),
            _ if member_packed => Some(1),
            _ => None,
        };
        let capped = |want: u64| match rust_packing {
            Some(max) => want.min(max),
            None => want,
        };

        // Pass one: where every member goes, and which storage run each
        // bit-field belongs to.
        let mut spots: Vec<Spot> = Vec::with_capacity(members.len());
        let mut runs: Vec<Run> = Vec::new();
        let mut run_of: Vec<usize> = vec![usize::MAX; members.len()];
        let mut off = 0u64;
        let mut union_bytes = 0u64;
        for (index, member) in members.iter().enumerate() {
            let item = self
                .types()
                .size_align(member.ty, target)
                .unwrap_or(Layout { size: 0, align: 1 });
            let want = effective(member, item);
            if kind == RecordKind::Union {
                off = 0;
            }
            let Some((width, _)) = member.bits else {
                align = align.max(want);
                if want > item.align {
                    raised = Some(raised.unwrap_or(1).max(want));
                }
                let offset = match kind {
                    RecordKind::Struct => round_up(off.div_ceil(8), want),
                    RecordKind::Union => 0,
                };
                spots.push(Spot::Byte(offset));
                match kind {
                    RecordKind::Struct => off = offset.saturating_add(item.size).saturating_mul(8),
                    RecordKind::Union => union_bytes = union_bytes.max(item.size),
                }
                continue;
            };
            // A bit-field never straddles a unit of its own type: if it would,
            // it starts at the next unit boundary instead. Width zero is the
            // request for that boundary and nothing else — and it asks for it
            // however the record is packed, which is the one part of the rule
            // packing does not switch off.
            let unit = item.size.saturating_mul(8).max(1);
            let w = u64::from(width);
            let unit_rule = packing.is_none() && !member.packed && member.align_request.is_none();
            if w == 0 {
                off = round_up(off, unit);
            } else if let Some(request) = member.align_request {
                off = round_up(off, request.saturating_mul(8));
            } else if unit_rule && off / unit != (off + w - 1) / unit {
                off = round_up(off, unit);
            }
            let (start, end) = (off, off + w);
            off = end;
            // Only a named bit-field makes the record stricter.
            if member.name.is_some() {
                align = align.max(want);
                if want > item.align {
                    raised = Some(raised.unwrap_or(1).max(want));
                }
            }
            spots.push(Spot::Bits { start });
            // In a union every member starts at bit zero, so no two of them
            // ever share a storage field.
            let extend =
                kind == RecordKind::Struct && runs.last().is_some_and(|run| run.last + 1 == index);
            if extend {
                let run = runs.last_mut().expect("just checked");
                run.end = end;
                run.last = index;
            } else {
                runs.push(Run {
                    start,
                    end,
                    last: index,
                });
            }
            run_of[index] = runs.len() - 1;
            if kind == RecordKind::Union {
                union_bytes = union_bytes.max(end.div_ceil(8));
            }
        }
        let bytes = match kind {
            RecordKind::Struct => off.div_ceil(8),
            RecordKind::Union => union_bytes,
        };
        let layout = Layout {
            size: round_up(bytes, align),
            align,
        };

        // Pass two: the C members, the Rust fields, and the padding that keeps
        // the two in step.
        let storage: Vec<Option<(String, u64, u64)>> = runs
            .iter()
            .scan(0u32, |next, run| {
                let (first, last) = (run.start / 8, run.end.div_ceil(8));
                Some((last > first).then(|| {
                    let name = format!("__cinrs_bits{next}");
                    *next += 1;
                    (name, first, last - first)
                }))
            })
            .collect();
        let mut fields: Vec<Field> = Vec::with_capacity(members.len());
        let mut rust_fields: Vec<RustField> = Vec::new();
        let mut emitted = vec![false; runs.len()];
        let mut natural = 1u64;
        let mut pos = 0u64;
        // The end of the widest member of a union, which is the size Rust
        // gives the item before any rounding.
        let mut widest = 0u64;
        let mut pads = 0u32;
        for (index, member) in members.iter().enumerate() {
            if kind == RecordKind::Union {
                widest = widest.max(pos);
                pos = 0;
            }
            match spots[index] {
                Spot::Byte(offset) => {
                    let item = self
                        .types()
                        .size_align(member.ty, target)
                        .unwrap_or(Layout { size: 0, align: 1 });
                    // The alignment Rust will place the field by: the one the
                    // *generated item* for its type really has, capped by what
                    // the record says about itself.
                    let rust_align = capped(self.rust_align_of(member.ty));
                    natural = natural.max(rust_align);
                    // `#[repr(C)]` inserts the padding an alignment calls for
                    // on its own; only a member C put *further* along than
                    // that — because `aligned(N)` moved it — needs a field of
                    // its own to get there.
                    if offset > round_up(pos, rust_align) {
                        rust_fields.push(RustField::Pad {
                            name: format!("__cinrs_pad{pads}"),
                            bytes: offset - pos,
                        });
                        pads += 1;
                    }
                    rust_fields.push(RustField::Member(fields.len()));
                    fields.push(Field {
                        name: member.name.clone().unwrap_or_default(),
                        anonymous: member.anonymous,
                        ty: member.ty,
                        is_const: member.is_const,
                        offset,
                        bits: None,
                        flexible: member.flexible,
                        range: member.range,
                    });
                    pos = offset.saturating_add(item.size);
                }
                Spot::Bits { start } => {
                    let run = run_of[index];
                    if let Some((name, first, bytes)) = &storage[run]
                        && !emitted[run]
                    {
                        emitted[run] = true;
                        if *first > pos {
                            rust_fields.push(RustField::Pad {
                                name: format!("__cinrs_pad{pads}"),
                                bytes: first - pos,
                            });
                            pads += 1;
                        }
                        rust_fields.push(RustField::Bits {
                            name: name.clone(),
                            offset: *first,
                            bytes: *bytes,
                        });
                        pos = first + bytes;
                    }
                    let Some(field_name) = &member.name else {
                        // An unnamed bit-field declares nothing; it has done
                        // its work by moving the offset along.
                        continue;
                    };
                    let (width, signed) = member.bits.expect("a bit-field");
                    let (storage_name, storage_offset) = match &storage[run] {
                        Some((name, first, _)) => (name.clone(), *first),
                        None => (String::new(), 0),
                    };
                    fields.push(Field {
                        name: field_name.clone(),
                        anonymous: false,
                        ty: member.ty,
                        is_const: member.is_const,
                        offset: start / 8,
                        bits: Some(BitField {
                            width,
                            bit_offset: start,
                            signed,
                            storage: storage_name,
                            storage_offset,
                            getter: String::new(),
                            setter: String::new(),
                        }),
                        flexible: false,
                        range: member.range,
                    });
                }
            }
        }
        // A trailing bit-field can push the record's size past its last Rust
        // field without leaving any storage behind — `struct { int a; long
        // long : 0; }` is four bytes of `a` and twelve of nothing — and a
        // packed record has no alignment left to round its own size up with.
        // Only an explicit field can make `#[repr(C)]` reproduce either.
        let want_end = if rust_packing.is_some() {
            layout.size
        } else {
            bytes
        };
        widest = widest.max(pos);
        // A packed item has no alignment left to round its own size up with,
        // so a union grows by a member of its own; every other case is Rust's
        // own rounding.
        let end = if kind == RecordKind::Struct {
            pos
        } else {
            widest
        };
        if want_end > end {
            // A union's members all start at zero, so the filler has to *be*
            // the size rather than make up the difference.
            let bytes = if kind == RecordKind::Struct {
                want_end - end
            } else {
                want_end
            };
            rust_fields.push(RustField::Pad {
                name: format!("__cinrs_pad{pads}"),
                bytes,
            });
        }
        name_accessors(&mut fields);
        // A record whose alignment comes from a bit-field's type — or from an
        // `aligned(N)` — has no Rust field that strict, so the item has to say
        // so itself. A packed one cannot: Rust refuses `packed` and `align`
        // together, so the generated item is left one byte aligned and
        // [`RecordDef::rust_align`] records that, which is what an enclosing
        // record's padding is then computed from.
        let align_attr = if rust_packing.is_some() {
            None
        } else if layout.align > natural {
            Some(layout.align)
        } else {
            raised
        };
        LaidOut {
            rust_align: align_attr.unwrap_or(natural),
            fields,
            rust_fields,
            align_attr,
            packed_attr: rust_packing,
            layout,
        }
    }

    /// The alignment the *generated Rust item* for a type really has.
    ///
    /// Usually the C alignment, and not always: a packed record's item cannot
    /// carry one, because Rust refuses `#[repr(C, packed)]` together with
    /// `align(N)`. Laying an enclosing record out has to know which of the two
    /// numbers Rust will use, or the padding it inserts would be computed from
    /// an offset the item does not have.
    fn rust_align_of(&self, ty: Ty) -> u64 {
        match ty {
            Ty::Record(id) => self.types().record(id).rust_align.max(1),
            Ty::Array(id) => self.rust_align_of(self.types().array_type(id).elem),
            // An `_Atomic T` member is a plain `T` field: C's alignment for it
            // is the size, which is stricter than what Rust gives the field
            // wherever the two differ, and the enclosing item then needs the
            // `#[repr(C, align(N))]` and the padding this number is what
            // decides.
            Ty::Atomic(id) => self.rust_align_of(self.types().atomic_inner(id)),
            other => self
                .types()
                .size_align(other, &self.target)
                .map_or(1, |layout| layout.align),
        }
    }

    // -- enum ---------------------------------------------------------------

    fn enum_ty(&mut self, spec_id: ast::EnumSpecId) -> Result<Ty, TypeError> {
        if let Some(ty) = self.enum_by_spec[spec_id.index()] {
            return Ok(ty);
        }
        let spec = self.enum_spec(spec_id);

        // C23's fixed underlying type. An enumeration with one *is* that
        // integer type: its enumerators have it, `sizeof` gives its size, and
        // the tag becomes an alias for it.
        let underlying = match &spec.underlying {
            Some(ty) => {
                // C23 6.7.2.2p5: the underlying type is the *unqualified,
                // non-atomic* version of the type written, so
                // `enum e : _Atomic(int)` is an `int` enumeration and not an
                // error (Clang's `C23/n3030_1`).
                let resolved = self.resolve_ty(ty)?;
                let resolved = self.types().unatomic(resolved);
                if !resolved.is_integer() || resolved.is_enum() {
                    return Err(TypeError::at(
                        ty.range,
                        format!(
                            "the underlying type of an enum must be an integer type, not '{}'",
                            self.tyname(resolved)
                        ),
                    ));
                }
                Some(resolved)
            }
            None => None,
        };

        let Some(enumerators) = &spec.enumerators else {
            let name = spec.name.as_ref().expect("the parser requires a tag here");
            return match self.lookup_tag(&name.name) {
                Some(TagEntry::Enum { ty, unsigned, .. }) => {
                    self.enum_by_spec[spec_id.index()] = Some(ty);
                    self.enum_unsigned[spec_id.index()] = Some(unsigned);
                    Ok(ty)
                }
                Some(TagEntry::Record(_)) => Err(TypeError::at(
                    spec.range,
                    format!(
                        "'{}' is already declared as a struct or union tag",
                        name.name
                    ),
                )),
                // C23 allows an enum with a fixed underlying type to be
                // declared before it is defined, and GNU allows *any*
                // enumeration to be — `enum e; enum e *p;` is common in code
                // that only passes the values around. Until the list is seen
                // the type is `int`, which is what an enumeration compiles to
                // here anyway; the tag then keeps that type when it is
                // completed rather than gaining an alias of its own, so that
                // the two mentions never disagree.
                None => {
                    let ty = underlying.unwrap_or(Ty::Int);
                    let unsigned = !ty.is_signed(&self.target);
                    self.insert_tag(
                        &name.name,
                        TagEntry::Enum {
                            ty,
                            unsigned,
                            complete: false,
                        },
                    );
                    self.enum_by_spec[spec_id.index()] = Some(ty);
                    self.enum_unsigned[spec_id.index()] = Some(unsigned);
                    Ok(ty)
                }
            };
        };

        // An enumeration this scope has only *declared* is completed here; one
        // it defined is a redefinition.
        let mut declared: Option<Ty> = None;
        if let Some(name) = &spec.name {
            match self.tag_here(&name.name) {
                Some(TagEntry::Enum { complete: true, .. }) => {
                    return Err(TypeError::at(
                        spec.range,
                        format!("redefinition of 'enum {}'", name.name),
                    ));
                }
                Some(TagEntry::Enum { ty, .. }) => declared = Some(ty),
                _ => {}
            }
        }

        // Only a file-scope `enum` becomes a named alias; one declared inside a
        // block would need a mangled name nobody could use, and `int` is what
        // its values are anyway.
        let file_scope = self.at_file_scope();
        let tag_name = spec.name.as_ref().map(|n| n.name.clone());
        // Not `let`: an enumerator too wide for `int` widens the whole
        // enumeration below (C23 6.7.2.2p13), and the type goes with it.
        let mut ty = match (file_scope, tag_name, underlying) {
            // The tag was declared incomplete first, so it already has a type
            // and every earlier mention of it used that one.
            _ if declared.is_some() => declared.expect("just checked"),
            // A fixed underlying type is the enumeration's type; the tag is
            // an alias for it rather than an `enum` item of its own.
            (true, Some(tag), Some(fixed)) => {
                let rust_name = if self.try_reserve_item_name(&tag) {
                    tag
                } else {
                    self.reserve_item_name(&format!("enum_{tag}"))
                };
                self.program.typedefs.push(ir::TypedefItem {
                    rust_name,
                    ty: fixed,
                    range: spec.range,
                });
                fixed
            }
            (_, _, Some(fixed)) => fixed,
            (true, Some(tag), None) => {
                let rust_name = if self.try_reserve_item_name(&tag) {
                    tag.clone()
                } else {
                    self.reserve_item_name(&format!("enum_{tag}"))
                };
                let id = self.program.types.add_enum(EnumDef {
                    unsigned: false,
                    tag: Some(tag),
                    rust_name,
                    anonymous: false,
                    emit: true,
                    range: spec.range,
                });
                Ty::Enum(id)
            }
            _ => Ty::Int,
        };
        if let Some(name) = &spec.name {
            self.insert_tag(
                &name.name,
                TagEntry::Enum {
                    ty,
                    unsigned: false,
                    complete: true,
                },
            );
        }
        self.enum_by_spec[spec_id.index()] = Some(ty);
        // With a fixed underlying type the enumerators have the enumeration's
        // own type; without one they are `int`, as C99 says — until one of
        // them will not fit, which is what C23 changed.
        let mut constant_ty = underlying.unwrap_or(Ty::Int);

        // C99 6.7.2.2: an enumerator without a value is one more than the
        // previous one, and the first is zero.
        let mut next = 0i128;
        // Which of `int` and `unsigned int` the implementation makes the
        // underlying type. GCC and Clang pick the unsigned one whenever no
        // enumerator is negative, and that choice is visible through a
        // bit-field of the type; see `Sema::bit_field_signed`.
        let mut unsigned = underlying.is_none_or(|fixed| !fixed.is_signed(&self.target));
        // C23 6.7.2.2p13 (N3029): an enumerator whose value will not fit the
        // enumeration's type *widens the enumeration*, and every enumerator
        // then has the widened type. Before C23 it was a constraint violation,
        // which the strict entry points keep; GCC and Clang have accepted it
        // for ever with a warning, so the GNU dialects widen as well. A
        // *fixed* underlying type is not widened — the value has to fit the
        // type the program wrote — and neither is a tag this scope had already
        // declared incomplete, whose earlier mentions used the type it had.
        let may_widen = underlying.is_none()
            && declared.is_none()
            && (self.gnu_leniency() || self.gating.standard >= crate::Standard::C23);
        let (mut lo, mut hi) = (0i128, 0i128);
        // Every enumerator, so that a widening one can retype the ones already
        // placed: `enum x { a = INT_MAX, b = ULLONG_MAX }` gives `a` the
        // enumeration's type too, which is what `_Generic(a)` selects on.
        let mut placed: Vec<(String, i128, SourceRange, Option<usize>)> = Vec::new();
        for enumerator in enumerators {
            let value = match &enumerator.value {
                Some(expr) => match self.expr(expr) {
                    Some(value) if !value.ty.is_integer() => {
                        self.error(
                            expr.range,
                            format!(
                                "enumerator value has non-integer type '{}'",
                                self.tyname(value.ty)
                            ),
                        );
                        next
                    }
                    Some(value) => match self.const_eval_at(&value, "enumerator value") {
                        Some(ir::ConstValue::Int(v)) => v,
                        _ => next,
                    },
                    None => next,
                },
                None => next,
            };
            if !constant_ty.can_represent(value, &self.target) {
                match may_widen
                    .then(|| widened_enum_ty(lo.min(value), hi.max(value), &self.target))
                    .flatten()
                {
                    Some(wider) => constant_ty = wider,
                    None => self.error(
                        enumerator.range,
                        format!(
                            "enumerator value {value} is outside the range of '{}'",
                            self.tyname(constant_ty)
                        ),
                    ),
                }
            }
            let value = constant_ty.wrap(value, &self.target);
            lo = lo.min(value);
            hi = hi.max(value);
            unsigned = unsigned && value >= 0;
            next = value.wrapping_add(1);
            self.check_redefinition(&enumerator.name);
            self.insert(
                &enumerator.name.name,
                Entry::Constant {
                    value: ir::ConstValue::Int(value),
                    ty: constant_ty,
                    range: enumerator.name.range,
                },
            );
            let at = if file_scope {
                let rust_name = self.reserve_item_name(&enumerator.name.name);
                self.program.enum_constants.push(ir::Enumerator {
                    name: enumerator.name.name.clone(),
                    rust_name,
                    ty: constant_ty,
                    value,
                    range: enumerator.name.range,
                });
                Some(self.program.enum_constants.len() - 1)
            } else {
                None
            };
            placed.push((
                enumerator.name.name.clone(),
                value,
                enumerator.name.range,
                at,
            ));
        }
        // The enumeration widened, so every enumerator has the widened type
        // and so does the enumeration itself. It stops being a `Ty::Enum` at
        // that point: `Ty::Enum` *is* `int` everywhere in this crate's type
        // model, and the honest answer for an enumeration whose underlying
        // type C23 made implementation-defined is the integer type it widened
        // to. The tag keeps its Rust alias, now an alias for that type.
        if constant_ty != underlying.unwrap_or(Ty::Int) {
            for (name, value, range, at) in &placed {
                self.insert(
                    name,
                    Entry::Constant {
                        value: ir::ConstValue::Int(*value),
                        ty: constant_ty,
                        range: *range,
                    },
                );
                if let Some(at) = at {
                    self.program.enum_constants[*at].ty = constant_ty;
                }
            }
            if let Ty::Enum(id) = ty {
                let def = self.program.types.enum_mut(id);
                def.emit = false;
                let rust_name = def.rust_name.clone();
                self.program.typedefs.push(ir::TypedefItem {
                    rust_name,
                    ty: constant_ty,
                    range: spec.range,
                });
            }
            ty = constant_ty;
            self.enum_by_spec[spec_id.index()] = Some(ty);
        }
        if let Some(name) = &spec.name {
            self.insert_tag(
                &name.name,
                TagEntry::Enum {
                    ty,
                    unsigned,
                    complete: true,
                },
            );
        }
        if let Ty::Enum(id) = ty {
            self.program.types.enum_mut(id).unsigned = unsigned;
        }
        self.enum_unsigned[spec_id.index()] = Some(unsigned);
        Ok(ty)
    }
}

/// The type C23 6.7.2.2p13 widens an enumeration to so that every value from
/// `lo` to `hi` fits, or `None` when no integer type this crate has does.
///
/// The order is the one GCC and Clang pick from: the narrowest type that holds
/// the whole range, preferring the signed one at each width. C23 leaves the
/// choice implementation-defined and only asks that it hold every value.
fn widened_enum_ty(lo: i128, hi: i128, target: &crate::TargetModel) -> Option<Ty> {
    [
        Ty::Int,
        Ty::UInt,
        Ty::Long,
        Ty::ULong,
        Ty::LongLong,
        Ty::ULongLong,
    ]
    .into_iter()
    .find(|ty| ty.can_represent(lo, target) && ty.can_represent(hi, target))
}

/// Gives every bit-field of a record the pair of accessor names it is
/// generated under.
///
/// The getter is the member's own name and the setter is `set_` in front of
/// it; a member whose name Rust cannot spell — `self`, `crate`, … — gets the
/// underscore [`ir::rust_name_of`] appends, and a keyword becomes a raw
/// identifier, which needs no help. What is left is a collision between two
/// names that were distinct in C: a member `x` next to a member `set_x` wants
/// `set_x` twice. Every *getter* is claimed first, in declaration order, so a
/// member's own name always reads it; the setter that then finds its name
/// taken grows `_2`, `_3`, … — the same shape everything else in this crate is
/// disambiguated with.
fn name_accessors(fields: &mut [Field]) {
    let mut used: HashSet<String> = HashSet::new();
    let take = |used: &mut HashSet<String>, base: String| -> String {
        if used.insert(ir::rust_name_of(&base)) {
            return base;
        }
        (2u32..)
            .map(|n| format!("{base}_{n}"))
            .find(|candidate| used.insert(ir::rust_name_of(candidate)))
            .expect("the sequence of candidates is unbounded")
    };
    for field in fields.iter_mut() {
        let name = field.name.clone();
        if let Some(bits) = &mut field.bits {
            bits.getter = take(&mut used, name);
        }
    }
    for field in fields {
        let name = field.name.clone();
        if let Some(bits) = &mut field.bits {
            bits.setter = take(&mut used, format!("set_{name}"));
        }
    }
}

/// The strictest alignment a zero-sized marker field can carry.
///
/// `u64` is the widest integer whose Rust alignment is its size on every
/// target this crate supports; `u128` is 16-byte aligned on x86-64 and 8-byte
/// aligned elsewhere, which is exactly the kind of difference a layout must
/// not depend on.
const MAX_MARKER_ALIGN: u64 = 8;

/// Rounds `value` up to a multiple of `align`.
fn round_up(value: u64, align: u64) -> u64 {
    if align <= 1 {
        return value;
    }
    value.div_ceil(align).saturating_mul(align)
}
