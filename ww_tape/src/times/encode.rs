//! Encoding a chunk's times into its columns.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::unreachable,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "STD-131: hardening of this module in progress"
)]

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
    /// More items than a chunk holds (u32).
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
        if n == 0 {
            return Err(EncodeError::Empty);
        }
        if n > u32::MAX as usize {
            return Err(EncodeError::TooMany);
        }
        let count = n as u32;
        if let Some(at) = starts.windows(2).position(|w| w[1] < w[0]) {
            return Err(EncodeError::Unsorted { at: at + 1 });
        }
        if ends.is_some_and(|e| e.len() != n) || flags.is_some_and(|f| f.len() != n) {
            return Err(EncodeError::Length);
        }
        let (first, last) = (starts[0], starts[n - 1]);
        let (end_mode, latest_end, ends) = match ends {
            None => (EndMode::Absent, last, None),
            Some(e) => {
                if let Some(at) = starts.iter().zip(e).position(|(s, e)| e < s) {
                    return Err(EncodeError::EndBeforeStart { at });
                }
                let latest = e.iter().copied().max().expect("an item at least");
                if e == starts {
                    (EndMode::Absent, last, None)
                } else if e[..n - 1] == starts[1..] {
                    (EndMode::NextStart, latest, None)
                } else {
                    let d: Vec<u64> = starts
                        .iter()
                        .zip(e)
                        .map(|(s, e)| e.wrapping_sub(*s) as u64)
                        .collect();
                    (EndMode::Durations, latest, Some(encode_durations(&d)))
                }
            }
        };
        let flags = flags
            .filter(|f| f.iter().any(|x| *x != 0))
            .map(encode_flags);
        let (time_mode, starts) = starts::encode(starts);
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
