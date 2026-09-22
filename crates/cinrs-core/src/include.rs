//! `#include` resolution: where a header is looked for, and what is bundled.
//!
//! # Search order
//!
//! `#include "name"` looks in
//!
//! 1. the directory of the file the directive is written in — for the macro's
//!    own text that is the directory of the invoking `.rs` file, and for a
//!    header it is the directory that header was found in;
//! 2. the configured include directories, in the order [`SearchPaths`]
//!    describes;
//! 3. the working directory, but only when the name *is* a path — when it
//!    holds a directory separator. That is what makes `#include __FILE__`
//!    work: [`Resolved::name`] is written relative to the working directory
//!    wherever it can be (a diagnostic naming an absolute path is a
//!    diagnostic that differs between two machines), so a header that
//!    includes itself by `__FILE__` is asking for
//!    `some/dir/thing.h` from a directive written *in* `some/dir`, which
//!    neither of the first two steps will find. A bare name is deliberately
//!    left out of this step, so that a `stdio.h` sitting in the working
//!    directory never shadows the bundled one;
//! 4. the [bundled headers](bundled);
//! 5. the platform's own directories — `/usr/include` and friends — but only
//!    when [the switch](System) is on.
//!
//! `#include <name>` skips steps 1 and 3. A name that is absolute is used as
//! it stands.
//!
//! # The platform's own directories
//!
//! Step 5 is **off by default**, and everything above it is enough for a
//! self-contained, target-model-portable unit. A real `<stdio.h>` is not
//! plain C: glibc's is a thicket of `__attribute__`, `__extension__`,
//! `__asm__` renaming and compiler builtins, and its layouts are the host's
//! rather than the [target model](crate::target)'s. So `cinrs` ships its own
//! small, plain-C99 declarations of the standard library: they declare exactly
//! what the platform's real library exports, the linker binds the calls to the
//! real implementation, and the C that uses them is ordinary C.
//!
//! **The bundled set is ISO C, plus what only the compiler can provide.**
//! Everything in [`BUNDLED`] is a header the C standard describes, with two
//! groups of exceptions: `<alloca.h>`, because `alloca` is implemented by this
//! crate rather than by any library, and the Intel intrinsics headers —
//! `<immintrin.h>`, `<xmmintrin.h>` and the rest — because `__m128i` and
//! `_mm_add_epi32` are the compiler's too. Neither has a library behind it, so
//! the platform's copy would have nothing to add and everything to break: a
//! real `<immintrin.h>` is a thicket of `__attribute__((vector_size))` and
//! `__builtin_ia32_*`, and this one is prototypes that
//! [`crate::x86`] maps onto `core::arch`.
//! POSIX is the platform's: `<unistd.h>`, `<fcntl.h>`, `<sys/types.h>`,
//! `<pthread.h>`, `<sys/stat.h>` and the rest come from the platform's own
//! directories, complete and consistent with each other, once the switch below
//! is on. (Up to 0.1.0 four small POSIX headers were bundled too; they were
//! incomplete — no `access`, no `fsync`, no `struct flock` — and a program that
//! turned the platform on still got the bundled ones, which is the opposite of
//! what it asked for.)
//!
//! What the bundled set cannot give is POSIX, and the things whose *layout*
//! only the platform knows — `struct stat`, `DIR`, `pthread_mutex_t`, the real
//! `FILE` — so a program that needs those turns the switch on with
//! `#pragma cinrs system_include` (or `CINRS_SYSTEM_INCLUDE=1` in the
//! environment, which is the crate-wide default the pragma overrides). With
//! [`System::Last`] the bundled headers still win, and only a name they do not
//! carry reaches the platform; with [`System::First`] the platform's copy of
//! every header wins, which is what makes `FILE` the real `struct _IO_FILE`.
//! The bundled ISO headers guard the types and macros a platform header would
//! also define with that platform's own guard macros, so that the two sets can
//! be mixed in [`System::Last`] mode: see the comments in `include/time.h`.
//!
//! The directories searched are [`SYSTEM_PATH_ENV_VAR`] when it is set, and
//! otherwise [`system_directories`]'s per-target default. **The compiler's own
//! private directories are never among them**: GCC's and Clang's
//! `.../include/{limits,stdint,stddef,stdarg}.h` chain to the next header of
//! the same name with `#include_next` and expect their own compiler's
//! builtins, and every one of those headers is bundled here anyway.
//!
//! Nothing found under step 5 is tracked for rebuilds: a system header is part
//! of the machine rather than of the crate, and `include_str!`-ing
//! `/usr/include/stdio.h` into the build would make every unit rebuild when the
//! libc package is upgraded, which is not what the file identifies.

use std::path::{Path, PathBuf};

use crate::target::{Arch, Env, Os, TargetModel, TargetSource};

