//! Bit-fields (C99 6.7.2.1), end to end.
//!
//! The *layout* is proved against the host's own C compiler in
//! `tests/bitfield_layout.rs`; this file is about what the generated code
//! does with it — truncation on a store, sign extension on a load, the
//! width-restricted integer promotions, the accessors Rust code reaches the
//! bits through, and every context a member can appear in.
//!
//! The byte patterns asserted here were read off `gcc` on an x86-64 Linux
//! host; the comment above each one says which declaration produced it.

use cinrs::{c11, c99};

// ---------------------------------------------------------------------------
// reading and writing
// ---------------------------------------------------------------------------

#[test]
fn signed_and_unsigned_fields_round_trip() {
    c99! {
        struct Bits {
            unsigned int u : 3;
            int          s : 3;
            unsigned int wide : 20;
        };

        unsigned int get_u(struct Bits b) { return b.u; }
        int          get_s(struct Bits b) { return b.s; }
        unsigned int get_wide(struct Bits b) { return b.wide; }

        struct Bits make(unsigned int u, int s, unsigned int wide) {
            struct Bits b;
            b.u = u;
            b.s = s;
            b.wide = wide;
            return b;
        }
    }

    unsafe {
        let b = make(5, -3, 0xf_ffff);
        assert_eq!(get_u(b), 5);
        assert_eq!(get_s(b), -3);
        assert_eq!(get_wide(b), 0xf_ffff);

        // A store keeps the low `width` bits, and a signed field
        // sign-extends what it kept.
        let truncated = make(0xff, 0xff, 0xffff_ffff);
        assert_eq!(get_u(truncated), 7);
        assert_eq!(get_s(truncated), -1);
        assert_eq!(get_wide(truncated), 0xf_ffff);

        // 4 is 0b100, which is the sign bit of a three-bit field.
        assert_eq!(get_s(make(0, 4, 0)), -4);
        assert_eq!(get_s(make(0, 3, 0)), 3);
    }
}

#[test]
fn plain_int_char_bool_and_enum_fields() {
    c99! {
        enum Colour { RED, GREEN, BLUE };

        struct Mixed {
            int         plain : 4;   /* signed, as GCC makes it */
            char        c     : 3;
            _Bool       flag  : 1;
            enum Colour hue   : 3;
        };

        int   plain_of(struct Mixed m) { return m.plain; }
        int   c_of(struct Mixed m)     { return m.c; }
        int   flag_of(struct Mixed m)  { return m.flag; }
        int   hue_of(struct Mixed m)   { return m.hue; }

        struct Mixed build(int plain, int c, int flag, int hue) {
            struct Mixed m;
            m.plain = plain;
            m.c = c;
            m.flag = flag;
            m.hue = hue;
            return m;
        }
    }

    unsafe {
        let m = build(-8, -4, 1, 2);
        assert_eq!(plain_of(m), -8);
        assert_eq!(c_of(m), -4);
        assert_eq!(flag_of(m), 1);
        assert_eq!(hue_of(m), 2);

        // A plain `int` bit-field is signed, so 15 stored in four bits reads
        // back as -1; the same for `char`, and `_Bool` keeps only zero-ness.
        let wrapped = build(15, 7, 2, 9);
        assert_eq!(plain_of(wrapped), -1);
        assert_eq!(c_of(wrapped), -1);
        assert_eq!(flag_of(wrapped), 1);
        // `enum Colour` has no negative enumerator, so its underlying type is
        // unsigned — as it is for GCC and Clang — and 9 truncated to three
        // bits reads back as 1 rather than as -7.
        assert_eq!(hue_of(wrapped), 1);
        assert_eq!(hue_of(build(0, 0, 0, 7)), 7);
    }
}

