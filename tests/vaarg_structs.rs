//! `va_arg` of a `struct`, differentially against the host's own C compiler.
//!
//! Reading an aggregate out of an argument list is not a language rule that can
//! be looked up: it is the x86-64 System V ABI's *classification* (AMD64 psABI
//! 3.2.3), which decides — from the fields that overlap each eightbyte of the
//! object — whether that eightbyte arrives in an integer register or an SSE
//! one. `cinrs` computes it in [`sema`](../crates/cinrs-core/src/sema/va.rs)
//! and emits one `next_arg` per eightbyte; the only honest way to check that is
//! to ask a compiler that implements the same ABI.
//!
//! So this test does what `tests/bitfield_layout.rs` does for the bit-field
//! layout. A corpus of a hundred and fifty `struct`s and `union`s of one to
//! sixteen bytes — mixing integers, floats, doubles, pointers, arrays, nested
//! records and bit-fields — is translated *twice*, once by `cinrs` and once by
//! `cc`, and the two are compared byte for byte on what the callee read back.
//!
//! # How the two halves are kept identical
//!
//! The corpus is not written by hand and not written twice. It lives in
//! `tests/include/vaarg_corpus.h`, a plain C header that
//!
//! * this file's `gnu11!` block `#include`s, so `cinrs` compiles it, and
//! * the generated `main.c` `#include`s, so `cc` compiles it.
//!
//! The header holds the *whole* experiment: the records, a variadic reader for
//! each, the driver that fills a value with a fixed byte pattern and passes it
//! through `...`, and `vb_report`, which writes the comparison into a
//! caller-supplied buffer with `sprintf`. Both sides therefore run the same C
//! and the test compares two strings.
//!
//! Filling the value from a *pattern* rather than by assigning members is what
//! makes a byte-for-byte comparison meaningful: the padding is then part of the
//! object, and an ABI that carries a whole eightbyte carries it too.
//!
//! Four probe shapes per record say more than one: the record alone, an `int`
//! on either side of it, a `double` on either side of it, and two records in a
//! row. The scalars are reported alongside the bytes, so a record that consumed
//! the wrong number of registers shows up as a wrong *neighbour* even when its
//! own bytes happen to look right. Every shape stays inside the six integer and
//! eight SSE registers the save area holds; see the module documentation of
//! `sema::va` for why the argument that would straddle its edge is the one case
//! the stable `VaList` cannot follow.
//!
//! # Skipping
//!
//! Without a C compiler on `PATH` there is nothing to compare against, and the
//! test prints why and passes. `CINRS_VAARG_CC` names the compiler, `CC` is
//! consulted next, and `cc` is the default. The half that runs the translated C
//! needs Rust 1.99, where a variadic *definition* can be generated at all.

use std::fmt::Write as _;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// the corpus generator
// ---------------------------------------------------------------------------

/// The seed the corpus is generated from. Changing it changes the corpus, and
/// the committed header has to be blessed again.
const SEED: u64 = 0x0000_5ec7_0e17_8b7e;

/// How many records the corpus holds.
const RECORDS: usize = 150;

/// How many probe shapes each record is read through.
const PROBES: usize = 4;

/// How many byte patterns each probe is run with.
const SEEDS: usize = 3;

/// The largest record the classification places in registers, in bytes.
///
/// Anything larger is class MEMORY, which `cinrs` refuses by name — see
/// `crates/cinrs-core/tests/sema.rs` and `tests/ui/va_arg_struct_errors.rs`,
/// which is where the refusals are pinned down. Nothing here may exceed it, or
/// the corpus would not compile at all.
const MAX_SIZE: u64 = 16;

/// `splitmix64`, so that the corpus is the same everywhere and forever.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n`.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// A value in `low..=high`.
    fn between(&mut self, low: u32, high: u32) -> u32 {
        low + (self.next() % u64::from(high - low + 1)) as u32
    }
}

/// One type an ordinary member may have.
struct Plain {
    /// What goes in front of the member's name.
    prefix: &'static str,
    /// What goes after it — an array bound, or nothing.
    suffix: &'static str,
    size: u64,
    align: u64,
    /// How many bits from the start of the member hold something a C program
    /// can read; the rest is the member's own trailing padding.
    ///
    /// It is `8 * size` for everything but the two nested records that end in
    /// padding of their own. See [`Layout::mask`] for why it matters.
    live: u64,
    /// Whether a floating value can be read out of the member's bytes.
    ///
    /// Such a record is filled from a pattern with the top bit of every byte
    /// cleared, which cannot make the exponent field of an IEEE `float` or
    /// `double` all ones — so no member of it is ever a NaN or an infinity,
    /// and nothing on either side of the comparison has the chance to
    /// canonicalise one.
    floating: bool,
}

