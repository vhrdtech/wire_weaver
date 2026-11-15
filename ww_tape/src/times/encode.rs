//! Encoding a chunk's times into its columns.

use super::packed::{encode_durations, encode_flags};
use super::{DURATIONS, FLAGS, STARTS, TimeHead, starts};
use crate::chunk::{Codec, ColumnKind, ColumnSpec, EndMode};
use std::vec::Vec;

/// Why times can't be encoded.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncodeError {
    /// No items.
    Empty,
    /// More items than a chunk holds ([ITEMS_MAX](super::ITEMS_MAX)).
    TooMany,
    /// Item `at` starts before the one before it.
    Unsorted { at: usize },
    /// Item `at` ends before it starts.
    EndBeforeStart { at: usize },
    /// Ends or flags given for another number of items than the starts.
    Length,
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EncodeError::Empty => write!(f, "no items"),
            EncodeError::TooMany => write!(f, "more items than a chunk holds"),
            EncodeError::Unsorted { at } => write!(f, "item {at} starts before the one before it"),
            EncodeError::EndBeforeStart { at } => write!(f, "item {at} ends before it starts"),
            EncodeError::Length => write!(f, "ends or flags for another number of items"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// A chunk's times encoded: the head fields and the time columns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedTimes {
    pub head: TimeHead,
    starts: Vec<u8>,
    ends: Option<Vec<u8>>,
    flags: Option<Vec<u8>>,
}

impl EncodedTimes {
    /// Encode ascending `starts`, with `ends` (None: every item an instant) and `flags` (None: all 0). The end mode
    /// is the smallest that holds the ends: [EndMode::Absent] when every end is its start, [EndMode::NextStart] when
    /// every end but the last is the next start, else [EndMode::Durations]. Flags that are all 0 take no column.
    pub fn encode(
        starts: &[i64],
        ends: Option<&[i64]>,
        flags: Option<&[u32]>,
    ) -> Result<Self, EncodeError> {
        let n = starts.len();
        let (Some(&first), Some(&last)) = (starts.first(), starts.last()) else {
            return Err(EncodeError::Empty);
        };
        let Some(count) = u32::try_from(n).ok().filter(|_| n <= super::ITEMS_MAX) else {
            return Err(EncodeError::TooMany);
        };
        if let Some(((at, _), _)) = starts
            .iter()
            .enumerate()
            .skip(1)
            .zip(starts)
            .find(|((_, s), before)| s < before)
        {
            return Err(EncodeError::Unsorted { at });
        }
        if ends.is_some_and(|e| e.len() != n) || flags.is_some_and(|f| f.len() != n) {
            return Err(EncodeError::Length);
        }
        let (end_mode, latest_end, ends) = match ends {
            None => (EndMode::Absent, last, None),
            Some(e) => {
                if let Some(at) = starts.iter().zip(e).position(|(s, e)| e < s) {
                    return Err(EncodeError::EndBeforeStart { at });
                }
                let latest = e.iter().copied().max().ok_or(EncodeError::Empty)?;
                if e == starts {
                    (EndMode::Absent, last, None)
                } else if e.split_last().map(|(_, init)| init)
                    == starts.split_first().map(|(_, rest)| rest)
                {
                    (EndMode::NextStart, latest, None)
                } else {
                    // ends are at or after their starts: the wrapping difference is the duration
                    let d: Vec<u64> = starts
                        .iter()
                        .zip(e)
                        .map(|(s, e)| e.wrapping_sub(*s).cast_unsigned())
                        .collect();
                    let column = encode_durations(&d).ok_or(EncodeError::TooMany)?;
                    (EndMode::Durations, latest, Some(column))
                }
            }
        };
        let flags = flags
            .filter(|f| f.iter().any(|x| *x != 0))
            .map(|f| encode_flags(f).ok_or(EncodeError::TooMany))
            .transpose()?;
        let (time_mode, starts) = starts::encode(starts).ok_or(EncodeError::TooMany)?;
        Ok(EncodedTimes {
            head: TimeHead {
                count,
                first_tick: first,
                last_tick: last,
                latest_end,
                time_mode,
                end_mode,
            },
            starts,
            ends,
            flags,
        })
    }

    /// The time columns with their bytes: starts, then ends and flags when present.
    pub fn columns(&self) -> impl Iterator<Item = (ColumnSpec, &[u8])> + '_ {
        core::iter::once(spec(ColumnKind::Starts, STARTS, &self.starts))
            .chain(
                self.ends
                    .as_ref()
                    .map(|b| spec(ColumnKind::Ends, DURATIONS, b)),
            )
            .chain(
                self.flags
                    .as_ref()
                    .map(|b| spec(ColumnKind::Flags, FLAGS, b)),
            )
    }
}

fn spec(kind: ColumnKind, codec: Codec, bytes: &[u8]) -> (ColumnSpec, &[u8]) {
    let spec = ColumnSpec {
        kind,
        codec,
        param: 0,
        len: bytes.len(),
    };
    (spec, bytes)
}
