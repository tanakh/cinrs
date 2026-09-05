//! The GNU language extensions, exercised the way real C uses them.
//!
//! `doc/gnu-extensions.md` is the catalogue; this file is the proof. Each test
//! is written the way the extension is actually met in the wild — the kernel's
//! `min`/`max`/`container_of`, `likely`/`unlikely`, a lexer's `case` ranges, a
//! protocol header — rather than as the smallest thing that exercises the
//! parser.

use cinrs::{c11, c99, gnu99};

// ---------------------------------------------------------------------------
// alternate keywords and `__extension__`
// ---------------------------------------------------------------------------

c99! {
    /* Every one of these is spelled with a double underscore, so it is
     * available in a strict entry point exactly as it is in GCC's. */
    __inline__ int alt_double(__const__ int n) { return n * 2; }

    __extension__ int alt_extension(int n) {
        __extension__ int m = __extension__ (n + 1);
        return m;
    }

    int alt_qualifiers(__const int *__restrict__ p, __volatile__ int *q) {
        *q = *p;
        return *q;
    }

    unsigned long alt_alignof(void) { return __alignof__(double); }

    __typeof__(int) alt_typeof(__typeof(long) n) {
        __typeof__(n) doubled = n * 2;
        return (int) doubled;
    }

    __signed int alt_signed(__signed__ char c) { return c; }
}

#[test]
fn the_double_underscore_keywords_work_in_a_strict_block() {
    assert_eq!(unsafe { alt_double(21) }, 42);
    assert_eq!(unsafe { alt_extension(41) }, 42);
    let (p, mut q) = (7, 0);
    assert_eq!(unsafe { alt_qualifiers(&raw const p, &raw mut q) }, 7);
    assert_eq!(unsafe { alt_alignof() }, align_of::<f64>() as u64);
    assert_eq!(unsafe { alt_typeof(21) }, 42);
    assert_eq!(unsafe { alt_signed(-3) }, -3);
}

// ---------------------------------------------------------------------------
// statement expressions
// ---------------------------------------------------------------------------

c99! {
    /* The canonical use: a macro that evaluates each operand exactly once. */
    #define max(a, b) ({ __typeof__(a) _a = (a); __typeof__(b) _b = (b); _a > _b ? _a : _b; })
    #define min(a, b) ({ __typeof__(a) _a = (a); __typeof__(b) _b = (b); _a < _b ? _a : _b; })

    static int calls;
    static int bump(int n) { calls++; return n; }

    int stmt_max(int a, int b) { return max(a, b); }
    int stmt_min(int a, int b) { return min(a, b); }

    int stmt_once(void) {
        calls = 0;
        int m = max(bump(3), bump(4));
        return m * 100 + calls;
    }

    /* Declarations, nesting and loops inside one. */
    int stmt_nested(int n) {
        return ({
            int total = 0;
            for (int i = 0; i < n; i++) {
                total += ({ int square = i * i; square; });
            }
            total;
        });
    }

    /* A statement expression with no value at all is `void`. */
    void stmt_void(int *out) { ({ *out = 5; }); }

    /* `return` inside one leaves the whole function. */
    int stmt_return(int n) {
        int v = ({ if (n < 0) return -1; n * 2; });
        return v;
    }

    /* And one that `break`s out of the loop around it. */
    int stmt_break(int n) {
        int sum = 0;
        for (int i = 0; i < n; i++) {
            sum += ({ if (i == 3) break; i; });
        }
        return sum;
    }
}

#[test]
fn statement_expressions_have_the_value_of_their_last_expression() {
    assert_eq!(unsafe { stmt_max(3, 9) }, 9);
    assert_eq!(unsafe { stmt_min(3, 9) }, 3);
    // Each operand is evaluated once: the result is 4 and `bump` ran twice.
    assert_eq!(unsafe { stmt_once() }, 402);
    assert_eq!(unsafe { stmt_nested(4) }, 1 + 4 + 9);
    let mut out = 0;
    unsafe { stmt_void(&raw mut out) };
    assert_eq!(out, 5);
    assert_eq!(unsafe { stmt_return(-1) }, -1);
    assert_eq!(unsafe { stmt_return(21) }, 42);
    assert_eq!(unsafe { stmt_break(10) }, 1 + 2);
}

