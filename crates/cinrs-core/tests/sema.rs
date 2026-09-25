//! What semantic analysis accepts, and what it says about the rest.
//!
//! The `.stderr` files under `tests/ui` pin down where a caret lands; this
//! file pins down the *wording*, which is cheaper to cover exhaustively here.

use std::str::FromStr;

use cinrs_core::{Level, Options, Standard, analyze, ir, sema};
use proc_macro2::TokenStream;

/// Analyses `source` and returns every error message, in source order.
///
/// `c_variadic` and `complex` are forced on: what the *language* rules say
/// must not depend on which toolchain or which cargo feature the test was
/// built with, and both switched off have tests of their own.
fn errors(source: &str) -> Vec<String> {
    let mut options = Options::new(Standard::C99);
    options.c_variadic = true;
    options.complex = true;
    errors_with(source, &options)
}

/// Analyses `source` under `options` and returns every error message.
fn errors_with(source: &str, options: &Options) -> Vec<String> {
    // String-literal mode accepts any C text, including the constructs the
    // Rust lexer refuses.
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let analysis = analyze(input, options);
    let front_end: Vec<String> = analysis
        .diagnostics
        .items()
        .iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(
        front_end.is_empty(),
        "the front end rejected it: {front_end:#?}"
    );

    let (_program, diagnostics) = sema::analyze(&analysis.unit, options, analysis.source.unit_id());
    diagnostics
        .sorted()
        .into_iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| d.message.clone())
        .collect()
}

/// Asserts that `source` is accepted.
fn accepted(source: &str) {
    let found = errors(source);
    assert!(found.is_empty(), "expected no errors, got {found:#?}");
}

/// Asserts that `source` produces exactly the given messages.
fn rejected(source: &str, expected: &[&str]) {
    let found = errors(source);
    assert_eq!(found, expected, "for:\n{source}");
}

// ---------------------------------------------------------------------------
// types
// ---------------------------------------------------------------------------

#[test]
fn every_scalar_type_is_accepted() {
    accepted(
        "
        _Bool b;
        char c;
        signed char sc;
        unsigned char uc;
        short s;
        unsigned short us;
        int i;
        unsigned int ui;
        long l;
        unsigned long ul;
        long long ll;
        unsigned long long ull;
        float f;
        double d;
        long double ld;
        void nothing(void) { }
        ",
    );
}

#[test]
fn typedefs_resolve_through_several_levels() {
    accepted(
        "
        typedef int level1;
        typedef level1 level2;
        typedef level2 level3;
        level3 identity(level3 v) { return v; }
        ",
    );
}

#[test]
fn a_typedef_of_an_unsupported_type_is_reported_where_it_is_used() {
    // The declaration itself is harmless; only a use has to be diagnosed.
    accepted("typedef double _Imaginary imag;");
    rejected(
        "typedef double _Imaginary imag; int f(void) { imag z; return 0; }",
        &[
            "imaginary types are not supported; no compiler implements '_Imaginary', and C99 \
             makes it optional. Write the '_Complex' type instead",
        ],
    );
}

#[test]
fn derived_and_tagged_types_are_accepted() {
    accepted("int f(void) { int *p; int a[3]; return 0; }");
    accepted("struct S { int x; }; int f(void) { struct S s; return s.x; }");
    accepted("union U { int x; float f; }; int f(void) { union U u; return u.x; }");
    accepted("enum E { A, B }; int f(void) { enum E e = B; return e; }");
    accepted("typedef int *intptr; int f(int *q) { intptr p = q; return *p; }");
    accepted("int f(int (*g)(int)) { return g(1); }");
    rejected(
        "int f(void) { return 0; } int g(void) { double _Imaginary z; return 0; }",
        &[
            "imaginary types are not supported; no compiler implements '_Imaginary', and C99 \
             makes it optional. Write the '_Complex' type instead",
        ],
    );
}

// ---------------------------------------------------------------------------
// complex
// ---------------------------------------------------------------------------

#[test]
fn the_complex_types_and_their_operators_are_accepted() {
    accepted("float _Complex a; double _Complex b; long double _Complex c;");
    accepted("double _Complex f(double _Complex a, double _Complex b) { return a * b / a - b; }");
    accepted("double _Complex f(double _Complex z, double x) { return z * x + x / z; }");
    accepted("int f(double _Complex a, double _Complex b) { return a == b || a != b; }");
    accepted("double f(double _Complex z) { return __real__ z + __imag__ z; }");
    accepted("void f(double _Complex *z) { __imag__ *z = 1.0; __real__ *z += 2.0; }");
    accepted("double _Complex f(double _Complex z) { return ~z; }");
    accepted("double _Complex f(double _Complex z) { z++; --z; return z; }");
    accepted("int f(double _Complex z) { return z ? 1 : !z; }");
    accepted("double _Complex g = 1.0 + 2.0i; float _Complex h = 3.0if;");
    accepted("double _Complex g = __builtin_complex(1.0, 2.0);");
    accepted("double f(double _Complex z) { return __builtin_creal(z) + __builtin_cimag(z); }");
    accepted("double _Complex f(double _Complex z) { return __builtin_cproj(__builtin_conj(z)); }");
    accepted("unsigned long f(void) { return sizeof(double _Complex) + sizeof(float _Complex); }");
    accepted("struct S { double _Complex z; int n; }; double _Complex a[4];");
    // `__real__` and `__imag__` mean something on a real operand too.
    accepted("double f(double x) { return __real__ x + __imag__ x; }");
    accepted("int f(int n) { return __real__ n + __imag__ (n + 1); }");
}

#[test]
fn the_operators_the_complex_types_do_not_have_are_refused() {
    rejected(
        "int f(double _Complex a, double _Complex b) { return a < b; }",
        &[
            "'<' is not defined for the complex type 'double _Complex': the complex numbers \
             are not ordered (C99 6.5.8 requires real operands). Compare the parts, or the \
             magnitudes with 'cabs'",
        ],
    );
    rejected(
        "double _Complex f(double _Complex a, double _Complex b) { return a % b; }",
        &[
            "operator '%' requires integer operands ('double _Complex' and 'double _Complex' \
             given)",
        ],
    );
    rejected(
        "double _Complex f(double _Complex a, int b) { return a << b; }",
        &["operator '<<' requires integer operands ('double _Complex' and 'int' given)"],
    );
    rejected(
        "struct S { double _Complex z : 3; };",
        &[
            "bit-field 'z' has invalid type 'double _Complex'; only the integer types may \
             be given a width",
        ],
    );
    let mut c11 = Options::new(Standard::C11);
    c11.complex = true;
    assert_eq!(
        errors_with("_Atomic double _Complex z;", &c11),
        [
            "'_Atomic double _Complex' is not supported yet: only the scalar types have a \
             lock-free atomic in `core::sync::atomic`, and nothing in the generated Rust could \
             stand for a lock"
        ]
    );
    assert_eq!(
        errors_with(
            "double f(void) { return _Generic((double _Complex) 1.0, \
             double _Complex: 1.0, default: 0.0); }",
            &c11
        ),
        Vec::<String>::new()
    );
}

/// With the `complex` feature off, every door is shut and each one says the
/// same thing.
#[test]
fn without_the_feature_the_complex_types_are_refused() {
    let mut options = Options::new(Standard::C99);
    options.complex = false;
    let hint = "complex types are not supported here: '_Complex' needs the 'complex' feature \
                of the cinrs crate, which is on by default and supplies the runtime the \
                generated code links against";
    assert_eq!(
        errors_with("int f(void) { double _Complex z; return 0; }", &options),
        [hint]
    );
    assert_eq!(
        errors_with("double f(double _Complex z) { return 0; }", &options),
        [hint]
    );
    assert_eq!(
        errors_with("double f(void) { return __builtin_creal(1.0); }", &options),
        [hint]
    );
}

/// Without the feature a complex type may still be *named* — a platform
/// `<complex.h>` declares forty functions with them, and `<tgmath.h>` includes
/// it — so a declared-only prototype, a `typedef`, a pointer, `sizeof` and a
/// `_Generic` association are fine. Where a value would exist, the refusal is
/// the same hint, by name for a call.
#[test]
fn without_the_feature_the_complex_types_may_still_be_named() {
    let mut options = Options::new(Standard::C11);
    options.complex = false;
    let hint = "complex types are not supported here: '_Complex' needs the 'complex' feature \
                of the cinrs crate, which is on by default and supplies the runtime the \
                generated code links against";
    let declarations = "double _Complex csin(double _Complex);\n\
                        float _Complex csinf(float _Complex);\n\
                        long double _Complex csinl(long double _Complex);\n\
                        typedef double _Complex cplx;\n\
                        _Complex double *p;\n\
                        void keep(cplx *q);\n\
                        typedef char size[sizeof(double _Complex) == 16 \
                        && _Alignof(float _Complex) == 4 ? 1 : -1];\n\
                        typedef char pick[_Generic(1.0, double _Complex: -1, default: 1)];\n";
    assert_eq!(errors_with(declarations, &options), Vec::<String>::new());
    assert_eq!(
        errors_with(
            &format!("{declarations}void f(cplx *z) {{ csin(*z); }}"),
            &options
        ),
        [format!(
            "'csin' returns 'double _Complex', which cannot be called: {hint}"
        )]
    );
    assert_eq!(
        errors_with(&format!("{declarations}double _Complex z;"), &options),
        [hint]
    );
    assert_eq!(
        errors_with(
            &format!("{declarations}void f(void) {{ (void)(double _Complex) 1; }}"),
            &options
        ),
        [format!(
            "a cast to 'double _Complex' is not supported: {hint}"
        )]
    );
}

#[test]
fn bit_fields_are_checked_against_their_type() {
    accepted("struct S { unsigned int flag : 1; int level : 3; unsigned : 0; char tag; };");
    accepted("struct S { _Bool a : 1; long long b : 40; };");
    accepted("enum E { A, B }; struct S { enum E e : 3; };");
    accepted("union U { unsigned a : 3; unsigned b : 20; };");
    rejected(
        "struct S { double d : 3; };",
        &["bit-field 'd' has invalid type 'double'; only the integer types may be given a width"],
    );
    rejected(
        "struct S { int *p : 3; };",
        &["bit-field 'p' has invalid type 'int *'; only the integer types may be given a width"],
    );
    rejected(
        "struct S { int : 3.5; };",
        &["the width of anonymous bit-field has non-integer type 'double'"],
    );
    rejected(
        "int n; struct S { int a : n; };",
        &["the width of bit-field 'a' is not an integer constant expression"],
    );
    rejected(
        "struct S { int a : -1; };",
        &["negative width in bit-field 'a'"],
    );
    rejected(
        "struct S { char a : 9; };",
        &["width 9 of bit-field 'a' exceeds the 8 bits of its type 'char'"],
    );
    rejected(
        "struct S { _Bool a : 2; };",
        &["width 2 of bit-field 'a' exceeds the 1 bit of its type '_Bool'"],
    );
    rejected(
        "struct S { int a : 0; };",
        &["zero width for bit-field 'a'; only an unnamed bit-field may be `: 0`"],
    );
    rejected(
        "struct S { int a : 3; int a : 3; };",
        &["duplicate member 'a'"],
    );
    // A bit-field has no address, so there is nothing for an alignment to
    // apply to; GCC says the same.
    let mut c11 = Options::new(Standard::C11);
    c11.c_variadic = true;
    assert_eq!(
        errors_with("struct S { _Alignas(4) int a : 3; };", &c11),
        ["'_Alignas' cannot be applied to a bit-field"]
    );
}

#[test]
fn a_bit_field_has_neither_an_address_nor_a_size() {
    rejected(
        "struct S { int a : 3; }; int f(struct S *s) { return (int) &s->a; }",
        &["cannot take the address of a bit-field"],
    );
    rejected(
        "struct S { int a : 3; }; unsigned long f(struct S *s) { return sizeof s->a; }",
        &["'sizeof' applied to a bit-field, which has no size of its own"],
    );
    rejected(
        "struct S { int a : 3; }; unsigned long f(void) { return __builtin_offsetof(struct S, a); }",
        &["'offsetof' applied to the bit-field 'a', which has no address"],
    );
}

#[test]
fn constructs_that_are_still_out_of_reach_are_named() {
    // C99's variably modified types work at block scope, in every shape.
    accepted("int f(int n) { int a[n]; return a[0]; }");
    accepted("int f(int n) { int a[n][3]; return a[0][2]; }");
    accepted("int f(int n) { int a[n][n]; return a[0][0]; }");
    accepted("int f(int n) { int a[3][n]; return a[0][0]; }");
    accepted("int f(int n) { int (*p)[n]; return p != 0; }");
    accepted("int f(int n) { typedef int A[n]; A a; return a[0]; }");
    accepted("int f(int n, int m, int a[n][m]) { return a[1][1]; }");
    accepted("int f(int n, int m, int a[*][*]); int f(int n, int m, int a[n][m]) { return **a; }");
    // What is left is what C itself forbids: a size nobody could evaluate.
    rejected(
        "int n; int a[n];",
        &["array size is not an integer constant expression"],
    );
    rejected(
        "int f(int n) { static int (*p)[n]; return p != 0; }",
        &["a variably modified type cannot have static storage duration"],
    );
    rejected(
        "int f(int n) { int a[*]; return a[0]; }",
        &["'[*]' is only allowed in a function prototype"],
    );
}

