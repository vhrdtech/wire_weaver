//! Encoding a chunk's payloads into its two columns.

use super::{GROUP, LENGTHS, RECORD_LEN};
use crate::bitpack;
use crate::chunk::{Codec, ColumnKind, ColumnSpec};
use crate::lz4::{LZ4_DECODED_MAX, Lz4Error, lz4_encode, lz4_max_len};
use alloc::vec;
use alloc::vec::Vec;

/// Why payloads can't be encoded.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
            EncodeError::Lz4(e) => write!(f, "lz4: {e:?}"),
        }
    }
}

impl core::error::Error for EncodeError {}

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
            if lens.len() >= u32::MAX as usize {
                return Err(EncodeError::TooMany);
            }
            lens.push(u32::try_from(item.len()).map_err(|_| EncodeError::TooLong { len })?);
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
    let groups: Vec<(u32, u32)> = lens
        .chunks(GROUP)
        .map(|g| {
            let min = g.iter().copied().min().unwrap_or(0);
            let max = g.iter().copied().max().unwrap_or(0);
            (min, bitpack::width_of(u64::from(max - min)))
        })
        .collect();
    let records = groups.len() * RECORD_LEN;
    let packed: usize = lens
        .chunks(GROUP)
        .zip(&groups)
        .map(|(g, (_, w))| bitpack::packed_len(g.len(), *w))
        .sum();
    let mut out = vec![0u8; records + packed];
    let (mut base, mut offset) = (0u32, 0usize);
    for (i, (g, (min, width))) in lens.chunks(GROUP).zip(&groups).enumerate() {
        let r = i * RECORD_LEN;
        let offset32 = u32::try_from(offset).ok()?;
        out[r..r + 4].copy_from_slice(&base.to_le_bytes());
        out[r + 4..r + 8].copy_from_slice(&min.to_le_bytes());
        out[r + 8] = *width as u8;
        out[r + 9..r + 13].copy_from_slice(&offset32.to_le_bytes());
        offset += bitpack::pack(
            &mut out[records + offset..],
            g.iter().map(|l| u64::from(l - min)),
            *width,
        );
        for l in g {
            base = base.checked_add(*l)?;
        }
    }
    Some(out)
}
