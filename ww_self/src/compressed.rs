//! Compressed introspection data (INT-16): a serialized [ApiBundle](crate::ApiBundle) in LZ4 blocks, for devices
//! that keep their introspection data compressed in flash (`ww_codegen!(.., introspect_compression = "lz4")`).
//!
//! Layout, all integers little-endian:
//!
//! | bytes | what                                                                                  |
//! |-------|---------------------------------------------------------------------------------------|
//! | 4     | [MAGIC], `WWz4`: never the first bytes of a bundle, which starts with [crate::MAGIC]   |
//! | 4     | length of the bundle bytes once decompressed                                          |
//! | 4 + n | per block: its length `n`, then a standard LZ4 block of `n` bytes                      |
//!
//! Each block may reach back into the blocks before it (up to 64 KiB), as [ww_lz4] streams do; a bundle up to
//! [CHUNK] bytes is one block. The API hashes (`ApiHashPair`) are those of the decompressed bytes: compression
//! doesn't change an API's identity.

use std::borrow::Cow;
use std::vec::Vec;

/// First bytes of compressed introspection data.
pub const MAGIC: [u8; 4] = *b"WWz4";

/// Bytes of the bundle compressed into one block, the rest of the encoder's buffer is history for the next one.
pub const CHUNK: usize = 32 * 1024;

/// Largest decompressed bundle accepted from a device, so a corrupt length can't make a host allocate gigabytes.
pub const MAX_LEN: usize = 16 * 1024 * 1024;

const HEADER: usize = 8;

/// Introspection data that doesn't decompress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Shorter than its header or a block length says.
    Truncated,
    /// The declared length is over [MAX_LEN].
    TooLong(usize),
    /// A block is not valid LZ4.
    Corrupt,
    /// The blocks decompressed to another length than the header declares.
    LengthMismatch { declared: usize, actual: usize },
    /// Neither a bundle ([crate::MAGIC]) nor compressed ([MAGIC]): another format, likely a newer one.
    UnknownFormat([u8; 4]),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Truncated => f.write_str("compressed introspection data is truncated"),
            Error::TooLong(n) => write!(
                f,
                "compressed introspection data declares {n} bytes, more than {MAX_LEN}"
            ),
            Error::Corrupt => f.write_str("compressed introspection data has a corrupt LZ4 block"),
            Error::LengthMismatch { declared, actual } => write!(
                f,
                "compressed introspection data decompressed to {actual} bytes, its header says {declared}"
            ),
            Error::UnknownFormat(m) => write!(
                f,
                "introspection data starts with {m:02x?}: neither a ww_self bundle nor a compressed one, likely a \
                 newer format than this host reads"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Compresses the bytes of a serialized bundle (build time, see the module docs for the layout).
pub fn compress(bundle_bytes: &[u8]) -> Vec<u8> {
    type Enc = ww_lz4::Encoder<{ 1 << 16 }, { 1 << 16 }>;
    // a host at build time: 192 KiB in a Box, and no backing off, every chunk is tried
    let mut enc = Box::new(Enc::new((1 << 16) - CHUNK).with_backoff(0, 0));
    let mut out = Vec::with_capacity(HEADER + bundle_bytes.len() / 2);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&(bundle_bytes.len() as u32).to_le_bytes());
    let mut block = vec![0u8; ww_lz4::max_block_len(CHUNK)];
    for chunk in bundle_bytes.chunks(CHUNK) {
        let n = enc
            .compress_slice(chunk, &mut block)
            .expect("a chunk fits the encoder and its output buffer");
        out.extend_from_slice(&(n as u32).to_le_bytes());
        out.extend_from_slice(&block[..n]);
    }
    out
}

/// Whether `bytes` is compressed introspection data rather than a plain bundle.
pub fn is_compressed(bytes: &[u8]) -> bool {
    bytes.starts_with(&MAGIC)
}

