//! A series track written and read back with ww_tape alone: its definition, its segments (a start, an autorange
//! prefix change, a gap) and its samples, from `Series<Fixed<..>>` batches to chunk columns without a float.

use std::path::PathBuf;

use exact_units::prefix::{Micro, Milli};
use exact_units::units::{Ampere, Second, Tick};
use exact_units::{Fixed, MagnitudeOf, units::Per};
use shrink_wrap::prelude::*;
use shrink_wrap::series::Series;
use ww_tape::chunk::ChunkHead;
use ww_tape::samples::{Samples, encode, max_encoded_len};
use ww_tape::{
    BlockKind, BlockWriter, ClockDef, ClockOrigin, ClockTime, Codec, ColumnKind, ColumnSpec,
    Describe, DescribeOwned, DescribeSet, EndMode, FileBlocks, FullVersion, Layout, Magnitude,
    Producer, Ratio64, Recovery, SampleShape, SeriesDef, SeriesSegment, SeriesTimeline,
    SeriesValue, Sticky, SyncPolicy, TapeReader, TapeWriter, TrackDef, TrackKind, TypeRef, Version,
    clock_key, track_key,
};

const CLOCK: u32 = 1;
const TRACK: u32 = 1;
const CHANNELS: u32 = 2;
/// Samples per chunk at most.
const CHUNK_MAX: u64 = 128;
/// Samples per received batch.
const BATCH: usize = 32;
/// First tick of the series.
const START: i64 = 1_000;
/// Ticks between the last sample before the gap and the first after it.
const GAP: i64 = 5_000_000;

type MicroAmps = Fixed<Ampere, Micro, i32>;
type MilliAmps = Fixed<Ampere, Milli, i32>;

/// 48 kHz on a 1 ns clock.
fn period() -> Ratio64 {
    Ratio64::new(62_500, 3).unwrap()
}

/// Stored number of channel `c` of sample `i`.
fn value(i: u64, c: u32) -> i32 {
    ((i as i32 * 37) % 1000) - 500 + c as i32 * 10_000
}

/// A stretch of the recording: samples `from..to` as the device sends them.
#[derive(Copy, Clone)]
enum Unit {
    Micro,
    Milli,
}

/// Samples 0..320 in µA, 320..480 in mA (the device autoranged), 480..640 in µA after a gap.
const STRETCHES: [(u64, u64, Unit, bool); 3] = [
    (0, 320, Unit::Micro, false),
    (320, 480, Unit::Milli, false),
    (480, 640, Unit::Micro, true),
];

/// One received batch of samples `from..to`, its bytes as they came from the wire.
fn batch(from: u64, to: u64, unit: Unit) -> Vec<u8> {
    let mut buf = vec![0u8; 4096];
    let len = match unit {
        Unit::Micro => {
            let items: Vec<[MicroAmps; 2]> = (from..to)
                .map(|i| [MicroAmps::new(value(i, 0)), MicroAmps::new(value(i, 1))])
                .collect();
            Series::new(&items).to_ww_bytes(&mut buf).unwrap().len()
        }
        Unit::Milli => {
            let items: Vec<[MilliAmps; 2]> = (from..to)
                .map(|i| [MilliAmps::new(value(i, 0)), MilliAmps::new(value(i, 1))])
                .collect();
            Series::new(&items).to_ww_bytes(&mut buf).unwrap().len()
        }
    };
    buf.truncate(len);
    buf
}

/// A received batch decoded and put into `column` as stored numbers, channels interleaved: no float on the way.
/// Its scale is the type's.
fn ingest<V>(bytes: &[u8], column: &mut Vec<i32>) -> Magnitude
where
    V: SeriesValue<Scalar = i32> + for<'i> DeserializeShrinkWrap<'i>,
    [V; 2]: for<'i> shrink_wrap::series::SeriesColumns<'i>,
{
    let s = Series::<[V; 2]>::from_ww_bytes(bytes).unwrap();
    for item in s.iter() {
        for v in item {
            let mut n = [0i32; 1];
            assert_eq!(v.write_parts(&mut n), 1);
            column.push(n[0]);
        }
    }
    V::SCALE
}

