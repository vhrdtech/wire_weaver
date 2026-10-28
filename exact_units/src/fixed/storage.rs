//! [Storage]: the number types a [Fixed](super::Fixed) can be stored as.

use core::fmt;
#[cfg(feature = "shrink_wrap")]
use shrink_wrap::{Nibble, UNib32, UVlq32, UVlq64};

/// A number as an exact integer or a float, for conversions between storages and prefixes.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Repr {
    Int(i128),
    Float(f64),
}

/// A number type a [Fixed](super::Fixed) is stored as: `u8`..`u64`, `i8`..`i64`, `f32`, `f64`, `UNib32`, `UVlq32`,
/// `UVlq64`, `Nibble`. Arithmetic is that of the number inside (`UNib32` wraps like `u32`, `Nibble` at 16); for floats,
/// `checked_*` is `None` for a result that isn't finite, `wrapping_*` and `saturating_*` are the plain operation.
pub trait Storage: Copy + PartialOrd + 'static {
    /// The type's name: `"u32"`, `"UNib32"`.
    const NAME: &'static str;
    const IS_FLOAT: bool;
    const ZERO: Self;

    /// The value as an exact integer or a float.
    fn to_repr(self) -> Repr;
    /// `x` if it fits; a float storage takes the nearest float.
    fn from_int(x: i128) -> Option<Self>;
    /// `x` if it is a whole number that fits (an integer storage), or `x` rounded to the storage (a float one).
    fn from_float(x: f64) -> Option<Self>;
    /// The number as stored: integers exactly, `f32` as `f32`.
    fn fmt_number(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;

    fn checked_add(self, rhs: Self) -> Option<Self>;
    fn checked_sub(self, rhs: Self) -> Option<Self>;
    fn checked_mul(self, rhs: Self) -> Option<Self>;
    fn checked_div(self, rhs: Self) -> Option<Self>;
    fn wrapping_add(self, rhs: Self) -> Self;
    fn wrapping_sub(self, rhs: Self) -> Self;
    fn wrapping_mul(self, rhs: Self) -> Self;
    fn saturating_add(self, rhs: Self) -> Self;
    fn saturating_sub(self, rhs: Self) -> Self;
    fn saturating_mul(self, rhs: Self) -> Self;
    /// `+` as the type's own operator: panics on overflow in debug builds.
    fn add(self, rhs: Self) -> Self;
    fn sub(self, rhs: Self) -> Self;
    fn mul(self, rhs: Self) -> Self;
    fn div(self, rhs: Self) -> Self;
}

/// An unsigned integer storage of up to 64 bits: what a [core::time::Duration] converts from.
pub trait UnsignedStorage: Storage {
    fn to_u64(self) -> u64;
    fn from_u64(x: u64) -> Option<Self>;
}

/// `f64` to an integer type if it is whole and in range.
fn whole(x: f64, min: i128, max: i128) -> Option<i128> {
    // the range check is in f64 (min/max of up to 64 bits are close enough, the cast below checks again)
    if x.is_nan() || x < min as f64 || x > max as f64 {
        return None;
    }
    let i = x as i128;
    if i as f64 != x || i < min || i > max {
        return None;
    }
    Some(i)
}

macro_rules! int {
    ($($ty:ident)*) => {$(
        impl Storage for $ty {
            const NAME: &'static str = stringify!($ty);
            const IS_FLOAT: bool = false;
            const ZERO: Self = 0;
            #[inline(always)]
            fn to_repr(self) -> Repr {
                Repr::Int(self as i128)
            }
            #[inline]
            fn from_int(x: i128) -> Option<Self> {
                $ty::try_from(x).ok()
            }
            #[inline]
            fn from_float(x: f64) -> Option<Self> {
                Some(whole(x, $ty::MIN as i128, $ty::MAX as i128)? as $ty)
            }
            fn fmt_number(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self, f)
            }
            #[inline(always)] fn checked_add(self, rhs: Self) -> Option<Self> { $ty::checked_add(self, rhs) }
            #[inline(always)] fn checked_sub(self, rhs: Self) -> Option<Self> { $ty::checked_sub(self, rhs) }
            #[inline(always)] fn checked_mul(self, rhs: Self) -> Option<Self> { $ty::checked_mul(self, rhs) }
            #[inline(always)] fn checked_div(self, rhs: Self) -> Option<Self> { $ty::checked_div(self, rhs) }
            #[inline(always)] fn wrapping_add(self, rhs: Self) -> Self { $ty::wrapping_add(self, rhs) }
            #[inline(always)] fn wrapping_sub(self, rhs: Self) -> Self { $ty::wrapping_sub(self, rhs) }
            #[inline(always)] fn wrapping_mul(self, rhs: Self) -> Self { $ty::wrapping_mul(self, rhs) }
            #[inline(always)] fn saturating_add(self, rhs: Self) -> Self { $ty::saturating_add(self, rhs) }
            #[inline(always)] fn saturating_sub(self, rhs: Self) -> Self { $ty::saturating_sub(self, rhs) }
            #[inline(always)] fn saturating_mul(self, rhs: Self) -> Self { $ty::saturating_mul(self, rhs) }
            #[inline(always)] fn add(self, rhs: Self) -> Self { self + rhs }
            #[inline(always)] fn sub(self, rhs: Self) -> Self { self - rhs }
            #[inline(always)] fn mul(self, rhs: Self) -> Self { self * rhs }
            #[inline(always)] fn div(self, rhs: Self) -> Self { self / rhs }
        }
    )*};
}

