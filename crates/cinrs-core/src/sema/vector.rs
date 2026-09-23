//! GCC's operators on the Intel vector types, lowered to the intrinsics.
//!
//! In GCC and Clang `__m128d` is `double __attribute__((vector_size(16)))`, so
//! real SIMD code writes `a * b`, `v * 2.0`, `1.0 / v`, `-v` and `v += w` on
//! the Intel types as often as it writes `_mm_mul_pd`. cinrs has no vector
//! arithmetic of its own — the vectors are `core::arch`'s opaque types — so
//! each operator is *lowered here to the intrinsic that does the same*, a call
//! exactly like one the program wrote: it names the function the included
//! header declares, so the `#[target_feature]`, the `[[cinrs::safe]]` check
//! and everything else about intrinsic calls apply unchanged.
//!
//! The lane type is GCC's: `float` for `__m128`/`__m256`/`__m512`, `double`
//! for the `…d` types, and `long long` for the `…i` types. What maps:
//!
//! | operator | float/double lanes | 64-bit integer lanes |
//! | --- | --- | --- |
//! | `+ - * /` | `add`/`sub`/`mul`/`div` `_ps`/`_pd` | `+ -`: `add`/`sub` `_epi64` |
//! | `& \| ^` | `and`/`or`/`xor` `_ps`/`_pd` (512-bit: through `_si512`) | `and`/`or`/`xor` `_si128`/`_si256`/`_si512` |
//! | unary `-` | `xor` with `set1(-0.0)`, GCC's sign flip | `sub_epi64` from zero |
//! | `~` | refused, as GCC refuses it | `xor` with `set1_epi32(-1)` |
//! | `== != < <= > >=` | 128-bit: `cmp{eq,neq,lt,le,gt,ge}`; 256-bit: `cmp` with `_CMP_*_OQ`/`_CMP_NEQ_UQ`; the lanes as the same-size integer vector | refused |
//!
//! A scalar operand is converted to the lane type, as an argument would be,
//! and broadcast with `set1`. Everything else — integer `*`, `/`, `%`, the
//! shifts, integer comparisons, the 512-bit comparisons, the bfloat16 and
//! half-precision vectors, `!`, `&&`, `||` — is refused with the intrinsic to
//! write instead: GCC's 64-bit-lane meaning has no single instruction there,
//! and a guess would be worse than the diagnostic.

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{self, BinOp, Callee, CmpOp, Expr, ExprKind, Place, PlaceKind, Ty, VecTy};

use super::{ConvContext, Entry, Sema};

/// What a vector's lanes are, in GCC's reading of the Intel type.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lane {
    F32,
    F64,
    I64,
}

/// A vector type's lanes and the prefix its intrinsics carry.
#[derive(Clone, Copy)]
struct Shape {
    lane: Lane,
    /// `_mm`, `_mm256` or `_mm512`.
    prefix: &'static str,
    bits: u32,
}

impl Shape {
    fn of(vec: VecTy) -> Option<Shape> {
        let (lane, bits) = match vec {
            VecTy::M128 => (Lane::F32, 128),
            VecTy::M128d => (Lane::F64, 128),
            VecTy::M128i => (Lane::I64, 128),
            VecTy::M256 => (Lane::F32, 256),
            VecTy::M256d => (Lane::F64, 256),
            VecTy::M256i => (Lane::I64, 256),
            VecTy::M512 => (Lane::F32, 512),
            VecTy::M512d => (Lane::F64, 512),
            VecTy::M512i => (Lane::I64, 512),
            VecTy::M128bh
            | VecTy::M256bh
            | VecTy::M512bh
            | VecTy::M128h
            | VecTy::M256h
            | VecTy::M512h => return None,
        };
        let prefix = match bits {
            128 => "_mm",
            256 => "_mm256",
            _ => "_mm512",
        };
        Some(Shape { lane, prefix, bits })
    }

