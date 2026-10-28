//! A plain Rust integer written as another integer type on the wire (DER-19): `#[wire(I4)] offset: i8`, `#[wire(UNib32)]
//! count: u64`. The field stays a plain type in Rust, the bytes are the wire type's.
//!
//! [WireAs] is what the derive calls: a value is range-checked when it is written ([Error::SubtypeOutOfRange], never
//! cut) and when it is read (a wider or variable-length wire type can hold more than the field). Wire types are
//! implemented for every plain integer of the same signedness: unsigned ones (`u8`..`u64`, [Nibble], `U1`..`U64`,
//! [UNib32], [UVlq32], [UVlq64]) for `u8`..`u128` and `usize`, signed ones (`i8`..`i64`, `I2`..`I64`) for `i8`..`i128`
//! and `isize`.
//!
//! A user type can be the wire type too (DER-23): `#[wire(Magnitude)] scale: ExactScale` keeps a one-byte handle in
//! Rust and writes the full value it stands for. Such a `W` implements [WireAs] for the field's type; in a
//! [Series](crate::series::Series) the field is an opaque run of `W`'s bytes ([WiredOpaque](crate::series::WiredOpaque)).

use crate::{Error, Nibble, UNib32, UVlq32, UVlq64};

/// A wire type that a plain Rust value `T` is written as, with `#[wire(Self)]` on a field of type `T` (DER-19).
pub trait WireAs<T>: Sized {
    /// The wire value of `value`, [Error::SubtypeOutOfRange] when it doesn't fit.
    fn from_plain(value: T) -> Result<Self, Error>;

    /// The plain value of what was read, [Error::SubtypeOutOfRange] when it doesn't fit in `T`.
    fn into_plain(self) -> Result<T, Error>;
}

/// A plain unsigned integer that unsigned wire types take (`u8`..`u128`, `usize`), through `u64`.
pub trait PlainUnsigned: Copy + sealed::Sealed {
    /// `None` when the value takes more than 64 bits.
    fn to_u64(self) -> Option<u64>;
    /// `None` when `value` doesn't fit.
    fn from_u64(value: u64) -> Option<Self>;
}

/// A plain signed integer that signed wire types take (`i8`..`i128`, `isize`), through `i64`.
pub trait PlainSigned: Copy + sealed::Sealed {
    /// `None` when the value takes more than 64 bits.
    fn to_i64(self) -> Option<i64>;
    /// `None` when `value` doesn't fit.
    fn from_i64(value: i64) -> Option<Self>;
}

mod sealed {
    pub trait Sealed {}
}

macro_rules! plain {
    ($trait_:ident, $to:ident, $from:ident, $carrier:ty: $($ty:ty),*) => {
        $(
            impl sealed::Sealed for $ty {}

            impl $trait_ for $ty {
                #[inline(always)]
                fn $to(self) -> Option<$carrier> {
                    <$carrier>::try_from(self).ok()
                }

                #[inline(always)]
                fn $from(value: $carrier) -> Option<Self> {
                    <$ty>::try_from(value).ok()
                }
            }
        )*
    };
}

plain!(PlainUnsigned, to_u64, from_u64, u64: u8, u16, u32, u64, u128, usize);
plain!(PlainSigned, to_i64, from_i64, i64: i8, i16, i32, i64, i128, isize);

/// `$wire` from and to its backing integer `$base`, `new` checking the range of `$base` values.
macro_rules! wire_as {
    ($plain:ident, $to:ident, $from:ident: $($wire:ty = $base:ty, |$v:ident| $new:expr, |$w:ident| $value:expr);* $(;)?) => {
        $(
            impl<T: $plain> WireAs<T> for $wire {
                #[inline(always)]
                fn from_plain(value: T) -> Result<Self, Error> {
                    value
                        .$to()
                        .and_then(|v| <$base>::try_from(v).ok())
                        .and_then(|$v: $base| $new)
                        .ok_or(Error::SubtypeOutOfRange)
                }

                #[inline(always)]
                fn into_plain(self) -> Result<T, Error> {
                    let $w = self;
                    T::$from($value as _).ok_or(Error::SubtypeOutOfRange)
                }
            }
        )*
    };
}

wire_as!(PlainUnsigned, to_u64, from_u64:
    u8 = u8, |v| Some(v), |w| w;
    u16 = u16, |v| Some(v), |w| w;
    u32 = u32, |v| Some(v), |w| w;
    u64 = u64, |v| Some(v), |w| w;
    Nibble = u8, |v| Nibble::new(v), |w| w.value();
    UNib32 = u32, |v| Some(UNib32(v)), |w| w.0;
    UVlq32 = u32, |v| Some(UVlq32(v)), |w| w.0;
    UVlq64 = u64, |v| Some(UVlq64(v)), |w| w.0;
);

wire_as!(PlainSigned, to_i64, from_i64:
    i8 = i8, |v| Some(v), |w| w;
    i16 = i16, |v| Some(v), |w| w;
    i32 = i32, |v| Some(v), |w| w;
    i64 = i64, |v| Some(v), |w| w;
);

macro_rules! un_wire_as {
    ($plain:ident, $to:ident, $from:ident, $prefix:ident $base_prefix:ident: $($bits:literal / $base_bits:literal),*) => {
        paste::paste! {
            wire_as!($plain, $to, $from: $(
                crate::un::[<$prefix $bits>] = [<$base_prefix $base_bits>],
                    |v| crate::un::[<$prefix $bits>]::new(v), |w| w.value()
            );*);
        }
    };
}

