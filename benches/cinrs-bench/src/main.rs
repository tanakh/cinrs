//! How fast is the code `cinrs` generates?
//!
//! The crate's claim is that a C program written inside a `c99!` block is the
//! same program: the same semantics, the same `sizeof`, the same output. This
//! harness asks the other question — whether it also runs at the same *speed*
//! — by building each of a few dozen whole C programs three times over and
//! timing them:
//!
//! 1. `gcc -O2`,
//! 2. `clang -O2`,
//! 3. `cinrs` — the C in a macro invocation, compiled by `rustc -C
//!    opt-level=3` against a release build of the crate.
//!
//! and then checking that all three produced **byte-identical output**, which
//! is a correctness check the timing gets for free. A miscompilation found
//! here is worth more than any of the numbers.
//!
//! # Running it
//!
//! ```text
//! cargo run -p cinrs-bench --release -- [OPTIONS]
//!
//!   --filter NAME       only programs whose name contains NAME
//!   --runs N            timed runs per build (default 5); the median is
//!                       reported, with the minimum next to it
//!   --compilers LIST    comma-separated, from gcc, gcc-O3, clang
//!                       (default gcc,clang)
//!   --timeout SECS      per-execution ceiling (default 60); a run that hits
//!                       it is killed and reported as timed out, which for a
//!                       benchmark is itself the finding
//!   --build-timeout S   per-compilation ceiling (default 600)
//!   --mem-limit-kb KB   address-space ceiling for every child (default
//!                       8000000, i.e. the house limit); 0 to inherit
//!   --markdown PATH     also write the table, the machine and the method to
//!                       PATH (doc/benchmarks.md is the checked-in one)
//!   --list              print the program table and stop
//!   --keep-going        carry on after a program fails to build
//! ```
//!
//! Every child process — compiler and program alike — is started through
//! `sh -c 'ulimit -v …; exec "$@"' … timeout -k 10 … `, which is the same
//! belt-and-braces the conformance harnesses use and which
//! [`doc/testsuites.md`](../../doc/testsuites.md) explains. Wrap the whole run
//! in one of your own as well:
//!
//! ```text
//! ( ulimit -v 8000000; timeout -k 10 1800 \
//!       cargo run -p cinrs-bench --release -- --markdown doc/benchmarks.md )
//! ```
//!
//! # What is measured
//!
//! Wall-clock time of the whole process, median of `--runs`, with `stdout`
//! sent to `/dev/null` for the timed runs and to a file for one extra
//! untimed run per build, which is what the outputs are compared from. The
//! process is started the same way for all three builds, so the constant few
//! milliseconds of `sh`, `timeout` and `execve` are in every column and cancel
//! in the ratio.
//!
//! The flags are the closest comparable pair that exists. `gcc -O2` and
//! `clang -O2` compile one translation unit; `rustc -C opt-level=3 -C
//! codegen-units=1` compiles one crate, which is also one unit — that is
//! Cargo's `release` profile (`opt-level = 3`, no debug assertions, no
//! overflow checks) with `codegen-units` turned down to one, so that the whole
//! program is offered to LLVM at once the way the whole `.c` file is offered
//! to GCC. `-O3` for gcc is available as a fourth column
//! (`--compilers gcc,gcc-O3,clang`) because `-O2` is what the crate is
//! measured against but `-O3` is what LLVM's `opt-level=3` nominally
//! corresponds to.
//!
//! # Adding a program
//!
//! Put the C in `programs/kernels/` and add a row to [`PROGRAMS`]. The row
//! says which entry point to translate with, what arguments and standard input
//! to give it, what to link, and which output lines — if any — are not
//! comparable across builds because the program prints a time or an address.
//! `benches/cinrs-bench/README.md` has the longer version.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// the program table
// ---------------------------------------------------------------------------

/// Which revision of C a program is translated with, and the `-std=` that
/// means the same thing to `gcc` and to `clang`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dialect {
    /// `gnu89!` / `-std=gnu89` — implicit `int`, implicit function
    /// declarations, K&R definitions. Dhrystone needs all three.
    Gnu89,
    /// `gnu11!` / `-std=gnu11` — the default here.
    Gnu11,
}

impl Dialect {
    fn macro_name(self) -> &'static str {
        match self {
            Dialect::Gnu89 => "gnu89",
            Dialect::Gnu11 => "gnu11",
        }
    }

    fn std_flag(self) -> &'static str {
        match self {
            Dialect::Gnu89 => "-std=gnu89",
            Dialect::Gnu11 => "-std=gnu11",
        }
    }
}

/// How the program's `main` is declared, which decides what the generated
/// Rust `fn main` calls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MainKind {
    /// `int main(void)`, or `int main()` in a revision where that means
    /// "unspecified".
    NoArgs,
    /// `int main(int argc, char **argv)`.
    ArgcArgv,
}

/// Whether a unit may read the platform's own headers, and in which order.
///
/// Off is the crate's default and what keeps a unit portable. `Yes` puts the
/// platform's directories *after* the bundled headers, so a name `cinrs`
/// bundles still comes from `cinrs` and only what it does not carry —
/// `<malloc.h>`, `<gmp.h>` — comes from the machine. `First` swaps the two,
/// which is how a program asks for the platform's `<stdio.h>` and so for the
/// declarations glibc adds to it beyond standard C.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SystemInclude {
    No,
    Yes,
    First,
}

impl SystemInclude {
    /// The value of `CINRS_SYSTEM_INCLUDE`, or `None` to leave it unset.
    fn env(self) -> Option<&'static str> {
        match self {
            SystemInclude::No => None,
            SystemInclude::Yes => Some("1"),
            SystemInclude::First => Some("first"),
        }
    }
}

/// Where a program's standard input comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StdinSource {
    /// `/dev/null`.
    Empty,
    /// A literal, written to a file once and fed from there.
    Text(&'static str),
    /// The output of `fasta`, generated once into `target/cinrs-bench/data/`
    /// with the `gcc` build, exactly as the Benchmarks Game feeds
    /// reverse-complement from fasta.
    Fasta,
}

/// One benchmark program.
struct Program {
    /// The name in the table and on the command line.
    name: &'static str,
    /// The C source, relative to `programs/`.
    source: &'static str,
    /// Which family the row belongs to in the report.
    group: Group,
    dialect: Dialect,
    main: MainKind,
    /// Command-line arguments, which are also what sizes the run.
    args: &'static [&'static str],
    stdin: StdinSource,
    /// `-DNAME` natively, `#define NAME 1` in front of the C for `cinrs`.
    defines: &'static [&'static str],
    /// `-lNAME` natively, `rustc -l NAME` for the `cinrs` build.
    libs: &'static [&'static str],
    /// Instruction sets above the x86-64 baseline the program is written for:
    /// `-mNAME` for each natively, `-C target-feature=+NAME,…` for the `cinrs`
    /// build. The names have to be ones GCC, Clang and rustc all spell the same
    /// way (`sse3`, `ssse3`, `sse4.1`, `avx`, `avx2`, `fma`, …). The rustc flag
    /// applies to the generated program's crate only — the prebuilt `cinrs`
    /// rlib is linked as it is — which is all that matters, since the
    /// intrinsics are expanded into the program's own functions and inlined
    /// there. It does not predefine `__AVX__` and the like for the C: a
    /// procedural macro cannot see `-C target-feature`.
    features: &'static [&'static str],
    /// Whether the unit may read the platform's own headers.
    system_include: SystemInclude,
    /// Output lines holding any of these substrings are dropped before the
    /// three outputs are compared: a program that prints its own elapsed time
    /// or a pointer value cannot be compared byte for byte, and saying so
    /// here is better than not comparing it at all.
    output_filter: &'static [&'static str],
    /// What the row is for, one line, printed in the report.
    note: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Group {
    BenchmarksGame,
    Classic,
    Kernel,
}

impl Group {
    fn title(self) -> &'static str {
        match self {
            Group::BenchmarksGame => "Benchmarks Game",
            Group::Classic => "Classic micro-benchmarks",
            Group::Kernel => "Kernels written for this suite",
        }
    }
}

/// The default row: `gnu11!`, `main(int, char **)`, no input, no libraries.
const fn program(name: &'static str, source: &'static str, group: Group) -> Program {
    Program {
        name,
        source,
        group,
        dialect: Dialect::Gnu11,
        main: MainKind::ArgcArgv,
        args: &[],
        stdin: StdinSource::Empty,
        defines: &[],
        libs: &[],
        features: &[],
        system_include: SystemInclude::No,
        output_filter: &[],
        note: "",
    }
}