/// The bundled standard headers, as `(name, text)` pairs.
///
/// They are compiled into the crate rather than installed anywhere, so no part
/// of a build depends on where `cinrs` itself lives on disk.
///
/// Every one of them is a header ISO C describes, with two exceptions:
/// `<alloca.h>` is here because `alloca` is generated by this crate rather than
/// called in any library, and the Intel intrinsics headers are here because
/// `_mm_add_epi32` is generated too — as a call to `core::arch`. In both cases
/// there is nothing for the platform's copy to add.
/// A POSIX header is *not* here — see the [module docs](self).
pub const BUNDLED: &[(&str, &str)] = &[
    ("alloca.h", include_str!("../include/alloca.h")),
    ("assert.h", include_str!("../include/assert.h")),
    ("avx2intrin.h", include_str!("../include/avx2intrin.h")),
    ("avxintrin.h", include_str!("../include/avxintrin.h")),
    ("complex.h", include_str!("../include/complex.h")),
    ("ctype.h", include_str!("../include/ctype.h")),
    ("emmintrin.h", include_str!("../include/emmintrin.h")),
    ("errno.h", include_str!("../include/errno.h")),
    ("float.h", include_str!("../include/float.h")),
    ("immintrin.h", include_str!("../include/immintrin.h")),
    ("inttypes.h", include_str!("../include/inttypes.h")),
    ("iso646.h", include_str!("../include/iso646.h")),
    ("limits.h", include_str!("../include/limits.h")),
    ("math.h", include_str!("../include/math.h")),
    ("mmintrin.h", include_str!("../include/mmintrin.h")),
    ("nmmintrin.h", include_str!("../include/nmmintrin.h")),
    ("pmmintrin.h", include_str!("../include/pmmintrin.h")),
    ("popcntintrin.h", include_str!("../include/popcntintrin.h")),
    ("setjmp.h", include_str!("../include/setjmp.h")),
    ("signal.h", include_str!("../include/signal.h")),
    ("smmintrin.h", include_str!("../include/smmintrin.h")),
    ("stdalign.h", include_str!("../include/stdalign.h")),
    ("stdarg.h", include_str!("../include/stdarg.h")),
    ("stdatomic.h", include_str!("../include/stdatomic.h")),
    ("stdbool.h", include_str!("../include/stdbool.h")),
    ("stdckdint.h", include_str!("../include/stdckdint.h")),
    ("stddef.h", include_str!("../include/stddef.h")),
    ("stdint.h", include_str!("../include/stdint.h")),
    ("stdio.h", include_str!("../include/stdio.h")),
    ("stdlib.h", include_str!("../include/stdlib.h")),
    ("stdnoreturn.h", include_str!("../include/stdnoreturn.h")),
    ("string.h", include_str!("../include/string.h")),
    ("threads.h", include_str!("../include/threads.h")),
    ("time.h", include_str!("../include/time.h")),
    ("tmmintrin.h", include_str!("../include/tmmintrin.h")),
    ("uchar.h", include_str!("../include/uchar.h")),
    ("wchar.h", include_str!("../include/wchar.h")),
    ("wctype.h", include_str!("../include/wctype.h")),
    ("wmmintrin.h", include_str!("../include/wmmintrin.h")),
    ("x86intrin.h", include_str!("../include/x86intrin.h")),
    ("xmmintrin.h", include_str!("../include/xmmintrin.h")),
];

/// The directory the bundled headers appear to live in.
///
/// It is not a directory at all — the headers are strings inside this crate —
/// but a diagnostic has to name the file it is talking about, and
/// `<cinrs>/stdio.h` says both which header it is and that it is ours. The
/// angle brackets keep it from being mistaken for a path that exists.
pub const BUNDLED_DIR: &str = "<cinrs>";

/// The text of a bundled header, by name.
pub fn bundled(name: &str) -> Option<&'static str> {
    BUNDLED
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| *text)
}

/// The display name a bundled header is known by: `<cinrs>/stdio.h`.
pub fn bundled_name(name: &str) -> String {
    format!("{BUNDLED_DIR}/{name}")
}

/// The POSIX headers a program is likely to reach for, none of which is
/// bundled.
///
/// The list exists for one diagnostic: a `#include <unistd.h>` with the
/// platform's directories switched off is not a missing file but a missing
/// *switch*, and saying so is the difference between a puzzle and an
/// instruction. The first four were bundled up to 0.1.0, which is why they come
/// first; the rest are the ones a search of any real code base turns up. It is
/// deliberately not the whole of POSIX — a name that is not here just gets the
/// ordinary "file not found", which is not wrong, only terser.
pub const POSIX_HEADERS: &[&str] = &[
    "unistd.h",
    "fcntl.h",
    "strings.h",
    "sys/types.h",
    "aio.h",
    "arpa/inet.h",
    "dirent.h",
    "dlfcn.h",
    "fnmatch.h",
    "glob.h",
    "grp.h",
    "langinfo.h",
    "libgen.h",
    "monetary.h",
    "net/if.h",
    "netdb.h",
    "netinet/in.h",
    "netinet/tcp.h",
    "nl_types.h",
    "poll.h",
    "pthread.h",
    "pwd.h",
    "regex.h",
    "sched.h",
    "semaphore.h",
    "spawn.h",
    "sys/ipc.h",
    "sys/mman.h",
    "sys/msg.h",
    "sys/resource.h",
    "sys/select.h",
    "sys/sem.h",
    "sys/shm.h",
    "sys/socket.h",
    "sys/stat.h",
    "sys/statvfs.h",
    "sys/time.h",
    "sys/times.h",
    "sys/uio.h",
    "sys/un.h",
    "sys/utsname.h",
    "sys/wait.h",
    "syslog.h",
    "tar.h",
    "termios.h",
    "ulimit.h",
    "utime.h",
    "utmpx.h",
    "wordexp.h",
];

/// Whether `name` is one of [`POSIX_HEADERS`].
pub fn is_posix_header(name: &str) -> bool {
    POSIX_HEADERS.contains(&name)
}

/// How the header name was spelled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// `#include "name"`, which searches the including file's directory first.
    Quoted,
    /// `#include <name>`, which does not.
    Angled,
}

/// The directory a file's own `#include "…"` searches first.
#[derive(Clone, Debug, Default)]
pub enum Origin {
    /// A directory on disk. Empty means the current directory, which is what
    /// the parent of a bare `lib.rs` comes out as.
    Dir(PathBuf),
    /// A directory on disk reached through one of the platform's own
    /// directories — the including file is a *system* header.
    ///
    /// It searches that directory first, as any other file does, and its
    /// `<…>` includes then go to the platform's directories **before** the
    /// bundled ones whatever the mode is. That is what keeps the platform's
    /// header set self-consistent: glibc's `<pthread.h>` gets glibc's
    /// `<time.h>`, and so one `struct timespec` rather than two.
    SystemDir(PathBuf),
    /// The bundled set: one bundled header including another finds it there.
    Bundled,
    /// Nowhere — the compiler would not say where the including file is.
    #[default]
    Unknown,
}

