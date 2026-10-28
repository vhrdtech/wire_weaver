//! [ExactScale]: a factor written with integers only, `num / den · 10^pow10 · 2^pow2 · π^pi`, so every reader computes
//! the same value from it and two scales compare exactly.

#[cfg(feature = "shrink_wrap")]
use shrink_wrap::prelude::*;

/// A factor `num / den · 10^pow10 · 2^pow2 · π^pi` with integers only: `10⁻⁶` (µ), `2¹⁰` (Ki), `1/65535` (a 16-bit
/// duty), `π/180` (°), `2π` (a turn). Never an `f64`, so every client reproduces a value bit-exactly, and a scale
/// fixed in a type ([Fixed](crate::Fixed)'s `P`) converts integers without a rounding step.
///
/// Always canonical: `num` and `den` have no common factor and no factor 2 or 5 (those are in `pow10` / `pow2`), so
/// two scales of the same factor are equal field by field. [new](Self::new) makes one canonical; a value read from the
/// wire that isn't is refused.
///
/// On the wire: `num` and `den` as [UNib32] (a nibble each for small ones), then the three exponents as `i8`.
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(borrowed, owned(feature = "std"), final_structure, validate = Self::validate)
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ExactScale {
    #[cfg_attr(feature = "shrink_wrap", wire(UNib32))]
    num: u32,
    #[cfg_attr(feature = "shrink_wrap", wire(UNib32))]
    den: u32,
    pow10: i8,
    pow2: i8,
    pi: i8,
}

impl ExactScale {
    /// 1.
    pub const ONE: ExactScale = ExactScale::pow10(0);

    /// `num / den · 10^pow10 · 2^pow2 · π^pi`, made canonical. Panics (at compile time in a `const`) where
    /// [try_new](Self::try_new) is `None`.
    pub const fn new(num: u32, den: u32, pow10: i8, pow2: i8, pi: i8) -> Self {
        match Self::try_new(num, den, pow10, pow2, pi) {
            Some(s) => s,
            None => panic!("a scale is not zero, and its exponents stay within i8"),
        }
    }

    /// `num / den · 10^pow10 · 2^pow2 · π^pi`, made canonical; `None` for `num == 0`, `den == 0`, or exponents that
    /// leave `i8` once the factors 2 and 5 are moved into them.
    pub const fn try_new(num: u32, den: u32, pow10: i8, pow2: i8, pi: i8) -> Option<Self> {
        if num == 0 || den == 0 {
            return None;
        }
        let (mut num, mut den) = (num, den);
        let (mut e10, mut e2) = (pow10 as i32, pow2 as i32);
        let g = gcd(num, den);
        num /= g;
        den /= g;
        // 2 and 5 out of num and den: 2 = 2¹, 5 = 10¹ · 2⁻¹
        while num.is_multiple_of(2) {
            num /= 2;
            e2 += 1;
        }
        while num.is_multiple_of(5) {
            num /= 5;
            e10 += 1;
            e2 -= 1;
        }
        while den.is_multiple_of(2) {
            den /= 2;
            e2 -= 1;
        }
        while den.is_multiple_of(5) {
            den /= 5;
            e10 -= 1;
            e2 += 1;
        }
        if e10 < i8::MIN as i32
            || e10 > i8::MAX as i32
            || e2 < i8::MIN as i32
            || e2 > i8::MAX as i32
        {
            return None;
        }
        Some(ExactScale {
            num,
            den,
            pow10: e10 as i8,
            pow2: e2 as i8,
            pi,
        })
    }

    /// `10^e`: a decimal prefix.
    pub const fn pow10(e: i8) -> Self {
        ExactScale {
            num: 1,
            den: 1,
            pow10: e,
            pow2: 0,
            pi: 0,
        }
    }

    /// `2^e`: a binary prefix.
    pub const fn pow2(e: i8) -> Self {
        ExactScale {
            num: 1,
            den: 1,
            pow10: 0,
            pow2: e,
            pi: 0,
        }
    }

    /// `num / den`.
    pub const fn rational(num: u32, den: u32) -> Self {
        Self::new(num, den, 0, 0, 0)
    }

    /// The parts: `(num, den, pow10, pow2, pi)`.
    pub const fn parts(&self) -> (u32, u32, i8, i8, i8) {
        (self.num, self.den, self.pow10, self.pow2, self.pi)
    }

    /// The power of ten when the scale is one alone (`Some(-6)` for µ), `None` otherwise.
    pub const fn as_pow10(&self) -> Option<i8> {
        if self.num == 1 && self.den == 1 && self.pow2 == 0 && self.pi == 0 {
            Some(self.pow10)
        } else {
            None
        }
    }

