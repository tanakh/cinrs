//! The platform's own headers, read by the front end and linked against.
//!
//! `#pragma cinrs system_include` puts `/usr/include` and its like on the
//! search path; see [`crate::include`](../crates/cinrs-core/src/include.rs) and
//! `doc/system-headers.md`. Two things have to be true for that to be worth
//! anything, and this file checks both:
//!
//! * **every header a program reaches for goes through the front end.** The
//!   first test runs each of the C standard headers and the POSIX set through
//!   lexing, preprocessing, parsing and sema *alone*, in `gnu11!` and in
//!   `c11!`, with the platform's copies preferred over the bundled ones. What
//!   fails is listed in [`KNOWN_GAPS`] with the reason, and a gap that closes
//!   fails the test too, so the table in `doc/system-headers.md` cannot go
//!   stale.
//! * **the declarations it read are the real ones.** The rest of the file is
//!   ordinary programs — `stat`, `opendir`, `pthread_create`, `socketpair`,
//!   `getopt`, `regcomp`, `dlopen`, `gettimeofday`, `uname`, `mmap` — compiled
//!   by `cinrs` against the platform's headers and linked against the platform's
//!   library. The layouts those headers describe are compared against `cc`'s,
//!   which is the only oracle there is for a `struct stat`.
//!
//! # Skipping
//!
//! The header sweep skips itself, with the reason, unless the host is Linux
//! with glibc and `/usr/include/stdio.h` is there. The programs are compiled at
//! *macro expansion* time and so cannot decide anything at run time: they are
//! `#[cfg]`-gated on a glibc Linux target, where the headers come from the same
//! package (`libc6-dev`) as the object files `rustc` already needs to link
//! anything at all. The differential comparison additionally needs a C compiler
//! on `PATH`, and says so and passes without one.

// ---------------------------------------------------------------------------
// the header sweep
// ---------------------------------------------------------------------------

/// The C standard headers, as C23 7.1.2 lists them minus the ones no
/// implementation of this vintage ships (`<stdbit.h>`, `<stdckdint.h>`).
const STANDARD_HEADERS: &[&str] = &[
    "assert.h",
    "complex.h",
    "ctype.h",
    "errno.h",
    "fenv.h",
    "float.h",
    "inttypes.h",
    "iso646.h",
    "limits.h",
    "locale.h",
    "math.h",
    "setjmp.h",
    "signal.h",
    "stdalign.h",
    "stdarg.h",
    "stdatomic.h",
    "stdbool.h",
    "stddef.h",
    "stdint.h",
    "stdio.h",
    "stdlib.h",
    "stdnoreturn.h",
    "string.h",
    "tgmath.h",
    "threads.h",
    "time.h",
    "uchar.h",
    "wchar.h",
    "wctype.h",
];

/// The POSIX headers a program written in C really reaches for.
const POSIX_HEADERS: &[&str] = &[
    "alloca.h",
    "arpa/inet.h",
    "dirent.h",
    "dlfcn.h",
    "fcntl.h",
    "fnmatch.h",
    "getopt.h",
    "glob.h",
    "grp.h",
    "iconv.h",
    "langinfo.h",
    "libgen.h",
    "netdb.h",
    "netinet/in.h",
    "poll.h",
    "pthread.h",
    "pwd.h",
    "regex.h",
    "sched.h",
    "semaphore.h",
    "spawn.h",
    "strings.h",
    "sys/ioctl.h",
    "sys/mman.h",
    "sys/resource.h",
    "sys/select.h",
    "sys/socket.h",
    "sys/stat.h",
    "sys/time.h",
    "sys/types.h",
    "sys/uio.h",
    "sys/un.h",
    "sys/utsname.h",
    "sys/wait.h",
    "syslog.h",
    "termios.h",
    "unistd.h",
    "utime.h",
];

