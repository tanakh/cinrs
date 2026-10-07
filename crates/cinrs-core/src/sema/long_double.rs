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
//!   exactly what "`long double` is `double`" means for this crate, and it is
//!   done on **every** target, including those whose platform `long double`
//!   is `double` too. Microsoft's is, and that is why its C runtime has no
//!   `powl`, `sinl`, `fabsl`, `hypotl` or the other C89 `l` forms to link to:
//!   the UCRT's `<corecrt_math.h>` defines those twenty-three as `__inline`
//!   wrappers that call `pow`, `sin`, `fabs`, which is what the redirect
//!   writes out, and a declaration linked by its own name was `lld-link:
//!   error: undefined symbol: powl` on every MSVC architecture. Every `double`
//!   sibling in the table is a real export of `ucrt.lib` (10.0.26100.0, read
//!   with `nm`); the C99 `l` forms the UCRT does export (`strtold`, `cbrtl`,
//!   `csinl`, `nexttowardl` …) are redirected all the same, which on such a
//!   target is the same function. Where the `l` forms all exist — glibc on
//!   32-bit Arm, Apple's arm64 — they are the siblings under another name.
//! * Where the platform's `long double` is wider than `double`, every other
//!   declared-only function whose return type or a parameter is
//!   `long double`, `long double _Complex`, or a pointer to one of them, is
//!   refused where it is **called or its address taken** — not where it is
//!   declared, since the platform's headers declare dozens. A pointer is the
//!   same size either way, but the callee reads and writes sixteen (or
//!   twelve) bytes through it where the object has eight. And a `long double`
//!   value or pointer handed to the variable part of a declared-only function
//!   (`printf("%Lf", x)`, `sscanf("%Lf", &x)`) is refused for the same
//!   reason.
//!
//! "Declared-only" is the platform's in a `c99!` block, which may call a C
//! library a C compiler built through that library's own header. Under
//! [`crate::Options::own_declarations_are_cinrs`] — what `ccinrs` sets, since
//! what it links is what it compiled — only a function declared in the
//! platform's headers or cinrs's bundled ones (or given the C library's
//! prototype on the program's behalf) is the platform's: one the program
//! declares in its own file or header is another file of the program, whose
//! `long double` is the `double` this one passes, `printf`-style variadic
//! functions of its own included. Redis's `ld2string` and `string2ld` are
//! those. The twins are redirected wherever they are declared.
//!
//! Which expressions are `long double` has to be worked out on the side,
//! since the type itself is already `double` in the IR: the declarations
//! (objects, parameters, members, `typedef`s) are remembered by the range of
//! their name, and the two expressions that *make* one — an `L` literal and a
//! cast — by the range of what they produced. [`Sema::long_double_depth_of`]
//! reads an expression back through those.

use super::Sema;
use crate::ast;
use crate::capture::SourceRange;
use crate::ir::{Callee, Expr, ExprKind, FuncId, Place, PlaceKind, StrId, Ty};
use crate::target::Arch;

