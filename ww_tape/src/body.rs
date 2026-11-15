//! Coded bodies of records and describe blocks ([crate::FLAG_BODY_CODED]).
//!
//! ```text
//! u8    codec (BodyCodec): 0 stored, 1 lz4 block
//! uvar  decoded length: the body as it would be stored, at most MAX_BLOCK_LEN - 36
//! ...   coded bytes: stored, the body itself; lz4, one lz4 block (no frame, no checksum)
//! ```
//!
//! The block CRC covers the coded bytes, so damage is found before decoding; the header's record count stays the
//! record count. A writer keeps the plain body when coding does not shrink it. A reader that doesn't know the codec
//! takes the block as one whose body it doesn't know ([Block::body_known] false). Chunk blocks are never coded: their
//! columns are read in place.

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

use crate::{
    Block, BlockError, BlockKind, CRC, CRC_LEN, FLAG_BODY_CODED, HEADER_LEN, MAX_BLOCK_LEN, varint,
};

/// The codec of a coded body, the byte after the header.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct BodyCodec(pub u8);

impl BodyCodec {
    /// The body itself after its length: records read in place.
    pub const STORED: BodyCodec = BodyCodec(0);
    /// One lz4 block (`lz4_flex`'s block format), decoded with [Block::decode].
    pub const LZ4: BodyCodec = BodyCodec(1);

    /// Whether this reader decodes it.
    pub const fn is_known(self) -> bool {
        matches!(self, Self::STORED | Self::LZ4)
    }
}

/// Longest decoded body: what a block of [MAX_BLOCK_LEN] holds.
pub const DECODED_BODY_MAX: usize = MAX_BLOCK_LEN - HEADER_LEN - CRC_LEN;

/// Most bytes one lz4 byte decodes to (a match token and its length bytes), and the slack a block of literals adds.
const LZ4_EXPANSION_MAX: usize = 255;
const LZ4_EXPANSION_SLACK: usize = 16;

/// The parts of a coded body, its lengths checked against the codec.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Coded<'b> {
    pub(crate) codec: BodyCodec,
    #[cfg_attr(
        not(feature = "alloc"),
        expect(dead_code, reason = "only decoding reads it, which needs alloc")
    )]
    pub(crate) decoded_len: usize,
    pub(crate) bytes: &'b [u8],
}

/// The parts of the coded body `body` (between header and CRC). None when it doesn't parse, or its decoded length
/// is over [DECODED_BODY_MAX] or over what a known codec's bytes can hold.
pub(crate) fn coded(body: &[u8]) -> Option<Coded<'_>> {
    let (&codec, rest) = body.split_first()?;
    let mut rd = varint::Reader(rest);
    let decoded_len = usize::try_from(rd.uvar()?).ok()?;
    let bytes = rd.0;
    let codec = BodyCodec(codec);
    let fits = match codec {
        BodyCodec::STORED => decoded_len == bytes.len(),
        BodyCodec::LZ4 => {
            decoded_len
                <= LZ4_EXPANSION_MAX
                    .saturating_mul(bytes.len())
                    .saturating_add(LZ4_EXPANSION_SLACK)
        }
        _ => true,
    };
    (fits && decoded_len <= DECODED_BODY_MAX).then_some(Coded {
        codec,
        decoded_len,
        bytes,
    })
}

/// Why a block's body can't be coded.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodeError {
    /// The input is not one good block.
    Block(BlockError),
    /// Only records and describe blocks of this version, not yet coded, are coded.
    NotRecords,
    /// A codec this writer doesn't have.
    UnknownCodec(BodyCodec),
    /// `out` is shorter than [coded_len_max] of the block.
    Full { needed: usize },
}

/// The most bytes [code_body] writes for a block of `len` bytes.
pub const fn coded_len_max(len: usize) -> usize {
    len + 1 + 10 + lz4_flex::block::get_maximum_output_size(len)
}

