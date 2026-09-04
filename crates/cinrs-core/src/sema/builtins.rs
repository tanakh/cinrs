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

use crate::ast;
use crate::capture::SourceRange;
use crate::gnu;
use crate::ir::{self, BinOp, BuiltinOp, Expr, ExprKind, FuncId, Function, Signature, Ty};

use super::{Entry, Sema};

/// What `__builtin_object_size` answers when it cannot work the size out.
///
/// The low bit of the mode selects between "the maximum" and "the minimum";
/// GCC's own documentation gives `(size_t) -1` and `0` as the two answers, and
/// a front end that does not track object sizes has to give exactly those.
fn object_size_answer(mode: i128) -> i128 {
    if mode & 2 == 0 { -1 } else { 0 }
}

impl Sema {
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
                let folds = self.const_eval(&value).is_some();
                Some(Expr::int(i128::from(folds), Ty::Int, range))
            }
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
            "huge_val" | "inf" => {
                self.builtin_arity(name, args, 0, range)?;
                Some(Expr::new(ExprKind::Float(f64::INFINITY), Ty::Double, range))
            }
            "huge_valf" | "inff" => {
                self.builtin_arity(name, args, 0, range)?;
                Some(Expr::new(ExprKind::Float(f64::INFINITY), Ty::Float, range))
            }
            // The payload string is ignored: a quiet NaN is a quiet NaN, and
            // Rust has no way to build one with a chosen payload.
            "nan" | "nanf" => {
                for arg in args {
                    self.expr(arg);
                }
                let ty = if rest == "nan" { Ty::Double } else { Ty::Float };
                Some(Expr::new(ExprKind::Float(f64::NAN), ty, range))
            }
            // Hints with nowhere to go. The operands are still evaluated,
            // because C says they are.
            "prefetch" | "assume" | "speculation_safe_value" => {
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
            "alloca" | "alloca_with_align" => {
                self.error(
                    range,
                    "'alloca' is not supported: Rust has no stack allocation with a size \
                     chosen at run time; use 'malloc' and 'free'",
                );
                None
            }
            // `__builtin_X` for a library function X is a call to X.
            _ if gnu::LIBRARY_BUILTINS.contains(&rest) => self.library_call(rest, args, range),
            _ => return None,
        };
        Some(result)
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
            locals: Vec::new(),
            body: None,
            range,
        });
        self.item_names.insert(name.to_owned());
        self.insert_at_file_scope(name, Entry::Function(id));
        Some(id)
    }

    /// The prototype of a library function the unit did not declare.
    ///
    /// Only the ones a `__builtin_` name is really used for are here; anything
    /// else has to be declared by including its header, which is what the
    /// diagnostic above says.
    fn library_signature(&mut self, name: &str) -> Option<Signature> {
        let size_t = self.size_ty();
        let void_ptr = self.ptr_to(Ty::Void, false);
        let const_void_ptr = self.ptr_to(Ty::Void, true);
        let char_ptr = self.ptr_to(Ty::Char, false);
        let const_char_ptr = self.ptr_to(Ty::Char, true);
        let sig = |ret: Ty, params: Vec<Ty>| Signature {
            ret,
            params,
            variadic: false,
        };
        Some(match name {
            "memcpy" | "memmove" => sig(void_ptr, vec![void_ptr, const_void_ptr, size_t]),
            "memset" => sig(void_ptr, vec![void_ptr, Ty::Int, size_t]),
            "memcmp" => sig(Ty::Int, vec![const_void_ptr, const_void_ptr, size_t]),
            "memchr" => sig(void_ptr, vec![const_void_ptr, Ty::Int, size_t]),
            "strlen" => sig(size_t, vec![const_char_ptr]),
            "strcpy" | "strcat" => sig(char_ptr, vec![char_ptr, const_char_ptr]),
            "strncpy" | "strncat" => sig(char_ptr, vec![char_ptr, const_char_ptr, size_t]),
            "strcmp" => sig(Ty::Int, vec![const_char_ptr, const_char_ptr]),
            "strncmp" => sig(Ty::Int, vec![const_char_ptr, const_char_ptr, size_t]),
            "strchr" | "strrchr" => sig(char_ptr, vec![const_char_ptr, Ty::Int]),
            "strstr" | "strpbrk" => sig(char_ptr, vec![const_char_ptr, const_char_ptr]),
            "strspn" | "strcspn" => sig(size_t, vec![const_char_ptr, const_char_ptr]),
            "abs" => sig(Ty::Int, vec![Ty::Int]),
            "labs" => sig(Ty::Long, vec![Ty::Long]),
            "llabs" => sig(Ty::LongLong, vec![Ty::LongLong]),
            "abort" => sig(Ty::Void, vec![]),
            "exit" => sig(Ty::Void, vec![Ty::Int]),
            "malloc" => sig(void_ptr, vec![size_t]),
            "calloc" => sig(void_ptr, vec![size_t, size_t]),
            "realloc" => sig(void_ptr, vec![void_ptr, size_t]),
            "free" => sig(Ty::Void, vec![void_ptr]),
            "putchar" => sig(Ty::Int, vec![Ty::Int]),
            "puts" => sig(Ty::Int, vec![const_char_ptr]),
            "tolower" | "toupper" | "isalnum" | "isalpha" | "isdigit" | "islower" | "isprint"
            | "isspace" | "isupper" => sig(Ty::Int, vec![Ty::Int]),
            "fabs" | "sqrt" | "floor" | "ceil" | "sin" | "cos" | "tan" | "sinh" | "cosh"
            | "tanh" | "exp" | "log" | "log10" | "log2" | "round" | "trunc" | "asin" | "acos"
            | "atan" => sig(Ty::Double, vec![Ty::Double]),
            "fabsf" | "sqrtf" | "floorf" | "ceilf" | "sinf" | "cosf" | "tanf" | "expf" | "logf"
            | "roundf" | "truncf" => sig(Ty::Float, vec![Ty::Float]),
            "pow" | "fmod" | "atan2" | "fmax" | "fmin" => {
                sig(Ty::Double, vec![Ty::Double, Ty::Double])
            }
            "powf" => sig(Ty::Float, vec![Ty::Float, Ty::Float]),
            _ => return None,
        })
    }
}
