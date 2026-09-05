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

mod atomics;
mod builtins;
mod decl;
mod expr;
mod init;
mod stmt;
mod types;
mod va;

pub use atomics::is_atomic_builtin;

use std::collections::{HashMap, HashSet};

use crate::Options;
use crate::ast;
use crate::capture::SourceRange;
use crate::diag::{Diagnostic, Diagnostics};
use crate::ir::{
    self, ConstValue, Expr, ExprKind, FuncId, INT128_TYPEDEF_NAMES, LoopId, ObjectId, Place,
    Program, RecordId, Signature, Storage, SwitchId, Ty, Types, VA_LIST_NAMES,
};
use crate::target::TargetModel;

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
    let mut sema = Sema::new(unit, options, unit_id);
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

/// The two rules that depend on a `#pragma cinrs`, checked once the pragmas
/// are known.
///
/// The preprocessor reads them, so they only reach the [`Program`] after
/// [`analyze`] has finished — see [`crate::expand`], which sets them and then
/// calls this. Both are about thread-local objects:
///
/// * `thread_local!` lives in `std`, and a unit that said `no_std` has none;
/// * there is no stable way to give a `thread_local!` item a C symbol, so
///   `#pragma cinrs export` cannot export one.
pub fn check_pragmas(program: &Program) -> Diagnostics {
    let mut diags = Diagnostics::new();
    if !program.no_std && !program.export {
        return diags;
    }
    for object in &program.objects {
        let Storage::ThreadLocal { exported, .. } = &object.storage else {
            continue;
        };
        if program.no_std {
            diags.error(
                object.range,
                "'_Thread_local' requires std; this unit says no_std. Rust's `thread_local!` \
                 is a `std` macro, and `core` has no thread-local storage"
                    .to_owned(),
            );
        }
        if program.export && *exported {
            diags.error(
                object.range,
                "a '_Thread_local' object cannot be exported: `#pragma cinrs export` gives an \
                 item a C symbol, and there is no stable way to give one to a `thread_local!`"
                    .to_owned(),
            );
        }
    }
    diags
}

// ---------------------------------------------------------------------------
// symbol table
// ---------------------------------------------------------------------------

/// One bound of a variably modified type, as the declaration that wrote it
/// left it behind.
#[derive(Clone, Debug)]
struct VmBound {
    /// The hidden `size_t` object the length lives in, when the context is one
    /// that gives it one; see [`BoundMode`].
    object: Option<ObjectId>,
    /// The bound, converted to `size_t` and evaluated exactly once.
    value: Expr,
}

/// What [`Sema::array_len`] does with an array bound that is not an integer
/// constant expression.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BoundMode {
    /// A declaration: the bound gets a hidden object of its own, which the
    /// type points at and which the generated code assigns where the
    /// declarator stands. This is what makes `sizeof a`, `a[i][j]` and `p + 1`
    /// computable later, from the type alone.
    Object,
    /// A type name: the bound is evaluated where it is written and the
    /// expression is handed back, but there is no object to keep it in and
    /// nothing later can ask for it. `sizeof(int[n][m])` is the one context
    /// that needs it (C99 6.5.3.4p2).
    Expression,
    /// A parameter's type in a declaration that is not a definition: the bound
    /// is checked and thrown away, because C99 6.7.5.3p7 keeps it out of the
    /// type and there is no moment at which it could be evaluated.
    Unevaluated,
}

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
        /// Whether an enumerator list has been seen. GNU allows `enum e;`
        /// before one, which C99 does not have at all.
        complete: bool,
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
    /// The label values already claimed. A [range](ir::CaseRange) can overlap
    /// another one, so this is a list rather than a set.
    seen: Vec<(ir::CaseRange, SourceRange)>,
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

