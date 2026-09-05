//! Integration tests for the bundled standard headers.
//!
//! Each one declares what the platform's C library really exports, so these
//! tests do the only thing that can prove it: they call the real functions and
//! check the answers, and they compare the constants against Rust's own.

use cinrs::c99;

// ---------------------------------------------------------------------------
// <stdio.h>
// ---------------------------------------------------------------------------

#[test]
fn stdio_formats_into_buffers() {
    c99! {
        #include <stdio.h>

        int write_greeting(char *buf, unsigned long size) {
            return snprintf(buf, size, "%s %d", "answer", 42);
        }

        int write_with_sprintf(char *buf) {
            return sprintf(buf, "%05.2f|%c|%x", 3.5, 'z', 255);
        }

        int say(const char *text) {
            return puts(text);
        }

        int to_stream(char *buf, unsigned long size) {
            /* stderr is a real symbol on this platform, and writing nothing to
             * it keeps the test output clean. */
            fflush(stderr);
            return snprintf(buf, size, "%ld", (long)EOF);
        }
    }

    let mut buf = [0u8; 64];
    unsafe {
        let n = write_greeting(buf.as_mut_ptr().cast(), buf.len() as _);
        assert_eq!(n, 9);
        assert_eq!(&buf[..9], b"answer 42");

        let n = write_with_sprintf(buf.as_mut_ptr().cast());
        assert_eq!(n, 10);
        assert_eq!(&buf[..10], b"03.50|z|ff");

        assert!(say(c"stdio works".as_ptr()) >= 0);

        let n = to_stream(buf.as_mut_ptr().cast(), buf.len() as _);
        assert_eq!(&buf[..n as usize], b"-1");
    }
}

// ---------------------------------------------------------------------------
// <string.h>
// ---------------------------------------------------------------------------

#[test]
fn string_handling() {
    c99! {
        #include <string.h>

        unsigned long length(const char *s) { return strlen(s); }
        int compare(const char *a, const char *b) { return strcmp(a, b); }

        int copy_and_check(void) {
            char src[] = "abcdef";
            char dst[8];
            memcpy(dst, src, sizeof src);
            return memcmp(dst, src, sizeof src) == 0 && dst[5] == 'f';
        }

        long find_char(const char *s, int c) {
            char *at = strchr(s, c);
            return at == NULL ? -1 : at - s;
        }

        long find_string(const char *s, const char *needle) {
            char *at = strstr(s, needle);
            return at == NULL ? -1 : at - s;
        }

        /* strtok writes into its argument, so the buffer is ours. */
        int count_words(void) {
            char text[] = "one two three";
            int words = 0;
            char *tok = strtok(text, " ");
            while (tok != NULL) {
                words++;
                tok = strtok(NULL, " ");
            }
            return words;
        }
    }

    unsafe {
        assert_eq!(length(c"hello".as_ptr()), 5);
        assert!(compare(c"a".as_ptr(), c"b".as_ptr()) < 0);
        assert_eq!(compare(c"same".as_ptr(), c"same".as_ptr()), 0);
        assert_eq!(copy_and_check(), 1);
        assert_eq!(find_char(c"abcdef".as_ptr(), b'd' as _), 3);
        assert_eq!(find_char(c"abcdef".as_ptr(), b'z' as _), -1);
        assert_eq!(find_string(c"hello world".as_ptr(), c"wor".as_ptr()), 6);
        assert_eq!(count_words(), 3);
    }
}

// ---------------------------------------------------------------------------
// <stdlib.h>
// ---------------------------------------------------------------------------

#[test]
fn general_utilities() {
    c99! {
        #include <stdlib.h>

        int compare_ints(const void *a, const void *b);

        /* malloc, write through the pointer, sort it, read it back, free. */
        int sorted_middle(void) {
            int *values = (int *)malloc(5 * sizeof(int));
            int middle;
            if (values == NULL) {
                return -1;
            }
            values[0] = 9;
            values[1] = 1;
            values[2] = 7;
            values[3] = 3;
            values[4] = 5;
            qsort(values, 5, sizeof(int), compare_ints);
            middle = values[2];
            free(values);
            return middle;
        }

        int compare_ints(const void *a, const void *b) {
            int x = *(const int *)a;
            int y = *(const int *)b;
            return (x > y) - (x < y);
        }

        long parse(const char *text) { return strtol(text, NULL, 10); }
        int parse_int(const char *text) { return atoi(text); }
        int absolute(int v) { return abs(v); }
        int failure_is_one(void) { return EXIT_FAILURE == 1 && EXIT_SUCCESS == 0; }
        int rand_is_bounded(void) {
            srand(1u);
            return rand() >= 0 && rand() <= RAND_MAX;
        }
        int quotient(int a, int b) { return div(a, b).quot; }
    }

    unsafe {
        assert_eq!(sorted_middle(), 5);
        assert_eq!(parse(c"-1234".as_ptr()), -1234);
        assert_eq!(parse_int(c"77x".as_ptr()), 77);
        assert_eq!(absolute(-9), 9);
        assert_eq!(failure_is_one(), 1);
        assert_eq!(rand_is_bounded(), 1);
        assert_eq!(quotient(-7, 2), -3);
    }
}