/// C23 6.7.2.3p1 (N3037): two definitions of one tag in one scope declare one
/// type when their members agree, and are the redefinition they always were
/// when they do not — with the difference named.
#[test]
fn a_repeated_tag_definition_is_c23s_same_type() {
    let mut c23 = Options::new(Standard::C23);
    c23.c_variadic = true;
    c23.complex = true;
    let accepted_c23 = |source: &str| {
        let found = errors_with(source, &c23);
        assert!(found.is_empty(), "expected no errors, got {found:#?}");
    };
    let rejected_c23 = |source: &str, expected: &[&str]| {
        assert_eq!(errors_with(source, &c23), expected, "for:\n{source}");
    };

    // The member list repeated, in every shape one can be written in.
    accepted_c23("struct S { int x; int y; }; struct S { int x, y; };");
    accepted_c23("union U { int x; float y; }; union U { int x; float y; };");
    accepted_c23("struct S { int x; }; struct S { signed int x; };");
    accepted_c23("typedef int T; struct S { int x; }; struct S { T x; };");
    accepted_c23("struct S { unsigned b : 3; }; struct S { unsigned b : 3; };");
    accepted_c23("struct S { struct S *next; }; struct S { struct S *next; };");
    accepted_c23("struct S { struct { int x; }; }; struct S { struct { int x; }; };");
    // An enumeration too, and its enumerators are not declared twice either:
    // the correspondence is by name, so the order may differ.
    accepted_c23("enum E { A, B }; enum E { A, B };");
    accepted_c23("enum E { A = 1, B = 0 }; enum E { B = 0, A = 1 };");
    accepted_c23("enum E : short { A }; enum E : short { A };");

    // A member list that differs, with the difference named.
    rejected_c23(
        "struct S { int x; }; struct S { int y; };",
        &[
            "redefinition of 'struct S' with an incompatible member list: the member at \
           position 1 is named 'y' here and 'x' in the first definition",
        ],
    );
    rejected_c23(
        "struct S { int x; }; struct S { float x; };",
        &[
            "redefinition of 'struct S' with an incompatible member list: member 'x' has type \
           'float' here and 'int' in the first definition",
        ],
    );
    rejected_c23(
        "struct S { int x; }; struct S { int x; int y; };",
        &[
            "redefinition of 'struct S' with an incompatible member list: the first definition \
           has one member and this one has 2 members",
        ],
    );
    rejected_c23(
        "struct S { unsigned b : 3; }; struct S { unsigned b : 4; };",
        &[
            "redefinition of 'struct S' with an incompatible member list: member 'b' is 4 bits \
           wide here and 3 in the first definition",
        ],
    );
    rejected_c23(
        "struct S { _Alignas(8) int x; }; struct S { int x; };",
        &[
            "redefinition of 'struct S' with an incompatible member list: the alignment differs: \
           the type's own here and 8 in the first definition",
        ],
    );
    rejected_c23(
        "enum E { A, B }; enum E { A, B, C };",
        &[
            "redefinition of 'enum E' with an incompatible enumerator list: the first definition \
           has 2 enumerators and this one has 3 enumerators",
        ],
    );
    rejected_c23(
        "enum E { A = 1 }; enum E { A = 2 };",
        &[
            "redefinition of 'enum E' with an incompatible enumerator list: enumerator 'A' is 2 \
           here and 1 in the first definition",
        ],
    );
    rejected_c23(
        "enum E { A }; enum E { B };",
        &[
            "redefinition of 'enum E' with an incompatible enumerator list: the first definition \
           has no enumerator named 'B'",
        ],
    );
    rejected_c23(
        "enum E : short { A }; enum E : long { A };",
        &[
            "redefinition of 'enum E' with an incompatible enumerator list: the underlying type \
           is 'long' here and 'short' in the first definition",
        ],
    );

    // Two tags of one name from two *scopes* are compatible on the same rule.
    // The `typedef` is how the outer one is named from inside the block, where
    // the tag itself means the inner declaration.
    accepted_c23(
        "struct S { int x; }; typedef struct S outer; \
         int f(void) { struct S { int x; } inner; return _Generic(inner, outer: 1); }",
    );
    accepted_c23(
        "struct S { int x; }; typedef struct S outer; \
         int f(void) { struct S { int x; } inner; outer *p = &inner; return p->x; }",
    );
    // …and are two types again when the members differ.
    rejected_c23(
        "struct S { int x; }; typedef struct S outer; \
         int f(void) { struct S { long x; } inner; return _Generic(inner, outer: 1); }",
        &["'_Generic' has no association for the controlling expression's type 'struct S'"],
    );
    // Before C23 compatibility is tag identity, so the same pointer is the
    // riddle the note explains.
    rejected(
        "struct S { int x; }; typedef struct S outer; \
         int f(void) { struct S { int x; } inner; outer *p = &inner; return p->x; }",
        &[
            "cannot initialize 'p', of type 'struct S *', with an expression of type \
           'struct S *'",
        ],
    );
}

#[test]
fn tags_are_checked() {
    rejected(
        "struct S { int x; }; struct S { int y; };",
        &["redefinition of 'struct S'"],
    );
    rejected(
        "struct S { int x; }; union S { int y; };",
        &["'S' defined as the wrong kind of tag"],
    );
    rejected(
        "int f(void) { struct Unknown u; return 0; }",
        &["variable 'u' has incomplete type 'struct Unknown'"],
    );
    rejected(
        "struct S { struct S inner; };",
        &["member 'inner' has incomplete type 'struct S'"],
    );
    rejected("struct S { int x; int x; };", &["duplicate member 'x'"]);
    // A tag may be declared before it is defined, and pointed at meanwhile.
    accepted(
        "struct Node; struct Node { int v; struct Node *next; }; int f(struct Node *n) { return n->v; }",
    );
    // Two blocks may each have their own `struct S`.
    accepted(
        "int f(void) { struct S { int a; } s; s.a = 1; return s.a; } int g(void) { struct S { double b; } s; s.b = 1; return 0; }",
    );
}

#[test]
fn pointer_conversions_follow_c() {
    accepted("int f(int *p) { void *v = p; int *q = v; return *q; }");
    accepted("int f(int *p) { const int *c = p; return *c; }");
    rejected(
        "int f(int *p, char *s) { p = s; return 0; }",
        &["assigning to 'int *' from incompatible type 'char *'"],
    );
    rejected(
        "int f(int *p) { p = 1; return 0; }",
        &["assigning to 'int *' from incompatible type 'int'"],
    );
    // The null pointer constant needs no cast.
    accepted("int f(void) { int *p = 0; return p == 0; }");
    accepted("int f(void) { int *p = (void *) 0; return !p; }");
    rejected(
        "int f(int *p, double *d) { return p == d; }",
        &["comparison of distinct pointer types 'int *' and 'double *'"],
    );
    // `p - n` is still a pointer, and comparing it with `p` is fine.
    accepted("int f(int *p, int n) { return p - n == p; }");
    rejected(
        "int f(int *p, int n) { return p == n; }",
        &[
            "comparison between a pointer and 'int'; an integer needs a cast, and only \
             the constant 0 is a null pointer",
        ],
    );
}

#[test]
fn pointer_arithmetic_is_checked() {
    accepted("int f(int *p, int n) { return *(p + n) + *(n + p) - *(p - 1); }");
    accepted("long f(int *a, int *b) { return a - b; }");
    rejected(
        "int f(int *p, double *q) { return p - q; }",
        &["invalid operands to binary '-' ('int *' and 'double *')"],
    );
    rejected(
        "int f(int *p, double d) { return *(p + d); }",
        &["invalid operands to binary '+' ('int *' and 'double')"],
    );
    rejected(
        "int f(void) { return 0; } int g(void) { return *(f + 1) ; }",
        &["arithmetic on a pointer to a function is not allowed"],
    );
}

#[test]
fn members_are_checked() {
    rejected(
        "struct S { int x; }; int f(struct S s) { return s.y; }",
        &["no member named 'y' in 'struct S'"],
    );
    rejected(
        "int f(int n) { return n.x; }",
        &["member reference base type 'int' is not a structure or union"],
    );
    rejected(
        "struct S { int x; }; int f(struct S s) { return s->x; }",
        &["member reference type 'struct S' is not a pointer; did you mean to use '.'?"],
    );
    rejected(
        "struct S { const int x; }; void f(struct S *s) { s->x = 1; }",
        &["cannot assign to a location of const-qualified type 'int'"],
    );
}

#[test]
fn arrays_are_checked() {
    accepted("int f(void) { int a[3] = {1, 2, 3}; return a[2]; }");
    accepted("int f(void) { int a[] = {1, 2, 3}; return sizeof(a); }");
    accepted("int f(void) { char s[] = \"hi\"; return s[0]; }");
    rejected(
        "void f(void) { int a[3]; int b[3]; a = b; }",
        &["array type 'int[3]' is not assignable"],
    );
    // An incomplete array type is a *type* (6.2.5p22) and only an error where
    // an object needs a size, which a block-scope one does.
    accepted("extern int j[]; int f(void) { return j[0]; }");
    accepted("int j[]; int f(void) { return j[0]; }");
    accepted("int j[]; int j[3]; int f(void) { return sizeof j; }");
    rejected(
        "int f(void) { int a[]; return 0; }",
        &["variable 'a' has incomplete type 'int[]'"],
    );
    rejected(
        "int j[]; int f(void) { return sizeof j; }",
        &["invalid application of 'sizeof' to an incomplete type 'int[]'"],
    );
    rejected(
        "int n; int a[n];",
        &["array size is not an integer constant expression"],
    );
    // An over-long string initialiser is a warning for GCC and Clang alike —
    // WG14 DR114 — and the excess characters are dropped rather than refused.
    accepted("int f(void) { char s[2] = \"long\"; return s[0]; }");
}

#[test]
fn initializers_are_checked() {
    accepted("struct S { int x; int y; }; struct S s = {.y = 2};");
    accepted("struct S { int x; int y; }; int f(void) { struct S s = {1}; return s.y; }");
    accepted("int a[2][2] = {1, 2, 3, 4};");
    rejected(
        "struct S { int x; }; struct S s = {.z = 1};",
        &["no member named 'z' in 'struct S'"],
    );
    rejected(
        "struct S { int x; }; struct S s = {1, 2};",
        &["excess elements in initializer"],
    );
    accepted("struct S { struct T { int a; } t; }; struct S s = {.t.a = 1};");
    accepted("struct S { struct T { int a[2]; } t; }; struct S s = {.t.a[0] = 1, 2};");
    rejected(
        "struct S { struct T { int a; } t; }; struct S s = {.t.z = 1};",
        &["no member named 'z' in 'struct T'"],
    );
    rejected(
        "struct S { struct T { int a; } t; }; struct S s = {.t.a.b = 1};",
        &["a field designator cannot initialize a subobject of type 'int'"],
    );
    rejected(
        "struct S { struct T { int a; } t; }; struct S s = {.t[0] = 1};",
        &["an array designator cannot initialize a subobject of type 'struct T'"],
    );
    rejected(
        "struct S { int a[2]; }; struct S s = {.a[5] = 1};",
        &["array designator index is out of bounds"],
    );
    rejected(
        "int f(void) { return 0; } int a[2] = {f()};",
        &["initializer is not a compile-time constant expression"],
    );
}

#[test]
fn void_cannot_name_an_object() {
    rejected(
        "int f(void) { void v; return 0; }",
        &["variable 'v' has incomplete type 'void'"],
    );
    rejected(
        "int f(void x) { return 0; }",
        &["parameter has incomplete type 'void'"],
    );
    rejected(
        "int f(void) { return sizeof(void); }",
        &["invalid application of 'sizeof' to an incomplete type 'void'"],
    );
}

#[test]
fn a_typedef_name_is_only_a_type_where_the_parser_saw_one() {
    // The parser needs to know which identifiers name types before it can tell
    // a cast from a parenthesised expression, so an undeclared name in type
    // position never even reaches semantic analysis — it is a syntax error.
    accepted("typedef int handle; int f(handle h) { return (handle) h; }");
}

#[test]
fn a_declarator_that_hides_the_specifiers_typedef_leaves_the_later_ones_its_type() {
    // Kissat's inlinequeue.h:62. The specifiers are one for all the
    // declarators; the first one's name is the variable from the end of its
    // declarator on (C11 6.2.1p7), so `links + idx` is pointer arithmetic on it.
    accepted(
        "typedef struct links { unsigned prev, next; } links;
         struct solver { links *links; };
         unsigned f(struct solver *solver, unsigned idx) {
             links *links = solver->links, *l = links + idx;
             return l->prev;
         }",
    );
    // Kissat's stack.h `all_stack (T, E, S)` with `T == E`.
    accepted(
        "typedef struct watch { unsigned lit; } watch;
         #define all_stack(T, E, B, N) T E, *E##_PTR = (B), *const E##_END = (B) + (N)
         unsigned f(watch *begin, unsigned n) {
             unsigned sum = 0;
             for (all_stack (watch, watch, begin, n) ; watch_PTR != watch_END; watch_PTR++)
                 watch = *watch_PTR, sum += watch.lit;
             return sum;
         }",
    );
    // At file scope the name cannot be both in one scope (C11 6.7p3), which
    // is GCC's "redeclared as different kind of symbol" too.
    rejected(
        "typedef struct links { int x; } links; links *links, *l;",
        &["redefinition of 'links' as a type"],
    );
    // `T T = 1, U;`: `U` is a `T`, which is `int` here.
    accepted(
        "typedef int T;
         int f(void) { T T = 1, U = 2; int *p = &U; return T + *p; }
         int g(void) { typedef long T; { T T = 3, U = T; return (int) (T + U); } }",
    );
}

#[test]
fn a_declaration_may_name_an_incomplete_return_or_parameter_type() {
    // Kissat's kimits.h: `struct changes` is completed nowhere, and neither
    // function is defined or called.
    accepted(
        "struct kissat;
         typedef struct changes changes;
         changes kissat_changes (struct kissat *);
         _Bool kissat_changed (changes before, changes after);
         struct s;
         void g(struct s x);
         struct s (*fp)(void);
         int main(void) { return fp == 0; }",
    );
    // Completed later, and called after that: fine.
    accepted(
        "struct s; struct s f(void);
         struct s { int x; };
         int g(void) { return f().x; }",
    );
    // A definition still needs it complete, and so does a call.
    rejected(
        "struct s; struct s f(void) { for (;;); }",
        &["function cannot return an incomplete type 'struct s'"],
    );
    rejected(
        "struct s; void f(struct s x) { }",
        &["parameter has incomplete type 'struct s'"],
    );
    rejected(
        "struct s; struct s f(void); void g(void) { f(); }",
        &["calling 'f' with incomplete return type 'struct s'"],
    );
    rejected(
        "struct s; struct s (*fp)(void); void g(void) { fp(); }",
        &["calling a function with incomplete return type 'struct s'"],
    );
}

