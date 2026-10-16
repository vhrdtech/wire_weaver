//! Runs: discrete values as the first value and one entry per change.
//!
//! ```text
//! first:      value of the first item
//! repeat:     1, gap: nib, value      a change: gap = items since the previous change (or the start) minus 1;
//!                                     value left out when two-valued (a change flips it)
//! end:        0
//! ```
//!
//! Values: an enum's discriminant as `nib`, a bool as 1 bit, opaque bytes as `len: nib`, zero bits to the next
//! byte, then the bytes. Two-valued are only `bool` and `Option`'s tag, which can never gain a value: an enum with
//! two variants may gain a third one, so it uses the general form. A change that would land at or past the end of
//! the column is malformed.

use super::{get_nib, put_nib};
use crate::series::BitSink;
use crate::{BufReader, Error};

/// What the values of a run of numbers are and how they are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub(crate) enum RunKind {
    /// An enum's discriminant, as `nib`, at every change.
    Tag,
    /// `Option`'s tag, 0 (`None`) or 1 (`Some`): the first as `nib`, a change flips it.
    OptionTag,
    /// A bool as 0 or 1: the first as 1 bit, a change flips it.
    Bool,
}

/// Writes the changes of `items`, the first value with `put_first`, each changed one with `put_change`.
fn write_changes<V, S>(
    sink: &mut S,
    mut items: impl Iterator<Item = V>,
    mut put_first: impl FnMut(&mut S, V) -> Result<(), Error>,
    mut put_change: impl FnMut(&mut S, V) -> Result<(), Error>,
) -> Result<(), Error>
where
    V: Copy + PartialEq,
    S: BitSink,
{
    let Some(mut prev) = items.next() else {
        return Ok(());
    };
    put_first(sink, prev)?;
    // items since the previous change (or the start), minus 1
    let mut gap = 0usize;
    for value in items {
        if value != prev {
            sink.put_bit(true)?;
            put_nib(sink, u32::try_from(gap).map_err(|_| Error::LenTooLong)?)?;
            put_change(sink, value)?;
            prev = value;
            gap = 0;
        } else {
            gap += 1;
        }
    }
    sink.put_bit(false)
}

/// Writes a run of tags or bools (0 / 1 for [RunKind::Bool] and [RunKind::OptionTag], else `SubtypeOutOfRange`).
/// Writes nothing for no items: a `Series` doesn't write empty columns at all.
pub(crate) fn write_run(
    sink: &mut impl BitSink,
    kind: RunKind,
    items: impl Iterator<Item = u32>,
) -> Result<(), Error> {
    let check = |v: u32| {
        if kind != RunKind::Tag && v > 1 {
            Err(Error::SubtypeOutOfRange)
        } else {
            Ok(())
        }
    };
    write_changes(
        sink,
        items,
        |sink, v| {
            check(v)?;
            match kind {
                RunKind::Bool => sink.put_bit(v == 1),
                _ => put_nib(sink, v),
            }
        },
        |sink, v| {
            check(v)?;
            match kind {
                RunKind::Tag => put_nib(sink, v),
                _ => Ok(()),
            }
        },
    )
}

/// Writes a run of opaque byte strings (`len` up to `u32::MAX`, else `LenTooLong`). Writes nothing for no items.
pub(crate) fn write_opaque_run<'a, S: BitSink>(
    sink: &mut S,
    items: impl Iterator<Item = &'a [u8]>,
) -> Result<(), Error> {
    let put = |sink: &mut S, bytes: &[u8]| {
        put_nib(
            sink,
            u32::try_from(bytes.len()).map_err(|_| Error::LenTooLong)?,
        )?;
        sink.put_bytes(bytes)
    };
    write_changes(sink, items, put, put)
}