// The kernel's `container_of`, which needs a statement expression, `typeof`
// and `offsetof` together. A backslash line continuation is one of the things
// Rust's own lexer refuses, so the block is written as a string literal.
c99! { r#"
    #include <stddef.h>
    #define container_of(ptr, type, member) ({ \
        const __typeof__(((type *)0)->member) *__mptr = (ptr); \
        (type *)((char *)__mptr - offsetof(type, member)); })

    struct Node { int id; struct Node *next; int payload; };

    int payload_of(int *id_field) {
        struct Node *node = container_of(id_field, struct Node, id);
        return node->payload;
    }
"# }

#[test]
fn container_of_finds_its_way_back_to_the_struct() {
    let mut node = Node {
        id: 7,
        next: core::ptr::null_mut(),
        payload: 99,
    };
    assert_eq!(unsafe { payload_of(&raw mut node.id) }, 99);
}

// ---------------------------------------------------------------------------
// `?:` with the middle operand omitted
// ---------------------------------------------------------------------------

c99! {
    static int elvis_calls;
    static int elvis_side(int n) { elvis_calls++; return n; }

    int elvis(int a, int b) { return a ?: b; }

    int elvis_once(int n) {
        elvis_calls = 0;
        int v = elvis_side(n) ?: 42;
        return v * 10 + elvis_calls;
    }

    char *elvis_pointer(char *p, char *fallback) { return p ?: fallback; }
}

#[test]
fn the_middle_operand_may_be_left_out() {
    assert_eq!(unsafe { elvis(0, 7) }, 7);
    assert_eq!(unsafe { elvis(3, 7) }, 3);
    // Evaluated once whichever way it goes.
    assert_eq!(unsafe { elvis_once(5) }, 51);
    assert_eq!(unsafe { elvis_once(0) }, 421);
    let mut fallback = *b"fallback\0";
    let p = unsafe { elvis_pointer(core::ptr::null_mut(), fallback.as_mut_ptr().cast()) };
    assert_eq!(p, fallback.as_mut_ptr().cast());
}

// ---------------------------------------------------------------------------
// `?:` with one `void` operand
// ---------------------------------------------------------------------------

c99! {
    static int void_calls;
    static int void_side(int n) { void_calls++; return n; }

    /* Both operands `void` is ISO C; one of each is GCC's, in every mode it
     * has, and the other operand's value is simply discarded. */
    int void_both(int c) {
        void_calls = 0;
        c ? (void) void_side(1) : (void) void_side(2);
        return void_calls;
    }

    int void_one(int c) {
        void_calls = 0;
        c ? (void) 0 : (void) void_side(1);
        return void_calls;
    }

    /* Written where a value is expected: the left operand of a comma, and
     * inside one. */
    int void_in_comma(int c) {
        void_calls = 0;
        int v = (c ? (void) 0 : void_side(1), 7);
        return v * 10 + void_calls;
    }
}

#[test]
fn a_conditional_may_have_one_void_operand() {
    assert_eq!(unsafe { void_both(1) }, 1);
    assert_eq!(unsafe { void_both(0) }, 1);
    assert_eq!(unsafe { void_one(1) }, 0);
    assert_eq!(unsafe { void_one(0) }, 1);
    assert_eq!(unsafe { void_in_comma(1) }, 70);
    assert_eq!(unsafe { void_in_comma(0) }, 71);
}

// ---------------------------------------------------------------------------
// `__func__` and its GNU spellings
// ---------------------------------------------------------------------------

c99! {
    #include <string.h>

    const char *whoami(void) { return __func__; }
    const char *whoami_gnu(void) { return __FUNCTION__; }
    const char *whoami_pretty(void) { return __PRETTY_FUNCTION__; }

    unsigned long name_length(void) { return sizeof(__func__); }

    /* It is in scope in every block, however deeply nested. */
    int nested_func_name(void) {
        {
            {
                return (int) strlen(__func__);
            }
        }
    }
}

