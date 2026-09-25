//! Variable length arrays (C99 6.7.5.2) and `alloca`.
//!
//! Both are emulated on the heap, which is the one thing about them that is
//! not C's own model: each call of a function that has either gets a bump
//! arena, dropped by the `return`; a VLA's elements are bumped off it and a
//! hidden frame guard whose `Drop` is the end of the block the declaration was
//! written in moves the arena back down, and `alloca`'s bytes are bumped off
//! it past anything a block's end gives back. Everything
//! a C program can observe is unchanged — the storage, the lifetime, the fresh
//! object each pass through a loop makes, the run-time `sizeof` — so the tests
//! below are about those observations rather than about where the bytes are.

use cinrs::{c99, gnu11, gnu99};

// ---------------------------------------------------------------------------
// the elements
// ---------------------------------------------------------------------------

#[test]
fn a_variable_length_array_holds_its_elements() {
    c99! {
        int sum_to(int n) {
            int a[n];
            for (int i = 0; i < n; i++) {
                a[i] = i * i;
            }
            int total = 0;
            for (int i = 0; i < n; i++) {
                total += a[i];
            }
            return total;
        }

        /* The elements start out as C leaves them: indeterminate. Writing
           through a pointer to the first one and reading back through the
           array is the same object either way. */
        int through_a_pointer(int n) {
            int a[n];
            int *p = a;
            for (int i = 0; i < n; i++) {
                *p++ = 1;
            }
            return a[n - 1] + (int)(p - a);
        }
    }

    unsafe {
        assert_eq!(sum_to(5), 1 + 4 + 9 + 16); // and 0 * 0
        assert_eq!(sum_to(1), 0);
        assert_eq!(through_a_pointer(4), 5);
    }
}

#[test]
fn a_zero_length_array_is_allocated_and_never_indexed() {
    // Zero elements is undefined behaviour in C; GCC allocates nothing and
    // carries on, and so does this.
    c99! {
        int empty(int n) {
            char a[n];
            return (int)sizeof a + (a != 0);
        }
    }

    unsafe {
        assert_eq!(empty(0), 1);
    }
}

#[test]
fn the_element_type_may_be_a_struct_or_a_fixed_array() {
    c99! {
        struct Point { int x; int y; };

        int of_structs(int n) {
            struct Point ps[n];
            for (int i = 0; i < n; i++) {
                ps[i].x = i;
                ps[i].y = -i;
            }
            int total = 0;
            for (int i = 0; i < n; i++) {
                total += ps[i].x - ps[i].y;
            }
            /* The whole object is `n` points, whatever the padding. */
            return total * 100 + (int)(sizeof ps / sizeof ps[0]);
        }

        /* `int a[n][3]` is a variable length array whose element type is the
           fixed `int[3]`, which is the one multi-dimensional form that needs
           no variably modified machinery at all. */
        int rows(int n) {
            int a[n][3];
            for (int i = 0; i < n; i++) {
                for (int j = 0; j < 3; j++) {
                    a[i][j] = i * 10 + j;
                }
            }
            int total = 0;
            for (int i = 0; i < n; i++) {
                total += a[i][0] + a[i][1] + a[i][2];
            }
            return total * 1000 + (int)sizeof a;
        }
    }

    unsafe {
        // `x - y` is 0, 2 and 4 over the three points, and there are three.
        assert_eq!(of_structs(3), 6 * 100 + 3);
        // (0+1+2) + (10+11+12) + (20+21+22) = 99, and 3 * 3 * 4 bytes.
        assert_eq!(rows(3), 99 * 1000 + 36);
    }
}

#[test]
fn a_char_array_is_a_buffer_for_the_c_library() {
    c99! {
        #include <stdio.h>
        #include <string.h>

        int describe(int n, const char *name) {
            char buf[n];
            int written = snprintf(buf, sizeof buf, "%s is %d", name, n);
            return written * 1000 + (int)strlen(buf);
        }
    }

    unsafe {
        // "cinrs is 32" is eleven characters, and all of it fits.
        assert_eq!(describe(32, c"cinrs".as_ptr()), 11 * 1000 + 11);
        // ... and a buffer of eight truncates to seven characters plus the NUL,
        // while `snprintf` still answers what it would have written.
        assert_eq!(describe(8, c"cinrs".as_ptr()), 10 * 1000 + 7);
    }
}

