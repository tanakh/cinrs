//! C's complex types, end to end.
//!
//! The file is in two halves. The first is ordinary translation: `_Complex`
//! declarations, arithmetic, `__real__`/`__imag__`, `<complex.h>`, statics,
//! `_Generic`, layout, and handing complex values back and forth between C and
//! Rust. The second is a *differential* check of the arithmetic itself against
//! the host's own C compiler — see [`the_product_and_quotient_agree_with_cc`],
//! which is the only honest way to test Annex G's infinity recovery.
//!
//! The whole file needs the `complex` feature, which is on by default; without
//! it `_Complex` is a diagnostic and there is nothing here to run.

#![cfg(feature = "complex")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use cinrs::rt::Complex;
use cinrs::rt::complex as rt;
use cinrs::{c11, c23, c99, gnu11, gnu89};

/// `Complex<f64>`, which is what `double _Complex` is.
type C = Complex<f64>;
/// `Complex<f32>`, which is what `float _Complex` is.
type Cf = Complex<f32>;

/// Bit-for-bit equality, with every NaN counting as one value; the sign of a
/// *zero* is compared, because the rules under test are about it.
fn same(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    a == b && a.is_sign_negative() == b.is_sign_negative()
}

fn same_c(a: C, b: C) -> bool {
    same(a.re, b.re) && same(a.im, b.im)
}

fn same_c32(a: Cf, b: Cf) -> bool {
    same(f64::from(a.re), f64::from(b.re)) && same(f64::from(a.im), f64::from(b.im))
}

#[track_caller]
fn eq_c(got: C, want: C) {
    assert!(same_c(got, want), "got {got:?}, want {want:?}");
}

// ---------------------------------------------------------------------------
// arithmetic, conversions and the operators
// ---------------------------------------------------------------------------