// ---------------------------------------------------------------------------
// declarations
// ---------------------------------------------------------------------------

#[test]
fn redefinitions_are_reported() {
    rejected(
        "int f(void) { int x = 1; int x = 2; return x; }",
        &["redefinition of 'x'"],
    );
    rejected(
        "int f(int x) { int x = 1; return x; }",
        &["redefinition of 'x'"],
    );
    rejected(
        "int f(void) { return 0; } int f(void) { return 1; }",
        &["redefinition of 'f'"],
    );
}

#[test]
fn conflicting_declarations_are_reported() {
    rejected(
        "int f(int a); long f(int a) { return a; }",
        &["conflicting types for 'f'"],
    );
    rejected(
        "int f(int a); int f(double a) { return 0; }",
        &["conflicting types for 'f'"],
    );
    accepted("int f(int a); int f(int a) { return a; }");
}

#[test]
fn shadowing_in_an_inner_block_is_fine() {
    accepted("int f(void) { int x = 1; { int x = 2; return x; } }");
}

#[test]
fn a_prototype_the_unit_never_defines_is_an_extern_declaration() {
    // C's whole linkage model: a prototype promises the symbol exists.
    accepted("int missing(int a); int missing(int a); int f(int a) { return missing(a); }");
    accepted("int printf(const char *fmt, ...); int f(void) { return printf(\"%d\", 1); }");
}

#[test]
fn old_style_definitions_are_accepted_and_checked() {
    // Obsolescent, but valid C99: the identifier list and the declaration
    // list make the parameters, and the function's type has no prototype.
    accepted("int f(a, b) int a; int b; { return a + b; }");
    accepted("int f(a, b) int a; char *b; { return a + (b != 0); }");
    accepted("int f(a) register int a; { return a; }");
    // A definition is the only place an identifier list may appear at all.
    rejected(
        "int f(a, b); int f(a, b) int a; int b; { return a + b; }",
        &[
            "an identifier list is only allowed in a function definition; a declaration \
             needs the parameter types",
        ],
    );
    rejected(
        "int f(a) int b; { return 0; }",
        &[
            "type specifier missing for parameter 'a'; C99 does not support implicit 'int'",
            "declaration for parameter 'b', which is not in the identifier list",
        ],
    );
    rejected(
        "int f(a) int a; int a; { return 0; }",
        &["redefinition of parameter 'a'"],
    );
    rejected(
        "int f(a) int a = 1; { return 0; }",
        &["parameter 'a' cannot have an initializer"],
    );
    rejected(
        "int f(a) static int a; { return a; }",
        &["'static' is not allowed on a parameter"],
    );
    rejected(
        "int f(int a) int a; { return a; }",
        &["a declaration list is not allowed after a parameter type list"],
    );
    // C99 removed implicit `int`, so a name the declaration list leaves out
    // is a constraint violation rather than an `int`.
    rejected(
        "int f(a, b) int a; { return a; }",
        &["type specifier missing for parameter 'b'; C99 does not support implicit 'int'"],
    );
}

#[test]
fn extern_objects_are_declared_not_defined() {
    accepted("extern int shared; int read_it(void) { return shared; }");
    accepted("extern int shared; int shared = 1; int read_it(void) { return shared; }");
    // C99 6.9.2: a declaration with no storage class is a definition, so the
    // `extern` in front of it is only a claim about linkage.
    accepted("extern int shared; int shared; int read_it(void) { return shared; }");
    accepted("int shared; extern int shared; int read_it(void) { return shared; }");
    accepted("extern int f(int n); int f(int n) { return n; }");
    rejected(
        "extern int shared = 1;",
        &["'shared' is declared 'extern' and cannot have an initializer here"],
    );
    // One definition is still all that is allowed.
    rejected(
        "extern int shared; int shared = 1; int shared = 2;",
        &["redefinition of 'shared'"],
    );
}

#[test]
fn compound_literals_follow_the_rules_of_the_objects_they_are() {
    accepted("struct S { int a; }; struct S *f(void) { return &(struct S){ 1 }; }");
    accepted("int f(int i) { return (int[]){ 1, 2, 3 }[i]; }");
    accepted("struct S { int a; }; struct S *p = &(struct S){ 1 };");
    accepted("char *f(void) { return (char[]){ \"abc\" }; }");
    // C requires the type name to be a complete object type.
    rejected(
        "void *f(void) { return &(void){ 0 }; }",
        &["a compound literal needs a complete object type, and 'void' is not one"],
    );
    rejected(
        "struct S; struct S *f(void) { return &(struct S){ 0 }; }",
        &["a compound literal needs a complete object type, and 'struct S' is not one"],
    );
    // At file scope the object has static storage duration, so the initialiser
    // has to be a constant expression.
    rejected(
        "int g(void) { return 1; } int *p = &(int){ g() };",
        &["the initializer of a compound literal is not a compile-time constant expression"],
    );
    // At block scope it does not, and so its address is not one either.
    rejected(
        "int f(void) { static int *p = &(int){ 1 }; return *p; }",
        &["initializer is not a compile-time constant expression"],
    );
    rejected(
        "int f(void) { return (const int){ 1 } = 2; }",
        &["cannot assign to a location of const-qualified type 'int'"],
    );
}

#[test]
fn objects_with_static_storage_need_constant_initializers() {
    accepted(
        "int a = 1 + 2 * 3; static double b = 1.5; int f(void) { static int c = 4; return c; }",
    );
    rejected(
        "int f(void) { return 1; } int g = f();",
        &["initializer is not a compile-time constant expression"],
    );
    rejected(
        "int f(void) { static int c = 1 / 0; return c; }",
        &[
            "division by zero in a constant expression",
            "initializer is not a compile-time constant expression",
        ],
    );
}

#[test]
fn a_const_object_cannot_be_assigned_to() {
    rejected(
        "int f(void) { const int limit = 1; limit = 2; return limit; }",
        &["cannot assign to variable 'limit' with const-qualified type"],
    );
}

// ---------------------------------------------------------------------------
// expressions
// ---------------------------------------------------------------------------

#[test]
fn calls_are_checked() {
    rejected(
        "int f(void) { return missing(1); }",
        &["implicit declaration of function 'missing' is invalid in C99"],
    );
    rejected(
        "int add(int a, int b) { return a + b; } int f(void) { return add(1); }",
        &["too few arguments to function call, expected 2, have 1"],
    );
    rejected(
        "int add(int a, int b) { return a + b; } int f(void) { return add(1, 2, 3); }",
        &["too many arguments to function call, expected 2, have 3"],
    );
    rejected(
        "int f(void) { int x = 0; return x(1); }",
        &["called object 'x' is not a function"],
    );
    // Arguments are converted to the parameter types, so this is fine.
    accepted("int f(double d) { return (int) d; } int g(void) { return f(1); }");
}

/// C99 6.7.5.3p14 and 6.5.2.2p6: before C23 an empty parameter list says
/// nothing about the parameters, so no argument count is wrong.
#[test]
fn a_call_without_a_prototype_takes_any_arguments() {
    accepted("int f(); int g(void) { return f() + f(1) + f(1, 2, 3); }");
    accepted("int g(int (*fp)()) { return fp(1, 2); }");
    // A definition written that way takes no parameters, and a call with one
    // is still legal C — the callee never looks at it.
    accepted("int f() { return 1; } int g(void) { return f(2); }");
    // C23 removed the form; there the empty list is `(void)`.
    let mut c23 = Options::new(Standard::C23);
    c23.c_variadic = true;
    assert_eq!(
        errors_with("int f(); int g(void) { return f(1); }", &c23),
        ["too many arguments to function call, expected 0, have 1"]
    );
}

/// C99 6.7.5.3p15: an empty parameter list is compatible with a prototype that
/// is not variadic and whose parameter types are all their own promoted forms.
/// GCC enforces the same rule, in the same places.
#[test]
fn compatibility_with_an_empty_parameter_list_follows_the_promotions() {
    accepted("int f(); int f(int);");
    accepted("int f(int); int f();");
    accepted("int f(); int f(void);");
    accepted("int f(); int f(double, char *);");
    rejected("int f(); int f(char);", &["conflicting types for 'f'"]);
    rejected("int f(float); int f();", &["conflicting types for 'f'"]);
    // A variadic prototype can never match one: the empty list promises a
    // fixed argument list.
    rejected("int f(); int f(int, ...);", &["conflicting types for 'f'"]);
    // The same rule decides an assignment between function pointers…
    accepted("int p(int, int); int g(void) { int (*fp)() = p; return fp(1, 2); }");
    accepted("int p(); int g(void) { int (*fp)(int) = p; return fp(1); }");
    rejected(
        "int p(char); int g(void) { int (*fp)() = p; return fp(1); }",
        &[
            "cannot initialize 'fp', of type 'int (*)()', with an expression of type 'int (*)(char)'",
        ],
    );
    // … and what `__builtin_types_compatible_p` answers, which is what `gcc`
    // answers for the same three.
    accepted(
        "int k(void) { \
             return __builtin_types_compatible_p(int (), int (int)) \
                  + __builtin_types_compatible_p(int (), int (void)) \
                  - __builtin_types_compatible_p(int (), int (char)); }",
    );
    // C11 6.5.1.1p2 puts `_Generic` on compatibility as well, so two
    // associations that differ only in the prototype are a duplicate pair.
    let mut c11 = Options::new(Standard::C11);
    c11.c_variadic = true;
    assert!(
        errors_with(
            "int p(int); int k(void) { int (*a)() = p; \
             return _Generic(a, int (*)(int): 1, default: 0); }",
            &c11,
        )
        .is_empty()
    );
    assert_eq!(
        errors_with(
            "int k(int (*a)()) { return _Generic(a, int (*)(): 1, int (*)(int): 2); }",
            &c11,
        ),
        ["'_Generic' has two associations for the compatible type 'int (*)(int)'"]
    );
}

#[test]
fn void_values_cannot_be_used() {
    rejected(
        "void v(void) { } int f(void) { int x = v(); return x; }",
        &["cannot initialize 'x', of type 'int', with an expression of type 'void'"],
    );
    rejected(
        "void v(void) { return 1; }",
        &["void function 'v' should not return a value"],
    );
    rejected(
        "void v(void) { } int f(void) { if (v()) { return 1; } return 0; }",
        &["value of type 'void' is not contextually convertible to a condition"],
    );
}

#[test]
fn lvalues_are_checked() {
    rejected(
        "int f(int x) { 1 = x; return x; }",
        &["expression is not assignable"],
    );
    rejected(
        "int f(int x) { (x + 1) = 2; return x; }",
        &["expression is not assignable"],
    );
    rejected(
        "int f(int x) { return 1++; }",
        &["expression is not assignable"],
    );
    rejected(
        "int g(void) { return 0; } int f(void) { g() = 1; return 0; }",
        &["expression is not assignable"],
    );
}

#[test]
fn operands_are_type_checked() {
    rejected(
        "int f(double d) { return d % 2; }",
        &["operator '%' requires integer operands ('double' and 'int' given)"],
    );
    rejected(
        "int f(double d) { return d << 1; }",
        &["operator '<<' requires integer operands ('double' and 'int' given)"],
    );
    rejected(
        "int f(double d) { return ~d; }",
        &["operator '~' requires an integer operand, but the operand has type 'double'"],
    );
}

#[test]
fn indirection_and_address_of_are_checked() {
    accepted("int f(void) { return \"hello\"[0]; }");
    accepted("int f(int x) { int *p = &x; return *p; }");
    rejected(
        "int f(int x) { return *x; }",
        &["indirection requires pointer operand ('int' invalid)"],
    );
    rejected(
        "int f(int x) { return &x; }",
        &["returning 'int *' from a function with incompatible result type 'int'"],
    );
    rejected(
        "int *f(int x) { return &(x + 1); }",
        &["cannot take the address of an rvalue of type 'int'"],
    );
    // An explicit cast from a pointer to an integer is legal C.
    accepted("int f(void) { return (int) \"s\"; }");
    rejected(
        "struct S { int x; }; int f(struct S s) { return (int) s; }",
        &["cannot cast an expression of type 'struct S' to 'int'"],
    );
    rejected(
        "struct S { int x; }; void f(int n) { struct S s = (struct S) n; }",
        &["a cast to 'struct S' is not allowed; only scalar types can be cast to"],
    );
}

// ---------------------------------------------------------------------------
// statements
// ---------------------------------------------------------------------------

#[test]
fn break_and_continue_need_something_to_leave() {
    rejected(
        "void f(void) { break; }",
        &["'break' statement not in a loop or 'switch' statement"],
    );
    rejected(
        "void f(void) { continue; }",
        &["'continue' statement not in a loop statement"],
    );
    // A `switch` is breakable but not continuable.
    rejected(
        "void f(int n) { switch (n) { case 1: continue; } }",
        &["'continue' statement not in a loop statement"],
    );
    accepted("void f(int n) { while (n) { switch (n) { case 1: break; } continue; } }");
}

#[test]
fn switch_labels_are_checked() {
    rejected(
        "int f(int n) { switch (n) { case 1: return 1; case 1: return 2; } return 0; }",
        &["duplicate case value '1'"],
    );
    rejected(
        "int f(int n) { switch (n) { default: return 1; default: return 2; } }",
        &["multiple 'default' labels in one 'switch'"],
    );
    rejected(
        "int f(int n) { case 1: return n; }",
        &["a 'case' or 'default' label must appear inside a 'switch' statement"],
    );
    rejected(
        "int f(double d) { switch (d) { case 1: return 1; } return 0; }",
        &["statement requires expression of integer type ('double' invalid)"],
    );
    rejected(
        "int f(int n, int m) { switch (n) { case m: return 1; } return 0; }",
        &["'case' label is not a compile-time constant expression"],
    );
}