// ---------------------------------------------------------------------------
// sizeof
// ---------------------------------------------------------------------------

#[test]
fn sizeof_is_evaluated_at_run_time() {
    c99! {
        unsigned long bytes(int n) {
            double a[n];
            return sizeof a;
        }

        unsigned long elements(int n) {
            double a[n];
            return sizeof a / sizeof a[0];
        }

        /* `sizeof(T[n])` on a *type name* evaluates the bound where it is
           written (C99 6.5.3.4p2), and nothing is allocated. */
        unsigned long of_a_type_name(int n) {
            return sizeof(int[n]);
        }

        /* ... including the side effects in it, exactly once. */
        int side_effects(void) {
            int n = 1;
            unsigned long size = sizeof(char[n++]);
            return (int)size * 10 + n;
        }
    }

    unsafe {
        assert_eq!(bytes(4), 32);
        assert_eq!(bytes(0), 0);
        assert_eq!(elements(7), 7);
        assert_eq!(of_a_type_name(5), 20);
        assert_eq!(side_effects(), 12);
    }
}

// ---------------------------------------------------------------------------
// lifetime
// ---------------------------------------------------------------------------

#[test]
fn each_pass_through_a_loop_allocates_afresh() {
    c99! {
        /* The bound changes every iteration, so an object that survived one
           would be too small for the next; storing into the last element is
           what says it did not. */
        long growing(int n) {
            long total = 0;
            for (int i = 1; i <= n; i++) {
                int a[i];
                for (int j = 0; j < i; j++) {
                    a[j] = 0;
                }
                a[i - 1] = i;
                total += a[i - 1] + (long)(sizeof a / sizeof a[0]);
            }
            return total;
        }
    }

    unsafe {
        // (1 + 1) + (2 + 2) + … + (n + n) for n = 100.
        assert_eq!(growing(100), 2 * (100 * 101 / 2));
    }
}

#[test]
fn the_bound_is_evaluated_exactly_once() {
    // C99 6.7.5.2p5: the size expression is evaluated when the declaration is
    // reached, and its side effects happen then and only then.
    c99! {
        int four(int *calls) { (*calls)++; return 4; }

        int once(void) {
            int calls = 0;
            int a[four(&calls)];
            a[3] = 1;
            return calls * 100 + (int)(sizeof a / sizeof a[0]) + a[3];
        }
    }

    unsafe {
        assert_eq!(once(), 105);
    }
}

#[test]
fn two_arrays_of_one_name_in_nested_blocks_stay_apart() {
    // In a function that jumps, every local is hoisted into one scope and
    // renamed apart; the hidden storage of each array has to be renamed with
    // it, or the inner declaration would take the outer one's memory away.
    c99! {
        int shadowed(int n) {
            int total = 0;
            int a[n];
            a[0] = 1;
            {
                int a[n + 1];
                a[n] = 2;
                total += a[n] + (int)sizeof a;
            }
            total += a[0] + (int)sizeof a;
            if (total) goto out;   /* which is what forces the lowering */
        out:
            return total;
        }
    }

    unsafe {
        // 2 + 4 * 4 bytes, then 1 + 3 * 4 bytes.
        assert_eq!(shadowed(3), 2 + 16 + 1 + 12);
    }
}

#[test]
fn a_nested_block_frees_its_array_and_the_code_after_it_runs() {
    c99! {
        int nested(int n) {
            int outer = 7;
            {
                int a[n];
                for (int i = 0; i < n; i++) {
                    a[i] = 1;
                }
                outer += a[n - 1];
            }
            /* The array is gone; everything else is untouched. */
            outer *= 2;
            {
                /* A second one in a sibling block, of a different length. */
                char b[n * 2];
                b[0] = 3;
                outer += b[0] + (int)sizeof b;
            }
            return outer;
        }
    }

    unsafe {
        assert_eq!(nested(4), (7 + 1) * 2 + 3 + 8);
    }
}