c99! {
    #include <complex.h>

    double _Complex c_add(double _Complex a, double _Complex b) { return a + b; }
    double _Complex c_sub(double _Complex a, double _Complex b) { return a - b; }
    double _Complex c_mul(double _Complex a, double _Complex b) { return a * b; }
    double _Complex c_div(double _Complex a, double _Complex b) { return a / b; }
    double _Complex c_neg(double _Complex a) { return -a; }
    double _Complex c_pos(double _Complex a) { return +a; }
    double _Complex c_conj(double _Complex a) { return ~a; }

    /* Every mixed form, with the real operand on each side. */
    double _Complex m_mul_l(double _Complex z, double x) { return z * x; }
    double _Complex m_mul_r(double x, double _Complex z) { return x * z; }
    double _Complex m_add_l(double _Complex z, double x) { return z + x; }
    double _Complex m_add_r(double x, double _Complex z) { return x + z; }
    double _Complex m_sub_l(double _Complex z, double x) { return z - x; }
    double _Complex m_sub_r(double x, double _Complex z) { return x - z; }
    double _Complex m_div_l(double _Complex z, double x) { return z / x; }
    double _Complex m_div_r(double x, double _Complex z) { return x / z; }
    /* An integer operand goes the same way, through `double`. */
    double _Complex m_int(double _Complex z) { return 2 * z + 1 - z / 2; }

    /* Conversions, 6.3.1.6 and 6.3.1.7. */
    double _Complex from_real(double x) { return x; }
    double _Complex from_int(int n) { return n; }
    double to_real(double _Complex z) { return z; }
    int to_int(double _Complex z) { return (int) z; }
    int to_bool(double _Complex z) { return (_Bool) z; }
    float _Complex narrow(double _Complex z) { return z; }
    double _Complex widen(float _Complex z) { return z; }
    int truthy(double _Complex z) { return z ? 1 : 0; }
    int negate(double _Complex z) { return !z; }

    /* Equality compares both parts; a real operand is widened for it. */
    int c_eq(double _Complex a, double _Complex b) { return a == b; }
    int c_ne(double _Complex a, double _Complex b) { return a != b; }
    int eq_real(double _Complex a, double x) { return a == x; }

    /* `__real__` and `__imag__`, as values and as lvalues. */
    double part_re(double _Complex z) { return __real__ z; }
    double part_im(double _Complex z) { return __imag__ z; }
    void set_parts(double _Complex *z, double re, double im) {
        __real__ *z = re;
        __imag__ *z = im;
    }
    double _Complex bump_re(double _Complex z) { __real__ z += 10.0; return z; }
    /* On a *real* operand: the value itself, and a zero. */
    double real_part_of_real(double x) { return __real__ x; }
    double imag_part_of_real(double x) { return __imag__ x; }
    int imag_part_of_int(int n) { return __imag__ n; }

    /* `++`/`--` step the real part (GCC). */
    double _Complex inc(double _Complex z) { z++; return z; }
    double _Complex dec(double _Complex z) { --z; return z; }
    double _Complex post(double _Complex *z) { return (*z)++; }

    /* Compound assignment, with a complex and with a real right operand. */
    double _Complex ca_mul(double _Complex z, double _Complex w) { z *= w; return z; }
    double _Complex ca_mul_real(double _Complex z, double x) { z *= x; return z; }
    double _Complex ca_div_real(double _Complex z, double x) { z /= x; return z; }
    double _Complex ca_add_real(double _Complex z, double x) { z += x; return z; }
    double _Complex ca_sub_real(double _Complex z, double x) { z -= x; return z; }
    /* A *real* place with a complex right operand: the result converts back. */
    double ra_mul(double x, double _Complex z) { x *= z; return x; }

    /* `__builtin_complex` and the `CMPLX` macros built on it. */
    double _Complex build(double re, double im) { return __builtin_complex(re, im); }
    double _Complex cmplx(double re, double im) { return CMPLX(re, im); }
    float _Complex cmplxf(float re, float im) { return CMPLXF(re, im); }
    double _Complex cmplxl(double re, double im) { return CMPLXL(re, im); }
    /* `I` is a `float _Complex`, so `x * I` is still a `double _Complex`. */
    double _Complex times_i(double x) { return x * I; }
    unsigned long i_is_float_complex(void) { return sizeof(I); }
    double _Complex from_i(double re, double im) { return re + im * I; }

    /* The library, by value in both directions. */
    double magnitude(double _Complex z) { return cabs(z); }
    double argument(double _Complex z) { return carg(z); }
    double _Complex root(double _Complex z) { return csqrt(z); }
    double _Complex expo(double _Complex z) { return cexp(z); }
    double _Complex logarithm(double _Complex z) { return clog(z); }
    double _Complex power(double _Complex a, double _Complex b) { return cpow(a, b); }
    double _Complex conjugate(double _Complex z) { return conj(z); }
    double _Complex project(double _Complex z) { return cproj(z); }
    double real_of(double _Complex z) { return creal(z); }
    double imag_of(double _Complex z) { return cimag(z); }
    float magnitude_f(float _Complex z) { return cabsf(z); }
    float _Complex root_f(float _Complex z) { return csqrtf(z); }

    /* `float _Complex` arithmetic really is single precision. */
    float _Complex f_mul(float _Complex a, float _Complex b) { return a * b; }
    float _Complex f_div(float _Complex a, float _Complex b) { return a / b; }
    float _Complex f_scale(float _Complex a, float x) { return a * x; }

    /* `long double _Complex` follows `long double` onto `double`. */
    long double _Complex ld(long double _Complex z) { return z * z; }
    unsigned long ld_size(void) { return sizeof(long double _Complex); }

    /* Layout. */
    unsigned long size_double_complex(void) { return sizeof(double _Complex); }
    unsigned long size_float_complex(void) { return sizeof(float _Complex); }

    struct Pair { double _Complex z; int n; };
    unsigned long pair_size(void) { return sizeof(struct Pair); }
    unsigned long pair_offset(void) { return __builtin_offsetof(struct Pair, n); }
    double _Complex pair_z(struct Pair *p) { return p->z; }

    /* A complex member of a *packed* record, whose parts are then reached at
       an offset the type is not aligned for. */
    struct __attribute__((packed)) Squeezed { char tag; double _Complex z; };
    unsigned long squeezed_size(void) { return sizeof(struct Squeezed); }
    double squeezed_im(struct Squeezed *p) { return __imag__ p->z; }
    void squeezed_set(struct Squeezed *p, double re, double im) {
        __real__ p->z = re;
        __imag__ p->z = im;
    }
    double _Complex squeezed_scaled(struct Squeezed *p) { return p->z * 2.0; }

    double _Complex sum_array(const double _Complex *a, int n) {
        double _Complex total = 0;
        for (int i = 0; i < n; i++) total += a[i];
        return total;
    }

    /* A compound literal of complex type, whose address outlives it. */
    double _Complex literal(double re, double im) {
        double _Complex *p = &(double _Complex){ 0 };
        *p = (double _Complex){ re + im * I };
        return *p;
    }

    /* A function that jumps, so the body goes through the CFG lowering. */
    double _Complex spin(double _Complex z, int n) {
        double _Complex acc = 0;
    again:
        if (n <= 0) goto done;
        acc += z;
        n--;
        goto again;
    done:
        return acc;
    }

    /* Statics: the constants have to fold. */
    double _Complex g_one = 1.0 + 2.0i;
    double _Complex g_product = (1.0 + 2.0i) * (3.0 - 4.0i);
    double _Complex g_quotient = (1.0 + 2.0i) / (3.0 - 4.0i);
    double _Complex g_mixed = 3.0 * (1.0 + 2.0i) - 1.0;
    double _Complex g_conj = ~(1.0 + 2.0i);
    double _Complex g_zero;
    float _Complex g_float = 1.5f + 2.5if;
    double _Complex g_array[3] = { 1.0 + 1.0i, 2.0 };
    struct Pair g_pair = { 1.0 + 2.0i, 7 };
    double g_real_part = __real__ (1.0 + 2.0i);

    /* The corners of the folded arithmetic, so that the front end's copy of it
       is measured against the runtime's on more than the easy cases. */
    double _Complex k_inf_mul = (1.0 / 0.0 + 0.0i) * (3.0 - 4.0i);
    double _Complex k_inf_div = (1.0 / 0.0 + 0.0i) / (3.0 - 4.0i);
    double _Complex k_by_inf = (3.0 - 4.0i) / (1.0 / 0.0 + 0.0i);
    double _Complex k_nan_mul = (0.0 / 0.0 + 0.0i) * (3.0 - 4.0i);
    double _Complex k_zero_div = (1.0 + 2.0i) / (0.0 + 0.0i);
    double _Complex k_negzero_div = (1.0 + 2.0i) / (-0.0 - 0.0i);
    double _Complex k_big_mul = (1.0e150 + 1.0e150i) * (1.0e150 + 1.0e150i);
    double _Complex k_small_div = (1.0e-150 + 1.0e-150i) / (1.0e150 + 1.0e150i);
    double _Complex k_real_div = 3.0 / (1.0 + 2.0i);
    double _Complex k_real_mul = 3.0 * (-0.0 - 0.0i);
    double _Complex k_real_sub = 0.0 - (1.0 + 0.0i);
    float _Complex k_f_mul = (1.0f + 2.0if) * (3.0f - 4.0if);
    float _Complex k_f_div = (1.0f + 2.0if) / (3.0f - 4.0if);
    float _Complex k_f_overflow = (1.0e20f + 1.0e20if) * (1.0e20f + 1.0e20if);
}

