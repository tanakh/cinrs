//! The C library of the machine the tests are running on, through the bundled
//! headers.
//!
//! Everything else about a target is decided while the macro expands, and is
//! therefore testable on any machine: `tests/cross_targets.rs` puts a real
//! `rustc` on the other side of a data model, and `tests/headers.rs` compiles
//! every bundled header for every model. Neither of them links anything, and
//! neither can: **the headers are cinrs's and the C library is the
//! platform's**, and only a program that is built, linked and run on that
//! platform says whether the two agree — whether the symbol a declaration
//! names is a symbol the library exports, whether the layout a header writes
//! down is the layout the library has, and whether the answer that comes back
//! is the one C promises.
//!
//! So this file is written to pass on Linux, macOS and Windows alike, and is
//! the one the `portability` job in `.github/workflows/ci.yml` runs on the
//! other two. Everything in it is ISO C and the bundled headers: nothing
//! POSIX-only (`<unistd.h>`, `<fcntl.h>`, `<strings.h>`, `<signal.h>`'s
//! `SIGUSR1`), nothing glibc-only, and no `#pragma cinrs link` — the C library
//! is what the Rust runtime already links on all three.
//!
//! Two calls are left out on Windows, and not because of C: `time` and
//! `difftime`, whose plain names the Windows SDK's import library does not
//! export. The `#if !defined(_WIN32)` below says the whole of it, and
//! `doc/cross-compilation.md` records it as a known gap.
//!
//! Every value is asserted **from Rust**: the C computes, Rust checks. What a
//! platform is allowed to differ about is written as the platform's own
//! answer — `sizeof(long)` is compared against `size_of::<c_long>()` rather
//! than against 8, `LONG_MAX` against `c_long::MAX`, `sizeof(wchar_t)` against
//! `size_of::<wchar_t>()` — so a difference between the two ends is a failure
//! and a difference between two machines is not.
//!
//! The unit is in [string-literal form](cinrs#input-forms) because it uses
//! `L"…"`, which Rust's own lexer refuses.

use std::ffi::CString;

use core::ffi::{c_double, c_int, c_long, c_longlong};