#[test]
fn a_thousand_large_arrays_do_not_grow_the_process() {
    // The storage is freed by `Drop` when the block ends, so a loop that
    // allocates a mebibyte on every pass ends where it started. Reading
    // `/proc/self/statm` turns "no leak by construction" into an assertion;
    // where there is no procfs the test still exercises the loop.
    c99! {
        /* `long long` rather than `long`: the running total on the Rust side
           is eleven thousand mebibytes, which a 32-bit `long` could not hold
           and Windows gives it only 32 bits. */
        long long touch(int n) {
            char a[n];
            a[0] = 1;
            a[n - 1] = 2;
            return a[0] + a[n - 1] + (long long)sizeof a;
        }
    }

    let pages = || -> Option<u64> {
        let text = std::fs::read_to_string("/proc/self/statm").ok()?;
        text.split_whitespace().next()?.parse().ok()
    };

    let mut total = 0i64;
    for _ in 0..1_000 {
        total += unsafe { touch(1 << 20) };
    }
    assert_eq!(total, 1_000 * (3 + (1 << 20)));

    let before = pages();
    for _ in 0..10_000 {
        total += unsafe { touch(1 << 20) };
    }
    assert_eq!(total, 11_000 * (3 + (1 << 20)));
    if let (Some(before), Some(after)) = (before, pages()) {
        // A mebibyte is 256 pages of 4 KiB; anything under a hundred of them
        // is the allocator moving its own furniture around.
        assert!(
            after <= before + 100,
            "the process grew from {before} to {after} pages over 10 000 arrays"
        );
    }
}

// ---------------------------------------------------------------------------
// the arena
// ---------------------------------------------------------------------------

/// The size of the process in pages, where there is a procfs to ask.
fn process_pages() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/statm").ok()?;
    text.split_whitespace().next()?.parse().ok()
}

/// [`process_pages`] as a C function pointer, for a C loop to ask from inside
/// one call — the arena lives as long as the call, so only a measurement
/// taken inside it can see it grow. Zero where there is no procfs.
unsafe extern "C" fn probe_pages() -> std::ffi::c_long {
    process_pages().unwrap_or(0) as std::ffi::c_long
}

/// Asserts that a C loop's two probes, taken early and at its end, are
/// within a hundred pages of each other.
fn assert_bounded(pages: [std::ffi::c_long; 2], what: &str) {
    let [early, late] = pages;
    assert!(
        late <= early + 100,
        "the process grew from {early} to {late} pages over {what}"
    );
}

/// A million passes through a block with an array, leaving it by falling off
/// its end, by `continue` and by `break` — each of them gives the space back,
/// so every pass gets the same address and the process does not grow inside
/// the one call that makes them all.
#[test]
fn a_million_arrays_in_one_call_reuse_one_place() {
    c99! {
        /* `long long` for the total: on Windows a `long` is 32 bits. */
        long long loop_of_arrays(int passes, long (*probe)(void), long pages[2]) {
            long long total = 0;
            int *first = 0;
            for (int i = 0; i < passes + 1; i++) {
                int n = 100 + i % 3;
                int a[n];
                if (first == 0) {
                    first = a;
                } else if (a != first) {
                    return -1;
                }
                for (int j = 0; j < n; j++) {
                    a[j] = i + j;
                }
                if (i == 1000) {
                    pages[0] = probe();
                }
                if (i == passes) {
                    break;
                }
                if (i % 2) {
                    total += a[n - 1];
                    continue;
                }
                total += a[0];
            }
            pages[1] = probe();
            return total;
        }
    }

    let passes = 1_000_000;
    let mut pages = [0; 2];
    let total = unsafe { loop_of_arrays(passes, Some(probe_pages), pages.as_mut_ptr()) };
    // An odd pass adds a[n - 1] = i + n - 1, an even one a[0] = i.
    let expected: i64 = (0..i64::from(passes))
        .map(|i| if i % 2 == 1 { i + 100 + i % 3 - 1 } else { i })
        .sum();
    assert_eq!(total, expected);
    assert_bounded(pages, "a million arrays");
}

