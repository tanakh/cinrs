//! GCC's vector extensions on vector types of the program's own.
//!
//! `typedef int v4si __attribute__((vector_size(16)));` declares a vector of
//! four `int`s, sixteen bytes wide, and GCC gives it the operators of its
//! elements: `a + b`, `a * 2`, `-a`, `a < b`, `v[i]`. `core::simd` is
//! unstable, so the type is a [`Ty::GnuVector`] — a `#[repr(C, align(N))]`
//! struct over `[T; K]` in the generated code — and every operator is one of
//! the [`BuiltinOp`] vector operations, which code generation writes out over
//! the elements for LLVM to vectorise. What GCC does, and so what this does:
//!
//! * the size is `N`, a power-of-two multiple of the element's, and the
//!   alignment is `N` capped at the target's widest vector alignment — 16
//!   bytes on x86-64 without AVX, 32 with it and 64 with AVX-512F, 16
//!   elsewhere; an `aligned` on a `typedef` of one replaces it, weaker or
//!   stricter, which is how glibc's `<bits/link.h>` declares its 32- and
//!   64-byte vectors;
//! * the arithmetic operators work element by element, on two vectors of one
//!   type or on a vector and a scalar, which is converted to the element type
//!   and broadcast — if it converts without losing anything, as GCC requires
//!   ("involves truncation"); integer elements wrap, as `int` arithmetic does
//!   everywhere in this crate;
//! * a comparison gives the signed integer vector of the same shape, each
//!   element -1 where it holds and 0 where it does not, and that result
//!   converts to any integer vector of its shape — GCC's "opaque" type;
//! * `v[i]` is an element, an lvalue when `v` is one;
//! * a brace initialiser lists the elements, the rest zero;
//! * a cast between two vector types of one size, or between a vector and an
//!   integer of its size, rereads the bytes, and so does the implicit
//!   conversion between a vector and the Intel type GCC defines as the same
//!   vector — `__m128i` is two `long long`s;
//! * `__builtin_convertvector` converts element by element, and
//!   `__builtin_shuffle` permutes.
//!
//! `?:` with a vector condition, and `!`, `&&` and `||` on vectors, are GCC's
//! C++ only and refused here as GCC refuses them in C. A vector is passed to
//! and from a function as that struct, which is not the platform's convention
//! — GCC passes one in a vector register — so a call that passes or returns
//! one by value to a function the unit does not define is refused, as is a
//! vector in the variable part of a call.

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{
    BinOp, BuiltinOp, Callee, ConstValue, Expr, ExprKind, Place, Signature, Ty, VecTy,
};

use super::Sema;

/// The elements GCC gives an Intel vector type: `__m128i` is `long long
/// __attribute__((vector_size(16)))` in GCC's `<emmintrin.h>`.
fn intel_elements(vec: VecTy) -> Option<(Ty, u32)> {
    Some(match vec {
        VecTy::M128 => (Ty::Float, 4),
        VecTy::M128d => (Ty::Double, 2),
        VecTy::M128i => (Ty::LongLong, 2),
        VecTy::M256 => (Ty::Float, 8),
        VecTy::M256d => (Ty::Double, 4),
        VecTy::M256i => (Ty::LongLong, 4),
        VecTy::M512 => (Ty::Float, 16),
        VecTy::M512d => (Ty::Double, 8),
        VecTy::M512i => (Ty::LongLong, 8),
        _ => return None,
    })
}

/// A vector operation of type `ty`.
fn vector_op(op: BuiltinOp, args: Vec<Expr>, ty: Ty, range: SourceRange) -> Expr {
    Expr::new(ExprKind::Builtin { op, args }, ty, range)
}

/// Whether `expr` is a vector comparison, whose result GCC lets convert to any
/// integer vector of its shape — or `~`, `-` or a bitwise operator on such
/// results, which keep that type: `~(a <= b)`.
fn is_comparison(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Builtin {
            op: BuiltinOp::VecCompare(_),
            ..
        } => true,
        ExprKind::Builtin {
            op: BuiltinOp::VecBitNot | BuiltinOp::VecNeg,
            args,
        } => args.iter().all(is_comparison),
        ExprKind::Builtin {
            op: BuiltinOp::VecBinary(BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor),
            args,
        } => args.iter().all(is_comparison),
        _ => false,
    }
}

