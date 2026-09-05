//! Code generation: the typed [`ir`] becomes Rust tokens.
//!
//! Every token this module emits is stamped with the span of the C construct
//! it came from, resolved through the [`SourceMap`]. That is what makes an
//! error `rustc` raises about the *generated* code — a call with the wrong
//! argument type, say — point at the C the user actually wrote.
//!
//! # Shape of the output
//!
//! One expansion produces, in this order: the `struct` and `union` items for
//! every tag, the `enum` aliases and their constants, the file-scope `typedef`
//! aliases, one `extern` block for everything the unit only declares, the
//! `static mut` items, and finally the functions. A C function becomes
//!
//! ```text
//! #[allow(…)]
//! pub unsafe extern "C" fn name(mut p: ::core::ffi::c_int) -> ::core::ffi::c_int {
//!     unsafe { … }
//! }
//! ```
//!
//! `pub` is dropped for a C `static` function, which is private to the module
//! [`crate::expand`] wraps all of this in — that module is what makes C's
//! internal linkage mean something, and what lets two `c99!` blocks in one
//! Rust module both `#include` the same header. `#[inline]` is added for an
//! `inline` function, and the body is wrapped in a single `unsafe` block
//! because edition 2024 no longer treats the body of an `unsafe fn` as an
//! unsafe block. The `#[allow(…)]` list covers everything a naive translation
//! provokes: unused bindings, redundant parentheses, non-Rust naming, code a
//! human can see is unreachable, and so on.
//!
//! A unit that asked for `#pragma cinrs export` gives everything with external
//! linkage `#[unsafe(no_mangle)]` on top of that, so that its functions and
//! objects are real C symbols another unit can link against — with C's own
//! risk of two definitions of one name, which only the linker will see.
//!
//! # Pointers, arrays and records
//!
//! Pointers are raw pointers — `T *` is `*mut T`, `const T *` is `*const T`,
//! `void *` is `*mut c_void` — and pointer arithmetic is `offset`, so nothing
//! in the output holds a reference and none of Rust's aliasing rules are
//! involved. Arrays are `[T; N]` and decay to a pointer through
//! `(&raw mut a).cast::<T>()`, which is a raw pointer from the start; a `&mut`
//! would both change the meaning and trip `static_mut_refs` on a global. A
//! function pointer is `Option<unsafe extern "C" fn(…) -> R>`, so that a null
//! one is representable, and a call through it unwraps first.
//!
//! # Places
//!
//! Assignment, compound assignment, `++`/`--` and `&` all go through this
//! module's `place` lowering, which turns an [`ir::Place`] into a *setup*
//! (statements that must run first, where the pointer arithmetic lands) plus an
//! *access* (a Rust place expression that may be evaluated more than once).
//! `p[i()] += 1` therefore evaluates `i()` exactly once, and `s.f`, `p->f` and
//! `a[i][j]` are all the same three lines of code.
//!
//! # Bit-fields
//!
//! A bit-field has no address, so it is not a field of the generated item: a
//! run of them shares one `[u8; K]`, and sema has already worked out which
//! bytes and bits each member owns (see [`crate::sema`]'s layout). This module
//! turns that into a pair of inherent methods per named member — plain inline
//! integer code, no helper type — and a place whose *access* is the record
//! rather than the member, read with `.f()` and written with `.set_f(v)`. A
//! constant initialiser is folded into the storage bytes here, which is what
//! lets a `static` hold one; a non-constant one becomes a zeroed literal
//! followed by setter calls. A place rooted in a `static mut` goes through
//! `&raw mut` first, because the accessors borrow.
//!
//! # Loops
//!
//! Every loop gets a unique Rust label so that `break` and `continue` never
//! depend on where they sit relative to a `switch`:
//!
//! ```text
//! while (c) B      'lN: while c { B }                continue → continue 'lN
//! do B while (c);  'lN: loop { 'lN_body: { B }       continue → break 'lN_body
//!                             if !(c) { break 'lN } }
//! for (i;c;s) B    { i; 'lN: loop { if !(c) { break 'lN }
//!                                   'lN_body: { B } s; } }
//! ```
//!
//! The body label exists exactly where `continue` has work to do afterwards —
//! re-testing the condition of a `do`/`while`, or running the step of a `for`.
//!
//! # Switch
//!
//! Fallthrough is what makes `switch` interesting: control enters at one label
//! and then runs *through* every group after it. Rust has no such construct,
//! but labelled blocks compose into one. For groups `g0 … gN` the output is
//!
//! ```text
//! 'swK: {
//!     'swK_cN: { … 'swK_c1: { 'swK_c0: { match v { … } } g0 } g1 … }
//!     gN
//! }
//! ```
//!
//! with the dispatch `match` innermost: `break 'swK_ci` lands immediately
//! before group *i*, and control then falls out of each enclosing block in
//! turn, running the groups after it in order. `break` inside the switch is
//! `break 'swK`, and the `_` arm goes to the `default:` group, or out of the
//! whole statement when there is none.
//!
//! # Functions that jump
//!
//! A body sema lowered into a [control-flow graph](crate::cfg) — because it
//! contains a `goto`, or a `case` label the groups above cannot express — is
//! emitted as a state machine over its blocks instead, with the function's
//! locals defined once at the top. Everything below the statement level is
//! shared: the expressions, places and conversions are generated by exactly
//! the same code in both modes.
//!
//! # Calls without a prototype
//!
//! `int f();` says nothing about the parameters (C99 6.7.5.3p14), so the item
//! generated for it is `unsafe extern "C" fn() -> R` — that is all the
//! declaration said. What the *call* passes is decided at the call site
//! (6.5.2.2p6): sema has already applied the default argument promotions to
//! every argument, and this module transmutes the callee to the signature they
//! make before calling it,
//!
//! ```text
//! ::core::mem::transmute::<unsafe extern "C" fn() -> R,
//!                          unsafe extern "C" fn(T1, …, Tn) -> R>(f)(a1, …, an)
//! ```
//!
//! with the `Option` unwrapped first for a function pointer, and no cast at all
//! when there are no arguments. Reinterpreting a function pointer like this is
//! exactly the contract C's own ABI rests on: on every ABI this crate targets
//! the address is the same one, and the call is defined precisely when the
//! callee really was defined with parameters of those promoted types —
//! undefined otherwise, which is the risk the program took by leaving the
//! prototype out. The same route is taken whenever the argument count and the
//! parameter count disagree at all, which keeps a call checked
//! against an empty list from being emitted against a prototype a later
//! declaration supplied.
//!
//! # Variadic definitions
//!
//! `R f(T a, ...)` becomes `unsafe extern "C" fn f(mut a: T, __cinrs_va: ...)`.
//! The extra parameter is the argument list as the caller left it and is never
//! advanced; `va_start` and every `va_list` local copy it, `va_arg` is
//! `next_arg`, `va_copy` and passing a list on are `clone`, and `va_end` is
//! nothing at all, because the list ends when its value is dropped. See
//! [`crate::sema`]'s `va` module for the model.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use proc_macro2::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};
use quote::quote_spanned;

use crate::Options;
use crate::capture::{SourceMap, SourceRange};
use crate::cfg::{BasicBlock, BlockId, Cfg, Terminator};
use crate::ir::{
    self, BinOp, Body, BreakTarget, Callee, CmpOp, ConstValue, Expr, ExprKind, Function, LogicalOp,
    LoopId, NEVER_RAW, Place, PlaceKind, Program, RecordKind, Stmt, Storage, Switch, Ty,
};

/// Generates the Rust items for a fully checked program.
pub fn generate(program: &Program, map: &SourceMap, options: &Options) -> TokenStream {
    let mut cg = Codegen::new(program, map, options);
    let mut out = cg.type_items();
    out.extend(cg.extern_block());
    for var in &program.statics {
        out.extend(cg.static_item(var));
    }
    let mut initialisers = TokenStream::new();
    for func in &program.functions {
        if func.body.is_some() && !cg.beyond_toolchain(func) {
            out.extend(cg.function_item(func));
            if let Some(kind) = func.init_kind {
                initialisers.extend(cg.init_array_item(func, kind));
            }
        }
    }
    if !initialisers.is_empty() {
        out.extend(cg.init_array_guard());
        out.extend(initialisers);
    }
    out
}

/// Generates signature-only items for a program that did not type check.
///
/// The bodies are `::core::unreachable!()`: the expansion also carries
/// `compile_error!`s, so nothing here can ever run. Their purpose is to keep a
/// Rust call site that mentions one of these functions from producing a second,
/// unrelated "cannot find function" error on top of the real one.
pub fn generate_stubs(program: &Program, map: &SourceMap, options: &Options) -> TokenStream {
    let mut cg = Codegen::new(program, map, options);
    let mut out = cg.type_items();
    out.extend(cg.extern_block());
    for var in &program.statics {
        out.extend(cg.static_item(var));
    }
    for func in &program.functions {
        if !func.is_extern() && !cg.beyond_toolchain(func) {
            out.extend(cg.stub_item(func));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// precedence
// ---------------------------------------------------------------------------

/// Rust's expression precedence levels, tightest last.
///
/// Emitted expressions carry the level they parse at so that parentheses are
/// added exactly where they are needed and nowhere else — the generated code is
/// meant to be read.
mod prec {
    /// A block-like expression (`if`, `{ … }`): usable on its own, but needing
    /// parentheses anywhere an operator or a statement boundary follows.
    pub const BLOCK: u8 = 0;
    /// The weakest level an operand can be handed to without parentheses.
    pub const LOWEST: u8 = 0;
    pub const OR: u8 = 2;
    pub const AND: u8 = 3;
    pub const CMP: u8 = 4;
    pub const BIT_OR: u8 = 5;
    pub const BIT_XOR: u8 = 6;
    pub const BIT_AND: u8 = 7;
    pub const SUM: u8 = 9;
    pub const PRODUCT: u8 = 10;
    pub const CAST: u8 = 11;
    pub const UNARY: u8 = 12;
    pub const CALL: u8 = 13;
    pub const ATOM: u8 = 14;
}

/// An emitted expression together with the precedence it parses at.
struct Value {
    tokens: TokenStream,
    prec: u8,
    /// Set when the tokens are a bare non-negative integer literal, whose Rust
    /// type is therefore still open to inference.
    bare_integer: bool,
    /// Set when the tokens end with the type of an `as`.
    ///
    /// `x as i32 < y` does not parse: Rust reads the `<` as the start of the
    /// generic arguments of `i32`. (`>`, `<=`, `>=` and `==` are unambiguous
    /// and need no help.)
    ends_with_type: bool,
}

impl Value {
    fn new(tokens: TokenStream, prec: u8) -> Self {
        Self {
            tokens,
            prec,
            bare_integer: false,
            ends_with_type: false,
        }
    }

    fn atom(tokens: TokenStream) -> Self {
        Self::new(tokens, prec::ATOM)
    }

    /// Marks the tokens as ending with the type of an `as`.
    fn type_end(mut self, flag: bool) -> Self {
        self.ends_with_type = flag;
        self
    }

    /// The tokens, parenthesised if they would not survive being used as an
    /// operand at `min`.
    fn at(self, min: u8, span: Span) -> TokenStream {
        if self.prec >= min {
            return self.tokens;
        }
        parenthesize(self.tokens, span)
    }

    /// The tokens, parenthesised only if they are block-like.
    ///
    /// Used where Rust starts parsing an expression that is followed by a block
    /// — the condition of an `if` or a `while` — and would otherwise mistake
    /// our expression's own braces for that block.
    fn at_condition(self, span: Span) -> TokenStream {
        if self.prec > prec::BLOCK {
            return self.tokens;
        }
        parenthesize(self.tokens, span)
    }
}

fn parenthesize(tokens: TokenStream, span: Span) -> TokenStream {
    let mut group = Group::new(Delimiter::Parenthesis, tokens);
    group.set_span(span);
    TokenStream::from(TokenTree::Group(group))
}

fn braced(tokens: TokenStream, span: Span) -> TokenStream {
    let mut group = Group::new(Delimiter::Brace, tokens);
    group.set_span(span);
    TokenStream::from(TokenTree::Group(group))
}

fn bracketed(tokens: TokenStream, span: Span) -> TokenStream {
    let mut group = Group::new(Delimiter::Bracket, tokens);
    group.set_span(span);
    TokenStream::from(TokenTree::Group(group))
}

// ---------------------------------------------------------------------------
// identifiers
// ---------------------------------------------------------------------------

/// Every Rust keyword, strict and reserved, in every edition up to 2024.
const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield", "Self",
];

/// Names that a `let` binding or a parameter must not carry, whatever the C
/// program calls them.
///
/// Rust resolves a binding pattern against the value namespace first: a name
/// that already means a unit variant there is a *pattern* that matches, not a
/// new binding, and Rust refuses the ambiguity outright with `E0530`. The four
/// below are in scope in every Rust file through the prelude; the rest of the
/// set is computed per translation unit in [`Codegen::new`], because a C
/// program may name a local exactly like one of its own globals or
/// enumerators:
///
/// ```c
/// int counter;
/// int f(int counter) { return counter; }   /* two different objects */
/// ```
const PRELUDE_PATTERNS: &[&str] = &["Some", "None", "Ok", "Err"];

/// Whether `name` can be used as the name of the generated module.
///
/// `#pragma cinrs module "…"` writes it, and it becomes a Rust identifier
/// verbatim: an ordinary one, since a module the user means to write
/// `geometry::Point` through should not have to be spelled `r#…`.
pub fn is_module_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !RUST_KEYWORDS.contains(&name)
        && !NEVER_RAW.contains(&name)
}

/// Turns a C identifier into the Rust identifier that stands for it.
///
/// C names are kept as they are, because that is what makes the expansion
/// readable and what lets Rust call the functions by the names their author
/// gave them. A name that collides with a Rust keyword becomes a raw
/// identifier (`match` → `r#match`); the five names that cannot even be raw
/// get an underscore appended instead.
fn c_ident(name: &str, span: Span) -> Ident {
    if NEVER_RAW.contains(&name) {
        return Ident::new(&format!("{name}_"), span);
    }
    if RUST_KEYWORDS.contains(&name) {
        return Ident::new_raw(name, span);
    }
    Ident::new(name, span)
}

/// The lint exemptions every generated item carries.
///
/// A transliteration of C is unidiomatic Rust by construction: names are not
/// snake case, locals are `mut` whether or not they are assigned again, a
/// `switch` leaves labels that nothing jumps to, and a function that ends in an
/// infinite loop leaves code behind that cannot run. `unknown_lints` comes
/// first so that the list may name a lint an older compiler has never heard of.
///
/// Two of them are about the `extern` block. A C program declares the library
/// its own way — `int strlen();` with no prototype is what a C89 program
/// writes, and an implicit declaration is exactly that — so the declaration
/// `cinrs` generates may disagree with the one Rust's own standard library
/// uses for the same symbol (`clashing_extern_declarations`, and Rust 1.99's
/// deny-by-default `invalid_runtime_symbol_definitions`). Nothing is
/// *defined*: the symbol is the C library's either way, and a call through a
/// type with no prototype is transmuted to the signature its arguments make
/// before it is made, which is the contract C's own ABI runs on.
fn allow_attr(span: Span) -> TokenStream {
    quote_spanned! {span=>
        #[allow(
            unknown_lints,
            clashing_extern_declarations,
            dead_code,
            improper_ctypes,
            improper_ctypes_definitions,
            invalid_runtime_symbol_definitions,
            non_camel_case_types,
            non_snake_case,
            non_upper_case_globals,
            overflowing_literals,
            static_mut_refs,
            suspicious_runtime_symbol_definitions,
            unpredictable_function_pointer_comparisons,
            unreachable_code,
            unreachable_patterns,
            unused_assignments,
            unused_braces,
            unused_comparisons,
            unused_labels,
            unused_mut,
            unused_parens,
            unused_unsafe,
            unused_variables,
            clippy::all
        )]
    }
}

// ---------------------------------------------------------------------------
// the generator
// ---------------------------------------------------------------------------

/// How `continue` leaves a particular loop.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ContinueStyle {
    /// The loop re-tests its condition on its own: `continue 'l`.
    Head,
    /// Work remains before the next iteration: `break 'l_body`.
    BodyLabel,
}

/// A place, ready to be read from or written to.
struct LoweredPlace {
    /// Statements that must run before `access` is used.
    setup: TokenStream,
    /// A Rust place expression that may be evaluated more than once.
    access: TokenStream,
    /// Set when the place is a bit-field, in which case `access` is the record
    /// holding it and the bits are reached through the generated accessors.
    bits: Option<BitAccess>,
    /// Set when the object may not be aligned the way its type asks.
    ///
    /// C reaches such an object through a packed member or a pointer cast, and
    /// says nothing about the load; Rust makes `*p` on an underaligned `p`
    /// undefined behaviour, which a debug build turns into an abort. Such a
    /// place is read and written through `read_unaligned` and
    /// `write_unaligned` instead. See [`Codegen::place_align`].
    unaligned: bool,
}

impl LoweredPlace {
    /// An ordinary place, whose access is the value.
    fn plain(setup: TokenStream, access: TokenStream) -> Self {
        Self {
            setup,
            access,
            bits: None,
            unaligned: false,
        }
    }
}

/// The accessors a bit-field place is read and written through.
struct BitAccess {
    getter: Ident,
    setter: Ident,
}

/// Where the pristine argument list of the function being generated lives.
#[derive(Clone, Copy, PartialEq, Eq)]
enum VaSource {
    /// There is none: the function takes no variable arguments.
    None,
    /// The synthetic `...` parameter of a variadic definition.
    Ellipsis,
    /// The function's own `va_list` parameter.
    Param(ir::ObjectId),
}

struct Codegen<'a> {
    program: &'a Program,
    map: &'a SourceMap,
    options: &'a Options,
    continue_styles: HashMap<LoopId, ContinueStyle>,
    /// The Rust names of the locals of the function being generated, wherever
    /// they differ from the C ones: in [CFG mode](crate::cfg), where every
    /// local of the function shares one scope, and for a local whose name
    /// would shadow a file-scope item (see [`PRELUDE_PATTERNS`]).
    local_names: HashMap<ir::ObjectId, String>,
    /// The names a `let` binding or a parameter must not use.
    reserved: HashSet<String>,
    va_source: VaSource,
    ret_ty: Ty,
    /// Whether the function being generated is a [state
    /// machine](crate::cfg), in which case every local is already bound at the
    /// top and a definition is an assignment.
    in_cfg: bool,
    temporaries: u32,
}

impl<'a> Codegen<'a> {
    fn new(program: &'a Program, map: &'a SourceMap, options: &'a Options) -> Self {
        let mut reserved: HashSet<String> =
            PRELUDE_PATTERNS.iter().map(|s| (*s).to_owned()).collect();
        // A `static mut` and a `const` are both in the value namespace, so a
        // binding of the same name is `E0530` rather than a shadow.
        for var in &program.statics {
            if let Storage::Static { item_name, .. } = &program.object(var.object).storage {
                reserved.insert(item_name.clone());
            }
        }
        for constant in &program.enum_constants {
            reserved.insert(constant.rust_name.clone());
        }
        Self {
            program,
            map,
            options,
            continue_styles: HashMap::new(),
            local_names: HashMap::new(),
            reserved,
            va_source: VaSource::None,
            ret_ty: Ty::Void,
            in_cfg: false,
            temporaries: 0,
        }
    }

    /// Whether a function's signature needs more than the compiling toolchain
    /// can do, in which case no item is generated for it at all.
    ///
    /// Sema has already reported it — see [`crate::C_VARIADIC_SUPPORTED`] — and
    /// emitting the item anyway would add `rustc`'s own `E0658` on top of a
    /// diagnostic that already says what to do.
    fn beyond_toolchain(&self, func: &Function) -> bool {
        if self.options.c_variadic {
            return false;
        }
        (func.sig.variadic && !func.is_extern())
            || self.uses_va_list(func.sig.ret)
            || func.sig.params.iter().any(|ty| self.uses_va_list(*ty))
    }

    /// Whether `va_list` appears anywhere in a type.
    fn uses_va_list(&self, ty: Ty) -> bool {
        match ty {
            Ty::VaList => true,
            Ty::Pointer(id) => self.uses_va_list(self.program.types.pointer_type(id).pointee),
            Ty::Array(id) => self.uses_va_list(self.program.types.array_type(id).elem),
            Ty::Func(id) => {
                let func = self.program.types.func_type(id);
                self.uses_va_list(func.ret) || func.params.iter().any(|ty| self.uses_va_list(*ty))
            }
            _ => false,
        }
    }