    /// `self · rhs`.
    pub const fn mul(self, rhs: Self) -> Self {
        let a = gcd(self.num, rhs.den);
        let b = gcd(rhs.num, self.den);
        let num = (self.num / a) as u64 * (rhs.num / b) as u64;
        let den = (self.den / b) as u64 * (rhs.den / a) as u64;
        assert!(
            num <= u32::MAX as u64 && den <= u32::MAX as u64,
            "scale too large"
        );
        Self::new(
            num as u32,
            den as u32,
            add(self.pow10, rhs.pow10 as i16),
            add(self.pow2, rhs.pow2 as i16),
            add(self.pi, rhs.pi as i16),
        )
    }

    /// `1 / self`.
    pub const fn recip(self) -> Self {
        ExactScale {
            num: self.den,
            den: self.num,
            pow10: add(0, -(self.pow10 as i16)),
            pow2: add(0, -(self.pow2 as i16)),
            pi: add(0, -(self.pi as i16)),
        }
    }

    /// `self / rhs`.
    pub const fn div(self, rhs: Self) -> Self {
        self.mul(rhs.recip())
    }

    /// The factor as an `f64` (rounded, unlike the scale itself).
    pub const fn to_f64(&self) -> f64 {
        let x = self.num as f64 / self.den as f64;
        x * crate::num::pow10_f64(self.pow10)
            * crate::num::powi_f64(2.0, self.pow2)
            * crate::num::powi_f64(core::f64::consts::PI, self.pi)
    }

    /// `x · self` in integers, exactly: `None` when the result is not whole, doesn't fit `i128`, or has a power of π.
    pub fn apply_int(&self, x: i128) -> Option<i128> {
        if self.pi != 0 {
            return None;
        }
        if x == 0 {
            return Some(0);
        }
        // 10^e10 · 2^e2 = 5^e10 · 2^(e10 + e2)
        let (d5, d2) = (self.pow10 as i32, self.pow10 as i32 + self.pow2 as i32);
        // multiplications first, so a division that is exact after them isn't refused
        let mut x = x.checked_mul(self.num as i128)?;
        if d5 > 0 {
            x = x.checked_mul(5i128.checked_pow(d5 as u32)?)?;
        }
        if d2 > 0 {
            x = x.checked_mul(2i128.checked_pow(d2 as u32)?)?;
        }
        let mut div = |p: i128| {
            if x % p == 0 {
                x /= p;
                true
            } else {
                false
            }
        };
        if !div(self.den as i128) {
            return None;
        }
        for (base, d) in [(5i128, d5), (2, d2)] {
            if d < 0 && !div(base.checked_pow(d.unsigned_abs())?) {
                return None;
            }
        }
        Some(x)
    }

    /// `x · self` in floats.
    pub fn apply_f64(&self, x: f64) -> f64 {
        let x = x * self.num as f64 / self.den as f64;
        let x = crate::num::scale_f64(x, self.pow10);
        x * crate::num::powi_f64(2.0, self.pow2)
            * crate::num::powi_f64(core::f64::consts::PI, self.pi)
    }

    #[cfg(feature = "shrink_wrap")]
    fn validate(&self) -> Result<(), shrink_wrap::Error> {
        let canonical = Self::try_new(self.num, self.den, self.pow10, self.pow2, self.pi);
        if canonical == Some(*self) {
            Ok(())
        } else {
            Err(shrink_wrap::Error::SubtypeOutOfRange)
        }
    }
}

/// `a + b` within `i8`, a panic (a compile error in a `const`) otherwise.
const fn add(a: i8, b: i16) -> i8 {
    let x = a as i16 + b;
    assert!(
        x >= i8::MIN as i16 && x <= i8::MAX as i16,
        "exponent of a scale out of i8"
    );
    x as i8
}

const fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// `·10⁻⁴`, `·1/65535`, `·π/180`, `·2π`: the factor as written after a number. Empty for 1.
impl core::fmt::Display for ExactScale {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (num, den, p10, p2, pi) = self.parts();
        if (num, den, pi) == (1, 1, 0) && (p10 == 0 || p2 == 0) {
            // a power alone: 10⁻⁴, 2¹⁰
            return match (p10, p2) {
                (0, 0) => Ok(()),
                (e, 0) => {
                    f.write_str("·10")?;
                    crate::unit::write_superscript(f, e)
                }
                (_, e) => {
                    f.write_str("·2")?;
                    crate::unit::write_superscript(f, e)
                }
            };
        }
        f.write_str("·")?;
        // a fraction: the powers of 10 and 2 multiplied into it where that fits
        let mut written = false;
        let (n, d) = match (spread(num, p10, p2, 1), spread(den, p10, p2, -1)) {
            (Some(n), Some(d)) => {
                let (mut a, mut b) = (n, d);
                while b != 0 {
                    (a, b) = (b, a % b);
                }
                (n / a, d / a)
            }
            _ => {
                write!(f, "{num}")?;
                for (base, e) in [("10", p10), ("2", p2)] {
                    if e != 0 {
                        write!(f, "·{base}")?;
                        crate::unit::write_superscript(f, e)?;
                    }
                }
                written = true;
                (1, den as u128)
            }
        };
        if !written && (n != 1 || pi == 0) {
            write!(f, "{n}")?;
        }
        if pi != 0 {
            f.write_str("π")?;
            if pi != 1 {
                crate::unit::write_superscript(f, pi)?;
            }
        }
        if d != 1 {
            write!(f, "/{d}")?;
        }
        Ok(())
    }
}

