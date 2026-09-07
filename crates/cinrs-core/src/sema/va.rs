//! `va_list` and the `<stdarg.h>` builtins.
//!
//! # The model
//!
//! Rust's `core::ffi::VaList` is a value that owns its position in the caller's
//! argument list: reading an argument advances it, cloning it is `va_copy`, and
//! dropping it is `va_end`. C's `va_list` is an object the program assigns to,
//! so the two are lined up like this:
//!
//! * A variadic definition takes one extra parameter, the `...` of the Rust
//!   signature. It is never advanced — it is the *pristine* list, the state
//!   `va_start` rewinds to.
//! * `va_list ap;` becomes a local initialised from that pristine list, so that
//!   the Rust value exists before `va_start` ever runs. In a function that has
//!   a `va_list` *parameter* instead of `...` (`vfprintf`-style), the parameter
//!   is what a local is copied from.
//! * `va_start(ap, last)` assigns the pristine list to `ap` again, which is
//!   exactly what "rewind to the first variable argument" means. `va_end(ap)`
//!   is therefore nothing at all, and a `va_start` after it works.
//! * `va_copy(dst, src)` and passing a list to another function are both
//!   `src.clone()`: C says the source is indeterminate afterwards, and cloning
//!   is what keeps Rust's move checking from disagreeing.
//! * `va_arg(ap, T)` is `ap.next_arg::<T>()`, using the `core::ffi` type of `T`
//!   — those are aliases of the primitives `VaArgSafe` is implemented for.
//!
//! # What is refused
//!
//! `va_list` is opaque: it has no size, nothing may point at it, and it may
//! only be a local or a parameter, because Rust's `VaList` carries a lifetime
//! that a `struct` member or a `static` would have to name. `va_arg` refuses a
//! type the default argument promotions would have changed on the way in
//! (`char`, `short`, `_Bool`, `float`), which is undefined behaviour in C and
//! which GCC diagnoses in the same words. It also refuses `__int128`: Rust
//! implements `VaArgSafe` for the 128-bit primitives only behind the unstable
//! `c_variadic_int128` feature. *Passing* one through `...` is unaffected —
//! that is the call site, and the ABI — so a program can still read it back as
//! two `unsigned long long` halves.

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{Expr, ExprKind, Place, Ty};

use super::{Sema, VA_LIST_PLACEMENT};

/// One of the builtins `<stdarg.h>`'s macros stand for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum VaBuiltin {
    /// `va_start(ap, last)`
    Start,
    /// `va_end(ap)`
    End,
    /// `va_copy(dst, src)`
    Copy,
}

impl VaBuiltin {
    /// Recognises a builtin by name.
    ///
    /// Only the `__builtin_` spellings, which is what GCC reserves too: the
    /// unprefixed names belong to `<stdarg.h>`, which `#define`s them to
    /// these, so a unit that does not include it may use them for whatever it
    /// likes.
    pub(super) fn from_name(name: &str) -> Option<Self> {
        Some(match name.strip_prefix("__builtin_")? {
            "va_start" => VaBuiltin::Start,
            "va_end" => VaBuiltin::End,
            "va_copy" => VaBuiltin::Copy,
            _ => return None,
        })
    }

    fn spelling(self) -> &'static str {
        match self {
            VaBuiltin::Start => "va_start",
            VaBuiltin::End => "va_end",
            VaBuiltin::Copy => "va_copy",
        }
    }

    fn arity(self) -> usize {
        match self {
            VaBuiltin::End => 1,
            VaBuiltin::Start | VaBuiltin::Copy => 2,
        }
    }
}