/// The headers whose platform copy the front end cannot read, with the reason.
///
/// One entry, and it is not really about C at all: glibc ties `_Float128` to
/// `__GNUC_PREREQ (4, 3)` while making `_Float64x` unconditional on x86-64, and
/// `<tgmath.h>` refuses that combination with an `#error` of its own. `cinrs`
/// claims `__GNUC__` 4.2.1 — the version Clang picked, and the one every
/// `__attribute__` guard in the wild tests against — so the `#error` fires.
/// Claiming 4.3 instead makes `<bits/floatn.h>` declare `__float128` and
/// `_Complex __float128` unconditionally, which takes `<stdio.h>`, `<stdlib.h>`,
/// `<math.h>` and `<complex.h>` down with it: one header lost is the cheaper
/// trade, and it is the one header of the set whose whole content is macros
/// that need a compiler builtin anyway.
const KNOWN_GAPS: &[(&str, &str)] = &[(
    "tgmath.h",
    "glibc's own #error for __HAVE_FLOAT64X without __HAVE_FLOAT128, which \
     follows from __GNUC__ being 4.2.1",
)];

/// Why the sweep cannot run here, if it cannot.
fn skip_reason() -> Option<String> {
    if !cfg!(target_os = "linux") {
        return Some("the platform's headers are only swept on Linux".to_owned());
    }
    if !cfg!(target_env = "gnu") {
        return Some("the sweep is written against glibc's headers".to_owned());
    }
    if !std::path::Path::new("/usr/include/stdio.h").is_file() {
        return Some("/usr/include/stdio.h is not there (no libc development headers)".to_owned());
    }
    None
}

