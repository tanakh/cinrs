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
//! # Nested functions
//!
//! GNU's nested function definitions are the one construct this pass does not
//! merely *check* but rewrites: a definition written inside another function's
//! body is **lambda-lifted** into a file-scope [`ir::Function`] of its own.
//!
//! The body is checked where it stands, in a scope nested in the enclosing
//! function's, so C's own scoping decides what it can see. `Sema::nest` is the
//! stack of levels that says which function is being checked, and
//! `Sema::object_level` records the level each object was created at, so a
//! name that resolves to an *enclosing* function's automatic object is
//! recognised as a **capture**. Each captured object becomes a hidden pointer
//! parameter of the lifted function — an [`ir::EnvParam`] — and the reference
//! becomes the place `*ptr`, which is what keeps assignment, `&x`, `x++`,
//! subscripting, member access and `sizeof x` meaning what C says and keeps
//! the object *shared* with the enclosing function rather than copied. A level
//! in between takes the pointer whether it uses the object or not, so that it
//! can pass it on.
//!
//! A call to a nested function needs the addresses of whatever the callee
//! captures, which the caller may itself only have as a pointer. What the
//! callee captures is not always known at the call site — GNU's
//! `auto int g(int);` exists so that two nested functions can call each other
//! — so the call *edges* are recorded and `Sema::propagate_nested_env` closes
//! over them at the end of the unit; [`crate::codegen`] then writes the
//! arguments out.
//!
//! What is refused is what a lifting cannot express: the **address** of a
//! nested function that captures (GCC writes a trampoline onto the stack for
//! it), a **nonlocal `goto`**, and capturing a variable length array or a
//! `va_list`.
//!
//! # Safe functions
//!
//! [`check_safe`] is the pass that runs *after* this one, once the
//! preprocessor's pragmas have reached the [`ir::Program`]: it applies
//! `#pragma cinrs safe`, refuses the three shapes that could never be safe,
//! and reports a safe function calling one of this unit's own that is not.
//! What it needs from here is the call graph, which `Sema::call` records as it
//! checks each direct call — see [`ir::CallEdge`] — and the
//! `[[cinrs::safe]]` attribute, which `Sema::declare_function` puts on the
//! [`ir::Function`]. Everything else about a safe function is `rustc`'s to
//! check, since [`crate::codegen`] simply leaves the `unsafe` out.
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

mod asm;
mod atomics;
mod builtins;
mod decl;
mod expr;
mod init;
mod stmt;
mod types;
mod va;
mod vector;

pub use atomics::is_atomic_builtin;

use std::collections::{HashMap, HashSet};

use crate::Options;
use crate::ast;
use crate::capture::SourceRange;
use crate::diag::{Diagnostic, Diagnostics};
use crate::ir::{
    self, ConstValue, Expr, ExprKind, FuncId, INT128_TYPEDEF_NAMES, LoopId, ObjectId, Place,
    PlaceKind, Program, RecordId, Signature, Storage, SwitchId, Ty, Types, VA_LIST_NAMES,
    X86_VECTOR_TYPEDEF_NAMES,
};
use crate::target::{Arch, TargetModel};

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
    if program.no_std {
        for range in &program.cpu_supports {
            diags.error(
                *range,
                "'__builtin_cpu_supports' requires std; this unit says no_std. It becomes \
                 `std::is_x86_feature_detected!`, and `core` has no processor detection — \
                 `cpuid` is not something a library can do without one. Ask for the \
                 instruction set with __attribute__((target(\"…\"))) and let the caller \
                 guarantee it, or drop the no_std pragma"
                    .to_owned(),
            );
        }
    }
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

/// `'a', 'b' and 'c'`, for a diagnostic that lists the names it knows.
pub(super) fn list_of_names(names: &[&str]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| format!("'{n}'")).collect();
    match quoted.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// Applies `#pragma cinrs safe` and checks every function that is to be
