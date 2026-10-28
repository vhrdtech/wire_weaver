//! Named rounding and overflow policies for conversions that aren't exact (STD-63): [Rounding], [Overflow], and
//! [requantize], the same conversion with the storage known only at run time (what a dynamic client does).

use super::storage::{Repr, Storage};
use crate::Magnitude;

/// How a conversion rounds a value that isn't a whole number of the target's steps. Every mode is exact: the
/// conversion works out the true quotient and rounds once (no float in between for integer storages).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Rounding {
    /// Toward zero, what integer division does: 7/2 is 3, -7/2 is -3.
    Truncate,
    /// Toward −∞: -7/2 is -4.
    Floor,
    /// Toward +∞: 7/2 is 4.
    Ceil,
    /// To the nearest, a tie to the even neighbour (IEEE 754's default, Python's `round`): 2.5 is 2, 3.5 is 4.
    NearestEven,
    /// To the nearest, a tie away from zero (C's `round` and `lround`): 2.5 is 3, -2.5 is -3.
    NearestAway,
}

/// What a conversion or an operation does with a result the storage can't hold.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Overflow {
    /// No value (`None`): the default, where neither of the others is the meaning.
    Checked,
    /// The nearest end of the storage: Q formats, levels, set points.
    Saturate,
    /// Modulo 2^bits (two's complement for signed storages): binary angles, free-running counters.
    Wrap,
}

macro_rules! round_div {
    ($($name:ident $t:ty;)*) => {$(
        /// `p / d` rounded by `r`, for `d > 0`.
        #[inline(always)]
        #[allow(unused_comparisons)]
        pub(crate) const fn $name(p: $t, d: $t, r: Rounding) -> $t {
            // floor quotient and a remainder in 0..d: an arithmetic shift and a mask where d is a power of 2 (LLVM
            // doesn't always see that in `div_euclid` at opt-level 3)
            let (q, rem) = if d & (d - 1) == 0 {
                (p >> d.trailing_zeros(), p & (d - 1))
            } else {
                (p.div_euclid(d), p.rem_euclid(d))
            };
            // with `rem != 0`, `d >= 2` and q < p / d, so `q + 1` doesn't overflow
            let up = match r {
                Rounding::Truncate => p < 0 && rem != 0,
                Rounding::Floor => false,
                Rounding::Ceil => rem != 0,
                Rounding::NearestEven | Rounding::NearestAway => {
                    // against half a step, a constant once d is: a tie only for an even d
                    let half = d / 2;
                    if rem != half || d % 2 != 0 {
                        rem > half
                    } else if matches!(r, Rounding::NearestAway) {
                        p >= 0
                    } else {
                        q % 2 != 0
                    }
                }
            };
            q + up as $t
        }
    )*};
}

round_div! {
    div_u8 u8; div_u16 u16; div_u32 u32; div_u64 u64;
    div_i8 i8; div_i16 i16; div_i32 i32; div_i64 i64; div_i128 i128;
}

/// `x` (a float) rounded to a whole number by `r`; `NaN` and infinities as they are. Without `std`, so by hand.
#[inline]
pub(crate) fn round_f64(x: f64, r: Rounding) -> f64 {
    // from 2⁵² on every f64 is whole
    const WHOLE: f64 = 4503599627370496.0;
    if !(x < WHOLE && x > -WHOLE) {
        return x;
    }
    let t = x as i64 as f64;
    let frac = x - t;
    if frac == 0.0 {
        return t;
    }
    let neg = x < 0.0;
    let away = if neg { t - 1.0 } else { t + 1.0 };
    match r {
        Rounding::Truncate => t,
        Rounding::Floor => {
            if neg {
                away
            } else {
                t
            }
        }
        Rounding::Ceil => {
            if neg {
                t
            } else {
                away
            }
        }
        Rounding::NearestEven | Rounding::NearestAway => {
            let a = if neg { -frac } else { frac };
            if a > 0.5 {
                away
            } else if a < 0.5 {
                t
            } else if r == Rounding::NearestAway || (t as i64) % 2 != 0 {
                away
            } else {
                t
            }
        }
    }
}

/// `x` into `T` by `o`.
#[inline(always)]
pub(crate) fn settle<T: Storage>(x: i128, o: Overflow) -> Option<T> {
    match o {
        Overflow::Checked => T::from_int(x),
        Overflow::Saturate => Some(T::from_int_wrapping(x.clamp(T::MIN_INT, T::MAX_INT))),
        Overflow::Wrap => Some(T::from_int_wrapping(x)),
    }
}

