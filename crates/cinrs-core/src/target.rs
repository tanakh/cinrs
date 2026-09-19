//! The target data model, and the triples it is derived from.
//!
//! C's arithmetic is defined in terms of the widths and the signedness of the
//! implementation's types: whether `-1 < 1u` holds depends on how wide `int`
//! is, `unsigned char a = 200, b = 100; a + b` is 300 rather than 44 only
//! because `int` is wider than `char`, and `'\xff'` is `-1` exactly when plain
//! `char` is signed. Sema therefore needs a concrete model of the machine
//! before it can type a single expression.
//!
//! # Where the model comes from
//!
//! A procedural macro cannot ask `rustc` what it is compiling for: `--target`
//! is not part of a macro's world, and `CARGO_CFG_TARGET_*` belongs to build
//! scripts. So the model is chosen, in this order:
//!
//! 1. `#pragma cinrs target "<triple>"`, written in the unit itself, which
//!    wins over everything;
//! 2. the **`CINRS_TARGET`** environment variable, which a crate sets from its
//!    own build script —
//!    `println!("cargo:rustc-env=CINRS_TARGET={}", std::env::var("TARGET").unwrap());`
//!    — because `cargo:rustc-env` reaches the very `rustc` process that runs
//!    the macro;
//! 3. otherwise the machine this crate was compiled for, the *host*.
//!
//! [`TargetSource`] records which of the three it was, so that a diagnostic —
//! above all the data-model assertion, which is what a wrong choice trips —
//! can say which knob to turn.
//!
//! # The check that guards it
//!
//! Whichever way the model was chosen it may still be the wrong one: a cross
//! build with no `CINRS_TARGET`, or one with the wrong triple in it. So every
//! expansion **states the model it was translated for**, as a
//! `const _: () = { assert!(…); };` block at the top of the unit's module: one
//! assertion per width, over the `core::ffi` aliases, which follow the real
//! target. See `codegen`'s `data_model_check`. A mismatch is therefore a
//! failed compile-time assertion with the caret on the C, naming both the
//! model cinrs used and how to change it, rather than a program that quietly
//! computes the wrong thing.
//!
//! # The table
//!
//! [`TargetModel::from_triple`] is a table of architecture families crossed
//! with operating-system families; `doc/c-status.md` prints it. Nothing in it
//! is guesswork: it comes from the ABI document each architecture is defined
//! by and from `core::ffi`'s own `cfg` cascade — because the generated code
//! uses those aliases, a model that disagreed with them would fail its own
//! assertion. In particular **the signedness of plain `char` follows
//! `core::ffi::c_char`**, which is not the same as following the architecture:
//! Windows and Apple's platforms make it signed whatever the machine is.
//!
//! A triple the table does not have is an error rather than a guess.

use std::fmt;

/// The architecture family a triple names.
///
/// Only the ones [`TargetModel::from_triple`] accepts are here. The variant
/// decides the `__x86_64__`-style predefined macros, whether `__int128`
/// exists, and — for Arm — the signedness of `wchar_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arch {
    /// 32-bit x86: `i386`, `i486`, `i586`, `i686`.
    X86,
    /// x86-64, the x32 ABI included.
    X86_64,
    /// 64-bit Arm.
    Aarch64,
    /// 32-bit Arm, `thumb*` included.
    Arm,
    /// 32-bit RISC-V.
    Riscv32,
    /// 64-bit RISC-V.
    Riscv64,
    /// 32-bit WebAssembly.
    Wasm32,
    /// 32-bit PowerPC.
    PowerPc,
    /// 64-bit PowerPC, big and little endian.
    PowerPc64,
    /// IBM z/Architecture.
    S390x,
    /// 32-bit MIPS.
    Mips,
    /// 64-bit MIPS, the n64 ABI.
    Mips64,
    /// 32-bit SPARC.
    Sparc,
    /// 64-bit SPARC.
    Sparc64,
    /// 64-bit LoongArch.
    LoongArch64,
}

impl Arch {
    /// The name used in diagnostics and in the documentation table.
    pub fn as_str(self) -> &'static str {
        match self {
            Arch::X86 => "x86",
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
            Arch::Arm => "arm",
            Arch::Riscv32 => "riscv32",
            Arch::Riscv64 => "riscv64",
            Arch::Wasm32 => "wasm32",
            Arch::PowerPc => "powerpc",
            Arch::PowerPc64 => "powerpc64",
            Arch::S390x => "s390x",
            Arch::Mips => "mips",
            Arch::Mips64 => "mips64",
            Arch::Sparc => "sparc",
            Arch::Sparc64 => "sparc64",
            Arch::LoongArch64 => "loongarch64",
        }
    }

    /// The `__x86_64__`-style macros this architecture predefines, each with
    /// the value it is given.
    ///
    /// Deliberately short. A program that tests for something not here sees a
    /// `0` in an `#if`, which is what C written for an unfamiliar compiler
    /// expects; a macro claimed wrongly sends it down a path built on an
    /// extension this crate does not have.
    pub fn macros(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Arch::X86 => &[("__i386__", "1"), ("__i386", "1")],
            Arch::X86_64 => &[
                ("__x86_64__", "1"),
                ("__x86_64", "1"),
                ("__amd64__", "1"),
                ("__amd64", "1"),
            ],
            Arch::Aarch64 => &[("__aarch64__", "1")],
            Arch::Arm => &[("__arm__", "1")],
            Arch::Riscv32 => &[("__riscv", "1"), ("__riscv_xlen", "32")],
            Arch::Riscv64 => &[("__riscv", "1"), ("__riscv_xlen", "64")],
            Arch::Wasm32 => &[
                ("__wasm", "1"),
                ("__wasm__", "1"),
                ("__wasm32", "1"),
                ("__wasm32__", "1"),
            ],
            Arch::PowerPc => &[("__powerpc__", "1"), ("__PPC__", "1")],
            Arch::PowerPc64 => &[
                ("__powerpc__", "1"),
                ("__powerpc64__", "1"),
                ("__PPC__", "1"),
                ("__PPC64__", "1"),
            ],
            Arch::S390x => &[("__s390__", "1"), ("__s390x__", "1")],
            Arch::Mips => &[("__mips__", "1"), ("__mips", "32")],
            Arch::Mips64 => &[("__mips__", "1"), ("__mips", "64")],
            Arch::Sparc => &[("__sparc__", "1"), ("__sparc", "1")],
            Arch::Sparc64 => &[
                ("__sparc__", "1"),
                ("__sparc", "1"),
                ("__sparc64__", "1"),
                ("__arch64__", "1"),
            ],
            Arch::LoongArch64 => &[("__loongarch__", "1"), ("__loongarch64", "1")],
        }
    }
}