#[test]
fn the_four_operators_on_two_complex_values() {
    let a = Complex::new(1.0, 2.0);
    let b = Complex::new(3.0, -4.0);
    unsafe {
        eq_c(c_add(a, b), Complex::new(4.0, -2.0));
        eq_c(c_sub(a, b), Complex::new(-2.0, 6.0));
        eq_c(c_mul(a, b), rt::mul_f64(a, b));
        eq_c(c_mul(a, b), Complex::new(11.0, 2.0));
        eq_c(c_div(a, b), rt::div_f64(a, b));
        eq_c(c_div(a, b), Complex::new(-0.2, 0.4));
        eq_c(c_neg(a), Complex::new(-1.0, -2.0));
        eq_c(c_pos(a), a);
        eq_c(c_conj(a), Complex::new(1.0, -2.0));
    }
}

/// Annex G's recovery reaches the generated code, not just the runtime: an
/// infinite operand makes the product infinite where the naive formula is a
/// NaN.
#[test]
fn an_infinity_survives_the_translated_product_and_quotient() {
    let inf = f64::INFINITY;
    let big = Complex::new(inf, 0.0);
    let b = Complex::new(3.0, -4.0);
    unsafe {
        eq_c(c_mul(big, b), Complex::new(inf, -inf));
        eq_c(c_div(big, b), Complex::new(inf, inf));
        eq_c(c_div(b, big), Complex::new(0.0, -0.0));
        eq_c(
            c_div(Complex::new(1.0, 2.0), Complex::new(-0.0, -0.0)),
            Complex::new(-inf, -inf),
        );
    }
}

/// A real operand is computed with componentwise, which the signs of the
/// zeros show: the full product would turn `−0` into `+0`.
#[test]
fn a_real_operand_is_not_widened() {
    let z = Complex::new(1.0, 2.0);
    let nzero = Complex::new(-0.0, -0.0);
    unsafe {
        eq_c(m_mul_l(z, 3.0), Complex::new(3.0, 6.0));
        eq_c(m_mul_r(3.0, z), Complex::new(3.0, 6.0));
        eq_c(m_mul_l(nzero, 3.0), Complex::new(-0.0, -0.0));
        eq_c(m_mul_r(3.0, nzero), Complex::new(-0.0, -0.0));
        eq_c(m_add_l(z, 3.0), Complex::new(4.0, 2.0));
        eq_c(m_add_r(3.0, z), Complex::new(4.0, 2.0));
        eq_c(m_sub_l(z, 3.0), Complex::new(-2.0, 2.0));
        eq_c(m_sub_r(3.0, z), Complex::new(2.0, -2.0));
        // `x - z` negates the imaginary part rather than subtracting from a
        // zero, so a `+0` imaginary part comes back as `−0`.
        eq_c(
            m_sub_r(0.0, Complex::new(1.0, 0.0)),
            Complex::new(-1.0, -0.0),
        );
        eq_c(m_div_l(z, 2.0), Complex::new(0.5, 1.0));
        // `complex / real` is componentwise even when the divisor is zero.
        eq_c(m_div_l(z, 0.0), Complex::new(f64::INFINITY, f64::INFINITY));
        // `real / complex` is the full division with a zero imaginary part.
        eq_c(m_div_r(3.0, z), rt::real_div_f64(3.0, z));
        eq_c(m_div_r(3.0, z), Complex::new(0.6, -1.2));
        // `2z + 1 − z/2` for `z = 1 + 2i` is `(2, 4) + 1 − (0.5, 1)`.
        eq_c(m_int(z), Complex::new(2.5, 3.0));
    }
}

#[test]
fn conversions_follow_the_standard() {
    unsafe {
        eq_c(from_real(2.5), Complex::new(2.5, 0.0));
        eq_c(from_int(-3), Complex::new(-3.0, 0.0));
        assert_eq!(to_real(Complex::new(3.7, 9.0)), 3.7);
        assert_eq!(to_int(Complex::new(3.7, 9.0)), 3);
        // 6.3.1.2: a complex value is true when *either* part is non-zero.
        assert_eq!(to_bool(Complex::new(0.0, 1.0)), 1);
        assert_eq!(to_bool(Complex::new(0.0, 0.0)), 0);
        assert_eq!(to_bool(Complex::new(-0.0, -0.0)), 0);
        assert_eq!(to_bool(Complex::new(f64::NAN, 0.0)), 1);
        assert_eq!(truthy(Complex::new(0.0, 1.0)), 1);
        assert_eq!(truthy(Complex::new(0.0, 0.0)), 0);
        assert_eq!(negate(Complex::new(0.0, 0.0)), 1);
        assert_eq!(negate(Complex::new(0.0, 1.0)), 0);
        assert!(same_c32(
            narrow(Complex::new(1e300, 2.0)),
            Complex::new(f32::INFINITY, 2.0)
        ));
        eq_c(widen(Complex::new(1.5f32, -2.5)), Complex::new(1.5, -2.5));
    }
}

#[test]
fn equality_compares_both_parts() {
    let a = Complex::new(1.0, 2.0);
    unsafe {
        assert_eq!(c_eq(a, a), 1);
        assert_eq!(c_eq(a, Complex::new(1.0, 3.0)), 0);
        assert_eq!(c_ne(a, Complex::new(1.0, 3.0)), 1);
        // A NaN part is equal to nothing, itself included.
        assert_eq!(
            c_eq(Complex::new(f64::NAN, 0.0), Complex::new(f64::NAN, 0.0)),
            0
        );
        // A signed zero compares equal to the other sign, as `==` on the parts
        // does.
        assert_eq!(c_eq(Complex::new(-0.0, 0.0), Complex::new(0.0, -0.0)), 1);
        // A real operand is widened for a comparison, unlike for arithmetic.
        assert_eq!(eq_real(a, 1.0), 0);
        assert_eq!(eq_real(Complex::new(1.0, 0.0), 1.0), 1);
    }
}

