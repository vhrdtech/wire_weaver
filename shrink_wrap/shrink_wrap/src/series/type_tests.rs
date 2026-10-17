use super::golden_signals::{NOISY_I_BASE, NOISY_V_BASE};
use super::type_check::*;
use super::{CodecPolicy, Series, SeriesOwned};
use crate::{
    DeserializeShrinkWrap, DeserializeShrinkWrapOwned, Error, SerializeShrinkWrap,
    SerializeShrinkWrapOwned,
};
use hex_literal::hex;
use proptest::prelude::*;

fn bytes_of<'i, T: super::SeriesColumns<'i> + 'i>(items: &'i [T]) -> Vec<u8> {
    let bytes = Series::new(items).to_ww_bytes_owned().unwrap();
    assert_within_bound::<T>(items.len(), &bytes);
    bytes
}

fn iv(v: (Prefix, NumericValue), i: (Prefix, NumericValue)) -> IvSample {
    IvSample {
        v: Quantity::new(v.0, v.1),
        i: Quantity::new(i.0, i.1),
    }
}

fn mv(x: u16) -> (Prefix, NumericValue) {
    (Prefix::Milli, NumericValue::U16(x))
}

fn ua(x: i32) -> (Prefix, NumericValue) {
    (Prefix::Micro, NumericValue::I32(x))
}

/// The design's first example, byte for byte.
#[test]
fn design_example_steady() {
    let items: Vec<IvSample> = [3300, 3301, 3301, 3299]
        .into_iter()
        .zip([152_000, 152_002, 151_999, 152_001])
        .map(|(v, i)| iv(mv(v), ua(i)))
        .collect();
    let bytes = bytes_of(&items);
    assert_eq!(bytes, hex!("42 11 14 33 90 51 91 8a 00 04 a3 80 1c b0"));
    let back: Vec<IvSample> = Series::<IvSample>::from_ww_bytes(&bytes)
        .unwrap()
        .iter()
        .collect();
    assert_eq!(back, items);
}

/// The design's second example: a range and storage switch at item 2, each variant's column on its own.
#[test]
fn design_example_switch() {
    let items = vec![
        iv(mv(3300), ua(480)),
        iv(mv(3300), ua(482)),
        iv(mv(3301), (Prefix::Milli, NumericValue::U16(120))),
        iv(mv(3300), (Prefix::Milli, NumericValue::U16(121))),
    ];
    let bytes = bytes_of(&items);
    assert_eq!(
        bytes,
        hex!("42 11 14 33 90 44 94 44 d1 20 5e 2f 22 57 81 e2 00")
    );
    let back = SeriesOwned::<IvSample>::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!(back.0, items);
}

/// The reference encoder's random numbers (design/series_proto).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn uni(&mut self, a: i64) -> i64 {
        (self.next() % (2 * a as u64 + 1)) as i64 - a
    }

    fn gauss(&mut self, s: f64) -> i64 {
        let mut acc = 0.0;
        for _ in 0..4 {
            acc += (self.next() % 10_000) as f64 / 10_000.0 - 0.5;
        }
        (acc * s * (12.0f64 / 4.0).sqrt() * 2.0 / 2.0).round() as i64
    }
}