/// Whether the platform's own include directories are searched, and where in
/// the order they go.
///
/// Set by `#pragma cinrs system_include` in a unit and by
/// [`SYSTEM_ENV_VAR`] across a crate; see the [module docs](self).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum System {
    /// Not searched at all, which is where the switch starts.
    #[default]
    Off,
    /// Searched after the bundled headers: the bundled `<stdio.h>` still wins,
    /// and only a header cinrs does not carry — `<sys/stat.h>`, `<pthread.h>`
    /// — comes from the platform. `#pragma cinrs system_include`.
    Last,
    /// Searched before them, so that the platform's copy of every header wins.
    /// `#pragma cinrs system_include first`.
    First,
}

impl System {
    /// The value [`SYSTEM_ENV_VAR`] holds, read the way a build system spells
    /// a boolean: `1`, `on`, `true` and `yes` for [`System::Last`], `first`
    /// for [`System::First`], and `0`, `off`, `false`, `no` or nothing at all
    /// for [`System::Off`]. Anything else is `None`, which the caller reports.
    pub fn from_env_value(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "0" | "off" | "false" | "no" => Some(System::Off),
            "1" | "on" | "true" | "yes" => Some(System::Last),
            "first" => Some(System::First),
            _ => None,
        }
    }

    /// Whether the platform's directories are searched at all.
    pub fn is_on(self) -> bool {
        self != System::Off
    }
}

/// The environment variable that turns the platform's directories on for a
/// whole crate: `1` for [`System::Last`], `first` for [`System::First`].
///
/// A `#pragma cinrs system_include` in a unit overrides it.
pub const SYSTEM_ENV_VAR: &str = "CINRS_SYSTEM_INCLUDE";

/// The environment variable that *replaces* [`system_directories`]'s
/// per-target default, split the way the platform splits `PATH`.
///
/// This is the only way to name the directories on a target whose default
/// cinrs does not know — Apple's, whose SDK path is knowable only from
/// `xcrun --show-sdk-path`, and Windows — and the only way to point a cross
/// build at a sysroot.
pub const SYSTEM_PATH_ENV_VAR: &str = "CINRS_SYSTEM_INCLUDE_PATH";

/// One place a search looks, in the order it looks.
///
/// A [`Resolved`] records the entry it was found under so that an
/// `#include_next` written inside it can go on from the entry *after* that
/// one, which is the whole of GCC's semantics for the directive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A configured directory: `#pragma cinrs include_path`,
    /// [`crate::Options::include_paths`] or [`ENV_VAR`].
    Dir(PathBuf),
    /// The working directory, which only a quoted name that is itself a path
    /// is looked for in; see the [module docs](self).
    WorkingDir,
    /// The [bundled headers](BUNDLED).
    Bundled,
    /// One of the platform's own directories.
    System(PathBuf),
}

/// The include directories a unit searches, in the order it searches them.
///
/// The lists are kept apart so that the order is a decision rather than an
/// accident: what the unit itself asks for wins over what the build asked for,
/// which wins over what the environment asked for, which wins over what is
/// bundled — and the platform's own directories are not there at all until
/// [`SearchPaths::enable_system`] puts them there.
#[derive(Clone, Debug, Default)]
pub struct SearchPaths {
    /// Directories from `#pragma cinrs include_path`, in the order written.
    pragma: Vec<PathBuf>,
    /// Directories from [`crate::Options::include_paths`].
    options: Vec<PathBuf>,
    /// Directories from the `CINRS_INCLUDE_PATH` environment variable.
    env: Vec<PathBuf>,
    /// The platform's own directories, empty while the switch is off.
    system: Vec<PathBuf>,
    /// Where those go, and whether they are searched at all.
    mode: System,
}

/// The environment variable holding a global list of include directories.
///
/// Split the way the platform splits `PATH`: with `:` on Unix and `;` on
/// Windows, through [`std::env::split_paths`].
pub const ENV_VAR: &str = "CINRS_INCLUDE_PATH";

/// The environment variable Cargo sets to the package's own directory, which
/// is what a relative `#pragma cinrs include_path` is resolved against.
pub const MANIFEST_DIR_VAR: &str = "CARGO_MANIFEST_DIR";

impl SearchPaths {
    /// The directories configured before preprocessing starts.
    pub fn new(options: &[PathBuf]) -> Self {
        Self {
            pragma: Vec::new(),
            options: options.to_vec(),
            env: std::env::var_os(ENV_VAR)
                .map(|value| std::env::split_paths(&value).collect())
                .unwrap_or_default(),
            system: Vec::new(),
            mode: System::Off,
        }
    }