#[test]
fn the_parts_are_readable_and_assignable() {
    let z = Complex::new(1.0, 2.0);
    unsafe {
        assert_eq!(part_re(z), 1.0);
        assert_eq!(part_im(z), 2.0);
        let mut w = Complex::new(0.0, 0.0);
        set_parts(&raw mut w, 5.0, -6.0);
        eq_c(w, Complex::new(5.0, -6.0));
        eq_c(bump_re(z), Complex::new(11.0, 2.0));
        // On a real operand `__real__` is the value and `__imag__` is a zero.
        assert_eq!(real_part_of_real(2.5), 2.5);
        assert_eq!(imag_part_of_real(2.5), 0.0);
        assert_eq!(imag_part_of_int(9), 0);
    }
}

#[test]
fn increment_steps_the_real_part() {
    let z = Complex::new(1.0, 2.0);
    unsafe {
        eq_c(inc(z), Complex::new(2.0, 2.0));
        eq_c(dec(z), Complex::new(0.0, 2.0));
        let mut w = z;
        eq_c(post(&raw mut w), z);
        eq_c(w, Complex::new(2.0, 2.0));
    }
}

#[test]
fn compound_assignment_keeps_a_real_operand_real() {
    let z = Complex::new(1.0, 2.0);
    let nzero = Complex::new(-0.0, -0.0);
    unsafe {
        eq_c(ca_mul(z, Complex::new(3.0, -4.0)), Complex::new(11.0, 2.0));
        eq_c(ca_mul_real(z, 3.0), Complex::new(3.0, 6.0));
        eq_c(ca_mul_real(nzero, 3.0), Complex::new(-0.0, -0.0));
        eq_c(ca_div_real(z, 2.0), Complex::new(0.5, 1.0));
        eq_c(ca_add_real(z, 1.0), Complex::new(2.0, 2.0));
        eq_c(ca_sub_real(z, 1.0), Complex::new(0.0, 2.0));
        // A real place: the complex result converts back by discarding the
        // imaginary part.
        assert_eq!(ra_mul(3.0, z), 3.0);
    }
}

#[test]
fn a_complex_value_is_built_from_its_two_parts() {
    let inf = f64::INFINITY;
    unsafe {
        eq_c(build(1.0, 2.0), Complex::new(1.0, 2.0));
        eq_c(cmplx(1.0, 2.0), Complex::new(1.0, 2.0));
        eq_c(cmplxl(1.0, 2.0), Complex::new(1.0, 2.0));
        assert!(same_c32(cmplxf(1.0, 2.0), Complex::new(1.0, 2.0)));
        // The whole point of the builtin: `x + inf * I` cannot make this,
        // because `inf * 0` is a NaN.
        eq_c(build(1.0, inf), Complex::new(1.0, inf));
        eq_c(cmplx(f64::NAN, inf), Complex::new(f64::NAN, inf));
        // `_Complex_I` is a `float _Complex` (7.3.1p4), as GCC's header has it.
        assert_eq!(i_is_float_complex(), 8);
        eq_c(times_i(2.0), Complex::new(0.0, 2.0));
        eq_c(from_i(1.0, 2.0), Complex::new(1.0, 2.0));
        // ... and `inf * I` really is the NaN the builtin exists to avoid.
        assert!(times_i(inf).re.is_nan());
    }
}

#[test]
fn the_library_functions_pass_complex_values_by_value() {
    let b = Complex::new(3.0, -4.0);
    let a = Complex::new(1.0, 2.0);
    unsafe {
        assert_eq!(magnitude(b), 5.0);
        assert_eq!(argument(b), (-4.0f64).atan2(3.0));
        eq_c(root(Complex::new(-1.0, 0.0)), Complex::new(0.0, 1.0));
        // e^(iπ) = −1, to within a rounding of π.
        let e = expo(Complex::new(0.0, std::f64::consts::PI));
        assert!((e.re + 1.0).abs() < 1e-15 && e.im.abs() < 1e-15);
        let l = logarithm(Complex::new(std::f64::consts::E, 0.0));
        assert!((l.re - 1.0).abs() < 1e-15 && l.im == 0.0);
        eq_c(power(Complex::new(2.0, 0.0), Complex::new(10.0, 0.0)), {
            let p = power(Complex::new(2.0, 0.0), Complex::new(10.0, 0.0));
            assert!((p.re - 1024.0).abs() < 1e-9);
            p
        });
        eq_c(conjugate(a), Complex::new(1.0, -2.0));
        eq_c(project(a), a);
        eq_c(
            project(Complex::new(1.0, f64::NEG_INFINITY)),
            Complex::new(f64::INFINITY, -0.0),
        );
        assert_eq!(real_of(a), 1.0);
        assert_eq!(imag_of(a), 2.0);
        assert_eq!(magnitude_f(Complex::new(3.0f32, -4.0)), 5.0);
        assert!(same_c32(
            root_f(Complex::new(-1.0f32, 0.0)),
            Complex::new(0.0, 1.0)
        ));
    }
}

#[test]
fn float_complex_arithmetic_is_single_precision() {
    let a = Complex::new(1.0f32, 2.0);
    let b = Complex::new(3.0f32, -4.0);
    unsafe {
        assert!(same_c32(f_mul(a, b), rt::mul_f32(a, b)));
        assert!(same_c32(f_mul(a, b), Complex::new(11.0, 2.0)));
        assert!(same_c32(f_div(a, b), rt::div_f32(a, b)));
        assert!(same_c32(f_scale(a, 3.0), Complex::new(3.0, 6.0)));
        // An intermediate that overflows in `float` is not one that would
        // overflow in `double`, which is what makes the width observable.
        let big = Complex::new(1e20f32, 1e20);
        assert!(same_c32(f_mul(big, big), rt::mul_f32(big, big)));
        assert!(f_mul(big, big).re.is_nan());
    }
}

