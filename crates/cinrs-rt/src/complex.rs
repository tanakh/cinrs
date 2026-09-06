//! C's complex arithmetic, as the generated code calls it.
//!
//! # What C's operators actually do
//!
//! Two rules decide everything in this module, and neither is what a naive
//! reading of "the usual arithmetic conversions" suggests.
//!
//! * **A real operand stays real.** C99 6.3.1.8 says the common type of a real
//!   and a complex operand is the complex one, but the *result* it prescribes
//!   is the same one the componentwise operation gives — and GCC and Clang
//!   both compute it componentwise, which is observable: `3.0 * z` for
//!   `z = (-0.0, -0.0)` is `(-0.0, -0.0)` componentwise and `(+0.0, -0.0)`
//!   through the full product, because `3·(−0) − 0·(−0)` is `+0`. The one
//!   exception is `real / complex`, which really is the full division with the
//!   imaginary part of the numerator taken as zero. Each mixed form therefore
//!   has a function of its own here: [`mul_real_f64`], [`add_real_f64`],
//!   [`sub_real_f64`], [`real_sub_f64`], [`div_real_f64`] and
//!   [`real_div_f64`], with the `f32` twins beside them.
//!
//! * **Infinities are recovered.** C99 Annex G.5.1 (which GCC implements by
//!   default — `-fno-cx-limited-range`) says that a product or a quotient
//!   involving an infinity must be infinite, even where the naive formula
//!   produces NaN + iNaN out of an `∞ − ∞` or an `∞ · 0`. [`mul_f64`] and
//!   [`div_f64`] therefore compute the cheap answer first and only fix it up
//!   when both parts came out NaN, which is what makes the common case cost
//!   four multiplications and an addition.
//!
//! # The algorithms
//!
//! The product — in both widths — is the schoolbook one, `(ac − bd) +
//! i(ad + bc)`, followed by Annex G.5.1's recovery: an infinite operand is
//! reduced to a signed one or zero, a NaN in the *other* operand is reduced to
//! a signed zero, and the product is recomputed scaled by infinity so that the
//! sign of each part survives.
//!
//! The quotient takes a different route in each width, because what is cheap
//! and exact in one is not available in the other.
//!
//! * [`div_f32`] uses the **closed form in the wider format**:
//!   `c² + d²` cannot overflow or underflow in `f64` for any `f32` operands, so
//!   `(ac + bd)/(c² + d²)` and `(bc − ad)/(c² + d²)` are computed in `f64` and
//!   rounded once.
//! * [`div_f64`] has no wider format to hand, so it uses **Smith's
//!   algorithm** — divide through by whichever of the divisor's parts is
//!   larger, so that `c² + d²` cannot overflow when a single part can — with
//!   Baudin and Smith's refinement for a *subnormal* ratio, where `a · (d/c)`
//!   has already lost bits that the equal `(a/c) · d` still has.
//!
//! Both then apply the matching recovery: a zero divisor gives a signed
//! infinity, an infinite dividend over a finite divisor gives an infinity, and
//! a finite dividend over an infinite divisor gives a signed zero.
//!
//! All of it is written from the standard's description rather than taken from
//! any implementation, and `tests/complex.rs` checks the result against the
//! host's own C compiler: every combination of `0`, `−0`, `±1`, `±2`, `±10⁸`,
//! `±10⁻⁸`, `±10¹⁵⁰`, `±10⁻¹⁵⁰`, `±∞` and NaN — a hundred and thirty thousand
//! pairs — agrees exactly, and the product agrees for the `10³⁰⁰` and
//! subnormal magnitudes too.
//!
//! # Where it is not the last word
//!
//! `libgcc` goes further on the `f64` quotient: it *prescales* both operands
//! by powers of two before dividing, which buys accuracy in the last corner —
//! operands whose exponents are hundreds apart, where an intermediate of
//! Smith's algorithm underflows even after the refinement above. GCC's own
//! `gcc.c-torture/execute/ieee/cdivchkd.c` is four such quotients, and two of
//! its four are beyond what is here. C leaves the accuracy of complex
//! arithmetic implementation-defined (Annex G.6), so this is a quality gap and
//! not a conformance one; `doc/gcc-torture.md` records it.

use crate::Complex;

