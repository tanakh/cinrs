//! Expressions: type checking, the implicit conversions, and lvalues.

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{
    BinOp, Callee, CmpOp, Expr, ExprKind, LogicalOp, Place, PlaceKind, Signature, StrData, StrId,
    Ty, UNREACHABLE_BUILTIN,
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

impl Sema {
    // -- expressions --------------------------------------------------------

    pub(super) fn expr(&mut self, expr: &ast::Expr) -> Option<Expr> {
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
            ast::ExprKind::Char(lit) => Some(Expr::int(self.char_literal(lit), Ty::Int, range)),
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
                }
                self.call(callee, args, range)
            }
            ast::ExprKind::VaArg { ap, ty } => self.va_arg(ap, ty, range),
            ast::ExprKind::OffsetOf { ty, member } => self.offsetof(ty, member, range),
            ast::ExprKind::Member { .. } | ast::ExprKind::Index { .. } => {
                let place = self.lvalue(expr)?;
                Some(self.load_or_decay(place, range))
            }
            ast::ExprKind::PostIncDec { op, operand } => self.inc_dec(*op, operand, true, range),
            ast::ExprKind::PreIncDec { op, operand } => self.inc_dec(*op, operand, false, range),
            ast::ExprKind::Cast { ty, expr: operand } => self.cast_expr(ty, operand, range),
            ast::ExprKind::SizeofExpr(operand) => {
                let ty = self.operand_ty(operand, "sizeof")?;
                self.sizeof(ty, operand.range, range)
            }
            ast::ExprKind::SizeofType(ty) => {
                let target = self.ty_of(&ty.ty)?;
                self.sizeof(target, ty.range, range)
            }
            ast::ExprKind::AlignofExpr(operand) => {
                let ty = self.operand_ty(operand, "_Alignof")?;
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
                // type name, which is exactly `Ty` equality here — `Ty` is
                // interned, and it carries no top-level qualifiers.
                Some(Expr::int(i128::from(lhs == rhs), Ty::Int, range))
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
        let Some(layout) = self.types().size_align(ty, &self.target) else {
            self.error(
                operand_range,
                format!(
                    "invalid application of '_Alignof' to an incomplete type '{}'",
                    self.tyname(ty)
                ),
            );
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
            if let Some((_, _, previous)) = seen
                .iter()
                .find(|(seen, seen_quals, _)| *seen == assoc_ty && *seen_quals == quals)
            {
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
            if assoc_ty == ty && !quals.any() && chosen.is_none() {
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
        let func = self
            .program
            .types
            .func(sig.ret, sig.params.clone(), sig.variadic);
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
        let ty = place.ty;
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
        let wide = lit.kind == StrKind::Wide;
        let id = StrId(self.program.strings.len() as u32);
        self.program.strings.push(StrData {
            values: lit.values.clone(),
            wide,
        });
        let elem = if wide { Ty::wchar_ty() } else { Ty::Char };
        let len = lit.values.len() as u64 + 1;
        let ty = self.program.types.array(elem, len, false);
        place_of(PlaceKind::Str(id), ty, false, range)
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
                let value = self.convert(value, promoted);
                if op == ast::UnaryOp::Plus {
                    return Some(Expr::new(value.kind, promoted, range));
                }
                // Folding `-` into the literal is what makes a negative
                // constant read like one in the generated code.
                if let ExprKind::Int(v) = &value.kind {
                    return Some(Expr::int(promoted.wrap(-*v, &self.target), promoted, range));
                }
                if let ExprKind::Float(v) = &value.kind {
                    return Some(Expr::new(ExprKind::Float(-*v), promoted, range));
                }
                Some(Expr::new(ExprKind::Neg(Box::new(value)), promoted, range))
            }
            ast::UnaryOp::BitNot => {
                let value = self.expr(operand)?;
                self.require_integer(&value, "~", operand.range)?;
                let promoted = self.promoted(&value);
                let value = self.convert(value, promoted);
                Some(Expr::new(
                    ExprKind::BitNot(Box::new(value)),
                    promoted,
                    range,
                ))
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
        use ast::BinaryOp as B;
        if matches!(op, B::LogAnd | B::LogOr) {
            let lhs_value = self.expr(lhs)?;
            let rhs_value = self.expr(rhs)?;
            self.require_scalar(&lhs_value, op.as_str(), lhs.range)?;
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

        let lhs_value = self.expr(lhs)?;
        let rhs_value = self.expr(rhs)?;

        if let Some(cmp) = compare_op(op) {
            if lhs_value.ty.is_pointer() || rhs_value.ty.is_pointer() {
                return self.pointer_compare(cmp, lhs_value, rhs_value, range);
            }
            self.require_arithmetic(&lhs_value, op.as_str(), lhs.range)?;
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
            // the type of the promoted left operand.
            let lhs_ty = self.promoted(&lhs_value);
            let rhs_ty = self.promoted(&rhs_value);
            let lhs_value = self.convert(lhs_value, lhs_ty);
            let rhs_value = self.convert(rhs_value, rhs_ty);
            return Some(Expr::new(
                ExprKind::Binary {
                    op: bin,
                    lhs: Box::new(lhs_value),
                    rhs: Box::new(rhs_value),
                },
                lhs_ty,
                range,
            ));
        }

        let (lhs_value, rhs_value, common) = self.balance(lhs_value, rhs_value);
        Some(Expr::new(
            ExprKind::Binary {
                op: bin,
                lhs: Box::new(lhs_value),
                rhs: Box::new(rhs_value),
            },
            common,
            range,
        ))
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
            if lhs.ty != rhs.ty
                && !matches!(lhs.kind, ExprKind::Zeroed)
                && !matches!(rhs.kind, ExprKind::Zeroed)
            {
                self.error(
                    range,
                    format!(
                        "comparison of distinct function pointer types '{}' and '{}'",
                        self.tyname(lhs.ty),
                        self.tyname(rhs.ty)
                    ),
                );
                return None;
            }
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
        if then_value.ty.is_void() && else_value.ty.is_void() {
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
        if self.types().is_void_pointer(lhs.ty) || self.types().is_void_pointer(rhs.ty) {
            let konst =
                self.types().points_to_const(lhs.ty) || self.types().points_to_const(rhs.ty);
            return Some(self.ptr_to(Ty::Void, konst));
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
                    // argument promotions: `float` widens to `double` and the
                    // small integer types to `int`.
                    let promoted = self.promoted_argument(&value);
                    let value = self.convert(value, promoted);
                    values.push(value);
                }
            }
        }

        let too_few = args.len() < sig.params.len();
        let too_many = args.len() > sig.params.len() && !sig.variadic;
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
                    let message = self.gating.newer_keyword(&name.name).unwrap_or_else(|| {
                        format!(
                            "implicit declaration of function '{}' is invalid in C99",
                            name.name
                        )
                    });
                    self.error(callee.range, message);
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
        let ty = place.ty;

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
        if place.ty.is_pointer() {
            self.check_pointee_arithmetic(place.ty, range)?;
        } else if !place.ty.is_arithmetic() {
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
        let ty = place.ty;
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
        let target = self.ty_of(&type_name.ty)?;
        let value = self.expr(operand)?;
        if value.ty.is_error() || target.is_error() {
            return None;
        }
        if target.is_void() {
            return Some(Expr::new(ExprKind::Cast(Box::new(value)), Ty::Void, range));
        }
        if !target.is_scalar() {
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
    fn offsetof(
        &mut self,
        type_name: &ast::TypeName,
        member: &ast::Ident,
        range: SourceRange,
    ) -> Option<Expr> {
        let ty = self.ty_of(&type_name.ty)?;
        if ty.is_error() {
            return None;
        }
        let Ty::Record(record) = ty else {
            self.error(
                type_name.range,
                format!(
                    "'offsetof' requires a struct or union type, not '{}'",
                    self.tyname(ty)
                ),
            );
            return None;
        };
        if !self.types().record(record).complete {
            self.error(
                type_name.range,
                format!("'offsetof' of the incomplete type '{}'", self.tyname(ty)),
            );
            return None;
        }
        let Some(path) = self.member_path(record, &member.name) else {
            self.error(
                member.range,
                format!("no member named '{}' in '{}'", member.name, self.tyname(ty)),
            );
            return None;
        };
        if self.member_bits(record, &path) {
            self.error(
                member.range,
                format!(
                    "'offsetof' applied to the bit-field '{}', which has no address",
                    member.name
                ),
            );
            return None;
        }
        Some(Expr::new(
            ExprKind::OffsetOf { record, path },
            self.size_ty(),
            range,
        ))
    }

    /// The type `sizeof` or `_Alignof` is being asked about, rejecting the one
    /// operand that has neither: a bit-field.
    fn operand_ty(&mut self, operand: &ast::Expr, what: &str) -> Option<Ty> {
        if !self.is_lvalue_form(operand) {
            return Some(self.expr(operand)?.ty);
        }
        let place = self.lvalue(operand)?;
        if self.bit_field_of(&place).is_some() {
            self.error(
                operand.range,
                format!("'{what}' applied to a bit-field, which has no size of its own"),
            );
            return None;
        }
        Some(place.ty)
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

    fn sizeof(&mut self, ty: Ty, operand_range: SourceRange, range: SourceRange) -> Option<Expr> {
        if ty.is_error() {
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
        let Some(size) = self.size_of(ty) else {
            self.error(
                operand_range,
                format!(
                    "invalid application of 'sizeof' to an incomplete type '{}'",
                    self.tyname(ty)
                ),
            );
            return None;
        };
        Some(Expr::int(size as i128, self.size_ty(), range))
    }

    // -- conversions --------------------------------------------------------

    /// Inserts the conversion C performs implicitly, folding it away when the
    /// operand is a constant.
    pub(super) fn convert(&mut self, expr: Expr, to: Ty) -> Expr {
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
            return to == from
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
        matches!((a.is_enum(), b.is_enum()), (true, false) | (false, true))
            && matches!(a, Ty::Int | Ty::UInt | Ty::Enum(_))
            && matches!(b, Ty::Int | Ty::UInt | Ty::Enum(_))
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

    /// The value of a character constant, which has type `int` in C.
    fn char_literal(&self, lit: &CharLit) -> i128 {
        let value = i128::from(lit.value);
        // The lexer hands over the raw execution-character value; whether the
        // top bit means "negative" is up to the target's plain `char`.
        if !lit.wide && self.target.char_signed && (128..=255).contains(&value) {
            value - 256
        } else {
            value
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
