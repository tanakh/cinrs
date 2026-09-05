//! Variably modified types beyond `T a[n]` (C99 6.7.5.2).
//!
//! A variably modified type is one whose size the running program computes:
//! `double a[n][m]`, `int (*p)[n]`, `typedef int T[n]`, and the parameter form
//! `void f(int n, int m, double a[n][m])` that adjusts to `double (*a)[m]`.
//! The model is one hidden `size_t` object per variable dimension, created
//! where the *type* is declared and pointed at by the type itself, so that
//! `sizeof`, indexing and pointer arithmetic can all find the length again
//! wherever the type turns up; `tests/vla.rs` covers the one-dimensional case
//! and the storage the two share.
//!
//! The observable facts are C's own: `sizeof a / sizeof a[0]` is the number of
//! rows, `a[i][j]` is `*(base + i * m + j)`, `p + 1` moves by a whole row, and
//! every bound is evaluated exactly once, where the declaration stands.

use cinrs::{c99, gnu99};

// ---------------------------------------------------------------------------
// objects
// ---------------------------------------------------------------------------

#[test]
fn a_two_dimensional_array_is_one_object() {
    c99! {
        /* Row-major, `n * m` elements, indexed the way C says. */
        long fill_and_sum(int n, int m) {
            long a[n][m];
            for (int i = 0; i < n; i++) {
                for (int j = 0; j < m; j++) {
                    a[i][j] = i * 100 + j;
                }
            }
            long total = 0;
            for (int i = 0; i < n; i++) {
                for (int j = 0; j < m; j++) {
                    total += a[i][j];
                }
            }
            return total;
        }

        /* The rows are contiguous: walking the whole object through a pointer
           to its first element sees every one of them. */
        long flat(int n, int m) {
            long a[n][m];
            for (int i = 0; i < n; i++) {
                for (int j = 0; j < m; j++) {
                    a[i][j] = 1;
                }
            }
            long *p = &a[0][0];
            long total = 0;
            for (long k = 0; k < (long)(n * m); k++) {
                total += p[k];
            }
            return total;
        }

        /* Three dimensions, and the innermost one variable too. */
        int cube(int n) {
            int a[n][n][n];
            for (int i = 0; i < n; i++) {
                for (int j = 0; j < n; j++) {
                    for (int k = 0; k < n; k++) {
                        a[i][j][k] = i * j * k;
                    }
                }
            }
            int total = 0;
            for (int i = 0; i < n; i++) {
                for (int j = 0; j < n; j++) {
                    for (int k = 0; k < n; k++) {
                        total += a[i][j][k];
                    }
                }
            }
            return total;
        }
    }

    unsafe {
        // sum over i of (100 i m) + sum over j of j, n times.
        let expect = |n: i64, m: i64| {
            (0..n)
                .map(|i| (0..m).map(|j| i * 100 + j).sum::<i64>())
                .sum::<i64>()
        };
        assert_eq!(fill_and_sum(3, 4), expect(3, 4));
        assert_eq!(fill_and_sum(1, 1), 0);
        assert_eq!(flat(5, 7), 35);
        // (sum 0..n)^3 with the i*j*k product: (n(n-1)/2)^3.
        assert_eq!(cube(4), 6 * 6 * 6);
    }
}

#[test]
fn fixed_and_variable_dimensions_mix() {
    c99! {
        /* `int a[n][3]`: a variable number of fixed rows. */
        int variable_outer(int n) {
            int a[n][3];
            for (int i = 0; i < n; i++) {
                for (int j = 0; j < 3; j++) {
                    a[i][j] = i + j;
                }
            }
            int total = 0;
            for (int i = 0; i < n; i++) {
                total += a[i][0] + a[i][1] + a[i][2];
            }
            /* `sizeof a` is `n * 3 * sizeof(int)`, and a row is twelve bytes. */
            return total * 10000 + (int)sizeof a * 10 + (int)sizeof a[0];
        }

        /* `int a[3][n]`: three rows of a variable width, which is variably
           modified even though the outermost bound is a constant. */
        int variable_inner(int n) {
            int a[3][n];
            for (int i = 0; i < 3; i++) {
                for (int j = 0; j < n; j++) {
                    a[i][j] = i * n + j;
                }
            }
            int total = 0;
            for (int i = 0; i < 3; i++) {
                for (int j = 0; j < n; j++) {
                    total += a[i][j];
                }
            }
            return total * 10000 + (int)sizeof a * 10 + (int)sizeof a[0];
        }
    }

    unsafe {
        // (0+1+2) + (1+2+3) = 9 for n = 2; 2*3*4 = 24 bytes, rows of 12.
        assert_eq!(variable_outer(2), 9 * 10000 + 24 * 10 + 12);
        // 0..(3n-1) summed is 15 for n = 2; 3*2*4 = 24 bytes, rows of 8.
        assert_eq!(variable_inner(2), 15 * 10000 + 24 * 10 + 8);
    }
}