/// `x` into `T` (of up to 32 bits) by `o`, in 64 bits.
#[inline(always)]
pub(crate) fn settle_i64<T: Storage>(x: i64, o: Overflow) -> Option<T> {
    match o {
        Overflow::Checked => T::from_i64(x),
        Overflow::Saturate => Some(T::from_int_wrapping(
            x.clamp(T::MIN_INT as i64, T::MAX_INT as i64) as i128,
        )),
        Overflow::Wrap => Some(T::from_int_wrapping(x as i128)),
    }
}

/// A whole float into the integer storage `T` by `o`. Saturating takes `NaN` to 0 like Rust's `as`; wrapping takes
/// `NaN` and infinities to 0.
#[inline]
pub(crate) fn settle_f64<T: Storage>(y: f64, o: Overflow) -> Option<T> {
    match o {
        Overflow::Checked => T::from_float(y),
        Overflow::Saturate => Some(T::from_int_wrapping(saturate_f64(
            y,
            T::MIN_INT,
            T::MAX_INT,
        ))),
        Overflow::Wrap => Some(T::from_int_wrapping(wrap_f64(y))),
    }
}

/// A whole float clamped to `min..=max`, `NaN` as 0.
fn saturate_f64(y: f64, min: i128, max: i128) -> i128 {
    if y.is_nan() {
        0
    } else if y >= max as f64 {
        // `max as f64` may round up (u64::MAX is 2⁶⁴ as f64): every whole float below it is in range
        max
    } else if y <= min as f64 {
        min
    } else {
        y as i128
    }
}

/// A whole float as an `i128` that has its low 64 bits: from 2¹²⁷ on a whole f64 is a multiple of 2⁷⁵, 0 modulo
/// 2⁶⁴, as `NaN` and infinities are taken to be.
fn wrap_f64(y: f64) -> i128 {
    const LIMIT: f64 = 170141183460469231731687303715884105728.0; // 2¹²⁷
    if y < LIMIT && y > -LIMIT {
        y as i128
    } else {
        0
    }
}

/// `x · m · 2^shift` rounded by `r`, for `|x| < 2⁶⁴` (any storage of up to 64 bits): its sign, the magnitude modulo
/// 2¹²⁸, and whether the magnitude is below 2¹²⁸. The product `|x| · m` is exact in 128 bits.
#[inline]
pub(crate) const fn mul_fixed_point(x: i128, m: u64, shift: i32, r: Rounding) -> Scaled {
    let neg = x < 0;
    let p = x.unsigned_abs() * m as u128;
    if shift >= 0 {
        let s = shift as u32;
        return Scaled {
            neg,
            low: if s < 128 { p << s } else { 0 },
            fits: p == 0 || p.leading_zeros() >= s,
        };
    }
    let k = shift.unsigned_abs();
    let (q, rem) = if k >= 128 {
        (0, p)
    } else {
        (p >> k, p & ((1 << k) - 1))
    };
    // rem against half a step, 2^(k - 1); from k = 129 on half a step is above any rem
    let (above, tie) = if k > 128 {
        (false, false)
    } else {
        let half = 1u128 << (k - 1);
        (rem > half, rem == half)
    };
    let up = match r {
        Rounding::Truncate => false,
        Rounding::Floor => neg && rem != 0,
        Rounding::Ceil => !neg && rem != 0,
        Rounding::NearestAway => above || tie,
        Rounding::NearestEven => above || (tie && q & 1 == 1),
    };
    Scaled {
        neg,
        low: q + up as u128,
        fits: true,
    }
}

/// [mul_fixed_point] for storages of up to 32 bits on both sides: the result's low 64 bits are all that wrapping
/// into the target needs, so it settles in 64 bits, not 128.
#[inline(always)]
pub(crate) const fn mul_fixed_point_narrow(x: i64, m: u64, shift: i32, r: Rounding) -> Scaled64 {
    if shift <= -34 && shift >= -96 {
        // |x| < 2³², so |x| · m < 2⁹⁶ and the product shifted by k - 1 >= 33 is below 2⁶³: twice the quotient plus
        // the bit for half a step; the bits below it only say whether there is more than that
        let neg = x < 0;
        let p = x.unsigned_abs() as u128 * m as u128;
        let k = shift.unsigned_abs();
        let t = (p >> (k - 1)) as u64;
        let q = t >> 1;
        let half = t & 1 == 1;
        let below = p & ((1 << (k - 1)) - 1) != 0;
        let up = match r {
            Rounding::Truncate => false,
            Rounding::Floor => neg && (half || below),
            Rounding::Ceil => !neg && (half || below),
            Rounding::NearestAway => half,
            Rounding::NearestEven => half && (below || q & 1 == 1),
        };
        // below 2⁶² + 1
        let y = (q + up as u64) as i64;
        return Scaled64 {
            value: if neg { -y } else { y },
            exact: true,
            neg,
        };
    }
    let s = mul_fixed_point(x as i128, m, shift, r);
    let exact = s.exact();
    Scaled64 {
        value: match exact {
            Some(y) if y >= i64::MIN as i128 && y <= i64::MAX as i128 => y as i64,
            _ => s.wrapped() as i64,
        },
        exact: matches!(exact, Some(y) if y >= i64::MIN as i128 && y <= i64::MAX as i128),
        neg: s.neg,
    }
}