/// `x` times the powers of `10^p10 · 2^p2` that are positive for `sign` 1 (a numerator) or negative for -1 (a
/// denominator), if that fits `u64`.
fn spread(x: u32, p10: i8, p2: i8, sign: i8) -> Option<u128> {
    let mut x = x as u128;
    for (base, e) in [(10u128, p10), (2, p2)] {
        let e = e as i32 * sign as i32;
        if e > 0 {
            x = x.checked_mul(base.checked_pow(e as u32)?)?;
        }
    }
    (x <= u64::MAX as u128).then_some(x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::ToString;

    #[test]
    fn canonical() {
        assert_eq!(ExactScale::rational(1000, 1), ExactScale::pow10(3));
        assert_eq!(ExactScale::rational(1024, 1), ExactScale::pow2(10));
        assert_eq!(ExactScale::rational(1, 65535).parts(), (1, 13107, -1, 1, 0));
        assert_eq!(ExactScale::rational(360, 65536).parts(), (9, 1, 1, -14, 0));
        assert_eq!(
            ExactScale::new(1, 180, 0, 0, 1),
            ExactScale::new(2, 360, 0, 0, 1)
        );
        assert_eq!(
            ExactScale::pow10(-3).div(ExactScale::pow10(-6)),
            ExactScale::pow10(3)
        );
        assert_eq!(ExactScale::pow10(-3).as_pow10(), Some(-3));
        assert_eq!(ExactScale::pow2(10).as_pow10(), None);
    }

    #[test]
    fn apply() {
        let duty = ExactScale::rational(1, 65535);
        assert_eq!(duty.recip().apply_int(1), Some(65535));
        assert_eq!(duty.apply_int(65535), Some(1));
        assert_eq!(duty.apply_int(32767), None);
        assert_eq!(ExactScale::pow2(10).apply_int(3), Some(3072));
        assert_eq!(ExactScale::pow10(-3).apply_int(1500), None);
        assert_eq!(ExactScale::pow10(-3).apply_int(-3000), Some(-3));
        assert_eq!(ExactScale::new(1, 1, 0, 0, 1).apply_int(1), None);
        let deg = ExactScale::new(1, 180, 0, 0, 1);
        assert!((deg.apply_f64(90.0) - core::f64::consts::FRAC_PI_2).abs() < 1e-15);
        assert!((deg.to_f64() - core::f64::consts::PI / 180.0).abs() < 1e-18);
    }

    #[test]
    fn display() {
        assert_eq!(ExactScale::ONE.to_string(), "");
        assert_eq!(ExactScale::pow10(-4).to_string(), "·10⁻⁴");
        assert_eq!(ExactScale::pow2(10).to_string(), "·2¹⁰");
        assert_eq!(ExactScale::rational(1, 3).to_string(), "·1/3");
        assert_eq!(ExactScale::new(1, 180, 0, 0, 1).to_string(), "·π/180");
        assert_eq!(ExactScale::new(2, 1, 0, 0, 1).to_string(), "·2π");
        assert_eq!(ExactScale::rational(1, 65535).to_string(), "·1/65535");
        assert_eq!(ExactScale::rational(3, 1024).to_string(), "·3/1024");
        assert_eq!(
            ExactScale::new(1, 1, -100, 3, 0).to_string(),
            "·1·10⁻¹⁰⁰·2³"
        );
    }

    #[cfg(feature = "shrink_wrap")]
    #[test]
    fn wire() {
        let mut buf = [0u8; 32];
        for s in [
            ExactScale::ONE,
            ExactScale::rational(1, 65535),
            ExactScale::new(1, 180, 0, 0, 1),
        ] {
            let bytes = s.to_ww_bytes(&mut buf).unwrap().to_vec();
            assert_eq!(ExactScale::from_ww_bytes(&bytes).unwrap(), s);
        }
        // not canonical: 2/1 is 2¹
        let bad = ExactScale {
            num: 2,
            den: 1,
            pow10: 0,
            pow2: 0,
            pi: 0,
        };
        let bytes = bad.to_ww_bytes(&mut buf).unwrap().to_vec();
        assert_eq!(
            ExactScale::from_ww_bytes(&bytes),
            Err(shrink_wrap::Error::SubtypeOutOfRange)
        );
    }
}
