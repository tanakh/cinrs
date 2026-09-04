//@check-pass
//! A larger translation unit that uses most of the scalar subset, to prove the
//! generated Rust compiles cleanly — no warnings, no lints, no surprises.

cinrs::c99! {
    typedef unsigned long size_type;

    int call_count = 0;
    static double accumulator = 0.5;

    static int clamp(int v, int lo, int hi) {
        if (v < lo) return lo;
        if (v > hi) return hi;
        return v;
    }

    inline int doubled(int v) {
        return v * 2;
    }

    unsigned char narrow(int v) {
        return (unsigned char) v;
    }

    double weigh(int v, double factor) {
        accumulator += v * factor;
        return accumulator;
    }

    size_type digits(unsigned long v) {
        size_type count = 0;
        do {
            count++;
            v /= 10;
        } while (v > 0);
        return count;
    }

    int categorize(int v) {
        static int seen = 0;
        seen++;
        call_count = seen;
        switch (v % 4) {
            case 0:
            case 1:
                return doubled(v);
            case 2:
                break;
            default:
                for (int i = 0; i < 3; i++) {
                    if (i == 2) continue;
                    v += i;
                }
        }
        return clamp(v, 0, 100);
    }

    _Bool any_negative(int a, int b, int c) {
        return a < 0 || b < 0 || c < 0;
    }

    void reset(void) {
        call_count = 0;
        accumulator = 0.0;
    }

    int summary(void) {
        reset();
        int total = categorize(7) + narrow(300) + (int) weigh(2, 1.5);
        total += (int) digits(12345);
        return any_negative(total, 1, 2) ? -total : total;
    }
}

fn main() {
    let value = unsafe { summary() };
    assert!(value != i32::MIN);
}
