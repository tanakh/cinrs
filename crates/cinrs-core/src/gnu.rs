//! The GNU extensions' name tables.
//!
//! Two questions are asked of this module, and it matters that both are
//! answered from the same place: the [parser](crate::parse) and
//! [sema](crate::sema) ask "what does this attribute or builtin mean?", and the
//! [preprocessor](crate::pp) answers `__has_attribute`, `__has_builtin`,
//! `__has_feature` and `__has_extension` from it — so a program that guards a
//! construct with `#if __has_attribute(packed)` gets an answer that is true of
//! this implementation rather than of GCC's.
//!
//! The one set of names that is *not* here is the atomic builtins', which
//! live beside the code that implements them in
//! [`crate::sema::is_atomic_builtin`] — for the same reason: one table, asked
//! by both.
//!
//! `doc/gnu-extensions.md` is the prose version of the same tables.

/// What an `__attribute__` (or a C23 `[[…]]`) asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Attribute {
    /// `noreturn`, `_Noreturn`: a call to the function does not come back.
    Noreturn,
    /// `always_inline`: `#[inline(always)]`.
    AlwaysInline,
    /// `noinline`: `#[inline(never)]`.
    NoInline,
    /// `cold`, and `hot` as its opposite: `#[cold]`.
    Cold,
    /// `hot`, which cancels `cold`.
    Hot,
    /// `deprecated`: `#[deprecated]`.
    Deprecated,
    /// `fallthrough`, which is a statement attribute and means nothing here —
    /// a `switch` group falls through in the generated Rust either way.
    Fallthrough,
    /// `packed`: no padding, on a record or on one member.
    Packed,
    /// `aligned(N)`: raise the alignment.
    Aligned,
    /// `section("…")`: `#[unsafe(link_section = "…")]`.
    Section,
    /// `constructor`, optionally with a priority: run before `main`.
    Constructor,
    /// `destructor`, likewise: run after it.
    Destructor,
    /// `cleanup(f)`: call `f(&x)` when `x` goes out of scope.
    Cleanup,
    /// `mode(M)`: the declared type is the one the machine mode names.
    Mode,
    /// `cinrs_safe`, and `[[cinrs::safe]]`: this crate's own attribute, which
    /// generates the function without `unsafe` so that `rustc` checks it.
    Safe,
    /// `weak`: the symbol may be missing at link time, and its address is then
    /// null. Refused on a *definition*, where Rust's unstable `#[linkage]`
    /// would be the only way to say it, and ignored on a declaration; see
    /// [`crate::sema`].
    Weak,
    /// `asm("symbol")` written as an attribute is not a thing, but
    /// `alias`, `weakref` and the rest are: known, and refused with the reason.
    Unsupported,
    /// Known and safe to ignore: a hint, a diagnostic request, or something
    /// the generated Rust cannot observe.
    Ignored,
}

/// The attributes that are refused rather than ignored, with the reason.
///
/// Silently ignoring one of these would change what the program *means*, which
/// is the one thing this crate will not do.
pub const UNSUPPORTED_ATTRIBUTES: &[(&str, &str)] = &[
    (
        "weakref",
        "is not supported: Rust's `#[linkage]` is unstable, so weak linkage cannot be asked for",
    ),
    (
        "alias",
        "is not supported: write a function that forwards to the other one instead",
    ),
    (
        "ifunc",
        "is not supported: choosing an implementation at load time has no stable Rust counterpart",
    ),
    (
        "vector_size",
        "is not supported: the vector extensions need `core::simd`, which is unstable",
    ),
    (
        "scalar_storage_order",
        "is not supported: it reverses the byte order of every scalar in the record, and \
         nothing in the generated Rust could carry that",
    ),
];

/// The attribute a name spells, accepting both the `name` and the `__name__`
/// forms GCC does.
pub fn attribute(name: &str) -> Option<Attribute> {
    let bare = name
        .strip_prefix("__")
        .and_then(|rest| rest.strip_suffix("__"))
        .unwrap_or(name);
    if UNSUPPORTED_ATTRIBUTES.iter().any(|(n, _)| *n == bare) {
        return Some(Attribute::Unsupported);
    }
    Some(match bare {
        "noreturn" => Attribute::Noreturn,
        "always_inline" => Attribute::AlwaysInline,
        "noinline" => Attribute::NoInline,
        "cold" => Attribute::Cold,
        "hot" => Attribute::Hot,
        "deprecated" => Attribute::Deprecated,
        "fallthrough" => Attribute::Fallthrough,
        "packed" => Attribute::Packed,
        "aligned" | "alignas" => Attribute::Aligned,
        "section" => Attribute::Section,
        "constructor" => Attribute::Constructor,
        "destructor" => Attribute::Destructor,
        "cleanup" => Attribute::Cleanup,
        "mode" => Attribute::Mode,
        "weak" => Attribute::Weak,
        // Not GCC's: this crate's own, spelled the way a GNU attribute of
        // another vendor's is, so that it works in every entry point.
        "cinrs_safe" => Attribute::Safe,
        _ if IGNORED_ATTRIBUTES.contains(&bare) => Attribute::Ignored,
        _ => return None,
    })
}

