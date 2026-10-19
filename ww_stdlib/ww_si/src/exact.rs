//! Exact integer arithmetic on quantities of the same unit: sums, differences and integer multiples that keep
//! integer storage, for timestamps and durations above all (`t0 + i * dt`). The operators (`+`, `*`, ..) compute in
//! floats instead, see [Quantity].

use crate::unit::Unit;
use crate::{Prefix, Quantity};
use ww_numeric::Integer;

// Everything in ww_numeric::Integer: no wider arithmetic than the enabled number types need (u32 with types up to 32
// bits), so a firmware that adds timestamps links no 64- or 128-bit code.

impl<U: Unit> Quantity<U> {
    /// `self + rhs` exactly, in the finer prefix of the two and in `self`'s number type: `Second::milli(5u32)` plus
    /// `Second::micro(40u16)` is `Second::micro(5040u32)`. `None` when either is a float (use `+`) or `UN` / `IN`,
    /// or when the sum doesn't fit `self`'s number type.
    pub fn checked_add(&self, rhs: &Self) -> Option<Self> {
        self.checked_combine(rhs, Integer::checked_add)
    }

    /// `self - rhs` exactly, like [checked_add](Self::checked_add): `None` below zero for an unsigned number type.
    pub fn checked_sub(&self, rhs: &Self) -> Option<Self> {
        self.checked_combine(rhs, Integer::checked_sub)
    }

    /// `self * n` exactly, in `self`'s prefix and number type: `Second::micro(40u16).checked_mul_int(3)` is
    /// `Second::micro(120u16)`. `n` is any integer that fits an [Integer] (`i32`, `u32`, .., `i64` with a 64-bit number
    /// type enabled). `None` for a float (use `*`) or `UN` / `IN`, and when the product doesn't fit.
    pub fn checked_mul_int(&self, n: impl Into<Integer>) -> Option<Self> {
        let x = self.value.to_integer()?.checked_mul(n.into())?;
        Some(Self::from_parts(
            self.prefix,
            self.value.ty().from_integer(x)?,
        ))
    }

    /// `self + rhs` in `self`'s prefix and number type, wrapping around at the type's width like the counter of a
    /// device clock: `Second::micro(u32::MAX)` plus 2 µs is `Second::micro(1u32)`. `None` when `rhs` is not a whole
    /// number of `self`'s prefix (40 µs to a value in ms), when either is a float or `UN` / `IN`, and when `rhs` in
    /// `self`'s prefix is beyond the widest enabled integer type (`u32::MAX` with types up to 32 bits).
    pub fn wrapping_add(&self, rhs: &Self) -> Option<Self> {
        let rhs = rhs.exact_in(self.prefix)?;
        Some(Self::from_parts(self.prefix, self.value.wrapping_add(rhs)?))
    }

    /// `self - rhs`, wrapping like [wrapping_add](Self::wrapping_add).
    pub fn wrapping_sub(&self, rhs: &Self) -> Option<Self> {
        let rhs = rhs.exact_in(self.prefix)?;
        Some(Self::from_parts(
            self.prefix,
            self.value.wrapping_add(-rhs)?,
        ))
    }

    /// `self`, `self + step`, `self + 2 * step`, .. without end, each computed exactly in `self`'s prefix and number
    /// type and wrapping like [wrapping_add](Self::wrapping_add): the times of regularly sampled items, from the first
    /// one's time and the sampling period. `None` like `wrapping_add`.
    ///
    /// ```
    /// use ww_si::Second;
    ///
    /// let t: Vec<_> = Second::micro(u32::MAX - 39).steps(&Second::micro(40u8)).unwrap().take(3).collect();
    /// assert_eq!(t, [Second::micro(u32::MAX - 39), Second::micro(0u32), Second::micro(40u32)]);
    /// // 40 µs is not a whole number of ms
    /// assert!(Second::milli(5u32).steps(&Second::micro(40u8)).is_none());
    /// ```
    pub fn steps(&self, step: &Self) -> Option<Steps<U>> {
        let step = step.exact_in(self.prefix)?;
        // checks that self is an integer of a type that wraps
        self.value.wrapping_add(Integer::ZERO)?;
        Some(Steps { next: *self, step })
    }

    /// The value in units of `prefix`, if that is an integer (scaling down to a finer prefix always is, if it fits).
    fn exact_in(&self, prefix: Prefix) -> Option<Integer> {
        let exp = i8::from(self.prefix) - i8::from(prefix);
        self.value.to_integer()?.exact_scale_pow10(exp)
    }

    fn checked_combine(
        &self,
        rhs: &Self,
        op: fn(Integer, Integer) -> Option<Integer>,
    ) -> Option<Self> {
        let prefix = if i8::from(self.prefix) <= i8::from(rhs.prefix) {
            self.prefix
        } else {
            rhs.prefix
        };
        let x = op(self.exact_in(prefix)?, rhs.exact_in(prefix)?)?;
        Some(Self::from_parts(prefix, self.value.ty().from_integer(x)?))
    }
}

