//! Integration tests that *run* translated C using pointers, arrays and
//! strings, including calls into the real libc.
//!
//! Every expected value here comes from C's semantics — pointer arithmetic in
//! units of the pointee, arrays decaying to pointers, `sizeof` measuring the
//! whole array — rather than from what the implementation happens to produce.
//!
//! Each test holds its own `c99!` invocation, which is one translation unit;
//! putting it inside the test function keeps the generated items local, so
//! names never collide between tests.

use cinrs::c99;

// ---------------------------------------------------------------------------
// pointers
// ---------------------------------------------------------------------------

#[test]
fn pointer_swap_and_indirection() {
    c99! {
        void swap(int *a, int *b) {
            int t = *a;
            *a = *b;
            *b = t;
        }

        int add_through(int *p, int n) {
            *p += n;
            return *p;
        }

        int deref_const(const int *p) {
            return *p;
        }
    }

    let mut a: core::ffi::c_int = 3;
    let mut b: core::ffi::c_int = 7;
    unsafe {
        swap(&raw mut a, &raw mut b);
        assert_eq!((a, b), (7, 3));
        assert_eq!(add_through(&raw mut a, 5), 12);
        assert_eq!(a, 12);
        assert_eq!(deref_const(&raw const a), 12);
    }
}

#[test]
fn pointer_arithmetic_and_offset_from() {
    c99! {
        int nth(int *p, int n) {
            return *(p + n);
        }

        int back(int *p, int n) {
            int *q = p + n;
            q -= 1;
            return *q;
        }

        long distance(int *a, int *b) {
            return b - a;
        }

        int walk(int *p, int n) {
            int total = 0;
            int *end = p + n;
            while (p != end) {
                total += *p++;
            }
            return total;
        }
    }

    let mut values: [core::ffi::c_int; 5] = [10, 20, 30, 40, 50];
    let base = (&raw mut values).cast::<core::ffi::c_int>();
    unsafe {
        assert_eq!(nth(base, 0), 10);
        assert_eq!(nth(base, 3), 40);
        assert_eq!(back(base, 3), 30);
        assert_eq!(distance(base, base.offset(4)), 4);
        assert_eq!(distance(base.offset(4), base), -4);
        assert_eq!(walk(base, 5), 150);
    }
}

#[test]
fn null_pointers() {
    c99! {
        int is_null(int *p) {
            return p == 0;
        }

        int is_null_cast(int *p) {
            return p == (void *) 0;
        }

        int negated(int *p) {
            return !p;
        }

        int as_condition(int *p) {
            if (p) {
                return *p;
            }
            return -1;
        }

        int *null_pointer(void) {
            return 0;
        }

        int roundtrip(int *p) {
            unsigned long bits = (unsigned long) p;
            int *back = (int *) bits;
            return *back;
        }
    }

    let mut value: core::ffi::c_int = 9;
    let p = &raw mut value;
    unsafe {
        assert_eq!(is_null(p), 0);
        assert_eq!(is_null(core::ptr::null_mut()), 1);
        assert_eq!(is_null_cast(core::ptr::null_mut()), 1);
        assert_eq!(negated(p), 0);
        assert_eq!(negated(core::ptr::null_mut()), 1);
        assert_eq!(as_condition(p), 9);
        assert_eq!(as_condition(core::ptr::null_mut()), -1);
        assert!(null_pointer().is_null());
        assert_eq!(roundtrip(p), 9);
    }
}

#[test]
fn void_pointers_and_generic_copies() {
    c99! {
        void *memcpy(void *dst, const void *src, unsigned long n);

        void byte_swap(void *a, void *b, unsigned long size) {
            unsigned char *x = (unsigned char *) a;
            unsigned char *y = (unsigned char *) b;
            for (unsigned long i = 0; i < size; i++) {
                unsigned char t = x[i];
                x[i] = y[i];
                y[i] = t;
            }
        }

        int copy_int(int value) {
            int out = 0;
            memcpy(&out, &value, sizeof(int));
            return out;
        }
    }

    let mut a: core::ffi::c_double = 1.5;
    let mut b: core::ffi::c_double = -2.25;
    unsafe {
        byte_swap(
            (&raw mut a).cast(),
            (&raw mut b).cast(),
            size_of::<core::ffi::c_double>() as core::ffi::c_ulong,
        );
        assert_eq!((a, b), (-2.25, 1.5));
        assert_eq!(copy_int(1234), 1234);
    }
}

