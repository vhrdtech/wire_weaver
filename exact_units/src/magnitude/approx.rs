//! The value of a [Magnitude] that isn't a fraction of integers (π, ln 2, ln 10, a half exponent) as a fixed-point
//! factor, worked out in `const` code: what a rounding conversion multiplies by (STD-63).
//!
//! The magnitude is evaluated in a 128-bit mantissa (every step rounded to nearest, a relative error of a few
//! 2⁻¹²⁸ in all), then rounded to a 64-bit mantissa `m` with a power of two: the factor is `m · 2^shift` with
//! `2⁶³ <= m < 2⁶⁴`, within 2⁻⁶⁴ of the magnitude relative. Every consumer that rounds by the same `m` and `shift`
//! gets the same result bit for bit; [Magnitude::fixed_point] is that pair.

use super::{Magnitude, Work};

/// `m · 2^e` with `2¹²⁷ <= m < 2¹²⁸`.
#[derive(Copy, Clone)]
struct Wide {
    m: u128,
    e: i32,
}

const LOW: u128 = u64::MAX as u128;
const TOP: u128 = 1 << 127;

/// The 256-bit product `a · b` as `(high, low)` halves.
const fn mul_256(a: u128, b: u128) -> (u128, u128) {
    let (a1, a0) = (a >> 64, a & LOW);
    let (b1, b0) = (b >> 64, b & LOW);
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let mid = (p00 >> 64) + (p01 & LOW) + (p10 & LOW);
    let lo = (p00 & LOW) | (mid << 64);
    let hi = p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64);
    (hi, lo)
}

impl Wide {
    const ONE: Wide = Wide { m: TOP, e: -127 };
    /// π, ln 2 and ln 10 rounded to 128 bits.
    const PI: Wide = Wide {
        m: 0xc90fdaa22168c234c4c6628b80dc1cd1,
        e: -126,
    };
    const LN2: Wide = Wide {
        m: 0xb17217f7d1cf79abc9e3b39803f2f6af,
        e: -128,
    };
    const LN10: Wide = Wide {
        m: 0x935d8dddaaa8ac16ea56d62b82d30a29,
        e: -126,
    };

    const fn int(x: u32) -> Wide {
        let z = (x as u128).leading_zeros();
        Wide {
            m: (x as u128) << z,
            e: -(z as i32),
        }
    }

    /// `m` rounded to nearest at bit `drop` (`m` has `128 + drop` significant bits in `hi`, `lo`).
    const fn round_top(hi: u128, lo: u128, e: i32) -> Wide {
        let (mut m, mut e) = if hi & TOP != 0 {
            (hi, e + 128)
        } else {
            ((hi << 1) | (lo >> 127), e + 127)
        };
        let rest = if hi & TOP != 0 { lo } else { lo << 1 };
        if rest & TOP != 0 {
            m = m.wrapping_add(1);
            if m == 0 {
                m = TOP;
                e += 1;
            }
        }
        Wide { m, e }
    }

    /// `m`, plus one when `up`, carried into the exponent.
    const fn bump(m: u128, up: bool, e: i32) -> Wide {
        if !up {
            return Wide { m, e };
        }
        match m.checked_add(1) {
            Some(m) => Wide { m, e },
            None => Wide { m: TOP, e: e + 1 },
        }
    }

    const fn mul(self, rhs: Wide) -> Wide {
        let (hi, lo) = mul_256(self.m, rhs.m);
        Wide::round_top(hi, lo, self.e + rhs.e)
    }

