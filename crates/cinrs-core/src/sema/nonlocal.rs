//! `setjmp` and `longjmp`: the non-local jumps, as a Rust unwind.
//!
//! A C `setjmp` saves a machine context and a `longjmp` restores it, which in
//! Rust is undefined behaviour: the compiler is entitled to keep values in
//! registers and stack slots across a call that it does not know returns
//! twice, and stable Rust has no way to tell it. What Rust *does* have is
//! unwinding, an edge out of every call that the compiler knows about. So:
//!
//! * `longjmp(buf, v)` reads from `buf` which activation of which function
//!   saved it, checks that the activation is still live, and *unwinds* to it
//!   with `std::panic::resume_unwind` — no panic message, no hook;
//! * a function that calls `setjmp` is lowered through the [control-flow
//!   graph](crate::cfg), with its locals hoisted *outside* a
//!   `catch_unwind` that the graph runs in. When the unwind for this
//!   activation arrives, the function re-enters its graph at the block that
//!   follows the `setjmp` the buffer names, with the `longjmp`'s value as the
//!   `setjmp`'s result. The locals keep the values they had when the unwind
//!   left them, which is stronger than C's promise (only `volatile` ones).
//!
//! That only works if the code after the `setjmp` can be resumed on its own,
//! which is why C17 7.13.1.1p4 restricts where a `setjmp` may appear — and
//! that restriction is enforced here, by [`Sema::setjmp_call`]. The statement
//! that is about to check an expression in which one is allowed marks the one
//! call node it allows; see [`Sema::with_setjmp_permit`].
//!
//! A `longjmp` the unit *declares* is replaced by a function of the unit's own
//! that unwinds, which is what lets a program take its address — libpng's
//! `png_jmpbuf` hands `longjmp` itself to the library. Every frame the unwind
//! crosses has to be `extern "C-unwind"`, which is [`crate::Options::unwind`].

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{BuiltinOp, Expr, ExprKind, FuncId, Ty};
use crate::target::{Arch, Os};

use super::{Entry, Sema};

/// Where a statement is about to check an expression that may hold a
/// `setjmp`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SetjmpPlace {
    /// The controlling expression of an `if`, `switch`, `while`, `do` or
    /// `for`: the call itself, `!` of it, or it compared with an integer
    /// constant — and, beyond C17, any of those with the call's value
    /// assigned first, `(rc = setjmp(buf)) == 0`.
    Control,
    /// A whole expression statement: the call, the call cast to `void`, and —
    /// beyond C17, as GCC accepts it — `lvalue = setjmp(buf)`.
    Statement,
    /// The initialiser of an object: `int r = setjmp(buf);`, again beyond
    /// C17, as GCC accepts it.
    Initializer,
}

/// The bytes `setjmp` writes into the buffer: five words, which is exactly
/// what GCC's `__builtin_setjmp` documents its buffer as.
const TOKEN_BYTES: u64 = 5 * 8;

/// What a `sigsetjmp` that saves the signal mask needs besides: room for a
/// `sigset_t` as glibc and musl have it.
const MASK_BYTES: u64 = 128;

/// Where a `setjmp` may stand, for the diagnostics that refuse one.
const SETJMP_PLACES: &str = "as the whole controlling expression of an 'if', 'switch', \
                             'while', 'do' or 'for' statement — on its own, negated with '!', \
                             or compared with an integer constant — or as a whole expression \
                             statement (C17 7.13.1.1p4); cinrs also takes 'r = setjmp(buf);', \
                             'int r = setjmp(buf);' and 'if ((r = setjmp(buf)) == 0)'";

