//! Integration tests that *run* translated C using `struct`, `union`, `enum`,
//! `typedef`, aggregate initialisers and function pointers.
//!
//! The layout test at the end is the important one: it checks that the
//! `sizeof` this crate folds into the generated code agrees with the layout
//! `rustc` gives the `#[repr(C)]` types it generates. If those two ever
//! disagreed, every `malloc(sizeof(struct X))` in translated code would be
//! quietly wrong.

use cinrs::c99;

// ---------------------------------------------------------------------------
// structs
// ---------------------------------------------------------------------------

#[test]
fn structs_by_value_and_by_pointer() {
    c99! {
        struct Point { int x; int y; };
        struct Rect { struct Point origin; struct Point size; };

        struct Point make_point(int x, int y) {
            struct Point p;
            p.x = x;
            p.y = y;
            return p;
        }

        int manhattan(struct Point p) {
            int x = p.x < 0 ? -p.x : p.x;
            int y = p.y < 0 ? -p.y : p.y;
            return x + y;
        }

        void translate(struct Point *p, int dx, int dy) {
            p->x += dx;
            p->y += dy;
        }

        int area(struct Rect r) {
            return r.size.x * r.size.y;
        }

        struct Rect grow(struct Rect r, int by) {
            r.size.x += by;
            r.size.y += by;
            return r;
        }

        int assign_copies(int x) {
            struct Point a = make_point(x, x + 1);
            struct Point b;
            b = a;
            a.x = 99;
            return b.x * 10 + b.y;
        }

        int through_temporary(void) {
            /* A member of a struct returned by value. */
            return make_point(3, 4).y;
        }
    }

    unsafe {
        let p = make_point(3, -4);
        assert_eq!((p.x, p.y), (3, -4));
        assert_eq!(manhattan(p), 7);

        let mut q = make_point(1, 1);
        translate(&raw mut q, 4, 5);
        assert_eq!((q.x, q.y), (5, 6));

        let r = Rect {
            origin: make_point(0, 0),
            size: make_point(3, 4),
        };
        assert_eq!(area(r), 12);
        assert_eq!(area(grow(r, 1)), 20);
        assert_eq!(assign_copies(1), 12);
        assert_eq!(through_temporary(), 4);
    }
}

#[test]
fn arrays_of_structs_and_structs_with_arrays() {
    c99! {
        struct Item { int id; int weight; };
        struct Bag { int count; int weights[4]; };

        int heaviest(struct Item *items, int n) {
            int best = 0;
            for (int i = 1; i < n; i++) {
                if (items[i].weight > items[best].weight) {
                    best = i;
                }
            }
            return items[best].id;
        }

        int total(struct Bag *bag) {
            int sum = 0;
            for (int i = 0; i < bag->count; i++) {
                sum += bag->weights[i];
            }
            return sum;
        }

        int local_table(void) {
            struct Item items[3] = {{1, 10}, {2, 30}, {3, 20}};
            return heaviest(items, 3);
        }

        int fill_bag(void) {
            struct Bag bag;
            bag.count = 4;
            for (int i = 0; i < 4; i++) {
                bag.weights[i] = i * i;
            }
            return total(&bag);
        }
    }

    unsafe {
        assert_eq!(local_table(), 2);
        assert_eq!(fill_bag(), 14);
        let mut items = [Item { id: 7, weight: 1 }, Item { id: 8, weight: 5 }];
        assert_eq!(heaviest((&raw mut items).cast(), 2), 8);
    }
}

