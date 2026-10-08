//! `ccinrs` driven as a build would drive it: C files in a directory, the
//! binary run on them, and the program it made run in turn.
//!
//! Every test compiles real C through `rustc`, so each costs a few hundred
//! milliseconds; they are few and each covers one thing a build depends on.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A directory of its own for one test, emptied first.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("ccinrs-cli")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the scratch directory");
        Self { dir }
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the file's directory");
        }
        std::fs::write(path, text).expect("the file");
    }

    /// Runs `ccinrs` in the directory, with a runtime cache of the tests' own.
    fn ccinrs(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ccinrs"))
            .args(args)
            .env("CCINRS_CACHE_DIR", cache_dir())
            .current_dir(&self.dir)
            .output()
            .expect("ccinrs runs")
    }

    /// Runs `ccinrs` and asserts that it succeeded.
    #[track_caller]
    fn compile(&self, args: &[&str]) {
        let out = self.ccinrs(args);
        assert!(
            out.status.success(),
            "ccinrs {args:?} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Runs a program the directory holds.
    fn run(&self, program: &str, args: &[&str]) -> Output {
        Command::new(self.dir.join(program))
            .args(args)
            .current_dir(&self.dir)
            .output()
            .expect("the program runs")
    }
}

/// Where the tests' `ccinrs` keeps the runtime it compiles.
fn cache_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("ccinrs-cache")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_program_builds_and_runs_with_its_arguments_and_exit_status() {
    let s = Scratch::new("hello");
    s.write(
        "hello.c",
        r#"#include <stdio.h>
#include <stdlib.h>
int main(int argc, char **argv) {
    printf("hello, %s (%d)\n", argc > 1 ? argv[1] : "nobody", argc);
    return argc > 2 ? atoi(argv[2]) : 0;
}
"#,
    );
    s.compile(&["-O2", "hello.c", "-o", "hello"]);
    let out = s.run("hello", &["world", "42"]);
    assert_eq!(stdout(&out), "hello, world (3)\n");
    assert_eq!(out.status.code(), Some(42));
    // Without -o, GCC's name.
    s.compile(&["hello.c"]);
    assert!(
        s.dir
            .join(if cfg!(windows) { "a.exe" } else { "a.out" })
            .is_file()
    );
}

/// Two translation units: a function and an object with external linkage
/// cross between them, and a `static` of the same name in each stays private.
#[test]
fn translation_units_link_the_way_c_links_them() {
    let s = Scratch::new("units");
    s.write(
        "counter.c",
        r#"int counter = 10;
static int helper(int x) { return x * 2; }
int bump(int by) { counter += helper(by); return counter; }
"#,
    );
    s.write(
        "main.c",
        r#"#include <stdio.h>
extern int counter;
int bump(int by);
static int helper(int x) { return x + 1000; }
int main(void) {
    int a = bump(1);
    int b = bump(5);
    printf("%d %d %d %d\n", a, b, counter, helper(1));
    return 0;
}
"#,
    );
    s.compile(&["counter.c", "main.c", "-o", "units"]);
    assert_eq!(stdout(&s.run("units", &[])), "12 22 22 1001\n");
}

/// `setjmp` in one file, `longjmp` in another, and the C library's `qsort` in
/// between: every function is `extern "C-unwind"`, and glibc's frames have
/// unwind tables. The platform's `<setjmp.h>` is the one read.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn setjmp_and_longjmp_across_files_and_the_c_library() {
    let s = Scratch::new("setjmp");
    s.write(
        "sort.c",
        r#"#include <setjmp.h>
#include <stdlib.h>
static jmp_buf *target;
static int compares;
static int cmp(const void *a, const void *b) {
    if (++compares == 10) longjmp(*target, compares);
    return *(const int *)a - *(const int *)b;
}
void sort_or_jump(jmp_buf *env, int *v, int n) {
    target = env;
    qsort(v, n, sizeof *v, cmp);
}
"#,
    );
    s.write(
        "main.c",
        r#"#include <setjmp.h>
#include <stdio.h>
void sort_or_jump(jmp_buf *env, int *v, int n);
int main(void) {
    int v[50];
    jmp_buf env;
    for (int i = 0; i < 50; i++) v[i] = (i * 7) % 50;
    int r = setjmp(env);
    if (r == 0) {
        sort_or_jump(&env, v, 50);
        printf("sorted\n");
        return 1;
    }
    printf("came back with %d\n", r);
    return 0;
}
"#,
    );
    s.compile(&["-O2", "sort.c", "main.c", "-o", "jump"]);
    let out = s.run("jump", &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "came back with 10\n");
}

/// A `longjmp` to a function that has returned is undefined in C; here the
/// program stops with a message instead of unwinding out of `main`.
#[cfg(unix)]
#[test]
fn a_longjmp_to_a_returned_function_aborts() {
    let s = Scratch::new("setjmp-dead");
    s.write(
        "dead.c",
        r#"#include <setjmp.h>
static jmp_buf env;
static int arm(void) { if (setjmp(env)) return 1; return 0; }
int main(void) { arm(); longjmp(env, 1); }
"#,
    );
    s.compile(&["dead.c", "-o", "dead"]);
    let out = s.run("dead", &[]);
    assert_aborted(&out);
    assert!(
        stderr(&out).starts_with(
            "cinrs: longjmp to a jmp_buf whose setjmp is not active on this thread: the \
             function that called setjmp has returned, or it ran on another thread"
        ),
        "{}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("doc/limitations.md#setjmp-and-longjmp"),
        "{}",
        stderr(&out)
    );
}

/// A `longjmp` out of a signal handler that interrupted the program's own
/// code has no call to unwind from; it stops with a message that says so,
/// before trying. One out of a handler of a signal raised inside a library
/// call works.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn a_longjmp_out_of_an_asynchronous_signal_handler_aborts_with_a_message() {
    let s = Scratch::new("setjmp-signal");
    s.write(
        "sig.c",
        r#"#include <setjmp.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/time.h>
static sigjmp_buf env;
static void handler(int sig) { siglongjmp(env, sig); }
static volatile unsigned long spin;
int main(int argc, char **argv) {
    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_handler = handler;
    sigaction(SIGUSR1, &sa, NULL);
    sigaction(SIGALRM, &sa, NULL);
    int r = sigsetjmp(env, 1);
    if (r == 0 && argc > 1) {
        struct itimerval t = { { 0, 0 }, { 0, 2000 } };
        setitimer(ITIMER_REAL, &t, NULL);
        for (;;) spin = spin * 1103515245u + 12345u;
    }
    if (r == 0) raise(SIGUSR1);
    printf("back with %d\n", r);
    return 0;
}
"#,
    );
    s.compile(&["-O2", "sig.c", "-o", "sig"]);
    let out = s.run("sig", &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "back with 10\n");
    let out = s.run("sig", &["alarm"]);
    assert_aborted(&out);
    assert!(
        stderr(&out).starts_with(
            "cinrs: longjmp out of a signal handler that interrupted the program's own code \
             (an asynchronous signal"
        ),
        "{}",
        stderr(&out)
    );
    assert!(!stderr(&out).contains("failed to initiate panic"));
}

/// `-fno-cinrs-unwind` makes every function `extern "C"`, so `setjmp` and
/// `longjmp` are refused where they are written, naming the option. (glibc's
/// `setjmp` is a macro over `_setjmp`, which is the name the error gives.)
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn no_cinrs_unwind_refuses_setjmp() {
    let s = Scratch::new("setjmp-nounwind");
    s.write(
        "jump.c",
        "#include <setjmp.h>\nstatic jmp_buf env;\nint main(void) {\n    if (setjmp(env)) return 0;\n    longjmp(env, 1);\n}\n",
    );
    let out = s.ccinrs(&["-fno-cinrs-unwind", "jump.c", "-o", "jump"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out)
            .contains("jump.c:4:9: error: '_setjmp' is not available under `-fno-cinrs-unwind`"),
        "{}",
        stderr(&out)
    );
    assert!(
        stderr(&out).contains("jump.c:5:5: error: 'longjmp' is not available"),
        "{}",
        stderr(&out)
    );
    s.write("plain.c", "int main(void) { return 0; }\n");
    s.compile(&["-fno-cinrs-unwind", "plain.c", "-o", "plain"]);
    assert!(s.run("plain", &[]).status.success());
}

