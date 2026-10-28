//! [Index], [IndexFrom1] and [Seq]: positions in a table of `Of` and sequence numbers, the point side of counted
//! things. Same bytes as the number they hold.
//!
//! A [Fixed]`<Of, One, T>` is an amount of `Of` (5 samples, 512 bytes), a vector. An index is a position, a point
//! with the table's start as its origin, and a sequence number a point on a circle. The algebra is
//! [Point](crate::Point)'s:
//!
//! - index + amount = index, index − amount = index, index − index = amount (a distance);
//! - index + index doesn't compile, nor does index · x;
//! - a sequence number moves forward and back by an amount, wrapping at the end of `T`; two of them compare in serial
//!   number order (RFC 1982), not as integers, and don't add.
//!
//! `Of` names what is counted: a counted marker ([Sample](crate::units::Sample), [Frame](crate::units::Frame), an
//! API's own one from [unit!](crate::unit)) or any other unit ([Byte](crate::units::Byte) for an offset). The amounts
//! are [Fixed]s of `Of` in steps of one ([One]), bounds are [Fixed]'s markers, and a [Seq] moves by [Wrapping]
//! arithmetic.

use crate::fixed::prefix::One;
use crate::{Bounds, Fixed, OutOfBounds, Unbounded, Unit, Unlimited, UnsignedStorage, Wrapping};
use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;
use core::ops::{Add, AddAssign, Sub, SubAssign};
#[cfg(feature = "shrink_wrap")]
use shrink_wrap::prelude::*;

/// A 0-based position in a table of `Of`, stored as `T` within the bounds `B`: `Index<Sample, u32>` is a sample's
/// place in a capture, `Index<Byte, u32>` an offset. On the wire exactly the `T`.
///
/// ```
/// use exact_units::{Fixed, Index, units::Sample};
///
/// let start = Index::<Sample, u32>::new(100);
/// let end = start + Fixed::new(50);
/// assert_eq!(end.get(), 150);
/// // two positions are apart by an amount of samples
/// assert_eq!(end - start, Fixed::new(50));
/// assert!(start < end);
/// ```
///
/// Bounds are those of [Fixed]: a table of 8 is `Range<0, 7>`, checked on construction and on every read from the
/// wire, and moving a bounded index is `checked_add` / `checked_sub`.
///
/// ```
/// use exact_units::{Fixed, Index, Range, units::Item};
///
/// type Slot = Index<Item, u8, Range<0, 7>>;
/// let last = Slot::try_new(7).unwrap();
/// assert!(Slot::try_new(8).is_err());
/// assert_eq!(last.checked_add(Fixed::new(1)), None);
/// assert_eq!(last.checked_sub(Fixed::new(7)), Some(Slot::try_new(0).unwrap()));
/// ```
///
/// Two indices don't add, an index doesn't scale, and indices of different tables don't mix:
///
/// ```compile_fail
/// use exact_units::{Index, units::Sample};
/// let _ = Index::<Sample, u32>::new(1) + Index::<Sample, u32>::new(2);
/// ```
///
/// ```compile_fail
/// use exact_units::{Index, units::Sample};
/// let _ = Index::<Sample, u32>::new(1) * 2;
/// ```
///
/// ```compile_fail
/// use exact_units::{Fixed, Index, units::{Frame, Sample}};
/// let _ = Index::<Sample, u32>::new(1) + Fixed::<Frame, exact_units::prefix::One, u32>::new(2);
/// ```
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(borrowed, owned(feature = "std"), transparent, validate = Self::validate)
)]
#[repr(transparent)]
pub struct Index<Of, T: UnsignedStorage, B: Bounds<T> = Unbounded> {
    index: T,
    _of: PhantomData<Of>,
    _bounds: PhantomData<B>,
}

impl<Of, T: UnsignedStorage> Index<Of, T> {
    /// Position `index`, 0 the first.
    #[inline(always)]
    pub const fn new(index: T) -> Self {
        Self::new_unchecked(index)
    }
}

