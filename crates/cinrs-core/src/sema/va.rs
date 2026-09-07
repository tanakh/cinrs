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
//! # `va_arg` of a `struct` or a `union`
//!
//! C99 allows any complete object type, and `VaArgSafe` covers none of the
//! aggregates. So an aggregate is not read at its own type: it is *reassembled
//! from the registers the ABI passed it in*. On x86-64 System V (AMD64 psABI
//! 3.2.3) an argument of at most sixteen bytes is split into one or two
//! **eightbytes**, each classified INTEGER or SSE from the fields that overlap
//! it, and each passed in one register of that file. So `va_arg(ap, struct S)`
//! becomes one `next_arg::<u64>()` per INTEGER eightbyte and one
//! `next_arg::<f64>()` per SSE eightbyte, gathered into a `[u64; N]` whose
//! bytes are then read as the record — see `Sema::record_eightbytes` for the
//! classification and `codegen`'s `va_arg` for what it emits.
//!
//! That is the whole of the support, and its edges are sharp:
//!
//! * **Only x86-64 System V.** Every other ABI classifies differently — the
//!   Microsoft x64 one passes an aggregate over eight bytes *by pointer*,
//!   AArch64 has homogeneous float aggregates, i686 puts everything on the
//!   stack — so any other [target](crate::target) is refused by name rather
//!   than translated with the wrong rules.
//! * **At most sixteen bytes, and naturally aligned.** Anything else is class
//!   MEMORY: the caller pushes it into the *overflow area*, which nothing in
//!   the stable `VaList` API can reach.
//! * `long double` is mapped to `double` throughout this crate, so it is
//!   classified SSE. A real `long double` member is X87 and would make the
//!   whole record MEMORY; a program that has one is already translated with
//!   the wrong width, which `doc/c-status.md` records.
//! * The eightbytes are read *one at a time*, and the ABI decides
//!   register-versus-stack for the argument *as a whole*. They agree unless a
//!   two-eightbyte record is the very argument that exhausts the register save
//!   area — five integer or seven SSE eightbytes into the list — where the
//!   caller pushes the whole record and reading its first eightbyte still
//!   finds a register. A record of at most eight bytes is one eightbyte and is
//!   therefore always exact.
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
//! two `unsigned long long` halves. A 128-bit *member* of a record is fine:
//! nothing reads it at its own type, only the two integer eightbytes it is.

use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{Eightbyte, Expr, ExprKind, Place, RecordId, RustField, Ty};
use crate::target::{Arch, Os};

use super::{Sema, VA_LIST_PLACEMENT};