struct Sema<'a> {
    /// The unit being analysed, which the tag specifiers a [`ast::Type`] names
    /// are looked up in; see [`ast::RecordSpecId`].
    unit: &'a ast::TranslationUnit,
    diags: Diagnostics,
    /// Diagnostics about what the compiling toolchain cannot do, which are
    /// only reported when the program is otherwise well formed; see
    /// [`analyze`].
    gate_diags: Vec<Diagnostic>,
    /// Whether the toolchain supports `va_list` and variadic definitions.
    c_variadic: bool,
    /// Which revision the block is written in, and whether the GNU extensions
    /// are on; an identifier another entry point would have made a keyword is
    /// reported against it.
    gating: crate::Gating,
    /// Whether the `va_list` gate has already been reported once. One mention
    /// of the type is enough to make the point.
    va_list_gate_reported: bool,
    target: TargetModel,
    program: Program,
    scopes: Vec<Scope>,
    tags: Vec<HashMap<String, TagEntry>>,
    /// The tag each `struct`/`union` specifier resolved to, by its id.
    ///
    /// The parser resolves declarators eagerly, so one specifier is named by
    /// several types: `struct S { int x; } a, b;` is the declaration's
    /// specifiers plus the type of each declarator, and all three carry the
    /// same [`ast::RecordSpecId`]. They are one type, and this is what says so.
    record_by_spec: Vec<Option<RecordId>>,
    /// The same, for the type an `enum` specifier resolved to.
    enum_by_spec: Vec<Option<Ty>>,
    /// Whether the enumeration a specifier names has an unsigned underlying
    /// type, indexed the same way as `enum_by_spec`.
    ///
    /// It decides whether a bit-field of the type sign-extends when it is read,
    /// which is the only place the choice shows; see `Sema::bit_field_signed`.
    enum_unsigned: Vec<Option<bool>>,
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
    /// The bounds the array type being resolved was given, in the order they
    /// were resolved (innermost dimension first), for the ones that were not
    /// constant expressions.
    ///
    /// `Sema::array_len` leaves them here, already converted to `size_t`, and
    /// whoever asked for the type takes them: a declaration binds each one to
    /// the hidden object its dimension names and evaluates it exactly once,
    /// where the declarator stands (C99 6.7.5.2p5), while `sizeof` of a type
    /// name evaluates them in place (6.5.3.4p2).
    vm_bounds: Vec<VmBound>,
    /// What [`Sema::array_len`] does with a bound that is not a constant.
    bound_mode: BoundMode,
    /// The name the hidden bound objects of the declarator being resolved are
    /// derived from; empty for a type name or an abstract declarator.
    vm_name: String,
    /// How many of [`Sema::scopes`] are [prototype
    /// scopes](Sema::push_prototype_scope), which are not blocks.
    proto_depth: usize,
    /// Whether the type being resolved is a *parameter's*.
    ///
    /// A parameter's array bound is not part of its type at all (C99
    /// 6.7.5.3p7): `void f(int n, int a[n])` declares an `int *`, and the
    /// bound may be anything even in a prototype at file scope, where an
    /// object's could not be. The *inner* dimensions of `void f(int n, double
    /// a[3][n])` are part of it — the parameter is a pointer to a variably
    /// modified type — and it is a definition, where the bounds are evaluated
    /// on entry (6.9.1p10), that gives them objects to live in.
    in_param_type: bool,
    /// The variable length arrays whose scope encloses the statement being
    /// checked, innermost last.
    ///
    /// C99 6.8.6.1p1 forbids a `goto` from outside the scope of an identifier
    /// with a variably modified type to inside it, and 6.8.4.2p2 says the same
    /// of a `switch`; comparing this list at the jump with the one recorded at
    /// its target is what answers both.
    vla_scopes: Vec<ObjectId>,
    /// The variable length arrays in scope where each label was written.
    label_vla_scopes: HashMap<ir::LabelId, Vec<ObjectId>>,
    /// The `goto`s of the function being checked, with the scopes they jump
    /// from, checked once the whole body has been walked — a forward jump
    /// names a label that has not been reached yet.
    goto_scopes: Vec<(SourceRange, ir::LabelId, Vec<ObjectId>)>,
    /// The depth [`Sema::vla_scopes`] had when each enclosing `switch` began,
    /// which is what a `case` label deeper than that would jump past.
    switch_vla_depths: Vec<usize>,
    /// Whether the function being checked calls `alloca`.
    func_uses_alloca: bool,
    /// Operands of the atomic builtin being checked whose value is not used
    /// but which C still evaluates: a memory order that was not a constant
    /// expression, and a `__sync_*` builtin's trailing arguments.
    ///
    /// [`Sema::atomic_builtin`] empties it into the comma operators in front
    /// of the node it builds, so it is never carried from one call to the
    /// next.
    pending_discard: Vec<Expr>,
    /// How many `cleanup` attributes are active in the scopes enclosing the
    /// statement being checked.
    ///
    /// Only one thing depends on it: a `return expr;` in [CFG
    /// mode](crate::cfg) has to compute the value *before* the cleanups run
    /// (GCC's order), and the temporary that says so is only worth emitting
    /// when there is a cleanup owed at all.
    cleanup_depth: usize,
    /// The file-scope compound literals, by the `statics` entry holding their
    /// value.
    ///
    /// A literal written where a constant expression has to go — inside
    /// another object's initialiser — is the value it was written with, and
    /// this is what lets `static_init` reach it. (c-testsuite `00216`.)
    static_literals: HashMap<ObjectId, usize>,
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