// ---------------------------------------------------------------------------
// arrays
// ---------------------------------------------------------------------------

#[test]
fn array_sum_and_in_place_reverse() {
    c99! {
        int sum(int *values, int n) {
            int total = 0;
            for (int i = 0; i < n; i++) {
                total += values[i];
            }
            return total;
        }

        void reverse(int values[], int n) {
            for (int i = 0; i < n / 2; i++) {
                int t = values[i];
                values[i] = values[n - 1 - i];
                values[n - 1 - i] = t;
            }
        }

        int local_array(void) {
            int values[4] = {1, 2, 3, 4};
            reverse(values, 4);
            return values[0] * 1000 + values[1] * 100 + values[2] * 10 + values[3];
        }

        int reversed_index(int *values, int i) {
            /* C says `i[values]` is `values[i]`; it really does. */
            return i[values];
        }
    }

    let mut values: [core::ffi::c_int; 5] = [1, 2, 3, 4, 5];
    let base = (&raw mut values).cast::<core::ffi::c_int>();
    unsafe {
        assert_eq!(sum(base, 5), 15);
        reverse(base, 5);
        assert_eq!(values, [5, 4, 3, 2, 1]);
        assert_eq!(local_array(), 4321);
        assert_eq!(reversed_index((&raw mut values).cast(), 1), 4);
    }
}

#[test]
fn two_dimensional_arrays() {
    c99! {
        int grid[3][4];

        void fill(void) {
            for (int i = 0; i < 3; i++) {
                for (int j = 0; j < 4; j++) {
                    grid[i][j] = i * 10 + j;
                }
            }
        }

        int at(int i, int j) {
            return grid[i][j];
        }

        int trace(void) {
            int local[2][2] = {{1, 2}, {3, 4}};
            return local[0][0] + local[1][1];
        }

        int flat_init(void) {
            /* The braces around each row may be left out. */
            int local[2][3] = {1, 2, 3, 4, 5, 6};
            return local[1][2];
        }

        unsigned long row_size(void) {
            return sizeof(grid[0]);
        }

        unsigned long whole_size(void) {
            return sizeof(grid);
        }
    }

    unsafe {
        fill();
        assert_eq!(at(0, 0), 0);
        assert_eq!(at(2, 3), 23);
        assert_eq!(trace(), 5);
        assert_eq!(flat_init(), 6);
        assert_eq!(
            row_size(),
            4 * size_of::<core::ffi::c_int>() as core::ffi::c_ulong
        );
        assert_eq!(
            whole_size(),
            12 * size_of::<core::ffi::c_int>() as core::ffi::c_ulong
        );
    }
}

#[test]
fn global_arrays_and_structs_are_visible_from_rust() {
    c99! {
        struct Pair { int a; int b; };

        int counters[3] = {1, 2, 3};
        struct Pair pair = {10, 20};

        void bump(void) {
            for (int i = 0; i < 3; i++) {
                counters[i] += 1;
            }
            pair.a += 1;
            pair.b += 1;
        }
    }

    unsafe {
        // A `static mut` has to be read by value, as documented.
        assert_eq!({ counters }, [1, 2, 3]);
        assert_eq!(({ pair }.a, { pair }.b), (10, 20));
        bump();
        assert_eq!({ counters }, [2, 3, 4]);
        assert_eq!(({ pair }.a, { pair }.b), (11, 21));
        counters[0] = 100;
        pair.b = 5;
        bump();
        assert_eq!({ counters }, [101, 4, 5]);
        assert_eq!({ pair }.b, 6);
    }
}

// ---------------------------------------------------------------------------
// strings and libc
// ---------------------------------------------------------------------------