#[test]
fn long_double_complex_is_double_complex() {
    unsafe {
        assert_eq!(ld_size(), 16);
        eq_c(ld(Complex::new(1.0, 2.0)), Complex::new(-3.0, 4.0));
    }
}

#[test]
fn the_layout_is_two_components_side_by_side() {
    assert_eq!(size_of::<C>(), 16);
    assert_eq!(align_of::<C>(), 8);
    assert_eq!(size_of::<Cf>(), 8);
    assert_eq!(align_of::<Cf>(), 4);
    unsafe {
        assert_eq!(size_double_complex(), 16);
        assert_eq!(size_float_complex(), 8);
        // What the C computed is what the Rust item really has.
        assert_eq!(pair_size() as usize, size_of::<Pair>());
        assert_eq!(pair_offset() as usize, core::mem::offset_of!(Pair, n));
    }
    let mut pair = Pair {
        z: Complex::new(1.0, 2.0),
        n: 7,
    };
    unsafe { eq_c(pair_z(&raw mut pair), Complex::new(1.0, 2.0)) };
}

/// A complex member of a packed record sits one byte in, so neither of its
/// parts is aligned the way `f64` asks — which is undefined behaviour to read
/// as an aligned `f64` and an abort in a debug build. Such a place goes
/// through `read_unaligned`/`write_unaligned` instead.
#[test]
fn a_complex_member_of_a_packed_record_is_reached_unaligned() {
    unsafe {
        assert_eq!(squeezed_size() as usize, size_of::<Squeezed>());
        assert_eq!(squeezed_size(), 17);
        let mut s = Squeezed {
            tag: 1,
            z: Complex::new(0.0, 0.0),
        };
        squeezed_set(&raw mut s, 3.0, -4.0);
        assert_eq!(squeezed_im(&raw mut s), -4.0);
        eq_c(squeezed_scaled(&raw mut s), Complex::new(6.0, -8.0));
    }
}

#[test]
fn arrays_and_compound_literals_of_complex_type() {
    let a = [
        Complex::new(1.0, 2.0),
        Complex::new(3.0, 4.0),
        Complex::new(5.0, 6.0),
    ];
    unsafe {
        eq_c(sum_array(a.as_ptr(), 3), Complex::new(9.0, 12.0));
        eq_c(literal(1.5, -2.5), Complex::new(1.5, -2.5));
        eq_c(spin(Complex::new(1.0, 1.0), 4), Complex::new(4.0, 4.0));
    }
}

/// The statics have to be *folded*: a Rust `static` initialiser is a constant
/// expression, so the front end computes the value with its own copy of the
/// arithmetic — and this is what checks that copy against the runtime's.
#[test]
fn static_initialisers_fold_to_the_same_values_the_runtime_computes() {
    let a = Complex::new(1.0, 2.0);
    let b = Complex::new(3.0, -4.0);
    unsafe {
        eq_c(g_one, a);
        eq_c(g_product, rt::mul_f64(a, b));
        eq_c(g_quotient, rt::div_f64(a, b));
        eq_c(g_mixed, rt::sub_real_f64(rt::real_mul_f64(3.0, a), 1.0));
        eq_c(g_conj, rt::conj_f64(a));
        eq_c(g_zero, Complex::new(0.0, 0.0));
        assert!(same_c32(g_float, Complex::new(1.5, 2.5)));
        eq_c(g_array[0], Complex::new(1.0, 1.0));
        eq_c(g_array[1], Complex::new(2.0, 0.0));
        eq_c(g_array[2], Complex::new(0.0, 0.0));
        eq_c(g_pair.z, a);
        // `assert_eq!` takes a reference, which edition 2024 will not give to
        // a `static mut`; copying the value out first is the way round it.
        let n = g_pair.n;
        assert_eq!(n, 7);
        let re = g_real_part;
        assert_eq!(re, 1.0);
    }
}

/// The same, for the corners: an infinite operand, a zero divisor, magnitudes
/// far enough apart to overflow an intermediate, and `float` precision.
///
/// The front end has to have its own copy of the arithmetic — it folds a
/// `static` initialiser before code generation, and it may not depend on the
/// runtime crate — so `cinrs_core::complex` and `cinrs_rt::complex` are two
/// implementations of one thing. This is what keeps them in step.
#[test]
fn the_folded_corners_are_what_the_runtime_computes() {
    let inf = f64::INFINITY;
    let a = Complex::new(1.0, 2.0);
    let b = Complex::new(3.0, -4.0);
    let big = Complex::new(1e150, 1e150);
    let small = Complex::new(1e-150, 1e-150);
    unsafe {
        eq_c(k_inf_mul, rt::mul_f64(Complex::new(inf, 0.0), b));
        eq_c(k_inf_div, rt::div_f64(Complex::new(inf, 0.0), b));
        eq_c(k_by_inf, rt::div_f64(b, Complex::new(inf, 0.0)));
        eq_c(k_nan_mul, rt::mul_f64(Complex::new(f64::NAN, 0.0), b));
        eq_c(k_zero_div, rt::div_f64(a, Complex::new(0.0, 0.0)));
        eq_c(k_negzero_div, rt::div_f64(a, Complex::new(-0.0, -0.0)));
        eq_c(k_big_mul, rt::mul_f64(big, big));
        eq_c(k_small_div, rt::div_f64(small, big));
        eq_c(k_real_div, rt::real_div_f64(3.0, a));
        eq_c(k_real_mul, rt::real_mul_f64(3.0, Complex::new(-0.0, -0.0)));
        eq_c(k_real_sub, rt::real_sub_f64(0.0, Complex::new(1.0, 0.0)));

        let fa = Complex::new(1.0f32, 2.0);
        let fb = Complex::new(3.0f32, -4.0);
        let fbig = Complex::new(1e20f32, 1e20);
        assert!(same_c32(k_f_mul, rt::mul_f32(fa, fb)));
        assert!(same_c32(k_f_div, rt::div_f32(fa, fb)));
        assert!(same_c32(k_f_overflow, rt::mul_f32(fbig, fbig)));
        // ... and that last one really is the `float` answer, which is not the
        // one `double` arithmetic would have given.
        assert!(k_f_overflow.re.is_nan() && k_f_overflow.im.is_infinite());
    }
}