#[test]
fn a_self_referential_struct() {
    c99! {
        struct Node;

        struct Node {
            int value;
            struct Node *left;
            struct Node *right;
        };

        int depth(struct Node *n) {
            if (n == 0) {
                return 0;
            }
            int left = depth(n->left);
            int right = depth(n->right);
            return 1 + (left > right ? left : right);
        }

        int sum(struct Node *n) {
            if (!n) return 0;
            return n->value + sum(n->left) + sum(n->right);
        }
    }

    unsafe {
        let mut leaf_a = Node {
            value: 2,
            left: core::ptr::null_mut(),
            right: core::ptr::null_mut(),
        };
        let mut leaf_b = Node {
            value: 3,
            left: core::ptr::null_mut(),
            right: core::ptr::null_mut(),
        };
        let mut root = Node {
            value: 1,
            left: &raw mut leaf_a,
            right: &raw mut leaf_b,
        };
        assert_eq!(depth(&raw mut root), 2);
        assert_eq!(sum(&raw mut root), 6);
        assert_eq!(depth(core::ptr::null_mut()), 0);
    }
}

#[test]
fn a_struct_holding_a_function_pointer() {
    c99! {
        struct Op {
            const char *name;
            int (*apply)(int, int);
        };

        int add(int a, int b) { return a + b; }
        int mul(int a, int b) { return a * b; }

        int run(struct Op *op, int a, int b) {
            return op->apply(a, b);
        }

        int dispatch(int which, int a, int b) {
            struct Op ops[2] = {{"add", add}, {"mul", mul}};
            return run(&ops[which], a, b);
        }

        int has_apply(struct Op *op) {
            return op->apply != 0;
        }
    }

    unsafe {
        assert_eq!(dispatch(0, 3, 4), 7);
        assert_eq!(dispatch(1, 3, 4), 12);
        let mut empty = Op {
            name: c"none".as_ptr(),
            apply: None,
        };
        assert_eq!(has_apply(&raw mut empty), 0);
        empty.apply = Some(add);
        assert_eq!(has_apply(&raw mut empty), 1);
        assert_eq!(run(&raw mut empty, 1, 2), 3);
    }
}

// ---------------------------------------------------------------------------
// unions and enums
// ---------------------------------------------------------------------------

#[test]
fn union_type_punning() {
    c99! {
        union Bits {
            float f;
            unsigned int u;
        };

        unsigned int bits_of(float value) {
            union Bits b;
            b.f = value;
            return b.u;
        }

        float float_of(unsigned int bits) {
            union Bits b;
            b.u = bits;
            return b.f;
        }

        unsigned int one_bits(void) {
            union Bits b = {1.0f};
            return b.u;
        }

        unsigned int designated(void) {
            union Bits b = {.u = 0x3f800000};
            return b.u;
        }
    }

    unsafe {
        assert_eq!(bits_of(1.0), 0x3f80_0000);
        assert_eq!(float_of(0x4000_0000), 2.0);
        assert_eq!(one_bits(), 0x3f80_0000);
        assert_eq!(designated(), 0x3f80_0000);
        assert_eq!(float_of(bits_of(-12.5)), -12.5);
    }
}

#[test]
fn enums_with_explicit_values() {
    c99! {
        enum Color { RED, GREEN = 10, BLUE, DARK = -1 };

        enum Color next(enum Color c) {
            if (c == RED) return GREEN;
            if (c == GREEN) return BLUE;
            return RED;
        }

        int value_of(enum Color c) {
            return c;
        }

        int arithmetic(void) {
            return GREEN + BLUE + RED;
        }

        int in_switch(enum Color c) {
            switch (c) {
                case RED: return 100;
                case GREEN: return 200;
                case BLUE: return 300;
                default: return 0;
            }
        }
    }

    unsafe {
        assert_eq!(RED, 0);
        assert_eq!(GREEN, 10);
        assert_eq!(BLUE, 11);
        assert_eq!(DARK, -1);
        assert_eq!(next(RED), GREEN);
        assert_eq!(next(BLUE), RED);
        assert_eq!(value_of(BLUE), 11);
        assert_eq!(arithmetic(), 21);
        assert_eq!(in_switch(GREEN), 200);
        assert_eq!(in_switch(DARK), 0);
        // The alias is usable from Rust, and is `c_int`.
        let c: Color = BLUE;
        assert_eq!(c, 11);
    }
}