    fn sp(&self, range: SourceRange) -> Span {
        self.map.span(range)
    }

    /// A name no C identifier can collide with.
    ///
    /// `Span::mixed_site()` gives the identifier this crate's own hygiene, so
    /// even a C variable spelled `__cinrs_tmp0` refers to something else.
    fn temporary(&mut self) -> Ident {
        let name = format!("__cinrs_tmp{}", self.temporaries);
        self.temporaries += 1;
        Ident::new(&name, Span::mixed_site())
    }

    /// Builds a Rust label such as `'l0`.
    fn label(&self, name: &str, span: Span) -> TokenStream {
        let mut tick = Punct::new('\'', Spacing::Joint);
        tick.set_span(span);
        let mut out = TokenStream::new();
        out.extend([
            TokenTree::Punct(tick),
            TokenTree::Ident(Ident::new(name, span)),
        ]);
        out
    }

    // -- types --------------------------------------------------------------

    /// The Rust type a C type maps to, always fully qualified.
    fn ty(&self, ty: Ty, span: Span) -> TokenStream {
        let name = match ty {
            // Only reachable on the error path, where a `compile_error!` is
            // already going out; the unit type keeps the stub items parseable.
            Ty::Void | Ty::Error => return quote_spanned! {span=> () },
            Ty::Bool => return quote_spanned! {span=> bool },
            Ty::Char => "c_char",
            Ty::SChar => "c_schar",
            Ty::UChar => "c_uchar",
            Ty::Short => "c_short",
            Ty::UShort => "c_ushort",
            Ty::Int => "c_int",
            Ty::UInt => "c_uint",
            Ty::Long => "c_long",
            Ty::ULong => "c_ulong",
            Ty::LongLong => "c_longlong",
            Ty::ULongLong => "c_ulonglong",
            Ty::Float => "c_float",
            Ty::Double => "c_double",
            // The lifetime is elided: `VaList` only ever appears as the type of
            // a parameter or of a local, where elision does the right thing.
            Ty::VaList => "VaList",
            Ty::Pointer(id) => {
                let pointer = self.program.types.pointer_type(id);
                if let Ty::Func(func) = pointer.pointee {
                    // C's function pointers can be null, and Rust's cannot;
                    // `Option` is how the two are reconciled, and it has the
                    // same representation.
                    let signature = self.fn_ty(func, span);
                    return quote_spanned! {span=> ::core::option::Option<#signature> };
                }
                let pointee = self.pointee_ty(pointer.pointee, span);
                return if pointer.konst {
                    quote_spanned! {span=> *const #pointee }
                } else {
                    quote_spanned! {span=> *mut #pointee }
                };
            }
            Ty::Array(id) => {
                let array = self.program.types.array_type(id);
                let elem = self.ty(array.elem, span);
                // A variable length array's object *is* a pointer to its first
                // element: the elements themselves live in a hidden `Vec`, and
                // nothing in the generated code ever names the array as a
                // value. See [`ir::VlaDef`].
                if array.vla {
                    return quote_spanned! {span=> *mut #elem };
                }
                let len = usize_literal(array.len, span);
                let inner = quote_spanned! {span=> #elem ; #len };
                return bracketed(inner, span);
            }
            Ty::Func(id) => return self.fn_ty(id, span),
            Ty::Record(id) => {
                let name = c_ident(&self.program.types.record(id).rust_name, span);
                return quote_spanned! {span=> #name };
            }
            Ty::Enum(id) => {
                let name = c_ident(&self.program.types.enum_def(id).rust_name, span);
                return quote_spanned! {span=> #name };
            }
        };
        let ident = Ident::new(name, span);
        quote_spanned! {span=> ::core::ffi::#ident }
    }

    /// `unsafe extern "C" fn(…) -> R`, the type a function pointer wraps.
    ///
    /// A function type with no prototype has no parameters to write, so it
    /// comes out as `unsafe extern "C" fn() -> R`; what a call through one
    /// really passes is written at the call site instead. See
    /// [`Codegen::call`].
    fn fn_ty(&self, id: ir::FuncTyId, span: Span) -> TokenStream {
        let func = self.program.types.func_type(id).clone();
        self.fn_ptr_ty(&func.params, func.variadic, func.ret, span)
    }

    // -- the heap the emulated automatic storage comes from -------------------

    /// The crate the `Vec` behind a variable length array and `alloca` comes
    /// from.
    ///
    /// Everything else the expansion generates is `core`-only; these two need
    /// an allocator, which is `std` in an ordinary crate and `alloc` in one
    /// that said `#pragma cinrs no_std` (and therefore wrote
    /// `extern crate alloc;` itself, since a procedural macro cannot add one).
    fn alloc_crate(&self, span: Span) -> Ident {
        let name = if self.program.no_std { "alloc" } else { "std" };
        Ident::new(name, span)
    }

    /// `::std::vec::Vec<T>`.
    fn vec_ty(&self, elem: TokenStream, span: Span) -> TokenStream {
        let krate = self.alloc_crate(span);
        quote_spanned! {span=> ::#krate::vec::Vec<#elem> }
    }

    /// `::std::vec::Vec::new()`.
    fn vec_new(&self, span: Span) -> TokenStream {
        let krate = self.alloc_crate(span);
        quote_spanned! {span=> ::#krate::vec::Vec::new() }
    }

    /// `::std::vec::from_elem(value, len)`, which is what `vec![value; len]`
    /// expands to; a procedural macro is better off naming the function.
    fn vec_of(&self, value: TokenStream, len: TokenStream, span: Span) -> TokenStream {
        let krate = self.alloc_crate(span);
        quote_spanned! {span=> ::#krate::vec::from_elem(#value, #len) }
    }

    /// The Rust type a *pointee* maps to.
    ///
    /// `void *` is the one place where C's `void` is not Rust's `()`: it stands
    /// for "some object of unknown type", which is exactly what
    /// [`core::ffi::c_void`] is for. Its size is one byte, so the `void *`
    /// arithmetic GCC allows keeps working.
    fn pointee_ty(&self, ty: Ty, span: Span) -> TokenStream {
        if ty.is_void() {
            let ident = Ident::new("c_void", span);
            return quote_spanned! {span=> ::core::ffi::#ident };
        }
        self.ty(ty, span)
    }

    // -- items --------------------------------------------------------------

    /// The `struct`, `union`, `enum` and `typedef` items of the unit.
    fn type_items(&mut self) -> TokenStream {
        let mut out = TokenStream::new();
        for record in self.program.types.records() {
            if !record.emit {
                continue;
            }
            out.extend(self.record_item(record));
        }
        for def in self.program.types.enums() {
            if !def.emit {
                continue;
            }
            let span = self.sp(def.range);
            let name = c_ident(&def.rust_name, span);
            let int = self.ty(Ty::Int, span);
            let attrs = allow_attr(span);
            // C says an enumerated type is compatible with an implementation
            // defined integer type; every ABI this targets picks `int`.
            out.extend(quote_spanned! {span=> #attrs pub type #name = #int; });
        }
        for constant in &self.program.enum_constants {
            let span = self.sp(constant.range);
            let name = c_ident(&constant.rust_name, span);
            let ty = self.ty(constant.ty, span);
            let value = bare_int_literal(constant.value, constant.ty, span);
            let attrs = allow_attr(span);
            out.extend(quote_spanned! {span=> #attrs pub const #name: #ty = #value; });
        }
        for typedef in &self.program.typedefs {
            // No alias is generated for `va_list`, which is what the
            // `typedef` in <stdarg.h> writes. `core::ffi::VaList` carries the
            // lifetime of the frame it reads, so `pub type va_list = VaList;`
            // does not even parse — and nothing needs the alias, since every
            // generated signature names the type directly.
            if self.uses_va_list(typedef.ty) {
                continue;
            }
            let span = self.sp(typedef.range);
            let name = c_ident(&typedef.rust_name, span);
            let ty = self.ty(typedef.ty, span);
            let attrs = allow_attr(span);
            out.extend(quote_spanned! {span=> #attrs pub type #name = #ty; });
        }
        out
    }

    fn record_item(&self, record: &ir::RecordDef) -> TokenStream {
        let span = self.sp(record.range);
        let name = c_ident(&record.rust_name, span);
        let attrs = allow_attr(span);
        // `Copy` is what makes a C struct behave like one: assigning it,
        // passing it and returning it all copy the bytes.
        let derives = match (record.align, record.packed) {
            // `__attribute__((packed))` and `#pragma pack(N)` are exactly
            // Rust's own `packed(N)`: every field's alignment is capped at N,
            // and so is the record's.
            (_, Some(1)) => quote_spanned! {span=> #[repr(C, packed)] #[derive(Copy, Clone)] },
            (_, Some(pack)) => {
                let pack = usize_literal(pack, span);
                quote_spanned! {span=>
                    #[repr(C, packed(#pack))] #[derive(Copy, Clone)]
                }
            }
            // `_Alignas` or `aligned(N)` on a member, or a bit-field whose
            // type is stricter than any field the item really has, is honoured
            // by raising the *record's* alignment; sema has already placed the
            // members where that leaves them.
            (Some(align), None) => {
                let align = usize_literal(align, span);
                quote_spanned! {span=>
                    #[repr(C, align(#align))] #[derive(Copy, Clone)]
                }
            }
            (None, None) => quote_spanned! {span=> #[repr(C)] #[derive(Copy, Clone)] },
        };
        if !record.complete {
            // A tag that is never completed can still be pointed at. An empty
            // body is the closest Rust has to C's incomplete type.
            return quote_spanned! {span=>
                #attrs #derives pub struct #name { _incomplete: [u8; 0] }
            };
        }
        let mut fields = TokenStream::new();
        for rust_field in &record.rust_fields {
            match rust_field {
                ir::RustField::Member(index) => {
                    let field = &record.fields[*index];
                    let fspan = self.sp(field.range);
                    let fname = c_ident(&field.name, fspan);
                    let fty = self.ty(field.ty, fspan);
                    // Members are `pub` so Rust code can build and read the
                    // value.
                    fields.extend(quote_spanned! {fspan=> pub #fname: #fty, });
                }
                ir::RustField::Bits { name, bytes, .. } | ir::RustField::Pad { name, bytes } => {
                    let fname = Ident::new(name, span);
                    let len = usize_literal(*bytes, span);
                    fields.extend(quote_spanned! {span=> pub #fname: [u8; #len], });
                }
                ir::RustField::Align { name, align } => {
                    let fname = Ident::new(name, span);
                    let unit = unsigned_rust_ty((*align * 8) as u32, span);
                    fields.extend(quote_spanned! {span=> pub #fname: [#unit; 0], });
                }
            }
        }
        if record.rust_fields.is_empty() && record.kind == RecordKind::Union {
            // GCC gives an empty `union` a size of zero, and so does an empty
            // Rust `struct`; a Rust `union` has to have at least one field, so
            // this is the one place the two kinds are generated differently.
            fields.extend(quote_spanned! {span=> pub __cinrs_empty: [u8; 0], });
        }
        let body = braced(fields, span);
        let item = match record.kind {
            RecordKind::Struct => quote_spanned! {span=> #attrs #derives pub struct #name #body },
            RecordKind::Union => quote_spanned! {span=> #attrs #derives pub union #name #body },
        };
        let accessors = self.bit_field_accessors(record, span);
        quote_spanned! {span=> #item #accessors }
    }

    /// The `impl` block holding one getter and one setter per named bit-field.
    ///
    /// The bits are not a field, so this is the only way to reach them — from
    /// the generated code and from Rust alike. Everything is written out
    /// inline: no helper type, no runtime, nothing to look up.
    fn bit_field_accessors(&self, record: &ir::RecordDef, span: Span) -> TokenStream {
        let mut methods = TokenStream::new();
        for field in &record.fields {
            let Some(bits) = &field.bits else {
                continue;
            };
            let fspan = self.sp(field.range);
            methods.extend(self.bit_field_getter(record, field, bits, fspan));
            methods.extend(self.bit_field_setter(record, field, bits, fspan));
        }
        if methods.is_empty() {
            return TokenStream::new();
        }
        let name = c_ident(&record.rust_name, span);
        let attrs = allow_attr(span);
        quote_spanned! {span=> #attrs impl #name { #methods } }
    }

    /// Reads the bytes a bit-field overlaps into a `u64`.
    ///
    /// The unit rule guarantees a field never spans more than eight bytes, so
    /// one `u64` is always enough.
    fn bit_field_window(&self, bits: &ir::BitField, span: Span) -> (TokenStream, u32, u64) {
        let storage = Ident::new(&bits.storage, span);
        let start = bits.offset_in_storage();
        let first = start / 8;
        let shift = (start % 8) as u32;
        let count = (shift + bits.width).div_ceil(8);
        let mut read = TokenStream::new();
        for step in 0..count {
            let index = usize_literal(first + u64::from(step), span);
            let byte = if count == 1 {
                quote_spanned! {span=> self.#storage[#index] as u64 }
            } else {
                quote_spanned! {span=> (self.#storage[#index] as u64) }
            };
            read.extend(if step == 0 {
                byte
            } else {
                let by = usize_literal(u64::from(step) * 8, span);
                quote_spanned! {span=> | (#byte << #by) }
            });
        }
        // The mask of the field's bits inside that window. The unit rule keeps
        // `shift + width` at 64 or less, and writing it this way makes that
        // true of the shift as well.
        let mask = mask_of(shift + bits.width) & !mask_of(shift);
        (read, shift, mask)
    }

    fn bit_field_getter(
        &self,
        record: &ir::RecordDef,
        field: &ir::Field,
        bits: &ir::BitField,
        span: Span,
    ) -> TokenStream {
        let (read, shift, _) = self.bit_field_window(bits, span);
        let ty = self.ty(field.ty, span);
        let name = c_ident(&bits.getter, span);
        let mask = hex_literal(mask_of(bits.width), span);
        let shifted = if shift == 0 {
            quote_spanned! {span=> raw & #mask }
        } else {
            let by = usize_literal(u64::from(shift), span);
            quote_spanned! {span=> (raw >> #by) & #mask }
        };
        let value = if field.ty.is_bool() {
            quote_spanned! {span=> value != 0 }
        } else if !bits.signed || bits.width == 64 {
            quote_spanned! {span=> value as #ty }
        } else {
            // Sign extension: shift the field's top bit up to the sign bit of
            // an `i64` and let the arithmetic shift bring it back down.
            let by = usize_literal(u64::from(64 - bits.width), span);
            quote_spanned! {span=> (((value << #by) as i64) >> #by) as #ty }
        };
        let body = self.accessor_body(
            record,
            quote_spanned! {span=>
                let raw: u64 = #read;
                let value: u64 = #shifted;
                #value
            },
            span,
        );
        quote_spanned! {span=>
            #[inline]
            pub fn #name(&self) -> #ty { #body }
        }
    }

    fn bit_field_setter(
        &self,
        record: &ir::RecordDef,
        field: &ir::Field,
        bits: &ir::BitField,
        span: Span,
    ) -> TokenStream {
        let (read, shift, mask) = self.bit_field_window(bits, span);
        let ty = self.ty(field.ty, span);
        let name = c_ident(&bits.setter, span);
        let storage = Ident::new(&bits.storage, span);
        let value = Ident::new("value", span);
        let field_mask = hex_literal(mask, span);
        let keep = hex_literal(!mask, span);
        let shifted = if shift == 0 {
            quote_spanned! {span=> (#value as u64) & #field_mask }
        } else {
            let by = usize_literal(u64::from(shift), span);
            quote_spanned! {span=> ((#value as u64) << #by) & #field_mask }
        };
        let start = bits.offset_in_storage();
        let first = start / 8;
        let count = (shift + bits.width).div_ceil(8);
        let mut writes = TokenStream::new();
        for step in 0..count {
            let index = usize_literal(first + u64::from(step), span);
            if step == 0 {
                writes.extend(quote_spanned! {span=> self.#storage[#index] = raw as u8; });
            } else {
                let by = usize_literal(u64::from(step) * 8, span);
                writes.extend(quote_spanned! {span=> self.#storage[#index] = (raw >> #by) as u8; });
            }
        }
        let body = self.accessor_body(
            record,
            quote_spanned! {span=>
                let bits: u64 = #shifted;
                let raw: u64 = #read;
                let raw: u64 = (raw & #keep) | bits;
                #writes
            },
            span,
        );
        quote_spanned! {span=>
            #[inline]
            pub fn #name(&mut self, #value: #ty) { #body }
        }
    }

    /// Wraps an accessor body in `unsafe` where reading the storage needs it,
    /// which is exactly when the record is a `union`.
    fn accessor_body(&self, record: &ir::RecordDef, body: TokenStream, span: Span) -> TokenStream {
        if record.kind == RecordKind::Union {
            let block = braced(body, span);
            return quote_spanned! {span=> unsafe #block };
        }
        body
    }

    /// The `extern` block declaring everything the unit does not define.
    fn extern_block(&mut self) -> TokenStream {
        if !self.program.has_externs() {
            return TokenStream::new();
        }
        let span = self.map.span(SourceRange::at(0));
        let mut items = TokenStream::new();
        for id in &self.program.externs {
            let object = self.program.object(*id);
            let Storage::Extern { item_name } = &object.storage else {
                continue;
            };
            let ospan = self.sp(object.range);
            let rust_name = Ident::new(&self.program.extern_name(item_name), ospan);
            let ty = self.ty(object.ty, ospan);
            // An `__asm__("symbol")` label renames the declaration, which is
            // exactly what `#[link_name]` already says.
            let symbol = object.asm_label.as_deref().unwrap_or(item_name);
            let link = link_name(symbol, ospan);
            items.extend(quote_spanned! {ospan=> #link pub static mut #rust_name: #ty; });
        }
        for func in &self.program.functions {
            if !func.is_extern() || self.beyond_toolchain(func) {
                continue;
            }
            let fspan = self.sp(func.range);
            let rust_name = Ident::new(&self.program.extern_name(&func.name), fspan);
            let params = self.extern_params(func, fspan);
            let ret = if func.sig.ret.is_void() {
                TokenStream::new()
            } else {
                let ty = self.ty(func.sig.ret, fspan);
                quote_spanned! {fspan=> -> #ty }
            };
            let symbol = func.asm_label.as_deref().unwrap_or(&func.name);
            let link = link_name(symbol, fspan);
            items.extend(quote_spanned! {fspan=> #link pub fn #rust_name(#params) #ret; });
        }
        let attrs = allow_attr(span);
        let links = self.link_attrs(span);
        quote_spanned! {span=> #attrs #links unsafe extern "C" { #items } }
    }

    /// `#[link(name = "…")]` for every `#pragma cinrs link` the unit wrote.
    ///
    /// Nothing here is needed for the C library itself, which the Rust runtime
    /// already links; this is for the program that calls into something else.
    fn link_attrs(&self, span: Span) -> TokenStream {
        let mut out = TokenStream::new();
        for name in &self.program.link_libraries {
            let mut literal = Literal::string(name);
            literal.set_span(span);
            out.extend(quote_spanned! {span=> #[link(name = #literal)] });
        }
        out
    }

    fn extern_params(&self, func: &Function, span: Span) -> TokenStream {
        let mut params = TokenStream::new();
        for (index, ty) in func.sig.params.iter().enumerate() {
            if index > 0 {
                params.extend(quote_spanned! {span=> , });
            }
            let ty = self.ty(*ty, span);
            match func.param_names.get(index).and_then(|n| n.as_ref()) {
                Some(name) => {
                    let name = c_ident(name, span);
                    params.extend(quote_spanned! {span=> #name: #ty });
                }
                None => params.extend(quote_spanned! {span=> _: #ty }),
            }
        }
        if func.sig.variadic {
            if !func.sig.params.is_empty() {
                params.extend(quote_spanned! {span=> , });
            }
            params.extend(quote_spanned! {span=> ... });
        }
        params
    }

    fn static_item(&mut self, var: &ir::StaticVar) -> TokenStream {
        let object = self.program.object(var.object);
        let span = self.sp(object.range);
        let Storage::Static {
            item_name,
            exported,
        } = &object.storage
        else {
            return TokenStream::new();
        };
        let name = c_ident(item_name, span);
        let ty = self.ty(object.ty, span);
        let init = self.static_init(&var.init, object.ty, span);
        let attrs = allow_attr(span);
        let (vis, export) = if *exported {
            let export = if self.program.export {
                let symbol = object.asm_label.as_deref().unwrap_or(&object.name);
                export_attr(symbol, &name, span)
            } else {
                TokenStream::new()
            };
            (quote_spanned! {span=> pub }, export)
        } else {
            (TokenStream::new(), TokenStream::new())
        };
        let section = match &object.section {
            Some(section) => {
                let mut literal = Literal::string(section);
                literal.set_span(span);
                quote_spanned! {span=> #[unsafe(link_section = #literal)] }
            }
            None => TokenStream::new(),
        };
        // `static mut` rather than a cell: C code assigns to globals from
        // anywhere, and reading or writing one directly (never taking a
        // reference) is what keeps edition 2024's `static_mut_refs` quiet.
        quote_spanned! {span=>
            #attrs
            #export
            #section
            #vis static mut #name: #ty = #init;
        }
    }

    /// The initialiser of a `static mut`, wrapped in `unsafe` when it needs to
    /// be (`mem::zeroed`, or the address of another `static mut`).
    fn static_init(&mut self, expr: &Expr, ty: Ty, span: Span) -> TokenStream {
        let tokens = self.expr_at(expr, ty);
        if needs_unsafe(expr) {
            let block = braced(tokens, span);
            return quote_spanned! {span=> unsafe #block };
        }
        tokens
    }

    fn signature(&mut self, func: &Function) -> TokenStream {
        let span = self.sp(func.range);
        let name = c_ident(&func.name, span);
        let mut params = TokenStream::new();
        // A definition whose parameters did not check out may have fewer than
        // the signature says; either way the list still has to have the right
        // shape, so it falls back to names of our own.
        let named = func.params.len() == func.sig.params.len();
        for (index, ty) in func.sig.params.iter().enumerate() {
            if index > 0 {
                params.extend(quote_spanned! {span=> , });
            }
            if named {
                let id = func.params[index];
                let object = self.program.object(id);
                let pspan = self.sp(object.range);
                let pname = self.object_ident(id, pspan);
                let pty = self.ty(*ty, pspan);
                params.extend(quote_spanned! {pspan=> mut #pname: #pty });
            } else {
                let pname = Ident::new(&format!("__cinrs_arg{index}"), Span::mixed_site());
                let pty = self.ty(*ty, span);
                params.extend(quote_spanned! {span=> #pname: #pty });
            }
        }
        if func.sig.variadic {
            if !func.sig.params.is_empty() {
                params.extend(quote_spanned! {span=> , });
            }
            // The variable part of the argument list arrives as one more
            // parameter. It is never advanced: `va_start` and every `va_list`
            // local copy it, so it stays the list as the caller left it.
            let name = self.va_ident();
            params.extend(quote_spanned! {span=> #name: ... });
        }
        let ret = if func.sig.ret.is_void() {
            TokenStream::new()
        } else {
            let ty = self.ty(func.sig.ret, span);
            quote_spanned! {span=> -> #ty }
        };
        let attrs = allow_attr(span);
        // A definition is a Rust item rather than a C symbol unless the unit
        // asked for real symbols, so an `__asm__("name")` label on one only
        // means something there — and there it names the symbol the item
        // takes, which is what `#[unsafe(export_name)]` says. On a *declaration*
        // the label is always honoured, through the `extern` block's
        // `#[link_name]`.
        let exported = !func.is_static && self.program.export;
        let vis = if func.is_static {
            TokenStream::new()
        } else {
            quote_spanned! {span=> pub }
        };
        let export = if exported {
            let symbol = func.asm_label.as_deref().unwrap_or(&func.name);
            export_attr(symbol, &name, span)
        } else {
            TokenStream::new()
        };
        // `#[inline]` is ignored on an exported function, and saying so is
        // `rustc`'s job rather than the user's to read: leave it out.
        let inline = match func.inline_hint {
            Some(_) if exported => TokenStream::new(),
            Some(ir::InlineHint::Always) => quote_spanned! {span=> #[inline(always)] },
            Some(ir::InlineHint::Never) => quote_spanned! {span=> #[inline(never)] },
            None if func.is_inline && !exported => quote_spanned! {span=> #[inline] },
            None => TokenStream::new(),
        };
        let cold = if func.cold {
            quote_spanned! {span=> #[cold] }
        } else {
            TokenStream::new()
        };
        let deprecated = match &func.deprecated {
            Some(Some(message)) => {
                let mut literal = Literal::string(message);
                literal.set_span(span);
                quote_spanned! {span=> #[deprecated(note = #literal)] }
            }
            Some(None) => quote_spanned! {span=> #[deprecated] },
            None => TokenStream::new(),
        };
        let section = match &func.section {
            Some(section) => {
                let mut literal = Literal::string(section);
                literal.set_span(span);
                quote_spanned! {span=> #[unsafe(link_section = #literal)] }
            }
            None => TokenStream::new(),
        };
        quote_spanned! {span=>
            #attrs
            #export
            #inline
            #cold
            #deprecated
            #section
            #vis unsafe extern "C" fn #name(#params) #ret
        }
    }

    /// The `static` that puts a `constructor` or `destructor` function into the
    /// table the runtime walks before `main` (or after it).
    ///
    /// ELF has `.init_array` and `.fini_array`, and Mach-O has
    /// `__DATA,__mod_init_func` and `__mod_term_func`; nothing else this crate
    /// can name has such a table, so a program that asks for one elsewhere is
    /// told so rather than quietly built without it.
    fn init_array_item(&mut self, func: &Function, kind: ir::InitKind) -> TokenStream {
        let span = self.sp(func.range);
        let name = c_ident(&func.name, span);
        let signature = self.function_pointer_ty(func, span);
        let item = Ident::new(
            &format!(
                "__CINRS_INIT_{:08x}_{}",
                self.program.unit_id as u32, func.name
            ),
            span,
        );
        let elf = match kind {
            ir::InitKind::Constructor => ".init_array",
            ir::InitKind::Destructor => ".fini_array",
        };
        let apple = match kind {
            ir::InitKind::Constructor => "__DATA,__mod_init_func",
            ir::InitKind::Destructor => "__DATA,__mod_term_func",
        };
        let mut elf_literal = Literal::string(elf);
        elf_literal.set_span(span);
        let mut apple_literal = Literal::string(apple);
        apple_literal.set_span(span);
        let attrs = allow_attr(span);
        // The section name is the one thing about this that is not portable,
        // so it is chosen at compile time rather than assumed.
        quote_spanned! {span=>
            #attrs
            #[used]
            #[cfg_attr(target_vendor = "apple", unsafe(link_section = #apple_literal))]
            #[cfg_attr(not(target_vendor = "apple"), unsafe(link_section = #elf_literal))]
            static #item: #signature = #name;
        }
    }

    /// The one check a unit with a `constructor` or a `destructor` carries.
    ///
    /// A procedural macro is compiled for the *host*, so it cannot know which
    /// target the code it generates is for; the check therefore has to be part
    /// of the expansion. It is emitted once per unit rather than once per
    /// function, since it says the same thing either way.
    fn init_array_guard(&self) -> TokenStream {
        let span = self.map.span(SourceRange::at(0));
        let attrs = allow_attr(span);
        quote_spanned! {span=>
            #attrs
            const _: () = {
                #[cfg(not(any(target_os = "linux", target_os = "android",
                              target_os = "freebsd", target_os = "netbsd",
                              target_os = "openbsd", target_os = "dragonfly",
                              target_vendor = "apple")))]
                ::core::compile_error!(
                    "'constructor' and 'destructor' need a target whose runtime walks an initialiser table (ELF or Mach-O)"
                );
            };
        }
    }

    fn function_item(&mut self, func: &Function) -> TokenStream {
        let span = self.sp(func.range);
        let Some(body) = &func.body else {
            return TokenStream::new();
        };
        self.enter_function(func);
        let signature = self.signature(func);
        let body = match body {
            Body::Structured(stmts) => self.stmts(stmts),
            Body::Cfg(cfg) => self.cfg_body(cfg, span),
        };
        // `alloca`'s memory belongs to the function, not to the block the call
        // was written in, so the arena is opened here and dropped by whichever
        // `return` runs.
        let arena = if func.uses_alloca {
            self.alloca_arena(span)
        } else {
            TokenStream::new()
        };
        // One `unsafe` block around the whole body: in edition 2024 the body of
        // an `unsafe fn` is not itself an unsafe block any more.
        quote_spanned! {span=>
            #signature {
                unsafe { #arena #body }
            }
        }
    }

    /// The arena `alloca` allocates out of, at the top of a function that
    /// calls it.
    ///
    /// One `Vec` per call, of `u128` so that every block is 16-byte aligned —
    /// the alignment a real `alloca` gives — and all of them freed together
    /// when the arena is dropped, which is when the function returns.
    fn alloca_arena(&self, span: Span) -> TokenStream {
        let name = self.alloca_ident();
        let block = self.vec_ty(quote_spanned! {span=> u128 }, span);
        let arena = self.vec_ty(block, span);
        let empty = self.vec_new(span);
        quote_spanned! {span=> let mut #name: #arena = #empty; }
    }

    /// The name of that arena, in this crate's own hygiene.
    fn alloca_ident(&self) -> Ident {
        Ident::new("__cinrs_alloca", Span::mixed_site())
    }

    fn stub_item(&mut self, func: &Function) -> TokenStream {
        let span = self.sp(func.range);
        self.enter_function(func);
        let signature = self.signature(func);
        quote_spanned! {span=>
            #signature {
                unsafe { ::core::unreachable!() }
            }
        }
    }

    /// Resets the per-function state before a body is generated.
    fn enter_function(&mut self, func: &Function) {
        self.ret_ty = func.sig.ret;
        self.in_cfg = matches!(func.body, Some(Body::Cfg(_)));
        self.temporaries = 0;
        self.continue_styles.clear();
        self.local_names.clear();

        // Every object this function binds with a `let` or a parameter, so
        // that one that would shadow a file-scope item can be renamed apart
        // and the rename can be checked against the others. Sema's list is
        // what makes a local declared inside a statement expression — which no
        // walk over the *statements* would reach — part of it.
        let mut bound: Vec<ir::ObjectId> = func.params.clone();
        if let Some(Body::Cfg(cfg)) = &func.body {
            for local in &cfg.locals {
                self.local_names
                    .insert(local.object, local.rust_name.clone());
            }
        }
        bound.extend(func.locals.iter().copied());
        self.rename_shadowing(&bound);

        self.va_source = if func.sig.variadic {
            VaSource::Ellipsis
        } else {
            match func
                .params
                .iter()
                .find(|id| self.program.object(**id).ty.is_va_list())
            {
                Some(id) => VaSource::Param(*id),
                None => VaSource::None,
            }
        };
    }

    /// The name a local or parameter would be generated under before the
    /// shadowing check.
    fn plain_local_name(&self, id: ir::ObjectId) -> String {
        match self.local_names.get(&id) {
            Some(name) => name.clone(),
            None => self.program.object(id).name.clone(),
        }
    }

    /// Renames the bindings of the current function whose names would shadow a
    /// file-scope item.
    ///
    /// The new name is the C one with `_1`, `_2`, … appended, the same shape
    /// the [CFG lowering](crate::cfg) uses when it hoists two locals of the
    /// same name into one scope, and it is checked against the function's
    /// other bindings so that a rename never captures one of them.
    fn rename_shadowing(&mut self, bound: &[ir::ObjectId]) {
        if self.reserved.is_empty() {
            return;
        }
        let mut used: HashSet<String> = bound.iter().map(|id| self.plain_local_name(*id)).collect();
        for id in bound {
            let base = self.plain_local_name(*id);
            if !self.reserved.contains(&base) {
                continue;
            }
            let name = (1u32..)
                .map(|n| format!("{base}_{n}"))
                .find(|c| !used.contains(c) && !self.reserved.contains(c))
                .expect("the sequence of candidates is unbounded");
            used.insert(name.clone());
            self.local_names.insert(*id, name);
        }
    }

    /// The name of the synthetic `...` parameter.
    ///
    /// Mixed-site hygiene keeps it distinct from a C variable of the same
    /// name, however unlikely one is.
    fn va_ident(&self) -> Ident {
        Ident::new("__cinrs_va", Span::mixed_site())
    }

    /// A fresh copy of the argument list the function was called with.
    fn va_pristine(&mut self, span: Span) -> TokenStream {
        match self.va_source {
            VaSource::Param(id) => {
                let name = self.object_ident(id, span);
                quote_spanned! {span=> #name.clone() }
            }
            // `VaSource::None` cannot reach codegen: sema refuses a `va_list`
            // that has nothing to copy.
            _ => {
                let name = self.va_ident();
                quote_spanned! {span=> #name.clone() }
            }
        }
    }

    /// The Rust name an object is generated under.
    fn object_ident(&self, id: ir::ObjectId, span: Span) -> Ident {
        let object = self.program.object(id);
        match &object.storage {
            Storage::Automatic => match self.local_names.get(&id) {
                Some(name) => c_ident(name, span),
                None => c_ident(&object.name, span),
            },
            // A `static mut` is used as a place, never referenced, so
            // edition 2024's `static_mut_refs` lint has nothing to say.
            Storage::Static { item_name, .. } => c_ident(item_name, span),
            Storage::Extern { item_name } => Ident::new(&self.program.extern_name(item_name), span),
        }
    }

    // -- the control-flow-graph form ----------------------------------------

    /// Emits a [CFG](crate::cfg) body as a state machine.
    ///
    /// Every arm of the `match` ends in `continue 'cfg` or in a `return`, so
    /// the loop never finishes and the function needs no value after it.
    fn cfg_body(&mut self, cfg: &Cfg, span: Span) -> TokenStream {
        let mut out = TokenStream::new();
        for local in &cfg.locals {
            let object = self.program.object(local.object);
            let ospan = self.sp(object.range);
            let name = self.object_ident(local.object, ospan);
            // The hidden `Vec` of a variable length array starts out empty and
            // is replaced where the declaration was written, which is also
            // what frees the storage of a previous pass over it.
            let (ty, init) = if object.vla_storage {
                let elem = self.ty(object.ty, ospan);
                (self.vec_ty(elem, ospan), self.vec_new(ospan))
            } else if object.ty.is_va_list() {
                // A `va_list` has no zero value; it starts out as a copy of
                // the list the function was called with, exactly as it does
                // when the declaration stays where it was written.
                (self.ty(object.ty, ospan), self.va_pristine(ospan))
            } else {
                (
                    self.ty(object.ty, ospan),
                    self.zero_tokens(object.ty, ospan),
                )
            };
            out.extend(quote_spanned! {ospan=> let mut #name: #ty = #init; });
        }
        let state = self.state_ident();
        let label = self.cfg_label();
        let mut arms = TokenStream::new();
        for (index, block) in cfg.blocks.iter().enumerate() {
            let bspan = self.block_span(block, span);
            let pattern = state_literal(index, bspan);
            let body = self.block_tokens(block, bspan);
            arms.extend(quote_spanned! {bspan=> #pattern => { #body } });
        }
        arms.extend(quote_spanned! {span=> _ => ::core::unreachable!(), });
        quote_spanned! {span=>
            #out
            let mut #state: u32 = 0;
            #label: loop {
                match #state { #arms }
            }
        }
    }

    /// The state variable, in this crate's own hygiene.
    fn state_ident(&self) -> Ident {
        Ident::new("__cinrs_state", Span::mixed_site())
    }

    fn cfg_label(&self) -> TokenStream {
        self.label("cfg", Span::mixed_site())
    }

    /// Where a block's tokens are attributed to: the first thing in it that
    /// came from the C source.
    fn block_span(&self, block: &BasicBlock, fallback: Span) -> Span {
        if let Some(range) = block.stmts.iter().find_map(|s| self.stmt_range(s)) {
            return self.sp(range);
        }
        match &block.term {
            Terminator::Jump { range, .. }
            | Terminator::Switch { range, .. }
            | Terminator::Return { range, .. } => self.sp(*range),
            Terminator::Branch { cond, .. } => self.sp(cond.range),
            Terminator::Unreachable => fallback,
        }
    }

    fn block_tokens(&mut self, block: &BasicBlock, span: Span) -> TokenStream {
        let mut out = self.stmts(&block.stmts);
        let label = self.cfg_label();
        match &block.term {
            Terminator::Jump { target, range } => {
                let jump = self.enter_block(*target, self.sp(*range));
                out.extend(quote_spanned! {span=> #jump continue #label; });
            }
            Terminator::Branch {
                cond,
                then_blk,
                else_blk,
            } => {
                let cspan = self.sp(cond.range);
                let test = self.condition(cond).at_condition(cspan);
                let then_tokens = self.enter_block(*then_blk, cspan);
                let else_tokens = self.enter_block(*else_blk, cspan);
                out.extend(quote_spanned! {cspan=>
                    if #test { #then_tokens } else { #else_tokens }
                    continue #label;
                });
            }
            Terminator::Switch {
                value,
                cases,
                default,
                range,
            } => {
                let sspan = self.sp(*range);
                let scrutinee = self.expr(value).at(prec::UNARY, sspan);
                let mut arms = TokenStream::new();
                for (target, values) in group_cases(cases) {
                    let mut pattern = TokenStream::new();
                    for (index, case) in values.iter().enumerate() {
                        if index > 0 {
                            pattern.extend(quote_spanned! {sspan=> | });
                        }
                        pattern.extend(case_pattern(*case, value.ty, sspan));
                    }
                    let enter = self.enter_block(target, sspan);
                    arms.extend(quote_spanned! {sspan=> #pattern => { #enter } });
                }
                let enter = self.enter_block(*default, sspan);
                arms.extend(quote_spanned! {sspan=> _ => { #enter } });
                out.extend(quote_spanned! {sspan=>
                    match #scrutinee { #arms }
                    continue #label;
                });
            }
            Terminator::Return { value, range } => {
                let rspan = self.sp(*range);
                match value {
                    Some(value) => {
                        let ret = self.ret_ty;
                        let tokens = self.expr_at(value, ret);
                        out.extend(quote_spanned! {rspan=> return #tokens; });
                    }
                    None => out.extend(quote_spanned! {rspan=> return; }),
                }
            }
            Terminator::Unreachable => {
                out.extend(quote_spanned! {span=> ::core::unreachable!(); });
            }
        }
        out
    }

    /// The assignment that moves the state machine to `target`.
    fn enter_block(&self, target: BlockId, span: Span) -> TokenStream {
        let state = self.state_ident();
        let value = state_literal(target.0 as usize, span);
        quote_spanned! {span=> #state = #value; }
    }

    // -- statements ---------------------------------------------------------

    fn stmts(&mut self, stmts: &[Stmt]) -> TokenStream {
        let mut out = TokenStream::new();
        for stmt in stmts {
            out.extend(self.stmt(stmt));
        }
        out
    }

    /// Emits a statement as a braced block, reusing the braces C already wrote
    /// when it wrote a compound statement.
    fn block_of(&mut self, stmt: &Stmt, span: Span) -> TokenStream {
        match stmt {
            Stmt::Block(items) => {
                let items = self.stmts(items);
                braced(items, span)
            }
            other => {
                let tokens = self.stmt(other);
                braced(tokens, span)
            }
        }
    }

    fn stmt(&mut self, stmt: &Stmt) -> TokenStream {
        match stmt {
            Stmt::Nop => TokenStream::new(),
            Stmt::Expr(expr) => self.expr_stmt(expr),
            Stmt::Let { object, init, .. } => {
                let name = self.object_ident(*object, self.sp(self.program.object(*object).range));
                let object = self.program.object(*object);
                let span = self.sp(object.range);
                let ty = self.ty(object.ty, span);
                let init = self.expr_at(init, object.ty);
                quote_spanned! {span=> let mut #name: #ty = #init; }
            }
            Stmt::Vla(def) => self.vla_def(def),
            Stmt::Block(items) => {
                let span = self.stmts_span(items);
                let items = self.stmts(items);
                braced(items, span)
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let span = self.sp(cond.range);
                let cond_tokens = self.condition(cond).at_condition(span);
                let then_tokens = self.block_of(then_branch, span);
                let else_tokens = match else_branch {
                    Some(branch) => {
                        let tokens = self.block_of(branch, span);
                        quote_spanned! {span=> else #tokens }
                    }
                    None => TokenStream::new(),
                };
                quote_spanned! {span=> if #cond_tokens #then_tokens #else_tokens }
            }
            Stmt::While {
                id,
                cond,
                body,
                range,
            } => {
                let span = self.sp(*range);
                self.continue_styles.insert(*id, ContinueStyle::Head);
                let label = self.loop_label(*id, span);
                let body_tokens = self.block_of(body, span);
                // `while (1)` becomes `loop`, not `while true`: only a `loop`
                // tells Rust the statement never finishes, which is what a
                // function ending in one relies on.
                if ir::is_always_true(cond) {
                    return quote_spanned! {span=> #label: loop #body_tokens };
                }
                let cond_tokens = self.condition(cond).at_condition(span);
                quote_spanned! {span=> #label: while #cond_tokens #body_tokens }
            }
            Stmt::DoWhile {
                id,
                body,
                cond,
                range,
            } => {
                let span = self.sp(*range);
                self.continue_styles.insert(*id, ContinueStyle::BodyLabel);
                let label = self.loop_label(*id, span);
                let body_label = self.loop_body_label(*id, span);
                let body_tokens = self.block_of(body, span);
                let test = if ir::is_always_true(cond) {
                    TokenStream::new()
                } else {
                    let cond_tokens = self.condition(cond).at(prec::LOWEST, span);
                    quote_spanned! {span=> if !(#cond_tokens) { break #label; } }
                };
                quote_spanned! {span=>
                    #label: loop {
                        #body_label: #body_tokens
                        #test
                    }
                }
            }
            Stmt::For {
                id,
                init,
                cond,
                step,
                body,
                range,
            } => {
                let span = self.sp(*range);
                self.continue_styles.insert(*id, ContinueStyle::BodyLabel);
                let label = self.loop_label(*id, span);
                let body_label = self.loop_body_label(*id, span);
                let init_tokens = self.stmts(init);
                let test = match cond {
                    Some(cond) if !ir::is_always_true(cond) => {
                        let tokens = self.condition(cond).at(prec::LOWEST, span);
                        quote_spanned! {span=> if !(#tokens) { break #label; } }
                    }
                    _ => TokenStream::new(),
                };
                let body_tokens = self.block_of(body, span);
                let step_tokens = match step {
                    Some(step) => self.expr_stmt(step),
                    None => TokenStream::new(),
                };
                // The whole loop is wrapped so that a C99 declaration in the
                // init clause stays scoped to the loop, as C says it is.
                quote_spanned! {span=>
                    {
                        #init_tokens
                        #label: loop {
                            #test
                            #body_label: #body_tokens
                            #step_tokens
                        }
                    }
                }
            }
            Stmt::Switch(switch) => self.switch(switch),
            // The CFG lowering consumes these; nothing can jump to a label in
            // the structured mode, so only the statement under it is left.
            Stmt::Label { body, .. } | Stmt::Case { body, .. } => self.stmt(body),
            Stmt::Goto { .. } => TokenStream::new(),
            Stmt::SwitchTree(switch) => self.stmt(&switch.body),
            Stmt::Break { target, range } => {
                let span = self.sp(*range);
                let label = match target {
                    BreakTarget::Loop(id) => self.loop_label(*id, span),
                    BreakTarget::Switch(id) => self.switch_label(*id, span),
                };
                quote_spanned! {span=> break #label; }
            }
            Stmt::Continue { id, range } => {
                let span = self.sp(*range);
                match self.continue_styles.get(id) {
                    Some(ContinueStyle::BodyLabel) => {
                        let label = self.loop_body_label(*id, span);
                        quote_spanned! {span=> break #label; }
                    }
                    _ => {
                        let label = self.loop_label(*id, span);
                        quote_spanned! {span=> continue #label; }
                    }
                }
            }
            Stmt::Return { value, range } => {
                let span = self.sp(*range);
                match value {
                    Some(value) => {
                        let ret = self.ret_ty;
                        let tokens = self.expr_at(value, ret);
                        quote_spanned! {span=> return #tokens; }
                    }
                    None => quote_spanned! {span=> return; },
                }
            }
        }
    }

    /// A variable length array's definition, `T a[n];`.
    ///
    /// Three bindings: the number of elements, evaluated exactly once here; the
    /// `Vec` that holds them, whose `Drop` at the end of the block is the
    /// object's lifetime; and the object itself, which is a pointer to the
    /// first element. In [CFG mode](crate::cfg) the three are already bound at
    /// the top of the function — Rust has no way to jump over a `let` — so what
    /// is written here is the three assignments instead.
    fn vla_def(&mut self, def: &ir::VlaDef) -> TokenStream {
        let span = self.sp(def.range);
        let object = self.program.object(def.object);
        let elem = self
            .program
            .types
            .elem(object.ty)
            .expect("a variable length array is an array type");
        let count_ty = self.program.object(def.len).ty;
        let len = self.object_ident(def.len, span);
        let store = self.object_ident(def.storage, span);
        let name = self.object_ident(def.object, span);
        let count = self.expr_at(&def.count, count_ty);
        let zero = self.zero_tokens(elem, span);
        let elem_ty = self.ty(elem, span);
        let elements = self.vec_of(zero, quote_spanned! {span=> #len as usize }, span);
        if self.in_cfg {
            return quote_spanned! {span=>
                #len = #count;
                #store = #elements;
                #name = #store.as_mut_ptr();
            };
        }
        let len_ty = self.ty(count_ty, span);
        let vec_ty = self.vec_ty(elem_ty.clone(), span);
        quote_spanned! {span=>
            let #len: #len_ty = #count;
            let mut #store: #vec_ty = #elements;
            let mut #name: *mut #elem_ty = #store.as_mut_ptr();
        }
    }

    /// A span standing for a run of statements.
    fn stmts_span(&self, stmts: &[Stmt]) -> Span {
        stmts
            .iter()
            .find_map(|s| self.stmt_range(s))
            .map_or_else(Span::call_site, |range| self.sp(range))
    }

    fn stmt_range(&self, stmt: &Stmt) -> Option<SourceRange> {
        Some(match stmt {
            Stmt::Expr(expr) => expr.range,
            Stmt::Let { object, .. } => self.program.object(*object).range,
            Stmt::Vla(def) => def.range,
            Stmt::If { cond, .. } => cond.range,
            Stmt::While { range, .. }
            | Stmt::DoWhile { range, .. }
            | Stmt::For { range, .. }
            | Stmt::Break { range, .. }
            | Stmt::Continue { range, .. }
            | Stmt::Return { range, .. }
            | Stmt::Label { range, .. }
            | Stmt::Case { range, .. }
            | Stmt::Goto { range, .. } => *range,
            Stmt::Switch(switch) => switch.range,
            Stmt::SwitchTree(switch) => switch.range,
            Stmt::Block(items) => return items.iter().find_map(|s| self.stmt_range(s)),
            Stmt::Nop => return None,
        })
    }

    fn loop_label(&self, id: LoopId, span: Span) -> TokenStream {
        self.label(&format!("l{}", id.0), span)
    }

    fn loop_body_label(&self, id: LoopId, span: Span) -> TokenStream {
        self.label(&format!("l{}_body", id.0), span)
    }

    fn switch_label(&self, id: ir::SwitchId, span: Span) -> TokenStream {
        self.label(&format!("sw{}", id.0), span)
    }

    fn switch_case_label(&self, id: ir::SwitchId, index: usize, span: Span) -> TokenStream {
        self.label(&format!("sw{}_c{index}", id.0), span)
    }

    /// Builds the labelled-block chain described in the [module docs](self).
    fn switch(&mut self, switch: &Switch) -> TokenStream {
        let span = self.sp(switch.range);
        let scrutinee_ty = switch.scrutinee.ty;
        let scrutinee = self.expr(&switch.scrutinee).at(prec::UNARY, span);

        let mut arms = TokenStream::new();
        for (index, group) in switch.groups.iter().enumerate() {
            if group.values.is_empty() {
                continue;
            }
            let label = self.switch_case_label(switch.id, index, span);
            let mut pattern = TokenStream::new();
            for (i, value) in group.values.iter().enumerate() {
                if i > 0 {
                    pattern.extend(quote_spanned! {span=> | });
                }
                // A pattern takes its type from the scrutinee, so the bare
                // literal is both correct and the most readable form.
                pattern.extend(case_pattern(*value, scrutinee_ty, span));
            }
            arms.extend(quote_spanned! {span=> #pattern => break #label, });
        }
        let fallback = match switch.default_group {
            Some(index) => self.switch_case_label(switch.id, index, span),
            None => self.switch_label(switch.id, span),
        };
        arms.extend(quote_spanned! {span=> _ => break #fallback, });

        // Anything before the first label can never be reached, but it is still
        // part of the program and may declare things later groups use.
        let prelude = self.stmts(&switch.prelude);
        let mut inner = quote_spanned! {span=> match #scrutinee { #arms } #prelude };
        for (index, group) in switch.groups.iter().enumerate() {
            let label = self.switch_case_label(switch.id, index, span);
            let body = self.stmts(&group.body);
            inner = quote_spanned! {span=> #label: { #inner } #body };
        }

        let mut hoisted = TokenStream::new();
        for id in &switch.hoisted {
            let object = self.program.object(*id);
            let ospan = self.sp(object.range);
            let name = self.object_ident(*id, ospan);
            let ty = self.ty(object.ty, ospan);
            let zero = self.zero_tokens(object.ty, ospan);
            hoisted.extend(quote_spanned! {ospan=> let mut #name: #ty = #zero; });
        }

        let label = self.switch_label(switch.id, span);
        // The extra block keeps the hoisted declarations from shadowing
        // anything the enclosing block declared under the same name.
        quote_spanned! {span=>
            {
                #hoisted
                #label: { #inner }
            }
        }
    }

    /// Emits an expression evaluated for its side effects.
    fn expr_stmt(&mut self, expr: &Expr) -> TokenStream {
        let span = self.sp(expr.range);
        match &expr.kind {
            ExprKind::Assign { place, value } => {
                let lowered = self.place(place, true);
                let value = self.expr_at(value, place.ty);
                let store = self.write(&lowered, value, span);
                let setup = &lowered.setup;
                quote_spanned! {span=> #setup #store }
            }
            ExprKind::CompoundAssign {
                place,
                op,
                value,
                compute,
            } => {
                let lowered = self.place(place, true);
                let (hoist, rhs) = self.compound_rhs(value);
                let current = self.read(&lowered, span);
                let updated = self.compound_value(current, place.ty, *op, value, rhs, *compute);
                let updated = updated.at(prec::LOWEST, span);
                let store = self.write(&lowered, updated, span);
                let setup = &lowered.setup;
                quote_spanned! {span=> #setup #hoist #store }
            }
            ExprKind::IncDec { place, dec, .. } => {
                let lowered = self.place(place, true);
                let current = self.read(&lowered, span);
                let next = self.step_value(current, place.ty, *dec, span);
                let store = self.write(&lowered, next, span);
                let setup = &lowered.setup;
                quote_spanned! {span=> #setup #store }
            }
            ExprKind::Call { .. } => {
                // A call to a `_Noreturn` function does not come back, and
                // Rust has to be told: the call's own type is whatever the
                // function was declared to return, so a function that ends in
                // `exit(1);` would otherwise be missing its value. The
                // `unreachable!()` is a panic rather than
                // `unreachable_unchecked`, because it is *this crate* putting
                // it there.
                let diverges = ir::expr_never_returns(expr, &self.program.functions);
                let tokens = self.expr(expr).at(prec::LOWEST, span);
                if diverges {
                    quote_spanned! {span=> #tokens; ::core::unreachable!(); }
                } else {
                    quote_spanned! {span=> #tokens; }
                }
            }
            ExprKind::Unreachable => {
                quote_spanned! {span=> ::core::hint::unreachable_unchecked(); }
            }
            ExprKind::Comma { .. } => {
                // A chain of comma operators is a flat sequence of statements
                // — and one stack frame per operand if it is walked
                // recursively, which a four-thousand-character logical source
                // line cannot afford. See [`Codegen::binary_chain`].
                let mut out = TokenStream::new();
                for operand in comma_operands(expr) {
                    out.extend(self.expr_stmt(operand));
                }
                out
            }
            ExprKind::Cond { .. } => {
                // A chain of them is `if … else if … else …`, built without
                // recursing down the chain; see [`Codegen::cond_chain`].
                let mut spine = Vec::new();
                let mut node = expr;
                while let ExprKind::Cond {
                    cond,
                    then_expr,
                    else_expr,
                } = &node.kind
                {
                    let span = self.sp(node.range);
                    let cond_tokens = self.condition(cond).at_condition(span);
                    let then_tokens = self.expr_stmt(then_expr);
                    spine.push((cond_tokens, then_tokens, span));
                    node = else_expr;
                }
                let mut tokens = self.expr_stmt(node);
                while let Some((cond_tokens, then_tokens, span)) = spine.pop() {
                    tokens = quote_spanned! {span=>
                        if #cond_tokens { #then_tokens } else { #tokens }
                    };
                }
                tokens
            }
            // `(void)x;` evaluates and discards, which is what the fall-through
            // below does anyway; unwrapping keeps the output tidy.
            ExprKind::Cast(inner) if expr.ty.is_void() => self.expr_stmt(inner),
            // `va_end(ap);` is a statement that does nothing at all.
            ExprKind::VaEnd => TokenStream::new(),
            _ => {
                let tokens = self.expr(expr).at(prec::LOWEST, span);
                quote_spanned! {span=> let _ = #tokens; }
            }
        }
    }

    // -- expressions --------------------------------------------------------

    /// Emits an expression whose type the surrounding context already fixes.
    ///
    /// A constant then needs no `as`, which is the difference between
    /// `let mut i: c_int = 0;` and `let mut i: c_int = 0 as c_int;`. It is the
    /// *only* place a bare literal is emitted from, and the reason it is safe
    /// is that every caller writes the tokens somewhere the type is already
    /// stated — the annotation of a `let`, a place being assigned to, a
    /// parameter, a field, the return type of the function.
    ///
    /// [`Codegen::expr`] has no such promise, so nothing it produces may
    /// depend on inference; that is what the conditional below is about.
    fn expr_at(&mut self, expr: &Expr, expected: Ty) -> TokenStream {
        let span = self.sp(expr.range);
        if expr.ty == expected {
            match &expr.kind {
                ExprKind::Int(value) => return bare_int_literal(*value, expected, span),
                ExprKind::Float(value) if value.is_finite() => {
                    return bare_float_literal(*value, span);
                }
                // The arms of a conditional may stay bare here, because
                // whatever fixes this expression's type fixes theirs — unless
                // the type is `void`, where the arms have no common type and
                // [`Codegen::expr`] emits each of them as a statement.
                ExprKind::Cond { .. } if !expected.is_void() => {
                    return self.cond_chain_at(expr, expected);
                }
                _ => {}
            }
        }
        self.expr(expr).at(prec::LOWEST, span)
    }

    fn expr(&mut self, expr: &Expr) -> Value {
        let value = self.expr_value(expr);
        // A chain of binary operators reduces each of its own nodes; see
        // `Codegen::binary_chain`.
        if matches!(expr.kind, ExprKind::Binary { .. }) {
            return value;
        }
        self.reduce_bits(value, expr)
    }

    /// Reduces a computed value to the precision the expression is evaluated
    /// in, which is narrower than its type only for a wide bit-field; see
    /// [`ir::Expr::bits`].
    ///
    /// Only an operator whose result can leave the field's range needs it: a
    /// bitwise `&`, `|` or `^` of two forty-bit values is a forty-bit value
    /// already, and so is a quotient, a remainder or a right shift.
    fn reduce_bits(&mut self, value: Value, expr: &Expr) -> Value {
        let Some(bits) = expr.bits else {
            return value;
        };
        let width = expr.ty.bits(&self.options.target);
        let overflows = match &expr.kind {
            ExprKind::Binary { op, .. } => {
                matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Shl)
            }
            ExprKind::Neg(_) | ExprKind::BitNot(_) => true,
            _ => false,
        };
        if !overflows || !expr.ty.is_integer() || bits == 0 || bits >= width {
            return value;
        }
        let span = self.sp(expr.range);
        if expr.ty.is_signed(&self.options.target) {
            // Sign extension: the two shifts are what a narrower signed type
            // does to a value that has just overflowed out of it.
            let shift = Literal::u32_unsuffixed(width - bits);
            let tokens = value.at(prec::CALL, span);
            return Value::new(
                quote_spanned! {span=> #tokens.wrapping_shl(#shift).wrapping_shr(#shift) },
                prec::CALL,
            );
        }
        let mask = bare_int_literal(((1u128 << bits) - 1) as i128, expr.ty, span);
        let tokens = value.at(prec::BIT_AND, span);
        Value::new(quote_spanned! {span=> #tokens & #mask }, prec::BIT_AND)
    }

    fn expr_value(&mut self, expr: &Expr) -> Value {
        let span = self.sp(expr.range);
        match &expr.kind {
            ExprKind::Int(value) => self.int_literal(*value, expr.ty, span),
            ExprKind::Float(value) => self.float_literal(*value, expr.ty, span),
            ExprKind::Zeroed => Value::new(self.zero_tokens(expr.ty, span), zero_prec(expr.ty)),
            // Reading a `va_list` copies it: Rust's is not `Copy`, and C says
            // a list passed on is indeterminate afterwards anyway.
            ExprKind::Load(place) if place.ty.is_va_list() => {
                // A `va_list` place is always a plain object — nothing may
                // point at one or keep one as a member — so it needs no setup.
                let access = self.place(place, false).access;
                Value::new(quote_spanned! {span=> #access.clone() }, prec::CALL)
            }
            ExprKind::Load(place) => {
                let lowered = self.place(place, false);
                let value = self.read(&lowered, span);
                if lowered.setup.is_empty() {
                    value
                } else {
                    let setup = &lowered.setup;
                    let tokens = value.at(prec::LOWEST, span);
                    Value::new(quote_spanned! {span=> { #setup #tokens } }, prec::BLOCK)
                }
            }
            ExprKind::AddrOf(place) => self.address_of(place, expr.ty, span),
            ExprKind::FuncAddr(id) => {
                let function = self.program.function(*id);
                let name = self.function_path(function, span);
                let signature = self.function_pointer_ty(function, span);
                Value::new(
                    quote_spanned! {span=> ::core::option::Option::Some(#name as #signature) },
                    prec::CALL,
                )
            }
            ExprKind::Assign { .. } => self.assign_chain(expr),
            ExprKind::CompoundAssign {
                place,
                op,
                value,
                compute,
            } => {
                let lowered = self.place(place, true);
                let (hoist, rhs) = self.compound_rhs(value);
                let current = self.read(&lowered, span);
                let updated = self.compound_value(current, place.ty, *op, value, rhs, *compute);
                let updated = updated.at(prec::LOWEST, span);
                let store = self.write(&lowered, updated, span);
                let read = self.read(&lowered, span).at(prec::LOWEST, span);
                let setup = &lowered.setup;
                Value::new(
                    quote_spanned! {span=> { #setup #hoist #store #read } },
                    prec::BLOCK,
                )
            }
            ExprKind::IncDec {
                place,
                dec,
                postfix,
            } => {
                let lowered = self.place(place, true);
                let current = self.read(&lowered, span);
                let next = self.step_value(current, place.ty, *dec, span);
                let store = self.write(&lowered, next, span);
                let read = self.read(&lowered, span).at(prec::LOWEST, span);
                let setup = &lowered.setup;
                if *postfix {
                    let tmp = self.temporary();
                    Value::new(
                        quote_spanned! {span=>
                            { #setup let #tmp = #read; #store #tmp }
                        },
                        prec::BLOCK,
                    )
                } else {
                    Value::new(
                        quote_spanned! {span=> { #setup #store #read } },
                        prec::BLOCK,
                    )
                }
            }
            ExprKind::Neg(operand) => {
                let value = self.expr(operand);
                if expr.ty.is_floating() {
                    let ends_with_type = value.ends_with_type;
                    let tokens = value.at(prec::UNARY, span);
                    Value::new(quote_spanned! {span=> -#tokens }, prec::UNARY)
                        .type_end(ends_with_type)
                } else {
                    // Signed overflow is undefined in C and a panic in Rust;
                    // wrapping is the predictable choice, and it is what
                    // unsigned arithmetic requires anyway.
                    let tokens = value.at(prec::CALL, span);
                    Value::new(quote_spanned! {span=> #tokens.wrapping_neg() }, prec::CALL)
                }
            }
            ExprKind::BitNot(operand) => {
                let value = self.expr(operand);
                let ends_with_type = value.ends_with_type;
                let tokens = value.at(prec::UNARY, span);
                Value::new(quote_spanned! {span=> !#tokens }, prec::UNARY).type_end(ends_with_type)
            }
            ExprKind::Binary { .. } => self.binary_chain(expr),
            ExprKind::PtrOffset { ptr, index, sub } => {
                let base = self.expr(ptr).at(prec::CALL, span);
                let offset = self.offset_argument(index, *sub, span);
                Value::new(quote_spanned! {span=> #base.offset(#offset) }, prec::CALL)
            }
            ExprKind::PtrDiff { lhs, rhs } => {
                let left = self.expr(lhs).at(prec::CALL, span);
                let right = self.expr(rhs).at(prec::LOWEST, span);
                let target = self.ty(expr.ty, span);
                Value::new(
                    quote_spanned! {span=> #left.offset_from(#right) as #target },
                    prec::CAST,
                )
                .type_end(true)
            }
            ExprKind::Compare { .. } | ExprKind::Logical { .. } => {
                // C's comparisons and logical operators produce an `int`.
                let condition = self.condition(expr).at(prec::LOWEST, span);
                let ty = self.ty(Ty::Int, span);
                let parens = parenthesize(condition, span);
                Value::new(quote_spanned! {span=> #parens as #ty }, prec::CAST).type_end(true)
            }
            // `(void)x` and a `void`-typed conditional have no value at all:
            // Rust's `()` is not something `as` produces, so each of them is
            // emitted as the statement it is, wrapped in a block whose own
            // value is `()`. A conditional gets here when it is written where
            // a value is expected — the left operand of a comma is a
            // statement, but the right one is not.
            ExprKind::Cast(inner) if expr.ty.is_void() => {
                let tokens = self.expr_stmt(inner);
                Value::new(quote_spanned! {span=> { #tokens } }, prec::BLOCK)
            }
            ExprKind::Cond { .. } if expr.ty.is_void() => {
                let tokens = self.expr_stmt(expr);
                Value::new(quote_spanned! {span=> { #tokens } }, prec::BLOCK)
            }
            ExprKind::Cast(inner) => {
                let from = inner.ty;
                let value = self.expr(inner);
                self.cast(value, from, expr.ty, span)
            }
            // Both arms are emitted as expressions of their own rather than at
            // the conditional's type: an `if` whose arms are two bare literals
            // is `{integer}`, which Rust either resolves to `i32` — wrong
            // wherever C said `long` — or refuses to resolve at all, as it
            // does for the receiver of `wrapping_mul` (`E0689`). The bare form
            // is still used from [`Codegen::expr_at`], where the context says
            // what the type is.
            ExprKind::Cond { .. } => self.cond_chain(expr),
            ExprKind::Comma { lhs, rhs } => {
                let lhs = self.expr_stmt(lhs);
                let rhs = self.expr(rhs).at(prec::LOWEST, span);
                Value::new(quote_spanned! {span=> { #lhs #rhs } }, prec::BLOCK)
            }
            ExprKind::Call { callee, args } => self.call(callee, args, span),
            ExprKind::RecordLit { record, fields } => self.record_literal(*record, fields, span),
            ExprKind::UnionLit {
                record,
                index,
                value,
            } => self.union_literal(*record, *index, value, span),
            ExprKind::ArrayLit(items) => {
                let elem = self.program.types.elem(expr.ty).unwrap_or(Ty::Int);
                let mut tokens = TokenStream::new();
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        tokens.extend(quote_spanned! {span=> , });
                    }
                    tokens.extend(self.expr_at(item, elem));
                }
                Value::atom(bracketed(tokens, span))
            }
            ExprKind::ArrayRepeat { value, len } => {
                let elem = self.program.types.elem(expr.ty).unwrap_or(value.ty);
                let tokens = self.expr_at(value, elem);
                let len = usize_literal(*len, span);
                Value::atom(bracketed(quote_spanned! {span=> #tokens ; #len }, span))
            }
            // GNU's `a ?: b`. The value is held in a temporary so that the
            // operand is evaluated exactly once, which is the whole point.
            ExprKind::CondDefault { value, else_expr } => {
                let ty = expr.ty;
                let first = self.expr_at(value, ty);
                let other = self.expr_at(else_expr, ty);
                let tmp = self.temporary();
                let target = self.ty(ty, span);
                let test = if ty.is_pointer() {
                    quote_spanned! {span=> !#tmp.is_null() }
                } else {
                    let zero = self.zero_tokens(ty, span);
                    quote_spanned! {span=> #tmp != #zero }
                };
                Value::new(
                    quote_spanned! {span=>
                        { let #tmp: #target = #first; if #test { #tmp } else { #other } }
                    },
                    prec::BLOCK,
                )
            }
            // GNU's statement expression, which is what a Rust block is.
            ExprKind::StmtExpr { stmts, value } => {
                let body = self.stmts(stmts);
                let tail = match value {
                    Some(value) => {
                        let ty = expr.ty;
                        self.expr_at(value, ty)
                    }
                    None => TokenStream::new(),
                };
                Value::new(quote_spanned! {span=> { #body #tail } }, prec::BLOCK)
            }
            ExprKind::Builtin { op, args } => self.builtin(*op, args, span),
            ExprKind::VaListPristine => Value::new(self.va_pristine(span), prec::CALL),
            ExprKind::VaArg { ap } => self.va_arg(ap, expr.ty, span),
            // `va_end` is nothing: the list ends when its value is dropped.
            ExprKind::VaEnd => Value::atom(quote_spanned! {span=> () }),
            ExprKind::OffsetOf { record, path } => self.offset_of(*record, path, expr.ty, span),
            // C23's `unreachable()`. C says reaching it is undefined, and
            // saying so to Rust is what lets the optimiser use the promise —
            // this is the one place the expansion trusts the C program with
            // undefined behaviour, because the program asked for it by name.
            ExprKind::Unreachable => Value::new(
                quote_spanned! {span=> ::core::hint::unreachable_unchecked() },
                prec::CALL,
            ),
        }
    }

    /// A `struct` value: one expression per member, in declaration order.
    ///
    /// Without bit-fields this is a plain Rust struct literal. With them the
    /// members that share a storage field have to be *packed* into it: every
    /// constant one is folded into the `[u8; K]` there and then, which is what
    /// lets a `static` — where nothing may run — hold a bit-field at all, and
    /// what makes the byte pattern visible in the expansion. A member whose
    /// value is not constant is stored afterwards through its setter, so the
    /// literal becomes a block.
    fn record_literal(&mut self, record: ir::RecordId, fields: &[Expr], span: Span) -> Value {
        let def = self.program.types.record(record).clone();
        let name = c_ident(&def.rust_name, span);
        let mut packed: HashMap<String, Vec<u8>> = HashMap::new();
        let mut dynamic: Vec<usize> = Vec::new();
        for rust_field in &def.rust_fields {
            if let ir::RustField::Bits { name, bytes, .. } = rust_field {
                packed.insert(name.clone(), vec![0u8; *bytes as usize]);
            }
        }
        for (index, field) in def.fields.iter().enumerate() {
            let Some(bits) = &field.bits else { continue };
            match fields.get(index).and_then(constant_bits) {
                Some(value) => {
                    if let Some(storage) = packed.get_mut(&bits.storage) {
                        pack_bits(storage, bits, value);
                    }
                }
                None => dynamic.push(index),
            }
        }

        let mut items = TokenStream::new();
        for rust_field in &def.rust_fields {
            match rust_field {
                ir::RustField::Member(index) => {
                    let field = &def.fields[*index];
                    let fname = c_ident(&field.name, span);
                    let tokens = self.expr_at(&fields[*index], field.ty);
                    items.extend(quote_spanned! {span=> #fname: #tokens, });
                }
                ir::RustField::Bits { name, .. } => {
                    let fname = Ident::new(name, span);
                    let value = byte_array(&packed[name], span);
                    items.extend(quote_spanned! {span=> #fname: #value, });
                }
                ir::RustField::Pad { name, bytes } => {
                    let fname = Ident::new(name, span);
                    let len = usize_literal(*bytes, span);
                    items.extend(quote_spanned! {span=> #fname: [0; #len], });
                }
                ir::RustField::Align { name, .. } => {
                    let fname = Ident::new(name, span);
                    items.extend(quote_spanned! {span=> #fname: [], });
                }
            }
        }
        let body = braced(items, span);
        let literal = quote_spanned! {span=> #name #body };
        if dynamic.is_empty() {
            return Value::new(literal, prec::ATOM);
        }
        // The members that are not constants are stored through their setters,
        // in declaration order, which is one of the orders C allows.
        let tmp = self.temporary();
        let ty = self.ty(Ty::Record(record), span);
        let mut stores = TokenStream::new();
        for index in dynamic {
            let field = &def.fields[index];
            let bits = field.bits.as_ref().expect("only bit-fields are deferred");
            let setter = c_ident(&bits.setter, span);
            let value = self.expr_at(&fields[index], field.ty);
            stores.extend(quote_spanned! {span=> #tmp.#setter(#value); });
        }
        Value::new(
            quote_spanned! {span=>
                { let mut #tmp: #ty = #literal; #stores #tmp }
            },
            prec::BLOCK,
        )
    }

    /// A `union` value, which initialises exactly one member.
    fn union_literal(
        &mut self,
        record: ir::RecordId,
        index: usize,
        value: &Expr,
        span: Span,
    ) -> Value {
        let def = self.program.types.record(record).clone();
        let name = c_ident(&def.rust_name, span);
        let field = &def.fields[index];
        let Some(bits) = &field.bits else {
            let fname = c_ident(&field.name, span);
            let tokens = self.expr_at(value, field.ty);
            let body = braced(quote_spanned! {span=> #fname: #tokens }, span);
            return Value::new(quote_spanned! {span=> #name #body }, prec::ATOM);
        };
        let bytes = def
            .rust_fields
            .iter()
            .find_map(|rust_field| match rust_field {
                ir::RustField::Bits { name, bytes, .. } if *name == bits.storage => Some(*bytes),
                _ => None,
            })
            .unwrap_or(0);
        let mut packed = vec![0u8; bytes as usize];
        let constant = constant_bits(value);
        if let Some(constant) = constant {
            pack_bits(&mut packed, bits, constant);
        }
        let storage = Ident::new(&bits.storage, span);
        let array = byte_array(&packed, span);
        let body = braced(quote_spanned! {span=> #storage: #array }, span);
        let literal = quote_spanned! {span=> #name #body };
        if constant.is_some() {
            return Value::new(literal, prec::ATOM);
        }
        let tmp = self.temporary();
        let ty = self.ty(Ty::Record(record), span);
        let setter = c_ident(&bits.setter, span);
        let value = self.expr_at(value, field.ty);
        Value::new(
            quote_spanned! {span=>
                { let mut #tmp: #ty = #literal; #tmp.#setter(#value); #tmp }
            },
            prec::BLOCK,
        )
    }

    /// `offsetof(T, member)`, asked of Rust rather than worked out here.
    ///
    /// `core::mem::offset_of!` gives the offset the generated `#[repr(C)]`
    /// item really has, which is the number the C program is asking for. The
    /// `as` puts it back into `size_t`, since the macro's own type is `usize`.
    /// A member reached through an anonymous member is a path — `offset_of!`
    /// takes one — which is what makes `offsetof(struct S, x)` work when `x`
    /// lives in an anonymous union inside `S`.
    fn offset_of(&mut self, record: ir::RecordId, path: &[usize], ty: Ty, span: Span) -> Value {
        let mut current = record;
        let mut fields = TokenStream::new();
        for (step, index) in path.iter().enumerate() {
            let field = self.program.types.record(current).fields[*index].clone();
            let name = c_ident(&field.name, span);
            if step > 0 {
                fields.extend(quote_spanned! {span=> . });
            }
            fields.extend(quote_spanned! {span=> #name });
            if let Ty::Record(inner) = field.ty {
                current = inner;
            }
        }
        let name = c_ident(&self.program.types.record(record).rust_name, span);
        let target = self.ty(ty, span);
        Value::new(
            quote_spanned! {span=> ::core::mem::offset_of!(#name, #fields) as #target },
            prec::CAST,
        )
        .type_end(true)
    }

    /// One of the builtins that becomes a fixed piece of Rust.
    ///
    /// The bit-manipulation ones are the integer methods of the same name, on
    /// the *unsigned* type of the operand's width — C's are defined on
    /// unsigned values and Rust's `leading_zeros` counts the same way. The
    /// overflow ones do the arithmetic in `i128` and ask whether the value
    /// survives the round trip through the type it is stored in, which is
    /// exactly "compute in infinite precision, then convert".
    fn builtin(&mut self, op: ir::BuiltinOp, args: &[Expr], span: Span) -> Value {
        use ir::BuiltinOp;
        let int = self.ty(Ty::Int, span);
        match op {
            BuiltinOp::Discard => {
                let mut out = TokenStream::new();
                for arg in args {
                    out.extend(self.expr_stmt(arg));
                }
                Value::new(quote_spanned! {span=> { #out } }, prec::BLOCK)
            }
            // One block of the function's arena per call, rounded up to a
            // whole number of `u128`s so that the pointer is 16-byte aligned.
            // The pointer is taken before the block is put away, since moving
            // a `Vec` does not move the memory it owns.
            BuiltinOp::Alloca => {
                let arena = self.alloca_ident();
                let size = self.expr(&args[0]).at(prec::CAST, span);
                let block = self.temporary();
                let pointer = self.temporary();
                let void = self.pointee_ty(Ty::Void, span);
                let elements = self.vec_of(
                    quote_spanned! {span=> 0u128 },
                    quote_spanned! {span=> (#size as usize).div_ceil(16) },
                    span,
                );
                Value::new(
                    quote_spanned! {span=>
                        {
                            let mut #block = #elements;
                            let #pointer = #block.as_mut_ptr().cast::<#void>();
                            #arena.push(#block);
                            #pointer
                        }
                    },
                    prec::BLOCK,
                )
            }
            BuiltinOp::Bswap => {
                let operand = args[0].ty;
                let value = self.unsigned_operand(&args[0], span);
                let target = self.ty(operand, span);
                Value::new(
                    quote_spanned! {span=> #value.swap_bytes() as #target },
                    prec::CAST,
                )
                .type_end(true)
            }
            BuiltinOp::Popcount => {
                let value = self.unsigned_operand(&args[0], span);
                Value::new(
                    quote_spanned! {span=> #value.count_ones() as #int },
                    prec::CAST,
                )
                .type_end(true)
            }
            BuiltinOp::Parity => {
                let value = self.unsigned_operand(&args[0], span);
                Value::new(
                    quote_spanned! {span=> (#value.count_ones() & 1) as #int },
                    prec::CAST,
                )
                .type_end(true)
            }
            BuiltinOp::Clz => {
                let value = self.unsigned_operand(&args[0], span);
                Value::new(
                    quote_spanned! {span=> #value.leading_zeros() as #int },
                    prec::CAST,
                )
                .type_end(true)
            }
            BuiltinOp::Ctz => {
                let value = self.unsigned_operand(&args[0], span);
                Value::new(
                    quote_spanned! {span=> #value.trailing_zeros() as #int },
                    prec::CAST,
                )
                .type_end(true)
            }
            BuiltinOp::Ffs => {
                let value = self.unsigned_operand(&args[0], span);
                let tmp = self.temporary();
                Value::new(
                    quote_spanned! {span=>
                        { let #tmp = #value;
                          if #tmp == 0 { 0 } else { #tmp.trailing_zeros() as #int + 1 } }
                    },
                    prec::BLOCK,
                )
            }
            // The number of leading bits that repeat the sign bit, not
            // counting the sign bit itself — which is what `leading_zeros` of
            // the value XORed with itself shifted left gives.
            BuiltinOp::Clrsb => {
                let width = args[0].ty.bits(&self.options.target);
                let signed = signed_rust_ty(width, span);
                let value = self.expr(&args[0]).at(prec::CAST, span);
                let tmp = self.temporary();
                let bits = usize_literal(u64::from(width), span);
                Value::new(
                    quote_spanned! {span=>
                        { let #tmp = #value as #signed;
                          ((#tmp ^ (#tmp << 1)).leading_zeros() as #int)
                              .min(#bits as #int - 1) }
                    },
                    prec::BLOCK,
                )
            }
            BuiltinOp::Overflow(bin) | BuiltinOp::OverflowP(bin) => {
                let store = matches!(op, BuiltinOp::Overflow(_));
                // The third operand carries the result type: the pointee of
                // the pointer the value is stored through, or the type of the
                // expression the `_p` forms only ask about.
                let result_ty = if store {
                    self.program.types.pointee(args[2].ty).unwrap_or(Ty::Int)
                } else {
                    args[2].ty
                };
                self.overflow_builtin(bin, args, store, result_ty, span)
            }
        }
    }

    /// The operand of a bit-manipulation builtin, as the unsigned integer of
    /// its own width.
    fn unsigned_operand(&mut self, arg: &Expr, span: Span) -> TokenStream {
        let width = arg.ty.bits(&self.options.target);
        let target = unsigned_rust_ty(width, span);
        let value = self.expr(arg).at(prec::CAST, span);
        parenthesize(quote_spanned! {span=> #value as #target }, span)
    }

    /// `__builtin_add_overflow(a, b, &r)` and its relatives.
    fn overflow_builtin(
        &mut self,
        op: BinOp,
        args: &[Expr],
        store: bool,
        result_ty: Ty,
        span: Span,
    ) -> Value {
        let method = match op {
            BinOp::Add => "wrapping_add",
            BinOp::Sub => "wrapping_sub",
            _ => "wrapping_mul",
        };
        let method = Ident::new(method, span);
        let checked = match op {
            BinOp::Add => "checked_add",
            BinOp::Sub => "checked_sub",
            _ => "checked_mul",
        };
        let checked = Ident::new(checked, span);
        let lhs = self.expr(&args[0]).at(prec::CAST, span);
        let rhs = self.expr(&args[1]).at(prec::CAST, span);
        let target = self.ty(result_ty, span);
        let a = self.temporary();
        let b = self.temporary();
        let wide = self.temporary();
        let narrow = self.temporary();
        let compute = quote_spanned! {span=>
            let #a: i128 = #lhs as i128;
            let #b: i128 = #rhs as i128;
            let #wide: i128 = #a.#method(#b);
            let #narrow: #target = #wide as #target;
        };
        // The value overflows exactly when the wrapped result no longer equals
        // what infinite precision gave — and `i128` itself can only overflow
        // on a multiplication, where the answer is certainly out of range.
        let flag = quote_spanned! {span=>
            #a.#checked(#b).is_none() || (#narrow as i128) != #wide
        };
        if !store {
            // The `_p` forms still evaluate their third operand.
            let third = self.expr_stmt(&args[2]);
            return Value::new(
                quote_spanned! {span=> { #third #compute #flag } },
                prec::BLOCK,
            );
        }
        let place = self.expr(&args[2]).at(prec::CALL, span);
        let out = self.temporary();
        Value::new(
            quote_spanned! {span=>
                { #compute let #out = #place; *#out = #narrow; #flag }
            },
            prec::BLOCK,
        )
    }

    /// `va_arg(ap, T)`: reads the next argument and advances the list.
    ///
    /// `VaArgSafe` — the bound `next_arg` needs — is implemented for the
    /// primitives the `core::ffi` aliases stand for, so the C type can be
    /// asked for by name. A function pointer is the exception: `Option<fn>` is
    /// not one of them, so the argument is read as a `void *` and transmuted,
    /// which is what it is.
    fn va_arg(&mut self, ap: &Place, ty: Ty, span: Span) -> Value {
        let access = self.place(ap, true).access;
        if self.program.types.is_func_pointer(ty) {
            let target = self.ty(ty, span);
            let void = self.pointee_ty(Ty::Void, span);
            return Value::new(
                quote_spanned! {span=>
                    ::core::mem::transmute::<*mut #void, #target>(
                        #access.next_arg::<*mut #void>()
                    )
                },
                prec::CALL,
            );
        }
        let target = self.ty(ty, span);
        Value::new(
            quote_spanned! {span=> #access.next_arg::<#target>() },
            prec::CALL,
        )
    }

    /// The argument of `offset`, which is always an `isize`.
    fn offset_argument(&mut self, index: &Expr, sub: bool, span: Span) -> TokenStream {
        if let ExprKind::Int(value) = &index.kind {
            let value = if sub { -*value } else { *value };
            return int_literal_token(value, span);
        }
        let tokens = self.expr(index).at(prec::CAST, span);
        if sub {
            quote_spanned! {span=> -(#tokens as isize) }
        } else {
            quote_spanned! {span=> #tokens as isize }
        }
    }

    fn call(&mut self, callee: &Callee, args: &[Expr], span: Span) -> Value {
        let sig = self.callee_signature(callee);
        // A call through a function type with *no prototype* passes as many
        // arguments as it was given, each with the default argument promotions
        // applied, and the callee reads them as though the prototype had said
        // so (C99 6.5.2.2p6). Rust has no such call, so the reinterpretation is
        // written out: the callee is transmuted to the signature the promoted
        // arguments make, and that signature is called.
        //
        // It is the same contract C's own ABI relies on — the program is
        // defined only if the function really does take parameters of those
        // types, and undefined otherwise — and on every ABI this crate targets
        // a function pointer transmuted this way is the same address. Sema has
        // already applied the promotions, so `arg.ty` is the parameter type to
        // write.
        let reinterpreted = !sig.variadic && args.len() != sig.params.len();
        let promoted: Vec<Ty> = if reinterpreted {
            args.iter().map(|arg| arg.ty).collect()
        } else {
            Vec::new()
        };
        let params = if reinterpreted {
            &promoted
        } else {
            &sig.params
        };

        let mut target = match callee {
            Callee::Direct(id) => {
                let function = self.program.function(*id);
                let path = self.function_path(function, span);
                if reinterpreted {
                    // A function *item* is not a function pointer, so the `as`
                    // coercion has to be written before it can be transmuted.
                    let source = self.function_pointer_ty(function, span);
                    parenthesize(quote_spanned! {span=> #path as #source }, span)
                } else {
                    path
                }
            }
            Callee::Indirect(expr) => {
                let value = self.expr(expr).at(prec::CALL, span);
                // C's function pointers may be null and Rust's may not, so the
                // `Option` has to come off before the call.
                parenthesize(
                    quote_spanned! {span=> #value.expect("null function pointer") },
                    span,
                )
            }
        };
        if reinterpreted {
            let source = self.fn_ptr_ty(&sig.params, sig.variadic, sig.ret, span);
            let wanted = self.fn_ptr_ty(params, false, sig.ret, span);
            target = parenthesize(
                quote_spanned! {span=>
                    ::core::mem::transmute::<#source, #wanted>(#target)
                },
                span,
            );
        }

        let mut tokens = TokenStream::new();
        for (index, arg) in args.iter().enumerate() {
            if index > 0 {
                tokens.extend(quote_spanned! {span=> , });
            }
            match params.get(index) {
                // A parameter's type is what the argument is written at, so a
                // constant needs no `as`.
                Some(expected) => tokens.extend(self.expr_at(arg, *expected)),
                // An argument matched by `...` has no parameter to take its
                // type from, and Rust gives an unsuffixed literal in that
                // position `i32` (or `f64`), whatever C says it is: `%ld` with
                // a bare `-1` would read four bytes of an eight-byte
                // argument. The type has to be written out.
                None => {
                    let arg_span = self.sp(arg.range);
                    tokens.extend(self.expr(arg).at(prec::LOWEST, arg_span));
                }
            }
        }
        let call = parenthesize(tokens, span);
        Value::new(quote_spanned! {span=> #target #call }, prec::CALL)
    }

    /// The signature a call goes through: the callee's own for a direct call,
    /// and the pointed-to function type for an indirect one.
    fn callee_signature(&self, callee: &Callee) -> ir::Signature {
        match callee {
            Callee::Direct(id) => self.program.function(*id).sig.clone(),
            Callee::Indirect(expr) => match self.program.types.pointee(expr.ty) {
                Some(Ty::Func(id)) => {
                    let func = self.program.types.func_type(id);
                    ir::Signature {
                        ret: func.ret,
                        params: func.params.clone(),
                        variadic: func.variadic,
                        prototyped: func.prototyped,
                    }
                }
                // Only reachable on the error path, where a `compile_error!` is
                // already going out.
                _ => ir::Signature {
                    ret: Ty::Void,
                    params: Vec::new(),
                    variadic: false,
                    prototyped: true,
                },
            },
        }
    }

    /// `unsafe extern "C" fn(P…) -> R`, written out from its pieces.
    fn fn_ptr_ty(&self, params: &[Ty], variadic: bool, ret: Ty, span: Span) -> TokenStream {
        let mut list = TokenStream::new();
        for (index, param) in params.iter().enumerate() {
            if index > 0 {
                list.extend(quote_spanned! {span=> , });
            }
            list.extend(self.ty(*param, span));
        }
        if variadic {
            if !params.is_empty() {
                list.extend(quote_spanned! {span=> , });
            }
            list.extend(quote_spanned! {span=> ... });
        }
        let list = parenthesize(list, span);
        let ret = if ret.is_void() {
            TokenStream::new()
        } else {
            let ty = self.ty(ret, span);
            quote_spanned! {span=> -> #ty }
        };
        quote_spanned! {span=> unsafe extern "C" fn #list #ret }
    }

    /// The path a call to `function` uses: its own name, or the renamed
    /// declaration in the `extern` block.
    fn function_path(&self, function: &Function, span: Span) -> TokenStream {
        if function.is_extern() {
            let name = Ident::new(&self.program.extern_name(&function.name), span);
            return quote_spanned! {span=> #name };
        }
        let name = c_ident(&function.name, span);
        quote_spanned! {span=> #name }
    }

    /// `unsafe extern "C" fn(…) -> R` for a named function, which is what its
    /// address has to be cast to.
    fn function_pointer_ty(&self, function: &Function, span: Span) -> TokenStream {
        let sig = &function.sig;
        self.fn_ptr_ty(&sig.params, sig.variadic, sig.ret, span)
    }

    /// Emits an expression as a Rust `bool`, the way C tests a scalar against
    /// zero.
    fn condition(&mut self, expr: &Expr) -> Value {
        let span = self.sp(expr.range);
        match &expr.kind {
            ExprKind::Compare { op, lhs, rhs } => {
                if let Some(value) = self.null_test(*op, lhs, rhs, span) {
                    return value;
                }
                let (lhs, rhs) = self.operands(lhs, rhs, BinOp::BitOr);
                let left_min = if *op == CmpOp::Lt && lhs.ends_with_type {
                    prec::CAST + 1
                } else {
                    prec::CMP + 1
                };
                let ends_with_type = rhs.ends_with_type;
                let lhs = lhs.at(left_min, span);
                let rhs = rhs.at(prec::CMP + 1, span);
                let op = cmp_tokens(*op, span);
                Value::new(quote_spanned! {span=> #lhs #op #rhs }, prec::CMP)
                    .type_end(ends_with_type)
            }
            ExprKind::Logical { .. } => self.logical_chain(expr),
            ExprKind::Int(value) => {
                let ident = Ident::new(if *value != 0 { "true" } else { "false" }, span);
                Value::atom(quote_spanned! {span=> #ident })
            }
            _ if expr.ty.is_bool() => self.expr(expr),
            _ if expr.ty.is_pointer() => self.not_null(expr, span),
            _ => {
                let ty = expr.ty;
                let value = self.expr(expr).at(prec::CMP + 1, span);
                let zero = self.zero_tokens(ty, span);
                Value::new(quote_spanned! {span=> #value != #zero }, prec::CMP)
            }
        }
    }

    /// `p != NULL` reads better as `!p.is_null()`, and that is also the only
    /// form that works for a function pointer.
    fn null_test(&mut self, op: CmpOp, lhs: &Expr, rhs: &Expr, span: Span) -> Option<Value> {
        if !matches!(op, CmpOp::Eq | CmpOp::Ne) {
            return None;
        }
        let (pointer, _) = match (&lhs.kind, &rhs.kind) {
            (ExprKind::Zeroed, _) if rhs.ty.is_pointer() => (rhs, lhs),
            (_, ExprKind::Zeroed) if lhs.ty.is_pointer() => (lhs, rhs),
            _ => return None,
        };
        let test = self.is_null(pointer, span);
        if op == CmpOp::Eq {
            return Some(test);
        }
        let tokens = test.at(prec::UNARY, span);
        Some(Value::new(quote_spanned! {span=> !#tokens }, prec::UNARY))
    }

    /// `p.is_null()`, or `{ p }.is_some()` for a function pointer.
    fn is_null(&mut self, expr: &Expr, span: Span) -> Value {
        if self.program.types.is_func_pointer(expr.ty) {
            let tokens = self.copied_receiver(expr, span);
            return Value::new(quote_spanned! {span=> #tokens.is_none() }, prec::CALL);
        }
        let tokens = self.expr(expr).at(prec::CALL, span);
        Value::new(quote_spanned! {span=> #tokens.is_null() }, prec::CALL)
    }

    fn not_null(&mut self, expr: &Expr, span: Span) -> Value {
        if self.program.types.is_func_pointer(expr.ty) {
            let tokens = self.copied_receiver(expr, span);
            return Value::new(quote_spanned! {span=> #tokens.is_some() }, prec::CALL);
        }
        let tokens = self.expr(expr).at(prec::CALL, span);
        Value::new(quote_spanned! {span=> !#tokens.is_null() }, prec::UNARY)
    }

    /// A receiver for a method that takes `&self`.
    ///
    /// `Option::is_some` borrows, and borrowing a `static mut` is an error in
    /// edition 2024; a block copies the value out first. Only a place that
    /// really is a `static mut` needs it, so an ordinary local keeps reading
    /// as one.
    fn copied_receiver(&mut self, expr: &Expr, span: Span) -> TokenStream {
        if self.reads_a_static(expr) {
            let tokens = self.expr(expr).at(prec::LOWEST, span);
            return braced(tokens, span);
        }
        self.expr(expr).at(prec::CALL, span)
    }

    /// Whether an expression reads an object with static storage duration,
    /// which is what a shared reference may not be taken to.
    fn reads_a_static(&self, expr: &Expr) -> bool {
        let ExprKind::Load(place) = &expr.kind else {
            return false;
        };
        let mut place = place;
        loop {
            match &place.kind {
                PlaceKind::Object(id) => {
                    return !matches!(self.program.object(*id).storage, Storage::Automatic);
                }
                PlaceKind::Field { base, .. } => place = base,
                // Anything reached through a pointer is behind a raw pointer
                // already, so no reference to the static itself is created.
                _ => return false,
            }
        }
    }

    /// Emits the operands of a binary operation.
    ///
    /// Both operands already have the result type, so a constant one can be
    /// left as a bare literal and take its Rust type from the other side —
    /// `n == 0` rather than `n == 0 as ::core::ffi::c_int`. That only works
    /// while the other side actually has a type to give, and never for the
    /// receiver of a method call, where a bare integer literal followed by a
    /// `.` would not survive being printed back out as text.
    fn operands(&mut self, lhs: &Expr, rhs: &Expr, op: BinOp) -> (Value, Value) {
        self.operands_with(None, lhs, rhs, op)
    }

    /// [`Codegen::operands`] with the left operand possibly already emitted.
    ///
    /// `folded` is what [`Codegen::binary_chain`] has built so far. It is only
    /// ever a binary operation, and [`constant_of`] answers `None` for one, so
    /// the bare-literal reasoning below is unaffected by it.
    fn operands_with(
        &mut self,
        folded: Option<Value>,
        lhs: &Expr,
        rhs: &Expr,
        op: BinOp,
    ) -> (Value, Value) {
        let uses_method = matches!(
            op,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Shl | BinOp::Shr
        );
        let lhs_constant = constant_of(lhs);
        let rhs_constant = constant_of(rhs);

        // At most one side may be bare, and it is never the left one where a
        // method call follows.
        let lhs_bare = lhs_constant.is_some() && !uses_method && rhs_constant.is_none();
        let lhs_value = match (folded, lhs_constant) {
            (Some(value), _) => value,
            (None, Some(value)) if lhs_bare => self.bare_value(value, lhs.ty, self.sp(lhs.range)),
            (None, _) => self.expr(lhs),
        };
        let rhs_value = match rhs_constant {
            // The shift amount is converted to `u32` whatever its C type is,
            // so a constant there never needs the other operand's help.
            Some(value) if op.is_shift() => self.bare_value(value, rhs.ty, self.sp(rhs.range)),
            Some(value) if !lhs_bare => self.bare_value(value, rhs.ty, self.sp(rhs.range)),
            _ => self.expr(rhs),
        };
        (lhs_value, rhs_value)
    }

    /// A constant emitted without its `as`, for a context that will supply the
    /// type.
    fn bare_value(&mut self, value: ConstValue, ty: Ty, span: Span) -> Value {
        match value {
            ConstValue::Int(v) => {
                let tokens = bare_int_literal(v, ty, span);
                let mut out = Value::new(tokens, if v < 0 { prec::UNARY } else { prec::ATOM });
                out.bare_integer = v >= 0 && !ty.is_bool();
                out
            }
            ConstValue::Float(v) => Value::new(
                bare_float_literal(v, span),
                if v.is_sign_negative() {
                    prec::UNARY
                } else {
                    prec::ATOM
                },
            ),
        }
    }

    /// A chain of conditional operators, folded without recursing down it.
    ///
    /// `a ? b : c ? d : e` is right-associative, so unlike the chains
    /// [`Codegen::binary_chain`] handles this one really is nesting: what
    /// comes out is `if … { … } else { if … }`, one level per operator, and
    /// that is as it should be. What must not be one level per operator is
    /// the *walk*, which runs on the eight mebibytes `rustc` gives macro
    /// expansion.
    ///
    /// The condition and the `then` arm of each level are emitted on the way
    /// down and the tree is built on the way back up, which is the order the
    /// recursive walk had — so the tokens, and the numbering of any
    /// temporaries in them, are exactly what it produced.
    fn cond_chain(&mut self, expr: &Expr) -> Value {
        let mut spine = Vec::new();
        let mut node = expr;
        while let ExprKind::Cond {
            cond,
            then_expr,
            else_expr,
        } = &node.kind
        {
            let span = self.sp(node.range);
            let cond_tokens = self.condition(cond).at_condition(span);
            let then_tokens = self.expr(then_expr).at(prec::LOWEST, span);
            spine.push((cond_tokens, then_tokens, span));
            node = else_expr;
        }
        // The innermost `else` is parenthesised against the span of the
        // conditional it belongs to, which is the innermost one on the spine.
        let inner = spine
            .last()
            .map_or_else(|| self.sp(expr.range), |(_, _, span)| *span);
        let mut tokens = self.expr(node).at(prec::LOWEST, inner);
        while let Some((cond_tokens, then_tokens, span)) = spine.pop() {
            tokens = quote_spanned! {span=>
                if #cond_tokens { #then_tokens } else { #tokens }
            };
        }
        Value::new(tokens, prec::BLOCK)
    }

    /// [`Codegen::cond_chain`] where the surrounding context fixes the type.
    ///
    /// The chain ends wherever a level's own type is no longer `expected`,
    /// which is where [`Codegen::expr_at`] would have stopped treating the
    /// arms as bare anyway.
    fn cond_chain_at(&mut self, expr: &Expr, expected: Ty) -> TokenStream {
        let mut spine = Vec::new();
        let mut node = expr;
        while let ExprKind::Cond {
            cond,
            then_expr,
            else_expr,
        } = &node.kind
        {
            if node.ty != expected {
                break;
            }
            let span = self.sp(node.range);
            let cond_tokens = self.condition(cond).at_condition(span);
            let then_tokens = self.expr_at(then_expr, expected);
            spine.push((cond_tokens, then_tokens, span));
            node = else_expr;
        }
        let mut tokens = self.expr_at(node, expected);
        while let Some((cond_tokens, then_tokens, span)) = spine.pop() {
            tokens = quote_spanned! {span=>
                if #cond_tokens { #then_tokens } else { #tokens }
            };
        }
        tokens
    }

    /// A chain of assignments, folded without recursing down it.
    ///
    /// `a = b = c` is right-associative, so this is nesting in the same way
    /// [`Codegen::cond_chain`] is, and the same bargain applies: the shape of
    /// the output is unchanged and only the walk is flattened. Each place is
    /// lowered on the way down — which is where the recursive walk lowered it,
    /// and therefore where it took any temporary it needed — and the blocks
    /// are built on the way back up, innermost first, exactly as the
    /// recursion unwound.
    fn assign_chain(&mut self, expr: &Expr) -> Value {
        let mut spine = Vec::new();
        let mut node = expr;
        while let ExprKind::Assign { place, value } = &node.kind {
            let span = self.sp(node.range);
            let lowered = self.place(place, true);
            spine.push((lowered, place.ty, span));
            node = value;
        }
        let (_, innermost, _) = spine
            .last()
            .expect("assign_chain is only entered on an assignment");
        let mut tokens = self.expr_at(node, *innermost);
        while let Some((lowered, _, span)) = spine.pop() {
            let store = self.write(&lowered, tokens, span);
            // The value of an assignment is the value stored, which for a
            // place is exactly what reading it back gives — including for a
            // bit-field, where reading back is what truncates.
            let read = self.read(&lowered, span).at(prec::LOWEST, span);
            let setup = &lowered.setup;
            tokens = quote_spanned! {span=> { #setup #store #read } };
        }
        Value::new(tokens, prec::BLOCK)
    }

    /// A chain of binary operators, folded without recursing down it.
    ///
    /// `a + b + c + …` is left-associative, so the tree it leaves behind is
    /// one node per operand with the whole of the rest hanging off its left.
    /// Walking that recursively is one stack frame per operand — and code
    /// generation runs on the caller's thread, which is the eight mebibytes
    /// `rustc` gives macro expansion, where a chain of about eight hundred is
    /// a "fatal runtime error: stack overflow" with no diagnostic at all.
    ///
    /// C23 5.2.5.2p1 asks every implementation to accept a logical source
    /// line of 4095 characters, which is well past that, so the spine is
    /// collected into a vector and folded back up in a loop: one frame,
    /// however long the chain. The tokens that come out are the same ones the
    /// recursive walk produced, and in the same order — `a.wrapping_add(b)
    /// .wrapping_add(c)` is a receiver chain, which is *flat*, so neither is
    /// the output deeply nested.
    ///
    /// Nesting — `a + (b + (c + …))`, which is one level of parentheses per
    /// operand — still costs a frame per level, and is what
    /// `parse::MAX_RECURSION_DEPTH` bounds.
    fn binary_chain(&mut self, expr: &Expr) -> Value {
        let mut spine = vec![expr];
        let mut node = expr;
        while let ExprKind::Binary { lhs, .. } = &node.kind {
            node = lhs;
            if matches!(node.kind, ExprKind::Binary { .. }) {
                spine.push(node);
            }
        }
        let mut folded = None;
        while let Some(node) = spine.pop() {
            let ExprKind::Binary { op, lhs, rhs } = &node.kind else {
                unreachable!("the spine holds binary operations only");
            };
            let span = self.sp(node.range);
            let (lhs_value, rhs_value) = self.operands_with(folded, lhs, rhs, *op);
            let value = self.binary(*op, lhs_value, rhs_value, node.ty, span);
            folded = Some(self.reduce_bits(value, node));
        }
        folded.expect("the chain has at least the node it started from")
    }

    /// A chain of `&&` or `||`, folded without recursing down it.
    ///
    /// The same shape and the same reason as [`Codegen::binary_chain`]; these
    /// live on the `bool` side of code generation, so the fold is over
    /// [`Codegen::condition`] rather than over `expr`.
    fn logical_chain(&mut self, expr: &Expr) -> Value {
        let mut spine = vec![expr];
        let mut node = expr;
        while let ExprKind::Logical { lhs, .. } = &node.kind {
            node = lhs;
            if matches!(node.kind, ExprKind::Logical { .. }) {
                spine.push(node);
            }
        }
        // `node` is now the innermost left operand, which is not itself a
        // logical operator; emitting it first keeps the order the recursive
        // walk had, which is source order.
        let mut folded = self.condition(node);
        while let Some(node) = spine.pop() {
            let ExprKind::Logical { op, rhs, .. } = &node.kind else {
                unreachable!("the spine holds logical operations only");
            };
            let span = self.sp(node.range);
            let (level, tokens) = match op {
                LogicalOp::And => (prec::AND, quote_spanned! {span=> && }),
                LogicalOp::Or => (prec::OR, quote_spanned! {span=> || }),
            };
            let mut out = folded.at(level, span);
            let rhs = self.condition(rhs);
            let ends_with_type = rhs.ends_with_type;
            let rhs = rhs.at(level + 1, span);
            out.extend(quote_spanned! {span=> #tokens #rhs });
            folded = Value::new(out, level).type_end(ends_with_type);
        }
        folded
    }

    fn binary(&mut self, op: BinOp, lhs: Value, rhs: Value, ty: Ty, span: Span) -> Value {
        if ty.is_floating() {
            let (level, tokens) = match op {
                BinOp::Add => (prec::SUM, quote_spanned! {span=> + }),
                BinOp::Sub => (prec::SUM, quote_spanned! {span=> - }),
                BinOp::Mul => (prec::PRODUCT, quote_spanned! {span=> * }),
                BinOp::Div => (prec::PRODUCT, quote_spanned! {span=> / }),
                // Sema rejects every other operator on floating operands.
                _ => (prec::PRODUCT, quote_spanned! {span=> % }),
            };
            let ends_with_type = rhs.ends_with_type;
            let mut out = lhs.at(level, span);
            let rhs = rhs.at(level + 1, span);
            // Appended in place rather than built into a fresh stream around
            // the left operand: a chain of a thousand would otherwise copy
            // the whole of it a thousand times over. See
            // [`Codegen::binary_chain`].
            out.extend(quote_spanned! {span=> #tokens #rhs });
            return Value::new(out, level).type_end(ends_with_type);
        }

        // `+`, `-`, `*` and the shifts go through the wrapping methods:
        // unsigned wrap-around is defined in C, and while signed overflow is
        // undefined, wrapping is far more predictable than a panic in the
        // middle of translated code.
        let method = match op {
            BinOp::Add => Some("wrapping_add"),
            BinOp::Sub => Some("wrapping_sub"),
            BinOp::Mul => Some("wrapping_mul"),
            BinOp::Shl => Some("wrapping_shl"),
            BinOp::Shr => Some("wrapping_shr"),
            _ => None,
        };
        if let Some(method) = method {
            let mut receiver = lhs.at(prec::CALL, span);
            let method = Ident::new(method, span);
            let argument = if op.is_shift() && !rhs.bare_integer {
                // `wrapping_shl` takes the shift amount as a `u32` whatever the
                // shifted type is; a bare literal simply is one already.
                let amount = rhs.at(prec::CAST, span);
                quote_spanned! {span=> #amount as u32 }
            } else {
                rhs.at(prec::LOWEST, span)
            };
            let args = parenthesize(argument, span);
            receiver.extend(quote_spanned! {span=> .#method #args });
            return Value::new(receiver, prec::CALL);
        }

        // `/` and `%` stay plain: Rust truncates towards zero and takes the
        // sign of the dividend, exactly as C99 does. Both panic where C is
        // undefined (division by zero, and `INT_MIN / -1`).
        let (level, tokens) = match op {
            BinOp::Div => (prec::PRODUCT, quote_spanned! {span=> / }),
            BinOp::Rem => (prec::PRODUCT, quote_spanned! {span=> % }),
            BinOp::BitAnd => (prec::BIT_AND, quote_spanned! {span=> & }),
            BinOp::BitXor => (prec::BIT_XOR, quote_spanned! {span=> ^ }),
            BinOp::BitOr => (prec::BIT_OR, quote_spanned! {span=> | }),
            _ => unreachable!("every other operator was handled above"),
        };
        let ends_with_type = rhs.ends_with_type;
        let mut out = lhs.at(level, span);
        let rhs = rhs.at(level + 1, span);
        out.extend(quote_spanned! {span=> #tokens #rhs });
        Value::new(out, level).type_end(ends_with_type)
    }

    /// The value `place op= value` stores.
    ///
    /// `hoisted` is the right operand when it was evaluated ahead of the read;
    /// see [`Codegen::compound_rhs`].
    fn compound_value(
        &mut self,
        current: Value,
        place_ty: Ty,
        op: BinOp,
        value: &Expr,
        hoisted: Option<Value>,
        compute: Ty,
    ) -> Value {
        let span = self.sp(value.range);
        if place_ty.is_pointer() {
            // `p += n` moves by elements, not by bytes.
            let access = current.at(prec::CALL, span);
            let offset = match hoisted {
                Some(index) => self.offset_of_value(index, op == BinOp::Sub, span),
                None => self.offset_argument(value, op == BinOp::Sub, span),
            };
            return Value::new(quote_spanned! {span=> #access.offset(#offset) }, prec::CALL);
        }
        let current = self.cast(current, place_ty, compute, span);
        // The left operand is a place and therefore always typed, so a
        // constant right operand can stay a bare literal.
        let rhs = match hoisted {
            Some(rhs) => rhs,
            None => match constant_of(value) {
                Some(constant) => self.bare_value(constant, value.ty, span),
                None => self.expr(value),
            },
        };
        let result = self.binary(op, current, rhs, compute, span);
        self.cast(result, compute, place_ty, span)
    }

    /// Evaluates the right operand of a compound assignment ahead of the read,
    /// when C says it happens either wholly before it or wholly after it.
    ///
    /// `E1 op= E2` is a read, an operation and a write, and C11 6.5.16.2p3
    /// makes the three of them *one* evaluation with respect to an
    /// indeterminately sequenced function call. Writing them out as
    /// `E1 = E1 op E2` would read `E1`, call whatever `E2` calls, and only then
    /// store — which is the one order the standard rules out, and which GCC's
    /// `pr58943` is about. Binding `E2` to a temporary first restores it: the
    /// place is computed, then the call happens, then the read-modify-write.
    ///
    /// Only an operand that can call something needs it; everything else is
    /// left where it was written, because a temporary per `i += 1` would be
    /// noise.
    fn compound_rhs(&mut self, value: &Expr) -> (TokenStream, Option<Value>) {
        if !ir::calls_a_function(value) {
            return (TokenStream::new(), None);
        }
        let span = self.sp(value.range);
        let tokens = self.expr(value).at(prec::LOWEST, span);
        let tmp = self.temporary();
        (
            quote_spanned! {span=> let #tmp = #tokens; },
            Some(Value::atom(quote_spanned! {span=> #tmp })),
        )
    }

    /// The `offset` argument for an index that has already been emitted.
    fn offset_of_value(&mut self, index: Value, sub: bool, span: Span) -> TokenStream {
        let tokens = index.at(prec::CAST, span);
        if sub {
            quote_spanned! {span=> -(#tokens as isize) }
        } else {
            quote_spanned! {span=> #tokens as isize }
        }
    }

    /// The value `++place` or `--place` stores.
    fn step_value(&mut self, current: Value, ty: Ty, dec: bool, span: Span) -> TokenStream {
        if ty.is_pointer() {
            let access = current.at(prec::CALL, span);
            let one = if dec {
                quote_spanned! {span=> -1 }
            } else {
                quote_spanned! {span=> 1 }
            };
            return quote_spanned! {span=> #access.offset(#one) };
        }
        if ty.is_floating() {
            let access = current.at(prec::SUM, span);
            let one = Literal::f64_unsuffixed(1.0);
            let op = if dec {
                quote_spanned! {span=> - }
            } else {
                quote_spanned! {span=> + }
            };
            return quote_spanned! {span=> #access #op #one };
        }
        if ty.is_bool() {
            // `b++` is `b = b + 1 != 0`, which is `true` for `++` and the
            // negation of `b` for `--`.
            let access = current.at(prec::CAST, span);
            let int = self.ty(Ty::Int, span);
            let method = Ident::new(if dec { "wrapping_sub" } else { "wrapping_add" }, span);
            return quote_spanned! {span=> (#access as #int).#method(1) != 0 };
        }
        let access = current.at(prec::CALL, span);
        let method = Ident::new(if dec { "wrapping_sub" } else { "wrapping_add" }, span);
        quote_spanned! {span=> #access.#method(1) }
    }

    /// Emits a conversion between two scalar types.
    fn cast(&mut self, value: Value, from: Ty, to: Ty, span: Span) -> Value {
        if from == to {
            return value;
        }
        let types = &self.program.types;
        let from_fn = types.is_func_pointer(from);
        let to_fn = types.is_func_pointer(to);
        if to.is_bool() {
            if from.is_pointer() {
                // Handled by the caller for the common shapes; this is the
                // explicit `(_Bool)p`.
                let tokens = value.at(prec::CALL, span);
                return if from_fn {
                    let braced = braced(tokens, span);
                    Value::new(quote_spanned! {span=> #braced.is_some() }, prec::CALL)
                } else {
                    Value::new(quote_spanned! {span=> !#tokens.is_null() }, prec::UNARY)
                };
            }
            // Converting to `_Bool` yields 0 or 1, which is what a comparison
            // against zero gives.
            let zero = self.zero_tokens(from, span);
            let tokens = value.at(prec::CMP + 1, span);
            return Value::new(quote_spanned! {span=> #tokens != #zero }, prec::CMP);
        }
        if from_fn || to_fn {
            // Rust has no `as` between an `Option<fn>` and anything else, so a
            // transmute is the honest translation of what C's cast does — but
            // only between two things of the same size. An integer of any
            // other width goes through `usize`, which is what C's own
            // implementation-defined conversion between a pointer and an
            // integer amounts to.
            let target = self.ty(to, span);
            let source = self.ty(from, span);
            if to_fn && !from.is_pointer() {
                let tokens = value.at(prec::CAST, span);
                return Value::new(
                    quote_spanned! {span=>
                        ::core::mem::transmute::<usize, #target>(#tokens as usize)
                    },
                    prec::CALL,
                );
            }
            if from_fn && !to.is_pointer() {
                let tokens = value.at(prec::LOWEST, span);
                return Value::new(
                    quote_spanned! {span=>
                        ::core::mem::transmute::<#source, usize>(#tokens) as #target
                    },
                    prec::CAST,
                )
                .type_end(true);
            }
            let tokens = value.at(prec::LOWEST, span);
            return Value::new(
                quote_spanned! {span=>
                    ::core::mem::transmute::<#source, #target>(#tokens)
                },
                prec::CALL,
            );
        }
        let target = self.ty(to, span);
        if from.is_pointer() && to.is_integer() {
            // Rust only casts a pointer to `usize`; the rest is an ordinary
            // integer conversion.
            let tokens = value.at(prec::CAST, span);
            return Value::new(
                quote_spanned! {span=> #tokens as usize as #target },
                prec::CAST,
            )
            .type_end(true);
        }
        if from.is_bool() && to.is_floating() {
            // Rust has no `bool as f64`; C's `_Bool` to floating conversion
            // goes through the integer value.
            let int = self.ty(Ty::Int, span);
            let tokens = value.at(prec::CAST, span);
            return Value::new(
                quote_spanned! {span=> #tokens as #int as #target },
                prec::CAST,
            )
            .type_end(true);
        }
        let tokens = value.at(prec::CAST, span);
        Value::new(quote_spanned! {span=> #tokens as #target }, prec::CAST).type_end(true)
    }

    // -- places -------------------------------------------------------------

    /// A place, ready to be read from or written to.
    ///
    /// `mutable` says whether the place is about to be assigned to or have its
    /// address taken, which is what decides whether a `const` pointer under it
    /// has to lose the qualifier: Rust refuses `&raw mut (*p).f` when `p` is a
    /// `*const T`, while reading through one is fine.
    fn place(&mut self, place: &Place, mutable: bool) -> LoweredPlace {
        let mut lowered = self.place_access(place, mutable);
        if lowered.bits.is_none() && self.place_underaligned(place) {
            lowered.unaligned = true;
        }
        lowered
    }

    /// Whether the place has to be reached through an unaligned load or store.
    ///
    /// Two shapes need one. A `*p` or a `p[i]` whose pointer this crate itself
    /// built out of a packed member is underaligned, and so is a member read
    /// through a pointer to a record that is: `(*(T *) &buf).f`, where `buf` is
    /// a `char` array, is a member access Rust would compile to an aligned load
    /// of a byte-aligned address. A member of a *packed* record whose own
    /// address is fine needs nothing — Rust knows the layout of the item it was
    /// given and reads such a member unaligned by itself.
    fn place_underaligned(&self, place: &Place) -> bool {
        match &place.kind {
            PlaceKind::Deref(_) | PlaceKind::Index { .. } => {
                self.place_align(place) < self.type_align(place.ty)
            }
            PlaceKind::Field { base, .. } => self.place_align(base) < self.type_align(base.ty),
            _ => false,
        }
    }

    /// The alignment code generation can count on for a place's address.
    ///
    /// Everything C promises about an object's alignment holds for a place that
    /// names one, so the interesting half is what this crate itself can see
    /// through: the address of a member sits at its offset from a base whose
    /// alignment is known, and a cast in between changes nothing. Anything else
    /// — a pointer out of a variable, a parameter, a call — is taken at C's
    /// word and assumed to point at something its type is aligned for.
    fn place_align(&self, place: &Place) -> u64 {
        match &place.kind {
            PlaceKind::Deref(ptr) => self.pointer_align(ptr),
            PlaceKind::Index { base, .. } => {
                // Every element of an array sits at a multiple of the element
                // size from the first, and a size is always a multiple of the
                // alignment, so the elements are no worse aligned than the
                // element type asks and no better than the array is.
                self.pointer_align(base).min(self.type_align(place.ty))
            }
            PlaceKind::Field {
                base,
                record,
                index,
            } => {
                let base_align = self.place_align(base);
                let offset = self.program.types.record(*record).fields[*index].offset;
                if offset == 0 {
                    base_align
                } else {
                    base_align.min(1 << offset.trailing_zeros())
                }
            }
            _ => self.type_align(place.ty),
        }
    }

    /// The alignment of what a pointer expression points at.
    fn pointer_align(&self, ptr: &Expr) -> u64 {
        match &ptr.kind {
            ExprKind::AddrOf(place) => self.place_align(place),
            // A pointer cast moves no bytes, and neither does the decay of an
            // array to its first element.
            ExprKind::Cast(inner) if inner.ty.is_pointer() || inner.ty.is_array() => {
                self.pointer_align(inner)
            }
            ExprKind::PtrOffset { ptr, .. } => self
                .pointer_align(ptr)
                .min(self.pointee_align(ptr.ty).unwrap_or(1)),
            _ => self.pointee_align(ptr.ty).unwrap_or(u64::MAX),
        }
    }

    /// The alignment of the type a pointer or array type addresses.
    fn pointee_align(&self, ty: Ty) -> Option<u64> {
        let pointee = self.program.types.pointee(ty)?;
        (!pointee.is_void() && !pointee.is_func()).then(|| self.type_align(pointee))
    }

    /// The alignment the *generated Rust type* has, which is what a `*p` in the
    /// expansion is checked against.
    fn type_align(&self, ty: Ty) -> u64 {
        match ty {
            // A packed record's item is one byte aligned however strict C says
            // the record is; see `ir::RecordDef::rust_align`.
            Ty::Record(id) => self.program.types.record(id).rust_align,
            Ty::Array(id) => self.type_align(self.program.types.array_type(id).elem),
            _ => self
                .program
                .types
                .size_align(ty, &self.options.target)
                .map_or(1, |layout| layout.align),
        }
    }

    fn place_access(&mut self, place: &Place, mutable: bool) -> LoweredPlace {
        let span = self.sp(place.range);
        match &place.kind {
            PlaceKind::Object(id) => {
                let name = self.object_ident(*id, span);
                LoweredPlace::plain(TokenStream::new(), quote_spanned! {span=> #name })
            }
            PlaceKind::Deref(ptr) => self.deref_place(ptr, place.ty, mutable, span),
            PlaceKind::Index { base, index } => {
                let pointer = self.pointer_operand(base, place.ty, mutable, span);
                let offset = self.offset_argument(index, false, span);
                let tmp = self.temporary();
                LoweredPlace::plain(
                    quote_spanned! {span=> let #tmp = #pointer.offset(#offset); },
                    parenthesize(quote_spanned! {span=> *#tmp }, span),
                )
            }
            PlaceKind::Field {
                base,
                record,
                index,
            } => {
                let field = self.program.types.record(*record).fields[*index].clone();
                let lowered = self.place(base, mutable);
                let access = lowered.access;
                let Some(bits) = &field.bits else {
                    let name = c_ident(&field.name, span);
                    return LoweredPlace::plain(
                        lowered.setup,
                        quote_spanned! {span=> #access.#name },
                    );
                };
                // The accessors take `&self` and `&mut self`, and a reference
                // to a `static mut` is exactly what edition 2024 refuses; the
                // raw pointer keeps the item out of the expression.
                let access = if rooted_in_static(base, self.program) {
                    parenthesize(quote_spanned! {span=> *(&raw mut #access) }, span)
                } else {
                    access
                };
                LoweredPlace {
                    setup: lowered.setup,
                    access,
                    bits: Some(BitAccess {
                        getter: c_ident(&bits.getter, span),
                        setter: c_ident(&bits.setter, span),
                    }),
                    unaligned: false,
                }
            }
            PlaceKind::Str(id) => {
                let pointer = self.string_pointer(*id, !mutable, span);
                let tmp = self.temporary();
                LoweredPlace::plain(
                    quote_spanned! {span=> let #tmp = #pointer; },
                    parenthesize(quote_spanned! {span=> *#tmp }, span),
                )
            }
            PlaceKind::Temporary(expr) => {
                let ty = expr.ty;
                let value = self.expr_at(expr, ty);
                let tmp = self.temporary();
                LoweredPlace::plain(
                    quote_spanned! {span=> let mut #tmp = #value; },
                    quote_spanned! {span=> #tmp },
                )
            }
            // The binding itself was made at the top of the enclosing block —
            // C gives the object that lifetime, and it is what lets `&(T){…}`
            // outlive the expression. What happens *here* is the
            // initialisation, so that side effects in it happen where the
            // literal was written and a literal in a loop is rebuilt on every
            // iteration.
            PlaceKind::CompoundLiteral { object, init } => {
                let name = self.object_ident(*object, span);
                let value = self.expr_at(init, place.ty);
                LoweredPlace::plain(
                    quote_spanned! {span=> #name = #value; },
                    quote_spanned! {span=> #name },
                )
            }
        }
    }

    /// Reads a lowered place.
    fn read(&self, place: &LoweredPlace, span: Span) -> Value {
        let access = &place.access;
        match &place.bits {
            Some(bits) => {
                let getter = &bits.getter;
                Value::new(quote_spanned! {span=> #access.#getter() }, prec::CALL)
            }
            None if place.unaligned => Value::new(
                quote_spanned! {span=> (&raw const #access).read_unaligned() },
                prec::CALL,
            ),
            None => Value::atom(access.clone()),
        }
    }

    /// The statement that stores `value` into a lowered place.
    fn write(&self, place: &LoweredPlace, value: TokenStream, span: Span) -> TokenStream {
        let access = &place.access;
        match &place.bits {
            Some(bits) => {
                let setter = &bits.setter;
                quote_spanned! {span=> #access.#setter(#value); }
            }
            None if place.unaligned => {
                quote_spanned! {span=> (&raw mut #access).write_unaligned(#value); }
            }
            None => quote_spanned! {span=> #access = #value; },
        }
    }

    /// `*p` as a place.
    fn deref_place(&mut self, ptr: &Expr, pointee: Ty, mutable: bool, span: Span) -> LoweredPlace {
        let simple =
            matches!(&ptr.kind, ExprKind::Load(p) if matches!(p.kind, PlaceKind::Object(_)));
        if simple {
            // A variable holding the pointer can be dereferenced as often as
            // needed, so no temporary is called for.
            let tokens = self.pointer_operand(ptr, pointee, mutable, span);
            return LoweredPlace::plain(
                TokenStream::new(),
                parenthesize(quote_spanned! {span=> *#tokens }, span),
            );
        }
        let value = self.pointer_operand(ptr, pointee, mutable, span);
        let tmp = self.temporary();
        LoweredPlace::plain(
            quote_spanned! {span=> let #tmp = #value; },
            parenthesize(quote_spanned! {span=> *#tmp }, span),
        )
    }

    /// The pointer a place is built on.
    ///
    /// Writing through a `*const T` is not allowed even in `unsafe` Rust, and
    /// C's own `const` checking has already happened in sema, so a place that
    /// is about to be written to (or have its address taken) drops the
    /// qualifier here rather than at every use.
    fn pointer_operand(
        &mut self,
        ptr: &Expr,
        pointee: Ty,
        mutable: bool,
        span: Span,
    ) -> TokenStream {
        if mutable && self.program.types.points_to_const(ptr.ty) {
            let target = self.pointee_ty(pointee, span);
            let tokens = self.expr(ptr).at(prec::CAST, span);
            return parenthesize(quote_spanned! {span=> #tokens as *mut #target }, span);
        }
        self.expr(ptr).at(prec::CALL, span)
    }

    /// The address of a place, as a pointer of type `want`.
    fn address_of(&mut self, place: &Place, want: Ty, span: Span) -> Value {
        // A variable length array's binding already *is* the address of its
        // first element, so both the decay `a` and the array pointer `&a` are
        // that binding — the second one only differs in its C type.
        if let PlaceKind::Object(id) = &place.kind
            && self.program.types.is_vla(place.ty)
        {
            let name = self.object_ident(*id, span);
            let value = Value::atom(quote_spanned! {span=> #name });
            let elem = self.program.types.elem(place.ty);
            let natural = self.program.types.pointee(want) == elem
                && !self.program.types.points_to_const(want);
            if natural {
                return value;
            }
            let target = self.ty(want, span);
            let tokens = value.at(prec::CAST, span);
            return Value::new(quote_spanned! {span=> #tokens as #target }, prec::CAST)
                .type_end(true);
        }
        // `&*p` is `p`, and `&a[i]` is `a + i`; saying so keeps the output
        // free of pointless round trips through a place.
        match &place.kind {
            PlaceKind::Deref(ptr) => {
                let from = ptr.ty;
                let value = self.expr(ptr);
                return self.pointer_cast(value, from, want, span);
            }
            PlaceKind::Index { base, index } => {
                let from = base.ty;
                let pointer = self.expr(base).at(prec::CALL, span);
                let offset = self.offset_argument(index, false, span);
                let value = Value::new(
                    quote_spanned! {span=> #pointer.offset(#offset) },
                    prec::CALL,
                );
                return self.pointer_cast(value, from, want, span);
            }
            PlaceKind::Str(id) => {
                let konst = self.program.types.points_to_const(want);
                let tokens = self.string_pointer(*id, konst, span);
                return Value::new(tokens, prec::CALL);
            }
            _ => {}
        }
        let lowered = self.place(place, true);
        let access = lowered.access;
        let address = quote_spanned! {span=> &raw mut #access };
        let natural = Value::new(parenthesize(address, span), prec::ATOM);
        let value = self.array_or_pointer_cast(natural, place.ty, want, span);
        if lowered.setup.is_empty() {
            return value;
        }
        let setup = lowered.setup;
        let tokens = value.at(prec::LOWEST, span);
        Value::new(quote_spanned! {span=> { #setup #tokens } }, prec::BLOCK)
    }

    /// Adjusts `&raw mut place` to the pointer type the expression wants.
    fn array_or_pointer_cast(&mut self, value: Value, from: Ty, want: Ty, span: Span) -> Value {
        if let Ty::Array(id) = from {
            // The address of an array is a pointer to the array; decaying it
            // to a pointer to the first element is a `cast`, which — unlike
            // `as` — cannot silently change anything else.
            let array = self.program.types.array_type(id);
            let wanted_pointee = self.program.types.pointee(want);
            if wanted_pointee == Some(array.elem) {
                let elem = self.pointee_ty(array.elem, span);
                let tokens = value.at(prec::CALL, span);
                let cast = Value::new(quote_spanned! {span=> #tokens.cast::<#elem>() }, prec::CALL);
                return self.constify(cast, want, span);
            }
        }
        let natural_mut = matches!(self.program.types.pointee(want), Some(pointee) if pointee == from)
            && !self.program.types.points_to_const(want);
        if natural_mut {
            return value;
        }
        let target = self.ty(want, span);
        let tokens = value.at(prec::CAST, span);
        Value::new(quote_spanned! {span=> #tokens as #target }, prec::CAST).type_end(true)
    }

    /// Adds the `as *const T` that a `*mut T` needs to become a `*const T`.
    fn constify(&mut self, value: Value, want: Ty, span: Span) -> Value {
        if !self.program.types.points_to_const(want) {
            return value;
        }
        let target = self.ty(want, span);
        let tokens = value.at(prec::CAST, span);
        Value::new(quote_spanned! {span=> #tokens as #target }, prec::CAST).type_end(true)
    }

    /// Casts a pointer value to the pointer type wanted.
    fn pointer_cast(&mut self, value: Value, from: Ty, want: Ty, span: Span) -> Value {
        if from == want {
            return value;
        }
        let target = self.ty(want, span);
        let tokens = value.at(prec::CAST, span);
        Value::new(quote_spanned! {span=> #tokens as #target }, prec::CAST).type_end(true)
    }

    /// The pointer a string literal decays to.
    fn string_pointer(&mut self, id: ir::StrId, konst: bool, span: Span) -> TokenStream {
        let data = self.program.string(id);
        let element = data.elem;
        if element.size_bytes(&self.options.target) > 1 {
            // A `wchar_t`, `char16_t` or `char32_t` literal needs real storage
            // of that type; a `static` in the enclosing block is the only
            // thing with a long enough lifetime.
            let elem = self.ty(element, span);
            let mut items = TokenStream::new();
            for value in data.values.iter().chain(std::iter::once(&0)) {
                let literal =
                    int_literal_token(element.wrap(i128::from(*value), &self.options.target), span);
                items.extend(quote_spanned! {span=> #literal, });
            }
            let len = usize_literal(data.len_with_nul(), span);
            let name = Ident::new("__CINRS_WIDE", Span::mixed_site());
            let array = bracketed(items, span);
            let ty = bracketed(quote_spanned! {span=> #elem ; #len }, span);
            let pointer = if konst {
                quote_spanned! {span=> (&raw const #name).cast::<#elem>() }
            } else {
                quote_spanned! {span=> (&raw const #name).cast::<#elem>().cast_mut() }
            };
            return quote_spanned! {span=>
                { static #name: #ty = #array; #pointer }
            };
        }
        // A narrow or `u8"…"` literal is a Rust byte string, whose elements
        // are the same bytes whether C calls them `char` or `char8_t`.
        let mut literal = Literal::byte_string(&nul_terminated(&data.values));
        literal.set_span(span);
        let elem = self.ty(element, span);
        if konst {
            quote_spanned! {span=> #literal.as_ptr().cast::<#elem>() }
        } else {
            quote_spanned! {span=> #literal.as_ptr().cast::<#elem>().cast_mut() }
        }
    }

    // -- literals -----------------------------------------------------------

    fn int_literal(&self, value: i128, ty: Ty, span: Span) -> Value {
        if ty.is_bool() {
            return Value::atom(bare_int_literal(value, ty, span));
        }
        let literal = int_literal_token(value, span);
        let target = self.ty(ty, span);
        Value::new(quote_spanned! {span=> #literal as #target }, prec::CAST).type_end(true)
    }

    fn float_literal(&self, value: f64, ty: Ty, span: Span) -> Value {
        if !value.is_finite() {
            return Value::new(self.non_finite_literal(value, ty, span), prec::CAST).type_end(true);
        }
        let literal = float_literal_token(value, span);
        let target = self.ty(ty, span);
        Value::new(quote_spanned! {span=> #literal as #target }, prec::CAST).type_end(true)
    }

    /// An infinity or a NaN, which no Rust literal can spell.
    fn non_finite_literal(&self, value: f64, ty: Ty, span: Span) -> TokenStream {
        let target = self.ty(ty, span);
        if value.is_nan() {
            return quote_spanned! {span=> f64::NAN as #target };
        }
        if value.is_sign_negative() {
            quote_spanned! {span=> -f64::INFINITY as #target }
        } else {
            quote_spanned! {span=> f64::INFINITY as #target }
        }
    }

    /// The all-bits-zero value of a type.
    fn zero_tokens(&self, ty: Ty, span: Span) -> TokenStream {
        match ty {
            _ if ty.is_floating() => bare_float_literal(0.0, span),
            _ if ty.is_integer() => bare_int_literal(0, ty, span),
            // A variable length array is generated as a pointer, and the only
            // thing that ever asks for its zero is the hoisting a [CFG
            // body](crate::cfg) does before the declaration is reached.
            Ty::Array(id) if self.program.types.array_type(id).vla => {
                let elem = self.ty(self.program.types.array_type(id).elem, span);
                quote_spanned! {span=> ::core::ptr::null_mut::<#elem>() }
            }
            Ty::Pointer(id) => {
                let pointer = self.program.types.pointer_type(id);
                if let Ty::Func(func) = pointer.pointee {
                    // The signature is written out rather than left to
                    // inference: a null function pointer is often the whole
                    // expression — `((void (*)(void))0)()` — and a bare
                    // `Option::None` there is `E0282`.
                    let signature = self.fn_ty(func, span);
                    return quote_spanned! {span=>
                        ::core::option::Option::<#signature>::None
                    };
                }
                let pointee = self.pointee_ty(pointer.pointee, span);
                if pointer.konst {
                    quote_spanned! {span=> ::core::ptr::null::<#pointee>() }
                } else {
                    quote_spanned! {span=> ::core::ptr::null_mut::<#pointee>() }
                }
            }
            other => {
                // A zeroed aggregate: `mem::zeroed` is a `const fn`, so this
                // works in a `static` initialiser as well as in a body.
                let target = self.ty(other, span);
                quote_spanned! {span=> ::core::mem::zeroed::<#target>() }
            }
        }
    }
}

/// The value a bit-field's initialiser folds to, if it folds at all.
///
/// Sema has already reduced a constant to a bare `Int` node, and the implicit
/// zero every unmentioned member gets is either that or [`ExprKind::Zeroed`].
fn constant_bits(expr: &Expr) -> Option<i128> {
    match &expr.kind {
        ExprKind::Int(value) => Some(*value),
        ExprKind::Zeroed if expr.ty.is_integer() => Some(0),
        _ => None,
    }
}

/// Writes the low `width` bits of `value` into a run's storage bytes.
fn pack_bits(storage: &mut [u8], bits: &ir::BitField, value: i128) {
    let start = bits.offset_in_storage();
    for bit in 0..u64::from(bits.width) {
        if (value as u128) >> bit & 1 == 0 {
            continue;
        }
        let at = start + bit;
        if let Some(byte) = storage.get_mut((at / 8) as usize) {
            *byte |= 1 << (at % 8);
        }
    }
}

/// `[0x1f, 0x00, …]`, the initialiser of a storage field.
fn byte_array(bytes: &[u8], span: Span) -> TokenStream {
    if bytes.iter().all(|byte| *byte == 0) {
        let len = usize_literal(bytes.len() as u64, span);
        return bracketed(quote_spanned! {span=> 0; #len }, span);
    }
    let mut items = TokenStream::new();
    for byte in bytes {
        let value = hex_literal(u64::from(*byte), span);
        items.extend(quote_spanned! {span=> #value, });
    }
    bracketed(items, span)
}

/// Whether a place ultimately names an object with static storage duration.
///
/// The bit-field accessors borrow, and edition 2024 refuses a reference to a
/// `static mut`; a place rooted in one is reached through `&raw mut` instead.
/// Anything behind a pointer is already a raw dereference, so it needs nothing.
fn rooted_in_static(place: &Place, program: &Program) -> bool {
    let mut place = place;
    loop {
        match &place.kind {
            PlaceKind::Object(id) => {
                return !matches!(program.object(*id).storage, Storage::Automatic);
            }
            PlaceKind::Field { base, .. } => place = base,
            _ => return false,
        }
    }
}

/// The Rust unsigned integer of a given width, which is what the bit-counting
/// builtins are defined on.
fn unsigned_rust_ty(width: u32, span: Span) -> TokenStream {
    let name = match width {
        0..=8 => "u8",
        9..=16 => "u16",
        17..=32 => "u32",
        _ => "u64",
    };
    let ident = Ident::new(name, span);
    quote_spanned! {span=> #ident }
}

/// The signed counterpart, for `__builtin_clrsb`.
fn signed_rust_ty(width: u32, span: Span) -> TokenStream {
    let name = match width {
        0..=8 => "i8",
        9..=16 => "i16",
        17..=32 => "i32",
        _ => "i64",
    };
    let ident = Ident::new(name, span);
    quote_spanned! {span=> #ident }
}

/// A mask of `width` low bits, in a `u64`.
fn mask_of(width: u32) -> u64 {
    if width >= 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    }
}

/// A `u64` literal written in hexadecimal, which is how a mask reads.
fn hex_literal(value: u64, span: Span) -> TokenStream {
    let mut literal = Literal::from_str(&format!("0x{value:x}"))
        .unwrap_or_else(|_| Literal::u64_unsuffixed(value));
    literal.set_span(span);
    TokenStream::from(TokenTree::Literal(literal))
}

/// A state number, which is a `u32` because the state variable is.
fn state_literal(value: usize, span: Span) -> TokenStream {
    let mut literal = Literal::u32_unsuffixed(value as u32);
    literal.set_span(span);
    TokenStream::from(TokenTree::Literal(literal))
}

/// Groups a switch's cases by the block they enter, keeping source order.
///
/// `case 0: case 1:` reaches the same block through two labels, and one arm
/// with an or-pattern is how that should read.
fn group_cases(cases: &[(ir::CaseRange, BlockId)]) -> Vec<(BlockId, Vec<ir::CaseRange>)> {
    let mut out: Vec<(BlockId, Vec<ir::CaseRange>)> = Vec::new();
    for (value, target) in cases {
        match out.iter_mut().find(|(block, _)| block == target) {
            Some((_, values)) => values.push(*value),
            None => out.push((*target, vec![*value])),
        }
    }
    out
}

/// The Rust pattern one `case` label matches: a literal, or a range.
fn case_pattern(value: ir::CaseRange, ty: Ty, span: Span) -> TokenStream {
    let low = bare_int_literal(value.low, ty, span);
    if value.is_single() {
        return low;
    }
    let high = bare_int_literal(value.high, ty, span);
    quote_spanned! {span=> #low ..= #high }
}

/// The precedence of the tokens [`Codegen::zero_tokens`] produces.
fn zero_prec(ty: Ty) -> u8 {
    if ty.is_arithmetic() {
        prec::ATOM
    } else {
        prec::CALL
    }
}

/// `#[link_name = "…"]`, which points a renamed declaration back at its symbol.
fn link_name(symbol: &str, span: Span) -> TokenStream {
    let mut literal = Literal::string(symbol);
    literal.set_span(span);
    quote_spanned! {span=> #[link_name = #literal] }
}

/// The attribute that gives a definition the C symbol `symbol`, for a unit
/// that asked for `#pragma cinrs export`.
///
/// `#[unsafe(no_mangle)]` is the edition-2024 spelling and is accepted in every
/// edition since 1.82, so one expansion works wherever it is written. It says
/// "the name of the item is the symbol", which is not quite always true here:
/// a C name that is a Rust keyword becomes `r#match`, and the five names that
/// cannot even be raw grow an underscore, so those go through `export_name`
/// instead and say the symbol outright.
fn export_attr(symbol: &str, item: &Ident, span: Span) -> TokenStream {
    if item.to_string().trim_start_matches("r#") == symbol {
        return quote_spanned! {span=> #[unsafe(no_mangle)] };
    }
    let mut literal = Literal::string(symbol);
    literal.set_span(span);
    quote_spanned! {span=> #[unsafe(export_name = #literal)] }
}

/// Whether a static initialiser has to be wrapped in `unsafe`.
fn needs_unsafe(expr: &Expr) -> bool {
    match &expr.kind {
        // `mem::zeroed` is unsafe, and so is naming a `static mut`.
        ExprKind::Zeroed => !expr.ty.is_scalar(),
        // A string literal's address is safe to take; anything else with static
        // storage duration is a `static mut`.
        ExprKind::AddrOf(place) => !matches!(place.kind, PlaceKind::Str(_)),
        ExprKind::Cast(inner) => needs_unsafe(inner),
        ExprKind::PtrOffset { ptr, .. } => needs_unsafe(ptr),
        ExprKind::RecordLit { fields, .. } => fields.iter().any(needs_unsafe),
        ExprKind::UnionLit { value, .. } => needs_unsafe(value),
        ExprKind::ArrayLit(items) => items.iter().any(needs_unsafe),
        ExprKind::ArrayRepeat { value, .. } => needs_unsafe(value),
        _ => false,
    }
}

/// The bytes of a narrow string literal, with its terminating NUL.
fn nul_terminated(values: &[u32]) -> Vec<u8> {
    let mut bytes: Vec<u8> = values.iter().map(|v| *v as u8).collect();
    bytes.push(0);
    bytes
}

// ---------------------------------------------------------------------------
// literal tokens
// ---------------------------------------------------------------------------

/// The largest magnitude an unsuffixed literal is safe to have: Rust infers
/// `i32` for a literal with no other constraint.
const UNSUFFIXED_LIMIT: i128 = i32::MAX as i128;

/// A literal for `value`, with a Rust suffix only when inference needs one.
fn int_literal_token(value: i128, span: Span) -> TokenStream {
    let magnitude = value.unsigned_abs();
    let mut literal = if magnitude <= UNSUFFIXED_LIMIT as u128 {
        Literal::u128_unsuffixed(magnitude)
    } else if value >= 0 {
        if magnitude <= u32::MAX as u128 {
            Literal::u32_suffixed(magnitude as u32)
        } else if magnitude <= u64::MAX as u128 {
            Literal::u64_suffixed(magnitude as u64)
        } else {
            Literal::u128_suffixed(magnitude)
        }
    } else if magnitude <= i64::MAX as u128 {
        Literal::i64_suffixed(magnitude as i64)
    } else {
        Literal::i128_suffixed(magnitude as i128)
    };
    literal.set_span(span);
    if value < 0 {
        quote_spanned! {span=> -#literal }
    } else {
        TokenStream::from(TokenTree::Literal(literal))
    }
}

/// A literal for `value` with no suffix at all, for a context that already
/// fixes its type.
fn bare_int_literal(value: i128, ty: Ty, span: Span) -> TokenStream {
    if ty.is_bool() {
        let ident = Ident::new(if value != 0 { "true" } else { "false" }, span);
        return quote_spanned! {span=> #ident };
    }
    let mut literal = Literal::u128_unsuffixed(value.unsigned_abs());
    literal.set_span(span);
    if value < 0 {
        quote_spanned! {span=> -#literal }
    } else {
        TokenStream::from(TokenTree::Literal(literal))
    }
}

/// An array length, which Rust counts in `usize`.
fn usize_literal(value: u64, span: Span) -> TokenStream {
    let mut literal = Literal::usize_unsuffixed(value as usize);
    literal.set_span(span);
    TokenStream::from(TokenTree::Literal(literal))
}

fn float_literal_token(value: f64, span: Span) -> TokenStream {
    let mut literal = Literal::f64_unsuffixed(value.abs());
    literal.set_span(span);
    if value.is_sign_negative() {
        quote_spanned! {span=> -#literal }
    } else {
        TokenStream::from(TokenTree::Literal(literal))
    }
}

fn bare_float_literal(value: f64, span: Span) -> TokenStream {
    float_literal_token(value, span)
}

/// The constant an expression is, if it is one.
///
/// Sema folds conversions of constants, so a constant is always a bare `Int`
/// or `Float` node rather than a cast wrapping one.
/// The operands of a chain of comma operators, in source order.
///
/// `a, b, c` is `Comma(Comma(a, b), c)`, so this walks down the left spine
/// into a vector and hands it back the right way round. Iterating rather than
/// recursing is what lets a logical source line hold the 4095 characters C23
/// 5.2.5.2p1 asks for; see [`Codegen::binary_chain`].
fn comma_operands(expr: &Expr) -> Vec<&Expr> {
    let mut out = Vec::new();
    let mut node = expr;
    while let ExprKind::Comma { lhs, rhs } = &node.kind {
        out.push(&**rhs);
        node = lhs;
    }
    out.push(node);
    out.reverse();
    out
}

fn constant_of(expr: &Expr) -> Option<ConstValue> {
    match &expr.kind {
        ExprKind::Int(value) => Some(ConstValue::Int(*value)),
        ExprKind::Float(value) if value.is_finite() => Some(ConstValue::Float(*value)),
        _ => None,
    }
}

fn cmp_tokens(op: CmpOp, span: Span) -> TokenStream {
    match op {
        CmpOp::Lt => quote_spanned! {span=> < },
        CmpOp::Gt => quote_spanned! {span=> > },
        CmpOp::Le => quote_spanned! {span=> <= },
        CmpOp::Ge => quote_spanned! {span=> >= },
        CmpOp::Eq => quote_spanned! {span=> == },
        CmpOp::Ne => quote_spanned! {span=> != },
    }
}