    /// Adds a directory named by `#pragma cinrs include_path`.
    ///
    /// A relative path is resolved against `CARGO_MANIFEST_DIR` — the package
    /// being compiled, which the procedural macro reads from the environment
    /// of the `rustc` process Cargo started — so that a `c99!` block means the
    /// same thing however the build was invoked. Without that variable (a unit
    /// test, a hand-rolled `rustc`) the path is left as it stands, and is then
    /// relative to the working directory.
    pub fn add_pragma(&mut self, dir: &str) {
        let path = Path::new(dir);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            match std::env::var_os(MANIFEST_DIR_VAR) {
                Some(root) => Path::new(&root).join(path),
                None => path.to_path_buf(),
            }
        };
        if !self.pragma.contains(&resolved) {
            self.pragma.push(resolved);
        }
    }

    /// Every configured directory, in search order — the ones a `#embed`
    /// resource is looked for in, which is everything but the header steps.
    fn dirs(&self) -> impl Iterator<Item = &PathBuf> {
        self.pragma
            .iter()
            .chain(self.options.iter())
            .chain(self.env.iter())
    }

    /// Puts the platform's own directories on the path.
    ///
    /// Idempotent in the mode that matters: turning the switch on twice with
    /// the same directories changes nothing, and asking for `first` after
    /// asking for the plain form moves them, which is what a unit that writes
    /// both pragmas means.
    pub fn enable_system(&mut self, mode: System, dirs: Vec<PathBuf>) {
        self.mode = mode;
        self.system = dirs;
    }

    /// Whether — and where — the platform's own directories are searched.
    pub fn system_mode(&self) -> System {
        self.mode
    }

    /// Every place a header is looked for, in the order it is looked for,
    /// after the including file's own directory.
    ///
    /// This is the list `#include_next` walks: a header found under
    /// entry *i* continues from entry *i + 1*.
    ///
    /// `from_system` says the directive was written *in* one of the platform's
    /// own headers, which puts the platform's directories ahead of the bundled
    /// ones however the switch was set; see [`Origin::SystemDir`].
    pub fn entries(&self, from_system: bool) -> Vec<Entry> {
        let mut entries: Vec<Entry> = self.dirs().cloned().map(Entry::Dir).collect();
        entries.push(Entry::WorkingDir);
        let system = self.system.iter().cloned().map(Entry::System);
        match self.mode {
            System::Off => entries.push(Entry::Bundled),
            System::Last if !from_system => {
                entries.push(Entry::Bundled);
                entries.extend(system);
            }
            System::Last | System::First => {
                entries.extend(system);
                entries.push(Entry::Bundled);
            }
        }
        entries
    }
}

/// The platform's own include directories for `target`, when nothing named
/// them.
///
/// [`SYSTEM_PATH_ENV_VAR`] takes priority over this and is checked first;
/// what is left is the default, and there are only two rules to it.
///
/// **A cross build has no default.** The directories below belong to the
/// machine the compiler is *running* on, and a `<sys/stat.h>` laid out for
/// another architecture is worse than no `<sys/stat.h>` at all — its
/// `struct stat` would have the wrong offsets and the program would read the
/// wrong bytes. So a target that is not the host is an error naming
/// [`SYSTEM_PATH_ENV_VAR`], which is how a sysroot is pointed at.
///
/// **Only the C library's directories, never the compiler's.** On Linux that
/// is `/usr/local/include`, the multiarch directory
/// (`/usr/include/x86_64-linux-gnu`, and its like, added only when it exists)
/// and `/usr/include`. GCC's `/usr/lib/gcc/*/include` and Clang's
/// `/usr/lib/clang/*/include` are deliberately absent: their `limits.h`,
/// `stdint.h`, `stddef.h` and `stdarg.h` are that compiler's own, chain onward
/// with `#include_next`, and are bundled here anyway.
///
/// **Apple's default is asked of `xcrun`.** The SDK moves with Xcode, so there
/// is no path to hard-code; `xcrun --show-sdk-path` is the one thing that knows
/// it, and `apple_sdk_include` below runs it once per process. A macOS user who
/// writes `#pragma cinrs system_include` therefore gets `<unistd.h>` the way a
/// Linux user does. When `xcrun` is missing or says nothing — no Xcode, no
/// command-line tools — the answer is the same error as before, naming
/// [`SYSTEM_PATH_ENV_VAR`].
///
/// Windows and everything else has no default, because there is no such
/// convention to follow.
pub fn system_directories(
    target: &TargetModel,
    source: &TargetSource,
) -> Result<Vec<PathBuf>, String> {
    if let Some(value) = std::env::var_os(SYSTEM_PATH_ENV_VAR) {
        let dirs: Vec<PathBuf> = std::env::split_paths(&value)
            .filter(|dir| !dir.as_os_str().is_empty())
            .collect();
        if !dirs.is_empty() {
            return Ok(dirs);
        }
    }
    if *target != TargetModel::host() {
        let named = match source.triple() {
            Some(triple) => format!("'{triple}'"),
            None => "another machine".to_owned(),
        };
        return Err(format!(
            "the platform's include directories are the *host*'s, and this unit is being \
             translated for {named}; a header laid out for another machine is worse than none, \
             so point {SYSTEM_PATH_ENV_VAR} at the target's sysroot include directories — or \
             leave the system headers switched off for this target"
        ));
    }
    match target.os {
        Os::Linux => {
            let mut dirs = vec![PathBuf::from("/usr/local/include")];
            for tuple in multiarch_tuples(target) {
                let dir = PathBuf::from(format!("/usr/include/{tuple}"));
                if dir.is_dir() {
                    dirs.push(dir);
                }
            }
            dirs.push(PathBuf::from("/usr/include"));
            Ok(dirs)
        }
        Os::Darwin => match apple_sdk_include() {
            Some(dir) => Ok(vec![dir]),
            None => Err(format!(
                "on Apple's platforms the C library's headers live inside the SDK, and \
                 'xcrun --show-sdk-path' — the one thing that knows where it is — is not \
                 installed or answered nothing, so cinrs has no directory to offer: set \
                 {SYSTEM_PATH_ENV_VAR} to \"$(xcrun --show-sdk-path)/usr/include\""
            )),
        },
        os => Err(format!(
            "cinrs has no default include directories for {}; set {SYSTEM_PATH_ENV_VAR} to the \
             directories the platform's headers live in",
            os.as_str()
        )),
    }
}