/// Every program, in report order.
///
/// The arguments are what size each run: they were chosen so that the
/// `gcc -O2` build takes roughly half a second on the machine the recorded
/// results were measured on, which is long enough for the median of a few runs
/// to be stable and short enough that the whole suite is a few minutes.
static PROGRAMS: &[Program] = &[
    // --- the Benchmarks Game --------------------------------------------
    Program {
        args: &["11"],
        note: "permutation flips over a small array; VLAs sized once",
        ..program(
            "fannkuch-redux",
            "benchmarksgame/fannkuchredux.c",
            Group::BenchmarksGame,
        )
    },
    Program {
        args: &["20000000"],
        note: "double-precision n-body integration, `sqrt` in the inner loop",
        ..program("n-body", "benchmarksgame/nbody.c", Group::BenchmarksGame)
    },
    Program {
        args: &["5500"],
        note: "eigenvalue by the power method; the array bounds are run-time values",
        ..program(
            "spectral-norm",
            "benchmarksgame/spectralnorm.c",
            Group::BenchmarksGame,
        )
    },
    Program {
        args: &["4000"],
        note: "escape-time loop over doubles, bit-packed PBM output through `putc`",
        ..program(
            "mandelbrot",
            "benchmarksgame/mandelbrot.c",
            Group::BenchmarksGame,
        )
    },
    Program {
        args: &["2500000"],
        // `setlinebuf` is BSD's, not standard C's, so the bundled `<stdio.h>`
        // — which is written in plain C99 — does not declare it, and `cinrs`
        // says so rather than guessing. `system_include first` is the answer
        // `doc/system-headers.md` gives: the platform's own `<stdio.h>` is
        // preferred,
        // which is where glibc keeps its extensions.
        system_include: SystemInclude::First,
        note: "weighted random selection, character at a time through a line-buffered stdout",
        ..program("fasta", "benchmarksgame/fasta.c", Group::BenchmarksGame)
    },
    Program {
        stdin: StdinSource::Fasta,
        main: MainKind::NoArgs,
        note: "reads 254 MB from stdin through `fgets`; a table lookup per byte",
        ..program(
            "reverse-complement",
            "benchmarksgame/revcomp.c",
            Group::BenchmarksGame,
        )
    },
    Program {
        args: &["18"],
        system_include: SystemInclude::Yes,
        note: "`malloc`/`free` churn over a recursive tree; needs the platform's <malloc.h>",
        ..program(
            "binary-trees",
            "benchmarksgame/binarytrees.c",
            Group::BenchmarksGame,
        )
    },
    Program {
        args: &["10000"],
        libs: &["gmp"],
        system_include: SystemInclude::Yes,
        note: "spigot digits of pi through GMP; the work is in the library, not in the C",
        ..program(
            "pidigits",
            "benchmarksgame/pidigits.c",
            Group::BenchmarksGame,
        )
    },
    // The SIMD-intrinsics versions, at the same sizes as the plain ones above
    // so that the two rows of a benchmark can be read against each other.
    Program {
        args: &["11"],
        features: &["ssse3"],
        note: "fannkuch-redux with each flip one `_mm_shuffle_epi8` (SSSE3)",
        ..program(
            "fannkuch-redux-ssse3",
            "benchmarksgame/fannkuchredux_ssse3.c",
            Group::BenchmarksGame,
        )
    },
    Program {
        args: &["20000000"],
        note: "n-body two pairs at a time in `__m128d`, `_mm_rsqrt_ps` and Newton steps (SSE2)",
        ..program(
            "n-body-sse",
            "benchmarksgame/nbody_sse.c",
            Group::BenchmarksGame,
        )
    },
    Program {
        args: &["20000000"],
        features: &["avx"],
        note: "n-body one body per `__m256d`, `_mm256_hadd_pd` and `_mm_rsqrt_ps` (AVX)",
        ..program(
            "n-body-avx",
            "benchmarksgame/nbody_avx.c",
            Group::BenchmarksGame,
        )
    },
    // The four spectral-norm versions and mandelbrot's carry `#pragma omp`,
    // which `cinrs` ignores and the native builds are not given `-fopenmp`
    // for, so all three columns are serial. `<malloc.h>` (`memalign`) and
    // `<unistd.h>` (`write`) are the platform's, hence `system_include`.
    Program {
        args: &["5500"],
        system_include: SystemInclude::Yes,
        note: "spectral-norm two columns at a time, `_mm_set_pd`/`_mm_div_pd` (SSE2)",
        ..program(
            "spectral-norm-sse2",
            "benchmarksgame/spectralnorm_sse2.c",
            Group::BenchmarksGame,
        )
    },
    Program {
        args: &["5500"],
        features: &["sse4.1"],
        system_include: SystemInclude::Yes,
        note: "spectral-norm with A's entries computed in `__m128i`, `_mm_mullo_epi32` (SSE4.1)",
        ..program(
            "spectral-norm-sse41",
            "benchmarksgame/spectralnorm_sse41.c",
            Group::BenchmarksGame,
        )
    },
    Program {
        args: &["5500"],
        features: &["avx2"],
        system_include: SystemInclude::Yes,
        note: "the SSE4.1 row at twice the width, `_mm256_mullo_epi32` (AVX2)",
        ..program(
            "spectral-norm-avx2",
            "benchmarksgame/spectralnorm_avx2.c",
            Group::BenchmarksGame,
        )
    },
    Program {
        args: &["5500"],
        features: &["avx"],
        note: "spectral-norm over 4x4 blocks of A, `_mm_rcp_ps` and a Goldschmidt step (AVX)",
        ..program(
            "spectral-norm-avx",
            "benchmarksgame/spectralnorm_avx.c",
            Group::BenchmarksGame,
        )
    },
    Program {
        args: &["4000"],
        features: &["sse3"],
        system_include: SystemInclude::Yes,
        note: "mandelbrot eight pixels in four `__m128d`, GNU vector operators and subscripts (SSE2)",
        ..program(
            "mandelbrot-sse2",
            "benchmarksgame/mandelbrot_sse2.c",
            Group::BenchmarksGame,
        )
    },
    // --- the classics ----------------------------------------------------
    Program {
        dialect: Dialect::Gnu89,
        main: MainKind::NoArgs,
        defines: &["TIME"],
        stdin: StdinSource::Text("50000000\n"),
        output_filter: &[
            "Ptr_Comp:",
            "Microseconds for one run",
            "Dhrystones per Second:",
            "Measured time too small",
            "Please increase number of runs",
        ],
        note: "Weicker 2.1: K&R C, struct assignment, `strcpy`/`strcmp`, an enum and a union",
        ..program("dhrystone", "classic/dhrystone.c", Group::Classic)
    },
    Program {
        // Whetstone times itself with `time(0)`, whose resolution is a whole
        // second, and *exits non-zero* when fewer than one has passed. So this
        // row is deliberately the longest in the suite: the loop count is the
        // one that puts every build safely over the boundary. `-DPRINTOUT` is
        // what makes it print anything deterministic at all — without it the
        // only output is the rate.
        args: &["400000"],
        defines: &["PRINTOUT"],
        output_filter: &["Loops:", "Whetstones:", "Insufficient duration"],
        note: "the floating-point classic: arrays, `sin`/`cos`/`exp`/`sqrt`, procedure calls",
        ..program("whetstone", "classic/whetstone.c", Group::Classic)
    },
    Program {
        args: &["1600"],
        note: "LINPACK-style LU with partial pivoting over `daxpy` (written here, see SOURCES.md)",
        ..program("linpack", "classic/linpack.c", Group::Classic)
    },
    // --- the kernels -----------------------------------------------------
    Program {
        args: &["40000000", "4"],
        note: "byte stores down a strided inner loop",
        ..program("sieve", "kernels/sieve.c", Group::Kernel)
    },
    Program {
        args: &["32", "10", "3", "8"],
        note: "call overhead: recursive fib, tak and Ackermann",
        ..program("recursion", "kernels/recursion.c", Group::Kernel)
    },
    Program {
        args: &["15"],
        note: "bitmask backtracking, no memory traffic",
        ..program("nqueens", "kernels/nqueens.c", Group::Kernel)
    },
    Program {
        args: &["1024", "4"],
        note: "double matmul over a flat array, `a[i * n + k]`",
        ..program("matmul", "kernels/matmul.c", Group::Kernel)
    },
    Program {
        args: &["1024", "4"],
        note: "the same, through the C99 parameter form `double a[n][n]`",
        ..program("matmul-vla", "kernels/matmul_vla.c", Group::Kernel)
    },
    Program {
        args: &["3000000", "2"],
        note: "quicksort and heapsort over `int`",
        ..program("sort", "kernels/sort.c", Group::Kernel)
    },
    Program {
        args: &["2000000", "8000000"],
        note: "unpredictable branches and cache misses",
        ..program("binsearch", "kernels/binsearch.c", Group::Kernel)
    },
    Program {
        args: &["20", "8"],
        note: "iterative radix-2 FFT, double precision",
        ..program("fft", "kernels/fft.c", Group::Kernel)
    },
    Program {
        args: &["4000000", "100"],
        note: "table-driven CRC-32: a serial dependency chain",
        ..program("crc32", "kernels/crc32.c", Group::Kernel)
    },
    Program {
        args: &["200000", "900"],
        note: "SHA-256 written out: 32-bit rotates and adds",
        ..program("sha256", "kernels/sha256.c", Group::Kernel)
    },
    Program {
        args: &["400000000"],
        note: "LCG and xorshift: `wrapping_mul` and `wrapping_add` and nothing else",
        ..program("prng", "kernels/prng.c", Group::Kernel)
    },
    Program {
        args: &["512", "500"],
        note: "nine-neighbour stencil over a byte grid",
        ..program("life", "kernels/life.c", Group::Kernel)
    },
    Program {
        args: &["2000", "200"],
        note: "edit-distance DP: three `min`s in the inner loop",
        ..program("levenshtein", "kernels/levenshtein.c", Group::Kernel)
    },
    Program {
        args: &["4000000", "20000000"],
        note: "open addressing with linear probing over a 16-byte struct",
        ..program("hashtable", "kernels/hashtable.c", Group::Kernel)
    },
    Program {
        args: &["4096", "4000000"],
        note: "control: `strlen`/`strcmp`/`memcpy`/`memcmp`, all of them libc's",
        ..program("libc-str", "kernels/libc_str.c", Group::Kernel)
    },
    Program {
        args: &["4096", "400000"],
        note: "the same routines written in C, so the compiler has to optimise them",
        ..program("hand-str", "kernels/hand_str.c", Group::Kernel)
    },
    Program {
        args: &["200000", "1200"],
        note: "bit-fields: every read and write is an accessor method in the expansion",
        ..program("bitfields", "kernels/bitfields.c", Group::Kernel)
    },
    Program {
        args: &["64", "10000000"],
        note: "a variable length array made afresh every iteration (a `Vec` in the expansion)",
        ..program("vla", "kernels/vla.c", Group::Kernel)
    },
    Program {
        args: &["64", "10000000"],
        note: "control for `vla`: the same work with one `malloc` outside the loop",
        ..program("vla-hoisted", "kernels/vla_hoisted.c", Group::Kernel)
    },
    Program {
        args: &["150000000"],
        note: "bytecode dispatch through a `switch`, with a fallthrough case",
        ..program("interp-switch", "kernels/interp_switch.c", Group::Kernel)
    },
    Program {
        args: &["150000000"],
        note: "the same machine through GCC's computed `goto`",
        ..program("interp-goto", "kernels/interp_goto.c", Group::Kernel)
    },
    Program {
        args: &["1000000000"],
        note: "small structs passed and returned by value",
        ..program("structval", "kernels/structval.c", Group::Kernel)
    },
    Program {
        args: &["4000000", "6000000"],
        note: "pointer chasing: a dependent load per step",
        ..program("chase", "kernels/chase.c", Group::Kernel)
    },
    Program {
        args: &["8000000", "30"],
        note: "a lexer written as `goto`s that jump into one another, which is what `cinrs` \
               lowers through a control-flow graph and reloops",
        ..program("statemachine", "kernels/statemachine.c", Group::Kernel)
    },
    Program {
        args: &["8000000", "30"],
        note: "control for `statemachine`: the same lexer with `while` and `switch`, no `goto`",
        ..program(
            "statemachine-structured",
            "kernels/statemachine_structured.c",
            Group::Kernel,
        )
    },
    Program {
        args: &["1600", "200"],
        note: "`double _Complex` arithmetic, Annex G recovery and all",
        ..program("complexmandel", "kernels/complexmandel.c", Group::Kernel)
    },
    Program {
        args: &["120000000"],
        note: "wrapping arithmetic at every width",
        ..program("wrapping", "kernels/wrapping.c", Group::Kernel)
    },
    Program {
        args: &["120000000"],
        note: "integer and floating division by run-time divisors",
        ..program("divide", "kernels/divide.c", Group::Kernel)
    },
    Program {
        args: &["200000", "4000"],
        note: "SSE2 intrinsics against the same kernels in scalar C: dot product, \
               sum of absolute differences, memchr",
        ..program("simd-dot", "kernels/simd_dot.c", Group::Kernel)
    },
];

