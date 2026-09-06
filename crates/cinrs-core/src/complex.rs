//! Complex arithmetic, for folding a constant expression.
//!
//! `static double _Complex z = (1.0 + 2.0i) * (3.0 - 4.0i);` has to become a
//! literal: a Rust `static` initialiser is a constant expression, and
//! [`crate::sema`] reduces every arithmetic one to a value before code
//! generation ever sees it. So the front end needs the same arithmetic the
//! *runtime* has, and this is it.
//!
//! # Why it is written twice
//!
//! The generated code calls `cinrs_rt::complex`, a `no_std` crate built around
//! `num_complex::Complex`; this crate is the procedural macro's own front end
//! and has no business depending on the runtime — a build that said
//! `default-features = false` would then still compile `num-complex` for a
//! feature it had switched off. The two copies are kept honest by a test
//! rather than by a shared crate: `tests/complex.rs` folds a table of
//! constants through *this* code and computes the same products and quotients
//! through the runtime at run time, and fails if any bit differs.
//!
//! See `cinrs_rt::complex` for what the algorithms are and why: the naive
//! product with Annex G.5.1's infinity recovery, Smith's algorithm for the
//! `double` quotient, and the closed form in the wider format for the `float`
//! one.

/// The two parts of a complex value, always carried as `f64`.
///
/// A `float _Complex` constant is carried the same way and rounded to `f32`
/// where it is stored, exactly as [`crate::ir::ConstValue::Float`] carries a
/// `float`.
pub type Parts = (f64, f64);

/// Generates the product and the `f64` quotient for one component width.
macro_rules! narrow_ops {
    ($t:ty, $mul:ident) => {
        /// The naive product with Annex G.5.1's recovery, in this width.
        fn $mul(mut a: $t, mut b: $t, mut c: $t, mut d: $t) -> ($t, $t) {
            let (ac, bd, ad, bc) = (a * c, b * d, a * d, b * c);
            let mut x = ac - bd;
            let mut y = ad + bc;
            if x.is_nan() && y.is_nan() {
                let mut recalc = false;
                let unit = |v: $t| if v.is_infinite() { 1.0 } else { 0.0 as $t }.copysign(v);
                let tame = |v: $t| {
                    if v.is_nan() {
                        (0.0 as $t).copysign(v)
                    } else {
                        v
                    }
                };
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
            (x, y)
        }
    };
}

narrow_ops!(f32, mul_parts_f32);
narrow_ops!(f64, mul_parts_f64);

/// The product of two `double _Complex` constants.
pub fn mul((a, b): Parts, (c, d): Parts) -> Parts {
    mul_parts_f64(a, b, c, d)
}

/// The product of two `float _Complex` constants, computed in single
/// precision.
///
/// The width matters: an intermediate that overflows to infinity in `float`
/// makes `(10²⁰ + 10²⁰i)²` a NaN there and a finite zero in `double`, and the
/// runtime computes it in `float`.
pub fn mul_f32((a, b): Parts, (c, d): Parts) -> Parts {
    let (x, y) = mul_parts_f32(a as f32, b as f32, c as f32, d as f32);
    (f64::from(x), f64::from(y))
}

/// The quotient of two `double _Complex` constants: Smith's algorithm with
/// Annex G.5.1's recovery.
pub fn div((a, b): Parts, (c, d): Parts) -> Parts {
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
    recover((a, b), (c, d), (x, y))
}

/// The quotient of two `float _Complex` constants: the closed form in
/// `double`, which cannot overflow for `float` operands, with the same
/// recovery and a single rounding at the end.
pub fn div_f32((a, b): Parts, (c, d): Parts) -> Parts {
    let (a, b) = (f64::from(a as f32), f64::from(b as f32));
    let (c, d) = (f64::from(c as f32), f64::from(d as f32));
    let denom = c * c + d * d;
    let quotient = ((a * c + b * d) / denom, (b * c - a * d) / denom);
    let (x, y) = recover((a, b), (c, d), quotient);
    (f64::from(x as f32), f64::from(y as f32))
}

/// Annex G.5.1's recovery for a quotient that came out NaN + iNaN.
fn recover((a, b): Parts, (c, d): Parts, (x, y): Parts) -> Parts {
    if !(x.is_nan() && y.is_nan()) {
        return (x, y);
    }
    let inf = f64::INFINITY;
    let unit = |v: f64| if v.is_infinite() { 1.0 } else { 0.0f64 }.copysign(v);
    if c == 0.0 && d == 0.0 && (!a.is_nan() || !b.is_nan()) {
        let scale = inf.copysign(c);
        return (scale * a, scale * b);
    }
    if (a.is_infinite() || b.is_infinite()) && c.is_finite() && d.is_finite() {
        let (a, b) = (unit(a), unit(b));
        return (inf * (a * c + b * d), inf * (b * c - a * d));
    }
    if (c.is_infinite() || d.is_infinite()) && a.is_finite() && b.is_finite() {
        let (c, d) = (unit(c), unit(d));
        return (0.0 * (a * c + b * d), 0.0 * (b * c - a * d));
    }
    (x, y)
}

fn abs(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & !(1u64 << 63))
}