/// The widest alignment GCC gives a vector type on the target: its
/// `BIGGEST_ALIGNMENT`, which on x86 follows the instruction sets the unit is
/// compiled for.
pub(super) fn vector_align_cap(options: &crate::Options) -> u64 {
    use crate::target::Arch;
    if !matches!(options.target.arch, Arch::X86 | Arch::X86_64) {
        return 16;
    }
    let features: Vec<&str> = options.target_features.iter().map(String::as_str).collect();
    let macros = crate::x86::target_macros(&["__SSE__", "__SSE2__"], &features);
    if macros.contains("__AVX512F__") {
        64
    } else if macros.contains("__AVX__") {
        32
    } else {
        16
    }
}

impl Sema<'_> {
    /// `__attribute__((mode(M)))` and `__attribute__((vector_size(N)))` on a
    /// declaration, in that order: `int v __attribute__((vector_size(16)))` is
    /// a vector of four `int`s.
    ///
    /// GCC applies `vector_size` to the *base* type of the declarator, through
    /// any pointers and arrays: `int *p __attribute__((vector_size(16)))` is a
    /// pointer to a vector, and `int a[2] __attribute__((vector_size(16)))` an
    /// array of two.
    pub(super) fn apply_type_attrs(&mut self, ty: Ty, attrs: &ast::Attributes) -> Ty {
        let ty = self.apply_mode(ty, attrs);
        let Some(size) = &attrs.vector_size else {
            return ty;
        };
        if ty.is_error() {
            return ty;
        }
        let Some(bytes) = self.vector_size_bytes(&size.node, size.range) else {
            return Ty::Error;
        };
        self.vector_of(ty, bytes, size.range)
    }

    /// The type a type name names — in a cast, `sizeof`, `_Alignof`, a
    /// compound literal — with a `vector_size` among its specifiers applied:
    /// `(__attribute__((vector_size(16))) float){ 1, 2, 3, 4 }`.
    pub(super) fn type_name_ty(&mut self, name: &ast::TypeName) -> Option<Ty> {
        let ty = self.ty_of(&name.ty)?;
        if name.specifiers.attrs.vector_size.is_none() {
            return Some(ty);
        }
        let ty = self.apply_type_attrs(ty, &name.specifiers.attrs);
        (!ty.is_error()).then_some(ty)
    }

    /// The operand of `vector_size`, in bytes.
    fn vector_size_bytes(&mut self, expr: &ast::Expr, range: SourceRange) -> Option<u64> {
        let value = self.expr(expr)?;
        if !value.ty.is_integer() {
            self.error(expr.range, "the vector size must be an integer constant");
            return None;
        }
        match self.const_eval_at(&value, "the vector size")? {
            ConstValue::Int(0) => {
                self.error(range, "zero vector size");
                None
            }
            ConstValue::Int(bytes) if bytes > 0 => u64::try_from(bytes).ok(),
            _ => {
                self.error(expr.range, "the vector size must be positive");
                None
            }
        }
    }

    /// `ty` with its base type made a vector `bytes` wide.
    fn vector_of(&mut self, ty: Ty, bytes: u64, range: SourceRange) -> Ty {
        match ty {
            Ty::Error => Ty::Error,
            Ty::Pointer(id) => {
                let pointer = self.types().pointer_type(id);
                let pointee = self.vector_of(pointer.pointee, bytes, range);
                if pointee.is_error() {
                    return pointee;
                }
                self.program.types.pointer(pointee, pointer.konst)
            }
            Ty::Array(id) => {
                let array = self.types().array_type(id);
                if array.vla || array.incomplete {
                    self.error(
                        range,
                        "'vector_size' on an array of unknown or variable length is not \
                         supported; declare the vector type with a 'typedef' first",
                    );
                    return Ty::Error;
                }
                let elem = self.vector_of(array.elem, bytes, range);
                if elem.is_error() {
                    return elem;
                }
                self.program.types.array(elem, array.len, array.elem_const)
            }
            _ => self.vector_of_bytes(ty, bytes, range),
        }
    }

    /// The vector of `elem`s `bytes` wide, or the reason there is none.
    pub(super) fn vector_of_bytes(&mut self, elem: Ty, bytes: u64, range: SourceRange) -> Ty {
        let integer = elem.is_integer() && !elem.is_bool() && !elem.is_enum();
        if !(integer || elem.is_floating()) {
            self.error(
                range,
                format!(
                    "invalid vector type for attribute 'vector_size': the elements of a vector \
                     are an integer or a real floating type, and '{}' is neither",
                    self.tyname(elem)
                ),
            );
            return Ty::Error;
        }
        let unit = elem.size_bytes(&self.target);
        if !bytes.is_multiple_of(unit) {
            self.error(
                range,
                format!(
                    "vector size {bytes} is not an integral multiple of the size of '{}', {unit}",
                    self.tyname(elem)
                ),
            );
            return Ty::Error;
        }
        let len = bytes / unit;
        if !len.is_power_of_two() {
            self.error(
                range,
                format!("number of vector components {len} is not a power of two"),
            );
            return Ty::Error;
        }
        let Ok(len) = u32::try_from(len) else {
            self.error(range, format!("a vector of {len} elements is too large"));
            return Ty::Error;
        };
        let align = bytes.min(self.vector_align_cap);
        self.program.types.gnu_vector(elem, len, align)
    }

    /// `typedef float T __attribute__((vector_size(32), aligned(16)))`: the
    /// vector type with the alignment the `typedef` asks for, weaker or
    /// stricter — though never weaker than an element's, which no Rust
    /// struct can be.
    pub(super) fn realign_gnu_vector(&mut self, ty: Ty, attrs: &ast::Attributes) -> Ty {
        let Ty::GnuVector(id) = ty else {
            return ty;
        };
        let Some(aligned) = attrs.aligned.clone() else {
            return ty;
        };
        let Some(want) = self.alignment_of(Some(&aligned)) else {
            return ty;
        };
        let vec = self.types().gnu_vector_type(id);
        let elem_align = self
            .types()
            .size_align(vec.elem, &self.target)
            .map_or(1, |layout| layout.align);
        self.program
            .types
            .gnu_vector(vec.elem, vec.len, want.max(elem_align))
    }

    /// The element type and the number of elements of a vector of either
    /// kind, the Intel types read as GCC defines them.
    fn elements_of(&self, ty: Ty) -> Option<(Ty, u32)> {
        match ty {
            Ty::GnuVector(id) => {
                let vec = self.types().gnu_vector_type(id);
                Some((vec.elem, vec.len))
            }
            Ty::Vector(vec) => intel_elements(vec),
            _ => None,
        }
    }

    /// The element type of a GCC vector type.
    fn elem_of(&self, ty: Ty) -> Ty {
        match ty {
            Ty::GnuVector(id) => self.types().gnu_vector_type(id).elem,
            _ => ty,
        }
    }

    /// The vector type of `len` `elem`s with GCC's own alignment for it.
    fn plain_vector(&mut self, elem: Ty, len: u32) -> Ty {
        let bytes = elem.size_bytes(&self.target) * u64::from(len);
        let align = bytes.min(self.vector_align_cap);
        self.program.types.gnu_vector(elem, len, align)
    }

    /// Whether `expr` converts to `to` without a cast, where one of the two is
    /// a GCC vector: the same elements, however each is aligned — which is
    /// also how GCC sees `__m128i` and two `long long`s — or a comparison's
    /// result going to an integer vector of its shape.
    pub(super) fn gnu_vector_convertible(&self, expr: &Expr, to: Ty) -> bool {
        if !(expr.ty.is_gnu_vector() || to.is_gnu_vector()) {
            return false;
        }
        let (Some((from_elem, from_len)), Some((to_elem, to_len))) =
            (self.elements_of(expr.ty), self.elements_of(to))
        else {
            return false;
        };
        if from_len != to_len {
            return false;
        }
        if from_elem == to_elem {
            return true;
        }
        is_comparison(expr)
            && to_elem.is_integer()
            && from_elem.size_bytes(&self.target) == to_elem.size_bytes(&self.target)
    }

    /// The implicit conversion [`Sema::gnu_vector_convertible`] allows, as the
    /// rereading of the bytes it is.
    pub(super) fn gnu_vector_retyped(expr: Expr, to: Ty) -> Expr {
        let range = expr.range;
        vector_op(BuiltinOp::VecBitcast, vec![expr], to, range)
    }

    /// `(T) v`, where `T` or `v` is a GCC vector: the bytes reread, between
    /// two vectors of one size or a vector and an integer of its size.
    pub(super) fn gnu_vector_cast(
        &mut self,
        value: Expr,
        target: Ty,
        range: SourceRange,
    ) -> Option<Expr> {
        if value.ty == target {
            return Some(value);
        }
        let castable =
            |ty: Ty| ty.is_gnu_vector() || ty.is_vector() || (ty.is_integer() && !ty.is_bool());
        if !castable(value.ty) || !castable(target) {
            self.error(
                range,
                format!(
                    "cannot convert a value of type '{}' to '{}': a vector converts only to \
                     another vector or to an integer type of the same size",
                    self.tyname(value.ty),
                    self.tyname(target)
                ),
            );
            return None;
        }
        let size = |sema: &Self, ty: Ty| sema.types().size_of(ty, &sema.target).unwrap_or(0);
        let (from, to) = (size(self, value.ty), size(self, target));
        if from != to {
            self.error(
                range,
                format!(
                    "cannot convert a value of type '{}' to '{}', which has a different size \
                     ({from} and {to} bytes)",
                    self.tyname(value.ty),
                    self.tyname(target)
                ),
            );
            return None;
        }
        Some(vector_op(BuiltinOp::VecBitcast, vec![value], target, range))
    }

    /// An operand of a vector operator that is an Intel vector, as the GCC
    /// vector of the other operand's type when GCC sees the two as one type.
    fn intel_operand(&self, value: Expr, other: Ty) -> Expr {
        if value.ty.is_vector()
            && other.is_gnu_vector()
            && self.gnu_vector_convertible(&value, other)
        {
            return Self::gnu_vector_retyped(value, other);
        }
        value
    }

    /// The two operands of a binary operator with a GCC vector among them,
    /// made two values of one vector type, and that type.
    ///
    /// The other operand is a vector of the same type — or of the same
    /// elements aligned differently — or a scalar, broadcast; a shift and a
    /// comparison also take two integer vectors that differ in signedness, as
    /// GCC does.
    fn gnu_vector_operands(
        &mut self,
        op: ast::BinaryOp,
        lhs: Expr,
        rhs: Expr,
        range: SourceRange,
    ) -> Option<(Expr, Expr, Ty)> {
        let lhs = self.intel_operand(lhs, rhs.ty);
        let rhs = self.intel_operand(rhs, lhs.ty);
        let bin = super::arith_op(op);
        let shift = matches!(bin, Some(BinOp::Shl | BinOp::Shr));
        let compare = super::compare_op(op).is_some();
        let (lhs_name, rhs_name) = (self.tyname(lhs.ty), self.tyname(rhs.ty));
        let invalid = |sema: &mut Self, why: &str| {
            sema.error(
                range,
                format!(
                    "invalid operands to binary '{}' (have '{lhs_name}' and '{rhs_name}'){why}",
                    op.as_str()
                ),
            );
        };
        if bin.is_none() && !compare {
            invalid(self, "");
            return None;
        }
        let (lhs, rhs, vec) = match (lhs.ty, rhs.ty) {
            (Ty::GnuVector(a), Ty::GnuVector(b)) => {
                let (x, y) = (
                    self.types().gnu_vector_type(a),
                    self.types().gnu_vector_type(b),
                );
                let same_elements = x.elem == y.elem && x.len == y.len;
                let differ_in_sign = x.len == y.len
                    && x.elem.is_integer()
                    && y.elem.is_integer()
                    && x.elem.size_bytes(&self.target) == y.elem.size_bytes(&self.target);
                if a == b {
                    let ty = lhs.ty;
                    (lhs, rhs, ty)
                } else if same_elements
                    || ((shift || compare || is_comparison(&rhs)) && differ_in_sign)
                {
                    let ty = lhs.ty;
                    (lhs, Self::gnu_vector_retyped(rhs, ty), ty)
                } else if differ_in_sign && is_comparison(&lhs) {
                    // A comparison's result takes the other operand's type.
                    let ty = rhs.ty;
                    (Self::gnu_vector_retyped(lhs, ty), rhs, ty)
                } else {
                    invalid(
                        self,
                        ": the two vectors have different types, and GCC converts one to the \
                         other only with a cast",
                    );
                    return None;
                }
            }
            (Ty::GnuVector(_), _) => {
                let ty = lhs.ty;
                let rhs = self.broadcast(rhs, ty, shift, &lhs_name, &rhs_name, op, range)?;
                (lhs, rhs, ty)
            }
            (_, Ty::GnuVector(_)) => {
                let ty = rhs.ty;
                let lhs = self.broadcast(lhs, ty, shift, &lhs_name, &rhs_name, op, range)?;
                (lhs, rhs, ty)
            }
            _ => unreachable!("gnu_vector_operands is only called with a vector operand"),
        };
        let elem = self.elem_of(vec);
        if elem.is_floating()
            && matches!(
                bin,
                Some(
                    BinOp::Rem
                        | BinOp::BitAnd
                        | BinOp::BitOr
                        | BinOp::BitXor
                        | BinOp::Shl
                        | BinOp::Shr
                )
            )
        {
            invalid(self, ": the elements are floating");
            return None;
        }
        Some((lhs, rhs, vec))
    }

    /// A scalar operand of a vector operator, broadcast to `vec`.
    ///
    /// GCC converts it to the element type only when nothing is lost: an
    /// integer constant that fits the element's width, a floating constant
    /// the element type holds exactly, or a value of a type no wider than the
    /// element's — "conversion of scalar 'int' to vector 'v16qi' involves
    /// truncation" otherwise. A shift count is any integer.
    #[allow(clippy::too_many_arguments)]
    fn broadcast(
        &mut self,
        scalar: Expr,
        vec: Ty,
        shift: bool,
        lhs_name: &str,
        rhs_name: &str,
        op: ast::BinaryOp,
        range: SourceRange,
    ) -> Option<Expr> {
        if !scalar.ty.is_arithmetic() || scalar.ty.is_complex() {
            self.error(
                range,
                format!(
                    "invalid operands to binary '{}' (have '{lhs_name}' and '{rhs_name}'): a \
                     vector operator takes a vector of the same type or a real arithmetic \
                     scalar, which is converted to the element type and broadcast",
                    op.as_str()
                ),
            );
            return None;
        }
        let elem = self.elem_of(vec);
        if elem.is_integer() && scalar.ty.is_floating() {
            self.error(
                range,
                format!(
                    "cannot convert '{}' to the integer vector '{}': GCC broadcasts a scalar \
                     to a vector only when it converts without losing anything",
                    self.tyname(scalar.ty),
                    self.tyname(vec)
                ),
            );
            return None;
        }
        if shift && !scalar.ty.is_integer() {
            self.error(range, "the count of a vector shift must be an integer");
            return None;
        }
        if !shift && self.broadcast_loses(&scalar, elem) {
            self.error(
                range,
                format!(
                    "conversion of scalar '{}' to vector '{}' involves truncation",
                    self.tyname(scalar.ty),
                    self.tyname(vec)
                ),
            );
            return None;
        }
        let at = scalar.range;
        let value = self.convert(scalar, elem);
        Some(vector_op(BuiltinOp::VecSplat, vec![value], vec, at))
    }

    /// Whether converting `scalar` to `elem` could lose something, by GCC's
    /// rule for a vector operand.
    fn broadcast_loses(&mut self, scalar: &Expr, elem: Ty) -> bool {
        let target = self.target;
        let from = scalar.ty;
        let saved = std::mem::take(&mut self.diags);
        let constant = self.const_eval(scalar);
        self.diags = saved;
        if elem.is_integer() {
            return match constant {
                // A constant that fits the element's width either way: `-1`
                // broadcasts to unsigned elements, `3000000000u` to `int`s.
                Some(ConstValue::Int(value)) => {
                    let bits = elem.bits(&target);
                    if bits >= 127 {
                        return false;
                    }
                    let min = -(1i128 << (bits - 1));
                    let max = (1i128 << bits) - 1;
                    !(min..=max).contains(&value)
                }
                _ => from.size_bytes(&target) > elem.size_bytes(&target),
            };
        }
        let float = elem == Ty::Float;
        match constant {
            Some(ConstValue::Int(value)) => {
                if float {
                    (value as f32) as i128 != value
                } else {
                    (value as f64) as i128 != value
                }
            }
            Some(ConstValue::Float(value)) => {
                float && value.is_finite() && f64::from(value as f32) != value
            }
            _ if from.is_integer() => from.bits(&target) > if float { 24 } else { 53 },
            _ => from.size_bytes(&target) > elem.size_bytes(&target),
        }
    }

    /// A binary operator with a GCC vector operand: element-wise arithmetic,
    /// or a comparison giving -1 and 0.
    pub(super) fn gnu_vector_binary(
        &mut self,
        op: ast::BinaryOp,
        lhs: Expr,
        rhs: Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        let (lhs, rhs, vec) = self.gnu_vector_operands(op, lhs, rhs, range)?;
        if let Some(cmp) = super::compare_op(op) {
            let ty = self.comparison_ty(vec);
            return Some(vector_op(
                BuiltinOp::VecCompare(cmp),
                vec![lhs, rhs],
                ty,
                range,
            ));
        }
        let bin = super::arith_op(op).expect("gnu_vector_operands refuses anything else");
        Some(vector_op(
            BuiltinOp::VecBinary(bin),
            vec![lhs, rhs],
            vec,
            range,
        ))
    }

    /// The type of a comparison of two `vec`s: the signed integers as wide as
    /// the elements, as many — `long` for a `double` on LP64, as GCC has it.
    fn comparison_ty(&mut self, vec: Ty) -> Ty {
        let Ty::GnuVector(id) = vec else {
            return vec;
        };
        let shape = self.types().gnu_vector_type(id);
        let bytes = shape.elem.size_bytes(&self.target);
        let target = self.target;
        let elem = [
            Ty::SChar,
            Ty::Short,
            Ty::Int,
            Ty::Long,
            Ty::LongLong,
            Ty::Int128,
        ]
        .into_iter()
        .find(|ty| ty.size_bytes(&target) == bytes)
        .unwrap_or(Ty::Int);
        self.plain_vector(elem, shape.len)
    }

    /// Unary `+`, `-` and `~` on a GCC vector.
    pub(super) fn gnu_vector_unary(
        &mut self,
        op: ast::UnaryOp,
        value: Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        let ty = value.ty;
        match op {
            ast::UnaryOp::Plus => Some(value),
            ast::UnaryOp::Minus => Some(vector_op(BuiltinOp::VecNeg, vec![value], ty, range)),
            ast::UnaryOp::BitNot if self.elem_of(ty).is_integer() => {
                Some(vector_op(BuiltinOp::VecBitNot, vec![value], ty, range))
            }
            _ => {
                self.error(
                    range,
                    format!(
                        "wrong type argument to unary '{}' ('{}')",
                        op.as_str(),
                        self.tyname(ty)
                    ),
                );
                None
            }
        }
    }

    /// `place op= value` on a GCC vector place: the place is evaluated once,
    /// as for a scalar, and the operation is the binary operator's.
    pub(super) fn gnu_vector_compound(
        &mut self,
        op: ast::BinaryOp,
        place: Place,
        value: Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        let ty = place.ty;
        // The place's value only has to be typed here.
        let current = Expr::new(ExprKind::Zeroed, ty, place.range);
        let (_, value, vec) = self.gnu_vector_operands(op, current, value, range)?;
        let bin = super::arith_op(op).expect("every compound assignment operator is arithmetic");
        debug_assert_eq!(vec, ty, "the place is the left operand");
        Some(Expr::new(
            ExprKind::CompoundAssign {
                place,
                op: bin,
                value: Box::new(value),
                compute: ty,
            },
            ty,
            range,
        ))
    }

    /// `v[i]` on a GCC vector: one element, an lvalue when `v` is one; see
    /// [`Sema::lane_place`].
    pub(super) fn gnu_vector_index_place(
        &mut self,
        vector: Expr,
        index: Expr,
        range: SourceRange,
    ) -> Option<Place> {
        let Ty::GnuVector(id) = vector.ty else {
            unreachable!("gnu_vector_index_place is only called with a vector")
        };
        let shape = self.types().gnu_vector_type(id);
        self.lane_place(vector, index, shape.elem, i128::from(shape.len), range)
    }

    /// The elements of a brace initialiser of a GCC vector, each already
    /// converted to the element type, as the vector: the missing ones zero.
    pub(super) fn gnu_vector_from_lanes(
        &mut self,
        ty: Ty,
        mut values: Vec<Expr>,
        range: SourceRange,
    ) -> Expr {
        let Ty::GnuVector(id) = ty else {
            unreachable!("gnu_vector_from_lanes is only called with a vector")
        };
        let shape = self.types().gnu_vector_type(id);
        while values.len() < shape.len as usize {
            values.push(self.zero(shape.elem, range));
        }
        vector_op(BuiltinOp::VecLanes, values, ty, range)
    }

    /// The number of elements of a GCC vector type, and their type.
    pub(super) fn gnu_vector_lanes(&self, ty: Ty) -> (Ty, usize) {
        match ty {
            Ty::GnuVector(id) => {
                let shape = self.types().gnu_vector_type(id);
                (shape.elem, shape.len as usize)
            }
            _ => (ty, 1),
        }
    }

    /// `__builtin_convertvector(v, T)`: each element of `v` converted to the
    /// element type of `T`, which has as many.
    pub(super) fn convert_vector(
        &mut self,
        operand: &ast::Expr,
        type_name: &ast::TypeName,
        range: SourceRange,
    ) -> Option<Expr> {
        let value = self.expr(operand)?;
        let target = self.type_name_ty(type_name)?;
        if value.ty.is_error() || target.is_error() {
            return None;
        }
        let Some((_, from_len)) = self.elements_of(value.ty) else {
            self.error(
                operand.range,
                format!(
                    "the first argument of '__builtin_convertvector' must be a vector, not '{}'",
                    self.tyname(value.ty)
                ),
            );
            return None;
        };
        let Ty::GnuVector(to) = target else {
            self.error(
                type_name.range,
                format!(
                    "the second argument of '__builtin_convertvector' must be a vector type, not \
                     '{}'",
                    self.tyname(target)
                ),
            );
            return None;
        };
        let to_len = self.types().gnu_vector_type(to).len;
        if from_len != to_len {
            self.error(
                range,
                format!(
                    "'__builtin_convertvector' number of elements of the first argument vector \
                     ({from_len}) and the second argument vector type ({to_len}) should be the \
                     same"
                ),
            );
            return None;
        }
        if value.ty == target {
            return Some(value);
        }
        // An Intel vector is converted as the GCC vector of its elements.
        let value = match value.ty {
            Ty::Vector(vec) => {
                let (elem, len) = intel_elements(vec).expect("elements_of answered");
                let ty = self.plain_vector(elem, len);
                Self::gnu_vector_retyped(value, ty)
            }
            _ => value,
        };
        Some(vector_op(BuiltinOp::VecConvert, vec![value], target, range))
    }

    /// `__builtin_shuffle(a, mask)` and `__builtin_shuffle(a, b, mask)`.
    ///
    /// GCC's rules for C: `a` and `b` are vectors of one type, and `mask` an
    /// integer vector with as many elements, each as wide as theirs. Element
    /// `i` of the result is element `mask[i]` of `a` — or of `a` and `b` side
    /// by side, `b`'s numbered from `a`'s count — the index taken modulo the
    /// number of elements there are, which is a power of two.
    pub(super) fn gnu_vector_shuffle(
        &mut self,
        name: &str,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        if !(2..=3).contains(&args.len()) {
            self.error(
                range,
                format!(
                    "'{name}' takes two or three arguments, the vectors and the mask, and was \
                     given {}",
                    args.len()
                ),
            );
            return None;
        }
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.expr(arg)?);
        }
        if values.iter().any(|value| value.ty.is_error()) {
            return None;
        }
        let mask = values.pop().expect("two or three");
        let first = values[0].ty;
        let Some((elem, len)) = self.elements_of(first).filter(|_| first.is_gnu_vector()) else {
            self.error(
                args[0].range,
                format!(
                    "'{name}' shuffles a vector, and the first argument is a '{}'",
                    self.tyname(first)
                ),
            );
            return None;
        };
        if let Some(second) = values.get(1).map(|value| value.ty)
            && second != first
        {
            self.error(
                args[1].range,
                format!(
                    "'{name}' argument vectors must be of the same type ('{}' and '{}')",
                    self.tyname(first),
                    self.tyname(second)
                ),
            );
            return None;
        }
        let mask_shape = self
            .elements_of(mask.ty)
            .filter(|(mask_elem, _)| mask.ty.is_gnu_vector() && mask_elem.is_integer());
        let Some((mask_elem, mask_len)) = mask_shape else {
            self.error(
                mask.range,
                format!(
                    "'{name}' last argument must be an integer vector, not '{}'",
                    self.tyname(mask.ty)
                ),
            );
            return None;
        };
        if mask_len != len {
            self.error(
                range,
                format!(
                    "'{name}' number of elements of the argument vector(s) ({len}) and the mask \
                     vector ({mask_len}) should be the same"
                ),
            );
            return None;
        }
        if mask_elem.size_bytes(&self.target) != elem.size_bytes(&self.target) {
            self.error(
                range,
                format!(
                    "'{name}' argument vector(s) inner type must have the same size as inner \
                     type of the mask"
                ),
            );
            return None;
        }
        values.push(mask);
        Some(vector_op(BuiltinOp::VecShuffle, values, first, range))
    }

    /// Whether a value of `ty` holds a GCC vector by value, itself or as a
    /// member or an element.
    pub(super) fn holds_gnu_vector(&self, ty: Ty) -> bool {
        match ty {
            Ty::GnuVector(_) => true,
            Ty::Array(id) => self.holds_gnu_vector(self.types().array_type(id).elem),
            Ty::Record(id) => self
                .types()
                .record(id)
                .fields
                .iter()
                .any(|field| self.holds_gnu_vector(field.ty)),
            _ => false,
        }
    }

    /// Refuses a call that passes or returns a GCC vector by value to a
    /// function the unit does not define: GCC passes one in a vector register,
    /// and the struct it is generated as goes where a struct goes. `true`
    /// says the call was refused.
    pub(super) fn refuse_vector_call(
        &mut self,
        target: &Callee,
        name: &str,
        sig: &Signature,
        range: SourceRange,
    ) -> bool {
        let Callee::Direct(id) = target else {
            return false;
        };
        if self.defined_functions.contains(name) || self.program.function(*id).intrinsic.is_some() {
            return false;
        }
        let Some(ty) = sig
            .params
            .iter()
            .copied()
            .chain(std::iter::once(sig.ret))
            .find(|ty| self.holds_gnu_vector(*ty))
        else {
            return false;
        };
        self.error(
            range,
            format!(
                "'{name}' passes or returns the vector type '{}' by value, and this unit does not \
                 define it: GCC passes a vector in a vector register, while cinrs passes its \
                 vector types as a struct of their elements, so a function compiled elsewhere \
                 would not find the value where it looks. Pass a pointer to it instead",
                self.tyname(ty)
            ),
        );
        true
    }

    /// Refuses a GCC vector in the variable part of a call, which GCC passes
    /// in a vector register and `va_arg` could not read back here.
    pub(super) fn refuse_variadic_vector(&mut self, value: &Expr, range: SourceRange) -> bool {
        if !self.holds_gnu_vector(value.ty) {
            return false;
        }
        self.error(
            range,
            format!(
                "passing the vector type '{}' as a variadic argument is not supported: GCC \
                 passes it in a vector register, and 'va_arg' has no way to read it back here. \
                 Pass a pointer to it instead",
                self.tyname(value.ty)
            ),
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::intel_elements;
    use crate::ir::{Ty, VecTy};

    #[test]
    fn the_intel_types_are_gccs_vectors() {
        assert_eq!(intel_elements(VecTy::M128i), Some((Ty::LongLong, 2)));
        assert_eq!(intel_elements(VecTy::M256), Some((Ty::Float, 8)));
        assert_eq!(intel_elements(VecTy::M128h), None);
    }
}