/// An array whose size doubles every pass, inside the scope of one that
/// stays: the arena runs out of room again and again while the outer array is
/// live, and the new room must never be where the outer array is.
#[test]
fn a_growing_inner_array_leaves_the_outer_one_where_it_is() {
    c99! {
        int nested_growth(int outer_len, int doublings) {
            int outer[outer_len];
            for (int i = 0; i < outer_len; i++) {
                outer[i] = 7 * i + 1;
            }
            int *saved = outer;
            for (int k = 0, n = 1; k < doublings; k++, n *= 2) {
                int inner[n];
                for (int j = 0; j < n; j++) {
                    inner[j] = -j;
                }
                if (inner[n - 1] != -(n - 1)) {
                    return -1;
                }
                if (inner < outer + outer_len && outer < inner + n) {
                    return -2;
                }
            }
            if (saved != outer) {
                return -3;
            }
            for (int i = 0; i < outer_len; i++) {
                if (outer[i] != 7 * i + 1) {
                    return -4;
                }
            }
            return 1;
        }
    }

    unsafe {
        // Up to 2^20 ints — four mebibytes — from a first chunk of four
        // kibibytes.
        assert_eq!(nested_growth(100, 21), 1);
        assert_eq!(nested_growth(2000, 16), 1);
    }
}

/// `alloca` in a block that also has an array: the block's end gives the
/// array back, but not the `alloca`'d bytes above it, which live until the
/// function returns — so a later array must not be put on top of them.
#[test]
fn alloca_in_a_block_with_an_array_outlives_the_block() {
    gnu99! {
        #include <alloca.h>

        int alloca_survives(int n) {
            unsigned char *kept;
            {
                int a[n];
                for (int i = 0; i < n; i++) {
                    a[i] = i;
                }
                kept = alloca(256);
                for (int i = 0; i < 256; i++) {
                    kept[i] = (unsigned char)(i ^ 0x5a);
                }
                if (a[n - 1] != n - 1) {
                    return -1;
                }
            }
            {
                int b[4 * n];
                for (int i = 0; i < 4 * n; i++) {
                    b[i] = -1;
                }
                if (b[0] != -1) {
                    return -2;
                }
            }
            for (int i = 0; i < 256; i++) {
                if (kept[i] != (unsigned char)(i ^ 0x5a)) {
                    return -3;
                }
            }
            return 1;
        }
    }

    unsafe {
        assert_eq!(alloca_survives(10), 1);
        // Large enough that the second array needs a chunk of its own.
        assert_eq!(alloca_survives(5000), 1);
    }
}

/// Every call has an arena of its own, so a recursive function's arrays —
/// one per activation, each a different size — stay apart.
#[test]
fn each_activation_of_a_recursive_function_has_its_own_arrays() {
    c99! {
        long descend(int depth, int n) {
            long a[n];
            for (int i = 0; i < n; i++) {
                a[i] = depth;
            }
            long below = depth > 0 ? descend(depth - 1, n + 3) : 0;
            if (below < 0) {
                return below;
            }
            long total = 0;
            for (int i = 0; i < n; i++) {
                if (a[i] != depth) {
                    return -1;
                }
                total += a[i];
            }
            return below + total;
        }
    }

    unsafe {
        // The activation at depth d has n = 5 + 3 * (20 - d) elements of d.
        let expected: std::ffi::c_long = (0..=20)
            .map(|d: std::ffi::c_long| d * (5 + 3 * (20 - d)))
            .sum();
        assert_eq!(descend(20, 5), expected);
    }
}