/// `$(xcrun --show-sdk-path)/usr/include`, asked once per process.
///
/// Apple's C library headers live inside whichever SDK Xcode is pointing at,
/// and `xcrun` is the only thing that knows which. A procedural macro is an
/// ordinary program and may run one, so it does — once, because the answer
/// cannot change while the process lives and because a unit may ask several
/// times. `None` when there is no `xcrun`, when it fails, when its output is
/// not a directory, or when the `usr/include` inside it is not there: every one
/// of those means "the caller should say to set the environment variable"
/// rather than "use a path that is not there".
///
/// The whole of the Darwin behaviour is in this one function so that the part
/// a Linux machine cannot test is as small as it can be; [`system_directories`]
/// is the part unit tests reach.
fn apple_sdk_include() -> Option<PathBuf> {
    static SDK: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    SDK.get_or_init(|| {
        let output = std::process::Command::new("xcrun")
            .arg("--show-sdk-path")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let path = String::from_utf8(output.stdout).ok()?;
        let path = path.trim();
        if path.is_empty() {
            return None;
        }
        let include = Path::new(path).join("usr").join("include");
        include.is_dir().then_some(include)
    })
    .clone()
}

/// The multiarch directory names a Linux target's headers may live under,
/// most specific first.
///
/// Debian's layout, which Ubuntu and a good many others follow: the tuple is
/// `<arch>-linux-<env>`, with the architecture spelled the way `dpkg` spells
/// it rather than the way the triple does — `i386` for 32-bit x86,
/// `powerpc64le` for little-endian 64-bit PowerPC. Only a directory that
/// really exists is used, which is what lets the Arm entries name both the
/// hard-float and the soft-float spelling without guessing.
fn multiarch_tuples(target: &TargetModel) -> Vec<String> {
    let env = match target.env {
        Env::Musl => "musl",
        Env::Bionic => "android",
        Env::Uclibc => "uclibc",
        // A Linux triple that names no environment is glibc; see `Env`.
        _ => "gnu",
    };
    let archs: &[&str] = match target.arch {
        Arch::X86_64 => &["x86_64"],
        Arch::X86 => &["i386"],
        Arch::Aarch64 => &["aarch64"],
        Arch::Arm => &["arm"],
        Arch::Riscv32 => &["riscv32"],
        Arch::Riscv64 => &["riscv64"],
        Arch::PowerPc => &["powerpc"],
        Arch::PowerPc64 => {
            if target.big_endian {
                &["powerpc64"]
            } else {
                &["powerpc64le"]
            }
        }
        Arch::S390x => &["s390x"],
        Arch::Mips => {
            if target.big_endian {
                &["mips"]
            } else {
                &["mipsel"]
            }
        }
        Arch::Mips64 => {
            if target.big_endian {
                &["mips64"]
            } else {
                &["mips64el"]
            }
        }
        Arch::Sparc => &["sparc"],
        Arch::Sparc64 => &["sparc64"],
        Arch::LoongArch64 => &["loongarch64"],
        // Nothing installs headers under a wasm tuple.
        Arch::Wasm32 => &[],
    };
    // Arm's ABI is part of the tuple and the model does not carry it, so both
    // spellings are offered and the one that exists is taken.
    let suffixes: &[&str] = if target.arch == Arch::Arm && env == "gnu" {
        &["eabihf", "eabi"]
    } else {
        &[""]
    };
    let mut tuples = Vec::new();
    for arch in archs {
        for suffix in suffixes {
            tuples.push(format!("{arch}-linux-{env}{suffix}"));
        }
    }
    tuples
}

/// A header that was found.
#[derive(Clone, Debug)]
pub struct Resolved {
    /// The name diagnostics call it: a path for a file on disk, and
    /// `<cinrs>/stdio.h` for a bundled header.
    pub name: String,
    /// Its contents.
    pub text: String,
    /// Where its own `#include "…"` looks first.
    pub origin: Origin,
    /// What identifies it for `#pragma once` and the include-guard
    /// optimisation: the canonical path of the file, or the bundled name.
    pub key: String,
    /// The absolute path of the file, for rebuild tracking. `None` for a
    /// bundled header, which cannot change without the crate changing, and for
    /// a header taken from one of the platform's own directories, which is
    /// part of the machine rather than of the crate.
    pub path: Option<PathBuf>,
    /// The [entry](Entry) it was found under, which is where an
    /// `#include_next` written inside it goes on *after*. `None` when it was
    /// not found by a search at all: an absolute name, or the including file's
    /// own directory.
    pub found_in: Option<Entry>,
    /// Whether it came from one of the platform's own directories.
    pub system: bool,
}

impl Resolved {
    /// Marks a header as one of the platform's own.
    ///
    /// Two things follow, and they are the whole of what "system header" means
    /// here: its own `<…>` includes prefer the platform's directories, so that
    /// the platform's header set stays self-consistent; and it is not tracked
    /// for rebuilds, because it belongs to the machine rather than to the
    /// crate.
    fn make_system(&mut self) {
        self.system = true;
        self.path = None;
        if let Origin::Dir(dir) = std::mem::take(&mut self.origin) {
            self.origin = Origin::SystemDir(dir);
        }
    }
}

/// Why a header could not be included.
#[derive(Clone, Debug)]
pub enum Error {
    /// Nothing of that name is anywhere the unit searches.
    NotFound {
        /// The directories that were looked in, in order, as a reader can
        /// check them.
        searched: Vec<String>,
    },
    /// A file of that name is there, but could not be read: no permission, not
    /// UTF-8, gone between the test and the read.
    Unreadable {
        /// The file that could not be read.
        path: String,
        /// What the operating system said about it.
        error: String,
    },
}

/// Looks a header name up.
pub fn resolve(
    name: &str,
    form: Form,
    origin: &Origin,
    paths: &SearchPaths,
) -> Result<Resolved, Error> {
    let mut searched: Vec<String> = Vec::new();

    // An absolute name is not searched for: it either is the file or it is
    // nothing.
    if Path::new(name).is_absolute() {
        return absolute(name, origin);
    }

    if form == Form::Quoted {
        match origin {
            Origin::Dir(dir) | Origin::SystemDir(dir) => {
                if let Some(mut found) = read_file(&dir.join(name))? {
                    // A header beside a system header is a system header too:
                    // `bits/types.h` is reached that way, and it must not
                    // suddenly start preferring the bundled set.
                    if let Origin::SystemDir(_) = origin {
                        found.make_system();
                    }
                    return Ok(found);
                }
                searched.push(display_dir(dir));
            }
            Origin::Bundled => {
                if let Some(found) = read_bundled(name) {
                    return Ok(found);
                }
                searched.push(BUNDLED_DIR.to_owned());
            }
            Origin::Unknown => {}
        }
    }

    walk(name, form, paths, from_system(origin), 0, searched)
}