#[test]
fn a_case_label_nested_in_another_statement_is_duffs_device() {
    // The label is not a direct child of the `switch` body, which is what
    // switches the function to the control-flow-graph lowering.
    accepted("int f(int n) { switch (n) { if (n) { case 1: return 1; } } return 0; }");
    accepted(
        "void copy(char *to, char *from, int count) {
             int n = (count + 7) / 8;
             switch (count % 8) {
             case 0: do { *to++ = *from++;
             case 7:      *to++ = *from++;
             case 1:      *to++ = *from++;
                     } while (--n > 0);
             }
         }",
    );
    // The labels are still checked, wherever they are.
    rejected(
        "int f(int n) { switch (n) { if (n) { case 1: return 1; case 1: return 2; } } return 0; }",
        &["duplicate case value '1'"],
    );
    rejected(
        "int f(int n) { switch (n) { if (n) { default: return 1; } default: return 2; } }",
        &["multiple 'default' labels in one 'switch'"],
    );
}

#[test]
fn labels_are_checked() {
    accepted("int f(int n) { if (n) goto done; n = 1; done: return n; }");
    // A label with nothing jumping to it is harmless.
    accepted("int f(int n) { done: return n; }");
    // Labels have function scope, so a forward jump into a block works.
    accepted("int f(int n) { goto inner; { inner: return n; } }");
    rejected(
        "int f(int n) { goto missing; return n; }",
        &["use of undeclared label 'missing'"],
    );
    rejected(
        "int f(int n) { again: n++; { again: return n; } goto again; }",
        &["redefinition of label 'again'"],
    );
    // Labels are their own namespace: one may share a name with a variable.
    accepted("int f(int n) { n: goto n; }");
}

#[test]
fn switch_bodies_may_declare_objects() {
    // C keeps the object alive across the whole body even though the jump
    // skips its initialiser.
    accepted(
        "int f(int n) {
            switch (n) {
                int local;
                case 1:
                    local = 10;
                    return local;
                default:
                    return 0;
            }
        }",
    );
}

// ---------------------------------------------------------------------------
// integer constants
// ---------------------------------------------------------------------------

#[test]
fn an_integer_constant_too_large_for_any_type_is_reported() {
    rejected(
        "unsigned long long f(void) { return 18446744073709551616; }",
        &["integer constant '18446744073709551616' is too large for any integer type"],
    );
}

#[test]
fn constant_expressions_are_evaluated() {
    accepted("int a = (1 + 2) * 3 - 4 / 2; int b = 1 < 2 ? 10 : 20; int c = sizeof(int) * 2;");
    accepted(
        "int f(int n) { switch (n) { case 'a': return 1; case 1 << 4: return 2; } return 0; }",
    );
}

#[test]
fn tentative_definitions_are_ordinary_c() {
    // 6.9.2: a file-scope declaration without an initialiser may be repeated,
    // and may be completed by one that has an initialiser.
    accepted("int n; int n; int n = 1; int f(void) { return n; }");
    accepted("int n = 1; int n; int f(void) { return n; }");
    rejected("int n = 1; int n = 2;", &["redefinition of 'n'"]);
    // A repeat with a different type is still a conflict.
    rejected("int n; double n;", &["redefinition of 'n'"]);
}

#[test]
fn two_parameters_may_not_share_a_name() {
    rejected(
        "int f(int x, int x) { return x; }",
        &["redefinition of 'x'"],
    );
}

// ---------------------------------------------------------------------------
// offsetof
// ---------------------------------------------------------------------------

#[test]
fn offsetof_is_checked_against_the_layout() {
    accepted(
        "#include <stddef.h>
         struct S { char a; int b; };
         size_t where(void) { return offsetof(struct S, b); }",
    );
    // A union is a record too, and every member of one is at zero.
    accepted(
        "#include <stddef.h>
         union U { int a; double b; };
         size_t where(void) { return offsetof(union U, b); }",
    );
    rejected(
        "#include <stddef.h>
         size_t where(void) { return offsetof(int, b); }",
        &["'offsetof' requires a struct or union type, not 'int'"],
    );
    rejected(
        "#include <stddef.h>
         struct S;
         size_t where(void) { return offsetof(struct S, b); }",
        &["'offsetof' of the incomplete type 'struct S'"],
    );
    rejected(
        "#include <stddef.h>
         struct S { int a; };
         size_t where(void) { return offsetof(struct S, nope); }",
        &["no member named 'nope' in 'struct S'"],
    );
    // Sema knows the layout, so the value is an integer constant expression
    // and may go everywhere C99 6.6 allows one.
    accepted(
        "#include <stddef.h>
         struct S { char a; int b; };
         static size_t where = offsetof(struct S, b);
         char probe[offsetof(struct S, b)];",
    );
    // C99 7.17p3's member designator reaches through members, elements and
    // anonymous members.
    accepted(
        "#include <stddef.h>
         struct Inner { char c; int i; };
         struct Outer { int head; struct Inner rows[3]; };
         size_t where(void) { return offsetof(struct Outer, rows[2].i); }",
    );
    rejected(
        "#include <stddef.h>
         struct Inner { int i; };
         struct Outer { struct Inner in; };
         size_t where(void) { return offsetof(struct Outer, in.nope); }",
        &["no member named 'nope' in 'struct Inner'"],
    );
    rejected(
        "#include <stddef.h>
         struct S { int a; };
         size_t where(void) { return offsetof(struct S, a[1]); }",
        &["a subscript in a member designator needs an array, not 'int'"],
    );
    rejected(
        "#include <stddef.h>
         struct S { unsigned bits : 3; };
         size_t where(void) { return offsetof(struct S, bits); }",
        &["'offsetof' applied to the bit-field 'bits', which has no address"],
    );
}

// ---------------------------------------------------------------------------
// va_list
// ---------------------------------------------------------------------------

/// What makes `va_list` a type and `va_start` a macro: without this header
/// they are ordinary identifiers, since the only name the compiler owns is
/// `__builtin_va_list`.
const STDARG: &str = "#include <stdarg.h>\n";

/// [`accepted`], with `<stdarg.h>` included.
#[track_caller]
fn va_accepted(source: &str) {
    accepted(&format!("{STDARG}{source}"));
}

/// [`rejected`], with `<stdarg.h>` included.
#[track_caller]
fn va_rejected(source: &str, expected: &[&str]) {
    rejected(&format!("{STDARG}{source}"), expected);
}

#[test]
fn variadic_definitions_are_accepted() {
    va_accepted(
        "int sum(int n, ...) {
             va_list ap;
             int total = 0;
             va_start(ap, n);
             for (int i = 0; i < n; i++) total += va_arg(ap, int);
             va_end(ap);
             return total;
         }",
    );
    // The `__builtin_` spellings the header is written in, and the `typedef`
    // it writes, which a unit may repeat.
    accepted(
        "typedef __builtin_va_list va_list;
         double first(int n, ...) {
             va_list ap;
             __builtin_va_start(ap, n);
             double d = __builtin_va_arg(ap, double);
             __builtin_va_end(ap);
             return d;
         }",
    );
    // A `va_list` parameter is what a v-prefixed function takes, and it is
    // what a local of such a function is copied from.
    va_accepted(
        "int vsum(int n, va_list ap) {
             va_list copy;
             va_copy(copy, ap);
             return va_arg(copy, int);
         }
         int sum(int n, ...) {
             va_list ap;
             va_start(ap, n);
             return vsum(n, ap);
         }",
    );
}

/// Without `<stdarg.h>` the names mean nothing in particular, which is what
/// lets a program use them for its own purposes.
#[test]
fn the_va_names_are_ordinary_identifiers_without_the_header() {
    accepted("int va_end(int x) { return x; } int f(void) { return va_end(1); }");
    accepted("typedef int va_list; va_list f(va_list x) { return x; }");
    accepted("struct S { int va_arg; }; int f(struct S s) { return s.va_arg; }");
}

#[test]
fn va_arg_refuses_a_promoted_type() {
    va_rejected(
        "int f(int n, ...) { va_list ap; va_start(ap, n); return va_arg(ap, char); }",
        &[
            "'char' is promoted to 'int' when passed through '...'; you should pass \
             'int' not 'char' to 'va_arg'",
        ],
    );
    va_rejected(
        "double f(int n, ...) { va_list ap; va_start(ap, n); return va_arg(ap, float); }",
        &[
            "'float' is promoted to 'double' when passed through '...'; you should pass \
             'double' not 'float' to 'va_arg'",
        ],
    );
}

/// A record of at most sixteen bytes is rebuilt from its eightbytes, so it is
/// accepted; anything the x86-64 System V ABI would put on the stack is not,
/// because nothing in Rust's stable `va_list` reaches the overflow area.
#[test]
fn va_arg_of_a_record_follows_the_system_v_classification() {
    va_accepted(
        "struct S { int x; double y; };
         union U { double d; long long l; };
         struct Empty { };
         int f(int n, ...) {
             va_list ap;
             va_start(ap, n);
             struct S s = va_arg(ap, struct S);
             union U u = va_arg(ap, union U);
             struct Empty e = va_arg(ap, struct Empty);
             (void) e;
             return s.x + (int) s.y + (int) u.d;
         }",
    );
    va_rejected(
        "struct Big { double a, b, c; };
         int f(int n, ...) { va_list ap; va_start(ap, n); return (int) va_arg(ap, struct Big).a; }",
        &[
            "va_arg with 'struct Big' is not supported: it is 24 bytes, and the x86-64 \
             System V ABI passes a struct larger than 16 bytes on the stack, where Rust's \
             'va_list' cannot reach it",
        ],
    );
    va_rejected(
        "struct P { char c; int i; } __attribute__((packed));
         int f(int n, ...) { va_list ap; va_start(ap, n); return va_arg(ap, struct P).i; }",
        &[
            "va_arg with 'struct P' is not supported: a member is not aligned the way its \
             own type asks, so the x86-64 System V ABI passes the struct on the stack, \
             where Rust's 'va_list' cannot reach it",
        ],
    );
    va_rejected(
        "struct S;
         int f(int n, ...) { va_list ap; va_start(ap, n); va_arg(ap, struct S); return 0; }",
        &["va_arg with 'struct S' is not supported: the type is incomplete"],
    );
}

/// The classification is one ABI's, and every other one differs — the
/// Microsoft x64 ABI passes an aggregate over eight bytes by pointer, AArch64
/// has homogeneous float aggregates — so nothing else is guessed at.
#[test]
fn va_arg_of_a_record_is_x86_64_system_v_only() {
    const SOURCE: &str = "struct S { int x; };
         int f(int n, ...) { va_list ap; va_start(ap, n); return va_arg(ap, struct S).x; }";
    for triple in [
        "aarch64-unknown-linux-gnu",
        "i686-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    ] {
        let target = cinrs_core::target::TargetModel::from_triple(triple).expect("a known triple");
        let mut options = Options::new(Standard::C99).for_target(target);
        options.c_variadic = true;
        options.complex = true;
        let found = errors_with(&format!("{STDARG}{SOURCE}"), &options);
        assert_eq!(
            found,
            ["va_arg of a struct type is only supported on x86-64 System V targets"],
            "for {triple}"
        );
        // A complex value is a pair and goes down the same road, so it stops
        // at the same place — saying so in its own words, since "a struct
        // type" is not what the program wrote.
        let found = errors_with(
            &format!(
                "{STDARG}double _Complex f(int n, ...) {{ va_list ap; va_start(ap, n); \
                 return va_arg(ap, double _Complex); }}"
            ),
            &options,
        );
        assert_eq!(
            found,
            [
                "va_arg with 'double _Complex' is not supported: a complex value is a pair, \
                 so it is read back the way an aggregate is, which is only supported on \
                 x86-64 System V targets"
            ],
            "for {triple}"
        );
    }
}

#[test]
fn va_list_may_only_be_a_local_or_a_parameter() {
    va_rejected(
        "struct S { va_list ap; };",
        &["va_list is only supported as a local variable or parameter"],
    );
    va_rejected(
        "va_list global;",
        &["va_list is only supported as a local variable or parameter"],
    );
    va_rejected(
        "int f(int n, ...) { static va_list ap; return 0; }",
        &["va_list is only supported as a local variable or parameter"],
    );
    va_rejected(
        "int f(int n, ...) { va_list aps[2]; return 0; }",
        &["va_list is only supported as a local variable or parameter"],
    );
    va_rejected(
        "va_list f(int n, ...);",
        &["va_list is only supported as a local variable or parameter"],
    );
    // A `va_list *` is a raw pointer, and a raw pointer may carry a lifetime
    // that only a *local* or a parameter can leave elided: nothing else may.
    va_rejected(
        "struct S { va_list *ap; };",
        &["va_list is only supported as a local variable or parameter"],
    );
    va_rejected(
        "va_list *shared;",
        &["va_list is only supported as a local variable or parameter"],
    );
    va_rejected(
        "int f(int n, ...) { static va_list *ap; return 0; }",
        &["va_list is only supported as a local variable or parameter"],
    );
    va_rejected(
        "va_list *f(int n, ...);",
        &["va_list is only supported as a local variable or parameter"],
    );
    // The address of a list is an ordinary `&`, and a mismatched pointer type
    // is diagnosed as one.
    va_rejected(
        "void g(int *p);
         int f(int n, ...) { va_list ap; va_start(ap, n); g(&ap); return 0; }",
        &["passing 'va_list *' to parameter 1 of 'g', of incompatible type 'int *'"],
    );
    va_rejected(
        "int f(int n, ...) { return sizeof(va_list); }",
        &["invalid application of 'sizeof' to 'va_list'"],
    );
}

#[test]
fn the_va_builtins_are_checked() {
    va_rejected(
        "int f(int n) { va_list ap; va_start(ap, n); return 0; }",
        &[
            "a 'va_list' variable can only be declared in a variadic function or in one \
             that takes a 'va_list' parameter",
        ],
    );
    va_rejected(
        "int vsum(int n, va_list ap) { va_start(ap, n); return 0; }",
        &["'va_start' used in a function with fixed arguments"],
    );
    va_rejected(
        "int f(int n, ...) { int x; va_start(x, n); return 0; }",
        &["expected an object of type 'va_list', not 'int'"],
    );
    va_rejected(
        "int f(int n, ...) { va_list ap; va_start(ap, 1); return 0; }",
        &["the second argument of 'va_start' must name a parameter of 'f'"],
    );
    // The unprefixed name is a macro of two parameters, so the preprocessor
    // catches this one; the builtin behind it is checked in its own right.
    va_rejected(
        "int f(int n, ...) { va_list ap; __builtin_va_start(ap); return 0; }",
        &["'va_start' expects 2 arguments, have 1"],
    );
    va_rejected(
        "int f(int n, ...) { va_list ap; va_list c; va_start(ap, n); va_copy(c, n); return 0; }",
        &["the second argument of 'va_copy' must have type 'va_list', not 'int'"],
    );
}

