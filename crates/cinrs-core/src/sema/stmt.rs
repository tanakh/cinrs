//! Statements: the lowering of `switch` into fallthrough groups, and the
//! decision between the structured and the [CFG](crate::cfg) form.

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{
    self, BreakTarget, ConstValue, Expr, ExprKind, LabelId, LoopId, ObjectId, Place, PlaceKind,
    Stmt, SwitchId, Ty,
};

use super::{Breakable, ConvContext, Label, Sema, SwitchState, render_case_value};

impl Sema {
    pub(super) fn block(&mut self, block: &ast::Block) -> Vec<Stmt> {
        self.push_scope();
        let out = self.block_items(&block.items);
        self.pop_scope();
        out
    }

    pub(super) fn block_items(&mut self, items: &[ast::BlockItem]) -> Vec<Stmt> {
        // A compound literal written anywhere in this block — including in a
        // statement of it that is not a block of its own, such as the
        // controlling expression of an `if` — needs an object that lives as
        // long as the block does. The list is saved and restored so that a
        // nested block claims only its own.
        let enclosing = std::mem::take(&mut self.compound_literals);
        let mut out = Vec::new();
        for item in items {
            match item {
                ast::BlockItem::Decl(decl) => {
                    if decl.declarators.is_empty() {
                        // A tag definition with nothing declared still defines
                        // the tag.
                        let _ = self.ty_of(&decl.specifiers.base);
                        continue;
                    }
                    for declarator in &decl.declarators {
                        out.extend(self.declarator(decl, declarator, false));
                    }
                }
                ast::BlockItem::Stmt(stmt) => out.push(self.stmt(stmt)),
                ast::BlockItem::StaticAssert(assert) => self.static_assert(assert),
            }
        }
        let mine = std::mem::replace(&mut self.compound_literals, enclosing);
        if mine.is_empty() {
            return out;
        }
        // The definitions go at the head of the block, zero-initialised; the
        // value each literal was written with is stored where it was written.
        let mut prologue = Vec::with_capacity(mine.len() + out.len());
        for object in mine {
            let info = self.program.object(object);
            let (ty, range) = (info.ty, info.range);
            let init = self.zero(ty, range);
            prologue.push(Stmt::Let {
                object,
                init,
                explicit: false,
            });
        }
        prologue.append(&mut out);
        prologue
    }

    /// GNU's statement expression, `({ …; e; })`.
    ///
    /// Its value and type are the last *expression statement*'s, and `void`
    /// when the block ends with anything else. Everything a block may hold is
    /// allowed in it, including declarations — which is what makes the
    /// kernel's `max()` evaluate its operands once.
    ///
    /// In the structured lowering it becomes a Rust block expression, where
    /// `break`, `continue` and `return` all still mean what C said. A function
    /// lowered through a [control-flow graph](crate::cfg) has no Rust loop to
    /// leave, so a jump out of the statement expression is refused there.
    pub(super) fn stmt_expr(&mut self, block: &ast::Block, range: SourceRange) -> Option<Expr> {
        self.check_stmt_expr_jumps(block);
        self.push_scope();
        // The value is the last expression statement, which is checked apart
        // from the rest so that its type survives.
        let (items, tail) = match block.items.split_last() {
            Some((ast::BlockItem::Stmt(last), rest)) => match &last.kind {
                ast::StmtKind::Expr(Some(expr)) => (rest, Some(expr)),
                _ => (&block.items[..], None),
            },
            _ => (&block.items[..], None),
        };
        let mut stmts = self.block_items(items);
        let value = tail.and_then(|expr| self.expr(expr));
        // An expression the block ended with that did not check out leaves the
        // statement expression with no value, and one diagnostic has been
        // reported already.
        if tail.is_some() && value.is_none() {
            self.pop_scope();
            return None;
        }
        // A hoisted compound literal belongs to this block, and `block_items`
        // has already put its definition at the head of `stmts`.
        if let Some(value) = &value
            && value.ty.is_va_list()
        {
            self.error(range, super::VA_LIST_PLACEMENT);
            self.pop_scope();
            return None;
        }
        self.pop_scope();
        let ty = value.as_ref().map_or(Ty::Void, |v| v.ty);
        if ty.is_record() && !self.types().is_complete(ty) {
            stmts.clear();
        }
        Some(Expr::new(
            ExprKind::StmtExpr {
                stmts,
                value: value.map(Box::new),
            },
            ty,
            range,
        ))
    }

