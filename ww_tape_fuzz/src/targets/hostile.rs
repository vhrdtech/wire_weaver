//! The structure-aware targets `tape-<group>-hostile`: each builds a block or file with [crate::hostile], then runs
//! the decoders of its group, read and salvage paths, and goes through everything they hand out.

use super::{chunk, index, payloads, reader, samples, series, summary, times};
use crate::hostile;
use shrink_wrap::DeserializeShrinkWrap;
use ww_tape::times::EventTimes;
use ww_tape::{
    Block, Describe, Element, Layout, Medium, OpenOptions, Ratio64, SampleShape, Samples,
    SeriesSegment, SeriesTimeline, TapeReader, TrackKind, block_len,
};

/// The blocks back to back at the start of `f`, up to the first that doesn't parse.
fn blocks(mut f: &[u8]) -> Vec<&[u8]> {
    let mut out = vec![];
    while let Ok(len) = block_len(f) {
        let Some((b, rest)) = f.split_at_checked(len) else {
            break;
        };
        out.push(b);
        f = rest;
    }
    out
}

/// Every record of a block read or salvaged, its coded body decoded, its describe records decoded.
fn records_of(b: &[u8]) {
    let Ok(block) = Block::salvage(b) else {
        return;
    };
    let _ = (
        block.body_codec(),
        block.clocks().count(),
        block.body_known(),
    );
    for r in block.records().flatten() {
        if r.at.is_none() {
            let _ = Describe::from_ww_bytes(r.item);
        }
    }
    let mut plain = Vec::new();
    if let Ok(decoded) = block.decode(&mut plain) {
        for r in decoded.records().flatten() {
            if r.at.is_none() {
                let _ = Describe::from_ww_bytes(r.item);
            }
        }
    }
}

/// `tape-records-hostile`: a records or describe block of hostile clock tables, record heads, lengths and coded
/// bodies, read alone and walked as a file.
pub fn records_hostile(data: &[u8]) {
    let b = hostile::records(data);
    records_of(&b);
    reader(&b);
}

/// Most items a target reads from one chunk. A reader's work on a chunk grows with its items, so a chunk that reads
/// as more is a failure of the case here, not a loop of minutes: ww_tape bounds no chunk's count yet.
const CHUNK_ITEMS_READ_MAX: usize = 1 << 20;

/// The times of the chunk `b` read and salvaged hold at most [CHUNK_ITEMS_READ_MAX] items.
fn items_bounded(b: &[u8]) {
    let Ok(block) = Block::salvage(b) else { return };
    let Some(c) = block.chunk() else { return };
    for t in [EventTimes::read(&c), EventTimes::salvage(&c)]
        .into_iter()
        .flatten()
    {
        assert!(
            t.len() <= CHUNK_ITEMS_READ_MAX,
            "a chunk of {} bytes reads as {} items",
            b.len(),
            t.len()
        );
    }
}

/// `tape-chunk-hostile`: a chunk block of a hostile head, read, salvaged and renumbered.
pub fn chunk_hostile(data: &[u8]) {
    let b = hostile::chunk(data);
    items_bounded(&b);
    chunk::walk(&b);
    times::walk(&b);
    payloads::walk(&b, None);
}

/// `tape-times-hostile`: an events chunk of hostile time group records, its times read and salvaged.
pub fn times_hostile(data: &[u8]) {
    let b = hostile::times(data);
    items_bounded(&b);
    times::walk(&b);
}

/// `tape-payloads-hostile`: an events chunk of hostile payload lengths and lz4 lengths, its payloads read and
/// salvaged.
pub fn payloads_hostile(data: &[u8]) {
    let b = hostile::payloads(data);
    payloads::walk(&b, None);
}