/// The names `[[cinrs::…]]` knows, for the diagnostic that lists them.
///
/// C23 6.7.13.1p3 lets an implementation ignore an attribute in a namespace it
/// does not know, and [`attribute`] does exactly that for `[[clang::…]]` and
/// the rest — but `cinrs` is *our* namespace, so a name we do not know there is
/// a mistake worth reporting, exactly as an unknown `#pragma cinrs` option is.
pub const CINRS_ATTRIBUTES: &[&str] = &["safe"];

/// What a `[[cinrs::name]]` attribute asks for.
///
/// The same attributes are spelled `__attribute__((cinrs_name))` for the entry
/// points where `[[…]]` is C23 and later only; see [`attribute`].
pub fn cinrs_attribute(name: &str) -> Option<Attribute> {
    match name {
        "safe" => Some(Attribute::Safe),
        _ => None,
    }
}

/// The reason an [`Attribute::Unsupported`] one is refused.
pub fn unsupported_reason(name: &str) -> Option<&'static str> {
    let bare = name
        .strip_prefix("__")
        .and_then(|rest| rest.strip_suffix("__"))
        .unwrap_or(name);
    UNSUPPORTED_ATTRIBUTES
        .iter()
        .find(|(n, _)| *n == bare)
        .map(|(_, reason)| *reason)
}

/// Attributes that are hints, diagnostic requests or optimiser instructions:
/// accepted, and ignored exactly as C23 6.7.13.1p3 allows.
const IGNORED_ATTRIBUTES: &[&str] = &[
    "access",
    "alloc_align",
    "alloc_size",
    "artificial",
    "assume_aligned",
    "cdecl",
    "const",
    "designated_init",
    "error",
    "externally_visible",
    "fastcall",
    "flatten",
    "format",
    "format_arg",
    "gnu_inline",
    "leaf",
    "malloc",
    "may_alias",
    "maybe_unused",
    "no_instrument_function",
    "no_sanitize",
    "no_split_stack",
    "noclone",
    "nodiscard",
    "noipa",
    "nonnull",
    "nonstring",
    "nothrow",
    "optimize",
    "pure",
    "reproducible",
    "returns_nonnull",
    "returns_twice",
    "sentinel",
    "stdcall",
    "target",
    "target_clones",
    "transparent_union",
    "unavailable",
    "unsequenced",
    "unused",
    "used",
    "visibility",
    "warn_unused_result",
    "warning",
];

/// Whether `__has_attribute(name)` answers yes.
///
/// `weak` answers **no** although a declaration carrying it is accepted: what
/// the question is really asked for is whether a weak *reference* can be
/// tested for null, and here it cannot — the symbol has to be there at link
/// time. A program that guards on the answer therefore takes the portable
/// branch, and one that writes the attribute unguarded on a declaration — as
/// glibc's `<pthread.h>` does — is not stopped by it.
pub fn has_attribute(name: &str) -> bool {
    attribute(name).is_some_and(|a| !matches!(a, Attribute::Unsupported | Attribute::Weak))
}

/// The value `__has_c_attribute(name)` answers with.
///
/// C23 6.10.1p6 wants the revision that added the attribute; every one this
/// crate honours arrived with C23 itself, so `202311L` is the honest answer,
/// and everything else is 0.
pub fn has_c_attribute(name: &str) -> u64 {
    let standard = matches!(
        name,
        "deprecated"
            | "fallthrough"
            | "maybe_unused"
            | "nodiscard"
            | "noreturn"
            | "unsequenced"
            | "reproducible"
    );
    if standard { 202_311 } else { 0 }
}

/// Whether `__has_builtin(name)` answers yes.
///
/// The atomic families are builtins too, and are not spelled `__builtin_…`:
/// `__has_builtin(__atomic_load_n)` and `__has_builtin(__sync_synchronize)`
/// are what a program guards those with, and GCC and Clang answer both.
pub fn has_builtin(name: &str) -> bool {
    if crate::sema::is_atomic_builtin(name) {
        return true;
    }
    let Some(rest) = name.strip_prefix("__builtin_") else {
        return false;
    };
    SPECIAL_BUILTINS.contains(&rest)
        || LIBRARY_BUILTINS.contains(&rest)
        || long_double_math(rest).is_some()
}