/// How a [RunReader] reads the values of its run.
pub(crate) trait RunValues<'i>: Copy {
    type Value: Copy;
    fn first(self, rd: &mut BufReader<'i>) -> Result<Self::Value, Error>;
    /// The value after a change from `cur`.
    fn change(self, rd: &mut BufReader<'i>, cur: Self::Value) -> Result<Self::Value, Error>;
}

impl<'i> RunValues<'i> for RunKind {
    type Value = u32;

    fn first(self, rd: &mut BufReader<'i>) -> Result<u32, Error> {
        match self {
            RunKind::Tag => get_nib(rd),
            RunKind::OptionTag => match get_nib(rd)? {
                v @ (0 | 1) => Ok(v),
                _ => Err(Error::MalformedSeries),
            },
            RunKind::Bool => Ok(rd.read_bool()? as u32),
        }
    }

    fn change(self, rd: &mut BufReader<'i>, cur: u32) -> Result<u32, Error> {
        match self {
            RunKind::Tag => get_nib(rd),
            _ => Ok(cur ^ 1),
        }
    }
}

/// Opaque run values: byte strings, borrowed from the buffer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Opaque;

impl<'i> RunValues<'i> for Opaque {
    type Value = &'i [u8];

    fn first(self, rd: &mut BufReader<'i>) -> Result<&'i [u8], Error> {
        let len = get_nib(rd)? as usize;
        // the padding to the byte is zero bits: anything else is not from a writer
        let (_, bit) = rd.pos();
        if bit != 7 && rd.read_un32(bit + 1)? != 0 {
            return Err(Error::MalformedSeries);
        }
        rd.read_raw_slice(len)
    }

    fn change(self, rd: &mut BufReader<'i>, _cur: &'i [u8]) -> Result<&'i [u8], Error> {
        self.first(rd)
    }
}

/// Reader of a run over `m` items, stepped one item at a time; reads each change when the item before it is
/// stepped, so it never looks further than one change ahead.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RunReader<'i, K: RunValues<'i>> {
    rd: BufReader<'i>,
    values: K,
    m: usize,
    /// Items stepped so far.
    pos: usize,
    cur: K::Value,
    /// Item index of the next change (`m`: none) and its value.
    next_change: usize,
    next_value: K::Value,
}

impl<'i, K: RunValues<'i>> RunReader<'i, K> {
    /// Reads the first value and the first change of a run over `m` items, `m` at least 1 (0 is
    /// `MalformedSeries`: no column is written for no items).
    pub(crate) fn new(mut rd: BufReader<'i>, values: K, m: usize) -> Result<Self, Error> {
        if m == 0 {
            return Err(Error::MalformedSeries);
        }
        let cur = values.first(&mut rd)?;
        let mut run = RunReader {
            rd,
            values,
            m,
            pos: 0,
            cur,
            next_change: m,
            next_value: cur,
        };
        run.read_change(0)?;
        Ok(run)
    }

    /// Reads the entry after a change at item `at` (0 for the start): the end bit, or the next change.
    fn read_change(&mut self, at: usize) -> Result<(), Error> {
        if !self.rd.read_bool()? {
            self.next_change = self.m;
            return Ok(());
        }
        let gap = get_nib(&mut self.rd)? as usize;
        let next = at
            .checked_add(gap)
            .and_then(|n| n.checked_add(1))
            .filter(|&n| n < self.m)
            .ok_or(Error::MalformedSeries)?;
        self.next_value = self.values.change(&mut self.rd, self.cur)?;
        self.next_change = next;
        Ok(())
    }

    /// The next item's value; past the `m` items `MalformedSeries`.
    pub(crate) fn next(&mut self) -> Result<K::Value, Error> {
        if self.pos >= self.m {
            return Err(Error::MalformedSeries);
        }
        if self.pos == self.next_change {
            self.cur = self.next_value;
            self.read_change(self.pos)?;
        }
        self.pos += 1;
        Ok(self.cur)
    }

    /// Whether the next item starts a new value (false for the first item): read before [next](Self::next).
    pub(crate) fn changes_next(&self) -> bool {
        self.pos == self.next_change
    }

    /// Reads the rest of the run without stepping items (work bounded by its changes, fewer than `m`), and returns
    /// the reader positioned after it, where the next column starts.
    pub(crate) fn finish(mut self) -> Result<BufReader<'i>, Error> {
        while self.next_change < self.m {
            self.cur = self.next_value;
            self.pos = self.next_change;
            self.read_change(self.pos)?;
        }
        Ok(self.rd)
    }
}