/// The operating-system family a triple names.
///
/// This is what the bundled headers branch on — `errno`, the standard streams,
/// `mbstate_t`, `struct tm`, `time_t`, `FILE` — through the `_WIN32` and
/// `__APPLE__` macros the model predefines. A family whose library layout
/// differs therefore has to be a variant here rather than a guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Os {
    /// Linux, whatever the C library: `gnu`, `musl`, `uclibc`, `android`.
    Linux,
    /// Apple's platforms: macOS, iOS, tvOS, watchOS, visionOS.
    Darwin,
    /// Windows, `msvc` and `gnu` alike.
    Windows,
    /// FreeBSD.
    FreeBsd,
    /// NetBSD.
    NetBsd,
    /// OpenBSD.
    OpenBsd,
    /// WASI.
    Wasi,
    /// No operating system at all: a `-none` triple, and
    /// `wasm32-unknown-unknown`.
    None,
}

impl Os {
    /// The name used in diagnostics and in the documentation table.
    pub fn as_str(self) -> &'static str {
        match self {
            Os::Linux => "linux",
            Os::Darwin => "darwin",
            Os::Windows => "windows",
            Os::FreeBsd => "freebsd",
            Os::NetBsd => "netbsd",
            Os::OpenBsd => "openbsd",
            Os::Wasi => "wasi",
            Os::None => "none",
        }
    }

    /// The `__linux__`-style macros this operating system predefines.
    ///
    /// `_WIN64` is not here: it follows the pointer width rather than the
    /// system, so [`TargetModel::macros`] adds it.
    pub fn macros(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Os::Linux => &[
                ("__linux__", "1"),
                ("__linux", "1"),
                ("__gnu_linux__", "1"),
                ("__unix__", "1"),
                ("__unix", "1"),
            ],
            Os::Darwin => &[
                ("__APPLE__", "1"),
                ("__MACH__", "1"),
                ("__unix__", "1"),
                ("__unix", "1"),
            ],
            Os::Windows => &[("_WIN32", "1")],
            Os::FreeBsd => &[("__FreeBSD__", "1"), ("__unix__", "1"), ("__unix", "1")],
            Os::NetBsd => &[("__NetBSD__", "1"), ("__unix__", "1"), ("__unix", "1")],
            Os::OpenBsd => &[("__OpenBSD__", "1"), ("__unix__", "1"), ("__unix", "1")],
            Os::Wasi => &[("__wasi__", "1")],
            Os::None => &[],
        }
    }

    /// Whether the object format is ELF, which is what `__ELF__` says.
    fn is_elf(self) -> bool {
        matches!(
            self,
            Os::Linux | Os::FreeBsd | Os::NetBsd | Os::OpenBsd | Os::None
        )
    }
}

/// The C library a triple's environment component names.
///
/// On most systems the [operating system](Os) settles the library — Apple has
/// libSystem, the BSDs each have their own — and this is [`Env::None`]. Linux
/// is the exception: `-gnu`, `-musl` and `-android` are three libraries with
/// three sets of layouts behind the same system macros, and a header that laid
/// a `mtx_t` out for the wrong one would corrupt memory. So the environment is
/// kept, and [`TargetModel::macros`] turns the two this crate models into
/// `__cinrs_glibc__` and `__cinrs_musl__` for the bundled headers to branch on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Env {
    /// The GNU C Library, `-gnu*`: `gnu`, `gnueabihf`, `gnux32`, `gnullvm`.
    Gnu,
    /// musl, `-musl*`.
    Musl,
    /// Android's bionic, which a triple names `-android` or `-androideabi`.
    Bionic,
    /// uClibc, `-uclibc*`.
    Uclibc,
    /// The Microsoft runtime, `-msvc`.
    Msvc,
    /// The triple says nothing, because the system has only one C library
    /// (Apple, the BSDs, WASI) or because there is none at all.
    #[default]
    None,
}

impl Env {
    /// The name used in diagnostics.
    pub fn as_str(self) -> &'static str {
        match self {
            Env::Gnu => "gnu",
            Env::Musl => "musl",
            Env::Bionic => "android",
            Env::Uclibc => "uclibc",
            Env::Msvc => "msvc",
            Env::None => "",
        }
    }

    /// The environment a triple's last component names.
    ///
    /// A Linux triple that names none — `x86_64-unknown-linux` — is glibc,
    /// which is what `rustc` and `gcc` both take it for.
    fn from_component(component: &str, os: Os) -> Self {
        if component.starts_with("gnu") {
            Env::Gnu
        } else if component.starts_with("musl") {
            Env::Musl
        } else if component.starts_with("android") {
            Env::Bionic
        } else if component.starts_with("uclibc") {
            Env::Uclibc
        } else if component == "msvc" {
            Env::Msvc
        } else if os == Os::Linux {
            Env::Gnu
        } else {
            Env::None
        }
    }
}

/// Where the [`TargetModel`] of an expansion came from.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum TargetSource {
    /// The machine the procedural macro itself was compiled for, because
    /// nothing said otherwise.
    #[default]
    Host,
    /// The `CINRS_TARGET` environment variable, holding this triple.
    Env(String),
    /// `#pragma cinrs target "…"`, holding this triple.
    Pragma(String),
    /// A [`crate::Options::target`] the caller set — a test, or a program
    /// driving the front end directly.
    Explicit,
}

impl TargetSource {
    /// The triple this source named, where it named one.
    pub fn triple(&self) -> Option<&str> {
        match self {
            TargetSource::Env(t) | TargetSource::Pragma(t) => Some(t),
            TargetSource::Host | TargetSource::Explicit => None,
        }
    }

    /// Where the model came from, as a diagnostic names it.
    pub fn as_str(&self) -> &'static str {
        match self {
            TargetSource::Host => "the host",
            TargetSource::Env(_) => "CINRS_TARGET",
            TargetSource::Pragma(_) => "#pragma cinrs target",
            TargetSource::Explicit => "the options given to the front end",
        }
    }

    /// How the data-model assertion describes where the model came from.
    ///
    /// Reads after "cinrs translated this unit for …".
    pub fn describe(&self) -> String {
        match self {
            TargetSource::Host => "the host, CINRS_TARGET being unset".to_owned(),
            TargetSource::Env(t) => format!("CINRS_TARGET={t}"),
            TargetSource::Pragma(t) => format!("#pragma cinrs target \"{t}\""),
            TargetSource::Explicit => "the options given to the front end".to_owned(),
        }
    }
}

/// Why a triple could not be turned into a [`TargetModel`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownTarget {
    /// The triple as it was written.
    pub triple: String,
    /// What about it was not recognised, as a sentence.
    detail: String,
}

impl UnknownTarget {
    /// The diagnostic for a triple that came from `source`.
    ///
    /// The knob that named the triple leads, so that the reader knows what to
    /// change before reading why; the fix follows the reason, and the list of
    /// families comes last.
    pub fn message(&self, source: &TargetSource) -> String {
        let (from, fix) = match source {
            TargetSource::Env(_) => (
                "CINRS_TARGET: ",
                "; set #pragma cinrs target or unset the variable",
            ),
            TargetSource::Pragma(_) => ("#pragma cinrs target: ", ""),
            TargetSource::Host | TargetSource::Explicit => ("", ""),
        };
        format!("{from}{}{fix}; {FAMILIES}", self.detail)
    }
}

