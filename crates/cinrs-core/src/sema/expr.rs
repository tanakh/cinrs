//! Expressions: type checking, the implicit conversions, and lvalues.

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{
    BinOp, Callee, CmpOp, Expr, ExprKind, FuncId, Function, LogicalOp, Place, PlaceKind, Signature,
    StrData, StrId, Ty, UNREACHABLE_BUILTIN,
};
use crate::lex::{CharLit, FloatLit, FloatSuffix, IntLit, LongKind, NumBase, StrKind, StrLit};

use super::va::VaBuiltin;
use super::{ConvContext, Entry, Sema, arith_op, compare_op, place_of, round_to};

/// What `*p` turned out to be.
enum Deref {
    /// An ordinary object.
    Place(Place),
    /// A function designator: `*fp` is `fp` again, which is why `(*fp)(x)` and
    /// `fp(x)` mean the same thing.
    Function(Expr),
}

/// The left operand of a node that a *chain* of operators leaves behind.
///
/// `a + b + c` and `a, b, c` are left-associative, so what the parser hands
/// over is one node per operand with the whole of the rest hanging off its
/// left: walking down it recursively is one stack frame per operand. C23
/// 5.2.5.2p1 asks every implementation to accept a logical source line of
/// 4095 characters, which is two thousand comma operands, so that walk has to
/// be a loop — see [`Sema::expr`].
fn chain_left_operand(expr: &ast::Expr) -> Option<&ast::Expr> {
    match &expr.kind {
        ast::ExprKind::Binary { lhs, .. } | ast::ExprKind::Comma { lhs, .. } => Some(lhs),
        _ => None,
    }
}