#[test]
fn func_names_the_function_it_is_written_in() {
    let name = |p: *const core::ffi::c_char| unsafe { core::ffi::CStr::from_ptr(p) };
    assert_eq!(name(unsafe { whoami() }).to_bytes(), b"whoami");
    assert_eq!(name(unsafe { whoami_gnu() }).to_bytes(), b"whoami_gnu");
    assert_eq!(
        name(unsafe { whoami_pretty() }).to_bytes(),
        b"whoami_pretty"
    );
    // `sizeof` sees the array, not the pointer.
    assert_eq!(unsafe { name_length() }, "name_length".len() as u64 + 1);
    assert_eq!(
        unsafe { nested_func_name() },
        "nested_func_name".len() as i32
    );
}

// ---------------------------------------------------------------------------
// case ranges
// ---------------------------------------------------------------------------

c99! {
    /* The classic use: a lexer's character classes. */
    enum Class { CLASS_OTHER, CLASS_DIGIT, CLASS_ALPHA, CLASS_SPACE };

    int classify(int c) {
        switch (c) {
        case '0' ... '9':
            return CLASS_DIGIT;
        case 'a' ... 'z':
        case 'A' ... 'Z':
        case '_':
            return CLASS_ALPHA;
        case ' ':
        case '\t' ... '\r':
            return CLASS_SPACE;
        default:
            return CLASS_OTHER;
        }
    }

    /* And the same through a `goto`, which is the control-flow-graph form. */
    int classify_cfg(int c) {
        int result;
        switch (c) {
        case '0' ... '9': result = CLASS_DIGIT; goto done;
        case 'a' ... 'z': result = CLASS_ALPHA; goto done;
        default: result = CLASS_OTHER; goto done;
        }
    done:
        return result;
    }
}

#[test]
fn case_ranges_label_every_value_between() {
    for (c, want) in [
        (b'0', CLASS_DIGIT),
        (b'5', CLASS_DIGIT),
        (b'9', CLASS_DIGIT),
        (b'a', CLASS_ALPHA),
        (b'Z', CLASS_ALPHA),
        (b'_', CLASS_ALPHA),
        (b' ', CLASS_SPACE),
        (b'\n', CLASS_SPACE),
        (b'+', CLASS_OTHER),
    ] {
        assert_eq!(unsafe { classify(c.into()) }, want, "classifying {c:?}");
    }
    assert_eq!(unsafe { classify_cfg(b'7'.into()) }, CLASS_DIGIT);
    assert_eq!(unsafe { classify_cfg(b'q'.into()) }, CLASS_ALPHA);
    assert_eq!(unsafe { classify_cfg(b'!'.into()) }, CLASS_OTHER);
}

// ---------------------------------------------------------------------------
// designated-initializer ranges and the old designator syntax
// ---------------------------------------------------------------------------

c99! {
    static const char lower[256] = {
        ['A' ... 'Z'] = 1,
        ['a' ... 'z'] = 2,
        [0] = 3,
    };

    int class_of(int c) { return lower[c]; }

    struct Point { int x; int y; };

    /* The pre-C99 designator spelling GNU still accepts. */
    static struct Point origin = { x: 3, y: 4 };
    int origin_sum(void) { return origin.x + origin.y; }
}

#[test]
fn range_designators_fill_the_whole_range() {
    assert_eq!(unsafe { class_of('A' as i32) }, 1);
    assert_eq!(unsafe { class_of('M' as i32) }, 1);
    assert_eq!(unsafe { class_of('Z' as i32) }, 1);
    assert_eq!(unsafe { class_of('a' as i32) }, 2);
    assert_eq!(unsafe { class_of('z' as i32) }, 2);
    assert_eq!(unsafe { class_of(0) }, 3);
    assert_eq!(unsafe { class_of('+' as i32) }, 0);
    assert_eq!(unsafe { origin_sum() }, 7);
}

// ---------------------------------------------------------------------------
// flexible array members and zero-length arrays
// ---------------------------------------------------------------------------