impl fmt::Display for UnknownTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}; {FAMILIES}", self.detail)
    }
}

/// One row of [`ARCHITECTURES`].
struct ArchRow {
    /// Matched against the start of the triple's architecture component.
    prefix: &'static str,
    arch: Arch,
    /// The natural pointer width, which an ABI in the environment component
    /// (`gnux32`, `gnu_ilp32`) may narrow.
    ptr_bits: u32,
    big_endian: bool,
}

impl ArchRow {
    const fn new(prefix: &'static str, arch: Arch, ptr_bits: u32) -> Self {
        Self {
            prefix,
            arch,
            ptr_bits,
            big_endian: false,
        }
    }

    const fn big_endian(mut self) -> Self {
        self.big_endian = true;
        self
    }
}

/// The architecture families, as [`TargetModel::from_triple`] matches them.
///
/// The first row whose `prefix` starts the architecture component wins, so a
/// longer spelling of the same family — `powerpc64le` before `powerpc64`
/// before `powerpc` — has to come first.
const ARCHITECTURES: &[ArchRow] = &[
    ArchRow::new("x86_64", Arch::X86_64, 64),
    ArchRow::new("i386", Arch::X86, 32),
    ArchRow::new("i486", Arch::X86, 32),
    ArchRow::new("i586", Arch::X86, 32),
    ArchRow::new("i686", Arch::X86, 32),
    ArchRow::new("aarch64_be", Arch::Aarch64, 64).big_endian(),
    ArchRow::new("aarch64", Arch::Aarch64, 64),
    ArchRow::new("arm64", Arch::Aarch64, 64),
    ArchRow::new("armeb", Arch::Arm, 32).big_endian(),
    ArchRow::new("arm", Arch::Arm, 32),
    ArchRow::new("thumb", Arch::Arm, 32),
    ArchRow::new("riscv32", Arch::Riscv32, 32),
    ArchRow::new("riscv64", Arch::Riscv64, 64),
    ArchRow::new("wasm32", Arch::Wasm32, 32),
    ArchRow::new("powerpc64le", Arch::PowerPc64, 64),
    ArchRow::new("powerpc64", Arch::PowerPc64, 64).big_endian(),
    ArchRow::new("powerpc", Arch::PowerPc, 32).big_endian(),
    ArchRow::new("s390x", Arch::S390x, 64).big_endian(),
    ArchRow::new("loongarch64", Arch::LoongArch64, 64),
    ArchRow::new("mipsisa64r6el", Arch::Mips64, 64),
    ArchRow::new("mipsisa64r6", Arch::Mips64, 64).big_endian(),
    ArchRow::new("mipsisa32r6el", Arch::Mips, 32),
    ArchRow::new("mipsisa32r6", Arch::Mips, 32).big_endian(),
    ArchRow::new("mips64el", Arch::Mips64, 64),
    ArchRow::new("mips64", Arch::Mips64, 64).big_endian(),
    ArchRow::new("mipsel", Arch::Mips, 32),
    ArchRow::new("mips", Arch::Mips, 32).big_endian(),
    ArchRow::new("sparc64", Arch::Sparc64, 64).big_endian(),
    ArchRow::new("sparcv9", Arch::Sparc64, 64).big_endian(),
    ArchRow::new("sparc", Arch::Sparc, 32).big_endian(),
];

/// The architectures that are recognised only in order to be refused by name,
/// with the reason. Each has a data model this crate does not implement.
const EXOTIC: &[(&str, &str)] = &[
    ("avr", "'int' is 16 bits and 'double' is 32"),
    ("msp430", "'int' is 16 bits"),
    ("xtensa", "cinrs has no model for it"),
    ("hexagon", "cinrs has no model for it"),
    ("csky", "cinrs has no model for it"),
    ("m68k", "cinrs has no model for it"),
    ("nvptx64", "cinrs has no model for it"),
    ("bpfel", "cinrs has no model for it"),
    ("bpfeb", "cinrs has no model for it"),
    ("wasm64", "cinrs has no model for it"),
];

/// The operating systems, compared whole against every component of the triple
/// after the architecture.
const OPERATING_SYSTEMS: &[(&str, Os)] = &[
    ("linux", Os::Linux),
    ("android", Os::Linux),
    ("androideabi", Os::Linux),
    ("darwin", Os::Darwin),
    ("macos", Os::Darwin),
    ("macosx", Os::Darwin),
    ("ios", Os::Darwin),
    ("tvos", Os::Darwin),
    ("watchos", Os::Darwin),
    ("visionos", Os::Darwin),
    ("windows", Os::Windows),
    ("freebsd", Os::FreeBsd),
    ("netbsd", Os::NetBsd),
    ("openbsd", Os::OpenBsd),
    ("wasi", Os::Wasi),
    ("wasip1", Os::Wasi),
    ("wasip2", Os::Wasi),
    ("none", Os::None),
    ("elf", Os::None),
];

/// The list every "unknown triple" diagnostic ends with.
const FAMILIES: &str = "the architectures cinrs models are x86, x86_64, aarch64, arm/thumb, \
                        riscv32, riscv64, wasm32, powerpc, powerpc64, s390x, mips, mips64, \
                        sparc, sparc64 and loongarch64, on linux (android included), darwin, \
                        windows, freebsd, netbsd, openbsd, wasi or none";

