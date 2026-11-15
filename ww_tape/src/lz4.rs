//! The lz4 payload codec ([Codec::LZ4]): a column's bytes as one lz4 block, through the pure-Rust `lz4_flex`.
//!
//! ```text
//! u32   decoded length, at most LZ4_DECODED_MAX
//! ...   one lz4 block (no frame, no checksum: the chunk's CRCs check it)
//! ```
//!
//! A writer picks it per chunk for a payload column ([ColumnKind::Payloads](crate::ColumnKind::Payloads)) when it
//! makes the column smaller; the payload lengths column stays as it is, so item `i`'s payload is found in the decoded
//! bytes.
//!
//! Encoding is deterministic only for one `lz4_flex` version on targets of the same pointer width and endianness:
//! `lz4_flex` hashes 5 bytes on 64-bit targets and 4 on 32-bit ones and reads words in native byte order, so a column
//! of 65535 bytes or more can compress to other bytes on a 32-bit or big-endian writer, and a writer's "lz4 when
//! smaller" choice can differ with it. The workspace pins `lz4_flex` to an exact version for that reason; the golden
//! file checks it. Decoding is the same everywhere.

use crate::Codec;

impl Codec {
    /// The column's bytes as one lz4 block after their decoded length.
    pub const LZ4: Codec = Codec(48);
}

/// Bytes of the decoded length in front of the block.
const LEN_BYTES: usize = 4;

/// Longest decoded column a reader takes: a whole block's worth, [MAX_BLOCK_LEN](crate::MAX_BLOCK_LEN).
pub const LZ4_DECODED_MAX: usize = crate::MAX_BLOCK_LEN;

/// Most bytes one byte of an lz4 block decodes to: a match token and its length bytes add 255 bytes per byte.
const EXPANSION_MAX: usize = 255;
/// Bytes a block of nothing but literals can hold past [EXPANSION_MAX] times its length.
const EXPANSION_SLACK: usize = 16;

/// Why an lz4 column can't be read or written.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Lz4Error {
    /// The column is shorter than its length field.
    Short,
    /// The decoded length is over [LZ4_DECODED_MAX], or over what the column's block can decode to (255 times its
    /// bytes and 16).
    TooLong { len: usize },
    /// The output buffer is shorter than needed ([lz4_max_len] to encode, [lz4_decoded_len] to decode).
    Full { needed: usize },
    /// The block is malformed, or decodes to another length than the column says.
    Corrupt,
}

/// The most bytes [lz4_encode] writes for `len` bytes.
pub const fn lz4_max_len(len: usize) -> usize {
    LEN_BYTES + lz4_flex::block::get_maximum_output_size(len)
}

/// Code `raw` into `out` (at least [lz4_max_len] bytes); returns the bytes written.
pub fn lz4_encode(raw: &[u8], out: &mut [u8]) -> Result<usize, Lz4Error> {
    if raw.len() > LZ4_DECODED_MAX {
        return Err(Lz4Error::TooLong { len: raw.len() });
    }
    let needed = lz4_max_len(raw.len());
    if out.len() < needed {
        return Err(Lz4Error::Full { needed });
    }
    let len = u32::try_from(raw.len()).expect("LZ4_DECODED_MAX fits a u32");
    out[..LEN_BYTES].copy_from_slice(&len.to_le_bytes());
    let n = lz4_flex::block::compress_into(raw, &mut out[LEN_BYTES..needed])
        .expect("the output holds lz4_flex's maximum output size");
    Ok(LEN_BYTES + n)
}

/// The decoded length an lz4 column states, checked against [LZ4_DECODED_MAX] and against the block's length, so
/// a short column can't make a reader allocate more than its block can fill.
pub fn lz4_decoded_len(column: &[u8]) -> Result<usize, Lz4Error> {
    let len: [u8; LEN_BYTES] = column
        .get(..LEN_BYTES)
        .ok_or(Lz4Error::Short)?
        .try_into()
        .expect("4 bytes");
    let len = u32::from_le_bytes(len) as usize;
    let block = column.len() - LEN_BYTES;
    if len > LZ4_DECODED_MAX || len > EXPANSION_MAX * block + EXPANSION_SLACK {
        return Err(Lz4Error::TooLong { len });
    }
    Ok(len)
}

/// Decode an lz4 column into `out` (at least [lz4_decoded_len] bytes); returns the decoded length.
pub fn lz4_decode(column: &[u8], out: &mut [u8]) -> Result<usize, Lz4Error> {
    let len = lz4_decoded_len(column)?;
    if out.len() < len {
        return Err(Lz4Error::Full { needed: len });
    }
    match lz4_flex::block::decompress_into(&column[LEN_BYTES..], &mut out[..len]) {
        Ok(n) if n == len => Ok(len),
        _ => Err(Lz4Error::Corrupt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(raw: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; lz4_max_len(raw.len())];
        let n = lz4_encode(raw, &mut out).unwrap();
        out.truncate(n);
        let mut back = vec![0u8; lz4_decoded_len(&out).unwrap()];
        assert_eq!(lz4_decode(&out, &mut back), Ok(raw.len()));
        assert_eq!(back, raw);
        out
    }

    #[test]
    fn round_trips_and_shrinks_repeats() {
        assert_eq!(round(&[]), [0, 0, 0, 0, 0]);
        round(b"x");
        let text: Vec<u8> = b"frame ok; ".iter().copied().cycle().take(4000).collect();
        assert!(round(&text).len() < 100);
        let mut x = 1u32;
        let noise: Vec<u8> = (0..5000)
            .map(|_| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (x >> 16) as u8
            })
            .collect();
        round(&noise);
    }

    #[test]
    fn rejects_bad_columns() {
        assert_eq!(lz4_decoded_len(&[1, 2]), Err(Lz4Error::Short));
        // a 5-byte column can't ask for 64 MiB, nor for more than its one block byte decodes to
        let max = (LZ4_DECODED_MAX as u32).to_le_bytes();
        assert_eq!(
            lz4_decoded_len(&[max[0], max[1], max[2], max[3], 0]),
            Err(Lz4Error::TooLong {
                len: LZ4_DECODED_MAX
            })
        );
        assert_eq!(lz4_decoded_len(&[15, 1, 0, 0, 0]), Ok(271));
        assert_eq!(
            lz4_decoded_len(&[16, 1, 0, 0, 0]),
            Err(Lz4Error::TooLong { len: 272 })
        );
        // the most compressible input stays within the bound
        let zeros = vec![0u8; 1 << 20];
        let col = round(&zeros);
        assert!(zeros.len() <= EXPANSION_MAX * (col.len() - LEN_BYTES) + EXPANSION_SLACK);
        assert_eq!(
            lz4_decoded_len(&[0, 0, 0, 0x10]),
            Err(Lz4Error::TooLong { len: 0x1000_0000 })
        );
        let col = round(b"hello hello hello hello");
        let mut out = [0u8; 64];
        // a length that disagrees with the block
        let mut wrong = col.clone();
        wrong[0] += 1;
        assert_eq!(lz4_decode(&wrong, &mut out), Err(Lz4Error::Corrupt));
        // a cut block
        assert_eq!(
            lz4_decode(&col[..col.len() - 2], &mut out),
            Err(Lz4Error::Corrupt)
        );
        assert_eq!(
            lz4_decode(&col, &mut out[..3]),
            Err(Lz4Error::Full { needed: 23 })
        );
    }
}
