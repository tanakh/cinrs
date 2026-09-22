//! Integration tests that *run* translated C using `goto` and the `case`
//! labels a `switch` cannot reach without one.
//!
//! There are two lowerings and both are here. An **outward** `goto` — forwards
//! to a label later in a block it is inside, or backwards to one that block
//! begins with — keeps Rust's own control flow: the statements the label
//! divides become a labelled block or a labelled loop, and the jump is the
//! `break` or the `continue` that leaves or restarts it. Anything else — a
//! jump *into* a block, a computed `goto`, a `case` label inside a loop the
//! `switch` wraps — is lowered through a control-flow graph, where every local
//! of the function is hoisted to the top and renamed and the body becomes a
//! state machine. These tests are what says both still compute what the C did.

use cinrs::{c99, gnu99};

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
// the jumps that keep Rust's own control flow
// ---------------------------------------------------------------------------

/// The cleanup idiom: several labels near the end of the function, each one
/// undoing a little more, and a `goto` to whichever is right from wherever the
/// failure was found. Each label ends a labelled block, and the blocks nest.
#[test]
fn a_cleanup_chain_of_several_labels() {
    c99! {
        static int trace[16];
        static int traced;

        static void note(int step) { trace[traced++] = step; }

        /* `open` counts as the resource that has to be released, and each
           failure jumps to the label that releases exactly what it took. */
        int prepare(int fail_at) {
            int rc = 0;
            traced = 0;
            note(1);
            if (fail_at == 1) { rc = -1; goto fail_none; }
            note(2);
            if (fail_at == 2) { rc = -2; goto fail_one; }
            note(3);
            if (fail_at == 3) { rc = -3; goto fail_two; }
            rc = 100;
        fail_two:
            note(30);
        fail_one:
            note(20);
        fail_none:
            note(10);
            return rc * 1000 + traced;
        }
    }

    unsafe {
        // Nothing fails: every release runs, so five steps.
        assert_eq!(prepare(0), 100 * 1000 + 6);
        assert_eq!(prepare(1), -1000 + 2);
        assert_eq!(prepare(2), -2 * 1000 + 4);
        assert_eq!(prepare(3), -3 * 1000 + 6);
    }
}

/// A backward `goto` is a loop: the label opens one, and the jump is the
/// `continue` that restarts it. A label that is *also* jumped forwards to gets
/// a block in front of the loop, and the forward jump falls into it.
#[test]
fn a_backward_goto_becomes_a_loop() {
    c99! {
        /* Nothing but a backward jump. */
        int gcd(int a, int b) {
            int t;
        retry:
            if (b != 0) {
                t = a % b;
                a = b;
                b = t;
                goto retry;
            }
            return a;
        }

        /* Both directions at one label: the forward jump skips the negation
           and enters the loop from the top. */
        int normalise(int n, int skip) {
            int steps = 0;
            if (skip) goto retry;
            n = -n;
        retry:
            steps++;
            if (n < 0) {
                n += 10;
                goto retry;
            }
            return steps * 1000 + n;
        }

        /* A backward jump out of a `do`/`while`, whose own `continue` and
           `break` still mean what C said. */
        int scan(const char *s) {
            int rounds = 0;
            int total = 0;
        again:
            rounds++;
            do {
                if (*s == 0) break;
                if (*s == ' ') { s++; continue; }
                total += *s - '0';
                s++;
                if (total > 9) goto again;
            } while (1);
            return rounds * 1000 + total;
        }
    }

    unsafe {
        assert_eq!(gcd(48, 18), 6);
        assert_eq!(gcd(7, 0), 7);
        // -25 → -15 → -5 → 5, entered at the top: four passes.
        assert_eq!(normalise(25, 0), 4 * 1000 + 5);
        // 25 stays positive, so the loop runs once.
        assert_eq!(normalise(25, 1), 1000 + 25);
        // "1 2 3 4" passes nine at the fourth digit, which restarts the scan
        // over what is left of the string — nothing, so it ends there.
        assert_eq!(scan(c"1 2 3 4".as_ptr()), 2 * 1000 + 10);
        assert_eq!(scan(c"1 2".as_ptr()), 1000 + 3);
    }
}

