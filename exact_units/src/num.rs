//! Powers of ten and of other numbers in `f64` / `f32`, without `powi` (not in `core`): what scales and prefixes
//! multiply by.

const POW10_F64: [f64; 31] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22, 1e23, 1e24, 1e25, 1e26, 1e27, 1e28, 1e29, 1e30,
];

/// `10^exp` in `const` code (what [Scale::Pow10](crate::Scale::Pow10) multiplies by).
pub const fn pow10_f64(exp: i8) -> f64 {
    let mut x = 1.0;
    let mut i = 0;
    while i < exp.unsigned_abs() {
        x *= 10.0;
        i += 1;
    }
    if exp < 0 { 1.0 / x } else { x }
}

/// `x^n` in `const` code (what [UnitExpr::Pow](crate::UnitExpr::Pow) multiplies by).
pub const fn powi_f64(x: f64, n: i8) -> f64 {
    let mut y = 1.0;
    let mut i = 0;
    while i < n.unsigned_abs() {
        y *= x;
        i += 1;
    }
    if n < 0 { 1.0 / y } else { y }
}

/// `√x` in `const` code, rounded to nearest like `f64::sqrt` (what [UnitExpr::Sqrt](crate::UnitExpr::Sqrt)
/// multiplies by): `sqrt` is not in `core`. An integer square root of the significand, NaN below zero.
pub const fn sqrt_f64(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 || x == f64::INFINITY {
        return x;
    }
    const MANT: u64 = (1 << 52) - 1;
    let bits = x.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as i32;
    // x = mant · 2^exp, mant with its leading bit at 52
    let (mut mant, mut exp) = if biased == 0 {
        (bits & MANT, -1074)
    } else {
        ((bits & MANT) | (1 << 52), biased - 1075)
    };
    while mant < (1 << 52) {
        mant <<= 1;
        exp -= 1;
    }
    if exp % 2 != 0 {
        mant <<= 1;
        exp -= 1;
    }
    // √(mant · 2^56) has 55 bits: 53 kept, the rest and the remainder round
    let n = (mant as u128) << 56;
    let mut r = n.isqrt();
    let inexact = r * r != n;
    let extra = (128 - r.leading_zeros()) as i32 - 53;
    let low = r & ((1 << extra) - 1);
    let half = 1 << (extra - 1);
    r >>= extra;
    if low > half || (low == half && (inexact || r & 1 == 1)) {
        r += 1;
    }
    let mut e = (exp >> 1) - 28 + extra;
    if r == 1 << 53 {
        r >>= 1;
        e += 1;
    }
    // r · 2^e with r in [2^52, 2^53): always a normal number
    f64::from_bits((((e + 52 + 1023) as u64) << 52) | (r as u64 & MANT))
}

/// `x * 10^exp`, for the powers prefixes have (-60..=60 between two of them).
pub fn scale_f64(x: f64, exp: i8) -> f64 {
    let mut x = x;
    let mut exp = exp as i32;
    while exp > 30 {
        x *= 1e30;
        exp -= 30;
    }
    while exp < -30 {
        x /= 1e30;
        exp += 30;
    }
    if exp >= 0 {
        x * POW10_F64[exp as usize]
    } else {
        x / POW10_F64[(-exp) as usize]
    }
}

/// Powers of ten that `f32` holds exactly (up to 10^10).
const POW10_F32: [f32; 11] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10];

/// `x * 10^exp` in `f32`, without `powi` (not in `core`): one multiplication or division by an exact power of ten for
/// the usual prefixes, so 2 m· is 0.002 rounded once, not three times.
pub fn scale_f32(x: f32, exp: i8) -> f32 {
    let mut x = x;
    let mut exp = exp as i32;
    while exp > 10 {
        x *= 1e10;
        exp -= 10;
    }
    while exp < -10 {
        x /= 1e10;
        exp += 10;
    }
    if exp >= 0 {
        x * POW10_F32[exp as usize]
    } else {
        x / POW10_F32[(-exp) as usize]
    }
}
