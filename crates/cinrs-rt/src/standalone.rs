//! The complex type without `num-complex`, for a build with no Cargo; see
//! [Without Cargo](super#without-cargo).

/// C's `float _Complex` and `double _Complex`: the real part, then the
/// imaginary part, as `num_complex::Complex` lays them out.
///
/// It has what the generated code uses of `num_complex::Complex` and nothing
/// more: the two fields, [`Complex::new`], and the componentwise `+`, `-` and
/// unary `-`. The product and the quotient are Annex G's, in
/// [`complex`](super::complex).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex<T> {
    /// The real part.
    pub re: T,
    /// The imaginary part.
    pub im: T,
}

impl<T> Complex<T> {
    /// `re + im·i`.
    pub const fn new(re: T, im: T) -> Self {
        Self { re, im }
    }
}

impl<T: core::ops::Add<Output = T>> core::ops::Add for Complex<T> {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        Self::new(self.re + other.re, self.im + other.im)
    }
}

impl<T: core::ops::Sub<Output = T>> core::ops::Sub for Complex<T> {
    type Output = Self;

    #[inline]
    fn sub(self, other: Self) -> Self {
        Self::new(self.re - other.re, self.im - other.im)
    }
}

impl<T: core::ops::Neg<Output = T>> core::ops::Neg for Complex<T> {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        Self::new(-self.re, -self.im)
    }
}
