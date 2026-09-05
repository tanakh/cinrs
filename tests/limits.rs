//! Translation limits, through the macro and at run time.
//!
//! `crates/cinrs-core/tests/limits.rs` measures what the front end spends on
//! the inputs C23 5.2.5.2p1 asks an implementation to accept. This file takes
//! the two that stress everything *after* the front end and puts them through
//! a real `c99!` invocation, so that code generation, `rustc` and the running
//! program all get a turn:
//!
//! * a `switch` with 1023 `case` labels, which is the labelled-block chain
//!   described in [`cinrs_core::codegen`] — one block per group, nested — and,
//!   in a function that also jumps, the state machine the
//!   [CFG lowering](cinrs_core::cfg) makes of it instead;
//! * a `struct` nested 63 deep, which is what the specifier arena was built
//!   for, together with a member path that reaches all the way in;
//! * an expression with 1500 operands — a sum, a comma expression and a
//!   conjunction — which is the length C23 5.2.5.2p1's 4095-character logical
//!   source line comes to. A left-associative chain is siblings rather than
//!   nesting, so every pass walks it in a loop; these run the result.
//!
//! The C is written with the preprocessor rather than by hand: `__COUNTER__`
//! gives every `case` a value of its own, and a ladder of doubling macros
//! (`D1`, `D2`, `D4`, …) writes the nesting. That keeps the file readable and
//! puts the macro expander under the same load. It goes in as a raw string
//! literal rather than as Rust tokens, because `##` and a line continuation
//! are C that Rust's own lexer will not hand over; see
//! [`cinrs_core::capture::InputMode`].

use cinrs::c99;

