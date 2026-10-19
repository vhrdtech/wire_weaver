//! [Integer]: the value of any integer number type as sign and magnitude, for exact arithmetic across number types
//! (comparisons, sums, powers of ten) in no wider type than the enabled number types need. With types up to 32 bits
//! everything is `u32` arithmetic, no 64- or 128-bit code is linked.

use crate::{NumericBaseType, NumericValue};
use core::cmp::Ordering;
use core::fmt;
use core::ops::Neg;

/// `$x: Integer` as the signed `$ty` (unsigned `$uty` of the same width) if it fits.
macro_rules! to_signed {
    ($x:expr, $ty:ty, $uty:ty) => {{
        let x: Integer = $x;
        if x.negative {
            if x.magnitude <= <$ty>::MIN.unsigned_abs() as Magnitude {
                Some((x.magnitude as $uty).wrapping_neg() as $ty)
            } else {
                None
            }
        } else {
            <$ty>::try_from(x.magnitude).ok()
        }
    }};
}

/// `$x: Integer` as the unsigned `$ty` if it fits.
macro_rules! to_unsigned {
    ($x:expr, $ty:ty) => {{
        let x: Integer = $x;
        if x.negative {
            None
        } else {
            <$ty>::try_from(x.magnitude).ok()
        }
    }};
}

/// The magnitude of an [Integer]: the narrowest unsigned type that holds every enabled integer number type, `u32`
/// with types up to 32 bits, `u64` with `u64`, `i64` or `uvlq64`, `u128` with `u128` or `i128`.
#[cfg(any(feature = "u128", feature = "i128"))]
pub type Magnitude = u128;
/// The magnitude of an [Integer]: the narrowest unsigned type that holds every enabled integer number type, `u32`
/// with types up to 32 bits, `u64` with `u64`, `i64` or `uvlq64`, `u128` with `u128` or `i128`.
#[cfg(all(
    not(any(feature = "u128", feature = "i128")),
    any(feature = "u64", feature = "i64", feature = "uvlq64")
))]
pub type Magnitude = u64;
/// The magnitude of an [Integer]: the narrowest unsigned type that holds every enabled integer number type, `u32`
/// with types up to 32 bits, `u64` with `u64`, `i64` or `uvlq64`, `u128` with `u128` or `i128`.
#[cfg(not(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
)))]
pub type Magnitude = u32;

/// An exact integer as sign and [Magnitude]: what every integer variant of [NumericValue] converts to
/// ([NumericValue::to_integer]) and back ([NumericBaseType::from_integer]), so values of different number types and
/// powers of ten are compared and combined exactly without widening to `i128`.
///
/// ```
/// use ww_numeric::{Integer, NumericValue};
///
/// let a = NumericValue::U32(3300).to_integer().unwrap();
/// let b = NumericValue::I32(3).to_integer().unwrap();
/// // 3300 * 10^-3 == 3 * 10^0 is false, 3300 * 10^-3 > 3
/// assert!(a.cmp_pow10(-3, b, 0).is_gt());
/// assert_eq!(a.checked_scale_pow10(-3), Some(Integer::from(3u8)));
/// assert_eq!(a.exact_scale_pow10(-3), None);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Integer {
    /// Never set for zero.
    negative: bool,
    magnitude: Magnitude,
}

impl Integer {
    pub const ZERO: Integer = Integer {
        negative: false,
        magnitude: 0,
    };

    /// `-magnitude` if `negative`, else `magnitude`.
    pub const fn new(negative: bool, magnitude: Magnitude) -> Self {
        Integer {
            negative: negative && magnitude != 0,
            magnitude,
        }
    }

    /// Below zero.
    pub const fn is_negative(&self) -> bool {
        self.negative
    }

    /// The absolute value.
    pub const fn magnitude(&self) -> Magnitude {
        self.magnitude
    }

    /// `self + rhs`, `None` when the magnitude doesn't fit.
    pub fn checked_add(self, rhs: Integer) -> Option<Integer> {
        Some(if self.negative == rhs.negative {
            Integer::new(self.negative, self.magnitude.checked_add(rhs.magnitude)?)
        } else if self.magnitude >= rhs.magnitude {
            Integer::new(self.negative, self.magnitude - rhs.magnitude)
        } else {
            Integer::new(rhs.negative, rhs.magnitude - self.magnitude)
        })
    }

