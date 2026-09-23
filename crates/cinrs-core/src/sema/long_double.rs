//! `long double` at the boundary with the platform.
//!
//! cinrs maps `long double` onto `double` when the type is resolved (see
//! [`crate::ir::Ty`]), which is self-consistent for everything a unit defines:
//! a `long double` object is eight bytes, a function the unit defines takes
//! and returns an `f64`, and its own `va_arg(ap, long double)` reads the
//! `double` its caller passed. It is **not** consistent with the platform's C
//! library wherever that library's `long double` is something else — the x87
//! eighty-bit type on x86-64 System V and i386, the 128-bit quad on AArch64
//! Linux and most other 64-bit ABIs (see
//! [`TargetModel::platform_long_double_is_double`]). A declared-only function
//! with `long double` in its prototype is then called with the `double`
//! convention while the callee uses its own, and the value that comes back is
//! garbage: chibicc's tokenizer called glibc's `strtold` and read every
//! floating literal wrong.
//!
//! So the boundary is handled rather than trusted, in two ways.
//!
//! * A declared-only function in `LONG_DOUBLE_TWINS` — an ISO C function
//!   whose only difference from a `double` sibling is the type — is linked to
//!   the sibling, through the `#[link_name]` its `asm_label` becomes. That is
//!   exactly what "`long double` is `double`" means for this crate.
//! * Every other declared-only function whose return type or a parameter is
//!   `long double`, `long double _Complex`, or a pointer to one of them, is
//!   refused where it is **called or its address taken** — not where it is
//!   declared, since the platform's headers declare dozens. A pointer is the
//!   same size either way, but the callee reads and writes sixteen (or
//!   twelve) bytes through it where the object has eight. And a `long double`
//!   value or pointer handed to the variable part of a declared-only function
//!   (`printf("%Lf", x)`, `sscanf("%Lf", &x)`) is refused for the same
//!   reason.
//!
//! Which expressions are `long double` has to be worked out on the side,
//! since the type itself is already `double` in the IR: the declarations
//! (objects, parameters, members, `typedef`s) are remembered by the range of
//! their name, and the two expressions that *make* one — an `L` literal and a
//! cast — by the range of what they produced. [`Sema::long_double_depth_of`]
//! reads an expression back through those.

use super::{Entry, Sema};
use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{Callee, Expr, ExprKind, FuncId, Place, PlaceKind, Ty};
use crate::target::Arch;

/// The ISO C functions whose only difference from a `double` sibling is the
/// `long double` type, each with the sibling a declaration of it links to on
/// a target whose platform `long double` is wider than `double`.
///
/// With `long double` being `double` here, the sibling *is* the function:
/// `strtold` parses to the precision the result has, `powl` computes it.
/// Pointers in these prototypes are to the unit's own objects, eight bytes, so
/// `frexpl`'s `int *`, `modfl`'s `long double *` and `remquol`'s `int *` are
/// the sibling's too.
///
/// `nexttoward`, `nexttowardf` and `nexttowardl` take their *direction* as a
/// `long double`; with that being a `double` value, stepping toward it is
/// exactly what `nextafter` (or `nextafterf`) does, so they are here as well.
///
/// Anything else — the GNU `l` extensions (`sincosl`, `exp10l`), a user's own
/// function — is refused at its use; see the module documentation.
pub(super) const LONG_DOUBLE_TWINS: &[(&str, &str)] = &[
    // <stdlib.h> and <wchar.h>
    ("strtold", "strtod"),
    ("wcstold", "wcstod"),
    // <math.h>
    ("acosl", "acos"),
    ("asinl", "asin"),
    ("atanl", "atan"),
    ("atan2l", "atan2"),
    ("cosl", "cos"),
    ("sinl", "sin"),
    ("tanl", "tan"),
    ("acoshl", "acosh"),
    ("asinhl", "asinh"),
    ("atanhl", "atanh"),
    ("coshl", "cosh"),
    ("sinhl", "sinh"),
    ("tanhl", "tanh"),
    ("expl", "exp"),
    ("exp2l", "exp2"),
    ("expm1l", "expm1"),
    ("frexpl", "frexp"),
    ("ilogbl", "ilogb"),
    ("ldexpl", "ldexp"),
    ("logl", "log"),
    ("log10l", "log10"),
    ("log1pl", "log1p"),
    ("log2l", "log2"),
    ("logbl", "logb"),
    ("modfl", "modf"),
    ("scalbnl", "scalbn"),
    ("scalblnl", "scalbln"),
    ("cbrtl", "cbrt"),
    ("fabsl", "fabs"),
    ("hypotl", "hypot"),
    ("powl", "pow"),
    ("sqrtl", "sqrt"),
    ("erfl", "erf"),
    ("erfcl", "erfc"),
    ("lgammal", "lgamma"),
    ("tgammal", "tgamma"),
    ("ceill", "ceil"),
    ("floorl", "floor"),
    ("nearbyintl", "nearbyint"),
    ("rintl", "rint"),
    ("lrintl", "lrint"),
    ("llrintl", "llrint"),
    ("roundl", "round"),
    ("lroundl", "lround"),
    ("llroundl", "llround"),
    ("truncl", "trunc"),
    ("fmodl", "fmod"),
    ("remainderl", "remainder"),
    ("remquol", "remquo"),
    ("copysignl", "copysign"),
    ("nanl", "nan"),
    ("nextafterl", "nextafter"),
    ("nexttoward", "nextafter"),
    ("nexttowardf", "nextafterf"),
    ("nexttowardl", "nextafter"),
    ("fdiml", "fdim"),
    ("fmaxl", "fmax"),
    ("fminl", "fmin"),
    ("fmal", "fma"),
    // <complex.h>; `long double _Complex` is `double _Complex` here.
    ("cabsl", "cabs"),
    ("cacosl", "cacos"),
    ("cacoshl", "cacosh"),
    ("cargl", "carg"),
    ("casinl", "casin"),
    ("casinhl", "casinh"),
    ("catanl", "catan"),
    ("catanhl", "catanh"),
    ("ccosl", "ccos"),
    ("ccoshl", "ccosh"),
    ("cexpl", "cexp"),
    ("cimagl", "cimag"),
    ("clogl", "clog"),
    ("conjl", "conj"),
    ("cpowl", "cpow"),
    ("cprojl", "cproj"),
    ("creall", "creal"),
    ("csinl", "csin"),
    ("csinhl", "csinh"),
    ("csqrtl", "csqrt"),
    ("ctanl", "ctan"),
    ("ctanhl", "ctanh"),
];