/// `aligned(64)` in a loop, behind a small array that leaves the arena's
/// position anywhere: the padding is by address, so every pass is aligned.
#[test]
fn an_over_aligned_array_in_a_loop_is_aligned_every_pass() {
    gnu11! {
        #include <stdint.h>

        int aligned_every_pass(int passes) {
            int ok = 1;
            for (int i = 1; i <= passes; i++) {
                char before[i % 13 + 1];
                double v[i] __attribute__((aligned(64)));
                before[0] = 1;
                v[i - 1] = i;
                ok &= (uintptr_t)v % 64 == 0 && v[i - 1] == i && before[0] == 1;
            }
            return ok;
        }
    }

    unsafe {
        assert_eq!(aligned_every_pass(2000), 1);
    }
}

/// A function that jumps backwards over two declarations, many times, with
/// the first array a different size every pass: each declaration reached
/// again gives back what the previous pass took — and forgets the second
/// array's old place, which is inside the first one now that it has grown.
#[test]
fn arrays_a_goto_declares_again_stay_apart_and_bounded() {
    c99! {
        long long redeclared(int passes, long (*probe)(void), long pages[2]) {
            long long total = 0;
            int i = 0;
        again:;
            int n = 10 + (i * 7) % 90;
            int m = 20;
            int a[n];
            int b[m];
            for (int j = 0; j < n; j++) {
                a[j] = i;
            }
            for (int j = 0; j < m; j++) {
                b[j] = -i;
            }
            for (int j = 0; j < n; j++) {
                if (a[j] != i) {
                    return -1;
                }
            }
            total += a[n - 1] - b[19];
            if (i == 1000) {
                pages[0] = probe();
            }
            if (++i < passes) {
                goto again;
            }
            pages[1] = probe();
            return total;
        }
    }

    let passes = 200_000;
    let mut pages = [0; 2];
    let total = unsafe { redeclared(passes, Some(probe_pages), pages.as_mut_ptr()) };
    let expected: i64 = (0..i64::from(passes)).map(|i| 2 * i).sum();
    assert_eq!(total, expected);
    assert_bounded(pages, "arrays a goto declared again");
}

// ---------------------------------------------------------------------------
// parameters, and the other lowering
// ---------------------------------------------------------------------------

#[test]
fn a_variable_length_array_parameter_is_a_pointer() {
    c99! {
        /* C99 6.7.5.3p7: an array parameter is adjusted to a pointer, bound
           and all, so `sizeof` of one is the size of a pointer. */
        int sum(int n, int a[n]) {
            int total = 0;
            for (int i = 0; i < n; i++) {
                total += a[i];
            }
            return total;
        }

        int sizeof_the_parameter(int n, int a[n]) {
            (void)a;
            return (int)sizeof a == (int)sizeof(int *);
        }

        int fill_and_sum(int n) {
            int a[n];
            for (int i = 0; i < n; i++) {
                a[i] = i + 1;
            }
            return sum(n, a) + sizeof_the_parameter(n, a);
        }
    }

    unsafe {
        assert_eq!(fill_and_sum(4), 1 + 2 + 3 + 4 + 1);
    }
}

#[test]
fn a_function_that_also_jumps_keeps_its_array() {
    // A `goto` sends the whole function through the control-flow graph, where
    // every local — the hidden storage included — is bound at the top and the
    // declaration becomes an assignment. Jumping *out of* the scope is fine;
    // jumping *in* is refused, and `tests/ui/sema_vla_errors.rs` is where that
    // is checked.
    c99! {
        int with_a_goto(int n) {
            int total = 0;
            int a[n];
            for (int i = 0; i < n; i++) {
                a[i] = i;
                if (i == 3) goto done;
            }
        done:
            for (int i = 0; i < n; i++) {
                total += a[i];
            }
            return total + (int)(sizeof a / sizeof a[0]);
        }

        /* Reaching the declaration again allocates again, which is what C99
           6.2.4p7 says a jump back into the scope does. */
        int twice(int n) {
            int passes = 0;
            int total = 0;
        again:
            {
                int a[n];
                a[n - 1] = n;
                total += a[n - 1] + (int)sizeof a;
            }
            if (++passes < 2) {
                n++;
                goto again;
            }
            return total;
        }
    }

    unsafe {
        // Only `a[0..=3]` was written, but every element was read: the rest is
        // the zero the emulation starts from, which C leaves indeterminate.
        assert_eq!(with_a_goto(8), 1 + 2 + 3 + 8);
        // (2 + 8) + (3 + 12)
        assert_eq!(twice(2), 25);
    }
}