#[test]
fn typedefs_of_aggregates() {
    c99! {
        typedef struct Point { int x; int y; } Point;
        typedef struct { double re; double im; } Complex;
        typedef Complex *ComplexPtr;
        typedef int Grid[2][2];

        Point origin(void) {
            Point p = {0, 0};
            return p;
        }

        double magnitude_squared(ComplexPtr z) {
            return z->re * z->re + z->im * z->im;
        }

        int grid_corner(void) {
            Grid g = {{1, 2}, {3, 4}};
            return g[1][1];
        }

        typedef enum { OFF, ON } Switch;

        int toggle(Switch s) {
            return s == OFF ? ON : OFF;
        }
    }

    unsafe {
        let p = origin();
        assert_eq!((p.x, p.y), (0, 0));
        let mut z = Complex { re: 3.0, im: 4.0 };
        assert_eq!(magnitude_squared(&raw mut z), 25.0);
        assert_eq!(grid_corner(), 4);
        assert_eq!(toggle(OFF), ON);
        assert_eq!((OFF, ON), (0, 1));
        // `Point` and `struct Point` are the same Rust type.
        let q: Point = origin();
        assert_eq!(q.x, 0);
    }
}

// ---------------------------------------------------------------------------
// initialisers
// ---------------------------------------------------------------------------

/// The two initialisers 6.7.9 makes constraint violations and every
/// implementation answers with a warning, keeping the part that fits.
///
/// A string literal with no room for its terminator is WG14 **DR114**
/// (`char array[2][5] = { "defghi" }`), and 6.7.9p14 already lets the NUL fall
/// off the end when the array is exactly as long as the text; anything more is
/// dropped, which is what GCC and Clang both do and what `gcc.c-torture`'s
/// `pr86714` was written to check. Braces around a *scalar* hold "a single
/// expression" (6.7.9p11), and the values after the first are dropped the same
/// way.
#[test]
fn an_initializer_with_more_than_fits_keeps_what_does() {
    c99! {
        char exactly[2] = "hi";
        char too_long[3] = "abcde";
        char rows[2][5] = { "defghi", "jk" };

        int scalar = { 1, 2 };

        int exact_at(int i) { return exactly[i]; }
        int long_at(int i) { return too_long[i]; }
        int row_at(int r, int i) { return rows[r][i]; }
        int the_scalar(void) { return scalar; }

        unsigned long sizes(int which) {
            return which ? sizeof exactly : sizeof too_long;
        }

        int local_scalar(void) { int n = { 3, 4 }; return n; }
        int local_string(void) { char s[2] = "long"; return s[1]; }

        /* WG14 DR032/DR035: a comma operator is not part of a constant
           expression (6.6p3), and both compilers take one in a static
           initialiser all the same, keeping the value of the right operand. */
        int comma_at_file_scope = (1, 2);
        int the_comma(void) { return comma_at_file_scope; }
    }

    unsafe {
        // No room for the terminator, so it is left out (6.7.9p14); the
        // characters past the end of the array are simply not part of the
        // value, and the object keeps its declared size.
        assert_eq!(
            (exact_at(0), exact_at(1)),
            (i32::from(b'h'), i32::from(b'i'))
        );
        assert_eq!(sizes(1), 2);
        assert_eq!(
            (long_at(0), long_at(1), long_at(2)),
            (i32::from(b'a'), i32::from(b'b'), i32::from(b'c'))
        );
        assert_eq!(sizes(0), 3);
        assert_eq!(
            (row_at(0, 0), row_at(0, 4)),
            (i32::from(b'd'), i32::from(b'h'))
        );
        assert_eq!((row_at(1, 0), row_at(1, 2)), (i32::from(b'j'), 0));
        assert_eq!(the_scalar(), 1);
        assert_eq!(local_scalar(), 3);
        assert_eq!(local_string(), i32::from(b'o'));
        assert_eq!(the_comma(), 2);
    }
}

