//! Summary blocks: the head (tree, level, node shift, element, fields, channels, scale length, item range, ticks),
//! the scale bytes and the node values.

use super::{Draw, Field, HEADER_FIELDS, HEADER_LEN, LEN_FIELD, hit, hit_some, seal};
use ww_tape::summary::{
    EventTree, ItemRun, NODE_SHIFT_MAX, SCALE_LEN_MAX, SUMMARY_FIXED_LEN, SeriesStart, SeriesStats,
    SeriesSummaries, SeriesSummarySpec,
};
use ww_tape::{Element, EventFields, Magnitude, Ratio64, SampleShape, SeriesSegment};

/// The fields of a summary head after the block header.
const HEAD_FIELDS: [Field; 16] = [
    Field::new(28, 4, &[]),
    Field::new(32, 8, &[]),
    Field::new(40, 4, &[]),
    Field::new(44, 1, &[1, 2]),
    Field::new(45, 1, &[15, 16, NODE_SHIFT_MAX as u64]),
    Field::new(
        46,
        1,
        &[
            NODE_SHIFT_MAX as u64,
            SeriesSummarySpec::BASE_SHIFT_MAX as u64,
            11,
        ],
    ),
    Field::new(47, 1, &[12, 13]),
    Field::new(48, 2, &[]),
    Field::new(50, 2, &[ww_tape::series::SERIES_CHANNELS_MAX as u64]),
    Field::new(52, 2, &[SCALE_LEN_MAX as u64]),
    Field::new(54, 2, &[]),
    Field::new(56, 8, &[]),
    Field::new(64, 8, &[1 << 62]),
    Field::new(72, 8, &[]),
    Field::new(80, 8, &[]),
    Field::new(88, 8, &[]),
];

/// The blocks of an event tree of items drawn from the input.
fn events(d: &mut Draw<'_>) -> Vec<Vec<u8>> {
    let fields = EventFields {
        flags: d.bool(),
        latest_end: d.bool(),
    };
    let first = u64::from(d.u16()) * 37;
    let mut tree = EventTree::new(1, 1, fields, first);
    tree.set_max_block_bytes(96 + 64 * d.range(1, 8));
    let mut t = i64::from(d.u8()) - 128;
    let mut r = d.bits();
    for _ in 0..1 + d.below(64) {
        let x = r.u8();
        t += i64::from(x >> 2);
        let pushed = if x & 0x80 != 0 {
            let room = 2048 - tree.next_item() % 2048;
            tree.push_run(ItemRun {
                count: room.min(r.u64() % 0x1_0000 + 1),
                first_tick: t,
                last_tick: t + 5,
                flags: u32::from(x),
                latest_end: t + 5 + i64::from(x & 3),
            })
        } else {
            tree.push(t, t + i64::from(x & 3), u32::from(x & 3))
        };
        if pushed.is_err() {
            break;
        }
        t += 5;
    }
    if tree.finish().is_err() {
        return vec![];
    }
    tree.take_blocks()
}

/// The blocks of series summaries of samples drawn from the input.
fn series(d: &mut Draw<'_>) -> Vec<Vec<u8>> {
    let element = if d.bool() {
        Element::ComplexI16
    } else {
        Element::I16
    };
    let shape = SampleShape::new(element, 1 + d.below(3) as u32);
    let lanes = shape.lanes().expect("lanes") as usize;
    let first = u64::from(d.u16());
    let start = SeriesStart {
        track_id: 2,
        track_number: 2,
        shape,
        first_item: first,
        segment: SeriesSegment::start(first, -5, Magnitude::pow10(-3)),
        period: Ratio64::new(3, 2).expect("a ratio"),
    };
    let spec = SeriesSummarySpec {
        base_shift: 4 + d.below(3) as u8,
        stats: SeriesStats::all(element),
        clip: None,
    };
    let Ok(mut s) = SeriesSummaries::<i16>::new(start, spec) else {
        return vec![];
    };
    s.set_max_block_bytes(512);
    let mut segment = start.segment;
    let mut r = d.bits();
    for i in 0..d.below(12) {
        let part: Vec<i16> = (0..lanes * (1 + r.below(40)))
            .map(|_| r.u64() as i16)
            .collect();
        if r.u8().is_multiple_of(5) && s.next_item() > first {
            segment = segment.rescaled(s.next_item(), Magnitude::pow10(-3 - 3 * r.below(3) as i8));
            if s.segment(segment).is_err() {
                break;
            }
        }
        if s.push(&part, i as u32).is_err() {
            break;
        }
    }
    if s.finish().is_err() {
        return vec![];
    }
    s.take_blocks()
}

/// A summary block of an event tree or series summaries drawn from the input, whose head, scale or node values take
/// hostile values.
pub fn summary(data: &[u8]) -> Vec<u8> {
    let mut d = Draw::new(data);
    let mut blocks = if d.bool() {
        events(&mut d)
    } else {
        series(&mut d)
    };
    if blocks.is_empty() {
        // one item of an event tree
        let mut tree = EventTree::new(1, 1, EventFields::default(), 0);
        tree.push(0, 0, 0).expect("an item");
        tree.finish().expect("a tree of one item finishes");
        blocks = tree.take_blocks();
    }
    let i = d.below(blocks.len());
    let mut b = blocks.swap_remove(i);
    let body_at = HEADER_LEN + SUMMARY_FIXED_LEN;
    match d.u8() % 10 {
        9 => hit(&mut d, &mut b, LEN_FIELD),
        1 | 2 => {
            let f = HEADER_FIELDS[d.below(HEADER_FIELDS.len())];
            hit(&mut d, &mut b, f);
        }
        3 => {
            // a byte of the scale or the node values
            let at = body_at + d.below(b.len().saturating_sub(body_at + 4).max(1));
            hit(&mut d, &mut b, Field::new(at, 1, &[]));
        }
        4 => {
            // a node value: 8 bytes in a column
            let at = body_at + d.below(b.len().saturating_sub(body_at + 12).max(1));
            hit(&mut d, &mut b, Field::new(at, 8, &[]));
        }
        _ => hit_some(&mut d, &mut b, &HEAD_FIELDS),
    }
    seal(&mut b);
    b
}
