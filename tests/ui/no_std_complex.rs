//@check-pass
//@compile-flags: --crate-type lib
//! Complex arithmetic needs no `std`: `cinrs-rt` is itself `#![no_std]`, so
//! `_Complex`, `<complex.h>` and the whole of the runtime compile into a
//! `#![no_std]` crate exactly as they stand. The library *functions* are
//! declarations linked against the platform's libm, which is a link-time
//! dependency of the program rather than a Rust one — the same arrangement
//! `<math.h>` already has.

#![no_std]

cinrs::c99! {
    #include <complex.h>

    double _Complex origin = 0;
    double _Complex unit = 1.0 + 0.0i;

    double _Complex product(double _Complex a, double _Complex b) { return a * b; }
    double _Complex quotient(double _Complex a, double _Complex b) { return a / b; }
    double _Complex scaled(double _Complex z, double x) { return z * x + x - x / z; }
    double _Complex conjugate(double _Complex z) { return ~z; }
    double magnitude(double _Complex z) { return cabs(z); }
    double parts(double _Complex z) { return __real__ z + __imag__ z; }
    void set(double _Complex *z, double v) { __imag__ *z = v; }
    float _Complex narrowed(double _Complex z) { return (float _Complex) z; }
    int nonzero(double _Complex z) { return z ? 1 : 0; }

    /* Passing one through `...` needs nothing of the runtime. */
    int sink(int n, ...);
    int through_varargs(double _Complex z) { return sink(1, z); }
}

pub fn call(z: cinrs::rt::Complex<f64>, w: cinrs::rt::Complex<f64>) -> f64 {
    unsafe {
        let p = product(z, w);
        let q = quotient(p, unit);
        let s = scaled(conjugate(q), 2.0);
        let mut t = s;
        set(&raw mut t, 1.0);
        magnitude(t) + parts(t) + f64::from(narrowed(t).re) + f64::from(nonzero(origin))
    }
}