#[test]
fn designated_and_partial_initializers() {
    c99! {
        struct Config {
            int width;
            int height;
            int depth;
            const char *name;
        };

        struct Config defaults = {.height = 20, .name = "default"};
        int scores[6] = {[4] = 5, [1] = 2};
        int partial[4] = {7};
        int all_zero[3] = {0};

        struct Config local_defaults(void) {
            struct Config c = {.depth = 3, .width = 1};
            return c;
        }

        struct Config from_values(int w, int h) {
            struct Config c = {w, h};
            return c;
        }

        int score(int i) {
            return scores[i];
        }

        int partial_at(int i) {
            return partial[i];
        }

        int zero_at(int i) {
            return all_zero[i];
        }

        struct Nested { struct Config inner; int tag; };

        int nested(void) {
            struct Nested n = {{.width = 4}, 9};
            return n.inner.width * 10 + n.tag;
        }
    }

    unsafe {
        assert_eq!({ defaults }.width, 0);
        assert_eq!({ defaults }.height, 20);
        assert_eq!(
            core::ffi::CStr::from_ptr({ defaults }.name)
                .to_str()
                .unwrap(),
            "default"
        );
        assert_eq!((score(0), score(1), score(4), score(5)), (0, 2, 5, 0));
        assert_eq!((partial_at(0), partial_at(3)), (7, 0));
        assert_eq!((zero_at(0), zero_at(2)), (0, 0));

        let c = local_defaults();
        assert_eq!((c.width, c.height, c.depth), (1, 0, 3));
        assert!(c.name.is_null());

        let d = from_values(6, 7);
        assert_eq!((d.width, d.height, d.depth), (6, 7, 0));
        assert_eq!(nested(), 49);
    }
}

/// C99 6.7.8's designator *lists*, and the "next subobject" rule that follows
/// one.
///
/// Every expected value here was taken from `gcc -std=c99` compiling the same
/// declarations, member by member — which is the only oracle worth having for
/// a corner of C this fiddly.
#[test]
fn nested_designators_reach_subobjects() {
    c99! {
        struct Pair { int x; int y; };
        struct Boxed { struct Pair p; int tag; };
        struct Row { int cells[3]; int tag; };
        struct Grid { struct Pair pairs[2]; int tag; };
        struct Inner { int p[2]; };
        struct Outer { struct Inner inner[2]; int tag; };

        /* `.p.x` names a member two levels down, and the elements after it
           carry on from there: `p.y`, then `tag`. */
        struct Boxed boxed = {.p.x = 1, 2, 3};
        /* A designator that names an array leaves its braces out. */
        struct Row row = {.cells = 1, 2, 3};
        /* `[1].y` inside an array of structs; the `8` lands on `tag`. */
        struct Grid grid = {.pairs[1].y = 7, 8};
        /* Two designators into the same member merge rather than replace. */
        struct Boxed merged = {.p = {1}, .p.y = 2};
        struct Boxed reversed = {.p.y = 2, .p.x = 1};
        /* Out of the innermost array, into the next one, then out again. */
        struct Outer outer = {.inner[0].p[1] = 5, 6, 7, 8};
        /* A designator after an element whose braces were elided names a
           member of the *outer* object (6.7.8p17). */
        struct Boxed elided = {1, .tag = 5};
        int matrix[2][2] = {1, [1] = {3, 4}};
        int matrix2[2][2] = {1, 2, [1][1] = 9};

        int boxed_at(int i) { return i == 0 ? boxed.p.x : i == 1 ? boxed.p.y : boxed.tag; }
        int row_at(int i) { return i < 3 ? row.cells[i] : row.tag; }
        int grid_at(int i) {
            return i == 0 ? grid.pairs[0].x : i == 1 ? grid.pairs[0].y
                 : i == 2 ? grid.pairs[1].x : i == 3 ? grid.pairs[1].y : grid.tag;
        }
        int merged_at(int i) { return i == 0 ? merged.p.x : i == 1 ? merged.p.y : merged.tag; }
        int reversed_at(int i) {
            return i == 0 ? reversed.p.x : i == 1 ? reversed.p.y : reversed.tag;
        }
        int outer_at(int i) {
            return i < 4 ? outer.inner[i / 2].p[i % 2] : outer.tag;
        }
        int elided_at(int i) { return i == 0 ? elided.p.x : i == 1 ? elided.p.y : elided.tag; }
        int matrix_at(int i) { return matrix[i / 2][i % 2]; }
        int matrix2_at(int i) { return matrix2[i / 2][i % 2]; }

        /* The same rules at block scope, where the initialiser is code. */
        int local_sum(int base) {
            struct Boxed b = {.p.x = base, base + 1, base + 2};
            struct Outer o = {.inner[1].p[0] = base, base + 1, base + 2};
            return b.p.x + b.p.y * 10 + b.tag * 100
                 + o.inner[1].p[0] * 1000 + o.inner[1].p[1] * 10000 + o.tag * 100000;
        }
    }

    unsafe {
        let read =
            |f: unsafe extern "C" fn(i32) -> i32, n| (0..n).map(|i| f(i)).collect::<Vec<_>>();
        assert_eq!(read(boxed_at, 3), [1, 2, 3]);
        assert_eq!(read(row_at, 4), [1, 2, 3, 0]);
        assert_eq!(read(grid_at, 5), [0, 0, 0, 7, 8]);
        assert_eq!(read(merged_at, 3), [1, 2, 0]);
        assert_eq!(read(reversed_at, 3), [1, 2, 0]);
        assert_eq!(read(outer_at, 5), [0, 5, 6, 7, 8]);
        assert_eq!(read(elided_at, 3), [1, 0, 5]);
        assert_eq!(read(matrix_at, 4), [1, 0, 3, 4]);
        assert_eq!(read(matrix2_at, 4), [1, 2, 0, 9]);
        // b = {1, 2, 3}; o.inner[1] = {1, 2}, o.tag = 3.
        assert_eq!(local_sum(1), 1 + 20 + 300 + 1000 + 20000 + 300000);
    }
}

