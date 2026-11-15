//! [FromFile]: a number read from untrusted bytes, usable as a length, a count, an offset or a loop bound only once
//! it is bounded.
//!
//! A block's bytes come from a file, a medium or a peer: a count, a length, an offset or a layout value read from
//! them can be anything its type holds. The crate's byte readers hand such a value out as `FromFile<T>`, which has no
//! arithmetic and no conversion to `usize`. The ways out:
//!
//! - [FromFile::at_most]: the value as a `usize` when it is at most a named cap (a `pub const` of the crate, listed in
//!   the format text), else a [BoundError];
//! - [FromFile::within] and [FromFile::below]: the value as a `usize` when it is at most, or under, a length already
//!   known from the bytes in hand (the block's length, a table's entry count);
//! - [FromFile::bounded]: the value as its own type when it is at most a cap, for a bit width or a shift;
//! - [FromFile::is]: whether it equals a value the reader computed, for a header that repeats what the body says.
//!
//! So a value from the file reaches an allocation, a slice length or a loop count only through a bound a reader can
//! name. The type depends on nothing else in the crate.

use core::fmt;

/// An unsigned integer type a [FromFile] wraps.
pub trait FileInt: Copy + Ord + Into<u64> + fmt::Debug + sealed::Sealed {}

impl FileInt for u8 {}
impl FileInt for u16 {}
impl FileInt for u32 {}
impl FileInt for u64 {}

mod sealed {
    pub trait Sealed {}
    impl Sealed for u8 {}
    impl Sealed for u16 {}
    impl Sealed for u32 {}
    impl Sealed for u64 {}
}

/// A count, length, offset or layout value read from untrusted bytes: see the [module](self) docs.
#[derive(Copy, Clone, PartialEq, Eq)]
#[must_use = "a value read from the file is bounded before it is used"]
pub struct FromFile<T>(T);

/// A value read from the file is over the bound it was checked against.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BoundError {
    /// The value read.
    pub value: u64,
    /// The bound it broke: a cap's value, or a length known to the reader.
    pub bound: u64,
}

impl fmt::Display for BoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} read from the file is over {}",
            self.value, self.bound
        )
    }
}

impl core::error::Error for BoundError {}

impl<T: FileInt> fmt::Debug for FromFile<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FromFile({:?})", self.0)
    }
}

/// `v` as a `u64`, saturating a `usize` wider than 64 bits.
fn wide(v: usize) -> u64 {
    u64::try_from(v).unwrap_or(u64::MAX)
}

impl<T: FileInt> FromFile<T> {
    /// Wrap a value a byte reader read.
    pub const fn new(value: T) -> Self {
        FromFile(value)
    }

    fn value(self) -> u64 {
        self.0.into()
    }

    /// The value as a `usize` when it is at most `cap`, a named cap of the format.
    pub fn at_most(self, cap: usize) -> Result<usize, BoundError> {
        let value = self.value();
        let bound = wide(cap);
        match usize::try_from(value) {
            Ok(v) if v <= cap => Ok(v),
            _ => Err(BoundError { value, bound }),
        }
    }

    /// The value as a `usize` when it is at most `len`, a length known from the bytes in hand.
    pub fn within(self, len: usize) -> Result<usize, BoundError> {
        self.at_most(len)
    }

    /// The value as a `usize` when it is under `len`: an index or an offset into something `len` long.
    pub fn below(self, len: usize) -> Result<usize, BoundError> {
        let value = self.value();
        let bound = wide(len);
        match usize::try_from(value) {
            Ok(v) if v < len => Ok(v),
            _ => Err(BoundError { value, bound }),
        }
    }

    /// The value as its own type when it is at most `cap`, a named cap of the format (a bit width, a shift).
    pub fn bounded(self, cap: T) -> Result<T, BoundError> {
        if self.0 <= cap {
            Ok(self.0)
        } else {
            Err(BoundError {
                value: self.value(),
                bound: cap.into(),
            })
        }
    }

    /// Whether the value is `known`, one the reader computed (a count the header repeats).
    pub fn is(self, known: T) -> bool {
        self.0 == known
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_only_through_a_bound() {
        let v = FromFile::new(5u32);
        assert_eq!(v.at_most(5), Ok(5));
        assert_eq!(v.at_most(4), Err(BoundError { value: 5, bound: 4 }));
        assert_eq!(v.within(9), Ok(5));
        assert_eq!(v.below(6), Ok(5));
        assert_eq!(v.below(5), Err(BoundError { value: 5, bound: 5 }));
        assert_eq!(v.bounded(7), Ok(5));
        assert_eq!(v.bounded(3), Err(BoundError { value: 5, bound: 3 }));
        assert!(v.is(5));
        assert!(!v.is(6));
        let big = FromFile::new(u64::MAX);
        assert!(big.at_most(usize::MAX).is_ok() == (usize::BITS >= 64));
        assert!(big.below(usize::MAX).is_err());
        assert_eq!(format!("{v:?}"), "FromFile(5)");
    }
}
