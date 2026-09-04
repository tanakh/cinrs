//! Semantic analysis: the untyped AST becomes the typed [`ir`].
//!
//! This is where C stops being syntax and starts being a language with rules.
//! The pass
//!
//! * resolves declarations into a symbol table with C's block scoping — and a
//!   second, separate scope stack for `struct`/`union`/`enum` tags, which C
//!   keeps in a namespace of their own — checking redefinitions, conflicting
//!   function types, arity, lvalues and the placement of `break`, `continue`,
//!   `case` and `default`;
//! * resolves every [`ast::Type`] into an [`ir::Ty`], interning the derived
//!   types (pointer, array, function) so that type equality is one integer
//!   comparison, and computing the layout of every complete record so that
//!   `sizeof` folds to a constant;
//! * makes every conversion C performs implicitly explicit — the integer
//!   promotions, the usual arithmetic conversions, array-to-pointer and
//!   function-to-pointer decay, and the conversions on assignment,
//!   initialisation, argument passing and `return` — so that codegen can be a
//!   transliteration rather than a second type checker;
//! * evaluates the constant expressions C requires: `case` labels, enumerators,
//!   array bounds and the initialisers of objects with static storage duration;
//! * collects each function's labels — C gives them function scope and a
//!   namespace of their own — and decides, before checking the body, whether
//!   the function can keep Rust's own control flow or has to be lowered
//!   through a [control-flow graph](crate::cfg).
//!
//! # Module layout
//!
//! `types` resolves AST types and computes layouts, `decl` handles
//! declarations, `expr` expressions, `stmt` statements, `init` initialisers and
//! `va` the `<stdarg.h>` builtins. They are all inherent `impl` blocks on the
//! one `Sema` struct; the split is for reading, not for encapsulation.
//!
//! # Purity
//!
//! Nothing here touches `proc_macro2`. Diagnostics and IR nodes carry byte
//! ranges only, which is what lets the whole pass run on a thread with a large
//! stack — recursive descent over deeply nested C would otherwise overflow the
//! comparatively small stack `rustc` runs macro expansion on.
//!
//! # Errors and recovery
//!
//! Checking an expression yields `Option`: `None` means "already reported,
//! produce no further noise". A statement that cannot be checked becomes
//! [`ir::Stmt::Nop`], so analysis always reaches the end of the translation
//! unit and reports everything that is wrong with it, not just the first
//! thing. Every function whose *signature* is well formed reaches the IR even
//! when its body does not, which is what lets codegen emit stub items and keep
//! a Rust call site from producing a second, meaningless error.
//!
//! One kind of diagnostic is held back: what the *compiling toolchain* cannot
//! do — see [`crate::C_VARIADIC_SUPPORTED`] — is only reported for a program
//! that is otherwise well formed, so that a C error and a "you need a newer
//! Rust" error never compete for the same construct, and so that a broken
//! program is diagnosed identically on every toolchain.

mod decl;
mod expr;
mod init;
mod stmt;
mod types;
mod va;

use std::collections::{HashMap, HashSet};

use crate::ast;
use crate::capture::{Pos, SourceRange};
use crate::diag::{Diagnostic, Diagnostics};
use crate::ir::{
    self, ConstValue, Expr, ExprKind, FuncId, LoopId, ObjectId, Place, Program, RecordId, Storage,
    SwitchId, Ty, Types, VA_LIST_NAMES,
};
use crate::target::TargetModel;
use crate::{Options, Standard};