c99! { r####"

    /* -- 1023 case labels ------------------------------------------------ */

    /* The value the first `case` below gets. `__COUNTER__` is one per
     * translation unit and this is its first use, but naming the base rather
     * than assuming it is zero keeps the test honest. */
    enum { FIRST = __COUNTER__ + 1 };
    enum { CASES = 1023 };

    #define ONCE case __COUNTER__: hits += 1; break;
    #define TEN ONCE ONCE ONCE ONCE ONCE ONCE ONCE ONCE ONCE ONCE
    #define HUNDRED TEN TEN TEN TEN TEN TEN TEN TEN TEN TEN
    #define THOUSAND HUNDRED HUNDRED HUNDRED HUNDRED HUNDRED \
                     HUNDRED HUNDRED HUNDRED HUNDRED HUNDRED

    /* 1023 groups, each of which leaves the `switch`: the generated Rust is
     * 1023 labelled blocks, one inside the next. */
    int one_of_1023(int x) {
        int hits = 0;
        switch (x) {
            THOUSAND TEN TEN ONCE ONCE ONCE
        }
        return hits;
    }

    int first_case(void) { return FIRST; }
    int case_count(void) { return CASES; }

    /* -- 1023 labels on one statement ------------------------------------ */

    #undef ONCE
    #undef TEN
    #undef HUNDRED
    #undef THOUSAND
    #define ONCE case __COUNTER__:
    #define TEN ONCE ONCE ONCE ONCE ONCE ONCE ONCE ONCE ONCE ONCE
    #define HUNDRED TEN TEN TEN TEN TEN TEN TEN TEN TEN TEN
    #define THOUSAND HUNDRED HUNDRED HUNDRED HUNDRED HUNDRED \
                     HUNDRED HUNDRED HUNDRED HUNDRED HUNDRED

    enum { FALL_FIRST = __COUNTER__ + 1 };

    /* The same labels, all on one statement: one group, and a chain of 1023
     * labelled statements for the parser to fold. */
    int any_of_1023(int x) {
        switch (x) {
            THOUSAND TEN TEN ONCE ONCE ONCE
                return 1;
            default:
                return 0;
        }
    }

    int first_fall_case(void) { return FALL_FIRST; }

    /* -- the same switch in a function that jumps ------------------------ */

    enum { CFG_FIRST = __COUNTER__ + 1 };

    #undef ONCE
    #define ONCE case __COUNTER__: total += 2; goto done;

    /* A `goto` makes sema lower the whole body through a control-flow graph,
     * so this is the `match` over states rather than the block chain, with a
     * thousand of them. */
    int cfg_one_of_1023(int x) {
        int total = 0;
        if (x < 0) {
            goto done;
        }
        switch (x) {
            THOUSAND TEN TEN ONCE ONCE ONCE
        }
        total += 1;
    done:
        return total;
    }

    int first_cfg_case(void) { return CFG_FIRST; }

    /* -- a struct nested 63 deep ----------------------------------------- */

    #define D1(x) struct { x } m;
    #define D2(x) D1(D1(x))
    #define D4(x) D2(D2(x))
    #define D8(x) D4(D4(x))
    #define D16(x) D8(D8(x))
    #define D32(x) D16(D16(x))

    /* 32 + 16 + 8 + 4 + 2 + 1 = 63 levels of anonymous `struct`, with an
     * `int` at the bottom. */
    struct Deep {
        D32(D16(D8(D4(D2(D1(int a;))))))
    };

    #define P1(x) x.m
    #define P2(x) P1(P1(x))
    #define P4(x) P2(P2(x))
    #define P8(x) P4(P4(x))
    #define P16(x) P8(P8(x))
    #define P32(x) P16(P16(x))
    #define INNERMOST(x) P32(P16(P8(P4(P2(P1(x))))))

    int deep_size(void) { return (int)sizeof(struct Deep); }

    int deep_roundtrip(int value) {
        struct Deep d;
        INNERMOST(d).a = value;
        return INNERMOST(d).a;
    }

    /* -- 1023 members and 1023 enumeration constants --------------------- */

    #define CAT_(a, b) a##b
    #define CAT(a, b) CAT_(a, b)
    #undef ONCE
    #define ONCE int CAT(m, __COUNTER__);

    struct Wide {
        THOUSAND TEN TEN ONCE ONCE ONCE
    };

    int wide_size(void) { return (int)sizeof(struct Wide); }

    /* -- 1500 operands in one expression --------------------------------- */

    /* C23 5.2.5.2p1 asks for 4095 characters in one logical source line,
     * which in the shortest thing worth writing is about two thousand comma
     * operands; Clang's own `C99/n590.c` spells exactly that out. A
     * left-associative chain is a run of *siblings* rather than nesting, so
     * the parser takes it in a loop and sema and code generation walk the
     * spine iteratively — but the proof that it works is a program that
     * builds and gives the right answer, which is what these three are.
     *
     * A doubling ladder writes them, so the file stays readable and the macro
     * expander does the work: 1024 + 256 + 128 + 64 + 16 + 8 + 4 = 1500. */

    #define ADD1 + n
    #define ADD4 ADD1 ADD1 ADD1 ADD1
    #define ADD8 ADD4 ADD4
    #define ADD16 ADD8 ADD8
    #define ADD64 ADD16 ADD16 ADD16 ADD16
    #define ADD128 ADD64 ADD64
    #define ADD256 ADD128 ADD128
    #define ADD1024 ADD256 ADD256 ADD256 ADD256
    #define TIMES1500 ADD1024 ADD256 ADD128 ADD64 ADD16 ADD8 ADD4

    /* A sum of 1501 operands: `0 + n + n + … + n`, folded to the left. */
    int sum_1500(int n) { return 0 TIMES1500; }

    #define STEP1 , (t += 1)
    #define STEP4 STEP1 STEP1 STEP1 STEP1
    #define STEP8 STEP4 STEP4
    #define STEP16 STEP8 STEP8
    #define STEP64 STEP16 STEP16 STEP16 STEP16
    #define STEP128 STEP64 STEP64
    #define STEP256 STEP128 STEP128
    #define STEP1024 STEP256 STEP256 STEP256 STEP256
    #define STEPS1500 STEP1024 STEP256 STEP128 STEP64 STEP16 STEP8 STEP4

    /* A comma expression with 1501 operands, every one of which is evaluated
     * for its side effect and all but the last discarded. */
    int comma_1500(void) {
        int t = 0;
        return (0 STEPS1500);
    }

    #define AND1 && n
    #define AND4 AND1 AND1 AND1 AND1
    #define AND8 AND4 AND4
    #define AND16 AND8 AND8
    #define AND64 AND16 AND16 AND16 AND16
    #define AND128 AND64 AND64
    #define AND256 AND128 AND128
    #define AND1024 AND256 AND256 AND256 AND256
    #define ALL1500 AND1024 AND256 AND128 AND64 AND16 AND8 AND4

    /* 1501 operands of `&&`, which short-circuits: the answer is 0 or 1. */
    int all_1500(int n) { return n ALL1500; }
"#### }

#[test]
fn a_switch_with_1023_case_labels_runs() {
    let first = unsafe { first_case() };
    let count = unsafe { case_count() };
    assert_eq!(count, 1023);
    // Every label is its own group, and each one leaves the `switch`.
    for x in [first, first + 1, first + 511, first + count - 1] {
        assert_eq!(unsafe { one_of_1023(x) }, 1, "case {x}");
    }
    for x in [first - 1, first + count, -1, 100_000] {
        assert_eq!(unsafe { one_of_1023(x) }, 0, "case {x}");
    }
}

#[test]
fn a_switch_with_1023_labels_on_one_statement_runs() {
    let first = unsafe { first_fall_case() };
    for x in [first, first + 1, first + 1022] {
        assert_eq!(unsafe { any_of_1023(x) }, 1, "case {x}");
    }
    for x in [first - 1, first + 1023, -1] {
        assert_eq!(unsafe { any_of_1023(x) }, 0, "case {x}");
    }
}

#[test]
fn the_same_switch_lowered_through_a_control_flow_graph_runs() {
    let first = unsafe { first_cfg_case() };
    // A matching case adds two and jumps to the label; anything else falls out
    // of the `switch` and adds one on the way past.
    for x in [first, first + 1, first + 1022] {
        assert_eq!(unsafe { cfg_one_of_1023(x) }, 2, "case {x}");
    }
    assert_eq!(unsafe { cfg_one_of_1023(first + 1023) }, 1);
    assert_eq!(unsafe { cfg_one_of_1023(-1) }, 0);
}

#[test]
fn a_struct_nested_63_deep_runs() {
    // 63 levels of `struct { … }` around one `int`, which every level is laid
    // out around: the whole thing is that `int`.
    assert_eq!(unsafe { deep_size() }, core::mem::size_of::<i32>() as i32);
    assert_eq!(unsafe { deep_roundtrip(-7) }, -7);
    assert_eq!(unsafe { deep_roundtrip(1_234_567) }, 1_234_567);
}

#[test]
fn a_struct_with_1023_members_runs() {
    assert_eq!(
        unsafe { wide_size() },
        1023 * core::mem::size_of::<i32>() as i32
    );
}

#[test]
fn a_1500_operand_expression_runs() {
    // 1500 additions of `n` to zero. What is being checked is not the
    // arithmetic but that the whole pipeline — preprocessor, parser, sema,
    // code generation and then `rustc` — took a chain this long at all: the
    // generated Rust is `.wrapping_add(n)` fifteen hundred times over, which
    // is a *flat* receiver chain rather than fifteen hundred nested
    // expressions.
    assert_eq!(unsafe { sum_1500(0) }, 0);
    assert_eq!(unsafe { sum_1500(1) }, 1500);
    assert_eq!(unsafe { sum_1500(-2) }, -3000);
    // Signed overflow wraps rather than panicking, as everywhere else here.
    assert_eq!(unsafe { sum_1500(i32::MAX) }, i32::MAX.wrapping_mul(1500));
}

#[test]
fn a_1500_operand_comma_expression_runs() {
    // Every operand is evaluated, left to right, and all but the last are
    // discarded; the count is the proof that none of them was dropped.
    assert_eq!(unsafe { comma_1500() }, 1500);
}

#[test]
fn a_1500_operand_conjunction_runs() {
    // `&&` short-circuits, so a false operand stops the chain — and a chain
    // this long still produces C's 0 or 1 rather than the operand itself.
    assert_eq!(unsafe { all_1500(0) }, 0);
    assert_eq!(unsafe { all_1500(1) }, 1);
    assert_eq!(unsafe { all_1500(-7) }, 1);
}