/// Whether `__has_feature(name)` / `__has_extension(name)` answers yes.
///
/// Clang's vocabulary, answered for what this crate really has:
/// `c_thread_local`, `blocks` and `nested_functions` are deliberately absent,
/// because the constructs behind them are diagnosed rather than translated —
/// a nested function definition is refused where it stands, in
/// [the parser](crate::parse), so a program that guards one with
/// `#if __has_extension(nested_functions)` takes the other branch and never
/// reaches the diagnostic. `c_atomic` is *not* absent any more:
/// `_Atomic` and `<stdatomic.h>` are here.
pub fn has_feature(name: &str) -> bool {
    SUPPORTED_FEATURES.contains(&name)
}

/// The features `__has_feature` and `__has_extension` answer yes to.
const SUPPORTED_FEATURES: &[&str] = &[
    "c_alignas",
    "c_alignof",
    "c_atomic",
    "c_attributes",
    "c_generic_selection",
    "c_generic_selections",
    "c_static_assert",
    "cinrs",
];

/// The builtins the front end implements itself.
///
/// Sema recognises exactly these names; `__has_builtin` answers from the same
/// list, so the two can never drift.
pub const SPECIAL_BUILTINS: &[&str] = &[
    "add_overflow",
    "add_overflow_p",
    "alloca",
    "alloca_with_align",
    "assume",
    "assume_aligned",
    "bswap16",
    "bswap32",
    "bswap64",
    "choose_expr",
    "cimag",
    "cimagf",
    "cimagl",
    "classify_type",
    "clrsb",
    "clrsbl",
    "clrsbll",
    "clz",
    "clzl",
    "clzll",
    "complex",
    "conj",
    "conjf",
    "conjl",
    "constant_p",
    "copysign",
    "copysignf",
    "copysignl",
    "cproj",
    "cprojf",
    "cprojl",
    "creal",
    "crealf",
    "creall",
    "ctz",
    "ctzl",
    "ctzll",
    "dynamic_object_size",
    "expect",
    "expect_with_probability",
    "fabs",
    "fabsf",
    "fabsl",
    "ffs",
    "ffsl",
    "ffsll",
    "fpclassify",
    "huge_val",
    "huge_valf",
    "huge_vall",
    "inf",
    "inff",
    "infl",
    "isfinite",
    "isgreater",
    "isgreaterequal",
    "isinf",
    "isinf_sign",
    "isinff",
    "isinfl",
    "isless",
    "islessequal",
    "islessgreater",
    "isnan",
    "isnanf",
    "isnanl",
    "isnormal",
    "issignaling",
    "isunordered",
    "mul_overflow",
    "mul_overflow_p",
    "nan",
    "nanf",
    "nanl",
    "nans",
    "nansf",
    "nansl",
    "object_size",
    "offsetof",
    "parity",
    "parityl",
    "parityll",
    "popcount",
    "popcountl",
    "popcountll",
    "prefetch",
    "signbit",
    "signbitf",
    "signbitl",
    "sub_overflow",
    "sub_overflow_p",
    "trap",
    "types_compatible_p",
    "unreachable",
    "va_arg",
    "va_copy",
    "va_end",
    "va_start",
    "FILE",
    "FUNCTION",
    "LINE",
];

/// The maths functions whose `long double` form is the `double` one here.
///
/// `long double` **is** `double` in `cinrs` — no portable Rust type has an x87
/// extended double's layout — so `__builtin_sqrtl` may not become a call to
/// the platform's `sqrtl`, whose argument and result would be an eighty-bit
/// value the generated Rust cannot pass. It becomes a call to `sqrt`, which is
/// the same function at the width this implementation gives the type.
pub fn long_double_math(rest: &str) -> Option<&'static str> {
    let base = rest.strip_suffix('l')?;
    LONG_DOUBLE_MATH.iter().copied().find(|name| *name == base)
}

