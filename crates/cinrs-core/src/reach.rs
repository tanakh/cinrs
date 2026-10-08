//! What an object file of a unit needs: the functions and the objects the
//! linker sees, and everything they reach.
//!
//! A C file brings in every declaration of every header it includes. git's
//! 327-line `abspath.c` comes to some 6000 functions only declared and 740
//! `static inline` ones defined, of glibc's, OpenSSL's and git's own, for an
//! object of ten functions; generating Rust for all of them is most of the
//! time code generation takes. An object needs only what a linker can see —
//! a definition with external linkage, or one that a section, a constructor
//! list or a weak alias puts in front of it — and what that reaches: the
//! functions it calls or takes the address of, and the objects it names,
//! through any number of steps.
//!
//! [`Options::reachable_only`](crate::Options::reachable_only) asks code
//! generation for just that. The walk is over the IR, where every reference
//! to a function or an object is its id, plus the words of the assembly
//! templates, which is how inline and file-scope `asm` names a symbol. What
//! is only *declared* — the `extern` block — is decided afterwards from the
//! Rust the kept definitions became; see [`crate::codegen`].

use std::collections::HashMap;

use crate::cfg::{Cfg, Terminator};
use crate::ir::{
    AsmOperandKind, Callee, Expr, ExprKind, FuncId, Function, Object, ObjectId, Place, PlaceKind,
    Program, Stmt, Storage,
};

/// Which of a program's functions and objects an object file of it needs.
pub struct Reach {
    functions: Vec<bool>,
    objects: Vec<bool>,
}

impl Reach {
    /// Whether the definition of `id` is needed.
    pub fn function(&self, id: FuncId) -> bool {
        self.functions[id.0 as usize]
    }

    /// Whether the definition of `id`, an object with static storage
    /// duration, is needed.
    pub fn object(&self, id: ObjectId) -> bool {
        self.objects[id.0 as usize]
    }
}

/// What an object file of `program` needs; see the [module](self).
pub fn reach(program: &Program) -> Reach {
    let mut walk = Walk {
        program,
        reach: Reach {
            functions: vec![false; program.functions.len()],
            objects: vec![false; program.objects.len()],
        },
        work: Vec::new(),
        inits: program
            .statics
            .iter()
            .enumerate()
            .map(|(index, var)| (var.object, index))
            .collect(),
        symbols: None,
    };
    for (index, func) in program.functions.iter().enumerate() {
        if func.body.is_some() && linker_sees_function(program, func) {
            walk.function(FuncId(index as u32));
        }
    }
    for var in &program.statics {
        if linker_sees_object(program.object(var.object)) {
            walk.object(var.object);
        }
    }
    for asm in &program.global_asm {
        walk.asm_text(&asm.template);
    }
    while let Some(item) = walk.work.pop() {
        match item {
            Item::Function(id) => walk.body(id),
            Item::Object(index) => walk.expr(&program.statics[index].init),
        }
    }
    walk.reach
}

/// Whether a function definition is one the linker sees: a symbol of the
/// unit's (a weak one included), something a section puts in the object, or
/// a constructor or destructor, which a list in the object names.
///
/// What is left is a `static` function, a GNU nested one lifted out of its
/// function, and an inline definition whose symbol another unit provides
/// ([`Program::inline_only`]) — each of which only the unit's own code can
/// reach.
fn linker_sees_function(program: &Program, func: &Function) -> bool {
    (!func.is_static && !func.is_nested() && !program.inline_only(func))
        || func.section.is_some()
        || func.init_kind.is_some()
}

/// [`linker_sees_function`] for an object with static storage duration.
fn linker_sees_object(object: &Object) -> bool {
    matches!(
        object.storage,
        Storage::Static { exported: true, .. } | Storage::ThreadLocal { exported: true, .. }
    ) || object.section.is_some()
}

/// Something whose references have yet to be walked.
enum Item {
    Function(FuncId),
    /// An object with static storage duration, by its index in
    /// [`Program::statics`], whose initialiser is what refers onwards.
    Object(usize),
}