/// generated without `unsafe`.
///
/// A safe function is an ordinary `extern "C" fn` whose body is not wrapped in
/// an `unsafe` block, so `rustc` checks it: a raw pointer dereference, a call
/// to a function that is not safe, a read of a C global or of a `union` member
/// — every unsafe operation the translation of the C needs — is an error with
/// the caret on the C that asked for it. This function does the part `rustc`
/// cannot do:
///
/// * the names `#pragma cinrs safe` gave are looked up, since only the program
///   knows what the unit defines, and one that names nothing is reported;
/// * the three shapes that could never be safe are refused where they were
///   written rather than becoming a puzzle from `rustc`: a function this unit
///   only *declares* (there is no body to check, and the call is a foreign one
///   whatever the attribute says), a *variadic* definition (Rust makes every
///   C-variadic function unsafe) and a GNU *nested* function (it reaches the
///   enclosing frame through hidden pointer parameters, which its body
///   dereferences);
/// * a call to a function of this unit that is not safe is reported in C's own
///   words, naming the callee and the attribute that would fix it — `rustc`
///   would say "call to unsafe function is unsafe", which is true and says
///   nothing about the C.
///
/// A call to a function the unit only declares — `printf`, or anything else the
/// program links against — is deliberately left to `rustc`: nothing here can
/// make a foreign function safe, and its message says exactly that.
pub fn check_safe(program: &mut Program, named: &[crate::pp::SafeName]) -> Diagnostics {
    let mut diags = Diagnostics::new();
    for request in named {
        // A lifted nested function keeps the name the program gave it, but it
        // is not what a pragma naming a function of the unit means; the item
        // name it was given is what tells the two apart.
        let found = program
            .functions
            .iter_mut()
            .find(|func| func.name == request.name && !func.is_nested());
        match found {
            Some(func) => func.safe = func.safe.or(Some(request.range)),
            None => diags.error(
                request.range,
                format!(
                    "#pragma cinrs safe names '{}', which this unit does not declare",
                    request.name
                ),
            ),
        }
    }

    // A refused request is *taken back*, not only reported: the item this unit
    // still generates for the function — a stub, since the unit no longer
    // compiles — has to be one `rustc` accepts, and `extern "C" fn f(n: c_int,
    // ...)` is not, so a second error about the same mistake would follow the
    // first. The request is what was wrong; the ordinary `unsafe` item is what
    // stands in its place.
    let mut refused: HashSet<usize> = HashSet::new();
    for (index, func) in program.functions.iter_mut().enumerate() {
        let Some(range) = func.safe else { continue };
        let refusal = if func.body.is_none() {
            format!(
                "'{}' is only declared here, so it cannot be safe: an extern function is \
                 compiled elsewhere and nothing about it can be checked",
                func.name
            )
        } else if func.sig.variadic {
            format!(
                "'{}' takes '...', so it cannot be safe: Rust makes every function with a \
                 C variable argument list unsafe",
                func.name
            )
        } else if func.is_nested() {
            format!(
                "'{}' is a nested function, so it cannot be safe: it reaches the enclosing \
                 function's objects through hidden pointer parameters, which its body \
                 dereferences",
                func.name
            )
        } else if let Some(feature) = func.target_features.first() {
            // `#[target_feature]` makes a function unsafe to *call* from
            // anywhere that does not have the same instruction set — even a
            // function Rust would otherwise call safely — which is exactly the
            // promise `[[cinrs::safe]]` makes. The two cannot both hold.
            format!(
                "'{}' asks for the '{feature}' instruction set, so it cannot be safe: Rust makes \
                 a '#[target_feature]' function unsafe to call from anywhere that does not have \
                 that instruction set, which is the opposite of what [[cinrs::safe]] promises. \
                 Whether the processor really has it is something only the caller knows — \
                 '__builtin_cpu_supports' is how to ask",
                func.name
            )
        } else {
            continue;
        };
        func.safe = None;
        refused.insert(index);
        diags.error(range, refusal);
    }

    // The one check worth making here rather than leaving to `rustc`: a call
    // between two functions of this unit, where the C names both. A callee
    // whose own request was just refused is left out of it — "mark it
    // [[cinrs::safe]]" is no advice for a function that was marked and told
    // why it could not be.
    for call in &program.calls {
        let caller = program.function(call.caller);
        let callee = program.function(call.callee);
        // A caller whose own request was refused above had it *taken back*, so
        // `is_safe` is already false for it and this is the only test needed.
        if !caller.is_safe() {
            continue;
        }
        // An [x86 intrinsic](crate::x86) is a `#[target_feature]` function, so
        // calling one needs either an `unsafe` block — which a safe function
        // does not have — or a caller that has the instruction set, which a
        // safe function cannot ask for (see the refusal above). `rustc` would
        // say so itself, at the caret on the C; saying it here names the
        // instruction set and what to do about it.
        if let Some(intr) = callee.intrinsic {
            // A comma-separated list — `gfni,avx512bw,avx512vl` — is read
            // out as the instruction sets it names.
            let feature = crate::x86::describe_features(intr.feature);
            diags.error(
                call.range,
                format!(
                    "'{}' needs {feature}, so it cannot be called from the safe function '{}': \
                     Rust makes every '#[target_feature]' function unsafe to call, because only \
                     the program knows whether the processor running it has those instructions. \
                     Drop [[cinrs::safe]] from '{}' and let its Rust caller write 'unsafe'",
                    intr.name, caller.name, caller.name
                ),
            );
            continue;
        }
        if callee.is_safe() || callee.is_extern() || refused.contains(&(call.callee.0 as usize)) {
            continue;
        }
        diags.error(
            call.range,
            format!(
                "function '{}' is not safe; mark it [[cinrs::safe]] or call it from a non-safe \
                 function",
                callee.name
            ),
        );
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
        /// Whether the type above was *written* — C23's fixed underlying type
        /// (N3030) — rather than the `int` this crate gives an enumeration
        /// that has none.
        ///
        /// Every declaration of one tag has to agree about it and about the
        /// type: `enum E : short; enum E : long { … };` is a constraint
        /// violation, and so is a fixed type on one declaration and none on
        /// another.
        fixed: bool,
        /// Whether an enumerator list has been seen. GNU allows `enum e;`
        /// before one, which C99 does not have at all.
        complete: bool,
        /// The list that was seen, as an index into [`Sema::enum_lists`], for
        /// the C23 rule that a second definition of one tag in one scope
        /// declares the same type when its enumerators agree (N3037).
        ///
        /// `None` until the list is seen, and `None` for ever in the entry
        /// points that have no such rule — nothing else reads it, so nothing
        /// else pays for it.
        list: Option<u32>,
    },
}

#[derive(Default)]
struct Scope {
    entries: HashMap<String, Entry>,
    /// The type each object *with linkage* declared in this scope has when it
    /// is named through this scope's declaration of it.
    ///
    /// C99 6.2.7p4 gives a redeclaration of a linked identifier the composite
    /// type of the prior visible declaration and this one — and gives it "for
    /// the remainder of the scope in which the later declaration appears"
    /// only. One object, several declarations, and the type a use sees is the
    /// innermost declaration's composite:
    ///
    /// ```c
    /// void f(void) {
    ///     extern int i[];
    ///     { extern int i[10]; (void)sizeof(i); } /* 40 */
    ///     (void)sizeof(i);                       /* an error again */
    /// }
    /// ```
    ///
    /// which is WG14 DR011. The [object](ir::Object) keeps the composite,
    /// because that is the type this unit generates for; this is what says
    /// which *declaration* a name went through. See
    /// [`Sema::visible_object_ty`].
    composites: HashMap<ObjectId, Ty>,
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

/// Which scope a function declaration's name belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FuncScope {
    /// The file scope, where C gives a function its linkage — including a
    /// block-scope `int g(int);`, which declares the external `g` (6.2.2p4).
    File,
    /// The block it was written in: GNU's nested function, which has no
    /// linkage at all.
    Nested,
}

/// One level of the function nesting the statement being checked sits in.
///
/// There is always exactly one frame per level while a body is checked: the
/// file-scope function is level zero, a [nested
/// function](Sema::nested_function_def) written in its body is level one, and
/// so on. The hidden environment parameters themselves live on
/// [`ir::Function`], where they are still reachable once the frame is gone.
struct NestFrame {
    /// The function this level is checking.
    func: FuncId,
    /// Its labels, so that a `goto` in a *deeper* level can be told the
    /// difference between a name nothing declares and GNU's nonlocal goto.
    labels: HashSet<String>,
}