/// [Scaled] within 64 bits: the value modulo 2⁶⁴, whether it is the value, and its sign.
#[derive(Copy, Clone)]
pub(crate) struct Scaled64 {
    value: i64,
    exact: bool,
    neg: bool,
}

impl Scaled64 {
    /// Into `T` (of up to 32 bits) by `o`.
    #[inline(always)]
    pub(crate) fn settle<T: Storage>(self, o: Overflow) -> Option<T> {
        match (o, self.exact) {
            (Overflow::Wrap, _) => Some(T::from_int_wrapping(self.value as i128)),
            (_, true) => settle_i64(self.value, o),
            (Overflow::Checked, false) => None,
            (Overflow::Saturate, false) => Some(T::from_int_wrapping(if self.neg {
                T::MIN_INT
            } else {
                T::MAX_INT
            })),
        }
    }
}

/// A signed result of up to 128 bits of magnitude, or beyond (then its low 128 bits).
#[derive(Copy, Clone)]
pub(crate) struct Scaled {
    neg: bool,
    low: u128,
    fits: bool,
}

impl Scaled {
    pub(crate) const fn of(x: i128) -> Scaled {
        Scaled {
            neg: x < 0,
            low: x.unsigned_abs(),
            fits: true,
        }
    }

    /// `self >= min`.
    pub(crate) const fn at_least(self, min: i128) -> bool {
        match self.exact() {
            Some(x) => x >= min,
            None => !self.neg,
        }
    }

    /// `self <= max`.
    pub(crate) const fn at_most(self, max: i128) -> bool {
        match self.exact() {
            Some(x) => x <= max,
            None => self.neg,
        }
    }
    /// The value as an `i128`, if it is one.
    const fn exact(self) -> Option<i128> {
        if !self.fits {
            return None;
        }
        if self.neg {
            if self.low <= 1 << 127 {
                Some((self.low as i128).wrapping_neg())
            } else {
                None
            }
        } else if self.low <= i128::MAX as u128 {
            Some(self.low as i128)
        } else {
            None
        }
    }

    /// The value modulo 2¹²⁸, as an `i128` of the same low bits.
    const fn wrapped(self) -> i128 {
        if self.neg {
            self.low.wrapping_neg() as i128
        } else {
            self.low as i128
        }
    }

    #[inline]
    pub(crate) fn settle<T: Storage>(self, o: Overflow) -> Option<T> {
        match (o, self.exact()) {
            (Overflow::Wrap, _) => Some(T::from_int_wrapping(self.wrapped())),
            (_, Some(x)) => settle(x, o),
            (Overflow::Checked, None) => None,
            (Overflow::Saturate, None) => Some(T::from_int_wrapping(if self.neg {
                T::MIN_INT
            } else {
                T::MAX_INT
            })),
        }
    }
}

/// A number type known at run time, the target of [requantize].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum StorageKind {
    /// An integer of `bits` (1 to 64), two's complement when `signed`: `u16` is `{ bits: 16, signed: false }`,
    /// `Nibble` 4 bits unsigned, `UNib32` and `UVlq32` 32.
    Int {
        bits: u32,
        signed: bool,
    },
    F32,
    F64,
}

impl StorageKind {
    /// The kind of a storage type.
    pub const fn of<T: Storage>() -> StorageKind {
        if T::IS_FLOAT {
            if T::BITS == 32 {
                StorageKind::F32
            } else {
                StorageKind::F64
            }
        } else {
            StorageKind::Int {
                bits: T::BITS,
                signed: T::MIN_INT < 0,
            }
        }
    }

    const fn range(bits: u32, signed: bool) -> (i128, i128) {
        if signed {
            (-(1 << (bits - 1)), (1 << (bits - 1)) - 1)
        } else {
            (0, (1 << bits) - 1)
        }
    }

    /// `x` in this integer kind by `o`: wrapping keeps the low `bits` (sign-extended when signed).
    fn settle_int(
        bits: u32,
        signed: bool,
        x: Option<i128>,
        low: i128,
        neg: bool,
        o: Overflow,
    ) -> Option<i128> {
        let (min, max) = Self::range(bits, signed);
        match (o, x) {
            (Overflow::Wrap, _) => {
                let v = (low as u128) & ((1u128 << bits) - 1);
                Some(if signed && v >> (bits - 1) == 1 {
                    v as i128 - (1i128 << bits)
                } else {
                    v as i128
                })
            }
            (Overflow::Checked, Some(x)) => (min..=max).contains(&x).then_some(x),
            (Overflow::Saturate, Some(x)) => Some(x.clamp(min, max)),
            (Overflow::Checked, None) => None,
            (Overflow::Saturate, None) => Some(if neg { min } else { max }),
        }
    }
}