impl<Of, T: UnsignedStorage> From<T> for Index<Of, T> {
    #[inline(always)]
    fn from(index: T) -> Self {
        Self::new(index)
    }
}

impl<Of, T: UnsignedStorage, B: Bounds<T>> Index<Of, T, B> {
    /// The position `offset` from the start of the table: `Index::at(Fixed::bounded(3))` for a `const` of a bounded
    /// index.
    #[inline(always)]
    pub const fn at(offset: Fixed<Of, One, T, B>) -> Self {
        Self::new_unchecked(offset.get())
    }

    #[inline(always)]
    const fn new_unchecked(index: T) -> Self {
        Index {
            index,
            _of: PhantomData,
            _bounds: PhantomData,
        }
    }

    /// Position `index`, if it is within `B`.
    #[inline(always)]
    pub fn try_new(index: T) -> Result<Self, OutOfBounds> {
        if B::contains(index) {
            Ok(Self::new_unchecked(index))
        } else {
            Err(OutOfBounds)
        }
    }

    /// The position as a number, 0 the first.
    #[inline(always)]
    pub const fn get(self) -> T {
        self.index
    }

    /// How far from the start of the table: the same number as an amount of `Of`.
    #[inline(always)]
    pub const fn offset(self) -> Fixed<Of, One, T, B> {
        Fixed::new_unchecked(self.index)
    }

    /// The same position without bounds.
    #[inline(always)]
    pub fn unbounded(self) -> Index<Of, T> {
        Index::new(self.get())
    }

    /// `n` further on, `None` past the end of `T` or outside `B`.
    #[inline(always)]
    pub fn checked_add<B2: Bounds<T>>(self, n: Fixed<Of, One, T, B2>) -> Option<Self> {
        Self::try_new(self.get().checked_add(n.get())?).ok()
    }

    /// `n` back, `None` before 0 or outside `B`.
    #[inline(always)]
    pub fn checked_sub<B2: Bounds<T>>(self, n: Fixed<Of, One, T, B2>) -> Option<Self> {
        Self::try_new(self.get().checked_sub(n.get())?).ok()
    }

    /// How far `self` is after `earlier`, `None` when it is before it.
    #[inline(always)]
    pub fn checked_distance(self, earlier: Self) -> Option<Fixed<Of, One, T>> {
        self.get().checked_sub(earlier.get()).map(Fixed::new)
    }

    /// Run on every value read from the wire: a position outside `B` is [shrink_wrap::Error::SubtypeOutOfRange].
    #[cfg(feature = "shrink_wrap")]
    #[inline(always)]
    fn validate(&self) -> Result<(), ShrinkWrapError> {
        if B::contains(self.index) {
            Ok(())
        } else {
            Err(ShrinkWrapError::SubtypeOutOfRange)
        }
    }
}

/// `index + n`: panics past the end of `T` in debug builds, like `T`'s `+`. Only without bounds.
impl<Of, T: UnsignedStorage> Add<Fixed<Of, One, T>> for Index<Of, T> {
    type Output = Self;
    #[inline(always)]
    fn add(self, n: Fixed<Of, One, T>) -> Self {
        Self::new(self.get().add(n.get()))
    }
}

impl<Of, T: UnsignedStorage> AddAssign<Fixed<Of, One, T>> for Index<Of, T> {
    #[inline(always)]
    fn add_assign(&mut self, n: Fixed<Of, One, T>) {
        *self = *self + n;
    }
}

/// `index - n`: panics before 0 in debug builds. Only without bounds.
impl<Of, T: UnsignedStorage> Sub<Fixed<Of, One, T>> for Index<Of, T> {
    type Output = Self;
    #[inline(always)]
    fn sub(self, n: Fixed<Of, One, T>) -> Self {
        Self::new(self.get().sub(n.get()))
    }
}

impl<Of, T: UnsignedStorage> SubAssign<Fixed<Of, One, T>> for Index<Of, T> {
    #[inline(always)]
    fn sub_assign(&mut self, n: Fixed<Of, One, T>) {
        *self = *self - n;
    }
}