/// Write the records or describe block `block` with its body coded by `codec` into `out` (at least
/// [coded_len_max] bytes); returns the bytes written. When lz4 does not shrink the block, the block is written as
/// it is, not coded. The run, seq, other flags and record count stay.
pub fn code_body(block: &[u8], codec: BodyCodec, out: &mut [u8]) -> Result<usize, CodeError> {
    let b = Block::read(block).map_err(CodeError::Block)?;
    if b.len() != block.len() {
        return Err(CodeError::Block(BlockError::BadLength(block.len() as u32)));
    }
    let records = matches!(b.kind, BlockKind::Records | BlockKind::Describe);
    if !(records && b.body_known() && b.body_codec().is_none()) {
        return Err(CodeError::NotRecords);
    }
    if !codec.is_known() {
        return Err(CodeError::UnknownCodec(codec));
    }
    let needed = coded_len_max(block.len());
    if out.len() < needed {
        return Err(CodeError::Full { needed });
    }
    let body = &block[HEADER_LEN..block.len() - CRC_LEN];
    out[..HEADER_LEN].copy_from_slice(&block[..HEADER_LEN]);
    let mut pos = HEADER_LEN;
    out[pos] = codec.0;
    pos += 1;
    pos += varint::put_uvar(&mut out[pos..], body.len() as u64);
    pos += if codec == BodyCodec::LZ4 {
        lz4_flex::block::compress_into(body, &mut out[pos..needed])
            .expect("the output holds lz4_flex's maximum output size")
    } else {
        out[pos..pos + body.len()].copy_from_slice(body);
        body.len()
    };
    if codec == BodyCodec::LZ4 && pos + CRC_LEN >= block.len() {
        out[..block.len()].copy_from_slice(block);
        return Ok(block.len());
    }
    let len = pos + CRC_LEN;
    let flags = b.flags | FLAG_BODY_CODED;
    out[6..8].copy_from_slice(&flags.to_le_bytes());
    out[8..12].copy_from_slice(&(len as u32).to_le_bytes());
    let crc = CRC.checksum(&out[..pos]);
    out[pos..len].copy_from_slice(&crc.to_le_bytes());
    Ok(len)
}