#[test]
fn an_older_toolchain_is_told_what_it_needs() {
    // The check is exact — this crate is compiled by the toolchain that
    // compiles the expansion — so the diagnostics an older one produces are
    // tested by asking for one rather than by having one.
    let mut options = Options::new(Standard::C99);
    options.c_variadic = false;
    assert_eq!(
        errors_with("int f(int n, ...) { return n; }", &options),
        ["variadic function definitions require Rust 1.99 or later (this toolchain is older)"]
    );
    assert_eq!(
        errors_with(
            &format!("{STDARG}int vsum(int n, va_list ap) {{ return va_arg(ap, int); }}"),
            &options
        ),
        ["'va_list' requires Rust 1.99 or later (this toolchain is older)"]
    );
    // One mention is enough to make the point.
    assert_eq!(
        errors_with(
            &format!(
                "{STDARG}int f(int n, va_list a) {{ return 0; }} \
                 int g(int n, va_list b) {{ return 0; }}"
            ),
            &options
        ),
        ["'va_list' requires Rust 1.99 or later (this toolchain is older)"]
    );
    // Naming the type is not using it: `<stdarg.h>` itself, and the
    // declaration of a `vprintf` nobody calls, generate nothing at all.
    assert!(errors_with(STDARG, &options).is_empty());
    assert!(errors_with(&format!("{STDARG}int vsum(int n, va_list ap);"), &options).is_empty());
    // Declaring and calling a variadic function needs nothing new.
    assert!(
        errors_with(
            "int printf(const char *, ...); int f(void) { return printf(\"hi\"); }",
            &options
        )
        .is_empty()
    );
    // A program that is wrong for other reasons hears about those instead.
    assert_eq!(
        errors_with("int f(int n, ...) { return missing(n); }", &options),
        ["implicit declaration of function 'missing' is invalid in C99"]
    );
}

// ---------------------------------------------------------------------------
// inline assembly
// ---------------------------------------------------------------------------

/// The x86-64 target, whatever the host is.
const X86_64: &str = "#pragma cinrs target \"x86_64-unknown-linux-gnu\"\n";

fn asm_options() -> Options {
    Options::gnu(Standard::C99)
}

/// Every `asm` statement of `source`, as `template | operand heads | clobbers`,
/// asserting there were no errors.
fn asm_ir(source: &str) -> Vec<String> {
    let options = asm_options();
    let source = format!("{X86_64}{source}");
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let analysis = analyze(input, &options);
    assert!(
        analysis.diagnostics.items().is_empty(),
        "front end: {:#?}",
        analysis.diagnostics.items()
    );
    let (program, diagnostics) =
        sema::analyze(&analysis.unit, &analysis.options, analysis.source.unit_id());
    let errors: Vec<_> = diagnostics
        .sorted()
        .into_iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(errors.is_empty(), "expected no errors, got {errors:#?}");
    let mut out = Vec::new();
    for function in &program.functions {
        match &function.body {
            Some(ir::Body::Structured(stmts)) => collect_asm(stmts, &mut out),
            Some(ir::Body::Cfg(cfg)) => {
                for block in &cfg.blocks {
                    collect_asm(&block.stmts, &mut out);
                }
            }
            None => {}
        }
    }
    out
}

fn collect_asm(stmts: &[ir::Stmt], out: &mut Vec<String>) {
    for stmt in stmts {
        match stmt {
            ir::Stmt::Asm(asm) => {
                let heads: Vec<String> = asm.operands.iter().map(|o| o.head()).collect();
                out.push(format!(
                    "{} | {} | {}",
                    asm.template,
                    heads.join(", "),
                    asm.clobbers.join(", ")
                ));
            }
            ir::Stmt::Block(items) => collect_asm(items, out),
            ir::Stmt::Region(region) => collect_asm(&region.body, out),
            ir::Stmt::Switch(switch) => {
                for group in &switch.groups {
                    collect_asm(&group.body, out);
                }
            }
            ir::Stmt::Label { body, .. }
            | ir::Stmt::If {
                then_branch: body, ..
            } => collect_asm(std::slice::from_ref(&**body), out),
            _ => {}
        }
    }
}

/// The errors `source` produces for x86-64.
fn asm_errors(source: &str) -> Vec<String> {
    asm_errors_with(&format!("{X86_64}{source}"), &asm_options())
}

/// The errors `source` produces, with the target its `#pragma cinrs target`
/// resolved to (which [`errors_with`] does not pass on).
fn asm_errors_with(source: &str, options: &Options) -> Vec<String> {
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let analysis = analyze(input, options);
    assert!(
        analysis.diagnostics.items().is_empty(),
        "front end: {:#?}",
        analysis.diagnostics.items()
    );
    let (_program, diagnostics) =
        sema::analyze(&analysis.unit, &analysis.options, analysis.source.unit_id());
    diagnostics
        .sorted()
        .into_iter()
        .filter(|d| d.level == Level::Error)
        .map(|d| d.message.clone())
        .collect()
}

#[test]
fn basic_asm_is_passed_through() {
    assert_eq!(
        asm_ir(
            r#"void f(void) {
                asm("mfence");
                __asm__ __volatile__("pause");
                asm volatile ("" ::: "memory");
                asm("movl %eax, %ebx");
            }"#
        ),
        [
            "mfence |  | ",
            "pause |  | ",
            " |  | ",
            // Basic asm: '%' is literal.
            "movl %eax, %ebx |  | ",
        ]
    );
}

#[test]
fn each_operand_kind_maps_onto_asm() {
    assert_eq!(
        asm_ir(
            r#"long f(long a, int b, unsigned char c, double d, int *p) {
                long r; int s; unsigned int lo, hi;
                asm("addq %1, %0" : "=r"(r) : "r"(a));
                asm("bsrl %1, %0" : "=&r"(s) : "rm"(b));
                asm("addl %1, %0" : "+r"(s) : "ri"(b));
                asm("notb %0" : "+q"(c));
                asm("incl %k0" : "+r"(r));
                asm("incw %w0; incb %b0; incb %h0" : "+r"(s));
                asm("incq %q0" : "+g"(r));
                asm("rdtsc" : "=a"(lo), "=d"(hi));
                asm("movl %1, %0" : "=a"(s) : "c"(b) : "rsi", "cc", "memory");
                asm("shlq %1, %0" : "+r"(r) : "i"(3 + 1));
                asm("addl %2, %0" : "=r"(s) : "r"(b), "0"(a));
                asm("incl (%0)" : : "r"(p));
                asm("addsd %1, %0" : "+x"(d) : "x"(d));
                asm("addl %[y], %[x]" : [x] "+r"(s) : [y] "r"(b));
                asm("movl %0, %%eax; movb %b0, %%al; %{%|%}" : : "a"(b) : "xmm1");
            }"#
        ),
        [
            "addq {o1}, {o0} | o0 = lateout(reg), o1 = in(reg) | ",
            // GCC prints a register at the operand's width, `asm!` at 64 bits
            // unless told: an `int` operand is `{o0:e}`.
            "bsrl {o1:e}, {o0:e} | o0 = out(reg), o1 = in(reg) | ",
            "addl {o1:e}, {o0:e} | o0 = inout(reg), o1 = in(reg) | ",
            "notb {o0} | o0 = inout(reg_byte) | ",
            "incl {o0:e} | o0 = inout(reg) | ",
            "incw {o0:x}; incb {o0:l}; incb {o0:h} | o0 = inout(reg_abcd) | ",
            "incq {o0:r} | o0 = inout(reg) | ",
            "rdtsc | lateout(\"eax\"), lateout(\"edx\") | ",
            "movl %ecx, %eax | lateout(\"eax\"), in(\"ecx\") | rsi",
            "shlq ${o1}, {o0} | o0 = inout(reg), o1 = const | ",
            // The tied input is folded into the output it is tied to, and '%2'
            // names that output. The operand the template never mentions is
            // mentioned in an assembler comment: `asm!` calls an unused named
            // operand an error, GCC does not.
            "addl {o0:e}, {o0:e} /* {o1:e} */ | o0 = inout(reg), o1 = in(reg) | ",
            "incl ({o0}) | o0 = in(reg) | ",
            "addsd {o1}, {o0} | o0 = inout(xmm_reg), o1 = in(xmm_reg) | ",
            "addl {o1:e}, {o0:e} | o0 = inout(reg), o1 = in(reg) | ",
            "movl %eax, %eax; movb %al, %al; {{|}} | in(\"eax\") | xmm1",
        ]
    );
}

/// GCC's `"x"` and `"v"` are a vector register as wide as the operand's type:
/// `asm!`'s `xmm_reg`, `ymm_reg` or `zmm_reg`. `%x`, `%t`, `%g` name the
/// operand's xmm, ymm, zmm register, which is `:x`, `:y`, `:z` on any of them.
#[test]
fn vector_operands_take_the_register_of_their_width() {
    assert_eq!(
        asm_ir(
            r#"#include <immintrin.h>
            __attribute__((target("avx512f,avx512bf16,avx512fp16")))
            void f(float s, double d, int i, long l, __m128i a, __m256i b, __m512i c,
                   __m256d e, __m512 g, __m256bh h, __m512h k) {
                asm("" : "+x"(s), "+x"(d), "+x"(i), "+x"(l));
                asm("" : "+x"(a), "+x"(b), "+x"(c));
                asm("" : "+v"(a), "+v"(b), "+v"(c));
                asm("vpaddd %2, %1, %0" : "=x"(b) : "x"(b), "x"(b));
                asm("vpaddd %2, %1, %0" : "=v"(c) : "v"(c), "0"(c));
                asm("" : "+x"(e), "+v"(g), "+x"(h), "+v"(k));
                asm("vpor %x0, %t0, %g0" : "+x"(a));
                asm("vpor %x0, %t0, %g0" : "+v"(b));
                asm("vpor %x0, %t0, %g0 %0" : "+v"(c));
            }"#
        ),
        [
            " /* {o0} {o1} {o2} {o3} */ | o0 = inout(xmm_reg), o1 = inout(xmm_reg), \
             o2 = inout(xmm_reg), o3 = inout(xmm_reg) | ",
            " /* {o0} {o1} {o2} */ | o0 = inout(xmm_reg), o1 = inout(ymm_reg), \
             o2 = inout(zmm_reg) | ",
            " /* {o0} {o1} {o2} */ | o0 = inout(xmm_reg), o1 = inout(ymm_reg), \
             o2 = inout(zmm_reg) | ",
            "vpaddd {o2}, {o1}, {o0} | o0 = lateout(ymm_reg), o1 = in(ymm_reg), \
             o2 = in(ymm_reg) | ",
            // The tied input is folded into its output.
            "vpaddd {o0}, {o1}, {o0} | o0 = inout(zmm_reg), o1 = in(zmm_reg) | ",
            " /* {o0} {o1} {o2} {o3} */ | o0 = inout(ymm_reg), o1 = inout(zmm_reg), \
             o2 = inout(ymm_reg), o3 = inout(zmm_reg) | ",
            "vpor {o0:x}, {o0:y}, {o0:z} | o0 = inout(xmm_reg) | ",
            "vpor {o0:x}, {o0:y}, {o0:z} | o0 = inout(ymm_reg) | ",
            "vpor {o0:x}, {o0:y}, {o0:z} {o0} | o0 = inout(zmm_reg) | ",
        ]
    );
    let cases: &[(&str, &str)] = &[
        (
            r#"void f(short x) { asm("" : "+v"(x)); }"#,
            "a 2-byte operand cannot live in an SSE register (\"+v\"): 'asm!' takes 32- and \
             64-bit values and the vector types",
        ),
        (
            r#"void f(int x) { asm("incl %t0" : "+r"(x)); }"#,
            "the operand modifier '%t' names a vector register, and this operand is in a \
             general-purpose one",
        ),
        (
            r#"#include <immintrin.h>
            void f(__m256i x) { asm("" : "+r"(x)); }"#,
            "a vector operand needs an SSE register: write \"x\", not \"+r\"",
        ),
        (
            r#"#include <immintrin.h>
            void f(__m128i x) { asm("%k0" : "+x"(x)); }"#,
            "the operand modifier '%k' cannot apply to a vector register (\"x\") operand",
        ),
        (
            r#"void f(int x) { asm("" : : "Yv"(x)); }"#,
            "the constraint \"Yv\" is not supported: 'asm!' has no class for it. Use \"x\" for \
             an SSE register",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(asm_errors(source), [*expected], "for:\n{source}");
    }
}

#[test]
fn asm_is_carried_by_the_control_flow_graph() {
    let found = asm_ir(
        r#"int f(int n) {
            int i = 0;
        again:
            asm volatile ("pause");
            if (++i < n) goto again;
            switch (n) { case 1: asm("nop"); break; default: break; }
            return i;
        }"#,
    );
    assert_eq!(found, ["pause |  | ", "nop |  | "]);
}