/// The `double` sibling of the ISO C function `name`, if it is one of
/// `LONG_DOUBLE_TWINS`.
///
/// glibc's `_Float64x` functions are its `long double` ones under TS
/// 18661-3's names — `strtof64x` is `strtold`, `sinf64x` is `sinl` — and are
/// twinned the same way.
pub fn long_double_twin(name: &str) -> Option<&'static str> {
    let lookup = |name: &str| {
        LONG_DOUBLE_TWINS
            .iter()
            .find(|(ld, _)| *ld == name)
            .map(|(_, twin)| *twin)
    };
    if let Some(twin) = lookup(name) {
        return Some(twin);
    }
    match name {
        "strtof64x" => Some("strtod"),
        "wcstof64x" => Some("wcstod"),
        _ => lookup(&format!("{}l", name.strip_suffix("f64x")?)),
    }
}

/// What a function's prototype says about `long double`, over every
/// declaration of it.
#[derive(Clone, Copy, Default, Debug)]
pub(super) struct LongDoubleSig {
    /// The return type's [depth](Sema::long_double_depth).
    pub ret: Option<u8>,
    /// The first thing in the prototype that crosses the boundary wrongly, as
    /// the diagnostic words it: "takes a 'long double'" and the like.
    pub offence: Option<&'static str>,
}

/// A use of the boundary that is checked once the unit is done, when it is
/// known which functions it defines.
#[derive(Clone, Debug)]
pub(super) enum LongDoubleUse {
    /// A call to, or the address of, a function whose prototype mentions
    /// `long double`.
    Function(FuncId, SourceRange),
    /// A `long double` (`pointer == false`) or a `long double *` passed to the
    /// variable part of a call.
    Variadic {
        func: FuncId,
        range: SourceRange,
        pointer: bool,
    },
}