/// Runs semantic analysis over a parsed translation unit.
///
/// `unit_id` distinguishes this expansion from every other one in the crate;
/// see [`crate::capture::Source::unit_id`]. It ends up in every synthetic item
/// name the program needs.
///
/// The returned [`Program`] is only worth generating code from when the
/// returned [`Diagnostics`] holds no errors; it is still populated on the error
/// path so that callers can emit stub definitions for the functions whose
/// signatures were fine.
pub fn analyze(
    unit: &ast::TranslationUnit,
    options: &Options,
    unit_id: u64,
) -> (Program, Diagnostics) {
    let mut sema = Sema::new(options, unit_id);
    sema.run(unit);
    let Sema {
        mut diags,
        gate_diags,
        program,
        ..
    } = sema;
    // What the toolchain cannot compile is only worth saying about a program
    // that is otherwise right: a C error and a "you need a newer Rust" error
    // for the same construct would be two ways of saying the same thing, and
    // the first one is the one the author can act on. Holding them back also
    // keeps the diagnostics of a broken program identical on every toolchain.
    if !diags.has_errors() {
        for diag in gate_diags {
            diags.push(diag);
        }
    }
    (program, diags)
}

// ---------------------------------------------------------------------------
// symbol table
// ---------------------------------------------------------------------------

/// What a name in the ordinary namespace refers to.
#[derive(Clone, Debug)]
enum Entry {
    Object(ObjectId),
    Function(FuncId),
    Typedef(TypedefEntry),
    /// An `enum` constant, or a C23 `constexpr` object: a constant of a known
    /// type wherever it is used.
    Constant {
        value: ConstValue,
        ty: Ty,
        range: SourceRange,
    },
}

impl Entry {
    fn describe(&self) -> &'static str {
        match self {
            Entry::Object(_) => "a variable",
            Entry::Function(_) => "a function",
            Entry::Typedef(_) => "a type",
            Entry::Constant { .. } => "a constant",
        }
    }
}

/// A `typedef` name.
///
/// The type is resolved once, at the declaration, but a resolution *failure*
/// is remembered rather than reported: a `typedef` for something this
/// release cannot represent is only a problem if somebody uses it.
#[derive(Clone, Debug)]
struct TypedefEntry {
    resolved: Result<Ty, String>,
    range: SourceRange,
}

/// What a tag name refers to.
#[derive(Clone, Copy, Debug)]
enum TagEntry {
    Record(RecordId),
    Enum {
        /// The type `enum X` names: [`Ty::Enum`] for a file-scope tag that
        /// becomes a named alias, the fixed underlying type of a C23
        /// enumeration, and `int` for everything else.
        ty: Ty,
        /// Whether the enumeration's underlying type is unsigned, which is
        /// only observable through a bit-field of the type.
        unsigned: bool,
    },
}

#[derive(Default)]
struct Scope {
    entries: HashMap<String, Entry>,
}

/// What an enclosing `break` or `continue` would leave.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Breakable {
    Loop(LoopId),
    Switch(SwitchId),
}

/// A `goto` label of the function being checked.
///
/// C gives labels function scope, so they are collected before the body is
/// walked and a forward jump resolves like any other.
#[derive(Clone, Copy, Debug)]
struct Label {
    id: ir::LabelId,
    range: SourceRange,
}

/// A `switch` whose body is being checked in [CFG mode](crate::cfg).
///
/// The labels can be anywhere inside the body there, so the duplicate checking
/// the structured lowering does while it splits the body into groups happens
/// against this instead.
struct SwitchState {
    id: SwitchId,
    /// The type the controlling expression was promoted to, which every label
    /// is converted to.
    ty: Ty,
    seen: HashMap<i128, SourceRange>,
    default: Option<SourceRange>,
}

/// Why a type could not be resolved, and where to say so.
struct TypeError {
    range: SourceRange,
    message: String,
    note: Option<(SourceRange, String)>,
}

impl TypeError {
    fn at(range: SourceRange, message: impl Into<String>) -> Self {
        Self {
            range,
            message: message.into(),
            note: None,
        }
    }

    /// The placeholder that means "already reported; say nothing more".
    fn silent(range: SourceRange) -> Self {
        Self::at(range, String::new())
    }
}

// ---------------------------------------------------------------------------
// the analyser
// ---------------------------------------------------------------------------

