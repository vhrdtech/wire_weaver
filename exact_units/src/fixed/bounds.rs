//! Bounds of a [Fixed](super::Fixed): [Unbounded] by default, or a range checked on construction and on every read.

use super::storage::Storage;
use crate::num::pow10_f64;
#[cfg(feature = "shrink_wrap")]
use shrink_wrap::{Nibble, UNib32, UVlq32, UVlq64};

/// The range a value of `T` must be in, both ends inclusive, `None` for no limit: a zero-size marker, never sent. A
/// bound that doesn't fit `T` is a compile error where the type is used. Bytes never change because of a bound.
///
/// The markers of this crate are [Unbounded], [Range] (integers up to `i64`), [RangeU64] (the top of `u64`) and
/// [RangeDec] (floats). An API's own marker works in Rust, but introspection sees only its name, while dynamic clients
/// read the limits of these three from their parameters.
pub trait Bounds<T: Storage>: Copy + Default + 'static {
    const MIN: Option<T>;
    const MAX: Option<T>;

    /// `MIN <= x <= MAX`; `false` for `NaN` when there is a bound.
    #[inline(always)]
    // `!(x >= min)` and not `x < min`: `NaN` compares false both ways and has to be refused
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn contains(x: T) -> bool {
        if let Some(min) = Self::MIN
            && !(x >= min)
        {
            return false;
        }
        if let Some(max) = Self::MAX
            && !(x <= max)
        {
            return false;
        }
        true
    }
}

/// A value outside its [Bounds], or one that doesn't fit the storage.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct OutOfBounds;

impl core::fmt::Display for OutOfBounds {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("value out of bounds")
    }
}

impl core::error::Error for OutOfBounds {}

/// No bounds: the whole range of the storage. The default of [Fixed](super::Fixed)'s `B`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Unbounded;

/// `MIN ..= MAX` for an integer storage up to 64 bits (`u8`..`u64`, `i8`..`i64`, `UNib32`, `UVlq32`, `UVlq64`,
/// `Nibble`). For a `u64` above `i64::MAX`, [RangeU64].
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Range<const MIN: i64, const MAX: i64>;

/// `MIN ..= MAX` for a `u64` or `UVlq64` whose bounds go above `i64::MAX`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct RangeU64<const MIN: u64, const MAX: u64>;

/// `MIN · 10^EXP ..= MAX · 10^EXP` for a float storage: `-0.5 ..= 2.5` is `RangeDec<-5, 25, -1>`. Only floats can't be
/// const parameters, so the bounds are decimal.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct RangeDec<const MIN: i64, const MAX: i64, const EXP: i8>;

impl<T: Storage> Bounds<T> for Unbounded {
    const MIN: Option<T> = None;
    const MAX: Option<T> = None;
}

/// `x` as an integer of `min..=max`, a compile error where it is used otherwise.
const fn fits(x: i128, min: i128, max: i128) -> i128 {
    if x < min || x > max {
        panic!("a bound doesn't fit the storage");
    }
    x
}

const fn ordered<const MIN: i64, const MAX: i64>() {
    if MIN > MAX {
        panic!("a range's MIN is above its MAX");
    }
}

macro_rules! range {
    ($($ty:ident $min:expr, $max:expr, $x:ident => $wrap:expr;)*) => {$(
        impl<const MIN: i64, const MAX: i64> Bounds<$ty> for Range<MIN, MAX> {
            const MIN: Option<$ty> = {
                ordered::<MIN, MAX>();
                let $x = fits(MIN as i128, $min as i128, $max as i128);
                Some($wrap)
            };
            const MAX: Option<$ty> = {
                let $x = fits(MAX as i128, $min as i128, $max as i128);
                Some($wrap)
            };
        }
    )*};
}

range! {
    u8 u8::MIN, u8::MAX, x => x as u8;
    u16 u16::MIN, u16::MAX, x => x as u16;
    u32 u32::MIN, u32::MAX, x => x as u32;
    u64 u64::MIN, u64::MAX, x => x as u64;
    i8 i8::MIN, i8::MAX, x => x as i8;
    i16 i16::MIN, i16::MAX, x => x as i16;
    i32 i32::MIN, i32::MAX, x => x as i32;
    i64 i64::MIN, i64::MAX, x => x as i64;
}

#[cfg(feature = "shrink_wrap")]
range! {
    UNib32 u32::MIN, u32::MAX, x => UNib32(x as u32);
    UVlq32 u32::MIN, u32::MAX, x => UVlq32(x as u32);
    UVlq64 u64::MIN, u64::MAX, x => UVlq64(x as u64);
    Nibble 0, 15, x => Nibble::new_masked(x as u8);
}

impl<const MIN: u64, const MAX: u64> Bounds<u64> for RangeU64<MIN, MAX> {
    const MIN: Option<u64> = {
        if MIN > MAX {
            panic!("a range's MIN is above its MAX");
        }
        Some(MIN)
    };
    const MAX: Option<u64> = Some(MAX);
}

#[cfg(feature = "shrink_wrap")]
impl<const MIN: u64, const MAX: u64> Bounds<UVlq64> for RangeU64<MIN, MAX> {
    const MIN: Option<UVlq64> = match <Self as Bounds<u64>>::MIN {
        Some(x) => Some(UVlq64(x)),
        None => None,
    };
    const MAX: Option<UVlq64> = Some(UVlq64(MAX));
}

impl<const MIN: i64, const MAX: i64, const EXP: i8> Bounds<f64> for RangeDec<MIN, MAX, EXP> {
    const MIN: Option<f64> = {
        ordered::<MIN, MAX>();
        Some(MIN as f64 * pow10_f64(EXP))
    };
    const MAX: Option<f64> = Some(MAX as f64 * pow10_f64(EXP));
}

impl<const MIN: i64, const MAX: i64, const EXP: i8> Bounds<f32> for RangeDec<MIN, MAX, EXP> {
    const MIN: Option<f32> = match <Self as Bounds<f64>>::MIN {
        Some(x) => Some(x as f32),
        None => None,
    };
    const MAX: Option<f32> = Some(MAX as f64 as f32 * pow10_f64(EXP) as f32);
}

/// `bounded(value)`: a `const fn` constructor per integer storage, a compile error for a value outside `B` in `const`
/// code (a panic at run time).
macro_rules! bounded {
    ($($ty:ident $x:ident => $get:expr;)*) => {$(
        impl<U, P, B: Bounds<$ty>> super::Fixed<U, P, $ty, B> {
            /// `value` in units of `P`: a compile error in `const` code when it is outside `B`, a panic at run time
            /// ([try_new](Self::try_new) doesn't).
            #[inline(always)]
            pub const fn bounded(value: $ty) -> Self {
                let v = {
                    let $x = value;
                    $get
                };
                if let Some($x) = B::MIN
                    && v < $get
                {
                    panic!("value below its bound");
                }
                if let Some($x) = B::MAX
                    && v > $get
                {
                    panic!("value above its bound");
                }
                Self::new_unchecked(value)
            }
        }
    )*};
}

bounded! {
    u8 x => x;
    u16 x => x;
    u32 x => x;
    u64 x => x;
    i8 x => x;
    i16 x => x;
    i32 x => x;
    i64 x => x;
}

#[cfg(feature = "shrink_wrap")]
bounded! {
    UNib32 x => x.0;
    UVlq32 x => x.0;
    UVlq64 x => x.0;
    Nibble x => x.value();
}