/// Widths, signedness, endianness and identity of the machine the generated
/// code runs on.
///
/// Everything the front end computes at expansion time — `sizeof`, `_Alignof`,
/// member offsets, bit-field storage, the type of an integer constant, the
/// value of an `#if`, the predefined macros and therefore the branch each
/// bundled header takes — comes from here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetModel {
    /// The architecture family, which decides the `__x86_64__`-style macros.
    pub arch: Arch,
    /// The operating system, which decides the `__linux__`-style macros and so
    /// the branch every bundled header takes.
    pub os: Os,
    /// The C library the triple's environment component names, which on Linux
    /// is the difference between glibc's layouts and musl's; see [`Env`].
    pub env: Env,
    /// Whether plain `char` is a signed type.
    ///
    /// This follows `core::ffi::c_char` exactly, because the generated code
    /// uses that alias: unsigned on AArch64, Arm, PowerPC, RISC-V and s390x —
    /// *except* on Windows and on Apple's platforms, which make it signed
    /// whatever the architecture — and signed everywhere else, LoongArch and
    /// wasm32 included.
    pub char_signed: bool,
    /// Width of `short`.
    pub short_bits: u32,
    /// Width of `int`.
    pub int_bits: u32,
    /// Width of `long`: 64 on a 64-bit system that is not Windows (LP64), 32
    /// otherwise (LLP64 and ILP32).
    pub long_bits: u32,
    /// Width of `long long`.
    pub long_long_bits: u32,
    /// Width of a pointer; also the width `size_t` and `ptrdiff_t` follow.
    pub ptr_bits: u32,
    /// The strictest alignment any scalar but `__int128` gets, **in bytes**.
    ///
    /// 4 on 32-bit x86 outside Windows and 8 everywhere else: the one place
    /// where two targets of the same *data model* lay a `struct` out
    /// differently, since the i386 System V ABI aligns `long long` and
    /// `double` to four bytes and the Microsoft one to eight. `rustc` splits
    /// the same way — `align_of::<u64>()` really is 4 on
    /// `i686-unknown-linux-gnu` and 8 on `i686-pc-windows-msvc` — so a
    /// `#[repr(C)]` item laid out with this is the one the compiler will
    /// build.
    pub max_scalar_align: u64,
    /// Alignment of `__int128`, **in bytes**.
    ///
    /// The width is not a knob: GCC's `__int128` is 128 bits wherever it
    /// exists at all. The alignment is, and it is the one place where the
    /// generated Rust could disagree with the model, `__int128` becoming
    /// `i128`, whose ABI Rust settled in 1.77.
    pub int128_align: u64,
    /// Whether `__int128` exists at all.
    ///
    /// GCC has it on the 64-bit architectures only and refuses it on a 32-bit
    /// one rather than emulating it; so does this.
    pub has_int128: bool,
    /// Whether the byte order is big-endian.
    ///
    /// Only bit-fields can tell, and cinrs allocates them from the
    /// least significant end, so a big-endian target refuses a bit-field
    /// rather than laying it out the wrong way round.
    pub big_endian: bool,
    /// Width of `wchar_t`: 16 on Windows, 32 everywhere else.
    pub wchar_bits: u32,
    /// Whether `wchar_t` is signed: `int` on most systems, `unsigned int` on
    /// Arm and AArch64 outside Apple's platforms, `unsigned short` on Windows.
    pub wchar_signed: bool,
    /// Width of `wint_t`: 16 on Windows, 32 everywhere else.
    pub wint_bits: u32,
    /// Whether `wint_t` is signed, which it is only on Apple's platforms,
    /// where it is `int`.
    pub wint_signed: bool,
}

impl TargetModel {
    /// The LP64 model 64-bit Unix uses, as `x86_64-unknown-linux-gnu` has it:
    /// 32-bit `int`, 64-bit `long` and pointers, signed `char`.
    pub const LP64: Self = Self {
        arch: Arch::X86_64,
        os: Os::Linux,
        env: Env::Gnu,
        char_signed: true,
        short_bits: 16,
        int_bits: 32,
        long_bits: 64,
        long_long_bits: 64,
        ptr_bits: 64,
        max_scalar_align: 8,
        int128_align: 16,
        has_int128: true,
        big_endian: false,
        wchar_bits: 32,
        wchar_signed: true,
        wint_bits: 32,
        wint_signed: false,
    };

    /// The ILP32 model 32-bit systems use, as `i686-unknown-linux-gnu` has it:
    /// 32-bit `int`, `long` and pointers, `long long` and `double` aligned to
    /// four bytes, and no `__int128`.
    pub const ILP32: Self = Self {
        arch: Arch::X86,
        long_bits: 32,
        ptr_bits: 32,
        max_scalar_align: 4,
        has_int128: false,
        ..Self::LP64
    };

    /// The LLP64 model 64-bit Windows uses: 32-bit `int` and `long`, 64-bit
    /// pointers, and a 16-bit `wchar_t`.
    pub const LLP64: Self = Self {
        os: Os::Windows,
        env: Env::Msvc,
        long_bits: 32,
        wchar_bits: 16,
        wchar_signed: false,
        wint_bits: 16,
        ..Self::LP64
    };

    /// The model of the machine this crate was compiled for.
    ///
    /// See the [module documentation](self) for when that is the right answer
    /// and what happens when it is not.
    pub const fn host() -> Self {
        // Kept in step with `core::ffi::c_char`, whose own list this is —
        // including the rule that Windows, Apple and the Vita make plain
        // `char` signed whatever the architecture. The generated code uses
        // that alias, so a disagreement here would be a failed data-model
        // assertion on the host itself.
        let char_signed = !(cfg!(any(
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "csky",
            target_arch = "hexagon",
            target_arch = "msp430",
            target_arch = "powerpc",
            target_arch = "powerpc64",
            target_arch = "riscv32",
            target_arch = "riscv64",
            target_arch = "s390x",
            target_arch = "xtensa",
        )) && !cfg!(any(windows, target_vendor = "apple", target_os = "vita")));
        let ptr_bits = if cfg!(target_pointer_width = "64") {
            64
        } else if cfg!(target_pointer_width = "32") {
            32
        } else {
            16
        };
        Self {
            arch: host_arch(),
            os: host_os(),
            env: host_env(),
            char_signed,
            short_bits: 16,
            int_bits: if cfg!(any(target_arch = "avr", target_arch = "msp430")) {
                16
            } else {
                32
            },
            // `long` is 64 bits on 64-bit Unix (LP64) and 32 everywhere else,
            // 64-bit Windows (LLP64) included.
            long_bits: if ptr_bits == 64 && !cfg!(windows) {
                64
            } else {
                32
            },
            long_long_bits: 64,
            ptr_bits,
            max_scalar_align: if cfg!(all(target_arch = "x86", not(windows))) {
                4
            } else {
                8
            },
            // Read off the compiling toolchain rather than guessed: `__int128`
            // is generated as `i128`, so the alignment the layout code works
            // with must be the one `rustc` will really give it.
            int128_align: core::mem::align_of::<i128>() as u64,
            has_int128: ptr_bits == 64,
            big_endian: cfg!(target_endian = "big"),
            wchar_bits: if cfg!(windows) { 16 } else { 32 },
            wchar_signed: !(cfg!(windows)
                || cfg!(all(
                    any(target_arch = "aarch64", target_arch = "arm"),
                    not(target_vendor = "apple")
                ))),
            wint_bits: if cfg!(windows) { 16 } else { 32 },
            wint_signed: cfg!(target_vendor = "apple"),
        }
    }