#[test]
fn an_enum_with_a_negative_enumerator_makes_the_field_signed() {
    c99! {
        enum Signed   { NEG = -1, ONE = 1 };
        enum Unsigned { ZERO = 0, MANY = 148 };

        struct Two { enum Signed s : 8; enum Unsigned u : 8; };

        int signed_of(struct Two t)   { return t.s; }
        int unsigned_of(struct Two t) { return t.u; }

        struct Two both(int value) {
            struct Two t;
            t.s = value;
            t.u = value;
            return t;
        }
    }

    unsafe {
        let t = both(200);
        assert_eq!(signed_of(t), -56);
        assert_eq!(unsigned_of(t), 200);
    }
}

// ---------------------------------------------------------------------------
// the integer promotions
// ---------------------------------------------------------------------------

#[test]
fn a_bit_field_promotes_by_its_width() {
    c99! {
        /* C99 6.3.1.1p2 restricts the range to the width, so a 31-bit
         * unsigned field takes part in arithmetic as an `int` and a 32-bit
         * one as an `unsigned int`. */
        struct Widths { unsigned int narrow : 31; unsigned int full : 32; };

        int narrow_is_signed(void) {
            struct Widths w;
            w.narrow = 1;
            return (w.narrow - 2) < 0;
        }

        int full_is_signed(void) {
            struct Widths w;
            w.full = 1;
            return (w.full - 2) < 0;
        }

        unsigned long narrow_size(void) {
            struct Widths w;
            w.narrow = 0;
            return sizeof(w.narrow + 0);
        }

        int bool_promotes_to_int(void) {
            struct { _Bool b : 1; } s;
            s.b = 1;
            return (s.b - 2) < 0;
        }

        int wide_keeps_its_type(void) {
            struct { unsigned long long a : 40; } s;
            s.a = 1;
            return (s.a - 2) < 0;
        }
    }

    unsafe {
        assert_eq!(narrow_is_signed(), 1);
        assert_eq!(full_is_signed(), 0);
        assert_eq!(narrow_size(), 4);
        assert_eq!(bool_promotes_to_int(), 1);
        // 40 bits fit in neither `int` nor `unsigned int`, so the field keeps
        // its declared type and the subtraction is unsigned.
        assert_eq!(wide_keeps_its_type(), 0);
    }
}

#[test]
fn a_cast_of_a_bit_field_is_not_a_no_op() {
    c99! {
        /* Reading `u` promotes it to `int` — every seven-bit unsigned value
         * fits in one — so `%` below is signed. Writing the cast turns the
         * same field into a full-width `unsigned int`, and the operation
         * around it becomes unsigned. The two answers differ, which is the
         * whole point: a cast to the field's own *declared* type is a
         * conversion and not a no-op. */
        struct Narrow { signed int s : 7; unsigned int u : 7; };

        int promoted(int lhs) {
            struct Narrow n;
            n.u = 61;
            return lhs % n.u;
        }

        unsigned int cast_to_unsigned(int lhs) {
            struct Narrow n;
            n.u = 61;
            return lhs % (unsigned int) n.u;
        }

        int both_fields(void) {
            struct Narrow n;
            n.s = -13;
            n.u = 61;
            return n.s % n.u;
        }

        unsigned int both_fields_cast(void) {
            struct Narrow n;
            n.s = -13;
            n.u = 61;
            return n.s % (unsigned int) n.u;
        }
    }

    unsafe {
        // -13 % 61 is -13; 4294967283 % 61 is 44. Which of the two an
        // operation gives is the whole difference the cast makes.
        assert_eq!(promoted(-13), -13);
        assert_eq!(cast_to_unsigned(-13), (-13i32 as u32) % 61);
        assert_eq!(both_fields(), -13);
        assert_eq!(both_fields_cast(), (-13i32 as u32) % 61);
    }
}