/// An array in the body of a loop the relooper recovered, with a `goto` out of
/// its scope.
///
/// The jump *in* is to a label before the array's own block — jumping into
/// that block is the error C99 6.8.6.1p1 gives, and sema reports it — and the
/// jump *out* leaves the scope, which is what frees the storage.
#[test]
fn an_array_in_a_relooped_loop_a_goto_leaves() {
    c99! {
        int scan(int n, int start) {
            int t = 0;
            if (start) goto mid;
            while (n > 0) {
            mid:
                {
                    int a[n < 1 ? 1 : n];
                    for (int i = 0; i < (n < 1 ? 1 : n); i++) a[i] = i + 1;
                    t += a[0] + (int)(sizeof a / sizeof a[0]);
                    if (t > 20) goto out;
                }
                n--;
            }
        out:
            return t;
        }
    }

    unsafe {
        // Passes of 1 + n for n = 4, 3, 2, 1: 5, 4, 3, 2.
        assert_eq!(scan(4, 0), 14);
        // Entering at the label only skips the loop's own first test.
        assert_eq!(scan(4, 1), 14);
        assert_eq!(scan(1, 0), 2);
        // n = 0 through the label: the body runs once with a one-element
        // array, and the loop's test then ends it.
        assert_eq!(scan(0, 1), 2);
    }
}

// ---------------------------------------------------------------------------
// alloca
// ---------------------------------------------------------------------------

#[test]
fn alloca_lives_until_the_function_returns() {
    c99! {
        #include <alloca.h>

        /* Ten blocks, all still readable after the loop that made them: that
           is `alloca`'s lifetime, and the difference from a variable length
           array in the same block. */
        int keeps_every_block(int n) {
            char *kept[10];
            for (int i = 0; i < n; i++) {
                char *p = alloca(64);
                p[0] = (char)(i + 1);
                kept[i] = p;
            }
            int total = 0;
            for (int i = 0; i < n; i++) {
                total += kept[i][0];
            }
            return total;
        }
    }

    unsafe {
        assert_eq!(keeps_every_block(10), 55);
    }
}

#[test]
fn alloca_hands_out_aligned_memory() {
    c99! {
        #include <alloca.h>
        #include <stdint.h>

        int aligned(int n) {
            int ok = 1;
            for (int i = 1; i <= n; i++) {
                void *p = alloca(i);
                ok = ok && p != 0 && (uintptr_t)p % 16 == 0;
            }
            return ok;
        }

        /* The builtin spelled directly needs no header at all, exactly as in
           GCC — and `__builtin_alloca_with_align` takes its alignment in bits. */
        int spelled_directly(void) {
            char *p = __builtin_alloca(8);
            char *q = __builtin_alloca_with_align(8, 64);
            p[0] = 4;
            q[0] = 5;
            return p[0] + q[0];
        }
    }

    unsafe {
        assert_eq!(aligned(40), 1);
        assert_eq!(spelled_directly(), 9);
    }
}

#[test]
fn alloca_and_a_variable_length_array_in_one_function() {
    gnu99! {
        #include <alloca.h>

        long both(int n) {
            int a[n];
            int *b = alloca(n * sizeof(int));
            for (int i = 0; i < n; i++) {
                a[i] = i;
                b[i] = i * 2;
            }
            long total = 0;
            for (int i = 0; i < n; i++) {
                total += a[i] + b[i];
            }
            return total + (long)sizeof a;
        }
    }

    unsafe {
        // 3 * (0 + 1 + 2) plus 4 * 4 bytes
        assert_eq!(both(4), 3 * 6 + 16);
    }
}