    /// The model of the machine `triple` names.
    ///
    /// The triple is a Rust one — `arch-vendor-os-env`, or `arch-os-env` where
    /// the vendor is left out — read the way `rustc` writes them: the first
    /// component is the architecture, and the operating system is whichever of
    /// the rest names one. The environment matters only where it changes the
    /// ABI: `gnux32` and `gnu_ilp32` narrow the pointer to 32 bits while
    /// leaving the architecture 64-bit.
    ///
    /// # Errors
    ///
    /// An architecture or an operating system the table does not have, and the
    /// handful recognised only to be refused because their data model is one
    /// this crate does not implement — `avr`'s 16-bit `int`, say.
    pub fn from_triple(triple: &str) -> Result<Self, UnknownTarget> {
        let unknown = |detail: String| UnknownTarget {
            triple: triple.to_owned(),
            detail,
        };
        let mut parts = triple.split('-');
        let Some(arch_name) = parts.next().filter(|a| !a.is_empty()) else {
            return Err(unknown(format!("unknown target triple '{triple}'")));
        };
        let rest: Vec<&str> = parts.collect();
        if let Some((name, why)) = EXOTIC.iter().find(|(name, _)| arch_name == *name) {
            return Err(unknown(format!(
                "the target triple '{triple}' is not supported: on '{name}' {why}"
            )));
        }
        let Some(row) = ARCHITECTURES
            .iter()
            .find(|row| arch_name.starts_with(row.prefix))
        else {
            return Err(unknown(format!(
                "unknown architecture '{arch_name}' in the target triple '{triple}'"
            )));
        };
        // The operating system is whichever component after the architecture
        // names one. `unknown` is a vendor as well as the placeholder Rust
        // uses where there is no system at all, so a triple made of nothing
        // but placeholders — `wasm32-unknown-unknown` — is freestanding.
        let os = match rest
            .iter()
            .find_map(|c| OPERATING_SYSTEMS.iter().find(|(name, _)| c == name))
        {
            Some((_, os)) => *os,
            None if rest.iter().all(|c| *c == "unknown") => Os::None,
            None => {
                let named = rest.last().copied().unwrap_or("");
                return Err(unknown(format!(
                    "unsupported operating system '{named}' in the target triple '{triple}'"
                )));
            }
        };
        let env = rest.last().copied().unwrap_or("");
        // The two ABIs that keep a 64-bit architecture and narrow the pointer.
        let ptr_bits = if env.starts_with("gnux32") || env.starts_with("gnu_ilp32") {
            32
        } else {
            row.ptr_bits
        };
        let arch = row.arch;
        let apple = os == Os::Darwin;
        let windows = os == Os::Windows;
        // `core::ffi::c_char`'s list, and its two overrides.
        let char_signed = !(matches!(
            arch,
            Arch::Aarch64
                | Arch::Arm
                | Arch::PowerPc
                | Arch::PowerPc64
                | Arch::Riscv32
                | Arch::Riscv64
                | Arch::S390x
        ) && !apple
            && !windows);
        let unsigned_wchar = matches!(arch, Arch::Aarch64 | Arch::Arm) && !apple;
        Ok(Self {
            arch,
            os,
            env: Env::from_component(env, os),
            char_signed,
            short_bits: 16,
            int_bits: 32,
            // LP64 unless Windows, which is LLP64 — plus the one oddity
            // `core::ffi` also carries, a wasm32 Linux ABI with a 64-bit
            // `long`.
            long_bits: if (ptr_bits == 64 && !windows) || (arch == Arch::Wasm32 && os == Os::Linux)
            {
                64
            } else {
                32
            },
            long_long_bits: 64,
            ptr_bits,
            max_scalar_align: if arch == Arch::X86 && !windows { 4 } else { 8 },
            int128_align: 16,
            // GCC has `__int128` on the 64-bit architectures, x32 included,
            // and refuses it on the 32-bit ones.
            has_int128: row.ptr_bits == 64,
            big_endian: row.big_endian,
            wchar_bits: if windows { 16 } else { 32 },
            wchar_signed: !windows && !unsigned_wchar,
            wint_bits: if windows { 16 } else { 32 },
            wint_signed: apple,
        })
    }

    /// Whether the C library is the Microsoft one: an `-msvc` environment on
    /// Windows.
    ///
    /// True of `*-windows-msvc` and `*-uwp-windows-msvc`, and of a host this
    /// crate was itself compiled for with `target_env = "msvc"`. **Not** true of
    /// mingw-w64 — `*-windows-gnu` and `*-windows-gnullvm` — which is Windows
    /// with its own runtime libraries in front of the system's, so a rule that
    /// holds for the Microsoft toolchain must not reach it.
    ///
    /// The one such rule is the `printf` family, which the UCRT defines inline
    /// rather than exporting; see `codegen`'s `LEGACY_STDIO`. The *data model*
    /// is the same either way, which is why nothing else here asks.
    pub fn is_msvc(&self) -> bool {
        self.os == Os::Windows && self.env == Env::Msvc
    }

    /// The name of the data model this is: `LP64`, `LLP64` or `ILP32`.
    pub fn data_model(&self) -> &'static str {
        match (self.int_bits, self.long_bits, self.ptr_bits) {
            (32, 64, 64) => "LP64",
            (32, 32, 64) => "LLP64",
            (32, 32, 32) => "ILP32",
            _ => "an unusual data model",
        }
    }

    /// One line naming the model and the knob that chose it, for the
    /// data-model assertion.
    pub fn describe(&self, source: &TargetSource) -> String {
        format!(
            "{} ({}-{}, {} 'char', {}-bit 'wchar_t'), chosen from {}",
            self.data_model(),
            self.arch.as_str(),
            self.os.as_str(),
            if self.char_signed {
                "signed"
            } else {
                "unsigned"
            },
            self.wchar_bits,
            source.describe()
        )
    }

    /// Every macro the target's *identity* predefines: the architecture, the
    /// operating system, the object format.
    ///
    /// The data-model family — `__LP64__`, `__ILP32__`, `__CHAR_UNSIGNED__`,
    /// the `__SIZEOF_*__` and `__*_MAX__` sets, `__BYTE_ORDER__` — is
    /// arithmetic rather than identity, and the preprocessor builds it from
    /// the widths above.
    pub fn macros(&self) -> Vec<(&'static str, String)> {
        let mut out: Vec<(&'static str, String)> = Vec::new();
        for (name, value) in self.arch.macros() {
            out.push((name, (*value).to_owned()));
        }
        for (name, value) in self.os.macros() {
            out.push((name, (*value).to_owned()));
        }
        if self.os == Os::Windows && self.ptr_bits == 64 {
            out.push(("_WIN64", "1".to_owned()));
        }
        // wasm is neither ELF nor anything `__ELF__` would be right about.
        if self.os.is_elf() && self.arch != Arch::Wasm32 {
            out.push(("__ELF__", "1".to_owned()));
        }
        // Which C library a *Linux* target links against, for the bundled
        // headers that have to lay one of its types out. Nothing else says it:
        // `__linux__` is true of all three, and the real `__GLIBC__` comes
        // from glibc's own `<features.h>` rather than from a compiler. These
        // two are cinrs's own, named so, and defined only where the answer is
        // known — a `-android` or `-uclibc` triple gets neither, and a header
        // that needs one then refuses rather than guessing. Elsewhere the
        // operating system settles the library, so there is nothing to say.
        if self.os == Os::Linux {
            match self.env {
                Env::Gnu => out.push(("__cinrs_glibc__", "1".to_owned())),
                Env::Musl => out.push(("__cinrs_musl__", "1".to_owned())),
                _ => {}
            }
        }
        out
    }
}

