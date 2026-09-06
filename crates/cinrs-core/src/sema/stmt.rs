//! Statements: the lowering of `switch` into fallthrough groups, and the
//! decision between the structured and the [CFG](crate::cfg) form.

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{
    self, BreakTarget, ConstValue, Expr, ExprKind, LabelId, LoopId, ObjectId, Place, PlaceKind,
    Stmt, SwitchId, Ty,
};

use super::{Breakable, ConvContext, Label, Sema, SwitchState, render_case_value};

impl Sema<'_> {
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
        // The scope of a variable length array declared here ends with the
        // block, and so does the lifetime of its storage — and so does what a
        // `cleanup` attribute written here owes.
        let vla_depth = self.vla_scopes.len();
        let cleanup_depth = self.cleanup_depth;
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
                // A nested function is lifted to an item of its own, so it
                // contributes nothing to the block it was written in.
                ast::BlockItem::NestedFunction(def) => self.nested_function_def(def),
            }
        }
        self.vla_scopes.truncate(vla_depth);
        self.cleanup_depth = cleanup_depth;
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
                // What a `goto` to this label would be jumping *into*; see
                // `Sema::vla_scopes`.
                if let Some(entry) = self.labels.get(&label.name) {
                    let id = entry.id;
                    let scopes = self.vla_scopes.clone();
                    self.label_vla_scopes.entry(id).or_insert(scopes);
                }
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
                // C99 6.8.4p3: a selection statement is a block of its own,
                // so a tag declared in the controlling expression —
                // `if (sizeof(enum { a, b }))` — is scoped to the `if` and
                // does not leak into the enclosing block.
                self.push_scope();
                let cond = self.condition(cond);
                let then_branch = Box::new(self.stmt(then_branch));
                let else_branch = else_branch.as_ref().map(|s| Box::new(self.stmt(s)));
                self.pop_scope();
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
                // C99 6.8.5p5: an iteration statement is a block of its own,
                // for the reason `if` is one just above.
                self.push_scope();
                let cond = self.condition(cond);
                self.breakables.push(Breakable::Loop(id));
                let body = Box::new(self.stmt(body));
                self.breakables.pop();
                self.pop_scope();
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
                self.push_scope();
                self.breakables.push(Breakable::Loop(id));
                let body = Box::new(self.stmt(body));
                self.breakables.pop();
                let cond = self.condition(cond);
                self.pop_scope();
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
                let vla_depth = self.vla_scopes.len();
                let cleanup_depth = self.cleanup_depth;
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
                    // Nothing to run: the assertion is checked here and
                    // generates no code, exactly as it does anywhere else.
                    ast::ForInit::StaticAssert(assert) => {
                        self.static_assert(assert);
                        Vec::new()
                    }
                };
                let cond = cond.as_ref().and_then(|c| self.condition(c));
                let step = step.as_ref().and_then(|s| self.expr(s));
                self.breakables.push(Breakable::Loop(id));
                let body = Box::new(self.stmt(body));
                self.breakables.pop();
                self.pop_scope();
                self.vla_scopes.truncate(vla_depth);
                self.cleanup_depth = cleanup_depth;
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
                Some(entry) => {
                    // The label may not have been reached yet, so what this
                    // jump would enter is checked once the body is done.
                    let id = entry.id;
                    self.goto_scopes
                        .push((stmt.range, id, self.vla_scopes.clone()));
                    Stmt::Goto {
                        id,
                        range: stmt.range,
                    }
                }
                None => {
                    self.report_missing_label(label);
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
                Some(value) if ret.is_void() => {
                    return self.void_return(value, expr.range, range);
                }
                Some(value) => Some(self.convert_for(value, ret, ConvContext::Return)),
            },
        };
        // GCC computes the returned value and *then* runs the cleanups the
        // scopes being left owe. Rust's own drop order says that already in
        // the structured lowering; in the [CFG](crate::cfg) one the calls are
        // statements in front of the `return`, so the value has to be put
        // somewhere they cannot change it.
        if self.cfg_mode
            && self.cleanup_depth > 0
            && let Some(value) = value
        {
            if matches!(value.kind, ExprKind::Int(_) | ExprKind::Float(_)) {
                return Stmt::Return {
                    value: Some(value),
                    range,
                };
            }
            let object = self.new_object(
                "__cinrs_ret",
                ret,
                crate::ir::Storage::Automatic,
                false,
                range,
            );
            let load = Expr::new(
                ExprKind::Load(super::place_of(
                    PlaceKind::Object(object),
                    ret,
                    false,
                    range,
                )),
                ret,
                range,
            );
            return Stmt::Block(vec![
                Stmt::Let {
                    object,
                    init: value,
                    explicit: true,
                },
                Stmt::Return {
                    value: Some(load),
                    range,
                },
            ]);
        }
        Stmt::Return { value, range }
    }

    /// `return expr;` in a function whose return type is `void`.
    ///
    /// C99 and C11 6.8.6.4p1 forbid it outright. **C23 lets the expression
    /// stand when it has type `void`** — `return f();` where `f` returns
    /// nothing is how a wrapper forwards a call, and there is no value to
    /// return — and GCC has accepted that, and a non-`void` expression with
    /// it, in every mode it has, with only a pedantic warning ("ISO C forbids
    /// 'return' with expression, in function returning void"). The GNU
    /// dialects follow GCC; the strict ones below C23 keep the constraint
    /// violation.
    ///
    /// Where it is accepted the expression is still *evaluated* — it is where
    /// the call was written — and only its value is dropped.
    fn void_return(&mut self, value: Expr, expr_range: SourceRange, range: SourceRange) -> Stmt {
        let allowed = if value.ty.is_void() {
            self.gnu_leniency() || self.gating.standard >= crate::Standard::C23
        } else {
            self.gnu_leniency()
        };
        if !allowed {
            let message = format!(
                "void function '{}' should not return a value",
                self.func_name
            );
            let note = if value.ty.is_void() {
                format!(
                    "the expression has type 'void', which C23 allows here (6.8.6.4p1); {}",
                    self.gnu_note()
                )
            } else {
                self.gnu_note()
            };
            self.diags
                .push(crate::diag::Diagnostic::error(expr_range, message).with_note(note));
            return Stmt::Return { value: None, range };
        }
        Stmt::Block(vec![Stmt::Expr(value), Stmt::Return { value: None, range }])
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

    /// Reports a `goto` whose label this function does not have.
    ///
    /// When an *enclosing* function has it, the jump is GNU's nonlocal goto:
    /// it unwinds to the enclosing frame, which GCC arranges with the frame
    /// pointer the nested function was handed. Lambda lifting keeps no such
    /// pointer, so the construct is named rather than reported as a label
    /// nobody wrote.
    fn report_missing_label(&mut self, label: &ast::Ident) {
        let nonlocal = self.nest.split_last().is_some_and(|(_, enclosing)| {
            enclosing
                .iter()
                .any(|frame| frame.labels.contains(&label.name))
        });
        if nonlocal {
            self.error(
                label.range,
                format!(
                    "'goto {}' leaves this nested function for a label of the enclosing one; \
                     GNU C calls that a nonlocal goto and reaches it through the enclosing \
                     frame, which cinrs cannot do. Return a value the enclosing function can \
                     branch on instead",
                    label.name
                ),
            );
            return;
        }
        self.error(
            label.range,
            format!("use of undeclared label '{}'", label.name),
        );
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
        // C99 6.8.4p3: the whole selection statement, controlling expression
        // and all, is a block of its own.
        self.push_scope();
        let Some(scrutinee) = self.scrutinee(cond) else {
            self.pop_scope();
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
        self.switch_vla_depths.push(self.vla_scopes.len());
        let body = self.stmt(body);
        self.switch_vla_depths.pop();
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
        self.check_jump_into_vla_scope(range);
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
        self.push_scope();
        let Some(scrutinee) = self.scrutinee(cond) else {
            self.pop_scope();
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
        self.switch_vla_depths.push(self.vla_scopes.len());

        let mut hoisted = Vec::new();
        let mut prelude = Vec::new();
        let mut groups: Vec<ir::SwitchGroup> = Vec::new();
        let mut default_group: Option<usize> = None;
        let mut default_range: Option<SourceRange> = None;
        let mut seen: Vec<(ir::CaseRange, SourceRange)> = Vec::new();

        for item in items {
            match item {
                ast::BlockItem::StaticAssert(assert) => self.static_assert(assert),
                ast::BlockItem::NestedFunction(def) => self.nested_function_def(def),
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
                        self.check_jump_into_vla_scope(labels[0].1);
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

        self.switch_vla_depths.pop();
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
            let vla_depth = self.vla_scopes.len();
            for stmt in self.declarator(decl, declarator, false) {
                match stmt {
                    // A variable length array cannot be hoisted ahead of the
                    // dispatch: its storage is only allocated where the
                    // declaration stands, so every `case` after it would be a
                    // jump into its scope — which is what C99 6.8.4.2p2 says.
                    Stmt::Vla(def) => {
                        self.error(
                            def.range,
                            "a variable length array cannot be declared directly in the body \
                             of a 'switch'; a label after it would jump into its scope",
                        );
                        // The declaration is refused, so nothing after it is
                        // inside its scope: leaving the entry behind would
                        // report every later label as well.
                        self.vla_scopes.truncate(vla_depth);
                    }
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

    /// Reports a `case` or `default` label that would jump into the scope of
    /// an identifier with a variably modified type (C99 6.8.4.2p2).
    ///
    /// Entering a `switch` jumps straight to the label, past whatever the body
    /// declared on the way — and past the allocation a variable length array's
    /// declaration performs, which would leave the object without storage.
    fn check_jump_into_vla_scope(&mut self, range: SourceRange) {
        let entered = self
            .switch_vla_depths
            .last()
            .is_some_and(|depth| self.vla_scopes.len() > *depth);
        if entered {
            self.error(range, super::JUMP_INTO_VM_SCOPE);
        }
    }

    /// Reports the `goto`s of the function just checked that would jump into
    /// the scope of an identifier with a variably modified type
    /// (C99 6.8.6.1p1).
    ///
    /// A jump *out of* such a scope is fine — the storage is freed on the way
    /// — so the test is one-sided: every variable length array in scope at the
    /// label must already be in scope at the `goto`.
    pub(super) fn check_goto_vla_scopes(&mut self) {
        let bad: Vec<SourceRange> = self
            .goto_scopes
            .iter()
            .filter(|(_, label, from)| {
                self.label_vla_scopes
                    .get(label)
                    .is_some_and(|into| into.iter().any(|object| !from.contains(object)))
            })
            .map(|(range, _, _)| *range)
            .collect();
        for range in bad {
            self.error(range, super::JUMP_INTO_VM_SCOPE);
        }
        self.goto_scopes.clear();
        self.label_vla_scopes.clear();
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
            // Neither can arrive: the label was checked to be an integer.
            ConstValue::Float(_) | ConstValue::Complex(..) => None,
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

/// How many `case` groups a `switch` may have before it is lowered through a
/// control-flow graph instead.
///
/// The structured lowering emits one labelled block per group, each inside the
/// last (see [`crate::codegen`]), and `rustc`'s own parser recurses once per
/// level of that: it reads four hundred of them and dies on the stack at
/// around eight hundred. C23 5.2.5.2p1 asks for 1023 `case` labels in one
/// `switch`, so a big one takes the other path — the state machine the CFG
/// lowering makes, whose `match` is flat however many states it has.
const MAX_SWITCH_GROUPS: usize = 200;

/// Whether any statement of `block` forces the CFG lowering.
///
/// `switch_depth` counts the `switch` bodies the block sits in and `at_top`
/// says whether a label written here would be a direct child of the innermost
/// one — which is the only place the structured lowering can put one.
fn block_needs_cfg(block: &ast::Block, switch_depth: u32, at_top: bool) -> bool {
    block.items.iter().any(|item| match item {
        ast::BlockItem::Stmt(stmt) => stmt_needs_cfg(stmt, switch_depth, at_top),
        // A declaration directly in a `switch` body is hoisted ahead of the
        // dispatch, and a drop guard hoisted with it would run at the end of
        // its own group rather than at the end of the body. The CFG lowering
        // has no such trouble: it emits the call on the edges that leave the
        // scope, wherever they are.
        ast::BlockItem::Decl(decl) if at_top && switch_depth > 0 => decl
            .declarators
            .iter()
            .any(|d| d.attrs.cleanup.is_some() || decl.specifiers.attrs.cleanup.is_some()),
        // A nested function definition is an item of its own: whether *its*
        // body needs the CFG lowering is decided when it is checked, and says
        // nothing about the function it was written in.
        ast::BlockItem::Decl(_)
        | ast::BlockItem::StaticAssert(_)
        | ast::BlockItem::NestedFunction(_) => false,
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
        ast::StmtKind::Switch { body, .. } => {
            if switch_groups(body) > MAX_SWITCH_GROUPS {
                return true;
            }
            match &body.kind {
                ast::StmtKind::Compound(block) => block_needs_cfg(block, switch_depth + 1, true),
                _ => stmt_needs_cfg(body, switch_depth + 1, true),
            }
        }
        _ => false,
    }
}

/// How many groups the structured lowering would split a `switch` body into.
///
/// One per statement that carries at least one `case` or `default` label,
/// however many labels that is: `case 1: case 2: case 3: x = 1;` is one group,
/// and one labelled block in the generated Rust.
fn switch_groups(body: &ast::Stmt) -> usize {
    let items: &[ast::BlockItem] = match &body.kind {
        ast::StmtKind::Compound(block) => &block.items,
        _ => return usize::from(starts_with_case(body)),
    };
    items
        .iter()
        .filter(|item| match item {
            ast::BlockItem::Stmt(stmt) => starts_with_case(stmt),
            _ => false,
        })
        .count()
}

/// Whether a statement carries a `case` or `default` label of its own.
fn starts_with_case(stmt: &ast::Stmt) -> bool {
    let mut stmt = stmt;
    loop {
        match &stmt.kind {
            ast::StmtKind::Case { .. } | ast::StmtKind::Default { .. } => return true,
            ast::StmtKind::Labeled { body, .. } => stmt = body,
            _ => return false,
        }
    }
}