c99! {
    #include <stdlib.h>
    #include <string.h>

    struct Buffer { int len; int data[]; };
    struct Legacy { int len; int data[0]; };

    struct Buffer *buffer_new(int n) {
        struct Buffer *b = malloc(sizeof(struct Buffer) + (unsigned long) n * sizeof(int));
        b->len = n;
        for (int i = 0; i < n; i++) b->data[i] = i * i;
        return b;
    }

    int buffer_sum(struct Buffer *b) {
        int total = 0;
        for (int i = 0; i < b->len; i++) total += b->data[i];
        return total;
    }

    void buffer_free(struct Buffer *b) { free(b); }

    unsigned long buffer_size(void) { return sizeof(struct Buffer); }
    unsigned long legacy_size(void) { return sizeof(struct Legacy); }
}

#[test]
fn a_flexible_array_member_costs_nothing_and_indexes_past_the_end() {
    // `sizeof` leaves the member out, exactly as C says.
    assert_eq!(unsafe { buffer_size() }, 4);
    assert_eq!(unsafe { legacy_size() }, 4);
    let b = unsafe { buffer_new(5) };
    assert_eq!(unsafe { buffer_sum(b) }, 1 + 4 + 9 + 16);
    unsafe { buffer_free(b) };
}

// ---------------------------------------------------------------------------
// empty records, incomplete enums, function pointers and `void *`
// ---------------------------------------------------------------------------

c99! {
    struct Empty {};
    union Nothing {};

    unsigned long empty_size(void) { return sizeof(struct Empty); }
    unsigned long nothing_size(void) { return sizeof(union Nothing); }

    /* An `enum` used before it is defined: not ISO C, accepted by GCC. */
    enum Colour;
    int colour_of(enum Colour *p);
    enum Colour { RED, GREEN, BLUE };
    int colour_of(enum Colour *p) { return (int) *p; }

    int green(void) { enum Colour c = GREEN; return colour_of(&c); }

    /* POSIX requires this conversion in both directions; ISO C forbids it. */
    static int twice(int n) { return n * 2; }
    void *as_data(void) { return twice; }
    int through_void(void *p, int n) {
        int (*f)(int) = p;
        return f(n);
    }
}

#[test]
fn the_shapes_gcc_allows_and_iso_c_does_not() {
    assert_eq!(unsafe { empty_size() }, 0);
    assert_eq!(unsafe { nothing_size() }, 0);
    assert_eq!(unsafe { green() }, 1);
    let p = unsafe { as_data() };
    assert_eq!(unsafe { through_void(p, 21) }, 42);
}

// ---------------------------------------------------------------------------
// `\e`, `__label__`, `__auto_type`
// ---------------------------------------------------------------------------