un_wire_as!(PlainUnsigned, to_u64, from_u64, U u:
    1/8, 2/8, 3/8, 4/8, 5/8, 6/8, 7/8, 8/8, 9/16, 10/16, 11/16, 12/16, 13/16, 14/16, 15/16, 16/16, 17/32, 18/32, 19/32,
    20/32, 21/32, 22/32, 23/32, 24/32, 25/32, 26/32, 27/32, 28/32, 29/32, 30/32, 31/32, 32/32, 33/64, 34/64, 35/64,
    36/64, 37/64, 38/64, 39/64, 40/64, 41/64, 42/64, 43/64, 44/64, 45/64, 46/64, 47/64, 48/64, 49/64, 50/64, 51/64,
    52/64, 53/64, 54/64, 55/64, 56/64, 57/64, 58/64, 59/64, 60/64, 61/64, 62/64, 63/64, 64/64);
un_wire_as!(PlainSigned, to_i64, from_i64, I i:
    2/8, 3/8, 4/8, 5/8, 6/8, 7/8, 8/8, 9/16, 10/16, 11/16, 12/16, 13/16, 14/16, 15/16, 16/16, 17/32, 18/32, 19/32,
    20/32, 21/32, 22/32, 23/32, 24/32, 25/32, 26/32, 27/32, 28/32, 29/32, 30/32, 31/32, 32/32, 33/64, 34/64, 35/64,
    36/64, 37/64, 38/64, 39/64, 40/64, 41/64, 42/64, 43/64, 44/64, 45/64, 46/64, 47/64, 48/64, 49/64, 50/64, 51/64,
    52/64, 53/64, 54/64, 55/64, 56/64, 57/64, 58/64, 59/64, 60/64, 61/64, 62/64, 63/64, 64/64);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::un::{I4, U4, U12, U64};

    #[test]
    fn unsigned_in_range_and_out() {
        assert_eq!(<U4 as WireAs<u8>>::from_plain(15).unwrap().value(), 15);
        assert_eq!(
            <U4 as WireAs<u8>>::from_plain(16),
            Err(Error::SubtypeOutOfRange)
        );
        assert_eq!(
            <U4 as WireAs<u64>>::from_plain(u64::MAX),
            Err(Error::SubtypeOutOfRange)
        );
        assert_eq!(
            <U12 as WireAs<u16>>::from_plain(4095).unwrap().value(),
            4095
        );
        assert_eq!(
            <U12 as WireAs<u16>>::from_plain(4096),
            Err(Error::SubtypeOutOfRange)
        );
        assert_eq!(
            <UNib32 as WireAs<u64>>::from_plain(u32::MAX as u64)
                .unwrap()
                .0,
            u32::MAX
        );
        assert_eq!(
            <UNib32 as WireAs<u64>>::from_plain(1 << 32),
            Err(Error::SubtypeOutOfRange)
        );
        assert_eq!(
            <u16 as WireAs<u128>>::from_plain(u128::MAX),
            Err(Error::SubtypeOutOfRange)
        );
        assert_eq!(
            <U64 as WireAs<u64>>::from_plain(u64::MAX).unwrap().value(),
            u64::MAX
        );
        assert_eq!(
            <Nibble as WireAs<usize>>::from_plain(16),
            Err(Error::SubtypeOutOfRange)
        );
    }

    #[test]
    fn signed_in_range_and_out() {
        assert_eq!(<I4 as WireAs<i8>>::from_plain(-8).unwrap().value(), -8);
        assert_eq!(<I4 as WireAs<i8>>::from_plain(7).unwrap().value(), 7);
        assert_eq!(
            <I4 as WireAs<i8>>::from_plain(8),
            Err(Error::SubtypeOutOfRange)
        );
        assert_eq!(
            <I4 as WireAs<i8>>::from_plain(-9),
            Err(Error::SubtypeOutOfRange)
        );
        assert_eq!(
            <i16 as WireAs<i64>>::from_plain(i16::MIN as i64 - 1),
            Err(Error::SubtypeOutOfRange)
        );
        assert_eq!(
            <i8 as WireAs<i128>>::from_plain(i128::MIN),
            Err(Error::SubtypeOutOfRange)
        );
    }

    #[test]
    fn read_back_checks_the_plain_range() {
        assert_eq!(<UNib32 as WireAs<u8>>::into_plain(UNib32(255)), Ok(255u8));
        assert_eq!(
            <UNib32 as WireAs<u8>>::into_plain(UNib32(256)),
            Err(Error::SubtypeOutOfRange)
        );
        assert_eq!(
            <u64 as WireAs<u32>>::into_plain(1 << 32),
            Err(Error::SubtypeOutOfRange)
        );
        assert_eq!(
            <i32 as WireAs<i8>>::into_plain(-129),
            Err(Error::SubtypeOutOfRange)
        );
        assert_eq!(
            <I4 as WireAs<i64>>::into_plain(I4::new(-3).unwrap()),
            Ok(-3i64)
        );
        assert_eq!(
            <U4 as WireAs<u128>>::into_plain(U4::new(9).unwrap()),
            Ok(9u128)
        );
    }
}