    /// `self - rhs`, `None` when the magnitude doesn't fit.
    pub fn checked_sub(self, rhs: Integer) -> Option<Integer> {
        self.checked_add(-rhs)
    }

    /// `self * rhs`, `None` when the magnitude doesn't fit.
    pub fn checked_mul(self, rhs: Integer) -> Option<Integer> {
        let (a, b) = (self.magnitude, rhs.magnitude);
        // with a `u32` magnitude not `checked_mul`, whose overflow check is a 64-bit multiply on Cortex-M0
        #[cfg(not(any(
            feature = "u64",
            feature = "i64",
            feature = "uvlq64",
            feature = "u128",
            feature = "i128"
        )))]
        let magnitude = if b != 0 && a > Magnitude::MAX / b {
            return None;
        } else {
            a * b
        };
        #[cfg(any(
            feature = "u64",
            feature = "i64",
            feature = "uvlq64",
            feature = "u128",
            feature = "i128"
        ))]
        let magnitude = a.checked_mul(b)?;
        Some(Integer::new(self.negative != rhs.negative, magnitude))
    }

    /// `self * 10^exp`: `None` when it doesn't fit, truncated toward zero for a negative `exp`.
    pub fn checked_scale_pow10(self, exp: i8) -> Option<Integer> {
        let magnitude = if exp >= 0 {
            mul_pow10(self.magnitude, exp as u16)?
        } else {
            div_pow10(self.magnitude, exp.unsigned_abs() as u16).0
        };
        Some(Integer::new(self.negative, magnitude))
    }

    /// `self * 10^exp` if that is an integer that fits: `None` for 3300 and `exp` -3.
    pub fn exact_scale_pow10(self, exp: i8) -> Option<Integer> {
        if exp >= 0 {
            return self.checked_scale_pow10(exp);
        }
        match div_pow10(self.magnitude, exp.unsigned_abs() as u16) {
            (magnitude, true) => Some(Integer::new(self.negative, magnitude)),
            (_, false) => None,
        }
    }

    /// `self * 10^exp` modulo `2^32`, as two's complement for negative values: what a 32-bit counter of
    /// `10^exp` units shows (`u32` arithmetic only). Truncated toward zero for a negative `exp`.
    // `Magnitude` is `u32` itself with types up to 32 bits
    #[allow(clippy::unnecessary_cast)]
    pub fn to_u32_wrapping_pow10(self, exp: i8) -> u32 {
        let magnitude = if exp >= 0 {
            // (m mod 2^32) * 10^k mod 2^32 is m * 10^k mod 2^32
            let mut m = self.magnitude as u32;
            for _ in 0..exp {
                m = m.wrapping_mul(10);
            }
            m
        } else {
            div_pow10(self.magnitude, exp.unsigned_abs() as u16).0 as u32
        };
        if self.negative {
            magnitude.wrapping_neg()
        } else {
            magnitude
        }
    }

    /// Compares `self * 10^exp` with `other * 10^other_exp` exactly, at any distance between the powers: the one
    /// with the bigger power is scaled in [Magnitude] (in `u32` when both fit one), and an overflow means it is the
    /// bigger one in magnitude.
    pub fn cmp_pow10(self, exp: i8, other: Integer, other_exp: i8) -> Ordering {
        if exp == other_exp {
            return self.cmp(&other);
        }
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (negative, _) => {
                let o = cmp_pow10(self.magnitude, exp, other.magnitude, other_exp);
                if negative { o.reverse() } else { o }
            }
        }
    }

    /// The value as `i32` if it fits.
    pub fn to_i32(self) -> Option<i32> {
        to_signed!(self, i32, u32)
    }

    /// The value as `u32` if it fits.
    pub fn to_u32(self) -> Option<u32> {
        to_unsigned!(self, u32)
    }
}

impl Neg for Integer {
    type Output = Integer;

    fn neg(self) -> Integer {
        Integer::new(!self.negative, self.magnitude)
    }
}

impl Ord for Integer {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.negative, other.negative) {
            (false, false) => self.magnitude.cmp(&other.magnitude),
            (true, true) => other.magnitude.cmp(&self.magnitude),
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
        }
    }
}