cinrs::c11! { r####"
#include <ctype.h>
#include <errno.h>
#include <limits.h>
#include <math.h>
#include <stdatomic.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <wchar.h>

/* -- <stdio.h>: formatting into a buffer ------------------------------ */

/* Five conversions in one call: the plain ones, `%zu` for a `size_t` and
 * `%lld` for a `long long` — the two whose width is not `int`'s — and a
 * `double`. */
int format_all(char *buf, size_t n) {
    size_t count = 7;
    long long big = -1234567890123LL;
    return snprintf(buf, n, "%d|%s|%zu|%lld|%.3f", 42, "cinrs", count, big, 0.25);
}

/* `%ld` takes a `long`, which is four bytes on Windows and eight elsewhere;
 * the value fits in both. */
int format_with_sprintf(char *buf) {
    return sprintf(buf, "%05.2f|%c|%x|%ld", 3.5, 'z', 255, -1234L);
}

/* A buffer one byte too short: `snprintf` truncates and answers what it would
 * have written. */
int format_truncated(char *buf, size_t n) {
    return snprintf(buf, n, "%s", "abcdef");
}

int parse_fields(const char *text, int *value, double *weight, char *word) {
    return sscanf(text, "%d %lf %7s", value, weight, word);
}

/* -- <stdio.h>: the standard streams ---------------------------------- */

int to_stderr(void) {
    int written = fprintf(stderr, "[cinrs portability %d]", 1);
    fflush(stderr);
    return written;
}

int say_with_fputs(const char *text) { return fputs(text, stdout); }
int say_with_puts(const char *text) { return puts(text); }
int flush_stdout(void) { return fflush(stdout); }

/* -- <stdio.h>: a real file ------------------------------------------- */

int write_file(const char *path, const char *data, size_t n) {
    FILE *stream = fopen(path, "wb");
    size_t written;
    if (stream == NULL) return -1;
    written = fwrite(data, 1, n, stream);
    if (fclose(stream) != 0) return -2;
    return (int) written;
}

int read_file(const char *path, char *buf, size_t n) {
    FILE *stream = fopen(path, "rb");
    size_t got;
    if (stream == NULL) return -1;
    got = fread(buf, 1, n, stream);
    if (ferror(stream) != 0) { fclose(stream); return -2; }
    if (feof(stream) == 0) { fclose(stream); return -3; }
    if (fclose(stream) != 0) return -4;
    return (int) got;
}

int delete_file(const char *path) { return remove(path); }

/* -- <stdlib.h>: the heap --------------------------------------------- */

int *make_ints(size_t n) { return (int *) calloc(n, sizeof(int)); }
int *grow_ints(int *p, size_t n) { return (int *) realloc(p, n * sizeof(int)); }
void set_int(int *p, size_t i, int v) { p[i] = v; }
int get_int(const int *p, size_t i) { return p[i]; }
void release(void *p) { free(p); }

/* `malloc` and `memset` on memory that never leaves C. */
int filled_block_holds_its_bytes(void) {
    unsigned char *p = (unsigned char *) malloc(64);
    int held;
    if (p == NULL) return -1;
    memset(p, 0xAB, 64);
    held = p[0] == 0xAB && p[63] == 0xAB;
    free(p);
    return held;
}

/* -- <stdlib.h>: qsort and bsearch ------------------------------------ */

static int compare_ints(const void *a, const void *b) {
    int x = *(const int *) a;
    int y = *(const int *) b;
    return (x > y) - (x < y);
}

void sort_ints(int *values, size_t n) {
    qsort(values, n, sizeof(int), compare_ints);
}

long find_int(const int *values, size_t n, int wanted) {
    const int *at = (const int *) bsearch(&wanted, values, n, sizeof(int), compare_ints);
    return at == NULL ? -1 : (long) (at - values);
}

/* -- <stdlib.h> and <errno.h>: the conversions ------------------------ */

/* `errno` is read inside the call, because anything Rust does between two
 * calls may set it. */
long parse_long(const char *text, int *err) {
    long value;
    errno = 0;
    value = strtol(text, NULL, 10);
    *err = errno;
    return value;
}

int erange(void) { return ERANGE; }

double parse_double(const char *text, size_t *used) {
    char *end = NULL;
    double value = strtod(text, &end);
    *used = (size_t) (end - text);
    return value;
}

int abs_int(int v) { return abs(v); }
long abs_long(long v) { return labs(v); }
long long abs_long_long(long long v) { return llabs(v); }

/* -- <string.h> -------------------------------------------------------- */

/* memset, strncpy, strcat, memcpy and memmove, in that order, over one
 * buffer; Rust reads the result out. */
size_t build_a_string(char *out, size_t n) {
    char work[32];
    memset(work, 0, sizeof work);
    strncpy(work, "abc", 4);
    strcat(work, "def");
    memcpy(work + 6, "ghi", 4);
    /* Overlapping, which is what separates memmove from memcpy. */
    memmove(work + 1, work, 9);
    work[10] = '\0';
    if (n > 0) {
        strncpy(out, work, n - 1);
        out[n - 1] = '\0';
    }
    return strlen(out);
}

size_t length(const char *s) { return strlen(s); }
int compare(const char *a, const char *b) { int r = strcmp(a, b); return (r > 0) - (r < 0); }
int compare_n(const char *a, const char *b, size_t n) {
    int r = strncmp(a, b, n);
    return (r > 0) - (r < 0);
}
int bytes_compare(const void *a, const void *b, size_t n) {
    int r = memcmp(a, b, n);
    return (r > 0) - (r < 0);
}
long index_of_char(const char *s, int c) {
    const char *at = strchr(s, c);
    return at == NULL ? -1 : (long) (at - s);
}
long last_index_of_char(const char *s, int c) {
    const char *at = strrchr(s, c);
    return at == NULL ? -1 : (long) (at - s);
}
long index_of_string(const char *s, const char *needle) {
    const char *at = strstr(s, needle);
    return at == NULL ? -1 : (long) (at - s);
}

/* -- <ctype.h> --------------------------------------------------------- */

int is_alpha(int c) { return isalpha(c) != 0; }
int is_digit(int c) { return isdigit(c) != 0; }
int is_alnum(int c) { return isalnum(c) != 0; }
int is_space(int c) { return isspace(c) != 0; }
int is_punct(int c) { return ispunct(c) != 0; }
int is_upper(int c) { return isupper(c) != 0; }
int is_xdigit(int c) { return isxdigit(c) != 0; }
int upper(int c) { return toupper(c); }
int lower(int c) { return tolower(c); }

/* -- <math.h> ---------------------------------------------------------- */

double root(double x) { return sqrt(x); }
double raise_to(double x, double y) { return pow(x, y); }
double round_down(double x) { return floor(x); }
double round_up(double x) { return ceil(x); }
double remainder_of(double x, double y) { return fmod(x, y); }
double magnitude(double x) { return fabs(x); }
double sine(double x) { return sin(x); }

/* -- <time.h> ---------------------------------------------------------- */

/* `(clock_t)-1` is the one value that means "no clock". */
int clock_is_available(void) { return clock() != (clock_t) -1; }

/* `strftime` over a `struct tm` this code fills in by hand, which is how the
 * *layout* of that structure gets tested: the bundled <time.h> gives it the two
 * BSD members on the Unix platforms and the nine standard ones on Windows, and
 * a header that had it wrong would hand the library the wrong bytes. Every
 * field is set, and to a consistent date — 2025-01-02 was a Thursday — because
 * the Microsoft library validates what it is given. */
size_t format_a_date(char *buf, size_t n) {
    struct tm when;
    memset(&when, 0, sizeof when);
    when.tm_year = 125; /* 1900 + 125 */
    when.tm_mon = 0;
    when.tm_mday = 2;
    when.tm_hour = 3;
    when.tm_min = 4;
    when.tm_sec = 5;
    when.tm_wday = 4;
    when.tm_yday = 1;
    when.tm_isdst = 0;
    return strftime(buf, n, "%Y-%m-%d %H:%M:%S", &when);
}

/* `time` and `difftime` are left out on Windows, and the reason is a link error
 * rather than anything about C: the Windows SDK's `ucrt.lib` — the import
 * library `rustc` links for a `*-windows-msvc` target — exports `_time64`,
 * `_difftime64`, `_mktime64`, `_localtime64`, `_gmtime64` and `_ctime64` and
 * *not* the plain names, which are macros in Microsoft's own <time.h>. cinrs's
 * bundled <time.h> declares the plain ones, so a unit that calls them does not
 * link there; `clock`, `asctime` and `strftime` are ordinary exports and do.
 * See the "Cross-compilation" section of the documentation. mingw-w64 is not
 * affected — `ucrtbase.dll` does export the plain names — but `_WIN32` is all
 * the C can ask about, so both are left out here. */
#if !defined(_WIN32)

int now_is_positive(void) { return time(NULL) > 0; }

/* The two-argument form writes the same value through the pointer. */
int both_forms_of_time_agree(void) {
    time_t stored = 0;
    time_t returned = time(&stored);
    return stored == returned;
}

double seconds_between(int seconds) {
    time_t now = time(NULL);
    return difftime(now + seconds, now);
}

#endif

/* -- <wchar.h> --------------------------------------------------------- */

size_t wide_length(void) { return wcslen(L"abcd"); }
size_t wide_char_size(void) { return sizeof(wchar_t); }

/* -- <limits.h>, <stdint.h>, <stddef.h> -------------------------------- */

int char_bit(void) { return CHAR_BIT; }
int int_max(void) { return INT_MAX; }
long long_max(void) { return LONG_MAX; }
long long_min(void) { return LONG_MIN; }
long long llong_max(void) { return LLONG_MAX; }
size_t size_max(void) { return SIZE_MAX; }
size_t size_of_long(void) { return sizeof(long); }
size_t size_of_pointer(void) { return sizeof(void *); }
int64_t i64_min(void) { return INT64_MIN; }
uint32_t u32_max(void) { return UINT32_MAX; }

struct Mixed {
    char tag;
    int count;
    double weight;
    char name[4];
};

size_t offset_of_tag(void) { return offsetof(struct Mixed, tag); }
size_t offset_of_count(void) { return offsetof(struct Mixed, count); }
size_t offset_of_weight(void) { return offsetof(struct Mixed, weight); }
size_t offset_of_name(void) { return offsetof(struct Mixed, name); }
size_t size_of_mixed(void) { return sizeof(struct Mixed); }

/* -- <stdatomic.h> ----------------------------------------------------- */

atomic_int counter;

void bump(void) { atomic_fetch_add(&counter, 1); }
int counter_value(void) { return atomic_load(&counter); }

/* -- the language, where the generated code is the whole answer -------- */

struct flags {
    unsigned kind : 3;
    unsigned rest : 13;
};

unsigned bitfield_kind(unsigned kind, unsigned rest) {
    struct flags f;
    f.kind = kind;
    f.rest = rest;
    return f.kind;
}

unsigned bitfield_rest(unsigned kind, unsigned rest) {
    struct flags f;
    f.kind = kind;
    f.rest = rest;
    return f.rest;
}

/* A variable length array, which is a `Vec` in the generated Rust. */
int vla_sum(int n) {
    int squares[n];
    int total = 0;
    for (int i = 0; i < n; i++) squares[i] = i * i;
    for (int i = 0; i < n; i++) total += squares[i];
    return total;
}

/* A backward `goto`, which is a state machine in the generated Rust. */
int first_divisor(int n) {
    int i = 2;
loop:
    if (i * i > n) goto done;
    if (n % i == 0) return i;
    i++;
    goto loop;
done:
    return n;
}
"#### }

// ---------------------------------------------------------------------------
// <stdio.h>
// ---------------------------------------------------------------------------

#[test]
fn formatting_into_a_buffer() {
    let mut buf = [0u8; 64];
    unsafe {
        let n = format_all(buf.as_mut_ptr().cast(), buf.len() as _);
        assert_eq!(n, 31);
        assert_eq!(&buf[..n as usize], b"42|cinrs|7|-1234567890123|0.250");

        let n = format_with_sprintf(buf.as_mut_ptr().cast());
        assert_eq!(n, 16);
        assert_eq!(&buf[..n as usize], b"03.50|z|ff|-1234");

        // C99 7.19.6.5p3: the answer is what *would* have been written, and
        // the buffer holds the truncation with its terminator.
        buf.fill(0);
        let n = format_truncated(buf.as_mut_ptr().cast(), 4);
        assert_eq!(n, 6);
        assert_eq!(&buf[..4], b"abc\0");
    }
}

#[test]
fn scanning_a_string() {
    let mut value: c_int = 0;
    let mut weight: c_double = 0.0;
    let mut word = [0u8; 8];
    let n = unsafe {
        parse_fields(
            c"42 2.5 cinrs".as_ptr(),
            &mut value,
            &mut weight,
            word.as_mut_ptr().cast(),
        )
    };
    assert_eq!(n, 3);
    assert_eq!(value, 42);
    assert_eq!(weight, 2.5);
    assert_eq!(&word[..6], b"cinrs\0");
}

#[test]
fn the_standard_streams() {
    unsafe {
        // The text goes to the harness's captured output; what is being tested
        // is that the stream pointer the bundled header names is the one the
        // platform's library exports.
        assert_eq!(to_stderr(), 21);
        assert!(say_with_fputs(c"[cinrs portability]".as_ptr()) >= 0);
        assert!(say_with_puts(c" ok".as_ptr()) >= 0);
        assert_eq!(flush_stdout(), 0);
    }
}

#[test]
fn a_file_in_the_temporary_directory() {
    // The path is Rust's: `std::env::temp_dir()` knows where it is on each
    // platform, and C is handed the answer.
    let path = std::env::temp_dir().join(format!("cinrs-portability-{}.tmp", std::process::id()));
    let c_path = CString::new(
        path.to_str()
            .expect("a temporary directory with a UTF-8 path"),
    )
    .expect("a path with no NUL in it");
    let data = b"cinrs portability";

    unsafe {
        let written = write_file(c_path.as_ptr(), data.as_ptr().cast(), data.len() as _);
        assert_eq!(written, data.len() as c_int, "fwrite wrote {written} bytes");
        assert!(path.exists(), "{} was not created", path.display());

        let mut buf = [0u8; 64];
        let got = read_file(c_path.as_ptr(), buf.as_mut_ptr().cast(), buf.len() as _);
        assert_eq!(got, data.len() as c_int, "fread read {got} bytes");
        assert_eq!(&buf[..data.len()], data);

        assert_eq!(delete_file(c_path.as_ptr()), 0);
    }
    assert!(!path.exists(), "{} was not removed", path.display());
}

// ---------------------------------------------------------------------------
// <stdlib.h>
// ---------------------------------------------------------------------------

#[test]
fn the_heap() {
    unsafe {
        let mut p = make_ints(4);
        assert!(!p.is_null(), "calloc returned null");
        // `calloc` zeroes, which is the difference from `malloc`.
        assert_eq!(get_int(p, 0), 0);
        assert_eq!(get_int(p, 3), 0);
        set_int(p, 3, 42);
        assert_eq!(get_int(p, 3), 42);
        // `realloc` keeps what was there.
        p = grow_ints(p, 64);
        assert!(!p.is_null(), "realloc returned null");
        assert_eq!(get_int(p, 3), 42);
        release(p.cast());

        assert_eq!(filled_block_holds_its_bytes(), 1);
    }
}

#[test]
fn sorting_and_searching() {
    let mut values: [c_int; 7] = [9, 1, 7, 3, 5, 8, 2];
    unsafe { sort_ints(values.as_mut_ptr(), values.len() as _) };
    assert_eq!(values, [1, 2, 3, 5, 7, 8, 9]);
    for (index, value) in values.iter().enumerate() {
        let found = unsafe { find_int(values.as_ptr(), values.len() as _, *value) };
        assert_eq!(found, index as c_long, "bsearch for {value}");
    }
    assert_eq!(
        unsafe { find_int(values.as_ptr(), values.len() as _, 4) },
        -1
    );
}

/// `strtol` on a number no `long` can hold: `LONG_MAX` and `ERANGE`. What
/// `LONG_MAX` *is* follows the platform — `long` is 32 bits on Windows and 64
/// on the Unix platforms — so the expectation is `c_long::MAX` rather than a
/// number.
#[test]
fn string_to_number() {
    let mut err: c_int = 0;
    unsafe {
        assert_eq!(parse_long(c"-1234".as_ptr(), &mut err), -1234);
        assert_eq!(err, 0);

        let huge = c"999999999999999999999999999999";
        assert_eq!(parse_long(huge.as_ptr(), &mut err), c_long::MAX);
        assert_eq!(err, erange());
        assert_eq!(erange(), 34);

        let mut used: size_t = 0;
        assert_eq!(parse_double(c"2.5abc".as_ptr(), &mut used), 2.5);
        assert_eq!(used as usize, 3);
        assert_eq!(parse_double(c"-0.125".as_ptr(), &mut used), -0.125);
        assert_eq!(used as usize, 6);

        assert_eq!(abs_int(-9), 9);
        assert_eq!(abs_long(-1234), 1234);
        assert_eq!(abs_long_long(-1234567890123), 1234567890123 as c_longlong);
    }
}

// ---------------------------------------------------------------------------
// <string.h> and <ctype.h>
// ---------------------------------------------------------------------------

#[test]
fn string_handling() {
    let mut buf = [0u8; 16];
    unsafe {
        let n = build_a_string(buf.as_mut_ptr().cast(), buf.len() as _);
        assert_eq!(n as usize, 10);
        assert_eq!(&buf[..10], b"aabcdefghi");

        assert_eq!(length(c"hello".as_ptr()) as usize, 5);
        assert_eq!(compare(c"a".as_ptr(), c"b".as_ptr()), -1);
        assert_eq!(compare(c"b".as_ptr(), c"a".as_ptr()), 1);
        assert_eq!(compare(c"same".as_ptr(), c"same".as_ptr()), 0);
        assert_eq!(compare_n(c"abcdef".as_ptr(), c"abcxyz".as_ptr(), 3), 0);
        assert_eq!(compare_n(c"abcdef".as_ptr(), c"abcxyz".as_ptr(), 4), -1);

        let a = b"abcd";
        let b = b"abce";
        assert_eq!(bytes_compare(a.as_ptr().cast(), a.as_ptr().cast(), 4), 0);
        assert_eq!(bytes_compare(a.as_ptr().cast(), b.as_ptr().cast(), 4), -1);
        assert_eq!(bytes_compare(b.as_ptr().cast(), a.as_ptr().cast(), 4), 1);

        assert_eq!(index_of_char(c"a,b,c".as_ptr(), i32::from(b',')), 1);
        assert_eq!(last_index_of_char(c"a,b,c".as_ptr(), i32::from(b',')), 3);
        assert_eq!(index_of_char(c"a,b,c".as_ptr(), i32::from(b'z')), -1);
        assert_eq!(index_of_string(c"hello world".as_ptr(), c"wor".as_ptr()), 6);
        assert_eq!(
            index_of_string(c"hello world".as_ptr(), c"xyz".as_ptr()),
            -1
        );
    }
}

#[test]
fn character_classification() {
    unsafe {
        for (c, alpha, digit, alnum, space, punct, upper_case, xdigit) in [
            (b'a', 1, 0, 1, 0, 0, 0, 1),
            (b'z', 1, 0, 1, 0, 0, 0, 0),
            (b'Q', 1, 0, 1, 0, 0, 1, 0),
            (b'7', 0, 1, 1, 0, 0, 0, 1),
            (b' ', 0, 0, 0, 1, 0, 0, 0),
            (b'\t', 0, 0, 0, 1, 0, 0, 0),
            (b'!', 0, 0, 0, 0, 1, 0, 0),
        ] {
            let c = c_int::from(c);
            assert_eq!(is_alpha(c), alpha, "isalpha({c})");
            assert_eq!(is_digit(c), digit, "isdigit({c})");
            assert_eq!(is_alnum(c), alnum, "isalnum({c})");
            assert_eq!(is_space(c), space, "isspace({c})");
            assert_eq!(is_punct(c), punct, "ispunct({c})");
            assert_eq!(is_upper(c), upper_case, "isupper({c})");
            assert_eq!(is_xdigit(c), xdigit, "isxdigit({c})");
        }
        assert_eq!(upper(c_int::from(b'q')), c_int::from(b'Q'));
        assert_eq!(lower(c_int::from(b'Q')), c_int::from(b'q'));
        // A character with no other case is itself.
        assert_eq!(upper(c_int::from(b'7')), c_int::from(b'7'));
    }
}

// ---------------------------------------------------------------------------
// <math.h> and <time.h>
// ---------------------------------------------------------------------------

#[test]
fn the_maths_functions() {
    unsafe {
        assert_eq!(root(16.0), 4.0);
        assert_eq!(raise_to(2.0, 10.0), 1024.0);
        assert_eq!(round_down(-1.5), -2.0);
        assert_eq!(round_up(-1.5), -1.0);
        assert_eq!(remainder_of(7.5, 2.0), 1.5);
        assert_eq!(magnitude(-3.25), 3.25);
        assert_eq!(sine(0.0), 0.0);
    }
}

#[test]
fn the_clock_and_a_formatted_date() {
    let mut buf = [0u8; 32];
    unsafe {
        assert_eq!(clock_is_available(), 1);
        // `strftime` reads a `struct tm` the C filled in, so this is the
        // bundled header's layout of it against the platform library's.
        let n = format_a_date(buf.as_mut_ptr().cast(), buf.len() as _);
        assert_eq!(n as usize, 19);
        assert_eq!(&buf[..19], b"2025-01-02 03:04:05");
    }
}

/// `time` and `difftime`, which are not on Windows: see the comment above the
/// `#if !defined(_WIN32)` in the unit — the Windows SDK's import library has
/// `_time64` and not `time`, so a unit calling the C name does not link there.
#[cfg(not(windows))]
#[test]
fn the_calendar() {
    unsafe {
        assert_eq!(now_is_positive(), 1, "time(NULL) was not positive");
        assert_eq!(both_forms_of_time_agree(), 1);
        assert_eq!(seconds_between(0), 0.0);
        assert_eq!(seconds_between(2), 2.0);
    }
}

// ---------------------------------------------------------------------------
// <wchar.h>
// ---------------------------------------------------------------------------

/// `wchar_t` is two bytes on Windows and four everywhere else, and an `L"…"`
/// literal is an array of whatever it is — so the length the library counts
/// and the size Rust sees have to agree with each other and with the platform.
#[test]
fn wide_characters() {
    unsafe {
        assert_eq!(wide_length() as usize, 4);
        assert_eq!(wide_char_size() as usize, size_of::<wchar_t>());
    }
    assert_eq!(
        size_of::<wchar_t>(),
        if cfg!(windows) { 2 } else { 4 },
        "the width of wchar_t"
    );
}

// ---------------------------------------------------------------------------
// <limits.h>, <stdint.h>, <stddef.h>
// ---------------------------------------------------------------------------

/// Every constant against Rust's own, so that a header that is wrong about the
/// platform is a failure rather than a number nobody checks.
#[test]
fn the_limits_and_the_layouts() {
    unsafe {
        assert_eq!(char_bit(), 8);
        assert_eq!(int_max(), c_int::MAX);
        assert_eq!(long_max(), c_long::MAX);
        assert_eq!(long_min(), c_long::MIN);
        assert_eq!(llong_max(), c_longlong::MAX);
        assert_eq!(size_max() as usize, usize::MAX);
        assert_eq!(size_of_long() as usize, size_of::<c_long>());
        assert_eq!(size_of_pointer() as usize, size_of::<*const u8>());
        assert_eq!(i64_min(), i64::MIN);
        assert_eq!(u32_max(), u32::MAX);

        assert_eq!(offset_of_tag() as usize, core::mem::offset_of!(Mixed, tag));
        assert_eq!(
            offset_of_count() as usize,
            core::mem::offset_of!(Mixed, count)
        );
        assert_eq!(
            offset_of_weight() as usize,
            core::mem::offset_of!(Mixed, weight)
        );
        assert_eq!(
            offset_of_name() as usize,
            core::mem::offset_of!(Mixed, name)
        );
        assert_eq!(size_of_mixed() as usize, size_of::<Mixed>());
    }
}

// ---------------------------------------------------------------------------
// <stdatomic.h> and the language
// ---------------------------------------------------------------------------

#[test]
fn atomic_increments() {
    unsafe {
        assert_eq!(counter_value(), 0);
        for expected in 1..=5 {
            bump();
            assert_eq!(counter_value(), expected);
        }
    }
}

#[test]
fn bit_fields_a_vla_and_a_goto() {
    unsafe {
        // Three bits and thirteen: the wider value survives, and the narrower
        // one is truncated to what fits.
        assert_eq!(bitfield_kind(5, 1000), 5);
        assert_eq!(bitfield_kind(13, 0), 13 & 7);
        assert_eq!(bitfield_rest(0, 8191), 8191);
        assert_eq!(bitfield_rest(7, 9000), 9000 & 8191);
        assert_eq!(size_of::<flags>(), 4);

        // The squares of 0..5, which the C sums out of an array whose length is
        // not known until the call.
        assert_eq!(vla_sum(5), 1 + 4 + 9 + 16);
        assert_eq!(vla_sum(0), 0);

        assert_eq!(first_divisor(91), 7);
        assert_eq!(first_divisor(97), 97);
    }
}

// ---------------------------------------------------------------------------
// _Complex
// ---------------------------------------------------------------------------

/// C's complex arithmetic, which is the one thing the generated code needs a
/// runtime for — `cinrs_rt` — and which is therefore behind the same feature.
/// Its own unit, so that the file still compiles with the feature off.
#[cfg(feature = "complex")]
mod complex_arithmetic {
    use cinrs::rt::Complex;

    cinrs::c11! {
        #include <complex.h>

        double _Complex times(double _Complex a, double _Complex b) { return a * b; }
        double real_part(double _Complex z) { return creal(z); }
    }

    #[test]
    fn multiplying_two_complex_numbers() {
        let a = Complex::new(1.0f64, 2.0);
        let b = Complex::new(3.0f64, -4.0);
        // (1 + 2i)(3 - 4i) = 3 - 4i + 6i + 8 = 11 + 2i
        assert_eq!(unsafe { times(a, b) }, Complex::new(11.0, 2.0));
        assert_eq!(unsafe { real_part(a) }, 1.0);
    }
}