/// The design's cases, generated exactly as the reference encoder does: steady, noisy, range switches, range and
/// storage switches, flat with spikes.
fn cases(n: usize, seed: u64) -> Vec<Vec<IvSample>> {
    let mut r = Rng(seed);
    let steady_v: Vec<_> = (0..n).map(|_| mv((3300 + r.uni(1)) as u16)).collect();
    let steady_i: Vec<_> = (0..n).map(|_| ua((152_000 + r.uni(2)) as i32)).collect();
    let noisy_v: Vec<_> = (0..n)
        .map(|k| mv((NOISY_V_BASE[k] + r.gauss(8.0)) as u16))
        .collect();
    let noisy_i: Vec<_> = (0..n)
        .map(|k| ua(NOISY_I_BASE[k] + r.gauss(300.0) as i32))
        .collect();
    let (a, b) = (n * 3 / 10, n * 7 / 10);
    let switch_v: Vec<_> = (0..n).map(|_| mv((3300 + r.uni(1)) as u16)).collect();
    let switch_i: Vec<_> = (0..n)
        .map(|k| {
            if (a..b).contains(&k) {
                (Prefix::Milli, NumericValue::I32((120 + r.uni(1)) as i32))
            } else {
                ua((480 + r.uni(3)) as i32)
            }
        })
        .collect();
    let switch_ty_i: Vec<_> = switch_i
        .iter()
        .map(|&(p, v)| match (p, v) {
            (Prefix::Milli, NumericValue::I32(x)) => (p, NumericValue::U16(x as u16)),
            _ => (p, v),
        })
        .collect();
    let spiky_v: Vec<_> = (0..n)
        .map(|k| mv(if k % 25 == 12 { 3900 } else { 3300 }))
        .collect();
    let spiky_i: Vec<_> = (0..n)
        .map(|k| ua(if k % 25 == 12 { 900_000 } else { 150_000 }))
        .collect();
    let zip = |v: &[(Prefix, NumericValue)], i: &[(Prefix, NumericValue)]| -> Vec<IvSample> {
        v.iter().zip(i).map(|(&v, &i)| iv(v, i)).collect()
    };
    vec![
        zip(&steady_v, &steady_i),
        zip(&noisy_v, &noisy_i),
        zip(&switch_v, &switch_i),
        zip(&switch_v, &switch_ty_i),
        zip(&spiky_v, &spiky_i),
    ]
}

/// FNV-1a 64, as the reference encoder prints it.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

fn check_golden<T>(items: &[T], size: usize, hash: u64, what: &str)
where
    T: for<'i> super::SeriesColumns<'i> + Clone + PartialEq + core::fmt::Debug,
{
    let bytes = Series::new(items).to_ww_bytes_owned().unwrap();
    assert_eq!((bytes.len(), fnv(&bytes)), (size, hash), "{what}");
    assert_within_bound::<T>(items.len(), &bytes);
    let back = SeriesOwned::<T>::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!(back.0, items, "{what}");
}

/// The design's first size table (100 items, column "Series"): sizes and bytes of the reference encoder.
#[test]
fn design_sizes_100() {
    let golden = [
        (104, 0x5d15e5657f4c4cf7),
        (251, 0xb825c6146de5631b),
        (110, 0x8a0350a01f3d3b8e),
        (111, 0xe993fb282244b171),
        (88, 0x82c94e8f8c51ac86),
    ];
    for (k, (items, (size, hash))) in cases(100, 0x2545F4914F6CDD1D)
        .iter()
        .zip(golden)
        .enumerate()
    {
        check_golden(items, size, hash, &format!("case {k}"));
    }
}

/// The design's `Series<Timed<IvSample>>` table: regular and jittered 1 kHz time.
#[test]
fn design_sizes_timed() {
    let n = 100;
    let mut r = Rng(77);
    let t_regular: Vec<i64> = (0..n).map(|k| 1_000_000 + k as i64 * 1000).collect();
    let t_jitter: Vec<i64> = (0..n)
        .map(|k| 1_000_000 + k as i64 * 1000 + r.uni(3))
        .collect();
    let golden = [
        [
            (113, 0x7dd762b66090c312),
            (260, 0x0fd185a9a56bfb46),
            (120, 0x84703b803fb1e043),
        ],
        [
            (177, 0x772d2980220f1981),
            (325, 0x0819d1d2bcad9afc),
            (184, 0x632ebd05fb2b9e50),
        ],
    ];
    let cs = cases(n, 0x2545F4914F6CDD1D);
    for (t, golden) in [&t_regular, &t_jitter].into_iter().zip(golden) {
        for (c, (size, hash)) in cs.iter().zip(golden) {
            let items: Vec<TimedIv> = c
                .iter()
                .zip(t)
                .map(|(&value, &t)| TimedIv {
                    t: Quantity::new(Prefix::Micro, NumericValue::U32(t as u32)),
                    value,
                })
                .collect();
            check_golden(&items, size, hash, "timed");
        }
    }
}