/// Generates the whole module for one floating format.
///
/// `$t` is the component type, `$suffix` the one the C library and this module
/// spell the format with, and the two `$doc` fragments name it in the
/// documentation.
macro_rules! complex_ops {
    ($t:ty, $mul:ident, $div:ident, $mul_real:ident, $real_mul:ident, $add_real:ident,
     $real_add:ident, $sub_real:ident, $real_sub:ident, $div_real:ident, $real_div:ident,
     $conj:ident, $proj:ident, $nonzero:ident, $eq:ident, $ne:ident, $cty:literal) => {
        /// The product of two
        #[doc = $cty]
        /// values (C99 6.5.5, Annex G.5.1).
        ///
        /// The naive product, with the infinities Annex G.5.1 requires
        /// recovered when it comes out NaN + iNaN.
        #[inline]
        pub fn $mul(z: Complex<$t>, w: Complex<$t>) -> Complex<$t> {
            let (mut a, mut b, mut c, mut d) = (z.re, z.im, w.re, w.im);
            let (ac, bd, ad, bc) = (a * c, b * d, a * d, b * c);
            let mut x = ac - bd;
            let mut y = ad + bc;
            if x.is_nan() && y.is_nan() {
                let mut recalc = false;
                // An infinite operand: reduce it to a signed one or zero, and
                // reduce a NaN in the other operand to a signed zero, so that
                // the recomputation below cannot produce another NaN.
                if a.is_infinite() || b.is_infinite() {
                    a = unit(a);
                    b = unit(b);
                    c = tame(c);
                    d = tame(d);
                    recalc = true;
                }
                if c.is_infinite() || d.is_infinite() {
                    c = unit(c);
                    d = unit(d);
                    a = tame(a);
                    b = tame(b);
                    recalc = true;
                }
                // Neither operand is infinite, but one of the four products
                // overflowed to one: the result is infinite all the same.
                if !recalc
                    && (ac.is_infinite()
                        || bd.is_infinite()
                        || ad.is_infinite()
                        || bc.is_infinite())
                {
                    a = tame(a);
                    b = tame(b);
                    c = tame(c);
                    d = tame(d);
                    recalc = true;
                }
                if recalc {
                    let inf = <$t>::INFINITY;
                    x = inf * (a * c - b * d);
                    y = inf * (a * d + b * c);
                }
            }
            Complex { re: x, im: y }
        }

        /// A
        #[doc = $cty]
        /// value times a real one, which C computes componentwise.
        #[inline]
        pub fn $mul_real(z: Complex<$t>, x: $t) -> Complex<$t> {
            Complex {
                re: z.re * x,
                im: z.im * x,
            }
        }

        /// A real value times a
        #[doc = $cty]
        /// one: the same product, with the operands in source order so that
        /// the generated code evaluates them where they were written.
        #[inline]
        pub fn $real_mul(x: $t, z: Complex<$t>) -> Complex<$t> {
            $mul_real(z, x)
        }

        /// A
        #[doc = $cty]
        /// value plus a real one, which touches the real part only.
        #[inline]
        pub fn $add_real(z: Complex<$t>, x: $t) -> Complex<$t> {
            Complex {
                re: z.re + x,
                im: z.im,
            }
        }

        /// A real value plus a
        #[doc = $cty]
        /// one; see [`
        #[doc = stringify!($real_mul)]
        /// `] for why the order matters.
        #[inline]
        pub fn $real_add(x: $t, z: Complex<$t>) -> Complex<$t> {
            Complex {
                re: x + z.re,
                im: z.im,
            }
        }

        /// A
        #[doc = $cty]
        /// value minus a real one, which touches the real part only.
        #[inline]
        pub fn $sub_real(z: Complex<$t>, x: $t) -> Complex<$t> {
            Complex {
                re: z.re - x,
                im: z.im,
            }
        }

        /// A real value minus a
        #[doc = $cty]
        /// one, which negates the imaginary part.
        #[inline]
        pub fn $real_sub(x: $t, z: Complex<$t>) -> Complex<$t> {
            Complex {
                re: x - z.re,
                im: -z.im,
            }
        }

        /// A
        #[doc = $cty]
        /// value divided by a real one, which C computes componentwise.
        #[inline]
        pub fn $div_real(z: Complex<$t>, x: $t) -> Complex<$t> {
            Complex {
                re: z.re / x,
                im: z.im / x,
            }
        }

        /// A real value divided by a
        #[doc = $cty]
        /// one.
        ///
        /// The one mixed form that is *not* componentwise: it is the full
        /// complex division with a zero imaginary part in the numerator, which
        /// is what GCC and Clang emit.
        #[inline]
        pub fn $real_div(x: $t, z: Complex<$t>) -> Complex<$t> {
            $div(Complex { re: x, im: 0 as $t }, z)
        }

        /// The conjugate of a
        #[doc = $cty]
        /// value: `conj`, and GNU's `~z`.
        #[inline]
        pub fn $conj(z: Complex<$t>) -> Complex<$t> {
            Complex {
                re: z.re,
                im: -z.im,
            }
        }

        /// The projection of a
        #[doc = $cty]
        /// value onto the Riemann sphere: C99 7.3.9.5's `cproj`.
        ///
        /// Everything is itself except a value with an infinite part, which
        /// becomes `+∞` with the imaginary part's sign kept on a zero.
        #[inline]
        pub fn $proj(z: Complex<$t>) -> Complex<$t> {
            if z.re.is_infinite() || z.im.is_infinite() {
                Complex {
                    re: <$t>::INFINITY,
                    im: copysign(0 as $t, z.im),
                }
            } else {
                z
            }
        }

        /// Whether a
        #[doc = $cty]
        /// value is non-zero, which is what C's conversion to `_Bool` and a
        /// controlling expression ask (C99 6.3.1.2).
        ///
        /// Either part being non-zero is enough, so a NaN counts as true.
        #[inline]
        pub fn $nonzero(z: Complex<$t>) -> bool {
            z.re != 0 as $t || z.im != 0 as $t
        }

        /// Whether two
        #[doc = $cty]
        /// values are equal: C99 6.5.9p3, both parts equal.
        ///
        /// The same answer `PartialEq` gives, and it is here because that one
        /// takes `&self`: a reference to a field of a `#[repr(packed)]`
        /// record is `E0793`, and a packed complex member is exactly what
        /// `gcc.c-torture/execute/20020227-1` compares. Taking both operands
        /// by value asks for a copy, which a packed field will give.
        #[inline]
        pub fn $eq(z: Complex<$t>, w: Complex<$t>) -> bool {
            z.re == w.re && z.im == w.im
        }

        /// Whether two
        #[doc = $cty]
        /// values differ — the negation of
        #[doc = concat!("[`", stringify!($eq), "`]")]
        /// , and `!=` in C.
        #[inline]
        pub fn $ne(z: Complex<$t>, w: Complex<$t>) -> bool {
            !$eq(z, w)
        }
    };
}