#[test]
fn a_field_wider_than_int_computes_in_its_own_width() {
    c99! {
        /* A field the integer promotions cannot reach keeps its declared
         * type, and C99 6.7.2.1p10 gives its value the declared *width*: the
         * arithmetic is done in that many bits, exactly as it is in thirty-two
         * for an `unsigned int`. Two fields of different widths meet in the
         * wider of the two. */
        struct Wide {
            unsigned long long a : 33;
            unsigned long long b : 40;
            unsigned long long c : 41;
        };

        unsigned long long square_a(unsigned long long v) {
            struct Wide w; w.a = v; return w.a * w.a;
        }
        unsigned long long a_times_c(unsigned long long v) {
            struct Wide w; w.a = v; w.c = v; return w.a * w.c;
        }
        unsigned long long shift_b(unsigned long long v, int by) {
            struct Wide w; w.b = v; return w.b << by;
        }
        unsigned long long rotate_b(unsigned long long v) {
            struct Wide w; w.b = v; return (w.b << 8) + (w.b >> 32);
        }
        unsigned long long minus_one(void) {
            struct Wide w; w.b = 0; return w.b - 1;
        }
        unsigned long long against_int(unsigned long long v) {
            /* An `int` operand has thirty-two bits and the field forty, so
             * forty is where the addition happens. */
            struct Wide w; w.b = v; return w.b + 1;
        }
        unsigned long long against_unsigned_long_long(unsigned long long v) {
            /* Sixty-four bits beat forty, and nothing is truncated. */
            struct Wide w; w.b = v; return w.b + 1ULL * 1;
        }
        unsigned long long complement(unsigned long long v) {
            struct Wide w; w.b = v; return ~w.b;
        }
        long long signed_wide(long long v) {
            struct { long long s : 33; } w;
            w.s = v;
            return w.s * 2;
        }
    }

    unsafe {
        // 2^20 squared is 2^40, which is zero in thirty-three bits.
        assert_eq!(square_a(1 << 20), 0);
        // …and 2^40 in forty-one, which is where `a * c` computes.
        assert_eq!(a_times_c(1 << 20), 1 << 40);
        assert_eq!(shift_b(0x100, 32), 0);
        assert_eq!(shift_b(1, 8), 0x100);
        assert_eq!(rotate_b(0x01_0000_0001), 0x101);
        assert_eq!(rotate_b(0x01_0000_0000), 1);
        assert_eq!(minus_one(), 0xff_ffff_ffff);
        assert_eq!(against_int(0xff_ffff_ffff), 0);
        assert_eq!(against_unsigned_long_long(0xff_ffff_ffff), 0x100_0000_0000);
        assert_eq!(complement(0), 0xff_ffff_ffff);
        assert_eq!(signed_wide(-1), -2);
        assert_eq!(signed_wide(1 << 30), 1 << 31);
        // Doubling 2^31 overflows a signed 33-bit field, which C leaves
        // undefined; it wraps here, in the field's width, exactly as signed
        // overflow wraps everywhere else in the generated code.
        assert_eq!(signed_wide(1 << 31), -(1i64 << 32));
    }
}

// ---------------------------------------------------------------------------
// updating in place
// ---------------------------------------------------------------------------

#[test]
fn increment_decrement_and_compound_assignment() {
    c99! {
        struct Counter { unsigned int n : 4; int d : 4; unsigned int other : 8; };

        unsigned int bump(struct Counter *c) { return ++c->n; }
        unsigned int bump_after(struct Counter *c) { return c->n++; }
        int drop(struct Counter *c) { return --c->d; }

        unsigned int add(unsigned int start, unsigned int by) {
            struct Counter c;
            c.n = start;
            c.other = 0;
            c.n += by;
            return c.n;
        }

        unsigned int shift(unsigned int start, int by) {
            struct Counter c;
            c.n = start;
            c.n <<= by;
            return c.n;
        }

        /* Through a pointer, and with the operand evaluated exactly once. */
        static int calls;
        struct Counter *once(struct Counter *c) { calls++; return c; }
        int call_count(void) { return calls; }
        void plus_one(struct Counter *c) { once(c)->n += 1; }
    }

    unsafe {
        let mut c = Counter {
            __cinrs_bits0: [0; 2],
        };
        c.set_n(14);
        assert_eq!(bump(&raw mut c), 15);
        // 15 + 1 wraps inside four bits.
        assert_eq!(bump(&raw mut c), 0);
        c.set_n(3);
        assert_eq!(bump_after(&raw mut c), 3);
        assert_eq!(c.n(), 4);

        c.set_d(-8);
        assert_eq!(drop(&raw mut c), 7);

        assert_eq!(add(10, 3), 13);
        assert_eq!(add(10, 10), 4);
        assert_eq!(shift(3, 3), 8);
        // Only the low four bits survive the store.
        assert_eq!(shift(3, 4), 0);

        assert_eq!(call_count(), 0);
        plus_one(&raw mut c);
        assert_eq!(call_count(), 1);
        assert_eq!(c.n(), 5);
    }
}