impl Plain {
    const fn new(prefix: &'static str, suffix: &'static str, size: u64, align: u64) -> Self {
        Self {
            prefix,
            suffix,
            size,
            align,
            live: size * 8,
            floating: false,
        }
    }

    /// The same, for a member whose own last bytes are padding.
    const fn live(mut self, bits: u64) -> Self {
        self.live = bits;
        self
    }

    const fn floating(mut self) -> Self {
        self.floating = true;
        self
    }
}

/// The types an ordinary member may have.
///
/// Every size and alignment here is the LP64 one, which is the only data model
/// the corpus is ever compiled for: `va_arg` of a record is x86-64 System V
/// only, and `the_corpus_layout_is_what_both_compilers_compute` checks the
/// numbers against `rustc` and against `cc` rather than trusting them.
const PLAIN_TYPES: &[Plain] = &[
    Plain::new("char", "", 1, 1),
    Plain::new("signed char", "", 1, 1),
    Plain::new("unsigned char", "", 1, 1),
    Plain::new("short", "", 2, 2),
    Plain::new("unsigned short", "", 2, 2),
    Plain::new("int", "", 4, 4),
    Plain::new("unsigned int", "", 4, 4),
    Plain::new("long", "", 8, 8),
    Plain::new("long long", "", 8, 8),
    Plain::new("unsigned long long", "", 8, 8),
    Plain::new("float", "", 4, 4).floating(),
    Plain::new("double", "", 8, 8).floating(),
    Plain::new("void *", "", 8, 8),
    Plain::new("int *", "", 8, 8),
    Plain::new("enum VbColour", "", 4, 4),
    Plain::new("char", "[3]", 3, 1),
    Plain::new("char", "[5]", 5, 1),
    Plain::new("char", "[13]", 13, 1),
    Plain::new("int", "[2]", 8, 4),
    Plain::new("short", "[3]", 6, 2),
    Plain::new("float", "[2]", 8, 4).floating(),
    Plain::new("float", "[3]", 12, 4).floating(),
    Plain::new("double", "[2]", 16, 8).floating(),
    Plain::new("struct VbInner", "", 8, 4).live(40),
    Plain::new("struct VbPair", "", 8, 4).floating(),
    Plain::new("union VbEither", "", 4, 4).floating(),
    Plain::new("struct VbBits", "", 4, 4).live(8),
    Plain::new("struct VbTiny", "", 1, 1),
    Plain::new("__int128", "", 16, 16),
];

/// The types a bit-field may have: the spelling, the width of its allocation
/// unit in bytes, and the widest field of it.
const BIT_TYPES: &[(&str, u64, u32)] = &[
    ("char", 1, 8),
    ("unsigned char", 1, 8),
    ("short", 2, 16),
    ("unsigned short", 2, 16),
    ("int", 4, 32),
    ("unsigned int", 4, 32),
    ("long long", 8, 64),
    ("unsigned long long", 8, 64),
];

/// Where the next member of a record goes, and how big the record is so far.
///
/// This is C's layout, as `crates/cinrs-core/src/sema/types.rs` computes it,
/// far enough to keep the generated corpus inside [`MAX_SIZE`]. It is a model
/// and could be wrong, so nothing trusts it: the numbers it predicts are
/// written into the corpus and checked against `rustc` and against `cc`.
#[derive(Clone)]
struct Layout {
    /// Whether the record is a `union`, whose members all start at bit zero.
    union: bool,
    /// The bit just past the last member of a `struct`.
    bits: u64,
    /// The bytes the widest member of a `union` covers.
    union_bytes: u64,
    align: u64,
    /// Which bits of the object belong to a member, least significant bit of
    /// byte zero first.
    ///
    /// The rest is padding, and **padding is not part of the ABI**: an
    /// implementation may carry an eightbyte whole or only as far as its last
    /// live bit, and the two compilers this test compares really do differ —
    /// GCC moves an eightbyte whose content stops before bit 32 as a four-byte
    /// value, while `rustc` moves the whole eight. So the report masks the
    /// padding away and compares what C says the object *is*.
    mask: [u8; MAX_SIZE as usize],
}