impl Sema<'_> {
    /// Checks `va_start`, `va_end` or `va_copy`.
    ///
    /// They are `void` expressions; `va_start` and `va_copy` become ordinary
    /// assignments to the list, which is all their semantics amount to here.
    pub(super) fn va_builtin(
        &mut self,
        builtin: VaBuiltin,
        args: &[ast::Expr],
        range: SourceRange,
    ) -> Option<Expr> {
        if args.len() != builtin.arity() {
            self.error(
                range,
                format!(
                    "'{}' expects {} arguments, have {}",
                    builtin.spelling(),
                    builtin.arity(),
                    args.len()
                ),
            );
            return None;
        }
        match builtin {
            VaBuiltin::Start => {
                let ap = self.va_list_lvalue(&args[0])?;
                if !self.func_variadic {
                    self.error(range, "'va_start' used in a function with fixed arguments");
                    return None;
                }
                self.check_va_start_parameter(&args[1]);
                let value = Expr::new(ExprKind::VaListPristine, Ty::VaList, range);
                Some(assign(ap, value, range))
            }
            VaBuiltin::End => {
                self.va_list_lvalue(&args[0])?;
                Some(Expr::new(ExprKind::VaEnd, Ty::Void, range))
            }
            VaBuiltin::Copy => {
                let dst = self.va_list_lvalue(&args[0])?;
                let src = self.expr(&args[1])?;
                if src.ty.is_error() {
                    return None;
                }
                if !src.ty.is_va_list() {
                    self.error(
                        args[1].range,
                        format!(
                            "the second argument of 'va_copy' must have type 'va_list', not '{}'",
                            self.tyname(src.ty)
                        ),
                    );
                    return None;
                }
                Some(assign(dst, src, range))
            }
        }
    }

    /// Checks `va_arg(ap, T)`.
    pub(super) fn va_arg(
        &mut self,
        ap: &ast::Expr,
        type_name: &ast::TypeName,
        range: SourceRange,
    ) -> Option<Expr> {
        let place = self.va_list_lvalue(ap)?;
        let target = self.ty_of(&type_name.ty)?;
        self.check_va_arg_type(target, type_name.range)?;
        Some(Expr::new(ExprKind::VaArg { ap: place }, target, range))
    }

    /// Whether a type may be read out of an argument list, and why not.
    fn check_va_arg_type(&mut self, ty: Ty, range: SourceRange) -> Option<()> {
        if ty.is_error() {
            return None;
        }
        if ty.is_pointer() || ty.is_enum() || ty == Ty::Double {
            return Some(());
        }
        if ty.is_int128() {
            // `VaArgSafe` is what `next_arg` needs, and Rust implements it for
            // the 128-bit primitives behind the unstable `c_variadic_int128`
            // feature. Nothing stable can read one out of an argument list, so
            // saying so beats an `E0658` about a feature the user never named.
            self.error(
                range,
                format!(
                    "va_arg with '{}' is not supported: Rust's `VaArgSafe` covers the \
                     128-bit types only behind the unstable `c_variadic_int128` feature",
                    self.tyname(ty)
                ),
            );
            return None;
        }
        if ty.is_complex() {
            // `next_arg` needs `VaArgSafe`, and Rust implements it for the
            // primitives only. A complex value is a two-field `#[repr(C)]`
            // struct, which is the same case as any other aggregate — and on
            // System V it is not even passed like one, being two SSE
            // eightbytes rather than memory.
            self.error(
                range,
                format!(
                    "va_arg with '{}' is not supported: Rust's `VaArgSafe` covers the \
                     primitive types only, and a complex value is a pair",
                    self.tyname(ty)
                ),
            );
            return None;
        }
        if ty.is_arithmetic() {
            let promoted = ty.promote_argument(&self.target);
            if promoted == ty {
                return Some(());
            }
            // GCC's wording, because it says exactly what went wrong: the
            // argument was widened on the way in, so reading it back at its
            // written type would read the wrong bytes.
            self.error(
                range,
                format!(
                    "'{}' is promoted to '{}' when passed through '...'; you should pass \
                     '{}' not '{}' to 'va_arg'",
                    self.tyname(ty),
                    self.tyname(promoted),
                    self.tyname(promoted),
                    self.tyname(ty)
                ),
            );
            return None;
        }
        if ty.is_record() {
            self.error(range, "va_arg with a struct type is not supported yet");
            return None;
        }
        self.error(
            range,
            format!("va_arg with type '{}' is not supported", self.tyname(ty)),
        );
        None
    }

    /// Resolves an argument that must denote a `va_list` object.
    fn va_list_lvalue(&mut self, expr: &ast::Expr) -> Option<Place> {
        let place = self.lvalue_assignable(expr)?;
        if place.ty.is_error() {
            return None;
        }
        if !place.ty.is_va_list() {
            self.error(
                expr.range,
                format!(
                    "expected an object of type 'va_list', not '{}'",
                    self.tyname(place.ty)
                ),
            );
            return None;
        }
        Some(place)
    }

    /// Checks the second argument of `va_start`.
    ///
    /// C requires the last named parameter and leaves anything else undefined;
    /// GCC only warns about naming a different one, and this follows it — the
    /// generated code does not use the argument at all. Naming something that
    /// is not a parameter is refused, because it is always a mistake.
    fn check_va_start_parameter(&mut self, arg: &ast::Expr) {
        if let ast::ExprKind::Ident(name) = &arg.kind
            && let Some(super::Entry::Object(id)) = self.lookup(&name.name)
            && self.func_params.contains(id)
        {
            return;
        }
        let func = self.func_name.clone();
        self.error(
            arg.range,
            format!("the second argument of 'va_start' must name a parameter of '{func}'"),
        );
    }

    /// The value a `va_list` local starts out holding.
    ///
    /// `None` — with the reason reported — when there is no argument list in
    /// scope to copy.
    pub(super) fn va_list_init(&mut self, range: SourceRange) -> Option<Expr> {
        if self.func_variadic || self.va_param.is_some() {
            return Some(Expr::new(ExprKind::VaListPristine, Ty::VaList, range));
        }
        self.error(
            range,
            "a 'va_list' variable can only be declared in a variadic function or in one \
             that takes a 'va_list' parameter",
        );
        None
    }

    /// Reports `va_list` used where the generated Rust could not name it.
    ///
    /// A *pointer* to one counts: `*mut core::ffi::VaList<'f>` carries the
    /// same lifetime the value does, and elision only supplies it inside a
    /// function — a `struct` member, a `static` and a return type would each
    /// have to name it.
    pub(super) fn reject_va_list(&mut self, ty: Ty, range: SourceRange) -> bool {
        if !self.mentions_va_list(ty) {
            return false;
        }
        self.error(range, VA_LIST_PLACEMENT);
        true
    }

    /// Whether a type is `va_list *` — a pointer straight to a list.
    pub(super) fn points_to_va_list(&self, ty: Ty) -> bool {
        self.types()
            .pointee(ty)
            .is_some_and(|pointee| pointee.is_va_list())
    }

    /// Whether `va_list` appears anywhere inside a type.
    pub(super) fn mentions_va_list(&self, ty: Ty) -> bool {
        match ty {
            Ty::VaList => true,
            Ty::Pointer(id) => self.mentions_va_list(self.types().pointer_type(id).pointee),
            Ty::Array(id) => self.mentions_va_list(self.types().array_type(id).elem),
            _ => false,
        }
    }
}

/// `place = value`, as a `void` expression.
fn assign(place: Place, value: Expr, range: SourceRange) -> Expr {
    Expr::new(
        ExprKind::Assign {
            place,
            value: Box::new(value),
        },
        Ty::Void,
        range,
    )
}