// ---------------------------------------------------------------------------
// the other entry points
// ---------------------------------------------------------------------------

c11! {
    #include <complex.h>

    /* `_Generic` selects on a complex type like any other. */
    const char *which(void) {
        double _Complex d = 0;
        float _Complex f = 0;
        double r = 0;
        return _Generic(d, double _Complex: "dc", default: "?")
            + 0 * (int) (long) _Generic(f, float _Complex: 0, default: 1)
            + 0 * (int) (long) _Generic(r, double: 0, default: 1);
    }

    unsigned long alignments(void) {
        return _Alignof(double _Complex) * 100 + _Alignof(float _Complex);
    }

    /* C11 gave `<complex.h>` its version macro. */
    long version(void) { return __STDC_VERSION_COMPLEX_H__; }

    _Static_assert(sizeof(double _Complex) == 2 * sizeof(double), "two doubles");
    _Static_assert(__builtin_complex(5.0, 2.0) == 5.0 + 1.0j * 2.0, "N1464");
    _Static_assert(CMPLX(5.0, 1.0 / 0.0) != 5.0 + 1.0j * (1.0 / 0.0), "the infinity case");
}

c23! {
    #include <complex.h>
    double _Complex c23_add(double _Complex a, double _Complex b) { return a + b; }
    constexpr double c23_pi = 3.5;
    double _Complex c23_scaled(double _Complex z) { return z * c23_pi; }
}

gnu89! {
    /* `_Complex` is C99, and a GNU dialect takes everything a later revision
       added — so `gnu89!` has it while `c89!` does not. `<complex.h>` is not
       included here: the macros are C99's library, and this is the language. */
    double _Complex gnu89_mul(double _Complex a, double _Complex b) { return a * b; }
    double _Complex gnu89_imag(void) { return 2.0i; }
    double gnu89_real(double _Complex z) { return __real__ z; }
}

gnu11! {
    #include <complex.h>
    double _Complex gnu11_conj(double _Complex z) { return ~z; }
    double _Complex gnu11_builtin(double _Complex z) {
        return __builtin_cproj(__builtin_conj(z)) + __builtin_creal(z);
    }
    /* GCC's complex machine modes. */
    typedef double cd __attribute__((mode(DC)));
    typedef float cf __attribute__((mode(SC)));
    unsigned long mode_sizes(void) { return sizeof(cd) * 100 + sizeof(cf); }
}

#[test]
fn every_entry_point_that_has_complex_has_all_of_it() {
    let a = Complex::new(1.0, 2.0);
    let b = Complex::new(3.0, -4.0);
    unsafe {
        assert_eq!(
            core::ffi::CStr::from_ptr(which()).to_bytes(),
            b"dc",
            "_Generic picked the wrong association"
        );
        assert_eq!(alignments(), 8 * 100 + 4);
        assert_eq!(version(), 202311);
        eq_c(c23_add(a, b), Complex::new(4.0, -2.0));
        eq_c(c23_scaled(a), Complex::new(3.5, 7.0));
        eq_c(gnu89_mul(a, b), Complex::new(11.0, 2.0));
        eq_c(gnu89_imag(), Complex::new(0.0, 2.0));
        assert_eq!(gnu89_real(a), 1.0);
        eq_c(gnu11_conj(a), Complex::new(1.0, -2.0));
        eq_c(gnu11_builtin(a), Complex::new(2.0, -2.0));
        assert_eq!(mode_sizes(), 16 * 100 + 8);
    }
}

// ---------------------------------------------------------------------------
// the crate-path pragma
// ---------------------------------------------------------------------------

/// The expansion names the facade crate in full, and `#pragma cinrs crate`
/// says where it is — which is what a renamed dependency, or one reached
/// through a re-export, needs.
mod renamed {
    pub use cinrs as vendored;
}

cinrs::c99! {
    #pragma cinrs crate "crate::renamed::vendored"
    double _Complex through_a_rename(double _Complex a, double _Complex b) { return a * b; }
}

#[test]
fn the_crate_path_pragma_finds_a_renamed_dependency() {
    unsafe {
        eq_c(
            through_a_rename(Complex::new(1.0, 2.0), Complex::new(3.0, -4.0)),
            Complex::new(11.0, 2.0),
        );
    }
}

// ---------------------------------------------------------------------------
// the differential oracle
// ---------------------------------------------------------------------------

/// The `double` values every pair is drawn from.
///
/// Zero and negative zero (whose signs the recovery rules propagate), the
/// small integers, both infinities, a NaN, and magnitudes far enough apart
/// that a naive `c² + d²` would overflow or underflow.
const VALUES: &[f64] = &[
    0.0,
    -0.0,
    1.0,
    -1.0,
    2.0,
    0.5,
    3.0,
    -4.0,
    1e150,
    -1e150,
    1e-150,
    -1e-150,
    1e60,
    1e-60,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NAN,
    1e8,
    1e-8,
];

/// Values added to [`VALUES`] for the *product* only.
///
/// A quotient whose intermediate overflows in both algorithms is where
/// implementations legitimately differ — `libgcc` prescales by a power of two
/// and produces a NaN where Smith's algorithm produces a zero, and C leaves
/// the accuracy of complex arithmetic implementation-defined (Annex G.6). The
/// product has no such corner: it agrees with the host everywhere, subnormals
/// included, so it is checked over the wider table.
const EXTREME: &[f64] = &[1e300, -1e300, 1e-300, -1e-300, 5e-324, 1e-320];

