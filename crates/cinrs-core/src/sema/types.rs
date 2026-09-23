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

use crate::COMPLEX_UNSUPPORTED;
use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{
    self, BitField, EnumDef, Field, Layout, ObjectId, RecordDef, RecordId, RecordKind, RustField,
    Stmt, Ty,
};

use super::{BoundMode, Entry, Sema, TagEntry, TypeError};

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
    /// [`Sema::vm_bounds`] and, where the context gives it one, in the hidden
    /// object named here.
    Variable(Option<ObjectId>),
    /// No bound at all — `int j[]`, an incomplete array type (6.2.5p22).
    Unspecified,
}

/// How complete the type of a declared object has to be where it is written.
///
/// C11 6.7p7 asks for a complete type only for an identifier "declared with no
/// linkage"; the two places an incomplete one may stand are a file-scope
/// *tentative* definition, which the end of the translation unit completes to
/// one element when it is still an incomplete array (6.9.2p5), and an `extern`
/// declaration, whose object is defined in another unit and whose size is
/// therefore none of this one's business (6.2.5p22). WG14 DR047 is the second
/// of those (`extern struct incomplete es1;`, `drs/dr0xx.c`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Completeness {
    /// The type has to be complete here.
    Required,
    /// `T x[];` is allowed, and nothing else incomplete is.
    TentativeArray,
    /// Any incomplete type is allowed.
    Any,
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
        let mut resolved = self.resolve_unqualified_ty(ty)?;
        // C99 6.7.3p9 qualifies the *elements* of an array, not the array. The
        // array declarator spelling has already done that — the `const` of
        // `const int a[1]` is written on `int` — but a `typedef` or a `typeof`
        // that names the array whole has the qualifier out here, and it has to
        // go the same place, or `&a` is not the `const int (*)[1]` the
        // standard says it is.
        if ty.qualifiers.is_const {
            resolved = self.program.types.const_elements(resolved);
        }
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
            // `long double _Complex` follows `long double` onto `double`, with
            // the same documented loss of precision and the same ABI caveat.
            ast::TypeKind::Complex(size) => {
                if !self.complex {
                    return Err(TypeError::at(range, COMPLEX_UNSUPPORTED.to_owned()));
                }
                Ok(match size {
                    ast::FloatSize::Float => Ty::ComplexFloat,
                    _ => Ty::ComplexDouble,
                })
            }
            // GCC has no `_Imaginary` either: no compiler implements the
            // imaginary types, and C99 6.7.2p2 leaves them optional.
            ast::TypeKind::Imaginary(_) => Err(TypeError::at(
                range,
                "imaginary types are not supported; no compiler implements '_Imaginary', and \
                 C99 makes it optional. Write the '_Complex' type instead",
            )),
            ast::TypeKind::Pointer(inner) => {
                let pointee = self.resolve_ty(inner)?;
                // `int (*p)[n]` is a pointer to a variably modified type: the
                // arithmetic on it scales by a run-time size, which the
                // pointee type carries. See [`ir::Types::vm_dims`].
                //
                // `va_list *` is `*mut core::ffi::VaList<'_>`: a raw pointer
                // may hold the lifetime a member or a `static` could not name,
                // so the type itself is fine wherever the *object* holding it
                // is a local or a parameter. `Sema::reject_va_list` is what
                // keeps it out of the other places.
                // A pointer to a `typedef` that is under- or over-aligned
                // (`const xxh_unalign64 *`) says so in the pointer type, which
                // is what makes `*p` an unaligned access.
                if let Some(align) = self.typedef_align(inner) {
                    let natural = self
                        .types()
                        .size_align(pointee, &self.target)
                        .map_or(1, |layout| layout.align);
                    if align != natural {
                        return Ok(self.program.types.pointer_aligned(
                            pointee,
                            inner.qualifiers.is_const,
                            Some(align),
                        ));
                    }
                }
                Ok(self.ptr_to(pointee, inner.qualifiers.is_const))
            }
            ast::TypeKind::Array { elem, size, .. } => {
                let element = self.resolve_ty(elem)?;
                self.check_element_type(element, elem.range)?;
                let konst = elem.qualifiers.is_const;
                Ok(match self.array_len(size, range)? {
                    ArrayLen::Fixed(len) => {
                        self.check_array_size(element, len, range)?;
                        self.program.types.array(element, len, konst)
                    }
                    ArrayLen::Variable(len) => self.program.types.vla_array(element, konst, len),
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

    /// Resolves the type of a *declaration*: one that gives every variable
    /// bound in it a hidden object of its own, evaluated where the declarator
    /// stands.
    ///
    /// [`Sema::take_vm_bounds`] is the other half — the statements that bind
    /// those objects, which the declaration has to put in front of whatever it
    /// generates.
    pub(super) fn resolve_declared_ty(
        &mut self,
        ty: &ast::Type,
        name: &str,
    ) -> Result<Ty, TypeError> {
        self.vm_bounds.clear();
        let outer = (
            std::mem::replace(&mut self.bound_mode, BoundMode::Object),
            std::mem::replace(&mut self.vm_name, name.to_owned()),
        );
        let resolved = self.resolve_ty(ty);
        (self.bound_mode, self.vm_name) = outer;
        resolved
    }

    /// The `let`s that bind the hidden bound objects the last
    /// [declared](Sema::resolve_declared_ty) type left behind.
    ///
    /// They come out in the order they were resolved — innermost dimension
    /// first, which is the order GCC evaluates them in — and each is
    /// `explicit`, so that a [hoisted](crate::cfg) definition still assigns
    /// where the declaration was written.
    pub(super) fn take_vm_bounds(&mut self) -> Vec<Stmt> {
        std::mem::take(&mut self.vm_bounds)
            .into_iter()
            .filter_map(|bound| {
                Some(Stmt::Let {
                    object: bound.object?,
                    init: bound.value,
                    explicit: true,
                })
            })
            .collect()
    }

    /// Resolves the type of a parameter, applying the adjustments C makes to
    /// one: an array parameter is a pointer, and a function parameter is a
    /// pointer to a function.
    pub(super) fn resolve_param_ty(&mut self, ty: &ast::Type) -> Result<Ty, TypeError> {
        let outer = (
            std::mem::replace(&mut self.in_param_type, true),
            // A declaration that is not a definition never evaluates a bound;
            // the *definition* re-resolves the same declarator on entry, with
            // objects to keep the lengths in. See
            // [`Sema::parameter_size_effects`].
            std::mem::replace(&mut self.bound_mode, BoundMode::Unevaluated),
        );
        let resolved = self.resolve_param_ty_inner(ty);
        (self.in_param_type, self.bound_mode) = outer;
        resolved
    }

    /// Resolves a parameter's type where the bounds *are* evaluated: the
    /// definition's own prologue (C99 6.9.1p10).
    pub(super) fn resolve_param_ty_declared(
        &mut self,
        ty: &ast::Type,
        name: &str,
    ) -> Result<Ty, TypeError> {
        self.vm_bounds.clear();
        let outer = (
            std::mem::replace(&mut self.in_param_type, true),
            std::mem::replace(&mut self.bound_mode, BoundMode::Object),
            std::mem::replace(&mut self.vm_name, name.to_owned()),
        );
        let resolved = self.resolve_param_ty_inner(ty);
        (self.in_param_type, self.bound_mode, self.vm_name) = outer;
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
            //
            // The *element* type is part of it: `double a[n][m]` is a pointer
            // to a variably modified `double[m]`, whose bound a definition
            // gives an object of its own on entry (C99 6.9.1p10).
            let element = self.resolve_ty(elem)?;
            // The adjustment to a pointer does not excuse the element type:
            // C11 6.7.6.2p1's "the element type shall not be an incomplete or
            // function type" is a constraint on the declarator as written, and
            // both GCC and Clang answer `struct incomplete a[]` with an error
            // (WG14 DR047, `drs/dr0xx.c`).
            self.check_element_type(element, elem.range)?;
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
    /// and converted to `size_t` here and left in [`Sema::vm_bounds`] for
    /// whoever is resolving the type; what becomes of it there is
    /// [`BoundMode`]'s business. A declaration also gives the dimension a
    /// hidden object of its own, which is what the *type* carries — so that
    /// `sizeof`, indexing and pointer arithmetic can find the length again
    /// wherever the type turns up later.
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
            // the parameter is a pointer and the bound never mattered.
            ast::ArraySize::Star => {
                if self.bound_mode == BoundMode::Unevaluated {
                    return Ok(ArrayLen::Variable(None));
                }
                return Err(TypeError::at(
                    range,
                    "'[*]' is only allowed in a function prototype",
                ));
            }
            ast::ArraySize::Expr(expr) => expr,
        };
        // The bound is an expression, and an expression may name a type of its
        // own — `int a[sizeof(int[k])]`. Resolving *that* one declares
        // nothing, so the mode and the name the hidden objects are derived
        // from are put aside while it is checked.
        let outer = (
            std::mem::replace(&mut self.bound_mode, BoundMode::Expression),
            std::mem::take(&mut self.vm_name),
            std::mem::replace(&mut self.in_param_type, false),
        );
        let value = self.expr(expr);
        (self.bound_mode, self.vm_name, self.in_param_type) = outer;
        let Some(value) = value else {
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
            // A bound that is not constant is a variable length array inside a
            // function, and simply invalid at file scope, where there is no
            // moment at which the bound could be evaluated. Inside a
            // parameter's type it is allowed to be anything, prototype at file
            // scope or not (6.7.5.3p7).
            if self.at_file_scope() && self.bound_mode != BoundMode::Unevaluated {
                return Err(TypeError::at(
                    expr.range,
                    "array size is not an integer constant expression",
                ));
            }
            let size_ty = self.size_ty();
            let value = self.convert(value, size_ty);
            let object = (self.bound_mode == BoundMode::Object).then(|| {
                let name = match self.vm_bounds.len() {
                    0 => format!("__cinrs_vla_len_{}", self.vm_name),
                    n => format!("__cinrs_vla_len{n}_{}", self.vm_name),
                };
                let object =
                    self.new_object(&name, size_ty, ir::Storage::Automatic, false, expr.range);
                // Everything from here to the end of the block is inside the
                // scope of an identifier with a variably modified type, which
                // nothing may jump into (C99 6.8.6.1p1): the bound would not
                // have been evaluated. It is the *bound* that is pushed rather
                // than the object being declared, because `int (*p)[n]` and
                // `typedef int T[n]` have one too.
                self.vla_scopes.push(object);
                object
            });
            if self.bound_mode != BoundMode::Unevaluated {
                self.vm_bounds.push(super::VmBound { object, value });
            }
            return Ok(ArrayLen::Variable(object));
        };
        if len < 0 {
            return Err(TypeError::at(expr.range, "array size is negative"));
        }
        u64::try_from(len)
            .map(ArrayLen::Fixed)
            .map_err(|_| TypeError::at(expr.range, "array size is too large"))
    }

    /// `__attribute__((mode(M)))`, which replaces the declared type.
    ///
    /// GCC's machine modes name a *width* rather than a type, and the
    /// declaration says the rest: `typedef unsigned int u8
    /// __attribute__((mode(QI)))` is the unsigned integer one byte wide, and
    /// `typedef int word __attribute__((mode(word)))` the signed one as wide
    /// as a machine word. So the width comes from the mode, the signedness
    /// from the type that was written, and the answer is whichever of this
    /// model's types has both.
    ///
    /// Only the modes that name a type this crate has are accepted. The
    /// floating ones are `SF` and `DF`; `XF` and `TF` are the extended and
    /// quad formats, the `V…` ones are vectors and the `…C` ones complex, and
    /// each of those is refused with the reason rather than rounded to
    /// something else.
    pub(super) fn apply_mode(&mut self, ty: Ty, attrs: &ast::Attributes) -> Ty {
        let Some(mode) = &attrs.mode else {
            return ty;
        };
        let range = mode.range;
        let spelling = mode.node.clone();
        let name = spelling
            .strip_prefix("__")
            .and_then(|rest| rest.strip_suffix("__"))
            .unwrap_or(&spelling);
        if ty.is_error() {
            return ty;
        }
        if !ty.is_arithmetic() {
            self.error(
                range,
                format!(
                    "'mode' applies to an arithmetic type, and '{}' is not one",
                    self.tyname(ty)
                ),
            );
            return ty;
        }
        match name {
            "SF" => return Ty::Float,
            "DF" => return Ty::Double,
            "XF" | "TF" | "KF" | "IF" | "HF" | "BF" => {
                self.error(
                    range,
                    format!(
                        "'mode({name})' names a floating format with no stable Rust type; \
                         `f16` and `f128` are unstable and x87's extended double has no \
                         Rust counterpart"
                    ),
                );
                return ty;
            }
            // The complex machine modes. `SC` and `DC` are `float _Complex`
            // and `double _Complex`; the wider ones name formats this
            // implementation does not have, exactly as `XF` and `TF` do.
            "SC" | "DC" if self.complex => {
                return if name == "SC" {
                    Ty::ComplexFloat
                } else {
                    Ty::ComplexDouble
                };
            }
            "SC" | "DC" => {
                self.error(range, COMPLEX_UNSUPPORTED.to_owned());
                return ty;
            }
            "XC" | "TC" | "KC" | "HC" => {
                self.error(
                    range,
                    format!(
                        "'mode({name})' names a complex type whose parts have a floating \
                         format with no stable Rust type; write 'double _Complex'"
                    ),
                );
                return ty;
            }
            _ if name.starts_with('V') && name[1..].starts_with(|c: char| c.is_ascii_digit()) => {
                self.error(
                    range,
                    format!(
                        "'mode({name})' names a vector type: the vector extensions need \
                         `core::simd`, which is unstable"
                    ),
                );
                return ty;
            }
            _ => {}
        }
        let word = Ty::size_ty(&self.target).size_bytes(&self.target);
        let bytes: u64 = match name {
            "QI" | "byte" => 1,
            "HI" => 2,
            "SI" => 4,
            "DI" => 8,
            "TI" => 16,
            "word" | "pointer" | "unwind_word" => word,
            _ => {
                self.error(range, format!("unknown machine mode '{spelling}'"));
                return ty;
            }
        };
        if bytes == 16 && !self.target.has_int128 {
            self.error(
                range,
                "'mode(TI)' asks for a 128-bit integer, which this target model does not \
                 have; see the data model in `doc/c-status.md`",
            );
            return ty;
        }
        let signed = ty.is_signed(&self.target);
        let candidates: &[Ty] = if signed {
            &[
                Ty::SChar,
                Ty::Short,
                Ty::Int,
                Ty::Long,
                Ty::LongLong,
                Ty::Int128,
            ]
        } else {
            &[
                Ty::UChar,
                Ty::UShort,
                Ty::UInt,
                Ty::ULong,
                Ty::ULongLong,
                Ty::UInt128,
            ]
        };
        let target = self.target;
        match candidates
            .iter()
            .copied()
            .find(|candidate| candidate.size_bytes(&target) == bytes)
        {
            Some(found) => found,
            None => {
                self.error(
                    range,
                    format!("no integer type of this target model is {bytes} bytes wide"),
                );
                ty
            }
        }
    }

    /// Rejects an array whose elements will not fit in the address space.
    ///
    /// C leaves the maximum size of an object implementation-defined, and what
    /// every implementation can actually say is `size_t`: `sizeof` has that
    /// type, so an object whose size does not fit it has no size at all. GCC
    /// ("size of array 'a' is too large") and Clang ("array is too large (N
    /// elements)") both refuse it, and `drs/dr2xx.c` writes
    /// `sizeof(int[SIZE_MAX/2][SIZE_MAX/2])` to check that they do.
    fn check_array_size(
        &mut self,
        elem: Ty,
        len: u64,
        range: SourceRange,
    ) -> Result<(), TypeError> {
        let stride = self
            .types()
            .size_align(elem, &self.target)
            .map_or(1, |layout| layout.size)
            .max(1);
        let limit = u64::MAX >> (64 - self.target.ptr_bits.min(64));
        if len > limit / stride {
            return Err(TypeError::at(
                range,
                format!("array is too large ({len} elements)"),
            ));
        }
        Ok(())
    }

    /// Rejects the element types an array cannot have.
    fn check_element_type(&mut self, elem: Ty, range: SourceRange) -> Result<(), TypeError> {
        if elem.is_func() {
            return Err(TypeError::at(range, "an array of functions is not allowed"));
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
                self.report_type_error(err);
                None
            }
        }
    }

    /// Reports what a type could not be resolved for, unless the reason has
    /// already been reported where it was found.
    pub(super) fn report_type_error(&mut self, err: TypeError) {
        if err.message.is_empty() {
            return;
        }
        match err.note {
            Some((range, note)) => self.error_note(err.range, err.message, range, note),
            None => self.error(err.range, err.message),
        }
    }

    /// Resolves a type that must name an object type, with `completeness`
    /// saying how complete it has to be here.
    pub(super) fn declared_object_ty_of(
        &mut self,
        ty: &ast::Type,
        name: &str,
        completeness: Completeness,
    ) -> Option<Ty> {
        let resolved = match self.resolve_declared_ty(ty, name) {
            Ok(resolved) => resolved,
            Err(err) => {
                self.report_type_error(err);
                return None;
            }
        };
        if resolved.is_void() {
            self.error(
                ty.range,
                format!("variable '{name}' has incomplete type 'void'"),
            );
            return None;
        }
        if completeness == Completeness::Any {
            return Some(resolved);
        }
        if completeness == Completeness::TentativeArray
            && self.types().is_incomplete_array(resolved)
        {
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
                let tag = spec
                    .name
                    .as_ref()
                    .expect("matched a named tag")
                    .name
                    .clone();
                // C23 6.7.2.3p1, as WG14 N3037 rewrote it: a second definition
                // of a tag in the same scope declares the *same type* when its
                // member list is compatible with the first's, and is the
                // constraint violation it always was when it is not. Only the
                // C23 entry points have it, which is where GCC 15 put it too —
                // `-std=c23` and `-std=gnu23`, not the GNU dialects of the
                // older revisions.
                let difference = if self.gating.standard >= crate::Standard::C23 {
                    self.compare_redefinition(id, spec_id, spec, fields)
                } else {
                    Some(String::new())
                };
                let Some(difference) = difference else {
                    // One type, defined twice: the first definition's, which
                    // every earlier use already names.
                    self.record_by_spec[spec_id.index()] = Some(id);
                    return Ok(Ty::Record(id));
                };
                let previous = self.types().record(id).range;
                let message = if difference.is_empty() {
                    format!("redefinition of '{} {tag}'", kind.as_str())
                } else {
                    format!(
                        "redefinition of '{} {tag}' with an incompatible member list: \
                         {difference}",
                        kind.as_str()
                    )
                };
                return Err(TypeError {
                    range: spec.range,
                    message,
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

    /// Resolves a second definition of a tag already defined in this scope and
    /// says whether it declares a *different* type (C23 6.2.7p1, N3037).
    ///
    /// `None` means the two definitions declare one type, which is what C23
    /// made them; `Some(difference)` names what makes them two, for the
    /// diagnostic. The second member list has to be resolved to be compared —
    /// `int` and a `typedef` of it are the same member, and two spellings of
    /// one pointer are the same member — so it is resolved into a tag of its
    /// own, which nothing then refers to and no item is generated for.
    fn compare_redefinition(
        &mut self,
        first: RecordId,
        spec_id: ast::RecordSpecId,
        spec: &ast::RecordSpec,
        fields: &[ast::FieldDecl],
    ) -> Option<String> {
        let kind = self.types().record(first).kind;
        let mark = self.program.types.records().len();
        let again =
            self.declare_record(kind, spec.name.as_ref().map(|n| n.name.clone()), spec.range);
        // The tag itself still means the first definition inside this list, so
        // `struct S { struct S *next; };` written twice names one type in both
        // and the two members compare equal.
        self.record_by_spec[spec_id.index()] = Some(first);
        self.define_record(again, spec, fields);
        for assert in &spec.asserts {
            self.static_assert(assert);
        }
        let difference = self.record_difference(first, again, &mut Vec::new());
        if difference.is_none() {
            // The two agree, so everything this resolution created is a
            // duplicate of something the first definition already generated —
            // the tag itself, and the type of any anonymous member of it.
            // Nothing refers to any of it, and no item is generated for it.
            self.program.types.suppress_records_from(mark);
        }
        difference
    }

    /// What makes two definitions of one tag two types, or `None` when they
    /// are one (C23 6.2.7p1).
    ///
    /// The rule is a one-to-one correspondence between the members in which
    /// each pair has the same name, compatible types, the same bit-field width
    /// and the same alignment specifier — and, for a `struct`, the same order.
    /// A `union` is held to the order as well: C23 asks it only of structures,
    /// but GCC and Clang both compare unions member by member, and a program
    /// that reorders a union's members between two definitions has said
    /// something worth being told about.
    ///
    /// `comparing` is the pairs of tags this comparison is already inside, so
    /// that two types that name each other — or themselves, from two scopes —
    /// are compared once rather than for ever. A pair already on it is taken
    /// to agree, which is the answer that makes the *whole* comparison say
    /// "the same" exactly when nothing else disagrees.
    pub(super) fn record_difference(
        &self,
        first: RecordId,
        again: RecordId,
        comparing: &mut Vec<(RecordId, RecordId)>,
    ) -> Option<String> {
        if comparing.contains(&(first, again)) {
            return None;
        }
        let types = self.types();
        let (a, b) = (types.record(first), types.record(again));
        if !a.complete || !b.complete {
            return Some("one of the two definitions left the type incomplete".to_owned());
        }
        if a.fields.len() != b.fields.len() {
            return Some(format!(
                "the first definition has {} and this one has {}",
                members(a.fields.len()),
                members(b.fields.len())
            ));
        }
        comparing.push((first, again));
        let difference = a
            .fields
            .iter()
            .zip(&b.fields)
            .enumerate()
            .find_map(|(position, (x, y))| self.field_difference(position, x, y, comparing));
        comparing.pop();
        if difference.is_some() {
            return difference;
        }
        // C23 6.2.7p1 asks for the same alignment specifier on corresponding
        // members; `_Alignas` on a member raises the *record's* alignment, and
        // `__attribute__((packed))` and `#pragma pack(N)` lower every member's,
        // so the two records are what carry the answer.
        if a.align != b.align {
            return Some(format!(
                "the alignment differs: {} here and {} in the first definition",
                alignment(b.align),
                alignment(a.align)
            ));
        }
        if a.packed != b.packed {
            return Some(format!(
                "the packing differs: {} here and {} in the first definition",
                packing(b.packed),
                packing(a.packed)
            ));
        }
        if a.layout != b.layout {
            return Some("the two definitions lay the type out differently".to_owned());
        }
        None
    }

    /// What makes two corresponding members two members, or `None`.
    ///
    /// `x` is the first definition's and `y` is the one being compared with it,
    /// which is why every message says "here" of `y`.
    fn field_difference(
        &self,
        position: usize,
        x: &ir::Field,
        y: &ir::Field,
        comparing: &mut Vec<(RecordId, RecordId)>,
    ) -> Option<String> {
        let at = position + 1;
        if x.anonymous != y.anonymous {
            return Some(format!(
                "the member at position {at} is anonymous in one definition and named in the other"
            ));
        }
        if !x.anonymous && x.name != y.name {
            return Some(format!(
                "the member at position {at} is named '{}' here and '{}' in the first definition",
                y.name, x.name
            ));
        }
        let named = if x.anonymous {
            format!("the anonymous member at position {at}")
        } else {
            format!("member '{}'", x.name)
        };
        match (&x.bits, &y.bits) {
            (Some(x), Some(y)) if x.width != y.width => {
                return Some(format!(
                    "{named} is {} bits wide here and {} in the first definition",
                    y.width, x.width
                ));
            }
            (Some(_), None) => {
                return Some(format!(
                    "{named} is a bit-field in the first definition and not here"
                ));
            }
            (None, Some(_)) => {
                return Some(format!(
                    "{named} is a bit-field here and not in the first definition"
                ));
            }
            _ => {}
        }
        if x.is_const != y.is_const {
            return Some(format!(
                "{named} is 'const' in one definition and not in the other"
            ));
        }
        if x.flexible != y.flexible {
            return Some(format!(
                "{named} is a flexible array member in one definition and not in the other"
            ));
        }
        // An anonymous member has no name to match on and its type is a tag of
        // its own, one per definition, so the two are compared the way the
        // records themselves are; `compatible` would say no, since neither has
        // a tag for the structural rule to match.
        let same = match (x.anonymous, x.ty, y.ty) {
            (true, Ty::Record(p), Ty::Record(q)) => {
                self.record_difference(p, q, comparing).is_none()
            }
            _ => self.compatible_in(x.ty, y.ty, comparing),
        };
        if !same {
            return Some(format!(
                "{named} has type '{}' here and '{}' in the first definition",
                self.tyname(y.ty),
                self.tyname(x.ty)
            ));
        }
        None
    }

    /// Creates an incomplete tag and reserves the Rust name it will use.
    pub(super) fn declare_record(
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
                    Some(ty) => self.apply_mode(ty, &field.attrs),
                    None => continue,
                }
            };
            // A member's size is part of the record's layout, so it has to be
            // known when the tag is defined; C99 6.7.2.1p8 says the same thing
            // by requiring a complete type that is not variably modified.
            if self.types().is_vm(ty) {
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
            let strictest =
                self.strictest_alignment(&field.specifiers.alignas, field.attrs.aligned.as_ref());
            let align_request = strictest.map(|(value, _)| value);
            let align_range = strictest.map(|(_, spec)| spec.range).or_else(|| {
                // A specifier whose operand was refused still says that one was
                // written, which is what the bit-field check below asks.
                field
                    .specifiers
                    .alignas
                    .first()
                    .or(field.attrs.aligned.as_ref())
                    .map(|spec| spec.range)
            });
            // Only the member's *own* `packed` makes it one-byte aligned; a
            // `#pragma pack(N)` caps every member at N instead, which
            // `packing` says on its own.
            let mut packed = field.attrs.packed.is_some();
            let mut align_request = align_request;
            // A member declared with a `typedef` that `aligned(N)` gave its
            // own alignment is N-aligned, as in GCC: `struct { char c;
            // xxh_unalign64 v; }` puts `v` at offset 1. One byte is exactly a
            // packed member, which the layout already has; a weaker alignment
            // above one byte is not something it can express yet, and neither
            // is an array of one-byte-aligned elements, so both are refused
            // rather than laid out wrongly. A stronger one is an `aligned(N)`.
            if field.bit_width.is_none() {
                let natural = self
                    .types()
                    .size_align(ty, &self.target)
                    .map_or(1, |layout| layout.align);
                match self.typedef_align(&field.ty) {
                    Some(1) if natural > 1 => packed = true,
                    Some(n) if n < natural => {
                        self.error(
                            field.range,
                            format!(
                                "a member whose 'typedef' is 'aligned({n})', less than its \
                                 type's {natural}, is not supported unless the alignment is 1"
                            ),
                        );
                        continue;
                    }
                    Some(n) if n > natural => {
                        align_request = Some(align_request.map_or(n, |a| a.max(n)));
                    }
                    _ => {}
                }
                if let Some(n) = self.array_of_typedef_align(&field.ty)
                    && let Ty::Array(id) = ty
                    && let elem = self.types().array_type(id).elem
                    && self
                        .types()
                        .size_align(elem, &self.target)
                        .is_some_and(|layout| n < layout.align)
                {
                    self.error(
                        field.range,
                        format!(
                            "a member that is an array of a 'typedef' made 'aligned({n})', \
                             less than its type's own alignment, is not supported"
                        ),
                    );
                    continue;
                }
            }

            if let Some(width) = &field.bit_width {
                if let Some(range) = align_range
                    && field
                        .specifiers
                        .alignas
                        .iter()
                        .any(|spec| !spec.from_attribute)
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
        if self.types().is_vm(element) {
            self.error(
                field.range,
                format!(
                    "a member of a {} cannot have a variably modified type",
                    kind.as_str()
                ),
            );
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

    /// The strictest of the alignment specifiers written on one declaration.
    ///
    /// C11 6.7.5p6: "if several alignment specifiers appear in the same
    /// declaration, the effective alignment requirement is the strictest one
    /// among them", and GCC folds an `aligned` attribute in with them. `specs`
    /// are the ones written among the declaration specifiers — an `aligned`
    /// there included, which is why `attr` is filtered against them rather than
    /// resolved a second time — and `attr` is the one written on the
    /// declarator.
    ///
    /// Every one of them is resolved, so that a bad operand is reported where
    /// it stands and not only when it happens to be the strictest.
    pub(super) fn strictest_alignment<'a>(
        &mut self,
        specs: &'a [ast::Alignment],
        attr: Option<&'a ast::Alignment>,
    ) -> Option<(u64, &'a ast::Alignment)> {
        let mut best: Option<(u64, &'a ast::Alignment)> = None;
        let written = specs
            .iter()
            .chain(attr.filter(|a| !specs.contains(a)))
            .collect::<Vec<_>>();
        for spec in written {
            let Some(value) = self.alignment_of(Some(spec)) else {
                continue;
            };
            if best.is_none_or(|(best, _)| value > best) {
                best = Some((value, spec));
            }
        }
        best
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

    /// C23 6.7.2.2p... (N3030): every declaration of one enumeration has to
    /// give it the same underlying type.
    ///
    /// `declared` is the type the tag already has and `fixed` whether that was
    /// written or defaulted to `int`; `again` is the fixed type this
    /// declaration writes. Two mistakes are possible and they read differently
    /// — a *different* fixed type, and a fixed type on a tag that was declared
    /// without one — because the second is the one that usually means the
    /// author forgot which of the two declarations came first.
    fn check_fixed_underlying(
        &mut self,
        tag: &str,
        declared: Ty,
        fixed: bool,
        again: Ty,
        range: SourceRange,
    ) -> Result<(), TypeError> {
        if !fixed {
            return Err(TypeError::at(
                range,
                format!(
                    "'enum {tag}' was previously declared without a fixed underlying type, \
                     and a later declaration may not add one"
                ),
            ));
        }
        if declared == again {
            return Ok(());
        }
        Err(TypeError::at(
            range,
            format!(
                "'enum {tag}' is redeclared with the underlying type '{}', where the \
                 previous declaration said '{}'",
                self.tyname(again),
                self.tyname(declared)
            ),
        ))
    }

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
            // C23 (N3030): a non-defining declaration of an enumeration with a
            // fixed underlying type is only permitted as a standalone
            // declaration, so `enum E : long;` is the whole of what may be
            // written — not `enum E : long x;`, not a parameter, a return
            // type, a `typedef` or a member. See [`Sema::standalone_enum`].
            if underlying.is_some() && self.standalone_enum != Some(spec_id) {
                self.error(
                    spec.range,
                    format!(
                        "a non-defining declaration of 'enum {}' with a fixed underlying type \
                         is only allowed as a standalone declaration; write the list of \
                         enumerators, or declare the enumeration on a line of its own",
                        name.name
                    ),
                );
            }
            return match self.lookup_tag(&name.name) {
                Some(TagEntry::Enum {
                    ty,
                    unsigned,
                    fixed,
                    complete,
                    ..
                }) => {
                    // A mention of a tag whose list has not been seen: it has
                    // a size all the same when a fixed underlying type gave it
                    // one (N3030), and none at all otherwise — which is what
                    // `sizeof` of an enumeration inside its own list asks
                    // about (WG14 DR118).
                    if !complete && !fixed {
                        self.enum_incomplete[spec_id.index()] = Some(name.name.clone());
                    }
                    // `enum E : long;` after an `enum E : short;` or an
                    // `enum E { … }` *in the same scope*. A mention with no
                    // underlying type of its own — `enum E x;` — asks nothing
                    // and is left alone; and one in an inner block declares a
                    // tag of that block rather than redeclaring the outer one,
                    // so it has nothing to agree with.
                    let same_scope =
                        matches!(self.tag_here(&name.name), Some(TagEntry::Enum { .. }));
                    if let Some(again) = underlying
                        && same_scope
                    {
                        self.check_fixed_underlying(&name.name, ty, fixed, again, spec.range)?;
                    }
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
                            fixed: underlying.is_some(),
                            complete: false,
                            list: None,
                        },
                    );
                    // `enum E : short;` names a type with a *size* — N3030
                    // settles it where the underlying type is written — even
                    // though its list has not been seen; `enum E;` is GNU's
                    // forward reference, and that one has no size until it is.
                    if underlying.is_none() {
                        self.enum_incomplete[spec_id.index()] = Some(name.name.clone());
                    }
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
                Some(TagEntry::Enum {
                    ty,
                    unsigned,
                    fixed,
                    complete: true,
                    list,
                }) => {
                    // C23 6.7.2.3p1 (N3037), as for a `struct`: a second
                    // definition of one tag in one scope declares the same type
                    // when the two agree, and is the redefinition it always was
                    // when they do not. The enumerators are not declared again
                    // either, which is what makes `enum E { m }; enum E { m };`
                    // legal in C23 where `m` used to be a redefinition too
                    // (`drs/dr1xx.c`).
                    let difference = if self.gating.standard >= crate::Standard::C23 {
                        self.enum_difference(list, ty, fixed, underlying, enumerators)
                    } else {
                        Some(String::new())
                    };
                    let Some(difference) = difference else {
                        self.enum_by_spec[spec_id.index()] = Some(ty);
                        self.enum_unsigned[spec_id.index()] = Some(unsigned);
                        return Ok(ty);
                    };
                    return Err(TypeError::at(
                        spec.range,
                        if difference.is_empty() {
                            format!("redefinition of 'enum {}'", name.name)
                        } else {
                            format!(
                                "redefinition of 'enum {}' with an incompatible enumerator \
                                 list: {difference}",
                                name.name
                            )
                        },
                    ));
                }
                Some(TagEntry::Enum { ty, fixed, .. }) => {
                    // The definition of a tag this scope has already declared:
                    // both have to say the same thing about the underlying
                    // type, whether that is a type or the absence of one.
                    match underlying {
                        Some(again) => {
                            self.check_fixed_underlying(&name.name, ty, fixed, again, spec.range)?;
                        }
                        None if fixed => {
                            return Err(TypeError::at(
                                spec.range,
                                format!(
                                    "'enum {}' was previously declared with the fixed \
                                     underlying type '{}', which every declaration of it \
                                     has to repeat",
                                    name.name,
                                    self.tyname(ty)
                                ),
                            ));
                        }
                        None => {}
                    }
                    declared = Some(ty);
                }
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
            // C23 6.7.3.3p12: "the enumerated type is incomplete until
            // immediately after the `}` that terminates the list" — *unless*
            // it has a fixed underlying type (N3030), which settles its size
            // and alignment where it is written. The tag is in scope for its
            // own list either way, which is what lets an enumerator name it,
            // so it goes in now with `complete` saying only that the list has
            // not been seen yet: `enum E { m = sizeof(enum E) }` is the
            // constraint violation WG14 DR118 (`drs/dr1xx.c`) is about, and
            // `enum E : unsigned long long { m = sizeof(enum E) }` is the
            // valid form `C23/n3030.c` writes beside it.
            self.insert_tag(
                &name.name,
                TagEntry::Enum {
                    ty,
                    unsigned: false,
                    fixed: underlying.is_some(),
                    complete: false,
                    list: None,
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
            // The list is kept only where a second definition of the tag could
            // be legal — C23 6.7.2.3p1 (N3037) — which is the one thing that
            // reads it back.
            let list = if self.gating.standard >= crate::Standard::C23 {
                self.enum_lists.push(
                    placed
                        .iter()
                        .map(|(name, value, _, _)| (name.clone(), *value))
                        .collect(),
                );
                Some((self.enum_lists.len() - 1) as u32)
            } else {
                None
            };
            self.insert_tag(
                &name.name,
                TagEntry::Enum {
                    ty,
                    unsigned,
                    fixed: underlying.is_some(),
                    complete: true,
                    list,
                },
            );
        }
        if let Ty::Enum(id) = ty {
            self.program.types.enum_mut(id).unsigned = unsigned;
        }
        self.enum_unsigned[spec_id.index()] = Some(unsigned);
        Ok(ty)
    }

    /// What makes a second definition of an `enum` tag a different type from
    /// the first, or `None` when C23 makes the two one (6.2.7p1, N3037).
    ///
    /// An empty string is "they are different and there is nothing useful to
    /// say about how", which is every case this cannot answer: an entry point
    /// that kept no list, or an enumerator whose value will not fold.
    ///
    /// The correspondence is by *name* rather than by position. 6.2.7p1 asks
    /// for "a one-to-one correspondence between their members" and requires the
    /// same order only of structures, so `enum E { A = 1, B = 2 }` and
    /// `enum E { B = 2, A = 1 }` are one type — which is what `C23/n3037_1.c`
    /// writes and what GCC accepts.
    fn enum_difference(
        &mut self,
        list: Option<u32>,
        ty: Ty,
        fixed: bool,
        underlying: Option<Ty>,
        enumerators: &[ast::Enumerator],
    ) -> Option<String> {
        // 6.2.7p1's first requirement: a fixed underlying type on one is the
        // same fixed underlying type on the other.
        match (fixed, underlying) {
            (true, Some(again)) if again != ty => {
                return Some(format!(
                    "the underlying type is '{}' here and '{}' in the first definition",
                    self.tyname(again),
                    self.tyname(ty)
                ));
            }
            (true, None) => {
                return Some(
                    "the first definition gave it a fixed underlying type and this one \
                     does not"
                        .to_owned(),
                );
            }
            (false, Some(_)) => {
                return Some(
                    "this definition gives it a fixed underlying type and the first one \
                     does not"
                        .to_owned(),
                );
            }
            _ => {}
        }
        let Some(first) = list.and_then(|list| self.enum_lists.get(list as usize).cloned()) else {
            return Some(String::new());
        };
        let Some(again) = self.enumerator_values(enumerators) else {
            return Some(String::new());
        };
        if first.len() != again.len() {
            return Some(format!(
                "the first definition has {} and this one has {}",
                enumerator_count(first.len()),
                enumerator_count(again.len())
            ));
        }
        for (name, value) in &again {
            match first.iter().find(|(first, _)| first == name) {
                Some((_, first)) if first == value => {}
                Some((_, first)) => {
                    return Some(format!(
                        "enumerator '{name}' is {value} here and {first} in the first \
                         definition"
                    ));
                }
                None => {
                    return Some(format!(
                        "the first definition has no enumerator named '{name}'"
                    ));
                }
            }
        }
        None
    }

    /// The `(name, value)` of every enumerator in a list, without declaring
    /// any of them — C99 6.7.2.2p3's sequence, evaluated only to be compared.
    ///
    /// `None` when a value will not fold, which the caller reads as "these two
    /// definitions cannot be shown to agree".
    fn enumerator_values(
        &mut self,
        enumerators: &[ast::Enumerator],
    ) -> Option<Vec<(String, i128)>> {
        let mut values = Vec::with_capacity(enumerators.len());
        let mut next = 0i128;
        for enumerator in enumerators {
            let value = match &enumerator.value {
                Some(expr) => {
                    let value = self.expr(expr)?;
                    if !value.ty.is_integer() {
                        return None;
                    }
                    match self.const_eval_at(&value, "enumerator value") {
                        Some(ir::ConstValue::Int(value)) => value,
                        _ => return None,
                    }
                }
                None => next,
            };
            next = value.wrapping_add(1);
            values.push((enumerator.name.name.clone(), value));
        }
        Some(values)
    }
}

/// "one enumerator" or "three enumerators", for the same diagnostic.
fn enumerator_count(count: usize) -> String {
    if count == 1 {
        "one enumerator".to_owned()
    } else {
        format!("{count} enumerators")
    }
}

/// "one member" or "three members", for the redefinition diagnostic.
fn members(count: usize) -> String {
    if count == 1 {
        "one member".to_owned()
    } else {
        format!("{count} members")
    }
}

/// How the alignment `_Alignas` gave a record reads in that diagnostic.
fn alignment(align: Option<u64>) -> String {
    match align {
        Some(align) => format!("{align}"),
        None => "the type's own".to_owned(),
    }
}

/// How `__attribute__((packed))` or `#pragma pack(N)` reads in it.
fn packing(packed: Option<u64>) -> String {
    match packed {
        Some(1) => "packed".to_owned(),
        Some(bytes) => format!("packed to {bytes} bytes"),
        None => "unpacked".to_owned(),
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