struct Walk<'a> {
    program: &'a Program,
    reach: Reach,
    work: Vec<Item>,
    /// Each defined object's index in [`Program::statics`].
    inits: HashMap<ObjectId, usize>,
    /// The functions and objects by the names an assembly template can use
    /// for them, built the first time a template is read.
    symbols: Option<HashMap<&'a str, Vec<Target>>>,
}

#[derive(Clone, Copy)]
enum Target {
    Function(FuncId),
    Object(ObjectId),
}

impl<'a> Walk<'a> {
    fn function(&mut self, id: FuncId) {
        let seen = &mut self.reach.functions[id.0 as usize];
        if !*seen {
            *seen = true;
            if self.program.function(id).body.is_some() {
                self.work.push(Item::Function(id));
            }
        }
    }

    fn object(&mut self, id: ObjectId) {
        let seen = &mut self.reach.objects[id.0 as usize];
        if !*seen {
            *seen = true;
            if let Some(&index) = self.inits.get(&id) {
                self.work.push(Item::Object(index));
            }
        }
    }

    /// Every function and object the words of an assembly template could
    /// name: its C name, the Rust item it is generated as, or the symbol an
    /// `__asm__` label gave it.
    fn asm_text(&mut self, template: &str) {
        let program = self.program;
        let symbols = self.symbols.get_or_insert_with(|| {
            let mut symbols: HashMap<&str, Vec<Target>> = HashMap::new();
            for (index, func) in program.functions.iter().enumerate() {
                let target = Target::Function(FuncId(index as u32));
                for name in [
                    Some(func.name.as_str()),
                    Some(func.item_name()),
                    func.asm_label.as_deref(),
                ]
                .into_iter()
                .flatten()
                {
                    symbols.entry(name).or_default().push(target);
                }
            }
            for (index, object) in program.objects.iter().enumerate() {
                if matches!(object.storage, Storage::Automatic) {
                    continue;
                }
                let target = Target::Object(ObjectId(index as u32));
                for name in [
                    Some(object.name.as_str()),
                    object.storage.item_name(),
                    object.asm_label.as_deref(),
                ]
                .into_iter()
                .flatten()
                {
                    symbols.entry(name).or_default().push(target);
                }
            }
            symbols
        });
        let found: Vec<Target> = template
            .split(|c: char| c != '_' && c != '.' && c != '$' && !c.is_ascii_alphanumeric())
            .filter_map(|word| symbols.get(word))
            .flatten()
            .copied()
            .collect();
        for target in found {
            match target {
                Target::Function(id) => self.function(id),
                Target::Object(id) => self.object(id),
            }
        }
    }

    fn body(&mut self, id: FuncId) {
        let program = self.program;
        let func = program.function(id);
        match &func.body {
            Some(crate::ir::Body::Structured(stmts)) => self.stmts(stmts),
            Some(crate::ir::Body::Cfg(cfg)) => self.cfg(cfg),
            None => {}
        }
    }

