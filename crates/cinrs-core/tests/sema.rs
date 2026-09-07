//! What semantic analysis accepts, and what it says about the rest.
//!
//! The `.stderr` files under `tests/ui` pin down where a caret lands; this
//! file pins down the *wording*, which is cheaper to cover exhaustively here.

use std::str::FromStr;

use cinrs_core::{Level, Options, Standard, analyze, sema};
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
