//! Running totals: a value written to a plain sink as the sum of everything added so far, so a lost write loses
//! nothing (API-70, `docs/api/totals.md`).

use exact_units::{Fixed, Storage, Unlimited};
use shrink_wrap::derive_shrink_wrap;

/// The sum of every change a sender made in one session, written to a plain sink (`sink!(motion: Totals<Delta>)`).
///
/// The receiver applies the difference to the last write it applied, so a lost write costs nothing: the next one
/// brings its change. A session's first write only sets where its totals start. `ww_stream::totals` has the two
/// ends: `TotalsTx` adds changes up and says what to write, `TotalsRx` turns writes back into changes.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    final_structure
)]
pub struct Totals<T> {
    /// The sender's session: a write of another session than the last one applied starts the totals anew.
    pub session: TotalsSession,
    /// Every change added in this session, wrapping at the ends of `T`.
    pub total: T,
}

/// Which session of a sender a [Totals] write belongs to: a sender picks a new one for every link it sends over,
/// and a new process starts at a random one.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq, Hash),
    transparent
)]
pub struct TotalsSession(pub u16);

impl TotalsSession {
    /// The session after this one, wrapping.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// A value that [Totals] can add up: wrapping addition and the difference of two totals.
///
/// Implemented for the integers, for [Fixed] over an integer without bounds (a `Range` bound would not survive the
/// wrap), and for arrays of them. A struct of several totals (`Delta { dx, dy }`) implements it field by field.
pub trait Accumulate: Copy + PartialEq + Default {
    /// `self` with `change` added, wrapping at the ends of the storage.
    #[must_use]
    fn accumulate(self, change: Self) -> Self;

    /// The change from `earlier` to `self`, wrapping: `a.accumulate(b).since(a) == b`.
    #[must_use]
    fn since(self, earlier: Self) -> Self;
}

macro_rules! accumulate_int {
    ($($t:ty)*) => {$(
        impl Accumulate for $t {
            #[inline(always)]
            fn accumulate(self, change: Self) -> Self {
                self.wrapping_add(change)
            }

            #[inline(always)]
            fn since(self, earlier: Self) -> Self {
                self.wrapping_sub(earlier)
            }
        }
    )*};
}

accumulate_int!(u8 u16 u32 u64 i8 i16 i32 i64);

impl<U, P, T, B> Accumulate for Fixed<U, P, T, B>
where
    T: Storage + Accumulate,
    B: Unlimited<T>,
    Self: PartialEq,
{
    #[inline(always)]
    fn accumulate(self, change: Self) -> Self {
        Self::of(self.get().accumulate(change.get()))
    }

    #[inline(always)]
    fn since(self, earlier: Self) -> Self {
        Self::of(self.get().since(earlier.get()))
    }
}

impl<T: Accumulate, const N: usize> Accumulate for [T; N]
where
    Self: Default,
{
    #[inline(always)]
    fn accumulate(self, change: Self) -> Self {
        core::array::from_fn(|i| self[i].accumulate(change[i]))
    }

    #[inline(always)]
    fn since(self, earlier: Self) -> Self {
        core::array::from_fn(|i| self[i].since(earlier[i]))
    }
}

#[cfg(test)]
mod tests {
    use super::Accumulate;
    use exact_units::{Fixed, prefix::One, units::Meter};

    #[test]
    fn wraps_at_the_ends_of_the_storage() {
        assert_eq!(i32::MAX.accumulate(2), i32::MIN + 1);
        assert_eq!((i32::MIN + 1).since(i32::MAX), 2);
        assert_eq!(250u8.accumulate(10).since(250), 10);
    }

    #[test]
    fn fixed_and_arrays_add_up_field_by_field() {
        type M = Fixed<Meter, One, i16>;
        let a = [M::new(i16::MAX), M::new(-3)];
        let b = a.accumulate([M::new(5), M::new(-7)]);
        assert_eq!(b, [M::new(i16::MIN + 4), M::new(-10)]);
        assert_eq!(b.since(a), [M::new(5), M::new(-7)]);
    }
}