// ---------------------------------------------------------------------------
// layout and byte patterns
// ---------------------------------------------------------------------------

#[test]
fn fields_share_bytes_and_zero_width_breaks_the_run() {
    c99! {
        /* gcc: sizeof 4, alignof 4, and `{0, -1}` is fc ff ff ff. */
        struct Straddle { char a : 2; int b : 30; };
        /* gcc: sizeof 8 — the `: 0` moves `b` to the next `unsigned` unit. */
        struct Broken { unsigned int a : 3; unsigned int : 0; unsigned int b : 3; };
        /* gcc: sizeof 4, `d` at offset 3, `x` at bits 8..17. */
        struct Interleaved { char c; int x : 9; char d; };
        /* gcc: sizeof 16 — 30 bits do not fit after 40, so `b` starts at 64. */
        struct Wide { unsigned long long a : 40; unsigned int b : 30; };
        /* gcc: sizeof 4, alignof 2 — `x` cannot straddle a `short`, so it
         * starts at bit 16 and the byte at offset 1 is padding. */
        struct Moved { char c; short x : 9; };

        unsigned long record_size(int which) {
            switch (which) {
                case 0: return sizeof(struct Straddle);
                case 1: return sizeof(struct Broken);
                case 2: return sizeof(struct Interleaved);
                case 3: return sizeof(struct Wide);
                case 4: return sizeof(struct Moved);
            }
            return 0;
        }

        void straddle_bytes(unsigned char *out) {
            struct Straddle s = {0, -1};
            unsigned char *p = (unsigned char *) &s;
            unsigned long i;
            for (i = 0; i < sizeof s; i++) out[i] = p[i];
        }

        void interleaved_bytes(unsigned char *out) {
            struct Interleaved s;
            unsigned char *p = (unsigned char *) &s;
            unsigned long i;
            for (i = 0; i < sizeof s; i++) p[i] = 0;
            s.x = -1;
            s.d = 0x7f;
            for (i = 0; i < sizeof s; i++) out[i] = p[i];
        }

        void moved_bytes(unsigned char *out) {
            struct Moved s;
            unsigned char *p = (unsigned char *) &s;
            unsigned long i;
            for (i = 0; i < sizeof s; i++) p[i] = 0;
            s.x = -1;
            for (i = 0; i < sizeof s; i++) out[i] = p[i];
        }

        unsigned long offset_of_d(void) {
            return __builtin_offsetof(struct Interleaved, d);
        }
    }

    unsafe {
        assert_eq!(record_size(0), 4);
        assert_eq!(record_size(1), 8);
        assert_eq!(record_size(2), 4);
        assert_eq!(record_size(3), 16);
        assert_eq!(record_size(4), 4);
        assert_eq!(offset_of_d(), 3);

        let mut bytes = [0u8; 4];
        straddle_bytes(bytes.as_mut_ptr());
        assert_eq!(bytes, [0xfc, 0xff, 0xff, 0xff]);

        interleaved_bytes(bytes.as_mut_ptr());
        assert_eq!(bytes, [0x00, 0xff, 0x01, 0x7f]);

        moved_bytes(bytes.as_mut_ptr());
        assert_eq!(bytes, [0x00, 0x00, 0xff, 0x01]);
    }

    // What the C says `sizeof` is has to be what the generated items are.
    assert_eq!(size_of::<Straddle>(), 4);
    assert_eq!(align_of::<Straddle>(), 4);
    assert_eq!(size_of::<Broken>(), 8);
    assert_eq!(size_of::<Interleaved>(), 4);
    assert_eq!(align_of::<Interleaved>(), 4);
    assert_eq!(size_of::<Wide>(), 16);
    assert_eq!(size_of::<Moved>(), 4);
    assert_eq!(align_of::<Moved>(), 2);
    assert_eq!(core::mem::offset_of!(Interleaved, d), 3);
    // The storage a run lives in starts at the byte its first bit falls in.
    assert_eq!(core::mem::offset_of!(Interleaved, __cinrs_bits0), 1);
    assert_eq!(core::mem::offset_of!(Moved, __cinrs_bits0), 2);
    assert_eq!(core::mem::offset_of!(Straddle, __cinrs_bits0), 0);
}