/// The block `b` (coded with [BodyCodec::LZ4]) decoded into `buf`, in plain form: flag cleared, length and CRC new.
#[cfg(feature = "alloc")]
pub(crate) fn decode_into<'d>(
    b: &Block<'_>,
    c: &Coded<'_>,
    buf: &'d mut alloc::vec::Vec<u8>,
) -> Result<Block<'d>, BlockError> {
    let len = HEADER_LEN + c.decoded_len + CRC_LEN;
    buf.clear();
    buf.resize(len, 0);
    buf[..HEADER_LEN].copy_from_slice(&b.bytes()[..HEADER_LEN]);
    let flags = b.flags & !FLAG_BODY_CODED;
    buf[6..8].copy_from_slice(&flags.to_le_bytes());
    buf[8..12].copy_from_slice(&(len as u32).to_le_bytes());
    let end = len - CRC_LEN;
    match lz4_flex::block::decompress_into(c.bytes, &mut buf[HEADER_LEN..end]) {
        Ok(n) if n == c.decoded_len => {}
        _ => return Err(BlockError::BadRecords),
    }
    let crc = CRC.checksum(&buf[..end]);
    buf[end..].copy_from_slice(&crc.to_le_bytes());
    let buf: &'d [u8] = buf;
    Block::read(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::id;
    use crate::{BlockWriter, ClockTime, FLAG_COPY, Record, mark_copy};

    /// A records block of `n` alike records.
    fn records_block(n: u32) -> Vec<u8> {
        let mut buf = vec![0u8; 4096];
        let mut w = BlockWriter::new(&mut buf, BlockKind::Records, id(3), &[1]).unwrap();
        for i in 0..n {
            w.push(
                5,
                ClockTime::new(1, i64::from(i) * 10),
                &[b"frame ok; frame ok"],
            )
            .unwrap();
        }
        let len = w.finish();
        buf.truncate(len);
        buf
    }

    fn code(block: &[u8], codec: BodyCodec) -> Vec<u8> {
        let mut out = vec![0u8; coded_len_max(block.len())];
        let n = code_body(block, codec, &mut out).unwrap();
        out.truncate(n);
        out
    }

    fn records(b: &Block<'_>) -> Vec<Record<'static>> {
        b.records()
            .map(|r| {
                let r = r.unwrap();
                Record {
                    item: r.item.to_vec().leak(),
                    ..r
                }
            })
            .collect()
    }

    #[test]
    fn an_lz4_body_decodes_to_the_same_records() {
        let plain = records_block(40);
        let want = records(&Block::read(&plain).unwrap());
        let coded = code(&plain, BodyCodec::LZ4);
        assert!(coded.len() < plain.len() / 4, "{} bytes", coded.len());
        let b = Block::read(&coded).unwrap();
        assert_eq!(
            (
                b.body_codec(),
                b.body_known(),
                b.flags,
                b.record_count,
                b.seq
            ),
            (Some(BodyCodec::LZ4), true, FLAG_BODY_CODED, 40, 3)
        );
        // records aren't read in place from a coded body: the reader is told so, not handed none
        assert_eq!(b.records().collect::<Vec<_>>(), [Err(BlockError::Coded)]);
        let mut buf = Vec::new();
        let d = b.decode(&mut buf).unwrap();
        assert_eq!((d.body_codec(), d.flags, d.len()), (None, 0, plain.len()));
        assert_eq!(records(&d), want);
        // a copy mark set later keeps the body coded
        let mut copy = coded.clone();
        mark_copy(&mut copy, id(9)).unwrap();
        let c = Block::read(&copy).unwrap();
        assert_eq!(c.flags, FLAG_BODY_CODED | FLAG_COPY);
        assert_eq!(records(&c.decode(&mut buf).unwrap()), want);
    }

    #[test]
    fn a_stored_body_reads_in_place() {
        let plain = records_block(3);
        let coded = code(&plain, BodyCodec::STORED);
        assert_eq!(coded.len(), plain.len() + 2);
        let b = Block::read(&coded).unwrap();
        assert_eq!(b.body_codec(), Some(BodyCodec::STORED));
        assert_eq!(records(&b), records(&Block::read(&plain).unwrap()));
        let mut buf = Vec::new();
        assert_eq!(b.decode(&mut buf).unwrap().bytes(), &coded[..]);
    }

    #[test]
    fn a_body_lz4_does_not_shrink_stays_plain() {
        let plain = records_block(1);
        assert_eq!(code(&plain, BodyCodec::LZ4), plain);
    }

    #[test]
    fn coding_takes_only_plain_records_blocks() {
        let plain = records_block(40);
        let coded = code(&plain, BodyCodec::LZ4);
        let mut out = vec![0u8; coded_len_max(plain.len())];
        assert_eq!(
            code_body(&coded, BodyCodec::LZ4, &mut out),
            Err(CodeError::NotRecords)
        );
        assert_eq!(
            code_body(&plain, BodyCodec(2), &mut out),
            Err(CodeError::UnknownCodec(BodyCodec(2)))
        );
        assert_eq!(
            code_body(&plain, BodyCodec::LZ4, &mut out[..10]),
            Err(CodeError::Full {
                needed: coded_len_max(plain.len())
            })
        );
    }

    /// Rewrite the coded block's codec byte and decoded length, its CRC made good.
    fn recode(coded: &[u8], codec: u8, len: Option<u8>) -> Vec<u8> {
        let mut x = coded.to_vec();
        x[HEADER_LEN] = codec;
        if let Some(len) = len {
            x[HEADER_LEN + 1] = len;
        }
        let end = x.len() - CRC_LEN;
        let crc = CRC.checksum(&x[..end]);
        x[end..].copy_from_slice(&crc.to_le_bytes());
        x
    }

    #[test]
    fn an_unknown_codec_is_an_unknown_body() {
        let coded = code(&records_block(40), BodyCodec::LZ4);
        let x = recode(&coded, 2, None);
        let b = Block::read(&x).unwrap();
        assert_eq!(
            (b.body_known(), b.body_codec()),
            (false, Some(BodyCodec(2)))
        );
        assert_eq!(b.records().count(), 0);
        let mut buf = Vec::new();
        assert_eq!(b.decode(&mut buf).unwrap().bytes(), &x[..]);
        // a walk skips it as any block it doesn't know, and goes on
        let mut file = x.clone();
        let mut next = records_block(2);
        crate::set_id(&mut next, id(4)).unwrap();
        file.extend_from_slice(&next);
        let mut rd = crate::FileBlocks::new(&file[..]);
        assert!(!rd.next_block().unwrap().unwrap().body_known());
        assert_eq!(rd.next_block().unwrap().unwrap().record_count, 2);
        assert!(rd.next_block().is_none());
    }

    #[test]
    fn a_bad_coded_body_is_refused() {
        let coded = code(&records_block(40), BodyCodec::LZ4);
        // stored with a length that isn't its bytes'
        assert_eq!(
            Block::read(&recode(&coded, 0, None)).err(),
            Some(BlockError::BadRecords)
        );
        // lz4 decoding to another length than said
        let x = recode(&coded, 1, Some(0x7f));
        let b = Block::read(&x).unwrap();
        let mut buf = Vec::new();
        assert_eq!(b.decode(&mut buf).err(), Some(BlockError::BadRecords));
        // a flagged body without even its codec byte
        let mut empty = vec![0u8; 64];
        crate::put_header(&mut empty, BlockKind::Records, 36, 0, id(0));
        empty[6] = FLAG_BODY_CODED as u8;
        empty.truncate(36);
        let crc = CRC.checksum(&empty[..32]);
        empty[32..].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(Block::read(&empty).err(), Some(BlockError::BadRecords));
    }

    #[test]
    fn a_coded_describe_block_gives_its_layout_to_a_walk() {
        use shrink_wrap::prelude::*;
        let layout = crate::Layout::ALIGNED.on_sd_card();
        let item = crate::Describe::Layout(layout)
            .to_ww_bytes(&mut [0u8; 64])
            .unwrap()
            .to_vec();
        let mut buf = vec![0u8; 4096];
        let mut w = BlockWriter::new(&mut buf, BlockKind::Describe, id(0), &[]).unwrap();
        for _ in 0..20 {
            w.push_describe(&[&item]).unwrap();
        }
        let len = w.finish();
        let plain = &buf[..len];
        for codec in [BodyCodec::STORED, BodyCodec::LZ4] {
            let coded = code(plain, codec);
            let b = Block::read(&coded).unwrap();
            assert_eq!(b.body_codec(), Some(codec));
            assert_eq!(crate::walk::layout_in(&b), Some(layout));
        }
    }
}