/// The per-function state a nested definition has to put aside while its own
/// body is checked, and give back afterwards.
///
/// A nested function is checked where it stands, in the middle of the
/// enclosing function's body, so everything the enclosing one had accumulated
/// — its labels, its loops, the variable length arrays in scope — has to
/// survive the detour untouched.
struct SavedFunc {
    ret_ty: Ty,
    func_name: String,
    func_variadic: bool,
    func_params: Vec<ObjectId>,
    va_param: Option<ObjectId>,
    cfg_mode: bool,
    region_labels: HashSet<String>,
    labels: HashMap<String, Label>,
    breakables: Vec<Breakable>,
    switch_stack: Vec<SwitchState>,
    vla_scopes: Vec<ObjectId>,
    label_vla_scopes: HashMap<ir::LabelId, Vec<ObjectId>>,
    goto_scopes: Vec<(SourceRange, ir::LabelId, Vec<ObjectId>)>,
    switch_vla_depths: Vec<usize>,
    func_uses_alloca: bool,
    cleanup_depth: usize,
    next_loop: u32,
    next_switch: u32,
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
    /// Whether the complex types are available; see [`crate::Options::complex`].
    complex: bool,
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
    /// The tag of the enumeration a specifier names, when that enumeration was
    /// still **incomplete** where the specifier was written — indexed the same
    /// way as `enum_by_spec`.
    ///
    /// An enumeration is incomplete until the `}` of its own list (C23
    /// 6.7.3.3p12), so `enum E { m = sizeof(enum E) }` asks for the size of
    /// something that has none; a tag that is only ever declared is incomplete
    /// for good. `Ty` cannot carry the answer — a block-scope enumeration is
    /// `int` here, and `int` is complete — so the *occurrence* records it.
    enum_incomplete: Vec<Option<String>>,
    /// The enumerator list of every `enum` tag that has been defined, as
    /// `(name, value)` pairs in declaration order, indexed by
    /// [`TagEntry::Enum::list`].
    ///
    /// C23 (N3037) made a second definition of one tag in one scope declare
    /// the *same type* when its enumerators agree with the first's, so the
    /// first's have to be kept to be compared with. Only a definition adds one
    /// — a `struct` keeps its members in [`ir::RecordDef`] and needs nothing
    /// here.
    enum_lists: Vec<Vec<(String, i128)>>,
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
    /// The hidden locals that hold a vector which is not an object while one
    /// of its lanes is read — `(a * b)[0]` — which, unlike a compound
    /// literal's, may not be assigned to: GCC's "lvalue required".
    rvalue_lanes: HashSet<ObjectId>,
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
    /// The file-scope tentative definitions whose type was an enumeration that
    /// had no list yet, with the tag they are waiting for.
    ///
    /// C99 6.9.2p2 makes a tentative definition a definition at the end of the
    /// translation unit, and there is nothing to define while the type is
    /// incomplete. The enumeration may be completed anywhere between here and
    /// there, so the answer is only known at the end; `C23/n3030.c` writes
    /// `enum E5 y;` for a tag it never completes.
    incomplete_enum_objects: Vec<(String, String, SourceRange)>,
    /// The `enum` specifier of the declaration being resolved, when that
    /// declaration is nothing but the specifier.
    ///
    /// C23 (N3030) lets a *non-defining* declaration of an enumeration with a
    /// fixed underlying type stand only on its own — `enum E : short;` and
    /// nothing else — so `enum E : short x;`, a parameter, a return type, a
    /// `typedef` and a member are all constraint violations. Which of those a
    /// specifier is in is not something [`Sema::enum_ty`] can see, so the two
    /// places a declaration has no declarators say so here.
    standalone_enum: Option<ast::EnumSpecId>,
    /// The name of the **underspecified** declaration whose initialiser is
    /// being checked — C23's `auto x = e;` — which may not appear in it.
    ///
    /// It is cleared by the first use that is reported, so that `auto a = a *
    /// a;` is one diagnostic and the recovery reads whatever an enclosing
    /// scope had. See `Sema::underspecified`.
    underspecified: Option<String>,
    /// How many of [`Sema::scopes`] are [prototype
    /// scopes](Sema::push_prototype_scope), which are not blocks.
    proto_depth: usize,
    /// The tags a function *definition*'s parameter list declared, waiting for
    /// the body's block to be pushed.
    ///
    /// C99 6.2.1p4 gives an identifier declared "within the list of parameter
    /// declarations in a function definition" block scope terminating at the
    /// end of the body, rather than the function prototype scope a
    /// *declaration*'s list gives it. That is what makes
    /// `int f(struct T { int a; } t) { return t.a; }` legal, and the type is
    /// gone again after the closing brace. The signature is resolved before
    /// the body's scope exists, so the tags wait here in between.
    param_tags: HashMap<String, TagEntry>,
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
    /// The function-nesting level each object was created at, indexed by
    /// [`ObjectId`]: zero for everything a file-scope declaration or an
    /// ordinary function body made, one for the body of a function nested in
    /// one of those, and so on.
    ///
    /// It is what answers "does this automatic object belong to the function
    /// being checked, or to one that encloses it?" — and therefore what makes
    /// a [nested function](Sema::nested_function_def) capture.
    object_level: Vec<u32>,
    /// The chain of functions enclosing the one being checked, outermost
    /// first, with the one being checked at the end. Empty outside a body.
    nest: Vec<NestFrame>,
    /// For every function that has been checked, the chain
    /// [`Sema::nest`] had while its body was: `[f]` for a file-scope function,
    /// `[f, g]` for a `g` nested in `f`.
    ///
    /// [`Sema::propagate_nested_env`] needs it after the fact, to give a
    /// caller the hidden parameters its callee turned out to want.
    nest_chains: HashMap<FuncId, Vec<FuncId>>,
    /// Every call from one function to a lifted nested one, which is what
    /// [`Sema::propagate_nested_env`] walks: a caller has to pass on whatever
    /// its callee captures and it does not own itself.
    nested_calls: Vec<(FuncId, FuncId)>,
    /// Where the address of a lifted nested function was taken, checked once
    /// every capture is known; see [`Sema::check_nested_addresses`].
    nested_addresses: Vec<(FuncId, SourceRange)>,
    /// The enclosing objects a nested function has already been told it cannot
    /// use, so that a body which reads one five times is told once.
    refused_captures: HashSet<(FuncId, ObjectId)>,
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
    /// graph, which is what a `goto` Rust cannot express and a nested `case`
    /// label need.
    cfg_mode: bool,
    /// The labels of the function being checked that a
    /// [region](crate::regions) will be built for, which are the ones a
    /// structured body has to keep. Empty in CFG mode, which keeps them all.
    region_labels: HashSet<String>,
    /// The labels of the function being checked, collected before its body is.
    labels: HashMap<String, Label>,
    /// Whether the initialiser being checked may give a flexible array member
    /// a value; see [`init::FlexibleInit`].
    flexible_init: init::FlexibleInit,
    /// Every label of the unit whose address a `&&label` has taken.
    ///
    /// Such a label keeps a block — and therefore a state number — of its own
    /// through the [CFG](crate::cfg) lowering's clean-up passes, and is a
    /// possible target of every computed `goto` in its function. The set is
    /// unit-wide, since a [`ir::LabelId`] is; the labels of *one* function are
    /// what [`Sema::labels`] holds at the moment its body is lowered.
    label_addrs: HashSet<ir::LabelId>,
    breakables: Vec<Breakable>,
    /// The `switch` statements enclosing the statement being checked, in CFG
    /// mode. Empty in the structured mode, which splits a `switch` body into
    /// groups as it walks it.
    switch_stack: Vec<SwitchState>,
    next_loop: u32,
    next_switch: u32,
    /// Numbers the `goto` labels of the whole unit, function after function.
    ///
    /// A [`ir::LabelId`] is unique across the translation unit rather than
    /// within a function, which is what lets code generation look one up
    /// without knowing whose it is — a block-scope `static void *t[] = {
    /// &&a };` becomes an item at module level, and the state number its
    /// initialiser holds belongs to a function the item says nothing about.
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
            complex: options.complex,
            gating: options.gating(),
            va_list_gate_reported: false,
            target: options.target,
            program: Program {
                unit_id,
                // `#pragma cinrs crate` is the preprocessor's and reaches the
                // program after this, in `crate::expand_with`; until then the
                // default is what a diagnostic path would generate with.
                crate_path: ir::DEFAULT_CRATE_PATH.to_owned(),
                ..Program::default()
            },
            scopes: vec![Scope::default()],
            tags: vec![HashMap::new()],
            record_by_spec: vec![None; unit.records.len()],
            enum_by_spec: vec![None; unit.enums.len()],
            enum_unsigned: vec![None; unit.enums.len()],
            enum_incomplete: vec![None; unit.enums.len()],
            enum_lists: Vec::new(),
            defined_functions: HashSet::new(),
            item_names: HashSet::new(),
            initialized: HashSet::new(),
            compound_literals: Vec::new(),
            rvalue_lanes: HashSet::new(),
            vm_bounds: Vec::new(),
            bound_mode: BoundMode::Expression,
            vm_name: String::new(),
            incomplete_enum_objects: Vec::new(),
            standalone_enum: None,
            underspecified: None,
            proto_depth: 0,
            param_tags: HashMap::new(),
            in_param_type: false,
            vla_scopes: Vec::new(),
            label_vla_scopes: HashMap::new(),
            goto_scopes: Vec::new(),
            switch_vla_depths: Vec::new(),
            func_uses_alloca: false,
            pending_discard: Vec::new(),
            cleanup_depth: 0,
            static_literals: HashMap::new(),
            object_level: Vec::new(),
            nest: Vec::new(),
            nest_chains: HashMap::new(),
            nested_calls: Vec::new(),
            nested_addresses: Vec::new(),
            refused_captures: HashSet::new(),
            ret_ty: Ty::Void,
            func_name: String::new(),
            func_variadic: false,
            func_params: Vec::new(),
            va_param: None,
            cfg_mode: false,
            region_labels: HashSet::new(),
            labels: HashMap::new(),
            flexible_init: init::FlexibleInit::Automatic,
            label_addrs: HashSet::new(),
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
        // The x86 vector types, which the bundled `<xmmintrin.h>` and
        // `<immintrin.h>` `typedef` to `__m128` and the rest. They are the
        // compiler's because GCC's own header writes them as
        // `__attribute__((vector_size(16)))`, which this front end refuses —
        // and only on an x86 target, where there is a `core::arch` type to
        // generate them as. [`crate::parse`] gates them the same way, so on any
        // other target the name is simply not a type name.
        if matches!(sema.target.arch, Arch::X86 | Arch::X86_64) {
            for (name, ty) in X86_VECTOR_TYPEDEF_NAMES {
                sema.scopes[0].entries.insert(
                    (*name).to_owned(),
                    Entry::Typedef(TypedefEntry {
                        resolved: Ok(*ty),
                        range: SourceRange::at(0),
                    }),
                );
            }
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
        self.check_tentative_enums();
        // What a nested function captures is only final once every call to it
        // has been seen, and what may be done with its address follows from
        // that.
        self.propagate_nested_env();
        self.check_nested_addresses();
        self.check_nested_definitions();
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

    /// C99 6.9.2p2: a tentative definition is a definition at the end of the
    /// translation unit, and an enumeration whose list was never written has
    /// nothing to define.
    ///
    /// The tag may be completed anywhere after the declaration, so this is the
    /// first moment the answer is known. See
    /// [`Sema::incomplete_enum_objects`].
    fn check_tentative_enums(&mut self) {
        for (name, tag, range) in std::mem::take(&mut self.incomplete_enum_objects) {
            if matches!(
                self.tags.first().and_then(|scope| scope.get(&tag)),
                Some(TagEntry::Enum { complete: true, .. })
            ) {
                continue;
            }
            self.error(
                range,
                format!(
                    "the tentative definition of '{name}' has type 'enum {tag}', which is \
                     never completed"
                ),
            );
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
    /// **Tags go in it too.** 6.2.1p4 scopes an identifier by where its
    /// declarator or type specifier stands, and a `struct S` written in a
    /// parameter list stands there, so — WG14 DR103 — the two `struct S` of
    ///
    /// ```c
    /// void f(struct S s);
    /// void f(struct S { int a; } s) { }
    /// ```
    ///
    /// are two unrelated types and the second declaration conflicts with the
    /// first. It is also why GCC warns that such a tag "will not be visible
    /// outside of this definition or declaration": nothing after the closing
    /// parenthesis can name it. A parameter list that belongs to a
    /// **definition** is the exception the same paragraph makes — the
    /// identifier has block scope, terminating with the body — and that is
    /// what [`Sema::param_tags`] carries across.
    ///
    /// The scope does not count as a block, so a compound literal or an array
    /// bound written in the list is still at file scope when the declaration
    /// is.
    fn push_prototype_scope(&mut self) {
        self.scopes.push(Scope::default());
        self.tags.push(HashMap::new());
        self.proto_depth += 1;
    }

    /// Leaves the prototype scope. `carry` is for a parameter list that
    /// belongs to a function *definition*, whose tags go on to the body's
    /// block; see [`Sema::param_tags`].
    fn pop_prototype_scope(&mut self, carry: bool) {
        self.scopes.pop();
        let tags = self.tags.pop().unwrap_or_default();
        self.param_tags = if carry { tags } else { HashMap::new() };
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

    /// Records the type a declaration of a linked object in *this* scope gives
    /// it; see [`Scope::composites`].
    fn note_object_ty(&mut self, id: ObjectId, ty: Ty) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.composites.insert(id, ty);
        }
    }

    /// The type `id` has where it is being named — the innermost declaration's
    /// composite (6.2.7p4), and the object's own type where no scope in
    /// between declared it.
    fn visible_object_ty(&self, id: ObjectId) -> Ty {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.composites.get(&id).copied())
            .unwrap_or_else(|| self.program.object(id).ty)
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

    /// Moves the tags a definition's parameter list declared into the scope
    /// just pushed for its body; see [`Sema::param_tags`].
    fn take_param_tags(&mut self) {
        let carried = std::mem::take(&mut self.param_tags);
        if let Some(scope) = self.tags.last_mut() {
            scope.extend(carried);
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

    /// The tag of the enumeration a type name is nothing but a mention of,
    /// when that enumeration was incomplete where the mention was written.
    ///
    /// The type has to be resolved first — the answer is what
    /// [`Sema::enum_incomplete`] recorded — and only a bare `enum E` is asked
    /// about: `enum E *p` is a pointer, which needs no size at all.
    fn incomplete_enum(&self, ty: &ast::Type) -> Option<&str> {
        let ast::TypeKind::Enum(id) = &ty.kind else {
            return None;
        };
        self.enum_incomplete[id.index()].as_deref()
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
    /// qualifiers, so `a == b` already answers the question for all but two
    /// cases: a function type with no prototype (6.7.5.3p15), which C23
    /// removed, and — in C23 — two tags of one name from two scopes, which
    /// 6.2.7p1 makes compatible when their members are (N3037). Keeping the
    /// difference this narrow is deliberate — an enumerated type is compatible
    /// with an implementation-defined integer type too, and opening *that*
    /// here would change what `_Generic` selects.
    fn compatible(&self, a: Ty, b: Ty) -> bool {
        self.compatible_in(a, b, &mut Vec::new())
    }

    /// [`Sema::compatible`], carrying the pairs of tags the comparison is
    /// already inside; see [`Sema::record_difference`].
    ///
    /// The list is empty for all but a `struct` or `union` comparison, and
    /// `Vec::new` allocates nothing, so the guard costs nothing where there is
    /// nothing to guard against.
    fn compatible_in(&self, a: Ty, b: Ty, comparing: &mut Vec<(RecordId, RecordId)>) -> bool {
        if a == b {
            return true;
        }
        match (a, b) {
            (Ty::Func(x), Ty::Func(y)) => self.func_compatible(x, y, comparing),
            (Ty::Pointer(x), Ty::Pointer(y)) => {
                let (x, y) = (self.types().pointer_type(x), self.types().pointer_type(y));
                x.konst == y.konst && self.compatible_in(x.pointee, y.pointee, comparing)
            }
            // C99 6.7.5.2p6: compatible element types, and equal sizes only
            // when *both* have one. `extern int j[]; int j[3];` declares one
            // object, and `__builtin_types_compatible_p(int[5], int[])` is
            // one of GCC's own torture cases.
            (Ty::Array(x), Ty::Array(y)) => {
                let (x, y) = (self.types().array_type(x), self.types().array_type(y));
                x.elem_const == y.elem_const
                    && x.vla == y.vla
                    && self.compatible_in(x.elem, y.elem, comparing)
                    && (x.incomplete || y.incomplete || x.len == y.len)
            }
            (Ty::Record(x), Ty::Record(y)) => self.records_compatible(x, y, comparing),
            _ => false,
        }
    }

    /// C23 6.2.7p1 (N3037) for two `struct` or `union` tags: same tag name,
    /// both complete, and a member list the other's agrees with.
    ///
    /// A tag belongs to the scope it was declared in, so one `struct S` inside
    /// a block and another at file scope have always been two types here — and
    /// two *incompatible* types, since compatibility was tag identity. C23
    /// made them compatible when they are spelled alike, which is the rule
    /// that was already true across translation units brought inside one.
    ///
    /// Two things it deliberately does not do. An **anonymous** record is
    /// never compatible with another: with no tag there is nothing to say the
    /// two spellings were meant to be one type, and C23's rule is written
    /// around the tag. And compatibility is not *identity*: the two are still
    /// two Rust items, so `_Generic`, `__builtin_types_compatible_p` and a
    /// pointer assignment see the rule, while assigning one whole `struct` to
    /// the other still wants the same tag.
    fn records_compatible(
        &self,
        x: RecordId,
        y: RecordId,
        comparing: &mut Vec<(RecordId, RecordId)>,
    ) -> bool {
        if self.gating.standard < crate::Standard::C23 {
            return false;
        }
        let types = self.types();
        let (a, b) = (types.record(x), types.record(y));
        let (Some(tag), Some(other)) = (&a.tag, &b.tag) else {
            return false;
        };
        if tag != other || a.kind != b.kind || !a.complete || !b.complete {
            return false;
        }
        self.record_difference(x, y, comparing).is_none()
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
    fn func_compatible(
        &self,
        x: ir::FuncTyId,
        y: ir::FuncTyId,
        comparing: &mut Vec<(RecordId, RecordId)>,
    ) -> bool {
        let (x, y) = (self.types().func_type(x), self.types().func_type(y));
        if !self.compatible_in(x.ret, y.ret, comparing) {
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
                        .all(|(a, b)| self.compatible_in(*a, *b, comparing))
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
        let level = self.nest.len().saturating_sub(1) as u32;
        self.new_object_at(name, ty, storage, is_const, range, level)
    }

    /// The same, for an object that belongs to a function *other* than the one
    /// being checked: the hidden environment parameter a middle level of a
    /// nesting has to take so that it can pass it on.
    fn new_object_at(
        &mut self,
        name: &str,
        ty: Ty,
        storage: Storage,
        is_const: bool,
        range: SourceRange,
        level: u32,
    ) -> ObjectId {
        let id = ObjectId(self.program.objects.len() as u32);
        self.program.objects.push(ir::Object {
            name: name.to_owned(),
            ty,
            storage,
            is_const,
            is_register: false,
            vla_storage: false,
            align: None,
            flexible_len: None,
            asm_label: None,
            section: None,
            range,
        });
        self.object_level.push(level);
        id
    }

    // -- nested functions ---------------------------------------------------

    /// The function-nesting level `id` was created at.
    fn object_level(&self, id: ObjectId) -> usize {
        self.object_level
            .get(id.0 as usize)
            .copied()
            .unwrap_or_default() as usize
    }

    /// The place a reference to `id` denotes here.
    ///
    /// An automatic object of an *enclosing* function is reached through the
    /// hidden pointer parameter the lifted function took it in, so that
    /// assignment, `&x`, `x++`, subscripting and member access all keep
    /// working and every one of them is seen by the enclosing function too.
    fn object_place(&mut self, id: ObjectId, range: SourceRange) -> Place {
        let is_const = self.program.object(id).is_const;
        // Not `object.ty`: a linked object may have been redeclared in an
        // inner block, and the composite that gave it belongs to that block
        // alone (6.2.7p4). See [`Scope::composites`].
        let ty = self.visible_object_ty(id);
        let Some(ptr) = self.capture(id, range) else {
            return place_of(PlaceKind::Object(id), ty, is_const, range);
        };
        let pty = self.program.object(ptr).ty;
        let load = Expr::new(
            ExprKind::Load(place_of(PlaceKind::Object(ptr), pty, true, range)),
            pty,
            range,
        );
        place_of(PlaceKind::Deref(Box::new(load)), ty, is_const, range)
    }

    /// The hidden pointer the function being checked reaches `owner` through,
    /// or `None` when it owns the object itself.
    ///
    /// Everything with static storage duration is an item rather than a frame
    /// slot, so nothing has to be captured to reach it; that covers a
    /// file-scope object and an enclosing function's `static` local alike.
    fn capture(&mut self, owner: ObjectId, range: SourceRange) -> Option<ObjectId> {
        if self.nest.len() < 2 || self.program.object(owner).storage != Storage::Automatic {
            return None;
        }
        let level = self.object_level(owner);
        if level + 1 >= self.nest.len() {
            return None;
        }
        if !self.capturable(owner, range) {
            return None;
        }
        let chain: Vec<FuncId> = self.nest.iter().map(|frame| frame.func).collect();
        self.env_chain(&chain, owner, level)
    }

    /// Whether an enclosing object may be captured at all.
    ///
    /// Two kinds cannot be, in this release: a variably modified object, whose
    /// bounds live in hidden objects of the enclosing frame that would have to
    /// be captured with it, and a `va_list`, which is a borrow of the caller's
    /// argument list and has no address a callee may keep.
    fn capturable(&mut self, owner: ObjectId, range: SourceRange) -> bool {
        let object = self.program.object(owner);
        let (ty, name, declared) = (object.ty, object.name.clone(), object.range);
        let reason = if object.vla_storage || self.types().is_vm(ty) {
            "it is a variable length array, whose length lives in the enclosing frame, and \
             cinrs does not capture that yet. Pass the array and its length as parameters"
        } else if ty.is_va_list() {
            "a 'va_list' belongs to the function whose arguments it walks, and cinrs does \
             not capture one yet. Read the arguments in the enclosing function and pass the \
             values"
        } else {
            return true;
        };
        // A body that reads the object five times is told once.
        let here = self.nest.last().expect("a capture needs a nesting").func;
        if self.refused_captures.insert((here, owner)) {
            self.error_note(
                range,
                format!("a nested function cannot use '{name}': {reason}"),
                declared,
                format!("'{name}' is declared here"),
            );
        }
        false
    }

    /// Gives every function in `chain` above `level` a hidden parameter for
    /// `owner`, and hands back the last one's.
    ///
    /// A two-level nesting is what makes this a loop: an inner function that
    /// uses the *outermost* function's variable receives it through the middle
    /// function, which has to take it whether it uses the variable itself or
    /// not.
    fn env_chain(&mut self, chain: &[FuncId], owner: ObjectId, level: usize) -> Option<ObjectId> {
        let mut ptr = None;
        for (index, func) in chain.iter().enumerate().skip(level + 1) {
            ptr = Some(self.env_param(*func, owner, index));
        }
        ptr
    }

    /// The hidden parameter `func` carries `owner` in, adding one if this is
    /// the first use.
    fn env_param(&mut self, func: FuncId, owner: ObjectId, level: usize) -> ObjectId {
        if let Some(entry) = self
            .program
            .function(func)
            .env
            .iter()
            .find(|entry| entry.owner == owner)
        {
            return entry.param;
        }
        let object = self.program.object(owner);
        let (ty, is_const, name, range) = (
            object.ty,
            object.is_const,
            object.name.clone(),
            object.range,
        );
        let pointer = self.ptr_to(ty, is_const);
        // The hidden parameter is named after the variable it carries, so that
        // the generated Rust reads as what it is.
        let param = self.new_object_at(
            &format!("__env_{name}"),
            pointer,
            Storage::Automatic,
            true,
            range,
            level as u32,
        );
        self.program.functions[func.0 as usize]
            .env
            .push(ir::EnvParam { owner, param });
        param
    }

    /// Passes on what a callee captures.
    ///
    /// A function that calls a nested one has to hand it the addresses it
    /// wants, and for an object it does not own itself that means taking a
    /// hidden parameter of its own. Doing it here rather than at the call site
    /// is what lets a call be checked before the callee's own body has been:
    /// GNU's `auto int g(int);` forward declaration exists for exactly that,
    /// and mutual recursion between two nested functions needs it.
    ///
    /// The iteration is a fixed point because one caller's new parameter may
    /// be another's, up the chain.
    fn propagate_nested_env(&mut self) {
        if self.nested_calls.is_empty() {
            return;
        }
        let edges = std::mem::take(&mut self.nested_calls);
        loop {
            let mut changed = false;
            for (caller, callee) in &edges {
                let owners: Vec<ObjectId> = self
                    .program
                    .function(*callee)
                    .env
                    .iter()
                    .map(|entry| entry.owner)
                    .collect();
                let Some(chain) = self.nest_chains.get(caller).cloned() else {
                    continue;
                };
                for owner in owners {
                    let level = self.object_level(owner);
                    if level + 1 >= chain.len() {
                        continue;
                    }
                    let before = self.program.function(*caller).env.len();
                    self.env_chain(&chain, owner, level);
                    changed |= self.program.function(*caller).env.len() != before;
                }
            }
            if !changed {
                return;
            }
        }
    }

    /// Reports every address taken of a nested function that turned out to
    /// capture something.
    ///
    /// The check waits until [`Sema::propagate_nested_env`] has run: a
    /// function that only calls a capturing sibling captures nothing of its
    /// own, and is still not a function whose address this crate can hand out.
    fn check_nested_addresses(&mut self) {
        for (id, range) in std::mem::take(&mut self.nested_addresses) {
            let function = self.program.function(id);
            if function.env.is_empty() {
                continue;
            }
            let name = function.name.clone();
            let uses: Vec<String> = function
                .env
                .iter()
                .map(|entry| format!("'{}'", self.program.object(entry.owner).name))
                .collect();
            let declared = function.range;
            self.error_note(
                range,
                format!(
                    "the address of the nested function '{name}' cannot be taken: it uses the \
                     enclosing function's {}, which GCC reaches through a trampoline written \
                     onto the stack and cinrs cannot generate. Move '{name}' to file scope and \
                     pass what it uses as parameters",
                    join_names(&uses)
                ),
                declared,
                format!("'{name}' is defined here"),
            );
        }
    }

    /// Reports a nested function that was forward-declared with `auto` and
    /// never defined.
    fn check_nested_definitions(&mut self) {
        let missing: Vec<(String, SourceRange)> = self
            .program
            .functions
            .iter()
            .filter(|function| function.is_nested() && function.is_extern())
            .map(|function| (function.name.clone(), function.range))
            .collect();
        for (name, range) in missing {
            self.error(
                range,
                format!(
                    "the nested function '{name}' is declared but never defined; a nested \
                     function has no linkage, so nothing outside this function can define it"
                ),
            );
        }
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
            ConstValue::Complex(re, im) => {
                let component = ty.complex_component();
                let part = |v: f64| Box::new(Expr::new(ExprKind::Float(v), component, range));
                Expr::new(
                    ExprKind::ComplexOf {
                        re: part(re),
                        im: part(im),
                    },
                    ty,
                    range,
                )
            }
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
            ExprKind::ComplexOf { re, im } => {
                let (re, _) = as_parts(self.const_eval(re)?);
                let (im, _) = as_parts(self.const_eval(im)?);
                Some(complex_value(expr.ty, (re, im)))
            }
            // `__real__ (1.0 + 2.0i)` and `creal(…)` of a constant: the part of
            // a *temporary* is as constant as the value it was taken from. A
            // part of a named object is not, which is why only this shape
            // folds.
            ExprKind::Load(Place {
                kind: PlaceKind::ComplexPart { base, imag },
                ..
            }) => {
                let PlaceKind::Temporary(inner) = &base.kind else {
                    return None;
                };
                let (re, im) = as_parts(self.const_eval(inner)?);
                let part = if *imag { im } else { re };
                Some(ConstValue::Float(round_to(part, expr.ty)))
            }
            ExprKind::Zeroed if expr.ty.is_complex() => Some(ConstValue::Complex(0.0, 0.0)),
            ExprKind::Zeroed if expr.ty.is_arithmetic() => Some(if expr.ty.is_floating() {
                ConstValue::Float(0.0)
            } else {
                ConstValue::Int(0)
            }),
            ExprKind::Cast(inner) => {
                // A pointer that is itself a constant converts to a constant
                // integer, which is what makes the hand-written `offsetof` —
                // `((size_t) &((struct s *) 0)->m)` — an array bound, a `case`
                // label and a static initialiser. GCC calls the folding an
                // extension and so is this: see [`Sema::place_offset`], which
                // is where the GNU dialects are required.
                if expr.ty.is_integer()
                    && inner.ty.is_pointer()
                    && let Some(address) = self.integer_pointer_value(inner)
                {
                    return Some(ConstValue::Int(expr.ty.wrap(address, &target)));
                }
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
                ConstValue::Complex(re, im) => Some(ConstValue::Complex(-re, -im)),
            },
            ExprKind::BitNot(inner) => match self.const_eval(inner)? {
                ConstValue::Int(v) => Some(ConstValue::Int(expr.ty.wrap(!v, &target))),
                ConstValue::Float(_) => None,
                // GNU's `~z` is the conjugate.
                ConstValue::Complex(re, im) => Some(ConstValue::Complex(re, -im)),
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
                    // Only `==` and `!=` reach a complex operand; sema refuses
                    // the ordering ones, and both parts have to agree.
                    (ConstValue::Complex(ar, ai), ConstValue::Complex(br, bi)) => {
                        let equal = ar == br && ai == bi;
                        match op {
                            ir::CmpOp::Eq => equal,
                            ir::CmpOp::Ne => !equal,
                            _ => return None,
                        }
                    }
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
            ExprKind::Builtin { op, args } => self.const_bit_builtin(*op, args, expr),
            _ => None,
        }
    }

    /// The bit-counting builtins, folded when their operand is a constant.
    ///
    /// GCC and Clang both make these constant expressions — `_Static_assert(
    /// __builtin_popcount(0) < 1, …)` is WG14 DR263 as Clang tests it
    /// (`drs/dr2xx.c`) — and the answer depends on the *width* of the operand,
    /// which the suffix of the builtin's name chose. `clz` and `ctz` are
    /// undefined for a zero operand and are left alone there, exactly as the
    /// generated code leaves them to Rust.
    fn const_bit_builtin(
        &mut self,
        op: ir::BuiltinOp,
        args: &[Expr],
        expr: &Expr,
    ) -> Option<ConstValue> {
        use ir::BuiltinOp;
        let [arg] = args else {
            return None;
        };
        let bits = u32::try_from(arg.ty.size_bytes(&self.target) * 8).ok()?;
        if bits == 0 || bits > 128 {
            return None;
        }
        let ConstValue::Int(value) = self.const_eval(arg)? else {
            return None;
        };
        // The operand as the unsigned bit pattern of its own width, which is
        // what every one of these counts over.
        let mask = if bits == 128 {
            u128::MAX
        } else {
            (1u128 << bits) - 1
        };
        let bitpattern = (value as u128) & mask;
        let result = match op {
            BuiltinOp::Popcount => i128::from(bitpattern.count_ones()),
            BuiltinOp::Parity => i128::from(bitpattern.count_ones() % 2),
            BuiltinOp::Ffs => match bitpattern {
                0 => 0,
                _ => i128::from(bitpattern.trailing_zeros() + 1),
            },
            BuiltinOp::Clz if bitpattern != 0 => {
                i128::from(bitpattern.leading_zeros() - (128 - bits))
            }
            BuiltinOp::Ctz if bitpattern != 0 => i128::from(bitpattern.trailing_zeros()),
            // The number of leading bits that repeat the sign bit, less the
            // sign bit itself, which is what GCC documents `clrsb` as.
            BuiltinOp::Clrsb => {
                let top = bitpattern >> (bits - 1);
                let folded = if top == 1 {
                    !bitpattern & mask
                } else {
                    bitpattern
                };
                i128::from(if folded == 0 {
                    bits - 1
                } else {
                    folded.leading_zeros() - (128 - bits) - 1
                })
            }
            BuiltinOp::Bswap => {
                let bytes = bits / 8;
                let mut swapped = 0u128;
                for byte in 0..bytes {
                    swapped |= ((bitpattern >> (byte * 8)) & 0xff) << ((bytes - 1 - byte) * 8);
                }
                swapped as i128
            }
            _ => return None,
        };
        Some(ConstValue::Int(expr.ty.wrap(result, &self.target)))
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
        if expr.ty.is_complex() {
            return self.const_complex_binary(op, lhs, rhs, expr);
        }
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

    /// `+ - * /` on a constant with at least one complex operand.
    ///
    /// The folding has to give exactly what the generated code would have
    /// computed, so it takes the same routes: componentwise for a real operand
    /// on either side of `+`, `-` and `*` and under a complex `/` real, and
    /// [`crate::complex`]'s Annex G arithmetic for the rest. See that module
    /// for why the mixed forms are not simply "convert and go".
    fn const_complex_binary(
        &mut self,
        op: ir::BinOp,
        lhs: ConstValue,
        rhs: ConstValue,
        expr: &Expr,
    ) -> Option<ConstValue> {
        use ir::BinOp;
        let ty = expr.ty;
        let narrow = ty == Ty::ComplexFloat;
        let (a, b) = as_parts(lhs);
        let (c, d) = as_parts(rhs);
        let left_complex = matches!(lhs, ConstValue::Complex(..));
        let right_complex = matches!(rhs, ConstValue::Complex(..));
        let parts = match op {
            // A real operand leaves the imaginary part exactly as it was —
            // *not* added to a zero, which would turn a negative zero
            // positive. See `cinrs_rt::complex`.
            BinOp::Add => match (left_complex, right_complex) {
                (true, true) => (a + c, b + d),
                (true, false) => (a + c, b),
                (false, _) => (a + c, d),
            },
            BinOp::Sub => match (left_complex, right_complex) {
                (true, true) => (a - c, b - d),
                (true, false) => (a - c, b),
                (false, _) => (a - c, -d),
            },
            BinOp::Mul => {
                if left_complex && right_complex {
                    if narrow {
                        crate::complex::mul_f32((a, b), (c, d))
                    } else {
                        crate::complex::mul((a, b), (c, d))
                    }
                } else if left_complex {
                    (a * c, b * c)
                } else {
                    (a * c, a * d)
                }
            }
            BinOp::Div => {
                if right_complex {
                    if narrow {
                        crate::complex::div_f32((a, b), (c, d))
                    } else {
                        crate::complex::div((a, b), (c, d))
                    }
                } else {
                    (a / c, b / c)
                }
            }
            // Sema refuses every other operator on a complex operand.
            _ => return None,
        };
        Some(complex_value(ty, parts))
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
        // C99 6.3.1.2: a complex value is true when *either* part is non-zero.
        ConstValue::Complex(re, im) => re != 0.0 || im != 0.0,
    }
}

/// The two parts of a constant, for the complex arithmetic in
/// [`crate::complex`]: a real value is `(v, 0)`.
fn as_parts(value: ConstValue) -> crate::complex::Parts {
    match value {
        ConstValue::Int(v) => (v as f64, 0.0),
        ConstValue::Float(v) => (v, 0.0),
        ConstValue::Complex(re, im) => (re, im),
    }
}

/// A folded complex value, with both parts rounded to the component type.
fn complex_value(ty: Ty, (re, im): crate::complex::Parts) -> ConstValue {
    let component = ty.complex_component();
    ConstValue::Complex(round_to(re, component), round_to(im, component))
}

fn convert_const(value: ConstValue, from: Ty, to: Ty, target: &TargetModel) -> ConstValue {
    // Complex is its own axis: to one, from one, and between the two widths.
    if to.is_complex() {
        return complex_value(to, as_parts(value));
    }
    if let ConstValue::Complex(re, _) = value {
        // C99 6.3.1.7: converting a complex value to a real type discards the
        // imaginary part, and the real part converts as usual.
        return convert_const(ConstValue::Float(re), from.complex_component(), to, target);
    }
    match (value, to.is_floating()) {
        // Both directions were answered above; this is what tells the compiler
        // the match is exhaustive.
        (ConstValue::Complex(..), _) => value,
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
    if ty != Ty::Float {
        return value;
    }
    // A NaN is narrowed bit for bit rather than with `as`, which is free to
    // quiet a signalling NaN and to choose the payload; see
    // [`ir::narrow_nan_bits`].
    if value.is_nan() {
        return f64::from_bits(ir::widen_nan_bits(ir::narrow_nan_bits(value.to_bits())));
    }
    value as f32 as f64
}

/// Renders a `case` value the way it should read in a diagnostic.
fn render_case_value(value: i128, ty: Ty, target: &TargetModel) -> String {
    if ty.is_signed(target) {
        value.to_string()
    } else {
        (value as u128).to_string()
    }
}

/// A list of names the way a sentence would read it: `a`, `a and b`,
/// `a, b and c`.
fn join_names(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
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