impl<'a> Sema<'a> {
    fn new(unit: &'a ast::TranslationUnit, options: &Options, unit_id: u64) -> Self {
        let mut sema = Self {
            unit,
            diags: Diagnostics::new(),
            gate_diags: Vec::new(),
            c_variadic: options.c_variadic,
            gating: options.gating(),
            va_list_gate_reported: false,
            target: options.target,
            program: Program {
                unit_id,
                ..Program::default()
            },
            scopes: vec![Scope::default()],
            tags: vec![HashMap::new()],
            record_by_spec: vec![None; unit.records.len()],
            enum_by_spec: vec![None; unit.enums.len()],
            enum_unsigned: vec![None; unit.enums.len()],
            defined_functions: HashSet::new(),
            item_names: HashSet::new(),
            initialized: HashSet::new(),
            compound_literals: Vec::new(),
            vm_bounds: Vec::new(),
            bound_mode: BoundMode::Expression,
            vm_name: String::new(),
            proto_depth: 0,
            in_param_type: false,
            vla_scopes: Vec::new(),
            label_vla_scopes: HashMap::new(),
            goto_scopes: Vec::new(),
            switch_vla_depths: Vec::new(),
            func_uses_alloca: false,
            pending_discard: Vec::new(),
            cleanup_depth: 0,
            static_literals: HashMap::new(),
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
        // GCC's own names for the two 128-bit integer types, predefined in
        // every mode beside the `__int128` keyword itself.
        for (name, ty) in INT128_TYPEDEF_NAMES {
            sema.scopes[0].entries.insert(
                (*name).to_owned(),
                Entry::Typedef(TypedefEntry {
                    resolved: Ok(*ty),
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
        self.complete_tentative_arrays();
    }

    /// C99 6.9.2p5: a tentative definition whose type is still an incomplete
    /// array type at the end of the translation unit becomes a definition of
    /// an array of *one* element.
    ///
    /// `int j[];` on its own is that; `int j[]; int j[3];` is not, because the
    /// second declaration completed the type long before here. An `extern`
    /// declaration is not a tentative definition and keeps its incomplete
    /// type: the object is somebody else's, and only its address is ever used.
    fn complete_tentative_arrays(&mut self) {
        for index in 0..self.program.statics.len() {
            let id = self.program.statics[index].object;
            let ty = self.program.object(id).ty;
            let Some(completed) = self.program.types.complete_tentative_array(ty) else {
                continue;
            };
            self.program.objects[id.0 as usize].ty = completed;
            let range = self.program.object(id).range;
            self.program.statics[index].init = self.zero(completed, range);
        }
    }

    // -- diagnostics --------------------------------------------------------

    fn error(&mut self, range: SourceRange, message: impl Into<String>) {
        self.diags.error(range, message);
    }

    /// Reports a construct a newer revision than this block's introduced.
    ///
    /// The parser gates what it can see for itself; this is for the ones only
    /// sema can tell apart — a variable length array is an array declarator
    /// until the bound turns out not to be a constant.
    fn require_standard(&mut self, needed: crate::Standard, what: &str, range: SourceRange) {
        if let Some(message) = self.gating.requires(what, needed) {
            self.error(range, message);
        }
    }

    /// Whether this block has the GNU leniencies.
    ///
    /// These are the handful of places where GCC takes a constraint violation
    /// as a warning and carries on, and where refusing valid-in-practice C
    /// would be worse than following it: `return expr;` in a `void` function,
    /// a comparison of two incompatible function pointers, `sizeof (void)`, a
    /// stray `;` at file scope, an undeclared `alloca`, an enumerator too wide
    /// for `int` before C23. Each of them is a
    /// [`Standard`](crate::Standard)-independent *dialect* question, which is
    /// why it is not [`Sema::require_standard`]; `doc/gnu-extensions.md` has
    /// the list.
    fn gnu_leniency(&self) -> bool {
        self.gating.dialect.is_gnu()
    }

    /// The note that names the entry point which would have accepted what
    /// [`Sema::gnu_leniency`] just refused.
    fn gnu_note(&self) -> String {
        format!(
            "GCC accepts this with a warning; write {} for the same leniency",
            self.gating.standard.macro_name_in(crate::Dialect::Gnu)
        )
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
        self.gating.newer_keyword(name)
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

    /// Enters C99 6.2.1p4's *function prototype scope*: the parameter names of
    /// a parameter list, visible to the declarators that follow them and
    /// nowhere else.
    ///
    /// Only the ordinary namespace gets a scope of its own — a tag first
    /// mentioned in a parameter list is left where this crate has always put
    /// it — and the scope does not count as a block, so a compound literal or
    /// an array bound written in the list is still at file scope when the
    /// declaration is.
    fn push_prototype_scope(&mut self) {
        self.scopes.push(Scope::default());
        self.proto_depth += 1;
    }

    fn pop_prototype_scope(&mut self) {
        self.scopes.pop();
        self.proto_depth -= 1;
    }

    /// Whether the declaration being processed is at file scope.
    fn at_file_scope(&self) -> bool {
        self.scopes.len() - self.proto_depth == 1
    }

    fn lookup(&self, name: &str) -> Option<&Entry> {
        self.scopes.iter().rev().find_map(|s| s.entries.get(name))
    }

    fn declared_here(&self, name: &str) -> Option<&Entry> {
        self.scopes.last().and_then(|s| s.entries.get(name))
    }

    /// What `name` denotes at file scope, which is where every declaration
    /// that has *linkage* is registered.
    ///
    /// C99 6.2.2p4 makes an `extern` declaration name the linked object even
    /// when a block-scope declaration of the same name is in scope: a prior
    /// declaration only decides the linkage when it has linkage itself, and a
    /// block-scope object declared without `extern` has none. Since every
    /// declaration that does have linkage is also entered here — that is what
    /// [`Sema::insert_at_file_scope`] is for — asking the file scope is
    /// exactly asking "is a declaration with linkage visible?".
    fn lookup_linked(&self, name: &str) -> Option<&Entry> {
        self.scopes.first().and_then(|s| s.entries.get(name))
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

    // -- tag specifiers -----------------------------------------------------

    /// The `struct`/`union` specifier a type names.
    ///
    /// The borrow is of the *unit*, not of `self`, so a caller may keep it
    /// while it goes on analysing.
    fn record_spec(&self, id: ast::RecordSpecId) -> &'a ast::RecordSpec {
        self.unit.record(id)
    }

    /// The `enum` specifier a type names.
    fn enum_spec(&self, id: ast::EnumSpecId) -> &'a ast::EnumSpec {
        self.unit.enum_spec(id)
    }

    /// The operand of a `typeof` specifier.
    fn typeof_operand(&self, id: ast::TypeofId) -> &'a ast::TypeofOperand {
        self.unit.typeof_operand(id)
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
        // An array type carries its qualifiers on its *elements* (6.7.3p9), so
        // `const double (*)[m]` is a pointer to an array whose elements are
        // const — and the pointer is the one that has to say so, because that
        // is what the generated `*const` is.
        let konst =
            konst || matches!(pointee, Ty::Array(id) if self.types().array_type(id).elem_const);
        self.program.types.pointer(pointee, konst)
    }

    fn size_of(&self, ty: Ty) -> Option<u64> {
        self.program.types.size_of(ty, &self.target)
    }

    fn size_ty(&self) -> Ty {
        Ty::size_ty(&self.target)
    }

    /// Whether a function type may be written without a prototype here.
    ///
    /// C23 removed the form (N2841): `int f()` means `int f(void)` there, and
    /// in a `c23!` or `gnu23!` block it does. Every earlier revision — ISO and
    /// GNU alike — reads it as "the parameters are unspecified", which is what
    /// C99 6.7.5.3p14 says and what a program written before 2023 means by it.
    fn empty_list_is_unprototyped(&self) -> bool {
        self.gating.standard < crate::Standard::C23
    }

    /// Whether an `ast::FunctionType` carries a prototype *here*.
    fn is_prototyped(&self, func: &ast::FunctionType) -> bool {
        func.has_prototype || !self.empty_list_is_unprototyped()
    }

    // -- type compatibility -------------------------------------------------

    /// C's type compatibility (C99 6.2.7), as far as it differs from type
    /// *identity*.
    ///
    /// The arena hash-conses every derived type and [`Ty`] carries no top-level
    /// qualifiers, so `a == b` already answers the question for everything but
    /// the one case C23 removed: a function type with no prototype
    /// (6.7.5.3p15). Keeping the difference this narrow is deliberate — an
    /// enumerated type is compatible with an implementation-defined integer
    /// type too, and opening that here would change what `_Generic` selects.
    fn compatible(&self, a: Ty, b: Ty) -> bool {
        if a == b {
            return true;
        }
        match (a, b) {
            (Ty::Func(x), Ty::Func(y)) => self.func_compatible(x, y),
            (Ty::Pointer(x), Ty::Pointer(y)) => {
                let (x, y) = (self.types().pointer_type(x), self.types().pointer_type(y));
                x.konst == y.konst && self.compatible(x.pointee, y.pointee)
            }
            // C99 6.7.5.2p6: compatible element types, and equal sizes only
            // when *both* have one. `extern int j[]; int j[3];` declares one
            // object, and `__builtin_types_compatible_p(int[5], int[])` is
            // one of GCC's own torture cases.
            (Ty::Array(x), Ty::Array(y)) => {
                let (x, y) = (self.types().array_type(x), self.types().array_type(y));
                x.elem_const == y.elem_const
                    && x.vla == y.vla
                    && self.compatible(x.elem, y.elem)
                    && (x.incomplete || y.incomplete || x.len == y.len)
            }
            _ => false,
        }
    }

    /// C99 6.7.5.3p15 for two function types.
    ///
    /// One with a prototype and one without are compatible when the prototyped
    /// one is not variadic and no parameter type is changed by the default
    /// argument promotions — which is exactly the condition under which a call
    /// through the unprototyped type reaches the other one's parameters
    /// unharmed. GCC enforces the same rule, and says so
    /// ("an argument type that has a default promotion cannot match an empty
    /// parameter name list declaration").
    fn func_compatible(&self, x: ir::FuncTyId, y: ir::FuncTyId) -> bool {
        let (x, y) = (self.types().func_type(x), self.types().func_type(y));
        if !self.compatible(x.ret, y.ret) {
            return false;
        }
        match (x.prototyped, y.prototyped) {
            (false, false) => true,
            (true, true) => {
                x.variadic == y.variadic
                    && x.params.len() == y.params.len()
                    && x.params
                        .iter()
                        .zip(&y.params)
                        .all(|(a, b)| self.compatible(*a, *b))
            }
            _ => {
                let prototyped = if x.prototyped { x } else { y };
                !prototyped.variadic
                    && prototyped
                        .params
                        .iter()
                        .all(|p| *p == p.promote_argument(&self.target))
            }
        }
    }

    /// The composite of two declarations of one function (C99 6.2.7p3), or
    /// `None` when the two are not compatible at all.
    ///
    /// A prototype wins over an empty parameter list, so
    /// `int f(); int f(int);` leaves `f` with the prototype and a later call is
    /// checked against it.
    fn composite_signature(&self, a: &Signature, b: &Signature) -> Option<Signature> {
        if a == b {
            return Some(a.clone());
        }
        if !self.compatible(a.ret, b.ret) {
            return None;
        }
        match (a.prototyped, b.prototyped) {
            // Two empty lists say the same nothing, and the return types
            // already agree.
            (false, false) => Some(a.clone()),
            // Two prototypes agree when they agree parameter by parameter,
            // which is not quite `==`: a parameter may itself be a pointer to
            // a function with no prototype.
            (true, true) => {
                let same = a.variadic == b.variadic
                    && a.params.len() == b.params.len()
                    && a.params
                        .iter()
                        .zip(&b.params)
                        .all(|(x, y)| self.compatible(*x, *y));
                same.then(|| a.clone())
            }
            // 6.7.5.3p15, and the composite type is the prototype's.
            _ => {
                let prototyped = if a.prototyped { a } else { b };
                let ok = !prototyped.variadic
                    && prototyped
                        .params
                        .iter()
                        .all(|p| *p == p.promote_argument(&self.target));
                ok.then(|| prototyped.clone())
            }
        }
    }

    // -- bit-fields ---------------------------------------------------------

    /// What makes a place a bit-field, if it is one.
    fn bit_field_of(&self, place: &Place) -> Option<&ir::BitField> {
        let ir::PlaceKind::Field { record, index, .. } = &place.kind else {
            return None;
        };
        self.types().record(*record).fields[*index].bits.as_ref()
    }

    /// Whether reading this expression read a bit-field.
    ///
    /// The value of a bit-field is the one thing whose range is narrower than
    /// its type's, which is what makes both the width-restricted promotions
    /// and a cast to the field's own declared type observable.
    pub(super) fn is_bit_field_load(&self, expr: &Expr) -> bool {
        matches!(&expr.kind, ExprKind::Load(place) if self.bit_field_of(place).is_some())
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

    /// The width an integer value is really reduced to, when that is narrower
    /// than its type.
    ///
    /// This is a bit-field the integer promotions do not reach: `unsigned long
    /// long b : 40` stays `unsigned long long` through 6.3.1.1p2, but 40 bits
    /// is the precision C99 6.7.2.1p10 gives the value and GCC computes with.
    /// It rides along on [`ir::Expr::bits`] once an operator has produced one,
    /// so a chain of them stays in the same precision.
    fn narrow_bits(&self, expr: &Expr) -> Option<u32> {
        if let Some(bits) = expr.bits {
            return Some(bits);
        }
        let ExprKind::Load(place) = &expr.kind else {
            return None;
        };
        let field = self.bit_field_of(place)?;
        let promoted = expr
            .ty
            .promote_bit_field(field.width, field.signed, &self.target);
        (promoted == expr.ty && field.width < expr.ty.bits(&self.target)).then_some(field.width)
    }

    /// The precision an operand takes part in arithmetic with: a narrow
    /// bit-field's width, and the full width of the promoted type otherwise.
    fn operand_bits(&self, expr: &Expr) -> u32 {
        if let Some(bits) = self.narrow_bits(expr) {
            return bits;
        }
        let ty = self.promoted(expr);
        if ty.is_integer() {
            ty.bits(&self.target)
        } else {
            0
        }
    }

    /// The precision the result of an operation on `operands` has, given the
    /// type the usual arithmetic conversions chose for it.
    ///
    /// `None` unless one of the operands really is narrower than `common`:
    /// GCC's `c_common_type` picks the operand type of greater precision, so
    /// a 40-bit bit-field against an `int` computes in forty bits and against
    /// an `unsigned long long` in sixty-four.
    fn result_bits(&self, common: Ty, operands: [&Expr; 2]) -> Option<u32> {
        if !common.is_integer() || !operands.iter().any(|e| self.narrow_bits(e).is_some()) {
            return None;
        }
        let bits = operands.map(|e| self.operand_bits(e)).into_iter().max()?;
        (bits < common.bits(&self.target)).then_some(bits)
    }

    /// The same, for the place a compound assignment computes in.
    fn promoted_place(&self, place: &Place) -> Ty {
        match self.bit_field_of(place) {
            Some(bits) => place
                .ty
                .promote_bit_field(bits.width, bits.signed, &self.target),
            // Reading the place is what is promoted, and reading an `_Atomic`
            // object gives the underlying type (C11 6.3.2.1p2).
            None => self.types().unatomic(place.ty).promote(&self.target),
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
            vla_storage: false,
            asm_label: None,
            section: None,
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
            // The zero of an `_Atomic T` is a `T`: a value never has an atomic
            // type, and initialising the object is a plain write (7.17.2.1).
            Ty::Atomic(id) => {
                let inner = self.types().atomic_inner(id);
                self.zero(inner, range)
            }
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
                    .then(|| convert_const(value, inner.ty, expr.ty, &target))
            }
            ExprKind::Neg(inner) => match self.const_eval(inner)? {
                // Wrapping, because `-(-2^127)` is not an `i128` and the
                // 128-bit types can reach it: C wraps there, and negating in
                // Rust would abort the macro instead.
                ConstValue::Int(v) => {
                    Some(ConstValue::Int(expr.ty.wrap(v.wrapping_neg(), &target)))
                }
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
                // Both operands have already been converted to their common
                // type, so either one says how the pair is ordered.
                let operand_ty = lhs.ty;
                let lhs = self.const_eval(lhs)?;
                let rhs = self.const_eval(rhs)?;
                let result = match (lhs, rhs) {
                    // An `unsigned __int128` above `i128::MAX` is carried as
                    // its bit pattern, which orders wrongly as a signed value;
                    // every narrower unsigned type's constant is its own
                    // non-negative value and needs nothing.
                    (ConstValue::Int(a), ConstValue::Int(b)) if unsigned_128(operand_ty) => {
                        compare_values(*op, &(a as u128), &(b as u128))
                    }
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
                // `unsigned __int128` is the one type whose constants are
                // carried as a bit pattern rather than as a value, so the
                // quotient and the remainder have to be taken unsigned.
                if unsigned_128(expr.ty) {
                    let (a, b) = (a as u128, b as u128);
                    let value = if op == BinOp::Div { a / b } else { a % b };
                    return Some(ConstValue::Int(value as i128));
                }
                // Wrapping for the same reason `-` is: `-2^127 / -1` is the
                // one signed division that leaves the type, and `__int128` is
                // the type that can write it.
                if op == BinOp::Div {
                    a.wrapping_div(b)
                } else {
                    a.wrapping_rem(b)
                }
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
                // A right shift of an `unsigned __int128` is logical, and
                // shifting the bit pattern as an `i128` would bring the sign
                // bit down instead.
                if op == BinOp::Shr && unsigned_128(expr.ty) {
                    return Some(ConstValue::Int(((a as u128) >> b) as i128));
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

/// C99 6.8.6.1p1 for `goto` and 6.8.4.2p2 for `switch`, which say the same
/// thing: the storage a variable length array's declaration allocates is not
/// there if the declaration was jumped over.
const JUMP_INTO_VM_SCOPE: &str = "jump into the scope of an identifier with variably modified type";

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

/// Whether constants of this type are carried as a bit pattern rather than as
/// a value; see [`Ty::wrap`](crate::ir::Ty::wrap).
///
/// `unsigned __int128` is the only one: every other type this models has all
/// of its values inside an `i128`, and the signedness of the two 128-bit types
/// does not depend on the target.
fn unsigned_128(ty: Ty) -> bool {
    ty == Ty::UInt128
}

fn is_true(value: ConstValue) -> bool {
    match value {
        ConstValue::Int(v) => v != 0,
        ConstValue::Float(v) => v != 0.0,
    }
}

fn convert_const(value: ConstValue, from: Ty, to: Ty, target: &TargetModel) -> ConstValue {
    match (value, to.is_floating()) {
        (ConstValue::Int(v), false) => ConstValue::Int(to.wrap(v, target)),
        // An `unsigned __int128` carried as a bit pattern is that many, not
        // the negative number the same bits read as an `i128`.
        (ConstValue::Int(v), true) if unsigned_128(from) => {
            ConstValue::Float(round_to(v as u128 as f64, to))
        }
        (ConstValue::Int(v), true) => ConstValue::Float(round_to(v as f64, to)),
        (ConstValue::Float(v), true) => ConstValue::Float(round_to(v, to)),
        (ConstValue::Float(v), false) => {
            if to.is_bool() {
                ConstValue::Int(i128::from(v != 0.0))
            } else if unsigned_128(to) {
                // The bit pattern of the `u128` the value really converts to;
                // `as i128` would saturate at `i128::MAX` instead.
                ConstValue::Int(v.trunc() as u128 as i128)
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