/// The C library this crate was compiled against, for [`TargetModel::host`].
const fn host_env() -> Env {
    if cfg!(target_env = "gnu") {
        Env::Gnu
    } else if cfg!(target_env = "musl") {
        Env::Musl
    } else if cfg!(target_os = "android") {
        Env::Bionic
    } else if cfg!(target_env = "uclibc") {
        Env::Uclibc
    } else if cfg!(target_env = "msvc") {
        Env::Msvc
    } else if cfg!(target_os = "linux") {
        Env::Gnu
    } else {
        Env::None
    }
}

/// The architecture this crate was compiled for, for [`TargetModel::host`].
///
/// A host whose architecture is not in the table falls back to the one whose
/// *data model* matches, since that is all the rest of the front end reads it
/// for; the identity macros are then simply absent, which is the same answer
/// an unfamiliar compiler gives.
const fn host_arch() -> Arch {
    if cfg!(target_arch = "x86_64") {
        Arch::X86_64
    } else if cfg!(target_arch = "x86") {
        Arch::X86
    } else if cfg!(target_arch = "aarch64") {
        Arch::Aarch64
    } else if cfg!(target_arch = "arm") {
        Arch::Arm
    } else if cfg!(target_arch = "riscv32") {
        Arch::Riscv32
    } else if cfg!(target_arch = "riscv64") {
        Arch::Riscv64
    } else if cfg!(target_arch = "wasm32") {
        Arch::Wasm32
    } else if cfg!(target_arch = "powerpc") {
        Arch::PowerPc
    } else if cfg!(target_arch = "powerpc64") {
        Arch::PowerPc64
    } else if cfg!(target_arch = "s390x") {
        Arch::S390x
    } else if cfg!(target_arch = "mips") {
        Arch::Mips
    } else if cfg!(target_arch = "mips64") {
        Arch::Mips64
    } else if cfg!(target_arch = "sparc") {
        Arch::Sparc
    } else if cfg!(target_arch = "sparc64") {
        Arch::Sparc64
    } else if cfg!(target_arch = "loongarch64") {
        Arch::LoongArch64
    } else if cfg!(target_pointer_width = "64") {
        Arch::X86_64
    } else {
        Arch::X86
    }
}

/// The operating system this crate was compiled for.
const fn host_os() -> Os {
    if cfg!(any(target_os = "linux", target_os = "android")) {
        Os::Linux
    } else if cfg!(target_vendor = "apple") {
        Os::Darwin
    } else if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "freebsd") {
        Os::FreeBsd
    } else if cfg!(target_os = "netbsd") {
        Os::NetBsd
    } else if cfg!(target_os = "openbsd") {
        Os::OpenBsd
    } else if cfg!(target_os = "wasi") {
        Os::Wasi
    } else {
        Os::None
    }
}

impl Default for TargetModel {
    fn default() -> Self {
        Self::host()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(triple: &str) -> TargetModel {
        TargetModel::from_triple(triple).unwrap_or_else(|e| panic!("{triple}: {e}"))
    }

    fn macros(triple: &str) -> Vec<String> {
        model(triple)
            .macros()
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect()
    }

    #[test]
    fn the_host_model_is_self_consistent() {
        let t = TargetModel::host();
        assert!(t.short_bits <= t.int_bits);
        assert!(t.int_bits <= t.long_bits);
        assert!(t.long_bits <= t.long_long_bits);
        assert!(t.long_long_bits >= 64);
    }

    /// The host model has to agree with `core::ffi`, which is what the
    /// generated code uses and what the data-model assertion checks. Every
    /// width here is asked of the toolchain rather than of the table.
    #[test]
    fn the_host_model_agrees_with_core_ffi() {
        let t = TargetModel::host();
        assert_eq!(t.short_bits, 8 * size_of::<core::ffi::c_short>() as u32);
        assert_eq!(t.int_bits, 8 * size_of::<core::ffi::c_int>() as u32);
        assert_eq!(t.long_bits, 8 * size_of::<core::ffi::c_long>() as u32);
        assert_eq!(
            t.long_long_bits,
            8 * size_of::<core::ffi::c_longlong>() as u32
        );
        assert_eq!(t.ptr_bits, 8 * size_of::<*const ()>() as u32);
        assert_eq!(t.char_signed, core::ffi::c_char::MIN != 0);
        assert_eq!(t.int128_align, align_of::<i128>() as u64);
        assert_eq!(t.max_scalar_align, align_of::<u64>() as u64);
        assert_eq!(t.max_scalar_align, align_of::<f64>() as u64);
    }

    #[test]
    fn a_64_bit_x86_linux_host_is_lp64() {
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(TargetModel::host(), TargetModel::LP64);
        }
    }

    /// The host model and the table have to say the same thing about the host,
    /// or `CINRS_TARGET` naming the host triple would change the translation.
    #[test]
    fn the_table_agrees_with_the_host() {
        if cfg!(all(
            target_os = "linux",
            target_arch = "x86_64",
            target_env = "gnu"
        )) {
            assert_eq!(
                model("x86_64-unknown-linux-gnu"),
                TargetModel::host(),
                "the table and the host disagree"
            );
        }
    }

    #[test]
    fn the_lp64_family() {
        for triple in [
            "x86_64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "x86_64-unknown-freebsd",
            "aarch64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "riscv64gc-unknown-linux-gnu",
            "powerpc64le-unknown-linux-gnu",
            "s390x-unknown-linux-gnu",
            "loongarch64-unknown-linux-gnu",
            "x86_64-unknown-none",
        ] {
            let t = model(triple);
            assert_eq!(t.data_model(), "LP64", "{triple}");
            assert_eq!(t.long_bits, 64, "{triple}");
            assert_eq!(t.ptr_bits, 64, "{triple}");
            assert!(t.has_int128, "{triple}");
        }
    }

    #[test]
    fn the_llp64_family() {
        for triple in [
            "x86_64-pc-windows-msvc",
            "x86_64-pc-windows-gnu",
            "x86_64-pc-windows-gnullvm",
            "aarch64-pc-windows-msvc",
            "x86_64-uwp-windows-msvc",
        ] {
            let t = model(triple);
            assert_eq!(t.data_model(), "LLP64", "{triple}");
            assert_eq!(t.long_bits, 32, "{triple}");
            assert_eq!(t.ptr_bits, 64, "{triple}");
            assert_eq!(t.wchar_bits, 16, "{triple}");
            assert!(!t.wchar_signed, "{triple}");
            assert_eq!(t.wint_bits, 16, "{triple}");
            // Windows makes plain `char` signed whatever the architecture,
            // AArch64 included; `core::ffi::c_char` says the same.
            assert!(t.char_signed, "{triple}");
            assert_eq!(t.max_scalar_align, 8, "{triple}");
        }
    }