fn round_up(value: u64, to: u64) -> u64 {
    value.div_ceil(to) * to
}

impl Layout {
    fn new(union: bool) -> Self {
        Self {
            union,
            bits: 0,
            union_bytes: 0,
            align: 1,
            mask: [0; MAX_SIZE as usize],
        }
    }

    /// Marks `count` bits from bit `start` as belonging to a member.
    ///
    /// Bits past [`MAX_SIZE`] are dropped rather than refused: this runs on a
    /// *trial* layout, and a member that would not fit is rejected by the
    /// size the trial ends up with.
    fn live(&mut self, start: u64, count: u64) {
        for bit in start..start + count {
            let byte = (bit / 8) as usize;
            if byte < self.mask.len() {
                self.mask[byte] |= 1 << (bit % 8);
            }
        }
    }

    /// Places an ordinary member.
    fn plain(&mut self, size: u64, align: u64, live: u64) {
        self.align = self.align.max(align);
        if self.union {
            self.union_bytes = self.union_bytes.max(size);
            self.live(0, live);
            return;
        }
        let offset = round_up(self.bits.div_ceil(8), align);
        self.live(offset * 8, live);
        self.bits = (offset + size) * 8;
    }

    /// Places a bit-field of `width` bits in a unit of `unit` bytes.
    ///
    /// A field never straddles a unit of its own type: one that would starts at
    /// the next unit boundary instead, and width zero is the request for that
    /// boundary and nothing else. Only a *named* field makes the record
    /// stricter.
    fn bit_field(&mut self, unit: u64, width: u32, named: bool) {
        if named {
            self.align = self.align.max(unit);
        }
        if self.union {
            self.union_bytes = self.union_bytes.max(u64::from(width).div_ceil(8));
            self.live(0, u64::from(width));
            return;
        }
        let unit = unit * 8;
        let width = u64::from(width);
        // A zero-width field, or one that would straddle a unit boundary, moves
        // to the next unit (the GCC allocation rule).
        if width == 0 || self.bits / unit != (self.bits + width - 1) / unit {
            self.bits = round_up(self.bits, unit);
        }
        self.live(self.bits, width);
        self.bits += width;
    }

    fn size(&self) -> u64 {
        let bytes = if self.union {
            self.union_bytes
        } else {
            self.bits.div_ceil(8)
        };
        round_up(bytes, self.align)
    }
}

/// One generated record.
struct Record {
    /// Whether it is a `union`.
    union: bool,
    /// The member declarations, without the trailing `;`.
    members: Vec<String>,
    /// Whether any member's bytes are ever read as a floating value.
    floating: bool,
    size: u64,
    align: u64,
    /// Which bits of the object belong to a member; see [`Layout::mask`].
    mask: [u8; MAX_SIZE as usize],
}

impl Record {
    fn keyword(&self) -> &'static str {
        if self.union { "union" } else { "struct" }
    }
}

/// The two generated files: the C corpus and the Rust layout assertions.
struct Corpus {
    header: String,
    checks: String,
}

