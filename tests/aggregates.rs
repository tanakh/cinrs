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
    c99! {
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

        Unary from_integer(unsigned long bits) { return (Unary) bits; }
        unsigned long to_integer(Unary f) { return (unsigned long) f; }
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