/// GCC's `-std=c11` differs from `-std=gnu11` in keywords, `__STRICT_ANSI__`
/// and trigraphs, and warns about the constraint violations `gnu11` takes;
/// only `-pedantic-errors` refuses them. So does every `-std=` here: CPython
/// builds with `-std=c11` past a stray `;` and glibc's `<sys/epoll.h>`, whose
/// `EPOLLET` is `1u << 31` in an `enum`, and FFmpeg with `-std=c17` past a
/// `;` after a function and an `#embed`. What makes the dialect ISO stays:
/// CPython's `Py_ARRAY_LENGTH` takes its plain, constant form because
/// `__STRICT_ANSI__` is defined, and its GNU form is no constant under
/// `-std=gnu11` here as in GCC. A diagnostic that says how to have a
/// leniency names the command line, not the macro a `c11!` block is written
/// with.
#[cfg(target_os = "linux")]
#[test]
fn an_iso_std_takes_what_gcc_takes_unless_pedantic_errors() {
    let s = Scratch::new("strict-std");
    s.write(
        "lenient.c",
        "#include <sys/epoll.h>\nint something(void);\n\
         int twice(int x) { return 2 * x; };\n\
         void forwards(void) { return something(); }\n\
         int main(void) { return twice(EPOLLIN) != 2; }\n",
    );
    for std in ["-std=c89", "-std=c99", "-std=c11", "-std=c17"] {
        s.compile(&[std, "-c", "lenient.c", "-o", "lenient.o"]);
    }
    let out = s.ccinrs(&["-std=c11", "-pedantic-errors", "-c", "lenient.c"]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("expected a declaration, found ';'"), "{err}");
    assert!(err.contains("should not return a value"), "{err}");
    assert!(
        err.contains(
            "GCC accepts this with a warning, and so does ccinrs without -pedantic-errors or \
             with -std=gnu11"
        ),
        "{err}"
    );
    assert!(!err.contains("gnu11!"), "{err}");
    // A GNU extension that is no leniency is the dialect's still.
    s.write(
        "suffix.c",
        "int main(void) { long x = 1.0q > 0; return (int) x; }\n",
    );
    let out = s.ccinrs(&["-std=c17", "-c", "suffix.c"]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(
        err.contains("requires a GNU dialect (-std=gnu17) (this file is compiled with -std=c17)"),
        "{err}"
    );
    // `#embed` and `__has_embed` follow the leniencies.
    s.write(
        "embed.c",
        "#include <stdio.h>\n#ifdef __has_embed\nstatic const char data[] = {\n#embed \"data.bin\" suffix(, 0)\n};\n\
         #else\nstatic const char data[] = \"no embed\";\n#endif\nint main(void) { puts(data); return 0; }\n",
    );
    s.write("data.bin", "embedded");
    s.compile(&["-std=c17", "embed.c", "-o", "c17"]);
    assert_eq!(stdout(&s.run("c17", &[])), "embedded\n");
    s.compile(&["-std=c17", "-pedantic-errors", "embed.c", "-o", "pedantic"]);
    assert_eq!(stdout(&s.run("pedantic", &[])), "no embed\n");
    s.compile(&["-std=gnu17", "embed.c", "-o", "gnu17"]);
    assert_eq!(stdout(&s.run("gnu17", &[])), "embedded\n");
    // CPython's faulthandler.c.
    s.write(
        "length.c",
        "#include <stddef.h>\n\
         #define Py_BUILD_ASSERT_EXPR(cond) \\\n    ((void)sizeof(struct { int dummy; _Static_assert(cond, #cond); }), 0)\n\
         #if defined(__GNUC__) && !defined(__STRICT_ANSI__)\n\
         #define Py_ARRAY_LENGTH(array) (sizeof(array) / sizeof((array)[0]) \\\n    \
         + Py_BUILD_ASSERT_EXPR(!__builtin_types_compatible_p(typeof(array), typeof(&(array)[0]))))\n\
         #else\n#define Py_ARRAY_LENGTH(array) (sizeof(array) / sizeof((array)[0]))\n#endif\n\
         static const int handlers[] = { 1, 2, 3 };\n\
         static const size_t nsignals = Py_ARRAY_LENGTH(handlers);\n\
         int main(void) { return (int) nsignals - 3; }\n",
    );
    s.compile(&["-std=c11", "length.c", "-o", "length"]);
    assert_eq!(s.run("length", &[]).status.code(), Some(0));
    let out = s.ccinrs(&["-std=gnu11", "-c", "length.c"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("is not a compile-time constant expression"),
        "{}",
        stderr(&out)
    );
}

/// `__DATE__` and `__TIME__` are the moment of translation, as in GCC:
/// `SOURCE_DATE_EPOCH` in UTC when it is set, and GCC's error where they are
/// used when it is no moment. `__TIMESTAMP__` is the file's modification
/// time, in the local zone `TZ` names, whatever `SOURCE_DATE_EPOCH` says.
#[test]
fn the_date_and_time_follow_source_date_epoch() {
    let s = Scratch::new("date-time");
    s.write(
        "when.c",
        "#include <stdio.h>\nint main(void) { printf(\"[%s] [%s] [%s]\\n\", __DATE__, __TIME__, \
         __TIMESTAMP__); return 0; }\n",
    );
    // Midsummer 2026, 12:34:56 UTC.
    let summer = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_782_995_696);
    std::fs::File::options()
        .write(true)
        .open(s.dir.join("when.c"))
        .and_then(|file| file.set_modified(summer))
        .expect("the file's modification time");
    let compile = |epoch: Option<&str>, tz: &str, out: &str| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ccinrs"));
        command
            .args(["when.c", "-o", out])
            .env("CCINRS_CACHE_DIR", cache_dir())
            .env("TZ", tz)
            .current_dir(&s.dir);
        match epoch {
            Some(epoch) => command.env("SOURCE_DATE_EPOCH", epoch),
            None => command.env_remove("SOURCE_DATE_EPOCH"),
        };
        command.output().expect("ccinrs runs")
    };
    let out = compile(Some("0"), "UTC", "utc");
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&s.run("utc", &[])),
        "[Jan  1 1970] [00:00:00] [Thu Jul  2 12:34:56 2026]\n"
    );
    // The epoch is UTC whatever the zone; the file's time is local.
    let out = compile(Some("86399"), "EST5EDT,M3.2.0,M11.1.0", "eastern");
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&s.run("eastern", &[])),
        "[Jan  1 1970] [23:59:59] [Thu Jul  2 08:34:56 2026]\n"
    );
    // GCC's error, once, at the first use.
    let out = compile(Some("1e3"), "UTC", "bad");
    assert!(!out.status.success());
    let err = stderr(&out);
    let message = "error: environment variable 'SOURCE_DATE_EPOCH' must expand to a \
                   non-negative integer less than or equal to 253402300799";
    assert!(err.contains(&format!("when.c:2:45: {message}")), "{err}");
    assert_eq!(err.matches(message).count(), 1, "{err}");
    // Without it, now: only the shape can be checked.
    let out = compile(None, "JST-9", "now");
    assert!(out.status.success(), "{}", stderr(&out));
    let now = stdout(&s.run("now", &[]));
    let shape = "[Oct  8 2026] [17:45:37] [Thu Jul  2 21:34:56 2026]\n";
    assert_eq!(now.len(), shape.len(), "{now}");
    assert!(now.ends_with("] [Thu Jul  2 21:34:56 2026]\n"), "{now}");
}

/// What ccinrs links is what it compiled, so a function the program declares
/// in its own header is another of its files, whose `long double` is the
/// `double` this one passes — Redis's `ld2string` — while one the platform's
/// headers declare keeps the x87 `long double` and is refused.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn a_programs_own_long_double_function_is_called() {
    let s = Scratch::new("own-long-double");
    s.write(
        "half.h",
        "long double half(long double x);\nlong double sum(int n, ...);\n",
    );
    s.write(
        "half.c",
        "#include <stdarg.h>\n#include \"half.h\"\n\
         long double half(long double x) { return x / 2; }\n\
         long double sum(int n, ...) {\n\
         \x20   va_list ap; long double s = 0;\n\
         \x20   va_start(ap, n);\n\
         \x20   for (int i = 0; i < n; i++) s += va_arg(ap, long double);\n\
         \x20   va_end(ap);\n\
         \x20   return s;\n\
         }\n",
    );
    s.write(
        "main.c",
        "#include <stdio.h>\n#include \"half.h\"\n\
         int main(void) { printf(\"%.3f %.1f\\n\", (double)half(3.0L), (double)sum(2, 1.5L, 2.0L)); return 0; }\n",
    );
    s.compile(&["main.c", "half.c", "-o", "half"]);
    assert_eq!(stdout(&s.run("half", &[])), "1.500 3.5\n");
    s.write(
        "platform.c",
        "#define _GNU_SOURCE\n#include <stdlib.h>\n\
         char *f(long double x, int *d, int *s) { return qfcvt(x, 2, d, s); }\n",
    );
    let out = s.ccinrs(&["-c", "platform.c"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("'qfcvt' takes a 'long double' (which is 'double' here)"),
        "{}",
        stderr(&out)
    );
}

/// A thread-local object with external linkage, defined in one file and
/// declared `extern` in another: the defining object exports the accessor
/// `counter.cinrs_tls`, and each thread sees its own copy through it. A
/// missing definition is an undefined accessor, named after the object.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn an_extern_thread_local_object_is_reached_through_its_accessor() {
    let s = Scratch::new("extern-tls");
    s.write(
        "def.c",
        "__thread int counter = 0;\nint bump(int by) { counter += by; return counter; }\n",
    );
    s.write(
        "use.c",
        "#include <pthread.h>\n#include <stdio.h>\n\
         extern __thread int counter;\nint bump(int by);\n\
         static void *worker(void *arg) {\n\
         \x20   counter = (int)(long)arg;\n\
         \x20   for (int i = 0; i < 100; i++) bump(1);\n\
         \x20   return (void *)(long)counter;\n\
         }\n\
         int main(void) {\n\
         \x20   pthread_t a, b; void *ra, *rb;\n\
         \x20   pthread_create(&a, NULL, worker, (void *)10L);\n\
         \x20   pthread_create(&b, NULL, worker, (void *)20L);\n\
         \x20   pthread_join(a, &ra); pthread_join(b, &rb);\n\
         \x20   bump(42);\n\
         \x20   printf(\"%ld %ld %d\\n\", (long)ra, (long)rb, counter);\n\
         \x20   return 0;\n\
         }\n",
    );
    s.compile(&["-pthread", "use.c", "def.c", "-o", "tls"]);
    assert_eq!(stdout(&s.run("tls", &[])), "110 120 42\n");
    let out = s.ccinrs(&["-pthread", "use.c", "-o", "missing"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("undefined symbol: counter.cinrs_tls"),
        "{}",
        stderr(&out)
    );
}

/// C99 6.7.4p7: an `inline` definition in a header two files include is an
/// *inline definition*, which provides no external definition; the file that
/// redeclares it `extern inline` provides the one external definition, and
/// the program links. Each file's own calls use its own copy, and the
/// function's address is the one symbol in every file. Reported in #1.
#[test]
fn a_c99_inline_definition_is_no_external_definition() {
    let s = Scratch::new("inline-c99");
    s.write("inl.h", "inline int f(int x) { return x + 1; }\n");
    s.write(
        "u1.c",
        "#include \"inl.h\"\nint u1(int x) { return f(x); }\nvoid *a1(void) { return (void *) &f; }\n",
    );
    s.write(
        "u2.c",
        "#include \"inl.h\"\nint u2(int x) { return f(x) * 2; }\nvoid *a2(void) { return (void *) f; }\n",
    );
    s.write(
        "main.c",
        "#include \"inl.h\"\nextern inline int f(int);\nint u1(int), u2(int);\n\
         void *a1(void), *a2(void);\n\
         int main(void) { return u1(1) + u2(1) == 6 && a1() == (void *) f && a2() == (void *) f \
         ? 0 : 1; }\n",
    );
    for std in ["-std=c99", "-std=c17", "-std=gnu11"] {
        s.compile(&[std, "u1.c", "u2.c", "main.c", "-o", "inline"]);
        assert_eq!(s.run("inline", &[]).status.code(), Some(0), "{std}");
    }
    // A plain `int f(int);` declaration makes the definition an external
    // one, and so does the absence of `inline` on any declaration.
    s.write(
        "ext.c",
        "inline int g(int x) { return x * 3; }\nint g(int);\n\
         int main(void) { return g(2) == 6 ? 0 : 1; }\n",
    );
    s.compile(&["-std=c11", "ext.c", "-o", "ext"]);
    assert_eq!(s.run("ext", &[]).status.code(), Some(0));
}