/// Builds one record: as many members as fit in [`MAX_SIZE`] bytes.
fn record(rng: &mut Rng) -> Record {
    // A union is the rarer shape, and its members all start at bit zero — so
    // "a union classifies per member" is the rule under test there.
    let union = rng.below(6) == 0;
    let wanted = rng.between(1, 5) as usize;
    let mut layout = Layout::new(union);
    let mut members: Vec<String> = Vec::new();
    let mut floating = false;
    let mut named = 0usize;
    for index in 0..wanted {
        // One member in four is a bit-field, since the ABI classifies a run of
        // them as one integer whatever their declared types are.
        let want_bits = rng.below(4) == 0;
        // An unnamed bit-field declares nothing, which is the only way a
        // record can hold bits no C member covers; a zero-width one is the
        // request for an alignment boundary, and a union has no boundaries to
        // ask for.
        let anonymous = want_bits && rng.below(5) == 0;
        let mut trial = layout.clone();
        let (decl, member_floating, adds_name) = if want_bits {
            let (ty, unit, limit) = BIT_TYPES[rng.below(BIT_TYPES.len())];
            let width = if anonymous && !union && rng.below(3) == 0 {
                0
            } else {
                rng.between(1, limit)
            };
            trial.bit_field(unit, width, !anonymous);
            let decl = if anonymous {
                format!("{ty} : {width}")
            } else {
                format!("{ty} f{index} : {width}")
            };
            (decl, false, !anonymous)
        } else {
            let plain = &PLAIN_TYPES[rng.below(PLAIN_TYPES.len())];
            trial.plain(plain.size, plain.align, plain.live);
            (
                format!("{} f{index}{}", plain.prefix, plain.suffix),
                plain.floating,
                true,
            )
        };
        if trial.size() > MAX_SIZE {
            // It does not fit; every later member would be tried against the
            // same room, so this is where the record ends.
            break;
        }
        layout = trial;
        members.push(decl);
        floating |= member_floating;
        named += usize::from(adds_name);
    }
    if named == 0 {
        // Every record needs a member C can name, and a record with no member
        // at all is not C.
        layout = Layout::new(union);
        layout.plain(1, 1, 8);
        members.clear();
        members.push("char f0".to_owned());
        floating = false;
    }
    Record {
        union,
        members,
        floating,
        size: layout.size(),
        align: layout.align,
        mask: layout.mask,
    }
}

/// Builds the corpus header and the Rust layout assertions that go with it.
fn corpus() -> Corpus {
    let mut rng = Rng(SEED);
    let mut out = String::from(HEADER_PROLOGUE);
    let mut checks = String::from(CHECKS_PROLOGUE);
    let mut readers = String::new();
    let mut records: Vec<Record> = Vec::with_capacity(RECORDS);

    for index in 0..RECORDS {
        let tag = format!("V{index:03}");
        let r = record(&mut rng);
        let keyword = r.keyword();
        let _ = writeln!(out, "{keyword} {tag} {{");
        for member in &r.members {
            let _ = writeln!(out, "    {member};");
        }
        let _ = writeln!(out, "}};");
        let _ = writeln!(out);

        let _ = writeln!(
            checks,
            "    check_record::<{tag}>({index}, \"{tag}\", {}, {});",
            r.size, r.align
        );

        // The reader: one variadic function per record, whose shape the first
        // named argument selects, so that the four probes share the `va_arg`
        // of the record itself.
        let _ = writeln!(
            readers,
            "static long vb_read_{index:03}(unsigned char *out, int probe, ...) {{"
        );
        let _ = writeln!(readers, "    va_list ap;");
        let _ = writeln!(readers, "    {keyword} {tag} v;");
        let _ = writeln!(readers, "    long extra = 0;");
        let _ = writeln!(readers, "    va_start(ap, probe);");
        let _ = writeln!(readers, "    if (probe == 1) extra = va_arg(ap, int);");
        let _ = writeln!(
            readers,
            "    if (probe == 2) extra = (long) (va_arg(ap, double) * 100);"
        );
        let _ = writeln!(readers, "    v = va_arg(ap, {keyword} {tag});");
        let _ = writeln!(readers, "    vb_copy(out, &v, sizeof v);");
        let _ = writeln!(
            readers,
            "    if (probe == 1) extra = extra * 1000 + va_arg(ap, int);"
        );
        let _ = writeln!(
            readers,
            "    if (probe == 2) extra = extra * 1000 + (long) (va_arg(ap, double) * 100);"
        );
        let _ = writeln!(readers, "    if (probe == 3) {{");
        let _ = writeln!(readers, "        v = va_arg(ap, {keyword} {tag});");
        let _ = writeln!(readers, "        vb_copy(out + 16, &v, sizeof v);");
        let _ = writeln!(readers, "    }}");
        let _ = writeln!(readers, "    va_end(ap);");
        let _ = writeln!(readers, "    return extra;");
        let _ = writeln!(readers, "}}");
        let _ = writeln!(readers);

        // The driver: the C *caller*, which is what puts the record into the
        // registers the reader takes it out of.
        let mask = if r.floating { "0x7f" } else { "0xff" };
        let _ = writeln!(
            readers,
            "static long vb_drive_{index:03}(int probe, int seed, unsigned char *out) {{"
        );
        let _ = writeln!(readers, "    {keyword} {tag} v;");
        let _ = writeln!(
            readers,
            "    vb_fill((unsigned char *) &v, sizeof v, seed, {mask});"
        );
        let _ = writeln!(
            readers,
            "    if (probe == 0) return vb_read_{index:03}(out, 0, v);"
        );
        let _ = writeln!(
            readers,
            "    if (probe == 1) return vb_read_{index:03}(out, 1, 11, v, 22);"
        );
        let _ = writeln!(
            readers,
            "    if (probe == 2) return vb_read_{index:03}(out, 2, 1.5, v, 2.25);"
        );
        let _ = writeln!(readers, "    return vb_read_{index:03}(out, 3, v, v);");
        let _ = writeln!(readers, "}}");
        let _ = writeln!(readers);
        records.push(r);
    }
    out.push_str(&readers);

    let _ = writeln!(
        out,
        "typedef long (*VbDrive)(int, int, unsigned char *);\n\
         static VbDrive vb_drives[{RECORDS}] = {{"
    );
    for index in 0..RECORDS {
        let _ = writeln!(out, "    vb_drive_{index:03},");
    }
    let _ = writeln!(out, "}};\n");
    let _ = writeln!(out, "static unsigned long vb_sizes[{RECORDS}] = {{");
    for (index, r) in records.iter().enumerate() {
        let _ = writeln!(out, "    sizeof({} V{index:03}),", r.keyword());
    }
    let _ = writeln!(out, "}};\n");
    let _ = writeln!(out, "static unsigned long vb_aligns[{RECORDS}] = {{");
    for (index, r) in records.iter().enumerate() {
        let _ = writeln!(out, "    _Alignof({} V{index:03}),", r.keyword());
    }
    let _ = writeln!(out, "}};\n");
    let _ = writeln!(
        out,
        "/* Which bits of each record belong to a member; the report compares\n\
         \x20* nothing else, because padding is not part of the ABI. */\n\
         static unsigned char vb_masks[{RECORDS}][{MAX_SIZE}] = {{"
    );
    for r in &records {
        let bytes: Vec<String> = r.mask.iter().map(|b| format!("0x{b:02x}")).collect();
        let _ = writeln!(out, "    {{ {} }},", bytes.join(", "));
    }
    let _ = writeln!(out, "}};\n");
    out.push_str(
        &HEADER_EPILOGUE
            .replace("@@PROBES@@", &PROBES.to_string())
            .replace("@@SEEDS@@", &SEEDS.to_string()),
    );
    checks.push_str("}\n");
    Corpus {
        header: out,
        checks,
    }
}

