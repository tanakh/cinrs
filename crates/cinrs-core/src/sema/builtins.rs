//! The `__builtin_*` forms that are not `<stdarg.h>`'s.
//!
//! GCC's builtins fall into three groups, and each is handled differently:
//!
//! * the ones that are really *operations* — `__builtin_clz`, the overflow
//!   builtins, `__builtin_expect` — become an [`ir::ExprKind::Builtin`] node
//!   that code generation writes out as inline Rust;
//! * the ones that are really *questions about the program* —
//!   `__builtin_constant_p`, `__builtin_types_compatible_p`,
//!   `__builtin_object_size` — are answered here and become constants;
//! * the ones that are a standard library function under another name —
//!   `__builtin_memcpy`, `__builtin_strlen` — become an ordinary call to that
//!   function, which is declared into the unit's `extern` block if the header
//!   that would have declared it was not included.
//!
//! Which names exist at all is [`crate::gnu`]'s table, so `__has_builtin`
//! answers about this implementation rather than about GCC's.
//!
//! The atomic families — `__atomic_*`, `__sync_*` and `__c11_atomic_*`, none
//! of them spelled `__builtin_` — are [`super::atomics`]'s, and are dispatched
//! from here before the prefix is looked at. Their name tables live there and
//! `crate::gnu::has_builtin` asks them, so the two answers cannot drift apart
//! any more than the `__builtin_` ones can.

use crate::ast;
use crate::capture::SourceRange;
use crate::gnu;
use crate::ir::{
    self, BinOp, BuiltinOp, Expr, ExprKind, FloatClass, FloatOrder, FuncId, Function, Signature, Ty,
};
use crate::target::Arch;

use super::{Entry, Sema};

/// What `__builtin_object_size` answers when it cannot work the size out.
///
/// The low bit of the mode selects between "the maximum" and "the minimum";
/// GCC's own documentation gives `(size_t) -1` and `0` as the two answers, and
/// a front end that does not track object sizes has to give exactly those.
fn object_size_answer(mode: i128) -> i128 {
    if mode & 2 == 0 { -1 } else { 0 }
}

/// The payload `__builtin_nan("…")`'s string names.
///
/// GCC reads it with `strtoull` in base 0 — so `"0x123"`, `"0123"` and `"291"`
/// are the same payload — and answers 0 for anything it cannot read, which is
/// what makes `__builtin_nan("")` the default quiet NaN.
fn parse_nan_payload(text: &str) -> u64 {
    let text = text.trim_start();
    let (radix, digits) = match text.as_bytes() {
        [b'0', b'x' | b'X', rest @ ..] => (16, rest),
        [b'0', rest @ ..] if !rest.is_empty() => (8, rest),
        _ => (10, text.as_bytes()),
    };
    let mut value = 0u64;
    for byte in digits {
        let Some(digit) = char::from(*byte).to_digit(radix) else {
            break;
        };
        value = value
            .wrapping_mul(u64::from(radix))
            .wrapping_add(digit.into());
    }
    value
}

/// Whether `__builtin_constant_p` says yes to something that is not a number.
///
/// A string literal is the one object whose address GCC calls constant — it is
/// in the constant pool, and asking for a character out of one at a constant
/// index folds too. The address of a *variable* is not: it is the linker that
/// decides it, which is why `bcp-1` requires `__builtin_constant_p(&global)`
/// to be zero and `__builtin_constant_p("hi")` to be one.
fn constant_string(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Cast(inner) => constant_string(inner),
        ExprKind::AddrOf(place) => matches!(place.kind, ir::PlaceKind::Str(_)),
        ExprKind::Load(place) => match &place.kind {
            ir::PlaceKind::Str(_) => true,
            ir::PlaceKind::Index { base, index } => {
                matches!(index.kind, ExprKind::Int(_)) && constant_string(base)
            }
            _ => false,
        },
        _ => false,
    }
}

