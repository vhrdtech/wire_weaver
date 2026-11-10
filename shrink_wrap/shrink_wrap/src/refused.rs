//! What a `validate` hook refused, with the numbers: the value read and the rule it broke (a range, NaN, ±∞, a
//! zero, a spare code).
//!
//! [Error] stays a fieldless `u32` so that every `Result` of a firmware stays a register wide; the numbers travel
//! beside it. A `validate = path` function may return `Result<(), Refused>` instead of `Result<(), Error>`: the
//! derive turns a [Refused] into [Error::SubtypeOutOfRange] for the result, and, with the `refused` feature (on with
//! `std`), writes it into the [Slot] of a reader made with
//! [BufReader::with_refusal_slot](crate::BufReader::with_refusal_slot), a pointer more in the reader.
//! [DeserializeShrinkWrapOwned::from_ww_bytes_explained](crate::DeserializeShrinkWrapOwned::from_ww_bytes_explained)
//! makes one and returns the numbers in a [ReadError], also for a value nested at any depth, read latched or in a
//! list. A reader made with [BufReader::new](crate::BufReader::new) has no slot. Without the feature (firmware)
//! the reader doesn't grow and the numbers are dropped where they are made, with the code that makes them.
//!
//! ```
//! use shrink_wrap::prelude::*;
//! use shrink_wrap::refused::{ReadError, Refused, RefusedNum, Rule};
//!
//! /// A percentage, 0..=100.
//! #[derive_shrink_wrap(owned, transparent, validate = Percent::validate)]
//! #[derive(Debug, PartialEq)]
//! pub struct Percent(u8);
//!
//! impl Percent {
//!     fn validate(&self) -> Result<(), Refused> {
//!         if self.0 <= 100 {
//!             Ok(())
//!         } else {
//!             Err(Refused::out_of(RefusedNum::Int(self.0.into()), RefusedNum::Int(0), RefusedNum::Int(100)))
//!         }
//!     }
//! }
//!
//! let err = Percent::from_ww_bytes_explained(&[101]).unwrap_err();
//! assert_eq!(err.error, shrink_wrap::Error::SubtypeOutOfRange);
//! assert_eq!(err.refused.map(|r| r.rule), Some(Rule::Range { low: RefusedNum::Int(0), high: RefusedNum::Int(100) }));
//! assert_eq!(err.to_string(), "101 is out of 0..=100");
//! // the plain read is the same error, without the numbers
//! assert_eq!(Percent::from_ww_bytes_owned(&[101]), Err(shrink_wrap::Error::SubtypeOutOfRange));
//! ```

use crate::Error;
use core::fmt::{self, Display, Formatter};

/// A number a [Refused] names: the stored number of the value, as read, never converted. Floats are equal by
/// their bits (a NaN equals the same NaN), so a refusal compares as a report.
#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum RefusedNum {
    /// An integer of any width up to 128 bits.
    Int(i128),
    /// A float, `f32` widened.
    Float(f64),
}

impl PartialEq for RefusedNum {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (RefusedNum::Int(a), RefusedNum::Int(b)) => a == b,
            (RefusedNum::Float(a), RefusedNum::Float(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}

impl Eq for RefusedNum {}

impl Display for RefusedNum {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            RefusedNum::Int(i) => write!(f, "{i}"),
            RefusedNum::Float(x) => write!(f, "{x}"),
        }
    }
}

/// The rule a refused value broke.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Rule {
    /// It must be within `low..=high`.
    Range {
        /// The lowest value taken.
        low: RefusedNum,
        /// The highest value taken.
        high: RefusedNum,
    },
    /// A float that is never NaN.
    NotNan,
    /// A float that is never ±∞ nor NaN.
    Finite,
    /// A number that is never 0 (counting from 1).
    NotZero,
    /// A code that means no value: a spare code of a codec, a code that is neither a value nor a niche.
    NoValue,
}

/// A value a `validate` hook refused: the number read and the rule it broke. Two numbers and a tag, no type and no
/// unit: the reader knows which type it read. See the [module](self).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Refused {
    /// The stored number of the refused value.
    pub value: RefusedNum,
    /// What it should have been.
    pub rule: Rule,
}

impl Refused {
    /// `value` is outside `low..=high`.
    #[inline(always)]
    pub const fn out_of(value: RefusedNum, low: RefusedNum, high: RefusedNum) -> Self {
        Refused {
            value,
            rule: Rule::Range { low, high },
        }
    }

    /// `value` broke `rule`.
    #[inline(always)]
    pub const fn new(value: RefusedNum, rule: Rule) -> Self {
        Refused { value, rule }
    }
}

