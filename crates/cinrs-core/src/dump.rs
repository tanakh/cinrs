//! A deterministic, human readable dump of the AST.
//!
//! Used by the snapshot tests and handy when debugging the parser. Source
//! ranges are deliberately left out: they would drown the interesting
//! structure, and span behaviour is covered by its own tests.

use crate::ast::*;
use crate::lex::{FloatSuffix, LongKind};

/// Renders a whole translation unit.
pub fn dump_translation_unit(unit: &TranslationUnit) -> String {
    let mut d = Dumper::new(unit);
    d.line("translation-unit");
    d.indent += 1;
    for item in &unit.items {
        d.external_decl(item);
    }
    d.out
}

/// Renders a single expression (useful for focused tests).
///
/// The unit is needed because a type inside the expression may name a tag
/// specifier, which lives in [`TranslationUnit::records`].
pub fn dump_expr(unit: &TranslationUnit, expr: &Expr) -> String {
    let mut d = Dumper::new(unit);
    d.expr(expr);
    d.out
}

/// Renders a type the way the dump does.
pub fn type_to_string(unit: &TranslationUnit, ty: &Type) -> String {
    let mut s = String::new();
    if ty.qualifiers.is_const {
        s.push_str("const ");
    }
    if ty.qualifiers.is_volatile {
        s.push_str("volatile ");
    }
    if ty.qualifiers.is_restrict {
        s.push_str("restrict ");
    }
    match &ty.kind {
        TypeKind::Void => s.push_str("void"),
        TypeKind::Bool => s.push_str("_Bool"),
        TypeKind::Char(None) => s.push_str("char"),
        TypeKind::Char(Some(Sign::Signed)) => s.push_str("signed char"),
        TypeKind::Char(Some(Sign::Unsigned)) => s.push_str("unsigned char"),
        TypeKind::Int { sign, size } => {
            if *sign == Sign::Unsigned {
                s.push_str("unsigned ");
            }
            s.push_str(match size {
                IntSize::Short => "short",
                IntSize::Int => "int",
                IntSize::Long => "long",
                IntSize::LongLong => "long long",
            });
        }
        TypeKind::Float(size) => s.push_str(float_size(*size)),
        TypeKind::Complex(size) => {
            s.push_str(float_size(*size));
            s.push_str(" _Complex");
        }
        TypeKind::Imaginary(size) => {
            s.push_str(float_size(*size));
            s.push_str(" _Imaginary");
        }
        TypeKind::Pointer(inner) => {
            s.push_str("ptr<");
            s.push_str(&type_to_string(unit, inner));
            s.push('>');
        }
        TypeKind::Array {
            elem,
            size,
            qualifiers,
            is_static,
        } => {
            s.push_str("array<");
            s.push_str(&type_to_string(unit, elem));
            s.push_str(", ");
            s.push_str(&array_size(size));
            if *is_static {
                s.push_str(", static");
            }
            if qualifiers.any() {
                s.push_str(", ");
                if qualifiers.is_const {
                    s.push_str("const ");
                }
                if qualifiers.is_volatile {
                    s.push_str("volatile ");
                }
                if qualifiers.is_restrict {
                    s.push_str("restrict ");
                }
                s.pop();
            }
            s.push('>');
        }
        TypeKind::Function(ft) => {
            s.push_str("fn(");
            if !ft.has_prototype {
                if ft.kr_names.is_empty() {
                    s.push_str("<unspecified>");
                } else {
                    s.push_str("<K&R> ");
                    let names: Vec<&str> = ft.kr_names.iter().map(|n| n.name.as_str()).collect();
                    s.push_str(&names.join(", "));
                }
            } else if ft.params.is_empty() && !ft.variadic {
                s.push_str("void");
            } else {
                let params: Vec<String> = ft
                    .params
                    .iter()
                    .map(|p| match &p.name {
                        Some(n) => format!("{}: {}", n.name, type_to_string(unit, &p.ty)),
                        None => type_to_string(unit, &p.ty),
                    })
                    .collect();
                s.push_str(&params.join(", "));
                if ft.variadic {
                    if !params.is_empty() {
                        s.push_str(", ");
                    }
                    s.push_str("...");
                }
            }
            s.push_str(") -> ");
            s.push_str(&type_to_string(unit, &ft.ret));
        }
        TypeKind::Record(id) => {
            let r = unit.record(*id);
            s.push_str(r.kind.as_str());
            s.push(' ');
            match &r.name {
                Some(n) => s.push_str(&n.name),
                None => s.push_str("<anonymous>"),
            }
            if r.fields.is_some() {
                s.push_str(" {…}");
            }
        }
        TypeKind::Enum(id) => {
            let e = unit.enum_spec(*id);
            s.push_str("enum ");
            match &e.name {
                Some(n) => s.push_str(&n.name),
                None => s.push_str("<anonymous>"),
            }
            if e.enumerators.is_some() {
                s.push_str(" {…}");
            }
        }
        TypeKind::Typedef(id) => {
            s.push_str("typedef-name ");
            s.push_str(&id.name);
        }
        TypeKind::Typeof(id) => {
            s.push_str("typeof<");
            match unit.typeof_operand(*id) {
                TypeofOperand::Expr(_) => s.push_str("expr"),
                TypeofOperand::Type(name) => s.push_str(&type_to_string(unit, &name.ty)),
            }
            s.push('>');
        }
        TypeKind::Auto => s.push_str("auto"),
        TypeKind::Error => s.push_str("<error>"),
    }
    s
}