struct Sema {
    diags: Diagnostics,
    /// Diagnostics about what the compiling toolchain cannot do, which are
    /// only reported when the program is otherwise well formed; see
    /// [`analyze`].
    gate_diags: Vec<Diagnostic>,
    /// Whether the toolchain supports `va_list` and variadic definitions.
    c_variadic: bool,
    /// Which revision the block is written in, which is what an identifier a
    /// newer one would have made a keyword is reported against.
    standard: Standard,
    /// Whether the `va_list` gate has already been reported once. One mention
    /// of the type is enough to make the point.
    va_list_gate_reported: bool,
    target: TargetModel,
    program: Program,
    scopes: Vec<Scope>,
    tags: Vec<HashMap<String, TagEntry>>,
    /// Tags keyed by the source range of the specifier that defined them.
    ///
    /// The parser resolves declarators eagerly, which means it *clones* the
    /// `struct { … }` specifier into the type of every declarator that shares
    /// it: `struct S { int x; } a, b;` arrives as two identical trees. They are
    /// one type, and the range they were written at is what says so.
    record_by_range: HashMap<(Pos, Pos), RecordId>,
    enum_by_range: HashMap<(Pos, Pos), Ty>,
    /// Whether the enumeration a specifier names has an unsigned underlying
    /// type, keyed the same way as `enum_by_range`.
    ///
    /// It decides whether a bit-field of the type sign-extends when it is read,
    /// which is the only place the choice shows; see `Sema::bit_field_signed`.
    enum_unsigned: HashMap<(Pos, Pos), bool>,
    /// Functions the unit defines, collected before anything else so that a
    /// prototype can be told from a declaration of an external symbol.
    defined_functions: HashSet<String>,
    /// Every Rust item name handed out so far, so that mangled names stay
    /// unique.
    item_names: HashSet<String>,
    /// Objects with static storage that an initialiser has already been seen
    /// for, which is what tells a tentative definition from a redefinition.
    initialized: HashSet<ObjectId>,
    /// The hidden locals the compound literals written in the block being
    /// checked need, waiting to be defined at the top of it.
    ///
    /// C99 6.5.2.5p5 gives a compound literal at block scope automatic storage
    /// duration and the lifetime of the *enclosing block*, so `&(T){…}` stays
    /// usable for the rest of that block — which a temporary inside the
    /// expression could not offer. [`Sema::block_items`] empties this list into
    /// definitions at the head of the block it belongs to.
    compound_literals: Vec<ObjectId>,
    /// Return type of the function being checked.
    ret_ty: Ty,
    /// Name of the function being checked, for diagnostics and for mangling.
    func_name: String,
    /// Whether the function being checked ends its parameter list with `...`.
    func_variadic: bool,
    /// The parameters of the function being checked, which `va_start` names
    /// one of.
    func_params: Vec<ObjectId>,
    /// The function's first `va_list` parameter, which a `va_list` local of a
    /// *non*-variadic function is copied from.
    va_param: Option<ObjectId>,
    /// Whether the function being checked is lowered through a control-flow
    /// graph, which is what `goto` and a nested `case` label need.
    cfg_mode: bool,
    /// The labels of the function being checked, collected before its body is.
    labels: HashMap<String, Label>,
    breakables: Vec<Breakable>,
    /// The `switch` statements enclosing the statement being checked, in CFG
    /// mode. Empty in the structured mode, which splits a `switch` body into
    /// groups as it walks it.
    switch_stack: Vec<SwitchState>,
    next_loop: u32,
    next_switch: u32,
    next_label: u32,
    next_anon: u32,
}