fn sample_type() -> TypeRef<'static> {
    TypeRef {
        bundle: RefVec::Slice { slice: &[] },
        krate: FullVersion::new("exact_units", Version::new(0, 1, 0)),
        path: "Fixed<Ampere, Micro, i32>",
    }
}

fn series_def() -> SeriesDef<'static> {
    SeriesDef::of::<MicroAmps>(Some(sample_type()), CHANNELS, period())
}

fn encode_value<T: SerializeShrinkWrap>(v: &T) -> Vec<u8> {
    let mut buf = [0u8; 256];
    v.to_ww_bytes(&mut buf).unwrap().to_vec()
}

fn describe_blocks(first: &SeriesSegment, seq: &mut u64) -> Vec<Vec<u8>> {
    let mut set = DescribeSet::new();
    set.insert(&Describe::Producer(Producer { name: "series" }))
        .unwrap();
    set.insert(&Describe::Layout(Layout::ALIGNED)).unwrap();
    set.insert(&Describe::Clock(ClockDef {
        number: UNib32(CLOCK),
        key: clock_key(0x5e, &[], 1),
        rate: MagnitudeOf::<Per<Tick, Second>>::from(Magnitude::pow10(9)),
        resolution: Magnitude::pow10(-9).into(),
        origin: ClockOrigin::Boot,
        label: "dev",
    }))
    .unwrap();
    set.insert(&Describe::Track(TrackDef {
        number: UNib32(TRACK),
        key: track_key(0x5e, &[3]),
        clock: UNib32(CLOCK),
        dimensions: RefVec::Slice { slice: &[] },
        api: FullVersion::new("motor_api", Version::new(0, 1, 0)),
        bundle: RefVec::Slice { slice: &[] },
        path: RefVec::Slice {
            slice: &[UNib32(3)],
        },
        item: None,
        label: "phase.current",
        sticky: true,
        kind: TrackKind::Series(series_def()),
    }))
    .unwrap();
    let item = encode_value(first);
    let sticky = [Sticky {
        track: TRACK,
        at: ClockTime::new(CLOCK, first.first_tick(period()).unwrap()),
        item: &item,
    }];
    let mut blocks = vec![];
    set.write_blocks(seq, 4096, &sticky, |b| blocks.push(b.to_vec()))
        .unwrap();
    blocks
}

/// The records block of one segment.
fn segment_block(s: &SeriesSegment, seq: u64) -> Vec<u8> {
    let mut buf = [0u8; 256];
    let mut w = BlockWriter::new(&mut buf, BlockKind::Records, seq, &[CLOCK]);
    let item = encode_value(s);
    let tick = s.first_tick(period()).unwrap();
    w.push(TRACK, ClockTime::new(CLOCK, tick), &[&item])
        .unwrap();
    let len = w.finish();
    buf[..len].to_vec()
}

/// Write the open column as chunks of at most [CHUNK_MAX] samples, all in the newest segment.
fn seal(w: &mut TapeWriter, t: &SeriesTimeline, first: &mut u64, column: &mut Vec<i32>) {
    let shape = SampleShape::new(i32::ELEMENT, CHANNELS);
    let lanes = CHANNELS as usize;
    for part in column.chunks(CHUNK_MAX as usize * lanes) {
        let count = (part.len() / lanes) as u32;
        let (first_tick, last_tick) = t.chunk_ticks(*first, count).unwrap();
        let mut bytes = vec![0u8; max_encoded_len(Codec::DELTA, shape, count as usize).unwrap()];
        let n = encode(Codec::DELTA, shape, part, &mut bytes).unwrap();
        bytes.truncate(n);
        let head = ChunkHead {
            kind: BlockKind::Series,
            track_id: 7,
            track_number: TRACK,
            first_item: *first,
            count,
            first_tick,
            last_tick,
            latest_end: last_tick,
            time_mode: 0,
            end_mode: EndMode::Absent,
        };
        let spec = ColumnSpec {
            kind: ColumnKind::Samples,
            codec: Codec::DELTA,
            param: 0,
            len: bytes.len(),
        };
        w.write_chunk(&head, &[(spec, &bytes)]).unwrap();
        *first += u64::from(count);
    }
    column.clear();
}

