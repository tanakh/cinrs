//! Old-style (K&R) function definitions, implicit `int` and implicit function
//! declarations.
//!
//! `int f(a, b) int a; char *b; { … }` is the way C was written before 1989
//! and is still valid, if obsolescent, C99 — C23 is the revision that removed
//! it (N2432), so every entry point below `c23!` has it. The two *rules* it
//! usually comes with are C89's alone, and live in `c89!` and `gnu89!`: a
//! declaration with no type specifier is an `int`, and a call to a function
//! nobody declared declares one.
//!
//! The definition's own parameters have their declared types, while a caller
//! passes the *promoted* ones (6.9.1p7, 6.5.2.2p6), so the generated item
//! takes `int` for a `char` parameter and converts on entry. That is what most
//! of this file is checking.

use cinrs::{c89, c90, c99, gnu89, gnu99};

// ---------------------------------------------------------------------------
// the definition form, in an entry point that is not C89
// ---------------------------------------------------------------------------

c99! {
    /* Obsolescent, not removed: `c99!` takes it. */
    int knr_add(a, b)
        int a;
        int b;
    {
        return a + b;
    }

    /* The parameters keep their declared types; the caller passes the
       promoted ones, so the generated item takes `int`, `int` and `double`
       and narrows on entry. */
    int knr_narrow(c, s, f)
        char c;
        short s;
        float f;
    {
        c = c + 1;
        s = s * 2;
        f = f / 2.0f;
        return c + s + (int)f;
    }

    /* `register` is the one storage class a parameter may have. */
    long knr_register(n)
        register long n;
    {
        register long total = 0;
        while (n > 0) { total += n; n--; }
        return total;
    }

    /* An array parameter is adjusted to a pointer here as anywhere else. */
    int knr_sum(a, n)
        int a[];
        int n;
    {
        int total = 0;
        int i;
        for (i = 0; i < n; i++) total += a[i];
        return total;
    }
}

#[test]
fn a_definition_with_an_identifier_list_works() {
    assert_eq!(unsafe { knr_add(2, 3) }, 5);
    assert_eq!(unsafe { knr_register(4) }, 10);
    let mut a = [1, 2, 3, 4];
    assert_eq!(unsafe { knr_sum(a.as_mut_ptr(), 4) }, 10);
}

/// The parameters are converted to their declared types on entry, so a value
/// that does not fit one of them is truncated exactly as an assignment to it
/// would be.
#[test]
fn the_parameters_are_narrowed_on_entry() {
    // 300 does not fit a `char`: it arrives as an `int` and is stored as one
    // byte, which is 44 (300 - 256). The body then adds 1 to it, doubles the
    // `short` and halves the `float`.
    assert_eq!(unsafe { knr_narrow(300, 4, 6.0) }, 45 + 8 + 3);
    assert_eq!(unsafe { knr_narrow(1, 2, 3.0) }, 2 + 4 + 1);
}

c99! {
    /* A body that jumps is lowered through a control-flow graph, where every
       local is hoisted to the top — so the entry conversion has to survive
       being hoisted with them. */
    int knr_goto(c, n)
        char c;
        int n;
    {
        int total = 0;
    again:
        total += c;
        n--;
        if (n > 0) goto again;
        return total;
    }
}

#[test]
fn the_entry_conversion_survives_the_cfg_lowering() {
    // 300 truncates to 44 in a `char`, three times over.
    assert_eq!(unsafe { knr_goto(300, 3) }, 44 * 3);
}

// ---------------------------------------------------------------------------
// the type has no prototype
// ---------------------------------------------------------------------------

c99! {
    /* C99 6.9.1p7: the definition's type has no prototype, so this pair is
       one function — the declaration says nothing about the parameters and
       the definition's are already their own promoted forms. */
    int knr_paired();

    int knr_paired(n)
        int n;
    {
        return n * 3;
    }

    int knr_call_paired(void) { return knr_paired(7); }

    /* A `char` parameter is *not* its own promoted form, so `int g(char);`
       and this definition would be a diagnostic — the compatible declaration
       is the one with no prototype at all. */
    int knr_char();

    int knr_char(c)
        char c;
    {
        return c + 1;
    }

    int knr_call_char(void) { return knr_char('A'); }
}

#[test]
fn a_definition_pairs_with_an_unprototyped_declaration() {
    assert_eq!(unsafe { knr_call_paired() }, 21);
    assert_eq!(unsafe { knr_paired(2) }, 6);
    assert_eq!(unsafe { knr_call_char() }, 'B' as i32);
}

// ---------------------------------------------------------------------------
// C89: implicit `int`, implicit declarations, and calls before the definition
// ---------------------------------------------------------------------------

c89! {
    /* A declaration with no type specifier declares an `int` (C89 6.5.2). */
    static counter;
    extern int c89_shared;
    int c89_shared = 4;
    const limit = 10;

    /* And so does a function definition with none, which is how nearly every
       program of the period wrote `main`. */
    bump()
    {
        counter = counter + 1;
        return counter;
    }

    /* A call to a function nothing has declared declares `extern int f();`
       from that point on (C89 6.3.2.2), so this call reaches the definition
       below — and the arguments get the default argument promotions. */
    int c89_early(void)
    {
        return twice(21) + counter + c89_shared + limit;
    }

    twice(n)
        int n;
    {
        return n * 2;
    }
}

#[test]
fn c89_has_implicit_int_and_implicit_declarations() {
    unsafe {
        assert_eq!(bump(), 1);
        assert_eq!(bump(), 2);
        // 42 + counter(2) + 4 + 10
        assert_eq!(c89_early(), 58);
    }
}

c90! {
    /* `c90!` is the same entry point under the other name the language has. */
    c90_answer()
    {
        return 42;
    }
}

