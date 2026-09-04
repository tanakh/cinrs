//! What semantic analysis accepts, and what it says about the rest.
//!
//! The `.stderr` files under `tests/ui` pin down where a caret lands; this
//! file pins down the *wording*, which is cheaper to cover exhaustively here.

use std::str::FromStr;

use cinrs_core::{Level, Options, Standard, analyze, sema};
use proc_macro2::TokenStream;

/// Analyses `source` and returns every error message, in source order.
///
/// `c_variadic` is forced on: what the *language* rules say must not depend on
/// which toolchain runs the test, and the diagnostics of an older one have a
/// test of their own.
fn errors(source: &str) -> Vec<String> {
    let mut options = Options::new(Standard::C99);
    options.c_variadic = true;
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
    accepted("typedef double _Complex cplx;");
    rejected(
        "typedef double _Complex cplx; int f(void) { cplx z; return 0; }",
        &["complex types are not supported"],
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
        "int f(void) { return 0; } int g(void) { double _Complex z; return 0; }",
        &["complex types are not supported"],
    );
}

#[test]
fn constructs_that_are_still_out_of_reach_are_named() {
    rejected(
        "struct S { unsigned int flag : 1; };",
        &["bit-fields are not supported yet; declare the members as whole integers instead"],
    );
    rejected(
        "int f(int n) { int a[n]; return a[0]; }",
        &[
            "variable length arrays are not supported yet; the bound of an array must be \
             an integer constant expression",
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
    rejected(
        "int f(void) { int a[]; return 0; }",
        &["definition of variable 'a' with array type needs an explicit size or an initializer"],
    );
    rejected(
        "int n; int a[n];",
        &["array size is not an integer constant expression"],
    );
    rejected(
        "int f(void) { char s[2] = \"long\"; return s[0]; }",
        &["initializer-string for char array is too long"],
    );
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
    rejected(
        "struct S { struct T { int a; } t; }; struct S s = {.t.a = 1};",
        &[
            "a designator naming a nested member is not supported yet; write nested braces \
             instead",
        ],
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
fn old_style_definitions_are_not_supported() {
    rejected(
        "int f(a, b) int a; int b; { return a + b; }",
        &["old-style (K&R) function definitions are not supported; write a prototype instead"],
    );
}

#[test]
fn extern_objects_are_declared_not_defined() {
    accepted("extern int shared; int read_it(void) { return shared; }");
    accepted("extern int shared; int shared = 1; int read_it(void) { return shared; }");
    rejected(
        "extern int shared = 1;",
        &["'shared' is declared 'extern' and cannot have an initializer here"],
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
    // The value is Rust's to compute, so it is not a constant expression here.
    rejected(
        "#include <stddef.h>
         struct S { char a; int b; };
         static size_t where = offsetof(struct S, b);",
        &["initializer is not a compile-time constant expression"],
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
    va_rejected(
        "struct S { int x; };
         int f(int n, ...) { va_list ap; va_start(ap, n); return va_arg(ap, struct S).x; }",
        &["va_arg with a struct type is not supported yet"],
    );
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
    va_rejected(
        "int f(va_list *ap);",
        &["pointers to va_list are not supported yet"],
    );
    va_rejected(
        "void g(int *p);
         int f(int n, ...) { va_list ap; va_start(ap, n); g(&ap); return 0; }",
        &["pointers to va_list are not supported yet"],
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