impl PartialOrd for Integer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Integer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.negative {
            write!(f, "-{}", self.magnitude)
        } else {
            write!(f, "{}", self.magnitude)
        }
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for Integer {
    fn format(&self, f: defmt::Formatter<'_>) {
        let sign = if self.negative { "-" } else { "" };
        defmt::write!(f, "{=str}{}", sign, self.magnitude)
    }
}

macro_rules! from_unsigned {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for Integer {
                fn from(x: $ty) -> Self {
                    Integer { negative: false, magnitude: x as Magnitude }
                }
            }
        )*
    };
}

macro_rules! from_signed {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for Integer {
                fn from(x: $ty) -> Self {
                    Integer { negative: x < 0, magnitude: x.unsigned_abs() as Magnitude }
                }
            }
        )*
    };
}

from_unsigned!(u8, u16, u32);
from_signed!(i8, i16, i32);
#[cfg(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
))]
from_unsigned!(u64);
#[cfg(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
))]
from_signed!(i64);
#[cfg(any(feature = "u128", feature = "i128"))]
from_unsigned!(u128);
#[cfg(any(feature = "u128", feature = "i128"))]
from_signed!(i128);

/// Powers of ten on magnitudes of one unsigned type.
macro_rules! pow10_ops {
    ($mul10:ident, $mul:ident, $div:ident, $cmp:ident, $ty:ty) => {
        /// `x * 10`, `None` on overflow. Not `checked_mul`: on Cortex-M0 its overflow check is a 64-bit multiply
        /// (`__aeabi_lmul`), a compare with a constant is enough here.
        #[allow(dead_code)]
        #[inline(always)]
        fn $mul10(x: $ty) -> Option<$ty> {
            if x > <$ty>::MAX / 10 {
                None
            } else {
                Some(x * 10)
            }
        }

        /// `x * 10^k`, `None` on overflow.
        #[allow(dead_code)]
        fn $mul(x: $ty, k: u16) -> Option<$ty> {
            let mut x = x;
            if x != 0 {
                // overflows within a few steps for any x > 0
                for _ in 0..k {
                    x = $mul10(x)?;
                }
            }
            Some(x)
        }

        /// `x / 10^k` truncated, and whether nothing was cut off.
        #[allow(dead_code)]
        fn $div(x: $ty, k: u16) -> ($ty, bool) {
            let mut p: $ty = 1;
            for _ in 0..k {
                match $mul10(p) {
                    Some(q) => p = q,
                    // 10^k is beyond the type, so beyond x
                    None => return (0, x == 0),
                }
            }
            (x / p, x % p == 0)
        }

        /// `a * 10^a_exp` against `b * 10^b_exp`: the one with the bigger power scaled down to the smaller power,
        /// an overflow decides.
        #[allow(dead_code)]
        fn $cmp(a: $ty, a_exp: i8, b: $ty, b_exp: i8) -> Ordering {
            let k = (a_exp as i16 - b_exp as i16).unsigned_abs();
            if a_exp >= b_exp {
                match $mul(a, k) {
                    Some(a) => a.cmp(&b),
                    None => Ordering::Greater,
                }
            } else {
                match $mul(b, k) {
                    Some(b) => a.cmp(&b),
                    None => Ordering::Less,
                }
            }
        }
    };
}

pow10_ops!(mul10_u32, mul_pow10_u32, div_pow10_u32, cmp_pow10_u32, u32);

#[cfg(not(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
)))]
use {cmp_pow10_u32 as cmp_pow10, div_pow10_u32 as div_pow10, mul_pow10_u32 as mul_pow10};

#[cfg(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
))]
pow10_ops!(
    mul10_wide,
    mul_pow10_wide,
    div_pow10_wide,
    cmp_pow10_wide,
    Magnitude
);

// Magnitudes wider than u32 are rare: values that fit a u32 take the u32 path, the wide one only when needed.

#[cfg(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
))]
fn mul_pow10(x: Magnitude, k: u16) -> Option<Magnitude> {
    if let Ok(x) = u32::try_from(x)
        && let Some(r) = mul_pow10_u32(x, k)
    {
        return Some(r.into());
    }
    mul_pow10_wide(x, k)
}