#[test]
fn c90_is_c89() {
    assert_eq!(unsafe { c90_answer() }, 42);
}

/// A library function nothing declared is `extern int f();` and is resolved by
/// the linker, which is how a C89 program called `abs` and `strlen`.
#[test]
fn an_implicitly_declared_library_function_links() {
    c89! {
        #pragma cinrs module "libc_by_implication"

        int magnitudes()
        {
            /* No <stdlib.h>, no prototype: the call declares `abs`. */
            return abs(-7) + abs(3);
        }

        int length(s)
            char *s;
        {
            /* `strlen` really returns a `size_t`; an implicit declaration
               says `int`, which is what a program of the period assumed and
               what its low half really holds. */
            return (int)strlen(s) + atoi("100");
        }
    }

    unsafe {
        assert_eq!(libc_by_implication::magnitudes(), 10);
        assert_eq!(
            libc_by_implication::length(c"cinrs".as_ptr() as *mut _),
            105
        );
    }
}

/// A parameter the declaration list leaves out is an `int` — implicit `int`
/// again, and therefore C89's alone.
#[test]
fn a_parameter_with_no_declaration_is_an_int() {
    c89! {
        #pragma cinrs module "default_int_params"

        scale(n, factor)
            int factor;              /* `n` is not declared: it is an `int` */
        {
            return n * factor;
        }
    }

    assert_eq!(unsafe { default_int_params::scale(6, 7) }, 42);
}

// ---------------------------------------------------------------------------
// `main(argc, argv)`, the shape every K&R-era program opens with
// ---------------------------------------------------------------------------

gnu89! {
    #pragma cinrs module "old_main"

    main(argc, argv)
        int argc;
        char **argv;
    {
        if (argc < 1 || argv == 0) {
            return 1;
        }
        return argc - 1;
    }
}

#[test]
fn main_may_be_written_the_old_way() {
    let program = c"prog";
    let mut argv = [
        program.as_ptr() as *mut core::ffi::c_char,
        core::ptr::null_mut(),
    ];
    assert_eq!(unsafe { old_main::main(1, argv.as_mut_ptr()) }, 0);
}

// ---------------------------------------------------------------------------
// a whole program in the old idiom
// ---------------------------------------------------------------------------

gnu89! {
    #pragma cinrs module "old_idiom"

    /* No prototypes anywhere, `register` on everything worth it, implicit
       `int` on the ones that return one, and the helpers declared by being
       called. This is what C looked like. */
    struct point {
        int x;
        int y;
    };

    static distance(p)
        struct point *p;
    {
        register int dx, dy;
        dx = p->x < 0 ? -p->x : p->x;
        dy = p->y < 0 ? -p->y : p->y;
        return dx + dy;
    }

    farthest(points, n)
        struct point points[];
        register int n;
    {
        register int i;
        int best;
        int here;
        best = 0;
        for (i = 0; i < n; i++) {
            here = distance(&points[i]);
            if (here > best) best = here;
        }
        return best;
    }

    total(points, n)
        struct point points[];
        int n;
    {
        register int i, sum;
        sum = 0;
        for (i = 0; i < n; i++) sum += distance(&points[i]);
        return sum + widest(points, n);
    }

    static widest(points, n)
        struct point points[];
        int n;
    {
        register int i;
        int best = 0;
        for (i = 0; i < n; i++) {
            if (points[i].x > best) best = points[i].x;
        }
        return best;
    }
}

#[test]
fn a_program_in_the_old_idiom_runs() {
    let mut points = [
        old_idiom::point { x: 3, y: -4 },
        old_idiom::point { x: 1, y: 1 },
        old_idiom::point { x: -8, y: 2 },
    ];
    unsafe {
        assert_eq!(old_idiom::farthest(points.as_mut_ptr(), 3), 10);
        // 7 + 2 + 10, plus the widest x, which is 3.
        assert_eq!(old_idiom::total(points.as_mut_ptr(), 3), 19 + 3);
    }
}

// ---------------------------------------------------------------------------
// the GNU dialect of C89 is C89's rules with everything else switched on
// ---------------------------------------------------------------------------

gnu89! {
    /* Everything a later revision added is accepted here, exactly as
       `gcc -std=gnu89` accepts it — and the three C89 rules are still C89's. */
    long long gnu89_wide(void) { return 1LL << 40; }   // a `//` comment, too

    int gnu89_mixed(void)
    {
        int a = 1;
        a++;
        int b = 2;                        /* mixed declarations and code */
        for (int i = 0; i < 3; i++) b++;
        return a + b;
    }

    struct gnu89_pair { int a; int b; };

    int gnu89_designated(void)
    {
        struct gnu89_pair p = { .b = 7 };
        return p.a + p.b;
    }

    gnu89_implicit(n)                     /* implicit `int`, K&R parameters */
    {
        return n + (int)(gnu89_wide() / (1 << 20));
    }
}

#[test]
fn gnu89_is_gnu99_plus_the_c89_rules() {
    unsafe {
        assert_eq!(gnu89_wide(), 1 << 40);
        assert_eq!(gnu89_mixed(), 2 + 5);
        assert_eq!(gnu89_designated(), 7);
        assert_eq!(gnu89_implicit(1), 1 + (1 << 20));
    }
}

// ---------------------------------------------------------------------------
// and the GNU dialect of C99 still has the definition form
// ---------------------------------------------------------------------------

gnu99! {
    int gnu99_knr(a, b)
        unsigned char a;
        double b;
    {
        return a + (int)b;
    }
}

#[test]
fn a_gnu99_block_takes_the_definition_form_too() {
    assert_eq!(unsafe { gnu99_knr(200, 2.5) }, 202);
}