/// The `float` values the `float _Complex` pairs are drawn from.
const VALUES_F32: &[f32] = &[
    0.0,
    -0.0,
    1.0,
    -1.0,
    3.0,
    -4.0,
    1e20,
    1e-20,
    f32::INFINITY,
    f32::NEG_INFINITY,
    f32::NAN,
];

/// The C the oracle is compiled from.
///
/// It reads the value table out of its own generated `values.h` and writes the
/// answers as raw little-endian bit patterns, which keeps a hundred and thirty
/// thousand pairs to a few megabytes and takes every question of formatting
/// out of the comparison.
const ORACLE_C: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <complex.h>
#include "values.h"

static FILE *out;

static void put(double x) {
    unsigned long long bits;
    unsigned char buf[8];
    int i;
    memcpy(&bits, &x, sizeof bits);
    for (i = 0; i < 8; i++) buf[i] = (unsigned char) (bits >> (8 * i));
    fwrite(buf, 1, 8, out);
}

static void putf(float x) {
    put((double) x);
}

static void putc_(double _Complex z) {
    put(__real__ z);
    put(__imag__ z);
}

static void putcf(float _Complex z) {
    putf(__real__ z);
    putf(__imag__ z);
}

int main(int argc, char **argv) {
    int i, j, k, l;
    if (argc < 2) return 1;
    out = fopen(argv[1], "wb");
    if (!out) return 1;

    /* 1. product over the wide table, quotient over the narrow one. */
    for (i = 0; i < N_ALL; i++)
        for (j = 0; j < N_ALL; j++)
            for (k = 0; k < N_ALL; k++)
                for (l = 0; l < N_ALL; l++) {
                    volatile double A = VAL[i], B = VAL[j], C = VAL[k], D = VAL[l];
                    double _Complex z = CMPLX(A, B), w = CMPLX(C, D);
                    putc_(z * w);
                    if (i < N_NARROW && j < N_NARROW && k < N_NARROW && l < N_NARROW)
                        putc_(z / w);
                }

    /* 2. the mixed real/complex forms. */
    for (i = 0; i < N_NARROW; i++)
        for (j = 0; j < N_NARROW; j++)
            for (k = 0; k < N_NARROW; k++) {
                volatile double A = VAL[i], B = VAL[j], X = VAL[k];
                double _Complex z = CMPLX(A, B);
                double x = X;
                putc_(z * x);
                putc_(x * z);
                putc_(z + x);
                putc_(x + z);
                putc_(z - x);
                putc_(x - z);
                putc_(z / x);
                putc_(x / z);
            }

    /* 3. `float _Complex`, product and quotient. */
    for (i = 0; i < N_F32; i++)
        for (j = 0; j < N_F32; j++)
            for (k = 0; k < N_F32; k++)
                for (l = 0; l < N_F32; l++) {
                    volatile float A = VALF[i], B = VALF[j], C = VALF[k], D = VALF[l];
                    float _Complex z = CMPLXF(A, B), w = CMPLXF(C, D);
                    putcf(z * w);
                    putcf(z / w);
                }

    fclose(out);
    return 0;
}
"#;