#[cfg(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
))]
fn div_pow10(x: Magnitude, k: u16) -> (Magnitude, bool) {
    if let Ok(x) = u32::try_from(x) {
        let (r, exact) = div_pow10_u32(x, k);
        return (r.into(), exact);
    }
    div_pow10_wide(x, k)
}

#[cfg(any(
    feature = "u64",
    feature = "i64",
    feature = "uvlq64",
    feature = "u128",
    feature = "i128"
))]
fn cmp_pow10(a: Magnitude, a_exp: i8, b: Magnitude, b_exp: i8) -> Ordering {
    if let (Ok(a), Ok(b)) = (u32::try_from(a), u32::try_from(b)) {
        return cmp_pow10_u32(a, a_exp, b, b_exp);
    }
    cmp_pow10_wide(a, a_exp, b, b_exp)
}

impl NumericValue {
    /// The exact value of an integer variant as sign and magnitude, `None` for floats and the not yet supported
    /// `UN` / `IN`.
    #[inline]
    pub fn to_integer(&self) -> Option<Integer> {
        Some(match *self {
            #[cfg(feature = "nibble")]
            NumericValue::Nibble(x) => x.value().into(),
            NumericValue::U8(x) => x.into(),
            #[cfg(feature = "u16")]
            NumericValue::U16(x) => x.into(),
            #[cfg(feature = "u32")]
            NumericValue::U32(x) => x.into(),
            #[cfg(feature = "unib32")]
            NumericValue::UNib32(x) => x.0.into(),
            #[cfg(feature = "u64")]
            NumericValue::U64(x) => x.into(),
            #[cfg(feature = "i32")]
            NumericValue::I32(x) => x.into(),
            #[cfg(feature = "u128")]
            NumericValue::U128(x) => x.into(),
            #[cfg(feature = "i8")]
            NumericValue::I8(x) => x.into(),
            #[cfg(feature = "i16")]
            NumericValue::I16(x) => x.into(),
            #[cfg(feature = "i64")]
            NumericValue::I64(x) => x.into(),
            #[cfg(feature = "i128")]
            NumericValue::I128(x) => x.into(),
            #[cfg(feature = "uvlq32")]
            NumericValue::UVlq32(x) => x.0.into(),
            #[cfg(feature = "uvlq64")]
            NumericValue::UVlq64(x) => x.0.into(),
            #[allow(unreachable_patterns)]
            _ => return None,
        })
    }

    /// `self + rhs` in the same number type, wrapping around at its width like a counter (`U32` modulo `2^32`,
    /// signed types as two's complement, `Nibble` modulo 16, `UVlq32` and `UNib32` modulo `2^32`): `None` for floats
    /// and `UN` / `IN`.
    pub fn wrapping_add(&self, rhs: Integer) -> Option<NumericValue> {
        // the magnitude truncated to the type's width is the same modulo 2^bits
        macro_rules! wrap {
            ($x:expr, $uty:ty) => {
                if rhs.negative {
                    $x.wrapping_sub(rhs.magnitude as $uty)
                } else {
                    $x.wrapping_add(rhs.magnitude as $uty)
                }
            };
        }
        #[allow(unused_macros)]
        macro_rules! wrap_signed {
            ($x:expr, $uty:ty) => {
                if rhs.negative {
                    $x.wrapping_sub_unsigned(rhs.magnitude as $uty)
                } else {
                    $x.wrapping_add_unsigned(rhs.magnitude as $uty)
                }
            };
        }
        Some(match *self {
            #[cfg(feature = "nibble")]
            NumericValue::Nibble(x) => {
                NumericValue::Nibble(shrink_wrap::Nibble::new_masked(wrap!(x.value(), u8)))
            }
            NumericValue::U8(x) => NumericValue::U8(wrap!(x, u8)),
            #[cfg(feature = "u16")]
            NumericValue::U16(x) => NumericValue::U16(wrap!(x, u16)),
            #[cfg(feature = "u32")]
            NumericValue::U32(x) => NumericValue::U32(wrap!(x, u32)),
            #[cfg(feature = "unib32")]
            NumericValue::UNib32(x) => NumericValue::UNib32(shrink_wrap::UNib32(wrap!(x.0, u32))),
            #[cfg(feature = "u64")]
            NumericValue::U64(x) => NumericValue::U64(wrap!(x, u64)),
            #[cfg(feature = "i32")]
            NumericValue::I32(x) => NumericValue::I32(wrap_signed!(x, u32)),
            #[cfg(feature = "u128")]
            NumericValue::U128(x) => NumericValue::U128(wrap!(x, u128)),
            #[cfg(feature = "i8")]
            NumericValue::I8(x) => NumericValue::I8(wrap_signed!(x, u8)),
            #[cfg(feature = "i16")]
            NumericValue::I16(x) => NumericValue::I16(wrap_signed!(x, u16)),
            #[cfg(feature = "i64")]
            NumericValue::I64(x) => NumericValue::I64(wrap_signed!(x, u64)),
            #[cfg(feature = "i128")]
            NumericValue::I128(x) => NumericValue::I128(wrap_signed!(x, u128)),
            #[cfg(feature = "uvlq32")]
            NumericValue::UVlq32(x) => NumericValue::UVlq32(crate::UVlq32(wrap!(x.0, u32))),
            #[cfg(feature = "uvlq64")]
            NumericValue::UVlq64(x) => NumericValue::UVlq64(crate::UVlq64(wrap!(x.0, u64))),
            #[allow(unreachable_patterns)]
            _ => return None,
        })
    }
}