/// A jump out of a nest of loops, a `switch` and a `do`/`while`, mixed with
/// the `break`s and `continue`s that belong to them.
#[test]
fn a_jump_out_of_everything_at_once() {
    c99! {
        int walk(const int *values, int n) {
            int total = 0;
            int rows = 0;
            for (int i = 0; i < n; i++) {
                rows++;
                do {
                    switch (values[i]) {
                    case 0:
                        continue;          /* the `do`, not the `for` */
                    case 9:
                        goto stop;         /* out of all three */
                    case 1:
                        break;             /* the `switch` */
                    default:
                        total += values[i];
                    }
                    total += 1000;
                } while (0);
                if (values[i] == 1) continue;
                total += 100;
            }
        stop:
            return rows * 1000000 + total;
        }
    }

    let values = [2, 1, 0, 5, 9, 7];
    unsafe {
        // 2: 1002 + 100; 1: 1000; 0: nothing; 5: 1005 + 100; 9: stop.
        assert_eq!(walk(values.as_ptr(), 6), 5 * 1000000 + 3307);
        assert_eq!(walk(values.as_ptr(), 1), 1000000 + 1102);
    }
}

/// Two labels in one compound statement, a declaration after one of them, and
/// a `goto` that leaves a block on the way — the region a jump skips may hold
/// scopes of its own.
#[test]
fn two_labels_and_a_declaration_in_one_block() {
    c99! {
        int classify(int n) {
            int out = 0;
            {
                if (n < 0) goto negative;
                if (n == 0) goto zero;
                out = 1;
            }
            out += 10;
        zero:
            out += 100;
            /* A declaration after a label is fine: it is outside the block the
               jump leaves. */
            int doubled = out * 2;
            out = doubled;
        negative:
            return out;
        }
    }

    unsafe {
        assert_eq!(classify(-1), 0);
        assert_eq!(classify(0), 200);
        assert_eq!(classify(5), 222);
    }
}

/// A `cleanup` variable in a scope the jump leaves is released by the `break`
/// itself, innermost first, and one whose scope the jump stays inside is not.
#[test]
fn cleanups_run_on_the_way_out_of_a_region() {
    gnu99! {
        static int trace[16];
        static int traced;

        static void note(int *p) { trace[traced++] = *p; }

        int layered(int leave) {
            traced = 0;
            {
                int outer __attribute__((cleanup(note))) = 1;
                {
                    int inner __attribute__((cleanup(note))) = 2;
                    if (leave) goto done;
                    trace[traced++] = 99;
                }
                trace[traced++] = 98;
            }
        done:
            return traced * 1000 + trace[0] * 100 + trace[1] * 10 + trace[2];
        }

        /* Once per pass through the loop the backward jump makes. */
        int per_pass(void) {
            int i = 0;
            traced = 0;
        again:
            {
                int a __attribute__((cleanup(note))) = i;
                if (++i < 3) goto again;
            }
            return traced * 1000 + trace[0] * 100 + trace[1] * 10 + trace[2];
        }
    }

    unsafe {
        // The jump drops `inner` (2) and then `outer` (1).
        assert_eq!(layered(1), 2 * 1000 + 2 * 100 + 10);
        // Falling through: 99, `inner`, 98, `outer`.
        assert_eq!(layered(0), 4 * 1000 + 99 * 100 + 2 * 10 + 98);
        // The three values are 0, 1 and 2, in that order.
        assert_eq!(per_pass(), 3 * 1000 + 10 + 2);
    }
}

/// A variable length array in the region a jump skips: its storage is freed on
/// the way out, exactly as leaving the block any other way would free it.
#[test]
fn a_variable_length_array_in_the_skipped_region() {
    c99! {
        int sums(int n, int leave) {
            int total = 0;
            {
                int a[n];
                for (int i = 0; i < n; i++) a[i] = i + 1;
                total += a[n - 1];
                if (leave) goto done;
                total += a[0];
            }
            total += 1000;
        done:
            return total;
        }
    }

    unsafe {
        assert_eq!(sums(4, 0), 4 + 1 + 1000);
        assert_eq!(sums(4, 1), 4);
    }
}

