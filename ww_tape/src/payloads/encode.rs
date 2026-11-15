//! Encoding a chunk's payloads into its two columns.

use super::{LENGTHS, RECORD_LEN};
use crate::bitpack;
use crate::bytes;
use crate::chunk::{Codec, ColumnKind, ColumnSpec};
use crate::lz4::{LZ4_DECODED_MAX, Lz4Error, lz4_encode, lz4_max_len};
use crate::times::encode_groups;
use alloc::vec;
use alloc::vec::Vec;

/// Why payloads can't be encoded.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncodeError {
    /// No items.
    Empty,
    /// More items than a chunk holds (u32).
    TooMany,
    /// The payloads add up to more than [LZ4_DECODED_MAX] bytes.
    TooLong { len: usize },
    /// The lz4 encoder refused the payloads.
    Lz4(Lz4Error),
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EncodeError::Empty => write!(f, "no items"),
            EncodeError::TooMany => write!(f, "more items than a chunk holds"),
            EncodeError::TooLong { len } => {
                write!(f, "{len} bytes of payloads, over {LZ4_DECODED_MAX}")
            }
            EncodeError::Lz4(_) => write!(f, "lz4 encoder refused the payloads"),
        }
    }
}

impl core::error::Error for EncodeError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            EncodeError::Lz4(e) => Some(e),
            _ => None,
        }
    }
}

/// A chunk's payloads encoded: the lengths column and the payloads column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedPayloads {
    lengths: Vec<u8>,
    payloads: Vec<u8>,
    codec: Codec,
}

impl EncodedPayloads {
    /// Encode the payloads of a chunk's items, in item order. The payloads column is [Codec::LZ4] when that is
    /// smaller than the bytes as they are, else [Codec::RAW]. The same payloads give the same bytes (lz4: see
    /// [crate::lz4]).
    pub fn encode<'a>(items: impl IntoIterator<Item = &'a [u8]>) -> Result<Self, EncodeError> {
        let mut lens: Vec<u32> = Vec::new();
        let mut raw: Vec<u8> = Vec::new();
        for item in items {
            let len = raw.len().saturating_add(item.len());
            if len > LZ4_DECODED_MAX {
                return Err(EncodeError::TooLong { len });
            }
            // the count stays a u32
            if !u32::try_from(lens.len()).is_ok_and(|n| n < u32::MAX) {
                return Err(EncodeError::TooMany);
            }
            // at most the running total, at most LZ4_DECODED_MAX
            let Ok(item_len) = u32::try_from(item.len()) else {
                return Err(EncodeError::TooLong { len });
            };
            lens.push(item_len);
            raw.extend_from_slice(item);
        }
        if lens.is_empty() {
            return Err(EncodeError::Empty);
        }
        let lengths = encode_lengths(&lens).ok_or(EncodeError::TooLong { len: raw.len() })?;
        let mut lz4 = vec![0u8; lz4_max_len(raw.len())];
        let n = lz4_encode(&raw, &mut lz4).map_err(EncodeError::Lz4)?;
        let (payloads, codec) = if n < raw.len() {
            lz4.truncate(n);
            (lz4, Codec::LZ4)
        } else {
            (raw, Codec::RAW)
        };
        Ok(EncodedPayloads {
            lengths,
            payloads,
            codec,
        })
    }

    /// The codec of the payloads column: [Codec::RAW] or [Codec::LZ4].
    pub fn codec(&self) -> Codec {
        self.codec
    }

    /// The two columns with their bytes: lengths, then payloads.
    pub fn columns(&self) -> [(ColumnSpec, &[u8]); 2] {
        let spec = |kind, codec, bytes: &[u8]| ColumnSpec {
            kind,
            codec,
            param: 0,
            len: bytes.len(),
        };
        [
            (
                spec(ColumnKind::PayloadLengths, LENGTHS, &self.lengths),
                &self.lengths,
            ),
            (
                spec(ColumnKind::Payloads, self.codec, &self.payloads),
                &self.payloads,
            ),
        ]
    }
}

/// The lengths column of `lens`; None when the payloads or the packed lengths don't fit u32 offsets.
fn encode_lengths(lens: &[u32]) -> Option<Vec<u8>> {
    let mut base = 0u32;
    encode_groups(lens, |g, offset| {
        let min = g.iter().copied().fold(u32::MAX, u32::min);
        let max = g.iter().copied().fold(0, u32::max);
        let width = bitpack::width_of(u64::from(max.abs_diff(min)));
        let mut rec = [0u8; RECORD_LEN];
        bytes::put_u32(&mut rec, 0, base)?;
        bytes::put_u32(&mut rec, 4, min)?;
        bytes::put(&mut rec, 8, [u8::try_from(width).ok()?])?;
        bytes::put_u32(&mut rec, 9, offset)?;
        for l in g {
            base = base.checked_add(*l)?;
        }
        // every length is at least the minimum: the difference is exact
        let vals = g.iter().map(|l| u64::from(l.wrapping_sub(min))).collect();
        Some((rec, vals, width))
    })
}