// ---------------------------------------------------------------------------
// sizeof
// ---------------------------------------------------------------------------

#[test]
fn the_sizeof_identities_hold_at_run_time() {
    c99! {
        /* `sizeof a / sizeof a[0]` is the number of rows, and
           `sizeof a[0] / sizeof a[0][0]` the number of columns — however many
           of the two are only known now. */
        int identities(int n, int m) {
            double a[n][m];
            int rows = (int)(sizeof a / sizeof a[0]);
            int cols = (int)(sizeof a[0] / sizeof a[0][0]);
            int bytes = (int)sizeof a;
            return rows * 1000000 + cols * 1000 + bytes;
        }

        /* Three dimensions, and `sizeof` of a *type name* built from the same
           bounds says the same thing. */
        int three(int n) {
            char a[n][n + 1][2];
            return (int)sizeof a * 100 + (int)sizeof(char[n][n + 1][2]);
        }

        /* A fixed row inside a variable one still contributes its length. */
        unsigned long mixed(int n) {
            short a[n][4];
            return sizeof a + sizeof a[0] * 1000;
        }
    }

    unsafe {
        assert_eq!(identities(3, 5), 3 * 1000000 + 5 * 1000 + 3 * 5 * 8);
        // 4 * 5 * 2 = 40, twice.
        assert_eq!(three(4), 40 * 100 + 40);
        assert_eq!(mixed(6), 6 * 4 * 2 + 4 * 2 * 1000);
    }
}

// ---------------------------------------------------------------------------
// pointers to variably modified types
// ---------------------------------------------------------------------------

#[test]
fn a_pointer_to_a_row_walks_by_rows() {
    c99! {
        /* `int (*p)[n]` steps by a whole row, and `(*p)[j]` reaches into it. */
        int walk(int n, int rows) {
            int a[rows][n];
            for (int i = 0; i < rows; i++) {
                for (int j = 0; j < n; j++) {
                    a[i][j] = i * n + j;
                }
            }
            int (*p)[n] = a;
            int total = 0;
            for (int i = 0; i < rows; i++) {
                total += (*p)[0] + p[0][n - 1];
                p++;
            }
            /* Back to the start: pointer subtraction is in whole rows. */
            int (*q)[n] = a;
            return total * 1000 + (int)(p - q) * 10 + (int)(sizeof *p / sizeof **p);
        }

        /* `sizeof *p` is the size of one row, computed now. */
        unsigned long row_size(int n) {
            double a[2][n];
            double (*p)[n] = a;
            return sizeof *p + sizeof p[0] * 1000 + sizeof(p) * 1000000;
        }
    }

    unsafe {
        // Rows 0..2 of width 4: (0 + 3) + (4 + 7) + (8 + 11) = 33.
        assert_eq!(walk(4, 3), 33 * 1000 + 3 * 10 + 4);
        let ptr = core::mem::size_of::<*const u8>() as u64;
        assert_eq!(row_size(5), 40 + 40 * 1000 + ptr * 1000000);
    }
}

// ---------------------------------------------------------------------------
// parameters
// ---------------------------------------------------------------------------