/// What every generated corpus starts with.
const HEADER_PROLOGUE: &str = r#"/* The `va_arg` struct corpus, generated by tests/vaarg_structs.rs.
 *
 * Do not edit: regenerate it with
 *
 *     CINRS_BLESS_VAARG_CORPUS=1 cargo test --test vaarg_structs
 *
 * It is included both by that test's `gnu11!` block and by the `main.c` the
 * test hands to the host C compiler, so that the two sides compile the same
 * text. Every record is at most sixteen bytes, which is what the x86-64
 * System V ABI passes in registers and therefore all that `va_arg` of a record
 * supports; the refusals are pinned down elsewhere.
 */
#ifndef CINRS_VAARG_CORPUS_H
#define CINRS_VAARG_CORPUS_H

#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>

/* The ingredients the generated records are built from. */
enum VbColour { VB_RED = 0, VB_GREEN = 1, VB_BLUE = 2 };
struct VbInner { int a; char b; };
struct VbPair { float x; float y; };
union VbEither { int i; float f; };
struct VbBits { unsigned int p : 3; int q : 5; };
struct VbTiny { char c; };

static void vb_copy(unsigned char *dst, const void *src, unsigned long n) {
    const unsigned char *p = (const unsigned char *) src;
    unsigned long i;
    for (i = 0; i < n; i++) dst[i] = p[i];
}

/* Fills an object with a pattern, padding included.
 *
 * That is what makes a byte-for-byte comparison mean something: an eightbyte
 * travels whole, so the padding inside it has to arrive as well. `mask` is
 * 0x7f for a record something can read a `float` or a `double` out of, which
 * keeps every exponent field short of all-ones and so keeps NaNs — whose bit
 * patterns a compiler is allowed to canonicalise — out of the corpus.
 */
