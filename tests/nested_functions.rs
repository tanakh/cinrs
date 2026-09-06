//! GNU's nested functions, lifted out of the function they were written in.
//!
//! GCC gives a nested function a *static chain*: a hidden pointer to the
//! enclosing frame, and a trampoline written onto the stack when its address is
//! taken. `cinrs` lambda-lifts instead. Every object of the enclosing function
//! the body uses becomes a hidden pointer parameter of a file-scope item, the
//! body reads and writes it through that pointer, and each call site passes the
//! address of the object it has — so the sharing C promises is kept exactly (a
//! store in the nested function is visible in the enclosing one the moment it
//! returns) without any code being written onto the stack.
//!
//! What that costs is the address of a *capturing* nested function, which is
//! the one thing a trampoline is for; `tests/ui/gnu_nested_functions.rs` has
//! that refusal and the two others. A nested function that captures nothing is
//! lifted to a plain function and may be passed to `qsort` like any other.

use cinrs::gnu99;

// ---------------------------------------------------------------------------
// the shape the extension exists for: a helper that reads and writes a local
// ---------------------------------------------------------------------------

gnu99! {
    /* The canonical use. `tally` is `report`'s local; `note` both reads and
       writes it, and `report` sees every write. */
    int report(int a, int b, int c)
    {
        int tally = 0;
        int seen = 0;

        void note(int value)
        {
            tally += value;
            seen += 1;
        }

        note(a);
        note(b);
        note(c);
        return tally * 10 + seen;
    }

    /* The torture-suite shape: the nested function assigns, the enclosing one
       reads the assignment back. */
    int assigned_from_inside(void)
    {
        int i = 0;
        void set(void) { i = 1; }
        set();
        return i;
    }

    /* C allows the call before the object has a value; nothing about the
       lifting changes that. */
    int written_before_read(void)
    {
        int slot;
        void fill(void) { slot = 42; }
        fill();
        return slot;
    }
}

#[test]
fn a_nested_helper_reads_and_writes_the_enclosing_local() {
    assert_eq!(unsafe { report(1, 2, 3) }, 63);
    assert_eq!(unsafe { assigned_from_inside() }, 1);
    assert_eq!(unsafe { written_before_read() }, 42);
}

// ---------------------------------------------------------------------------
// recursion, siblings and two levels of nesting
// ---------------------------------------------------------------------------

gnu99! {
    /* A recursive nested function passes its own environment through. */
    int fib(int n)
    {
        int calls = 0;

        int step(int k)
        {
            calls += 1;
            if (k < 2) return k;
            return step(k - 1) + step(k - 2);
        }

        int value = step(n);
        return value * 1000 + calls;
    }

    /* One nested function calling another, both sharing the same local. */
    int siblings(int n)
    {
        int total = 0;

        void add(int v) { total += v; }
        void add_twice(int v) { add(v); add(v); }

        add(n);
        add_twice(n);
        return total;
    }

    /* Two levels: the innermost uses the *outermost* function's variable, which
       reaches it through the middle one — which never mentions it. */
    int two_levels(int n)
    {
        int outermost = n;

        int middle(int a)
        {
            int inner(int b)
            {
                outermost += b;
                return outermost;
            }
            return inner(a) + inner(a);
        }

        return middle(1) * 100 + outermost;
    }

    /* A nested function may also use the enclosing function's parameter, which
       is an ordinary local once the call has been made. */
    int from_a_parameter(int p)
    {
        int bump(void) { p += 1; return p; }
        int first = bump();
        int second = bump();
        return first * 100 + second * 10 + p;
    }
}

#[test]
fn a_nested_function_may_recurse_call_a_sibling_and_nest_again() {
    // fib(7) is 13, reached in 41 calls.
    assert_eq!(unsafe { fib(7) }, 13_041);
    assert_eq!(unsafe { siblings(5) }, 15);
    // inner(1) gives 1 then 2, so middle is 3 and `outermost` ends at 2.
    assert_eq!(unsafe { two_levels(0) }, 302);
    assert_eq!(unsafe { from_a_parameter(7) }, 8_9_9);
}

// ---------------------------------------------------------------------------
// aggregates: the hidden pointer is a place, so everything still works
// ---------------------------------------------------------------------------