impl Sema<'_> {
    /// Checks a call to a `__builtin_*` form.
    ///
    /// `None` means the name is not one of ours and the call is an ordinary
    /// one; `Some(None)` means it was, and something was wrong with it.
    pub(super) fn builtin_call(
        &mut self,
        name: &str,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Option<Expr>> {
        // The three atomic families are overloaded on the type of the object
        // rather than named for it; see [`super::atomics`].
        if let Some(result) = self.atomic_builtin(name, args, range) {
            return Some(result);
        }
        let rest = name.strip_prefix("__builtin_")?;
        // The typed overflow builtins are the generic ones with the result
        // type spelled out in the name.
        if let Some(op) = gnu::typed_overflow(rest) {
            let op = match op {
                "add" => BinOp::Add,
                "sub" => BinOp::Sub,
                _ => BinOp::Mul,
            };
            return Some(self.overflow(name, op, args, true, range));
        }
        let result = match rest {
            // `__builtin_expect(value, expected)` is `value`; the hint has
            // nowhere to go, since `core::hint::likely` is unstable.
            "expect" | "expect_with_probability" => {
                let arity = if rest == "expect" { 2 } else { 3 };
                self.builtin_arity(name, args, arity, range)?;
                let value = self.expr(&args[0])?;
                for arg in &args[1..] {
                    self.expr(arg);
                }
                // GCC gives the builtin the type `long`, and code that assigns
                // the result somewhere narrower converts as usual.
                Some(self.convert(value, Ty::Long))
            }
            "trap" => {
                self.builtin_arity(name, args, 0, range)?;
                // `core::intrinsics::abort` is unstable and `std::process` is
                // not available to a `no_std` crate; the C library's `abort`
                // is what a translated program should call anyway.
                self.library_call("abort", &[], range)
            }
            "constant_p" => {
                self.builtin_arity(name, args, 1, range)?;
                let value = self.expr(&args[0])?;
                let folds = self.const_eval(&value).is_some() || constant_string(&value);
                Some(Expr::int(i128::from(folds), Ty::Int, range))
            }
            // GCC's `__builtin_cpu_init` runs the `cpuid` queries that fill the
            // table `__builtin_cpu_supports` reads, and is only needed from a
            // constructor that runs before the library's own initialiser.
            // `std_detect` does its own lazy one-time detection, so there is
            // nothing to run: the call evaluates to nothing, which is what a
            // `void` builtin is.
            "cpu_init" => {
                self.builtin_arity(name, args, 0, range)?;
                Some(Expr::new(
                    ExprKind::Builtin {
                        op: BuiltinOp::Discard,
                        args: Vec::new(),
                    },
                    Ty::Void,
                    range,
                ))
            }
            "cpu_supports" => self.cpu_supports(args, range),
            "popcount" | "popcountl" | "popcountll" => {
                self.bit_builtin(name, BuiltinOp::Popcount, args, range)
            }
            "clz" | "clzl" | "clzll" => self.bit_builtin(name, BuiltinOp::Clz, args, range),
            "ctz" | "ctzl" | "ctzll" => self.bit_builtin(name, BuiltinOp::Ctz, args, range),
            "ffs" | "ffsl" | "ffsll" => self.bit_builtin(name, BuiltinOp::Ffs, args, range),
            "parity" | "parityl" | "parityll" => {
                self.bit_builtin(name, BuiltinOp::Parity, args, range)
            }
            "clrsb" | "clrsbl" | "clrsbll" => self.bit_builtin(name, BuiltinOp::Clrsb, args, range),
            "bswap16" | "bswap32" | "bswap64" => self.bswap(name, args, range),
            "add_overflow" => self.overflow(name, BinOp::Add, args, true, range),
            "sub_overflow" => self.overflow(name, BinOp::Sub, args, true, range),
            "mul_overflow" => self.overflow(name, BinOp::Mul, args, true, range),
            "add_overflow_p" => self.overflow(name, BinOp::Add, args, false, range),
            "sub_overflow_p" => self.overflow(name, BinOp::Sub, args, false, range),
            "mul_overflow_p" => self.overflow(name, BinOp::Mul, args, false, range),
            // `long double` is `double` here, so the `l` forms are the plain
            // ones; `huge_val` and `inf` differ only in that the first is
            // `<math.h>`'s spelling of the second.
            "huge_val" | "huge_vall" | "inf" | "infl" => {
                self.builtin_arity(name, args, 0, range)?;
                Some(Expr::new(ExprKind::Float(f64::INFINITY), Ty::Double, range))
            }
            "huge_valf" | "inff" => {
                self.builtin_arity(name, args, 0, range)?;
                Some(Expr::new(ExprKind::Float(f64::INFINITY), Ty::Float, range))
            }
            "nan" | "nanl" | "nanf" | "nans" | "nansl" | "nansf" => {
                self.nan_builtin(name, rest, args, range)
            }
            // TS 18661-3's spellings, which glibc's `<math.h>` writes
            // `HUGE_VAL_F32`, `INFINITY`'s relatives and `SNANF64` with for a
            // compiler claiming GCC 7 or later: `_Float32` is `float`, the
            // other three are `double` (see `parse::floatn_type`). The
            // `_Float128` ones stay unknown, since no value of it can exist.
            "huge_valf32" | "inff32" => {
                self.builtin_arity(name, args, 0, range)?;
                Some(Expr::new(ExprKind::Float(f64::INFINITY), Ty::Float, range))
            }
            "huge_valf64" | "huge_valf32x" | "huge_valf64x" | "inff64" | "inff32x" | "inff64x" => {
                self.builtin_arity(name, args, 0, range)?;
                Some(Expr::new(ExprKind::Float(f64::INFINITY), Ty::Double, range))
            }
            "nanf32" | "nanf64" | "nanf32x" | "nanf64x" | "nansf32" | "nansf64" | "nansf32x"
            | "nansf64x" => {
                let signalling = rest.starts_with("nans");
                let base = match (signalling, rest.ends_with("f32")) {
                    (false, true) => "nanf",
                    (false, false) => "nan",
                    (true, true) => "nansf",
                    (true, false) => "nans",
                };
                self.nan_builtin(name, base, args, range)
            }
            "fabs" | "fabsl" | "fabsf" => {
                let ty = if rest == "fabsf" {
                    Ty::Float
                } else {
                    Ty::Double
                };
                self.builtin_arity(name, args, 1, range)?;
                let value = self.float_conversion(name, &args[0], ty)?;
                Some(Expr::new(
                    ExprKind::Builtin {
                        op: BuiltinOp::Fabs,
                        args: vec![value],
                    },
                    ty,
                    range,
                ))
            }
            "copysign" | "copysignl" | "copysignf" => {
                let ty = if rest == "copysignf" {
                    Ty::Float
                } else {
                    Ty::Double
                };
                self.builtin_arity(name, args, 2, range)?;
                let magnitude = self.float_conversion(name, &args[0], ty)?;
                let sign = self.float_conversion(name, &args[1], ty)?;
                Some(Expr::new(
                    ExprKind::Builtin {
                        op: BuiltinOp::Copysign,
                        args: vec![magnitude, sign],
                    },
                    ty,
                    range,
                ))
            }
            // C11's `CMPLX` is built on this one, which is also how a program
            // writes a complex value whose imaginary part is an infinity or a
            // NaN — `x + y * I` cannot, because `y * I` multiplies.
            "complex" => self.builtin_complex(name, args, range),
            "creal" | "crealf" | "creall" => self.complex_part_builtin(name, false, args, range),
            "cimag" | "cimagf" | "cimagl" => self.complex_part_builtin(name, true, args, range),
            "conj" | "conjf" | "conjl" => self.complex_unary(name, false, args, range),
            "cproj" | "cprojf" | "cprojl" => self.complex_unary(name, true, args, range),
            "isgreater" => self.float_order(name, FloatOrder::Greater, args, range),
            "isgreaterequal" => self.float_order(name, FloatOrder::GreaterEqual, args, range),
            "isless" => self.float_order(name, FloatOrder::Less, args, range),
            "islessequal" => self.float_order(name, FloatOrder::LessEqual, args, range),
            "islessgreater" => self.float_order(name, FloatOrder::LessGreater, args, range),
            "isunordered" => self.float_order(name, FloatOrder::Unordered, args, range),
            "isnan" | "isnanl" | "isnanf" => self.float_class(name, FloatClass::IsNan, args, range),
            "isinf" | "isinfl" | "isinff" => self.float_class(name, FloatClass::IsInf, args, range),
            "isinf_sign" => self.float_class(name, FloatClass::IsInfSign, args, range),
            "isfinite" => self.float_class(name, FloatClass::IsFinite, args, range),
            "isnormal" => self.float_class(name, FloatClass::IsNormal, args, range),
            "issignaling" => self.float_class(name, FloatClass::IsSignaling, args, range),
            "signbit" | "signbitl" | "signbitf" => {
                self.float_class(name, FloatClass::SignBit, args, range)
            }
            "fpclassify" => self.fpclassify(name, args, range),
            "classify_type" => self.classify_type(name, args, range),
            // Hints with nowhere to go. The operands are still evaluated,
            // because C says they are.
            "prefetch" => self.prefetch(name, args, range),
            "assume" | "speculation_safe_value" => {
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.expr(arg)?);
                }
                Some(Expr::new(
                    ExprKind::Builtin {
                        op: BuiltinOp::Discard,
                        args: values,
                    },
                    Ty::Void,
                    range,
                ))
            }
            // The pointer comes back unchanged; the promise it carries is one
            // the generated Rust has no way to pass on.
            "assume_aligned" => {
                if args.is_empty() {
                    self.error(
                        range,
                        format!("'{name}' expects at least 1 argument, have {}", args.len()),
                    );
                    return Some(None);
                }
                let pointer = self.expr(&args[0])?;
                for arg in &args[1..] {
                    self.expr(arg);
                }
                if !pointer.ty.is_pointer() {
                    self.error(
                        args[0].range,
                        format!(
                            "the first argument of '{name}' must be a pointer, not '{}'",
                            self.tyname(pointer.ty)
                        ),
                    );
                    return Some(None);
                }
                let void = self.ptr_to(Ty::Void, false);
                Some(self.convert(pointer, void))
            }
            "object_size" | "dynamic_object_size" => {
                self.builtin_arity(name, args, 2, range)?;
                self.expr(&args[0])?;
                let mode = self.expr(&args[1])?;
                let mode = match self.const_eval(&mode) {
                    Some(ir::ConstValue::Int(v)) => v,
                    _ => {
                        self.error(
                            args[1].range,
                            format!(
                                "the second argument of '{name}' must be a constant 0, 1, 2 or 3"
                            ),
                        );
                        return Some(None);
                    }
                };
                let size_ty = self.size_ty();
                let answer = size_ty.wrap(object_size_answer(mode), &self.target);
                Some(Expr::int(answer, size_ty, range))
            }
            "alloca" | "alloca_with_align" => self.alloca(name, args, range),
            // `__builtin_X` for a library function X is a call to X — and
            // `__builtin_Xl`, the `long double` form, is a call to the same
            // one, `long double` being `double` here.
            _ if gnu::LIBRARY_BUILTINS.contains(&rest) => self.library_call(rest, args, range),
            _ => {
                let base = gnu::long_double_math(rest)?;
                self.library_call(base, args, range)
            }
        };
        Some(result)
    }

    /// `__builtin_complex(re, im)` — C11's `CMPLX`, and the only way to write
    /// a complex constant whose imaginary part is an infinity or a NaN.
    ///
    /// `x + y * I` cannot do it: `y * I` is a multiplication, and `inf * 0` is
    /// a NaN. GCC and Clang both require the two operands to have the same
    /// *real* floating type, and so does this.
    fn builtin_complex(
        &mut self,
        name: &str,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        self.builtin_arity(name, args, 2, range)?;
        let re = self.expr(&args[0])?;
        let im = self.expr(&args[1])?;
        if re.ty.is_error() || im.ty.is_error() {
            return None;
        }
        for (value, arg) in [(&re, &args[0]), (&im, &args[1])] {
            if !value.ty.is_floating() {
                self.error(
                    arg.range,
                    format!(
                        "an operand of '{name}' must have a real floating type, not '{}'",
                        self.tyname(value.ty)
                    ),
                );
                return None;
            }
        }
        if re.ty != im.ty {
            self.error(
                range,
                format!(
                    "the operands of '{name}' have different types, '{}' and '{}'",
                    self.tyname(re.ty),
                    self.tyname(im.ty)
                ),
            );
            return None;
        }
        if !self.complex {
            self.error(range, crate::COMPLEX_UNSUPPORTED.to_owned());
            return None;
        }
        let ty = re.ty.complex_of();
        Some(Expr::new(
            ExprKind::ComplexOf {
                re: Box::new(re),
                im: Box::new(im),
            },
            ty,
            range,
        ))
    }

    /// The complex type a `__builtin_c…` name asks for: the `f` forms are
    /// `float _Complex` and everything else — the `l` forms included, `long
    /// double` being `double` here — is `double _Complex`.
    fn complex_builtin_ty(rest: &str) -> Ty {
        if rest.ends_with('f') {
            Ty::ComplexFloat
        } else {
            Ty::ComplexDouble
        }
    }

    /// `__builtin_creal(z)` and `__builtin_cimag(z)`, with their `f` and `l`
    /// forms.
    fn complex_part_builtin(
        &mut self,
        name: &str,
        imag: bool,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        self.builtin_arity(name, args, 1, range)?;
        let ty = Self::complex_builtin_ty(name.trim_start_matches("__builtin_"));
        let value = self.complex_argument(name, &args[0], ty)?;
        Some(self.complex_part_of(value, imag, range))
    }

    /// `__builtin_conj(z)` and `__builtin_cproj(z)`, with their `f` and `l`
    /// forms.
    ///
    /// Conjugation is what GNU's `~z` already means, so it reuses that node;
    /// projection needs one of its own.
    fn complex_unary(
        &mut self,
        name: &str,
        proj: bool,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        self.builtin_arity(name, args, 1, range)?;
        let ty = Self::complex_builtin_ty(name.trim_start_matches("__builtin_"));
        let value = self.complex_argument(name, &args[0], ty)?;
        let kind = if proj {
            ExprKind::Builtin {
                op: BuiltinOp::ComplexProj,
                args: vec![value],
            }
        } else {
            ExprKind::BitNot(Box::new(value))
        };
        Some(Expr::new(kind, ty, range))
    }

    /// The single argument of a complex builtin, converted to the type the
    /// name asks for.
    fn complex_argument(&mut self, name: &str, arg: &ast::Expr, ty: Ty) -> Option<Expr> {
        let value = self.expr(arg)?;
        if value.ty.is_error() {
            return None;
        }
        if !value.ty.is_arithmetic() {
            self.error(
                arg.range,
                format!(
                    "the argument of '{name}' must have an arithmetic type, not '{}'",
                    self.tyname(value.ty)
                ),
            );
            return None;
        }
        if !self.complex {
            self.error(arg.range, crate::COMPLEX_UNSUPPORTED.to_owned());
            return None;
        }
        Some(self.convert(value, ty))
    }

    /// `__builtin_alloca(size)` and `__builtin_alloca_with_align(size, bits)`.
    ///
    /// The memory is taken from a per-function arena — see
    /// [`ir::Function::uses_arena`] — which is dropped by the `return`. That
    /// is exactly `alloca`'s lifetime: its memory belongs to the *function*,
    /// not to the block the call was written in, and a pointer to it returned
    /// to the caller dangles in C too.
    fn alloca(&mut self, name: &str, args: &[ast::Expr], range: SourceRange) -> Option<Expr> {
        let with_align = name.ends_with("_with_align");
        self.builtin_arity(name, args, 1 + usize::from(with_align), range)?;
        // The arena belongs to a function, and an initialiser at file scope is
        // not inside one. (The name of the function last *checked* is no use
        // here: it outlives the body it came from.)
        if self.at_file_scope() {
            self.error(
                range,
                format!(
                    "'{name}' is only allowed inside a function; the memory it returns lives \
                     until that function returns"
                ),
            );
            return None;
        }
        let size = self.expr(&args[0])?;
        if !size.ty.is_integer() {
            self.error(
                args[0].range,
                format!(
                    "'{name}' requires an integer size, not '{}'",
                    self.tyname(size.ty)
                ),
            );
            return None;
        }
        if with_align {
            // GCC's alignment is a constant *in bits*. The arena hands out
            // 16-byte blocks, so anything up to 128 bits is already satisfied
            // and anything above it would be a promise this cannot keep.
            let alignment = self.expr(&args[1])?;
            let bits = match self.const_eval(&alignment) {
                Some(ir::ConstValue::Int(bits)) => bits,
                _ => {
                    self.error(
                        args[1].range,
                        format!("the alignment of '{name}' must be an integer constant"),
                    );
                    return None;
                }
            };
            if bits <= 0 || bits > 128 {
                self.error(
                    args[1].range,
                    format!(
                        "the alignment of '{name}' must be between 1 and 128 bits; the \
                         emulated arena is 16-byte aligned"
                    ),
                );
                return None;
            }
        }
        let size_ty = self.size_ty();
        let size = self.convert(size, size_ty);
        self.func_uses_arena = true;
        let void_ptr = self.ptr_to(Ty::Void, false);
        Some(Expr::new(
            ExprKind::Builtin {
                op: BuiltinOp::Alloca,
                args: vec![size],
            },
            void_ptr,
            range,
        ))
    }

    /// `__builtin_nan(s)`, `__builtin_nans(s)` and their `f` and `l` forms.
    ///
    /// The string names the NaN's *payload*, in the base `strtoull` would read
    /// it in, and an empty one asks for the default: no payload at all for a
    /// quiet NaN, and the leading payload bit for a signalling one, which is
    /// what GCC produces. A payload is part of the value, so the constant is
    /// carried — and written out — bit for bit; see [`ir::narrow_nan_bits`].
    fn nan_builtin(
        &mut self,
        name: &str,
        rest: &str,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        self.builtin_arity(name, args, 1, range)?;
        let signalling = rest.starts_with("nans");
        let ty = if rest.ends_with('f') {
            Ty::Float
        } else {
            Ty::Double
        };
        let Some(text) = self.string_argument(name, "the payload", &args[0]) else {
            // The operand still has to be checked, and an error is already out.
            self.expr(&args[0]);
            return None;
        };
        // The mantissa is 23 bits wide in a `float` and 52 in a `double`; the
        // top one of them is the quiet bit, and a signalling NaN with no
        // payload at all would be an infinity, so GCC gives it the next bit
        // down.
        let quiet = if ty == Ty::Float { 1 << 22 } else { 1 << 51 };
        let mut payload = parse_nan_payload(&text) & (quiet - 1);
        if signalling && payload == 0 {
            payload = quiet >> 1;
        }
        let mantissa = if signalling { payload } else { quiet | payload };
        let bits = if ty == Ty::Float {
            ir::widen_nan_bits(0x7f80_0000 | mantissa as u32)
        } else {
            0x7ff0_0000_0000_0000 | mantissa
        };
        Some(Expr::new(ExprKind::Float(f64::from_bits(bits)), ty, range))
    }

    /// The text of a string-literal argument, which is all `__builtin_nan` and
    /// `__builtin_cpu_supports` accept.
    fn string_argument(&mut self, name: &str, what: &str, arg: &ast::Expr) -> Option<String> {
        if let ast::ExprKind::Str(literal) = &arg.kind
            && let Some(bytes) = literal.as_bytes()
        {
            return Some(bytes.iter().map(|b| char::from(*b)).collect());
        }
        self.error(
            arg.range,
            format!("the argument of '{name}' must be a string literal naming {what}"),
        );
        None
    }

    /// `__builtin_cpu_supports("avx2")`, which becomes
    /// `::std::is_x86_feature_detected!("avx2")`.
    ///
    /// This is the run-time half of the SIMD story, and the half that matters:
    /// a procedural macro cannot see rustc's `-C target-feature`, so cinrs
    /// predefines only the x86-64 baseline — `__SSE__` and `__SSE2__` — and a
    /// program that wants AVX2 asks the processor rather than the compiler.
    /// The answer is an `int`, exactly as GCC's builtin returns.
    fn cpu_supports(&mut self, args: &[ast::Expr], range: SourceRange) -> Option<Expr> {
        self.builtin_arity("__builtin_cpu_supports", args, 1, range)?;
        if !matches!(self.target.arch, Arch::X86 | Arch::X86_64) {
            self.error(
                range,
                format!(
                    "'__builtin_cpu_supports' is x86 only, and this unit is being translated for \
                     {}",
                    self.target.arch.as_str()
                ),
            );
            return None;
        }
        let asked = self.string_argument(
            "__builtin_cpu_supports",
            "an instruction set, as in __builtin_cpu_supports(\"avx2\")",
            &args[0],
        )?;
        if let Some(why) = crate::x86::unsupported_feature(&asked) {
            self.error(
                args[0].range,
                format!("'__builtin_cpu_supports(\"{asked}\")': {why}"),
            );
            return None;
        }
        let Some(row) = crate::x86::feature_row(&asked) else {
            self.error(
                args[0].range,
                format!(
                    "unknown instruction set '{asked}'; the ones cinrs can ask the processor \
                     about are {}. A processor *name* — what GCC's __builtin_cpu_is takes — has \
                     no equivalent: ask about the instruction you mean to use",
                    super::list_of_names(&crate::x86::feature_names())
                ),
            );
            return None;
        };
        // `is_x86_feature_detected!` is a `std` macro: `core` has no CPU
        // detection at all, and there is nothing to fall back on. The unit's
        // `no_std` is only known once the pragmas have been read, so the site
        // is recorded and [`super::check_pragmas`] reports it.
        self.program.cpu_supports.push(range);
        Some(Expr::new(
            ExprKind::Builtin {
                // The row fits in a byte; `crate::x86` asserts that the table
                // never outgrows one.
                op: BuiltinOp::CpuSupports(row as u8),
                args: Vec::new(),
            },
            Ty::Int,
            range,
        ))
    }

    /// An operand of a builtin with a fixed floating prototype, converted to
    /// the type that prototype gives it.
    fn float_conversion(&mut self, name: &str, arg: &ast::Expr, ty: Ty) -> Option<Expr> {
        let value = self.expr(arg)?;
        if !value.ty.is_arithmetic() {
            self.error(
                arg.range,
                format!(
                    "the arguments of '{name}' must have arithmetic types, not '{}'",
                    self.tyname(value.ty)
                ),
            );
            return None;
        }
        Some(self.convert(value, ty))
    }

    /// `__builtin_isgreater` and the other five quiet comparisons.
    ///
    /// The operands go through the usual arithmetic conversions, and the
    /// result must be a real floating type: these say nothing about integers,
    /// which have no unordered pair.
    fn float_order(
        &mut self,
        name: &str,
        order: FloatOrder,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        self.builtin_arity(name, args, 2, range)?;
        let lhs = self.expr(&args[0])?;
        let rhs = self.expr(&args[1])?;
        if !lhs.ty.is_arithmetic() || !rhs.ty.is_arithmetic() {
            self.error(
                range,
                format!("the arguments of '{name}' must have real floating types"),
            );
            return None;
        }
        let (lhs, rhs, common) = self.balance(lhs, rhs);
        if !common.is_floating() {
            self.error(
                range,
                format!("non-floating-point arguments in call to '{name}'"),
            );
            return None;
        }
        Some(Expr::new(
            ExprKind::Builtin {
                op: BuiltinOp::FloatOrder(order),
                args: vec![lhs, rhs],
            },
            Ty::Int,
            range,
        ))
    }

    /// `__builtin_isnan` and the other classifications, which GCC overloads on
    /// the argument's own type rather than giving them a prototype.
    fn float_class(
        &mut self,
        name: &str,
        class: FloatClass,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        self.builtin_arity(name, args, 1, range)?;
        let value = self.float_operand(name, &args[0])?;
        Some(Expr::new(
            ExprKind::Builtin {
                op: BuiltinOp::FloatClass(class),
                args: vec![value],
            },
            Ty::Int,
            range,
        ))
    }

    /// The operand of a type-generic floating builtin: a `float` stays one and
    /// a `double` or `long double` is a `double`, which is what the two
    /// widths of the generated code can inspect.
    fn float_operand(&mut self, name: &str, arg: &ast::Expr) -> Option<Expr> {
        let value = self.expr(arg)?;
        if !value.ty.is_floating() {
            self.error(
                arg.range,
                format!("non-floating-point argument in call to '{name}'"),
            );
            return None;
        }
        Some(value)
    }

    /// `__builtin_fpclassify(nan, inf, normal, subnormal, zero, x)`, which is
    /// how `<math.h>`'s `fpclassify` names its own five answers.
    fn fpclassify(&mut self, name: &str, args: &[ast::Expr], range: SourceRange) -> Option<Expr> {
        self.builtin_arity(name, args, 6, range)?;
        let mut values = Vec::with_capacity(6);
        for arg in &args[..5] {
            let value = self.expr(arg)?;
            if !value.ty.is_integer() {
                self.error(
                    arg.range,
                    format!("the first five arguments of '{name}' must have integer types"),
                );
                return None;
            }
            values.push(self.convert(value, Ty::Int));
        }
        values.push(self.float_operand(name, &args[5])?);
        Some(Expr::new(
            ExprKind::Builtin {
                op: BuiltinOp::Fpclassify,
                args: values,
            },
            Ty::Int,
            range,
        ))
    }

    /// `__builtin_classify_type(e)`: GCC's number for the class of `e`'s type.
    ///
    /// The operand is not evaluated — the whole thing is an integer constant
    /// expression — and the type it is classified by is the one the default
    /// argument promotions give it, which is why `char`, `_Bool` and an
    /// enumeration all answer `1` and an array answers `5`.
    fn classify_type(
        &mut self,
        name: &str,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        self.builtin_arity(name, args, 1, range)?;
        let value = self.expr(&args[0])?;
        let ty = self.promoted_argument(&value);
        let class = match ty {
            Ty::Void => 0,
            Ty::Float | Ty::Double => 8,
            // GCC's `complex_type_class`.
            Ty::ComplexFloat | Ty::ComplexDouble => 9,
            Ty::Pointer(_) => 5,
            Ty::Func(_) => 10,
            Ty::Array(_) => 14,
            Ty::Record(id) => {
                if self.types().record(id).kind == ir::RecordKind::Union {
                    13
                } else {
                    12
                }
            }
            _ => 1,
        };
        Some(Expr::int(class, Ty::Int, range))
    }

    /// `__builtin_prefetch(p)`, `(p, rw)` or `(p, rw, locality)`.
    ///
    /// GCC requires `rw` and `locality` to be integer constants — it refuses
    /// anything else, since they select the instruction — and they default to
    /// 0 (a read) and 3 (keep in every level of cache). A value outside 0..=1
    /// or 0..=3 is an error here, as it is in Clang; GCC warns and uses zero.
    /// The pointer is the only operand that reaches the generated code, as a
    /// `const void *`.
    fn prefetch(&mut self, name: &str, args: &[ast::Expr], range: SourceRange) -> Option<Expr> {
        if args.is_empty() || args.len() > 3 {
            self.error(
                range,
                format!("'{name}' expects 1 to 3 arguments, have {}", args.len()),
            );
            return None;
        }
        let pointer = self.expr(&args[0])?;
        if !pointer.ty.is_pointer() {
            self.error(
                args[0].range,
                format!(
                    "the first argument of '{name}' must be a pointer, not '{}'",
                    self.tyname(pointer.ty)
                ),
            );
            return None;
        }
        let mut hint = [0i128, 3];
        let limits = [("second", 1), ("third", 3)];
        for (index, (arg, (which, max))) in args[1..].iter().zip(limits).enumerate() {
            let value = self.expr(arg)?;
            match self.const_eval(&value) {
                Some(ir::ConstValue::Int(v)) if (0..=max).contains(&v) => hint[index] = v,
                Some(ir::ConstValue::Int(v)) => {
                    self.error(
                        arg.range,
                        format!("the {which} argument of '{name}' must be 0 to {max}, not {v}"),
                    );
                    return None;
                }
                _ => {
                    self.error(
                        arg.range,
                        format!("the {which} argument of '{name}' must be an integer constant"),
                    );
                    return None;
                }
            }
        }
        let void = self.ptr_to(Ty::Void, true);
        let pointer = self.convert(pointer, void);
        // Both are in range, so the packed byte is at most 7.
        let packed = (hint[1] | hint[0] << 2) as u8;
        Some(Expr::new(
            ExprKind::Builtin {
                op: BuiltinOp::Prefetch(packed),
                args: vec![pointer],
            },
            Ty::Void,
            range,
        ))
    }

    /// Checks a builtin's argument count.
    fn builtin_arity(
        &mut self,
        name: &str,
        args: &[ast::Expr],
        wanted: usize,
        range: SourceRange,
    ) -> Option<()> {
        if args.len() == wanted {
            return Some(());
        }
        self.error(
            range,
            format!(
                "'{name}' expects {wanted} argument{}, have {}",
                if wanted == 1 { "" } else { "s" },
                args.len()
            ),
        );
        None
    }

    /// `__builtin_popcount` and its relatives, whose operand type the suffix
    /// gives and whose value is an `int`.
    fn bit_builtin(
        &mut self,
        name: &str,
        op: BuiltinOp,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        self.builtin_arity(name, args, 1, range)?;
        let value = self.expr(&args[0])?;
        if !value.ty.is_integer() {
            self.error(
                args[0].range,
                format!(
                    "'{name}' requires an integer argument, not '{}'",
                    self.tyname(value.ty)
                ),
            );
            return None;
        }
        // The suffix names the width the builtin works at: none is `int`, `l`
        // is `long` and `ll` is `long long`. `clrsb` counts sign bits and is
        // therefore signed; everything else is defined on unsigned values.
        let signed = name.contains("clrsb");
        let ty = match name {
            _ if name.ends_with("ll") => {
                if signed {
                    Ty::LongLong
                } else {
                    Ty::ULongLong
                }
            }
            _ if name.ends_with('l') => {
                if signed {
                    Ty::Long
                } else {
                    Ty::ULong
                }
            }
            _ => {
                if signed {
                    Ty::Int
                } else {
                    Ty::UInt
                }
            }
        };
        let value = self.convert(value, ty);
        Some(Expr::new(
            ExprKind::Builtin {
                op,
                args: vec![value],
            },
            Ty::Int,
            range,
        ))
    }

    /// `__builtin_bswap16/32/64`, whose value has the operand's own type.
    fn bswap(&mut self, name: &str, args: &[ast::Expr], range: SourceRange) -> Option<Expr> {
        self.builtin_arity(name, args, 1, range)?;
        let value = self.expr(&args[0])?;
        if !value.ty.is_integer() {
            self.error(
                args[0].range,
                format!(
                    "'{name}' requires an integer argument, not '{}'",
                    self.tyname(value.ty)
                ),
            );
            return None;
        }
        let ty = match name {
            "__builtin_bswap16" => Ty::UShort,
            "__builtin_bswap32" => Ty::UInt,
            _ => Ty::ULongLong,
        };
        let value = self.convert(value, ty);
        Some(Expr::new(
            ExprKind::Builtin {
                op: BuiltinOp::Bswap,
                args: vec![value],
            },
            ty,
            range,
        ))
    }

    /// `__builtin_add_overflow(a, b, &r)` and its relatives.
    ///
    /// The arithmetic happens in infinite precision and the result is then
    /// converted to the type `r` has; the value of the builtin says whether
    /// that conversion lost anything. `store` is false for the `_p` forms,
    /// whose third operand only names a type.
    fn overflow(
        &mut self,
        name: &str,
        op: BinOp,
        args: &[ast::Expr],
        store: bool,
        range: SourceRange,
    ) -> Option<Expr> {
        self.builtin_arity(name, args, 3, range)?;
        let lhs = self.expr(&args[0])?;
        let rhs = self.expr(&args[1])?;
        let third = self.expr(&args[2])?;
        for (index, value) in [&lhs, &rhs].into_iter().enumerate() {
            if !value.ty.is_integer() {
                self.error(
                    args[index].range,
                    format!(
                        "the operands of '{name}' must have integer types, and the \
                         {} has type '{}'",
                        if index == 0 { "first" } else { "second" },
                        self.tyname(value.ty)
                    ),
                );
                return None;
            }
        }
        let result_ty = if store {
            let Some(pointee) = self.pointee(third.ty).filter(|ty| ty.is_integer()) else {
                self.error(
                    args[2].range,
                    format!(
                        "the third argument of '{name}' must be a pointer to an integer, \
                         not '{}'",
                        self.tyname(third.ty)
                    ),
                );
                return None;
            };
            pointee
        } else {
            if !third.ty.is_integer() {
                self.error(
                    args[2].range,
                    format!(
                        "the third argument of '{name}' must have an integer type, not '{}'",
                        self.tyname(third.ty)
                    ),
                );
                return None;
            }
            third.ty
        };
        // The arithmetic is done in an `i128`, which is exactly what "infinite
        // precision" amounts to while both *operands* are at most 64 bits
        // wide: their sum, difference and product all fit. A 128-bit operand
        // would need 129 bits and then some, and quietly giving a wrong answer
        // about overflow is worse than not having the builtin at all. The
        // 128-bit *result* type is fine — see `Codegen::overflow_builtin`,
        // where it only changes how the answer is checked.
        if let Some(wide) = [lhs.ty, rhs.ty].into_iter().find(|t| t.is_int128()) {
            self.error(
                range,
                format!(
                    "'{name}' with a 128-bit operand is not supported: the check is computed \
                     one width up from the operands, and there is nothing above '{}'",
                    self.tyname(wide)
                ),
            );
            return None;
        }
        // Resolving it was the point — it is where the third operand is
        // checked — and code generation reads it off that operand again.
        let _ = result_ty;
        let op = if store {
            BuiltinOp::Overflow(op)
        } else {
            BuiltinOp::OverflowP(op)
        };
        // The third operand travels either way: code generation reads the
        // result type off it, and the `_p` forms still evaluate it.
        Some(Expr::new(
            ExprKind::Builtin {
                op,
                args: vec![lhs, rhs, third],
            },
            // GCC's overflow builtins have type `_Bool`.
            Ty::Bool,
            range,
        ))
    }

    /// A call to the library function a `__builtin_` name stands for.
    ///
    /// The function is declared into the unit if the header that would have
    /// declared it was not included, so `__builtin_strlen(s)` works without
    /// `<string.h>` exactly as it does in GCC.
    fn library_call(&mut self, name: &str, args: &[ast::Expr], range: SourceRange) -> Option<Expr> {
        self.declare_library_function(name, range)?;
        let callee = ast::Expr {
            kind: ast::ExprKind::Ident(ast::Ident {
                name: name.to_owned(),
                range,
            }),
            range,
        };
        self.call(&callee, args, range)
    }

    /// Finds, or declares, the library function `name`.
    fn declare_library_function(&mut self, name: &str, range: SourceRange) -> Option<FuncId> {
        if let Some(Entry::Function(id)) = self.lookup(name) {
            return Some(*id);
        }
        if self.lookup(name).is_some() {
            self.error(
                range,
                format!("'{name}' is declared as something other than a function here"),
            );
            return None;
        }
        let Some(sig) = self.library_signature(name) else {
            self.error(
                range,
                format!(
                    "'__builtin_{name}' needs '{name}' to be declared; include the header \
                     that declares it"
                ),
            );
            return None;
        };
        let id = FuncId(self.program.functions.len() as u32);
        let params = sig.params.len();
        self.program.functions.push(Function {
            name: name.to_owned(),
            sig,
            params: Vec::new(),
            param_names: vec![None; params],
            is_static: false,
            is_inline: false,
            noreturn: name == "abort" || name == "exit",
            inline_hint: None,
            cold: false,
            deprecated: None,
            section: None,
            asm_label: None,
            init_kind: None,
            target_features: Vec::new(),
            address_taken: false,
            // A `__builtin_memcpy` is `memcpy`, which is a symbol; nothing
            // here is ever an intrinsic mapped onto `core::arch`.
            intrinsic: None,
            safe: None,
            locals: Vec::new(),
            uses_arena: false,
            body: None,
            item_name: None,
            env: Vec::new(),
            range,
        });
        self.item_names.insert(name.to_owned());
        self.insert_at_file_scope(name, Entry::Function(id));
        Some(id)
    }

    /// The prototype of a library function the unit did not declare.
    ///
    /// GCC knows the prototype of every function it has a builtin for, and
    /// declares it on the spot rather than making the program include the
    /// header first; this is that table, and every entry is the declaration
    /// the bundled header writes, so a `#include` that arrives later
    /// redeclares it compatibly. What is *not* here is the handful whose
    /// prototype mentions a type only a header can introduce — `FILE` and
    /// `va_list` — where the diagnostic above is the right answer.
    fn library_signature(&mut self, name: &str) -> Option<Signature> {
        let size_t = self.size_ty();
        let intmax = self.intmax_ty();
        let void_ptr = self.ptr_to(Ty::Void, false);
        let const_void_ptr = self.ptr_to(Ty::Void, true);
        let char_ptr = self.ptr_to(Ty::Char, false);
        let const_char_ptr = self.ptr_to(Ty::Char, true);
        let char_ptr_ptr = self.ptr_to(char_ptr, false);
        let int_ptr = self.ptr_to(Ty::Int, false);
        let double_ptr = self.ptr_to(Ty::Double, false);
        let float_ptr = self.ptr_to(Ty::Float, false);
        let sig = |ret: Ty, params: Vec<Ty>| Signature {
            ret,
            params,
            variadic: false,
            prototyped: true,
        };
        let variadic = |ret: Ty, params: Vec<Ty>| Signature {
            ret,
            params,
            variadic: true,
            prototyped: true,
        };
        Some(match name {
            // <string.h> and the GNU functions that live beside them.
            "memcpy" | "memmove" | "mempcpy" => {
                sig(void_ptr, vec![void_ptr, const_void_ptr, size_t])
            }
            "memset" => sig(void_ptr, vec![void_ptr, Ty::Int, size_t]),
            "memcmp" | "bcmp" => sig(Ty::Int, vec![const_void_ptr, const_void_ptr, size_t]),
            "memchr" => sig(void_ptr, vec![const_void_ptr, Ty::Int, size_t]),
            "bzero" => sig(Ty::Void, vec![void_ptr, size_t]),
            "bcopy" => sig(Ty::Void, vec![const_void_ptr, void_ptr, size_t]),
            "strlen" => sig(size_t, vec![const_char_ptr]),
            "strcpy" | "strcat" | "stpcpy" => sig(char_ptr, vec![char_ptr, const_char_ptr]),
            "strncpy" | "strncat" | "stpncpy" => {
                sig(char_ptr, vec![char_ptr, const_char_ptr, size_t])
            }
            "strcmp" | "strcoll" | "strcasecmp" => {
                sig(Ty::Int, vec![const_char_ptr, const_char_ptr])
            }
            "strncmp" | "strncasecmp" => sig(Ty::Int, vec![const_char_ptr, const_char_ptr, size_t]),
            "strchr" | "strrchr" | "index" | "rindex" => {
                sig(char_ptr, vec![const_char_ptr, Ty::Int])
            }
            "strstr" | "strpbrk" => sig(char_ptr, vec![const_char_ptr, const_char_ptr]),
            "strspn" | "strcspn" => sig(size_t, vec![const_char_ptr, const_char_ptr]),
            "strdup" => sig(char_ptr, vec![const_char_ptr]),
            // <stdlib.h>
            "abs" => sig(Ty::Int, vec![Ty::Int]),
            "labs" => sig(Ty::Long, vec![Ty::Long]),
            "llabs" => sig(Ty::LongLong, vec![Ty::LongLong]),
            "imaxabs" => sig(intmax, vec![intmax]),
            "abort" => sig(Ty::Void, vec![]),
            "exit" | "_Exit" => sig(Ty::Void, vec![Ty::Int]),
            "malloc" => sig(void_ptr, vec![size_t]),
            "calloc" => sig(void_ptr, vec![size_t, size_t]),
            "realloc" => sig(void_ptr, vec![void_ptr, size_t]),
            "free" => sig(Ty::Void, vec![void_ptr]),
            "atoi" => sig(Ty::Int, vec![const_char_ptr]),
            "atol" => sig(Ty::Long, vec![const_char_ptr]),
            "atoll" => sig(Ty::LongLong, vec![const_char_ptr]),
            "atof" => sig(Ty::Double, vec![const_char_ptr]),
            "strtol" => sig(Ty::Long, vec![const_char_ptr, char_ptr_ptr, Ty::Int]),
            "strtoul" => sig(Ty::ULong, vec![const_char_ptr, char_ptr_ptr, Ty::Int]),
            "strtoll" => sig(Ty::LongLong, vec![const_char_ptr, char_ptr_ptr, Ty::Int]),
            "strtoull" => sig(Ty::ULongLong, vec![const_char_ptr, char_ptr_ptr, Ty::Int]),
            "strtod" => sig(Ty::Double, vec![const_char_ptr, char_ptr_ptr]),
            "strtof" => sig(Ty::Float, vec![const_char_ptr, char_ptr_ptr]),
            // <stdio.h>, as far as it can be written without `FILE`.
            "printf" => variadic(Ty::Int, vec![const_char_ptr]),
            "sprintf" => variadic(Ty::Int, vec![char_ptr, const_char_ptr]),
            "snprintf" => variadic(Ty::Int, vec![char_ptr, size_t, const_char_ptr]),
            "putchar" => sig(Ty::Int, vec![Ty::Int]),
            "puts" => sig(Ty::Int, vec![const_char_ptr]),
            // <ctype.h>
            "tolower" | "toupper" | "isalnum" | "isalpha" | "isblank" | "iscntrl" | "isdigit"
            | "isgraph" | "islower" | "isprint" | "ispunct" | "isspace" | "isupper"
            | "isxdigit" => sig(Ty::Int, vec![Ty::Int]),
            // <math.h>. `long double` is `double` here, so `sqrtl` and its
            // relatives are the unsuffixed function; see `gnu::LONG_DOUBLE_MATH`.
            "acos" | "asin" | "atan" | "cbrt" | "ceil" | "cos" | "cosh" | "erf" | "erfc"
            | "exp" | "exp2" | "expm1" | "fabs" | "floor" | "lgamma" | "log" | "log10"
            | "log1p" | "log2" | "nearbyint" | "rint" | "round" | "sin" | "sinh" | "sqrt"
            | "tan" | "tanh" | "tgamma" | "trunc" => sig(Ty::Double, vec![Ty::Double]),
            "acosf" | "asinf" | "atanf" | "cbrtf" | "ceilf" | "cosf" | "coshf" | "expf"
            | "exp2f" | "expm1f" | "fabsf" | "floorf" | "logf" | "log10f" | "log1pf" | "log2f"
            | "nearbyintf" | "rintf" | "roundf" | "sinf" | "sinhf" | "sqrtf" | "tanf" | "tanhf"
            | "truncf" => sig(Ty::Float, vec![Ty::Float]),
            "atan2" | "copysign" | "fdim" | "fmax" | "fmin" | "fmod" | "hypot" | "nextafter"
            | "pow" | "remainder" => sig(Ty::Double, vec![Ty::Double, Ty::Double]),
            "atan2f" | "copysignf" | "fdimf" | "fmaxf" | "fminf" | "fmodf" | "hypotf"
            | "nextafterf" | "powf" | "remainderf" => sig(Ty::Float, vec![Ty::Float, Ty::Float]),
            "fma" => sig(Ty::Double, vec![Ty::Double, Ty::Double, Ty::Double]),
            "fmaf" => sig(Ty::Float, vec![Ty::Float, Ty::Float, Ty::Float]),
            "ldexp" | "scalbn" => sig(Ty::Double, vec![Ty::Double, Ty::Int]),
            "ldexpf" | "scalbnf" => sig(Ty::Float, vec![Ty::Float, Ty::Int]),
            "frexp" => sig(Ty::Double, vec![Ty::Double, int_ptr]),
            "frexpf" => sig(Ty::Float, vec![Ty::Float, int_ptr]),
            "modf" => sig(Ty::Double, vec![Ty::Double, double_ptr]),
            "modff" => sig(Ty::Float, vec![Ty::Float, float_ptr]),
            _ => return None,
        })
    }

    /// The prototype a C89 implicit declaration of `name`, or a declaration
    /// of it with no prototype, takes when `name` is a library function GCC
    /// has a built-in for.
    ///
    /// GCC declares `strcpy` as `char *(char *, const char *)` whether or not
    /// `<string.h>` was included, and a call to an undeclared `strcpy` or an
    /// `int strcmp();` of K&R vintage is a declaration *of that built-in*:
    /// the arguments are converted to its parameter types, the result has its
    /// return type ("incompatible implicit declaration of built-in function
    /// 'strcpy'" when that is not `int`), and the call is the library call the
    /// optimiser knows — which is what lets it fold `strcmp` of two strings it
    /// can see. The table is [`Sema::library_signature`]'s.
    ///
    /// Which names are built-ins depends on the dialect, as it does in GCC:
    /// the GNU and POSIX functions are only in the GNU dialects
    /// (`-std=c89` has no built-in `index`, so a program may have its own),
    /// and the ones C99 added are not in strict C89.
    pub(super) fn implicit_library_signature(&mut self, name: &str) -> Option<Signature> {
        const GNU_ONLY: &[&str] = &[
            "bcmp",
            "bcopy",
            "bzero",
            "index",
            "mempcpy",
            "rindex",
            "stpcpy",
            "stpncpy",
            "strcasecmp",
            "strdup",
            "strncasecmp",
        ];
        const C99_ONLY: &[&str] = &[
            "_Exit",
            "atoll",
            "cbrt",
            "copysign",
            "erf",
            "erfc",
            "exp2",
            "expm1",
            "fdim",
            "fma",
            "fmax",
            "fmin",
            "hypot",
            "imaxabs",
            "isblank",
            "lgamma",
            "llabs",
            "log1p",
            "log2",
            "nearbyint",
            "nextafter",
            "remainder",
            "rint",
            "round",
            "scalbn",
            "snprintf",
            "strtof",
            "strtoll",
            "strtoull",
            "tgamma",
            "trunc",
        ];
        if !self.gating.dialect.is_gnu() {
            if GNU_ONLY.contains(&name) {
                return None;
            }
            if self.gating.standard < crate::Standard::C99 && C99_ONLY.contains(&name) {
                return None;
            }
        }
        self.library_signature(name)
    }

    /// `intmax_t`, which is the widest signed integer the model has.
    fn intmax_ty(&self) -> Ty {
        if Ty::Long.bits(&self.target) >= Ty::LongLong.bits(&self.target) {
            Ty::Long
        } else {
            Ty::LongLong
        }
    }
}