/// Whether the file writing the directive is one of the platform's own.
fn from_system(origin: &Origin) -> bool {
    matches!(origin, Origin::SystemDir(_))
}

/// A header named by an absolute path, which is not searched for: it either is
/// the file or it is nothing.
///
/// One header of the platform's naming another by its full path keeps the set
/// together, exactly as a relative name found beside it does.
fn absolute(name: &str, origin: &Origin) -> Result<Resolved, Error> {
    match read_file(Path::new(name))? {
        Some(mut found) => {
            if from_system(origin) {
                found.make_system();
            }
            Ok(found)
        }
        None => Err(Error::NotFound {
            searched: vec![display_path(Path::new(name))],
        }),
    }
}

/// `#include_next <name>`: the same search, taken up again at the entry
/// *after* the one the file writing the directive was found under.
///
/// GCC's semantics exactly, and the reason the directive exists: a platform's
/// `<limits.h>` finishes with `#include_next <limits.h>` to reach the *next*
/// `limits.h` on the path rather than itself. `current` is
/// [`Resolved::found_in`] of the file the directive is written in; a file that
/// was not found by a search at all — the unit's own text, or a header taken
/// from the including file's directory — has no entry to go on from, and GCC
/// starts such a search at the beginning, which is what `None` does here.
///
/// The quoted and the angled forms mean the same thing, as they do in GCC: the
/// including file's own directory is never step one of an `#include_next`.
pub fn resolve_next(
    name: &str,
    origin: &Origin,
    current: Option<&Entry>,
    paths: &SearchPaths,
) -> Result<Resolved, Error> {
    if Path::new(name).is_absolute() {
        return absolute(name, origin);
    }
    let system = from_system(origin);
    let start = match current {
        Some(entry) => paths
            .entries(system)
            .iter()
            .position(|e| e == entry)
            .map_or(0, |at| at + 1),
        None => 0,
    };
    walk(name, Form::Angled, paths, system, start, Vec::new())
}

/// The part of the search that walks [`SearchPaths::entries`], from `start`.
fn walk(
    name: &str,
    form: Form,
    paths: &SearchPaths,
    from_system: bool,
    start: usize,
    mut searched: Vec<String>,
) -> Result<Resolved, Error> {
    for entry in paths.entries(from_system).into_iter().skip(start) {
        let shown = match &entry {
            Entry::Dir(dir) | Entry::System(dir) => {
                if let Some(mut found) = read_file(&dir.join(name))? {
                    if matches!(entry, Entry::System(_)) {
                        found.make_system();
                    }
                    found.found_in = Some(entry);
                    return Ok(found);
                }
                display_dir(dir)
            }
            // A name that is itself a path, taken from the working directory.
            // This is what `#include __FILE__` needs: the name a header is
            // known by is relative to the working directory, so a header that
            // includes itself asks for `some/dir/thing.h` from inside
            // `some/dir`, which neither the origin nor a `-I` will resolve. A
            // bare header name is not looked for here, so nothing in the
            // working directory can shadow a bundled header.
            Entry::WorkingDir => {
                if form != Form::Quoted || !is_path(name) {
                    continue;
                }
                if let Some(mut found) = read_file(Path::new(name))? {
                    found.found_in = Some(entry);
                    return Ok(found);
                }
                display_dir(Path::new(""))
            }
            Entry::Bundled => {
                if let Some(mut found) = read_bundled(name) {
                    found.found_in = Some(entry);
                    return Ok(found);
                }
                BUNDLED_DIR.to_owned()
            }
        };
        if !searched.contains(&shown) {
            searched.push(shown);
        }
    }
    Err(Error::NotFound { searched })
}

/// A resource `#embed` found.
#[derive(Clone, Debug)]
pub struct Embedded {
    /// The name diagnostics call it.
    pub name: String,
    /// Its contents, byte for byte.
    pub bytes: Vec<u8>,
    /// Its absolute path, for rebuild tracking.
    pub path: PathBuf,
}

/// Looks an `#embed` resource up (C23 6.10.3).
///
/// The search is `#include`'s with the two steps that are about *headers* left
/// out: there are no bundled resources — a picture is not a declaration — and
/// nothing is read from the working directory by a bare name. What is left is
/// the directory of the file the directive is written in, for the quoted form,
/// and then the include path.
pub fn resolve_embed(
    name: &str,
    form: Form,
    origin: &Origin,
    paths: &SearchPaths,
) -> Result<Embedded, Error> {
    let mut searched: Vec<String> = Vec::new();

    if Path::new(name).is_absolute() {
        return match read_bytes(Path::new(name))? {
            Some(found) => Ok(found),
            None => Err(Error::NotFound {
                searched: vec![display_path(Path::new(name))],
            }),
        };
    }

    if form == Form::Quoted {
        match origin {
            Origin::Dir(dir) | Origin::SystemDir(dir) => {
                if let Some(found) = read_bytes(&dir.join(name))? {
                    return Ok(found);
                }
                searched.push(display_dir(dir));
            }
            // A bundled header's `#embed` has nowhere of its own to look, and
            // the platform's directories hold no resources either — `#embed`
            // is for a picture, not for a declaration.
            Origin::Bundled | Origin::Unknown => {}
        }
    }

    for dir in paths.dirs() {
        if let Some(found) = read_bytes(&dir.join(name))? {
            return Ok(found);
        }
        let shown = display_dir(dir);
        if !searched.contains(&shown) {
            searched.push(shown);
        }
    }

    // As for a header, a name that is itself a path is looked for from the
    // working directory, so that a resource named relative to it can be found
    // from a directive written elsewhere.
    if form == Form::Quoted && is_path(name) {
        if let Some(found) = read_bytes(Path::new(name))? {
            return Ok(found);
        }
        let shown = display_dir(Path::new(""));
        if !searched.contains(&shown) {
            searched.push(shown);
        }
    }

    Err(Error::NotFound { searched })
}