int!(u8 u16 u32 u64 i8 i16 i32 i64);

macro_rules! unsigned {
    ($($ty:ident)*) => {$(
        impl UnsignedStorage for $ty {
            #[inline(always)]
            fn to_u64(self) -> u64 {
                self as u64
            }
            #[inline(always)]
            fn from_u64(x: u64) -> Option<Self> {
                $ty::try_from(x).ok()
            }
        }
    )*};
}

unsigned!(u8 u16 u32 u64);

/// A newtype around an unsigned integer (`UNib32(u32)`): the integer's arithmetic. shrink_wrap's integers, with its
/// feature.
#[cfg(feature = "shrink_wrap")]
macro_rules! newtype {
    ($($ty:ident($inner:ident))*) => {$(
        impl Storage for $ty {
            const NAME: &'static str = stringify!($ty);
            const IS_FLOAT: bool = false;
            const ZERO: Self = $ty(0);
            #[inline(always)]
            fn to_repr(self) -> Repr {
                Repr::Int(self.0 as i128)
            }
            #[inline]
            fn from_int(x: i128) -> Option<Self> {
                $inner::from_int(x).map($ty)
            }
            #[inline]
            fn from_float(x: f64) -> Option<Self> {
                $inner::from_float(x).map($ty)
            }
            fn fmt_number(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
            #[inline(always)] fn checked_add(self, rhs: Self) -> Option<Self> { self.0.checked_add(rhs.0).map($ty) }
            #[inline(always)] fn checked_sub(self, rhs: Self) -> Option<Self> { self.0.checked_sub(rhs.0).map($ty) }
            #[inline(always)] fn checked_mul(self, rhs: Self) -> Option<Self> { self.0.checked_mul(rhs.0).map($ty) }
            #[inline(always)] fn checked_div(self, rhs: Self) -> Option<Self> { self.0.checked_div(rhs.0).map($ty) }
            #[inline(always)] fn wrapping_add(self, rhs: Self) -> Self { $ty(self.0.wrapping_add(rhs.0)) }
            #[inline(always)] fn wrapping_sub(self, rhs: Self) -> Self { $ty(self.0.wrapping_sub(rhs.0)) }
            #[inline(always)] fn wrapping_mul(self, rhs: Self) -> Self { $ty(self.0.wrapping_mul(rhs.0)) }
            #[inline(always)] fn saturating_add(self, rhs: Self) -> Self { $ty(self.0.saturating_add(rhs.0)) }
            #[inline(always)] fn saturating_sub(self, rhs: Self) -> Self { $ty(self.0.saturating_sub(rhs.0)) }
            #[inline(always)] fn saturating_mul(self, rhs: Self) -> Self { $ty(self.0.saturating_mul(rhs.0)) }
            #[inline(always)] fn add(self, rhs: Self) -> Self { $ty(self.0 + rhs.0) }
            #[inline(always)] fn sub(self, rhs: Self) -> Self { $ty(self.0 - rhs.0) }
            #[inline(always)] fn mul(self, rhs: Self) -> Self { $ty(self.0 * rhs.0) }
            #[inline(always)] fn div(self, rhs: Self) -> Self { $ty(self.0 / rhs.0) }
        }

        impl UnsignedStorage for $ty {
            #[inline(always)]
            fn to_u64(self) -> u64 {
                self.0 as u64
            }
            #[inline(always)]
            fn from_u64(x: u64) -> Option<Self> {
                $inner::try_from(x).ok().map($ty)
            }
        }
    )*};
}

#[cfg(feature = "shrink_wrap")]
newtype!(UNib32(u32) UVlq32(u32) UVlq64(u64));

/// 0..=15, wrapping at 16.
#[cfg(feature = "shrink_wrap")]
impl Storage for Nibble {
    const NAME: &'static str = "Nibble";
    const IS_FLOAT: bool = false;
    const ZERO: Self = Nibble::zero();
    #[inline(always)]
    fn to_repr(self) -> Repr {
        Repr::Int(self.value() as i128)
    }
    #[inline]
    fn from_int(x: i128) -> Option<Self> {
        Nibble::new(u8::try_from(x).ok()?)
    }
    #[inline]
    fn from_float(x: f64) -> Option<Self> {
        Nibble::new(u8::from_float(x)?)
    }
    fn fmt_number(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.value(), f)
    }
    #[inline(always)]
    fn checked_add(self, rhs: Self) -> Option<Self> {
        Nibble::new(self.value() + rhs.value())
    }
    #[inline(always)]
    fn checked_sub(self, rhs: Self) -> Option<Self> {
        Nibble::new(self.value().checked_sub(rhs.value())?)
    }
    #[inline(always)]
    fn checked_mul(self, rhs: Self) -> Option<Self> {
        Nibble::new(self.value() * rhs.value())
    }
    #[inline(always)]
    fn checked_div(self, rhs: Self) -> Option<Self> {
        Nibble::new(self.value().checked_div(rhs.value())?)
    }
    #[inline(always)]
    fn wrapping_add(self, rhs: Self) -> Self {
        Nibble::new_masked(self.value() + rhs.value())
    }
    #[inline(always)]
    fn wrapping_sub(self, rhs: Self) -> Self {
        Nibble::new_masked(self.value().wrapping_sub(rhs.value()))
    }
    #[inline(always)]
    fn wrapping_mul(self, rhs: Self) -> Self {
        Nibble::new_masked(self.value() * rhs.value())
    }
    #[inline(always)]
    fn saturating_add(self, rhs: Self) -> Self {
        Nibble::new(self.value() + rhs.value()).unwrap_or(Nibble::max())
    }
    #[inline(always)]
    fn saturating_sub(self, rhs: Self) -> Self {
        Nibble::new_masked(self.value().saturating_sub(rhs.value()))
    }
    #[inline(always)]
    fn saturating_mul(self, rhs: Self) -> Self {
        Nibble::new(self.value() * rhs.value()).unwrap_or(Nibble::max())
    }
    #[inline(always)]
    fn add(self, rhs: Self) -> Self {
        let x = self.value() + rhs.value();
        debug_assert!(x <= 15, "attempt to add with overflow");
        Nibble::new_masked(x)
    }
    #[inline(always)]
    fn sub(self, rhs: Self) -> Self {
        Nibble::new_masked(self.value() - rhs.value())
    }
    #[inline(always)]
    fn mul(self, rhs: Self) -> Self {
        let x = self.value() * rhs.value();
        debug_assert!(x <= 15, "attempt to multiply with overflow");
        Nibble::new_masked(x)
    }
    #[inline(always)]
    fn div(self, rhs: Self) -> Self {
        Nibble::new_masked(self.value() / rhs.value())
    }
}