impl Sema {
    fn new(options: &Options, unit_id: u64) -> Self {
        let mut sema = Self {
            diags: Diagnostics::new(),
            gate_diags: Vec::new(),
            c_variadic: options.c_variadic,
            standard: options.standard,
            va_list_gate_reported: false,
            target: options.target,
            program: Program {
                unit_id,
                ..Program::default()
            },
            scopes: vec![Scope::default()],
            tags: vec![HashMap::new()],
            record_by_range: HashMap::new(),
            enum_by_range: HashMap::new(),
            enum_unsigned: HashMap::new(),
            defined_functions: HashSet::new(),
            item_names: HashSet::new(),
            initialized: HashSet::new(),
            compound_literals: Vec::new(),
            ret_ty: Ty::Void,
            func_name: String::new(),
            func_variadic: false,
            func_params: Vec::new(),
            va_param: None,
            cfg_mode: false,
            labels: HashMap::new(),
            breakables: Vec::new(),
            switch_stack: Vec::new(),
            next_loop: 0,
            next_switch: 0,
            next_label: 0,
            next_anon: 0,
        };
        // `__builtin_va_list` is the compiler's own name for the type; the
        // bundled `<stdarg.h>` is what turns it into `va_list`. It is
        // reserved: a program may repeat the `typedef` the header writes, but
        // not give the name a different meaning.
        for name in VA_LIST_NAMES {
            sema.scopes[0].entries.insert(
                (*name).to_owned(),
                Entry::Typedef(TypedefEntry {
                    resolved: Ok(Ty::VaList),
                    range: SourceRange::at(0),
                }),
            );
        }
        sema
    }

    fn run(&mut self, unit: &ast::TranslationUnit) {
        for item in &unit.items {
            if let ast::ExternalDecl::Function(f) = item {
                self.defined_functions.insert(f.name.name.clone());
            }
        }
        for item in &unit.items {
            match item {
                ast::ExternalDecl::Function(f) => self.function_def(f),
                ast::ExternalDecl::Decl(d) => self.file_scope_decl(d),
                ast::ExternalDecl::StaticAssert(assert) => self.static_assert(assert),
            }
        }
    }

    // -- diagnostics --------------------------------------------------------

    fn error(&mut self, range: SourceRange, message: impl Into<String>) {
        self.diags.error(range, message);
    }

    /// Records that the program needs a toolchain this one is not.
    ///
    /// The diagnostic is held back until the end of the unit; see [`analyze`].
    fn gate(&mut self, range: SourceRange, message: impl Into<String>) {
        self.gate_diags.push(Diagnostic::error(range, message));
    }

    /// Reports, once, that having a `va_list` object needs Rust 1.99.
    ///
    /// It is the *object* that needs `core::ffi::VaList`, not the name: a unit
    /// that includes `<stdarg.h>` and only calls `printf` generates nothing
    /// the older toolchain cannot compile, and code generation drops the
    /// declarations that mention the type (see `beyond_toolchain`).
    fn gate_va_list(&mut self, range: SourceRange) {
        if self.c_variadic || self.va_list_gate_reported {
            return;
        }
        self.va_list_gate_reported = true;
        self.gate(
            range,
            "'va_list' requires Rust 1.99 or later (this toolchain is older)",
        );
    }

    /// The gate diagnostic for a name a newer standard would have made a
    /// keyword, if it is one.
    ///
    /// `nullptr`, `bool` and the rest are ordinary identifiers before C23 —
    /// they have to be, or `<stdbool.h>` could not `#define bool _Bool` — so a
    /// C11 block that uses one gets "use of undeclared identifier" unless
    /// somebody says what is really going on.
    fn newer_keyword(&self, name: &str) -> Option<String> {
        let keyword = crate::lex::Keyword::from_str(name, Standard::C23)?;
        let needed = keyword.since();
        (needed > self.standard).then(|| self.standard.requires(&format!("'{name}'"), needed))
    }

    fn error_note(
        &mut self,
        range: SourceRange,
        message: impl Into<String>,
        note_range: SourceRange,
        note: impl Into<String>,
    ) {
        self.diags
            .push(Diagnostic::error(range, message).with_note_at(note_range, note));
    }