/// A C label may be called anything C allows — a Rust keyword, or one of the
/// names the expansion gives its own loops and `switch`es. The generated label
/// is renamed where it has to be, and the jump still lands where C said.
#[test]
fn a_label_may_be_named_anything() {
    c99! {
        /* `loop` is a Rust keyword, which no Rust label may be. */
        int keyword(int n) {
            int total = 0;
            while (n > 0) {
                n--;
                if (n == 2) goto loop;
                total += n;
            }
        loop:
            return total;
        }

        /* `l0` and `sw0` are what the expansion calls the first loop and the
           first `switch` of a function: a `break` naming one of those instead
           of the region would leave the `switch` rather than the label. */
        int generated(int n) {
            if (n == 0) goto l0;
            n += 10;
        l0:
            switch (n) {
            case 1:
                n = 1000;
                goto sw0;
            default:
                n += 100;
            }
            n += 1;
        sw0:
            return n;
        }

        /* An extended identifier, which Rust cannot spell as a label at all. */
        int café(int n) {
            if (n < 0) goto naïve;
            n *= 2;
        naïve:
            return n;
        }
    }

    unsafe {
        // n = 4 and 3 are added, and n = 2 leaves the loop through the label.
        assert_eq!(keyword(5), 7);
        assert_eq!(keyword(2), 1);
        assert_eq!(generated(0), 101);
        assert_eq!(generated(1), 112);
        assert_eq!(generated(-9), 1000);
        assert_eq!(café(-3), -3);
        assert_eq!(café(3), 6);
    }
}