    /// Which Windows targets are the *Microsoft* library, which is what decides
    /// whether a unit calling `printf` links `legacy_stdio_definitions`.
    #[test]
    fn the_msvc_environment() {
        for triple in [
            "x86_64-pc-windows-msvc",
            "i686-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
            "x86_64-uwp-windows-msvc",
            "thumbv7a-pc-windows-msvc",
        ] {
            assert!(model(triple).is_msvc(), "{triple} is MSVC");
        }
        // mingw-w64 is Windows and is not the Microsoft library; neither is
        // anything that is not Windows at all.
        for triple in [
            "x86_64-pc-windows-gnu",
            "i686-pc-windows-gnu",
            "x86_64-pc-windows-gnullvm",
            "aarch64-pc-windows-gnullvm",
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "x86_64-unknown-freebsd",
            "wasm32-unknown-unknown",
            "thumbv7em-none-eabihf",
        ] {
            assert!(!model(triple).is_msvc(), "{triple} is not MSVC");
        }
        assert!(TargetModel::LLP64.is_msvc());
        assert!(!TargetModel::LP64.is_msvc());
        assert!(!TargetModel::ILP32.is_msvc());
        // And the host, which is what an expansion with no `CINRS_TARGET` and
        // no pragma is translated for.
        assert_eq!(
            TargetModel::host().is_msvc(),
            cfg!(all(windows, target_env = "msvc"))
        );
    }

    #[test]
    fn the_ilp32_family() {
        for triple in [
            "i686-unknown-linux-gnu",
            "i586-unknown-linux-gnu",
            "i686-pc-windows-msvc",
            "armv7-unknown-linux-gnueabihf",
            "thumbv7em-none-eabihf",
            "riscv32imac-unknown-none-elf",
            "wasm32-unknown-unknown",
            "wasm32-wasip1",
            "mips-unknown-linux-gnu",
            "powerpc-unknown-linux-gnu",
            "sparc-unknown-linux-gnu",
            "x86_64-unknown-linux-gnux32",
            "aarch64-unknown-linux-gnu_ilp32",
        ] {
            let t = model(triple);
            assert_eq!(t.data_model(), "ILP32", "{triple}");
            assert_eq!(t.long_bits, 32, "{triple}");
            assert_eq!(t.ptr_bits, 32, "{triple}");
        }
    }

    /// `__int128` follows the architecture rather than the pointer: the x32
    /// ABI keeps it, the machine still being x86-64.
    #[test]
    fn int128_follows_the_architecture() {
        assert!(model("x86_64-unknown-linux-gnux32").has_int128);
        assert!(model("aarch64-unknown-linux-gnu").has_int128);
        assert!(!model("i686-unknown-linux-gnu").has_int128);
        assert!(!model("wasm32-unknown-unknown").has_int128);
        assert!(!model("armv7-unknown-linux-gnueabihf").has_int128);
    }

    /// The signedness of plain `char`, which has to be `core::ffi::c_char`'s
    /// or the generated code fails its own assertion.
    #[test]
    fn plain_char_signedness_follows_core_ffi() {
        for triple in [
            "x86_64-unknown-linux-gnu",
            "i686-unknown-linux-gnu",
            "loongarch64-unknown-linux-gnu",
            "wasm32-unknown-unknown",
            "mips-unknown-linux-gnu",
            "sparc64-unknown-netbsd",
            // Windows and Apple override the architecture's own default.
            "aarch64-pc-windows-msvc",
            "aarch64-apple-darwin",
            "armv7-apple-ios",
        ] {
            assert!(model(triple).char_signed, "{triple} should be signed");
        }
        for triple in [
            "aarch64-unknown-linux-gnu",
            "armv7-unknown-linux-gnueabihf",
            "thumbv7em-none-eabihf",
            "riscv64gc-unknown-linux-gnu",
            "riscv32imac-unknown-none-elf",
            "powerpc64le-unknown-linux-gnu",
            "powerpc-unknown-linux-gnu",
            "s390x-unknown-linux-gnu",
        ] {
            assert!(!model(triple).char_signed, "{triple} should be unsigned");
        }
    }

    /// `long long` and `double` are four-byte aligned by the i386 System V ABI
    /// and eight-byte aligned by the Microsoft one — the one place where two
    /// ILP32 targets lay a `struct` out differently.
    #[test]
    fn scalar_alignment_splits_i386_from_i386_on_windows() {
        assert_eq!(model("i686-unknown-linux-gnu").max_scalar_align, 4);
        assert_eq!(model("i586-unknown-netbsd").max_scalar_align, 4);
        assert_eq!(model("i686-pc-windows-msvc").max_scalar_align, 8);
        assert_eq!(model("i686-pc-windows-gnu").max_scalar_align, 8);
        assert_eq!(model("armv7-unknown-linux-gnueabihf").max_scalar_align, 8);
        assert_eq!(model("wasm32-unknown-unknown").max_scalar_align, 8);
    }

    #[test]
    fn endianness() {
        for triple in [
            "s390x-unknown-linux-gnu",
            "powerpc64-unknown-linux-gnu",
            "powerpc-unknown-linux-gnu",
            "sparc64-unknown-linux-gnu",
            "mips-unknown-linux-gnu",
            "aarch64_be-unknown-linux-gnu",
        ] {
            assert!(model(triple).big_endian, "{triple} is big-endian");
        }
        for triple in [
            "x86_64-unknown-linux-gnu",
            "powerpc64le-unknown-linux-gnu",
            "mipsel-unknown-linux-gnu",
            "mips64el-unknown-linux-gnuabi64",
            "aarch64-unknown-linux-gnu",
        ] {
            assert!(!model(triple).big_endian, "{triple} is little-endian");
        }
    }

    /// `wchar_t` is `unsigned int` on Arm outside Apple's platforms, `unsigned
    /// short` on Windows and `int` everywhere else; `wint_t` is `int` on
    /// Apple's, `unsigned short` on Windows and `unsigned int` elsewhere.
    #[test]
    fn wchar_and_wint() {
        let arm = model("aarch64-unknown-linux-gnu");
        assert_eq!((arm.wchar_bits, arm.wchar_signed), (32, false));
        assert_eq!((arm.wint_bits, arm.wint_signed), (32, false));

        let mac = model("aarch64-apple-darwin");
        assert_eq!((mac.wchar_bits, mac.wchar_signed), (32, true));
        assert_eq!((mac.wint_bits, mac.wint_signed), (32, true));

        let win = model("x86_64-pc-windows-msvc");
        assert_eq!((win.wchar_bits, win.wchar_signed), (16, false));
        assert_eq!((win.wint_bits, win.wint_signed), (16, false));

        let linux = model("x86_64-unknown-linux-gnu");
        assert_eq!((linux.wchar_bits, linux.wchar_signed), (32, true));
        assert_eq!((linux.wint_bits, linux.wint_signed), (32, false));
    }