    // -- scopes -------------------------------------------------------------

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
        self.tags.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        self.tags.pop();
    }

    /// Whether the declaration being processed is at file scope.
    fn at_file_scope(&self) -> bool {
        self.scopes.len() == 1
    }

    fn lookup(&self, name: &str) -> Option<&Entry> {
        self.scopes.iter().rev().find_map(|s| s.entries.get(name))
    }

    fn declared_here(&self, name: &str) -> Option<&Entry> {
        self.scopes.last().and_then(|s| s.entries.get(name))
    }

    fn insert(&mut self, name: &str, entry: Entry) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.entries.insert(name.to_owned(), entry);
        }
    }

    /// Registers `name` in the file scope, where C gives functions their
    /// linkage regardless of where they were declared.
    fn insert_at_file_scope(&mut self, name: &str, entry: Entry) {
        if let Some(scope) = self.scopes.first_mut() {
            scope.entries.insert(name.to_owned(), entry);
        }
    }

    fn lookup_tag(&self, name: &str) -> Option<TagEntry> {
        self.tags.iter().rev().find_map(|s| s.get(name)).copied()
    }

    fn tag_here(&self, name: &str) -> Option<TagEntry> {
        self.tags.last().and_then(|s| s.get(name)).copied()
    }

    fn insert_tag(&mut self, name: &str, entry: TagEntry) {
        if let Some(scope) = self.tags.last_mut() {
            scope.insert(name.to_owned(), entry);
        }
    }

    // -- convenience over the type arena ------------------------------------

    fn types(&self) -> &Types {
        &self.program.types
    }

    fn tyname(&self, ty: Ty) -> String {
        self.program.types.name(ty)
    }

    fn pointee(&self, ty: Ty) -> Option<Ty> {
        self.program.types.pointee(ty)
    }

    fn ptr_to(&mut self, pointee: Ty, konst: bool) -> Ty {
        self.program.types.pointer(pointee, konst)
    }

    fn size_of(&self, ty: Ty) -> Option<u64> {
        self.program.types.size_of(ty, &self.target)
    }

    fn size_ty(&self) -> Ty {
        Ty::size_ty(&self.target)
    }

    // -- bit-fields ---------------------------------------------------------

    /// What makes a place a bit-field, if it is one.
    fn bit_field_of(&self, place: &Place) -> Option<&ir::BitField> {
        let ir::PlaceKind::Field { record, index, .. } = &place.kind else {
            return None;
        };
        self.types().record(*record).fields[*index].bits.as_ref()
    }

    /// The type a value takes part in arithmetic as, after the integer
    /// promotions.
    ///
    /// Reading a bit-field is the one case where the promotions do not follow
    /// from the type alone: 6.3.1.1p2 restricts the range to the field's width,
    /// so `unsigned x : 31` promotes to `int`. Nothing else can produce a value
    /// with that range, so recognising the load here is enough — the IR does
    /// not have to carry the width around.
    fn promoted(&self, expr: &Expr) -> Ty {
        if let ExprKind::Load(place) = &expr.kind
            && let Some(bits) = self.bit_field_of(place)
        {
            return expr
                .ty
                .promote_bit_field(bits.width, bits.signed, &self.target);
        }
        expr.ty.promote(&self.target)
    }

    /// The same, for the place a compound assignment computes in.
    fn promoted_place(&self, place: &Place) -> Ty {
        match self.bit_field_of(place) {
            Some(bits) => place
                .ty
                .promote_bit_field(bits.width, bits.signed, &self.target),
            None => place.ty.promote(&self.target),
        }
    }

    /// The default argument promotions, applied to a value.
    fn promoted_argument(&self, expr: &Expr) -> Ty {
        if expr.ty == Ty::Float {
            return Ty::Double;
        }
        self.promoted(expr)
    }

    // -- objects ------------------------------------------------------------

    fn new_object(
        &mut self,
        name: &str,
        ty: Ty,
        storage: Storage,
        is_const: bool,
        range: SourceRange,
    ) -> ObjectId {
        let id = ObjectId(self.program.objects.len() as u32);
        self.program.objects.push(ir::Object {
            name: name.to_owned(),
            ty,
            storage,
            is_const,
            range,
        });
        id
    }

    /// Reports a redefinition if `name` already means something in this scope.
    fn check_redefinition(&mut self, name: &ast::Ident) -> bool {
        let Some(existing) = self.declared_here(&name.name) else {
            return false;
        };
        let what = existing.describe();
        let message = match existing {
            Entry::Object(_) => format!("redefinition of '{}'", name.name),
            _ => format!("redefinition of '{}' as {what}", name.name),
        };
        let previous = match existing {
            Entry::Object(id) => self.program.object(*id).range,
            Entry::Function(id) => self.program.function(*id).range,
            Entry::Typedef(entry) => entry.range,
            Entry::Constant { range, .. } => *range,
        };
        self.error_note(
            name.range,
            message,
            previous,
            format!("previous declaration of '{}' is", name.name),
        );
        true
    }

    /// Reserves `base` as a Rust item name, or reports that it is taken.
    fn try_reserve_item_name(&mut self, base: &str) -> bool {
        self.item_names.insert(base.to_owned())
    }

    /// Reserves a unique Rust item name derived from `base`.
    fn reserve_item_name(&mut self, base: &str) -> String {
        if self.try_reserve_item_name(base) {
            return base.to_owned();
        }
        for n in 2u32.. {
            let candidate = format!("{base}_{n}");
            if self.try_reserve_item_name(&candidate) {
                return candidate;
            }
        }
        unreachable!("the loop above always terminates")
    }

    /// A name for something the source did not name, unique to this expansion.
    fn anonymous_name(&mut self, what: &str) -> String {
        let n = self.next_anon;
        self.next_anon += 1;
        let base = format!("__cinrs_{:08x}_{what}{n}", self.program.unit_id as u32);
        self.reserve_item_name(&base)
    }

    // -- zero values --------------------------------------------------------

    /// The value C initialises an object of type `ty` with when no initialiser
    /// is written.
    fn zero(&mut self, ty: Ty, range: SourceRange) -> Expr {
        match ty {
            t if t.is_floating() => Expr::new(ExprKind::Float(0.0), ty, range),
            t if t.is_integer() => Expr::int(0, ty, range),
            Ty::Array(id) => {
                let array = self.types().array_type(id);
                let elem = self.zero(array.elem, range);
                Expr::new(
                    ExprKind::ArrayRepeat {
                        value: Box::new(elem),
                        len: array.len,
                    },
                    ty,
                    range,
                )
            }
            _ => Expr::new(ExprKind::Zeroed, ty, range),
        }
    }

    fn const_to_expr(&self, value: ConstValue, ty: Ty, range: SourceRange) -> Expr {
        match value {
            ConstValue::Int(v) => Expr::int(v, ty, range),
            ConstValue::Float(v) => Expr::new(ExprKind::Float(v), ty, range),
        }
    }

    // -- constant expressions ----------------------------------------------

    fn const_eval_at(&mut self, expr: &Expr, what: &str) -> Option<ConstValue> {
        match self.const_eval(expr) {
            Some(value) => Some(value),
            None => {
                self.error(
                    expr.range,
                    format!("{what} is not a compile-time constant expression"),
                );
                None
            }
        }
    }

    /// Evaluates a constant expression over the typed IR.
    ///
    /// Because it works on the IR, every conversion it has to honour is
    /// already an explicit node; the same evaluator serves `case` labels,
    /// enumerators, array bounds and static initialisers.
    fn const_eval(&mut self, expr: &Expr) -> Option<ConstValue> {
        let target = self.target;
        match &expr.kind {
            ExprKind::Int(v) => Some(ConstValue::Int(*v)),
            ExprKind::Float(v) => Some(ConstValue::Float(*v)),
            ExprKind::Zeroed if expr.ty.is_arithmetic() => Some(if expr.ty.is_floating() {
                ConstValue::Float(0.0)
            } else {
                ConstValue::Int(0)
            }),
            ExprKind::Cast(inner) => {
                let value = self.const_eval(inner)?;
                expr.ty
                    .is_arithmetic()
                    .then(|| convert_const(value, expr.ty, &target))
            }
            ExprKind::Neg(inner) => match self.const_eval(inner)? {
                ConstValue::Int(v) => Some(ConstValue::Int(expr.ty.wrap(-v, &target))),
                ConstValue::Float(v) => Some(ConstValue::Float(-v)),
            },
            ExprKind::BitNot(inner) => match self.const_eval(inner)? {
                ConstValue::Int(v) => Some(ConstValue::Int(expr.ty.wrap(!v, &target))),
                ConstValue::Float(_) => None,
            },
            ExprKind::Binary { op, lhs, rhs } => {
                let lhs = self.const_eval(lhs)?;
                let rhs = self.const_eval(rhs)?;
                self.const_binary(*op, lhs, rhs, expr)
            }
            ExprKind::Compare { op, lhs, rhs } => {
                let lhs = self.const_eval(lhs)?;
                let rhs = self.const_eval(rhs)?;
                let result = match (lhs, rhs) {
                    (ConstValue::Int(a), ConstValue::Int(b)) => compare_values(*op, &a, &b),
                    (ConstValue::Float(a), ConstValue::Float(b)) => compare_values(*op, &a, &b),
                    _ => return None,
                };
                Some(ConstValue::Int(i128::from(result)))
            }
            ExprKind::Logical { op, lhs, rhs } => {
                let lhs = is_true(self.const_eval(lhs)?);
                let result = match (op, lhs) {
                    (ir::LogicalOp::And, false) => false,
                    (ir::LogicalOp::Or, true) => true,
                    _ => is_true(self.const_eval(rhs)?),
                };
                Some(ConstValue::Int(i128::from(result)))
            }
            ExprKind::Cond {
                cond,
                then_expr,
                else_expr,
            } => {
                if is_true(self.const_eval(cond)?) {
                    self.const_eval(then_expr)
                } else {
                    self.const_eval(else_expr)
                }
            }
            _ => None,
        }
    }

    fn const_binary(
        &mut self,
        op: ir::BinOp,
        lhs: ConstValue,
        rhs: ConstValue,
        expr: &Expr,
    ) -> Option<ConstValue> {
        use ir::BinOp;
        let target = self.target;
        if let (ConstValue::Float(a), ConstValue::Float(b)) = (lhs, rhs) {
            let value = match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div => a / b,
                _ => return None,
            };
            return Some(ConstValue::Float(round_to(value, expr.ty)));
        }
        let (ConstValue::Int(a), ConstValue::Int(b)) = (lhs, rhs) else {
            return None;
        };
        let value = match op {
            BinOp::Add => a.wrapping_add(b),
            BinOp::Sub => a.wrapping_sub(b),
            BinOp::Mul => a.wrapping_mul(b),
            BinOp::Div | BinOp::Rem => {
                if b == 0 {
                    self.error(
                        expr.range,
                        "division by zero in a constant expression".to_owned(),
                    );
                    return None;
                }
                if op == BinOp::Div { a / b } else { a % b }
            }
            BinOp::BitAnd => a & b,
            BinOp::BitXor => a ^ b,
            BinOp::BitOr => a | b,
            BinOp::Shl | BinOp::Shr => {
                let width = i128::from(expr.ty.bits(&target));
                if b < 0 || b >= width {
                    self.error(
                        expr.range,
                        format!(
                            "shift count {b} is out of range for type '{}'",
                            self.tyname(expr.ty)
                        ),
                    );
                    return None;
                }
                if op == BinOp::Shl { a << b } else { a >> b }
            }
        };
        Some(ConstValue::Int(expr.ty.wrap(value, &target)))
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// The diagnostic every place `va_list` may not appear shares.
const VA_LIST_PLACEMENT: &str = "va_list is only supported as a local variable or parameter";