    /// `ps` or `pd`, for a floating vector.
    fn fsuffix(self) -> &'static str {
        if self.lane == Lane::F32 { "ps" } else { "pd" }
    }

    /// `si128`, `si256` or `si512`.
    fn isuffix(self) -> &'static str {
        match self.bits {
            128 => "si128",
            256 => "si256",
            _ => "si512",
        }
    }

    fn name(self, op: &str, suffix: &str) -> String {
        format!("{}_{op}_{suffix}", self.prefix)
    }

    /// The broadcast of one lane: `set1_ps`, `set1_pd`, `set1_epi64x`, or
    /// `_mm512_set1_epi64`.
    fn set1(self) -> String {
        match self.lane {
            Lane::I64 if self.bits == 512 => self.name("set1", "epi64"),
            Lane::I64 => self.name("set1", "epi64x"),
            _ => self.name("set1", self.fsuffix()),
        }
    }
}

/// Whether evaluating `place` twice is the same as evaluating it once: no
/// calls, no stores, nothing volatile about the *address*. A compound
/// assignment to a vector is lowered to `place = op(place, value)`, which
/// computes the address twice.
fn place_is_repeatable(place: &Place) -> bool {
    match &place.kind {
        PlaceKind::Object(_) => true,
        PlaceKind::Field { base, .. } => place_is_repeatable(base),
        PlaceKind::Deref(ptr) => expr_is_repeatable(ptr),
        PlaceKind::Index { base, index } => expr_is_repeatable(base) && expr_is_repeatable(index),
        _ => false,
    }
}

fn expr_is_repeatable(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Int(_) | ExprKind::Float(_) => true,
        ExprKind::Load(place) | ExprKind::AddrOf(place) => place_is_repeatable(place),
        ExprKind::Cast(inner) | ExprKind::Neg(inner) | ExprKind::BitNot(inner) => {
            expr_is_repeatable(inner)
        }
        ExprKind::Binary { lhs, rhs, .. } => expr_is_repeatable(lhs) && expr_is_repeatable(rhs),
        ExprKind::PtrOffset { ptr, index, .. } => {
            expr_is_repeatable(ptr) && expr_is_repeatable(index)
        }
        _ => false,
    }
}