/// The design's bytes per item by batch size (steady, noisy, range switches).
#[test]
fn design_sizes_by_n() {
    let golden: [(usize, [(usize, u64); 3]); 9] = [
        (
            1,
            [
                (10, 0x306995bb4733db5a),
                (10, 0x3e286f392be8ed5e),
                (9, 0x3a6688fcce61249f),
            ],
        ),
        (
            2,
            [
                (12, 0x55dde8f15d6e407b),
                (14, 0x9bd464e46589c321),
                (12, 0x2999994771de03fc),
            ],
        ),
        (
            4,
            [
                (14, 0x28aa2c3465b5d1ea),
                (19, 0xb5d2bdea98c94968),
                (16, 0xce35fd9f42ba87ce),
            ],
        ),
        (
            8,
            [
                (17, 0xd5135f3581958f72),
                (29, 0xf765a73812afd713),
                (23, 0x2cdc4cf739649bf8),
            ],
        ),
        (
            16,
            [
                (25, 0x7372666afd5ebb84),
                (47, 0x360afc65872ebd61),
                (36, 0xa1d0771c1a0246d6),
            ],
        ),
        (
            32,
            [
                (39, 0x0cd25f471cd40ccb),
                (86, 0xdfbdd4bf2b07c703),
                (50, 0xce40c428fff3e610),
            ],
        ),
        (
            64,
            [
                (69, 0x86e60b76798eb717),
                (165, 0xd9e1acffda0114c6),
                (81, 0x97ee2761e8b9dfc0),
            ],
        ),
        (
            128,
            [
                (126, 0xf280360d79832285),
                (319, 0xf37a997cab940d4b),
                (135, 0xa35a74a3798d91bb),
            ],
        ),
        (
            256,
            [
                (241, 0xea50eeb66cf47694),
                (623, 0x93a0e1ebedfdbb26),
                (254, 0xfc53a6f13bd32ffa),
            ],
        ),
    ];
    for (n, golden) in golden {
        for (items, (size, hash)) in cases(n, 0x1234_5678_9abc_def1).iter().zip(golden) {
            check_golden(items, size, hash, &format!("n = {n}"));
        }
    }
}

#[test]
fn empty_series() {
    let bytes = bytes_of::<IvSample>(&[]);
    assert_eq!(bytes, [0]);
    let s = Series::<IvSample>::from_ww_bytes(&bytes).unwrap();
    assert!(s.is_empty());
    assert_eq!(s.iter().count(), 0);
}

#[test]
fn count_cap() {
    let items = vec![0u8; super::SERIES_MAX_ITEMS + 1];
    assert_eq!(
        Series::new(&items).to_ww_bytes_owned(),
        Err(Error::LenTooLong)
    );
    let ok = &items[..super::SERIES_MAX_ITEMS];
    let bytes = Series::new(ok).to_ww_bytes_owned().unwrap();
    // a constant column: n, the header and the first value
    assert!(bytes.len() < 8, "{bytes:?}");
    let back = SeriesOwned::<u8>::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!(back.len(), super::SERIES_MAX_ITEMS);
    // n = 65 536 (UNib32: 0b1000 groups, least significant first): rejected before anything else
    let mut too_many = Vec::new();
    let mut wr = crate::BufWriterOwned::new();
    wr.write_unib32(65_536).unwrap();
    too_many.extend(wr.finish_and_take().unwrap());
    too_many.extend([0u8; 8]);
    assert_eq!(
        Series::<u8>::from_ww_bytes(&too_many).err(),
        Some(Error::MalformedSeries)
    );
}

/// An older reader of a newer writer's outermost struct stops after the columns it knows; a newer reader of an
/// older writer fills the default into the appended field.
#[test]
fn evolution_outermost() {
    let v1: Vec<IvSample> = cases(20, 7).swap_remove(2);
    let v2: Vec<IvSampleV2> = v1
        .iter()
        .enumerate()
        .map(|(k, s)| IvSampleV2 {
            v: s.v,
            i: s.i,
            range_ok: if k % 3 == 0 { None } else { Some(k % 2 == 0) },
        })
        .collect();

    let new_bytes = bytes_of(&v2);
    let old_read: Vec<IvSample> = Series::<IvSample>::from_ww_bytes(&new_bytes)
        .unwrap()
        .iter()
        .collect();
    assert_eq!(old_read, v1);
    // a decoded Series keeps the unknown columns when serialized again
    let mut buf = [0u8; 256];
    let again = Series::<IvSample>::from_ww_bytes(&new_bytes).unwrap();
    assert_eq!(again.to_ww_bytes(&mut buf).unwrap(), &new_bytes[..]);

    let old_bytes = bytes_of(&v1);
    let new_read = SeriesOwned::<IvSampleV2>::from_ww_bytes_owned(&old_bytes).unwrap();
    for (a, b) in new_read.iter().zip(&v2) {
        assert_eq!((a.v, a.i, a.range_ok), (b.v, b.i, None));
    }
    assert_eq!(new_read.len(), v2.len());
}