/// The recording: batches as they come, a segment at the start, at the prefix change and after the gap.
fn write(path: &std::path::Path) {
    let p = period();
    let mut w = TapeWriter::create(path, SyncPolicy::None).unwrap();
    w.set_alignment(ww_tape::ALIGNMENT);
    let first = series_def().first_segment(START);
    let mut seq = 0;
    for b in describe_blocks(&first, &mut seq) {
        w.write_block(&b).unwrap();
    }
    let mut t = SeriesTimeline::new(p);
    t.push(first).unwrap();
    let mut next = 0u64;
    let mut column = Vec::new();
    for (from, to, unit, gap) in STRETCHES {
        for b in (from..to).step_by(BATCH) {
            let bytes = batch(b, (b + BATCH as u64).min(to), unit);
            let mut numbers = Vec::new();
            let scale = match unit {
                Unit::Micro => ingest::<MicroAmps>(&bytes, &mut numbers),
                Unit::Milli => ingest::<MilliAmps>(&bytes, &mut numbers),
            };
            let at = next + column.len() as u64 / u64::from(CHANNELS);
            let newest = *t.segments().last().unwrap();
            let segment = if gap && b == from {
                let last = t.tick(at - 1).unwrap();
                Some(SeriesSegment::start(at, last + GAP, scale))
            } else if scale != newest.scale {
                Some(newest.rescaled(at, scale))
            } else {
                None
            };
            if let Some(s) = segment {
                seal(&mut w, &t, &mut next, &mut column);
                t.push(s).unwrap();
                w.defer_block(segment_block(&s, w.next_seq())).unwrap();
            }
            column.extend(numbers);
            if column.len() as u64 >= CHUNK_MAX * u64::from(CHANNELS) {
                seal(&mut w, &t, &mut next, &mut column);
            }
        }
    }
    seal(&mut w, &t, &mut next, &mut column);
    w.finish().unwrap();
}

/// What a reader with ww_tape alone gets: the definition, the segments, every chunk's head and numbers.
struct Read {
    shape: SampleShape,
    period: Ratio64,
    scale: Magnitude,
    timeline: SeriesTimeline,
    chunks: Vec<(ChunkHead, Vec<i32>)>,
}