/// The ISO C functions whose only difference from a `double` sibling is the
/// `long double` type, each with the sibling a declaration of it links to on
/// every target (Microsoft's C runtime exports no `powl` at all; see the module
/// documentation).
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
    /// A call to a `printf` or a `scanf` whose `long double` arguments are
    /// each named by an `L` conversion of its literal format: the `L`s, at
    /// these indexes of the string, become `l` if the function is the
    /// platform's. See [`format_conversions`].
    Format {
        func: FuncId,
        string: StrId,
        at: Vec<usize>,
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
            ast::TypeKind::Typedef(name) => self
                .lookup_typedef(name)
                .and_then(|entry| self.long_double_decls.get(&entry.range).copied()),
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
    /// part that is a `long double` or a pointer to one — unless the call is
    /// a `printf` or a `scanf` whose literal format names every one of them
    /// with an `L`, which is then recorded as a format to rewrite instead.
    /// `args` are the call's arguments, the variable part last.
    pub(super) fn note_long_double_call(
        &mut self,
        target: &Callee,
        callee_range: SourceRange,
        args: &[Expr],
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
        let format = self.format_rewrite(*id, args, variadic_args);
        let covered: &[usize] = format.as_ref().map_or(&[], |(_, _, covered)| covered);
        for (index, (range, depth)) in variadic_args.iter().enumerate() {
            if covered.contains(&index) {
                continue;
            }
            if let Some(depth @ (0 | 1)) = depth {
                self.long_double_uses.push(LongDoubleUse::Variadic {
                    func: *id,
                    range: *range,
                    pointer: *depth == 1,
                });
            }
        }
        if let Some((string, at, _)) = format {
            self.long_double_uses.push(LongDoubleUse::Format {
                func: *id,
                string,
                at,
            });
        }
    }

    /// For a call to one of the [`FORMAT_FUNCTIONS`] with a literal format,
    /// the string, the indexes of the `L`s that name its `long double`
    /// arguments (`long double *` for a `scanf`), and which arguments of the
    /// variable part those are. `None` when the call is anything else, or a
    /// `long double` argument is not named that way — a format built at run
    /// time, a `%Lf` given a `double`, an argument too many — which leaves
    /// the call to be refused.
    ///
    /// The rewrite is `L` to `l`: `%lf` is `%f` to `printf` (C99 7.19.6.1p7,
    /// "has no effect") and a `double *` to `scanf`, which with `long double`
    /// being `double` is exactly what the argument is. The string keeps its
    /// length, and each literal is its own `StrData`, so no other use of the
    /// text sees it change.
    fn format_rewrite(
        &self,
        id: FuncId,
        args: &[Expr],
        variadic_args: &[(SourceRange, Option<u8>)],
    ) -> Option<(StrId, Vec<usize>, Vec<usize>)> {
        let name = &self.program.function(id).name;
        let &(_, index, scanf) = FORMAT_FUNCTIONS.iter().find(|(n, _, _)| n == name)?;
        // The format is the last named parameter, the variable part right
        // after it; anything else is not the function the name says.
        if args.len() != index + 1 + variadic_args.len() {
            return None;
        }
        let string = literal_string(&args[index])?;
        let data = &self.program.strings[string.0 as usize];
        if data.elem != Ty::Char {
            return None;
        }
        let consumed = format_conversions(&data.values, scanf)?;
        let wanted = u8::from(scanf);
        let mut at = Vec::new();
        let mut covered = Vec::new();
        for (arg, (_, depth)) in variadic_args.iter().enumerate() {
            if *depth != Some(wanted) {
                continue;
            }
            let Some(Some(l)) = consumed.get(arg) else {
                return None;
            };
            at.push(*l);
            covered.push(arg);
        }
        (!covered.is_empty()).then_some((string, at, covered))
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

    /// Whether the function `id` is declared here and defined nowhere in the
    /// unit.
    fn is_declared_only(&self, id: FuncId) -> bool {
        let func = self.program.function(id);
        func.body.is_none()
            && func.intrinsic.is_none()
            && !self.defined_functions.contains(&func.name)
    }

    /// Whether the function `id` is the platform's: declared only, and —
    /// where [`crate::Options::own_declarations_are_cinrs`] says the
    /// program's own declarations are cinrs's — declared in the platform's
    /// headers or cinrs's bundled ones, or given the C library's prototype on
    /// the program's behalf. A function the program declares in its own file
    /// or header is then another file of the program, compiled by cinrs, whose
    /// `long double` is the `double` this one passes.
    fn is_platform_function(&self, id: FuncId) -> bool {
        self.is_declared_only(id)
            && (!self.own_declarations_are_cinrs
                || self.library_declared.contains(&id)
                || self.library_prototyped.contains(&id))
    }

    /// Redirects the [twins](LONG_DOUBLE_TWINS) and refuses the rest, once the
    /// unit is done and it is known which functions it defines.
    ///
    /// The redirect is made on every target, the refusals only where the
    /// platform's `long double` is wider than `double`; see the module
    /// documentation for why the two differ.
    pub(super) fn check_long_double_boundary(&mut self) {
        let uses = std::mem::take(&mut self.long_double_uses);
        let mut redirected = std::collections::HashSet::new();
        for index in 0..self.program.functions.len() {
            let id = FuncId(index as u32);
            // Wherever it is declared: a program that declares `powl` itself
            // means the C library's, and that is `pow` here.
            if !self.is_declared_only(id) {
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
        if self.target.platform_long_double_is_double() {
            return;
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
                // A function of the unit's own reads its `long double`
                // arguments as `double`, as they were passed, whatever the
                // format says; only the platform's is told otherwise.
                LongDoubleUse::Format { func, string, at } => {
                    if !self.is_platform_function(func) {
                        continue;
                    }
                    let values = &mut self.program.strings[string.0 as usize].values;
                    for index in at {
                        values[index] = u32::from(b'l');
                    }
                }
            }
        }
    }
}

/// The `printf`s and `scanf`s whose literal format a `long double` argument
/// is rewritten in: the name, the index of the format parameter, and whether
/// it is a `scanf`. See [`Sema::format_rewrite`].
const FORMAT_FUNCTIONS: &[(&str, usize, bool)] = &[
    ("printf", 0, false),
    ("fprintf", 1, false),
    ("sprintf", 1, false),
    ("snprintf", 2, false),
    ("dprintf", 1, false),
    ("asprintf", 1, false),
    ("scanf", 0, true),
    ("fscanf", 1, true),
    ("sscanf", 1, true),
];

/// The string literal an argument is, through the decay and the conversion
/// to `const char *` it went through.
fn literal_string(expr: &Expr) -> Option<StrId> {
    match &expr.kind {
        ExprKind::Cast(inner) => literal_string(inner),
        ExprKind::AddrOf(place) => match place.kind {
            PlaceKind::Str(id) => Some(id),
            _ => None,
        },
        _ => None,
    }
}

/// What a `printf` format — or with `scanf`, a `scanf` format — takes from
/// the variable arguments, one entry per argument in order: the index of the
/// `L` of a floating conversion written with one (`%Lf`, `%.3Lg`), and
/// `None` for every other argument, a `*` width or precision among them.
///
/// `None` altogether for a format this does not follow: one that numbers its
/// arguments (`%2$Lf`), or that ends in the middle of a conversion.
fn format_conversions(format: &[u32], scanf: bool) -> Option<Vec<Option<usize>>> {
    // What the function reads stops at the first NUL.
    let format = &format[..format.iter().position(|&c| c == 0).unwrap_or(format.len())];
    let at = |i: usize| {
        format
            .get(i)
            .and_then(|&c| char::from_u32(c))
            .unwrap_or('\0')
    };
    let digits = |mut i: usize| {
        while at(i).is_ascii_digit() {
            i += 1;
        }
        i
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < format.len() {
        if at(i) != '%' {
            i += 1;
            continue;
        }
        i += 1;
        if at(i) == '%' {
            i += 1;
            continue;
        }
        if digits(i) > i && at(digits(i)) == '$' {
            return None;
        }
        let mut suppressed = false;
        if scanf {
            if at(i) == '*' {
                suppressed = true;
                i += 1;
            }
            i = digits(i);
        } else {
            while "-+ #0'I".contains(at(i)) {
                i += 1;
            }
            if at(i) == '*' {
                out.push(None);
                i += 1;
            } else {
                i = digits(i);
            }
            if at(i) == '.' {
                i += 1;
                if at(i) == '*' {
                    out.push(None);
                    i += 1;
                } else {
                    i = digits(i);
                }
            }
        }
        let mut l = None;
        loop {
            match at(i) {
                'L' => l = Some(i),
                'h' | 'l' | 'j' | 'z' | 't' | 'q' => {}
                // `scanf`'s `%ms`: allocate the string.
                'm' if scanf => {}
                _ => break,
            }
            i += 1;
        }
        let conversion = at(i);
        if conversion == '\0' {
            return None;
        }
        i += 1;
        if scanf && conversion == '[' {
            if at(i) == '^' {
                i += 1;
            }
            // A `]` right after the bracket is one of the set.
            if at(i) == ']' {
                i += 1;
            }
            while at(i) != ']' {
                if at(i) == '\0' {
                    return None;
                }
                i += 1;
            }
            i += 1;
        }
        // `%m` is glibc's `strerror(errno)`, and takes nothing.
        if suppressed || (!scanf && conversion == 'm') {
            continue;
        }
        out.push(l.filter(|_| "aAeEfFgG".contains(conversion)));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::format_conversions;

    fn conversions(format: &str, scanf: bool) -> Option<Vec<Option<usize>>> {
        let values: Vec<u32> = format.bytes().map(u32::from).collect();
        format_conversions(&values, scanf)
    }

    #[test]
    fn what_a_printf_format_takes() {
        assert_eq!(
            conversions("%.2Lf%c %d%%", false),
            Some(vec![Some(3), None, None])
        );
        assert_eq!(
            conversions("%*.*Lg|%-08Le|%Ld", false),
            Some(vec![None, None, Some(4), Some(11), None])
        );
        assert_eq!(conversions("%m %s %lf", false), Some(vec![None, None]));
        assert_eq!(conversions("%2$Lf %1$d", false), None);
        assert_eq!(conversions("100%", false), None);
    }

    #[test]
    fn what_a_scanf_format_takes() {
        assert_eq!(
            conversions("%Lf %*d %[^]x] %5Le %ms", true),
            Some(vec![Some(1), None, Some(17), None])
        );
        assert_eq!(conversions("%[abc", true), None);
    }
}