    fn cfg(&mut self, cfg: &Cfg) {
        for block in &cfg.blocks {
            self.stmts(&block.stmts);
            match &block.term {
                Terminator::Branch { cond: value, .. }
                | Terminator::Switch { value, .. }
                | Terminator::Return {
                    value: Some(value), ..
                } => self.expr(value),
                Terminator::Jump { .. }
                | Terminator::Return { value: None, .. }
                | Terminator::Unreachable
                | Terminator::InvalidTarget => {}
            }
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Nop
            | Stmt::Goto { .. }
            | Stmt::Break { .. }
            | Stmt::Continue { .. }
            | Stmt::Return { value: None, .. } => {}
            Stmt::Expr(expr)
            | Stmt::GotoPtr { target: expr, .. }
            | Stmt::Return {
                value: Some(expr), ..
            }
            | Stmt::Let { init: expr, .. } => self.expr(expr),
            Stmt::Vla(def) => self.expr(&def.count),
            Stmt::Cleanup(def) => {
                self.function(def.func);
                self.expr(&def.call);
            }
            Stmt::Block(items) => self.stmts(items),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(cond);
                self.stmt(then_branch);
                if let Some(branch) = else_branch {
                    self.stmt(branch);
                }
            }
            Stmt::While { cond, body, .. } | Stmt::DoWhile { cond, body, .. } => {
                self.expr(cond);
                self.stmt(body);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
                ..
            } => {
                self.stmts(init);
                for expr in cond.iter().chain(step) {
                    self.expr(expr);
                }
                self.stmt(body);
            }
            Stmt::Switch(switch) => {
                self.expr(&switch.scrutinee);
                self.stmts(&switch.prelude);
                for group in &switch.groups {
                    self.stmts(&group.body);
                }
            }
            Stmt::SwitchTree(switch) => {
                self.expr(&switch.scrutinee);
                self.stmt(&switch.body);
            }
            Stmt::Case { body, .. } | Stmt::Label { body, .. } => self.stmt(body),
            Stmt::Region(region) => self.stmts(&region.body),
            Stmt::Asm(asm) => {
                self.asm_text(&asm.template);
                for operand in &asm.operands {
                    match &operand.kind {
                        AsmOperandKind::In(expr)
                        | AsmOperandKind::Scratch(expr)
                        | AsmOperandKind::Memory(expr) => self.expr(expr),
                        AsmOperandKind::Out { place, .. } => self.place(place),
                        AsmOperandKind::InOut { input, output } => {
                            if let Some(input) = input {
                                self.expr(input);
                            }
                            self.place(output);
                        }
                        AsmOperandKind::Discard | AsmOperandKind::Const(_) => {}
                    }
                }
            }
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Zeroed
            | ExprKind::LabelAddr(_)
            | ExprKind::VaListPristine
            | ExprKind::Unreachable
            | ExprKind::VaEnd => {}
            ExprKind::FuncAddr(id) => self.function(*id),
            ExprKind::Load(place) | ExprKind::AddrOf(place) | ExprKind::IncDec { place, .. } => {
                self.place(place);
            }
            ExprKind::VaArg { ap, .. } => self.place(ap),
            ExprKind::Assign { place, value } | ExprKind::CompoundAssign { place, value, .. } => {
                self.place(place);
                self.expr(value);
            }
            ExprKind::Neg(inner)
            | ExprKind::BitNot(inner)
            | ExprKind::Cast(inner)
            | ExprKind::UnionLit { value: inner, .. }
            | ExprKind::ArrayRepeat { value: inner, .. } => self.expr(inner),
            ExprKind::Binary { lhs, rhs, .. }
            | ExprKind::Compare { lhs, rhs, .. }
            | ExprKind::Logical { lhs, rhs, .. }
            | ExprKind::PtrDiff { lhs, rhs }
            | ExprKind::Comma { lhs, rhs }
            | ExprKind::ComplexOf { re: lhs, im: rhs }
            | ExprKind::PtrOffset {
                ptr: lhs,
                index: rhs,
                ..
            }
            | ExprKind::CondDefault {
                value: lhs,
                else_expr: rhs,
            } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Cond {
                cond,
                then_expr,
                else_expr,
            } => {
                self.expr(cond);
                self.expr(then_expr);
                self.expr(else_expr);
            }
            ExprKind::StmtExpr { stmts, value } => {
                self.stmts(stmts);
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            ExprKind::Builtin { args, .. }
            | ExprKind::ArrayLit(args)
            | ExprKind::RecordLit { fields: args, .. } => {
                for arg in args {
                    self.expr(arg);
                }
            }
            ExprKind::Atomic(atomic) => {
                for operand in atomic.operands() {
                    self.expr(operand);
                }
            }
            ExprKind::Call { callee, args } => {
                match callee {
                    Callee::Direct(id) => self.function(*id),
                    Callee::Indirect(target) => self.expr(target),
                }
                for arg in args {
                    self.expr(arg);
                }
            }
        }
    }

    fn place(&mut self, place: &Place) {
        match &place.kind {
            PlaceKind::Object(id) => self.object(*id),
            PlaceKind::Str(_) => {}
            PlaceKind::Deref(ptr) => self.expr(ptr),
            PlaceKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            PlaceKind::Field { base, .. } | PlaceKind::ComplexPart { base, .. } => {
                self.place(base);
            }
            PlaceKind::Temporary(value) => self.expr(value),
            PlaceKind::CompoundLiteral { object, init } => {
                self.object(*object);
                self.expr(init);
            }
        }
    }
}