fn read(bytes: &[u8]) -> Read {
    let r = TapeReader::open(bytes).unwrap();
    assert_eq!(r.pieces().len(), 1);
    let mut def = None;
    let mut records: Vec<(i64, Vec<u8>)> = vec![];
    let mut chunks = vec![];
    let mut buf = vec![];
    for e in r.entries(0).unwrap() {
        let b = r.read_block(0, &e, &mut buf).unwrap();
        match b.kind {
            BlockKind::Describe | BlockKind::Records => {
                for rec in b.records() {
                    let rec = rec.unwrap();
                    if rec.track == 0 {
                        let d = DescribeOwned::from_ww_bytes_owned(rec.item).unwrap();
                        if let DescribeOwned::Track(t) = d
                            && let ww_tape::TrackKindOwned::Series(s) = t.kind
                        {
                            def = Some(s);
                        }
                    } else if rec.track == TRACK {
                        records.push((rec.at.unwrap().ticks, rec.item.to_vec()));
                    }
                }
            }
            BlockKind::Series => {
                let c = b.chunk().unwrap();
                let head = c.head();
                let def = def.as_ref().expect("describe before chunks");
                let col = c.column(0).unwrap();
                assert_eq!(col.kind, ColumnKind::Samples);
                let s = Samples::of(&col, def.shape, head.count as usize).unwrap();
                let mut out = vec![0i32; head.count as usize * s.lanes()];
                s.decode(0, &mut out).unwrap();
                chunks.push((head, out));
            }
            _ => {}
        }
    }
    let def = def.expect("a series track");
    let (timeline, errors) =
        SeriesTimeline::from_records(def.period, records.iter().map(|(t, b)| (*t, &b[..])));
    assert!(errors.is_empty(), "{errors:?}");
    Read {
        shape: def.shape,
        period: def.period,
        scale: def.scale,
        timeline,
        chunks,
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ww_tape_series_{name}_{}", std::process::id()));
    _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn series_reads_back_to_samples_ticks_and_scales() {
    let dir = scratch("read");
    let path = dir.join("s.wwt");
    write(&path);
    let bytes = std::fs::read(&path).unwrap();
    let got = read(&bytes);
    assert_eq!(got.shape, SampleShape::new(i32::ELEMENT, CHANNELS));
    assert_eq!(got.period, period());
    assert_eq!(got.scale, Magnitude::pow10(-6));

    let micro = Magnitude::pow10(-6);
    let milli = Magnitude::pow10(-3);
    let p = period();
    let t = &got.timeline;
    // the three segments: start, prefix change on the same grid, gap
    let s = t.segments();
    assert_eq!(s.len(), 3);
    assert_eq!(s[0], SeriesSegment::start(0, START, micro));
    assert_eq!(s[1], s[0].rescaled(320, milli));
    let after_gap = s[0].tick(p, 479).unwrap() + GAP;
    assert_eq!(s[2], SeriesSegment::start(480, after_gap, micro));

    let mut index = 0u64;
    for (head, numbers) in &got.chunks {
        assert_eq!(head.first_item, index, "chunks follow each other");
        t.check_chunk(head).unwrap();
        for (k, sample) in numbers.chunks(CHANNELS as usize).enumerate() {
            let i = index + k as u64;
            for (c, n) in sample.iter().enumerate() {
                assert_eq!(*n, value(i, c as u32), "sample {i} channel {c}");
            }
            let (grid, scale) = match i {
                0..320 => (
                    START + p.apply_rounded(i as i64, ww_tape::Rounding::Floor).unwrap(),
                    micro,
                ),
                320..480 => (
                    START + p.apply_rounded(i as i64, ww_tape::Rounding::Floor).unwrap(),
                    milli,
                ),
                _ => (
                    after_gap
                        + p.apply_rounded(i as i64 - 480, ww_tape::Rounding::Floor)
                            .unwrap(),
                    micro,
                ),
            };
            assert_eq!(t.tick(i), Some(grid), "tick of sample {i}");
            assert_eq!(t.scale(i), Some(scale), "scale of sample {i}");
        }
        index += u64::from(head.count);
    }
    assert_eq!(index, 640);
    _ = std::fs::remove_dir_all(&dir);
}

/// The same recording gives the same bytes, and they are golden/v2-series.hex's (`WW_TAPE_GOLDEN=update` rewrites
/// it): one block per line, in hex.
#[test]
fn series_file_is_deterministic_and_golden() {
    let dir = scratch("golden");
    let (a, b) = (dir.join("a.wwt"), dir.join("b.wwt"));
    write(&a);
    write(&b);
    let bytes = std::fs::read(&a).unwrap();
    assert_eq!(
        bytes,
        std::fs::read(&b).unwrap(),
        "two writes of the same series"
    );
    let mut hex = String::new();
    let mut rd = FileBlocks::with_recovery(&bytes[..], Recovery::Prefix);
    while let Some(block) = rd.next_block() {
        for x in block.unwrap().bytes() {
            hex.push_str(&format!("{x:02x}"));
        }
        hex.push('\n');
    }
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/v2-series.hex");
    if std::env::var("WW_TAPE_GOLDEN").as_deref() == Ok("update") {
        std::fs::write(&golden, &hex).unwrap();
    }
    let want = std::fs::read_to_string(&golden)
        .expect("golden/v2-series.hex: run with WW_TAPE_GOLDEN=update");
    assert_eq!(
        hex, want,
        "the format changed: bytes differ from golden/v2-series.hex"
    );
    let want: Vec<u8> = want
        .lines()
        .flat_map(|l| {
            (0..l.len())
                .step_by(2)
                .map(move |i| u8::from_str_radix(&l[i..i + 2], 16).unwrap())
        })
        .collect();
    assert_eq!(read(&want).chunks.len(), read(&bytes).chunks.len());
    _ = std::fs::remove_dir_all(&dir);
}
