//! Root integration test: the macro works end to end for both input forms and
//! in both item and statement position.
//!
//! The behaviour of the generated code lives in `execute.rs`; this file is
//! about the macro fitting into Rust where a macro should.

// Raw-token mode, at item position.
cinrs::c99! {
    int add(int a, int b) {
        return a + b;
    }
}

// String-literal mode, at item position.
cinrs::c99! { r#"int sub(int a, int b) { return a - b; }"# }

// Raw-token mode over a larger unit, exercising the whole grammar path.
cinrs::c99! {
    typedef unsigned long size_type;

    static int clamp(int v, int lo, int hi) {
        if (v < lo) return lo;
        if (v > hi) return hi;
        return v;
    }

    int fact(int n) {
        if (n == 0) {
            return 1;
        } else {
            return n * fact(n - 1);
        }
    }

    size_type clamped_length(int n) {
        return clamp(n, 0, 10);
    }
}

#[test]
fn macro_expands_in_statement_position() {
    cinrs::c99! {
        int local_double(int n) { return n * 2; }
    }
    cinrs::c99! { "int local_triple(int n) { return n * 3; }" }

    assert_eq!(unsafe { local_double(21) }, 42);
    assert_eq!(unsafe { local_triple(14) }, 42);
}

#[test]
fn items_from_the_file_scope_are_callable() {
    assert_eq!(unsafe { add(19, 23) }, 42);
    assert_eq!(unsafe { sub(50, 8) }, 42);
    assert_eq!(unsafe { fact(5) }, 120);
    assert_eq!(unsafe { clamped_length(99) }, 10);
}