/// The compiler to compare against, if there is one.
fn c_compiler() -> Option<String> {
    let named = std::env::var("CINRS_COMPLEX_CC")
        .or_else(|_| std::env::var("CINRS_BITFIELD_CC"))
        .or_else(|_| std::env::var("CC"))
        .unwrap_or_else(|_| "cc".to_owned());
    let ok = Command::new(&named)
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    ok.then_some(named)
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `values.h`: the two tables, as bit patterns so that no decimal literal has
/// to round the same way twice.
fn values_header() -> String {
    use std::fmt::Write as _;
    let all: Vec<f64> = VALUES.iter().chain(EXTREME).copied().collect();
    let mut out = String::from("#include <string.h>\n");
    let _ = writeln!(out, "#define N_ALL {}", all.len());
    let _ = writeln!(out, "#define N_NARROW {}", VALUES.len());
    let _ = writeln!(out, "#define N_F32 {}", VALUES_F32.len());
    out.push_str("static double VAL[N_ALL];\n");
    out.push_str("static float VALF[N_F32];\n");
    out.push_str("static const unsigned long long VAL_BITS[N_ALL] = {\n");
    for value in &all {
        let _ = writeln!(out, "    0x{:016x}ULL,", value.to_bits());
    }
    out.push_str("};\n");
    out.push_str("static const unsigned int VALF_BITS[N_F32] = {\n");
    for value in VALUES_F32 {
        let _ = writeln!(out, "    0x{:08x}U,", value.to_bits());
    }
    out.push_str("};\n");
    // Filling the tables from the bit patterns at run time keeps the C free of
    // any decimal literal the two languages could round differently.
    out.push_str(
        "__attribute__((constructor)) static void fill_tables(void) {\n\
         \x20   int i;\n\
         \x20   for (i = 0; i < N_ALL; i++) memcpy(&VAL[i], &VAL_BITS[i], sizeof VAL[i]);\n\
         \x20   for (i = 0; i < N_F32; i++) memcpy(&VALF[i], &VALF_BITS[i], sizeof VALF[i]);\n\
         }\n",
    );
    out
}

/// Runs the oracle and returns everything it wrote.
fn oracle(cc: &str) -> Vec<u8> {
    let dir = manifest_dir().join("target/complex-oracle");
    std::fs::create_dir_all(&dir).expect("target/ must be writable");
    std::fs::write(dir.join("values.h"), values_header()).expect("target/ must be writable");
    let source = dir.join("oracle.c");
    std::fs::write(&source, ORACLE_C).expect("target/ must be writable");
    let binary = dir.join("oracle");
    let data = dir.join("oracle.bin");
    let output = Command::new(cc)
        .args(["-std=c11", "-w", "-O0"])
        .arg("-I")
        .arg(&dir)
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .arg("-lm")
        .output()
        .unwrap_or_else(|e| panic!("could not run {cc}: {e}"));
    assert!(
        output.status.success(),
        "{cc} could not compile the complex oracle:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = Command::new(&binary)
        .arg(&data)
        .output()
        .unwrap_or_else(|e| panic!("could not run the oracle: {e}"));
    assert!(
        run.status.success(),
        "the oracle exited with {}",
        run.status
    );
    std::fs::read(&data).expect("the oracle wrote its answers")
}

/// Reads the answers back, one `f64` at a time.
struct Answers {
    bytes: Vec<u8>,
    at: usize,
}

impl Answers {
    fn next(&mut self) -> f64 {
        let end = self.at + 8;
        assert!(end <= self.bytes.len(), "the oracle stopped early");
        let mut bits = [0u8; 8];
        bits.copy_from_slice(&self.bytes[self.at..end]);
        self.at = end;
        f64::from_bits(u64::from_le_bytes(bits))
    }

    fn next_complex(&mut self) -> Complex<f64> {
        let re = self.next();
        let im = self.next();
        Complex::new(re, im)
    }

    fn next_complex_f32(&mut self) -> Complex<f32> {
        let re = self.next() as f32;
        let im = self.next() as f32;
        Complex::new(re, im)
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// Every product, quotient and mixed form the runtime computes, against what
/// the host's C compiler computes for the same operands.
///
/// This is the test that Annex G's recovery is right: there is no way to write
/// down the expected answer for `(∞ + 0i) · (3 − 4i)` from first principles
/// that is not simply a second implementation of the rule, so the oracle is
/// the compiler the crate claims to agree with.
#[test]
fn the_product_and_quotient_agree_with_cc() {
    let Some(cc) = c_compiler() else {
        println!(
            "skipping: no C compiler on PATH (set CINRS_COMPLEX_CC or CC to name one), \
             so there is nothing to compare the complex arithmetic against"
        );
        return;
    };
    let mut answers = Answers {
        bytes: oracle(&cc),
        at: 0,
    };
    let all: Vec<f64> = VALUES.iter().chain(EXTREME).copied().collect();
    let narrow = VALUES.len();
    // One entry per kind of operation, so that a mistake in a rare one is not
    // buried under thousands of failures from a common one.
    let mut failures: BTreeMap<&'static str, (usize, String)> = BTreeMap::new();
    let mut note = |what: &'static str, detail: String| {
        let entry = failures.entry(what).or_insert((0, detail));
        entry.0 += 1;
    };

    for (i, &a) in all.iter().enumerate() {
        for (j, &b) in all.iter().enumerate() {
            for (k, &c) in all.iter().enumerate() {
                for (l, &d) in all.iter().enumerate() {
                    let (z, w) = (Complex::new(a, b), Complex::new(c, d));
                    let want = answers.next_complex();
                    let got = rt::mul_f64(z, w);
                    if !same_c(got, want) {
                        note(
                            "double _Complex *",
                            format!("({a:e},{b:e}) * ({c:e},{d:e}): {cc} {want:?}, cinrs {got:?}"),
                        );
                    }
                    if i < narrow && j < narrow && k < narrow && l < narrow {
                        let want = answers.next_complex();
                        let got = rt::div_f64(z, w);
                        if !same_c(got, want) {
                            note(
                                "double _Complex /",
                                format!(
                                    "({a:e},{b:e}) / ({c:e},{d:e}): {cc} {want:?}, cinrs {got:?}"
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    for &a in VALUES {
        for &b in VALUES {
            for &x in VALUES {
                let z = Complex::new(a, b);
                let cases: [(&'static str, Complex<f64>); 8] = [
                    ("z * x", rt::mul_real_f64(z, x)),
                    ("x * z", rt::mul_real_f64(z, x)),
                    ("z + x", rt::add_real_f64(z, x)),
                    ("x + z", rt::add_real_f64(z, x)),
                    ("z - x", rt::sub_real_f64(z, x)),
                    ("x - z", rt::real_sub_f64(x, z)),
                    ("z / x", rt::div_real_f64(z, x)),
                    ("x / z", rt::real_div_f64(x, z)),
                ];
                for (what, got) in cases {
                    let want = answers.next_complex();
                    if !same_c(got, want) {
                        note(
                            what,
                            format!(
                                "{what} for z=({a:e},{b:e}) x={x:e}: {cc} {want:?}, cinrs {got:?}"
                            ),
                        );
                    }
                }
            }
        }
    }

    for &a in VALUES_F32 {
        for &b in VALUES_F32 {
            for &c in VALUES_F32 {
                for &d in VALUES_F32 {
                    let (z, w) = (Complex::new(a, b), Complex::new(c, d));
                    let cases: [(&'static str, Complex<f32>); 2] = [
                        ("float _Complex *", rt::mul_f32(z, w)),
                        ("float _Complex /", rt::div_f32(z, w)),
                    ];
                    for (what, got) in cases {
                        let want = answers.next_complex_f32();
                        if !same_c32(got, want) {
                            note(
                                what,
                                format!(
                                    "float ({a:e},{b:e}) {what} ({c:e},{d:e}): \
                                     {cc} {want:?}, cinrs {got:?}"
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    assert!(answers.done(), "the oracle wrote more than was read back");
    let report: Vec<String> = failures
        .iter()
        .map(|(what, (count, first))| format!("{what}: {count} disagreements, e.g. {first}"))
        .collect();
    assert!(
        report.is_empty(),
        "cinrs::rt disagrees with {cc} on complex arithmetic:\n{}",
        report.join("\n")
    );
}
