//! Integration tests that *run* translated C using `goto` and the `case`
//! labels a `switch` cannot reach without one.
//!
//! Both are lowered through a control-flow graph rather than through Rust's own
//! control flow, and every local of such a function is hoisted to the top and
//! renamed. These tests are what says the state machine that comes out still
//! computes what the C did.

use cinrs::c99;

// ---------------------------------------------------------------------------
// goto
// ---------------------------------------------------------------------------

#[test]
fn forward_and_backward_jumps() {
    c99! {
        /* A forward jump over code that must not run. */
        int clamp(int n, int hi) {
            int out = n;
            if (n > hi) {
                out = hi;
                goto done;
            }
            out = out * 2;
        done:
            return out;
        }

        /* A loop built out of nothing but a backward jump. */
        int triangle(int n) {
            int total = 0;
            int i = 1;
        again:
            if (i > n) goto end;
            total += i;
            i++;
            goto again;
        end:
            return total;
        }

        /* A declaration right after a label. C99 needs the null statement in
           between — a label may only precede a statement — and the object is
           hoisted out of the jump's way. */
        int after_label(int n) {
            if (n < 0) goto negative;
            return n;
        negative: ;
            int positive = -n;
            return positive;
        }
    }

    unsafe {
        assert_eq!(clamp(3, 10), 6);
        assert_eq!(clamp(30, 10), 10);
        assert_eq!(triangle(4), 10);
        assert_eq!(triangle(0), 0);
        assert_eq!(after_label(7), 7);
        assert_eq!(after_label(-7), 7);
    }
}

#[test]
fn goto_fail_leaves_nested_loops() {
    c99! {
        /* The cleanup idiom: one exit for every failure, from any depth. */
        int find_pair(const int *values, int n, int target, int *steps) {
            int found = 0;
            int count = 0;
            for (int i = 0; i < n; i++) {
                for (int j = i + 1; j < n; j++) {
                    count++;
                    if (values[i] + values[j] == target) {
                        found = i * 100 + j;
                        goto fail;
                    }
                    if (values[j] < 0) goto fail;
                }
            }
        fail:
            *steps = count;
            return found;
        }
    }

    let values = [1, 2, 3, 9];
    let mut steps = 0;
    unsafe {
        // values[1] + values[2] == 5, found after four inner iterations.
        assert_eq!(find_pair(values.as_ptr(), 4, 5, &mut steps), 102);
        assert_eq!(steps, 4);
        // Nothing matches: the loops run to completion and fall into the label.
        assert_eq!(find_pair(values.as_ptr(), 4, 100, &mut steps), 0);
        assert_eq!(steps, 6);
    }
}

#[test]
fn a_jump_into_a_loop_body_skips_the_first_test() {
    c99! {
        /* Entering a `while` in the middle of its body — legal C, and the
           reason the body cannot stay a Rust loop. */
        int countdown(int n, int start_inside) {
            int steps = 0;
            if (start_inside) goto inside;
            while (n > 0) {
                n--;
            inside:
                steps++;
            }
            return steps * 10 + n;
        }
    }

    unsafe {
        assert_eq!(countdown(3, 0), 30);
        // The jump runs the body once before the condition is ever tested.
        assert_eq!(countdown(0, 1), 10);
        assert_eq!(countdown(2, 1), 30);
    }
}

#[test]
fn siblings_may_share_a_name() {
    c99! {
        /* Three different objects, all called `x`; hoisting them to the top of
           the function has to keep them apart. */
        int shadowing(int n) {
            int total = 0;
            {
                int x = 1;
                total += x;
            }
            {
                int x = 20;
                {
                    int x = 300;
                    total += x;
                }
                total += x;
            }
            int x = 4000;
            total += x;
            if (n) goto out;
            total = -1;
        out:
            return total;
        }
    }

    unsafe {
        assert_eq!(shadowing(1), 4321);
        assert_eq!(shadowing(0), -1);
    }
}

#[test]
fn a_state_machine_written_with_goto() {
    c99! {
        /* Counts the fields of a comma-separated string, ignoring what is
           inside quotes: the shape of a hand-written scanner. */
        int count_fields(const char *s) {
            int fields = 1;
            const char *p = s;

        text:
            if (*p == 0) goto done;
            if (*p == '"') { p++; goto quoted; }
            if (*p == ',') { fields++; p++; goto text; }
            p++;
            goto text;

        quoted:
            if (*p == 0) goto done;
            if (*p == '"') { p++; goto text; }
            p++;
            goto quoted;

        done:
            return fields;
        }
    }

    let input = c"a,b,\"c,d\",e";
    unsafe {
        assert_eq!(count_fields(input.as_ptr()), 4);
        assert_eq!(count_fields(c"".as_ptr()), 1);
        assert_eq!(count_fields(c",,".as_ptr()), 3);
    }
}