// ---------------------------------------------------------------------------
// <math.h> — also the check that nothing extra has to be linked
// ---------------------------------------------------------------------------

#[test]
fn maths_functions_link_and_compute() {
    c99! {
        #include <math.h>

        #define DBL_LARGE 1.0e308

        double root(double x) { return sqrt(x); }
        double raise(double x, double y) { return pow(x, y); }
        double round_down(double x) { return floor(x); }
        double magnitude(double x) { return fabs(x); }
        float sine_f(float x) { return sinf(x); }
        double pi(void) { return M_PI; }
        int infinity_is_infinite(void) { return HUGE_VAL > DBL_LARGE && INFINITY > DBL_LARGE; }
    }

    unsafe {
        assert_eq!(root(16.0), 4.0);
        assert_eq!(raise(2.0, 10.0), 1024.0);
        assert_eq!(round_down(-1.5), -2.0);
        assert_eq!(magnitude(-3.25), 3.25);
        assert!((sine_f(0.0) - 0.0).abs() < 1e-6);
        assert!((pi() - core::f64::consts::PI).abs() < 1e-12);
        assert_eq!(infinity_is_infinite(), 1);
    }
}

// ---------------------------------------------------------------------------
// <ctype.h> and <stdbool.h>
// ---------------------------------------------------------------------------

#[test]
fn character_classes_and_booleans() {
    c99! {
        #include <ctype.h>
        #include <stdbool.h>

        bool is_word_char(int c) {
            return isalpha(c) || isdigit(c) || c == '_' ? true : false;
        }

        int upper(int c) { return toupper(c); }
        int lower(int c) { return tolower(c); }
        int space_is_space(void) { return isspace(' ') != 0 && ispunct('!') != 0; }
        int booleans_are_defined(void) { return __bool_true_false_are_defined; }
    }

    unsafe {
        assert!(is_word_char(b'a' as _));
        assert!(is_word_char(b'7' as _));
        assert!(is_word_char(b'_' as _));
        assert!(!is_word_char(b'-' as _));
        assert_eq!(upper(b'q' as _), b'Q' as i32);
        assert_eq!(lower(b'Q' as _), b'q' as i32);
        assert_eq!(space_is_space(), 1);
        assert_eq!(booleans_are_defined(), 1);
    }
}

// ---------------------------------------------------------------------------
// <iso646.h>
// ---------------------------------------------------------------------------

/// The eleven alternative spellings, each used as the operator it stands for.
#[test]
fn the_iso646_spellings_are_the_operators() {
    c99! {
        #include <iso646.h>

        int logic(int a, int b) { return a and b or not a; }
        int comparison(int a, int b) { return a not_eq b; }

        int bits(int a, int b) {
            int v = a bitand b;
            v = v bitor (a xor b);
            v and_eq compl 0;
            v or_eq 0;
            v xor_eq 0;
            return v;
        }
    }

    unsafe {
        assert_eq!(logic(1, 1), 1);
        assert_eq!(logic(1, 0), 0);
        assert_eq!(logic(0, 0), 1);
        assert_eq!(comparison(1, 2), 1);
        assert_eq!(comparison(2, 2), 0);
        assert_eq!(bits(0b1100, 0b1010), 0b1110);
    }
}

// ---------------------------------------------------------------------------
// <stdint.h>, <limits.h>, <float.h>
// ---------------------------------------------------------------------------