#[test]
fn what_asm_cannot_express_is_refused_by_name() {
    let cases: &[(&str, &str)] = &[
        (
            r#"void f(int x) { asm("incl %0" : "+m"(x)); }"#,
            "the constraint \"+m\" asks for a memory operand, and Rust's 'asm!' has none: pass \
             the address in a register (\"r\"(&x)) and write the memory reference in the \
             template, such as '(%0)'",
        ),
        (
            r#"void f(int x) { asm("" : : "m"(x)); }"#,
            "the constraint \"m\" asks for a memory operand, and Rust's 'asm!' has none: pass \
             the address in a register (\"r\"(&x)) and write the memory reference in the \
             template, such as '(%0)'",
        ),
        (
            r#"void f(unsigned long long x) { asm("rdtsc" : "=A"(x)); }"#,
            "the constraint \"A\" (the edx:eax pair) is not supported: 'asm!' has no operand \
             that spans two registers. Use \"=a\" and \"=d\" with two variables and combine them",
        ),
        (
            r#"void f(double x) { asm("fsqrt" : "+t"(x)); }"#,
            "the constraint \"t\" (an x87 stack register) is not supported: 'asm!' has no \
             operand on the x87 register stack",
        ),
        (
            r#"void f(double x) { asm("" : "=f"(x)); }"#,
            "the constraint \"f\" (an x87 stack register) is not supported: 'asm!' has no \
             operand on the x87 register stack",
        ),
        (
            r#"void f(int x) { asm("" : : "y"(x)); }"#,
            "the constraint \"y\" (an MMX register) is not supported: Rust has no MMX",
        ),
        (
            r#"void f(int x) { asm("" : : "X"(x)); }"#,
            "the constraint \"X\" (any operand at all) is not supported: say which register \
             class, such as \"r\"",
        ),
        (
            r#"void f(int x) { asm("" : : "I"(x)); }"#,
            "the constraint \"I\" (a range-checked immediate) is not supported: write \"i\", \
             which 'asm!' takes as a 'const' operand",
        ),
        (
            r#"void f(int x) { asm("" : : "R"(x)); }"#,
            "the constraint \"R\" (a legacy register) is not supported: 'asm!' has no class for \
             it. Use \"r\", or name the register",
        ),
        (
            r#"void f(int x) { asm("" : : "Yz"(x)); }"#,
            "the constraint \"Yz\" is not supported: 'asm!' has no class for it. Use \"x\" for \
             an SSE register",
        ),
        (
            r#"void f(int x, int y) { asm("cpuid" : "=b"(x) : "b"(y)); }"#,
            "operand 1 cannot be in \"b\" too: operand 0 is in rbx already, and one register \
             holds one operand",
        ),
        (
            r#"void f(int x) { asm("cpuid" : "=b"(x) : : "ebx"); }"#,
            "the clobber \"ebx\" is also operand 0 (\"b\") of this 'asm' statement: drop the \
             clobber, as cinrs restores rbx after the template anyway",
        ),
        (
            r#"void f(unsigned char x) { asm("" : "+b"(x)); }"#,
            "a one-byte operand in \"b\" is not supported: cinrs carries a \"b\" operand in a \
             scratch register swapped with rbx by an 'xchg', and a byte register would restore \
             only bl. Widen the operand to 'unsigned int'",
        ),
        (
            r#"int f(int a, int b) { char z; asm("cmpl %1, %2" : "=@ccz"(z) : "r"(a), "r"(b)); return z; }"#,
            "the flag output \"=@ccz\" is not supported: 'asm!' has no flag outputs. Set a byte \
             register from the flag in the template ('setz %b0') and use \"=q\"",
        ),
        (
            r#"void f(void) { asm("1: jmp 1b%=" : :); }"#,
            "'%=' is not supported: 'asm!' has no number unique to each instance. Use a GNU as \
             local label ('1:' with '1b' or '1f') instead",
        ),
        (
            r#"void f(void) { asm(".byte %c0" : : "i"(1)); }"#,
            "the operand modifier '%c' is not supported: it prints a constant or an address \
             without its '$', which 'asm!' has no spelling for. Write the operand with \"i\" \
             and '%0', or pass the address in a register",
        ),
        (
            r#"void f(int x) { asm("" : : "i"(x)); }"#,
            "the constraint \"i\" asks for an immediate, and this operand is not an integer \
             constant expression: 'asm!' takes an immediate as a 'const' operand. Write a \
             constant, or use \"r\" to pass the value in a register",
        ),
        (
            r#"struct S { int b : 3; }; void f(struct S *s) { asm("" : "=r"(s->b)); }"#,
            "a bit-field cannot be an 'asm' output: it has no register-sized storage of its own. \
             Write the output to a local and assign the bit-field from it",
        ),
        (
            r#"void f(void) { asm("cpuid" : : : "rbx"); }"#,
            "the clobber \"rbx\" is not supported: rustc reserves rbx (LLVM uses it \
             internally) and refuses it as an 'asm!' clobber; give the value the instruction \
             leaves in rbx a \"=b\" operand instead, which cinrs carries in and out of rbx with \
             an 'xchg' around the template",
        ),
        (
            r#"void f(void) { asm("" : : : "rsp"); }"#,
            "the clobber \"rsp\" is not supported: the stack pointer cannot be an 'asm!' operand",
        ),
        (
            r#"void f(void) { asm("{movl|mov} %eax, %ebx"); }"#,
            "'{' or '}' in a basic 'asm' template: GCC passes a basic template to the \
             assembler as it is, so its dialect alternatives '{att|intel}' are only chosen \
             in an extended 'asm' (one with a ':'), and the assembler rejects the braces. \
             Write the AT&T form alone, or add ':' to make it extended",
        ),
        (
            r#"void f(void) { asm("{movl|mov %%eax, %%ebx" : :); }"#,
            "a '{' with no '}' after it in an 'asm' template: braces there are GCC's \
             assembler dialect alternatives, '{att|intel}', which cannot nest and must be \
             closed; write '%{' and '%}' for a literal brace",
        ),
        (
            r#"void f(void) { asm("{a|{b}}" : :); }"#,
            "a '{' inside another in an 'asm' template: braces there are GCC's \
             assembler dialect alternatives, '{att|intel}', which cannot nest and must be \
             closed; write '%{' and '%}' for a literal brace",
        ),
        (
            r#"void f(void) { asm("nop}" : :); }"#,
            "a '}' with no '{' before it in an 'asm' template: braces there are GCC's \
             assembler dialect alternatives, '{att|intel}', which cannot nest and must be \
             closed; write '%{' and '%}' for a literal brace",
        ),
        (
            r#"void f(void) { asm(".intel_syntax noprefix\n mov eax, ebx"); }"#,
            "a template that switches to Intel syntax with '.intel_syntax' is not supported: \
             GCC's x86 templates are AT&T, and cinrs gives 'asm!' options(att_syntax) to match. \
             Write the instructions in AT&T syntax",
        ),
        (
            r#"int f(int x) { asm goto ("jmp %l0" : : : : out); return 0; out: return 1; }"#,
            "'asm goto' is not supported yet: Rust's 'asm!' has 'label' blocks, but the jump to \
             a C label has to go through the function's control flow, which this release does \
             not do. Write the branch in C on a flag the 'asm' sets",
        ),
        (
            r#"void f(void) { register int x asm("eax") = 1; (void)x; }"#,
            "an 'asm' label on a local variable is not supported: GCC's register variable has no \
             counterpart in Rust's 'asm!'. Write the register as a constraint of the 'asm' \
             statement instead, such as \"a\"(x) for eax",
        ),
        (
            r#"struct P { int a, b; }; void f(struct P p) { asm("" : : "r"(p)); }"#,
            "an 'asm' operand has to have integer, floating or pointer type, not 'struct P'",
        ),
        (
            r#"void f(char c) { asm("incl %k0" : "+r"(c)); }"#,
            "the operand modifier '%k' cannot apply to an 8-bit operand: 'asm!' has no wider \
             name for a byte register. Widen the operand to 'unsigned int'",
        ),
        (
            r#"void f(int x) { asm("mov %1, %0" : "=r"(x)); }"#,
            "'%1' names operand 1 that this 'asm' statement does not have (1 operands)",
        ),
        (
            r#"void f(int x) { asm("" : "r"(x)); }"#,
            "the output constraint \"r\" has to start with '=' or '+'",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(asm_errors(source), [*expected], "for:\n{source}");
    }
}

#[test]
fn asm_is_refused_where_it_cannot_be_had() {
    // A safe function has no `unsafe` block to hold an `asm!`.
    let mut options = Options::gnu(Standard::C23);
    options.c_variadic = true;
    assert_eq!(
        asm_errors_with(
            &format!("{X86_64}[[cinrs::safe]] void f(void) {{ asm(\"nop\"); }}"),
            &options
        ),
        [
            "inline assembly cannot be written in the safe function 'f': Rust's 'asm!' is unsafe, \
          and a safe function has no 'unsafe' block to put it in. Drop [[cinrs::safe]] from 'f'"
        ]
    );
    // Another architecture's template would be handed over unchanged, and the
    // registers are x86's.
    assert_eq!(
        asm_errors_with(
            "#pragma cinrs target \"aarch64-unknown-linux-gnu\"\nvoid f(void) { asm(\"nop\"); }",
            &asm_options()
        ),
        [
            "inline assembly is only supported on x86 and x86-64: the template is assembly for \
          one architecture and the operands are mapped onto x86's registers, and the target \
          here is aarch64"
        ]
    );
}

// ---------------------------------------------------------------------------
// GCC's vector operators on the Intel types
// ---------------------------------------------------------------------------

/// The errors of `body`, on x86-64 with `<immintrin.h>` included.
fn vector_errors(body: &str) -> Vec<String> {
    errors(&format!("{X86_64}#include <immintrin.h>\n{body}"))
}

/// The operators GCC's vector extension gives the Intel types are accepted,
/// and their results have the types GCC gives them: the operand's own for
/// arithmetic, the same-size integer vector for a comparison.
#[test]
fn vector_operators_are_lowered_with_gccs_types() {
    let found = vector_errors(
        "__attribute__((target(\"avx512f\"))) void f(__m128d a, __m128d b, __m128 x, \
             __m256d c, __m512d d, __m128i i, __m128i j, double s, int n) {\n\
             __m128d r = a * b + a / b - a;\n\
             __m128 y = x * 2.0f + 1 - x;\n\
             __m128d t = 2.0 / a + n * a;\n\
             __m256d u = c * c - 3.0;\n\
             __m512d w = d / d + s;\n\
             __m128d neg = -a, pos = +a, bits = (a & b) | (a ^ b);\n\
             __m512d neg512 = -d, and512 = d & d;\n\
             __m128i k = i + j - i, l = (i & j) | (i ^ ~j), m = -i;\n\
             __m128i lt = a < b, ne = x != x;\n\
             __m256i ge = c >= c;\n\
             struct { __m128d v; } h = { a };\n\
             __m128d arr[2] = { a, b };\n\
             h.v += a; h.v *= 2.0; arr[n] -= b; arr[1] /= arr[0]; i ^= j;\n\
             (void)r; (void)y; (void)t; (void)u; (void)w; (void)neg; (void)pos; (void)bits;\n\
             (void)neg512; (void)and512; (void)k; (void)l; (void)m; (void)lt; (void)ne; (void)ge;\n\
         }\n",
    );
    assert!(found.is_empty(), "{found:#?}");
    // A comparison is an integer vector, not the operand's type.
    let found = vector_errors("void f(__m128d a) { __m128d r = a < a; (void)r; }\n");
    assert_eq!(found.len(), 1, "{found:#?}");
}

/// What GCC's extension means but no single instruction does, what GCC
/// refuses, and the vectors without operators: each a diagnostic naming the
/// intrinsic to write.
#[test]
fn vector_operators_without_an_intrinsic_are_refused() {
    let cases: &[(&str, &str)] = &[
        ("__m128i f(__m128i a) { return a * a; }", "_mm_mullo_epi32"),
        ("__m128i f(__m128i a) { return a << 1; }", "_mm_slli_epi64"),
        ("__m128i f(__m128i a) { return a % a; }", "_mm_mullo_epi32"),
        ("__m128i f(__m128i a) { return a == a; }", "_mm_cmpeq_epi64"),
        (
            "__attribute__((target(\"avx512f\"))) __m512i f(__m512d a) { return a < a; }",
            "_mm512_cmp_pd_mask",
        ),
        ("__m128d f(__m128d a) { return ~a; }", "unary operator '~'"),
        (
            "__m128d f(__m128d a) { return a % a; }",
            "floating vector type",
        ),
        (
            "__attribute__((target(\"avx512fp16\"))) __m512h f(__m512h a) { return a + a; }",
            "_mm512_add_ph",
        ),
        (
            "__m128d f(__m128d a, __m128 b) { return a + b; }",
            "different types",
        ),
        (
            "__m128d f(__m128d a, double *p) { return a + p; }",
            "real arithmetic scalar",
        ),
        ("int f(__m128d a) { return !a; }", "'!'"),
        ("int f(__m128d a) { return a && a; }", "'&&'"),
        (
            "__m128d f(__m128d *p, int i) { p[i++] += *p; return *p; }",
            "side effects",
        ),
    ];
    for (source, expected) in cases {
        let found = vector_errors(source);
        assert!(
            found.iter().any(|m| m.contains(expected)),
            "{source}: expected a message containing {expected:?}, got {found:#?}"
        );
    }
}

/// `v[i]` is a lane of the lane type, an lvalue when `v` is one; `{a, b}`
/// initialises a vector lane by lane, wherever the vector stands.
#[test]
fn vector_subscripts_and_braces_are_accepted() {
    let found = vector_errors(
        "__m128d g(void);\n\
         void f(__m128d a, __m128i q, __m128 x, int n) {\n\
             double d = a[0] + a[n];\n\
             long long l = q[1];\n\
             float y = x[3];\n\
             double *p = &a[1];\n\
             a[1] = d; a[0] += 1.0; q[0] ^= l; x[n] = y;\n\
             d = (a * a)[1] + g()[0];\n\
             __m128d v = {1.0, 2}, w = {3.0}, z = {0};\n\
             __m128i i = {1, 2};\n\
             __m128 four = {1, 2, 3, 4};\n\
             struct { int t; __m128d v; } s = {1, {2.0, 3.0}};\n\
             __m128d arr[2] = {{1.0, 2.0}, {3.0, 4.0}};\n\
             __m128d lit = _mm_add_pd((__m128d){1.0, 2.0}, v);\n\
             (void)p; (void)w; (void)z; (void)i; (void)four; (void)s; (void)arr; (void)lit;\n\
         }\n",
    );
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn vector_subscripts_and_braces_are_checked() {
    let cases: &[(&str, &str)] = &[
        (
            "double f(__m128d a) { return a[2]; }",
            "index 2 is out of range for '__m128d', which has 2 lanes",
        ),
        (
            "float f(__m128 a) { return a[-1]; }",
            "index -1 is out of range",
        ),
        ("double f(__m128d a) { return a[0.5]; }", "not an integer"),
        ("double f(__m128d a) { return 0[a]; }", "reversed 'i[v]'"),
        (
            "__m128d f(void) { __m128d v = {1.0, 2.0, 3.0}; return v; }",
            "excess elements",
        ),
        (
            "__m128d f(void) { __m128d v = {[1] = 2.0}; return v; }",
            "designator",
        ),
        // A lane of a vector that is not an object is not an lvalue either.
        (
            "void f(__m128d a, __m128d b) { (a * b)[0] = 1.0; }",
            "expression is not assignable",
        ),
        (
            "void f(__m128d a) { (a + a)[1] += 1.0; }",
            "expression is not assignable",
        ),
        ("void f(__m128d a) { (a + a)[1]++; }", "not assignable"),
    ];
    for (source, expected) in cases {
        let found = vector_errors(source);
        assert!(
            found.iter().any(|m| m.contains(expected)),
            "{source}: expected a message containing {expected:?}, got {found:#?}"
        );
    }
    // A file-scope vector is initialised by a call, which a static cannot be.
    // A static vector is zero-filled, but not given lanes: that is a call to
    // `core::arch`, which a Rust `static` cannot make.
    let found = vector_errors("static __m128d k = {1.0, 2.0};\n");
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0].contains("assign the lanes in a function"),
        "{found:#?}"
    );
    assert!(
        vector_errors("static __m128d zeroed; __m128d *p(void) { return &zeroed; }\n").is_empty()
    );
}