#[test]
fn string_literals_and_a_length_loop() {
    c99! {
        int length(const char *s) {
            int n = 0;
            while (*s != 0) {
                n++;
                s++;
            }
            return n;
        }

        int first_char(void) {
            return "hello"[0];
        }

        unsigned long literal_size(void) {
            return sizeof("hello");
        }

        const char *greeting(void) {
            return "hello, world";
        }

        char stored[] = "abc";

        int stored_length(void) {
            return length(stored);
        }

        unsigned long stored_size(void) {
            return sizeof(stored);
        }
    }

    unsafe {
        assert_eq!(length(c"hello".as_ptr()), 5);
        assert_eq!(length(c"".as_ptr()), 0);
        assert_eq!(first_char(), i32::from(b'h'));
        assert_eq!(literal_size(), 6);
        let text = core::ffi::CStr::from_ptr(greeting());
        assert_eq!(text.to_str().unwrap(), "hello, world");
        assert_eq!(stored_length(), 3);
        assert_eq!(stored_size(), 4);
    }
}

#[test]
fn a_table_of_string_literals() {
    c99! {
        static const char *names[3] = {"zero", "one", "two"};

        const char *name_of(int i) {
            if (i < 0 || i > 2) {
                return "?";
            }
            return names[i];
        }
    }

    unsafe {
        for (index, expected) in ["zero", "one", "two"].iter().enumerate() {
            let name = core::ffi::CStr::from_ptr(name_of(index as core::ffi::c_int));
            assert_eq!(&name.to_str().unwrap(), expected);
        }
        assert_eq!(core::ffi::CStr::from_ptr(name_of(9)).to_str().unwrap(), "?");
    }
}

#[test]
fn libc_string_functions() {
    c99! {
        unsigned long strlen(const char *s);
        void *memcpy(void *dst, const void *src, unsigned long n);
        int memcmp(const void *a, const void *b, unsigned long n);
        int snprintf(char *out, unsigned long size, const char *fmt, ...);

        unsigned long length_of(const char *s) {
            return strlen(s);
        }

        int copy_and_compare(void) {
            char buffer[8];
            memcpy(buffer, "abcdefg", 8);
            return memcmp(buffer, "abcdefg", 8);
        }

        int format(char *out, int value, double scale) {
            return snprintf(out, 32, "%s=%d/%.2f", "v", value, scale);
        }
    }

    let mut buffer = [0i8; 32];
    unsafe {
        assert_eq!(length_of(c"cinrs".as_ptr()), 5);
        assert_eq!(copy_and_compare(), 0);
        let written = format(buffer.as_mut_ptr(), 42, 1.5);
        let text = core::ffi::CStr::from_ptr(buffer.as_ptr());
        assert_eq!(text.to_bytes(), b"v=42/1.50");
        assert_eq!(written, 9);
    }
}

#[test]
fn malloc_and_a_linked_list() {
    c99! {
        void *malloc(unsigned long size);
        void free(void *p);

        struct Node {
            int value;
            struct Node *next;
        };

        struct Node *push(struct Node *head, int value) {
            struct Node *node = (struct Node *) malloc(sizeof(struct Node));
            node->value = value;
            node->next = head;
            return node;
        }

        int total(struct Node *head) {
            int sum = 0;
            while (head) {
                sum += head->value;
                head = head->next;
            }
            return sum;
        }

        int length(struct Node *head) {
            int n = 0;
            for (struct Node *p = head; p != 0; p = p->next) {
                n++;
            }
            return n;
        }

        void destroy(struct Node *head) {
            while (head != 0) {
                struct Node *next = head->next;
                free(head);
                head = next;
            }
        }

        int build_and_sum(int n) {
            struct Node *head = 0;
            for (int i = 1; i <= n; i++) {
                head = push(head, i);
            }
            int sum = total(head) * 100 + length(head);
            destroy(head);
            return sum;
        }
    }

    // 1 + 2 + … + 10 = 55, and the list holds ten nodes.
    assert_eq!(unsafe { build_and_sum(10) }, 5510);
}