/// Where a conversion is happening, which is all the phrasing of the
/// diagnostic depends on.
enum ConvContext {
    Assign,
    Init(String),
    Return,
    Argument { index: usize, func: String },
}

impl ConvContext {
    fn message(&self, from: &str, to: &str) -> String {
        match self {
            ConvContext::Assign => {
                format!("assigning to '{to}' from incompatible type '{from}'")
            }
            ConvContext::Init(name) => format!(
                "cannot initialize '{name}', of type '{to}', with an expression of type '{from}'"
            ),
            ConvContext::Return => {
                format!("returning '{from}' from a function with incompatible result type '{to}'")
            }
            ConvContext::Argument { index, func } => format!(
                "passing '{from}' to parameter {index} of '{func}', of incompatible type '{to}'"
            ),
        }
    }
}

fn compare_op(op: ast::BinaryOp) -> Option<ir::CmpOp> {
    use ir::CmpOp;
    Some(match op {
        ast::BinaryOp::Lt => CmpOp::Lt,
        ast::BinaryOp::Gt => CmpOp::Gt,
        ast::BinaryOp::Le => CmpOp::Le,
        ast::BinaryOp::Ge => CmpOp::Ge,
        ast::BinaryOp::Eq => CmpOp::Eq,
        ast::BinaryOp::Ne => CmpOp::Ne,
        _ => return None,
    })
}