impl Sema<'_> {
    // -- expressions --------------------------------------------------------

    /// Checks one expression.
    ///
    /// A chain of left-associative operators is walked iteratively — down the
    /// spine into a vector, then back up it in a loop — so that the depth of
    /// the recursion is the *nesting* of the expression and not the length of
    /// the chain; see [`chain_left_operand`]. Everything else is
    /// [`Sema::expr_node`], one frame per level as recursive descent always
    /// is.
    pub(super) fn expr(&mut self, expr: &ast::Expr) -> Option<Expr> {
        let Some(lhs) = chain_left_operand(expr) else {
            return self.expr_node(expr);
        };
        let mut spine = vec![expr];
        let mut node = lhs;
        while let Some(lhs) = chain_left_operand(node) {
            spine.push(node);
            node = lhs;
        }
        // The innermost left operand is checked first, exactly as the
        // recursive walk checked it first, so the diagnostics come out in
        // source order and an error in it stops the chain there.
        let mut value = self.expr_node(node)?;
        while let Some(node) = spine.pop() {
            value = self.chain_step(node, value)?;
        }
        Some(value)
    }

    /// One step back up a chain: `node` with its left operand already checked.
    fn chain_step(&mut self, node: &ast::Expr, lhs: Expr) -> Option<Expr> {
        match &node.kind {
            ast::ExprKind::Binary {
                op,
                lhs: lhs_expr,
                rhs,
            } => self.binary_with(*op, lhs, lhs_expr.range, rhs, node.range),
            ast::ExprKind::Comma { rhs, .. } => {
                let rhs = self.expr(rhs)?;
                let ty = rhs.ty;
                Some(Expr::new(
                    ExprKind::Comma {
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty,
                    node.range,
                ))
            }
            _ => unreachable!("chain_left_operand matches exactly these two"),
        }
    }

    fn expr_node(&mut self, expr: &ast::Expr) -> Option<Expr> {
        let range = expr.range;
        match &expr.kind {
            ast::ExprKind::Ident(name) => match self.lookup(&name.name) {
                Some(Entry::Object(_)) => {
                    let place = self.lvalue(expr)?;
                    Some(self.load_or_decay(place, range))
                }
                Some(Entry::Function(id)) => {
                    let id = *id;
                    Some(self.function_designator(id, range))
                }
                Some(Entry::Constant { value, ty, .. }) => {
                    let (value, ty) = (*value, *ty);
                    Some(self.const_to_expr(value, ty, range))
                }
                Some(Entry::Typedef(_)) => {
                    self.error(range, format!("'{}' names a type, not a value", name.name));
                    None
                }
                None => {
                    // `__func__` and its two GNU spellings are predeclared in
                    // every function body, and nowhere else.
                    if let Some(place) = self.function_name_literal(&name.name, range) {
                        return Some(self.load_or_decay(place, range));
                    }
                    self.report_undeclared(name);
                    None
                }
            },
            ast::ExprKind::Int(lit) => {
                let (value, ty) = self.int_literal(lit, range);
                Some(Expr::int(value, ty, range))
            }
            ast::ExprKind::Float(lit) => {
                let (value, ty) = float_literal(lit);
                Some(Expr::new(ExprKind::Float(value), ty, range))
            }
            ast::ExprKind::Char(lit) => {
                let (value, ty) = self.char_literal(lit);
                Some(Expr::int(value, ty, range))
            }
            ast::ExprKind::Str(lit) => {
                let place = self.string_place(lit, range);
                Some(self.load_or_decay(place, range))
            }
            ast::ExprKind::Unary { op, operand } => self.unary(*op, operand, range),
            ast::ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, range),
            ast::ExprKind::Assign { op, lhs, rhs } => self.assign(*op, lhs, rhs, range),
            ast::ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => self.conditional(cond, then_expr.as_deref(), else_expr, range),
            ast::ExprKind::Comma { lhs, rhs } => {
                let lhs = self.expr(lhs)?;
                let rhs = self.expr(rhs)?;
                let ty = rhs.ty;
                Some(Expr::new(
                    ExprKind::Comma {
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty,
                    range,
                ))
            }
            ast::ExprKind::Call { callee, args } => {
                // The `__builtin_va_*` forms look like calls but are not:
                // their names are reserved, so no declaration can shadow one.
                if let ast::ExprKind::Ident(name) = &callee.kind {
                    if let Some(builtin) = VaBuiltin::from_name(&name.name) {
                        return self.va_builtin(builtin, args, range);
                    }
                    // What C23's `unreachable()` expands to.
                    if name.name == UNREACHABLE_BUILTIN {
                        if !args.is_empty() {
                            self.error(range, "'unreachable' takes no arguments");
                            return None;
                        }
                        return Some(Expr::new(ExprKind::Unreachable, Ty::Void, range));
                    }
                    // Everything else GCC spells `__builtin_…`.
                    if let Some(result) = self.builtin_call(&name.name, args, range) {
                        return result;
                    }
                    // `alloca` is a *builtin* rather than a library function:
                    // no ISO header declares it, and GCC answers a call to an
                    // undeclared one with `__builtin_alloca` in its `gnu`
                    // modes ("incompatible implicit declaration of built-in
                    // function 'alloca'"). Without that the C89 implicit
                    // declaration would type it `int()` and `void *p =
                    // alloca(n)` would be a constraint violation.
                    if name.name == "alloca"
                        && self.gnu_leniency()
                        && self.lookup("alloca").is_none()
                        && let Some(result) = self.builtin_call("__builtin_alloca", args, range)
                    {
                        return result;
                    }
                }
                self.call(callee, args, range)
            }
            ast::ExprKind::VaArg { ap, ty } => self.va_arg(ap, ty, range),
            ast::ExprKind::OffsetOf { ty, member, path } => self.offsetof(ty, member, path, range),
            ast::ExprKind::Member { .. } | ast::ExprKind::Index { .. } => {
                let place = self.lvalue(expr)?;
                Some(self.load_or_decay(place, range))
            }
            ast::ExprKind::PostIncDec { op, operand } => self.inc_dec(*op, operand, true, range),
            ast::ExprKind::PreIncDec { op, operand } => self.inc_dec(*op, operand, false, range),
            ast::ExprKind::Cast { ty, expr: operand } => self.cast_expr(ty, operand, range),
            ast::ExprKind::SizeofExpr(operand) => self.sizeof_expr(operand, range),
            ast::ExprKind::SizeofType(ty) => {
                // A bound written here is evaluated here (C99 6.5.3.4p2),
                // which is the one thing `sizeof` of a variably modified type
                // does that `sizeof` of any other type does not.
                self.vla_bound = None;
                let target = self.ty_of(&ty.ty)?;
                let bound = self.vla_bound.take();
                if self.types().is_vla(target) {
                    let Some(count) = bound else {
                        self.error(ty.range, super::VM_UNSUPPORTED);
                        return None;
                    };
                    let elem = self
                        .types()
                        .elem(target)
                        .expect("a variable length array is an array type");
                    return self.vla_size(count, elem, ty.range, range);
                }
                self.sizeof(target, ty.range, range)
            }
            ast::ExprKind::AlignofExpr(operand) => {
                let (_, ty) = self.operand_place(operand, "_Alignof")?;
                self.alignof(ty, operand.range, range)
            }
            ast::ExprKind::AlignofType(name) => {
                let ty = self.ty_of(&name.ty)?;
                self.alignof(ty, name.range, range)
            }
            ast::ExprKind::Generic {
                controlling,
                assocs,
            } => self.generic_selection(controlling, assocs, range),
            ast::ExprKind::Bool(value) => Some(Expr::int(i128::from(*value), Ty::Bool, range)),
            // `nullptr` has type `nullptr_t`, which this crate does not model:
            // it is a null `void *`, which converts to every object pointer
            // and compares equal to every null one, and that is what every use
            // of it needs.
            ast::ExprKind::Nullptr => {
                let ty = self.ptr_to(Ty::Void, false);
                Some(Expr::new(ExprKind::Zeroed, ty, range))
            }
            // A compound literal is an object, so reading one goes through its
            // place — and an array one decays, just as a named array does.
            ast::ExprKind::CompoundLiteral { ty, init } => {
                let place = self.compound_literal(ty, init, range)?;
                Some(self.load_or_decay(place, range))
            }
            ast::ExprKind::StmtExpr(block) => self.stmt_expr(block, range),
            ast::ExprKind::TypesCompatible { lhs, rhs } => {
                let lhs = self.ty_of(&lhs.ty)?;
                let rhs = self.ty_of(&rhs.ty)?;
                // The types are compared after the adjustments C makes to a
                // type name, which is `Ty` equality here — `Ty` is interned,
                // and it carries no top-level qualifiers — plus the one case
                // where C's compatibility is wider than identity; see
                // `Sema::compatible`.
                Some(Expr::int(
                    i128::from(self.compatible(lhs, rhs)),
                    Ty::Int,
                    range,
                ))
            }
            ast::ExprKind::ChooseExpr {
                cond,
                then_expr,
                else_expr,
            } => self.choose_expr(cond, then_expr, else_expr, range),
            // Reported by the parser, which knows the spelling that was used.
            ast::ExprKind::ComplexPart { .. } => None,
            ast::ExprKind::Error => None,
        }
    }

    /// `__builtin_choose_expr(c, a, b)`: the unchosen operand is not even
    /// type checked, which is what makes the builtin usable in a macro that
    /// has to work for several types.
    fn choose_expr(
        &mut self,
        cond: &ast::Expr,
        then_expr: &ast::Expr,
        else_expr: &ast::Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        let value = self.expr(cond)?;
        if !value.ty.is_integer() {
            self.error(
                cond.range,
                format!(
                    "the condition of '__builtin_choose_expr' must have an integer type, not '{}'",
                    self.tyname(value.ty)
                ),
            );
            return None;
        }
        let constant = match self.const_eval(&value) {
            Some(constant) => constant,
            None => {
                self.error(
                    cond.range,
                    "the condition of '__builtin_choose_expr' is not a compile-time constant \
                     expression",
                );
                return None;
            }
        };
        let chosen = if super::is_true(constant) {
            then_expr
        } else {
            else_expr
        };
        let value = self.expr(chosen)?;
        Some(Expr::new(value.kind, value.ty, range))
    }

    /// The string literal `__func__` stands for inside a function body.
    ///
    /// C99 6.4.2.2 declares it as `static const char __func__[] = "name";`, and
    /// a string literal is exactly that object: it has the array type, so
    /// `sizeof` gives the length, and it decays like any other array.
    /// `__FUNCTION__` and `__PRETTY_FUNCTION__` are GCC's spellings of the same
    /// thing in C.
    pub(super) fn function_name_literal(
        &mut self,
        name: &str,
        range: SourceRange,
    ) -> Option<Place> {
        if !matches!(name, "__func__" | "__FUNCTION__" | "__PRETTY_FUNCTION__") {
            return None;
        }
        if self.func_name.is_empty() {
            return None;
        }
        // C99 6.4.2.2 (N611). GCC's own two spellings are extensions rather
        // than C, so a GNU dialect has them however old it is — which is what
        // `Gating::requires` already says.
        if name == "__func__" {
            self.require_standard(crate::Standard::C99, "'__func__'", range);
        }
        let lit = StrLit {
            kind: StrKind::Narrow,
            values: self.func_name.bytes().map(u32::from).collect(),
            text: String::new(),
        };
        Some(self.string_place(&lit, range))
    }

    /// "use of undeclared identifier", unless the name is a keyword a newer
    /// standard would have made of it.
    pub(super) fn report_undeclared(&mut self, name: &ast::Ident) {
        let message = self
            .newer_keyword(&name.name)
            .unwrap_or_else(|| format!("use of undeclared identifier '{}'", name.name));
        self.error(name.range, message);
    }

    /// `_Alignof`, whose value is the alignment the layout gives the type.
    fn alignof(&mut self, ty: Ty, operand_range: SourceRange, range: SourceRange) -> Option<Expr> {
        if ty.is_error() {
            return None;
        }
        if ty.is_func() {
            self.error(
                operand_range,
                "invalid application of '_Alignof' to a function type",
            );
            return None;
        }
        // An array is as strictly aligned as its elements, which is an answer
        // even for a variable length one, whose *size* nobody knows here.
        let ty = match self.types().elem(ty) {
            Some(elem) if self.types().is_vla(ty) => elem,
            _ => ty,
        };
        if ty.is_void() && self.gnu_leniency() {
            return Some(Expr::int(1, self.size_ty(), range));
        }
        let Some(layout) = self.types().size_align(ty, &self.target) else {
            let message = format!(
                "invalid application of '_Alignof' to an incomplete type '{}'",
                self.tyname(ty)
            );
            if ty.is_void() {
                let note = self.gnu_note();
                self.diags
                    .push(crate::diag::Diagnostic::error(operand_range, message).with_note(note));
            } else {
                self.error(operand_range, message);
            }
            return None;
        };
        Some(Expr::int(layout.align as i128, self.size_ty(), range))
    }

    /// `_Generic`: picks the association whose type is the controlling
    /// expression's, and checks only that one.
    ///
    /// The controlling expression is never evaluated — C says so — but it is
    /// checked, because its *type* is the whole question. That type has had
    /// the lvalue conversion applied (an array is a pointer, a function is a
    /// pointer to one, and the top-level qualifiers are gone), which is what
    /// C11 DR 481 settled. Two consequences follow, and both are about
    /// qualifiers:
    ///
    /// * an association is chosen only when its type is *unqualified* and
    ///   equal to the controlling type, since a qualified type can never be
    ///   compatible with the unqualified one lvalue conversion produced —
    ///   `_Generic(x, const int: 1, int: 2)` is 2 for every `int` lvalue,
    ///   `const` or not; and
    /// * the rule that no two associations may name *compatible* types
    ///   (C11 6.5.1.1p2) compares the types **with** their qualifiers, so
    ///   `int` and `const int` may both appear.
    fn generic_selection(
        &mut self,
        controlling: &ast::Expr,
        assocs: &[ast::GenericAssoc],
        range: SourceRange,
    ) -> Option<Expr> {
        let value = self.expr(controlling)?;
        let ty = value.ty;
        if ty.is_error() {
            return None;
        }
        let mut chosen: Option<&ast::GenericAssoc> = None;
        let mut default: Option<&ast::GenericAssoc> = None;
        let mut default_range: Option<SourceRange> = None;
        let mut seen: Vec<(Ty, ast::TypeQualifiers, SourceRange)> = Vec::new();
        for assoc in assocs {
            let Some(name) = &assoc.ty else {
                match default_range {
                    Some(previous) => self.error_note(
                        assoc.range,
                        "'_Generic' has more than one 'default' association",
                        previous,
                        "the first is",
                    ),
                    None => {
                        default_range = Some(assoc.range);
                        default = Some(assoc);
                    }
                }
                continue;
            };
            let Some(assoc_ty) = self.ty_of(&name.ty) else {
                continue;
            };
            let quals = name.ty.qualifiers;
            if let Some((_, _, previous)) = seen.iter().find(|(seen, seen_quals, _)| {
                self.compatible(*seen, assoc_ty) && *seen_quals == quals
            }) {
                let previous = *previous;
                let spelled = self.qualified_name(assoc_ty, quals);
                self.error_note(
                    name.range,
                    format!("'_Generic' has two associations for the compatible type '{spelled}'"),
                    previous,
                    "the first is",
                );
                continue;
            }
            seen.push((assoc_ty, quals, name.range));
            if self.compatible(assoc_ty, ty) && !quals.any() && chosen.is_none() {
                chosen = Some(assoc);
            }
        }
        let Some(picked) = chosen.or(default) else {
            self.error(
                range,
                format!(
                    "'_Generic' has no association for the controlling expression's type '{}'",
                    self.tyname(ty)
                ),
            );
            return None;
        };
        // Only the chosen association is checked: the others may name types
        // the operation is not defined for, which is the point of `_Generic`.
        let value = self.expr(&picked.value)?;
        Some(Expr::new(value.kind, value.ty, range))
    }

    /// A type spelled the way it was written, qualifiers and all.
    ///
    /// [`Ty`] carries no top-level qualifiers — they change nothing about the
    /// generated Rust — so a diagnostic that is *about* them has to put them
    /// back. They go in front of the type name, except on a pointer, where C
    /// writes them after the `*` (`int * const`, not `const int *`, which
    /// means something else).
    fn qualified_name(&self, ty: Ty, quals: ast::TypeQualifiers) -> String {
        let mut written = String::new();
        for (set, word) in [
            (quals.is_const, "const"),
            (quals.is_volatile, "volatile"),
            (quals.is_restrict, "restrict"),
        ] {
            if set {
                if !written.is_empty() {
                    written.push(' ');
                }
                written.push_str(word);
            }
        }
        let name = self.tyname(ty);
        if written.is_empty() {
            name
        } else if ty.is_pointer() {
            format!("{name} {written}")
        } else {
            format!("{written} {name}")
        }
    }

    /// The value of a function name used as an expression: a pointer to it.
    fn function_designator(&mut self, id: crate::ir::FuncId, range: SourceRange) -> Expr {
        let sig = self.program.function(id).sig.clone();
        let func = if sig.prototyped {
            self.program
                .types
                .func(sig.ret, sig.params.clone(), sig.variadic)
        } else {
            self.program.types.unprototyped_func(sig.ret)
        };
        let ty = self.ptr_to(func, false);
        Expr::new(ExprKind::FuncAddr(id), ty, range)
    }

    /// Reads a place, decaying an array into a pointer to its first element as
    /// C does everywhere but under `sizeof` and `&`.
    fn load_or_decay(&mut self, place: Place, range: SourceRange) -> Expr {
        if place.ty.is_array() {
            let konst = place.is_const;
            let ty = self.program.types.decayed(place.ty, konst);
            return Expr::new(ExprKind::AddrOf(place), ty, range);
        }
        // Lvalue conversion drops the qualifiers, `_Atomic` among them (C11
        // 6.3.2.1p2): reading an atomic object is an atomic load whose *value*
        // has the underlying type, which is what keeps every rule downstream
        // of here — the arithmetic conversions, `_Generic`, a call's arguments
        // — from having to know about atomics at all.
        let ty = self.types().unatomic(place.ty);
        Expr::new(ExprKind::Load(place), ty, range)
    }

    // -- lvalues ------------------------------------------------------------

    /// Whether an expression *could* denote an object, which is what decides
    /// between reading it and taking its address.
    pub(super) fn is_lvalue_form(&self, expr: &ast::Expr) -> bool {
        match &expr.kind {
            ast::ExprKind::Ident(name) => {
                matches!(self.lookup(&name.name), Some(Entry::Object(_)))
                    // `__func__` is an object too, which is what makes
                    // `sizeof(__func__)` the length of the name.
                    || (self.lookup(&name.name).is_none()
                        && !self.func_name.is_empty()
                        && matches!(
                            name.name.as_str(),
                            "__func__" | "__FUNCTION__" | "__PRETTY_FUNCTION__"
                        ))
            }
            ast::ExprKind::Unary {
                op: ast::UnaryOp::Deref,
                ..
            }
            | ast::ExprKind::Index { .. }
            | ast::ExprKind::Member { .. }
            | ast::ExprKind::Str(_)
            | ast::ExprKind::CompoundLiteral { .. } => true,
            _ => false,
        }
    }

    /// Resolves an expression that denotes an object.
    pub(super) fn lvalue(&mut self, expr: &ast::Expr) -> Option<Place> {
        let range = expr.range;
        match &expr.kind {
            ast::ExprKind::Ident(name) => match self.lookup(&name.name) {
                Some(Entry::Object(id)) => {
                    let id = *id;
                    let info = self.program.object(id);
                    Some(place_of(
                        PlaceKind::Object(id),
                        info.ty,
                        info.is_const,
                        range,
                    ))
                }
                Some(_) => {
                    self.error(range, "expression is not assignable");
                    None
                }
                None => {
                    if let Some(place) = self.function_name_literal(&name.name, range) {
                        return Some(place);
                    }
                    self.report_undeclared(name);
                    None
                }
            },
            ast::ExprKind::Unary {
                op: ast::UnaryOp::Deref,
                operand,
            } => match self.deref(operand, range)? {
                Deref::Place(place) => Some(place),
                Deref::Function(_) => {
                    self.error(range, "a function designator is not an object");
                    None
                }
            },
            ast::ExprKind::Index { base, index } => self.index_place(base, index, range),
            ast::ExprKind::Member { base, arrow, field } => {
                self.member_place(base, *arrow, field, range)
            }
            ast::ExprKind::Str(lit) => Some(self.string_place(lit, range)),
            ast::ExprKind::CompoundLiteral { ty, init } => self.compound_literal(ty, init, range),
            _ => {
                self.error(range, "expression is not assignable");
                None
            }
        }
    }

    /// Resolves an lvalue that is about to be written to.
    pub(super) fn lvalue_assignable(&mut self, expr: &ast::Expr) -> Option<Place> {
        if !self.is_lvalue_form(expr) {
            self.error(expr.range, "expression is not assignable");
            return None;
        }
        let place = self.lvalue(expr)?;
        if place.ty.is_error() {
            return None;
        }
        if place.ty.is_array() {
            self.error(
                expr.range,
                format!("array type '{}' is not assignable", self.tyname(place.ty)),
            );
            return None;
        }
        if rooted_in_temporary(&place) {
            self.error(expr.range, "expression is not assignable");
            return None;
        }
        if place.is_const {
            self.report_const_assignment(&place, expr.range);
            return None;
        }
        Some(place)
    }

    fn report_const_assignment(&mut self, place: &Place, range: SourceRange) {
        if let PlaceKind::Object(id) = &place.kind {
            let object = self.program.object(*id);
            let (name, previous) = (object.name.clone(), object.range);
            self.error_note(
                range,
                format!("cannot assign to variable '{name}' with const-qualified type"),
                previous,
                format!("'{name}' is declared const"),
            );
            return;
        }
        self.error(
            range,
            format!(
                "cannot assign to a location of const-qualified type '{}'",
                self.tyname(place.ty)
            ),
        );
    }

    fn deref(&mut self, operand: &ast::Expr, range: SourceRange) -> Option<Deref> {
        let ptr = self.expr(operand)?;
        if ptr.ty.is_error() {
            return None;
        }
        let Some(pointee) = self.pointee(ptr.ty) else {
            self.error(
                range,
                format!(
                    "indirection requires pointer operand ('{}' invalid)",
                    self.tyname(ptr.ty)
                ),
            );
            return None;
        };
        if pointee.is_func() {
            return Some(Deref::Function(ptr));
        }
        if !self.types().is_complete(pointee) {
            self.error(
                range,
                format!(
                    "dereference of a pointer to the incomplete type '{}'",
                    self.tyname(pointee)
                ),
            );
            return None;
        }
        let konst = self.types().points_to_const(ptr.ty);
        Some(Deref::Place(place_of(
            PlaceKind::Deref(Box::new(ptr)),
            pointee,
            konst,
            range,
        )))
    }

    fn index_place(
        &mut self,
        base: &ast::Expr,
        index: &ast::Expr,
        range: SourceRange,
    ) -> Option<Place> {
        let lhs = self.expr(base)?;
        let rhs = self.expr(index)?;
        if lhs.ty.is_error() || rhs.ty.is_error() {
            return None;
        }
        // `a[i]` and `i[a]` are the same thing, which falls straight out of
        // C's definition of subscripting as `*(a + i)`.
        let (ptr, subscript) = if lhs.ty.is_pointer() {
            (lhs, rhs)
        } else if rhs.ty.is_pointer() {
            (rhs, lhs)
        } else {
            self.error(
                range,
                format!(
                    "subscripted value is not an array or pointer ('{}' invalid)",
                    self.tyname(lhs.ty)
                ),
            );
            return None;
        };
        if !subscript.ty.is_integer() {
            self.error(
                subscript.range,
                format!(
                    "array subscript is not an integer ('{}' invalid)",
                    self.tyname(subscript.ty)
                ),
            );
            return None;
        }
        let pointee = self.pointee(ptr.ty).expect("checked above");
        if pointee.is_func() || !self.types().is_complete(pointee) {
            self.error(
                range,
                format!(
                    "subscript of a pointer to the incomplete type '{}'",
                    self.tyname(pointee)
                ),
            );
            return None;
        }
        let konst = self.types().points_to_const(ptr.ty);
        Some(place_of(
            PlaceKind::Index {
                base: Box::new(ptr),
                index: Box::new(subscript),
            },
            pointee,
            konst,
            range,
        ))
    }

    fn member_place(
        &mut self,
        base: &ast::Expr,
        arrow: bool,
        field: &ast::Ident,
        range: SourceRange,
    ) -> Option<Place> {
        let base_place = if arrow {
            let ptr = self.expr(base)?;
            if ptr.ty.is_error() {
                return None;
            }
            let Some(pointee) = self.pointee(ptr.ty) else {
                self.error(
                    range,
                    format!(
                        "member reference type '{}' is not a pointer; did you mean to use '.'?",
                        self.tyname(ptr.ty)
                    ),
                );
                return None;
            };
            let konst = self.types().points_to_const(ptr.ty);
            place_of(PlaceKind::Deref(Box::new(ptr)), pointee, konst, base.range)
        } else if self.is_lvalue_form(base) {
            self.lvalue(base)?
        } else {
            let value = self.expr(base)?;
            let ty = value.ty;
            place_of(PlaceKind::Temporary(Box::new(value)), ty, false, base.range)
        };

        if base_place.ty.is_error() {
            return None;
        }
        let Ty::Record(record) = base_place.ty else {
            let what = if arrow {
                "is not a pointer to a structure or union"
            } else {
                "is not a structure or union"
            };
            self.error(
                range,
                format!(
                    "member reference base type '{}' {what}",
                    self.tyname(base_place.ty)
                ),
            );
            return None;
        };
        if !self.types().record(record).complete {
            self.error(
                range,
                format!(
                    "member access into the incomplete type '{}'",
                    self.tyname(base_place.ty)
                ),
            );
            return None;
        }
        let Some(path) = self.member_path(record, &field.name) else {
            self.error(
                field.range,
                format!(
                    "no member named '{}' in '{}'",
                    field.name,
                    self.tyname(base_place.ty)
                ),
            );
            return None;
        };
        // A member reached through an anonymous member is a chain of accesses:
        // `s.x` is `s.__cinrs_anon0.x` in the generated Rust, and the place
        // says exactly that.
        let mut place = base_place;
        let mut current = record;
        for index in path {
            let member = self.types().record(current).fields[index].clone();
            let is_const = place.is_const || member.is_const;
            place = place_of(
                PlaceKind::Field {
                    base: Box::new(place),
                    record: current,
                    index,
                },
                member.ty,
                is_const,
                range,
            );
            if let Ty::Record(inner) = member.ty {
                current = inner;
            }
        }
        Some(place)
    }

    /// The chain of member indices that reaches `name`, looking through the
    /// anonymous members C11 makes transparent.
    ///
    /// A direct member always wins over one inside an anonymous member, which
    /// is what the "member of the enclosing structure" wording amounts to.
    pub(super) fn member_path(
        &self,
        record: crate::ir::RecordId,
        name: &str,
    ) -> Option<Vec<usize>> {
        let fields = &self.types().record(record).fields;
        if let Some(index) = fields
            .iter()
            .position(|field| !field.anonymous && field.name == name)
        {
            return Some(vec![index]);
        }
        for (index, field) in fields.iter().enumerate() {
            if !field.anonymous {
                continue;
            }
            if let Ty::Record(inner) = field.ty
                && let Some(rest) = self.member_path(inner, name)
            {
                let mut path = Vec::with_capacity(rest.len() + 1);
                path.push(index);
                path.extend(rest);
                return Some(path);
            }
        }
        None
    }

    fn string_place(&mut self, lit: &StrLit, range: SourceRange) -> Place {
        let elem = self.string_elem(lit.kind);
        let id = StrId(self.program.strings.len() as u32);
        self.program.strings.push(StrData {
            values: lit.values.clone(),
            elem,
        });
        let len = lit.values.len() as u64 + 1;
        let ty = self.program.types.array(elem, len, false);
        place_of(PlaceKind::Str(id), ty, false, range)
    }

    /// The element type of a string literal of this kind (C11 6.4.5p6).
    ///
    /// `u8"…"` is the one that moved: its elements were `char` in C11 and C17,
    /// and C23 gave it `char8_t`, which is an `unsigned char`.
    pub(super) fn string_elem(&self, kind: StrKind) -> Ty {
        match kind {
            StrKind::Narrow => Ty::Char,
            StrKind::Utf8 if self.gating.standard >= crate::Standard::C23 => Ty::UChar,
            StrKind::Utf8 => Ty::Char,
            StrKind::Utf16 => Ty::char16_ty(),
            StrKind::Utf32 => Ty::char32_ty(),
            StrKind::Wide => Ty::wchar_ty(&self.target),
        }
    }

    // -- unary operators ----------------------------------------------------

    fn unary(&mut self, op: ast::UnaryOp, operand: &ast::Expr, range: SourceRange) -> Option<Expr> {
        match op {
            ast::UnaryOp::AddrOf => self.address_of(operand, range),
            ast::UnaryOp::Deref => match self.deref(operand, range)? {
                Deref::Place(place) => Some(self.load_or_decay(place, range)),
                Deref::Function(ptr) => Some(ptr),
            },
            ast::UnaryOp::Plus | ast::UnaryOp::Minus => {
                let value = self.expr(operand)?;
                self.require_arithmetic(&value, op.as_str(), operand.range)?;
                let promoted = self.promoted(&value);
                let bits = self.narrow_bits(&value);
                let value = self.convert(value, promoted);
                if op == ast::UnaryOp::Plus {
                    return Some(Expr::new(value.kind, promoted, range).narrowed(bits));
                }
                // Folding `-` into the literal is what makes a negative
                // constant read like one in the generated code.
                if let ExprKind::Int(v) = &value.kind {
                    return Some(Expr::int(promoted.wrap(-*v, &self.target), promoted, range));
                }
                if let ExprKind::Float(v) = &value.kind {
                    return Some(Expr::new(ExprKind::Float(-*v), promoted, range));
                }
                Some(Expr::new(ExprKind::Neg(Box::new(value)), promoted, range).narrowed(bits))
            }
            ast::UnaryOp::BitNot => {
                let value = self.expr(operand)?;
                self.require_integer(&value, "~", operand.range)?;
                let promoted = self.promoted(&value);
                let bits = self.narrow_bits(&value);
                let value = self.convert(value, promoted);
                Some(Expr::new(ExprKind::BitNot(Box::new(value)), promoted, range).narrowed(bits))
            }
            ast::UnaryOp::LogNot => {
                // `!x` is `x == 0`, which also gives it the right type.
                let value = self.expr(operand)?;
                if !value.ty.is_scalar() {
                    self.error(
                        operand.range,
                        format!(
                            "invalid operand of type '{}' to unary operator '!'",
                            self.tyname(value.ty)
                        ),
                    );
                    return None;
                }
                let (lhs, rhs) = if value.ty.is_pointer() {
                    let zero = Expr::new(ExprKind::Zeroed, value.ty, range);
                    (value, zero)
                } else {
                    let zero = Expr::int(0, Ty::Int, range);
                    let (lhs, rhs, _) = self.balance(value, zero);
                    (lhs, rhs)
                };
                Some(Expr::new(
                    ExprKind::Compare {
                        op: CmpOp::Eq,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    Ty::Int,
                    range,
                ))
            }
        }
    }

    fn address_of(&mut self, operand: &ast::Expr, range: SourceRange) -> Option<Expr> {
        if let ast::ExprKind::Ident(name) = &operand.kind
            && let Some(Entry::Function(id)) = self.lookup(&name.name)
        {
            let id = *id;
            return Some(self.function_designator(id, range));
        }
        if self.is_lvalue_form(operand) {
            let place = self.lvalue(operand)?;
            if place.ty.is_error() {
                return None;
            }
            if self.bit_field_of(&place).is_some() {
                // A bit-field has no address: it may share a byte with its
                // neighbours, and it need not start on one.
                self.error(range, "cannot take the address of a bit-field");
                return None;
            }
            if rooted_in_temporary(&place) {
                self.error(
                    range,
                    "cannot take the address of a temporary; the object does not outlive \
                     the expression",
                );
                return None;
            }
            if place.ty.is_va_list() {
                self.error(range, "pointers to va_list are not supported yet");
                return None;
            }
            // `&a` on an array is a pointer *to the array*, not to its first
            // element, which is exactly what the place's own type gives.
            let ty = self.ptr_to(place.ty, place.is_const);
            return Some(Expr::new(ExprKind::AddrOf(place), ty, range));
        }
        let value = self.expr(operand)?;
        self.error(
            range,
            format!(
                "cannot take the address of an rvalue of type '{}'",
                self.tyname(value.ty)
            ),
        );
        None
    }

    fn require_arithmetic(&mut self, value: &Expr, op: &str, range: SourceRange) -> Option<Ty> {
        if value.ty.is_arithmetic() {
            return Some(value.ty);
        }
        if value.ty.is_error() {
            return None;
        }
        self.error(
            range,
            format!(
                "invalid operand of type '{}' to unary operator '{op}'",
                self.tyname(value.ty)
            ),
        );
        None
    }

    fn require_integer(&mut self, value: &Expr, op: &str, range: SourceRange) -> Option<Ty> {
        if value.ty.is_integer() {
            return Some(value.ty);
        }
        if value.ty.is_error() {
            return None;
        }
        self.error(
            range,
            format!(
                "operator '{op}' requires an integer operand, but the operand has type '{}'",
                self.tyname(value.ty)
            ),
        );
        None
    }

    // -- binary operators ---------------------------------------------------

    fn binary(
        &mut self,
        op: ast::BinaryOp,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        let lhs_value = self.expr(lhs)?;
        self.binary_with(op, lhs_value, lhs.range, rhs, range)
    }

    /// [`Sema::binary`] with the left operand already checked.
    ///
    /// This is the half a chain folds with; `lhs_range` is where the left
    /// operand was written, which is where a complaint about it points.
    fn binary_with(
        &mut self,
        op: ast::BinaryOp,
        lhs_value: Expr,
        lhs_range: SourceRange,
        rhs: &ast::Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        use ast::BinaryOp as B;
        if matches!(op, B::LogAnd | B::LogOr) {
            let rhs_value = self.expr(rhs)?;
            self.require_scalar(&lhs_value, op.as_str(), lhs_range)?;
            self.require_scalar(&rhs_value, op.as_str(), rhs.range)?;
            let logical = if op == B::LogAnd {
                LogicalOp::And
            } else {
                LogicalOp::Or
            };
            return Some(Expr::new(
                ExprKind::Logical {
                    op: logical,
                    lhs: Box::new(lhs_value),
                    rhs: Box::new(rhs_value),
                },
                Ty::Int,
                range,
            ));
        }

        let rhs_value = self.expr(rhs)?;

        if let Some(cmp) = compare_op(op) {
            if lhs_value.ty.is_pointer() || rhs_value.ty.is_pointer() {
                return self.pointer_compare(cmp, lhs_value, rhs_value, range);
            }
            self.require_arithmetic(&lhs_value, op.as_str(), lhs_range)?;
            self.require_arithmetic(&rhs_value, op.as_str(), rhs.range)?;
            let (lhs_value, rhs_value, _) = self.balance(lhs_value, rhs_value);
            return Some(Expr::new(
                ExprKind::Compare {
                    op: cmp,
                    lhs: Box::new(lhs_value),
                    rhs: Box::new(rhs_value),
                },
                Ty::Int,
                range,
            ));
        }

        let bin = arith_op(op).expect("every remaining operator is arithmetic");
        if matches!(bin, BinOp::Add | BinOp::Sub)
            && (lhs_value.ty.is_pointer() || rhs_value.ty.is_pointer())
        {
            return self.pointer_arithmetic(bin, lhs_value, rhs_value, range);
        }

        let integer_only = matches!(
            bin,
            BinOp::Rem | BinOp::BitAnd | BinOp::BitXor | BinOp::BitOr | BinOp::Shl | BinOp::Shr
        );
        if integer_only {
            self.require_binary_integer(&lhs_value, &rhs_value, bin, range)?;
        } else {
            self.require_binary_arithmetic(&lhs_value, &rhs_value, bin, range)?;
        }

        if bin.is_shift() {
            // The operands of a shift are promoted separately: the result has
            // the type of the promoted left operand, and — for a bit-field the
            // promotions do not reach — its width. `x.b << 32` on a forty-bit
            // field shifts in forty bits.
            let lhs_ty = self.promoted(&lhs_value);
            let rhs_ty = self.promoted(&rhs_value);
            let bits = self.narrow_bits(&lhs_value);
            let lhs_value = self.convert(lhs_value, lhs_ty);
            let rhs_value = self.convert(rhs_value, rhs_ty);
            return Some(
                Expr::new(
                    ExprKind::Binary {
                        op: bin,
                        lhs: Box::new(lhs_value),
                        rhs: Box::new(rhs_value),
                    },
                    lhs_ty,
                    range,
                )
                .narrowed(bits),
            );
        }

        let common = Ty::usual_arithmetic(
            self.promoted(&lhs_value),
            self.promoted(&rhs_value),
            &self.target,
        );
        let bits = self.result_bits(common, [&lhs_value, &rhs_value]);
        let (lhs_value, rhs_value, common) = self.balance(lhs_value, rhs_value);
        Some(
            Expr::new(
                ExprKind::Binary {
                    op: bin,
                    lhs: Box::new(lhs_value),
                    rhs: Box::new(rhs_value),
                },
                common,
                range,
            )
            .narrowed(bits),
        )
    }

    /// `p + n`, `n + p`, `p - n` and `p - q`.
    fn pointer_arithmetic(
        &mut self,
        op: BinOp,
        lhs: Expr,
        rhs: Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        let both = lhs.ty.is_pointer() && rhs.ty.is_pointer();
        if both {
            if op != BinOp::Sub || !self.subtractable(lhs.ty, rhs.ty) {
                self.report_bad_operands(op, &lhs, &rhs, range);
                return None;
            }
            self.check_pointee_arithmetic(lhs.ty, range)?;
            let ty = Ty::ptrdiff_ty(&self.target);
            return Some(Expr::new(
                ExprKind::PtrDiff {
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                ty,
                range,
            ));
        }
        let (ptr, index, sub) = if lhs.ty.is_pointer() {
            (lhs, rhs, op == BinOp::Sub)
        } else if op == BinOp::Add {
            (rhs, lhs, false)
        } else {
            self.report_bad_operands(op, &lhs, &rhs, range);
            return None;
        };
        if !index.ty.is_integer() {
            self.report_bad_operands(op, &ptr, &index, range);
            return None;
        }
        self.check_pointee_arithmetic(ptr.ty, range)?;
        let ty = ptr.ty;
        Some(Expr::new(
            ExprKind::PtrOffset {
                ptr: Box::new(ptr),
                index: Box::new(index),
                sub,
            },
            ty,
            range,
        ))
    }

    /// Whether `a - b` is defined for two pointer types.
    fn subtractable(&self, a: Ty, b: Ty) -> bool {
        self.types().same_pointee(a, b)
    }

    /// Arithmetic needs to know how big the pointee is.
    fn check_pointee_arithmetic(&mut self, ptr: Ty, range: SourceRange) -> Option<()> {
        let pointee = self.pointee(ptr).expect("called on a pointer");
        if pointee.is_func() {
            self.error(
                range,
                "arithmetic on a pointer to a function is not allowed",
            );
            return None;
        }
        // `void *` arithmetic is GCC's extension, with `sizeof(void) == 1`;
        // it is common enough in real C to be worth accepting.
        if !pointee.is_void() && !self.types().is_complete(pointee) {
            self.error(
                range,
                format!(
                    "arithmetic on a pointer to the incomplete type '{}'",
                    self.tyname(pointee)
                ),
            );
            return None;
        }
        Some(())
    }

    fn pointer_compare(
        &mut self,
        op: CmpOp,
        lhs: Expr,
        rhs: Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        // A null pointer constant on either side takes the other's type.
        let (lhs, rhs) = match (lhs.ty.is_pointer(), rhs.ty.is_pointer()) {
            (true, false) if self.is_null_constant(&rhs) => {
                let ty = lhs.ty;
                let range = rhs.range;
                (lhs, Expr::new(ExprKind::Zeroed, ty, range))
            }
            (false, true) if self.is_null_constant(&lhs) => {
                let ty = rhs.ty;
                let range = lhs.range;
                (Expr::new(ExprKind::Zeroed, ty, range), rhs)
            }
            (true, true) => (lhs, rhs),
            _ => {
                let name = if lhs.ty.is_pointer() {
                    self.tyname(rhs.ty)
                } else {
                    self.tyname(lhs.ty)
                };
                self.error(
                    range,
                    format!(
                        "comparison between a pointer and '{name}'; an integer needs a cast, \
                         and only the constant 0 is a null pointer"
                    ),
                );
                return None;
            }
        };

        let functions =
            self.types().is_func_pointer(lhs.ty) || self.types().is_func_pointer(rhs.ty);
        if functions {
            if !matches!(op, CmpOp::Eq | CmpOp::Ne) {
                self.error(range, "function pointers can only be compared for equality");
                return None;
            }
            // A null pointer constant took the other operand's type above, so
            // the two agree; `void *` against a function pointer is the
            // conversion `pointer_assignable` already allows for `dlsym`'s
            // sake, which ISO C forbids only under `-pedantic`. What is left
            // is two function pointer types that are not compatible — and note
            // that `double (*)()` and `double (*)(double)` *are* compatible
            // (6.7.6.3p15), which is what `compatible` knows and plain type
            // equality does not.
            let null = matches!(lhs.kind, ExprKind::Zeroed) || matches!(rhs.kind, ExprKind::Zeroed);
            let void_pointer =
                self.types().is_void_pointer(lhs.ty) || self.types().is_void_pointer(rhs.ty);
            // GCC and Clang only warn here (`-Wcompare-distinct-pointer-
            // types`) and compare the two addresses; the GNU dialects follow
            // them, and the strict ones keep the constraint violation.
            if !null && !void_pointer && !self.compatible(lhs.ty, rhs.ty) && !self.gnu_leniency() {
                let message = format!(
                    "comparison of distinct function pointer types '{}' and '{}'",
                    self.tyname(lhs.ty),
                    self.tyname(rhs.ty)
                );
                let note = self.gnu_note();
                self.diags
                    .push(crate::diag::Diagnostic::error(range, message).with_note(note));
                return None;
            }
            // Rust compares raw pointers only when they have the same type.
            let common = if self.types().is_func_pointer(lhs.ty) {
                lhs.ty
            } else {
                rhs.ty
            };
            let lhs = self.convert(lhs, common);
            let rhs = self.convert(rhs, common);
            return Some(Expr::new(
                ExprKind::Compare {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                Ty::Int,
                range,
            ));
        }

        if lhs.ty != rhs.ty
            && !self.types().same_pointee(lhs.ty, rhs.ty)
            && !self.types().is_void_pointer(lhs.ty)
            && !self.types().is_void_pointer(rhs.ty)
        {
            self.error(
                range,
                format!(
                    "comparison of distinct pointer types '{}' and '{}'",
                    self.tyname(lhs.ty),
                    self.tyname(rhs.ty)
                ),
            );
            return None;
        }
        // Rust compares raw pointers only when they have the same type.
        let common = lhs.ty;
        let rhs = self.convert(rhs, common);
        Some(Expr::new(
            ExprKind::Compare {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
            Ty::Int,
            range,
        ))
    }

    fn report_bad_operands(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, range: SourceRange) {
        if lhs.ty.is_error() || rhs.ty.is_error() {
            return;
        }
        self.error(
            range,
            format!(
                "invalid operands to binary '{}' ('{}' and '{}')",
                op.as_str(),
                self.tyname(lhs.ty),
                self.tyname(rhs.ty)
            ),
        );
    }

    fn require_scalar(&mut self, value: &Expr, op: &str, range: SourceRange) -> Option<()> {
        if value.ty.is_scalar() {
            return Some(());
        }
        if value.ty.is_error() {
            return None;
        }
        self.error(
            range,
            format!(
                "invalid operand of type '{}' to operator '{op}'",
                self.tyname(value.ty)
            ),
        );
        None
    }

    fn require_binary_arithmetic(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        op: BinOp,
        range: SourceRange,
    ) -> Option<()> {
        if lhs.ty.is_arithmetic() && rhs.ty.is_arithmetic() {
            return Some(());
        }
        self.report_bad_operands(op, lhs, rhs, range);
        None
    }

    fn require_binary_integer(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        op: BinOp,
        range: SourceRange,
    ) -> Option<()> {
        if lhs.ty.is_integer() && rhs.ty.is_integer() {
            return Some(());
        }
        if lhs.ty.is_error() || rhs.ty.is_error() {
            return None;
        }
        self.error(
            range,
            format!(
                "operator '{}' requires integer operands ('{}' and '{}' given)",
                op.as_str(),
                self.tyname(lhs.ty),
                self.tyname(rhs.ty)
            ),
        );
        None
    }

    // -- conditional, assignment, calls -------------------------------------

    fn conditional(
        &mut self,
        cond: &ast::Expr,
        then_expr: Option<&ast::Expr>,
        else_expr: &ast::Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        // GNU's `a ?: b` is `a ? a : b` with `a` evaluated once, so the two
        // share everything but the node they end up in.
        let Some(then_expr) = then_expr else {
            return self.conditional_default(cond, else_expr, range);
        };
        let cond = self.condition(cond)?;
        let then_value = self.expr(then_expr)?;
        let else_value = self.expr(else_expr)?;
        let build = |cond: Expr, then_value: Expr, else_value: Expr, ty: Ty| {
            Expr::new(
                ExprKind::Cond {
                    cond: Box::new(cond),
                    then_expr: Box::new(then_value),
                    else_expr: Box::new(else_value),
                },
                ty,
                range,
            )
        };

        if then_value.ty == else_value.ty {
            let ty = then_value.ty;
            if !ty.is_arithmetic() {
                return Some(build(cond, then_value, else_value, ty));
            }
        }
        if then_value.ty.is_arithmetic() && else_value.ty.is_arithmetic() {
            let (then_value, else_value, common) = self.balance(then_value, else_value);
            return Some(build(cond, then_value, else_value, common));
        }
        if (then_value.ty.is_pointer() || else_value.ty.is_pointer())
            && let Some(common) = self.common_pointer(&then_value, &else_value)
        {
            let then_value = self.convert(then_value, common);
            let else_value = self.convert(else_value, common);
            return Some(build(cond, then_value, else_value, common));
        }
        // C requires both operands to be `void` or neither; GCC accepts one of
        // each in every mode it has — `-std=c99` included, where it is only a
        // pedantic warning — and the value of the other operand is discarded.
        // `x ? (void)0 : f()` is how a macro writes "call `f` only sometimes"
        // in an expression, and refusing it would be refusing an extension
        // that the strict modes of the compiler this crate follows still have.
        if then_value.ty.is_void() || else_value.ty.is_void() {
            return Some(build(cond, then_value, else_value, Ty::Void));
        }
        self.error(
            range,
            format!(
                "the second and third operands of '?:' have incompatible types '{}' and '{}'",
                self.tyname(then_value.ty),
                self.tyname(else_value.ty)
            ),
        );
        None
    }

    /// GNU's `a ?: b`, whose first operand is both the condition and the
    /// result — and is evaluated exactly once, which is the whole reason the
    /// extension exists.
    fn conditional_default(
        &mut self,
        cond: &ast::Expr,
        else_expr: &ast::Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        let value = self.condition(cond)?;
        let other = self.expr(else_expr)?;
        let build = |value: Expr, other: Expr, ty: Ty| {
            Expr::new(
                ExprKind::CondDefault {
                    value: Box::new(value),
                    else_expr: Box::new(other),
                },
                ty,
                range,
            )
        };
        if value.ty.is_arithmetic() && other.ty.is_arithmetic() {
            let (value, other, common) = self.balance(value, other);
            return Some(build(value, other, common));
        }
        if let Some(common) = self.common_pointer(&value, &other) {
            let value = self.convert(value, common);
            let other = self.convert(other, common);
            return Some(build(value, other, common));
        }
        self.error(
            range,
            format!(
                "the operands of '?:' have incompatible types '{}' and '{}'",
                self.tyname(value.ty),
                self.tyname(other.ty)
            ),
        );
        None
    }

    /// The type C gives `cond ? p : q` when pointers are involved.
    fn common_pointer(&mut self, lhs: &Expr, rhs: &Expr) -> Option<Ty> {
        if lhs.ty == rhs.ty {
            return Some(lhs.ty);
        }
        if lhs.ty.is_pointer() && self.is_null_constant(rhs) {
            return Some(lhs.ty);
        }
        if rhs.ty.is_pointer() && self.is_null_constant(lhs) {
            return Some(rhs.ty);
        }
        if !lhs.ty.is_pointer() || !rhs.ty.is_pointer() {
            return None;
        }
        if self.types().same_pointee(lhs.ty, rhs.ty) {
            // The result keeps `const` if either side has it.
            let pointee = self.pointee(lhs.ty).expect("a pointer");
            let konst =
                self.types().points_to_const(lhs.ty) || self.types().points_to_const(rhs.ty);
            return Some(self.ptr_to(pointee, konst));
        }
        // C11 6.5.15p6: one operand a pointer to `void` and the other a
        // pointer to an object type gives a pointer to `void`, and this comes
        // before the composite below so that `void *` wins whichever side it
        // is on.
        if self.types().is_void_pointer(lhs.ty) || self.types().is_void_pointer(rhs.ty) {
            let konst =
                self.types().points_to_const(lhs.ty) || self.types().points_to_const(rhs.ty);
            return Some(self.ptr_to(Ty::Void, konst));
        }
        // 6.5.15p6 again: two pointers to compatible types give a pointer to
        // the *composite* type, and either of a compatible pair will do for
        // one. `pointer_assignable` is what knows the three ways two pointee
        // types can be interchangeable without being equal — an enumerated
        // type and the integer type it is compatible with (6.7.2.2p4), a
        // prototyped function type and one with an empty parameter list
        // (6.7.6.3p15), and two integer types that differ only in signedness,
        // which is `-Wpointer-sign` and which GCC accepts here too.
        // `execute/enum-3` is `1 ? (enum e *)q : (int *)p`.
        if self.pointer_assignable(lhs.ty, rhs.ty) {
            let pointee = self.pointee(lhs.ty).expect("a pointer");
            let konst =
                self.types().points_to_const(lhs.ty) || self.types().points_to_const(rhs.ty);
            return Some(self.ptr_to(pointee, konst));
        }
        None
    }

    pub(super) fn call(
        &mut self,
        callee: &ast::Expr,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        let (target, sig, name) = self.callee(callee)?;

        let mut values = Vec::with_capacity(args.len());
        let mut failed = false;
        for (index, arg) in args.iter().enumerate() {
            let Some(value) = self.expr(arg) else {
                failed = true;
                continue;
            };
            match sig.params.get(index) {
                Some(param) => {
                    let value = self.convert_for(
                        value,
                        *param,
                        ConvContext::Argument {
                            index: index + 1,
                            func: name.clone(),
                        },
                    );
                    values.push(value);
                }
                None => {
                    // The variable part of a variadic call gets the default
                    // argument promotions, and so does *every* argument of a
                    // call through a type with no prototype (C99 6.5.2.2p6):
                    // `float` widens to `double` and the small integer types
                    // to `int`.
                    let promoted = self.promoted_argument(&value);
                    let value = self.convert(value, promoted);
                    values.push(value);
                }
            }
        }

        let too_few = args.len() < sig.params.len();
        // A function type with no prototype says nothing about how many
        // arguments it takes, so no count can be wrong.
        let too_many = args.len() > sig.params.len() && !sig.variadic && sig.prototyped;
        if too_few || too_many {
            let word = if too_few { "few" } else { "many" };
            let expected = if sig.variadic {
                format!("at least {}", sig.params.len())
            } else {
                sig.params.len().to_string()
            };
            let message = format!(
                "too {word} arguments to function call, expected {expected}, have {}",
                args.len()
            );
            match &target {
                Callee::Direct(id) => {
                    let declared = self.program.function(*id).range;
                    self.error_note(range, message, declared, format!("'{name}' is declared"));
                }
                Callee::Indirect(_) => self.error(range, message),
            }
            return None;
        }
        if failed {
            return None;
        }
        Some(Expr::new(
            ExprKind::Call {
                callee: target,
                args: values,
            },
            sig.ret,
            range,
        ))
    }

    /// Declares `extern int f();` at file scope, as a call to an undeclared
    /// `f` does in C89 (6.3.2.2).
    ///
    /// The type has no prototype, which is exactly what the standard's own
    /// `extern int identifier();` says: the call passes what it passes, with
    /// the default argument promotions applied, and a later declaration has to
    /// be *compatible* with it or it is the ordinary "conflicting types"
    /// error. Having no body makes it an `extern` declaration like any other,
    /// so `abort()` in a program that never declared it links against the C
    /// library.
    fn implicit_function(&mut self, name: &ast::Ident) -> FuncId {
        let id = FuncId(self.program.functions.len() as u32);
        self.program.functions.push(Function {
            name: name.name.clone(),
            sig: Signature {
                ret: Ty::Int,
                params: Vec::new(),
                variadic: false,
                prototyped: false,
            },
            params: Vec::new(),
            param_names: Vec::new(),
            is_static: false,
            is_inline: false,
            noreturn: false,
            inline_hint: None,
            cold: false,
            deprecated: None,
            section: None,
            asm_label: None,
            init_kind: None,
            locals: Vec::new(),
            uses_alloca: false,
            body: None,
            range: name.range,
        });
        self.item_names.insert(name.name.clone());
        self.insert_at_file_scope(&name.name, Entry::Function(id));
        id
    }

    /// Resolves what a call expression calls.
    fn callee(&mut self, callee: &ast::Expr) -> Option<(Callee, Signature, String)> {
        if let ast::ExprKind::Ident(name) = &callee.kind {
            match self.lookup(&name.name) {
                Some(Entry::Function(id)) => {
                    let id = *id;
                    let sig = self.program.function(id).sig.clone();
                    return Some((Callee::Direct(id), sig, name.name.clone()));
                }
                Some(Entry::Object(id)) => {
                    let ty = self.program.object(*id).ty;
                    if !self.types().is_func_pointer(ty) {
                        self.error(
                            callee.range,
                            format!("called object '{}' is not a function", name.name),
                        );
                        return None;
                    }
                }
                Some(Entry::Typedef(_)) => {
                    self.error(
                        callee.range,
                        format!("'{}' names a type and cannot be called", name.name),
                    );
                    return None;
                }
                Some(Entry::Constant { .. }) => {
                    self.error(
                        callee.range,
                        format!("called object '{}' is not a function", name.name),
                    );
                    return None;
                }
                None => {
                    // A name another entry point would have made a keyword is
                    // almost always that keyword rather than a function nobody
                    // declared — `asm("nop")` looks exactly like a call.
                    if let Some(message) = self.gating.newer_keyword(&name.name) {
                        self.error(callee.range, message);
                        return None;
                    }
                    // C89 6.3.2.2: a call to a name nothing declares declares
                    // `extern int name();` — no prototype, so the arguments
                    // get the default argument promotions and the linker is
                    // what resolves it. C99 removed the rule (N636).
                    //
                    // A `__builtin_` name is the one exception. It belongs to
                    // the implementation, so nothing will ever define it and
                    // the implicit declaration would turn a diagnostic this
                    // crate can give into a link error nobody can read.
                    if name.name.starts_with("__builtin_") {
                        self.error(
                            callee.range,
                            format!(
                                "'{}' is not a builtin this crate implements, and a \
                                 '__builtin_' name is never implicitly declared",
                                name.name
                            ),
                        );
                        return None;
                    }
                    if self.gating.implicit_function_declarations() {
                        let id = self.implicit_function(name);
                        let sig = self.program.function(id).sig.clone();
                        return Some((Callee::Direct(id), sig, name.name.clone()));
                    }
                    self.error(
                        callee.range,
                        format!(
                            "implicit declaration of function '{}' is invalid in C99",
                            name.name
                        ),
                    );
                    return None;
                }
            }
        }
        let value = self.expr(callee)?;
        if value.ty.is_error() {
            return None;
        }
        let Some(Ty::Func(func)) = self.pointee(value.ty) else {
            self.error(
                callee.range,
                format!(
                    "called object type '{}' is not a function or function pointer",
                    self.tyname(value.ty)
                ),
            );
            return None;
        };
        let ft = self.types().func_type(func).clone();
        let sig = Signature {
            ret: ft.ret,
            params: ft.params,
            variadic: ft.variadic,
            prototyped: ft.prototyped,
        };
        Some((
            Callee::Indirect(Box::new(value)),
            sig,
            "the callee".to_owned(),
        ))
    }

    fn assign(
        &mut self,
        op: Option<ast::BinaryOp>,
        lhs: &ast::Expr,
        rhs: &ast::Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        let place = self.lvalue_assignable(lhs)?;
        let value = self.expr(rhs)?;
        // The value of an assignment is the value stored, "with the type the
        // left operand would have after lvalue conversion" (C99 6.5.16p3) —
        // which is the unqualified, non-atomic type. Every check below is
        // about that type too; the *place* keeps the `_Atomic`, and that is
        // what makes the store, the read-modify-write and the `++` atomic.
        let ty = self.types().unatomic(place.ty);

        let Some(op) = op else {
            let value = self.convert_for(value, ty, ConvContext::Assign);
            return Some(Expr::new(
                ExprKind::Assign {
                    place,
                    value: Box::new(value),
                },
                ty,
                range,
            ));
        };

        let bin = arith_op(op).expect("every compound assignment operator is arithmetic");

        // `p += n` is pointer arithmetic, not an addition.
        if ty.is_pointer() {
            if !matches!(bin, BinOp::Add | BinOp::Sub) || !value.ty.is_integer() {
                let dummy = Expr::new(ExprKind::Zeroed, ty, place.range);
                self.report_bad_operands(bin, &dummy, &value, range);
                return None;
            }
            self.check_pointee_arithmetic(ty, range)?;
            return Some(Expr::new(
                ExprKind::CompoundAssign {
                    place,
                    op: bin,
                    value: Box::new(value),
                    compute: ty,
                },
                ty,
                range,
            ));
        }

        let integer_only = matches!(
            bin,
            BinOp::Rem | BinOp::BitAnd | BinOp::BitXor | BinOp::BitOr | BinOp::Shl | BinOp::Shr
        );
        let dummy = Expr::new(ExprKind::Int(0), ty, place.range);
        if integer_only {
            self.require_binary_integer(&dummy, &value, bin, range)?;
        } else {
            self.require_binary_arithmetic(&dummy, &value, bin, range)?;
        }

        let (compute, value) = if bin.is_shift() {
            let compute = self.promoted_place(&place);
            let promoted = self.promoted(&value);
            (compute, self.convert(value, promoted))
        } else {
            let compute = Ty::usual_arithmetic(
                self.promoted_place(&place),
                self.promoted(&value),
                &self.target,
            );
            (compute, self.convert(value, compute))
        };

        Some(Expr::new(
            ExprKind::CompoundAssign {
                place,
                op: bin,
                value: Box::new(value),
                compute,
            },
            ty,
            range,
        ))
    }

    fn inc_dec(
        &mut self,
        op: ast::IncDec,
        operand: &ast::Expr,
        postfix: bool,
        range: SourceRange,
    ) -> Option<Expr> {
        let place = self.lvalue_assignable(operand)?;
        let ty = self.types().unatomic(place.ty);
        if ty.is_pointer() {
            self.check_pointee_arithmetic(ty, range)?;
        } else if !ty.is_arithmetic() {
            self.error(
                range,
                format!(
                    "cannot apply '{}' to an operand of type '{}'",
                    op.as_str(),
                    self.tyname(place.ty)
                ),
            );
            return None;
        }
        Some(Expr::new(
            ExprKind::IncDec {
                place,
                dec: op == ast::IncDec::Dec,
                postfix,
            },
            ty,
            range,
        ))
    }

    // -- casts --------------------------------------------------------------

    fn cast_expr(
        &mut self,
        type_name: &ast::TypeName,
        operand: &ast::Expr,
        range: SourceRange,
    ) -> Option<Expr> {
        // A cast produces a *value*, and a value never has an atomic type:
        // `(_Atomic int) x` is a conversion to `int`, which is what C11 makes
        // it too — the result of a cast is not an lvalue, so there is nothing
        // for the qualifier to qualify.
        let target = self.ty_of(&type_name.ty)?;
        let target = self.types().unatomic(target);
        let value = self.expr(operand)?;
        if value.ty.is_error() || target.is_error() {
            return None;
        }
        if target.is_void() {
            return Some(Expr::new(ExprKind::Cast(Box::new(value)), Ty::Void, range));
        }
        if !target.is_scalar() {
            // A cast to the type the operand already has does nothing, and
            // both GCC and Clang accept it silently for a `struct` or `union`
            // — 6.5.4p2's "scalar type" is about a *conversion*, and there is
            // none to make here. C11 6.2.4p8 gives the result temporary
            // lifetime, which is what a `Temporary` place already is;
            // `C11/n1285.c` is a file about exactly that and writes
            // `((struct X)x).a` four times.
            if self.compatible(value.ty, target) {
                return Some(value);
            }
            self.error(
                type_name.range,
                format!(
                    "a cast to '{}' is not allowed; only scalar types can be cast to",
                    self.tyname(target)
                ),
            );
            return None;
        }
        if !value.ty.is_scalar() {
            self.error(
                operand.range,
                format!(
                    "cannot cast an expression of type '{}' to '{}'",
                    self.tyname(value.ty),
                    self.tyname(target)
                ),
            );
            return None;
        }
        if target.is_pointer() && self.is_null_constant(&value) {
            return Some(Expr::new(ExprKind::Zeroed, target, range));
        }
        if target.is_arithmetic() && value.ty.is_arithmetic() {
            // A cast of a bit-field is never a no-op, even to the field's own
            // declared type. `unsigned u : 7` reads back as an `int` — the
            // width-restricted promotions of 6.3.1.1p2 — while
            // `(unsigned int) x.u` converts to a full-width `unsigned int`
            // and takes the arithmetic around it with it. Eliding the cast
            // here would leave a bare load behind for `Sema::promoted` to
            // promote all over again, which is exactly the bug GCC's
            // `bitfld-1` was written for.
            if value.ty == target && self.is_bit_field_load(&value) {
                return Some(Expr::new(ExprKind::Cast(Box::new(value)), target, range));
            }
            let converted = self.convert(value, target);
            // A cast is always explicit in the output, even when it is a
            // no-op, so that the generated code mirrors the source.
            return Some(Expr::new(converted.kind, target, range));
        }
        Some(Expr::new(ExprKind::Cast(Box::new(value)), target, range))
    }

    /// Checks `__builtin_offsetof(T, member)`, which `offsetof` expands to.
    ///
    /// The value is not computed here even though the layout is known: code
    /// generation hands it to Rust's own `core::mem::offset_of!`, so the
    /// answer is the offset the generated `#[repr(C)]` item really has rather
    /// than one this crate worked out separately. The cost is that `offsetof`
    /// is not an integer constant expression here, so it cannot be an array
    /// bound or a `case` label the way C99 allows.
    /// `offsetof(T, member-designator)` — C99 7.17, spelled
    /// `__builtin_offsetof` here because `<stddef.h>` defines the macro in
    /// terms of it.
    ///
    /// The answer is folded to an *integer constant* rather than left as
    /// Rust's `core::mem::offset_of!`, because sema computes the layout itself
    /// (`ir::Field::offset` is where every member's offset already is) and
    /// because C99 6.6 wants a constant here: `struct B { char a[sizeof
    /// (struct A) - offsetof (struct A, a)]; };` is a member declaration, and
    /// `case offsetof(…)` and a file-scope initialiser are the other two
    /// places. That the folded value agrees with the generated Rust item's own
    /// layout is what `tests/aggregates.rs` and the differential corpus in
    /// `tests/bitfield_layout.rs` check, both against `offset_of!`.
    fn offsetof(
        &mut self,
        type_name: &ast::TypeName,
        member: &ast::Ident,
        path: &[ast::Designator],
        range: SourceRange,
    ) -> Option<Expr> {
        let ty = self.ty_of(&type_name.ty)?;
        if ty.is_error() {
            return None;
        }
        if !matches!(ty, Ty::Record(_)) {
            self.error(
                type_name.range,
                format!(
                    "'offsetof' requires a struct or union type, not '{}'",
                    self.tyname(ty)
                ),
            );
            return None;
        }
        let mut offset = self.member_offset(ty, member, type_name.range)?;
        let mut current = self.designated_ty(ty, member)?;
        for step in path {
            match step {
                ast::Designator::Field(name) => {
                    if !matches!(current, Ty::Record(_)) {
                        self.error(
                            name.range,
                            format!(
                                "'.{}' in a member designator needs a struct or union, not '{}'",
                                name.name,
                                self.tyname(current)
                            ),
                        );
                        return None;
                    }
                    offset += self.member_offset(current, name, name.range)?;
                    current = self.designated_ty(current, name)?;
                }
                ast::Designator::Index(index) => {
                    let Some(elem) = self.types().elem(current) else {
                        self.error(
                            index.range,
                            format!(
                                "a subscript in a member designator needs an array, not '{}'",
                                self.tyname(current)
                            ),
                        );
                        return None;
                    };
                    let value = self.expr(index)?;
                    let Some(crate::ir::ConstValue::Int(count)) = self.const_eval(&value) else {
                        self.error(
                            index.range,
                            "a subscript in a member designator must be a constant expression",
                        );
                        return None;
                    };
                    let size = self.size_of(elem).unwrap_or(0);
                    offset += (count.max(0) as u64).saturating_mul(size);
                    current = elem;
                }
                ast::Designator::Range(low, _) => {
                    self.error(
                        low.range,
                        "a range designator has no meaning in a member designator",
                    );
                    return None;
                }
            }
        }
        Some(Expr::int(i128::from(offset), self.size_ty(), range))
    }

    /// The byte offset of `name` within the record type `ty`, reporting the
    /// two things that can be wrong with it.
    fn member_offset(&mut self, ty: Ty, name: &ast::Ident, at: SourceRange) -> Option<u64> {
        let Ty::Record(record) = ty else {
            return None;
        };
        if !self.types().record(record).complete {
            self.error(
                at,
                format!("'offsetof' of the incomplete type '{}'", self.tyname(ty)),
            );
            return None;
        }
        let Some(path) = self.member_path(record, &name.name) else {
            self.error(
                name.range,
                format!("no member named '{}' in '{}'", name.name, self.tyname(ty)),
            );
            return None;
        };
        if self.member_bits(record, &path) {
            self.error(
                name.range,
                format!(
                    "'offsetof' applied to the bit-field '{}', which has no address",
                    name.name
                ),
            );
            return None;
        }
        // An anonymous member contributes its own offset on the way through,
        // which is what makes `offsetof(struct S, x)` work for an `x` declared
        // inside an anonymous `union` (C11 6.7.2.1p13).
        let mut offset = 0;
        let mut current = record;
        for index in &path {
            let field = &self.types().record(current).fields[*index];
            offset += field.offset;
            if let Ty::Record(inner) = field.ty {
                current = inner;
            }
        }
        Some(offset)
    }

    /// The type of the member `name` names in the record type `ty`.
    fn designated_ty(&mut self, ty: Ty, name: &ast::Ident) -> Option<Ty> {
        let Ty::Record(record) = ty else {
            return None;
        };
        let path = self.member_path(record, &name.name)?;
        let mut current = record;
        let mut result = Ty::Error;
        for index in &path {
            let field = &self.types().record(current).fields[*index];
            result = field.ty;
            if let Ty::Record(inner) = field.ty {
                current = inner;
            }
        }
        Some(result)
    }

    /// What `sizeof` or `_Alignof` is being asked about, rejecting the one
    /// operand that has neither: a bit-field.
    ///
    /// The *place* comes back too, when the operand denotes an object at all,
    /// because a variable length array's size belongs to the object rather
    /// than to its type.
    fn operand_place(&mut self, operand: &ast::Expr, what: &str) -> Option<(Option<Place>, Ty)> {
        if !self.is_lvalue_form(operand) {
            return Some((None, self.expr(operand)?.ty));
        }
        let place = self.lvalue(operand)?;
        if self.bit_field_of(&place).is_some() {
            self.error(
                operand.range,
                format!("'{what}' applied to a bit-field, which has no size of its own"),
            );
            return None;
        }
        let ty = place.ty;
        Some((Some(place), ty))
    }

    /// Whether the member a [path](Sema::member_path) reaches is a bit-field.
    fn member_bits(&self, record: crate::ir::RecordId, path: &[usize]) -> bool {
        let field = &self.types().record(record).fields[path[0]];
        match (path.len(), field.ty) {
            (1, _) => field.bits.is_some(),
            (_, Ty::Record(inner)) => self.member_bits(inner, &path[1..]),
            _ => false,
        }
    }

    /// `sizeof expr`, whose operand is not evaluated — except that a variable
    /// length array's size is only known at run time.
    fn sizeof_expr(&mut self, operand: &ast::Expr, range: SourceRange) -> Option<Expr> {
        let (place, ty) = self.operand_place(operand, "sizeof")?;
        // `sizeof a` where `a` is a variable length array is the number of
        // elements the declaration allocated times the element size, read out
        // of the hidden object that declaration left behind.
        if let Some(Place {
            kind: PlaceKind::Object(id),
            ..
        }) = &place
            && let Some(len) = self.vla_lengths.get(id).copied()
        {
            let elem = self
                .types()
                .elem(ty)
                .expect("a variable length array is an array type");
            let size_ty = self.size_ty();
            let count = Expr::new(
                ExprKind::Load(place_of(PlaceKind::Object(len), size_ty, false, range)),
                size_ty,
                range,
            );
            return self.vla_size(count, elem, operand.range, range);
        }
        self.sizeof(ty, operand.range, range)
    }

    /// `count * sizeof(elem)`, as a `size_t` computed at run time.
    fn vla_size(
        &mut self,
        count: Expr,
        elem: Ty,
        operand_range: SourceRange,
        range: SourceRange,
    ) -> Option<Expr> {
        let size_ty = self.size_ty();
        let Some(size) = self.size_of(elem) else {
            self.error(
                operand_range,
                format!(
                    "invalid application of 'sizeof' to an incomplete type '{}'",
                    self.tyname(elem)
                ),
            );
            return None;
        };
        if size == 1 {
            return Some(count);
        }
        Some(Expr::new(
            ExprKind::Binary {
                op: BinOp::Mul,
                lhs: Box::new(count),
                rhs: Box::new(Expr::int(size as i128, size_ty, range)),
            },
            size_ty,
            range,
        ))
    }

    fn sizeof(&mut self, ty: Ty, operand_range: SourceRange, range: SourceRange) -> Option<Expr> {
        if ty.is_error() {
            return None;
        }
        // A variable length array reached from somewhere that does not carry
        // its bound: `sizeof *&a`, or a type name whose bound was evaluated
        // elsewhere. The two forms that *do* know it are handled above.
        if self.types().is_vla(ty) {
            self.error(operand_range, super::VM_UNSUPPORTED);
            return None;
        }
        if ty.is_func() {
            self.error(
                operand_range,
                "invalid application of 'sizeof' to a function type",
            );
            return None;
        }
        if ty.is_va_list() {
            // It has a size, but only the target's ABI knows it.
            self.error(
                operand_range,
                "invalid application of 'sizeof' to 'va_list'",
            );
            return None;
        }
        // GNU gives `void` a size of one, which is what makes `void *`
        // arithmetic — already accepted here — mean anything; ISO C has it as
        // an incomplete type that can never be completed (6.2.5p19).
        if ty.is_void() && self.gnu_leniency() {
            return Some(Expr::int(1, self.size_ty(), range));
        }
        let Some(size) = self.size_of(ty) else {
            let message = format!(
                "invalid application of 'sizeof' to an incomplete type '{}'",
                self.tyname(ty)
            );
            if ty.is_void() {
                let note = self.gnu_note();
                self.diags
                    .push(crate::diag::Diagnostic::error(operand_range, message).with_note(note));
            } else {
                self.error(operand_range, message);
            }
            return None;
        };
        Some(Expr::int(size as i128, self.size_ty(), range))
    }

    // -- conversions --------------------------------------------------------

    /// Inserts the conversion C performs implicitly, folding it away when the
    /// operand is a constant.
    pub(super) fn convert(&mut self, expr: Expr, to: Ty) -> Expr {
        // A *value* never has an atomic type: converting to `_Atomic T` — on
        // assignment, on initialisation, on a `return` — is converting to `T`,
        // and it is the store that is atomic.
        let to = self.types().unatomic(to);
        if expr.ty == to {
            return expr;
        }
        if let Some(folded) = self.fold_conversion(&expr, to) {
            return folded;
        }
        let range = expr.range;
        Expr::new(ExprKind::Cast(Box::new(expr)), to, range)
    }

    fn fold_conversion(&self, expr: &Expr, to: Ty) -> Option<Expr> {
        if matches!(expr.kind, ExprKind::Zeroed) && to.is_pointer() && expr.ty.is_pointer() {
            return Some(Expr::new(ExprKind::Zeroed, to, expr.range));
        }
        let value = match (&expr.kind, to) {
            (ExprKind::Int(v), t) if t.is_integer() => {
                crate::ir::ConstValue::Int(t.wrap(*v, &self.target))
            }
            // An `unsigned __int128` constant is carried as its bit pattern,
            // so the value it converts to is the `u128` those bits spell.
            (ExprKind::Int(v), t) if t.is_floating() && expr.ty == Ty::UInt128 => {
                crate::ir::ConstValue::Float(round_to(*v as u128 as f64, t))
            }
            (ExprKind::Int(v), t) if t.is_floating() => {
                crate::ir::ConstValue::Float(round_to(*v as f64, t))
            }
            (ExprKind::Float(v), t) if t.is_floating() => {
                crate::ir::ConstValue::Float(round_to(*v, t))
            }
            (ExprKind::Float(v), t) if t.is_integer() && !t.is_bool() => {
                let truncated = v.trunc();
                if !truncated.is_finite()
                    || truncated < t.min_value(&self.target) as f64
                    || truncated > t.max_value(&self.target) as f64
                {
                    // Out of range is undefined behaviour in C; leave the cast
                    // in place rather than inventing a value for it.
                    return None;
                }
                crate::ir::ConstValue::Int(truncated as i128)
            }
            (ExprKind::Float(v), Ty::Bool) => crate::ir::ConstValue::Int(i128::from(*v != 0.0)),
            _ => return None,
        };
        Some(self.const_to_expr(value, to, expr.range))
    }

    /// Converts a value for an assignment-like context, reporting the
    /// conversions C does not allow.
    pub(super) fn convert_for(&mut self, expr: Expr, to: Ty, context: ConvContext) -> Expr {
        let to = self.types().unatomic(to);
        if expr.ty == to || expr.ty.is_error() || to.is_error() {
            return expr;
        }
        if to.is_arithmetic() && expr.ty.is_arithmetic() {
            return self.convert(expr, to);
        }
        if to.is_pointer() {
            if self.is_null_constant(&expr) {
                return Expr::new(ExprKind::Zeroed, to, expr.range);
            }
            if expr.ty.is_pointer() && self.pointer_assignable(to, expr.ty) {
                return self.convert(expr, to);
            }
        }
        if to.is_bool() && expr.ty.is_pointer() {
            return self.convert(expr, to);
        }

        let message = context.message(&self.tyname(expr.ty), &self.tyname(to));
        let note = if to.is_pointer() && expr.ty.is_integer() {
            Some(
                "a pointer cannot be made from an integer without a cast; only the \
                 constant 0 is a null pointer",
            )
        } else if to.is_integer() && expr.ty.is_pointer() {
            Some("a pointer cannot be converted to an integer without a cast")
        } else if to.is_pointer() && expr.ty.is_pointer() {
            Some("the pointee types differ; add a cast if the conversion is intended")
        } else {
            None
        };
        let range = expr.range;
        match note {
            Some(note) => self
                .diags
                .push(crate::diag::Diagnostic::error(range, message).with_note(note)),
            None => self.error(range, message),
        }
        self.zero(to, range)
    }

    /// Whether a pointer of type `from` may be assigned to one of type `to`
    /// without a cast.
    fn pointer_assignable(&self, to: Ty, from: Ty) -> bool {
        let to_func = self.types().is_func_pointer(to);
        let from_func = self.types().is_func_pointer(from);
        if to_func || from_func {
            // Standard C keeps function pointers and `void *` apart; POSIX
            // requires the conversion, `dlsym` is built on it, and GCC allows
            // it. `cinrs` follows GCC — the two have the same size on every
            // target it supports, and codegen makes the reinterpretation
            // explicit.
            //
            // `compatible` is what lets `int (*fp)() = g;` through, where `g`
            // is an `int(int)`: a function type with no prototype is
            // compatible with a prototyped one whose parameters are their own
            // promoted forms (6.7.5.3p15), and GCC accepts exactly that pair.
            return self.compatible(to, from)
                || self.types().is_void_pointer(to)
                || self.types().is_void_pointer(from);
        }
        if self.types().is_void_pointer(to) || self.types().is_void_pointer(from) {
            return true;
        }
        if self.types().same_pointee(to, from) {
            return true;
        }
        // C99 6.7.2.2p4 makes an enumerated type compatible with an
        // implementation-defined integer type; GCC and Clang pick `unsigned
        // int` when no enumerator is negative and `int` otherwise, and a
        // pointer to one is then a pointer to the other. c-testsuite `00170`
        // is exactly that.
        let (Some(a), Some(b)) = (self.pointee(to), self.pointee(from)) else {
            return false;
        };
        if matches!((a.is_enum(), b.is_enum()), (true, false) | (false, true))
            && matches!(a, Ty::Int | Ty::UInt | Ty::Enum(_))
            && matches!(b, Ty::Int | Ty::UInt | Ty::Enum(_))
        {
            return true;
        }
        self.differ_only_in_sign(a, b)
    }

    /// Whether two pointee types are the same integer type but for its
    /// signedness — GCC's and Clang's `-Wpointer-sign`.
    ///
    /// `strlen` over an `unsigned char *`, a `long *` argument where the
    /// parameter is an `unsigned long *`, a `char *` buffer handed to a
    /// routine that takes `unsigned char *`: ISO C makes each of these a
    /// constraint violation (6.5.16.1p1, because the unqualified pointee types
    /// are not compatible), and no compiler anybody uses has ever refused one.
    /// GCC 14 and Clang both *warn*, and only `-pedantic-errors` promotes it.
    /// `cinrs` accepts it silently in every entry point, because the amount of
    /// real C that leans on it is not small; `doc/gnu-extensions.md` says so.
    ///
    /// Plain `char` is a type of its own, distinct from both `signed char` and
    /// `unsigned char` whatever the target's signedness, so it counts as
    /// differing in sign from either — which is the wording both compilers use
    /// ("one is of the unique plain 'char' type and the other is not").
    fn differ_only_in_sign(&self, a: Ty, b: Ty) -> bool {
        if a == b || a.is_enum() || b.is_enum() || a.is_bool() || b.is_bool() {
            return false;
        }
        if !(a.is_integer() && b.is_integer()) {
            return false;
        }
        a.bits(&self.target) == b.bits(&self.target)
            && (a.is_signed(&self.target) != b.is_signed(&self.target)
                || matches!(
                    (a, b),
                    (Ty::Char, Ty::SChar | Ty::UChar) | (Ty::SChar | Ty::UChar, Ty::Char)
                ))
    }

    /// Whether an expression is C's null pointer constant.
    pub(super) fn is_null_constant(&self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Int(v) => *v == 0 && expr.ty.is_integer(),
            ExprKind::Zeroed => expr.ty.is_pointer(),
            ExprKind::Cast(inner) => expr.ty.is_integer() && self.is_null_constant(inner),
            _ => false,
        }
    }

    /// Applies the usual arithmetic conversions to a pair of operands.
    pub(super) fn balance(&mut self, lhs: Expr, rhs: Expr) -> (Expr, Expr, Ty) {
        // The operands are promoted first, which is where a bit-field's width
        // has its say; `usual_arithmetic` promotes again, and the promotions
        // are idempotent, so passing the promoted types through is exact.
        let common = Ty::usual_arithmetic(self.promoted(&lhs), self.promoted(&rhs), &self.target);
        let lhs = self.convert(lhs, common);
        let rhs = self.convert(rhs, common);
        (lhs, rhs, common)
    }

    /// Checks a controlling expression: anything C can compare against zero.
    pub(super) fn condition(&mut self, expr: &ast::Expr) -> Option<Expr> {
        let value = self.expr(expr)?;
        if value.ty.is_error() {
            return None;
        }
        if !value.ty.is_scalar() {
            self.error(
                expr.range,
                format!(
                    "value of type '{}' is not contextually convertible to a condition",
                    self.tyname(value.ty)
                ),
            );
            return None;
        }
        Some(value)
    }

    // -- literals -----------------------------------------------------------

    /// Types an integer constant per C99 6.4.4.1.
    fn int_literal(&mut self, lit: &IntLit, range: SourceRange) -> (i128, Ty) {
        use Ty::*;
        let decimal = lit.base == NumBase::Decimal;
        let candidates: &[Ty] = match (lit.unsigned, lit.long, decimal) {
            (false, LongKind::None, true) => &[Int, Long, LongLong],
            (false, LongKind::None, false) => &[Int, UInt, Long, ULong, LongLong, ULongLong],
            (false, LongKind::Long, true) => &[Long, LongLong],
            (false, LongKind::Long, false) => &[Long, ULong, LongLong, ULongLong],
            (false, LongKind::LongLong, true) => &[LongLong],
            (false, LongKind::LongLong, false) => &[LongLong, ULongLong],
            (true, LongKind::None, _) => &[UInt, ULong, ULongLong],
            (true, LongKind::Long, _) => &[ULong, ULongLong],
            (true, LongKind::LongLong, _) => &[ULongLong],
        };
        let value = i128::try_from(lit.value).unwrap_or(i128::MAX);
        for ty in candidates {
            if ty.can_represent(value, &self.target) {
                return (value, *ty);
            }
        }
        self.error(
            range,
            format!(
                "integer constant '{}' is too large for any integer type",
                lit.text
            ),
        );
        let ty = *candidates.last().expect("every list is non-empty");
        (ty.wrap(value, &self.target), ty)
    }

    /// The value and type of a character constant.
    ///
    /// `'x'` is an `int` — that much is C, not a choice — and each prefix
    /// names the type of one element of the corresponding string literal:
    /// `L'x'` is a `wchar_t`, `u'x'` a `char16_t`, `U'x'` a `char32_t`, and
    /// C23's `u8'x'` a `char8_t`, which is an `unsigned char`.
    fn char_literal(&self, lit: &CharLit) -> (i128, Ty) {
        let value = i128::from(lit.value);
        let ty = match lit.kind {
            StrKind::Narrow => Ty::Int,
            StrKind::Utf8 => Ty::UChar,
            StrKind::Utf16 => Ty::char16_ty(),
            StrKind::Utf32 => Ty::char32_ty(),
            StrKind::Wide => Ty::wchar_ty(&self.target),
        };
        // The lexer hands over the raw execution-character value; whether the
        // top bit means "negative" is up to the target's plain `char`.
        if lit.kind == StrKind::Narrow && self.target.char_signed && (128..=255).contains(&value) {
            (value - 256, ty)
        } else {
            (value, ty)
        }
    }
}

/// Whether a place ultimately addresses a temporary rather than an object.
fn rooted_in_temporary(place: &Place) -> bool {
    match &place.kind {
        PlaceKind::Temporary(_) => true,
        PlaceKind::Field { base, .. } => rooted_in_temporary(base),
        _ => false,
    }
}

fn float_literal(lit: &FloatLit) -> (f64, Ty) {
    match lit.suffix {
        FloatSuffix::Float => (lit.value as f32 as f64, Ty::Float),
        // `long double` is mapped onto `double`; see `Ty`.
        FloatSuffix::None | FloatSuffix::LongDouble => (lit.value, Ty::Double),
    }
}
