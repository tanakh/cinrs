//@check-pass
//! A translation unit that exercises most of what this release added —
//! pointers, arrays, strings, `struct`, `union`, `enum`, `typedef`, function
//! pointers, `sizeof`, casts, designated initialisers and `extern`
//! declarations — to prove the generated Rust compiles cleanly: no warnings,
//! no lints, no surprises.

cinrs::c99! {
    typedef unsigned long size_type;

    unsigned long strlen(const char *s);
    void *memcpy(void *dst, const void *src, unsigned long n);
    int printf(const char *fmt, ...);

    enum Kind { EMPTY, SCALAR, VECTOR = 10 };

    struct Vector {
        double x;
        double y;
    };

    union Payload {
        double scalar;
        struct Vector vector;
    };

    typedef struct Value {
        enum Kind kind;
        union Payload payload;
        const char *label;
    } Value;

    typedef double (*Reducer)(const struct Vector *);

    static const char *kind_names[3] = {"empty", "scalar", "vector"};
    static Value cache = {.kind = SCALAR, .label = "cached"};
    static int histogram[8];
    static char scratch[16] = "scratch";

    double magnitude(const struct Vector *v) {
        return v->x * v->x + v->y * v->y;
    }

    double first_component(const struct Vector *v) {
        return v->x;
    }

    Value make_scalar(double s, const char *label) {
        Value v;
        v.kind = SCALAR;
        v.payload.scalar = s;
        v.label = label;
        return v;
    }

    Value make_vector(double x, double y) {
        Value v = {VECTOR, {.vector = {x, y}}, "vector"};
        return v;
    }

    double reduce(const Value *v, Reducer with) {
        switch (v->kind) {
            case EMPTY:
                return 0.0;
            case SCALAR:
                return v->payload.scalar;
            case VECTOR:
                return with(&v->payload.vector);
            default:
                return -1.0;
        }
    }

    const char *name_of(enum Kind kind) {
        if (kind == VECTOR) {
            return kind_names[2];
        }
        return kind_names[kind == SCALAR];
    }

    void record(int bucket) {
        if (bucket < 0 || bucket >= 8) {
            return;
        }
        histogram[bucket] += 1;
    }

    int total(void) {
        int sum = 0;
        int *p = histogram;
        int *end = histogram + 8;
        while (p != end) {
            sum += *p++;
        }
        return sum;
    }

    size_type copy_label(char *out, size_type limit, const Value *v) {
        size_type n = strlen(v->label);
        if (n + 1 > limit) {
            n = limit - 1;
        }
        memcpy(out, v->label, n);
        out[n] = 0;
        return n;
    }

    int summary(void) {
        Value scalar = make_scalar(1.5, scratch);
        Value vector = make_vector(3.0, 4.0);
        cache = scalar;
        record((int) reduce(&vector, magnitude) % 8);
        record(0);
        char buffer[16];
        size_type copied = copy_label(buffer, sizeof(buffer), &cache);
        Reducer reducers[2] = {magnitude, first_component};
        double sum = 0.0;
        for (int i = 0; i < 2; i++) {
            sum += reduce(&vector, reducers[i]);
        }
        printf("%s %s %d\n", name_of(vector.kind), buffer, (int) sum);
        return total() + (int) copied + (int) sizeof(Value) + (int) sum;
    }
}

fn main() {
    let value = unsafe { summary() };
    assert!(value != i32::MIN);
}