/// A nested evolvable struct: an older reader skips the columns it doesn't know by the group length, a newer one
/// sees the group end and fills the defaults. The string in the new field aligns to a byte inside the group.
#[test]
fn evolution_nested() {
    let v2: Vec<OuterV2Owned> = (0..30)
        .map(|k| OuterV2Owned {
            a: k as u8,
            inner: InnerV2Owned {
                x: 1000 + k * 3,
                unit: if k < 12 { Prefix::Milli } else { Prefix::Micro },
                label: if k % 7 == 0 {
                    "seven".into()
                } else {
                    "x".into()
                },
            },
            b: -(k as i32) * 100,
        })
        .collect();
    let v1: Vec<OuterV1> = v2
        .iter()
        .map(|o| OuterV1 {
            a: o.a,
            inner: InnerV1 { x: o.inner.x },
            b: o.b,
        })
        .collect();

    let new_bytes = bytes_of(&v2);
    let old_read = SeriesOwned::<OuterV1>::from_ww_bytes_owned(&new_bytes).unwrap();
    assert_eq!(old_read.0, v1);

    let old_bytes = bytes_of(&v1);
    let new_read = SeriesOwned::<OuterV2Owned>::from_ww_bytes_owned(&old_bytes).unwrap();
    let expected: Vec<OuterV2Owned> = v1
        .iter()
        .map(|o| OuterV2Owned {
            a: o.a,
            inner: InnerV2Owned {
                x: o.inner.x,
                ..InnerV2Owned::default()
            },
            b: o.b,
        })
        .collect();
    assert_eq!(new_read.0, expected);
}

/// A tag the reader doesn't know fails as a plain decode of that item would.
#[test]
fn unknown_tag() {
    // NumericValue with tag 4 (UNib32 in ww_numeric), unknown to the test type: a U8 column, then the tag patched
    let items = [Quantity::new(Prefix::Unit, NumericValue::U8(5))];
    let bytes = bytes_of(&items);
    // n = 1 (0001), prefix run: 0000 0, value tag run: 0001 0 -> patch the tag nibble to 0100
    let mut bad = bytes.clone();
    assert_eq!(bad[0], 0b0001_0000);
    assert_eq!(bad[1] & 0b0111_1000, 0b0000_1000);
    bad[1] = (bad[1] & !0b0111_1000) | 0b0010_0000;
    assert_eq!(
        Series::<Quantity>::from_ww_bytes(&bad).err(),
        Some(Error::EnumFutureVersionOrMalformedData)
    );
}

/// Restarts from an earlier enum field (the prefix) and on entering a variant (`Some` again), seen through the
/// delta + prefix code, where a restart is a full value and a delta across it would be a long residual.
#[test]
fn restarts_from_earlier_enum_and_variant_entry() {
    let delta_prefix = CodecPolicy::fixed(
        super::NumericCodec::new(super::Predictor::Delta, super::Coder::Prefix),
        super::FloatCodec::Plain,
    );
    // value I32 throughout, the prefix switches at item 2
    let items = vec![
        Quantity::new(Prefix::Micro, NumericValue::I32(480)),
        Quantity::new(Prefix::Micro, NumericValue::I32(482)),
        Quantity::new(Prefix::Milli, NumericValue::I32(120)),
        Quantity::new(Prefix::Milli, NumericValue::I32(121)),
    ];
    let bytes = Series::with_policy(&items, delta_prefix)
        .to_ww_bytes_owned()
        .unwrap();
    // n 4, prefix run 4 + 1 + 4 + 4 + 1, tag run 4 + 1, header 4, 480 in 32, +2 in 9, 120 in 32 (a restart, not a
    // 12-bit residual of -362), +1 in 9: 109 bits
    assert_eq!(bytes.len(), 14);
    let back = SeriesOwned::<Quantity>::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!(back.0, items);

    // n 4, tag run 4 + (1 + 4) x 2 + 1, header 4, 1000 in 16, 5000 in 16 (a restart, not a 25-bit residual), +1 in
    // 9: 64 bits
    let opts = [Some(1000u16), None, Some(5000), Some(5001)];
    let bytes = Series::with_policy(&opts, delta_prefix)
        .to_ww_bytes_owned()
        .unwrap();
    assert_eq!(bytes.len(), 8);
    let back = SeriesOwned::<Option<u16>>::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!(back.0, opts);
}