// ---------------------------------------------------------------------------
// initialisers
// ---------------------------------------------------------------------------

#[test]
fn constant_initializers_are_folded_into_the_storage() {
    c99! {
        struct Flags {
            unsigned int ready : 1;
            int          level : 3;
            unsigned int       : 0;
            unsigned int mask  : 30;
            char         tag;
        };

        /* gcc: 0d 00 00 00 05 00 00 00 78 00 00 00 */
        /* Not `static`: external linkage makes the item `pub`, so the Rust
         * half of the test can read the same object. */
        struct Flags defaults = { 1, -2, .mask = 5, .tag = 'x' };
        static struct Flags empty = { 0 };

        void defaults_bytes(unsigned char *out) {
            unsigned char *p = (unsigned char *) &defaults;
            unsigned long i;
            for (i = 0; i < sizeof defaults; i++) out[i] = p[i];
        }

        int empty_is_zero(void) {
            return empty.ready == 0 && empty.level == 0 && empty.mask == 0 && empty.tag == 0;
        }

        /* A local whose initialiser is not constant goes through the
         * setters instead. */
        int local_level(int from) {
            struct Flags f = { 1, from, .tag = 'y' };
            return f.level;
        }

        int partial(void) {
            struct Flags f = { .level = -1 };
            return f.ready == 0 && f.level == -1 && f.mask == 0;
        }
    }

    unsafe {
        let mut bytes = [0u8; 12];
        defaults_bytes(bytes.as_mut_ptr());
        assert_eq!(bytes, [0x0d, 0, 0, 0, 0x05, 0, 0, 0, 0x78, 0, 0, 0]);
        assert_eq!(empty_is_zero(), 1);
        assert_eq!(local_level(-4), -4);
        assert_eq!(local_level(5), -3);
        assert_eq!(partial(), 1);
        // The accessors borrow, so a `static mut` is reached through a raw
        // pointer; see `a_static_object_is_shared_between_the_c_and_the_rust`.
        let it = &raw const defaults;
        assert_eq!((*it).ready(), 1);
        assert_eq!((*it).level(), -2);
        assert_eq!((*it).mask(), 5);
    }
    assert_eq!(size_of::<Flags>(), 12);
    assert_eq!(align_of::<Flags>(), 4);
    assert_eq!(core::mem::offset_of!(Flags, tag), 8);
}