/// GNU89's rules, the other way round: an `extern inline` definition is for
/// inlining only, and a plain `inline` one is an external definition. They
/// are `-std=gnu89`'s, `-fgnu89-inline`'s, and those of any function a
/// declaration of which says `__attribute__((gnu_inline))` — glibc's
/// `__extern_inline` and macOS's `__header_inline`, which a header two files
/// include defines. Reported in #1.
#[test]
fn a_gnu89_extern_inline_definition_is_no_external_definition() {
    let s = Scratch::new("inline-gnu89");
    s.write(
        "u1.c",
        "extern inline int g(int x) { return x + 1; }\nint u1(int x) { return g(x); }\n",
    );
    s.write("def.c", "int g(int x) { return x + 1; }\n");
    s.write(
        "main.c",
        "int u1(int);\nint main(void) { return u1(1) == 2 ? 0 : 1; }\n",
    );
    s.compile(&["-std=gnu89", "u1.c", "def.c", "main.c", "-o", "gnu89"]);
    assert_eq!(s.run("gnu89", &[]).status.code(), Some(0));
    s.compile(&[
        "-std=c17",
        "-fgnu89-inline",
        "u1.c",
        "def.c",
        "main.c",
        "-o",
        "flag",
    ]);
    assert_eq!(s.run("flag", &[]).status.code(), Some(0));
    // `__GNUC_GNU_INLINE__` follows the choice.
    s.write(
        "which.c",
        "#if defined(__GNUC_GNU_INLINE__) && !defined(__GNUC_STDC_INLINE__)\nint gnu = 1;\n#else\n\
         int gnu = 0;\n#endif\nint main(void) { return gnu; }\n",
    );
    for (args, want) in [
        (&["-std=gnu89"][..], 1),
        (&["-std=gnu89", "-fno-gnu89-inline"][..], 0),
        (&["-std=c17"][..], 0),
        (&["-std=c17", "-fgnu89-inline"][..], 1),
    ] {
        let mut all = args.to_vec();
        all.extend(["which.c", "-o", "which"]);
        s.compile(&all);
        assert_eq!(s.run("which", &[]).status.code(), Some(want), "{args:?}");
    }
    // `gnu_inline` under `-std=c17`, glibc's way: an inline-only
    // definition in a header two files include, and the external definition
    // in a third. A plain `inline` one under `gnu_inline` is an external
    // definition, and is the program's one.
    s.write(
        "fast.h",
        "#define __extern_inline extern __inline __attribute__((__gnu_inline__))\n\
         __extern_inline int h(int x) { return x + 10; }\n",
    );
    s.write(
        "v1.c",
        "#include \"fast.h\"\nint v1(int x) { return h(x); }\n",
    );
    s.write(
        "v2.c",
        "int h(int);\nint k(int);\nint v2(int x) { return h(x) + k(x); }\n",
    );
    s.write("hdef.c", "int h(int x) { return x + 10; }\n");
    s.write(
        "kdef.c",
        "#include \"fast.h\"\nint v3(int x) { return h(x) * 2; }\n\
         __inline __attribute__((__gnu_inline__)) int k(int x) { return x - 1; }\n",
    );
    s.write(
        "vmain.c",
        "int v1(int), v2(int), v3(int);\n\
         int main(void) { return v1(1) == 11 && v2(1) == 11 && v3(1) == 22 ? 0 : 1; }\n",
    );
    s.compile(&[
        "-std=c17", "v1.c", "v2.c", "hdef.c", "kdef.c", "vmain.c", "-o", "glibc",
    ]);
    assert_eq!(s.run("glibc", &[]).status.code(), Some(0));
}

/// A weak definition is a real one on ELF: a function and an object another
/// file overrides — reached through the symbol by the defining file's own
/// callers too — the default when nothing does, jemalloc's weak tentative
/// `malloc_conf` overridden by a test's definition, and a shared library's
/// weak symbol interposed by the program. Under `-flto` the file is compiled
/// without it where the linker optimises, and falls back to an ordinary
/// definition, with the warning, where `rustc` does.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn a_weak_definition_is_overridden_by_a_strong_one() {
    let s = Scratch::new("weak-definition");
    s.write(
        "fdef.c",
        "#include <stdio.h>\n\
         __attribute__((weak)) int hook(int x) { return x + 1; }\n\
         __attribute__((weak)) int level = 3;\n\
         const char *conf __attribute__((weak));\n\
         int call_hook(int x) { return hook(x); }\n\
         int (*hook_address(void))(int) { return hook; }\n\
         void report(void) { printf(\"%d %d %s\\n\", call_hook(1), level, conf ? conf : \"(null)\"); }\n",
    );
    s.write(
        "override.c",
        "int hook(int x) { return x + 100; }\nint level = 30;\nconst char *conf = \"override\";\n",
    );
    s.write(
        "main.c",
        "#include <stdio.h>\nvoid report(void);\nint hook(int);\nint (*hook_address(void))(int);\n\
         int main(void) { report(); printf(\"%d\\n\", hook_address() == hook); return 0; }\n",
    );
    s.compile(&["-O2", "main.c", "fdef.c", "-o", "alone"]);
    assert_eq!(stdout(&s.run("alone", &[])), "2 3 (null)\n1\n");
    s.compile(&["-O2", "main.c", "fdef.c", "override.c", "-o", "overridden"]);
    assert_eq!(stdout(&s.run("overridden", &[])), "101 30 override\n1\n");
    // The symbols are weak and defined, as GCC makes them.
    s.compile(&["-c", "fdef.c", "-o", "fdef.o"]);
    let nm = Command::new("nm").arg(s.dir.join("fdef.o")).output();
    if let Ok(nm) = nm {
        let table = stdout(&nm);
        assert!(table.contains(" W hook\n"), "{table}");
        assert!(table.contains(" V level\n"), "{table}");
    }
    // A shared library's weak definitions, which the program interposes.
    s.compile(&["-O2", "-shared", "-fPIC", "fdef.c", "-o", "libfdef.so"]);
    let rpath = format!("-Wl,-rpath,{}", s.dir.display());
    s.compile(&[
        "-O2",
        "main.c",
        "override.c",
        "-L.",
        "-lfdef",
        &rpath,
        "-o",
        "shared",
    ]);
    assert_eq!(stdout(&s.run("shared", &[])), "101 30 override\n1\n");
    // Under `-flto`, where the linker does the optimisation (x86-64 Linux),
    // the file with the weak definitions is compiled without it and linked
    // beside the others, and says so under `-v` only. Where `rustc` does, it
    // merges the files before they are assembled, so the definition is an
    // ordinary one, with the warning.
    let lto = s.ccinrs(&["-O2", "-flto", "-v", "main.c", "fdef.c", "-o", "lto"]);
    assert!(lto.status.success(), "{}", stderr(&lto));
    if cfg!(target_arch = "x86_64") {
        assert!(
            stderr(&lto)
                .contains("fdef.c makes a weak definition, so it is compiled without -flto"),
            "{}",
            stderr(&lto)
        );
        let quiet = s.ccinrs(&[
            "-O2",
            "-flto",
            "main.c",
            "fdef.c",
            "override.c",
            "-o",
            "lto-overridden",
        ]);
        assert!(quiet.status.success(), "{}", stderr(&quiet));
        assert!(!stderr(&quiet).contains("weak"), "{}", stderr(&quiet));
        assert_eq!(
            stdout(&s.run("lto-overridden", &[])),
            "101 30 override\n1\n"
        );
    } else {
        assert!(
            stderr(&lto).contains("'hook' is defined weakly, which Rust cannot express"),
            "{}",
            stderr(&lto)
        );
    }
    assert_eq!(stdout(&s.run("lto", &[])), "2 3 (null)\n1\n");
}

/// `__attribute__((common))` on a tentative definition in a header that two
/// files include, as Redis's `redismodule.h` declares its API's pointers: one
/// object, which the linker merges, as it does a weak definition.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn a_common_definition_in_two_files_is_one_object() {
    let s = Scratch::new("common-definition");
    s.write(
        "api.h",
        "extern int (*api_hook)(int) __attribute__((common));\n\
         int (*api_hook)(int) __attribute__((common));\n\
         int api_level __attribute__((__common__));\n",
    );
    s.write(
        "one.c",
        "#include \"api.h\"\nstatic int twice(int x) { return 2 * x; }\n\
         void install(void) { api_hook = twice; api_level = 7; }\n",
    );
    s.write(
        "main.c",
        "#include <stdio.h>\n#include \"api.h\"\nvoid install(void);\n\
         int main(void) { install(); printf(\"%d %d\\n\", api_hook(21), api_level); return 0; }\n",
    );
    s.compile(&["-O2", "main.c", "one.c", "-o", "common"]);
    assert_eq!(stdout(&s.run("common", &[])), "42 7\n");
    // Redis's build does it under `-flto`, which keeps the definitions where
    // the linker does the optimisation.
    if cfg!(target_arch = "x86_64") {
        s.compile(&["-O2", "-flto", "main.c", "one.c", "-o", "common-lto"]);
        assert_eq!(stdout(&s.run("common-lto", &[])), "42 7\n");
    }
}

/// The x86-64 psABI gives an array variable of 16 bytes or more 16-byte
/// alignment, and code compiled by another compiler relies on it for an array
/// `ccinrs` defined: here `_mm_load_ps`, an aligned SSE load, on the extern
/// array, from an object `cc` compiled. A local and a variable length array
/// have it too, and `__alignof__` still answers the type's.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn an_array_variable_is_aligned_as_the_psabi_says() {
    let s = Scratch::new("psabi-arrays");
    s.write(
        "tables.c",
        "#include <stdint.h>\n\
         char pad = 1;\n\
         float weights[4] = { 1, 2, 3, 4 };\n\
         char tail = 2;\n\
         double more[2] = { 5, 6 };\n\
         int locals_aligned(int n) {\n\
           char local[16]; short vla[n]; local[0] = (char)n; vla[0] = 0;\n\
           return ((uintptr_t)local & 15) == 0 && ((uintptr_t)vla & 15) == 0\n\
             && ((uintptr_t)weights & 15) == 0 && ((uintptr_t)more & 15) == 0\n\
             && __alignof__(weights) == 4 && _Alignof(float[4]) == 4 && sizeof weights == 16\n\
             && local[0] == (char)n && vla[0] == 0;\n\
         }\n",
    );
    s.write(
        "reader.c",
        "#include <xmmintrin.h>\n\
         extern float weights[4];\n\
         float total(void) {\n\
           float out[4];\n\
           _mm_storeu_ps(out, _mm_load_ps(weights));\n\
           return out[0] + out[1] + out[2] + out[3];\n\
         }\n",
    );
    s.write(
        "main.c",
        "#include <stdio.h>\nfloat total(void);\nint locals_aligned(int);\n\
         int main(void) { printf(\"%g %d\\n\", total(), locals_aligned(5)); return 0; }\n",
    );
    let cc = Command::new("cc")
        .args(["-O2", "-c", "reader.c", "-o", "reader.o"])
        .current_dir(&s.dir)
        .status();
    if !cc.is_ok_and(|status| status.success()) {
        return;
    }
    s.compile(&["-O2", "main.c", "tables.c", "reader.o", "-o", "aligned"]);
    assert_eq!(stdout(&s.run("aligned", &[])), "10 1\n");
}

/// GCC's vector extensions: a vector type's alignment follows `-mavx` as
/// GCC's does, glibc's `<link.h>` — whose vectors an `aligned` lowers to 16
/// bytes — compiles, and a vector passed by value to a function of another
/// file is refused with the reason.
#[cfg(all(target_os = "linux", target_env = "gnu", target_arch = "x86_64"))]
#[test]
fn gcc_vectors_follow_the_target_and_the_platform_headers() {
    let s = Scratch::new("gcc-vectors");
    s.write(
        "align.c",
        "#define _GNU_SOURCE\n#include <link.h>\n#include <stdio.h>\n\
         typedef float v8sf __attribute__((vector_size(32)));\n\
         typedef unsigned long long v2du __attribute__((vector_size(16)));\n\
         int main(void) {\n\
           v2du a = {1, 2}, b = {3, 4}, c = a ^ b;\n\
           printf(\"%zu %zu %zu %llu %llu\\n\", _Alignof(v8sf), _Alignof(La_x86_64_ymm),\n\
                  sizeof(La_x86_64_zmm), c[0], c[1]);\n\
           return 0;\n\
         }\n",
    );
    s.compile(&["-O2", "align.c", "-o", "plain"]);
    assert_eq!(stdout(&s.run("plain", &[])), "16 16 64 2 6\n");
    s.compile(&["-O2", "-mavx", "align.c", "-o", "avx"]);
    assert_eq!(stdout(&s.run("avx", &[])), "32 16 64 2 6\n");
    s.write(
        "call.c",
        "typedef int v4si __attribute__((vector_size(16)));\n\
         v4si twice(v4si);\n\
         v4si call(v4si a) { return twice(a); }\n",
    );
    let refused = s.ccinrs(&["-c", "call.c", "-o", "call.o"]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("GCC passes a vector in a vector register"),
        "{}",
        stderr(&refused)
    );
}