/// The times `t0 + i * step`, see [Quantity::steps].
#[derive(Clone, Debug)]
pub struct Steps<U: Unit> {
    next: Quantity<U>,
    /// In `next`'s prefix.
    step: Integer,
}

impl<U: Unit> Iterator for Steps<U> {
    type Item = Quantity<U>;

    fn next(&mut self) -> Option<Quantity<U>> {
        let current = self.next;
        // checked in `steps`: an integer that wraps
        self.next = Quantity::from_parts(current.prefix, current.value.wrapping_add(self.step)?);
        Some(current)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (usize::MAX, None)
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use crate::{Second, Volt};
    use ww_numeric::NumericValue;

    #[test]
    fn add_across_prefixes() {
        let t = Second::milli(5u32)
            .checked_add(&Second::micro(40u16))
            .unwrap();
        assert_eq!(t.prefix, crate::Prefix::Micro);
        assert_eq!(t.value, NumericValue::U32(5040));
        // the finer prefix wins whichever side it is on, the number type is self's
        let t = Second::micro(40u16)
            .checked_add(&Second::milli(5u32))
            .unwrap();
        assert_eq!(t.value, NumericValue::U16(5040));
        // same prefix
        assert_eq!(
            Volt::milli(3300u16).checked_add(&Volt::milli(200u8)),
            Some(Volt::milli(3500u16))
        );
    }

    #[test]
    fn add_overflow_and_floats() {
        assert_eq!(
            Second::micro(u32::MAX).checked_add(&Second::micro(1u8)),
            None
        );
        // 70 s in µs doesn't fit u16
        assert_eq!(Second::base(70u16).checked_add(&Second::micro(1u8)), None);
        assert_eq!(Second::from_f32(1.0).checked_add(&Second::micro(1u8)), None);
        assert_eq!(Second::micro(1u8).checked_add(&Second::from_f32(1.0)), None);
    }

    #[test]
    fn sub() {
        assert_eq!(
            Second::milli(5u32).checked_sub(&Second::micro(40u16)),
            Some(Second::micro(4960u32))
        );
        assert_eq!(Second::micro(1u32).checked_sub(&Second::micro(2u8)), None);
        assert_eq!(
            Volt::micro(1i32).checked_sub(&Volt::micro(2u8)),
            Some(Volt::micro(-1i32))
        );
    }

    #[test]
    fn mul_int() {
        let t = Second::micro(40u16).checked_mul_int(3).unwrap();
        assert_eq!(t.value, NumericValue::U16(120));
        assert_eq!(Second::micro(40u16).checked_mul_int(2000), None);
        assert_eq!(Second::micro(40u16).checked_mul_int(-1), None);
        assert_eq!(
            Volt::milli(-5i32).checked_mul_int(-3),
            Some(Volt::milli(15i32))
        );
        assert_eq!(Second::from_f32(1.0).checked_mul_int(2), None);
    }

    #[test]
    fn wrapping() {
        let t = Second::micro(u32::MAX)
            .wrapping_add(&Second::micro(2u8))
            .unwrap();
        assert_eq!(t.value, NumericValue::U32(1));
        // a step in a coarser prefix is exact in the finer one
        let t = Second::micro(u32::MAX)
            .wrapping_add(&Second::milli(1u8))
            .unwrap();
        assert_eq!(t.value, NumericValue::U32(999));
        let t = Second::milli(0u16)
            .wrapping_sub(&Second::milli(1u8))
            .unwrap();
        assert_eq!(t.value, NumericValue::U16(u16::MAX));
        // not a whole number of ms
        assert_eq!(Second::milli(5u16).wrapping_add(&Second::micro(40u8)), None);
        // a whole number of ms
        assert_eq!(
            Second::milli(5u16).wrapping_add(&Second::micro(2000u16)),
            Some(Second::milli(7u16))
        );
        assert_eq!(
            Second::from_f32(1.0).wrapping_add(&Second::micro(1u8)),
            None
        );
    }

    #[test]
    fn steps_wrap_in_every_storage() {
        let at = |t: Second, n: usize| t.steps(&Second::micro(40u8)).unwrap().nth(n).unwrap();
        assert_eq!(
            at(Second::micro(u32::MAX - 39), 2).value,
            NumericValue::U32(40)
        );
        assert_eq!(
            at(Second::micro(u16::MAX - 39), 1).value,
            NumericValue::U16(0)
        );
        assert_eq!(at(Second::micro(u64::MAX), 1).value, NumericValue::U64(39));
        assert_eq!(at(Second::micro(5u64), 3).value, NumericValue::U64(125));
        // a ms timestamp with a 2 ms step
        let t: Vec<_> = Second::milli(65535u16)
            .steps(&Second::milli(2u8))
            .unwrap()
            .take(3)
            .collect();
        assert_eq!(
            t,
            [
                Second::milli(65535u16),
                Second::milli(1u16),
                Second::milli(3u16)
            ]
        );
        assert!(Second::from_f32(0.0).steps(&Second::micro(1u8)).is_none());
    }
}