    #[test]
    fn the_identity_macros() {
        let linux = macros("x86_64-unknown-linux-gnu");
        for want in [
            "__x86_64__=1",
            "__amd64__=1",
            "__linux__=1",
            "__gnu_linux__=1",
            "__unix__=1",
            "__ELF__=1",
        ] {
            assert!(linux.contains(&want.to_owned()), "{want} in {linux:?}");
        }
        assert!(!linux.iter().any(|m| m.starts_with("_WIN")));

        let win = macros("x86_64-pc-windows-msvc");
        assert!(win.contains(&"_WIN32=1".to_owned()));
        assert!(win.contains(&"_WIN64=1".to_owned()));
        assert!(!win.iter().any(|m| m.starts_with("__ELF__")));
        assert!(!win.iter().any(|m| m.starts_with("__unix")));

        let win32 = macros("i686-pc-windows-msvc");
        assert!(win32.contains(&"_WIN32=1".to_owned()));
        assert!(!win32.contains(&"_WIN64=1".to_owned()));
        assert!(win32.contains(&"__i386__=1".to_owned()));

        let mac = macros("aarch64-apple-darwin");
        assert!(mac.contains(&"__APPLE__=1".to_owned()));
        assert!(mac.contains(&"__MACH__=1".to_owned()));
        assert!(mac.contains(&"__aarch64__=1".to_owned()));
        assert!(!mac.iter().any(|m| m.starts_with("__ELF__")));

        let wasm = macros("wasm32-unknown-unknown");
        assert!(wasm.contains(&"__wasm32__=1".to_owned()));
        assert!(!wasm.iter().any(|m| m.starts_with("__ELF__")));

        let riscv = macros("riscv64gc-unknown-linux-gnu");
        assert!(riscv.contains(&"__riscv=1".to_owned()));
        assert!(riscv.contains(&"__riscv_xlen=64".to_owned()));

        let bare = macros("thumbv7em-none-eabihf");
        assert!(bare.contains(&"__arm__=1".to_owned()));
        assert!(bare.contains(&"__ELF__=1".to_owned()));
        assert!(!bare.iter().any(|m| m.starts_with("__linux")));
    }

    /// The C library a Linux triple names, which is the one thing `__linux__`
    /// does not say and the bundled `<threads.h>` has to know.
    #[test]
    fn the_c_library_of_a_linux_triple() {
        for (triple, want) in [
            ("x86_64-unknown-linux-gnu", Env::Gnu),
            ("armv7-unknown-linux-gnueabihf", Env::Gnu),
            ("x86_64-unknown-linux-gnux32", Env::Gnu),
            // A Linux triple that names no environment is glibc, which is what
            // `rustc` and `gcc` both take it for.
            ("x86_64-unknown-linux", Env::Gnu),
            ("x86_64-unknown-linux-musl", Env::Musl),
            ("aarch64-unknown-linux-musl", Env::Musl),
            ("aarch64-linux-android", Env::Bionic),
            ("armv7-unknown-linux-uclibceabi", Env::Uclibc),
            ("x86_64-pc-windows-msvc", Env::Msvc),
            // mingw is `-gnu` and is *not* glibc; the macro below is what keeps
            // the two apart, since it is defined only on Linux.
            ("x86_64-pc-windows-gnu", Env::Gnu),
            ("aarch64-apple-darwin", Env::None),
            ("x86_64-unknown-freebsd", Env::None),
            ("wasm32-unknown-unknown", Env::None),
        ] {
            assert_eq!(model(triple).env, want, "{triple}");
        }

        let glibc = "__cinrs_glibc__=1".to_owned();
        let musl = "__cinrs_musl__=1".to_owned();
        assert!(macros("x86_64-unknown-linux-gnu").contains(&glibc));
        assert!(macros("i686-unknown-linux-gnu").contains(&glibc));
        assert!(macros("x86_64-unknown-linux-musl").contains(&musl));
        // One or the other, never both, and neither where the answer is not
        // known: a `<threads.h>` that guessed would corrupt memory.
        for triple in [
            "x86_64-unknown-linux-gnu",
            "x86_64-unknown-linux-musl",
            "aarch64-linux-android",
            "x86_64-pc-windows-gnu",
            "aarch64-apple-darwin",
            "x86_64-unknown-freebsd",
            "wasm32-unknown-unknown",
        ] {
            let macros = macros(triple);
            let named = usize::from(macros.contains(&glibc)) + usize::from(macros.contains(&musl));
            assert!(named <= 1, "{triple} claims two C libraries");
        }
        for triple in [
            "aarch64-linux-android",
            "x86_64-pc-windows-gnu",
            "aarch64-apple-darwin",
        ] {
            let macros = macros(triple);
            assert!(!macros.contains(&glibc), "{triple}");
            assert!(!macros.contains(&musl), "{triple}");
        }
    }

    #[test]
    fn an_unknown_architecture_is_refused() {
        let err = TargetModel::from_triple("gizmo-unknown-linux-gnu").unwrap_err();
        let message = err.message(&TargetSource::Env("gizmo-unknown-linux-gnu".to_owned()));
        assert!(
            message.starts_with(
                "CINRS_TARGET: unknown architecture 'gizmo' in the target triple \
                 'gizmo-unknown-linux-gnu'; set #pragma cinrs target or unset the variable; "
            ),
            "{message}"
        );
        assert!(message.contains("the architectures cinrs models are x86, x86_64"));
    }

    #[test]
    fn an_unknown_operating_system_is_refused() {
        let err = TargetModel::from_triple("x86_64-unknown-plan9").unwrap_err();
        let message = err.message(&TargetSource::Pragma("x86_64-unknown-plan9".to_owned()));
        assert!(
            message.starts_with(
                "#pragma cinrs target: unsupported operating system 'plan9' in the target \
                 triple 'x86_64-unknown-plan9'; "
            ),
            "{message}"
        );
    }

    #[test]
    fn the_exotic_data_models_are_refused_by_name() {
        let err = TargetModel::from_triple("avr-none-unknown").unwrap_err();
        let message = err.message(&TargetSource::Env("avr-none-unknown".to_owned()));
        assert!(
            message.starts_with(
                "CINRS_TARGET: the target triple 'avr-none-unknown' is not supported: on \
                 'avr' 'int' is 16 bits and 'double' is 32; set #pragma cinrs target or \
                 unset the variable; "
            ),
            "{message}"
        );
        assert!(TargetModel::from_triple("msp430-none-elf").is_err());
    }

    #[test]
    fn an_empty_triple_is_refused() {
        assert!(TargetModel::from_triple("").is_err());
        assert!(TargetModel::from_triple("-linux-gnu").is_err());
    }

    /// The description the data-model assertion carries names both the model
    /// and the knob that chose it.
    #[test]
    fn the_description_names_the_source() {
        let t = model("x86_64-pc-windows-msvc");
        let said = t.describe(&TargetSource::Env("x86_64-pc-windows-msvc".to_owned()));
        assert_eq!(
            said,
            "LLP64 (x86_64-windows, signed 'char', 16-bit 'wchar_t'), chosen from \
             CINRS_TARGET=x86_64-pc-windows-msvc"
        );
        let host = TargetModel::host().describe(&TargetSource::Host);
        assert!(host.contains("CINRS_TARGET being unset"), "{host}");
    }
}