/// GCC's `-ftrivial-auto-var-init=`: `zero`, the default, clears a local
/// array nothing initialised; `uninitialized` leaves it a `MaybeUninit`, and
/// the program that writes before it reads behaves the same; `pattern` is
/// taken as `zero` with a warning; anything else is GCC's error.
#[test]
fn trivial_auto_var_init_chooses_what_locals_start_as() {
    let s = Scratch::new("auto-var-init");
    s.write(
        "locals.c",
        "#include <stdio.h>\n\
         static void fill(int *p, int n) { for (int i = 0; i < n; i++) p[i] = i; }\n\
         int main(void) {\n\
         \x20   int buf[256];\n\
         \x20   fill(buf, 10);\n\
         \x20   printf(\"%d\\n\", buf[9]);\n\
         \x20   return 0;\n\
         }\n",
    );
    for (flag, maybe_uninit) in [
        ("-ftrivial-auto-var-init=zero", false),
        ("-ftrivial-auto-var-init=uninitialized", true),
    ] {
        s.compile(&[flag, "locals.c", "-o", "locals"]);
        let out = s.run("locals", &[]);
        assert!(out.status.success(), "{flag}: {}", stderr(&out));
        assert_eq!(stdout(&out), "9\n", "{flag}");
        let rust = s.ccinrs(&[flag, "-S", "locals.c", "-o", "locals.rs"]);
        assert!(rust.status.success(), "{flag}: {}", stderr(&rust));
        let text = std::fs::read_to_string(s.dir.join("locals.rs")).expect("the Rust");
        assert_eq!(
            text.contains("MaybeUninit"),
            maybe_uninit,
            "{flag}:\n{text}"
        );
    }
    let pattern = s.ccinrs(&[
        "-ftrivial-auto-var-init=pattern",
        "-c",
        "locals.c",
        "-o",
        "p.o",
    ]);
    assert!(pattern.status.success(), "{}", stderr(&pattern));
    assert!(
        stderr(&pattern).contains(
            "warning: '-ftrivial-auto-var-init=pattern' is taken as '-ftrivial-auto-var-init=zero'"
        ),
        "{}",
        stderr(&pattern)
    );
    let bogus = s.ccinrs(&["-ftrivial-auto-var-init=maybe", "-c", "locals.c"]);
    assert!(!bogus.status.success());
    assert!(
        stderr(&bogus).contains(
            "unrecognized argument in option '-ftrivial-auto-var-init=maybe'; valid arguments \
             to '-ftrivial-auto-var-init=' are: pattern uninitialized zero"
        ),
        "{}",
        stderr(&bogus)
    );
}

/// The program stopped by `abort`, as a failed run-time check or a `longjmp`
/// that cannot be done stops it.
#[cfg(unix)]
#[track_caller]
fn assert_aborted(out: &Output) {
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(out.status.signal(), Some(6), "{}", stderr(out));
}

/// A `volatile` object is read from memory every time: the flag a signal
/// handler sets ends the loop that waits for it. With the read hoisted out of
/// the loop, as `-O2` did while a `volatile` access was an ordinary one, the
/// loop spins for ever, so the program is given ten seconds rather than
/// being waited for.
#[cfg(unix)]
#[test]
fn a_volatile_flag_a_signal_handler_sets_ends_the_loop() {
    use std::io::Read;
    use std::time::{Duration, Instant};
    let s = Scratch::new("volatile-signal");
    s.write(
        "alarm.c",
        r#"#include <signal.h>
#include <stdio.h>
#include <sys/time.h>
static volatile sig_atomic_t done;
static void on_alarm(int sig) { (void)sig; done = 1; }
int main(void) {
    struct itimerval t = { { 0, 0 }, { 0, 20000 } };
    signal(SIGALRM, on_alarm);
    setitimer(ITIMER_REAL, &t, 0);
    while (!done)
        ;
    puts("done");
    return 0;
}
"#,
    );
    s.compile(&["-O2", "alarm.c", "-o", "alarm"]);
    let mut child = Command::new(s.dir.join("alarm"))
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("the program runs");
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().expect("the program can be waited for") {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the loop never saw the flag the signal handler set");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut out = String::new();
    child
        .stdout
        .take()
        .expect("piped")
        .read_to_string(&mut out)
        .expect("the output");
    assert!(status.success(), "{status}");
    assert_eq!(out, "done\n");
}

/// `-c` makes one object per file, named as GCC names it, and a later run
/// links them; an object from another `rustc` is refused by name.
#[test]
fn separate_compilation() {
    let s = Scratch::new("separate");
    s.write("lib.c", "int twice(int x) { return 2 * x; }\n");
    s.write(
        "main.c",
        "#include <stdio.h>\nint twice(int);\nint main(void) { printf(\"%d\\n\", twice(21)); return 0; }\n",
    );
    s.compile(&["-O2", "-c", "lib.c"]);
    s.compile(&["-c", "main.c", "-o", "m.o"]);
    assert!(s.dir.join("lib.o").is_file() && s.dir.join("m.o").is_file());
    s.compile(&["lib.o", "m.o", "-o", "prog"]);
    assert_eq!(stdout(&s.run("prog", &[])), "42\n");

    // One file compiled twice into one program, as zstd builds its decoder
    // with and without a feature: two crates, not one twice.
    s.write("scale.c", "int NAME(int x) { return FACTOR * x; }\n");
    s.write(
        "two.c",
        "#include <stdio.h>\nint by2(int);\nint by3(int);\n\
         int main(void) { printf(\"%d %d\\n\", by2(5), by3(5)); return 0; }\n",
    );
    s.compile(&["-DNAME=by2", "-DFACTOR=2", "-c", "scale.c", "-o", "by2.o"]);
    s.compile(&["-DNAME=by3", "-DFACTOR=3", "-c", "scale.c", "-o", "by3.o"]);
    s.compile(&["two.c", "by2.o", "by3.o", "-o", "two"]);
    assert_eq!(stdout(&s.run("two", &[])), "10 15\n");

    // Two links at once in one directory, of outputs with one stem, as
    // `make -j` runs mbedtls's `test_suite_aes.ecb` and `test_suite_aes.cbc`:
    // `rustc`'s files beside its output used to collide, now and then.
    for round in 0..3 {
        let links: Vec<_> = ["prog.one", "prog.two"]
            .map(|output| {
                Command::new(env!("CARGO_BIN_EXE_ccinrs"))
                    .args(["lib.o", "m.o", "-o", output])
                    .env("CCINRS_CACHE_DIR", cache_dir())
                    .current_dir(&s.dir)
                    .spawn()
                    .expect("ccinrs runs")
            })
            .into_iter()
            .collect();
        for mut link in links {
            assert!(link.wait().expect("ccinrs ends").success(), "round {round}");
        }
        assert_eq!(stdout(&s.run("prog.one", &[])), "42\n");
        assert_eq!(stdout(&s.run("prog.two", &[])), "42\n");
    }

    let out = s.ccinrs(&["-c", "lib.c", "main.c", "-o", "both.o"]);
    assert_eq!(
        stderr(&out),
        "ccinrs: error: cannot specify '-o' with '-c' and more than one C file\n"
    );

    // What a `.o` from another build of rustc carries.
    std::fs::write(
        s.dir.join("old.o"),
        b"\x7fELF...ccinrs object: rustc 1.99.0 (0123456789) for x86_64-unknown-linux-gnu\0...",
    )
    .expect("the file");
    let out = s.ccinrs(&["m.o", "old.o", "-o", "prog"]);
    assert!(
        stderr(&out).starts_with(
            "ccinrs: error: old.o was compiled by ccinrs with rustc 1.99.0 (0123456789) for \
             x86_64-unknown-linux-gnu, and this link uses rustc "
        ),
        "{}",
        stderr(&out)
    );
}

/// `-S` writes the Rust a C file becomes — `<stem>.rs`, or standard output
/// for `-o -` — which `rustc` alone builds into the same program.
#[test]
fn the_rust_a_file_becomes() {
    let s = Scratch::new("emit-rust");
    s.write(
        "hello.c",
        "#include <stdio.h>\nstatic int twice(int x) { return 2 * x; }\nint main(void) { printf(\"%d\\n\", twice(21)); return 0; }\n",
    );
    s.compile(&["-S", "hello.c"]);
    let rust = std::fs::read_to_string(s.dir.join("hello.rs")).expect("hello.rs");
    assert!(rust.starts_with("//! Translated by ccinrs "), "{rust}");
    assert!(
        rust.contains("fn twice(mut x: ::core::ffi::c_int)"),
        "{rust}"
    );
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let out = Command::new(rustc)
        .args(["--edition", "2024", "hello.rs", "-o", "hello"])
        .current_dir(&s.dir)
        .output()
        .expect("rustc runs");
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&s.run("hello", &[])), "42\n");

    let out = s.ccinrs(&["-S", "hello.c", "-o", "-"]);
    assert_eq!(stdout(&out), rust);
}

#[test]
fn include_directories_and_command_line_macros() {
    let s = Scratch::new("macros");
    s.write("inc/config.h", "#define GREETING \"from inc\"\n");
    s.write(
        "m.c",
        r#"#include <stdio.h>
#include "config.h"
#define STR2(x) #x
#define STR(x) STR2(x)
int main(void) {
#ifdef GONE
    puts("GONE survived -U");
#endif
    printf("%s %d %s %d\n", GREETING, LEVEL, STR(NAME), SQ(7));
    return 0;
}
"#,
    );
    s.compile(&[
        "-I",
        "inc",
        "-DLEVEL=3",
        "-D",
        "NAME=widget",
        "-DSQ(x)=((x)*(x))",
        "-DGONE",
        "-UGONE",
        "m.c",
        "-o",
        "m",
    ]);
    assert_eq!(stdout(&s.run("m", &[])), "from inc 3 widget 49\n");
}