#[cfg(feature = "shrink_wrap")]
impl UnsignedStorage for Nibble {
    #[inline(always)]
    fn to_u64(self) -> u64 {
        self.value() as u64
    }
    #[inline(always)]
    fn from_u64(x: u64) -> Option<Self> {
        Nibble::new(u8::try_from(x).ok()?)
    }
}

macro_rules! float {
    ($($ty:ident)*) => {$(
        impl Storage for $ty {
            const NAME: &'static str = stringify!($ty);
            const IS_FLOAT: bool = true;
            const ZERO: Self = 0.0;
            #[inline(always)]
            fn to_repr(self) -> Repr {
                Repr::Float(self as f64)
            }
            #[inline]
            fn from_int(x: i128) -> Option<Self> {
                Some(x as $ty)
            }
            #[inline]
            fn from_float(x: f64) -> Option<Self> {
                Some(x as $ty)
            }
            fn fmt_number(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self, f)
            }
            #[inline(always)] fn checked_add(self, rhs: Self) -> Option<Self> { Some(self + rhs).filter(|x| x.is_finite()) }
            #[inline(always)] fn checked_sub(self, rhs: Self) -> Option<Self> { Some(self - rhs).filter(|x| x.is_finite()) }
            #[inline(always)] fn checked_mul(self, rhs: Self) -> Option<Self> { Some(self * rhs).filter(|x| x.is_finite()) }
            #[inline(always)] fn checked_div(self, rhs: Self) -> Option<Self> { Some(self / rhs).filter(|x| x.is_finite()) }
            #[inline(always)] fn wrapping_add(self, rhs: Self) -> Self { self + rhs }
            #[inline(always)] fn wrapping_sub(self, rhs: Self) -> Self { self - rhs }
            #[inline(always)] fn wrapping_mul(self, rhs: Self) -> Self { self * rhs }
            #[inline(always)] fn saturating_add(self, rhs: Self) -> Self { self + rhs }
            #[inline(always)] fn saturating_sub(self, rhs: Self) -> Self { self - rhs }
            #[inline(always)] fn saturating_mul(self, rhs: Self) -> Self { self * rhs }
            #[inline(always)] fn add(self, rhs: Self) -> Self { self + rhs }
            #[inline(always)] fn sub(self, rhs: Self) -> Self { self - rhs }
            #[inline(always)] fn mul(self, rhs: Self) -> Self { self * rhs }
            #[inline(always)] fn div(self, rhs: Self) -> Self { self / rhs }
        }
    )*};
}

float!(f32 f64);