#[test]
fn every_platform_header_goes_through_the_front_end() {
    if let Some(reason) = skip_reason() {
        println!("skipping the system-header sweep: {reason}");
        return;
    }
    let mut gaps: Vec<(String, String)> = Vec::new();
    let mut closed: Vec<String> = Vec::new();
    for header in STANDARD_HEADERS.iter().chain(POSIX_HEADERS) {
        for gnu in [true, false] {
            let entry = if gnu { "gnu11" } else { "c11" };
            let id = format!("{header} ({entry})");
            let errors = sweep::front_end_errors(header, gnu);
            match (errors.is_empty(), expected_gap(header)) {
                (true, None) => {}
                (true, Some(_)) => closed.push(id),
                (false, Some(_)) => {}
                (false, None) => gaps.push((id, errors.join("\n    "))),
            }
        }
    }
    assert!(
        gaps.is_empty(),
        "these platform headers no longer go through the front end:\n{}",
        gaps.iter()
            .map(|(id, errors)| format!("  {id}\n    {errors}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        closed.is_empty(),
        "these headers are listed in KNOWN_GAPS but go through cleanly now; \
         drop them from the list and from doc/system-headers.md:\n  {}",
        closed.join("\n  ")
    );
}

/// The reason `header` is expected to fail, if it is.
///
/// `<complex.h>` and `<tgmath.h>` are both built on `_Complex`, which a build
/// with the `complex` feature off diagnoses on purpose; that is a property of
/// the build rather than of the header, so it is not in [`KNOWN_GAPS`].
fn expected_gap(header: &str) -> Option<&'static str> {
    if !cinrs_core::COMPLEX_SUPPORTED && matches!(header, "complex.h" | "tgmath.h") {
        return Some("the crate was built with the 'complex' feature off");
    }
    KNOWN_GAPS
        .iter()
        .find(|(name, _)| *name == header)
        .map(|(_, reason)| *reason)
}

/// `_FILE_OFFSET_BITS=64`, which is how half of glibc's declarations become
/// `__REDIRECT`s.
///
/// A `__REDIRECT` is a prototype plus an `__asm__` label built out of two
/// adjacent string literals — `__asm__ ("" "stat64")` — so this is the setting
/// that puts asm labels, and their concatenation, on nearly every function in
/// `<sys/stat.h>` and `<stdio.h>`. Worth its own test because the sweep above
/// never defines it.
#[test]
fn the_large_file_declarations_go_through_too() {
    if let Some(reason) = skip_reason() {
        println!("skipping the large-file check: {reason}");
        return;
    }
    let errors = sweep::front_end_errors_for(
        "#pragma cinrs system_include first\n\
         #define _FILE_OFFSET_BITS 64\n\
         #include <sys/stat.h>\n\
         #include <stdio.h>\n\
         long size_of(const char *p) { struct stat s; return stat(p, &s) ? -1 : (long) s.st_size; }\n",
        true,
    );
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}

/// A feature test macro reaches glibc's headers, which is how a strict entry
/// point asks for POSIX.
///
/// The strict entry points define `__STRICT_ANSI__`, exactly as `gcc -std=c11`
/// does, and glibc then withholds everything outside C — `sigset_t` and
/// `struct sigaction` among it. `_GNU_SOURCE` ahead of the first `#include` is
/// the answer, and it has to work in both entry points.
#[test]
fn a_feature_test_macro_opens_the_posix_declarations() {
    if let Some(reason) = skip_reason() {
        println!("skipping the feature-test-macro check: {reason}");
        return;
    }
    let unit = "#pragma cinrs system_include first\n\
                #define _GNU_SOURCE 1\n\
                #include <signal.h>\n\
                unsigned long sizes(void) { return sizeof(sigset_t) + sizeof(struct sigaction); }\n";
    for gnu in [true, false] {
        let errors = sweep::front_end_errors_for(unit, gnu);
        assert!(
            errors.is_empty(),
            "_GNU_SOURCE in {}: {}",
            if gnu { "gnu11" } else { "c11" },
            errors.join("\n")
        );
    }
}

mod sweep {
    use std::str::FromStr;

    use cinrs_core::{Dialect, Level, Options, Standard, include::System};

    /// Room for sema's recursion over a header the size of `<stdio.h>`'s
    /// closure; the front end already runs its own passes on a stack of its
    /// own, and this is the one that carries them.
    const WORKER_STACK: usize = 64 << 20;

    /// The errors the front end reports for `#include <header>` alone, with the
    /// platform's own copies preferred over the bundled ones.
    pub(super) fn front_end_errors(header: &str, gnu: bool) -> Vec<String> {
        front_end_errors_for(
            &format!("#pragma cinrs system_include first\n#include <{header}>\n"),
            gnu,
        )
    }

    /// The same, over a whole unit written out.
    pub(super) fn front_end_errors_for(unit: &str, gnu: bool) -> Vec<String> {
        let unit = unit.to_owned();
        let mut options = if gnu {
            Options::gnu(Standard::C11)
        } else {
            Options::new(Standard::C11)
        };
        options.dialect = if gnu { Dialect::Gnu } else { Dialect::Iso };
        options.system_include = System::First;
        std::thread::Builder::new()
            .name("system-header-sweep".to_owned())
            .stack_size(WORKER_STACK)
            .spawn(move || {
                let literal = format!("r#\"{unit}\"#");
                let stream = proc_macro2::TokenStream::from_str(&literal)
                    .expect("the directive fits in a raw string literal");
                let analysis = cinrs_core::analyze(stream, &options);
                let unit_id = analysis.source.unit_id();
                let mut diagnostics = analysis.diagnostics;
                let (_program, mut sema) =
                    cinrs_core::sema::analyze(&analysis.unit, &options, unit_id);
                analysis.expansions.annotate(&mut sema);
                diagnostics.extend(sema);
                let map = &analysis.source.map;
                diagnostics
                    .sorted()
                    .into_iter()
                    .filter(|diag| diag.level == Level::Error)
                    .map(|diag| match map.header_position(diag.range.start) {
                        Some((header, line, col)) => {
                            format!("{header}:{line}:{col}: {}", diag.message)
                        }
                        None => {
                            let (line, col) = map.line_col(diag.range.start);
                            format!("{line}:{col}: {}", diag.message)
                        }
                    })
                    .collect()
            })
            .expect("a thread can be spawned")
            .join()
            .expect("the front end does not panic")
    }
}

// ---------------------------------------------------------------------------
// programs written against the platform's headers
// ---------------------------------------------------------------------------

/// A glibc Linux, where the C below can be compiled at all.
///
/// The headers come from the same package as the object files `rustc` links
/// every binary against, so a target that can build this crate at all has them.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod glibc {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use cinrs::gnu11;

    gnu11! {
        /* The bundled headers still win: only what cinrs does not carry —
         * `struct stat`, `DIR`, `pthread_mutex_t`, `struct utsname` — comes
         * from the platform. */
        #pragma cinrs system_include

        /* The bundled `<stdint.h>` and `<sys/types.h>` come first on purpose,
         * and `<sys/stat.h>` — which is only the platform's — then declares
         * `dev_t`, `ino_t`, `mode_t`, `nlink_t`, `off_t`, `blkcnt_t` and
         * `blksize_t` all over again out of glibc's `__dev_t` and its
         * relatives. C11 6.7p3 (N1360) makes that legal exactly as long as the
         * two spellings are the *same type*, which is what
         * `the_layouts_cinrs_computed_are_the_ones_cc_computes` checks below
         * and why the bundled header spells them the way glibc does. */
        #include <stdint.h>
        #include <sys/types.h>
        #include <sys/stat.h>
        #include <sys/mman.h>
        #include <sys/socket.h>
        #include <sys/time.h>
        #include <sys/utsname.h>
        #include <dirent.h>
        #include <dlfcn.h>
        #include <getopt.h>
        #include <pthread.h>
        #include <regex.h>
        #include <stddef.h>
        #include <string.h>
        #include <unistd.h>

        /* -- struct stat, through the platform's own layout ---------------- */

        long file_size(const char *path) {
            struct stat st;
            if (stat(path, &st) != 0) return -1;
            return (long) st.st_size;
        }

        int is_directory(const char *path) {
            struct stat st;
            if (stat(path, &st) != 0) return -1;
            return S_ISDIR(st.st_mode) ? 1 : 0;
        }

        unsigned long sizeof_struct_stat(void)  { return sizeof(struct stat); }
        unsigned long offsetof_st_size(void)    { return offsetof(struct stat, st_size); }
        unsigned long sizeof_pthread_mutex(void){ return sizeof(pthread_mutex_t); }
        unsigned long sizeof_dirent(void)       { return sizeof(struct dirent); }
        unsigned long offsetof_d_name(void)     { return offsetof(struct dirent, d_name); }
        unsigned long sizeof_timeval(void)      { return sizeof(struct timeval); }
        unsigned long sizeof_utsname(void)      { return sizeof(struct utsname); }

        /* The typedefs both header sets declare. Whichever spelling won here —
         * the bundled one, since it came first — has to be the type glibc's own
         * `<sys/stat.h>` redeclared, or the redefinition would not have been
         * benign and this unit would not have compiled at all. Comparing the
         * widths against `cc` is what proves the two really are one type. */
        unsigned long sizeof_dev_t(void)        { return sizeof(dev_t); }
        unsigned long sizeof_ino_t(void)        { return sizeof(ino_t); }
        unsigned long sizeof_mode_t(void)       { return sizeof(mode_t); }
        unsigned long sizeof_nlink_t(void)      { return sizeof(nlink_t); }
        unsigned long sizeof_off_t(void)        { return sizeof(off_t); }
        unsigned long sizeof_blkcnt_t(void)     { return sizeof(blkcnt_t); }
        unsigned long sizeof_size_t(void)       { return sizeof(size_t); }
        unsigned long sizeof_ssize_t(void)      { return sizeof(ssize_t); }

        /* -- opendir / readdir --------------------------------------------- */

        int count_entries(const char *path) {
            DIR *dir = opendir(path);
            struct dirent *entry;
            int n = 0;
            if (dir == NULL) return -1;
            while ((entry = readdir(dir)) != NULL) {
                if (entry->d_name[0] != '\0') n++;
            }
            closedir(dir);
            return n;
        }

        /* -- pthreads ------------------------------------------------------- */

        static pthread_mutex_t counter_lock = PTHREAD_MUTEX_INITIALIZER;
        static int counter;

        static void *bump(void *arg) {
            int i;
            int times = *(int *) arg;
            for (i = 0; i < times; i++) {
                pthread_mutex_lock(&counter_lock);
                counter++;
                pthread_mutex_unlock(&counter_lock);
            }
            return arg;
        }

        int two_threads_counting(int times) {
            pthread_t a, b;
            void *ra, *rb;
            counter = 0;
            if (pthread_create(&a, NULL, bump, &times) != 0) return -1;
            if (pthread_create(&b, NULL, bump, &times) != 0) return -2;
            if (pthread_join(a, &ra) != 0) return -3;
            if (pthread_join(b, &rb) != 0) return -4;
            if (ra != &times || rb != &times) return -5;
            return counter;
        }

        /* -- socketpair ----------------------------------------------------- */

        int round_trip(const char *message, char *back, unsigned long size) {
            int fds[2];
            long n;
            unsigned long len = strlen(message);
            if (socketpair(AF_UNIX, SOCK_STREAM, 0, fds) != 0) return -1;
            if (write(fds[1], message, len) != (long) len) { close(fds[0]); close(fds[1]); return -2; }
            n = read(fds[0], back, size);
            close(fds[0]);
            close(fds[1]);
            return (int) n;
        }

        /* -- getopt --------------------------------------------------------- */

        static int atoi_local(const char *s) {
            int n = 0;
            while (*s >= '0' && *s <= '9') { n = n * 10 + (*s - '0'); s++; }
            return n;
        }

        int parse_flags(int argc, char **argv, int *value) {
            int seen = 0;
            int c;
            /* getopt keeps state across calls; rewinding it is what lets the
             * same process parse a second argv. */
            optind = 1;
            opterr = 0;
            while ((c = getopt(argc, argv, "ab:")) != -1) {
                if (c == 'a') seen |= 1;
                else if (c == 'b') { seen |= 2; *value = atoi_local(optarg); }
                else return -1;
            }
            return seen;
        }

        /* -- regex ---------------------------------------------------------- */

        int matches(const char *pattern, const char *text) {
            regex_t re;
            int rc;
            if (regcomp(&re, pattern, REG_EXTENDED) != 0) return -1;
            rc = regexec(&re, text, 0, NULL, 0);
            regfree(&re);
            return rc == 0 ? 1 : 0;
        }

        /* -- dlopen / dlsym -------------------------------------------------- */

        int strlen_through_dlsym(const char *text) {
            void *self = dlopen(NULL, RTLD_LAZY);
            unsigned long (*fn)(const char *);
            unsigned long n;
            if (self == NULL) return -1;
            fn = (unsigned long (*)(const char *)) dlsym(self, "strlen");
            if (fn == NULL) { dlclose(self); return -2; }
            n = fn(text);
            dlclose(self);
            return (int) n;
        }

        /* -- gettimeofday ---------------------------------------------------- */

        long seconds_since_epoch(void) {
            struct timeval tv;
            if (gettimeofday(&tv, NULL) != 0) return -1;
            return (long) tv.tv_sec;
        }

        /* -- uname ------------------------------------------------------------ */

        int system_name(char *out, unsigned long size) {
            struct utsname u;
            unsigned long n;
            if (uname(&u) != 0) return -1;
            n = strlen(u.sysname);
            if (n + 1 > size) return -2;
            memcpy(out, u.sysname, n + 1);
            return (int) n;
        }

        /* -- mmap / munmap ----------------------------------------------------- */

        int map_and_touch(unsigned long size) {
            unsigned char *page = (unsigned char *)
                mmap(NULL, size, PROT_READ | PROT_WRITE,
                     MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            int ok;
            if (page == MAP_FAILED) return -1;
            page[0] = 0xa5;
            page[size - 1] = 0x5a;
            ok = page[0] == 0xa5 && page[size - 1] == 0x5a;
            if (munmap(page, size) != 0) return -2;
            return ok;
        }

        /* -- the typedefs both sides declare ----------------------------------- */

        /* `<stdint.h>` is bundled and came first; `<sys/types.h>` is the
         * platform's and declares `int64_t`, `uint32_t` and `size_t` all over
         * again. C11 6.7p3 makes a typedef redefinition with the same type
         * legal (N1360), which is the only reason a unit may mix the two. */
        int64_t widen(int32_t n)    { return (int64_t) n; }
        size_t  as_size(uint32_t n) { return (size_t) n; }
        ssize_t as_signed(off_t n)  { return (ssize_t) n; }
    }

    /// The platform's own `<stdio.h>`, which is what makes `FILE` real.
    ///
    /// A separate unit from the one above: `system_include first` changes what
    /// *every* header in the unit resolves to, and the point of having both is
    /// that the two modes are different.
    mod platform_stdio {
        use cinrs::gnu11;

        gnu11! {
            #pragma cinrs system_include first

            #include <stdio.h>
            #include <string.h>

            /* `FILE` is `struct _IO_FILE` here rather than the opaque tag the
             * bundled header declares, so its size is a number. */
            unsigned long sizeof_file(void) { return sizeof(FILE); }

            int write_and_read_back(const char *path, char *back, int size) {
                FILE *f = fopen(path, "w");
                int written, read_back;
                if (f == NULL) return -1;
                written = fprintf(f, "%s %d", "answer", 42);
                if (fclose(f) != 0) return -2;
                f = fopen(path, "r");
                if (f == NULL) return -3;
                read_back = (int) fread(back, 1, (size_t) size - 1, f);
                back[read_back] = '\0';
                if (fclose(f) != 0) return -4;
                return written == read_back ? written : -5;
            }

            /* The standard streams are `extern FILE *` in the platform's header
             * exactly as in the bundled one; writing nothing to `stderr` keeps
             * the test output clean and still proves the symbol binds. */
            int flush_the_streams(void) {
                return fflush(stderr) == 0 && fflush(stdout) == 0;
            }
        }
    }

    // -----------------------------------------------------------------------
    // the tests
    // -----------------------------------------------------------------------

    fn manifest_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn stat_reads_the_size_the_filesystem_reports() {
        let path = manifest_dir().join("Cargo.toml");
        let wanted = std::fs::metadata(&path).expect("Cargo.toml is there").len();
        let c_path = std::ffi::CString::new(path.to_str().expect("utf-8")).expect("no NUL");
        let size = unsafe { file_size(c_path.as_ptr()) };
        assert_eq!(size as u64, wanted, "st_size");
        assert_eq!(unsafe { is_directory(c_path.as_ptr()) }, 0);

        let dir = std::ffi::CString::new(manifest_dir().to_str().expect("utf-8")).expect("no NUL");
        assert_eq!(unsafe { is_directory(dir.as_ptr()) }, 1);
    }

    #[test]
    fn readdir_lists_a_directory() {
        let dir = manifest_dir().join("tests");
        let wanted = std::fs::read_dir(&dir).expect("tests/ is there").count();
        let c_dir = std::ffi::CString::new(dir.to_str().expect("utf-8")).expect("no NUL");
        let counted = unsafe { count_entries(c_dir.as_ptr()) };
        // `.` and `..` are entries too, and `read_dir` leaves them out.
        assert_eq!(counted, wanted as i32 + 2, "entries in tests/");
    }

    #[test]
    fn two_threads_share_a_mutex() {
        assert_eq!(unsafe { two_threads_counting(5_000) }, 10_000);
    }

    #[test]
    fn a_socketpair_carries_bytes() {
        let message = c"hello, socket";
        let mut back = [0u8; 32];
        let n = unsafe {
            round_trip(
                message.as_ptr(),
                back.as_mut_ptr().cast(),
                back.len() as core::ffi::c_ulong,
            )
        };
        assert_eq!(n, message.count_bytes() as i32);
        assert_eq!(&back[..n as usize], message.to_bytes());
    }

    #[test]
    fn getopt_parses_a_fake_argv() {
        let argv0 = c"prog";
        let a = c"-a";
        let b = c"-b";
        let value = c"17";
        let mut argv = [
            argv0.as_ptr().cast_mut(),
            a.as_ptr().cast_mut(),
            b.as_ptr().cast_mut(),
            value.as_ptr().cast_mut(),
            core::ptr::null_mut(),
        ];
        let mut got = 0;
        let seen = unsafe { parse_flags(4, argv.as_mut_ptr(), &raw mut got) };
        assert_eq!(seen, 3, "both -a and -b were seen");
        assert_eq!(got, 17, "-b's argument");
    }

    #[test]
    fn regcomp_and_regexec_work() {
        let pattern = c"^[a-z]+[0-9]{2}$";
        assert_eq!(unsafe { matches(pattern.as_ptr(), c"abc42".as_ptr()) }, 1);
        assert_eq!(unsafe { matches(pattern.as_ptr(), c"abc4".as_ptr()) }, 0);
    }

    #[test]
    fn dlopen_finds_strlen_in_this_process() {
        let text = c"twelve chars";
        let n = unsafe { strlen_through_dlsym(text.as_ptr()) };
        assert_eq!(n, text.count_bytes() as i32);
    }

    #[test]
    fn gettimeofday_is_after_the_turn_of_the_century() {
        let seconds = unsafe { seconds_since_epoch() };
        assert!(
            seconds > 1_600_000_000,
            "seconds since the epoch: {seconds}"
        );
    }

    #[test]
    fn uname_names_the_system() {
        let mut out = [0u8; 64];
        let n = unsafe { system_name(out.as_mut_ptr().cast(), out.len() as _) };
        assert!(n > 0, "uname failed: {n}");
        assert_eq!(&out[..n as usize], b"Linux");
    }

    #[test]
    fn mmap_gives_writable_pages() {
        assert_eq!(unsafe { map_and_touch(4096) }, 1);
    }

    #[test]
    fn a_bundled_header_and_a_platform_one_agree_about_their_typedefs() {
        assert_eq!(unsafe { widen(-7) }, -7);
        assert_eq!(unsafe { as_size(9) }, 9);
        assert_eq!(unsafe { as_signed(11) }, 11);
    }

    #[test]
    fn the_platform_stdio_gives_a_real_file() {
        let size = unsafe { platform_stdio::sizeof_file() };
        assert!(size > 8, "FILE is a real struct, not an opaque tag: {size}");

        let dir = manifest_dir().join("target/system-headers");
        std::fs::create_dir_all(&dir).expect("target/ must be writable");
        let path = dir.join("stdio-round-trip.txt");
        let c_path = std::ffi::CString::new(path.to_str().expect("utf-8")).expect("no NUL");
        let mut back = [0u8; 32];
        let n = unsafe {
            platform_stdio::write_and_read_back(c_path.as_ptr(), back.as_mut_ptr().cast(), 32)
        };
        assert_eq!(n, 9, "fprintf wrote and fread read the same nine bytes");
        assert_eq!(&back[..9], b"answer 42");
        assert_eq!(unsafe { platform_stdio::flush_the_streams() }, 1);
    }

    // -----------------------------------------------------------------------
    // the differential comparison
    // -----------------------------------------------------------------------

    /// The C expressions both sides are asked to evaluate, in order.
    ///
    /// Each is written once and compiled twice — by `cinrs` in the blocks above
    /// and by `cc` in the probe below — which is the only way to check a layout
    /// that belongs to the C library rather than to the ABI.
    const LAYOUT_QUERIES: &[&str] = &[
        "sizeof(struct stat)",
        "offsetof(struct stat, st_size)",
        "sizeof(pthread_mutex_t)",
        "sizeof(struct dirent)",
        "offsetof(struct dirent, d_name)",
        "sizeof(struct timeval)",
        "sizeof(struct utsname)",
        "sizeof(FILE)",
        "sizeof(dev_t)",
        "sizeof(ino_t)",
        "sizeof(mode_t)",
        "sizeof(nlink_t)",
        "sizeof(off_t)",
        "sizeof(blkcnt_t)",
        "sizeof(size_t)",
        "sizeof(ssize_t)",
    ];

    /// The same numbers as `cinrs` folded them, in the same order.
    fn layout_through_cinrs() -> Vec<u64> {
        unsafe {
            vec![
                sizeof_struct_stat(),
                offsetof_st_size(),
                sizeof_pthread_mutex(),
                sizeof_dirent(),
                offsetof_d_name(),
                sizeof_timeval(),
                sizeof_utsname(),
                platform_stdio::sizeof_file(),
                sizeof_dev_t(),
                sizeof_ino_t(),
                sizeof_mode_t(),
                sizeof_nlink_t(),
                sizeof_off_t(),
                sizeof_blkcnt_t(),
                sizeof_size_t(),
                sizeof_ssize_t(),
            ]
        }
    }

    /// The compiler to compare against, if there is one.
    fn c_compiler() -> Option<String> {
        let named = std::env::var("CINRS_SYSTEM_HEADER_CC")
            .or_else(|_| std::env::var("CC"))
            .unwrap_or_else(|_| "cc".to_owned());
        let ok = Command::new(&named)
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success());
        ok.then_some(named)
    }

    /// The layout the platform's headers really describe, asked of `cc`.
    ///
    /// The only oracle there is: `struct stat` is whatever the C library says
    /// it is, and the whole point of reading the platform's header is to agree
    /// with the compiler that reads the same one.
    #[test]
    fn the_layouts_cinrs_computed_are_the_ones_cc_computes() {
        let Some(cc) = c_compiler() else {
            println!("skipping the layout comparison: no C compiler on PATH");
            return;
        };
        let dir = manifest_dir().join("target/system-headers");
        std::fs::create_dir_all(&dir).expect("target/ must be writable");
        let source = dir.join("layout.c");
        let mut program = String::from(
            "#include <stddef.h>\n\
             #include <stdio.h>\n\
             #include <sys/types.h>\n\
             #include <sys/stat.h>\n\
             #include <sys/time.h>\n\
             #include <sys/utsname.h>\n\
             #include <dirent.h>\n\
             #include <pthread.h>\n\
             int main(void) {\n",
        );
        for expr in LAYOUT_QUERIES {
            program.push_str(&format!(
                "    printf(\"%llu\\n\", (unsigned long long)({expr}));\n"
            ));
        }
        program.push_str("    return 0;\n}\n");
        std::fs::write(&source, &program).expect("target/ must be writable");

        let binary = dir.join("layout");
        let out = Command::new(&cc)
            .args(["-std=gnu11", "-w", "-O0"])
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap_or_else(|e| panic!("could not run {cc}: {e}"));
        assert!(
            out.status.success(),
            "{cc} could not compile the layout probe:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let wanted = run(&binary);
        let got = layout_through_cinrs();
        assert_eq!(wanted.len(), got.len(), "one number per query");
        let mut wrong = Vec::new();
        for (i, name) in LAYOUT_QUERIES.iter().enumerate() {
            if wanted[i] != got[i] {
                wrong.push(format!("  {name}: cinrs {} vs {cc} {}", got[i], wanted[i]));
            }
        }
        assert!(
            wrong.is_empty(),
            "the platform's layouts differ between cinrs and {cc}:\n{}",
            wrong.join("\n")
        );
    }

    fn run(binary: &Path) -> Vec<u64> {
        let out = Command::new(binary)
            .output()
            .unwrap_or_else(|e| panic!("could not run {}: {e}", binary.display()));
        assert!(out.status.success(), "{} failed", binary.display());
        String::from_utf8(out.stdout)
            .expect("numbers are ASCII")
            .lines()
            .map(|line| line.trim().parse().expect("a number per line"))
            .collect()
    }
}

/// Says why the programs above were not compiled, on a target where they
/// cannot be.
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
#[test]
fn the_platform_programs_need_a_glibc_linux() {
    println!(
        "skipping the programs written against the platform's headers: they are compiled at \
         macro expansion time and this target is not a glibc Linux"
    );
}