    /// Refuses the jumps a statement expression may not make.
    ///
    /// A label — and therefore a `goto` — inside one is refused in every mode:
    /// the decision to lower a function through a
    /// [control-flow graph](crate::cfg) is made from the *statements* of its
    /// body, so a jump buried in an expression would be silently dropped
    /// rather than lowered. `break` and `continue` that leave the statement
    /// expression are fine in the structured mode, where a Rust block
    /// expression is exactly what a statement expression is, and refused in
    /// CFG mode, where there is no loop left to leave.
    fn check_stmt_expr_jumps(&mut self, block: &ast::Block) {
        let mut bad: Vec<(SourceRange, Escape)> = Vec::new();
        collect_escaping_jumps(&block.items, 0, &mut bad);
        let cfg_mode = self.cfg_mode;
        for (range, escape) in bad {
            let message = match escape {
                Escape::Label(what) => format!(
                    "{what} cannot appear inside a statement expression; nothing outside the \
                     expression can jump to it"
                ),
                Escape::Leaves(what) if cfg_mode => format!(
                    "{what} inside a statement expression is not supported in a function that \
                     also uses 'goto': the function is lowered into a state machine, and there \
                     is no enclosing loop left to leave"
                ),
                Escape::Leaves(_) => continue,
            };
            self.error(range, message);
        }
    }