// `\e` is one of the escapes Rust's own lexer refuses, so this block is
// written as a string literal too.
c99! { r#"
    const char *reset_sequence(void) { return "\e[0m"; }
"# }

c99! {
    int with_local_label(int n) {
        __label__ retry, done;
        int tries = 0;
    retry:
        tries++;
        if (tries < n) goto retry;
        goto done;
    done:
        return tries;
    }

    int auto_typed(int n) {
        __auto_type doubled = n * 2;
        return doubled;
    }
}

#[test]
fn the_small_extensions() {
    let s = unsafe { core::ffi::CStr::from_ptr(reset_sequence()) };
    assert_eq!(s.to_bytes(), b"\x1b[0m");
    assert_eq!(unsafe { with_local_label(4) }, 4);
    assert_eq!(unsafe { auto_typed(21) }, 42);
}

// ---------------------------------------------------------------------------
// dialects
// ---------------------------------------------------------------------------

gnu99! {
    /* The plain spellings GCC keeps for its `gnu*` modes. */
    typeof(int) plain_typeof(int n) { return n; }

    /* And a feature of a later revision, which `gnu99` takes without a word. */
    _Static_assert(sizeof(int) == 4, "int is 32 bits here");

    int c11_in_gnu99(int n) { return _Generic(n, int: 1, default: 0); }

    int binary_literal(void) { return 0b1011; }
}

c11! {
    /* `typeof` is not a keyword here, so it may be an ordinary identifier. */
    int typeof_is_a_name(void) { int typeof = 5; return typeof; }
}

#[test]
fn a_gnu_dialect_takes_the_plain_spellings_and_the_later_revisions() {
    assert_eq!(unsafe { plain_typeof(7) }, 7);
    assert_eq!(unsafe { c11_in_gnu99(0) }, 1);
    assert_eq!(unsafe { binary_literal() }, 11);
    assert_eq!(unsafe { typeof_is_a_name() }, 5);
}

// ---------------------------------------------------------------------------
// the leniencies GCC has where ISO C has a constraint violation
// ---------------------------------------------------------------------------

#[test]
fn a_void_function_may_return_an_expression() {
    // C99 and C11 6.8.6.4p1 forbid `return expr;` in a `void` function
    // outright. C23 lets the expression stand when it has type `void` —
    // `return f();` is how a wrapper forwards a call — and GCC accepts that
    // and a non-`void` expression with it in every mode. The expression is
    // still evaluated; only its value is dropped.
    gnu99! {
        static int calls;
        static void bump(void) { calls++; }
        static int answer(void) { calls += 10; return 3; }

        void forward(void) { return bump(); }
        void discard(void) { return answer(); }
        int how_many(void) { return calls; }
    }

    unsafe {
        forward();
        assert_eq!(how_many(), 1);
        discard();
        assert_eq!(how_many(), 11);
    }
}

#[test]
fn function_pointers_of_unrelated_types_compare_by_address() {
    // Two *compatible* function pointer types need no leniency at all: a
    // prototyped type and one with an empty parameter list are compatible when
    // no parameter is changed by the default argument promotions (6.7.6.3p15),
    // which `double (*)()` against `double (*)(double)` is. `void *` against a
    // function pointer, and two genuinely unrelated function pointers, are the
    // GNU ones.
    gnu99! {
        double takes_double(double a) { return a; }
        int takes_int(int a) { return a; }

        int unprototyped_matches(void) {
            double (*loose)() = &takes_double;
            double (*tight)(double) = &takes_double;
            return loose == tight;
        }

        int against_void_pointer(void) {
            void *p = (void *)&takes_double;
            double (*f)(double) = &takes_double;
            return p == f;
        }

        int unrelated(void) {
            double (*a)(double) = &takes_double;
            int (*b)(int) = &takes_int;
            return a == b;
        }
    }

    unsafe {
        assert_eq!(unprototyped_matches(), 1);
        assert_eq!(against_void_pointer(), 1);
        assert_eq!(unrelated(), 0);
    }
}

#[test]
fn void_has_a_size_of_one() {
    // GNU gives `void` a size and an alignment of one, which is what makes the
    // `void *` arithmetic this crate already accepts mean anything.
    gnu99! {
        unsigned long size_of_void(void) { return sizeof(void); }
        unsigned long align_of_void(void) { return __alignof__(void); }
        long step(void *p) { void *q = p; q = q + 3; return (char *)q - (char *)p; }
    }

    unsafe {
        assert_eq!(size_of_void(), 1);
        assert_eq!(align_of_void(), 1);
        let mut buffer = [0u8; 8];
        assert_eq!(step(buffer.as_mut_ptr().cast()), 3);
    }
}

#[test]
fn a_stray_semicolon_at_file_scope_is_an_empty_declaration() {
    // C's grammar has no empty external declaration — 6.9p1 is a declaration
    // or a function definition — but GCC accepts one with a pedantic warning,
    // and a macro whose expansion already ends in `;` written with one after
    // it is common enough that seven cases of the torture suite do it.
    gnu99! {
        int leading(void) { return 1; };
        ;
        int trailing(void) { return 2; };;
    }

    unsafe {
        assert_eq!(leading(), 1);
        assert_eq!(trailing(), 2);
    }
}

#[test]
fn an_undeclared_alloca_is_the_builtin() {
    // No ISO header declares `alloca`, and GCC answers a call to an undeclared
    // one with `__builtin_alloca` in its `gnu` modes. Without that the C89
    // implicit declaration would type it `int()` and `void *p = alloca(n)`
    // would be a constraint violation.
    gnu99! {
        int fill(int n) {
            char *p = alloca(n);
            int i;
            int total = 0;
            for (i = 0; i < n; i++) { p[i] = (char)i; }
            for (i = 0; i < n; i++) { total += p[i]; }
            return total;
        }
    }

    assert_eq!(unsafe { fill(5) }, 1 + 2 + 3 + 4);
}