fn arith_op(op: ast::BinaryOp) -> Option<ir::BinOp> {
    use ir::BinOp;
    Some(match op {
        ast::BinaryOp::Add => BinOp::Add,
        ast::BinaryOp::Sub => BinOp::Sub,
        ast::BinaryOp::Mul => BinOp::Mul,
        ast::BinaryOp::Div => BinOp::Div,
        ast::BinaryOp::Rem => BinOp::Rem,
        ast::BinaryOp::Shl => BinOp::Shl,
        ast::BinaryOp::Shr => BinOp::Shr,
        ast::BinaryOp::BitAnd => BinOp::BitAnd,
        ast::BinaryOp::BitXor => BinOp::BitXor,
        ast::BinaryOp::BitOr => BinOp::BitOr,
        _ => return None,
    })
}

fn compare_values<T: PartialOrd>(op: ir::CmpOp, a: &T, b: &T) -> bool {
    use ir::CmpOp;
    match op {
        CmpOp::Lt => a < b,
        CmpOp::Gt => a > b,
        CmpOp::Le => a <= b,
        CmpOp::Ge => a >= b,
        CmpOp::Eq => a == b,
        CmpOp::Ne => a != b,
    }
}

fn is_true(value: ConstValue) -> bool {
    match value {
        ConstValue::Int(v) => v != 0,
        ConstValue::Float(v) => v != 0.0,
    }
}