#[test]
fn arrays_compound_literals_and_by_value_passing() {
    c99! {
        struct Pair { unsigned int lo : 4; unsigned int hi : 4; };

        static struct Pair table[3] = { {1, 2}, {.hi = 3}, {15, 15} };

        int table_lo(int i) { return table[i].lo; }
        int table_hi(int i) { return table[i].hi; }

        int sum(struct Pair p) { return p.lo + p.hi; }

        int literal(int lo) {
            return sum((struct Pair){ lo, 7 });
        }

        struct Pair swapped(struct Pair p) {
            struct Pair out;
            out.lo = p.hi;
            out.hi = p.lo;
            return out;
        }

        int copy_keeps_the_bits(void) {
            struct Pair a = {5, 6};
            struct Pair b = a;
            b.lo = 1;
            return a.lo == 5 && b.lo == 1 && b.hi == 6;
        }

        int address_of_the_struct(void) {
            struct Pair *p = &(struct Pair){ 9, 10 };
            return p->lo * 100 + p->hi;
        }
    }

    unsafe {
        assert_eq!((table_lo(0), table_hi(0)), (1, 2));
        assert_eq!((table_lo(1), table_hi(1)), (0, 3));
        assert_eq!((table_lo(2), table_hi(2)), (15, 15));
        assert_eq!(literal(3), 10);
        let p = swapped(Pair {
            __cinrs_bits0: [0x21],
        });
        assert_eq!((p.lo(), p.hi()), (2, 1));
        assert_eq!(sum(p), 3);
        assert_eq!(copy_keeps_the_bits(), 1);
        assert_eq!(address_of_the_struct(), 910);
    }
    // Two four-bit `unsigned int` fields fit in one byte, but a *named*
    // bit-field raises the record's alignment to its type's.
    assert_eq!(size_of::<Pair>(), 4);
    assert_eq!(align_of::<Pair>(), 4);
}

// ---------------------------------------------------------------------------
// unions, anonymous members and statics
// ---------------------------------------------------------------------------

#[test]
fn unions_give_every_field_its_own_bit_zero() {
    c99! {
        union Overlay {
            unsigned int all;
            struct { unsigned int lo : 8; unsigned int hi : 24; } split;
        };

        union Small { unsigned int a : 3; unsigned int b : 20; };

        unsigned int lo_of(unsigned int all) {
            union Overlay o;
            o.all = all;
            return o.split.lo;
        }

        unsigned int rebuild(unsigned int lo, unsigned int hi) {
            union Overlay o;
            o.split.lo = lo;
            o.split.hi = hi;
            return o.all;
        }

        unsigned int both(unsigned int value) {
            union Small s;
            s.b = value;
            return s.a;
        }

        unsigned long small_size(void) { return sizeof(union Small); }
    }

    unsafe {
        assert_eq!(lo_of(0xdead_beef), 0xef);
        assert_eq!(rebuild(0x12, 0x0034_5678), 0x3456_7812);
        // Both members start at bit zero, so `a` is the low three bits of `b`.
        assert_eq!(both(0b1101), 0b101);
        assert_eq!(small_size(), 4);
    }
    assert_eq!(size_of::<Small>(), 4);
    assert_eq!(align_of::<Small>(), 4);
}

#[test]
fn bit_fields_inside_an_anonymous_member() {
    c11! {
        struct Packet {
            unsigned int length;
            struct { unsigned int version : 4; unsigned int kind : 4; };
            union { unsigned int flags : 3; unsigned char raw; };
        };

        struct Packet make(unsigned int version, unsigned int kind, unsigned int flags) {
            struct Packet p;
            p.length = 7;
            p.version = version;
            p.kind = kind;
            p.flags = flags;
            return p;
        }

        unsigned int version_of(struct Packet p) { return p.version; }
        unsigned int kind_of(struct Packet p)    { return p.kind; }
        unsigned int raw_of(struct Packet p)     { return p.raw; }
    }

    unsafe {
        let p = make(3, 5, 6);
        assert_eq!(version_of(p), 3);
        assert_eq!(kind_of(p), 5);
        assert_eq!(raw_of(p), 6);
        // The same bits, through the accessors Rust sees.
        assert_eq!(p.__cinrs_anon0.version(), 3);
        assert_eq!(p.__cinrs_anon1.flags(), 6);
    }
    // gcc: 12 — the anonymous `struct` and `union` are members of their own.
    assert_eq!(size_of::<Packet>(), 12);
}