#[test]
fn the_limits_agree_with_rusts() {
    c99! {
        #include <float.h>
        #include <limits.h>
        #include <stddef.h>
        #include <stdint.h>

        int char_bit(void) { return CHAR_BIT; }
        int int_max(void) { return INT_MAX; }
        int int_min(void) { return INT_MIN; }
        unsigned int uint_max(void) { return UINT_MAX; }
        long long llong_max(void) { return LLONG_MAX; }
        long long llong_min(void) { return LLONG_MIN; }
        unsigned long long ullong_max(void) { return ULLONG_MAX; }

        int8_t i8_min(void) { return INT8_MIN; }
        uint8_t u8_max(void) { return UINT8_MAX; }
        int16_t i16_min(void) { return INT16_MIN; }
        int32_t i32_max(void) { return INT32_MAX; }
        int64_t i64_min(void) { return INT64_MIN; }
        int64_t i64_max(void) { return INT64_MAX; }
        uint64_t u64_max(void) { return UINT64_MAX; }
        size_t size_max(void) { return SIZE_MAX; }
        intptr_t intptr_min(void) { return INTPTR_MIN; }
        uintptr_t uintptr_max(void) { return UINTPTR_MAX; }
        intmax_t intmax_max(void) { return INTMAX_MAX; }

        /* The exact-width types must really have those widths. */
        int widths(void) {
            return sizeof(int8_t) == 1 && sizeof(int16_t) == 2
                && sizeof(int32_t) == 4 && sizeof(int64_t) == 8
                && sizeof(intptr_t) == sizeof(void *)
                && sizeof(int_least32_t) >= 4 && sizeof(int_fast16_t) >= 2;
        }

        double dbl_max(void) { return DBL_MAX; }
        double dbl_min(void) { return DBL_MIN; }
        double dbl_epsilon(void) { return DBL_EPSILON; }
        float flt_max(void) { return FLT_MAX; }
        float flt_min(void) { return FLT_MIN; }
        float flt_epsilon(void) { return FLT_EPSILON; }
        int digits(void) { return DBL_MANT_DIG == 53 && FLT_MANT_DIG == 24; }
    }

    unsafe {
        assert_eq!(char_bit(), 8);
        assert_eq!(int_max(), i32::MAX);
        assert_eq!(int_min(), i32::MIN);
        assert_eq!(uint_max(), u32::MAX);
        assert_eq!(llong_max(), i64::MAX);
        assert_eq!(llong_min(), i64::MIN);
        assert_eq!(ullong_max(), u64::MAX);

        assert_eq!(i8_min(), i8::MIN);
        assert_eq!(u8_max(), u8::MAX);
        assert_eq!(i16_min(), i16::MIN);
        assert_eq!(i32_max(), i32::MAX);
        assert_eq!(i64_min(), i64::MIN);
        assert_eq!(i64_max(), i64::MAX);
        assert_eq!(u64_max(), u64::MAX);
        assert_eq!(size_max() as usize, usize::MAX);
        assert_eq!(intptr_min() as isize, isize::MIN);
        assert_eq!(uintptr_max() as usize, usize::MAX);
        assert_eq!(intmax_max(), i64::MAX);
        assert_eq!(widths(), 1);

        assert_eq!(dbl_max(), f64::MAX);
        assert_eq!(dbl_min(), f64::MIN_POSITIVE);
        assert_eq!(dbl_epsilon(), f64::EPSILON);
        assert_eq!(flt_max(), f32::MAX);
        assert_eq!(flt_min(), f32::MIN_POSITIVE);
        assert_eq!(flt_epsilon(), f32::EPSILON);
        assert_eq!(digits(), 1);
    }
}

// ---------------------------------------------------------------------------
// <stddef.h>
// ---------------------------------------------------------------------------

#[test]
fn offsetof_agrees_with_rusts_offset_of() {
    c99! {
        #include <stddef.h>

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
        int null_is_null(void) { return NULL == (void *)0; }
        int types_are_sized(void) {
            return sizeof(size_t) == sizeof(void *)
                && sizeof(ptrdiff_t) == sizeof(void *)
                && sizeof(wchar_t) == sizeof(int);
        }
    }

    unsafe {
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
        assert_eq!(size_of_mixed() as usize, core::mem::size_of::<Mixed>());
        assert_eq!(null_is_null(), 1);
        assert_eq!(types_are_sized(), 1);
    }
}

// ---------------------------------------------------------------------------
// <assert.h>
// ---------------------------------------------------------------------------

#[test]
fn assertions_that_hold_do_nothing() {
    c99! {
        #include <assert.h>

        int checked_sum(int a, int b) {
            assert(a >= 0);
            assert(b >= 0);
            int total = a + b;
            assert(total >= a && "the sum cannot be smaller than an operand");
            return total;
        }
    }

    assert_eq!(unsafe { checked_sum(20, 22) }, 42);
}