impl Display for Refused {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let v = self.value;
        match self.rule {
            Rule::Range { low, high } => write!(f, "{v} is out of {low}..={high}"),
            Rule::NotNan => write!(f, "{v} is refused: never NaN"),
            Rule::Finite => write!(f, "{v} is refused: never ±∞ nor NaN"),
            Rule::NotZero => write!(f, "{v} is refused: it counts from 1"),
            Rule::NoValue => write!(f, "{v} is a code that means no value"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Refused {}

/// Every refusal is [Error::SubtypeOutOfRange] as a plain error.
impl From<Refused> for Error {
    #[inline(always)]
    fn from(_: Refused) -> Self {
        Error::SubtypeOutOfRange
    }
}

/// What a `validate = path` function may return as its error: [Error], or [Refused] with the numbers. The derive
/// passes it to `BufReader::refuse`.
pub trait Refusal: Copy {
    /// The error of the read.
    fn error(self) -> Error;
    /// The numbers, if this refusal has them.
    #[inline(always)]
    fn refused(self) -> Option<Refused> {
        None
    }
}

impl Refusal for Error {
    #[inline(always)]
    fn error(self) -> Error {
        self
    }
}

impl Refusal for Refused {
    #[inline(always)]
    fn error(self) -> Error {
        Error::SubtypeOutOfRange
    }

    #[inline(always)]
    fn refused(self) -> Option<Refused> {
        Some(self)
    }
}

/// A failed read with what a `validate` hook refused, if one did:
/// [from_ww_bytes_explained](crate::DeserializeShrinkWrapOwned::from_ww_bytes_explained).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ReadError {
    /// The error of the read.
    pub error: Error,
    /// The value and rule, when the error is [Error::SubtypeOutOfRange] from a hook that returned [Refused].
    pub refused: Option<Refused>,
}

/// Where a reader made with [BufReader::with_refusal_slot](crate::BufReader::with_refusal_slot) keeps what a hook
/// refused: one per read, owned by the caller. Every reader split off that reader (a nested value, an item of a
/// list) writes into the same slot; a reader made anywhere else, a nested decode inside a `validate` included, has
/// none, so two reads never see each other's refusal.
pub struct Slot {
    /// Held while the value is written or taken: a few stores, never across a call that could panic
    #[cfg(feature = "refused")]
    busy: core::sync::atomic::AtomicBool,
    #[cfg(feature = "refused")]
    value: core::cell::UnsafeCell<Option<Refused>>,
}

// SAFETY: `value` is only touched in `with`, while `busy` is held by this thread alone.
#[cfg(feature = "refused")]
unsafe impl Sync for Slot {}

impl Default for Slot {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Slot {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("Slot")
    }
}

impl Slot {
    /// An empty slot.
    pub const fn new() -> Self {
        Slot {
            #[cfg(feature = "refused")]
            busy: core::sync::atomic::AtomicBool::new(false),
            #[cfg(feature = "refused")]
            value: core::cell::UnsafeCell::new(None),
        }
    }

    /// What a hook refused in the read, and empty the slot. Always `None` without the `refused` feature.
    pub fn take(&self) -> Option<Refused> {
        #[cfg(feature = "refused")]
        {
            self.with(Option::take)
        }
        #[cfg(not(feature = "refused"))]
        {
            None
        }
    }

    /// Keep `refused`. A read refuses once: a plain read stops at its first refusal, a latched one takes none
    /// after an error ([BufReader::refuse_latch](crate::BufReader::refuse_latch)), so the first refusal wins.
    #[cfg(feature = "refused")]
    #[inline]
    pub(crate) fn keep(&self, refused: Refused) {
        self.with(|v| *v = Some(refused));
    }

    /// `f` on the value, alone: readers sharing a slot (copies of a `RefVec`'s reader) may be on several threads.
    #[cfg(feature = "refused")]
    fn with<R>(&self, f: impl FnOnce(&mut Option<Refused>) -> R) -> R {
        use core::sync::atomic::Ordering;
        while self
            .busy
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        // SAFETY: `busy` is held, so no other thread is in here
        let r = f(unsafe { &mut *self.value.get() });
        self.busy.store(false, Ordering::Release);
        r
    }
}

/// The [Slot] a [BufReader](crate::BufReader) writes into, a pointer wide (`refused` feature). Equal when it is the
/// same slot.
#[cfg(feature = "refused")]
#[derive(Copy, Clone)]
pub(crate) struct SlotRef<'i>(pub(crate) &'i Slot);

#[cfg(feature = "refused")]
impl PartialEq for SlotRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.0, other.0)
    }
}

#[cfg(feature = "refused")]
impl Eq for SlotRef<'_> {}

#[cfg(feature = "refused")]
impl fmt::Debug for SlotRef<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("SlotRef")
    }
}

#[cfg(all(feature = "refused", feature = "defmt"))]
impl defmt::Format for SlotRef<'_> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "SlotRef")
    }
}

impl ReadError {
    /// `error`, with what `slot` kept when it is [Error::SubtypeOutOfRange].
    pub fn of(error: Error, slot: &Slot) -> Self {
        ReadError {
            error,
            refused: match error {
                Error::SubtypeOutOfRange => slot.take(),
                _ => None,
            },
        }
    }
}

impl From<ReadError> for Error {
    fn from(e: ReadError) -> Self {
        e.error
    }
}

impl Display for ReadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.refused {
            Some(r) => write!(f, "{r}"),
            None => write!(f, "{}", self.error),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ReadError {}
