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
    /// What `_Alignas` asked for.
    align_request: Option<u64>,
    /// Where it asked for it.
    align_range: Option<SourceRange>,
    range: SourceRange,
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
    /// The alignment the generated item needs `#[repr(C, align(N))]` for.
    align_attr: Option<u64>,
    /// The members whose `_Alignas` could not be honoured.
    unsupported: Vec<usize>,
}

impl Sema {
    /// Resolves an AST type into a [`Ty`], or explains why it cannot.
    pub(super) fn resolve_ty(&mut self, ty: &ast::Type) -> Result<Ty, TypeError> {
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
                let len = self.array_len(size, range)?;
                self.check_element_type(element, elem.range)?;
                Ok(self
                    .program
                    .types
                    .array(element, len, elem.qualifiers.is_const))
            }
            ast::TypeKind::Function(func) => {
                let ret = self.resolve_ty(&func.ret)?;
                if ret.is_array() {
                    return Err(TypeError::at(
                        func.ret.range,
                        "a function cannot return an array type",
                    ));
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
            ast::TypeKind::Record(record) => self.record_ty(record),
            ast::TypeKind::Enum(spec) => self.enum_ty(spec),
            // C23's `typeof`. The operand of the expression form is not
            // evaluated, and it does *not* decay: `typeof(a)` of an array is
            // the array type, which is the whole point of the operator.
            ast::TypeKind::Typeof(operand) => match operand.as_ref() {
                ast::TypeofOperand::Expr(expr) => {
                    let ty = if self.is_lvalue_form(expr) {
                        self.lvalue(expr).map(|place| place.ty)
                    } else {
                        self.expr(expr).map(|value| value.ty)
                    };
                    ty.ok_or_else(|| TypeError::silent(range))
                }
                ast::TypeofOperand::Type(name) => self.resolve_ty(&name.ty),
            },
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
        if let ast::TypeKind::Array { elem, .. } = &ty.kind {
            // The bound of an array parameter is not part of its type at all,
            // so it is not even evaluated.
            let element = self.resolve_ty(elem)?;
            return Ok(self.ptr_to(element, elem.qualifiers.is_const));
        }
        let resolved = self.resolve_ty(ty)?;
        if resolved.is_func() {
            return Ok(self.ptr_to(resolved, false));
        }
        Ok(resolved)
    }

    /// Evaluates an array bound.
    fn array_len(&mut self, size: &ast::ArraySize, range: SourceRange) -> Result<u64, TypeError> {
        let vla = |range| {
            TypeError::at(
                range,
                "variable length arrays are not supported yet; the bound of an array \
                 must be an integer constant expression",
            )
        };
        let expr = match size {
            ast::ArraySize::Unspecified => {
                return Err(TypeError::at(
                    range,
                    "an array of unspecified size must have an initializer",
                ));
            }
            ast::ArraySize::Star => return Err(vla(range)),
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
            // A bound that is not constant is a VLA inside a function, and
            // simply invalid at file scope.
            return Err(if self.at_file_scope() {
                TypeError::at(
                    expr.range,
                    "array size is not an integer constant expression",
                )
            } else {
                vla(expr.range)
            });
        };
        if len < 0 {
            return Err(TypeError::at(expr.range, "array size is negative"));
        }
        u64::try_from(len).map_err(|_| TypeError::at(expr.range, "array size is too large"))
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

    /// Resolves a type that must name a complete object type.
    pub(super) fn object_ty_of(&mut self, ty: &ast::Type, name: &str) -> Option<Ty> {
        let resolved = self.ty_of(ty)?;
        if resolved.is_void() {
            self.error(
                ty.range,
                format!("variable '{name}' has incomplete type 'void'"),
            );
            return None;
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

    fn record_ty(&mut self, spec: &ast::RecordType) -> Result<Ty, TypeError> {
        let key = (spec.range.start, spec.range.end);
        // The parser clones a specifier into every declarator that shares it;
        // the range is what identifies the one type they all mean.
        if let Some(id) = self.record_by_range.get(&key) {
            return Ok(Ty::Record(*id));
        }
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
                self.record_by_range.insert(key, id);
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
            self.record_by_range.insert(key, id);
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
        self.record_by_range.insert(key, id);
        self.define_record(id, fields);
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
            emit: true,
            range,
        })
    }

    /// Resolves a member list and computes the record's layout.
    fn define_record(&mut self, id: RecordId, fields: &[ast::FieldDecl]) {
        let kind = self.types().record(id).kind;
        let mut members: Vec<Member> = Vec::with_capacity(fields.len());
        let mut anonymous = 0u32;
        for field in fields {
            let Some(ty) = self.ty_of(&field.ty) else {
                continue;
            };
            // Rust's `VaList` borrows the caller's frame; a member would have
            // to name that lifetime, and the record would stop being a plain
            // `#[repr(C)]` type.
            if self.reject_va_list(ty, field.range) {
                continue;
            }
            let align_request = self.alignment_of(field.specifiers.alignas.as_ref());
            let align_range = field.specifiers.alignas.as_ref().map(|a| a.range);

            if let Some(width) = &field.bit_width {
                if align_request.is_some() {
                    // Which is what GCC says too: a bit-field has no address,
                    // so there is nothing for an alignment to apply to.
                    self.error(
                        align_range.unwrap_or(field.range),
                        "'_Alignas' cannot be applied to a bit-field",
                    );
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
                    align_request: None,
                    align_range: None,
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
                    align_range,
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
                align_range,
                range: name.range,
            });
        }

        let laid_out = self.lay_out(kind, &members);
        for index in laid_out.unsupported {
            // Report at the `_Alignas` rather than at the member: the
            // specifier is what has to change.
            let range = members[index].align_range.unwrap_or(members[index].range);
            self.error(
                range,
                "_Alignas on this member is not supported yet; the member's natural \
                 offset does not already satisfy the alignment asked for, and honouring \
                 it would change how Rust code reaches the field",
            );
        }
        let record = self.program.types.record_mut(id);
        record.fields = laid_out.fields;
        record.rust_fields = laid_out.rust_fields;
        record.complete = true;
        record.layout = Some(laid_out.layout);
        record.align = laid_out.align_attr;
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
    /// exactly here and nowhere else.
    fn bit_field_signed(&self, written: &ast::Type, ty: Ty) -> bool {
        if let ast::TypeKind::Enum(spec) = &written.kind
            && let Some(unsigned) = self.enum_unsigned.get(&(spec.range.start, spec.range.end))
        {
            return !*unsigned;
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
    fn alignment_of(&mut self, spec: Option<&ast::Alignment>) -> Option<u64> {
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
    /// A member's `_Alignas` is honoured by raising the *record's* alignment,
    /// which is what `#[repr(C, align(N))]` says: Rust then lays the members
    /// out by their own natural alignment, so the offset our layout computes
    /// and the offset the generated item really has only agree while the
    /// member's natural offset already satisfies what it asked for. Where it
    /// does not — `struct { char c; _Alignas(16) int x; }` — the two would
    /// disagree, and the member is reported instead.
    ///
    /// Bit-fields need the same reconciliation, and get it the other way
    /// round: the *Rust* fields are chosen so that `#[repr(C)]` reproduces the
    /// offsets C asked for, with explicit padding wherever it would place one
    /// too early and `#[repr(C, align(N))]` wherever a bit-field's type made
    /// the record stricter than any Rust field of it is.
    fn lay_out(&self, kind: RecordKind, members: &[Member]) -> LaidOut {
        let target = &self.target;
        let mut align = 1u64;
        let mut raised: Option<u64> = None;
        let mut unsupported = Vec::new();

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
            if kind == RecordKind::Union {
                off = 0;
            }
            let Some((width, _)) = member.bits else {
                align = align.max(item.align);
                let offset = match kind {
                    RecordKind::Struct => round_up(off.div_ceil(8), item.align),
                    RecordKind::Union => 0,
                };
                if let Some(want) = member.align_request
                    && want > item.align
                {
                    if offset % want == 0 {
                        align = align.max(want);
                        raised = Some(raised.unwrap_or(1).max(want));
                    } else {
                        unsupported.push(index);
                    }
                }
                spots.push(Spot::Byte(offset));
                match kind {
                    RecordKind::Struct => off = offset.saturating_add(item.size).saturating_mul(8),
                    RecordKind::Union => union_bytes = union_bytes.max(item.size),
                }
                continue;
            };
            // A bit-field never straddles a unit of its own type: if it would,
            // it starts at the next unit boundary instead. Width zero is the
            // request for that boundary and nothing else.
            let unit = item.size.saturating_mul(8).max(1);
            let w = u64::from(width);
            if w == 0 || off / unit != (off + w - 1) / unit {
                off = round_up(off, unit);
            }
            let (start, end) = (off, off + w);
            off = end;
            // Only a named bit-field makes the record stricter.
            if member.name.is_some() {
                align = align.max(item.align);
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
        let mut pads = 0u32;
        for (index, member) in members.iter().enumerate() {
            if kind == RecordKind::Union {
                pos = 0;
            }
            match spots[index] {
                Spot::Byte(offset) => {
                    let item = self
                        .types()
                        .size_align(member.ty, target)
                        .unwrap_or(Layout { size: 0, align: 1 });
                    natural = natural.max(item.align);
                    // `#[repr(C)]` inserts the padding an alignment calls for
                    // on its own; only a member C put *further* along than
                    // that needs a field of its own to get there.
                    if offset > round_up(pos, item.align) {
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
                        range: member.range,
                    });
                }
            }
        }
        // A trailing bit-field can push the record's size past its last Rust
        // field without leaving any storage behind — `struct { int a; long
        // long : 0; }` is four bytes of `a` and twelve of nothing. Only an
        // explicit field can make `#[repr(C)]` reproduce that.
        if kind == RecordKind::Struct && bytes > pos {
            rust_fields.push(RustField::Pad {
                name: format!("__cinrs_pad{pads}"),
                bytes: bytes - pos,
            });
        }
        name_accessors(&mut fields);
        LaidOut {
            fields,
            rust_fields,
            // A record whose alignment comes from a bit-field's type has no
            // Rust field that strict, so the item has to say so itself.
            align_attr: if layout.align > natural {
                Some(layout.align)
            } else {
                raised
            },
            layout,
            unsupported,
        }
    }

    // -- enum ---------------------------------------------------------------

    fn enum_ty(&mut self, spec: &ast::EnumType) -> Result<Ty, TypeError> {
        let key = (spec.range.start, spec.range.end);
        if let Some(ty) = self.enum_by_range.get(&key) {
            return Ok(*ty);
        }

        // C23's fixed underlying type. An enumeration with one *is* that
        // integer type: its enumerators have it, `sizeof` gives its size, and
        // the tag becomes an alias for it.
        let underlying = match &spec.underlying {
            Some(ty) => {
                let resolved = self.resolve_ty(ty)?;
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
                Some(TagEntry::Enum { ty, unsigned }) => {
                    self.enum_by_range.insert(key, ty);
                    self.enum_unsigned.insert(key, unsigned);
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
                // declared before it is defined; nothing else can be.
                None => match underlying {
                    Some(ty) => {
                        let unsigned = !ty.is_signed(&self.target);
                        self.insert_tag(&name.name, TagEntry::Enum { ty, unsigned });
                        self.enum_by_range.insert(key, ty);
                        self.enum_unsigned.insert(key, unsigned);
                        Ok(ty)
                    }
                    None => Err(TypeError::at(
                        spec.range,
                        format!(
                            "'enum {}' has not been defined; C99 has no incomplete enum types",
                            name.name
                        ),
                    )),
                },
            };
        };

        if let Some(name) = &spec.name
            && let Some(TagEntry::Enum { .. }) = self.tag_here(&name.name)
        {
            return Err(TypeError::at(
                spec.range,
                format!("redefinition of 'enum {}'", name.name),
            ));
        }

        // Only a file-scope `enum` becomes a named alias; one declared inside a
        // block would need a mangled name nobody could use, and `int` is what
        // its values are anyway.
        let file_scope = self.at_file_scope();
        let tag_name = spec.name.as_ref().map(|n| n.name.clone());
        let ty = match (file_scope, tag_name, underlying) {
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
                },
            );
        }
        self.enum_by_range.insert(key, ty);
        // With a fixed underlying type the enumerators have the enumeration's
        // own type; without one they are `int`, as C99 says.
        let constant_ty = underlying.unwrap_or(Ty::Int);

        // C99 6.7.2.2: an enumerator without a value is one more than the
        // previous one, and the first is zero.
        let mut next = 0i128;
        // Which of `int` and `unsigned int` the implementation makes the
        // underlying type. GCC and Clang pick the unsigned one whenever no
        // enumerator is negative, and that choice is visible through a
        // bit-field of the type; see `Sema::bit_field_signed`.
        let mut unsigned = underlying.is_none_or(|fixed| !fixed.is_signed(&self.target));
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
                self.error(
                    enumerator.range,
                    format!(
                        "enumerator value {value} is outside the range of '{}'",
                        self.tyname(constant_ty)
                    ),
                );
            }
            let value = constant_ty.wrap(value, &self.target);
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
            if file_scope {
                let rust_name = self.reserve_item_name(&enumerator.name.name);
                self.program.enum_constants.push(ir::Enumerator {
                    name: enumerator.name.name.clone(),
                    rust_name,
                    ty: constant_ty,
                    value,
                    range: enumerator.name.range,
                });
            }
        }
        if let Some(name) = &spec.name {
            self.insert_tag(&name.name, TagEntry::Enum { ty, unsigned });
        }
        if let Ty::Enum(id) = ty {
            self.program.types.enum_mut(id).unsigned = unsigned;
        }
        self.enum_unsigned.insert(key, unsigned);
        Ok(ty)
    }
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

/// Rounds `value` up to a multiple of `align`.
fn round_up(value: u64, align: u64) -> u64 {
    if align <= 1 {
        return value;
    }
    value.div_ceil(align).saturating_mul(align)
}