#[test]
fn kitchen_round_trips() {
    for seed in 0..64u64 {
        let bytes: Vec<u8> = (0..2000u64)
            .map(|k| {
                (k.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ seed.wrapping_mul(0xBF58_476D_1CE4_E5B9))
                    .to_le_bytes()[7]
            })
            .collect();
        let mut picks = Picks::new(&bytes);
        let items = kitchen_items(&mut picks, 60);
        for pick in [0u8, 128, 129, 140, 200, 255] {
            round_trip(&items, policy(pick));
        }
    }
}

#[test]
fn primitive_round_trips() {
    fn check<T>(items: &[T])
    where
        T: for<'i> super::SeriesColumns<'i> + Clone + PartialEq + core::fmt::Debug,
    {
        let bytes = Series::new(items).to_ww_bytes_owned().unwrap();
        let back = SeriesOwned::<T>::from_ww_bytes_owned(&bytes).unwrap();
        assert_eq!(back.0, items);
        assert_within_bound::<T>(items.len(), &bytes);
    }
    check(&[1u8, 2, 3]);
    check(&[-1i64, i64::MIN, i64::MAX, 0]);
    check(&[u128::MAX, 0, 7]);
    check(&[i128::MIN, -1, 1]);
    check(&[f64::MAX, 1.5, -0.0]);
    check(&[true, true, false]);
    check(&[crate::UNib32(7), crate::UNib32(u32::MAX)]);
    check(&[crate::UVlq64(u64::MAX), crate::UVlq64(1)]);
    check(&[
        crate::un::I12::new(-2048).unwrap(),
        crate::un::I12::new(2047).unwrap(),
    ]);
    check(&[(1u8, true), (2, false)]);
    check(&[[Some(1u8), None], [None, Some(3)]]);
    check(&["a".to_string(), "".into(), "a".into()]);
    check(&[vec![1u8, 2], vec![]]);
    check(&[Some(Some(3u8)), Some(None), None]);
    use crate::un::{IN, U3, U5, UN};
    check(&[
        UN::UN8 {
            bit_count: U3::new(3).unwrap(),
            value: 5,
        },
        UN::UN8 {
            bit_count: U3::new(3).unwrap(),
            value: 6,
        },
        UN::UN32 {
            bit_count: U5::new(20).unwrap(),
            value: 1 << 19,
        },
        UN::UN8 {
            bit_count: U3::new(1).unwrap(),
            value: 1,
        },
    ]);
    check(&[
        IN::IN32 {
            bit_count: U5::new(17).unwrap(),
            value: -65_000,
        },
        IN::IN8 {
            bit_count: U3::new(4).unwrap(),
            value: -8,
        },
    ]);
}

/// Written with `policy`, within the bound and read back the same (compared as bytes: NaN floats).
fn within_bound<T>(items: &[T], policy: CodecPolicy)
where
    T: for<'i> super::SeriesColumns<'i> + Clone + SerializeShrinkWrapOwned + core::fmt::Debug,
{
    let series = Series::with_policy(items, policy);
    let bytes = series.to_ww_bytes_owned().unwrap();
    let max = Series::<T>::max_bytes(items.len()).expect("bounded");
    assert!(bytes.len() <= max, "{} bytes > {max}", bytes.len());
    assert_eq!(series.encoded_len(), Ok(bytes.len()), "counted");
    let back = SeriesOwned::<T>::from_ww_bytes_owned(&bytes).unwrap();
    for (a, b) in back.iter().zip(items) {
        assert_eq!(
            a.to_ww_bytes_owned().unwrap(),
            b.to_ww_bytes_owned().unwrap()
        );
    }
}