/// `later - earlier`, the distance between two positions as an amount of `Of`: panics in debug builds when `later`
/// is before `earlier` ([checked_distance](Index::checked_distance) doesn't).
impl<Of, T: UnsignedStorage, B: Bounds<T>> Sub for Index<Of, T, B> {
    type Output = Fixed<Of, One, T>;
    #[inline(always)]
    fn sub(self, earlier: Self) -> Fixed<Of, One, T> {
        Fixed::new(self.get().sub(earlier.get()))
    }
}

/// A 1-based position in a table of `Of`, as people and datasheets count: channel 1 to 4, pin 1, motor 1 to 4. Never
/// 0: 0 is refused on construction and on reading from the wire. `B` bounds the number as shown (`Range<1, 4>` for
/// four channels). On the wire exactly the `T`.
///
/// ```
/// use exact_units::{Index, IndexFrom1, Range, units::Item};
///
/// type Channel = IndexFrom1<Item, u8, Range<1, 4>>;
/// let ch = Channel::try_new(3).unwrap();
/// assert_eq!(ch.to_string(), "3");
/// assert_eq!(ch.to_index(), Index::new(2));
/// assert_eq!(Channel::from_index(Index::<Item, u8>::new(0)), Some(Channel::try_new(1).unwrap()));
/// assert!(Channel::try_new(0).is_err());
/// assert!(Channel::try_new(5).is_err());
/// ```
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(borrowed, owned(feature = "std"), transparent, validate = Self::validate)
)]
#[repr(transparent)]
pub struct IndexFrom1<Of, T: UnsignedStorage, B: Bounds<T> = Unbounded> {
    number: T,
    _of: PhantomData<Of>,
    _bounds: PhantomData<B>,
}

impl<Of, T: UnsignedStorage, B: Bounds<T>> IndexFrom1<Of, T, B> {
    /// Number `number`, 1 the first: an error for 0 and outside `B`.
    #[inline(always)]
    pub fn try_new(number: T) -> Result<Self, OutOfBounds> {
        if number == T::ZERO || !B::contains(number) {
            return Err(OutOfBounds);
        }
        Ok(IndexFrom1 {
            number,
            _of: PhantomData,
            _bounds: PhantomData,
        })
    }

    /// The number as people count it, 1 the first.
    #[inline(always)]
    pub const fn get(self) -> T {
        self.number
    }

    /// The 0-based position: one less than the number.
    #[inline(always)]
    pub fn to_index(self) -> Index<Of, T> {
        Index::new(self.get().sub(T::from_int(1).unwrap_or(T::ZERO)))
    }

    /// The number of a 0-based position: one more, `None` past the end of `T` or outside `B`.
    #[inline(always)]
    pub fn from_index<B2: Bounds<T>>(index: Index<Of, T, B2>) -> Option<Self> {
        Self::try_new(index.get().checked_add(T::from_int(1)?)?).ok()
    }

    /// Run on every value read from the wire: 0 and a number outside `B` are
    /// [shrink_wrap::Error::SubtypeOutOfRange].
    #[cfg(feature = "shrink_wrap")]
    #[inline(always)]
    fn validate(&self) -> Result<(), ShrinkWrapError> {
        if self.number == T::ZERO || !B::contains(self.number) {
            Err(ShrinkWrapError::SubtypeOutOfRange)
        } else {
            Ok(())
        }
    }
}