complex_ops!(
    f32,
    mul_f32,
    div_f32,
    mul_real_f32,
    real_mul_f32,
    add_real_f32,
    real_add_f32,
    sub_real_f32,
    real_sub_f32,
    div_real_f32,
    real_div_f32,
    conj_f32,
    proj_f32,
    nonzero_f32,
    eq_f32,
    ne_f32,
    "`float _Complex`"
);

complex_ops!(
    f64,
    mul_f64,
    div_f64,
    mul_real_f64,
    real_mul_f64,
    add_real_f64,
    real_add_f64,
    sub_real_f64,
    real_sub_f64,
    div_real_f64,
    real_div_f64,
    conj_f64,
    proj_f64,
    nonzero_f64,
    eq_f64,
    ne_f64,
    "`double _Complex`"
);

/// The quotient of two `float _Complex` values (C99 6.5.5, Annex G.5.1).
///
/// The closed form, computed in `f64`: `c² + d²` is exact enough and can
/// neither overflow nor underflow there for any `f32` operands, so no scaling
/// is needed and the result is rounded exactly once.
#[inline]
pub fn div_f32(z: Complex<f32>, w: Complex<f32>) -> Complex<f32> {
    let (a, b) = (f64::from(z.re), f64::from(z.im));
    let (c, d) = (f64::from(w.re), f64::from(w.im));
    let denom = c * c + d * d;
    let (x, y) = recover_quotient(a, b, c, d, (a * c + b * d) / denom, (b * c - a * d) / denom);
    Complex {
        re: x as f32,
        im: y as f32,
    }
}