gnu99! {
    struct Point { int x; int y; };

    /* Subscripting, member access, `&` and `sizeof` all go through the hidden
       pointer, and all mean what they meant. */
    int aggregates(void)
    {
        int cells[4] = { 1, 2, 3, 4 };
        struct Point origin = { 10, 20 };

        int touch(void)
        {
            cells[0] = 5;
            cells[3] += cells[1];
            origin.x += 1;
            int *first = &cells[0];
            *first += 1;
            return (int) sizeof cells + (int) sizeof origin + (int) _Alignof(cells);
        }

        int sizes = touch();
        return sizes * 1000000
             + cells[0] * 100000 + cells[3] * 1000
             + origin.x * 10 + origin.y / 10;
    }

    /* A whole struct returned from a nested function that reads a local
       (gcc.c-torture's `nestfunc-7`). */
    struct Point offset_point(int base)
    {
        struct Point make(void)
        {
            struct Point p;
            p.x = base + 1;
            p.y = base + 2;
            return p;
        }
        base = 10;
        return make();
    }

    /* A pointer taken to a captured object is a pointer to the enclosing
       function's object, not to a copy. */
    int address_of_a_captured_local(void)
    {
        int value = 1;
        int *escape(void) { return &value; }
        int *p = escape();
        *p = 9;
        return value;
    }
}

#[test]
fn a_captured_array_or_struct_keeps_every_operation() {
    // sizeof cells is 16, sizeof origin is 8, _Alignof(cells) is 4.
    let sizes = (4 * size_of::<i32>() + 2 * size_of::<i32>() + align_of::<i32>()) as i32;
    assert_eq!(
        unsafe { aggregates() },
        sizes * 1_000_000 + 6 * 100_000 + 6 * 1_000 + 11 * 10 + 2
    );
    let p = unsafe { offset_point(0) };
    assert_eq!((p.x, p.y), (11, 12));
    assert_eq!(unsafe { address_of_a_captured_local() }, 9);
}

// ---------------------------------------------------------------------------
// a nested function that captures nothing is an ordinary function
// ---------------------------------------------------------------------------

gnu99! {
    #include <stdlib.h>

    /* No trampoline is needed for a nested function that uses nothing of the
       enclosing frame, so its address may be taken — which is what makes this
       the shape `qsort` can be handed. */
    int sorted_first(void)
    {
        char data[5] = { 40, 10, 50, 20, 30 };

        int by_value(const void *a, const void *b)
        {
            return *(const char *) a - *(const char *) b;
        }

        qsort(data, 5, 1, by_value);
        return data[0] * 10000 + data[1] * 100 + data[4];
    }

    static int call_through(int (*fn)(int), int v) { return fn(v); }

    int through_a_pointer(int n)
    {
        int doubled(int x) { return x * 2; }
        return call_through(doubled, n);
    }

    static int released;

    /* `cleanup` holds the function in a drop guard, which is its address, so
       it wants the same kind of nested function a callback does. */
    int cleaned_up(void)
    {
        void release(int *p) { released += *p; }

        released = 0;
        {
            int a __attribute__((cleanup(release))) = 3;
            int b __attribute__((cleanup(release))) = 4;
            (void) a; (void) b;
        }
        return released;
    }
}

#[test]
fn a_nested_function_that_captures_nothing_is_addressable() {
    assert_eq!(unsafe { sorted_first() }, 10 * 10_000 + 20 * 100 + 50);
    assert_eq!(unsafe { through_a_pointer(21) }, 42);
    assert_eq!(unsafe { cleaned_up() }, 7);
}

// ---------------------------------------------------------------------------
// declarations, storage and identity
// ---------------------------------------------------------------------------