static void vb_fill(unsigned char *p, unsigned long size, int seed,
                    unsigned int mask) {
    unsigned long i;
    for (i = 0; i < size; i++) {
        p[i] = (unsigned char) (((seed * 61 + (int) i * 37 + 19) ^ (int) (i << 3))
                                & (int) mask);
    }
}

"#;

/// What every generated corpus ends with: the report both sides print.
const HEADER_EPILOGUE: &str = r#"/* How many probe shapes each record is read through, and how many patterns
 * each shape is run with. */
#define VB_PROBES @@PROBES@@
#define VB_SEEDS @@SEEDS@@

int vb_count(void) { return (int) (sizeof vb_drives / sizeof vb_drives[0]); }
unsigned long vb_size(int s) { return vb_sizes[s]; }
unsigned long vb_align(int s) { return vb_aligns[s]; }

long vb_drive(int s, int probe, int seed, unsigned char *out) {
    return vb_drives[s](probe, seed, out);
}

/* Writes the whole comparison into `out` and returns its length.
 *
 * Both halves of the differential test call this, so the format cannot drift
 * between them.
 */
long vb_report(char *out) {
    char *w = out;
    int s, probe, seed;
    unsigned long i, size;
    long extra;
    unsigned char bytes[64];
    for (s = 0; s < vb_count(); s++) {
        size = vb_size(s);
        w += sprintf(w, "V%03d size=%lu align=%lu\n", s, size, vb_align(s));
        for (probe = 0; probe < VB_PROBES; probe++) {
            for (seed = 0; seed < VB_SEEDS; seed++) {
                for (i = 0; i < sizeof bytes; i++) bytes[i] = 0;
                extra = vb_drive(s, probe, seed, bytes);
                w += sprintf(w, "V%03d p%d s%d extra=%ld ", s, probe, seed, extra);
                for (i = 0; i < size; i++) {
                    w += sprintf(w, "%02x", bytes[i] & vb_masks[s][i]);
                }
                if (probe == 3) {
                    w += sprintf(w, " ");
                    for (i = 0; i < size; i++) {
                        w += sprintf(w, "%02x", bytes[16 + i] & vb_masks[s][i]);
                    }
                }
                w += sprintf(w, "\n");
            }
        }
    }
    return (long) (w - out);
}

#endif /* CINRS_VAARG_CORPUS_H */
"#;