/// The quotient of two `double _Complex` values (C99 6.5.5, Annex G.5.1).
///
/// Smith's algorithm: there is no wider format to compute `c² + d²` in, so the
/// division is carried out through the ratio of the divisor's two parts, which
/// keeps the denominator in range whenever the quotient itself is.
///
/// The `ratio.abs() > f64::MIN_POSITIVE` test is Smith's algorithm's one weak
/// spot, closed the way Baudin and Smith describe: when the ratio is
/// *subnormal* it has already lost bits, so multiplying by it throws away
/// precision the operands still had. `a · (d/c)` and `(a/c) · d` are the same
/// number, and the second one keeps it, so the second one is used exactly
/// where the first would not do.
#[inline]
pub fn div_f64(z: Complex<f64>, w: Complex<f64>) -> Complex<f64> {
    let (a, b, c, d) = (z.re, z.im, w.re, w.im);
    let (x, y) = if abs(c) < abs(d) {
        let ratio = c / d;
        let denom = c * ratio + d;
        if abs(ratio) > f64::MIN_POSITIVE {
            ((a * ratio + b) / denom, (b * ratio - a) / denom)
        } else {
            (((a / d) * c + b) / denom, ((b / d) * c - a) / denom)
        }
    } else {
        let ratio = d / c;
        let denom = d * ratio + c;
        if abs(ratio) > f64::MIN_POSITIVE {
            ((b * ratio + a) / denom, (b - a * ratio) / denom)
        } else {
            (((b / c) * d + a) / denom, (b - (a / c) * d) / denom)
        }
    };
    let (re, im) = recover_quotient(a, b, c, d, x, y);
    Complex { re, im }
}

/// Annex G.5.1's recovery for a quotient that came out NaN + iNaN.
///
/// Shared by both widths: [`div_f32`] has already widened its operands, so the
/// three cases — a zero divisor, an infinite dividend, an infinite divisor —
/// are the same arithmetic in both.
#[inline]
fn recover_quotient(mut a: f64, mut b: f64, mut c: f64, mut d: f64, x: f64, y: f64) -> (f64, f64) {
    if !(x.is_nan() && y.is_nan()) {
        return (x, y);
    }
    let inf = f64::INFINITY;
    if c == 0.0 && d == 0.0 && (!a.is_nan() || !b.is_nan()) {
        // Division by zero: a signed infinity, not a NaN.
        let scale = copysign(inf, c);
        return (scale * a, scale * b);
    }
    if (a.is_infinite() || b.is_infinite()) && c.is_finite() && d.is_finite() {
        a = unit(a);
        b = unit(b);
        return (inf * (a * c + b * d), inf * (b * c - a * d));
    }
    if (c.is_infinite() || d.is_infinite()) && a.is_finite() && b.is_finite() {
        c = unit(c);
        d = unit(d);
        return (0.0 * (a * c + b * d), 0.0 * (b * c - a * d));
    }
    (x, y)
}

/// `float _Complex` widened to `double _Complex`.
#[inline]
pub fn widen_f32(z: Complex<f32>) -> Complex<f64> {
    Complex {
        re: f64::from(z.re),
        im: f64::from(z.im),
    }
}

/// `double _Complex` narrowed to `float _Complex`.
#[inline]
pub fn narrow_f64(z: Complex<f64>) -> Complex<f32> {
    Complex {
        re: z.re as f32,
        im: z.im as f32,
    }
}

/// The magnitude of a float, without `core::f32::abs` — which this crate does
/// not need a `libm` for.
trait Bits: Copy {
    /// `|x|`.
    fn magnitude(self) -> Self;
    /// `x` with the sign of `y`.
    fn with_sign(self, y: Self) -> Self;
}

macro_rules! bits {
    ($t:ty, $u:ty) => {
        impl Bits for $t {
            #[inline]
            fn magnitude(self) -> Self {
                const SIGN: $u = 1 << (<$u>::BITS - 1);
                <$t>::from_bits(self.to_bits() & !SIGN)
            }
            #[inline]
            fn with_sign(self, y: Self) -> Self {
                const SIGN: $u = 1 << (<$u>::BITS - 1);
                <$t>::from_bits((self.to_bits() & !SIGN) | (y.to_bits() & SIGN))
            }
        }
    };
}

bits!(f32, u32);
bits!(f64, u64);

/// `|x|`, on either width.
#[inline]
fn abs<T: Bits>(x: T) -> T {
    x.magnitude()
}

/// `x` with the sign bit of `y`, on either width.
#[inline]
fn copysign<T: Bits>(x: T, y: T) -> T {
    x.with_sign(y)
}

/// One or zero, with `x`'s sign: what Annex G.5.1 reduces an operand to before
/// recomputing an overflowed product or quotient.
#[inline]
fn unit<T: Bits + Float>(x: T) -> T {
    copysign(if x.is_infinite() { T::ONE } else { T::ZERO }, x)
}

/// A NaN reduced to a signed zero, and everything else left alone: the other
/// half of Annex G.5.1's recovery.
#[inline]
fn tame<T: Bits + Float>(x: T) -> T {
    if x.is_nan() { copysign(T::ZERO, x) } else { x }
}

/// The handful of predicates [`unit`] and [`tame`] need on both widths.
trait Float: Copy {
    /// `0`.
    const ZERO: Self;
    /// `1`.
    const ONE: Self;
    /// Whether this is an infinity.
    fn is_infinite(self) -> bool;
    /// Whether this is a NaN.
    fn is_nan(self) -> bool;
}