/// GCC's `aligned(N)` on a `typedef` of a scalar makes a variant of the type
/// with alignment N — one byte for xxHash's `xxh_unalign64`. The checks are
/// constant expressions, so a wrong answer is a negative array size.
#[test]
fn an_aligned_typedef_of_a_scalar_has_that_alignment() {
    accepted(
        "typedef __attribute__((__aligned__(1))) __attribute__((__may_alias__)) \
             unsigned long long xxh_unalign64;\n\
         typedef unsigned int __attribute__((aligned(1))) u32_any;\n\
         typedef xxh_unalign64 again;\n\
         typedef double __attribute__((aligned(16))) d16;\n\
         struct S { char c; xxh_unalign64 v; };\n\
         struct T { char c; d16 d; };\n\
         typedef char a1[__alignof__(xxh_unalign64) == 1 ? 1 : -1];\n\
         typedef char a2[__alignof__(u32_any) == 1 ? 1 : -1];\n\
         typedef char a3[__alignof__(again) == 1 ? 1 : -1];\n\
         typedef char a4[sizeof(xxh_unalign64) == 8 ? 1 : -1];\n\
         typedef char a5[__builtin_offsetof(struct S, v) == 1 ? 1 : -1];\n\
         typedef char a6[sizeof(struct S) == 9 ? 1 : -1];\n\
         typedef char a7[__alignof__(d16) == 16 ? 1 : -1];\n\
         typedef char a8[__builtin_offsetof(struct T, d) == 16 ? 1 : -1];\n\
         typedef char a9[sizeof(struct T) == 32 ? 1 : -1];\n\
         unsigned long long read(const void *p) { return *(const xxh_unalign64 *) p; }\n\
         void write(void *p, unsigned long long v) { *(xxh_unalign64 *) p = v; }\n\
         unsigned long long *plain(xxh_unalign64 *p) { return (unsigned long long *) p; }\n",
    );
}

/// What an aligned `typedef` cannot be made correct for yet is refused, and
/// `_Alignas` on a `typedef` stays the constraint violation it is (C11 6.7.5p2).
#[test]
fn what_an_aligned_typedef_cannot_do_is_refused() {
    let mut c11 = Options::new(Standard::C11);
    c11.c_variadic = true;
    assert_eq!(
        errors_with("typedef _Alignas(8) int T;", &c11),
        ["an alignment specifier is not allowed on a 'typedef'"]
    );
    rejected(
        "typedef unsigned long long __attribute__((aligned(2))) u2;\n\
         struct S { char c; u2 v; };",
        &[
            "a member whose 'typedef' is 'aligned(2)', less than its type's 8, is not \
           supported unless the alignment is 1",
        ],
    );
    rejected(
        "typedef unsigned long long __attribute__((aligned(1))) u1;\n\
         struct S { char c; u1 v[2]; };",
        &[
            "a member that is an array of a 'typedef' made 'aligned(1)', less than its \
           type's own alignment, is not supported",
        ],
    );
    rejected(
        "struct P { int a; };\ntypedef struct P P1 __attribute__((aligned(1)));",
        &[
            "'aligned(1)' on a 'typedef' of 'struct P' would make it less aligned than its \
           type, which is supported for a scalar or a pointer 'typedef' only",
        ],
    );
}

// ---------------------------------------------------------------------------
// `long double` at the platform boundary
// ---------------------------------------------------------------------------

/// Options for `triple`, with the switches [`errors`] forces on.
fn options_for(triple: &str) -> Options {
    let target = cinrs_core::target::TargetModel::from_triple(triple).expect("a known triple");
    let mut options = Options::new(Standard::C99).for_target(target);
    options.c_variadic = true;
    options.complex = true;
    options
}

/// The errors `source` gets on `triple`.
fn errors_on(triple: &str, source: &str) -> Vec<String> {
    errors_with(source, &options_for(triple))
}

/// The symbol each declared function of `source` links by on `triple`, as
/// `name=symbol`, for the ones that have one of their own.
fn link_names_on(triple: &str, source: &str) -> Vec<String> {
    let options = options_for(triple);
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let analysis = analyze(input, &options);
    let (program, diagnostics) = sema::analyze(&analysis.unit, &options, analysis.source.unit_id());
    assert!(!diagnostics.has_errors(), "{:#?}", diagnostics.items());
    program
        .functions
        .iter()
        .filter_map(|f| Some(format!("{}={}", f.name, f.asm_label.as_ref()?)))
        .collect()
}

const X86_64_LINUX: &str = "x86_64-unknown-linux-gnu";

/// A declared-only ISO C function whose only difference from a `double`
/// sibling is the type is linked to the sibling, which is what "`long double`
/// is `double`" means; a definition in the unit, and an `__asm__` label, are
/// the program's own and are left alone.
#[test]
fn the_long_double_iso_functions_link_to_their_double_twins() {
    let source = "#include <stdlib.h>\n\
         long double powl(long double, long double);\n\
         long double sinl(long double);\n\
         long double _Complex csinl(long double _Complex);\n\
         double nexttoward(double, long double);\n\
         long double fabsl(long double x) { return x < 0 ? -x : x; }\n\
         long double cosl(long double) __asm__(\"my_cos\");\n\
         double f(void) { return strtold(\"2.5\", 0) + powl(2.0L, 10.0L) + sinl(0) \
         + fabsl(-1.0L) + nexttoward(1.0, 2.0L); }";
    assert_eq!(
        link_names_on(X86_64_LINUX, source),
        [
            "strtold=strtod",
            "powl=pow",
            "sinl=sin",
            "csinl=csin",
            "nexttoward=nextafter",
            "cosl=my_cos",
        ]
    );
    for triple in ["aarch64-unknown-linux-gnu", "i686-unknown-linux-gnu"] {
        assert!(
            link_names_on(triple, source).contains(&"strtold=strtod".to_owned()),
            "for {triple}"
        );
    }
    // Where the platform's `long double` *is* `double`, the twins are still
    // redirected — Microsoft's C runtime has no `powl` or `sinl` symbol at
    // all, only inline wrappers over `pow` and `sin` — but nothing is refused.
    for triple in [
        "x86_64-pc-windows-msvc",
        "i686-pc-windows-msvc",
        "aarch64-pc-windows-msvc",
        "aarch64-apple-darwin",
        "armv7-unknown-linux-gnueabihf",
    ] {
        assert_eq!(
            link_names_on(triple, source),
            [
                "strtold=strtod",
                "powl=pow",
                "sinl=sin",
                "csinl=csin",
                "nexttoward=nextafter",
                "cosl=my_cos",
            ],
            "for {triple}"
        );
        assert!(
            errors_on(
                triple,
                "long double f(long double); int printf(const char *, ...);\n\
                 int g(long double x) { printf(\"%Lf\", x); return f(x) > 0; }"
            )
            .is_empty(),
            "for {triple}"
        );
    }
}

/// An arm a constant condition excludes calls nothing, and GCC diagnoses
/// nothing in it. glibc's `isnan` under `__GNUC__` is `__MATH_TG`, which
/// names all three functions and lets `sizeof` pick one: for a `double` the
/// `__isnanl` arm is dead. So is it for a `long double`, which is eight bytes
/// here and so takes the `double` arm — the right function for the value
/// cinrs passes.
#[test]
fn the_long_double_boundary_ignores_arms_a_constant_condition_excludes() {
    const VALUE: &str = "(which is 'double' here), and the platform passes a 'long double' \
                         on the x87 stack, so the call would read the wrong register; use the \
                         'double' function or wrap it in C compiled by a C compiler";
    let decls = "int __isnanf(float); int __isnan(double); int __isnanl(long double);\n\
                 #define __MATH_TG(TG_ARG, FUNC, ARGS) \\\n\
                 (sizeof (TG_ARG) == sizeof (float) ? FUNC ## f ARGS \\\n\
                 : sizeof (TG_ARG) == sizeof (double) ? FUNC ARGS : FUNC ## l ARGS)\n\
                 #define isnan(x) __MATH_TG ((x), __isnan, (x))\n";
    for arg in ["float", "double", "long double"] {
        assert!(
            errors_on(
                X86_64_LINUX,
                &format!("{decls}int k({arg} x) {{ return isnan(x); }}")
            )
            .is_empty(),
            "for {arg}"
        );
    }
    let refused = format!("'__isnanl' takes a 'long double' {VALUE}");
    // The live arm, and an arm whose condition is only known at run time, are
    // still refused.
    assert_eq!(
        errors_on(
            X86_64_LINUX,
            &format!(
                "{decls}int k(long double x, int c) {{ return (1 ? __isnanl(x) : 0) \
                 + (c ? __isnanl(x) : 0) + (sizeof x == 8 && __isnanl(x)); }}"
            )
        ),
        [refused.clone(), refused.clone(), refused.clone()]
    );
    // `if` with a constant condition, `&&` and `||` with a constant left
    // operand, GNU's `?:` with a true one, and a function designator rather
    // than a call.
    assert!(
        errors_on(
            X86_64_LINUX,
            &format!(
                "{decls}int k(long double x) {{\n\
                 if (0) __isnanl(x);\n\
                 if (sizeof x == 8) ; else {{ int (*p)(long double) = __isnanl; p(x); }}\n\
                 return (0 && __isnanl(x)) + (1 || __isnanl(x)) + (1 ?: __isnanl(x)); }}"
            )
        )
        .is_empty()
    );
    // A label inside the `if (0)` branch is a way in the condition does not
    // guard, so the call there can run.
    assert_eq!(
        errors_on(
            X86_64_LINUX,
            &format!(
                "{decls}int k(long double x) {{ goto in;\n\
                 if (0) {{ in: return __isnanl(x); }} return 0; }}"
            )
        ),
        [refused]
    );
}

/// Everything else with a `long double` in its prototype is refused where it
/// is used — not where it is declared, since the platform's headers declare
/// dozens.
#[test]
fn a_platform_function_with_a_long_double_in_its_prototype_is_refused_where_used() {
    const VALUE: &str = "(which is 'double' here), and the platform passes a 'long double' \
                         on the x87 stack, so the call would read the wrong register; use the \
                         'double' function or wrap it in C compiled by a C compiler";
    assert_eq!(
        errors_on(
            X86_64_LINUX,
            "long double f(long double);\n\
             void g(double, long double);\n\
             void h(long double *);\n\
             long double unused(long double);\n\
             double k(void) { long double x = 1; g(1, 2); h(&x); \
             long double (*p)(long double) = f; return f(x) + p(x); }"
        ),
        [
            format!("'g' takes a 'long double' {VALUE}"),
            "'h' takes a 'long double *' ('long double' is 'double' here, eight bytes), and the \
             platform's function reads or writes sixteen x87 bytes through it; use the 'double' \
             function or wrap it in C compiled by a C compiler"
                .to_owned(),
            format!("'f' returns a 'long double' {VALUE}"),
            format!("'f' returns a 'long double' {VALUE}"),
        ]
    );
    // AArch64 Linux's is a 128-bit quad rather than an x87 value.
    assert_eq!(
        errors_on(
            "aarch64-unknown-linux-gnu",
            "long double f(long double); double k(void) { return f(1); }"
        ),
        [
            "'f' returns a 'long double' (which is 'double' here), and the platform passes a \
             'long double' as a 128-bit quad, so the call would read the wrong register; use \
             the 'double' function or wrap it in C compiled by a C compiler"
        ]
    );
    // Defined further down the unit, it is this unit's own function and takes
    // the `double` its callers pass.
    assert!(
        errors_on(
            X86_64_LINUX,
            "long double f(long double);\n\
             double k(void) { return f(1.0L); }\n\
             long double f(long double x) { return x * 2; }"
        )
        .is_empty()
    );
}

