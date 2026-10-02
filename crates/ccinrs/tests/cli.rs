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

    /// Runs `ccinrs` in the directory.
    fn ccinrs(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ccinrs"))
            .args(args)
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
    s.compile(&["-O2", "-fno-cinrs-checks", "align.c", "-o", "unchecked"]);
    let out = s.run("unchecked", &[]);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "16843009\n");
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
}