#[test]
fn every_path_still_returns_its_own_value() {
    c99! {
        int classify(int n) {
            if (n < 0) goto negative;
            if (n == 0) goto zero;
            if (n < 10) return 1;
            goto big;
        negative:
            return -1;
        zero:
            return 0;
        big:
            return 2;
        }
    }

    unsafe {
        assert_eq!(classify(-5), -1);
        assert_eq!(classify(0), 0);
        assert_eq!(classify(5), 1);
        assert_eq!(classify(50), 2);
    }
}

// ---------------------------------------------------------------------------
// case labels the structured lowering cannot reach
// ---------------------------------------------------------------------------

#[test]
fn duffs_device() {
    c99! {
        /* The original, from Tom Duff's 1983 message: the `switch` jumps into
           the middle of a `do`/`while` body. */
        void copy(char *to, const char *from, int count) {
            int n = (count + 7) / 8;
            switch (count % 8) {
            case 0: do { *to++ = *from++;
            case 7:      *to++ = *from++;
            case 6:      *to++ = *from++;
            case 5:      *to++ = *from++;
            case 4:      *to++ = *from++;
            case 3:      *to++ = *from++;
            case 2:      *to++ = *from++;
            case 1:      *to++ = *from++;
                    } while (--n > 0);
            }
        }
    }

    let from: [u8; 13] = *b"abcdefghijklm";
    let mut to = [0u8; 16];
    unsafe {
        copy(
            to.as_mut_ptr().cast(),
            from.as_ptr().cast(),
            from.len() as i32,
        );
    }
    assert_eq!(&to[..13], &from[..]);
    // The unrolled loop must not have written past the count.
    assert_eq!(&to[13..], &[0, 0, 0]);

    // Every count from 0 to 24 copies exactly that many bytes.
    for count in 0..=24usize {
        let source: Vec<u8> = (0..count).map(|i| (i as u8) + 1).collect();
        let mut target = vec![0u8; count + 4];
        if count > 0 {
            unsafe {
                copy(
                    target.as_mut_ptr().cast(),
                    source.as_ptr().cast(),
                    count as i32,
                );
            }
        }
        assert_eq!(&target[..count], &source[..], "count {count}");
        assert_eq!(&target[count..], &[0, 0, 0, 0], "count {count}");
    }
}

#[test]
fn a_case_label_inside_an_if() {
    c99! {
        int pick(int n, int guard) {
            int out = 0;
            switch (n) {
            case 0:
                out = 1;
                break;
            default:
                if (guard) {
            case 5:
                    out = 50;
                    break;
                }
                out = 99;
            }
            return out;
        }
    }

    unsafe {
        assert_eq!(pick(0, 0), 1);
        assert_eq!(pick(5, 0), 50);
        assert_eq!(pick(5, 1), 50);
        // The `default` group runs the `if`, which is false, and falls past it.
        assert_eq!(pick(7, 0), 99);
        assert_eq!(pick(7, 1), 50);
    }
}

#[test]
fn a_jump_out_of_a_switch() {
    c99! {
        int leave(int n) {
            int out = 0;
            switch (n) {
            case 1:
                out = 10;
                goto after;
            case 2:
                out = 20;
                break;
            default:
                out = 30;
            }
            out += 1;
        after:
            return out;
        }
    }

    unsafe {
        assert_eq!(leave(1), 10);
        assert_eq!(leave(2), 21);
        assert_eq!(leave(3), 31);
    }
}

#[test]
fn a_switch_inside_a_loop_with_a_jump_out_of_both() {
    c99! {
        /* `break` leaves the `switch`, `continue` the loop, and `goto` both. */
        int scan(const int *values, int n) {
            int total = 0;
            for (int i = 0; i < n; i++) {
                switch (values[i]) {
                case 0:
                    continue;
                case -1:
                    goto stop;
                default:
                    total += values[i];
                    break;
                }
                total += 100;
            }
        stop:
            return total;
        }
    }

    let values = [1, 0, 2, -1, 9];
    unsafe {
        assert_eq!(scan(values.as_ptr(), 5), 203);
        assert_eq!(scan(values.as_ptr(), 3), 203);
        assert_eq!(scan(values.as_ptr(), 2), 101);
    }
}