#[test]
fn a_static_object_is_shared_between_the_c_and_the_rust() {
    c99! {
        struct Status { unsigned int code : 12; int delta : 8; unsigned int busy : 1; };

        struct Status current;

        void set_code(unsigned int code) { current.code = code; }
        void nudge(int by) { current.delta += by; }
        void toggle(void) { current.busy = !current.busy; }
        unsigned int code_of(void) { return current.code; }
    }

    unsafe {
        // The accessors borrow, so a `static mut` is reached through a raw
        // pointer — which is what the generated code does too, and what
        // edition 2024's `static_mut_refs` asks of any Rust that touches one.
        let it = &raw mut current;

        set_code(0xabc);
        assert_eq!(code_of(), 0xabc);
        // The same object, read through the generated getters.
        assert_eq!((*it).code(), 0xabc);
        assert_eq!((*it).delta(), 0);

        nudge(100);
        nudge(100);
        // 200 does not fit in eight signed bits.
        assert_eq!((*it).delta(), -56);

        assert_eq!((*it).busy(), 0);
        toggle();
        assert_eq!((*it).busy(), 1);
        toggle();
        assert_eq!((*it).busy(), 0);

        // And written from Rust, then read back through the C.
        (*it).set_code(1);
        assert_eq!(code_of(), 1);
    }
}

// ---------------------------------------------------------------------------
// control flow
// ---------------------------------------------------------------------------

#[test]
fn a_bit_field_controls_a_switch_and_survives_a_goto() {
    c99! {
        struct Op { unsigned int code : 3; int arg : 5; };

        int dispatch(struct Op op) {
            switch (op.code) {
                case 0: return 100;
                case 1: return op.arg;
                case 7: return -1;
                default: return 0;
            }
        }

        /* A `goto` sends the whole body through the control-flow graph, where
         * the same place lowering has to work from a different shape. */
        int countdown(struct Op op) {
            int total = 0;
        again:
            if (op.code == 0) {
                goto done;
            }
            total += op.arg;
            op.code--;
            goto again;
        done:
            return total;
        }
    }

    unsafe {
        let mut op = Op {
            __cinrs_bits0: [0; 1],
        };
        op.set_code(0);
        assert_eq!(dispatch(op), 100);
        op.set_code(1);
        op.set_arg(-9);
        assert_eq!(dispatch(op), -9);
        op.set_code(7);
        assert_eq!(dispatch(op), -1);
        op.set_code(3);
        assert_eq!(dispatch(op), 0);

        op.set_code(4);
        op.set_arg(3);
        assert_eq!(countdown(op), 12);
    }
}

#[test]
fn a_bit_field_in_a_conditional_and_a_variadic_call() {
    c99! {
        #include <stdio.h>

        struct Row { unsigned int narrow : 31; unsigned int full : 32; int small : 5; };

        /* `?:` balances the two arms by their *promoted* types, so `narrow`
         * is an `int` here and `full` an `unsigned int`. */
        long pick(struct Row r, int which) {
            return which ? r.narrow : r.small;
        }

        /* Through `...` a bit-field gets the default argument promotions,
         * which for a 31-bit unsigned field means `int` and for a 32-bit one
         * `unsigned int` — so `%d` and `%u` are the right conversions. */
        int print_row(char *out, struct Row r) {
            return sprintf(out, "%d %u %d", r.narrow, r.full, r.small);
        }
    }

    unsafe {
        let mut r = Row {
            __cinrs_bits0: [0; 9],
        };
        r.set_narrow(7);
        r.set_full(0xffff_ffff);
        r.set_small(-3);
        assert_eq!(pick(r, 1), 7);
        assert_eq!(pick(r, 0), -3);

        let mut out = [0u8; 64];
        let written = print_row(out.as_mut_ptr().cast(), r);
        let text = core::str::from_utf8(&out[..written as usize]).expect("ascii");
        assert_eq!(text, "7 4294967295 -3");
    }
}