fn float_size(size: FloatSize) -> &'static str {
    match size {
        FloatSize::Float => "float",
        FloatSize::Double => "double",
        FloatSize::LongDouble => "long double",
    }
}

fn array_size(size: &ArraySize) -> String {
    match size {
        ArraySize::Unspecified => "?".to_owned(),
        ArraySize::Star => "*".to_owned(),
        ArraySize::Expr(e) => match &e.kind {
            ExprKind::Int(lit) => lit.value.to_string(),
            _ => "expr".to_owned(),
        },
    }
}

struct Dumper<'a> {
    /// The unit being dumped, which the tag specifiers are looked up in.
    unit: &'a TranslationUnit,
    out: String,
    indent: usize,
}

impl<'a> Dumper<'a> {
    fn new(unit: &'a TranslationUnit) -> Self {
        Self {
            unit,
            out: String::new(),
            indent: 0,
        }
    }

    /// The rendering of a type, in this unit.
    fn ty(&self, ty: &Type) -> String {
        type_to_string(self.unit, ty)
    }

    fn line(&mut self, text: impl AsRef<str>) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
        self.out.push_str(text.as_ref());
        self.out.push('\n');
    }

    /// Emits `label` and runs `f` one level deeper.
    fn under(&mut self, label: &str, f: impl FnOnce(&mut Self)) {
        self.line(label);
        self.indent += 1;
        f(self);
        self.indent -= 1;
    }

    // -- declarations -------------------------------------------------------

    fn external_decl(&mut self, item: &ExternalDecl) {
        match item {
            ExternalDecl::Function(f) => self.function_def(f),
            ExternalDecl::Decl(d) => self.decl(d),
            ExternalDecl::StaticAssert(sa) => self.static_assert(sa),
        }
    }

    fn static_assert(&mut self, sa: &StaticAssert) {
        let label = match &sa.message {
            Some(message) => format!("static-assert {message}"),
            None => "static-assert".to_owned(),
        };
        self.under(&label, |d| d.expr(&sa.cond));
    }

    fn function_def(&mut self, f: &FunctionDef) {
        let mut header = format!("function '{}' : {}", f.name.name, self.ty(&f.ty));
        if let Some(storage) = &f.specifiers.storage {
            header.push_str(&format!(" [{}]", storage.node.as_str()));
        }
        if f.specifiers.inline {
            header.push_str(" [inline]");
        }
        self.line(header);
        self.indent += 1;
        self.type_body(&f.specifiers.base);
        for kr in &f.kr_decls {
            self.under("kr-param-decl", |d| d.decl_inner(kr));
        }
        self.under("body", |d| {
            for item in &f.body.items {
                d.block_item(item);
            }
        });
        self.indent -= 1;
    }

    fn decl(&mut self, decl: &Decl) {
        self.line("declaration");
        self.indent += 1;
        self.decl_inner(decl);
        self.indent -= 1;
    }

    fn decl_inner(&mut self, decl: &Decl) {
        let mut header = format!("specifiers: {}", self.ty(&decl.specifiers.base));
        if let Some(storage) = &decl.specifiers.storage {
            header.push_str(&format!(" [{}]", storage.node.as_str()));
        }
        if decl.specifiers.inline {
            header.push_str(" [inline]");
        }
        self.line(header);
        self.type_body(&decl.specifiers.base);
        for d in &decl.declarators {
            let name = d.name.as_ref().map_or("<abstract>", |n| n.name.as_str());
            self.line(format!("declarator '{}' : {}", name, self.ty(&d.ty)));
            if let Some(init) = &d.init {
                self.indent += 1;
                self.under("init", |dd| dd.initializer(init));
                self.indent -= 1;
            }
        }
    }

    /// Dumps the members of a `struct`/`union`/`enum` specifier that has a
    /// body, so that bit-fields and enumerator values are visible.
    fn type_body(&mut self, ty: &Type) {
        match &ty.kind {
            TypeKind::Record(id) => {
                let r = self.unit.record(*id);
                let Some(fields) = &r.fields else { return };
                let label = format!(
                    "{} '{}' members",
                    r.kind.as_str(),
                    r.name.as_ref().map_or("<anonymous>", |n| n.name.as_str())
                );
                self.under(&label, |d| {
                    for f in fields {
                        let name = f.name.as_ref().map_or("<anonymous>", |n| n.name.as_str());
                        let mut line = format!("field '{}' : {}", name, d.ty(&f.ty));
                        if let Some(w) = &f.bit_width {
                            if let ExprKind::Int(lit) = &w.kind {
                                line.push_str(&format!(" : {}", lit.value));
                            } else {
                                line.push_str(" : <expr>");
                            }
                        }
                        d.line(line);
                        d.indent += 1;
                        d.type_body(&f.specifiers.base);
                        d.indent -= 1;
                    }
                    for assert in &r.asserts {
                        d.static_assert(assert);
                    }
                });
            }
            TypeKind::Enum(id) => {
                let e = self.unit.enum_spec(*id);
                let Some(enumerators) = &e.enumerators else {
                    return;
                };
                let label = format!(
                    "enum '{}' enumerators",
                    e.name.as_ref().map_or("<anonymous>", |n| n.name.as_str())
                );
                self.under(&label, |d| {
                    for en in enumerators {
                        match &en.value {
                            Some(v) => {
                                d.under(&format!("enumerator '{}'", en.name.name), |dd| dd.expr(v))
                            }
                            None => d.line(format!("enumerator '{}'", en.name.name)),
                        }
                    }
                });
            }
            _ => {}
        }
    }

    fn initializer(&mut self, init: &Initializer) {
        match &init.kind {
            InitializerKind::Expr(e) => self.expr(e),
            InitializerKind::List(items) => self.under("init-list", |d| {
                for item in items {
                    let label = element_label(&item.designators);
                    d.under(&label, |dd| dd.initializer(&item.init));
                }
            }),
        }
    }

    // -- statements ---------------------------------------------------------

    fn block_item(&mut self, item: &BlockItem) {
        match item {
            BlockItem::Decl(d) => self.decl(d),
            BlockItem::Stmt(s) => self.stmt(s),
            BlockItem::StaticAssert(sa) => self.static_assert(sa),
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Labeled { label, body } => {
                self.under(&format!("label '{}'", label.name), |d| d.stmt(body));
            }
            StmtKind::Case { value, upper, body } => self.under("case", |d| {
                d.under("value", |dd| dd.expr(value));
                if let Some(upper) = upper {
                    d.under("upto", |dd| dd.expr(upper));
                }
                d.stmt(body);
            }),
            StmtKind::Default { body } => self.under("default", |d| d.stmt(body)),
            StmtKind::Compound(block) => self.under("block", |d| {
                for item in &block.items {
                    d.block_item(item);
                }
            }),
            StmtKind::Expr(None) => self.line("null-stmt"),
            StmtKind::Expr(Some(e)) => self.under("expr-stmt", |d| d.expr(e)),
            StmtKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.under("if", |d| {
                d.under("cond", |dd| dd.expr(cond));
                d.under("then", |dd| dd.stmt(then_branch));
                if let Some(e) = else_branch {
                    d.under("else", |dd| dd.stmt(e));
                }
            }),
            StmtKind::Switch { cond, body } => self.under("switch", |d| {
                d.under("cond", |dd| dd.expr(cond));
                d.under("body", |dd| dd.stmt(body));
            }),
            StmtKind::While { cond, body } => self.under("while", |d| {
                d.under("cond", |dd| dd.expr(cond));
                d.under("body", |dd| dd.stmt(body));
            }),
            StmtKind::DoWhile { body, cond } => self.under("do-while", |d| {
                d.under("body", |dd| dd.stmt(body));
                d.under("cond", |dd| dd.expr(cond));
            }),
            StmtKind::For {
                init,
                cond,
                step,
                body,
            } => self.under("for", |d| {
                match init {
                    ForInit::None => d.line("init: <none>"),
                    ForInit::Expr(e) => d.under("init", |dd| dd.expr(e)),
                    ForInit::Decl(decl) => d.under("init", |dd| dd.decl(decl)),
                }
                match cond {
                    Some(e) => d.under("cond", |dd| dd.expr(e)),
                    None => d.line("cond: <none>"),
                }
                match step {
                    Some(e) => d.under("step", |dd| dd.expr(e)),
                    None => d.line("step: <none>"),
                }
                d.under("body", |dd| dd.stmt(body));
            }),
            StmtKind::Goto(label) => self.line(format!("goto '{}'", label.name)),
            StmtKind::Continue => self.line("continue"),
            StmtKind::Break => self.line("break"),
            StmtKind::Return(None) => self.line("return"),
            StmtKind::Return(Some(e)) => self.under("return", |d| d.expr(e)),
            StmtKind::Error => self.line("<error-stmt>"),
        }
    }

    // -- expressions --------------------------------------------------------

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Ident(id) => self.line(format!("ident '{}'", id.name)),
            ExprKind::Int(lit) => {
                let mut s = format!("int {}", lit.value);
                if lit.unsigned {
                    s.push_str(" unsigned");
                }
                match lit.long {
                    LongKind::None => {}
                    LongKind::Long => s.push_str(" long"),
                    LongKind::LongLong => s.push_str(" long-long"),
                }
                self.line(s);
            }
            ExprKind::Float(lit) => {
                let mut s = format!("float {}", lit.value);
                match lit.suffix {
                    FloatSuffix::None => {}
                    FloatSuffix::Float => s.push_str(" f"),
                    FloatSuffix::LongDouble => s.push_str(" l"),
                }
                self.line(s);
            }
            ExprKind::Char(lit) => {
                self.line(format!(
                    "char {}{} = {}",
                    lit.kind.prefix(),
                    lit.text,
                    lit.value
                ));
            }
            ExprKind::Str(lit) => {
                self.line(format!(
                    "string {}{} ({} elements)",
                    lit.kind.prefix(),
                    lit.text,
                    lit.values.len()
                ));
            }
            ExprKind::Unary { op, operand } => {
                self.under(&format!("unary '{}'", op.as_str()), |d| d.expr(operand));
            }
            ExprKind::Binary { op, lhs, rhs } => {
                self.under(&format!("binary '{}'", op.as_str()), |d| {
                    d.expr(lhs);
                    d.expr(rhs);
                });
            }
            ExprKind::Assign { op, lhs, rhs } => {
                let label = match op {
                    Some(op) => format!("assign '{}='", op.as_str()),
                    None => "assign '='".to_owned(),
                };
                self.under(&label, |d| {
                    d.expr(lhs);
                    d.expr(rhs);
                });
            }
            ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => self.under("conditional", |d| {
                d.under("cond", |dd| dd.expr(cond));
                if let Some(then_expr) = then_expr {
                    d.under("then", |dd| dd.expr(then_expr));
                }
                d.under("else", |dd| dd.expr(else_expr));
            }),
            ExprKind::Comma { lhs, rhs } => self.under("comma", |d| {
                d.expr(lhs);
                d.expr(rhs);
            }),
            ExprKind::Call { callee, args } => self.under("call", |d| {
                d.under("callee", |dd| dd.expr(callee));
                for arg in args {
                    d.under("arg", |dd| dd.expr(arg));
                }
            }),
            ExprKind::Member { base, arrow, field } => {
                let op = if *arrow { "->" } else { "." };
                self.under(&format!("member '{}{}'", op, field.name), |d| d.expr(base));
            }
            ExprKind::Index { base, index } => self.under("index", |d| {
                d.expr(base);
                d.expr(index);
            }),
            ExprKind::PostIncDec { op, operand } => {
                self.under(&format!("postfix '{}'", op.as_str()), |d| d.expr(operand));
            }
            ExprKind::PreIncDec { op, operand } => {
                self.under(&format!("prefix '{}'", op.as_str()), |d| d.expr(operand));
            }
            ExprKind::Cast { ty, expr } => {
                self.under(&format!("cast to {}", self.ty(&ty.ty)), |d| d.expr(expr));
            }
            ExprKind::SizeofExpr(inner) => self.under("sizeof-expr", |d| d.expr(inner)),
            ExprKind::SizeofType(ty) => {
                self.line(format!("sizeof-type {}", self.ty(&ty.ty)));
            }
            ExprKind::AlignofExpr(inner) => self.under("alignof-expr", |d| d.expr(inner)),
            ExprKind::AlignofType(ty) => {
                self.line(format!("alignof-type {}", self.ty(&ty.ty)));
            }
            ExprKind::Generic {
                controlling,
                assocs,
            } => self.under("generic", |d| {
                d.under("controlling", |dd| dd.expr(controlling));
                for assoc in assocs {
                    let label = match &assoc.ty {
                        Some(ty) => format!("assoc {}", d.ty(&ty.ty)),
                        None => "assoc default".to_owned(),
                    };
                    d.under(&label, |dd| dd.expr(&assoc.value));
                }
            }),
            ExprKind::Bool(value) => self.line(if *value { "true" } else { "false" }),
            ExprKind::Nullptr => self.line("nullptr"),
            ExprKind::VaArg { ap, ty } => {
                self.under(&format!("va_arg {}", self.ty(&ty.ty)), |d| d.expr(ap));
            }
            ExprKind::OffsetOf { ty, member } => {
                self.line(format!("offsetof {} .{}", self.ty(&ty.ty), member.name));
            }
            ExprKind::CompoundLiteral { ty, init } => {
                self.under(&format!("compound-literal {}", self.ty(&ty.ty)), |d| {
                    for item in init {
                        let label = element_label(&item.designators);
                        d.under(&label, |dd| dd.initializer(&item.init));
                    }
                })
            }
            ExprKind::StmtExpr(block) => self.under("stmt-expr", |d| {
                for item in &block.items {
                    d.block_item(item);
                }
            }),
            ExprKind::TypesCompatible { lhs, rhs } => self.line(format!(
                "types-compatible {} {}",
                self.ty(&lhs.ty),
                self.ty(&rhs.ty)
            )),
            ExprKind::ChooseExpr {
                cond,
                then_expr,
                else_expr,
            } => self.under("choose-expr", |d| {
                d.under("cond", |dd| dd.expr(cond));
                d.under("then", |dd| dd.expr(then_expr));
                d.under("else", |dd| dd.expr(else_expr));
            }),
            ExprKind::ComplexPart { real, operand } => {
                let name = if *real { "__real__" } else { "__imag__" };
                self.under(name, |d| d.expr(operand));
            }
            ExprKind::Error => self.line("<error-expr>"),
        }
    }
}

/// The label one element of an initialiser list is dumped under.
fn element_label(designators: &[Designator]) -> String {
    let mut label = String::from("element");
    let index = |e: &Expr| match &e.kind {
        ExprKind::Int(lit) => lit.value.to_string(),
        _ => "expr".to_owned(),
    };
    for designator in designators {
        match designator {
            Designator::Field(f) => label.push_str(&format!(" .{}", f.name)),
            Designator::Index(e) => label.push_str(&format!(" [{}]", index(e))),
            Designator::Range(low, high) => {
                label.push_str(&format!(" [{} ... {}]", index(low), index(high)));
            }
        }
    }
    label
}