impl Sema<'_> {
    /// Runs `check` over `expr` with the one `setjmp` C allows in `place`, if
    /// `expr` has one there, permitted.
    pub(super) fn with_setjmp_permit<T>(
        &mut self,
        expr: &ast::Expr,
        place: SetjmpPlace,
        check: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let permit = if self.stmt_expr_depth == 0 {
            permitted_setjmp(expr, place, &|e| self.looks_constant(e))
                .map(|call| std::ptr::from_ref(call) as usize)
        } else {
            None
        };
        let saved = std::mem::replace(&mut self.setjmp_permit, permit);
        let out = check(self);
        self.setjmp_permit = saved;
        out
    }

    /// Whether `expr` can only be an integer constant expression, judged from
    /// its shape: literals, operators over them, casts, `sizeof` and
    /// enumeration constants. It is what C17 allows the other side of a
    /// comparison with a `setjmp` to be, and what makes evaluating it after
    /// the `setjmp` rather than before unobservable.
    fn looks_constant(&self, expr: &ast::Expr) -> bool {
        match &expr.kind {
            ast::ExprKind::Int(_) | ast::ExprKind::Char(_) | ast::ExprKind::Bool(_) => true,
            ast::ExprKind::SizeofType(_)
            | ast::ExprKind::AlignofType(_)
            | ast::ExprKind::SizeofExpr(_)
            | ast::ExprKind::AlignofExpr(_) => true,
            ast::ExprKind::Ident(name) => {
                matches!(self.lookup(&name.name), Some(Entry::Constant { .. }))
            }
            ast::ExprKind::Unary { op, operand } => {
                !matches!(op, ast::UnaryOp::Deref | ast::UnaryOp::AddrOf)
                    && self.looks_constant(operand)
            }
            ast::ExprKind::Binary { lhs, rhs, .. } => {
                self.looks_constant(lhs) && self.looks_constant(rhs)
            }
            ast::ExprKind::Cast { expr, .. } => self.looks_constant(expr),
            ast::ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                self.looks_constant(cond)
                    && then_expr.as_deref().is_none_or(|e| self.looks_constant(e))
                    && self.looks_constant(else_expr)
            }
            _ => false,
        }
    }

    /// Whether `name`, about to be called, is the library's `setjmp` rather
    /// than something the program declared under that name: a declared
    /// function for the standard spellings, and nothing at all for GCC's
    /// builtin.
    pub(super) fn is_setjmp_callee(&self, name: &str) -> bool {
        if !ast::is_setjmp_name(name) {
            return false;
        }
        match self.lookup(name) {
            Some(Entry::Function(_)) => name != "__builtin_setjmp",
            None => name == "__builtin_setjmp",
            _ => false,
        }
    }

    /// A call to a `setjmp`; see [`BuiltinOp::SetJmp`] for what it becomes.
    ///
    /// `call` is the call's own node, which is what the statement around it
    /// permitted or did not.
    pub(super) fn setjmp_call(
        &mut self,
        call: &ast::Expr,
        name: &str,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        let permitted = self.setjmp_permit == Some(std::ptr::from_ref(call) as usize);
        if !permitted {
            let message = if self.stmt_expr_depth > 0 {
                format!(
                    "'{name}' inside a statement expression is not supported: a 'longjmp' \
                     comes back into the middle of the function, and only its statements can \
                     be resumed, not the ones of an expression. It may stand {SETJMP_PLACES}"
                )
            } else {
                format!(
                    "'{name}' cannot be called here: what follows it could not be resumed \
                     when a 'longjmp' comes back to it. It may stand {SETJMP_PLACES}"
                )
            };
            self.error(range, message);
            return None;
        }
        // One call per permit.
        self.setjmp_permit = None;
        if self.refuse_nonunwinding_target(name, range) {
            return None;
        }
        let saves_mask = matches!(name, "sigsetjmp" | "__sigsetjmp");
        let wanted = if saves_mask { 2 } else { 1 };
        if args.len() != wanted {
            self.error(
                range,
                format!(
                    "'{name}' takes {wanted} argument{}, but {} {} given",
                    if wanted == 1 { "" } else { "s" },
                    args.len(),
                    if args.len() == 1 { "was" } else { "were" }
                ),
            );
            return None;
        }
        let buf = self.jump_buffer(name, &args[0])?;
        let mask = if saves_mask {
            let value = self.expr(&args[1])?;
            if !value.ty.is_integer() {
                self.error(
                    args[1].range,
                    format!("the second argument of '{name}' must be an integer"),
                );
                return None;
            }
            // The mask needs room the five words do not have.
            if !is_zero(&value) {
                self.check_buffer_size(name, &args[0], buf.ty, TOKEN_BYTES + MASK_BYTES)?;
            }
            self.convert(value, Ty::Int)
        } else {
            Expr::int(0, Ty::Int, range)
        };
        self.func_setjmp = true;
        self.program.nonlocal_jumps.push(range);
        Some(Expr::new(
            ExprKind::Builtin {
                op: BuiltinOp::SetJmp,
                args: vec![self.void_pointer(buf), mask],
            },
            Ty::Int,
            range,
        ))
    }

    /// `__builtin_longjmp(buf, value)`.
    pub(super) fn builtin_longjmp(
        &mut self,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        let name = "__builtin_longjmp";
        if self.refuse_nonunwinding_target(name, range) {
            return None;
        }
        if args.len() != 2 {
            self.error(range, format!("'{name}' takes 2 arguments"));
            return None;
        }
        let buf = self.jump_buffer(name, &args[0])?;
        let value = self.expr(&args[1])?;
        if !value.ty.is_integer() {
            self.error(
                args[1].range,
                format!("the second argument of '{name}' must be an integer"),
            );
            return None;
        }
        let value = self.convert(value, Ty::Int);
        self.program.nonlocal_jumps.push(range);
        Some(Expr::new(
            ExprKind::Builtin {
                op: BuiltinOp::LongJmp,
                args: vec![self.void_pointer(buf), value],
            },
            Ty::Void,
            range,
        ))
    }

    /// Notes a call to, or the address of, a `longjmp` the unit declares,
    /// which code generation replaces with a function of its own.
    pub(super) fn note_longjmp_use(&mut self, id: FuncId, range: SourceRange) {
        let func = self.program.function(id);
        if func.body.is_some() || !ast::is_longjmp_name(&func.name) {
            return;
        }
        let name = func.name.clone();
        if self.refuse_nonunwinding_target(&name, range) {
            return;
        }
        self.program.nonlocal_jumps.push(range);
    }

    /// The buffer operand of a `setjmp` or a `longjmp`: a pointer, after the
    /// array a `jmp_buf` is has decayed.
    fn jump_buffer(&mut self, name: &str, arg: &ast::Expr) -> Option<Expr> {
        let buf = self.expr(arg)?;
        if !buf.ty.is_pointer() {
            self.error(
                arg.range,
                format!(
                    "the first argument of '{name}' must be a 'jmp_buf', not '{}'",
                    self.tyname(buf.ty)
                ),
            );
            return None;
        }
        // GCC's builtins take any buffer of five words, which is usually a
        // `void *buf[5]` that has decayed to `void **` — so there is no size
        // to check. The library's take a `jmp_buf`, which is a whole one.
        if !name.starts_with("__builtin_") {
            self.check_buffer_size(name, arg, buf.ty, TOKEN_BYTES)?;
        }
        Some(buf)
    }

    /// Refuses a buffer whose type says it is smaller than `needed` bytes.
    fn check_buffer_size(
        &mut self,
        name: &str,
        arg: &ast::Expr,
        ptr: Ty,
        needed: u64,
    ) -> Option<()> {
        let size = self
            .pointee(ptr)
            .filter(|pointee| !pointee.is_void())
            .and_then(|pointee| self.size_of(pointee));
        match size {
            Some(size) if size < needed => {
                self.error(
                    arg.range,
                    format!(
                        "the buffer given to '{name}' is {size} bytes, and cinrs needs {needed}: \
                         declare it as the 'jmp_buf' (or 'sigjmp_buf') of <setjmp.h>"
                    ),
                );
                None
            }
            _ => Some(()),
        }
    }

    fn void_pointer(&mut self, expr: Expr) -> Expr {
        let ty = self.ptr_to(Ty::Void, false);
        self.convert(expr, ty)
    }

    /// Refuses a non-local jump on a target whose Rust does not unwind:
    /// WebAssembly, where `panic=abort` is the default and the only
    /// strategy the standard library is built with, and a bare-metal one,
    /// which has no `std` to catch an unwind with.
    fn refuse_nonunwinding_target(&mut self, name: &str, range: SourceRange) -> bool {
        let target = self.target;
        let why = if target.arch == Arch::Wasm32 {
            "WebAssembly, whose Rust aborts on a panic rather than unwinding"
        } else if target.os == Os::None {
            "a target with no operating system, which has no 'std' to catch an unwind with"
        } else {
            return false;
        };
        self.error(
            range,
            format!(
                "'{name}' is not supported on {why}: cinrs makes a 'longjmp' a Rust unwind \
                 that the function which called 'setjmp' catches"
            ),
        );
        true
    }
}