/// A designator into a `union` member, and one that crosses an anonymous
/// member, plus GNU's range designator in a nested position.
#[test]
fn nested_designators_into_unions_and_ranges() {
    cinrs::gnu99! {
        struct Pair { int x; int y; };
        union Holder { int i; double f; struct Pair p; };
        struct Wrapper { union Holder h; int tag; };
        struct Anon { union { int a; int b; }; int tag; };

        struct Wrapper into_union = {.h.p.y = 5, 9};
        union Holder floating = {.f = 1.5};
        /* A union takes one member, and an element after a nested designator
           continues *inside* it. */
        union Holder carried = {.p.x = 1, 2};
        struct Anon anon = {.a = 7, .tag = 8};
        struct Pair spread[3] = {[0 ... 1].x = 3, [2].y = 4};
        struct Row2 { int cells[3]; int tag; };
        struct Row2 ranged = {.cells[0 ... 1] = 3, 9};

        int into_union_at(int i) {
            return i == 0 ? into_union.h.p.x : i == 1 ? into_union.h.p.y : into_union.tag;
        }
        double floating_value(void) { return floating.f; }
        int carried_at(int i) { return i == 0 ? carried.p.x : carried.p.y; }
        int anon_at(int i) { return i == 0 ? anon.a : anon.tag; }
        int spread_at(int i) { return i % 2 == 0 ? spread[i / 2].x : spread[i / 2].y; }
        int ranged_at(int i) { return i < 3 ? ranged.cells[i] : ranged.tag; }
    }

    unsafe {
        let read =
            |f: unsafe extern "C" fn(i32) -> i32, n| (0..n).map(|i| f(i)).collect::<Vec<_>>();
        assert_eq!(read(into_union_at, 3), [0, 5, 9]);
        assert_eq!(floating_value(), 1.5);
        assert_eq!(read(carried_at, 2), [1, 2]);
        assert_eq!(read(anon_at, 2), [7, 8]);
        assert_eq!(read(spread_at, 6), [3, 0, 3, 0, 0, 4]);
        assert_eq!(read(ranged_at, 4), [3, 3, 9, 0]);
    }
}

// ---------------------------------------------------------------------------
// layout
// ---------------------------------------------------------------------------