/// Reads a candidate resource, with [`read_file`]'s convention: `Ok(None)`
/// means there is no such file and the search goes on.
fn read_bytes(path: &Path) -> Result<Option<Embedded>, Error> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {}
        _ => return Ok(None),
    }
    let bytes = std::fs::read(path).map_err(|error| Error::Unreadable {
        path: display_path(path),
        error: error.to_string(),
    })?;
    Ok(Some(Embedded {
        name: display_path(path),
        bytes,
        path: std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()),
    }))
}

/// Whether a header name names a directory as well as a file.
///
/// Both separators count on every platform: a `#include "sub/thing.h"` is
/// written with a forward slash in portable C whatever the host does with
/// them.
fn is_path(name: &str) -> bool {
    name.contains('/') || name.contains('\\')
}

fn read_bundled(name: &str) -> Option<Resolved> {
    let text = bundled(name)?;
    let display = bundled_name(name);
    Some(Resolved {
        text: text.to_owned(),
        origin: Origin::Bundled,
        key: display.clone(),
        name: display,
        path: None,
        found_in: Some(Entry::Bundled),
        system: false,
    })
}

/// Reads the file an `include_c99!("…")` names.
///
/// Not a search: the path was resolved against the directory of the invoking
/// `.rs` file before it got here, so it either is the translation unit or it is
/// nothing. What comes back is the same [`Resolved`] a header does — the
/// display name for diagnostics and for `__FILE__`, the text, the directory its
/// own `#include "…"` searches first, and the absolute path the expansion
/// tracks for rebuilds.
pub fn read_source(path: &Path) -> Result<Resolved, Error> {
    match read_file(path)? {
        Some(found) => Ok(found),
        None => Err(Error::NotFound {
            searched: vec![display_path(path)],
        }),
    }
}

/// Reads a candidate path: `Ok(None)` when there is no such file, which means
/// the search goes on, and an error when there is one and it cannot be used,
/// which means it does not.
fn read_file(path: &Path) -> Result<Option<Resolved>, Error> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {}
        // A directory of that name, or nothing at all: keep looking.
        _ => return Ok(None),
    }
    let text = std::fs::read_to_string(path).map_err(|error| Error::Unreadable {
        path: display_path(path),
        error: error.to_string(),
    })?;
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let key = std::fs::canonicalize(path)
        .unwrap_or_else(|_| absolute.clone())
        .display()
        .to_string();
    Ok(Some(Resolved {
        name: display_path(path),
        text,
        origin: Origin::Dir(path.parent().unwrap_or(Path::new("")).to_path_buf()),
        key,
        path: Some(absolute),
        // Filled in by the search that found it; a file read by name — an
        // absolute `#include`, or `include_c99!` — was found by no search.
        found_in: None,
        system: false,
    }))
}

/// How a path is written in a diagnostic.
///
/// Relative wherever it can be — a message naming
/// `/home/someone/crate/include/foo.h` is a message that differs between two
/// machines, which makes it useless in a test and noisy everywhere else — so a
/// path inside the working directory is shown relative to it.
pub fn display_path(path: &Path) -> String {
    let relative = std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(&cwd).ok().map(Path::to_path_buf));
    relative
        .unwrap_or_else(|| path.to_path_buf())
        .display()
        .to_string()
}