#[test]
fn an_error_is_reported_as_gcc_reports_it_and_nothing_is_linked() {
    let s = Scratch::new("error");
    s.write(
        "bad.c",
        "#include <stdio.h>\nint main(void) {\n\tint x = 1;\n\treturn missing(x);\n}\n",
    );
    let out = s.ccinrs(&["bad.c", "-o", "bad"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        "bad.c:4:9: error: implicit declaration of function 'missing' is invalid in C99\n    \
         4 | \treturn missing(x);\n      | \t       ^\n"
    );
    assert!(!s.dir.join("bad").exists());

    // A note that points elsewhere says where first, as GCC's do.
    s.write(
        "twice.c",
        "int f(void) { return 1; }\nint f(void) { return 2; }\n",
    );
    let out = s.ccinrs(&["-c", "twice.c"]);
    assert!(
        stderr(&out).ends_with("twice.c:1:5: note: previous definition of 'f' is here\n"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn warnings_are_printed_unless_w() {
    let s = Scratch::new("warnings");
    s.write(
        "w.c",
        "int puts(const char *);\nint main(void) {\n    char b[8];\n    strcpy(b, \"ok\");\n    return puts(b) < 0;\n}\n",
    );
    let out = s.ccinrs(&["-std=gnu89", "w.c", "-o", "w"]);
    assert!(out.status.success());
    assert!(
        stderr(&out).starts_with(
            "w.c:4:5: warning: incompatible implicit declaration of built-in function 'strcpy'\n"
        ),
        "{}",
        stderr(&out)
    );
    assert_eq!(stdout(&s.run("w", &[])), "ok\n");
    let out = s.ccinrs(&["-std=gnu89", "-w", "w.c", "-o", "w"]);
    assert_eq!(stderr(&out), "");

    // A macro redefined is a warning and the later definition stands, but not
    // when the later one is the system's: zstd defines `assert` before
    // `<assert.h>` does.
    s.write(
        "redefined.c",
        "#define assert(c) ((void)0)\n#define N 1\n#define N 2\n#include <assert.h>\n\
         int main(void) { assert(N == 2); return N; }\n",
    );
    let out = s.ccinrs(&["redefined.c", "-o", "redefined"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stderr(&out).starts_with("redefined.c:3:9: warning: macro 'N' redefined\n")
            && stderr(&out)
                .ends_with("redefined.c:2:9: note: previous definition of 'N' is here\n")
            && !stderr(&out).contains("'assert'"),
        "{}",
        stderr(&out)
    );
    assert_eq!(s.run("redefined", &[]).status.code(), Some(2));
}

/// Rust's own checks stay in by default: a misaligned dereference aborts with
/// its message rather than reading whatever it reads. `-fno-cinrs-checks`
/// takes them out.
#[test]
fn run_time_checks_are_on_unless_asked_off() {
    let s = Scratch::new("checks");
    s.write(
        "align.c",
        r#"#include <stdio.h>
#include <string.h>
int main(int argc, char **argv) {
    char buf[16];
    memset(buf, 1, sizeof buf);
    int *p = (int *)(buf + argc);
    printf("%d\n", *p);
    return 0;
}
"#,
    );
    s.compile(&["-O2", "align.c", "-o", "checked"]);
    let out = s.run("checked", &[]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("misaligned pointer dereference"),
        "{}",
        stderr(&out)
    );
    // The panic hook the link installs stops it there: no unwind is tried,
    // so the unwinder has nothing to say.
    assert!(
        !stderr(&out).contains("failed to initiate panic")
            && !stderr(&out).contains("cannot unwind"),
        "{}",
        stderr(&out)
    );
    // Where it happened is a line of the C: the generated Rust keeps the C's
    // lines, and `rustc` is told to call it by the C file's name. (The column
    // is the Rust's, which is longer than the C.)
    assert!(
        stderr(&out).contains("panicked at align.c:7:"),
        "{}",
        stderr(&out)
    );
    s.compile(&["-O2", "-fno-cinrs-checks", "align.c", "-o", "unchecked"]);
    let out = s.run("unchecked", &[]);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "16843009\n");

    // With the checks, LLVM's inliner is told to reach further, so that they
    // do not change what is inlined; without them, nothing is said.
    let threshold = "llvm-args=-inlinehint-threshold=1000";
    let out = s.ccinrs(&["-v", "-O2", "-c", "align.c"]);
    assert!(stderr(&out).contains(threshold), "{}", stderr(&out));
    let out = s.ccinrs(&["-v", "-O2", "-fno-cinrs-checks", "-c", "align.c"]);
    assert!(!stderr(&out).contains(threshold), "{}", stderr(&out));
}

/// A function with a `goto` is generated as a graph, whose locals are all
/// declared at its top — and a panic in it still names its own C line.
#[test]
fn a_panic_names_its_line_in_a_function_with_goto() {
    let s = Scratch::new("goto");
    s.write(
        "goto.c",
        r#"#include <stdio.h>
int average(int n, int count) {
    int total = 0;
    if (n < 0)
        goto fail;
    for (int i = 0; i < n; i++) {
        int step = i * 2;
        total += step;
    }
    int mean = total / count;
    return mean;
fail:
    return -1;
}
int main(int argc, char **argv) {
    (void)argv;
    printf("%d\n", average(4, argc - 1));
    return 0;
}
"#,
    );
    s.compile(&["goto.c", "-o", "goto"]);
    assert_eq!(stdout(&s.run("goto", &["x"])), "12\n");
    let out = s.run("goto", &[]);
    assert!(
        stderr(&out).contains("panicked at goto.c:10:"),
        "{}",
        stderr(&out)
    );
    // A panic that could unwind — a division by zero — ends with Rust's
    // message and `abort`, not an unwind that finds no handler below `main`.
    #[cfg(unix)]
    assert_aborted(&out);
    assert!(
        !stderr(&out).contains("failed to initiate panic"),
        "{}",
        stderr(&out)
    );
}

/// A file that is not UTF-8 compiles, and the bytes of a string literal come
/// out as they went in, as GCC passes them.
#[test]
fn a_latin1_file_keeps_its_bytes() {
    let s = Scratch::new("latin1");
    let path = s.dir.join("latin1.c");
    std::fs::write(
        &path,
        b"#include <stdio.h>\n/* caf\xe9 */\nint main(void) {\n    fputs(\"\xe9t\xe9\\n\", stdout);\n    return 0;\n}\n",
    )
    .expect("the file");
    s.compile(&["latin1.c", "-o", "latin1"]);
    assert_eq!(s.run("latin1", &[]).stdout, b"\xe9t\xe9\n");
}

#[test]
fn the_standard_is_gnu17_unless_told() {
    let s = Scratch::new("standard");
    s.write(
        "v.c",
        "#include <stdio.h>\nint main(void) {\n#ifdef __STRICT_ANSI__\n    puts(\"strict\");\n#endif\n    printf(\"%ld\\n\", __STDC_VERSION__);\n    return 0;\n}\n",
    );
    s.compile(&["v.c", "-o", "v"]);
    assert_eq!(stdout(&s.run("v", &[])), "201710\n");
    s.compile(&["-std=c11", "v.c", "-o", "v"]);
    assert_eq!(stdout(&s.run("v", &[])), "strict\n201112\n");
}

#[test]
fn a_library_on_the_command_line_is_linked() {
    let s = Scratch::new("libm");
    s.write(
        "root.c",
        "#include <math.h>\n#include <stdio.h>\n#include <stdlib.h>\nint main(int argc, char **argv) {\n    printf(\"%.3f\\n\", sqrt(atof(argv[1])));\n    return 0;\n}\n",
    );
    s.compile(&["root.c", "-lm", "-o", "root"]);
    assert_eq!(stdout(&s.run("root", &["2"])), "1.414\n");
}

#[test]
fn the_command_line_is_checked() {
    let s = Scratch::new("options");
    s.write("x.c", "int main(void) { return 0; }\n");
    let out = s.ccinrs(&[
        "-Wall",
        "-Wfrobnicate",
        "-fPIC",
        "-ffrobnicate",
        "x.c",
        "-o",
        "x",
    ]);
    assert!(out.status.success());
    assert_eq!(
        stderr(&out),
        "ccinrs: warning: ignoring unknown warning option '-Wfrobnicate'\n\
         ccinrs: warning: ignoring unknown option '-ffrobnicate'\n"
    );
    // A `configure` script's probe of whether a warning option is taken.
    let out = s.ccinrs(&["-Werror", "-Wfrobnicate", "x.c", "-o", "x"]);
    assert_eq!(
        stderr(&out),
        "ccinrs: error: unrecognized command-line option '-Wfrobnicate'\n"
    );
    let out = s.ccinrs(&["-Werror", "-Wduplicated-cond", "x.c", "-o", "x"]);
    assert!(
        out.status.success() && out.stderr.is_empty(),
        "{}",
        stderr(&out)
    );
    // And of whether `-Werror` works at all: PCRE2's writes a `#warning`.
    s.write("warned.c", "#warning e\nint main(void) { return 0; }\n");
    let out = s.ccinrs(&["-Werror", "-c", "warned.c"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        "warned.c:1:1: error: #warning e\n    1 | #warning e\n      | ^\n\
         ccinrs: all warnings being treated as errors\n"
    );
    assert!(!s.dir.join("warned.o").exists());
    assert!(s.ccinrs(&["-c", "warned.c"]).status.success());
    // Two more probes: jansson's compiles `/dev/null` with the option, and
    // CMake's `check_c_compiler_flag(/W4)` must not take an MSVC option for
    // a file it does not find.
    #[cfg(unix)]
    assert!(
        s.ccinrs(&["-Werror", "-S", "-o", "/dev/null", "-xc", "/dev/null"])
            .status
            .success()
    );
    let out = s.ccinrs(&["-c", "x.c", "/W4"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        "ccinrs: warning: /W4: linker input file unused because linking not done\n\
         ccinrs: error: /W4: linker input file not found: No such file or directory\n"
    );
    // `-fsyntax-only` checks and writes nothing: no object, even with `-c`,
    // and an error in the C is still one.
    s.write("bad.c", "int f(void) { return undeclared; }\n");
    let out = s.ccinrs(&["-fsyntax-only", "-c", "x.c"]);
    assert!(
        out.status.success() && out.stderr.is_empty(),
        "{}",
        stderr(&out)
    );
    assert!(!s.dir.join("x.o").exists());
    let out = s.ccinrs(&["-fsyntax-only", "bad.c"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("undeclared"), "{}", stderr(&out));
    let out = s.ccinrs(&["-fshort-enums", "x.c"]);
    assert_eq!(
        stderr(&out),
        "ccinrs: error: '-fshort-enums' changes what the program means, and ccinrs cannot follow it\n"
    );
    let out = s.ccinrs(&["--frobnicate", "x.c"]);
    assert_eq!(
        stderr(&out),
        "ccinrs: error: unrecognized command-line option '--frobnicate'\n"
    );
    let out = s.ccinrs(&[]);
    assert_eq!(stderr(&out), "ccinrs: error: no input files\n");
    let out = s.ccinrs(&["x.cpp"]);
    assert_eq!(
        stderr(&out),
        "ccinrs: error: x.cpp: ccinrs compiles C, not this language\n"
    );
    // A header is preprocessed, as curl's tests preprocess `curl.h`, and
    // compiled by nothing else.
    s.write(
        "api.h",
        "#define TWICE(x) ((x) * 2)\nint api(int);\nTWICE(21)\n",
    );
    let out = s.ccinrs(&["-E", "-P", "api.h"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("int api(int);") && stdout(&out).contains("21 ) * 2"),
        "{}",
        stdout(&out)
    );
    let out = s.ccinrs(&["-c", "api.h"]);
    assert_eq!(
        stderr(&out),
        "ccinrs: error: api.h: a header is not compiled on its own; ccinrs makes no precompiled headers\n"
    );
    let out = s.ccinrs(&["--target=foo-bar", "x.c"]);
    assert!(
        stderr(&out).starts_with("ccinrs: error: unknown architecture 'foo'"),
        "{}",
        stderr(&out)
    );
}

/// The complex types call the runtime, which `ccinrs` compiles the first time
/// and keeps: Annex G's product, an object made by `-c` that needs it, and
/// `-S`, whose file carries the runtime itself.
#[test]
fn the_complex_types_bring_the_runtime() {
    let s = Scratch::new("complex");
    s.write(
        "main.c",
        "#include <complex.h>\n#include <math.h>\n#include <stdio.h>\ndouble _Complex rotate(double _Complex z);\nint main(void) {\n    double _Complex z = 1.0 + 2.0 * I, w = 3.0 - 4.0 * I;\n    double _Complex p = z * w, q = z / w, r = (INFINITY + 0.0 * I) * w;\n    float _Complex f = (1.5f + 0.5f * I) * (1.5f + 0.5f * I);\n    double _Complex t = rotate(z) + z - -z;\n    printf(\"%g%+gi %g%+gi %g%+gi %g%+gi %g%+gi\\n\", creal(p), cimag(p), creal(q), cimag(q),\n           creal(r), cimag(r), crealf(f), cimagf(f), creal(t), cimag(t));\n    return 0;\n}\n",
    );
    s.write(
        "rotate.c",
        "double _Complex rotate(double _Complex z) { return z * __builtin_complex(0.0, 1.0); }\n",
    );
    let expected = "11+2i -0.2+0.4i inf-infi 2+1.5i 0+5i\n";
    s.compile(&["main.c", "rotate.c", "-o", "together"]);
    assert_eq!(stdout(&s.run("together", &[])), expected);
    s.compile(&["-c", "rotate.c"]);
    s.compile(&["main.c", "rotate.o", "-o", "apart"]);
    assert_eq!(stdout(&s.run("apart", &[])), expected);
    s.compile(&["-S", "rotate.c"]);
    let rust = std::fs::read_to_string(s.dir.join("rotate.rs")).expect("the .rs");
    assert!(rust.contains("extern crate self as cinrs;"), "{rust}");
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let out = Command::new(rustc)
        .args(["--edition", "2024", "--crate-type", "lib", "rotate.rs"])
        .current_dir(&s.dir)
        .output()
        .expect("rustc runs");
    assert!(out.status.success(), "{}", stderr(&out));
}

/// `-march=` and the `-m` switches reach `rustc` as `target-cpu` and
/// `target-feature`, and the preprocessor as GCC's feature macros.
#[cfg(target_arch = "x86_64")]
#[test]
fn the_instruction_sets_are_the_command_lines() {
    let s = Scratch::new("march");
    s.write(
        "f.c",
        "#include <stdio.h>\n#include <immintrin.h>\nint main(void) {\n#ifdef __AVX2__\n    int out[8];\n    _mm256_storeu_si256((__m256i *)out, _mm256_add_epi32(_mm256_set1_epi32(3), _mm256_set1_epi32(4)));\n    printf(\"avx2 %d\\n\", out[5]);\n#endif\n#ifdef __FMA__\n    puts(\"fma\");\n#endif\n#ifdef __SSE4_2__\n    puts(\"sse4.2\");\n#endif\n    puts(\"done\");\n    return 0;\n}\n",
    );
    s.compile(&["f.c", "-o", "plain"]);
    assert_eq!(stdout(&s.run("plain", &[])), "done\n");
    s.compile(&["-march=x86-64-v2", "f.c", "-o", "v2"]);
    s.compile(&["-mavx2", "f.c", "-o", "avx2"]);
    s.compile(&["-march=haswell", "f.c", "-o", "haswell"]);
    s.compile(&["-march=haswell", "-mno-avx2", "f.c", "-o", "not"]);
    // The programs run only where the processor has what they were built for.
    if std::arch::is_x86_feature_detected!("sse4.2") {
        assert_eq!(stdout(&s.run("v2", &[])), "sse4.2\ndone\n");
    }
    if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
        assert_eq!(stdout(&s.run("avx2", &[])), "avx2 7\nsse4.2\ndone\n");
        assert_eq!(
            stdout(&s.run("haswell", &[])),
            "avx2 7\nfma\nsse4.2\ndone\n"
        );
        assert_eq!(stdout(&s.run("not", &[])), "fma\nsse4.2\ndone\n");
    }
    let out = s.ccinrs(&["-march=frobnicator", "f.c"]);
    assert_eq!(
        stderr(&out),
        "ccinrs: error: '-march=frobnicator': rustc does not know that processor; \
         `rustc --print target-cpus` lists those it does\n"
    );
    let out = s.ccinrs(&["-mfrobnicate", "f.c"]);
    assert!(
        stderr(&out).starts_with("ccinrs: error: '-mfrobnicate' is not supported"),
        "{}",
        stderr(&out)
    );
}

// ---------------------------------------------------------------------------
// The preprocessor, and what make asks
// ---------------------------------------------------------------------------

const UTIL_H: &str = "#ifndef UTIL_H\n#define UTIL_H\n#define SQUARE(x) ((x) * (x))\n#pragma pack(push, 1)\nstruct packed { char c; int i; };\n#pragma pack(pop)\nint util(int);\n#endif\n";

const MAIN_C: &str = "#include <stdio.h>\n#include \"util.h\"\n#include \"util.h\"\n\nint main(int argc, char **argv) {\n    int y = SQUARE(argc + 1);\n    printf(\"%d %zu\\n\", y, sizeof(struct packed));\n    return util(y);\n}\n";

/// `-E` prints the preprocessed C with GCC's line markers — the pragmas that
/// change what follows them kept — and the result compiles as a `.i` file;
/// `-P` leaves the markers out.
#[test]
fn the_preprocessor_on_its_own() {
    let s = Scratch::new("preprocess");
    s.write("inc/util.h", UTIL_H);
    s.write("main.c", MAIN_C);
    s.write("util.c", "int util(int x) { return x - 4; }\n");
    let out = s.ccinrs(&["-E", "-Iinc", "main.c"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("# 1 \"main.c\"\n"), "{text}");
    for expected in [
        "#pragma pack(1)\n# 5 \"inc/util.h\" 1\nstruct packed { char c; int i; };\n\
         #pragma pack()\n# 7 \"inc/util.h\"\nint util(int);\n",
        "# 5 \"main.c\" 2\nint main(int argc, char **argv) {\n    int y = ( ( argc + 1 ) * ( argc + 1 ) ) ;\n",
    ] {
        assert!(text.contains(expected), "{expected:?} in:\n{text}");
    }
    let out = s.ccinrs(&["-E", "-P", "-Iinc", "main.c", "-o", "main.i"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = std::fs::read_to_string(s.dir.join("main.i")).expect("main.i");
    assert!(!text.lines().any(|line| line.starts_with("# ")), "{text}");
    s.compile(&["main.i", "util.c", "-o", "prog"]);
    let out = s.run("prog", &[]);
    assert_eq!(stdout(&out), "4 5\n");
    assert_eq!(out.status.code(), Some(0));
}

/// `-MM` and `-MMD` write the rule `make` includes; `-MP` adds an empty rule
/// per header, `-MT` and `-MQ` name the target, `-MF` the file. A header of
/// the program's own is listed once however often it was included, and the
/// platform's only by `-M`.
#[test]
fn dependency_rules_for_make() {
    let s = Scratch::new("deps");
    s.write("inc/util.h", UTIL_H);
    s.write("main.c", MAIN_C);
    s.write("util.c", "int util(int x) { return x - 4; }\n");
    let out = s.ccinrs(&["-MM", "-MP", "-Iinc", "main.c"]);
    assert_eq!(
        stdout(&out),
        "main.o: main.c inc/util.h\n\ninc/util.h:\n",
        "{}",
        stderr(&out)
    );
    let out = s.ccinrs(&["-M", "-Iinc", "main.c"]);
    assert!(stdout(&out).contains("stdio.h"), "{}", stdout(&out));
    // As in GCC, the directory an output goes to has to be there.
    let out = s.ccinrs(&["-c", "-Iinc", "main.c", "-o", "obj/main.o"]);
    assert_eq!(
        stderr(&out),
        "ccinrs: error: cannot open output file obj/main.o: the directory 'obj' does not exist\n"
    );
    std::fs::create_dir(s.dir.join("obj")).expect("obj/");
    s.compile(&["-MMD", "-c", "-Iinc", "main.c", "-o", "obj/main.o"]);
    assert_eq!(
        std::fs::read_to_string(s.dir.join("obj/main.d")).expect("obj/main.d"),
        "obj/main.o: main.c inc/util.h\n"
    );
    s.compile(&[
        "-MMD", "-MF", "custom.d", "-MT", "a.o", "-MQ", "$(B)", "-c", "-Iinc", "main.c",
    ]);
    assert_eq!(
        std::fs::read_to_string(s.dir.join("custom.d")).expect("custom.d"),
        "a.o $$(B): main.c inc/util.h\n"
    );
    s.compile(&["-MMD", "-Iinc", "main.c", "util.c", "-o", "prog"]);
    assert_eq!(
        std::fs::read_to_string(s.dir.join("prog-util.d")).expect("prog-util.d"),
        "util.o: util.c\n"
    );
}

/// `-shared` makes a shared library of C — from objects or from the C itself
/// — that exports the C's symbols and nothing of the Rust in it, and a
/// program links against it with `-l` as it would against GCC's.
#[cfg(target_os = "linux")]
#[test]
fn a_shared_library() {
    let s = Scratch::new("shared");
    s.write(
        "counter.c",
        "int counter = 41;\nint bump(int x) { return ++counter + x; }\nstatic int one(void) { return 1; }\nint use_one(void) { return one(); }\n",
    );
    s.write(
        "main.c",
        "#include <stdio.h>\nint bump(int);\nextern int counter;\nint use_one(void);\nint main(void) {\n    printf(\"%d %d %d\\n\", bump(1), counter, use_one());\n    return 0;\n}\n",
    );
    let run = |program: &str| {
        let out = Command::new(s.dir.join(program))
            .env("LD_LIBRARY_PATH", &s.dir)
            .output()
            .expect("the program runs");
        stdout(&out)
    };
    s.compile(&["-c", "-fPIC", "counter.c"]);
    s.compile(&[
        "-shared",
        "counter.o",
        "-Wl,-soname=libcounter.so.1",
        "-o",
        "libcounter.so.1",
    ]);
    // The name the link looks for, and the one the program then asks for.
    std::os::unix::fs::symlink("libcounter.so.1", s.dir.join("libcounter.so")).expect("the link");
    s.compile(&["main.c", "-L.", "-lcounter", "-o", "from-objects"]);
    assert_eq!(run("from-objects"), "43 42 1\n");
    s.compile(&["-shared", "-fPIC", "counter.c", "-o", "libdirect.so"]);
    s.compile(&["main.c", "-L.", "-ldirect", "-o", "from-source"]);
    assert_eq!(run("from-source"), "43 42 1\n");

    // A convenience archive linked whole into a shared library, as libtool
    // does it — libsodium's `librdrand.la`: its symbols are the library's.
    let ar = Command::new("ar")
        .args(["cr", "libcounter.a", "counter.o"])
        .current_dir(&s.dir)
        .status()
        .expect("ar runs");
    assert!(ar.success());
    s.write("other.c", "int other(void) { return 0; }\n");
    s.compile(&["-c", "-fPIC", "other.c"]);
    s.compile(&[
        "-shared",
        "other.o",
        "-Wl,--whole-archive",
        "libcounter.a",
        "-Wl,--no-whole-archive",
        "-o",
        "libwhole.so",
    ]);
    s.compile(&["main.c", "-L.", "-l:libwhole.so", "-o", "from-archive"]);
    assert_eq!(run("from-archive"), "43 42 1\n");

    // An archive and a shared library named on the command line through a
    // relative symbolic link, as Redis's `deps/xxhash/libxxhash.a` and
    // jemalloc's `lib/libjemalloc.so` are.
    std::fs::create_dir_all(s.dir.join("deps")).expect("the directory");
    std::os::unix::fs::symlink("../libcounter.a", s.dir.join("deps/libcounter.a"))
        .expect("the link");
    std::os::unix::fs::symlink("../libwhole.so", s.dir.join("deps/libwhole.so")).expect("the link");
    s.compile(&["main.c", "deps/libcounter.a", "-o", "through-link"]);
    assert_eq!(run("through-link"), "43 42 1\n");
    // A library with no soname is recorded as it was named, as GCC records
    // it: `deps/libwhole.so`, found from the directory the program runs in.
    s.compile(&["main.c", "deps/libwhole.so", "-o", "through-so-link"]);
    assert_eq!(stdout(&s.run("through-so-link", &[])), "43 42 1\n");

    // An object from another compiler in a shared library — on its own, and
    // in an archive linked whole: its symbols are exported too, as from
    // GCC's, which a mixed build of libwebp's `libsharpyuv.so` needs.
    s.write("foreign.c", "int foreign_fn(void) { return 7; }\n");
    let cc = Command::new("cc")
        .args(["-fPIC", "-c", "foreign.c", "-o", "foreign.o"])
        .current_dir(&s.dir)
        .status()
        .expect("cc runs");
    assert!(cc.success());
    let ar = Command::new("ar")
        .args(["cr", "libforeign.a", "foreign.o"])
        .current_dir(&s.dir)
        .status()
        .expect("ar runs");
    assert!(ar.success());
    s.write(
        "call.c",
        "int foreign_fn(void);\nint bump(int);\nint main(void) { return foreign_fn() + bump(0) - 49; }\n",
    );
    for (name, inputs) in [
        ("libmixed.so", vec!["counter.o", "foreign.o"]),
        (
            "libmixeda.so",
            vec![
                "counter.o",
                "-Wl,--whole-archive",
                "libforeign.a",
                "-Wl,--no-whole-archive",
            ],
        ),
    ] {
        let mut args = vec!["-shared", "-o", name];
        args.extend(inputs);
        s.compile(&args);
        let lib = format!("-l:{name}");
        s.compile(&["call.c", "-L.", &lib, "-o", "call"]);
        let out = Command::new(s.dir.join("call"))
            .env("LD_LIBRARY_PATH", &s.dir)
            .output()
            .expect("the program runs");
        assert_eq!(out.status.code(), Some(0), "{name}");
    }

    // `-fuse-ld=bfd` links a program with GNU ld, which takes an option LLD
    // does not; a shared library stays with LLD, saying so.
    #[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
    {
        s.compile(&[
            "-fuse-ld=bfd",
            "main.c",
            "-L.",
            "-lcounter",
            "-Wl,--default-symver",
            "-o",
            "with-bfd",
        ]);
        assert_eq!(run("with-bfd"), "43 42 1\n");
        let out = s.ccinrs(&["-shared", "-fuse-ld=bfd", "counter.o", "-o", "libbfd.so"]);
        assert!(out.status.success(), "{}", stderr(&out));
        assert!(
            stderr(&out).starts_with("ccinrs: warning: ignoring '-fuse-ld=bfd'"),
            "{}",
            stderr(&out)
        );
    }

    // A version script decides what is exported, given the way libtool gives
    // it — one `-Wl,` per word — or through `-Xlinker`: `use_one` is not.
    s.write(
        "lib.map",
        "COUNTER_1 { global: bump; counter; local: *; };\n",
    );
    s.write(
        "bump.c",
        "#include <stdio.h>\nint bump(int);\nint main(void) { printf(\"%d\\n\", bump(1)); return 0; }\n",
    );
    for (name, script) in [
        ("libsplit.so", ["-Wl,--version-script", "-Wl,lib.map"]),
        ("libxlinker.so", ["-Xlinker", "--version-script=lib.map"]),
    ] {
        let mut args = vec!["-shared", "counter.o", "-o", name];
        args.extend(script);
        s.compile(&args);
        let lib = format!("-l:{name}");
        s.compile(&["bump.c", "-L.", &lib, "-o", "bump"]);
        assert_eq!(run("bump"), "43\n");
        let out = s.ccinrs(&["main.c", "-L.", &lib, "-o", "hidden"]);
        assert!(
            !out.status.success() && stderr(&out).contains("use_one"),
            "{name}: {}",
            stderr(&out)
        );
    }
}

/// `-static` links the C library in.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn a_static_program() {
    let s = Scratch::new("static");
    s.write(
        "hello.c",
        "#include <stdio.h>\nint main(void) { puts(\"static\"); return 0; }\n",
    );
    s.compile(&["-static", "hello.c", "-o", "hello"]);
    assert_eq!(stdout(&s.run("hello", &[])), "static\n");
}

/// What a build pipes in or forces in: `-dM -E -` for the predefined macros,
/// a program on standard input under `-x c`, and `-include`.
#[test]
fn standard_input_dm_and_include() {
    use std::io::Write;
    use std::process::Stdio;
    let s = Scratch::new("stdin");
    let piped = |args: &[&str], input: &str| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ccinrs"))
            .args(args)
            .env("CCINRS_CACHE_DIR", cache_dir())
            .current_dir(&s.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("ccinrs runs");
        child
            .stdin
            .take()
            .expect("its input")
            .write_all(input.as_bytes())
            .expect("the input is written");
        child.wait_with_output().expect("ccinrs ends")
    };
    let out = piped(&["-dM", "-E", "-"], "#define F(a, ...) a + __VA_ARGS__\n");
    let macros = stdout(&out);
    assert!(macros.contains("#define __GNUC__ 14\n"), "{macros}");
    assert!(
        macros.contains("#define F(a, ...) a + __VA_ARGS__\n"),
        "{macros}"
    );
    assert!(!macros.contains("__LINE__"), "{macros}");
    let out = piped(
        &["-x", "c", "-", "-o", "piped"],
        "#include <stdio.h>\nint main(void) { puts(__FILE__); return 0; }\n",
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&s.run("piped", &[])), "<stdin>\n");
    let out = piped(&["-c", "-"], "int x;\n");
    assert_eq!(
        stderr(&out),
        "ccinrs: error: -E or -x required when input is from standard input\n"
    );
    s.write("config.h", "#define ANSWER 42\n");
    s.write(
        "uses.c",
        "#include <stdio.h>\nint main(void) { printf(\"%d\\n\", ANSWER); return 0; }\n",
    );
    s.compile(&["-include", "config.h", "uses.c", "-o", "uses"]);
    assert_eq!(stdout(&s.run("uses", &[])), "42\n");
}

/// What a build asks a compiler about itself.
#[test]
fn the_compiler_says_what_it_is() {
    let s = Scratch::new("version");
    let out = s.ccinrs(&["--version"]);
    assert!(out.status.success());
    assert!(stdout(&out).starts_with("ccinrs "), "{}", stdout(&out));
    assert_eq!(stdout(&s.ccinrs(&["-dumpversion"])), "14\n");
    assert_eq!(stdout(&s.ccinrs(&["-dumpfullversion"])), "14.2.0\n");
    assert_eq!(
        stdout(&s.ccinrs(&["--target=wasm32-wasip1", "-dumpmachine"])),
        "wasm32-wasip1\n"
    );
    let out = s.ccinrs(&["-v"]);
    assert!(out.status.success());
    assert!(stderr(&out).starts_with("ccinrs "), "{}", stderr(&out));
    // What libtool and CMake ask of a compiler.
    assert_eq!(
        stdout(&s.ccinrs(&["--target=aarch64-unknown-linux-musl", "-print-multiarch"])),
        "aarch64-linux-musl\n"
    );
    assert_eq!(stdout(&s.ccinrs(&["-print-prog-name=ld"])), "ld\n");
    let out = stdout(&s.ccinrs(&["-print-search-dirs"]));
    assert!(
        out.starts_with("install: ") && out.contains("\nlibraries: ="),
        "{out}"
    );
}

/// `-flto` makes objects that are crates, which a link with `-flto`
/// optimises together: in one command, or compiled apart and linked later,
/// alongside an object compiled without it. An archive of them is refused
/// with the reason, as one of GCC's needs `gcc-ar`.
#[test]
fn link_time_optimisation() {
    let s = Scratch::new("lto");
    s.write(
        "step.c",
        "unsigned step(unsigned x) { return x * 3u + 1u; }\n",
    );
    s.write(
        "loop.c",
        "#include <stdio.h>\nunsigned step(unsigned);\nint main(void) {\n    unsigned x = 1;\n    for (int i = 0; i < 1000; i++) x = step(x);\n    printf(\"%u\\n\", x);\n    return 0;\n}\n",
    );
    s.compile(&["-O2", "loop.c", "step.c", "-o", "plain"]);
    let expected = stdout(&s.run("plain", &[]));
    s.compile(&["-O2", "-flto", "loop.c", "step.c", "-o", "together"]);
    assert_eq!(stdout(&s.run("together", &[])), expected);
    s.compile(&["-O2", "-flto=thin", "-c", "step.c"]);
    s.compile(&["-O2", "-c", "loop.c"]);
    s.compile(&["-O2", "-flto", "loop.o", "step.o", "-o", "apart"]);
    assert_eq!(stdout(&s.run("apart", &[])), expected);
    let archived = Command::new("ar")
        .args(["rcs", "libstep.a", "step.o"])
        .current_dir(&s.dir)
        .status();
    if archived.is_ok_and(|status| status.success()) {
        let out = s.ccinrs(&["loop.o", "libstep.a", "-o", "archived"]);
        assert!(
            stderr(&out).contains("compiled with -flto, which cannot be linked from an archive"),
            "{}",
            stderr(&out)
        );
    }
    // An archive compiled without `-flto`, whose machine code calls Rust's
    // standard library by symbol — a dereference's run-time check does — in
    // a link with it: Redis links its `deps/` so. Rust's LTO would keep only
    // the symbols its own modules use, so the link is made without it.
    s.write(
        "pick.c",
        "int pick(const int *p, int i) { return p[i] * 2; }\n",
    );
    s.write(
        "use.c",
        "#include <stdio.h>\nint pick(const int *, int);\nint main(void) { int a[2] = { 4, 5 }; printf(\"%d\\n\", pick(a, 1)); return 0; }\n",
    );
    s.compile(&["-O2", "-c", "pick.c"]);
    let archived = Command::new("ar")
        .args(["rcs", "libpick.a", "pick.o"])
        .current_dir(&s.dir)
        .status();
    if archived.is_ok_and(|status| status.success()) {
        s.compile(&["-O2", "-flto", "use.c", "libpick.a", "-o", "mixed"]);
        assert_eq!(stdout(&s.run("mixed", &[])), "10\n");
    }
}

// ---------------------------------------------------------------------------
// Other targets
// ---------------------------------------------------------------------------

/// Whether the Rust standard library for `target` is installed — printing
/// why not, in which case the test that asks passes without running.
fn target_installed(target: &str) -> bool {
    let libdir = Command::new(rustc())
        .args(["--print", "target-libdir", "--target", target])
        .output();
    let installed =
        libdir.is_ok_and(|out| Path::new(String::from_utf8_lossy(&out.stdout).trim()).is_dir());
    if !installed {
        eprintln!("skipped: the {target} standard library is not installed");
    }
    installed
}

/// The `rustc` the tests run, by its full path: `RUSTC`, or the one the
/// `rustc` on `PATH` says it is.
fn rustc() -> PathBuf {
    if let Some(rustc) = std::env::var_os("RUSTC") {
        return PathBuf::from(rustc);
    }
    let out = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .expect("rustc runs");
    Path::new(String::from_utf8_lossy(&out.stdout).trim()).join("bin/rustc")
}

/// `--target=x86_64-unknown-linux-musl`: Rust ships musl's C library and its
/// start files with the target, and `rust-lld` links them, so a program
/// builds — static — on a machine with no C compiler and no linker at all:
/// here, with nothing on `PATH`. `-lm` is part of musl's libc.a.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn a_program_for_musl_with_no_c_compiler_anywhere() {
    const TARGET: &str = "x86_64-unknown-linux-musl";
    if !target_installed(TARGET) {
        return;
    }
    let s = Scratch::new("musl");
    s.write(
        "m.c",
        "#include <errno.h>\n#include <math.h>\n#include <stdio.h>\n#include <string.h>\nint main(int argc, char **argv) {\n    FILE *f = fopen(\"/nowhere\", \"r\");\n    printf(\"%s %.3f %s\\n\", argc > 1 ? argv[1] : \"?\", sqrt(2.0), f ? \"opened\" : strerror(errno));\n    return 0;\n}\n",
    );
    let out = Command::new(env!("CARGO_BIN_EXE_ccinrs"))
        .args(["--target", TARGET, "m.c", "-lm", "-o", "m"])
        .env_clear()
        .env("PATH", "/nonexistent")
        .env("RUSTC", rustc())
        .env("CCINRS_CACHE_DIR", cache_dir())
        .current_dir(&s.dir)
        .output()
        .expect("ccinrs runs");
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stdout(&s.run("m", &["musl"])),
        "musl 1.414 No such file or directory\n"
    );
    let out = s.ccinrs(&["--target", TARGET, "-shared", "m.c", "-o", "m.so"]);
    assert!(
        stderr(&out).contains("has musl only as an archive"),
        "{}",
        stderr(&out)
    );
}

/// The same for another architecture, from this one: an aarch64 program,
/// linked with nothing but the Rust target installed, run under
/// `qemu-aarch64` when it is there (`QEMU_AARCH64` names one). Two units, the
/// complex runtime compiled for aarch64, and `-O2`.
#[test]
fn a_program_for_aarch64_musl_under_qemu() {
    const TARGET: &str = "aarch64-unknown-linux-musl";
    if !target_installed(TARGET) {
        return;
    }
    let qemu = std::env::var_os("QEMU_AARCH64").unwrap_or_else(|| "qemu-aarch64".into());
    if Command::new(&qemu).arg("--version").output().is_err() {
        eprintln!("skipped: no qemu-aarch64 to run {TARGET} programs with");
        return;
    }
    let s = Scratch::new("aarch64");
    s.write(
        "main.c",
        "#include <complex.h>\n#include <stdio.h>\n#include <string.h>\nint twice(int x);\n\
         int main(int argc, char **argv) {\n    double complex z = (1.0 + 2.0 * I) * (3.0 - I);\n    \
         printf(\"%s %d %g%+gi %zu %d\\n\", argc > 1 ? argv[1] : \"?\", twice(21), creal(z), cimag(z), \
         sizeof(long double), (int) strlen(\"aarch64\"));\n    return 7;\n}\n",
    );
    s.write("twice.c", "int twice(int x) { return 2 * x; }\n");
    s.compile(&["--target", TARGET, "-O2", "main.c", "twice.c", "-o", "prog"]);
    let out = Command::new(&qemu)
        .args(["prog", "arm"])
        .current_dir(&s.dir)
        .output()
        .expect("qemu runs");
    // cinrs's `long double` is `double` on every target.
    assert_eq!(stdout(&out), "arm 42 5+5i 8 7\n", "{}", stderr(&out));
    assert_eq!(out.status.code(), Some(7));
}

// ---------------------------------------------------------------------------
// WebAssembly
// ---------------------------------------------------------------------------

/// `wasm32-unknown-unknown` has no system under it, and what is made is a
/// module a host calls into: it exports the C's functions, its memory and a
/// `malloc` family made of Rust's allocator, and imports from `env` what the
/// C calls and does not define. Node drives it here, when it is installed:
/// it writes an array into memory it `malloc`s, has the C sum it — calling
/// back into the host — and reads a string the C `malloc`ed and `realloc`ed.
#[test]
fn a_module_for_wasm32_unknown_unknown() {
    const TARGET: &str = "wasm32-unknown-unknown";
    if !target_installed(TARGET) {
        return;
    }
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: no node to run the {TARGET} module with");
        return;
    }
    let s = Scratch::new("wasm-bare");
    s.write(
        "lib.c",
        "#include <stdlib.h>\n#include <string.h>\nvoid host_log(int value);\nstatic int calls;\n\
         long sum(const int *values, int count) {\n    long total = 0;\n    for (int i = 0; i < count; i++) total += values[i];\n    calls++;\n    host_log((int) total);\n    return total;\n}\n\
         char *greet(int n) {\n    char *s = malloc(32);\n    memcpy(s, \"hello wasm \", 11);\n    s[11] = (char) ('0' + n % 10);\n    s[12] = 0;\n    return realloc(s, 13);\n}\n\
         int count_calls(void) { return calls; }\n",
    );
    s.write(
        "run.mjs",
        "import fs from 'node:fs';\n\
         const module = new WebAssembly.Module(fs.readFileSync('lib.wasm'));\n\
         console.log(WebAssembly.Module.imports(module).map(i => `${i.module}.${i.name}`).join(' '));\n\
         console.log(WebAssembly.Module.exports(module).map(e => e.name).sort().join(' '));\n\
         const logged = [];\n\
         const { exports } = new WebAssembly.Instance(module, { env: { host_log: v => logged.push(v) } });\n\
         const p = exports.malloc(16);\n\
         new Int32Array(exports.memory.buffer, p, 4).set([1, 2, 3, 4]);\n\
         console.log(exports.sum(p, 4), logged.join(','));\n\
         exports.free(p);\n\
         const s = exports.greet(7);\n\
         const bytes = new Uint8Array(exports.memory.buffer, s, 32);\n\
         console.log(new TextDecoder().decode(bytes.subarray(0, bytes.indexOf(0))));\n\
         exports.free(s);\n\
         console.log(exports.count_calls());\n",
    );
    s.compile(&["--target", TARGET, "-O2", "lib.c", "-o", "lib.wasm"]);
    // V8 reserves several gigabytes of address space for each WebAssembly
    // memory, to bounds-check with guard pages, and under `scripts/ci.sh`'s
    // `ulimit -v` that reservation fails ("Cannot allocate Wasm memory").
    // A Node that has the switch is told to check explicitly instead.
    let explicit_checks = Command::new("node")
        .args(["--disable-wasm-trap-handler", "-e", "0"])
        .output()
        .is_ok_and(|out| out.status.success());
    let mut node = Command::new("node");
    if explicit_checks {
        node.arg("--disable-wasm-trap-handler");
    }
    let out = node
        .arg("run.mjs")
        .current_dir(&s.dir)
        .output()
        .expect("node runs");
    assert_eq!(
        stdout(&out),
        "env.host_log\n\
         calloc count_calls free greet malloc memory realloc sum\n\
         10 10\n\
         hello wasm 7\n\
         1\n",
        "{}",
        stderr(&out)
    );
}

/// A `wasmtime` to run `target`'s programs with, or `None` — with the reason
/// printed — when there is none or the target's standard library is not
/// installed, in which case the test passes without running anything.
/// `WASMTIME` names one; otherwise `PATH`, then `~/.wasmtime/bin`, where its
/// installer puts it.
fn wasmtime_for(target: &str) -> Option<PathBuf> {
    if !target_installed(target) {
        return None;
    }
    let found = std::env::var_os("WASMTIME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::split_paths(&std::env::var_os("PATH")?)
                .map(|dir| dir.join("wasmtime"))
                .find(|path| path.is_file())
        })
        .or_else(|| {
            let path = Path::new(&std::env::var_os("HOME")?).join(".wasmtime/bin/wasmtime");
            path.is_file().then_some(path)
        });
    if found.is_none() {
        eprintln!("skipped: no wasmtime to run {target} programs with");
    }
    found
}

const WASI_PROGRAM: &str = r#"#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
int twice(int x);
int main(int argc, char **argv) {
    char *p = malloc(16);
    strcpy(p, argc > 1 ? argv[1] : "nobody");
    errno = 0;
    FILE *f = fopen("/nowhere", "r");
    printf("hello %s %d; ENOENT=%d errno=%d %s; time_t %zu bytes\n", p, twice(21), ENOENT,
           errno, f ? "opened" : strerror(errno), sizeof(time_t));
    free(p);
    return 3;
}
"#;

/// The C library is wasi-libc, which Rust ships with the target: `printf`,
/// `malloc`, `errno` at WASI's own numbers, a 64-bit `time_t`, two units, and
/// `main` with its arguments and its exit status. The second unit doubles
/// through the complex types, so the runtime is compiled for the target too.
#[test]
fn a_program_for_wasm32_wasip1() {
    let Some(wasmtime) = wasmtime_for("wasm32-wasip1") else {
        return;
    };
    let s = Scratch::new("wasip1");
    s.write("main.c", WASI_PROGRAM);
    s.write(
        "twice.c",
        "#include <complex.h>\nint twice(int x) { return creal(x * (1.0 + I) * (1.0 - I)); }\n",
    );
    s.compile(&[
        "--target=wasm32-wasip1",
        "-O2",
        "main.c",
        "twice.c",
        "-o",
        "prog.wasm",
    ]);
    let out = run_wasm(&wasmtime, &s);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
}

/// Runs `prog.wasm` with one argument, asserting what [`WASI_PROGRAM`]
/// prints.
#[track_caller]
fn run_wasm(wasmtime: &Path, s: &Scratch) -> Output {
    // wasmtime reserves four gigabytes of address space per linear memory by
    // default, which the address-space ceiling every test here runs under
    // (see scripts/ci.sh) refuses; these programs need a few megabytes.
    let out = Command::new(wasmtime)
        .args([
            "run",
            "-O",
            "memory-reservation=0",
            "-O",
            "memory-guard-size=0",
            "prog.wasm",
            "wasi",
        ])
        .current_dir(&s.dir)
        .output()
        .expect("wasmtime runs");
    assert_eq!(
        stdout(&out),
        "hello wasi 42; ENOENT=44 errno=44 No such file or directory; time_t 8 bytes\n",
        "{}",
        stderr(&out)
    );
    out
}

/// The same for WASI 0.2, whose programs are components. The exit status is
/// only success or failure there: wasi-libc ends a run with `wasi:cli/exit`,
/// whose status-code form is still unstable.
#[test]
fn a_program_for_wasm32_wasip2() {
    let Some(wasmtime) = wasmtime_for("wasm32-wasip2") else {
        return;
    };
    let s = Scratch::new("wasip2");
    s.write("main.c", WASI_PROGRAM);
    s.write("twice.c", "int twice(int x) { return 2 * x; }\n");
    s.compile(&[
        "--target",
        "wasm32-wasip2",
        "main.c",
        "twice.c",
        "-o",
        "prog.wasm",
    ]);
    let out = run_wasm(&wasmtime, &s);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
}