/// A wrapping sequence number of `Of`, stored as `T`: a request's, an envelope's, a frame's (`Seq<Frame, u16>`,
/// `Seq<Packet, Nibble>`). It moves on modulo 2^bits of `T` and compares in serial number order (RFC 1982): `a` is
/// before `b` when `b` is less than half the range ahead of it, so the order holds across the wrap. On the wire
/// exactly the `T`.
///
/// ```
/// use exact_units::{Fixed, Seq, units::Frame};
///
/// let a = Seq::<Frame, u8>::new(250);
/// let b = a.wrapping_add(10);
/// assert_eq!(b.get(), 4);
/// assert!(a.is_before(b));
/// assert!(b.is_after(a));
/// assert_eq!(a.distance(b), Fixed::of(10));
/// assert_eq!(a.next().get(), 251);
/// ```
///
/// No `<` (integer order is wrong across the wrap), and two sequence numbers don't add:
///
/// ```compile_fail
/// use exact_units::{Seq, units::Frame};
/// let _ = Seq::<Frame, u8>::new(1) < Seq::<Frame, u8>::new(2);
/// ```
///
/// ```compile_fail
/// use exact_units::{Seq, units::Frame};
/// let _ = Seq::<Frame, u8>::new(1) + Seq::<Frame, u8>::new(2);
/// ```
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(borrowed, owned(feature = "std"), transparent)
)]
#[repr(transparent)]
pub struct Seq<Of, T: UnsignedStorage> {
    seq: T,
    _of: PhantomData<Of>,
}

impl<Of, T: UnsignedStorage> Seq<Of, T> {
    /// Sequence number `seq`.
    #[inline(always)]
    pub const fn new(seq: T) -> Self {
        Seq {
            seq,
            _of: PhantomData,
        }
    }

    /// The number as stored.
    #[inline(always)]
    pub const fn get(self) -> T {
        self.seq
    }

    /// The next one, 0 after the largest `T`.
    #[inline(always)]
    pub fn next(self) -> Self {
        self.wrapping_add(T::from_int(1).unwrap_or(T::ZERO))
    }

    /// `n` further on, modulo 2^bits of `T`.
    #[inline(always)]
    pub fn wrapping_add(self, n: T) -> Self {
        Self::new(self.get().wrapping_add(n))
    }

    /// `n` back, modulo 2^bits of `T`.
    #[inline(always)]
    pub fn wrapping_sub(self, n: T) -> Self {
        Self::new(self.get().wrapping_sub(n))
    }

    /// How many steps forward `later` is from `self`, modulo 2^bits of `T`: `0..2^bits`, below half the range when
    /// `self.is_before(later)`.
    #[inline(always)]
    pub fn distance(self, later: Self) -> Fixed<Of, One, T, Wrapping> {
        Fixed::new_unchecked(later.get().wrapping_sub(self.get()))
    }

    /// `self` comes before `other` in serial number order (RFC 1982): `other` is ahead by more than 0 and less than
    /// half the range. Two numbers exactly half the range apart are neither before nor after each other.
    #[inline(always)]
    pub fn is_before(self, other: Self) -> bool {
        let d = self.distance(other).get().to_u64();
        d != 0 && d < 1u64 << (T::BITS - 1)
    }

    /// `other.is_before(self)`.
    #[inline(always)]
    pub fn is_after(self, other: Self) -> bool {
        other.is_before(self)
    }

    /// The serial number order of RFC 1982: `None` for two numbers exactly half the range apart, whose order is
    /// undefined. Not [PartialOrd]: it isn't transitive.
    #[inline(always)]
    pub fn serial_cmp(self, other: Self) -> Option<Ordering> {
        if self == other {
            Some(Ordering::Equal)
        } else if self.is_before(other) {
            Some(Ordering::Less)
        } else if other.is_before(self) {
            Some(Ordering::Greater)
        } else {
            None
        }
    }
}

/// `seq + n`, wrapping.
impl<Of, T: UnsignedStorage, B: Unlimited<T>> Add<Fixed<Of, One, T, B>> for Seq<Of, T> {
    type Output = Self;
    #[inline(always)]
    fn add(self, n: Fixed<Of, One, T, B>) -> Self {
        self.wrapping_add(n.get())
    }
}

impl<Of, T: UnsignedStorage, B: Unlimited<T>> AddAssign<Fixed<Of, One, T, B>> for Seq<Of, T> {
    #[inline(always)]
    fn add_assign(&mut self, n: Fixed<Of, One, T, B>) {
        *self = *self + n;
    }
}