#[test]
fn sizeof_agrees_with_the_generated_rust_types() {
    c99! {
        struct Small { char a; };
        struct Padded { char a; int b; char c; };
        struct Wide { double a; char b; };
        struct Nested { struct Padded inner; char tail; };
        struct WithArray { char head; int values[3]; char tail; };
        struct WithPointer { char tag; void *data; int n; };
        struct Ordered { int a; int b; };
        union Mixed { char a; int b; double c; };
        union OneByte { char a; char b; };

        unsigned long sizes(int which) {
            switch (which) {
                case 0: return sizeof(struct Small);
                case 1: return sizeof(struct Padded);
                case 2: return sizeof(struct Wide);
                case 3: return sizeof(struct Nested);
                case 4: return sizeof(struct WithArray);
                case 5: return sizeof(struct WithPointer);
                case 6: return sizeof(struct Ordered);
                case 7: return sizeof(union Mixed);
                case 8: return sizeof(union OneByte);
                default: return 0;
            }
        }

        unsigned long offset_of_b(void) {
            struct Padded p;
            char *base = (char *) &p;
            char *field = (char *) &p.b;
            return (unsigned long) (field - base);
        }
    }

    // What the C front end folded `sizeof` into must be what `rustc` lays the
    // generated `#[repr(C)]` types out as.
    unsafe {
        assert_eq!(sizes(0) as usize, size_of::<Small>());
        assert_eq!(sizes(1) as usize, size_of::<Padded>());
        assert_eq!(sizes(2) as usize, size_of::<Wide>());
        assert_eq!(sizes(3) as usize, size_of::<Nested>());
        assert_eq!(sizes(4) as usize, size_of::<WithArray>());
        assert_eq!(sizes(5) as usize, size_of::<WithPointer>());
        assert_eq!(sizes(6) as usize, size_of::<Ordered>());
        assert_eq!(sizes(7) as usize, size_of::<Mixed>());
        assert_eq!(sizes(8) as usize, size_of::<OneByte>());
        assert_eq!(offset_of_b() as usize, core::mem::offset_of!(Padded, b));
    }

    // And the alignments have to agree too, since the sizes are derived from
    // them.
    assert_eq!(align_of::<Padded>(), align_of::<core::ffi::c_int>());
    assert_eq!(align_of::<Wide>(), align_of::<core::ffi::c_double>());
    assert_eq!(align_of::<Small>(), 1);
}