impl Sema<'_> {
    /// How far below the type `ty` a `long double` is: `Some(0)` for
    /// `long double` (or `long double _Complex`) itself, `Some(1)` for a
    /// pointer to one or an array of them, and so on; `None` when there is
    /// none. A `typedef` answers what its declaration did.
    pub(super) fn long_double_depth(&self, ty: &ast::Type) -> Option<u8> {
        match &ty.kind {
            ast::TypeKind::Float(ast::FloatSize::LongDouble | ast::FloatSize::Float64x)
            | ast::TypeKind::Complex(ast::FloatSize::LongDouble | ast::FloatSize::Float64x) => {
                Some(0)
            }
            ast::TypeKind::Pointer(inner) => self.long_double_depth(inner)?.checked_add(1),
            ast::TypeKind::Array { elem, .. } => self.long_double_depth(elem)?.checked_add(1),
            ast::TypeKind::Typedef(name) => match self.lookup(&name.name) {
                Some(Entry::Typedef(entry)) => self.long_double_decls.get(&entry.range).copied(),
                _ => None,
            },
            _ => None,
        }
    }

    /// Remembers that the declaration whose name is at `range` has type `ty`,
    /// if that type involves `long double`.
    pub(super) fn note_long_double(&mut self, range: SourceRange, ty: &ast::Type) {
        if let Some(depth) = self.long_double_depth(ty) {
            self.long_double_decls.insert(range, depth);
        }
    }

    /// Remembers what the prototype `func` of the function `id` says about
    /// `long double`.
    pub(super) fn note_long_double_function(&mut self, id: FuncId, func: &ast::FunctionType) {
        let ret = self.long_double_depth(&func.ret);
        let offence = match ret {
            Some(0) => Some("returns a 'long double'"),
            Some(1) => Some("returns a 'long double *'"),
            _ => None,
        }
        .or_else(|| {
            func.params
                .iter()
                .find_map(|param| match self.long_double_depth(&param.ty) {
                    Some(0) => Some("takes a 'long double'"),
                    Some(1) => Some("takes a 'long double *'"),
                    _ => None,
                })
        });
        if ret.is_none() && offence.is_none() {
            return;
        }
        let entry = self.long_double_funcs.entry(id).or_default();
        entry.ret = entry.ret.or(ret);
        entry.offence = entry.offence.or(offence);
    }

    /// Records that the expression `result`, made by a cast to `ty` or by a
    /// literal, is (or, for a cast to anything else, is not) a `long double`.
    pub(super) fn note_long_double_expr(&mut self, result: &Expr, depth: Option<u8>) {
        if depth.is_some() || self.long_double_depth_of(result).is_some() {
            self.long_double_exprs.insert(result.range, depth);
        }
    }

    /// How far below the type of `expr` a `long double` is, in the sense of
    /// [`Sema::long_double_depth`]: `Some(0)` for a `long double` value,
    /// `Some(1)` for a pointer to one.
    pub(super) fn long_double_depth_of(&self, expr: &Expr) -> Option<u8> {
        let depth = match self.long_double_exprs.get(&expr.range) {
            Some(depth) => *depth,
            None => self.long_double_depth_walk(expr),
        }?;
        // A value that is not a floating one or a pointer is not what it
        // came from: `(int) x`, `x < y`.
        let plausible = match depth {
            0 => matches!(expr.ty, Ty::Double | Ty::ComplexDouble),
            _ => expr.ty.is_pointer() || matches!(expr.ty, Ty::Array(_)),
        };
        plausible.then_some(depth)
    }

    fn long_double_depth_walk(&self, expr: &Expr) -> Option<u8> {
        match &expr.kind {
            ExprKind::Load(place)
            | ExprKind::Assign { place, .. }
            | ExprKind::CompoundAssign { place, .. }
            | ExprKind::IncDec { place, .. } => self.place_long_double_depth(place),
            ExprKind::AddrOf(place) => {
                let depth = self.place_long_double_depth(place)?;
                // An array decays to a pointer to its first element, which
                // the array's own depth already counts.
                if matches!(place.ty, Ty::Array(_)) {
                    Some(depth)
                } else {
                    depth.checked_add(1)
                }
            }
            ExprKind::Neg(inner) => self.long_double_depth_of(inner),
            // `long double` wins the usual arithmetic conversions.
            ExprKind::Binary { lhs, rhs, .. } => {
                let zero = |e: &Expr| self.long_double_depth_of(e) == Some(0);
                (zero(lhs) || zero(rhs)).then_some(0)
            }
            ExprKind::PtrOffset { ptr, .. } => self.long_double_depth_of(ptr),
            ExprKind::Cast(inner) if inner.ty == expr.ty => self.long_double_depth_of(inner),
            ExprKind::Cond {
                then_expr,
                else_expr,
                ..
            } => self
                .long_double_depth_of(then_expr)
                .or_else(|| self.long_double_depth_of(else_expr)),
            ExprKind::CondDefault { value, else_expr } => self
                .long_double_depth_of(value)
                .or_else(|| self.long_double_depth_of(else_expr)),
            ExprKind::Comma { rhs, .. } => self.long_double_depth_of(rhs),
            ExprKind::StmtExpr {
                value: Some(value), ..
            } => self.long_double_depth_of(value),
            ExprKind::Call {
                callee: Callee::Direct(id),
                ..
            } => self.long_double_funcs.get(id).and_then(|sig| sig.ret),
            _ => None,
        }
    }

    fn place_long_double_depth(&self, place: &Place) -> Option<u8> {
        match &place.kind {
            PlaceKind::Object(id) => self
                .long_double_decls
                .get(&self.program.object(*id).range)
                .copied(),
            PlaceKind::Deref(ptr) | PlaceKind::Index { base: ptr, .. } => {
                self.long_double_depth_of(ptr)?.checked_sub(1)
            }
            PlaceKind::Field { record, index, .. } => {
                let field = self.types().record(*record).fields.get(*index)?;
                self.long_double_decls.get(&field.range).copied()
            }
            PlaceKind::ComplexPart { base, .. } => self.place_long_double_depth(base),
            PlaceKind::Temporary(value) => self.long_double_depth_of(value),
            PlaceKind::Str(_) | PlaceKind::CompoundLiteral { .. } => None,
        }
    }

    /// Records what a call does at the boundary: the callee itself, when its
    /// prototype mentions `long double`, and each argument in the variable
    /// part that is a `long double` or a pointer to one.
    pub(super) fn note_long_double_call(
        &mut self,
        target: &Callee,
        callee_range: SourceRange,
        variadic_args: &[(SourceRange, Option<u8>)],
    ) {
        let Callee::Direct(id) = target else {
            return;
        };
        // An arm a constant condition excludes never calls anything.
        if self.dead_code > 0 {
            return;
        }
        self.note_long_double_function_use(*id, callee_range);
        for (range, depth) in variadic_args {
            if let Some(depth @ (0 | 1)) = depth {
                self.long_double_uses.push(LongDoubleUse::Variadic {
                    func: *id,
                    range: *range,
                    pointer: *depth == 1,
                });
            }
        }
    }

    /// Records a call to, or the address of, the function `id`.
    pub(super) fn note_long_double_function_use(&mut self, id: FuncId, range: SourceRange) {
        if self.dead_code == 0
            && self
                .long_double_funcs
                .get(&id)
                .is_some_and(|sig| sig.offence.is_some())
        {
            self.long_double_uses
                .push(LongDoubleUse::Function(id, range));
        }
    }

    /// Whether the function `id` is the platform's: declared here and defined
    /// nowhere in the unit.
    fn is_platform_function(&self, id: FuncId) -> bool {
        let func = self.program.function(id);
        func.body.is_none()
            && func.intrinsic.is_none()
            && !self.defined_functions.contains(&func.name)
    }

    /// Redirects the [twins](LONG_DOUBLE_TWINS) and refuses the rest, once the
    /// unit is done and it is known which functions it defines.
    pub(super) fn check_long_double_boundary(&mut self) {
        let uses = std::mem::take(&mut self.long_double_uses);
        if self.target.platform_long_double_is_double() {
            return;
        }
        let mut redirected = std::collections::HashSet::new();
        for index in 0..self.program.functions.len() {
            let id = FuncId(index as u32);
            if !self.is_platform_function(id) {
                continue;
            }
            let func = &mut self.program.functions[index];
            // An `__asm__("symbol")` label is the program's own answer and
            // names a symbol that is not the ISO function.
            if func.asm_label.is_some() {
                continue;
            }
            if let Some(twin) = long_double_twin(&func.name) {
                func.asm_label = Some(twin.to_owned());
                redirected.insert(id);
            }
        }
        let (how, bytes) = match self.target.arch {
            Arch::X86_64 => ("on the x87 stack", "sixteen x87 bytes"),
            Arch::X86 => ("on the x87 stack", "twelve x87 bytes"),
            _ => ("as a 128-bit quad", "sixteen bytes of a 128-bit quad"),
        };
        for used in uses {
            match used {
                LongDoubleUse::Function(id, range) => {
                    if redirected.contains(&id) || !self.is_platform_function(id) {
                        continue;
                    }
                    let name = self.program.function(id).name.clone();
                    let offence = self.long_double_funcs[&id].offence.unwrap_or_default();
                    let message = if offence.ends_with("*'") {
                        format!(
                            "'{name}' {offence} ('long double' is 'double' here, eight bytes), \
                             and the platform's function reads or writes {bytes} through it; use \
                             the 'double' function or wrap it in C compiled by a C compiler"
                        )
                    } else {
                        format!(
                            "'{name}' {offence} (which is 'double' here), and the platform passes \
                             a 'long double' {how}, so the call would read the wrong register; \
                             use the 'double' function or wrap it in C compiled by a C compiler"
                        )
                    };
                    self.error(range, message);
                }
                LongDoubleUse::Variadic {
                    func,
                    range,
                    pointer,
                } => {
                    if !self.is_platform_function(func) {
                        continue;
                    }
                    let name = self.program.function(func).name.clone();
                    let message = if pointer {
                        format!(
                            "a 'long double *' cannot be passed to the platform's '{name}': \
                             'long double' is 'double' here, eight bytes, and '{name}' would read \
                             or write {bytes} through it; pass a 'double *' and use '%lf'"
                        )
                    } else {
                        format!(
                            "a 'long double' cannot be passed to the platform's '{name}': it is \
                             'double' here and would be read as {bytes}; cast it to 'double' and \
                             use '%f'"
                        )
                    };
                    self.error(range, message);
                }
            }
        }
    }
}
