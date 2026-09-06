//! GCC's floating suffixes beyond C's three.
//!
//! `d`, `w`, `q` and the `_FloatN` spellings name formats wider than `double`,
//! and every one of them **is** `double` here — the same mapping `long double`
//! has. They are plain-spelled extensions, so a strict entry point says which
//! GNU entry point has them. The decimal and imaginary suffixes name types
//! this crate has no counterpart for at all, and are refused everywhere.

cinrs::gnu99! {
    double gnu_widths(void) {
        return 1.5d + 1.5w + 1.5q + 1.5f64 + 1.5f64x + 1.5f32x + 1.5F128;
    }
    float narrow(void) { return 1.5f32; }

    double decimal(void) { return 0.5dd; } //~ ERROR: decimal floating types
    double decimal_upper(void) { return 0.5DF; } //~ ERROR: decimal floating types
    double imaginary(void) { return 2.0i; } //~ ERROR: an imaginary constant needs _Complex
    double half(void) { return 1.0f16; } //~ ERROR: '_Float16' is not supported
}

cinrs::c99! {
    /* Spelled without underscores, so the strict entry point refuses it and
     * names the one that does not. */
    double strict(void) { return 1.5q; } //~ ERROR: requires a GNU dialect
    double still_invalid(void) { return 1.5z; } //~ ERROR: invalid suffix 'z'
}

fn main() {}