    /// `self / rhs` by long division, rounded to nearest.
    const fn div(self, rhs: Wide) -> Wide {
        // quotient bits of (self.m · 2¹²⁸) / rhs.m: 128 or 129 of them
        let d = rhs.m;
        let (mut r, mut q_hi, mut q_lo) = (0u128, 0u128, 0u128);
        let mut i = 0;
        while i < 256 {
            let bit = if i < 128 {
                (self.m >> (127 - i)) & 1
            } else {
                0
            };
            let carry = r >> 127;
            r = (r << 1) | bit;
            let one = carry == 1 || r >= d;
            if one {
                r = r.wrapping_sub(d);
            }
            q_hi = (q_hi << 1) | (q_lo >> 127);
            q_lo = (q_lo << 1) | one as u128;
            i += 1;
        }
        // the quotient is q_hi · 2¹²⁸ + q_lo, in (2¹²⁷, 2¹²⁹): one bit more than the mantissa, or none
        let e = self.e - rhs.e - 128;
        let (m, up, e) = if q_hi != 0 {
            ((q_hi << 127) | (q_lo >> 1), q_lo & 1 == 1, e + 1)
        } else {
            (q_lo, r >> 127 == 1 || (r << 1) >= d, e)
        };
        Wide::bump(m, up, e)
    }

    /// `self + rhs`, both positive.
    const fn add(self, rhs: Wide) -> Wide {
        let (a, b) = if self.e >= rhs.e {
            (self, rhs)
        } else {
            (rhs, self)
        };
        let shift = (a.e - b.e) as u32;
        let bm = if shift >= 128 { 0 } else { b.m >> shift };
        let (s, carry) = a.m.overflowing_add(bm);
        if carry {
            let mut m = (s >> 1) | TOP;
            let mut e = a.e + 1;
            if s & 1 == 1 {
                m = m.wrapping_add(1);
                if m == 0 {
                    m = TOP;
                    e += 1;
                }
            }
            Wide { m, e }
        } else {
            Wide { m: s, e: a.e }
        }
    }

    const fn powi(self, n: i32) -> Wide {
        let mut acc = Wide::ONE;
        let mut base = self;
        let mut k = n.unsigned_abs();
        while k > 0 {
            if k & 1 == 1 {
                acc = acc.mul(base);
            }
            base = base.mul(base);
            k >>= 1;
        }
        if n < 0 { Wide::ONE.div(acc) } else { acc }
    }

    /// The square root by Newton's method from a power of two within a factor of 2.
    const fn sqrt(self) -> Wide {
        let exp = self.e + 127; // self is in [2^exp, 2^(exp + 1))
        let mut y = Wide {
            m: TOP,
            e: exp.div_euclid(2) - 127,
        };
        let mut i = 0;
        while i < 10 {
            let mut s = y.add(self.div(y));
            s.e -= 1;
            y = s;
            i += 1;
        }
        y
    }
}

impl Magnitude {
    /// The factor as `(m, shift)`: `m · 2^shift` with `2⁶³ <= m < 2⁶⁴`, within 2⁻⁶⁴ of the magnitude relative. What
    /// a rounding conversion multiplies by when the magnitude isn't a fraction of integers (π, ln 2, ln 10, a half
    /// exponent): the product `x · m` is exact in 128 bits for any `x` of up to 64 bits, so the result differs from
    /// the correctly rounded `x · magnitude` only where that lies within `|x| · 2⁻⁶⁴` of a rounding step, and then by
    /// one step.
    pub const fn fixed_point(&self) -> (u64, i32) {
        let w = Work::of(self);
        // the doubled exponents give the square of the magnitude
        let mut v = Wide::int(5).powi(w.pow10);
        v.e += w.pow10 + w.pow2;
        v = v.mul(Wide::PI.powi(w.irr[0]));
        v = v.mul(Wide::LN2.powi(w.irr[1]));
        v = v.mul(Wide::LN10.powi(w.irr[2]));
        let mut i = 0;
        while i < w.len {
            v = v.mul(Wide::int(w.primes[i]).powi(w.exps[i]));
            i += 1;
        }
        let r = v.sqrt();
        let mut m = (r.m >> 64) + ((r.m >> 63) & 1);
        let mut e = r.e + 64;
        if m >> 64 != 0 {
            m >>= 1;
            e += 1;
        }
        (m as u64, e)
    }
}