/// The largest argument the classification can place in registers: two
/// eightbytes. Anything larger is class MEMORY.
///
/// `rustc_target`'s own limit is eight eightbytes, which is only ever reached
/// by a 512-bit SIMD vector — a whole-register class this crate has no way to
/// produce.
const MAX_EIGHTBYTES: u64 = 2;

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
        let record = self.check_va_arg_type(target, type_name.range)?;
        Some(Expr::new(
            ExprKind::VaArg { ap: place, record },
            target,
            range,
        ))
    }

    /// Whether a type may be read out of an argument list, and why not.
    ///
    /// The success value is what [`ExprKind::VaArg`] carries: the eightbyte
    /// classes of a `struct` or a `union`, and `None` for every other type,
    /// which is read in one `next_arg` at the type itself.
    #[expect(clippy::option_option, reason = "the outer one is the error")]
    fn check_va_arg_type(&mut self, ty: Ty, range: SourceRange) -> Option<Option<Vec<Eightbyte>>> {
        if ty.is_error() {
            return None;
        }
        if ty.is_pointer() || ty.is_enum() || ty == Ty::Double {
            return Some(None);
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
        // A complex value is a pair of components side by side — a two-field
        // `#[repr(C)]` struct — so it is read the way an aggregate is, and the
        // ABI classifies it the same way: `float _Complex` is one SSE
        // eightbyte holding both halves, `double _Complex` is two.
        if ty.is_record() || ty.is_complex() {
            return self.record_eightbytes(ty, range).map(Some);
        }
        if ty.is_arithmetic() {
            let promoted = ty.promote_argument(&self.target);
            if promoted == ty {
                return Some(None);
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
        self.error(
            range,
            format!("va_arg with type '{}' is not supported", self.tyname(ty)),
        );
        None
    }

    /// The eightbyte classes of a `struct` or `union` argument, or the reason
    /// it cannot be read back.
    fn record_eightbytes(&mut self, ty: Ty, range: SourceRange) -> Option<Vec<Eightbyte>> {
        let refuse = |sema: &mut Self, why: &str| {
            sema.error(
                range,
                format!("va_arg with '{}' is not supported: {why}", sema.tyname(ty)),
            );
            None::<Vec<Eightbyte>>
        };
        // Every other ABI classifies differently, and one of them — the
        // Microsoft x64 one — does not pass a large aggregate by value at all.
        // Naming the rule beats translating with the wrong one.
        if self.target.arch != Arch::X86_64 || self.target.os == Os::Windows {
            if ty.is_complex() {
                return refuse(
                    self,
                    "a complex value is a pair, so it is read back the way an aggregate is, \
                     which is only supported on x86-64 System V targets",
                );
            }
            self.error(
                range,
                "va_arg of a struct type is only supported on x86-64 System V targets",
            );
            return None;
        }
        if !self.types().is_complete(ty) {
            return refuse(self, "the type is incomplete");
        }
        let Some(layout) = self.types().size_align(ty, &self.target) else {
            return refuse(self, "the type has no size");
        };
        let eightbytes = layout.size.div_ceil(8);
        if eightbytes > MAX_EIGHTBYTES {
            return refuse(
                self,
                &format!(
                    "it is {} bytes, and the x86-64 System V ABI passes a struct larger \
                     than 16 bytes on the stack, where Rust's 'va_list' cannot reach it",
                    layout.size
                ),
            );
        }
        let mut classes = vec![Eightbyte::None; eightbytes as usize];
        if self.classify(ty, 0, &mut classes).is_none() {
            return refuse(
                self,
                "a member is not aligned the way its own type asks, so the x86-64 System V \
                 ABI passes the struct on the stack, where Rust's 'va_list' cannot reach it",
            );
        }
        Some(classes)
    }

    /// Merges the eightbyte classes of everything in `ty` at byte `offset`
    /// into `classes`; `None` is class MEMORY.
    ///
    /// This mirrors `classify` in `rustc_target`'s
    /// `compiler/rustc_target/src/callconv/x86_64.rs`, which is the reference
    /// for the rules the AMD64 psABI states in 3.2.3 — right down to the
    /// merge, which is `min` over an ordering in which INTEGER comes first, so
    /// that one integer field anywhere in an eightbyte makes the whole of it
    /// INTEGER. What is missing from this copy is what a C program cannot
    /// reach: SIMD vectors, and therefore the `SseUp` class, and the X87 class
    /// that a real `long double` would have (see the module documentation).
    fn classify(&self, ty: Ty, offset: u64, classes: &mut [Eightbyte]) -> Option<()> {
        let ty = self.types().unatomic(ty);
        let layout = self.types().size_align(ty, &self.target)?;
        // "If the size of an object is larger than eight eightbytes, or it
        // contains unaligned fields, it has class MEMORY" — a zero-sized
        // member cannot be unaligned, having nothing to align.
        if layout.size == 0 {
            return Some(());
        }
        if !offset.is_multiple_of(layout.align.max(1)) {
            return None;
        }
        let class = match ty {
            Ty::Float | Ty::Double => Eightbyte::Sse,
            // A complex value is a pair of components side by side, and that
            // is how the ABI sees it: `float _Complex` is one SSE eightbyte
            // holding both halves, `double _Complex` is two.
            Ty::ComplexFloat | Ty::ComplexDouble => {
                let half = ty.complex_component();
                self.classify(half, offset, classes)?;
                let stride = half.size_bytes(&self.target);
                return self.classify(half, offset + stride, classes);
            }
            Ty::Array(id) => {
                let elem = self.types().array_type(id).elem;
                let stride = self.types().size_of(elem, &self.target)?.max(1);
                let mut at = offset;
                while at < offset + layout.size {
                    self.classify(elem, at, classes)?;
                    at += stride;
                }
                return Some(());
            }
            Ty::Record(id) => return self.classify_record(id, offset, classes),
            // Everything left is an integer, an enumeration or a pointer.
            _ => Eightbyte::Int,
        };
        merge(classes, offset, layout.size, class);
        Some(())
    }

    /// [`Sema::classify`] for the members of one record.
    ///
    /// The members come from the *laid out* record rather than from the C
    /// declaration, because a bit-field is not a member of the generated item:
    /// a maximal run of them shares one storage field, which is what the ABI
    /// classifies — "bit-fields are always classified as integer" — and which
    /// is the only place an *unnamed* bit-field can be seen at all. A union
    /// needs no case of its own: every one of its members is at offset zero,
    /// and classifying them all is what "a union classifies per member" means.
    fn classify_record(&self, id: RecordId, offset: u64, classes: &mut [Eightbyte]) -> Option<()> {
        let record = self.types().record(id);
        for field in &record.rust_fields {
            match field {
                RustField::Member(index) => {
                    let member = &record.fields[*index];
                    // A flexible array member has no elements, and the ABI
                    // ignores it exactly as `sizeof` does.
                    if member.flexible {
                        continue;
                    }
                    self.classify(member.ty, offset + member.offset, classes)?;
                }
                RustField::Bits {
                    offset: at, bytes, ..
                } => merge(classes, offset + at, *bytes, Eightbyte::Int),
                // Padding and the zero-sized field that carries an alignment
                // are not part of the object's value and carry no class.
                RustField::Pad { .. } | RustField::Align { .. } => {}
            }
        }
        Some(())
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

/// Gives every eightbyte that `size` bytes at `offset` reach the class
/// `class`, merged with whatever is already there.
///
/// The merge is `rustc_target`'s: INTEGER wins over SSE, and either wins over
/// an eightbyte nothing has reached yet — which is what makes
/// `struct { int i; float f; }` one integer register rather than two halves of
/// two.
fn merge(classes: &mut [Eightbyte], offset: u64, size: u64, class: Eightbyte) {
    if size == 0 {
        return;
    }
    let first = (offset / 8) as usize;
    let last = ((offset + size - 1) / 8) as usize;
    for slot in classes.iter_mut().take(last + 1).skip(first) {
        *slot = match *slot {
            Eightbyte::None => class,
            Eightbyte::Int => Eightbyte::Int,
            Eightbyte::Sse => class,
        };
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
