//! Series chunks of the sample codecs, behind the describe block of their track: the group end table, each group's
//! lane headers (first value, minimum, width; ALP's scheme, exponents, dictionary bits and exception counts), the
//! packed values, the chunk's sample count and the track's element and channels.

use super::chunk::{build, column_start, hit_head, reseal};
use super::series::track_item;
use super::{Draw, Field, RUN, describe_block, hit};
use ww_tape::samples::{Element, SampleShape, encode, max_encoded_len};
use ww_tape::series::SERIES_CHANNELS_MAX;
use ww_tape::{
    BlockId, BlockKind, ChunkHead, Codec, ColumnKind, ColumnSpec, EndMode, Magnitude, Ratio64,
    Sample,
};

/// Every sample codec a reader takes.
pub(crate) const CODECS: [Codec; 11] = [
    Codec::RAW,
    Codec::BIG_ENDIAN,
    Codec::PACKED10_LSB,
    Codec::PACKED10_MSB,
    Codec::PACKED12_LSB,
    Codec::PACKED12_MSB,
    Codec::PACKED24_LSB,
    Codec::PACKED24_MSB,
    Codec::DELTA,
    Codec::DELTA2,
    Codec::ALP,
];

/// `values` lanes of `T` from the input, coded in `codec`; None when the codec doesn't code the element.
fn coded<T: Sample>(
    d: &mut Draw<'_>,
    codec: Codec,
    shape: SampleShape,
    count: usize,
) -> Option<Vec<u8>> {
    let lanes = shape.lanes()? as usize;
    let smooth = d.bool();
    let mut x = d.u64();
    let mut r = d.bits();
    let values: Vec<T> = (0..count * lanes)
        .map(|_| {
            x = if smooth {
                x.wrapping_add(u64::from(r.u8() % 5))
            } else {
                x ^ r.u64()
            };
            T::from_bits(x)
        })
        .collect();
    let mut out = vec![0u8; max_encoded_len(codec, shape, count).ok()?];
    let n = encode(codec, shape, &values, &mut out).ok()?;
    out.truncate(n);
    Some(out)
}

fn column(d: &mut Draw<'_>, codec: Codec, shape: SampleShape, count: usize) -> Option<Vec<u8>> {
    match shape.element.scalar() {
        Element::I8 => coded::<i8>(d, codec, shape, count),
        Element::I16 => coded::<i16>(d, codec, shape, count),
        Element::I32 => coded::<i32>(d, codec, shape, count),
        Element::I64 => coded::<i64>(d, codec, shape, count),
        Element::U8 => coded::<u8>(d, codec, shape, count),
        Element::U16 => coded::<u16>(d, codec, shape, count),
        Element::U32 => coded::<u32>(d, codec, shape, count),
        Element::U64 => coded::<u64>(d, codec, shape, count),
        Element::F32 => coded::<f32>(d, codec, shape, count),
        Element::F64 => coded::<f64>(d, codec, shape, count),
        Element::Bool => coded::<bool>(d, codec, shape, count),
        _ => None,
    }
}

/// The fields of lane 0 of the group whose bytes start at `at`, for a scalar of `b` bytes.
fn lane_fields(codec: Codec, b: usize, at: usize) -> Vec<Field<'static>> {
    let width: &'static [u64] = &[8, 16, 32, 64];
    match codec {
        Codec::DELTA => vec![
            Field::new(at, b, &[]),
            Field::new(at + b, b, &[]),
            Field::new(at + 2 * b, 1, width),
        ],
        Codec::DELTA2 => vec![
            Field::new(at, b, &[]),
            Field::new(at + b, b, &[]),
            Field::new(at + 2 * b, b, &[]),
            Field::new(at + 3 * b, 1, width),
        ],
        // ALP: the scheme, then e, f, base, width, exceptions (decimal) or l, z, c, the dictionary (rd)
        _ => vec![
            Field::new(at, 1, &[0, 1, 2]),
            Field::new(at + 1, 1, &[10, 16, 21]),
            Field::new(at + 2, 1, &[3, 10, 21]),
            Field::new(at + 3, b, &[]),
            Field::new(at + 3, 1, &[2, 3, 4]),
            Field::new(at + 3 + b, 1, width),
            Field::new(at + 4 + b, 1, &[128, 255]),
        ],
    }
}

