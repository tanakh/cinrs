//! What `va_arg` of an aggregate refuses, and why.
//!
//! Reading a `struct` back out of an argument list means undoing the ABI's
//! *classification*, so what can be undone is exactly what the ABI put in
//! registers: at most sixteen bytes, naturally aligned, on x86-64 System V.
//! Everything else is a located error rather than a wrong answer — the stable
//! `VaList` has no way to reach the overflow area, and no way to know that
//! another ABI would have passed the thing by pointer.
//!
//! Every block names its target, so the diagnostics are the same wherever the
//! test is run. These diagnose the *program*, so they are identical on every
//! toolchain too: what an older Rust cannot compile is only reported for a
//! program that has nothing else wrong with it.

cinrs::gnu11! {
    #pragma cinrs target "x86_64-unknown-linux-gnu"
    #include <stdarg.h>

    /* Twenty-four bytes: the caller pushes it into the overflow area. */
    struct Big { double a, b, c; };

    /* Packed, so `i` is not where its own type would put it — which is the
       ABI's other route to class MEMORY. */
    struct Packed { char c; int i; } __attribute__((packed));

    /* No member list, so there is nothing to classify. */
    struct Opaque;

    double first(int n, ...) {
        va_list ap;
        va_start(ap, n);
        return va_arg(ap, struct Big).a;
        //~^ ERROR: it is 24 bytes
    }

    int second(int n, ...) {
        va_list ap;
        va_start(ap, n);
        return va_arg(ap, struct Packed).i;
        //~^ ERROR: a member is not aligned the way its own type asks
    }

    void third(int n, ...) {
        va_list ap;
        va_start(ap, n);
        va_arg(ap, struct Opaque);
        //~^ ERROR: the type is incomplete
    }

    int fourth(int n, ...) {
        va_list ap;
        va_start(ap, n);
        return va_arg(ap, int[2])[0];
        //~^ ERROR: va_arg with type 'int[2]' is not supported
    }
}

/* Another ABI classifies differently — AArch64 has homogeneous float
 * aggregates — so nothing about it is guessed at. */
cinrs::gnu11! {
    #pragma cinrs target "aarch64-unknown-linux-gnu"
    #include <stdarg.h>

    struct Point { int x; int y; };

    int arm(int n, ...) {
        va_list ap;
        va_start(ap, n);
        return va_arg(ap, struct Point).x;
        //~^ ERROR: only supported on x86-64 System V targets
    }
}

/* Same machine, different ABI: the Microsoft x64 one passes an aggregate
 * larger than eight bytes *by pointer*. */
cinrs::gnu11! {
    #pragma cinrs target "x86_64-pc-windows-msvc"
    #include <stdarg.h>

    struct Wide { long long a, b; };

    long long windows(int n, ...) {
        va_list ap;
        va_start(ap, n);
        return va_arg(ap, struct Wide).a;
        //~^ ERROR: only supported on x86-64 System V targets
    }
}

fn main() {}