#[test]
fn bounds_of_types() {
    use super::bound::*;
    type Ser<T> = Series<'static, T>;
    assert_eq!(<u32 as super::SeriesColumns>::MAX_ITEM_BITS, Some(38));
    assert_eq!(<bool as super::SeriesColumns>::MAX_FIXED_BITS, Some(2));
    // Option: the tag run around the value
    assert_eq!(
        <Option<u8> as super::SeriesColumns>::MAX_ITEM_BITS,
        Some(OPTION_TAG_ITEM_BITS + numeric_item_bits(8))
    );
    // three variants (discriminants up to 2): first value 4 bits, a change 1 + 4 + 4
    assert_eq!(<Prefix as super::SeriesColumns>::MAX_ITEM_BITS, Some(9));
    assert_eq!(<Prefix as super::SeriesColumns>::MAX_FIXED_BITS, Some(5));
    // NumericValue: the tag (up to 7), the widest variant (32 bits, numeric beats float), a header per variant
    assert_eq!(
        <NumericValue as super::SeriesColumns>::MAX_ITEM_BITS,
        Some(9 + float_item_bits(32, 5))
    );
    assert_eq!(
        <NumericValue as super::SeriesColumns>::MAX_FIXED_BITS,
        Some(5 + 5 * 4)
    );
    // an evolvable struct: its head
    assert_eq!(
        <IvSample as super::SeriesColumns>::MAX_FIXED_BITS,
        Some(STRUCT_HEAD_BITS + 2 * (5 + 25))
    );
    assert_eq!(Ser::<&str>::max_bytes(1), None);
    assert_eq!(Ser::<u8>::max_bytes(0), Some(1));
    assert_eq!(Ser::<u8>::max_bytes(super::SERIES_MAX_ITEMS + 1), None);
    // 32 items: n in 8 bits, 4 + 32 x 12 bits, padding
    assert_eq!(
        Ser::<u8>::max_bytes(32),
        Some((8 + 4 + 32 * 12usize).div_ceil(8))
    );
    // Quantity: Prefix 9 + NumericValue 53 per item, 5 + 25 fixed; IvSample evolvable (+44); Timed of the three
    assert_eq!(
        <TimedIv as super::SeriesColumns>::MAX_ITEM_BITS,
        Some(3 * 62)
    );
    assert_eq!(
        <TimedIv as super::SeriesColumns>::MAX_FIXED_BITS,
        Some(3 * 30 + 44)
    );
    assert_eq!(
        Ser::<TimedIv>::max_bytes(32),
        Some((8 + 134 + 32 * 186usize).div_ceil(8))
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn random_items_within_bound(
        items in prop::collection::vec(any::<(u8, u32, i64, u64, bool, u8)>(), 0..200),
        pick in any::<u8>(),
    ) {
        use crate::un::{I12, U5};
        let policy = policy(pick);
        let n: Vec<(u8, u32, i64)> = items.iter().map(|t| (t.0, t.1, t.2)).collect();
        within_bound(&n, policy);
        let f: Vec<(f32, f64)> = items
            .iter()
            .map(|t| (f32::from_bits(t.1), f64::from_bits(t.3)))
            .collect();
        within_bound(&f, policy);
        let big: Vec<u128> = items.iter().map(|t| (t.3 as u128) << 64 | t.1 as u128).collect();
        within_bound(&big, policy);
        let o: Vec<Option<(u8, bool)>> =
            items.iter().map(|t| (t.5 % 3 != 0).then_some((t.0, t.4))).collect();
        within_bound(&o, policy);
        let a: Vec<[Option<i16>; 3]> = items
            .iter()
            .map(|t| [Some(t.2 as i16), (t.4).then_some(t.1 as i16), None])
            .collect();
        within_bound(&a, policy);
        let w: Vec<(U5, I12, crate::Nibble, crate::UNib32)> = items
            .iter()
            .map(|t| {
                (
                    U5::new(t.0 % 32).unwrap(),
                    I12::new((t.1 % 4096) as i16 - 2048).unwrap(),
                    crate::Nibble::new_masked(t.5),
                    crate::UNib32(t.1),
                )
            })
            .collect();
        within_bound(&w, policy);
        let q: Vec<Quantity> = items
            .iter()
            .map(|t| {
                let v = match t.5 % 5 {
                    0 => NumericValue::U8(t.0),
                    1 => NumericValue::U16(t.1 as u16),
                    2 => NumericValue::U32(t.1),
                    3 => NumericValue::I32(t.2 as i32),
                    _ => NumericValue::F32(f32::from_bits(t.1)),
                };
                let p = [Prefix::Unit, Prefix::Milli, Prefix::Micro][t.0 as usize % 3];
                Quantity::new(p, v)
            })
            .collect();
        within_bound(&q, policy);
    }

    #[test]
    fn fuzz_like(bytes in prop::collection::vec(any::<u8>(), 0..600)) {
        fuzz_one(&bytes);
    }

    #[test]
    fn random_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..128)) {
        random_bytes(&bytes);
    }
}