#[test]
fn a_bit_field_keeps_its_declared_type_and_reads_through_a_const_pointer() {
    c11! {
        struct Frame { unsigned int kind : 3; int weight : 12; char name[4]; };

        /* Lvalue conversion gives a bit-field its *declared* type, so this is
         * the association `_Generic` picks — the width only restricts the
         * integer promotions, which `_Generic` does not apply. */
        int kind_type(struct Frame f) {
            return _Generic(f.kind, unsigned int: 1, int: 2, default: 0);
        }

        int weight_type(struct Frame f) {
            return _Generic(f.weight, int: 1, unsigned int: 2, default: 0);
        }

        /* Reading through a pointer to const: the getter borrows, and the
         * place is behind a `*const` all the same. */
        int weight_of(const struct Frame *f) { return f->weight; }

        /* `sizeof` the record is a constant, so it may be an array bound. */
        char sizes[sizeof(struct Frame)];
        unsigned long bound(void) { return sizeof sizes; }
    }

    unsafe {
        let mut f = Frame {
            __cinrs_bits0: [0; 2],
            name: [0; 4],
        };
        f.set_weight(-1000);
        assert_eq!(kind_type(f), 1);
        assert_eq!(weight_type(f), 1);
        assert_eq!(weight_of(&raw const f), -1000);
        assert_eq!(bound() as usize, size_of::<Frame>());
    }
}

// ---------------------------------------------------------------------------
// the accessors, from Rust
// ---------------------------------------------------------------------------

#[test]
fn the_accessors_are_ordinary_rust_methods() {
    c99! {
        struct Named {
            unsigned int x : 5;
            /* A member whose name collides with the setter of another. */
            unsigned int set_x : 5;
            /* And one whose name is a Rust keyword. */
            unsigned int match : 5;
        };

        unsigned int read_x(struct Named n) { return n.x; }
        unsigned int read_set_x(struct Named n) { return n.set_x; }
        unsigned int read_match(struct Named n) { return n.match; }
    }

    let mut n = Named {
        __cinrs_bits0: [0; 2],
    };
    // Every getter keeps its member's own name, so the setter of `x` — whose
    // name the member `set_x` already has — is `set_x_2`.
    n.set_x_2(3);
    n.set_set_x(9);
    n.set_match(17);
    assert_eq!(n.x(), 3);
    assert_eq!(n.set_x(), 9);
    assert_eq!(n.r#match(), 17);
    unsafe {
        assert_eq!(read_x(n), 3);
        assert_eq!(read_set_x(n), 9);
        assert_eq!(read_match(n), 17);
    }
}

/// A setter given more than the field can hold keeps the low bits, and the
/// getter reads them back in the member's declared type — sign-extended for a
/// signed field. That is the same truncation a store from the C side makes;
/// this is the Rust-side half of it, and it is what
/// `doc/translation.md`'s bit-field example shows.
#[test]
fn a_setter_keeps_the_low_bits_and_reads_back_sign_extended() {
    c99! {
        struct Flags {
            unsigned int ready : 1;
            int          level : 3;
            unsigned int       : 0;   /* start the next field on a new unit */
            unsigned int mask  : 30;
        };

        void arm(struct Flags *f, int level) {
            f->ready = 1;
            f->level = level;
            f->mask += 2;
        }
    }

    let mut f = Flags {
        __cinrs_bits0: [0; 8],
    };
    unsafe { arm(&raw mut f, -3) };
    assert_eq!((f.ready(), f.level(), f.mask()), (1, -3, 2));

    // Nine does not fit in three bits: the low three are kept, and reading one
    // back sign-extends them, so `0b001` is 1.
    f.set_level(9);
    assert_eq!(f.level(), 1);

    // The same for the unsigned field, which has nothing to sign-extend.
    f.set_mask(0xffff_ffff);
    assert_eq!(f.mask(), 0x3fff_ffff);
}