/// Programs that are deliberately absent, and why. Printed in the report so
/// that a gap is a recorded decision rather than an oversight.
static SKIPPED: &[(&str, &str)] = &[
    (
        "k-nucleotide",
        "the only C version in the corpus (`knucleotide.gcc`) uses `khash.h` from samtools, \
         which is not part of the Benchmarks Game distribution, and OpenMP for its outer loop; \
         there is no version with a hash table of its own to vendor",
    ),
    (
        "regex-redux",
        "every C version needs PCRE2, and `pcre2.h` is not installed on this machine; with it, \
         the row would be `#pragma cinrs system_include` plus `#pragma cinrs link \"pcre2-8\"` \
         and nothing else",
    ),
];

// ---------------------------------------------------------------------------
// options
// ---------------------------------------------------------------------------

struct Options {
    filter: Option<String>,
    runs: usize,
    compilers: Vec<Native>,
    timeout: u64,
    build_timeout: u64,
    mem_limit_kb: u64,
    markdown: Option<PathBuf>,
    list: bool,
    keep_going: bool,
    /// Set from `/proc/self/limits` once, at startup: whether a timed run can
    /// be started without a shell in front of it. See [`timed_command`].
    limit_is_inherited: bool,
}

/// One of the native compilers, at one optimisation level.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Native {
    key: &'static str,
    program: &'static str,
    opt: &'static str,
}

const NATIVES: &[Native] = &[
    Native {
        key: "gcc",
        program: "gcc",
        opt: "-O2",
    },
    Native {
        key: "gcc-O3",
        program: "gcc",
        opt: "-O3",
    },
    Native {
        key: "clang",
        program: "clang",
        opt: "-O2",
    },
];

impl Options {
    fn parse() -> Result<Options, String> {
        let mut o = Options {
            filter: None,
            runs: 5,
            compilers: vec![NATIVES[0], NATIVES[2]],
            timeout: 60,
            build_timeout: 600,
            mem_limit_kb: 8_000_000,
            markdown: None,
            list: false,
            keep_going: false,
            limit_is_inherited: inherited_address_space_limit().is_some(),
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
            match arg.as_str() {
                "--filter" => o.filter = Some(value("--filter")?),
                "--runs" => {
                    o.runs = value("--runs")?
                        .parse()
                        .map_err(|_| "--runs needs a number")?
                }
                "--timeout" => {
                    o.timeout = value("--timeout")?
                        .parse()
                        .map_err(|_| "--timeout needs a number")?
                }
                "--build-timeout" => {
                    o.build_timeout = value("--build-timeout")?
                        .parse()
                        .map_err(|_| "--build-timeout")?
                }
                "--mem-limit-kb" => {
                    o.mem_limit_kb = value("--mem-limit-kb")?
                        .parse()
                        .map_err(|_| "--mem-limit-kb")?
                }
                "--markdown" => o.markdown = Some(PathBuf::from(value("--markdown")?)),
                "--compilers" => {
                    let list = value("--compilers")?;
                    let mut chosen = Vec::new();
                    for key in list.split(',').map(str::trim).filter(|k| !k.is_empty()) {
                        match NATIVES.iter().find(|n| n.key == key) {
                            Some(n) => chosen.push(*n),
                            None => {
                                return Err(format!(
                                    "unknown compiler {key:?}; known: gcc, gcc-O3, clang"
                                ));
                            }
                        }
                    }
                    o.compilers = chosen;
                }
                "--list" => o.list = true,
                "--keep-going" => o.keep_going = true,
                "-h" | "--help" => {
                    print_help();
                    std::process::exit(0);
                }
                other => return Err(format!("unknown argument {other:?}; try --help")),
            }
        }
        if o.runs == 0 {
            return Err("--runs must be at least 1".to_owned());
        }
        Ok(o)
    }
}

fn print_help() {
    println!(
        "cinrs-bench: cinrs-translated C against gcc -O2 and clang -O2

  --filter NAME       only programs whose name contains NAME
  --runs N            timed runs per build (default 5)
  --compilers LIST    gcc, gcc-O3, clang (default gcc,clang)
  --timeout SECS      per-execution ceiling (default 60)
  --build-timeout S   per-compilation ceiling (default 600)
  --mem-limit-kb KB   address-space ceiling per child (default 8000000; 0 inherits)
  --markdown PATH     write the report to PATH as well
  --list              print the program table and stop
  --keep-going        carry on after a build failure"
    );
}

// ---------------------------------------------------------------------------
// running a child, under a clock and a ceiling
// ---------------------------------------------------------------------------

/// Puts `ulimit -v` in front of a command:
///
/// ```text
/// sh -c 'ulimit -v "$0" 2>/dev/null; exec "$@"' <kb> <program> <args…>
/// ```
///
/// The shell lowers the limit and then `exec`s, so nothing is left waiting.
/// Lowering it is silent and best-effort: a process already under a *tighter*
/// ceiling — which is what the house wrapper around the whole harness provides
/// — keeps that one, since `RLIMIT_AS` is inherited by every descendant.
///
/// The clock is **not** `timeout(1)`. On this machine `timeout` is uutils'
/// reimplementation, which waits for its child by polling at 100 ms and so
/// rounds every measurement up to the next tenth of a second — which is most
/// of a benchmark. [`run_child`] blocks on the child itself instead, which is
/// exact and one process fewer.
fn shell_limited(program: &str, args: &[OsString], mem_limit_kb: u64) -> Command {
    if mem_limit_kb == 0 {
        let mut cmd = Command::new(program);
        cmd.args(args);
        return cmd;
    }
    let mut cmd = Command::new("sh");
    cmd.arg("-c");
    cmd.arg("ulimit -v \"$0\" 2>/dev/null; exec \"$@\"");
    cmd.arg(mem_limit_kb.to_string());
    cmd.arg(program);
    cmd.args(args);
    cmd
}

/// Whether the harness is already running under a finite `RLIMIT_AS`, which
/// every process it starts then inherits.
///
/// That is what the house wrapper — `( ulimit -v 8000000; … )` — provides, and
/// when it is there a *timed* run needs no `sh` in front of it, which is two
/// milliseconds of `fork` and `execve` off every measurement. When it is not,
/// the wrapper goes back on and the report says so.
fn inherited_address_space_limit() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/limits").ok()?;
    let line = text.lines().find(|l| l.starts_with("Max address space"))?;
    let soft = line["Max address space".len()..]
        .split_whitespace()
        .next()?;
    soft.parse::<u64>().ok().map(|bytes| bytes / 1024)
}

/// A command to *time*.
///
/// No shell in front of it when the ceiling is already inherited; the shell
/// wrapper otherwise, so that the ceiling is never simply absent.
fn timed_command(program: &str, args: &[OsString], o: &Options) -> Command {
    if o.limit_is_inherited {
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd
    } else {
        shell_limited(program, args, o.mem_limit_kb)
    }
}

struct Run {
    seconds: f64,
    status: Option<i32>,
    timed_out: bool,
    stderr: String,
}

/// Runs `cmd` to completion, or kills it after `timeout` seconds.
///
/// There is no polling loop and no sleep anywhere in the measurement. The
/// `Child` is handed to a thread that blocks in `wait()` and reads the clock
/// the instant it returns, so the elapsed time is the process's own lifetime
/// and not "the first moment a poll happened to notice". The caller waits on a
/// channel with `recv_timeout`, which is the timer.
///
/// That arrangement is also why this crate depends on `libc`: the waiting
/// thread owns the `&mut Child`, so the side holding the timer cannot call
/// `Child::kill` and signals by pid instead. A run that has overrun is killed
/// outright — it has nothing to flush, and its output is thrown away.
///
/// `stderr` is drained on a third thread, so that a compiler with a lot to say
/// cannot fill the pipe and deadlock the wait.
fn run_child(mut cmd: Command, stdin: Stdio, stdout: Stdio, timeout: u64) -> std::io::Result<Run> {
    use std::sync::mpsc::{RecvTimeoutError, channel};

    cmd.stdin(stdin).stdout(stdout).stderr(Stdio::piped());
    let started = Instant::now();
    let mut child = cmd.spawn()?;
    let pid = child.id() as libc::pid_t;

    let mut pipe = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(pipe) = pipe.as_mut() {
            use std::io::Read as _;
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });

    let (tx, rx) = channel();
    let waiter = std::thread::spawn(move || {
        let status = child.wait();
        let elapsed = started.elapsed();
        let _ = tx.send((status, elapsed));
    });

    let vanished = || std::io::Error::other("the thread waiting on the child went away");
    let (status, elapsed, timed_out) = if timeout == 0 {
        let (status, elapsed) = rx.recv().map_err(|_| vanished())?;
        (status?, elapsed, false)
    } else {
        match rx.recv_timeout(Duration::from_secs(timeout)) {
            Ok((status, elapsed)) => (status?, elapsed, false),
            Err(RecvTimeoutError::Timeout) => {
                // SAFETY: `pid` is this process's own child and has not been
                // reaped, because the only `wait` on it is the thread above and
                // it has not answered. (Should it answer in the moment between
                // the two, the signal goes to a pid that is no longer ours;
                // `kill` then fails, harmlessly, and the status arrives next.)
                unsafe { libc::kill(pid, libc::SIGKILL) };
                let (status, elapsed) = rx.recv().map_err(|_| vanished())?;
                (status?, elapsed, true)
            }
            Err(RecvTimeoutError::Disconnected) => return Err(vanished()),
        }
    };
    let _ = waiter.join();
    let stderr = reader.join().unwrap_or_default();

    Ok(Run {
        seconds: elapsed.as_secs_f64(),
        status: status.code(),
        timed_out,
        stderr,
    })
}

// ---------------------------------------------------------------------------
// generating the Rust file for the cinrs build
// ---------------------------------------------------------------------------

/// How many `#` a raw string literal holding `source` needs.
///
/// A `r#…#"` literal ends at the first `"` followed by that many `#`, so one
/// more than the longest run of `#` after a `"` anywhere inside is always
/// enough — and never fewer than one, so a bare `"` is safe. (The same
/// function as `tests/support/conformance.rs`, for the same reason.)
fn raw_string_hashes(source: &str) -> usize {
    let bytes = source.as_bytes();
    let mut needed = 1;
    for at in bytes
        .iter()
        .enumerate()
        .filter_map(|(at, &b)| (b == b'"').then_some(at))
    {
        let run = bytes[at + 1..].iter().take_while(|&&b| b == b'#').count();
        needed = needed.max(run + 1);
    }
    needed
}