    pub(super) fn stmt(&mut self, stmt: &ast::Stmt) -> Stmt {
        match &stmt.kind {
            ast::StmtKind::Labeled { label, body } => {
                let body = Box::new(self.stmt(body));
                match self.labels.get(&label.name) {
                    // Outside CFG mode nothing can jump to a label, so it is
                    // only the statement under it that matters. The range says
                    // whether this is the occurrence that *defined* the label:
                    // a repeat has been reported already, and giving the graph
                    // two entries into one block would only confuse it.
                    Some(entry) if self.cfg_mode && entry.range == label.range => Stmt::Label {
                        id: entry.id,
                        body,
                        range: label.range,
                    },
                    _ => *body,
                }
            }
            ast::StmtKind::Case { value, upper, body } => {
                self.case_label(Some((value, upper.as_ref())), body, stmt.range)
            }
            ast::StmtKind::Default { body } => self.case_label(None, body, stmt.range),
            ast::StmtKind::Compound(block) => Stmt::Block(self.block(block)),
            ast::StmtKind::Expr(None) => Stmt::Nop,
            ast::StmtKind::Expr(Some(expr)) => match self.expr(expr) {
                Some(expr) => Stmt::Expr(expr),
                None => Stmt::Nop,
            },
            ast::StmtKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond = self.condition(cond);
                let then_branch = Box::new(self.stmt(then_branch));
                let else_branch = else_branch.as_ref().map(|s| Box::new(self.stmt(s)));
                match cond {
                    Some(cond) => Stmt::If {
                        cond,
                        then_branch,
                        else_branch,
                    },
                    None => Stmt::Nop,
                }
            }
            ast::StmtKind::While { cond, body } => {
                let id = self.new_loop();
                let cond = self.condition(cond);
                self.breakables.push(Breakable::Loop(id));
                let body = Box::new(self.stmt(body));
                self.breakables.pop();
                match cond {
                    Some(cond) => Stmt::While {
                        id,
                        cond,
                        body,
                        range: stmt.range,
                    },
                    None => Stmt::Nop,
                }
            }
            ast::StmtKind::DoWhile { body, cond } => {
                let id = self.new_loop();
                self.breakables.push(Breakable::Loop(id));
                let body = Box::new(self.stmt(body));
                self.breakables.pop();
                let cond = self.condition(cond);
                match cond {
                    Some(cond) => Stmt::DoWhile {
                        id,
                        body,
                        cond,
                        range: stmt.range,
                    },
                    None => Stmt::Nop,
                }
            }
            ast::StmtKind::For {
                init,
                cond,
                step,
                body,
            } => {
                let id = self.new_loop();
                // C99 scopes a declaration in the init clause to the loop.
                self.push_scope();
                let init = match init {
                    ast::ForInit::None => Vec::new(),
                    ast::ForInit::Expr(expr) => match self.expr(expr) {
                        Some(expr) => vec![Stmt::Expr(expr)],
                        None => Vec::new(),
                    },
                    ast::ForInit::Decl(decl) => {
                        let mut out = Vec::new();
                        for declarator in &decl.declarators {
                            out.extend(self.declarator(decl, declarator, false));
                        }
                        out
                    }
                };
                let cond = cond.as_ref().and_then(|c| self.condition(c));
                let step = step.as_ref().and_then(|s| self.expr(s));
                self.breakables.push(Breakable::Loop(id));
                let body = Box::new(self.stmt(body));
                self.breakables.pop();
                self.pop_scope();
                Stmt::For {
                    id,
                    init,
                    cond,
                    step,
                    body,
                    range: stmt.range,
                }
            }
            ast::StmtKind::Switch { cond, body } => self.switch(cond, body, stmt.range),
            ast::StmtKind::Goto(label) => match self.labels.get(&label.name) {
                Some(entry) => Stmt::Goto {
                    id: entry.id,
                    range: stmt.range,
                },
                None => {
                    self.error(
                        label.range,
                        format!("use of undeclared label '{}'", label.name),
                    );
                    Stmt::Nop
                }
            },
            ast::StmtKind::Break => {
                let target = match self.breakables.last() {
                    Some(Breakable::Loop(id)) => BreakTarget::Loop(*id),
                    Some(Breakable::Switch(id)) => BreakTarget::Switch(*id),
                    None => {
                        self.error(
                            stmt.range,
                            "'break' statement not in a loop or 'switch' statement",
                        );
                        return Stmt::Nop;
                    }
                };
                Stmt::Break {
                    target,
                    range: stmt.range,
                }
            }
            ast::StmtKind::Continue => {
                let target = self.breakables.iter().rev().find_map(|b| match b {
                    Breakable::Loop(id) => Some(*id),
                    Breakable::Switch(_) => None,
                });
                match target {
                    Some(id) => Stmt::Continue {
                        id,
                        range: stmt.range,
                    },
                    None => {
                        self.error(stmt.range, "'continue' statement not in a loop statement");
                        Stmt::Nop
                    }
                }
            }
            ast::StmtKind::Return(value) => self.return_stmt(value.as_ref(), stmt.range),
            ast::StmtKind::Error => Stmt::Nop,
        }
    }

    fn return_stmt(&mut self, value: Option<&ast::Expr>, range: SourceRange) -> Stmt {
        let ret = self.ret_ty;
        let value = match value {
            None => {
                // Falling out of a value-returning function is legal C as long
                // as the caller ignores the result; Rust needs a value, so it
                // gets a zero.
                (!ret.is_void()).then(|| self.zero(ret, range))
            }
            Some(expr) => match self.expr(expr) {
                None => (!ret.is_void()).then(|| self.zero(ret, range)),
                Some(_) if ret.is_void() => {
                    self.error(
                        expr.range,
                        format!(
                            "void function '{}' should not return a value",
                            self.func_name
                        ),
                    );
                    None
                }
                Some(value) => Some(self.convert_for(value, ret, ConvContext::Return)),
            },
        };
        Stmt::Return { value, range }
    }

    fn new_loop(&mut self) -> LoopId {
        let id = LoopId(self.next_loop);
        self.next_loop += 1;
        id
    }

    // -- labels -------------------------------------------------------------

    /// Collects the labels of a function before its body is checked.
    ///
    /// C gives labels function scope and a namespace of their own, which is
    /// what makes `goto` able to jump forwards; collecting them up front is
    /// what lets the jump resolve in one pass.
    pub(super) fn collect_labels(&mut self, block: &ast::Block) {
        self.labels.clear();
        for item in &block.items {
            if let ast::BlockItem::Stmt(stmt) = item {
                self.collect_labels_in(stmt);
            }
        }
    }

    fn collect_labels_in(&mut self, stmt: &ast::Stmt) {
        match &stmt.kind {
            ast::StmtKind::Labeled { label, body } => {
                match self.labels.get(&label.name) {
                    Some(previous) => {
                        let previous = previous.range;
                        self.error_note(
                            label.range,
                            format!("redefinition of label '{}'", label.name),
                            previous,
                            format!("previous definition of label '{}' is", label.name),
                        );
                    }
                    None => {
                        let id = LabelId(self.next_label);
                        self.next_label += 1;
                        self.labels.insert(
                            label.name.clone(),
                            Label {
                                id,
                                range: label.range,
                            },
                        );
                    }
                }
                self.collect_labels_in(body);
            }
            ast::StmtKind::Case { body, .. }
            | ast::StmtKind::Default { body }
            | ast::StmtKind::Switch { body, .. }
            | ast::StmtKind::While { body, .. }
            | ast::StmtKind::DoWhile { body, .. }
            | ast::StmtKind::For { body, .. } => self.collect_labels_in(body),
            ast::StmtKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.collect_labels_in(then_branch);
                if let Some(branch) = else_branch {
                    self.collect_labels_in(branch);
                }
            }
            ast::StmtKind::Compound(block) => {
                for item in &block.items {
                    if let ast::BlockItem::Stmt(stmt) = item {
                        self.collect_labels_in(stmt);
                    }
                }
            }
            _ => {}
        }
    }

    // -- switch -------------------------------------------------------------

    /// Checks the controlling expression of a `switch`, giving it the type the
    /// labels are converted to.
    fn scrutinee(&mut self, cond: &ast::Expr) -> Option<Expr> {
        let scrutinee = self.expr(cond)?;
        if !scrutinee.ty.is_integer() {
            self.error(
                cond.range,
                format!(
                    "statement requires expression of integer type ('{}' invalid)",
                    self.tyname(scrutinee.ty)
                ),
            );
            return None;
        }
        let promoted = self.promoted(&scrutinee);
        Some(self.convert(scrutinee, promoted))
    }

    fn new_switch(&mut self) -> SwitchId {
        let id = SwitchId(self.next_switch);
        self.next_switch += 1;
        id
    }

    /// Lowers a `switch` whose body stays a statement tree, for CFG mode.
    ///
    /// The labels are checked where they are found — see [`Sema::case_label`]
    /// — rather than by splitting the body, which is what lets one sit inside
    /// a loop the `switch` wraps.
    fn switch_tree(&mut self, cond: &ast::Expr, body: &ast::Stmt, range: SourceRange) -> Stmt {
        let Some(scrutinee) = self.scrutinee(cond) else {
            return Stmt::Nop;
        };
        let id = self.new_switch();
        self.breakables.push(Breakable::Switch(id));
        self.switch_stack.push(SwitchState {
            id,
            ty: scrutinee.ty,
            seen: Vec::new(),
            default: None,
        });
        self.push_scope();
        let body = self.stmt(body);
        self.pop_scope();
        self.switch_stack.pop();
        self.breakables.pop();
        Stmt::SwitchTree(Box::new(ir::SwitchTree {
            id,
            scrutinee,
            body: Box::new(body),
            range,
        }))
    }

    /// Checks a `case` or `default` label that stays where it was written.
    fn case_label(
        &mut self,
        value: Option<(&ast::Expr, Option<&ast::Expr>)>,
        body: &ast::Stmt,
        range: SourceRange,
    ) -> Stmt {
        let Some(state) = self.switch_stack.last() else {
            self.error(
                range,
                "a 'case' or 'default' label must appear inside a 'switch' statement",
            );
            return self.stmt(body);
        };
        let (switch, ty) = (state.id, state.ty);
        let value = match value {
            Some((expr, upper)) => {
                let Some(v) = self.case_range(expr, upper, ty, range) else {
                    return self.stmt(body);
                };
                let state = self.switch_stack.last_mut().expect("checked above");
                let clash = state
                    .seen
                    .iter()
                    .find(|(seen, _)| seen.overlaps(v))
                    .map(|(_, at)| *at);
                if let Some(previous) = clash {
                    let rendered = render_case_range(v, ty, &self.target);
                    self.error_note(
                        range,
                        format!("duplicate case value '{rendered}'"),
                        previous,
                        "previous case is",
                    );
                    return self.stmt(body);
                }
                let state = self.switch_stack.last_mut().expect("checked above");
                state.seen.push((v, range));
                Some(v)
            }
            None => {
                let state = self.switch_stack.last_mut().expect("checked above");
                let previous = state.default;
                if previous.is_none() {
                    state.default = Some(range);
                }
                if let Some(previous) = previous {
                    // The first `default` keeps the label; a second one is an
                    // error and is left out of the graph.
                    self.error_note(
                        range,
                        "multiple 'default' labels in one 'switch'",
                        previous,
                        "previous 'default' is",
                    );
                    return self.stmt(body);
                }
                None
            }
        };
        let body = Box::new(self.stmt(body));
        Stmt::Case {
            switch,
            value,
            body,
            range,
        }
    }

    /// Lowers a `switch` into the ordered groups its labels delimit.
    fn switch(&mut self, cond: &ast::Expr, body: &ast::Stmt, range: SourceRange) -> Stmt {
        if self.cfg_mode {
            return self.switch_tree(cond, body, range);
        }
        let Some(scrutinee) = self.scrutinee(cond) else {
            return Stmt::Nop;
        };
        let promoted = scrutinee.ty;
        let id = self.new_switch();

        // The body of a `switch` need not be a compound statement, though it
        // is useless when it is not: without labels nothing in it can run.
        let owned;
        let items: &[ast::BlockItem] = match &body.kind {
            ast::StmtKind::Compound(block) => &block.items,
            _ => {
                owned = [ast::BlockItem::Stmt(body.clone())];
                &owned
            }
        };

        self.breakables.push(Breakable::Switch(id));
        self.push_scope();

        let mut hoisted = Vec::new();
        let mut prelude = Vec::new();
        let mut groups: Vec<ir::SwitchGroup> = Vec::new();
        let mut default_group: Option<usize> = None;
        let mut default_range: Option<SourceRange> = None;
        let mut seen: Vec<(ir::CaseRange, SourceRange)> = Vec::new();

        for item in items {
            match item {
                ast::BlockItem::StaticAssert(assert) => self.static_assert(assert),
                ast::BlockItem::Decl(decl) => {
                    let stmts = self.hoisted_decl(decl, &mut hoisted);
                    match groups.last_mut() {
                        Some(group) => group.body.extend(stmts),
                        None => prelude.extend(stmts),
                    }
                }
                ast::BlockItem::Stmt(stmt) => {
                    let mut current = stmt;
                    #[allow(clippy::type_complexity)]
                    let mut labels: Vec<(
                        Option<(&ast::Expr, Option<&ast::Expr>)>,
                        SourceRange,
                    )> = Vec::new();
                    loop {
                        match &current.kind {
                            ast::StmtKind::Case { value, upper, body } => {
                                labels.push((Some((value, upper.as_ref())), current.range));
                                current = body;
                            }
                            ast::StmtKind::Default { body } => {
                                labels.push((None, current.range));
                                current = body;
                            }
                            // `l: case 1:` — nothing can jump to the label in
                            // this mode, and peeling it keeps the `case` under
                            // it a label of this `switch`.
                            ast::StmtKind::Labeled { body, .. } => current = body,
                            _ => break,
                        }
                    }
                    if !labels.is_empty() {
                        groups.push(ir::SwitchGroup {
                            values: Vec::new(),
                            body: Vec::new(),
                        });
                        let index = groups.len() - 1;
                        for (value, range) in labels {
                            match value {
                                Some((expr, upper)) => {
                                    if let Some(v) = self.case_range(expr, upper, promoted, range) {
                                        let clash = seen
                                            .iter()
                                            .find(|(seen, _)| seen.overlaps(v))
                                            .map(|(_, at)| *at);
                                        match clash {
                                            Some(previous) => self.error_note(
                                                range,
                                                format!(
                                                    "duplicate case value '{}'",
                                                    render_case_range(v, promoted, &self.target)
                                                ),
                                                previous,
                                                "previous case is",
                                            ),
                                            None => {
                                                seen.push((v, range));
                                                groups[index].values.push(v);
                                            }
                                        }
                                    }
                                }
                                None => match default_range {
                                    Some(previous) => self.error_note(
                                        range,
                                        "multiple 'default' labels in one 'switch'",
                                        previous,
                                        "previous 'default' is",
                                    ),
                                    None => {
                                        default_range = Some(range);
                                        default_group = Some(index);
                                    }
                                },
                            }
                        }
                    }
                    let lowered = self.stmt(current);
                    match groups.last_mut() {
                        Some(group) => group.body.push(lowered),
                        None => prelude.push(lowered),
                    }
                }
            }
        }

        self.pop_scope();
        self.breakables.pop();

        Stmt::Switch(Box::new(ir::Switch {
            id,
            scrutinee,
            hoisted,
            prelude,
            groups,
            default_group,
            range,
        }))
    }

    /// Declares the objects of `decl` ahead of a `switch` dispatch, leaving
    /// their initialisers behind as assignments.
    fn hoisted_decl(&mut self, decl: &ast::Decl, hoisted: &mut Vec<ObjectId>) -> Vec<Stmt> {
        let mut out = Vec::new();
        for declarator in &decl.declarators {
            for stmt in self.declarator(decl, declarator, false) {
                match stmt {
                    Stmt::Let {
                        object,
                        init,
                        explicit,
                    } => {
                        hoisted.push(object);
                        // Only an initialiser that was actually written should
                        // run where it was written; a synthesised zero is
                        // already covered by the hoisted definition.
                        if explicit {
                            let info = self.program.object(object);
                            let (ty, is_const) = (info.ty, info.is_const);
                            let place = Place {
                                kind: PlaceKind::Object(object),
                                ty,
                                is_const,
                                range: declarator.range,
                            };
                            out.push(Stmt::Expr(Expr::new(
                                ExprKind::Assign {
                                    place,
                                    value: Box::new(init),
                                },
                                ty,
                                declarator.range,
                            )));
                        }
                    }
                    other => out.push(other),
                }
            }
        }
        out
    }

    /// Whether a function's body has to be lowered through a control-flow
    /// graph.
    ///
    /// Two things force it: a `goto`, which Rust has nothing to offer for, and
    /// a `case` or `default` label that is not a direct child of its `switch`
    /// body, which the fallthrough-group lowering cannot express. Everything
    /// else keeps the structured form, whose output reads like the C it came
    /// from.
    pub(super) fn needs_cfg(block: &ast::Block) -> bool {
        block_needs_cfg(block, 0, false)
    }

    /// Evaluates a `case` label and converts it to the switch's type.
    fn case_value(&mut self, expr: &ast::Expr, ty: Ty) -> Option<i128> {
        let value = self.expr(expr)?;
        if !value.ty.is_integer() {
            self.error(
                expr.range,
                format!(
                    "expression of type '{}' is not an integer constant expression",
                    self.tyname(value.ty)
                ),
            );
            return None;
        }
        match self.const_eval_at(&value, "'case' label")? {
            ConstValue::Int(v) => Some(ty.wrap(v, &self.target)),
            ConstValue::Float(_) => None,
        }
    }

    /// Evaluates `case low:` or GNU's `case low ... high:`.
    ///
    /// GCC merely warns about an empty range and then matches nothing, which
    /// is a `switch` arm that silently never runs; this refuses it, because
    /// the only way to write one is by mistake.
    fn case_range(
        &mut self,
        expr: &ast::Expr,
        upper: Option<&ast::Expr>,
        ty: Ty,
        range: SourceRange,
    ) -> Option<ir::CaseRange> {
        let low = self.case_value(expr, ty)?;
        let Some(upper) = upper else {
            return Some(ir::CaseRange::single(low));
        };
        let high = self.case_value(upper, ty)?;
        let signed = ty.is_signed(&self.target);
        let ordered = if signed {
            low <= high
        } else {
            (low as u128) <= (high as u128)
        };
        if !ordered {
            self.error(
                range,
                format!(
                    "empty case range: '{}' is above '{}', so nothing can enter here",
                    render_case_value(low, ty, &self.target),
                    render_case_value(high, ty, &self.target)
                ),
            );
            return None;
        }
        Some(ir::CaseRange { low, high })
    }
}