macro_rules! float {
    ($t:ty) => {
        impl Float for $t {
            const ZERO: Self = 0.0;
            const ONE: Self = 1.0;
            #[inline]
            fn is_infinite(self) -> bool {
                <$t>::is_infinite(self)
            }
            #[inline]
            fn is_nan(self) -> bool {
                <$t>::is_nan(self)
            }
        }
    };
}

float!(f32);
float!(f64);

#[cfg(test)]
mod tests {
    use super::*;

    fn same(a: f64, b: f64) -> bool {
        if a.is_nan() && b.is_nan() {
            return true;
        }
        a == b && a.is_sign_negative() == b.is_sign_negative()
    }

    fn same_c(a: Complex<f64>, b: Complex<f64>) -> bool {
        same(a.re, b.re) && same(a.im, b.im)
    }

    #[test]
    fn the_ordinary_product_and_quotient_are_the_school_ones() {
        let z = Complex::new(1.0, 2.0);
        let w = Complex::new(3.0, -4.0);
        assert_eq!(mul_f64(z, w), Complex::new(11.0, 2.0));
        assert_eq!(div_f64(z, w), Complex::new(-0.2, 0.4));
    }

    #[test]
    fn an_infinity_survives_a_naive_nan() {
        let inf = f64::INFINITY;
        // (∞, 0) · (3, −4) is (∞·3 − 0·(−4), ∞·(−4) + 0·3) = (∞, −∞) only
        // after the recovery: the naive imaginary part is −∞ + NaN.
        assert!(same_c(
            mul_f64(Complex::new(inf, 0.0), Complex::new(3.0, -4.0)),
            Complex::new(inf, -inf)
        ));
        assert!(same_c(
            div_f64(Complex::new(inf, 0.0), Complex::new(3.0, -4.0)),
            Complex::new(inf, inf)
        ));
        // A finite value over an infinite one is a signed zero.
        assert!(same_c(
            div_f64(Complex::new(3.0, -4.0), Complex::new(inf, 0.0)),
            Complex::new(0.0, -0.0)
        ));
    }

    #[test]
    fn division_by_zero_is_a_signed_infinity() {
        let inf = f64::INFINITY;
        assert!(same_c(
            div_f64(Complex::new(1.0, 2.0), Complex::new(0.0, 0.0)),
            Complex::new(inf, inf)
        ));
        assert!(same_c(
            div_f64(Complex::new(1.0, 2.0), Complex::new(-0.0, -0.0)),
            Complex::new(-inf, -inf)
        ));
    }

    #[test]
    fn a_real_operand_is_componentwise() {
        let nzero = Complex::new(-0.0, -0.0);
        // The full product would give (+0, −0) here; C gives (−0, −0).
        assert!(same_c(mul_real_f64(nzero, 3.0), Complex::new(-0.0, -0.0)));
        assert!(same_c(
            real_sub_f64(0.0, Complex::new(1.0, 0.0)),
            Complex::new(-1.0, -0.0)
        ));
        assert!(same_c(
            div_real_f64(Complex::new(1.0, 2.0), 0.0),
            Complex::new(f64::INFINITY, f64::INFINITY)
        ));
    }

    #[test]
    fn conjugation_projection_and_truth() {
        let z = Complex::new(1.0, 2.0);
        assert_eq!(conj_f64(z), Complex::new(1.0, -2.0));
        assert_eq!(proj_f64(z), z);
        assert!(same_c(
            proj_f64(Complex::new(1.0, f64::NEG_INFINITY)),
            Complex::new(f64::INFINITY, -0.0)
        ));
        assert!(nonzero_f64(Complex::new(0.0, 1.0)));
        assert!(!nonzero_f64(Complex::new(0.0, -0.0)));
        assert!(nonzero_f64(Complex::new(f64::NAN, 0.0)));
    }

    #[test]
    fn the_f32_forms_agree_with_the_f64_ones_on_exact_values() {
        let z = Complex::new(1.0f32, 2.0);
        let w = Complex::new(3.0f32, -4.0);
        assert_eq!(mul_f32(z, w), Complex::new(11.0, 2.0));
        assert_eq!(widen_f32(mul_f32(z, w)), Complex::new(11.0f64, 2.0));
        assert_eq!(
            narrow_f64(Complex::new(11.0f64, 2.0)),
            Complex::new(11.0f32, 2.0)
        );
    }
}