#[test]
fn sizeof_agrees_with_the_generated_rust_types_for_bit_fields() {
    c99! {
        /* A bit-field is not a Rust field, so a run of them shares an
         * `[u8; K]` and everything after it has to be reconciled by hand.
         * These are the shapes where that goes wrong if it is done naively. */
        struct Packed { char a : 2; int b : 30; };
        struct Split { char c; int x : 9; char d; };
        struct Moved { char c; short x : 9; };
        struct Zeroed { unsigned int a : 3; unsigned int : 0; unsigned int b : 3; };
        struct Trailing { int a; long long : 0; };
        struct After { char a : 3; double d; char b; };
        union Overlaid { unsigned int a : 3; unsigned int b : 20; double d; };

        unsigned long sizes(int which) {
            switch (which) {
                case 0: return sizeof(struct Packed);
                case 1: return sizeof(struct Split);
                case 2: return sizeof(struct Moved);
                case 3: return sizeof(struct Zeroed);
                case 4: return sizeof(struct Trailing);
                case 5: return sizeof(struct After);
                case 6: return sizeof(union Overlaid);
                default: return 0;
            }
        }

        unsigned long offsets(int which) {
            switch (which) {
                case 0: return __builtin_offsetof(struct Split, d);
                case 1: return __builtin_offsetof(struct After, d);
                case 2: return __builtin_offsetof(struct After, b);
                default: return 0;
            }
        }
    }

    // What the C front end folded `sizeof` into must be what `rustc` lays the
    // generated `#[repr(C)]` types out as — bit-field storage, padding and
    // raised alignment included.
    unsafe {
        assert_eq!(sizes(0) as usize, size_of::<Packed>());
        assert_eq!(sizes(1) as usize, size_of::<Split>());
        assert_eq!(sizes(2) as usize, size_of::<Moved>());
        assert_eq!(sizes(3) as usize, size_of::<Zeroed>());
        assert_eq!(sizes(4) as usize, size_of::<Trailing>());
        assert_eq!(sizes(5) as usize, size_of::<After>());
        assert_eq!(sizes(6) as usize, size_of::<Overlaid>());

        assert_eq!(offsets(0) as usize, core::mem::offset_of!(Split, d));
        assert_eq!(offsets(1) as usize, core::mem::offset_of!(After, d));
        assert_eq!(offsets(2) as usize, core::mem::offset_of!(After, b));
    }

    // The numbers themselves, as gcc gives them.
    assert_eq!((size_of::<Packed>(), align_of::<Packed>()), (4, 4));
    assert_eq!((size_of::<Split>(), align_of::<Split>()), (4, 4));
    assert_eq!((size_of::<Moved>(), align_of::<Moved>()), (4, 2));
    assert_eq!((size_of::<Zeroed>(), align_of::<Zeroed>()), (8, 4));
    // A trailing `long long : 0` grows the record with no storage of its own,
    // and — being unnamed — leaves its alignment alone.
    assert_eq!((size_of::<Trailing>(), align_of::<Trailing>()), (8, 4));
    assert_eq!((size_of::<After>(), align_of::<After>()), (24, 8));
    assert_eq!((size_of::<Overlaid>(), align_of::<Overlaid>()), (8, 8));

    // And the storage a run lives in starts at the byte its first bit is in,
    // which is what makes the members after it land where C puts them.
    assert_eq!(core::mem::offset_of!(Packed, __cinrs_bits0), 0);
    assert_eq!(core::mem::offset_of!(Split, __cinrs_bits0), 1);
    assert_eq!(core::mem::offset_of!(Split, d), 3);
    // `short x : 9` cannot straddle a `short`, so it starts at bit 16 and the
    // byte before it is explicit padding.
    assert_eq!(core::mem::offset_of!(Moved, __cinrs_bits0), 2);
    assert_eq!(core::mem::offset_of!(After, d), 8);
    assert_eq!(core::mem::offset_of!(After, b), 16);
}

#[test]
fn initializers_may_arrive_in_any_order() {
    c99! {
        struct Rgb { int r; int g; int b; };

        struct Rgb reversed(void) {
            struct Rgb c = {.b = 3, .g = 2, .r = 1};
            return c;
        }

        int sparse[5] = {[4] = 40, [1] = 10};

        int sparse_at(int i) {
            return sparse[i];
        }

        struct Rgb palette[2] = {[1] = {.g = 9}};

        int palette_green(int i) {
            return palette[i].g;
        }
    }

    unsafe {
        let c = reversed();
        assert_eq!((c.r, c.g, c.b), (1, 2, 3));
        assert_eq!(
            (
                sparse_at(0),
                sparse_at(1),
                sparse_at(2),
                sparse_at(3),
                sparse_at(4)
            ),
            (0, 10, 0, 0, 40)
        );
        assert_eq!((palette_green(0), palette_green(1)), (0, 9));
    }
}

#[test]
fn a_static_table_of_function_pointers() {
    c99! {
        int twice(int n) { return n * 2; }
        int square(int n) { return n * n; }

        typedef int (*Unary)(int);

        static Unary table[2] = {twice, square};
        static Unary chosen = square;

        int apply(int which, int n) {
            return table[which](n);
        }

        int apply_chosen(int n) {
            return chosen(n);
        }

        void choose(Unary f) {
            chosen = f;
        }
    }

    unsafe {
        assert_eq!(apply(0, 5), 10);
        assert_eq!(apply(1, 5), 25);
        assert_eq!(apply_chosen(4), 16);
        choose(Some(twice));
        assert_eq!(apply_chosen(4), 8);
    }
}