/// The base names [`long_double_math`] recognises. Every one of them is in
/// [`LIBRARY_BUILTINS`] too, so a name that reaches the `l` rule has already
/// failed to match exactly — which is what keeps `__builtin_atoll` from being
/// read as the `long double` form of `atol`.
const LONG_DOUBLE_MATH: &[&str] = &[
    "acos",
    "asin",
    "atan",
    "atan2",
    "cbrt",
    "ceil",
    "cos",
    "cosh",
    "erf",
    "erfc",
    "exp",
    "exp2",
    "expm1",
    "fdim",
    "floor",
    "fma",
    "fmax",
    "fmin",
    "fmod",
    "frexp",
    "hypot",
    "ldexp",
    "lgamma",
    "log",
    "log10",
    "log1p",
    "log2",
    "modf",
    "nearbyint",
    "nextafter",
    "pow",
    "remainder",
    "rint",
    "round",
    "scalbn",
    "sin",
    "sinh",
    "sqrt",
    "tan",
    "tanh",
    "tgamma",
    "trunc",
];

/// The operation a typed overflow builtin performs, if the name spells one.
///
/// `__builtin_sadd_overflow` and its fifteen relatives are the generic
/// builtins with the result type written into the name; the type the value is
/// stored in says the same thing, so only the operation has to be read out.
pub fn typed_overflow(rest: &str) -> Option<&'static str> {
    let rest = rest.strip_prefix('s').or_else(|| rest.strip_prefix('u'))?;
    let (op, rest) = if let Some(rest) = rest.strip_prefix("add") {
        ("add", rest)
    } else if let Some(rest) = rest.strip_prefix("sub") {
        ("sub", rest)
    } else {
        ("mul", rest.strip_prefix("mul")?)
    };
    matches!(rest, "_overflow" | "l_overflow" | "ll_overflow").then_some(op)
}

/// The library functions `__builtin_X` may name.
///
/// GCC has a builtin for every standard library function; `cinrs` calls the
/// real one, declaring it if the unit did not include the header. The list is
/// what the bundled headers declare, which is what a call can be typed from.
pub const LIBRARY_BUILTINS: &[&str] = &[
    "_Exit",
    "abort",
    "abs",
    "acos",
    "acosf",
    "asin",
    "asinf",
    "atan",
    "atan2",
    "atan2f",
    "atanf",
    "atof",
    "atoi",
    "atol",
    "atoll",
    "bcmp",
    "bcopy",
    "bzero",
    "calloc",
    "cbrt",
    "cbrtf",
    "ceil",
    "ceilf",
    "cos",
    "cosf",
    "cosh",
    "coshf",
    "erf",
    "erfc",
    "exit",
    "exp",
    "exp2",
    "exp2f",
    "expf",
    "expm1",
    "expm1f",
    "fdim",
    "fdimf",
    "floor",
    "floorf",
    "fma",
    "fmaf",
    "fmax",
    "fmaxf",
    "fmin",
    "fminf",
    "fmod",
    "fmodf",
    "fprintf",
    "fputc",
    "fputs",
    "free",
    "frexp",
    "frexpf",
    "hypot",
    "hypotf",
    "imaxabs",
    "index",
    "isalnum",
    "isalpha",
    "isblank",
    "iscntrl",
    "isdigit",
    "isgraph",
    "islower",
    "isprint",
    "ispunct",
    "isspace",
    "isupper",
    "isxdigit",
    "labs",
    "ldexp",
    "ldexpf",
    "lgamma",
    "llabs",
    "log",
    "log10",
    "log10f",
    "log1p",
    "log1pf",
    "log2",
    "log2f",
    "logf",
    "malloc",
    "memchr",
    "memcmp",
    "memcpy",
    "memmove",
    "mempcpy",
    "memset",
    "modf",
    "modff",
    "nearbyint",
    "nearbyintf",
    "nextafter",
    "nextafterf",
    "pow",
    "powf",
    "printf",
    "putchar",
    "puts",
    "realloc",
    "remainder",
    "remainderf",
    "rindex",
    "rint",
    "rintf",
    "round",
    "roundf",
    "scalbn",
    "scalbnf",
    "sin",
    "sinf",
    "sinh",
    "sinhf",
    "snprintf",
    "sprintf",
    "sqrt",
    "sqrtf",
    "stpcpy",
    "stpncpy",
    "strcasecmp",
    "strcat",
    "strchr",
    "strcmp",
    "strcoll",
    "strcpy",
    "strcspn",
    "strdup",
    "strlen",
    "strncasecmp",
    "strncat",
    "strncmp",
    "strncpy",
    "strpbrk",
    "strrchr",
    "strspn",
    "strstr",
    "strtod",
    "strtof",
    "strtol",
    "strtoll",
    "strtoul",
    "strtoull",
    "tan",
    "tanf",
    "tanh",
    "tanhf",
    "tgamma",
    "tolower",
    "toupper",
    "trunc",
    "truncf",
];