/// A describe block of a series track and one series chunk of a sample column whose table, group headers, count or
/// shape take hostile values.
pub fn samples(data: &[u8]) -> Vec<u8> {
    let mut d = Draw::new(data);
    let mut codec = CODECS[d.below(CODECS.len())];
    let mut element = Element::ALL[d.below(Element::ALL.len())];
    let mut channels = 1 + d.below(3) as u32;
    let per =
        (element.scalar_bits() as usize / 8).max(1) * element.parts() as usize * channels as usize;
    // half the columns of 64 samples at most, half up to the 1 KiB a block may take
    let most = 832 / per;
    let most = if d.bool() { most.min(64) } else { most };
    let count = 1 + d.below(most);
    let shape = SampleShape::new(element, channels);
    let col = match column(&mut d, codec, shape, count) {
        Some(c) => c,
        None => {
            codec = Codec::RAW;
            column(&mut d, codec, shape, count).expect("raw codes every element")
        }
    };
    let groups = count.div_ceil(ww_tape::samples::DELTA_GROUP);
    let mut count = count as u64;
    // the shape the track says, the count the chunk says
    match d.u8() % 6 {
        0 => {
            let caps = &[SERIES_CHANNELS_MAX as u64, 1 << 16];
            channels = d.value(32, caps, &[u64::from(channels)]) as u32;
        }
        1 => element = Element::from_code(d.u8() % 16).unwrap_or(element),
        2 => {
            let caps = &[u64::from(Codec::SERIES_ITEMS_MAX), 128, 256];
            count = d.value(32, caps, &[count]);
        }
        _ => {}
    }
    let head = ChunkHead {
        kind: BlockKind::Series,
        track_id: 1,
        track_number: 1,
        first_item: 0,
        count: count.max(1) as u32,
        first_tick: 0,
        last_tick: 0,
        latest_end: 0,
        time_mode: 0,
        end_mode: EndMode::Absent,
    };
    let spec = ColumnSpec {
        kind: ColumnKind::Samples,
        codec,
        param: 0,
        len: col.len(),
    };
    let mut b = build(&head, &[(spec, &col)]);
    let at = column_start(&b, 0).expect("the column is in the block");
    let scalar = (element.scalar_bits() as usize / 8).max(1);
    for _ in 0..1 + d.below(3) {
        match d.u8() % 8 {
            7 => hit_head(&mut d, &mut b),
            1 => {
                let at = at + d.below(col.len().max(1));
                hit(&mut d, &mut b, Field::new(at, 1, &[]));
            }
            2 | 3 => {
                let g = d.below(groups + 1);
                let len = [col.len() as u64];
                hit(&mut d, &mut b, Field::new(at + 4 * g, 4, &len));
            }
            _ if matches!(codec, Codec::DELTA | Codec::DELTA2 | Codec::ALP) => {
                let g = d.below(groups);
                let start = if g == 0 {
                    0
                } else {
                    super::get(&b, at + 4 * (g - 1), 4) as usize
                };
                let fields = lane_fields(codec, scalar, at + 4 * groups + start);
                let f = fields[d.below(fields.len())];
                hit(&mut d, &mut b, f);
            }
            _ => {}
        }
    }
    reseal(&mut b, d.u8() % 4 == 3);
    let period = Ratio64::new(1, 1).expect("a ratio");
    let def = SampleShape {
        element,
        channels: shrink_wrap::prelude::UNib32(channels),
    };
    let item = track_item(def, period, Magnitude::pow10(0));
    let mut out = describe_block(BlockId::new(RUN, 0), &[&item]);
    out.extend_from_slice(&b);
    out
}
