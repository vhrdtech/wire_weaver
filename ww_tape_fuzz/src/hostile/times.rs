//! Events chunks of the time codec: the implicit period, the linear starts' group records (first tick, slope,
//! residual minimum, width, offset), the durations' and flags' group records and their packed values.

use super::chunk::{build, column_start, hit_head, reseal};
use super::{Draw, Field, hit};
use ww_tape::bitpack::{GROUP, WIDTH_MAX};
use ww_tape::times::{EncodedTimes, SLOPE_MAX};
use ww_tape::{Block, ColumnKind};

/// Bytes of a linear starts group record, a durations record and a flags record.
const STARTS_RECORD: usize = 25;
const DURATIONS_RECORD: usize = 13;
const FLAGS_RECORD: usize = 8;

const WIDTH_CAPS: &[u64] = &[WIDTH_MAX as u64, 32, 63];

/// The fields of group `g`'s record in a column of `kind` starting at `at`.
fn record_fields(kind: ColumnKind, linear: bool, at: usize, g: usize) -> Vec<Field<'static>> {
    match kind {
        ColumnKind::Starts if !linear => vec![Field::new(at, 8, &[SLOPE_MAX as u64])],
        ColumnKind::Starts => {
            let r = at + g * STARTS_RECORD;
            vec![
                Field::new(r, 8, &[]),
                Field::new(r + 8, 8, &[SLOPE_MAX as u64]),
                Field::new(r + 16, 4, &[]),
                Field::new(r + 20, 1, WIDTH_CAPS),
                Field::new(r + 21, 4, &[]),
            ]
        }
        ColumnKind::Ends => {
            let r = at + g * DURATIONS_RECORD;
            vec![
                Field::new(r, 8, &[]),
                Field::new(r + 8, 1, WIDTH_CAPS),
                Field::new(r + 9, 4, &[]),
            ]
        }
        _ => {
            let r = at + g * FLAGS_RECORD;
            vec![Field::new(r, 4, &[]), Field::new(r + 4, 4, &[])]
        }
    }
}

/// Starts, ends and flags drawn from the input: `n` items, ascending.
fn items(d: &mut Draw<'_>) -> (Vec<i64>, Vec<i64>, Vec<u32>) {
    let n = 1 + d.below(GROUP + 32);
    let shift = d.below(12) as u32;
    let mut t = i64::from(d.u32() as i32) << d.below(24);
    let (mut starts, mut ends, mut flags) = (vec![], vec![], vec![]);
    let step = i64::from(d.u8());
    let mut r = d.bits();
    for i in 0..n {
        t = t.saturating_add((step + (i as i64 * 7919) % 13) << shift.min(20));
        starts.push(t);
        ends.push(t.saturating_add(i64::from(r.u8() % 4) << shift.min(30)));
        flags.push(u32::from(i as u8 & r.u8()));
    }
    (starts, ends, flags)
}

/// An events chunk of times drawn from the input whose group records, packed values or head take hostile values.
pub fn times(data: &[u8]) -> Vec<u8> {
    let mut d = Draw::new(data);
    let (starts, ends, flags) = items(&mut d);
    let ends = d.bool().then_some(ends.as_slice());
    let flags = d.bool().then_some(flags.as_slice());
    let enc = EncodedTimes::encode(&starts, ends, flags).expect("valid times");
    let cols: Vec<_> = enc.columns().collect();
    let mut b = build(&enc.head.chunk_head(1, 1, 0), &cols);
    let groups = starts.len().div_ceil(GROUP);
    let linear = enc.head.time_mode == ww_tape::times::TimeMode::Linear;
    let kinds: Vec<ColumnKind> = {
        let block = Block::read(&b).expect("written good");
        let chunk = block.chunk().expect("a chunk");
        chunk.columns().map(|c| c.kind).collect()
    };
    for _ in 0..1 + d.below(3) {
        match d.u8() % 8 {
            7 => hit_head(&mut d, &mut b),
            6 => {
                // a byte of packed values
                let i = d.below(kinds.len());
                if let Some(at) = column_start(&b, i) {
                    let at = at + d.below(b.len() - at);
                    hit(&mut d, &mut b, Field::new(at, 1, &[]));
                }
            }
            _ => {
                let i = d.below(kinds.len());
                let g = d.below(groups + 1);
                if let Some(at) = column_start(&b, i) {
                    let fields = record_fields(kinds[i], linear, at, g);
                    let f = fields[d.below(fields.len())];
                    hit(&mut d, &mut b, f);
                }
            }
        }
    }
    reseal(&mut b, d.u8() % 3 == 2);
    b
}