#[test]
fn aggregates_reached_through_pointers_and_arrays() {
    c99! {
        void *malloc(unsigned long size);
        void free(void *p);

        struct Inner { int values[3]; };
        struct Outer { struct Inner inner; struct Inner *link; };

        int inner_at(struct Outer *o, int i) {
            return o->inner.values[i];
        }

        int link_at(struct Outer *o, int i) {
            return o->link->values[i];
        }

        int through_deref(struct Outer *o, int i) {
            return (*o).inner.values[i];
        }

        struct Inner copy_of(const struct Inner *p) {
            struct Inner copy = *p;
            return copy;
        }

        int sum_of(struct Inner v) {
            return v.values[0] + v.values[1] + v.values[2];
        }

        void store(struct Inner *table, int i, struct Inner v) {
            table[i] = v;
        }

        int allocate_and_use(void) {
            struct Outer *o = (struct Outer *) malloc(sizeof(struct Outer));
            o->inner.values[0] = 1;
            o->inner.values[1] = 2;
            o->inner.values[2] = 3;
            o->link = &o->inner;
            int total = sum_of(copy_of(o->link)) + link_at(o, 2) + inner_at(o, 0)
                + through_deref(o, 1);
            free(o);
            return total;
        }

        int fill_table(void) {
            struct Inner table[2];
            struct Inner v = {{7, 8, 9}};
            store(table, 1, v);
            return sum_of(table[1]);
        }
    }

    unsafe {
        // 6 + 3 + 1 + 2
        assert_eq!(allocate_and_use(), 12);
        assert_eq!(fill_table(), 24);
    }
}

#[test]
fn integer_constants_cast_to_a_function_pointer_type() {
    // `(void (*)(void))0` is a null function pointer, and Rust's `Option<fn>`
    // has to be told *which* function type it is null of: a bare
    // `Option::None` there is `E0282`, even where the value is never called.
    // A non-zero constant is an implementation-defined conversion, and goes
    // through `usize`, since the integer type C names need not be
    // pointer-sized. (c-testsuite 00159.)
    //
    // The round trip goes through `uintptr_t` rather than `unsigned long`,
    // which is the type C has for "an integer a pointer survives": `long` is
    // four bytes on Windows and would truncate the address, and a truncated
    // function pointer called is an access violation rather than a test
    // failure. `a_narrow_constant` below is the case where the integer is
    // deliberately *not* wide enough.
    c99! {
        #include <stdint.h>

        typedef void (*Action)(void);
        typedef int (*Unary)(int);

        int negate(int n) { return -n; }

        int never_called(void) {
            void (*f)(void) = (void (*)(void)) 0;
            if (f) {
                f();
                return 1;
            }
            return 0;
        }

        int null_forms(void) {
            Action a = (Action) 0;
            Action b = (void *) 0;
            Unary c = 0;
            return (a == 0) + (b == 0) + (c == 0);
        }

        Unary from_integer(uintptr_t bits) { return (Unary) bits; }
        uintptr_t to_integer(Unary f) { return (uintptr_t) f; }
        int roundtrip(int n) { return from_integer(to_integer(negate))(n); }

        int a_narrow_constant(void) {
            /* The constant has type `int`, which is half a pointer wide. */
            return (Unary) 1 == 0;
        }
    }

    unsafe {
        assert_eq!(never_called(), 0);
        assert_eq!(null_forms(), 3);
        assert_eq!(roundtrip(7), -7);
        assert!(to_integer(Some(negate)) != 0);
        assert_eq!(a_narrow_constant(), 0);
    }
}

#[test]
fn function_pointers_cast_to_and_from_other_pointers() {
    c99! {
        typedef int (*Unary)(int);

        int negate(int n) { return -n; }

        void *as_data(Unary f) {
            return (void *) f;
        }

        Unary as_function(void *p) {
            return (Unary) p;
        }

        int roundtrip(int n) {
            return as_function(as_data(negate))(n);
        }
    }

    unsafe {
        assert_eq!(roundtrip(7), -7);
        assert!(!as_data(Some(negate)).is_null());
    }
}