/// What the generated Rust assertions start with.
const CHECKS_PROLOGUE: &str = "\
// The layout assertions over the `va_arg` corpus, generated by
// tests/vaarg_structs.rs alongside include/vaarg_corpus.h.
//
// Do not edit: regenerate both with
//
//     CINRS_BLESS_VAARG_CORPUS=1 cargo test --test vaarg_structs
//
// It is `include!`d by that test, which defines `check_record`.
fn check_generated_layout() {
";

/// The `main` handed to the host compiler.
///
/// Only the half that has something to compare against uses it, and that half
/// needs a toolchain that can generate a variadic definition at all.
#[rustversion::since(1.99)]
const MAIN_C: &str = r#"#include <stdio.h>
#include "vaarg_corpus.h"

static char buffer[1 << 23];

int main(void) {
    long n = vb_report(buffer);
    fwrite(buffer, 1, (size_t) n, stdout);
    return 0;
}
"#;

// ---------------------------------------------------------------------------
// the corpus is what the generator produces
// ---------------------------------------------------------------------------

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn corpus_path() -> PathBuf {
    manifest_dir().join("tests/include/vaarg_corpus.h")
}

fn checks_path() -> PathBuf {
    manifest_dir().join("tests/include/vaarg_corpus_checks.rs")
}

#[test]
fn the_committed_corpus_is_what_the_generator_produces() {
    let generated = corpus();
    let files = [
        (corpus_path(), generated.header),
        (checks_path(), generated.checks),
    ];
    let bless = std::env::var_os("CINRS_BLESS_VAARG_CORPUS").is_some();
    for (path, wanted) in files {
        if bless {
            std::fs::write(&path, &wanted).expect("the corpus directory must be writable");
            continue;
        }
        let on_disk = std::fs::read_to_string(&path).expect("the corpus must be checked in");
        assert!(
            on_disk == wanted,
            "{} is not what the generator in this file produces; regenerate it with\n    \
             CINRS_BLESS_VAARG_CORPUS=1 cargo test --test vaarg_structs",
            path.display()
        );
    }
}

// ---------------------------------------------------------------------------
// the differential test
// ---------------------------------------------------------------------------

/// Everything here needs a variadic *definition*, which is Rust 1.99.
#[rustversion::since(1.99)]
mod differential {
    use std::path::Path;
    use std::process::Command;

    use cinrs::gnu11;

    use super::{MAIN_C, corpus_path, manifest_dir};

    gnu11! {
        #include "include/vaarg_corpus.h"

        /* The four classifications the ABI document is worth reading for, by
           name rather than by chance: `Mixed` is SSE then INTEGER, `Other` is
           INTEGER then SSE, `Three` puts two floats in one SSE eightbyte and
           the third in a second, and `Bytes` is thirteen bytes of INTEGER. */
        struct VbMixed { double d; int i; };
        struct VbOther { int i; double d; };
        struct VbThree { float a, b, c; };
        struct VbBytes { char x[13]; };

        long long vb_take_mixed(int n, ...) {
            va_list ap;
            struct VbMixed v;
            va_start(ap, n);
            v = va_arg(ap, struct VbMixed);
            va_end(ap);
            return (long long) v.d * 1000 + v.i;
        }

        long long vb_take_other(int n, ...) {
            va_list ap;
            struct VbOther v;
            va_start(ap, n);
            v = va_arg(ap, struct VbOther);
            va_end(ap);
            return (long long) v.d * 1000 + v.i;
        }

        long long vb_take_three(int n, ...) {
            va_list ap;
            struct VbThree v;
            int tail;
            va_start(ap, n);
            v = va_arg(ap, struct VbThree);
            tail = va_arg(ap, int);
            va_end(ap);
            return (long long) (v.a * 1000000 + v.b * 1000 + v.c) * 10 + tail;
        }

        long long vb_take_bytes(int n, ...) {
            va_list ap;
            struct VbBytes v;
            long long total = 0;
            int i;
            va_start(ap, n);
            v = va_arg(ap, struct VbBytes);
            va_end(ap);
            for (i = 0; i < 13; i++) total = total * 3 + v.x[i];
            return total;
        }

        /* The same four, called from C — where the *passing* side is the
           generated Rust variadic call, which is what follows the ABI. */
        long long vb_mixed_from_c(void) {
            struct VbMixed v;
            v.d = 3.0;
            v.i = 7;
            return vb_take_mixed(1, v);
        }

        long long vb_other_from_c(void) {
            struct VbOther v;
            v.i = 11;
            v.d = 4.0;
            return vb_take_other(1, v);
        }

        long long vb_three_from_c(void) {
            struct VbThree v;
            v.a = 1.0f;
            v.b = 2.0f;
            v.c = 3.0f;
            return vb_take_three(1, v, 4);
        }

        long long vb_bytes_from_c(void) {
            struct VbBytes v;
            int i;
            for (i = 0; i < 13; i++) v.x[i] = (char) (i + 1);
            return vb_take_bytes(1, v);
        }
    }

    // The generated `check_generated_layout`, one assertion per record.
    include!("include/vaarg_corpus_checks.rs");

    /// One record: the size and alignment the generator predicted against what
    /// `rustc` gave the item and what the translated C computes.
    ///
    /// Three answers rather than two, because the generator has a layout model
    /// of its own — it needs one to keep every record inside sixteen bytes —
    /// and a model nothing checks is a model that drifts.
    fn check_record<T>(index: usize, name: &str, size: u64, align: u64) {
        assert_eq!(size as usize, size_of::<T>(), "the size rustc gave {name}");
        assert_eq!(
            align as usize,
            align_of::<T>(),
            "the alignment rustc gave {name}"
        );
        let index = index as ::core::ffi::c_int;
        assert_eq!(size, unsafe { vb_size(index) }, "sizeof {name}");
        assert_eq!(align, unsafe { vb_align(index) }, "_Alignof {name}");
    }

    /// The compiler to compare against, if there is one.
    fn c_compiler() -> Option<String> {
        let named = std::env::var("CINRS_VAARG_CC")
            .or_else(|_| std::env::var("CC"))
            .unwrap_or_else(|_| "cc".to_owned());
        let ok = Command::new(&named)
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success());
        ok.then_some(named)
    }

    /// The report as the host compiler produces it.
    fn report_through(cc: &str) -> String {
        let dir = manifest_dir().join("target/vaarg-structs");
        std::fs::create_dir_all(&dir).expect("target/ must be writable");
        let source = dir.join("main.c");
        std::fs::write(&source, MAIN_C).expect("target/ must be writable");
        let binary = dir.join("corpus_report");

        let include = corpus_path()
            .parent()
            .expect("the corpus has a directory")
            .to_path_buf();
        let output = Command::new(cc)
            .args(["-std=gnu11", "-w", "-O0"])
            .arg("-I")
            .arg(&include)
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap_or_else(|e| panic!("could not run {cc}: {e}"));
        assert!(
            output.status.success(),
            "{cc} could not compile the corpus:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        run(&binary)
    }

    fn run(binary: &Path) -> String {
        let output = Command::new(binary)
            .output()
            .unwrap_or_else(|e| panic!("could not run {}: {e}", binary.display()));
        assert!(
            output.status.success(),
            "{} exited with {}",
            binary.display(),
            output.status
        );
        String::from_utf8(output.stdout).expect("the report is ASCII")
    }

    /// Compares two reports and says where they first differ.
    fn compare(ours: &str, theirs: &str, cc: &str) {
        if ours == theirs {
            return;
        }
        let mismatch = ours
            .lines()
            .zip(theirs.lines())
            .find(|(a, b)| a != b)
            .map(|(a, b)| format!("cinrs: {a}\n   {cc}: {b}"))
            .unwrap_or_else(|| {
                format!(
                    "the reports have different lengths: {} lines from cinrs, {} from {cc}",
                    ours.lines().count(),
                    theirs.lines().count()
                )
            });
        panic!("va_arg of a struct differs from {cc}'s:\n{mismatch}");
    }

    /// The report as the translated corpus produces it.
    fn report_through_cinrs() -> String {
        // Large enough for the whole corpus; the C side uses the same bound.
        let mut buffer = vec![0u8; 1 << 23];
        let written = unsafe { vb_report(buffer.as_mut_ptr().cast()) };
        let len = usize::try_from(written).expect("the report cannot be negative");
        assert!(len < buffer.len(), "the report buffer was too small");
        String::from_utf8(buffer[..len].to_vec()).expect("the report is ASCII")
    }

    /// The generator's layout model, `rustc`'s and the translated C's all have
    /// to agree, or the corpus is not testing what it says it is.
    #[test]
    fn the_corpus_layout_is_what_both_compilers_compute() {
        check_generated_layout();
    }

    #[test]
    fn va_arg_of_a_struct_agrees_with_the_host_c_compiler() {
        let Some(cc) = c_compiler() else {
            println!(
                "skipping: no C compiler on PATH (set CINRS_VAARG_CC or CC to name one), \
                 so there is nothing to compare va_arg of a struct against"
            );
            return;
        };
        compare(&report_through_cinrs(), &report_through(&cc), &cc);
    }

    /// The four mixed classifications, read back from a value **Rust** built
    /// and passed through Rust's own `extern "C"` variadic call.
    #[test]
    fn the_mixed_classes_round_trip_from_rust() {
        unsafe {
            assert_eq!(vb_take_mixed(1, VbMixed { d: 3.0, i: 7 }), 3007);
            assert_eq!(vb_take_other(1, VbOther { i: 11, d: 4.0 }), 4011);
            assert_eq!(
                vb_take_three(
                    1,
                    VbThree {
                        a: 1.0,
                        b: 2.0,
                        c: 3.0
                    },
                    4i32
                ),
                1002003 * 10 + 4
            );
            let mut bytes = VbBytes { x: [0; 13] };
            for (i, byte) in bytes.x.iter_mut().enumerate() {
                *byte = i as ::core::ffi::c_char + 1;
            }
            let want = (1..=13i64).fold(0i64, |total, b| total * 3 + b);
            assert_eq!(vb_take_bytes(1, bytes), want);
        }
    }

    /// And the same four with the *caller* in C, which is the other half of
    /// the round trip: the generated Rust builds the argument list.
    #[test]
    fn the_mixed_classes_round_trip_from_c() {
        unsafe {
            assert_eq!(vb_mixed_from_c(), 3007);
            assert_eq!(vb_other_from_c(), 4011);
            assert_eq!(vb_three_from_c(), 1002003 * 10 + 4);
            let want = (1..=13i64).fold(0i64, |total, b| total * 3 + b);
            assert_eq!(vb_bytes_from_c(), want);
        }
    }
}