/// The shape of the series track a describe block names; None when it names none.
fn series_def(b: &[u8]) -> Option<(SampleShape, Ratio64)> {
    let block = Block::read(b).ok()?;
    let r = block.records().flatten().next()?;
    match Describe::from_ww_bytes(r.item).ok()? {
        Describe::Track(t) => {
            let _ = (t.check_kind(), t.id(), t.dimensions_canonical());
            match t.kind {
                TrackKind::Series(s) => {
                    let _ = (s.check(), s.first_segment(0));
                    Some((s.shape, s.period))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Decode a samples column as `T` whole, from `from` on and group by group.
fn decode_column(
    column: &ww_tape::chunk::Column<'_>,
    shape: SampleShape,
    count: usize,
    from: usize,
) {
    macro_rules! go {
        ($t:ty) => {
            samples::read::<$t>(column.codec, shape, count, column.bytes, from)
        };
    }
    match shape.element.scalar() {
        Element::I8 => go!(i8),
        Element::I16 => go!(i16),
        Element::I32 => go!(i32),
        Element::I64 => go!(i64),
        Element::U8 => go!(u8),
        Element::U16 => go!(u16),
        Element::U32 => go!(u32),
        Element::U64 => go!(u64),
        Element::F32 => go!(f32),
        Element::F64 => go!(f64),
        Element::Bool => go!(bool),
        _ => {}
    }
    if let Ok(s) = Samples::of(column, shape, count) {
        let _ = s.sample_bytes(0..count.min(s.count()));
    }
}

/// `tape-samples-hostile`: a series track and a series chunk of a hostile sample column, its samples decoded as the
/// track's shape says, from the chunk's count.
pub fn samples_hostile(data: &[u8]) {
    let f = hostile::samples(data);
    let bs = blocks(&f);
    let shape = bs
        .first()
        .and_then(|b| series_def(b))
        .map_or(SampleShape::scalar(Element::I16), |(s, _)| s);
    for b in &bs {
        chunk::walk(b);
        let Ok(block) = Block::salvage(b) else {
            continue;
        };
        let Some(c) = block.chunk() else { continue };
        let count = c.head().count as usize;
        for col in c.columns() {
            let _ = c.column_damaged(&col);
            decode_column(&col, shape, count, data.len());
        }
    }
}

/// `tape-series-hostile`: a series track and its segment records of hostile indexes, ticks and scales, built into a
/// timeline that is then asked about every segment.
pub fn series_hostile(data: &[u8]) {
    let f = hostile::series(data);
    let bs = blocks(&f);
    let period = bs
        .first()
        .and_then(|b| series_def(b))
        .map_or(Ratio64::new(1, 1).expect("a ratio"), |(_, p)| p);
    let Some(Ok(block)) = bs.get(1).map(|b| Block::read(b)) else {
        return;
    };
    let records: Vec<(i64, &[u8])> = block
        .records()
        .flatten()
        .filter_map(|r| Some((r.at?.ticks, r.item)))
        .collect();
    for (_, item) in &records {
        if let Ok(s) = SeriesSegment::from_record(item) {
            let _ = (s.first_tick(period), s.tick(period, s.first_index));
        }
    }
    let (t, _) = SeriesTimeline::from_records(period, records.iter().copied());
    series::check(&t);
    let count = data.len() as u32 % 300;
    let firsts: Vec<u64> = t.segments().iter().map(|s| s.first_index).collect();
    for at in firsts {
        series::query(&t, at, count);
    }
    series::query(&t, u64::from(count), 1);
}

/// `tape-summary-hostile`: a summary block of a hostile head, scale or node values, every node and column read.
pub fn summary_hostile(data: &[u8]) {
    let b = hostile::summary(data);
    summary::walk(&b);
    let _ = Block::salvage(&b).map(|b| b.summary().map(|s| s.scale()));
}

/// `tape-index-hostile`: an index, finish, piece head or tail block of hostile offsets, directory or entries, read
/// alone, from its head and as a file.
pub fn index_hostile(data: &[u8]) {
    let b = hostile::index(data);
    index::decode(&b);
    index::read(&b, OpenOptions::default());
}

/// `tape-reader-hostile`: a piece of hostile offsets, lengths, layout or stretches between blocks, opened by the
/// positional reader as a file and as a raw medium, every piece's entries, blocks and describe set read, and walked
/// block by block.
pub fn reader_hostile(data: &[u8]) {
    let f = hostile::file(data);
    let raw = OpenOptions {
        medium: Medium::Raw,
        layout: Some(Layout::UNALIGNED.on_sd_card()),
        ..OpenOptions::default()
    };
    for options in [OpenOptions::default(), raw] {
        index::read(&f, options);
        if let Ok(r) = TapeReader::open_with(&f[..], options) {
            for i in 0..r.pieces().len() {
                let _ = r.describe_set(i);
            }
        }
    }
    reader(&f);
}