/// The bundle bytes in compressed introspection data.
pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let header = bytes.get(..HEADER).ok_or(Error::Truncated)?;
    let declared = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    if declared > MAX_LEN {
        return Err(Error::TooLong(declared));
    }
    let mut rest = &bytes[HEADER..];
    let mut decoder = ww_lz4::Decoder::new();
    let mut out = Vec::with_capacity(declared);
    while !rest.is_empty() {
        let n = rest.get(..4).ok_or(Error::Truncated)?;
        let n = u32::from_le_bytes(n.try_into().unwrap()) as usize;
        let block = rest.get(4..4 + n).ok_or(Error::Truncated)?;
        let decoded = decoder.decode(block).map_err(|_| Error::Corrupt)?;
        if out.len() + decoded.len() > declared {
            return Err(Error::LengthMismatch {
                declared,
                actual: out.len() + decoded.len(),
            });
        }
        out.extend_from_slice(decoded);
        rest = &rest[4 + n..];
    }
    if out.len() != declared {
        return Err(Error::LengthMismatch {
            declared,
            actual: out.len(),
        });
    }
    Ok(out)
}

/// The bundle bytes of introspection data as a device sends it: decompressed when compressed, as is when they start
/// with [crate::MAGIC]. Anything else is [Error::UnknownFormat]: the first bytes are what tells the formats apart, so
/// a format added later fails here with a clear error instead of being read as a bundle.
pub fn bundle_bytes(bytes: &[u8]) -> Result<Cow<'_, [u8]>, Error> {
    if is_compressed(bytes) {
        return decompress(bytes).map(Cow::Owned);
    }
    match bytes.first_chunk::<4>() {
        Some(m) if *m == crate::MAGIC.to_le_bytes() => Ok(Cow::Borrowed(bytes)),
        Some(m) => Err(Error::UnknownFormat(*m)),
        None => Err(Error::Truncated),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(len: usize) -> Vec<u8> {
        let words = [
            "property", "stream", "method", "voltage", "Fixed", "u32", "docs",
        ];
        let mut out = Vec::new();
        let mut i = 0usize;
        while out.len() < len {
            out.extend_from_slice(words[i % words.len()].as_bytes());
            out.push(b' ' + (i % 7) as u8);
            i = i.wrapping_mul(31).wrapping_add(7);
        }
        out.truncate(len);
        out
    }

    #[test]
    fn round_trip_one_and_several_blocks() {
        for len in [0, 1, 100, CHUNK, CHUNK + 1, 3 * CHUNK + 17] {
            let plain = text(len);
            let packed = compress(&plain);
            assert!(is_compressed(&packed));
            assert_eq!(decompress(&packed).unwrap(), plain, "len {len}");
            if len > 1000 {
                assert!(
                    packed.len() < plain.len() / 2,
                    "len {len}: {}",
                    packed.len()
                );
            }
        }
    }

    /// The layout is frozen: these bytes (magic, length 37, one 21-byte block) decode the same in every version.
    #[test]
    fn frozen_layout() {
        let golden = "57577a342500000015000000bf696e74726f7370656374200b0002502064617461";
        let bytes: Vec<u8> = (0..golden.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&golden[i..i + 2], 16).unwrap())
            .collect();
        assert_eq!(
            decompress(&bytes).unwrap(),
            b"introspect introspect introspect data"
        );
    }

    #[test]
    fn plain_bundle_passes_through() {
        let plain = crate::MAGIC.to_le_bytes();
        assert!(!is_compressed(&plain));
        assert_eq!(bundle_bytes(&plain).unwrap().as_ref(), &plain);
        assert_eq!(
            bundle_bytes(b"WWzd...."),
            Err(Error::UnknownFormat(*b"WWzd"))
        );
        assert_eq!(bundle_bytes(&[0xF0, 0x14]), Err(Error::Truncated));
    }

    #[test]
    fn corrupt_data_is_an_error() {
        let packed = compress(&text(5000));
        assert_eq!(decompress(&packed[..6]), Err(Error::Truncated));
        assert_eq!(
            decompress(&packed[..packed.len() - 1]),
            Err(Error::Truncated)
        );
        let mut wrong_len = packed.clone();
        wrong_len[4] ^= 1;
        assert!(matches!(
            decompress(&wrong_len),
            Err(Error::LengthMismatch { .. })
        ));
        let mut huge = packed.clone();
        huge[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(decompress(&huge), Err(Error::TooLong(_))));
        let mut bad_block = packed;
        bad_block[12] = 0xFF;
        bad_block.truncate(14);
        bad_block[8..12].copy_from_slice(&2u32.to_le_bytes());
        assert!(decompress(&bad_block).is_err());
    }
}