#[test]
fn qsort_with_a_c_comparator() {
    c99! {
        void qsort(void *base, unsigned long count, unsigned long size,
                   int (*compare)(const void *, const void *));

        int ascending(const void *a, const void *b) {
            int x = *(const int *) a;
            int y = *(const int *) b;
            if (x < y) return -1;
            if (x > y) return 1;
            return 0;
        }

        int descending(const void *a, const void *b) {
            return ascending(b, a);
        }

        void sort(int *values, int n, int reverse) {
            qsort(values, n, sizeof(int), reverse ? descending : ascending);
        }
    }

    let mut values: [core::ffi::c_int; 6] = [5, 3, 9, 1, 4, 1];
    unsafe {
        sort((&raw mut values).cast(), 6, 0);
        assert_eq!(values, [1, 1, 3, 4, 5, 9]);
        sort((&raw mut values).cast(), 6, 1);
        assert_eq!(values, [9, 5, 4, 3, 1, 1]);
    }
}

#[test]
fn two_translation_units_in_one_module() {
    // Both blocks declare the same external function and both have a `static`
    // local of the same name; nothing may collide.
    c99! {
        int abs(int n);

        int first_call(int n) {
            static int calls = 0;
            calls++;
            return abs(n) + calls;
        }
    }

    c99! {
        int abs(int n);

        int second_call(int n) {
            static int calls = 100;
            calls++;
            return abs(n) + calls;
        }
    }

    unsafe {
        assert_eq!(first_call(-5), 6);
        assert_eq!(first_call(-5), 7);
        assert_eq!(second_call(-5), 106);
        assert_eq!(first_call(-5), 8);
    }
}

#[test]
fn wide_string_literals_and_pointers_to_arrays() {
    // `L"…"` is a prefixed literal the Rust lexer refuses, so the C source
    // comes in as a string literal.
    c99! { r#"
        typedef int wchar_t;

        static const wchar_t *wide = L"hi";

        int wide_at(int i) {
            return wide[i];
        }

        int wide_length(void) {
            const wchar_t *p = L"abcd";
            int n = 0;
            while (*p) {
                n++;
                p++;
            }
            return n;
        }

        int through_array_pointer(void) {
            int values[3] = {4, 5, 6};
            int (*whole)[3] = &values;
            return (*whole)[1] + sizeof(*whole);
        }
    "# }

    unsafe {
        assert_eq!(wide_at(0), i32::from(b'h'));
        assert_eq!(wide_at(1), i32::from(b'i'));
        assert_eq!(wide_at(2), 0);
        assert_eq!(wide_length(), 4);
        // 5 plus `sizeof(int[3])`.
        assert_eq!(
            through_array_pointer(),
            5 + 3 * size_of::<core::ffi::c_int>() as i32
        );
    }
}

// ---------------------------------------------------------------------------
// pointers that differ only in the signedness of the pointee
// ---------------------------------------------------------------------------

#[test]
fn a_pointee_that_differs_only_in_signedness_needs_no_cast() {
    // GCC's and Clang's `-Wpointer-sign`: ISO C makes this a constraint
    // violation (6.5.16.1p1, the unqualified pointee types are not
    // compatible), no compiler anybody uses refuses it, and the amount of real
    // C that leans on it — `strlen` over an `unsigned char *` is the classic —
    // is not small. `doc/gnu-extensions.md` has the rule.
    c99! {
        #include <string.h>

        unsigned long length(unsigned char *s) { return strlen(s); }

        int first_signed(unsigned char *s) { char *p = s; return *p; }
        int first_unsigned(char *s) { unsigned char *p = s; return *p; }

        long widest(unsigned long *p) { long *q = p; return *q; }
        int as_int(unsigned int *p) { int *q = p; return *q; }

        int through_a_parameter(const char *s);
        int call_with_unsigned(unsigned char *s) { return through_a_parameter(s); }
        int through_a_parameter(const char *s) { return s[0]; }
    }

    unsafe {
        let mut text = *b"hi\0";
        assert_eq!(length(text.as_mut_ptr()), 2);
        assert_eq!(first_signed(text.as_mut_ptr()), i32::from(b'h'));
        assert_eq!(call_with_unsigned(text.as_mut_ptr()), i32::from(b'h'));

        let mut signed_text = b"hi\0".map(|b| b as core::ffi::c_char);
        assert_eq!(first_unsigned(signed_text.as_mut_ptr()), i32::from(b'h'));

        let mut wide: core::ffi::c_ulong = 7;
        assert_eq!(widest(&raw mut wide), 7);
        let mut narrow: core::ffi::c_uint = 9;
        assert_eq!(as_int(&raw mut narrow), 9);
    }
}