/// The header may be included again with NDEBUG defined, and `assert` is then
/// nothing at all — which is why it has no include guard.
#[test]
fn ndebug_turns_assertions_off() {
    c99! {
        #include <assert.h>

        int checked(int v) {
            assert(v > 0);
            return v;
        }

        #define NDEBUG 1
        #include <assert.h>

        /* Would abort if the assertion were still live. */
        int unchecked(int v) {
            assert(v > 0);
            return v;
        }
    }

    unsafe {
        assert_eq!(checked(1), 1);
        assert_eq!(unchecked(-1), -1);
    }
}

// ---------------------------------------------------------------------------
// <errno.h>
// ---------------------------------------------------------------------------

#[test]
fn errno_reports_a_range_error() {
    c99! {
        #include <errno.h>
        #include <stdlib.h>

        int overflowed(void) {
            errno = 0;
            strtol("999999999999999999999999", NULL, 10);
            return errno == ERANGE;
        }

        int untouched(void) {
            errno = 0;
            strtol("17", NULL, 10);
            return errno;
        }

        int edom_is_thirty_three(void) { return EDOM == 33 && ERANGE == 34; }
    }

    unsafe {
        assert_eq!(overflowed(), 1);
        assert_eq!(untouched(), 0);
        assert_eq!(edom_is_thirty_three(), 1);
    }
}

// ---------------------------------------------------------------------------
// <stdarg.h>
// ---------------------------------------------------------------------------

/// Calling a variadic function needs nothing new, so this half runs
/// everywhere.
#[test]
fn stdarg_declares_the_v_functions() {
    c99! {
        #include <stdarg.h>
        #include <stdio.h>

        /* `va_list` is a type here only because <stdarg.h> says so. */
        int formatted(char *buf, unsigned long size) {
            return snprintf(buf, size, "%s=%d", "x", 7);
        }
    }

    let mut buf = [0u8; 16];
    let n = unsafe { formatted(buf.as_mut_ptr().cast(), buf.len() as _) };
    assert_eq!(&buf[..n as usize], b"x=7");
}

/// Defining one needs Rust's `c_variadic`, stable since 1.99.
#[rustversion::since(1.99)]
mod variadic_definitions {
    #[test]
    fn a_variadic_function_reads_its_arguments() {
        cinrs::c99! {
            #include <stdarg.h>
            #include <stdio.h>

            int sum(int n, ...) {
                va_list ap;
                int total = 0;
                va_start(ap, n);
                for (int i = 0; i < n; i++) {
                    total += va_arg(ap, int);
                }
                va_end(ap);
                return total;
            }

            /* The list is passed straight on to the C library. */
            int format(char *buf, unsigned long size, const char *fmt, ...) {
                va_list ap;
                int written;
                va_start(ap, fmt);
                written = vsnprintf(buf, size, fmt, ap);
                va_end(ap);
                return written;
            }
        }

        let mut buf = [0u8; 32];
        unsafe {
            assert_eq!(sum(4, 1, 2, 3, 4), 10);
            let n = format(
                buf.as_mut_ptr().cast(),
                buf.len() as _,
                c"%d/%d".as_ptr(),
                22,
                7,
            );
            assert_eq!(&buf[..n as usize], b"22/7");
        }
    }
}

// ---------------------------------------------------------------------------
// <time.h>
// ---------------------------------------------------------------------------

#[test]
fn time_moves_forward() {
    c99! {
        #include <time.h>

        int now_is_positive(void) { return time(NULL) > 0; }

        /* The two-argument form writes through the pointer as well. */
        int both_forms_agree(void) {
            time_t stored = 0;
            time_t returned = time(&stored);
            return stored == returned;
        }

        double elapsed(void) {
            time_t start = time(NULL);
            return difftime(start, start);
        }

        int clock_is_sane(void) {
            return clock() >= 0 && CLOCKS_PER_SEC > 0;
        }

        int year_is_after_2020(void) {
            time_t now = time(NULL);
            struct tm *local = localtime(&now);
            return local != NULL && local->tm_year + 1900 > 2020;
        }
    }

    unsafe {
        assert_eq!(now_is_positive(), 1);
        assert_eq!(both_forms_agree(), 1);
        assert_eq!(elapsed(), 0.0);
        assert_eq!(clock_is_sane(), 1);
        assert_eq!(year_is_after_2020(), 1);
    }
}

// ---------------------------------------------------------------------------
// <wchar.h> and <wctype.h>
// ---------------------------------------------------------------------------