/// The `setjmp` call `expr` holds where C allows one in `place`, if any.
fn permitted_setjmp<'e>(
    expr: &'e ast::Expr,
    place: SetjmpPlace,
    constant: &dyn Fn(&ast::Expr) -> bool,
) -> Option<&'e ast::Expr> {
    let call = |e: &'e ast::Expr| -> Option<&'e ast::Expr> {
        match &e.kind {
            ast::ExprKind::Call { callee, .. } => match &callee.kind {
                ast::ExprKind::Ident(name) if ast::is_setjmp_name(&name.name) => Some(e),
                _ => None,
            },
            _ => None,
        }
    };
    // Beyond C17 again, as GCC takes it: `if ((rc = setjmp(buf)) == 0)`,
    // which Jim Tcl writes. The assignment is the rest of the expression, so
    // it is resumed with it.
    let call = |e: &'e ast::Expr| -> Option<&'e ast::Expr> {
        match &e.kind {
            ast::ExprKind::Assign { op: None, rhs, .. } if place == SetjmpPlace::Control => {
                call(rhs)
            }
            _ => call(e),
        }
    };
    match place {
        SetjmpPlace::Control => {
            if let Some(found) = call(expr) {
                return Some(found);
            }
            match &expr.kind {
                ast::ExprKind::Unary {
                    op: ast::UnaryOp::LogNot,
                    operand,
                } => call(operand),
                ast::ExprKind::Binary { op, lhs, rhs }
                    if matches!(
                        op,
                        ast::BinaryOp::Lt
                            | ast::BinaryOp::Gt
                            | ast::BinaryOp::Le
                            | ast::BinaryOp::Ge
                            | ast::BinaryOp::Eq
                            | ast::BinaryOp::Ne
                    ) =>
                {
                    match (call(lhs), call(rhs)) {
                        (Some(found), None) if constant(rhs) => Some(found),
                        (None, Some(found)) if constant(lhs) => Some(found),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        SetjmpPlace::Statement => {
            let mut inner = expr;
            // `(void) setjmp(buf);`
            while let ast::ExprKind::Cast { expr, .. } = &inner.kind {
                inner = expr;
            }
            if let Some(found) = call(inner) {
                return Some(found);
            }
            match &expr.kind {
                ast::ExprKind::Assign { op: None, rhs, .. } => call(rhs),
                _ => None,
            }
        }
        SetjmpPlace::Initializer => call(expr),
    }
}

/// Whether `expr` is the constant zero.
fn is_zero(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Int(0) => true,
        ExprKind::Cast(inner) => is_zero(inner),
        _ => false,
    }
}