gnu99! {
    /* `auto int g(int);` is GNU's forward declaration of a nested function,
       and the only way to write two that call each other. */
    int parity(int n)
    {
        int steps = 0;
        auto int even(int);
        int odd(int k) { steps += 1; return k == 0 ? 0 : even(k - 1); }
        int even(int k) { steps += 1; return k == 0 ? 1 : odd(k - 1); }
        return even(n) * 100 + steps;
    }

    /* `__func__` inside a nested body is the nested function's own name. */
    static const char *inner_name(void)
    {
        const char *mine(void) { return __func__; }
        return mine();
    }

    int names_match(void)
    {
        const char *n = inner_name();
        return n[0] == 'm' && n[1] == 'i' && n[2] == 'n' && n[3] == 'e' && n[4] == 0;
    }

    /* A `static` local of a nested function is its own item, and lives across
       calls the way any other does. */
    int counted(void)
    {
        int tick(void)
        {
            static int count;
            count += 1;
            return count;
        }
        tick();
        tick();
        return tick();
    }

    /* A nested function is visible from its definition to the end of the block
       it was written in, and nowhere else — two blocks may each have their
       own `helper`. */
    int block_scoped(void)
    {
        int out = 0;
        {
            int helper(int x) { return x + 1; }
            out += helper(1);
        }
        {
            int helper(int x) { return x + 10; }
            out += helper(1);
        }
        return out;
    }

    /* One at file scope and one nested under the same name: the nested one
       shadows for the rest of its block. */
    int shared_name(int x) { return x * 2; }

    int shadowing(int n)
    {
        int outer = shared_name(n);
        int shared_name(int x) { return x * 3; }
        return outer * 100 + shared_name(n);
    }
}

#[test]
fn nested_declarations_scoping_and_identity() {
    // even(4) is 1, reached in five calls.
    assert_eq!(unsafe { parity(4) }, 105);
    assert_eq!(unsafe { names_match() }, 1);
    assert_eq!(unsafe { counted() }, 3);
    assert_eq!(unsafe { block_scoped() }, 13);
    assert_eq!(unsafe { shadowing(2) }, 400 + 6);
}

// ---------------------------------------------------------------------------
// the two lowerings, and the old-style definition
// ---------------------------------------------------------------------------

gnu99! {
    /* The enclosing function jumps, so it is lowered through a control-flow
       graph; the nested one is a separate item and keeps the structured form.
       Which lowering each gets is decided for each of them on its own. */
    int enclosing_jumps(int n)
    {
        int scale = 3;
        int scaled(int v) { scale += 1; return v * scale; }

        int total = 0;
        if (n < 0) goto negative;
        total = scaled(n);
        goto done;
    negative:
        total = -scaled(-n);
    done:
        return total * 10 + scale;
    }

    /* And the other way round: the *nested* function jumps and the enclosing
       one does not. */
    int nested_jumps(int n)
    {
        int seen = 0;

        int loop_to(int limit)
        {
            int i = 0;
        again:
            if (i >= limit) goto out;
            seen += i;
            i += 1;
            goto again;
        out:
            return i;
        }

        return loop_to(n) * 100 + seen;
    }

    /* A K&R enclosing definition: its parameters are ordinary locals after
       entry, so a nested function captures them like any other. */
    int old_style(n)
        int n;
    {
        int doubled(void) { n *= 2; return n; }
        doubled();
        return n;
    }

    /* And a K&R *nested* definition. */
    int old_style_nested(int n)
    {
        int base = 5;
        int add(x)
            int x;
        {
            return x + base;
        }
        return add(n);
    }
}

#[test]
fn the_two_lowerings_and_old_style_definitions() {
    assert_eq!(unsafe { enclosing_jumps(2) }, 84);
    assert_eq!(unsafe { enclosing_jumps(-2) }, -76);
    // loop_to(4) returns 4 and leaves `seen` at 0+1+2+3.
    assert_eq!(unsafe { nested_jumps(4) }, 406);
    assert_eq!(unsafe { old_style(21) }, 42);
    assert_eq!(unsafe { old_style_nested(4) }, 9);
}

// ---------------------------------------------------------------------------
// the lifted item is private to the unit
// ---------------------------------------------------------------------------

gnu99! {
    #pragma cinrs export

    /* `#pragma cinrs export` gives every function with external linkage a real
       C symbol. A nested function has no linkage at all, so it never gets one
       — the item stays private to the expansion. */
    int exported_host(int n)
    {
        int helper(int x) { return x + n; }
        return helper(1);
    }
}

#[test]
fn a_lifted_function_is_never_exported() {
    assert_eq!(unsafe { exported_host(41) }, 42);
}
