//! Bounds of a [Quantity](crate::Quantity): the same markers as a [Fixed](crate::Fixed)'s, the range in the unit
//! itself (coherent units, whatever prefix a value has), checked when a value is built with
//! [try_new](crate::Quantity::try_new) and when it is read.

use crate::ops::cmp_limit;
use crate::{OutOfBounds, Prefix, Range, RangeDec, RangeU64, Unbounded};
use core::cmp::Ordering;
use ww_numeric::{Integer, Magnitude, NumericValue};

/// The range a [Quantity](crate::Quantity)'s value must be in, both ends inclusive and in the unit itself (`3.3 V`
/// whether a value is `3300 mV` or `3.3 V`), `None` for no limit: a zero-size marker, never sent, and in the API
/// model as the `Quantity`'s second argument, so dynamic clients read the limits.
///
/// [Unbounded] (the default), [Range] (integers), [RangeU64] and [RangeDec] (`MIN · 10^EXP ..= MAX · 10^EXP`):
/// `Quantity<units::Volt, RangeDec<0, 33, -1>>` is `0 V ..= 3.3 V`. The markers are those of [Fixed](crate::Fixed),
/// where the range is in units of the `Fixed`'s prefix: there it bounds the stored number, here the value.
pub trait QuantityBounds: Copy + Default + 'static {
    const MIN: Option<Limit>;
    const MAX: Option<Limit>;

    /// `MIN <= value · prefix <= MAX`; `false` for `NaN` and a value that can't be compared (`UN` / `IN`) when there
    /// is a bound. Nothing to check for [Unbounded]: the call is gone in a build.
    #[inline(always)]
    fn contains(prefix: Prefix, value: &NumericValue) -> bool {
        if let Some(min) = Self::MIN
            && !matches!(cmp_limit(prefix, value, &min), Some(Ordering::Greater | Ordering::Equal))
        {
            return false;
        }
        if let Some(max) = Self::MAX
            && !matches!(cmp_limit(prefix, value, &max), Some(Ordering::Less | Ordering::Equal))
        {
            return false;
        }
        true
    }

    /// [contains](Self::contains) as a `Result`.
    #[inline(always)]
    fn check(prefix: Prefix, value: &NumericValue) -> Result<(), OutOfBounds> {
        if Self::contains(prefix, value) {
            Ok(())
        } else {
            Err(OutOfBounds)
        }
    }
}

/// An end of a [QuantityBounds] range: `value · 10^exp` in the unit itself, exact.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Limit {
    pub value: Integer,
    pub exp: i8,
}

impl Limit {
    /// `x · 10^exp`, trailing zeros moved into `exp` while `x` doesn't fit the widest integer enabled: a compile
    /// error where the bound is used when it still doesn't (enable `ww_numeric`'s `u64` or `i64`).
    pub const fn new(x: i128, exp: i8) -> Limit {
        let negative = x < 0;
        let mut m = x.unsigned_abs();
        let mut exp = exp;
        while m > Magnitude::MAX as u128 {
            assert!(
                m.is_multiple_of(10) && exp < i8::MAX,
                "a Quantity bound wider than the integers enabled in ww_numeric (feature u64 or i64)"
            );
            m /= 10;
            exp += 1;
        }
        Limit {
            value: Integer::new(negative, m as Magnitude),
            exp,
        }
    }
}

const fn ordered(min: i128, max: i128) {
    assert!(min <= max, "a bound's MIN is above its MAX");
}

impl QuantityBounds for Unbounded {
    const MIN: Option<Limit> = None;
    const MAX: Option<Limit> = None;
}

impl<const MIN: i64, const MAX: i64> QuantityBounds for Range<MIN, MAX> {
    const MIN: Option<Limit> = {
        ordered(MIN as i128, MAX as i128);
        Some(Limit::new(MIN as i128, 0))
    };
    const MAX: Option<Limit> = Some(Limit::new(MAX as i128, 0));
}

impl<const MIN: u64, const MAX: u64> QuantityBounds for RangeU64<MIN, MAX> {
    const MIN: Option<Limit> = {
        ordered(MIN as i128, MAX as i128);
        Some(Limit::new(MIN as i128, 0))
    };
    const MAX: Option<Limit> = Some(Limit::new(MAX as i128, 0));
}

impl<const MIN: i64, const MAX: i64, const EXP: i8> QuantityBounds for RangeDec<MIN, MAX, EXP> {
    const MIN: Option<Limit> = {
        ordered(MIN as i128, MAX as i128);
        Some(Limit::new(MIN as i128, EXP))
    };
    const MAX: Option<Limit> = Some(Limit::new(MAX as i128, EXP));
}