/// `x · factor` in the storage `to`, rounded by `r` and settled by `o`: the conversion of
/// [Fixed::requantize](super::Fixed::requantize) with the scales' quotient and the target storage known only at run
/// time, as a dynamic client has them from the model; [Requantizer] for many values. Gives the same number as the
/// typed conversion, bit for bit (the conformance corpus checks both). `None` where `o` is [Overflow::Checked] and
/// the result doesn't fit, where `x · num` of a fraction factor doesn't fit 128 bits (the typed conversion doesn't
/// compile there), and for an integer `x` of more than 64 bits with a factor that isn't a fraction of integers.
pub fn requantize(
    x: Repr,
    factor: &Magnitude,
    to: StorageKind,
    r: Rounding,
    o: Overflow,
) -> Option<Repr> {
    Requantizer::new(factor, to, r, o).apply(x)
}

/// [requantize] with the factor worked out once, for a series of values.
#[derive(Copy, Clone, Debug)]
pub struct Requantizer {
    factor: Magnitude,
    exact: Exact,
    to: StorageKind,
    r: Rounding,
    o: Overflow,
}

#[derive(Copy, Clone, Debug)]
enum Exact {
    /// `num` (`None` past `i128`) and `den` (`i128::MAX` past it: rounds the same, every product is below it).
    Ratio {
        num: Option<i128>,
        num_low: i128,
        den: i128,
    },
    FixedPoint {
        m: u64,
        shift: i32,
    },
}

impl Requantizer {
    pub fn new(factor: &Magnitude, to: StorageKind, r: Rounding, o: Overflow) -> Self {
        let exact = match factor.to_ratio() {
            Some((num, den)) => Exact::Ratio {
                num: i128::try_from(num).ok(),
                num_low: num as i128,
                den: i128::try_from(den).unwrap_or(i128::MAX),
            },
            None => {
                let (m, shift) = factor.fixed_point();
                Exact::FixedPoint { m, shift }
            }
        };
        Requantizer {
            factor: *factor,
            exact,
            to,
            r,
            o,
        }
    }

    /// `x` converted, as [requantize] does.
    pub fn apply(&self, x: Repr) -> Option<Repr> {
        let (r, o, factor) = (self.r, self.o, &self.factor);
        let (bits, signed) = match (self.to, x) {
            (StorageKind::F64, Repr::Int(i)) => {
                return Some(Repr::Float(factor.apply_f64(i as f64)));
            }
            (StorageKind::F64, Repr::Float(f)) => return Some(Repr::Float(factor.apply_f64(f))),
            (StorageKind::F32, Repr::Int(i)) => {
                return Some(Repr::Float(factor.apply_f64(i as f64) as f32 as f64));
            }
            (StorageKind::F32, Repr::Float(f)) => {
                return Some(Repr::Float(factor.apply_f64(f) as f32 as f64));
            }
            (StorageKind::Int { bits, signed }, _) => (bits, signed),
        };
        let x = match x {
            Repr::Float(f) => {
                let y = round_f64(factor.apply_f64(f), r);
                let (min, max) = StorageKind::range(bits, signed);
                return match o {
                    Overflow::Checked => {
                        let i = y as i128;
                        (y == i as f64 && (min..=max).contains(&i)).then_some(Repr::Int(i))
                    }
                    Overflow::Saturate => Some(Repr::Int(saturate_f64(y, min, max))),
                    Overflow::Wrap => {
                        StorageKind::settle_int(bits, signed, None, wrap_f64(y), false, o)
                            .map(Repr::Int)
                    }
                };
            }
            Repr::Int(0) => return Some(Repr::Int(0)),
            Repr::Int(x) => x,
        };
        let neg = x < 0;
        let (exact, low) = match self.exact {
            Exact::Ratio { num, num_low, den } => match num.and_then(|n| x.checked_mul(n)) {
                Some(p) => {
                    let y = div_i128(p, den, r);
                    (Some(y), y)
                }
                // a whole factor: the result is beyond 128 bits, and modulo 2¹²⁸ still right
                None if den == 1 => (None, x.wrapping_mul(num_low)),
                // the product doesn't fit 128 bits, where the typed conversion doesn't compile
                None => return None,
            },
            Exact::FixedPoint { m, shift } => {
                if x.unsigned_abs() >> 64 != 0 {
                    return None;
                }
                let s = mul_fixed_point(x, m, shift, r);
                (s.exact(), s.wrapped())
            }
        };
        StorageKind::settle_int(bits, signed, exact, low, neg, o).map(Repr::Int)
    }
}
