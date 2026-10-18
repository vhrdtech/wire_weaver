//! [SeriesStore::write_store]: rows of a store written column by column are the bytes of the same items written
//! as a `Series` by the item walk, under every policy; rows where a tag changes are left to the walk.

use super::columns::check::{FloatCodec, numeric_codecs};
use super::{
    AlignedPlain, CodecPolicy, DeltaPacked, RawPlain, Series, SeriesStore, write_store_series,
};
use crate::un::I12;
use crate::{BufWriter, Nibble, SerializeShrinkWrap, derive_shrink_wrap};
use proptest::prelude::*;

const N: usize = 16;

#[derive_shrink_wrap(owned, series, ww_repr = u4, derive(Clone, Copy, Debug, PartialEq))]
enum Value {
    Small(u8),
    Signed(i32),
    Float(f32),
    Wide { a: u64, b: I12 },
    Off,
}

#[derive_shrink_wrap(owned, series, final_structure, derive(Clone, Copy, Debug, PartialEq))]
struct Inner {
    v: Value,
    t: (u16, bool),
    arr: [i16; 2],
}

/// Evolvable: a head before its fields.
#[derive_shrink_wrap(owned, series, derive(Clone, Copy, Debug, PartialEq))]
struct Row {
    a: u32,
    b: i64,
    f: f64,
    o: Option<Inner>,
    n: Nibble,
    x: u128,
    flag: bool,
}

fn value(k: u8, x: u64) -> Value {
    match k % 5 {
        0 => Value::Small(x as u8),
        1 => Value::Signed(x as i32),
        2 => Value::Float(f32::from_bits(x as u32)),
        3 => Value::Wide {
            a: x.rotate_left(17),
            b: I12::new((x as i16) >> 4).unwrap(),
        },
        _ => Value::Off,
    }
}

/// Item `i` of a series whose `Option` and enum keep one variant (`some`, `k`) unless `vary` says where to change.
fn row(i: usize, some: bool, k: u8, vary: Option<usize>, x: u64, step: i64) -> Row {
    let changed = vary == Some(i);
    let v = (x as i64).wrapping_add(step.wrapping_mul(i as i64)) as u64;
    Row {
        a: v as u32,
        b: v as i64 ^ (i as i64) << 40,
        f: (v % 1000) as f64 * 0.25,
        o: (some != changed).then(|| Inner {
            v: value(if changed { k + 1 } else { k }, v),
            t: (v as u16, i.is_multiple_of(3)),
            arr: [v as i16, (v >> 16) as i16],
        }),
        n: Nibble::new_masked(v as u8),
        x: (v as u128) << 70 | i as u128,
        flag: i.is_multiple_of(5),
    }
}

fn policies() -> impl Iterator<Item = CodecPolicy> {
    let fixed = numeric_codecs().flat_map(|n| {
        [FloatCodec::Plain, FloatCodec::Xor, FloatCodec::Aligned]
            .into_iter()
            .map(move |f| CodecPolicy::fixed(n, f))
    });
    [CodecPolicy::SMALLEST].into_iter().chain(fixed)
}

/// `rows` of the store written both ways after `offset` bits: the same bytes, or the store declining exactly when
/// a tag changes.
fn same_as_walk<P: super::SeriesPolicy + Copy>(
    items: &[Row],
    start: usize,
    policy: P,
    offset: u8,
    tags_change: bool,
) {
    let mut store = Row::empty_store::<N>();
    for (i, item) in items.iter().enumerate() {
        Row::set(&mut store, start + i, item);
    }
    let rows = start..start + items.len();
    let mut a = [0u8; 4096];
    let mut wr = BufWriter::new(&mut a);
    wr.write_un32(offset, 0).unwrap();
    let written = write_store_series::<Row, P, N>(&mut wr, &store, rows, &policy).unwrap();
    assert_eq!(written, !tags_change || items.len() < 2);
    if !written {
        assert_eq!(wr.pos(), (0, 7 - offset));
        return;
    }
    let a = wr.finish_and_take().unwrap().to_vec();
    let mut b = [0u8; 4096];
    let mut wr = BufWriter::new(&mut b);
    wr.write_un32(offset, 0).unwrap();
    Series::with_policy(items, policy)
        .ser_shrink_wrap(&mut wr)
        .unwrap();
    let b = wr.finish_and_take().unwrap();
    assert_eq!(a, b);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn store_writes_what_the_walk_writes(
        len in 0..=N,
        start in 0..N,
        some in any::<bool>(),
        k in 0..5u8,
        vary in prop::option::of(0..N),
        x in any::<u64>(),
        step in prop_oneof![Just(0i64), -3..3i64, any::<i64>()],
        offset in 0..8u8,
    ) {
        let start = start.min(N - len);
        let vary = vary.filter(|&v| v < len && v > 0);
        let items: Vec<Row> = (0..len).map(|i| row(i, some, k, vary, x, step)).collect();
        for policy in policies() {
            same_as_walk(&items, start, policy, offset, vary.is_some());
        }
        same_as_walk(&items, start, DeltaPacked, offset, vary.is_some());
        same_as_walk(&items, start, RawPlain, offset, vary.is_some());
        same_as_walk(&items, start, AlignedPlain, offset, vary.is_some());
    }
}