/// The shapes that keep the state machine, all still computing what they must.
#[test]
fn the_jumps_rust_cannot_make_still_work() {
    c99! {
        /* Backwards to a label inside a block the jump is not in. */
        int into_a_block(int n) {
            int total = 0;
            {
            again:
                total += n;
                n--;
            }
            if (n > 0) goto again;
            return total;
        }

        /* A declaration between the jump and the label it names, which no
           labelled block may hold. */
        int declaring(int n) {
            if (n < 0) goto done;
            int doubled = n * 2;
            n = doubled;
        done:
            return n;
        }
    }

    unsafe {
        assert_eq!(into_a_block(3), 6);
        assert_eq!(into_a_block(0), 0);
        assert_eq!(declaring(-3), -3);
        assert_eq!(declaring(4), 8);
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

// ---------------------------------------------------------------------------
// labels as values (GNU computed goto)
// ---------------------------------------------------------------------------

/// The construct's reason for existing: a threaded interpreter, whose dispatch
/// is a jump through a table of label addresses rather than a `switch`.
#[test]
fn a_threaded_dispatch_loop() {
    c99! {
        enum { OP_PUSH, OP_ADD, OP_DUP, OP_HALT };

        int run(const int *code, int n) {
            static void *table[] = { &&do_push, &&do_add, &&do_dup, &&do_halt };
            int stack[16];
            int sp = 0;
            int pc = 0;
            (void) n;
            goto *table[code[pc]];

        do_push:
            stack[sp++] = code[++pc];
            pc++;
            goto *table[code[pc]];

        do_add:
            stack[sp - 2] = stack[sp - 2] + stack[sp - 1];
            sp--;
            pc++;
            goto *table[code[pc]];

        do_dup:
            stack[sp] = stack[sp - 1];
            sp++;
            pc++;
            goto *table[code[pc]];

        do_halt:
            return stack[sp - 1];
        }
    }

    // push 3, push 4, add, dup, add, halt  =>  (3 + 4) * 2
    let code = [0, 3, 0, 4, 1, 2, 1, 3];
    assert_eq!(unsafe { run(code.as_ptr(), 8) }, 14);
}

/// `&&label` is an ordinary rvalue: it goes into a variable, through a `?:`,
/// and into a function's own `void *` parameter-shaped local.
#[test]
fn a_label_address_is_an_ordinary_value() {
    c99! {
        int pick(int c) {
            void *p = c ? &&yes : &&no;
            goto *p;
        yes:
            return 1;
        no:
            return 0;
        }

        /* Straight through the conditional, with no variable in between. */
        int pick_inline(int c) {
            goto *(c ? &&hit : &&miss);
        hit:
            return 10;
        miss:
            return 20;
        }

        /* Reassigned in a loop, which is the "next state" idiom. */
        int walk(int steps) {
            void *next = &&step;
            int seen = 0;
            goto *next;
        step:
            seen++;
            next = (seen < steps) ? &&step : &&out;
            goto *next;
        out:
            return seen;
        }
    }

    unsafe {
        assert_eq!(pick(1), 1);
        assert_eq!(pick(0), 0);
        assert_eq!(pick_inline(1), 10);
        assert_eq!(pick_inline(0), 20);
        assert_eq!(walk(4), 4);
    }
}

/// A label whose address is taken keeps a state of its own, and ordinary
/// `goto`, `switch` and loops still reach it in the same function.
#[test]
fn label_addresses_mix_with_the_ordinary_jumps() {
    c99! {
        int mixed(int which, int n) {
            void *target = &&fallback;
            int total = 0;
            switch (which) {
            case 0:
                target = &&doubled;
                break;
            case 1:
                target = &&halved;
                break;
            default:
                goto fallback;
            }
            goto *target;

        doubled:
            total = n * 2;
            goto done;
        halved:
            total = n / 2;
            goto done;
        fallback:
            for (int i = 0; i < n; i++) {
                total += i;
            }
        done:
            return total;
        }
    }

    unsafe {
        assert_eq!(mixed(0, 21), 42);
        assert_eq!(mixed(1, 42), 21);
        assert_eq!(mixed(7, 4), 6);
    }
}

/// A label address may be taken without any `goto *` being written at all, and
/// two different labels never share a value.
#[test]
fn a_label_address_is_a_value_of_its_own() {
    c99! {
        int distinct(void) {
            void *a = &&first;
            void *b = &&second;
            if (a == b) return 0;
            if (a == 0 || b == 0) return 0;
            goto *a;
        first:
            if (&&first != a) return 0;
            goto *b;
        second:
            return 1;
        }
    }

    assert_eq!(unsafe { distinct() }, 1);
}

/// GCC makes the *difference* of two label addresses an integer constant, so a
/// jump table can be a `static` array of offsets rather than of pointers.
#[test]
fn a_table_of_label_differences() {
    c99! {
        int offsets(int x) {
            static int table[] = { &&second - &&first, &&third - &&first };
            void *target = &&first + table[x];
            int out = 0;
            goto *target;
        second:
            out += 2;
        third:
            out += 1;
        first:
            return out;
        }
    }

    unsafe {
        assert_eq!(offsets(0), 3);
        assert_eq!(offsets(1), 1);
    }
}

// ---------------------------------------------------------------------------
// the shapes the relooper recovers
// ---------------------------------------------------------------------------
//
// Everything below goes through the graph, and the graph is read back into
// Rust's own loops and branches — see `doc/translation.md`. The tiers are
// asserted in `crates/cinrs-core/tests/cfg.rs`; what these say is that each
// shape still computes what the C did.

/// `sqlite3VdbeExec`'s idiom in miniature: a label inside one `case` that four
/// other `case`s jump to. Reducible, so it comes out as a `match` inside a
/// `loop` with no state variable at all.
#[test]
fn a_label_shared_between_switch_cases() {
    c99! {
        int run(int n, const int *ops) {
            int rc = 0;
            int i = 0;
            for (;;) {
                switch (ops[i]) {
                case 1: rc += 1; break;
                case 2: if (n < 0) goto fail; rc += 2; break;
                case 3: rc += 3; goto fail;
                case 4: rc += 4; break;
                case 5:
                    rc = 5;
                fail:
                    rc = -rc;
                    goto done;
                default:
                    goto done;
                }
                i++;
            }
        done:
            return rc;
        }
    }

    unsafe {
        // 1, 4, 1 and then 0, which is the `default` that leaves.
        let ops = [1i32, 4, 1, 0];
        assert_eq!(run(1, ops.as_ptr()), 6);
        // `case 2` with a negative `n` jumps into `case 5`'s label.
        let ops = [1i32, 2];
        assert_eq!(run(-1, ops.as_ptr()), -1);
        assert_eq!(run(1, [1i32, 2, 0].as_ptr()), 3);
        // `case 3` adds and then jumps to the same label.
        assert_eq!(run(1, [3i32].as_ptr()), -3);
        // `case 5` falls into it.
        assert_eq!(run(1, [1i32, 5].as_ptr()), -5);
    }
}

/// The `statemachine` benchmark's shape: a dozen labels jumping among one
/// another in both directions. Every cycle has one head, so it is reducible
/// and becomes nested Rust loops named after the C labels.
#[test]
fn a_lexer_written_as_a_dozen_labels() {
    c99! {
        int lex(const char *p) {
            int words = 0, numbers = 0, strings = 0, comments = 0, punct = 0;
        start:
            if (*p == '\0') goto done;
            if (*p == ' ' || *p == '\t' || *p == '\n') { p++; goto start; }
            if (*p >= 'a' && *p <= 'z') goto in_word;
            if (*p >= '0' && *p <= '9') goto in_number;
            if (*p == '"') goto in_string;
            if (*p == '/') goto maybe_comment;
            punct++;
            p++;
            goto start;
        in_word:
            p++;
            if ((*p >= 'a' && *p <= 'z') || (*p >= '0' && *p <= '9')) goto in_word;
            words++;
            goto start;
        in_number:
            p++;
            if (*p >= '0' && *p <= '9') goto in_number;
            numbers++;
            goto start;
        in_string:
            p++;
            if (*p == '\0') goto unterminated;
            if (*p != '"') goto in_string;
            p++;
            strings++;
            goto start;
        unterminated:
            strings--;
            goto done;
        maybe_comment:
            p++;
            if (*p != '*') { punct++; goto start; }
            p++;
            goto in_comment;
        in_comment:
            if (*p == '\0') goto done;
            if (*p == '*') goto comment_star;
            p++;
            goto in_comment;
        comment_star:
            p++;
            if (*p != '/') goto in_comment;
            p++;
            comments++;
            goto start;
        done:
            return words + 10 * numbers + 100 * strings + 1000 * comments + 10000 * punct;
        }
    }

    let lex = |text: &str| unsafe {
        let owned = std::ffi::CString::new(text).expect("no interior NUL");
        lex(owned.as_ptr())
    };
    // two words, one number, one string, one comment, two punctuation.
    assert_eq!(lex("ab c1 42 \"s\" /*x*/ +-"), 2 + 10 + 100 + 1000 + 20000);
    assert_eq!(lex(""), 0);
    // A `/` that opens no comment is punctuation of its own.
    assert_eq!(lex("/x"), 10000 + 1);
    // An unterminated string takes its own count back off again.
    assert_eq!(lex("\"open"), -100);
    // A comment that never closes ends the scan where it is.
    assert_eq!(lex("/*forever"), 0);
}

/// Two loops that jump into each other: one cycle with two heads, which no
/// arrangement of Rust's blocks can enter twice, so the region — and only the
/// region — dispatches on a state variable.
#[test]
fn two_loops_that_jump_into_each_other() {
    c99! {
        int zigzag(int n, int odd) {
            int t = 0;
            if (odd) goto b;
        a:
            t += n;
            if (--n <= 0) return t;
        b:
            t += 2 * n;
            if (--n <= 0) return t;
            goto a;
        }
    }

    // n = 4: a adds 4, b adds 2*3, a adds 2, b adds 2*1 → 4+6+2+2 = 14.
    assert_eq!(unsafe { zigzag(4, 0) }, 14);
    // Entering at b: 2*4 then 3 then 2*2 then 1 → 8+3+4+1 = 16.
    assert_eq!(unsafe { zigzag(4, 1) }, 16);
    assert_eq!(unsafe { zigzag(1, 0) }, 1);
    assert_eq!(unsafe { zigzag(1, 1) }, 2);
}

/// A loop with three ways out, to three different labels — and a declaration
/// between the loop and them, which is what keeps it off the structured path.
#[test]
fn a_loop_with_three_exits_to_three_labels() {
    c99! {
        int pick(int n) {
            int t = 0;
            while (n > 0) {
                if (n == 3) goto one;
                if (n == 5) goto two;
                if (n == 7) goto three;
                t += n;
                n--;
            }
            int extra = n + 1;
            t += extra;
        one:
            return t - 1;
        two:
            return t - 2;
        three:
            return t - 3;
        }
    }

    unsafe {
        // n = 2: 2 + 1 then the fall-out adds `extra` = 1, then `one` takes 1.
        assert_eq!(pick(2), 2 + 1 + 1 - 1);
        // n = 4: 4 then n == 3 leaves through `one`.
        assert_eq!(pick(4), 4 - 1);
        // n = 6: 6 then n == 5 leaves through `two`.
        assert_eq!(pick(6), 6 - 2);
        // n = 8: 8 then n == 7 leaves through `three`.
        assert_eq!(pick(8), 8 - 3);
    }
}

/// Nested loops left through labels of their own, from a function the graph
/// has to lower: `goto continue_outer` and `goto break_outer` out of the
/// inner one, and a jump into the outer one's body to force the graph.
#[test]
fn continue_outer_and_break_outer_through_the_graph() {
    c99! {
        int walk(int n, int m, int start_inside) {
            int t = 0;
            int i = 0, j;
            if (start_inside) goto inner;
            for (i = 0; i < n; i++) {
            inner:
                for (j = 0; j < m; j++) {
                    if (j == 3) goto continue_outer;
                    if (i == 2) goto break_outer;
                    t += j + 1;
                }
            continue_outer: ;
            }
        break_outer:
            return t * 10 + i;
        }
    }

    unsafe {
        // Two passes of 1 + 2 + 3, then the loop's own test ends it.
        assert_eq!(walk(2, 10, 0), (6 + 6) * 10 + 2);
        // The same two passes, and then `i == 2` leaves everything at once.
        assert_eq!(walk(4, 10, 0), (6 + 6) * 10 + 2);
        // m = 2 never reaches `j == 3`; the inner loop's own test ends it.
        assert_eq!(walk(2, 2, 0), (3 + 3) * 10 + 2);
        // Entering the outer body without running the `for`'s init clause.
        assert_eq!(walk(1, 10, 1), 6 * 10 + 1);
        assert_eq!(walk(0, 10, 0), 0);
    }
}

/// A `switch` inside a `do`/`while` whose `continue` leaves the `switch` and
/// not the loop, entered by a `goto` into one of its groups.
#[test]
fn a_switch_inside_a_do_while_with_continue() {
    c99! {
        int tally(int n) {
            int t = 0;
            if (n > 100) goto odd;
            do {
                switch (n & 3) {
                case 0: t += 1; break;
                case 1:
                odd:
                    t += 2;
                    continue;
                default: t += 4; break;
                }
                t += 8;
            } while (--n > 0);
            return t;
        }
    }

    unsafe {
        // n = 4: 4&3 == 0 → 1 + 8; n = 3: 4 + 8; n = 2: 4 + 8; n = 1: 2 and
        // `continue`, which ends the loop.
        assert_eq!(tally(4), 9 + 12 + 12 + 2);
        // Entering at the label: 2, then the `continue` tests `--n > 0` with
        // n still 101, so the loop runs on from there.
        assert_eq!(tally(101), 877);
        assert_eq!(tally(1), 2);
    }
}