#[test]
fn a_matrix_parameter_is_a_pointer_to_a_row() {
    c99! {
        /* The classic: `a[i][j]` in a function that was handed a flat buffer.
           `double a[n][m]` adjusts to `double (*a)[m]`, whose `m` is read on
           entry. */
        void matmul(int n, int k, int m,
                    const double a[n][k], const double b[k][m], double c[n][m]) {
            for (int i = 0; i < n; i++) {
                for (int j = 0; j < m; j++) {
                    double sum = 0;
                    for (int t = 0; t < k; t++) {
                        sum += a[i][t] * b[t][j];
                    }
                    c[i][j] = sum;
                }
            }
        }

        /* `sizeof` of a variably modified parameter: the parameter itself is a
           pointer, and a row of it is `m` elements. */
        unsigned long row_of(int n, int m, double a[n][m]) {
            (void)n;
            return sizeof a[0] + sizeof a * 1000;
        }

        /* The bound may be any expression of the names in scope, and it is
           evaluated on entry — so changing `m` afterwards changes nothing. */
        int frozen(int n, int m, int a[n][m]) {
            m = 1;
            (void)n;
            return (int)(sizeof a[0] / sizeof a[0][0]) * 10 + m;
        }
    }

    unsafe {
        // [[1, 2], [3, 4]] * [[5, 6], [7, 8]] = [[19, 22], [43, 50]]
        let a = [1.0f64, 2.0, 3.0, 4.0];
        let b = [5.0f64, 6.0, 7.0, 8.0];
        let mut c = [0.0f64; 4];
        matmul(2, 2, 2, a.as_ptr(), b.as_ptr(), c.as_mut_ptr());
        assert_eq!(c, [19.0, 22.0, 43.0, 50.0]);

        // A 2x3 times a 3x2, into a 2x2.
        let a = [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0];
        let b = [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0];
        let mut c = [0.0f64; 4];
        matmul(2, 3, 2, a.as_ptr(), b.as_ptr(), c.as_mut_ptr());
        assert_eq!(c, [22.0, 28.0, 49.0, 64.0]);

        let ptr = core::mem::size_of::<*const u8>() as u64;
        assert_eq!(row_of(2, 3, core::ptr::null_mut()), 24 + ptr * 1000);
        assert_eq!(frozen(2, 7, core::ptr::null_mut()), 70 + 1);
    }
}

#[test]
fn a_prototype_may_leave_the_bounds_unspecified() {
    c99! {
        /* `int a[*][*]` says "variably modified, bounds unspecified", which is
           only allowed in a declaration that is not a definition. The
           definition below supplies them. */
        int sum(int n, int m, int a[*][*]);

        int sum(int n, int m, int a[n][m]) {
            int total = 0;
            for (int i = 0; i < n; i++) {
                for (int j = 0; j < m; j++) {
                    total += a[i][j];
                }
            }
            return total;
        }

        /* A declaration with the bounds written out agrees with one without. */
        int first(int n, int m, int a[n][m]);
        int first(int n, int m, int a[*][*]);
        int first(int n, int m, int a[n][m]) {
            (void)n;
            (void)m;
            return a[0][0];
        }
    }

    unsafe {
        let mut buf = [1i32, 2, 3, 4, 5, 6];
        assert_eq!(sum(2, 3, buf.as_mut_ptr()), 21);
        assert_eq!(first(2, 3, buf.as_mut_ptr()), 1);
    }
}

// ---------------------------------------------------------------------------
// typedefs
// ---------------------------------------------------------------------------

#[test]
fn a_typedef_evaluates_its_bound_once_where_it_is_written() {
    c99! {
        int side(int *counter, int value) {
            *counter += 1;
            return value;
        }

        /* C99 6.7.7p4: the bound is evaluated where the `typedef` stands, and
           the objects declared with it all share that one length. */
        int shared(int n) {
            int calls = 0;
            typedef int Row[side(&calls, n)];
            Row a, b;
            for (int i = 0; i < n; i++) {
                a[i] = i;
                b[i] = 10 * i;
            }
            int total = 0;
            for (int i = 0; i < n; i++) {
                total += a[i] + b[i];
            }
            /* One evaluation, however many objects; `sizeof(Row)` is that
               same length. */
            return total * 1000 + calls * 100 + (int)(sizeof(Row) / sizeof(int));
        }

        /* A two-dimensional one, and a pointer to it. */
        int matrix(int n) {
            typedef double Grid[n][n + 1];
            Grid g;
            for (int i = 0; i < n; i++) {
                for (int j = 0; j < n + 1; j++) {
                    g[i][j] = i * 10 + j;
                }
            }
            return (int)(sizeof(Grid) / sizeof(double)) * 1000 + (int)g[1][2];
        }
    }

    unsafe {
        // (0+1+2) + (0+10+20) = 33, one call, three elements.
        assert_eq!(shared(3), 33 * 1000 + 100 + 3);
        // 3 * 4 = 12 doubles, and g[1][2] is 12.
        assert_eq!(matrix(3), 12 * 1000 + 12);
    }
}

// ---------------------------------------------------------------------------
// evaluation of the bounds
// ---------------------------------------------------------------------------