/// A `long double` handed to the variable part of a platform function is read
/// as sixteen x87 bytes, and a pointer to one is written through as sixteen.
#[test]
fn a_long_double_through_the_platforms_ellipsis_is_refused() {
    const PRINTF: &str = "a 'long double' cannot be passed to the platform's 'printf': it is \
                          'double' here and would be read as sixteen x87 bytes; cast it to \
                          'double' and use '%f'";
    assert_eq!(
        errors_on(
            X86_64_LINUX,
            "int printf(const char *, ...);\n\
             int sscanf(const char *, const char *, ...);\n\
             typedef long double ld;\n\
             struct S { long double m; ld a[2]; };\n\
             long double get(void);\n\
             void f(double d, struct S *s) {\n\
               long double x = d, arr[3];\n\
               printf(\"%Lf\", x);\n\
               printf(\"%Lf\", 2.5L);\n\
               printf(\"%Lf\", -2.5L);\n\
               printf(\"%Lf\", (long double)d);\n\
               printf(\"%Lf\", x * d);\n\
               printf(\"%Lf\", s->m);\n\
               printf(\"%Lf\", s->a[1]);\n\
               printf(\"%Lf\", arr[0]);\n\
               sscanf(\"1\", \"%Lf\", &x);\n\
               sscanf(\"1\", \"%Lf\", arr);\n\
             }"
        ),
        [
            PRINTF,
            PRINTF,
            PRINTF,
            PRINTF,
            PRINTF,
            PRINTF,
            PRINTF,
            PRINTF,
            "a 'long double *' cannot be passed to the platform's 'sscanf': 'long double' is \
             'double' here, eight bytes, and 'sscanf' would read or write sixteen x87 bytes \
             through it; pass a 'double *' and use '%lf'",
            "a 'long double *' cannot be passed to the platform's 'sscanf': 'long double' is \
             'double' here, eight bytes, and 'sscanf' would read or write sixteen x87 bytes \
             through it; pass a 'double *' and use '%lf'",
        ]
    );
    // The redirect makes `strtold` a `double` function, but what it returns is
    // still a `long double` to the C program, and to `printf`.
    assert_eq!(
        errors_on(
            X86_64_LINUX,
            "#include <stdlib.h>\nint printf(const char *, ...);\n\
             void f(void) { printf(\"%Lf\", strtold(\"1\", 0)); }"
        ),
        [PRINTF]
    );
}

/// What stays inside the unit is self-consistent, and a cast to `double` is
/// the rewrite the refusal asks for.
#[test]
fn a_long_double_that_stays_in_the_unit_is_accepted() {
    assert!(
        errors_on(
            X86_64_LINUX,
            "#include <stdarg.h>\n\
             int printf(const char *, ...);\n\
             long double sum(int n, ...) {\n\
               va_list ap; va_start(ap, n); long double s = 0;\n\
               for (int i = 0; i < n; i++) s += va_arg(ap, long double);\n\
               va_end(ap); return s;\n\
             }\n\
             void scale(long double *p) { *p *= 2; }\n\
             double f(void) {\n\
               long double x = 1.5L, y = x * x + 2;\n\
               scale(&x);\n\
               printf(\"%f %f %d %p\\n\", (double)x, (double)(x + y), (int)x, (void *)&x);\n\
               return sum(2, x, y);\n\
             }"
        )
        .is_empty()
    );
}

// ---------------------------------------------------------------------------
// TS 18661-3's `_FloatN` types
// ---------------------------------------------------------------------------

/// The errors `source` gets as C11 on x86-64 Linux, where `_Generic` exists
/// and the platform's `long double` is the x87 one.
fn c11_errors(source: &str) -> Vec<String> {
    let mut options = options_for(X86_64_LINUX);
    options.standard = Standard::C11;
    errors_with(source, &options)
}

/// `_Float32` is `float`, `_Float64` and `_Float32x` are `double`, and
/// `_Float64x` is `long double`, which is `double` here — `_Generic` answers
/// the type, and `sizeof` the format.
#[test]
fn the_floatn_types_are_the_types_of_their_format() {
    let found = c11_errors(
        "typedef char f32[_Generic((_Float32)0, float: 1, default: -1)];\n\
         typedef char f64[_Generic((_Float64)0, double: 1, default: -1)];\n\
         typedef char f32x[_Generic((_Float32x)0, double: 1, default: -1)];\n\
         typedef char f64x[_Generic((_Float64x)0, double: 1, default: -1)];\n\
         typedef char c64[_Generic((_Complex _Float64)0, double _Complex: 1, default: -1)];\n\
         typedef char c32[sizeof(_Complex _Float32) == 8 ? 1 : -1];\n\
         _Float32 half(_Float32 x) { return x / 2; }\n\
         _Float64 twice(_Float64 x) { return x * 2; }\n\
         _Float32x add(_Float32x a, _Float32x b) { return a + b; }",
    );
    assert!(found.is_empty(), "{found:#?}");
}

/// glibc's `_Float64x` functions are its `long double` ones, and are twinned
/// the same way; any other declared-only function taking one is refused at
/// its call, as a `long double` one is.
#[test]
fn float64x_crosses_the_platform_boundary_as_long_double() {
    assert_eq!(
        link_names_on(
            X86_64_LINUX,
            "_Float64x strtof64x(const char *, char **);\n\
             _Float64x sinf64x(_Float64x);\n\
             double f(void) { return strtof64x(\"2.5\", 0) + sinf64x(0); }"
        ),
        ["strtof64x=strtod", "sinf64x=sin"]
    );
    let found = c11_errors(
        "void take(_Float64x);\n\
         void f(void) { take(1.0); }",
    );
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0].starts_with("'take' takes a 'long double' (which is 'double' here)"),
        "{found:#?}"
    );
}

/// `_Float128` can be named — in a `typedef`, a pointer, a prototype, a
/// `_Generic` association and `sizeof` — which is all glibc's headers do.
#[test]
fn float128_may_be_named() {
    let found = c11_errors(
        "typedef _Float128 quad;\n\
         typedef __float128 gnu_quad;\n\
         _Float128 strtof128(const char *, char **);\n\
         _Complex _Float128 csqrtf128(_Complex _Float128);\n\
         void keep(quad *p);\n\
         typedef char size[sizeof(_Float128) == 16 && _Alignof(_Float128) == 16 ? 1 : -1];\n\
         typedef char pick[_Generic(1.0, float: -1, default: 1, _Float128: -1)];\n\
         double f(int n) { return 0 ? strtof128(\"1\", 0), 1.0 : 2.0; }",
    );
    assert!(found.is_empty(), "{found:#?}");
}

/// What would make a `_Float128` value is refused, by name, with the reason.
#[test]
fn float128_values_are_refused() {
    const REASON: &str = "binary128 has no Rust type to become (Rust's 'f128' is unstable), \
                          and mapping it onto 'double' would compute and pass the wrong values";
    assert_eq!(
        c11_errors("_Float128 q;"),
        [format!(
            "'q' cannot be an object of type '_Float128': {REASON}"
        )]
    );
    assert_eq!(
        c11_errors("double f(double x) { return (double)(_Float128)x; }"),
        [format!("a cast to '_Float128' is not supported: {REASON}")]
    );
    assert_eq!(
        c11_errors(
            "_Float128 strtof128(const char *, char **);\n\
             void f(void) { strtof128(\"1\", 0); }"
        ),
        [format!(
            "'strtof128' returns '_Float128', which cannot be called: {REASON}"
        )]
    );
    assert_eq!(
        c11_errors(
            "int strfromf128(char *, unsigned long, const char *, _Float128);\n\
             void f(char *s) { strfromf128(s, 8, \"%g\", 0); }"
        ),
        [format!(
            "'strfromf128' takes '_Float128', which cannot be called: {REASON}"
        )]
    );
}

/// GCC makes `_Float32` a type distinct from `float`, and glibc's `<math.h>`
/// lists both in one `_Generic`; here they are one type, so the second is
/// dropped. Two associations for one type spelled the standard way are still
/// an error.
#[test]
fn a_floatn_association_may_repeat_its_standard_twin() {
    let found = c11_errors(
        "typedef char a[_Generic(1.0f, float: 1, _Float32: -1, default: -1)];\n\
         typedef char b[_Generic(1.0f, _Float32: 1, float: -1, default: -1)];\n\
         typedef char c[_Generic(1.0L, long double: 1, _Float64x: -1, default: -1)];",
    );
    assert!(found.is_empty(), "{found:#?}");
    assert_eq!(
        c11_errors("int x = _Generic(1.0, double: 1, long double: 2);"),
        ["'_Generic' has two associations for the compatible type 'double'"]
    );
}

// ---------------------------------------------------------------------------
// implicit declarations of the C library's built-in functions
// ---------------------------------------------------------------------------

/// Analyses `source` under `options`, returning the program and every
/// diagnostic as `level: message`.
fn analysed(source: &str, options: &Options) -> (ir::Program, Vec<String>) {
    let literal = format!("r#####\"{source}\"#####");
    let input = TokenStream::from_str(&literal).expect("the wrapper must lex");
    let analysis = analyze(input, options);
    assert!(
        analysis.diagnostics.items().is_empty(),
        "front end: {:#?}",
        analysis.diagnostics.items()
    );
    let (program, diagnostics) = sema::analyze(&analysis.unit, options, analysis.source.unit_id());
    let messages = diagnostics
        .sorted()
        .into_iter()
        .map(|d| {
            let level = if d.level == Level::Error {
                "error"
            } else {
                "warning"
            };
            format!("{level}: {}", d.message)
        })
        .collect();
    (program, messages)
}

/// The signature of the function called `name`.
fn signature_of(program: &ir::Program, name: &str) -> ir::Signature {
    program
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no function '{name}'"))
        .sig
        .clone()
}

#[test]
fn an_implicit_declaration_of_a_library_function_has_its_prototype() {
    let source = "int f(char *p) {\n\
                  \x20   char *q = strcpy(p, \"x\");\n\
                  \x20   void *m = memcpy(q, p, 2);\n\
                  \x20   printf(\"%d\", strcmp(p, q));\n\
                  \x20   return m != 0;\n\
                  }";
    let (program, messages) = analysed(source, &Options::gnu(Standard::C89));
    // GCC's wording, and only where `int ()` is not compatible with the real
    // type: `strcmp` returns `int` and takes what its arguments promote to.
    assert_eq!(
        messages,
        [
            "warning: incompatible implicit declaration of built-in function 'strcpy'",
            "warning: incompatible implicit declaration of built-in function 'memcpy'",
            "warning: incompatible implicit declaration of built-in function 'printf'",
        ]
    );
    let strcpy = signature_of(&program, "strcpy");
    assert!(strcpy.prototyped && strcpy.ret.is_pointer() && strcpy.params.len() == 2);
    let strcmp = signature_of(&program, "strcmp");
    assert!(strcmp.prototyped && strcmp.ret == ir::Ty::Int && strcmp.params.len() == 2);
    let memcpy = signature_of(&program, "memcpy");
    assert!(memcpy.prototyped && memcpy.params.len() == 3);
    let printf = signature_of(&program, "printf");
    assert!(printf.prototyped && printf.variadic && printf.params.len() == 1);
    // Strict C89 has the ISO functions as built-ins too.
    let (program, messages) = analysed(source, &Options::new(Standard::C89));
    assert_eq!(messages.len(), 3, "{messages:#?}");
    assert!(signature_of(&program, "strcmp").prototyped);
}

#[test]
fn a_name_that_is_no_library_builtin_is_still_int_with_no_prototype() {
    let (program, messages) = analysed(
        "int f(void) { return helper(1) + index(\"a\", 'a'); }",
        &Options::new(Standard::C89),
    );
    assert!(messages.is_empty(), "{messages:#?}");
    let helper = signature_of(&program, "helper");
    assert!(!helper.prototyped && helper.ret == ir::Ty::Int && helper.params.is_empty());
    // `index` is a GNU built-in, and `-std=c89` does not have it.
    assert!(!signature_of(&program, "index").prototyped);
    let (program, _) = analysed(
        "int f(void) { return index(\"a\", 'a') != 0; }",
        &Options::gnu(Standard::C89),
    );
    assert!(signature_of(&program, "index").prototyped);
}

#[test]
fn a_later_declaration_of_an_implicit_library_function_merges() {
    let (program, messages) = analysed(
        "int f(char *p) { strcpy(p, \"x\"); return strcmp(p, \"x\"); }\n\
         #include <string.h>\n\
         char *strcpy();\n\
         int strcmp(const char *, const char *);\n\
         int g(char *p) { return strcmp(strcpy(p, \"y\"), \"y\"); }",
        &Options::gnu(Standard::C89),
    );
    assert_eq!(
        messages,
        ["warning: incompatible implicit declaration of built-in function 'strcpy'"]
    );
    assert!(signature_of(&program, "strcpy").prototyped);
    // A program that goes on to define one of them has its own function.
    let (program, messages) = analysed(
        "int f(char *p) { return strcmp(p, \"x\"); }\n\
         int strcmp(a, b) char *a; char *b; { return *a - *b; }",
        &Options::gnu(Standard::C89),
    );
    assert!(messages.is_empty(), "{messages:#?}");
    assert!(!signature_of(&program, "strcmp").prototyped);
}

#[test]
fn a_declaration_with_no_prototype_takes_the_library_one() {
    let (program, messages) = analysed(
        "char *strcpy(); int strcmp(); char *malloc();\n\
         int f(char *p) { return strcmp(strcpy(p, \"x\"), \"x\"); }",
        &Options::new(Standard::C99),
    );
    assert!(messages.is_empty(), "{messages:#?}");
    assert!(signature_of(&program, "strcpy").prototyped);
    assert!(signature_of(&program, "strcmp").prototyped);
    // `void *malloc(size_t)` is not what this says, so it stays as written.
    assert!(!signature_of(&program, "malloc").prototyped);
    // Nor does a prototype of the program's own change.
    let (program, messages) = analysed(
        "int strcmp(char *, char *); int strcmp();\n\
         int f(char *p) { return strcmp(p, p); }",
        &Options::new(Standard::C99),
    );
    assert!(messages.is_empty(), "{messages:#?}");
    let strcmp = signature_of(&program, "strcmp");
    assert!(strcmp.prototyped && !program.types.points_to_const(strcmp.params[0]));
}

#[test]
fn an_implicit_library_function_is_still_an_error_in_c99() {
    assert_eq!(
        errors("int f(char *p) { return strcmp(p, \"x\"); }"),
        ["implicit declaration of function 'strcmp' is invalid in C99"]
    );
    assert_eq!(
        errors_with(
            "int f(char *p) { return strcmp(p, \"x\"); }",
            &Options::gnu(Standard::C11)
        ),
        ["implicit declaration of function 'strcmp' is invalid in C99"]
    );
}