/// `seq - n`, wrapping.
impl<Of, T: UnsignedStorage, B: Unlimited<T>> Sub<Fixed<Of, One, T, B>> for Seq<Of, T> {
    type Output = Self;
    #[inline(always)]
    fn sub(self, n: Fixed<Of, One, T, B>) -> Self {
        self.wrapping_sub(n.get())
    }
}

impl<Of, T: UnsignedStorage, B: Unlimited<T>> SubAssign<Fixed<Of, One, T, B>> for Seq<Of, T> {
    #[inline(always)]
    fn sub_assign(&mut self, n: Fixed<Of, One, T, B>) {
        *self = *self - n;
    }
}

impl<Of, T: UnsignedStorage> From<T> for Seq<Of, T> {
    #[inline(always)]
    fn from(seq: T) -> Self {
        Self::new(seq)
    }
}

/// Clone, Copy, Eq, Hash and Default by the number, for every `Of` (a marker needn't derive them); `Ord` for the two
/// indices only.
macro_rules! by_number {
    ($($ty:ident [$($b:tt)*] [$($bp:tt)*];)*) => {$(
        impl<Of, T: UnsignedStorage $($b)*> Clone for $ty<Of, T $($bp)*> {
            #[inline(always)]
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<Of, T: UnsignedStorage $($b)*> Copy for $ty<Of, T $($bp)*> {}

        impl<Of, T: UnsignedStorage $($b)*> PartialEq for $ty<Of, T $($bp)*> {
            #[inline(always)]
            fn eq(&self, other: &Self) -> bool {
                self.get() == other.get()
            }
        }

        impl<Of, T: UnsignedStorage + Eq $($b)*> Eq for $ty<Of, T $($bp)*> {}

        impl<Of, T: UnsignedStorage + Hash $($b)*> Hash for $ty<Of, T $($bp)*> {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.get().hash(state)
            }
        }

        /// The number as stored: `3`.
        impl<Of, T: UnsignedStorage $($b)*> fmt::Display for $ty<Of, T $($bp)*> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.get().fmt_number(f)
            }
        }

        /// `Index<Sample>(3 u32)`: the type, what it counts, the number and its storage.
        impl<Of: Unit, T: UnsignedStorage $($b)*> fmt::Debug for $ty<Of, T $($bp)*> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}<{}>(", stringify!($ty), Of::EXPR.name())?;
                self.get().fmt_number(f)?;
                write!(f, " {})", T::NAME)
            }
        }

        /// `Index(3)`.
        #[cfg(feature = "defmt")]
        impl<Of, T: UnsignedStorage + defmt::Format $($b)*> defmt::Format for $ty<Of, T $($bp)*> {
            fn format(&self, f: defmt::Formatter<'_>) {
                defmt::write!(f, "{=str}({})", stringify!($ty), self.get())
            }
        }
    )*};
}

by_number! {
    Index [, B: Bounds<T>] [, B];
    IndexFrom1 [, B: Bounds<T>] [, B];
    Seq [] [];
}

impl<Of, T: UnsignedStorage + Ord, B: Bounds<T>> PartialOrd for Index<Of, T, B> {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<Of, T: UnsignedStorage + Ord, B: Bounds<T>> Ord for Index<Of, T, B> {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> Ordering {
        self.get().cmp(&other.get())
    }
}

impl<Of, T: UnsignedStorage + Ord, B: Bounds<T>> PartialOrd for IndexFrom1<Of, T, B> {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<Of, T: UnsignedStorage + Ord, B: Bounds<T>> Ord for IndexFrom1<Of, T, B> {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> Ordering {
        self.get().cmp(&other.get())
    }
}

impl<Of, T: UnsignedStorage + Default, B: Unlimited<T>> Default for Index<Of, T, B> {
    /// The first position, 0.
    fn default() -> Self {
        Index::at(Fixed::of(T::default()))
    }
}

impl<Of, T: UnsignedStorage + Default> Default for Seq<Of, T> {
    /// 0.
    fn default() -> Self {
        Seq::new(T::default())
    }
}

#[cfg(all(test, feature = "std"))]
mod tests;