impl Sema<'_> {
    /// The operand is a vector, or one of two operands is: whatever GCC's
    /// vector extension gives the operator, as a call to the intrinsic.
    pub(super) fn vector_binary(
        &mut self,
        op: ast::BinaryOp,
        lhs: Expr,
        rhs: Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        let (vec, lhs_is_vector) = match (lhs.ty, rhs.ty) {
            (Ty::Vector(a), Ty::Vector(b)) if a != b => {
                self.error(
                    range,
                    format!(
                        "invalid operands to binary '{}' ('{}' and '{}'): the two vectors have \
                         different types, and GCC does not convert one to the other",
                        op.as_str(),
                        a.name(),
                        b.name()
                    ),
                );
                return None;
            }
            (Ty::Vector(a), _) => (a, true),
            (_, Ty::Vector(b)) => (b, false),
            _ => unreachable!("vector_binary is only called with a vector operand"),
        };
        let shape = self.vector_shape(vec, op.as_str(), range)?;

        // The scalar side is broadcast; a pointer or a record is not a scalar
        // of any lane type.
        let (lhs, rhs) = if lhs_is_vector && rhs.ty == lhs.ty {
            (lhs, rhs)
        } else {
            let scalar = if lhs_is_vector { &rhs } else { &lhs };
            if !scalar.ty.is_arithmetic() || scalar.ty.is_complex() {
                let (a, b) = (self.tyname(lhs.ty), self.tyname(rhs.ty));
                self.error(
                    range,
                    format!(
                        "invalid operands to binary '{}' ('{a}' and '{b}'): a vector operator \
                         takes a vector of the same type or a real arithmetic scalar, which is \
                         converted to the lane type and broadcast",
                        op.as_str()
                    ),
                );
                return None;
            }
            if lhs_is_vector {
                let splat = self.vector_call(&shape.set1(), vec![rhs], range)?;
                (lhs, splat)
            } else {
                let splat = self.vector_call(&shape.set1(), vec![lhs], range)?;
                (splat, rhs)
            }
        };

        if let Some(cmp) = super::compare_op(op) {
            return self.vector_compare(cmp, vec, shape, lhs, rhs, range);
        }
        let Some(bin) = super::arith_op(op) else {
            self.error(
                range,
                format!(
                    "'{}' is not defined on the vector type '{}'",
                    op.as_str(),
                    vec.name()
                ),
            );
            return None;
        };
        self.vector_arith(bin, vec, shape, lhs, rhs, range)
    }

    /// `+ - * / & | ^` on two operands of the same vector type.
    fn vector_arith(
        &mut self,
        bin: BinOp,
        vec: VecTy,
        shape: Shape,
        lhs: Expr,
        rhs: Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        let bitwise = match bin {
            BinOp::BitAnd => Some("and"),
            BinOp::BitOr => Some("or"),
            BinOp::BitXor => Some("xor"),
            _ => None,
        };
        if shape.lane == Lane::I64 {
            let name = match (bin, bitwise) {
                (_, Some(word)) => shape.name(word, shape.isuffix()),
                (BinOp::Add, _) => shape.name("add", "epi64"),
                (BinOp::Sub, _) => shape.name("sub", "epi64"),
                _ => {
                    let op = bin_spelling(bin);
                    self.error(
                        range,
                        format!(
                            "'{op}' on '{}' is not supported: GCC reads the type as two, four or \
                             eight 64-bit lanes, and SSE and AVX have no single instruction for \
                             that. Write the intrinsic for the lanes you mean — _mm_mullo_epi32, \
                             _mm_mul_epu32, _mm_slli_epi64, _mm_srli_epi64, _mm_srai_epi32 and \
                             their 256- and 512-bit forms",
                            vec.name()
                        ),
                    );
                    return None;
                }
            };
            return self.vector_call(&name, vec![lhs, rhs], range);
        }
        if let Some(word) = bitwise {
            return self.vector_float_bitwise(word, shape, lhs, rhs, range);
        }
        let word = match bin {
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::Mul => "mul",
            BinOp::Div => "div",
            _ => {
                self.error(
                    range,
                    format!(
                        "'{}' is not defined on the floating vector type '{}' (GCC refuses it \
                         too)",
                        bin_spelling(bin),
                        vec.name()
                    ),
                );
                return None;
            }
        };
        self.vector_call(&shape.name(word, shape.fsuffix()), vec![lhs, rhs], range)
    }

    /// `& | ^` on float or double lanes. The 128- and 256-bit forms are
    /// intrinsics of their own; the 512-bit ones are AVX512DQ's, so for
    /// AVX-512F alone they go through the integer form and the free casts.
    fn vector_float_bitwise(
        &mut self,
        word: &str,
        shape: Shape,
        lhs: Expr,
        rhs: Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        if shape.bits != 512 {
            return self.vector_call(&shape.name(word, shape.fsuffix()), vec![lhs, rhs], range);
        }
        let to_int = format!("_mm512_cast{}_si512", shape.fsuffix());
        let to_float = format!("_mm512_castsi512_{}", shape.fsuffix());
        let a = self.vector_call(&to_int, vec![lhs], range)?;
        let b = self.vector_call(&to_int, vec![rhs], range)?;
        let both = self.vector_call(&shape.name(word, "si512"), vec![a, b], range)?;
        self.vector_call(&to_float, vec![both], range)
    }

    /// `== != < <= > >=` on float or double lanes: all ones in a lane where
    /// the comparison holds, typed as the integer vector of the same size.
    fn vector_compare(
        &mut self,
        cmp: CmpOp,
        vec: VecTy,
        shape: Shape,
        lhs: Expr,
        rhs: Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        if shape.lane == Lane::I64 {
            self.error(
                range,
                format!(
                    "comparing '{}' vectors is not supported: GCC compares them as 64-bit lanes, \
                     which SSE2 cannot do in one instruction. Write the intrinsic for the lanes \
                     you mean — _mm_cmpeq_epi32, _mm_cmpgt_epi32, _mm_cmpeq_epi64 (SSE4.1), \
                     _mm_cmpgt_epi64 (SSE4.2) and their 256-bit forms",
                    vec.name()
                ),
            );
            return None;
        }
        if shape.bits == 512 {
            self.error(
                range,
                format!(
                    "comparing '{}' vectors is not supported: an AVX-512 comparison produces a \
                     mask, not a vector. Write _mm512_cmp_{}_mask(a, b, _CMP_LT_OQ) and use the \
                     __mmask it returns",
                    vec.name(),
                    shape.fsuffix()
                ),
            );
            return None;
        }
        let suffix = shape.fsuffix();
        let compared = if shape.bits == 128 {
            let word = match cmp {
                CmpOp::Eq => "cmpeq",
                CmpOp::Ne => "cmpneq",
                CmpOp::Lt => "cmplt",
                CmpOp::Le => "cmple",
                CmpOp::Gt => "cmpgt",
                CmpOp::Ge => "cmpge",
            };
            self.vector_call(&shape.name(word, suffix), vec![lhs, rhs], range)?
        } else {
            // `_CMP_EQ_OQ`, `_CMP_NEQ_UQ`, `_CMP_LT_OQ`, `_CMP_LE_OQ`,
            // `_CMP_GT_OQ`, `_CMP_GE_OQ`.
            let predicate = match cmp {
                CmpOp::Eq => 0x00,
                CmpOp::Ne => 0x04,
                CmpOp::Lt => 0x11,
                CmpOp::Le => 0x12,
                CmpOp::Gt => 0x1e,
                CmpOp::Ge => 0x1d,
            };
            let imm = Expr::int(predicate, Ty::Int, range);
            self.vector_call(&shape.name("cmp", suffix), vec![lhs, rhs, imm], range)?
        };
        let cast = if shape.bits == 128 {
            format!("_mm_cast{suffix}_si128")
        } else {
            format!("_mm256_cast{suffix}_si256")
        };
        self.vector_call(&cast, vec![compared], range)
    }

    /// Unary `-`, `+` and `~` on a vector operand.
    pub(super) fn vector_unary(
        &mut self,
        op: ast::UnaryOp,
        value: Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        let Ty::Vector(vec) = value.ty else {
            unreachable!("vector_unary is only called with a vector operand")
        };
        let shape = self.vector_shape(vec, op.as_str(), range)?;
        match op {
            ast::UnaryOp::Plus => Some(value),
            ast::UnaryOp::Minus if shape.lane == Lane::I64 => {
                let zero =
                    self.vector_call(&shape.name("setzero", shape.isuffix()), vec![], range)?;
                self.vector_call(&shape.name("sub", "epi64"), vec![zero, value], range)
            }
            ast::UnaryOp::Minus => {
                // GCC flips the sign bit, which is exact for zeros, NaNs and
                // infinities alike: `-v` is `v ^ -0.0`.
                let lane = if shape.lane == Lane::F32 {
                    Ty::Float
                } else {
                    Ty::Double
                };
                let minus_zero = Expr::new(ExprKind::Float(-0.0), lane, range);
                let sign = self.vector_call(&shape.set1(), vec![minus_zero], range)?;
                self.vector_float_bitwise("xor", shape, value, sign, range)
            }
            ast::UnaryOp::BitNot if shape.lane == Lane::I64 => {
                let ones = Expr::int(-1, Ty::Int, range);
                let ones = self.vector_call(&shape.name("set1", "epi32"), vec![ones], range)?;
                self.vector_call(
                    &shape.name("xor", shape.isuffix()),
                    vec![value, ones],
                    range,
                )
            }
            _ => {
                self.error(
                    range,
                    format!(
                        "invalid operand of type '{}' to unary operator '{}'",
                        vec.name(),
                        op.as_str()
                    ),
                );
                None
            }
        }
    }

    /// `place op= value` on a vector place, as `place = place op value`.
    pub(super) fn vector_compound(
        &mut self,
        op: ast::BinaryOp,
        place: Place,
        value: Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        let ty = place.ty;
        if !place_is_repeatable(&place) {
            self.error(
                range,
                format!(
                    "'{}' on a vector is supported where the object is named directly — a \
                     variable, a member, an element or '*p' — and this one is reached through \
                     an expression with side effects. Take its address into a pointer first",
                    op.as_str()
                ),
            );
            return None;
        }
        let current = Expr::new(ExprKind::Load(place.clone()), ty, place.range);
        let result = self.vector_binary(op, current, value, range)?;
        if result.ty != ty {
            self.error(
                range,
                format!(
                    "'{}' on '{}' does not give a '{}'",
                    op.as_str(),
                    self.tyname(ty),
                    self.tyname(ty)
                ),
            );
            return None;
        }
        Some(Expr::new(
            ExprKind::Assign {
                place,
                value: Box::new(result),
            },
            ty,
            range,
        ))
    }

    /// The lane type of `vec` and how many lanes it has, or `None` (with the
    /// refusal reported) for the bfloat16 and half-precision vectors.
    pub(super) fn vector_lanes(&mut self, vec: VecTy, range: SourceRange) -> Option<(Ty, usize)> {
        let shape = self.vector_shape(vec, "{ }", range)?;
        Some(match shape.lane {
            Lane::F32 => (Ty::Float, shape.bits as usize / 32),
            Lane::F64 => (Ty::Double, shape.bits as usize / 64),
            Lane::I64 => (Ty::LongLong, shape.bits as usize / 64),
        })
    }

    /// The vector whose lanes are `values`, in memory order, each already of
    /// the lane type; missing lanes are zero, as they are in GCC. `{ }` is
    /// `setzero`, and anything else `setr` — or `_mm_set_epi64x` high lane
    /// first, the one 128-bit integer form SSE2 has.
    pub(super) fn vector_from_lanes(
        &mut self,
        vec: VecTy,
        mut values: Vec<Expr>,
        range: SourceRange,
    ) -> Option<Expr> {
        let shape = self.vector_shape(vec, "{ }", range)?;
        let (lane, lanes) = self.vector_lanes(vec, range)?;
        if values.is_empty() {
            let suffix = match shape.lane {
                Lane::I64 => shape.isuffix(),
                _ => shape.fsuffix(),
            };
            return self.vector_call(&shape.name("setzero", suffix), vec![], range);
        }
        while values.len() < lanes {
            let zero = if lane.is_integer() {
                Expr::int(0, lane, range)
            } else {
                Expr::new(ExprKind::Float(0.0), lane, range)
            };
            values.push(zero);
        }
        let name = match (shape.lane, shape.bits) {
            (Lane::I64, 128) => {
                values.reverse();
                "_mm_set_epi64x".to_owned()
            }
            (Lane::I64, 256) => "_mm256_setr_epi64x".to_owned(),
            (Lane::I64, _) => "_mm512_setr_epi64".to_owned(),
            _ => shape.name("setr", shape.fsuffix()),
        };
        self.vector_call(&name, values, range)
    }

    /// `v[i]`: one lane of a vector, as GCC's vector extension reads it.
    ///
    /// It is `*((lane *)&v + i)`, which is what code wrote for itself before
    /// the extension had subscripts — an lvalue when `v` is one, so `v[1] = x`
    /// and `v[0] += y` store into the vector's own memory. A vector that is
    /// not an object (`(a * b)[0]`, a call's result) is held in a temporary
    /// first, the way `f().x` is. There is no bounds check, as GCC has none;
    /// a constant index outside the lanes is an error, as it is in GCC.
    pub(super) fn vector_index_place(
        &mut self,
        vector: Expr,
        index: Expr,
        range: SourceRange,
    ) -> Option<Place> {
        let Ty::Vector(vec) = vector.ty else {
            unreachable!("vector_index_place is only called with a vector")
        };
        let shape = self.vector_shape(vec, "[]", range)?;
        if !index.ty.is_integer() {
            self.error(
                index.range,
                format!(
                    "array subscript is not an integer ('{}' invalid)",
                    self.tyname(index.ty)
                ),
            );
            return None;
        }
        let (lane, lane_bits) = match shape.lane {
            Lane::F32 => (Ty::Float, 32),
            Lane::F64 => (Ty::Double, 64),
            Lane::I64 => (Ty::LongLong, 64),
        };
        let lanes = i128::from(shape.bits / lane_bits);
        if let Some(ir::ConstValue::Int(at)) = self.const_eval(&index)
            && !(0..lanes).contains(&at)
        {
            self.error(
                index.range,
                format!(
                    "index {at} is out of range for '{}', which has {lanes} lanes of '{}'",
                    vec.name(),
                    self.tyname(lane)
                ),
            );
            return None;
        }
        let ty = vector.ty;
        let vector_range = vector.range;
        let place = match vector.kind {
            ExprKind::Load(place) => place,
            // A vector that is not an object gets one, the way a compound
            // literal does: a hidden local at the head of the block,
            // initialised where the expression stands. A pointer to its lanes
            // then stays valid for as long as the lane is read — which a
            // temporary scoped to the expression would not.
            kind if !self.at_file_scope() => {
                let name = self.anonymous_name("lanes");
                let id = self.new_object(&name, ty, ir::Storage::Automatic, false, vector_range);
                self.compound_literals.push(id);
                self.rvalue_lanes.insert(id);
                super::place_of(
                    PlaceKind::CompoundLiteral {
                        object: id,
                        init: Box::new(Expr::new(kind, ty, vector_range)),
                    },
                    ty,
                    false,
                    vector_range,
                )
            }
            kind => super::place_of(
                PlaceKind::Temporary(Box::new(Expr::new(kind, ty, vector_range))),
                ty,
                false,
                vector_range,
            ),
        };
        let konst = place.is_const;
        let address = Expr::new(
            ExprKind::AddrOf(place),
            self.ptr_to(ty, konst),
            vector_range,
        );
        let lanes_ptr = Expr::new(
            ExprKind::Cast(Box::new(address)),
            self.ptr_to(lane, konst),
            vector_range,
        );
        Some(super::place_of(
            PlaceKind::Index {
                base: Box::new(lanes_ptr),
                index: Box::new(index),
            },
            lane,
            konst,
            range,
        ))
    }

    /// The shape of `vec`, or the refusal for the bfloat16 and half-precision
    /// vectors, which GCC gives operators and this crate does not.
    fn vector_shape(&mut self, vec: VecTy, op: &str, range: SourceRange) -> Option<Shape> {
        let shape = Shape::of(vec);
        if shape.is_none() {
            self.error(
                range,
                format!(
                    "'{op}' on '{}' is not supported: the bfloat16 and half-precision vectors \
                     have no operators here. Write the intrinsic — _mm512_add_ph, \
                     _mm512_mul_ph, _mm512_dpbf16_ps and their relatives",
                    vec.name()
                ),
            );
        }
        shape
    }

    /// A call to the intrinsic `name`, exactly as if the program had written
    /// it: the function the included header declares, the arguments
    /// converted to its parameters, and an edge in the call graph for the
    /// `[[cinrs::safe]]` check.
    fn vector_call(&mut self, name: &str, args: Vec<Expr>, range: SourceRange) -> Option<Expr> {
        let id = match self.lookup_linked(name) {
            Some(Entry::Function(id)) if self.program.function(*id).intrinsic.is_some() => *id,
            _ => {
                self.error(
                    range,
                    format!(
                        "this vector operator is '{name}', which is not declared here: include \
                         <immintrin.h>, which declares it"
                    ),
                );
                return None;
            }
        };
        if let Some(frame) = self.nest.last() {
            let caller = frame.func;
            self.program.calls.push(ir::CallEdge {
                caller,
                callee: id,
                range,
            });
        }
        let sig = self.program.function(id).sig.clone();
        let args = args
            .into_iter()
            .zip(sig.params.iter())
            .enumerate()
            .map(|(index, (arg, param))| {
                self.convert_for(
                    arg,
                    *param,
                    ConvContext::Argument {
                        index: index + 1,
                        func: name.to_owned(),
                    },
                )
            })
            .collect();
        Some(Expr::new(
            ExprKind::Call {
                callee: Callee::Direct(id),
                args,
            },
            sig.ret,
            range,
        ))
    }
}

/// The C spelling of an arithmetic operator, for a diagnostic.
fn bin_spelling(bin: BinOp) -> &'static str {
    match bin {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
    }
}
