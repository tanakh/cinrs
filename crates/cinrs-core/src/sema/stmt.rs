//! Statements: the lowering of `switch` into fallthrough groups, and the
//! decision between the structured and the [CFG](crate::cfg) form.

use std::collections::HashMap;

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
            ast::StmtKind::Case { value, body } => self.case_label(Some(value), body, stmt.range),
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
            seen: HashMap::new(),
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
        value: Option<&ast::Expr>,
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
            Some(expr) => {
                let Some(v) = self.case_value(expr, ty) else {
                    return self.stmt(body);
                };
                let state = self.switch_stack.last_mut().expect("checked above");
                if let Some(previous) = state.seen.insert(v, range) {
                    self.error_note(
                        range,
                        format!(
                            "duplicate case value '{}'",
                            render_case_value(v, ty, &self.target)
                        ),
                        previous,
                        "previous case is",
                    );
                    return self.stmt(body);
                }
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
        let mut seen: HashMap<i128, SourceRange> = HashMap::new();

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
                    let mut labels: Vec<(Option<&ast::Expr>, SourceRange)> = Vec::new();
                    loop {
                        match &current.kind {
                            ast::StmtKind::Case { value, body } => {
                                labels.push((Some(value), current.range));
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
                                Some(expr) => {
                                    if let Some(v) = self.case_value(expr, promoted) {
                                        match seen.insert(v, range) {
                                            Some(previous) => self.error_note(
                                                range,
                                                format!(
                                                    "duplicate case value '{}'",
                                                    render_case_value(v, promoted, &self.target)
                                                ),
                                                previous,
                                                "previous case is",
                                            ),
                                            None => groups[index].values.push(v),
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