fn convert_const(value: ConstValue, to: Ty, target: &TargetModel) -> ConstValue {
    match (value, to.is_floating()) {
        (ConstValue::Int(v), false) => ConstValue::Int(to.wrap(v, target)),
        (ConstValue::Int(v), true) => ConstValue::Float(round_to(v as f64, to)),
        (ConstValue::Float(v), true) => ConstValue::Float(round_to(v, to)),
        (ConstValue::Float(v), false) => {
            if to.is_bool() {
                ConstValue::Int(i128::from(v != 0.0))
            } else {
                ConstValue::Int(to.wrap(v.trunc() as i128, target))
            }
        }
    }
}

/// Rounds a value to the precision of `ty`.
fn round_to(value: f64, ty: Ty) -> f64 {
    if ty == Ty::Float {
        value as f32 as f64
    } else {
        value
    }
}

/// Renders a `case` value the way it should read in a diagnostic.
fn render_case_value(value: i128, ty: Ty, target: &TargetModel) -> String {
    if ty.is_signed(target) {
        value.to_string()
    } else {
        (value as u128).to_string()
    }
}

/// A place that is not `const` and can therefore be assigned to.
fn place_of(kind: ir::PlaceKind, ty: Ty, is_const: bool, range: SourceRange) -> Place {
    Place {
        kind,
        ty,
        is_const,
        range,
    }
}