/// A `case` label the way it should read in a diagnostic.
fn render_case_range(value: ir::CaseRange, ty: Ty, target: &crate::TargetModel) -> String {
    if value.is_single() {
        return render_case_value(value.low, ty, target);
    }
    format!(
        "{} ... {}",
        render_case_value(value.low, ty, target),
        render_case_value(value.high, ty, target)
    )
}

/// What a jump found inside a statement expression does.
enum Escape {
    /// A label or a `goto`, which is refused in every mode.
    Label(&'static str),
    /// A `break` or a `continue` that leaves the statement expression.
    Leaves(&'static str),
}

/// Collects the jumps inside a statement expression that would leave it.
///
/// `depth` counts the loops and `switch`es the statement sits in *within* the
/// statement expression; a `break` or `continue` at depth zero leaves it.
fn collect_escaping_jumps(
    items: &[ast::BlockItem],
    depth: u32,
    out: &mut Vec<(SourceRange, Escape)>,
) {
    for item in items {
        if let ast::BlockItem::Stmt(stmt) = item {
            escaping_jumps(stmt, depth, out);
        }
    }
}

fn escaping_jumps(stmt: &ast::Stmt, depth: u32, out: &mut Vec<(SourceRange, Escape)>) {
    match &stmt.kind {
        ast::StmtKind::Break if depth == 0 => out.push((stmt.range, Escape::Leaves("a 'break'"))),
        ast::StmtKind::Continue if depth == 0 => {
            out.push((stmt.range, Escape::Leaves("a 'continue'")));
        }
        ast::StmtKind::Goto(_) => out.push((stmt.range, Escape::Label("a 'goto'"))),
        ast::StmtKind::Labeled { body, .. } => {
            out.push((stmt.range, Escape::Label("a label")));
            escaping_jumps(body, depth, out);
        }
        ast::StmtKind::Case { body, .. } => {
            if depth == 0 {
                out.push((stmt.range, Escape::Label("a 'case' label")));
            }
            escaping_jumps(body, depth, out);
        }
        ast::StmtKind::Default { body } => {
            if depth == 0 {
                out.push((stmt.range, Escape::Label("a 'default' label")));
            }
            escaping_jumps(body, depth, out);
        }
        ast::StmtKind::Compound(block) => collect_escaping_jumps(&block.items, depth, out),
        ast::StmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            escaping_jumps(then_branch, depth, out);
            if let Some(branch) = else_branch {
                escaping_jumps(branch, depth, out);
            }
        }
        ast::StmtKind::While { body, .. }
        | ast::StmtKind::DoWhile { body, .. }
        | ast::StmtKind::For { body, .. }
        | ast::StmtKind::Switch { body, .. } => escaping_jumps(body, depth + 1, out),
        _ => {}
    }
}

/// Whether any statement of `block` forces the CFG lowering.
///
/// `switch_depth` counts the `switch` bodies the block sits in and `at_top`
/// says whether a label written here would be a direct child of the innermost
/// one — which is the only place the structured lowering can put one.
fn block_needs_cfg(block: &ast::Block, switch_depth: u32, at_top: bool) -> bool {
    block.items.iter().any(|item| match item {
        ast::BlockItem::Stmt(stmt) => stmt_needs_cfg(stmt, switch_depth, at_top),
        ast::BlockItem::Decl(_) | ast::BlockItem::StaticAssert(_) => false,
    })
}

fn stmt_needs_cfg(stmt: &ast::Stmt, switch_depth: u32, at_top: bool) -> bool {
    match &stmt.kind {
        ast::StmtKind::Goto(_) => true,
        // A chain of labels on one statement is as much a direct child of the
        // `switch` as the statement itself.
        ast::StmtKind::Case { body, .. }
        | ast::StmtKind::Default { body }
        | ast::StmtKind::Labeled { body, .. } => {
            let nested = switch_depth > 0
                && !at_top
                && matches!(
                    stmt.kind,
                    ast::StmtKind::Case { .. } | ast::StmtKind::Default { .. }
                );
            nested || stmt_needs_cfg(body, switch_depth, at_top)
        }
        ast::StmtKind::Compound(block) => block_needs_cfg(block, switch_depth, false),
        ast::StmtKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_needs_cfg(then_branch, switch_depth, false)
                || else_branch
                    .as_ref()
                    .is_some_and(|s| stmt_needs_cfg(s, switch_depth, false))
        }
        ast::StmtKind::While { body, .. }
        | ast::StmtKind::DoWhile { body, .. }
        | ast::StmtKind::For { body, .. } => stmt_needs_cfg(body, switch_depth, false),
        ast::StmtKind::Switch { body, .. } => match &body.kind {
            ast::StmtKind::Compound(block) => block_needs_cfg(block, switch_depth + 1, true),
            _ => stmt_needs_cfg(body, switch_depth + 1, true),
        },
        _ => false,
    }
}