/// The Rust file that wraps one C program.
///
/// The shape is `tests/c_testsuite.rs`'s: the whole translation unit inside
/// one raw string literal — the input form that accepts every C token — in a
/// `mod bench` of its own, and a `fn main` that calls the C `main` through the
/// glob re-export inside that module and exits with what it returned. The
/// `mod` is there because the unit defines a function called `main`, which
/// beside the wrapper's own would be a warning.
///
/// Two things are *prepended* to the C, because the preprocessor acts on them
/// where it reads them and they have to be in force before the first
/// `#include`: the include path (so that a program which includes a second
/// `.c` file next to it — Dhrystone does — finds it, since a string literal
/// has no directory of its own) and the row's `#define`s, which stand in for
/// the `-D` the native compilers get.
///
/// A UTF-8 byte order mark opening the program is dropped here, because
/// with the lines above in front of it it would no longer be at the start of
/// the text, where the lexer skips it, but in the middle, where it is a stray
/// character. The vendored file keeps it; the native compilers skip it too.
///
/// The prepended lines do shift every line number by their count, and a
/// `#line 1` would not undo that: cinrs honours `#line` for `__LINE__` and
/// `__FILE__`, but a diagnostic always points at the line really written.
fn generate_rust(p: &Program, csrc: &str, source_dir: &Path) -> String {
    let csrc = csrc.strip_prefix('\u{feff}').unwrap_or(csrc);
    let mut c = String::new();
    c.push_str(&format!(
        "#pragma cinrs include_path {:?}\n",
        source_dir.display().to_string()
    ));
    for define in p.defines {
        c.push_str(&format!("#define {define} 1\n"));
    }
    c.push_str(csrc);
    if !c.ends_with('\n') {
        c.push('\n');
    }
    let hashes = "#".repeat(raw_string_hashes(&c));

    let entry = p.dialect.macro_name();
    let name = p.name;
    let call = match p.main {
        MainKind::NoArgs => {
            "    let status = unsafe { bench::main() };\n\
             \x20   let _ = argv;\n"
        }
        MainKind::ArgcArgv => "    let status = unsafe { bench::main(argc, argv.as_mut_ptr()) };\n",
    };

    format!(
        "\
// GENERATED FILE - do not edit, do not check in.
//
// Written by `benches/cinrs-bench` from `programs/{source}` for the `{name}`
// benchmark. The C below is that file verbatim, with an include path and the
// row's `#define`s prepended to it inside the string literal.
//
// Regenerate by running `cargo run -p cinrs-bench --release -- --filter {name}`.

mod bench {{
    cinrs::{entry}! {{ r{hashes}\"{c}\"{hashes} }}
}}

fn main() {{
    let owned: Vec<std::ffi::CString> = std::env::args()
        .map(|a| std::ffi::CString::new(a).expect(\"an argument with no NUL in it\"))
        .collect();
    let mut argv: Vec<*mut core::ffi::c_char> =
        owned.iter().map(|a| a.as_ptr() as *mut core::ffi::c_char).collect();
    let argc = argv.len() as core::ffi::c_int;
    argv.push(core::ptr::null_mut());
{call}\
    std::process::exit(status as i32);
}}
",
        source = p.source,
    )
}

// ---------------------------------------------------------------------------
// building
// ---------------------------------------------------------------------------

struct Build {
    exe: PathBuf,
    /// Wall-clock seconds the compiler took.
    seconds: f64,
    /// Peak resident set of the compiler, in KiB, when `/usr/bin/time` could
    /// report it.
    rss_kb: Option<u64>,
}

/// Reads the `%e %M` line `/usr/bin/time` was asked to write.
fn read_time_file(path: &Path) -> Option<(f64, u64)> {
    let text = std::fs::read_to_string(path).ok()?;
    let line = text.lines().last()?;
    let mut it = line.split_whitespace();
    let secs = it.next()?.parse().ok()?;
    let rss = it.next()?.parse().ok()?;
    Some((secs, rss))
}

/// `/usr/bin/time -f "%e %M" -o FILE` in front of a command, when it is there.
fn with_time(program: &str, args: &[OsString], time_file: &Path) -> (String, Vec<OsString>) {
    if Path::new("/usr/bin/time").exists() {
        let mut wrapped: Vec<OsString> = vec![
            "-f".into(),
            "%e %M".into(),
            "-o".into(),
            time_file.into(),
            program.into(),
        ];
        wrapped.extend_from_slice(args);
        ("/usr/bin/time".to_owned(), wrapped)
    } else {
        (program.to_owned(), args.to_vec())
    }
}

fn build_native(
    p: &Program,
    native: Native,
    src: &Path,
    out_dir: &Path,
    o: &Options,
) -> Result<Build, String> {
    let exe = out_dir.join(format!("{}.{}", p.name, native.key));
    let time_file = out_dir.join(format!("{}.{}.time", p.name, native.key));
    let _ = std::fs::remove_file(&time_file);

    let mut args: Vec<OsString> = vec![
        native.opt.into(),
        p.dialect.std_flag().into(),
        // The vendored programs are decades old and warn freely; the warnings
        // are not what is being measured.
        "-w".into(),
        "-o".into(),
        exe.clone().into_os_string(),
        src.into(),
    ];
    for define in p.defines {
        args.push(format!("-D{define}").into());
    }
    for feature in p.features {
        args.push(format!("-m{feature}").into());
    }
    args.push("-lm".into());
    for lib in p.libs {
        args.push(format!("-l{lib}").into());
    }

    let (prog, args) = with_time(native.program, &args, &time_file);
    let cmd = shell_limited(&prog, &args, o.mem_limit_kb);
    let run = run_child(cmd, Stdio::null(), Stdio::piped(), o.build_timeout)
        .map_err(|e| format!("could not start {}: {e}", native.program))?;
    if run.timed_out {
        return Err(format!(
            "{} did not finish in {}s",
            native.program, o.build_timeout
        ));
    }
    if run.status != Some(0) {
        return Err(format!("{} failed:\n{}", native.program, run.stderr.trim()));
    }
    let (seconds, rss_kb) = match read_time_file(&time_file) {
        Some((s, r)) => (s, Some(r)),
        None => (run.seconds, None),
    };
    Ok(Build {
        exe,
        seconds,
        rss_kb,
    })
}

fn build_cinrs(
    p: &Program,
    csrc: &str,
    source_dir: &Path,
    out_dir: &Path,
    root: &Path,
    o: &Options,
) -> Result<Build, String> {
    let gen_dir = out_dir.join(p.name);
    std::fs::create_dir_all(&gen_dir).map_err(|e| format!("{}: {e}", gen_dir.display()))?;
    let generated = gen_dir.join("main.rs");
    std::fs::write(&generated, generate_rust(p, csrc, source_dir))
        .map_err(|e| format!("{}: {e}", generated.display()))?;

    let exe = out_dir.join(format!("{}.cinrs", p.name));
    let time_file = out_dir.join(format!("{}.cinrs.time", p.name));
    let _ = std::fs::remove_file(&time_file);

    let rlib = root.join("target/release/libcinrs.rlib");
    let deps = root.join("target/release/deps");
    let mut args: Vec<OsString> = vec![
        "--edition".into(),
        "2024".into(),
        // Cargo's `release` profile, with codegen-units turned down to one so
        // that the whole program reaches LLVM at once — the way the whole `.c`
        // file reaches GCC.
        "-C".into(),
        "opt-level=3".into(),
        "-C".into(),
        "codegen-units=1".into(),
        "-C".into(),
        "debug-assertions=off".into(),
        "-C".into(),
        "overflow-checks=off".into(),
        "-C".into(),
        "strip=symbols".into(),
        "--extern".into(),
        {
            let mut s = OsString::from("cinrs=");
            s.push(rlib.as_os_str());
            s
        },
        "-L".into(),
        deps.into_os_string(),
        "-o".into(),
        exe.clone().into_os_string(),
        generated.clone().into_os_string(),
    ];
    for lib in p.libs {
        args.push("-l".into());
        args.push((*lib).into());
    }
    if !p.features.is_empty() {
        let enabled: Vec<String> = p.features.iter().map(|f| format!("+{f}")).collect();
        args.push("-C".into());
        args.push(format!("target-feature={}", enabled.join(",")).into());
    }

    let (prog, args) = with_time("rustc", &args, &time_file);
    let mut cmd = shell_limited(&prog, &args, o.mem_limit_kb);
    if let Some(value) = p.system_include.env() {
        cmd.env("CINRS_SYSTEM_INCLUDE", value);
    }
    let run = run_child(cmd, Stdio::null(), Stdio::piped(), o.build_timeout)
        .map_err(|e| format!("could not start rustc: {e}"))?;
    if run.timed_out {
        return Err(format!("rustc did not finish in {}s", o.build_timeout));
    }
    if run.status != Some(0) {
        return Err(format!("rustc failed:\n{}", tail(&run.stderr, 40)));
    }
    let (seconds, rss_kb) = match read_time_file(&time_file) {
        Some((s, r)) => (s, Some(r)),
        None => (run.seconds, None),
    };
    Ok(Build {
        exe,
        seconds,
        rss_kb,
    })
}

/// What one `rustc` costs when there is no C in it at all.
///
/// Compiles `fn main() {}` with the very flags the `cinrs` builds get,
/// `--extern cinrs` included, so that the compile-time and peak-RSS column can
/// be read against something. Most of both numbers is `rustc` starting up and
/// loading the proc-macro; without this line a reader would have to guess how
/// much of a 170 MiB figure belongs to the expansion.
fn rustc_baseline(root: &Path, out_dir: &Path, o: &Options) -> Option<(f64, u64)> {
    let src = out_dir.join("baseline.rs");
    std::fs::write(
        &src,
        "// GENERATED: the empty program, compiled with the flags a `cinrs` build gets, so\n\
         // that the compile-time and peak-RSS figures have a floor to be read against.\n\
         fn main() {}\n",
    )
    .ok()?;
    let exe = out_dir.join("baseline");
    let time_file = out_dir.join("baseline.time");
    let _ = std::fs::remove_file(&time_file);

    let rlib = root.join("target/release/libcinrs.rlib");
    let deps = root.join("target/release/deps");
    let args: Vec<OsString> = vec![
        "--edition".into(),
        "2024".into(),
        "-C".into(),
        "opt-level=3".into(),
        "-C".into(),
        "codegen-units=1".into(),
        "-C".into(),
        "debug-assertions=off".into(),
        "-C".into(),
        "overflow-checks=off".into(),
        "-C".into(),
        "strip=symbols".into(),
        "--extern".into(),
        {
            let mut s = OsString::from("cinrs=");
            s.push(rlib.as_os_str());
            s
        },
        "-L".into(),
        deps.into_os_string(),
        "-o".into(),
        exe.clone().into_os_string(),
        src.into_os_string(),
    ];
    let (prog, args) = with_time("rustc", &args, &time_file);
    let cmd = shell_limited(&prog, &args, o.mem_limit_kb);
    let run = run_child(cmd, Stdio::null(), Stdio::piped(), o.build_timeout).ok()?;
    let _ = std::fs::remove_file(&exe);
    (run.status == Some(0))
        .then(|| read_time_file(&time_file))
        .flatten()
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    let from = all.len().saturating_sub(lines);
    all[from..].join("\n")
}

// ---------------------------------------------------------------------------
// measuring
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Measurement {
    /// Median of the timed runs, in seconds. `None` when a run timed out or
    /// the program failed.
    median: Option<f64>,
    min: Option<f64>,
    timed_out: bool,
    failure: Option<String>,
    /// Compile wall-clock and peak RSS.
    build_seconds: f64,
    build_rss_kb: Option<u64>,
    /// The bytes the program printed, after the row's filter.
    output: Vec<u8>,
}

fn stdin_for(p: &Program, data: &Paths) -> Result<Stdio, String> {
    Ok(match p.stdin {
        StdinSource::Empty => Stdio::null(),
        StdinSource::Text(_) => {
            let f = std::fs::File::open(&data.stdin_text)
                .map_err(|e| format!("{}: {e}", data.stdin_text.display()))?;
            Stdio::from(f)
        }
        StdinSource::Fasta => {
            let f = std::fs::File::open(&data.fasta)
                .map_err(|e| format!("{}: {e}", data.fasta.display()))?;
            Stdio::from(f)
        }
    })
}

/// One capture run (output kept) and `runs` timed runs (output to
/// `/dev/null`).
fn measure(
    p: &Program,
    build: &Build,
    label: &str,
    o: &Options,
    data: &Paths,
    out_dir: &Path,
) -> Measurement {
    let args: Vec<OsString> = p.args.iter().map(|a| OsString::from(*a)).collect();
    let exe = build.exe.display().to_string();
    let capture = out_dir.join(format!("{}.{}.out", p.name, label));

    let mut m = Measurement {
        median: None,
        min: None,
        timed_out: false,
        failure: None,
        build_seconds: build.seconds,
        build_rss_kb: build.rss_kb,
        output: Vec::new(),
    };

    // the capture run
    let stdin = match stdin_for(p, data) {
        Ok(s) => s,
        Err(e) => {
            m.failure = Some(e);
            return m;
        }
    };
    let file = match std::fs::File::create(&capture) {
        Ok(f) => f,
        Err(e) => {
            m.failure = Some(format!("{}: {e}", capture.display()));
            return m;
        }
    };
    eprintln!("    {label:<8} capture");
    let cmd = timed_command(&exe, &args, o);
    match run_child(cmd, stdin, Stdio::from(file), o.timeout) {
        Ok(run) if run.timed_out => {
            m.timed_out = true;
            m.failure = Some(format!("timed out after {}s", o.timeout));
            return m;
        }
        Ok(run) if run.status != Some(0) => {
            m.failure = Some(format!(
                "exit status {:?}: {}",
                run.status,
                tail(run.stderr.trim(), 6)
            ));
            return m;
        }
        Ok(_) => {}
        Err(e) => {
            m.failure = Some(format!("could not run: {e}"));
            return m;
        }
    }
    m.output = match std::fs::read(&capture) {
        Ok(bytes) => filter_output(&bytes, p.output_filter),
        Err(e) => {
            m.failure = Some(format!("{}: {e}", capture.display()));
            return m;
        }
    };
    let _ = std::fs::remove_file(&capture);

    // the timed runs
    let mut times = Vec::with_capacity(o.runs);
    for i in 1..=o.runs {
        let stdin = match stdin_for(p, data) {
            Ok(s) => s,
            Err(e) => {
                m.failure = Some(e);
                return m;
            }
        };
        eprint!("    {label:<8} run {i}/{}", o.runs);
        let cmd = timed_command(&exe, &args, o);
        match run_child(cmd, stdin, Stdio::null(), o.timeout) {
            Ok(run) if run.timed_out => {
                eprintln!(" — timed out");
                m.timed_out = true;
                m.failure = Some(format!("timed out after {}s", o.timeout));
                return m;
            }
            Ok(run) if run.status != Some(0) => {
                eprintln!(" — exit {:?}", run.status);
                m.failure = Some(format!("exit status {:?}", run.status));
                return m;
            }
            Ok(run) => {
                eprintln!(" — {:.3}s", run.seconds);
                times.push(run.seconds);
            }
            Err(e) => {
                eprintln!(" — {e}");
                m.failure = Some(format!("could not run: {e}"));
                return m;
            }
        }
    }
    times.sort_by(|a, b| a.partial_cmp(b).expect("no NaN in a measured duration"));
    m.min = times.first().copied();
    m.median = Some(times[times.len() / 2]);
    m
}

/// Drops the lines a row says are not comparable, and normalises the trailing
/// newline so that a program which ends without one still compares equal to
/// itself.
fn filter_output(bytes: &[u8], filters: &[&str]) -> Vec<u8> {
    if filters.is_empty() {
        return bytes.to_vec();
    }
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    for line in text.lines() {
        if filters.iter().any(|f| line.contains(f)) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.into_bytes()
}

// ---------------------------------------------------------------------------
// one program
// ---------------------------------------------------------------------------

struct Result1 {
    name: &'static str,
    group: Group,
    input: String,
    note: &'static str,
    /// Keyed by column label: the native keys, and `cinrs`.
    measurements: BTreeMap<String, Measurement>,
    /// `None` when every build agreed, otherwise what disagreed.
    mismatch: Option<String>,
}

struct Paths {
    root: PathBuf,
    programs: PathBuf,
    out: PathBuf,
    data: PathBuf,
    fasta: PathBuf,
    stdin_text: PathBuf,
}

fn describe_input(p: &Program) -> String {
    let mut s = p.args.join(" ");
    match p.stdin {
        StdinSource::Empty => {}
        StdinSource::Text(t) => {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(&format!("stdin={}", t.trim()));
        }
        StdinSource::Fasta => {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(&format!("stdin=fasta {FASTA_N} (254 MB)"));
        }
    }
    if s.is_empty() { "—".to_owned() } else { s }
}

fn bench_one(p: &Program, o: &Options, paths: &Paths) -> Result<Result1, String> {
    let src = paths.programs.join(p.source);
    let csrc = std::fs::read_to_string(&src).map_err(|e| format!("{}: {e}", src.display()))?;
    let source_dir = src
        .parent()
        .expect("a source file has a directory")
        .to_path_buf();

    let mut measurements: BTreeMap<String, Measurement> = BTreeMap::new();

    for native in &o.compilers {
        eprintln!("  {} [{}]", p.name, native.key);
        let build = build_native(p, *native, &src, &paths.out, o)?;
        let m = measure(p, &build, native.key, o, paths, &paths.out);
        let _ = std::fs::remove_file(&build.exe);
        measurements.insert(native.key.to_owned(), m);
    }

    eprintln!("  {} [cinrs]", p.name);
    let build = build_cinrs(p, &csrc, &source_dir, &paths.out, &paths.root, o)?;
    eprintln!(
        "    cinrs    compiled in {:.1}s, peak RSS {}",
        build.seconds,
        match build.rss_kb {
            Some(kb) => format!("{:.0} MiB", kb as f64 / 1024.0),
            None => "unknown".to_owned(),
        }
    );
    let m = measure(p, &build, "cinrs", o, paths, &paths.out);
    let _ = std::fs::remove_file(&build.exe);
    measurements.insert("cinrs".to_owned(), m);

    // Every build must have printed the same bytes.
    let mut mismatch = None;
    let reference = o
        .compilers
        .first()
        .map(|n| n.key.to_owned())
        .unwrap_or_else(|| "cinrs".into());
    if let Some(want) = measurements.get(&reference).map(|m| m.output.clone()) {
        for (label, m) in &measurements {
            if *label == reference || m.failure.is_some() {
                continue;
            }
            if m.output != want {
                mismatch = Some(format!(
                    "{label} printed {} bytes, {reference} printed {}{}",
                    m.output.len(),
                    want.len(),
                    first_difference(&want, &m.output)
                ));
                break;
            }
        }
    }

    Ok(Result1 {
        name: p.name,
        group: p.group,
        input: describe_input(p),
        note: p.note,
        measurements,
        mismatch,
    })
}

fn first_difference(a: &[u8], b: &[u8]) -> String {
    let at = a.iter().zip(b).position(|(x, y)| x != y);
    match at {
        Some(at) => {
            let show = |s: &[u8]| {
                let from = at.saturating_sub(20);
                let to = (at + 40).min(s.len());
                String::from_utf8_lossy(&s[from..to]).replace('\n', "\\n")
            };
            format!(
                "; first difference at byte {at}: {:?} vs {:?}",
                show(a),
                show(b)
            )
        }
        None => "; one is a prefix of the other".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// preparation
// ---------------------------------------------------------------------------

/// The workspace root, from this package's directory.
fn workspace_root() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .and_then(Path::parent)
        .expect("benches/cinrs-bench is two deep")
        .to_path_buf()
}

fn ensure_cinrs_built(root: &Path, o: &Options) -> Result<(), String> {
    eprintln!("building the crate: cargo build --release -p cinrs");
    let args: Vec<OsString> = vec![
        "build".into(),
        "--release".into(),
        "-p".into(),
        "cinrs".into(),
    ];
    let mut cmd = shell_limited("cargo", &args, o.mem_limit_kb);
    cmd.current_dir(root);
    let run = run_child(cmd, Stdio::null(), Stdio::piped(), o.build_timeout)
        .map_err(|e| format!("could not start cargo: {e}"))?;
    if run.status != Some(0) {
        return Err(format!("cargo build failed:\n{}", tail(&run.stderr, 30)));
    }
    let rlib = root.join("target/release/libcinrs.rlib");
    if !rlib.exists() {
        return Err(format!(
            "{} is missing after a release build",
            rlib.display()
        ));
    }
    Ok(())
}

/// The fasta output reverse-complement reads, generated once with the `gcc`
/// build of fasta — which is how the Benchmarks Game feeds it.
const FASTA_N: &str = "25000000";

fn ensure_fasta(paths: &Paths, o: &Options) -> Result<(), String> {
    if paths.fasta.exists() {
        return Ok(());
    }
    eprintln!("generating {} (fasta {FASTA_N})", paths.fasta.display());
    let src = paths.programs.join("benchmarksgame/fasta.c");
    let fasta_program = PROGRAMS
        .iter()
        .find(|p| p.name == "fasta")
        .ok_or("the table has no `fasta` row to build the input with")?;
    let build = build_native(fasta_program, NATIVES[0], &src, &paths.out, o)?;
    let file = std::fs::File::create(&paths.fasta)
        .map_err(|e| format!("{}: {e}", paths.fasta.display()))?;
    let cmd = shell_limited(
        &build.exe.display().to_string(),
        &[OsString::from(FASTA_N)],
        o.mem_limit_kb,
    );
    let run = run_child(cmd, Stdio::null(), Stdio::from(file), o.build_timeout)
        .map_err(|e| format!("could not run fasta: {e}"))?;
    let _ = std::fs::remove_file(&build.exe);
    if run.status != Some(0) {
        let _ = std::fs::remove_file(&paths.fasta);
        return Err(format!("fasta exited with {:?}", run.status));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// the machine
// ---------------------------------------------------------------------------

fn first_line(program: &str, args: &[&str], pick: impl Fn(&str) -> bool) -> String {
    let out = Command::new(program).args(args).output();
    match out {
        Ok(out) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            text.lines()
                .find(|l| pick(l))
                .unwrap_or("unknown")
                .trim()
                .to_owned()
        }
        Err(_) => "not found".to_owned(),
    }
}

struct Machine {
    cpu: String,
    cores: String,
    memory: String,
    kernel: String,
    rustc: String,
    gcc: String,
    clang: String,
    date: String,
}

fn describe_machine() -> Machine {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let cpu = cpuinfo
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    let cores = cpuinfo
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count()
        .to_string();
    let memory = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|t| {
            t.lines().find(|l| l.starts_with("MemTotal")).map(|l| {
                let kb: f64 = l
                    .split_whitespace()
                    .nth(1)
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0.0);
                format!("{:.0} GiB", kb / 1024.0 / 1024.0)
            })
        })
        .unwrap_or_else(|| "unknown".to_owned());
    Machine {
        cpu,
        cores,
        memory,
        kernel: first_line("uname", &["-sr"], |_| true),
        rustc: first_line("rustc", &["-V"], |_| true),
        gcc: first_line("gcc", &["--version"], |_| true),
        clang: first_line("clang", &["--version"], |l| l.contains("clang version")),
        date: first_line("date", &["-u", "+%Y-%m-%d"], |_| true),
    }
}

// ---------------------------------------------------------------------------
// the report
// ---------------------------------------------------------------------------

fn fmt_time(m: &Measurement) -> String {
    if m.timed_out {
        return "timed out".to_owned();
    }
    match m.median {
        Some(s) => format!("{s:.3}"),
        None => "failed".to_owned(),
    }
}

fn fmt_ratio(m: &Measurement, base: Option<&Measurement>) -> String {
    if m.timed_out {
        return "> timeout".to_owned();
    }
    match (m.median, base.and_then(|b| b.median)) {
        (Some(a), Some(b)) if b > 0.0 => format!("{:.2}×", a / b),
        _ => "—".to_owned(),
    }
}

fn write_report(
    out: &mut dyn std::io::Write,
    results: &[Result1],
    o: &Options,
    machine: &Machine,
    // What one `rustc` costs with no C in it; see `rustc_baseline`.
    baseline: Option<(f64, u64)>,
) -> std::io::Result<()> {
    let columns: Vec<String> = o
        .compilers
        .iter()
        .map(|n| n.key.to_owned())
        .chain(["cinrs".to_owned()])
        .collect();

    writeln!(out, "# cinrs benchmarks\n")?;
    writeln!(
        out,
        "How fast is the code `cinrs` generates? Each program below is one whole C program,\n\
         built three ways — `gcc -O2`, `clang -O2`, and as a `cinrs` macro invocation compiled\n\
         by `rustc -C opt-level=3` — and run {} times per build with the median wall clock\n\
         reported. The outputs of all three builds are compared byte for byte, so a\n\
         miscompilation shows up here as loudly as a slowdown.\n",
        o.runs
    )?;
    writeln!(
        out,
        "The programs are the single-threaded C entries from [The Computer Language\n\
         Benchmarks Game](https://benchmarksgame-team.pages.debian.net/benchmarksgame/),\n\
         Dhrystone 2.1 and Whetstone, and two dozen kernels written to isolate one\n\
         construct each (bit-fields, heap-emulated variable length arrays, `goto`\n\
         lowering, `switch` against computed `goto`, `_Complex`, wrapping arithmetic,\n\
         division). That comparison makes the suite a differential test as well as a\n\
         benchmark, and `benches/cinrs-bench` is the harness: its\n\
         [README](../benches/cinrs-bench/README.md) says how to run it and how to add a\n\
         program.\n"
    )?;
    writeln!(out, "Regenerate with\n")?;
    writeln!(out, "```text")?;
    writeln!(out, "( ulimit -v 8000000; timeout -k 10 3600 \\")?;
    writeln!(
        out,
        "      cargo run -p cinrs-bench --release -- --markdown doc/benchmarks.md )"
    )?;
    writeln!(out, "```\n")?;

    writeln!(out, "## The machine\n")?;
    writeln!(out, "| | |")?;
    writeln!(out, "| --- | --- |")?;
    writeln!(
        out,
        "| CPU | {} ({} logical cores) |",
        machine.cpu, machine.cores
    )?;
    writeln!(out, "| Memory | {} |", machine.memory)?;
    writeln!(out, "| Kernel | {} |", machine.kernel)?;
    writeln!(out, "| Rust | {} |", machine.rustc)?;
    writeln!(out, "| GCC | {} |", machine.gcc)?;
    writeln!(out, "| Clang | {} |", machine.clang)?;
    writeln!(out, "| Measured | {} |", machine.date)?;
    writeln!(out)?;

    // the results, group by group
    for group in [Group::BenchmarksGame, Group::Classic, Group::Kernel] {
        let rows: Vec<&Result1> = results.iter().filter(|r| r.group == group).collect();
        if rows.is_empty() {
            continue;
        }
        writeln!(out, "## {}\n", group.title())?;

        write!(out, "| program | input |")?;
        for c in &columns {
            write!(out, " {c} (s) |")?;
        }
        for c in &columns {
            if c != "cinrs" {
                write!(out, " cinrs/{c} |")?;
            }
        }
        writeln!(out, " output |")?;

        write!(out, "| --- | --- |")?;
        for _ in &columns {
            write!(out, " ---: |")?;
        }
        for c in &columns {
            if c != "cinrs" {
                write!(out, " ---: |")?;
            }
        }
        writeln!(out, " --- |")?;

        for r in rows {
            write!(out, "| `{}` | `{}` |", r.name, r.input)?;
            for c in &columns {
                match r.measurements.get(c) {
                    Some(m) => write!(out, " {} |", fmt_time(m))?,
                    None => write!(out, " — |")?,
                }
            }
            let cinrs = r.measurements.get("cinrs");
            for c in &columns {
                if c == "cinrs" {
                    continue;
                }
                match (cinrs, r.measurements.get(c)) {
                    (Some(m), base) => write!(out, " {} |", fmt_ratio(m, base))?,
                    _ => write!(out, " — |")?,
                }
            }
            match &r.mismatch {
                Some(_) => writeln!(out, " **MISMATCH** |")?,
                None => writeln!(out, " same |")?,
            }
        }
        writeln!(out)?;

        writeln!(out, "<details><summary>what each row exercises</summary>\n")?;
        for r in results.iter().filter(|r| r.group == group) {
            writeln!(out, "* `{}` — {}", r.name, r.note)?;
        }
        writeln!(out, "\n</details>\n")?;
    }

    // failures and mismatches
    let failures: Vec<(&Result1, &String, &Measurement)> = results
        .iter()
        .flat_map(|r| {
            r.measurements
                .iter()
                .filter_map(move |(label, m)| m.failure.as_ref().map(|_| (r, label, m)))
        })
        .collect();
    if !failures.is_empty() {
        writeln!(out, "## Runs that did not finish\n")?;
        for (r, label, m) in failures {
            writeln!(
                out,
                "* `{}` [{}] — {}",
                r.name,
                label,
                m.failure.as_deref().unwrap_or("?")
            )?;
        }
        writeln!(out)?;
    }

    let mismatches: Vec<&Result1> = results.iter().filter(|r| r.mismatch.is_some()).collect();
    writeln!(out, "## Output agreement\n")?;
    if mismatches.is_empty() {
        writeln!(
            out,
            "Every program printed the same bytes from all {} builds.\n",
            columns.len()
        )?;
    } else {
        writeln!(
            out,
            "**These programs did not print the same bytes from every build.**\n"
        )?;
        for r in mismatches {
            writeln!(
                out,
                "* `{}` — {}",
                r.name,
                r.mismatch.as_deref().unwrap_or("?")
            )?;
        }
        writeln!(out)?;
    }

    // compile time and memory
    writeln!(out, "## Compile time and memory\n")?;
    writeln!(
        out,
        "The `cinrs` column is one `rustc` process: the macro expands the C — lexer, \
         preprocessor, parser, semantic analysis, code generation — and then `rustc` compiles \
         what came out at `-C opt-level=3 -C codegen-units=1`. The native columns are one \
         `gcc`/`clang` process each. Peak resident set is `/usr/bin/time -f %M`.\n"
    )?;
    write!(out, "| program | C lines |")?;
    for c in &columns {
        if c != "cinrs" {
            write!(out, " {c} (s) |")?;
        }
    }
    writeln!(out, " cinrs rustc (s) | cinrs peak RSS |")?;
    write!(out, "| --- | ---: |")?;
    for c in &columns {
        if c != "cinrs" {
            write!(out, " ---: |")?;
        }
    }
    writeln!(out, " ---: | ---: |")?;
    for r in results {
        write!(out, "| `{}` | {} |", r.name, line_count(r))?;
        for c in &columns {
            if c == "cinrs" {
                continue;
            }
            match r.measurements.get(c) {
                Some(m) => write!(out, " {:.2} |", m.build_seconds)?,
                None => write!(out, " — |")?,
            }
        }
        match r.measurements.get("cinrs") {
            Some(m) => {
                write!(out, " {:.2} |", m.build_seconds)?;
                match m.build_rss_kb {
                    Some(kb) => writeln!(out, " {:.0} MiB |", kb as f64 / 1024.0)?,
                    None => writeln!(out, " — |")?,
                }
            }
            None => writeln!(out, " — | — |")?,
        }
    }
    writeln!(out)?;

    // The point of the two numbers is to catch a front end that has gone
    // quadratic in something, so say plainly whether either is anywhere near
    // being a problem rather than leaving it to be read off forty rows.
    let worst_time = results
        .iter()
        .filter_map(|r| {
            r.measurements
                .get("cinrs")
                .map(|m| (m.build_seconds, r.name))
        })
        .max_by(|a, b| a.0.partial_cmp(&b.0).expect("no NaN in a compile time"));
    let worst_rss = results
        .iter()
        .filter_map(|r| {
            r.measurements
                .get("cinrs")
                .and_then(|m| m.build_rss_kb)
                .map(|kb| (kb, r.name))
        })
        .max();
    if let (Some((secs, slowest)), Some((kb, fattest))) = (worst_time, worst_rss) {
        let mib = kb as f64 / 1024.0;
        let over = secs > 30.0 || mib > 2048.0;
        writeln!(
            out,
            "The slowest `cinrs` compilation is `{slowest}` at **{secs:.2} s**, and the largest \
             is `{fattest}` at **{mib:.0} MiB** — {}.{}",
            if over {
                "**over the thresholds this report flags** (30 s, 2 GiB)"
            } else {
                "neither is close to the thresholds this report flags, which are 30 s and 2 GiB"
            },
            if over {
                " **This needs looking at.**"
            } else {
                ""
            }
        )?;
        match baseline {
            Some((base_secs, base_kb)) => writeln!(
                out,
                " Read them against the floor: `fn main() {{}}`, compiled with the very same \
                 flags and the same `--extern cinrs`, costs **{base_secs:.2} s** and \
                 **{:.0} MiB** on this machine. Nearly all of both columns is `rustc` starting \
                 and loading the procedural macro, not the C being translated.\n",
                base_kb as f64 / 1024.0
            )?,
            None => writeln!(out)?,
        }
    }
    writeln!(
        out,
        "\"C lines\" is the length of the file named in the program table. `dhrystone` is the \
         exception: the twenty-one lines there are an amalgamation that `#include`s Weicker's \
         two source files, about seven hundred lines between them.\n"
    )?;

    writeln!(out, "## Programs not in the suite\n")?;
    for (name, why) in SKIPPED {
        writeln!(out, "* **{name}** — {why}")?;
    }
    writeln!(out)?;

    write_interpretation(out, results, o)?;

    Ok(())
}

/// The ratio of a result's `cinrs` time to one native column's, when both ran.
fn ratio(r: &Result1, against: &str) -> Option<f64> {
    let cinrs = r.measurements.get("cinrs")?.median?;
    let base = r.measurements.get(against)?.median?;
    (base > 0.0).then(|| cinrs / base)
}

/// Everything below this is "the same speed" as far as this suite is
/// concerned: process startup, page-cache state and the operating system's
/// scheduling account for a few percent between runs, and nothing here is a
/// microbenchmark harness with warm-up phases.
const NOISE: f64 = 1.10;

/// What the numbers say, worked out from the numbers.
///
/// The prose is fixed — it is about mechanisms in the expansion, which do not
/// change from run to run — but every row named in it is selected from the
/// measurements that were just taken, so the section cannot go stale the way a
/// hand-written summary would.
fn write_interpretation(
    out: &mut dyn std::io::Write,
    results: &[Result1],
    o: &Options,
) -> std::io::Result<()> {
    let has_clang = o.compilers.iter().any(|n| n.key == "clang");
    writeln!(out, "## What the numbers say\n")?;

    let mut ratios: Vec<(f64, &Result1)> = results
        .iter()
        .filter_map(|r| ratio(r, "gcc").map(|x| (x, r)))
        .collect();
    if ratios.is_empty() {
        writeln!(
            out,
            "Nothing was measured against `gcc`, so there is nothing to say.\n"
        )?;
        return Ok(());
    }
    ratios.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("no NaN in a ratio"));

    let n = ratios.len();
    let within10 = ratios.iter().filter(|(x, _)| *x <= NOISE).count();
    let median = ratios[n / 2].0;
    let (worst, worst_row) = *ratios.last().expect("checked non-empty");
    let (best, best_row) = ratios[0];

    writeln!(
        out,
        "Across the {n} programs measured, the median `cinrs`/`gcc -O2` ratio is **{median:.2}×**, \
         and **{within10} of {n}** are within 10 % of `gcc -O2` or faster. The extremes are \
         `{}` at {best:.2}× and `{}` at {worst:.2}×.\n",
        best_row.name, worst_row.name
    )?;

    if has_clang {
        // A row where clang is as slow as cinrs is not a cinrs finding: both
        // go through LLVM, and the difference is GCC's code generator against
        // LLVM's.
        let mut own: Vec<(&Result1, f64, f64)> = Vec::new();
        let mut llvm: Vec<(&Result1, f64, f64)> = Vec::new();
        let mut faster: Vec<(&Result1, f64)> = Vec::new();
        for (rg, r) in &ratios {
            let Some(rc) = ratio(r, "clang") else {
                continue;
            };
            if *rg > NOISE && rc > NOISE {
                own.push((r, *rg, rc));
            } else if *rg > NOISE {
                llvm.push((r, *rg, rc));
            } else if *rg < 1.0 / NOISE {
                faster.push((r, *rg));
            }
        }

        writeln!(
            out,
            "The `clang` column is what separates the two kinds of difference. `cinrs` and \
             `clang` share a back end, so a row where `clang` is exactly as slow as `cinrs` is \
             not saying anything about the translation at all — it is LLVM's code generator \
             against GCC's, and every Rust program on the machine is subject to it. A row where \
             `cinrs` is slower than **both** is the translation's own.\n"
        )?;

        writeln!(
            out,
            "**Slower than both, which is `cinrs`'s own to answer for:**\n"
        )?;
        if own.is_empty() {
            writeln!(
                out,
                "* none — no program is more than 10 % behind both compilers.\n"
            )?;
        } else {
            for (r, rg, rc) in &own {
                writeln!(
                    out,
                    "* `{}` — {rg:.2}× gcc, {rc:.2}× clang. {}",
                    r.name, r.note
                )?;
            }
            writeln!(out)?;
        }

        writeln!(
            out,
            "**Slower than `gcc` but level with `clang`, i.e. LLVM against GCC and not this \
             crate:**\n"
        )?;
        if llvm.is_empty() {
            writeln!(out, "* none.\n")?;
        } else {
            for (r, rg, rc) in &llvm {
                writeln!(out, "* `{}` — {rg:.2}× gcc, {rc:.2}× clang.", r.name,)?;
            }
            writeln!(out)?;
        }

        writeln!(out, "**Faster than `gcc -O2`:**\n")?;
        if faster.is_empty() {
            writeln!(out, "* none.\n")?;
        } else {
            for (r, rg) in &faster {
                writeln!(out, "* `{}` — {rg:.2}× gcc.", r.name)?;
            }
            writeln!(out)?;
        }
    }

    writeln!(
        out,
        "### Why, construct by construct\n\n\
         What the expansion does with each of these is in [What works](features.md); what it \
         costs is here.\n\n\
         * **Arithmetic wraps for free.** C's unsigned arithmetic is modular, so the expansion \
           is `wrapping_add`, `wrapping_mul` and their relatives rather than Rust's `+` and `*`. \
           Those are `#[inline]` intrinsics that lower to the bare instruction, and `prng` — a \
           loop that is nothing but a multiply, an add and three shifts — and `wrapping`, which \
           does the same at every width C has, are the measurement: both land on `gcc`. If they \
           did not, every arithmetic row in the table would be paying for it. *Signed* \
           arithmetic wraps too, by choice: C leaves its overflow undefined, and `cinrs` is \
           `gcc -fwrapv` rather than `gcc`, because a translation into Rust should do one \
           definite thing. That has a price where an `int` index or a signed division is on \
           the hot path, since a back end that may assume no overflow can prove \
           `(i + j) * (i + j + 1)` non-negative and halve it with a shift, and can widen an \
           `int` counter to 64 bits once instead of sign-extending it at every use. \
           `spectral-norm-sse2` is that row: `gcc` and `clang` built with `-fwrapv` take the \
           time `cinrs` takes, and nothing else in the table is affected.\n\
         * **There are no bounds checks.** A C array is a raw pointer and a subscript is \
           `.offset()`, which is plain address arithmetic with no check in it — `sieve`, \
           `matmul`, `life`, `crc32` and `binsearch` are where that shows, and none of them has \
           a Rust tax to pay.\n\
         * **Division has a check C does not.** Rust's `/` and `%` panic on a zero divisor, and \
           signed division also has to rule out `INT_MIN / -1`; C's do neither. `divide` is a \
           loop of nothing but divisions by run-time divisors, which is the worst case, and the \
           check does not show above the latency of the divider itself.\n\
         * **A bit-field is a pair of methods.** A bit-field has no address, so it is not a \
           field of the generated `#[repr(C)]` struct: a run of them shares one `[u8; K]` and \
           each named member becomes `s.ttl()` and `s.set_ttl(v)`. `bitfields` parses and \
           repacks an IP header a hundred million times over, so every one of those is a call \
           that has to be inlined and folded back into a shift and a mask before it can keep \
           up. It does.\n\
         * **A variable length array is a bump off an arena.** Rust cannot move the stack \
           pointer by an amount chosen at run time, so a VLA and `alloca` are emulated on the \
           heap: each call of a function that has one gets a bump arena, an array is a bump \
           off it, and the end of the block moves the arena back down. `vla` makes one per \
           iteration and `vla-hoisted` does the same work with one `malloc` outside the loop; \
           the difference between those two rows is exactly what the emulation costs, and it \
           is a pointer bump and a `memset` of the zeroed elements per declaration where a \
           native compiler adjusts the stack — about what the `vla` row's own work costs, \
           where an allocation and a free per iteration were a quarter of the time. A VLA \
           declared once and used in a loop — which is what `spectral-norm` and \
           `fannkuch-redux` do — costs nothing.\n\
         * **A `goto` keeps the shape its C had.** A jump forwards to a label later in a block \
           it stands in becomes `break 'done`, and one backwards to a label that block begins \
           with becomes `continue 'retry`, so the function keeps the shape its C had — hot \
           loop included. `whetstone`, whose `main` and whose inner `PA` are built out of \
           backward jumps, and `interp-switch`, whose dispatch loop leaves through \
           `goto done`, are what that is worth: both were more than 40 % behind `gcc` when \
           every jump went through a state machine over basic blocks. What no Rust label can \
           express — a jump *into* a block, or two labels whose regions would have to overlap \
           without nesting — goes through a control-flow graph, and a relooper reads that \
           back into Rust's own loops and `match`es, so the hot loop is a hot loop there too. \
           `statemachine` and `statemachine-structured` are the same lexer over the same \
           input, one written with a dozen labels that jump into one another every which way \
           and one with `while` and `switch`: all three compilers take the same time over \
           both, where `cinrs` was 2.16× on the `goto` version and 1.02× on the other before \
           the graph was relooped. A computed `goto` is lowered as GCC lowers it, a `switch` \
           over the labels whose address is taken, and relooped like any other — \
           `interp-goto` is that row.\n\
         * **`switch` becomes `match`.** `interp-switch` is a bytecode dispatch loop with a \
           fallthrough case; the fallthrough has to run the next arm's body without \
           re-dispatching, which is what the labelled-block chain the expansion builds is for. \
           The `goto done` that leaves the loop costs it nothing any more.\n\
         * **Structs by value go through Rust's C ABI.** `structval` passes and returns a \
           two-`double` struct (two SSE registers on x86-64 System V) and a three-`long` one \
           (memory). Nothing in the expansion decides that — `#[repr(C)]` and \
           `extern \"C\"` hand it to `rustc`. It is not what the row's gap to `gcc` is, \
           either: every call is inlined in all three builds and no struct touches memory. \
           `cinrs` and `clang` take the same time because they share a back end, and LLVM's \
           SLP vectoriser packs the pair of `double`s into one vector and turns the periodic \
           reset of it into a branchless blend on the loop-carried chain, about eight cycles \
           an iteration, where `gcc` keeps two scalars and a predicted branch, about two and \
           a half. `clang -fno-slp-vectorize` is within a quarter of `gcc`. That is a \
           back-end heuristic, and no expansion can reach it.\n\
         * **`_Complex` is `num_complex::Complex`.** `complexmandel` multiplies complex numbers \
           in the inner loop, and C's complex multiplication is not four multiplies and two \
           adds: Annex G.5.1 requires an infinity-recovery path. What the row measures is what \
           that path costs when it is never taken. As in `gcc` and `clang`, that is a NaN test: \
           the product is inline and the recovery is a cold function it never calls. Inlined, \
           the recovery made LLVM pack the real and imaginary parts into one vector and put \
           the shuffles on the loop-carried chain, and the row was 1.47× `gcc`.\n\
         * **The C library is the C library.** `libc-str`, `chase` and `pidigits` are controls \
           — a `strlen` call, a dependent load, and a program whose work is all inside GMP. \
           They should be the same in every column, and are.\n"
    )?;

    writeln!(
        out,
        "### What is *not* being measured\n\n\
         * **Parallelism.** Every program here is single-threaded, and the Benchmarks Game \
           versions were chosen to be. `#pragma omp` is ignored by `cinrs`, so the native \
           builds are compiled without `-fopenmp` and the comparison is serial against serial.\n\
         * **`-march=native`.** Neither side gets it. Both are built for the base x86-64 \
           target, so neither back end may use AVX-512 unless it can prove it is there.\n\
         * **Link-time optimisation.** Neither side gets that either. Each program is one \
           translation unit and one crate, so there is nothing across units to optimise.\n\
         * **Long-running programs.** Everything here is under two seconds, which is enough for \
           the median of five runs to be stable to about a millisecond but not enough to say \
           anything about a program whose working set grows for an hour.\n"
    )?;

    Ok(())
}

/// The line count of a program's C source, filled in by `main` before the
/// report is written.
fn line_count(r: &Result1) -> usize {
    LINE_COUNTS.with(|m| m.borrow().get(r.name).copied().unwrap_or(0))
}

thread_local! {
    static LINE_COUNTS: std::cell::RefCell<BTreeMap<&'static str, usize>> =
        const { std::cell::RefCell::new(BTreeMap::new()) };
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let o = match Options::parse() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("cinrs-bench: {e}");
            std::process::exit(2);
        }
    };

    let root = workspace_root();
    let programs = root.join("benches/cinrs-bench/programs");
    let out = root.join("target/cinrs-bench");
    let data = out.join("data");
    let paths = Paths {
        fasta: data.join(format!("fasta{FASTA_N}.txt")),
        stdin_text: data.join("stdin.txt"),
        root: root.clone(),
        programs,
        out: out.clone(),
        data: data.clone(),
    };

    let selected: Vec<&Program> = PROGRAMS
        .iter()
        .filter(|p| o.filter.as_deref().is_none_or(|f| p.name.contains(f)))
        .collect();

    if o.list {
        for p in &selected {
            println!(
                "{:<18} {:<40} {:<7} {}",
                p.name,
                p.source,
                p.dialect.macro_name(),
                p.args.join(" ")
            );
        }
        return;
    }
    if selected.is_empty() {
        eprintln!("cinrs-bench: no program matches the filter");
        std::process::exit(2);
    }

    if let Err(e) = std::fs::create_dir_all(&paths.data) {
        eprintln!("cinrs-bench: {}: {e}", paths.data.display());
        std::process::exit(1);
    }

    match inherited_address_space_limit() {
        Some(kb) => eprintln!(
            "address-space ceiling: {:.1} GiB, inherited — timed runs are started directly",
            kb as f64 / 1024.0 / 1024.0
        ),
        None => eprintln!(
            "warning: this process has no address-space ceiling. Every timed run will be \
             started through `sh -c 'ulimit -v …'`, which adds a couple of milliseconds to \
             every measurement. Wrap the harness in `( ulimit -v 8000000; … )` instead."
        ),
    }

    if let Err(e) = ensure_cinrs_built(&root, &o) {
        eprintln!("cinrs-bench: {e}");
        std::process::exit(1);
    }

    // The one literal standard input any row asks for.
    let literal_stdin = selected.iter().find_map(|p| match p.stdin {
        StdinSource::Text(t) => Some(t),
        _ => None,
    });
    if let Some(text) = literal_stdin
        && let Err(e) = std::fs::write(&paths.stdin_text, text)
    {
        eprintln!("cinrs-bench: {}: {e}", paths.stdin_text.display());
        std::process::exit(1);
    }
    if selected.iter().any(|p| p.stdin == StdinSource::Fasta)
        && let Err(e) = ensure_fasta(&paths, &o)
    {
        eprintln!("cinrs-bench: {e}");
        std::process::exit(1);
    }

    let machine = describe_machine();
    let started = Instant::now();
    let mut results = Vec::new();
    let mut failed = Vec::new();

    for (i, p) in selected.iter().enumerate() {
        eprintln!(
            "[{}/{}] {} ({} elapsed)",
            i + 1,
            selected.len(),
            p.name,
            fmt_elapsed(started.elapsed())
        );
        if let Ok(text) = std::fs::read_to_string(paths.programs.join(p.source)) {
            LINE_COUNTS.with(|m| m.borrow_mut().insert(p.name, text.lines().count()));
        }
        match bench_one(p, &o, &paths) {
            Ok(r) => {
                if let Some(what) = &r.mismatch {
                    eprintln!("  !! OUTPUT MISMATCH: {what}");
                }
                results.push(r);
            }
            Err(e) => {
                eprintln!("  !! {}: {e}", p.name);
                failed.push((p.name, e));
                if !o.keep_going {
                    eprintln!(
                        "cinrs-bench: stopping; pass --keep-going to carry on past a build failure"
                    );
                    break;
                }
            }
        }
    }

    eprintln!("\ndone in {}", fmt_elapsed(started.elapsed()));

    // Measured last, so it is not competing with anything that was timed.
    let baseline = rustc_baseline(&root, &paths.out, &o);

    let mut stdout = std::io::stdout().lock();
    if let Err(e) = write_report(&mut stdout, &results, &o, &machine, baseline) {
        eprintln!("cinrs-bench: writing the report: {e}");
    }
    if !failed.is_empty() {
        println!("\n## Programs that did not build\n");
        for (name, why) in &failed {
            println!("* `{name}` — {why}\n");
        }
    }
    drop(stdout);

    if let Some(path) = &o.markdown {
        match std::fs::File::create(path) {
            Ok(mut f) => {
                if let Err(e) = write_report(&mut f, &results, &o, &machine, baseline) {
                    eprintln!("cinrs-bench: {}: {e}", path.display());
                } else {
                    let _ = f.flush();
                    eprintln!("wrote {}", path.display());
                }
            }
            Err(e) => eprintln!("cinrs-bench: {}: {e}", path.display()),
        }
    }

    let mismatches = results.iter().filter(|r| r.mismatch.is_some()).count();
    if mismatches > 0 {
        eprintln!("cinrs-bench: {mismatches} program(s) did not agree across builds");
        std::process::exit(1);
    }
    if !failed.is_empty() {
        std::process::exit(1);
    }
}

fn fmt_elapsed(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}m{:02}s", s / 60, s % 60)
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row names a file that is there, and no two rows share a name.
    #[test]
    fn the_table_is_consistent() {
        let programs = workspace_root().join("benches/cinrs-bench/programs");
        let mut seen = std::collections::BTreeSet::new();
        for p in PROGRAMS {
            assert!(seen.insert(p.name), "two rows are called {:?}", p.name);
            let path = programs.join(p.source);
            assert!(
                path.exists(),
                "{} names {} which is missing",
                p.name,
                path.display()
            );
        }
    }

    /// The generator wraps the C the way `tests/c_testsuite.rs` does.
    #[test]
    fn the_generator_wraps_the_c() {
        let p = &PROGRAMS[0];
        let rs = generate_rust(p, "int main(void) { return 0; }\n", Path::new("/tmp"));
        // A Rust `mod` is what gives the unit's `main` a path of its own, and
        // nothing is added to the C but the include path and the `#define`s.
        assert!(rs.contains("mod bench {"), "{rs}");
        assert!(rs.contains("cinrs::gnu11! { r#\""), "{rs}");
        assert!(!rs.contains("#pragma cinrs module"), "{rs}");
        assert!(rs.contains("bench::main("), "{rs}");
    }

    /// A raw string literal that would end early gets more hashes.
    #[test]
    fn raw_strings_are_long_enough() {
        assert_eq!(raw_string_hashes("plain"), 1);
        assert_eq!(raw_string_hashes("a \"# b"), 2);
        assert_eq!(raw_string_hashes("a \"## b"), 3);
    }

    /// The output filter drops the lines a row names and keeps the rest.
    #[test]
    fn the_output_filter_drops_named_lines() {
        let got = filter_output(
            b"keep\nDhrystones per Second: 5\nkeep2\n",
            &["Dhrystones per"],
        );
        assert_eq!(got, b"keep\nkeep2\n");
    }

    /// Smoke test: the harness really can build and run one small kernel with
    /// the platform's C compiler. Skipped when there is no `cc`, and capped
    /// like everything else the harness starts.
    #[test]
    fn a_small_kernel_builds_and_runs_natively() {
        if Command::new("gcc")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            eprintln!("skipping: no gcc on the PATH");
            return;
        }
        let root = workspace_root();
        let out = root.join("target/cinrs-bench-test");
        std::fs::create_dir_all(&out).expect("the output directory");
        let o = Options {
            filter: None,
            runs: 1,
            compilers: vec![NATIVES[0]],
            timeout: 60,
            build_timeout: 120,
            mem_limit_kb: 4_000_000,
            markdown: None,
            list: false,
            keep_going: false,
            limit_is_inherited: false,
        };
        let p = PROGRAMS
            .iter()
            .find(|p| p.name == "prng")
            .expect("the table has a `prng` row");
        let src = root.join("benches/cinrs-bench/programs").join(p.source);
        let build = build_native(p, NATIVES[0], &src, &out, &o).expect("gcc builds the kernel");

        let cmd = shell_limited(
            &build.exe.display().to_string(),
            &[OsString::from("1000")],
            o.mem_limit_kb,
        );
        let run =
            run_child(cmd, Stdio::null(), Stdio::piped(), o.timeout).expect("the kernel runs");
        assert_eq!(
            run.status,
            Some(0),
            "the kernel exited with {:?}",
            run.status
        );
        let _ = std::fs::remove_file(&build.exe);
    }
}