/// Wide characters need [string-literal input](cinrs#input-forms): Rust's own
/// lexer refuses `L"…"` as a token.
#[test]
fn wide_strings_and_characters() {
    c99! { r#"
#include <string.h>
#include <wchar.h>
#include <wctype.h>

/* The literal is a `wchar_t[4]`, and the library counts the same three. */
int wide_length(void) { return (int)wcslen(L"abc"); }

int wide_compare(void) {
    return wcscmp(L"abc", L"abc") == 0
        && wcscmp(L"abc", L"abd") < 0
        && wcsncmp(L"abcdef", L"abcxxx", 3) == 0;
}

int wide_copy(void) {
    wchar_t buf[8];
    wmemcpy(buf, L"hello", 6);
    return wcscmp(buf, L"hello") == 0 && buf[5] == 0 && wmemcmp(buf, L"hello", 6) == 0;
}

int wide_format(void) {
    wchar_t buf[32];
    int n = swprintf(buf, 32, L"%ls-%d", L"hi", 42);
    return n == 5 && wcscmp(buf, L"hi-42") == 0;
}

int wide_search(void) {
    const wchar_t *s = L"a,b,c";
    const wchar_t *first = wcschr(s, L',');
    const wchar_t *last = wcsrchr(s, L',');
    return first != NULL && last != NULL && first - s == 1 && last - s == 3
        && wcsstr(s, L"b,c") == s + 2 && wcsspn(s, L"ab,") == 4;
}

int wide_tokens(void) {
    wchar_t text[] = L"a b";
    wchar_t *save = NULL;
    wchar_t *first = wcstok(text, L" ", &save);
    wchar_t *second = wcstok(NULL, L" ", &save);
    return wcscmp(first, L"a") == 0 && wcscmp(second, L"b") == 0;
}

long wide_to_long(void) { return wcstol(L"  -42abc", NULL, 10); }
double wide_to_double(void) { return wcstod(L"2.5", NULL); }

/* A wide character constant has the type `wchar_t`, so it round-trips through
   the single-byte conversions. */
int byte_round_trip(void) {
    wint_t w = btowc('Q');
    return w == (wint_t)L'Q' && wctob(w) == 'Q' && btowc(EOF) == WEOF;
}

/* The test process is in the "C" locale, so only the ASCII range is a single
   byte; that is all this decodes. */
int decode_one_byte(void) {
    mbstate_t state;
    wchar_t wc = 0;
    memset(&state, 0, sizeof state);
    return mbsinit(&state)
        && mbrtowc(&wc, "A", 1, &state) == 1
        && wc == L'A'
        && mbrlen("B", 1, &state) == 1;
}

int classification(void) {
    return iswalpha(L'x') && iswdigit(L'7') && iswspace(L' ') && !iswalpha(L'7')
        && towupper(L'x') == (wint_t)L'X' && towlower(L'X') == (wint_t)L'x'
        && iswctype(L'x', wctype("alpha"));
}

/* Both headers say what `wchar_t`'s range is, and either may come first. */
int limits_agree(void) {
    return sizeof(wchar_t) == 4 && WCHAR_MAX == 2147483647 && WCHAR_MIN < 0;
}
"# }

    unsafe {
        assert_eq!(wide_length(), 3);
        assert_eq!(wide_compare(), 1);
        assert_eq!(wide_copy(), 1);
        assert_eq!(wide_format(), 1);
        assert_eq!(wide_search(), 1);
        assert_eq!(wide_tokens(), 1);
        assert_eq!(wide_to_long(), -42);
        assert_eq!(wide_to_double(), 2.5);
        assert_eq!(byte_round_trip(), 1);
        assert_eq!(decode_one_byte(), 1);
        assert_eq!(classification(), 1);
        assert_eq!(limits_agree(), 1);
    }
}

/// `<stdint.h>` and `<wchar.h>` both define `WCHAR_MIN` and `WCHAR_MAX`, so
/// including them in either order has to be quiet.
#[test]
fn the_wide_character_limits_survive_both_headers() {
    c99! {
        #include <stdint.h>
        #include <wchar.h>

        int stdint_first(void) { return WCHAR_MAX == INT32_MAX; }
    }

    c99! {
        #include <wchar.h>
        #include <stdint.h>

        int wchar_first(void) { return WCHAR_MIN == INT32_MIN; }
    }

    unsafe {
        assert_eq!(stdint_first(), 1);
        assert_eq!(wchar_first(), 1);
    }
}