/// How a directory is written in the list a "not found" message shows.
fn display_dir(dir: &Path) -> String {
    let shown = display_path(dir);
    if shown.is_empty() {
        // `Path::parent` of a bare file name; the working directory.
        ".".to_owned()
    } else {
        shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_header_is_listed_once_and_sorted() {
        let mut names: Vec<&str> = BUNDLED.iter().map(|(n, _)| *n).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "a header is listed twice");
        let listed: Vec<&str> = BUNDLED.iter().map(|(n, _)| *n).collect();
        assert_eq!(listed, names, "keep the table sorted by name");
    }

    #[test]
    fn a_bundled_header_is_found_without_touching_the_disk() {
        let found = resolve(
            "stddef.h",
            Form::Angled,
            &Origin::Unknown,
            &SearchPaths::default(),
        )
        .expect("stddef.h is bundled");
        assert_eq!(found.name, "<cinrs>/stddef.h");
        assert!(found.path.is_none());
        assert!(found.text.contains("size_t"));
    }

    #[test]
    fn a_quoted_name_that_is_a_path_is_taken_from_the_working_directory() {
        // What `#include __FILE__` in a header comes to: the name is a path
        // relative to the working directory, and the directive is written in
        // the directory that path leads to, so neither the origin nor a `-I`
        // resolves it.
        // `cargo test` runs with the package directory as the working
        // directory, and `include/` is where the bundled headers live as
        // files.
        let found = resolve(
            "include/stddef.h",
            Form::Quoted,
            &Origin::Dir(PathBuf::from("include")),
            &SearchPaths::default(),
        )
        .expect("the file is there, relative to the package directory");
        assert!(found.text.contains("size_t"));
        assert!(
            found.path.is_some(),
            "it is a real file, not the bundled one"
        );
    }

    #[test]
    fn a_bare_name_in_the_working_directory_does_not_shadow_a_bundled_header() {
        // `Cargo.toml` is in the working directory and is not a header, but
        // the point is the rule: a name with no separator is never looked for
        // there, so the step cannot take a `stdio.h` somebody left lying
        // about in preference to ours.
        assert!(!is_path("stdio.h"));
        assert!(is_path("sub/stdio.h"));
        assert!(is_path("sub\\stdio.h"));
    }

    #[test]
    fn a_missing_header_lists_where_it_looked() {
        let error = resolve(
            "nowhere.h",
            Form::Quoted,
            &Origin::Dir(PathBuf::from("src")),
            &SearchPaths::default(),
        )
        .expect_err("nothing is called nowhere.h");
        match error {
            Error::NotFound { searched } => assert_eq!(searched, ["src", "<cinrs>"]),
            other => panic!("expected a not-found error, got {other:?}"),
        }
    }

    // -- the platform's own directories -------------------------------------

    fn with_system(mode: System) -> SearchPaths {
        let mut paths = SearchPaths::default();
        paths.enable_system(mode, vec![PathBuf::from("/usr/include")]);
        paths
    }

    #[test]
    fn the_switch_reads_the_spellings_a_build_system_uses() {
        assert_eq!(System::from_env_value("1"), Some(System::Last));
        assert_eq!(System::from_env_value(" YES "), Some(System::Last));
        assert_eq!(System::from_env_value("First"), Some(System::First));
        assert_eq!(System::from_env_value("0"), Some(System::Off));
        assert_eq!(System::from_env_value(""), Some(System::Off));
        assert_eq!(System::from_env_value("maybe"), None);
        assert!(!System::Off.is_on());
        assert!(System::Last.is_on() && System::First.is_on());
    }

    #[test]
    fn the_mode_decides_where_the_platform_goes() {
        let system = Entry::System(PathBuf::from("/usr/include"));
        assert_eq!(
            SearchPaths::default().entries(false),
            [Entry::WorkingDir, Entry::Bundled],
            "off by default"
        );
        assert_eq!(
            with_system(System::Last).entries(false),
            [Entry::WorkingDir, Entry::Bundled, system.clone()]
        );
        assert_eq!(
            with_system(System::First).entries(false),
            [Entry::WorkingDir, system.clone(), Entry::Bundled]
        );
        // A directive written inside a system header prefers the platform
        // whatever the mode, so that glibc's `<pthread.h>` gets glibc's
        // `<time.h>` and there is one `struct timespec` rather than two.
        assert_eq!(
            with_system(System::Last).entries(true),
            [Entry::WorkingDir, system.clone(), Entry::Bundled]
        );
        assert_eq!(
            SearchPaths::default().entries(true),
            [Entry::WorkingDir, Entry::Bundled],
            "and nothing at all while the switch is off"
        );
    }

    #[test]
    fn the_platform_is_searched_only_when_the_switch_is_on() {
        // Something every Unix has and this crate does not bundle. The test is
        // about the *order*, so a platform without it simply checks less.
        let name = "sys/stat.h";
        let there = Path::new("/usr/include").join(name).is_file();
        let off = resolve(
            name,
            Form::Angled,
            &Origin::Unknown,
            &SearchPaths::default(),
        );
        assert!(off.is_err(), "the platform is not searched by default");
        if there {
            let on = resolve(
                name,
                Form::Angled,
                &Origin::Unknown,
                &with_system(System::Last),
            )
            .expect("the platform has it");
            assert!(on.system, "and it is marked as the platform's");
            assert!(on.path.is_none(), "so it is not tracked for rebuilds");
            assert!(matches!(on.origin, Origin::SystemDir(_)));
        }
    }

    #[test]
    fn the_bundled_copy_wins_unless_the_platform_goes_first() {
        let name = "stdio.h";
        if !Path::new("/usr/include").join(name).is_file() {
            return;
        }
        let last = resolve(
            name,
            Form::Angled,
            &Origin::Unknown,
            &with_system(System::Last),
        )
        .expect("bundled");
        assert_eq!(last.name, bundled_name(name));
        let first = resolve(
            name,
            Form::Angled,
            &Origin::Unknown,
            &with_system(System::First),
        )
        .expect("the platform's");
        assert!(first.system, "the platform's copy came first");
    }

    #[test]
    fn a_cross_target_has_no_default_directories() {
        // The one target that is certainly not the host, whichever machine
        // this is: the model differs from the host's in at least its
        // architecture.
        let host = TargetModel::host();
        let triple = if host.arch == Arch::S390x {
            "sparc64-unknown-linux-gnu"
        } else {
            "s390x-unknown-linux-gnu"
        };
        let target = TargetModel::from_triple(triple).expect("a target this crate models");
        let source = TargetSource::Env(triple.to_owned());
        // Only meaningful when the environment has not named directories
        // itself, which is exactly the case the rule is about.
        if std::env::var_os(SYSTEM_PATH_ENV_VAR).is_some() {
            return;
        }
        let message = system_directories(&target, &source).expect_err("a cross build has none");
        assert!(message.contains(SYSTEM_PATH_ENV_VAR), "{message}");
        assert!(message.contains(triple), "{message}");
    }

    #[test]
    fn the_multiarch_tuple_follows_the_model() {
        let tuples =
            |triple: &str| multiarch_tuples(&TargetModel::from_triple(triple).expect("modelled"));
        assert_eq!(tuples("x86_64-unknown-linux-gnu"), ["x86_64-linux-gnu"]);
        assert_eq!(tuples("i686-unknown-linux-gnu"), ["i386-linux-gnu"]);
        assert_eq!(tuples("aarch64-unknown-linux-musl"), ["aarch64-linux-musl"]);
        assert_eq!(
            tuples("arm-unknown-linux-gnueabihf"),
            ["arm-linux-gnueabihf", "arm-linux-gnueabi"],
            "the ABI is not in the model, so both spellings are offered"
        );
        assert_eq!(
            tuples("powerpc64le-unknown-linux-gnu"),
            ["powerpc64le-linux-gnu"]
        );
        assert_eq!(tuples("s390x-unknown-linux-gnu"), ["s390x-linux-gnu"]);
    }
}
