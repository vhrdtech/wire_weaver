//! Events chunks of the payload codec: the lengths' group records (base, minimum, width, offset), their packed
//! lengths, an lz4 column's decoded length and the payload columns' codecs.

use super::chunk::{build, column_fields, column_start, hit_head, reseal};
use super::{Draw, Field, hit};
use ww_tape::bitpack::GROUP;
use ww_tape::lz4::LZ4_DECODED_MAX;
use ww_tape::payloads::EncodedPayloads;
use ww_tape::times::EncodedTimes;
use ww_tape::{Codec, ColumnKind, ColumnSpec};

/// Bytes of a lengths group record.
const LENGTHS_RECORD: usize = 13;

/// An events chunk of payloads drawn from the input whose lengths, lz4 length, codecs or head take hostile values.
pub fn payloads(data: &[u8]) -> Vec<u8> {
    let mut d = Draw::new(data);
    // mostly one short group, now and then a second group
    let n = if d.u8() % 4 == 3 {
        GROUP + 1 + d.below(8)
    } else {
        1 + d.below(24)
    };
    // payloads that repeat give lz4 something to find
    let repeat = d.bool();
    let mut r = d.bits();
    let items: Vec<Vec<u8>> = (0..n)
        .map(|_| {
            let len = r.below(if repeat { 8 } else { 4 });
            vec![r.u8() & if repeat { 0x3 } else { 0xff }; len]
        })
        .collect();
    let starts: Vec<i64> = (0..n as i64).collect();
    let times = EncodedTimes::encode(&starts, None, None).expect("valid times");
    let enc = EncodedPayloads::encode(items.iter().map(Vec::as_slice)).expect("valid payloads");
    let cols: Vec<(ColumnSpec, &[u8])> = times.columns().chain(enc.columns()).collect();
    let mut b = build(&times.head.chunk_head(1, 1, 0), &cols);
    // the chunk's columns are in the order written
    let kinds: Vec<ColumnKind> = cols.iter().map(|c| c.0.kind).collect();
    let groups = n.div_ceil(GROUP);
    let lengths = kinds.iter().position(|k| *k == ColumnKind::PayloadLengths);
    let payloads = kinds.iter().position(|k| *k == ColumnKind::Payloads);
    for _ in 0..1 + d.below(3) {
        match d.u8() % 10 {
            9 => hit_head(&mut d, &mut b),
            1 => {
                // a payload column's codec or kind
                let i = d.below(kinds.len());
                let f = column_fields(i)[d.below(2)];
                let caps: &'static [u64] = &[
                    Codec::RAW.0 as u64,
                    Codec::LZ4.0 as u64,
                    Codec::SERIES.0 as u64,
                    0x80,
                ];
                hit(&mut d, &mut b, Field { caps, ..f });
            }
            2 | 3 => {
                // the lz4 column's decoded length, or a byte of its block
                if let Some(at) = payloads.and_then(|i| column_start(&b, i)) {
                    let col_len = b.len() - at;
                    let caps: &'static [u64] = &[LZ4_DECODED_MAX as u64, 255 * 1024 + 16];
                    let f = if d.bool() {
                        Field::new(at, 4, caps)
                    } else {
                        Field::new(at + d.below(col_len), 1, &[])
                    };
                    hit(&mut d, &mut b, f);
                }
            }
            4 => {
                // a byte of packed lengths
                if let Some(at) = lengths.and_then(|i| column_start(&b, i)) {
                    let at = at + groups * LENGTHS_RECORD + d.below(16);
                    hit(&mut d, &mut b, Field::new(at, 1, &[]));
                }
            }
            _ => {
                if let Some(at) = lengths.and_then(|i| column_start(&b, i)) {
                    let r = at + d.below(groups + 1) * LENGTHS_RECORD;
                    let fields = [
                        Field::new(r, 4, &[LZ4_DECODED_MAX as u64]),
                        Field::new(r + 4, 4, &[]),
                        Field::new(r + 8, 1, &[32, 33, 64]),
                        Field::new(r + 9, 4, &[]),
                    ];
                    let f = fields[d.below(fields.len())];
                    hit(&mut d, &mut b, f);
                }
            }
        }
    }
    reseal(&mut b, d.u8() % 3 == 2);
    b
}