impl NumericBaseType {
    /// `x` in this number type, exactly: `None` when it doesn't fit, for a type that is not enabled, and for
    /// `F16`, `UB`, `IB`, `UN`, `IN`. Floats take `x` rounded to their precision.
    pub fn from_integer(&self, x: Integer) -> Option<NumericValue> {
        Some(match self {
            #[cfg(feature = "nibble")]
            NumericBaseType::Nibble => {
                NumericValue::Nibble(shrink_wrap::Nibble::new(to_unsigned!(x, u8)?)?)
            }
            NumericBaseType::U8 => NumericValue::U8(to_unsigned!(x, u8)?),
            #[cfg(feature = "u16")]
            NumericBaseType::U16 => NumericValue::U16(to_unsigned!(x, u16)?),
            #[cfg(feature = "u32")]
            NumericBaseType::U32 => NumericValue::U32(to_unsigned!(x, u32)?),
            #[cfg(feature = "unib32")]
            NumericBaseType::UNib32 => {
                NumericValue::UNib32(shrink_wrap::UNib32(to_unsigned!(x, u32)?))
            }
            #[cfg(feature = "u64")]
            NumericBaseType::U64 => NumericValue::U64(to_unsigned!(x, u64)?),
            #[cfg(feature = "i32")]
            NumericBaseType::I32 => NumericValue::I32(to_signed!(x, i32, u32)?),
            #[cfg(feature = "f32")]
            NumericBaseType::F32 => {
                let m = x.magnitude as f32;
                NumericValue::F32(if x.negative { -m } else { m })
            }
            #[cfg(feature = "u128")]
            NumericBaseType::U128 => NumericValue::U128(to_unsigned!(x, u128)?),
            #[cfg(feature = "i8")]
            NumericBaseType::I8 => NumericValue::I8(to_signed!(x, i8, u8)?),
            #[cfg(feature = "i16")]
            NumericBaseType::I16 => NumericValue::I16(to_signed!(x, i16, u16)?),
            #[cfg(feature = "i64")]
            NumericBaseType::I64 => NumericValue::I64(to_signed!(x, i64, u64)?),
            #[cfg(feature = "i128")]
            NumericBaseType::I128 => NumericValue::I128(to_signed!(x, i128, u128)?),
            #[cfg(feature = "f64")]
            NumericBaseType::F64 => {
                let m = x.magnitude as f64;
                NumericValue::F64(if x.negative { -m } else { m })
            }
            #[cfg(feature = "uvlq32")]
            NumericBaseType::UVlq32 => NumericValue::UVlq32(crate::UVlq32(to_unsigned!(x, u32)?)),
            #[cfg(feature = "uvlq64")]
            NumericBaseType::UVlq64 => NumericValue::UVlq64(crate::UVlq64(to_unsigned!(x, u64)?)),
            _ => return None,
        })
    }
}