#[test]
fn every_bound_is_evaluated_exactly_once_and_in_order() {
    c99! {
        static int log_at[8];
        static int logged;

        static int note(int value) {
            log_at[logged++] = value;
            return value;
        }

        /* GCC evaluates the dimensions of one declarator innermost first, and
           each of them exactly once, where the declaration stands. */
        int order(void) {
            logged = 0;
            int a[note(2)][note(3)];
            a[0][0] = 0;
            return logged * 100 + log_at[0] * 10 + log_at[1];
        }

        /* Reaching the declaration again evaluates them again — the object is
           a fresh one (C99 6.2.4p7). */
        int in_a_loop(void) {
            logged = 0;
            for (int i = 0; i < 3; i++) {
                int a[note(1)];
                a[0] = i;
            }
            return logged;
        }

        /* A parameter's bounds are evaluated on entry, in declaration order;
           the outermost one is not part of the type, and is still evaluated. */
        static int entry(int a[note(4)][note(5)]) {
            (void)a;
            return (int)(sizeof a[0] / sizeof a[0][0]);
        }

        int on_entry(void) {
            logged = 0;
            int cols = entry(0);
            return logged * 10000 + log_at[0] * 100 + log_at[1] * 10 + cols;
        }
    }

    unsafe {
        // Two bounds, `3` before `2`.
        assert_eq!(order(), 2 * 100 + 3 * 10 + 2);
        assert_eq!(in_a_loop(), 3);
        // Two bounds, `5` before `4`, and the row is five elements wide.
        assert_eq!(on_entry(), 2 * 10000 + 5 * 100 + 4 * 10 + 5);
    }
}

// ---------------------------------------------------------------------------
// the rest of the language
// ---------------------------------------------------------------------------

#[test]
fn a_variably_modified_type_is_a_type_like_any_other() {
    gnu99! {
        /* `typeof` of one carries the bounds with it, so the second object is
           the same shape as the first — and shares the length it evaluated. */
        int through_typeof(int n) {
            int a[n][2];
            __typeof__(a) b;
            for (int i = 0; i < n; i++) {
                b[i][0] = i;
                b[i][1] = -i;
            }
            return (int)(sizeof b / sizeof b[0]) * 10 + b[n - 1][0];
        }

        /* `_Generic`'s controlling expression undergoes the usual conversions,
           so a variably modified array arrives as the pointer it decays to. */
        int generic(int n) {
            int a[n][3];
            (void)a;
            return _Generic(a, int (*)[3] : 1, default : 0)
                 + _Generic(&a[0][0], int * : 10, default : 0);
        }

        /* Handing one to a function that takes the parameter form: the object
           decays to exactly the pointer the parameter is. */
        static int first_of(int n, int m, int a[n][m]) {
            (void)n;
            return a[1][m - 1];
        }

        int passed_along(int n, int m) {
            int a[n][m];
            for (int i = 0; i < n; i++) {
                for (int j = 0; j < m; j++) {
                    a[i][j] = i * m + j;
                }
            }
            return first_of(n, m, a);
        }
    }

    unsafe {
        assert_eq!(through_typeof(4), 4 * 10 + 3);
        assert_eq!(generic(2), 11);
        // Row 1, last column, of a 3-by-4: 1 * 4 + 3.
        assert_eq!(passed_along(3, 4), 7);
    }
}

// ---------------------------------------------------------------------------
// the control-flow-graph lowering
// ---------------------------------------------------------------------------

#[test]
fn a_function_that_jumps_gets_the_same_answers() {
    gnu99! {
        /* A `goto` puts the whole function through the CFG lowering, where the
           bindings are hoisted and the allocation stays where it was written.
           Everything a two-dimensional array does has to survive that. */
        int with_goto(int n, int m) {
            int total = 0;
            int i = 0;
        again:
            {
                int a[n][m];
                for (int r = 0; r < n; r++) {
                    for (int c = 0; c < m; c++) {
                        a[r][c] = r * m + c + i;
                    }
                }
                total += a[n - 1][m - 1] + (int)(sizeof a / sizeof a[0]);
            }
            if (++i < 3) {
                goto again;
            }
            return total;
        }
    }

    unsafe {
        // The last element is n*m-1+i, and there are n rows; i = 0, 1, 2.
        let one = |i: i32| (3 * 4 - 1 + i) + 3;
        assert_eq!(with_goto(3, 4), one(0) + one(1) + one(2));
    }
}
