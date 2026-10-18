//! `Series<T>` (SW-60) of plain numbers as fields of derived types, next to other fields.

use shrink_wrap::prelude::*;
use shrink_wrap::series::{CodecPolicy, Coder, FloatCodec, NumericCodec, Predictor, SeriesColumns};

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq))]
struct Block<'i> {
    ts: u32,
    size: TailSize<2>,
    stamps: Series<'i, u64>,
    counts: Series<'i, i16>,
    temps: Series<'i, f32>,
    flag: bool,
    note: Option<Series<'i, u8>>,
}

#[derive_shrink_wrap(borrowed, owned, ww_repr = u4, derive(Debug, PartialEq))]
enum Column<'i> {
    Percent(Series<'i, i32>),
    Bytes { v: Series<'i, i64> },
    Float(Series<'i, f64>),
    None,
}

#[test]
fn struct_round_trip_and_garbage_after() {
    let stamps = [
        1_700_000_000u64,
        1_700_000_060,
        1_700_000_120,
        1_700_000_181,
    ];
    let counts = [5i16, 5, 6, 4, -300];
    let temps = [21.5f32, 21.5, 21.75, f32::NAN, 22.0];
    let note = [1u8, 2, 3];
    let block = Block {
        ts: 42,
        size: TailSize(0),
        stamps: Series::new(&stamps),
        counts: Series::new(&counts),
        temps: Series::new(&temps),
        flag: true,
        note: Some(Series::new(&note)),
    };
    let mut region = [0xA5u8; 128]; // a file region longer than the value
    let len = block.to_ww_bytes(&mut region).unwrap().len();
    assert!(len < 60, "{len}");

    let owned = BlockOwned::from_ww_bytes_owned(&region).unwrap(); // bounded by the TailSize slot
    assert_eq!(owned.ts, 42);
    assert_eq!(owned.stamps, SeriesOwned(stamps.to_vec()));
    assert_eq!(owned.counts, SeriesOwned(counts.to_vec()));
    assert_eq!(owned.temps.len(), temps.len());
    assert!(
        owned
            .temps
            .iter()
            .map(|v| v.to_bits())
            .eq(temps.iter().map(|v| v.to_bits()))
    );
    assert!(owned.flag);
    assert_eq!(owned.note, Some(SeriesOwned(note.to_vec())));
    assert_eq!(owned.to_ww_bytes_owned().unwrap(), region[..len]);

    let back = Block::from_ww_bytes(&region).unwrap();
    assert_eq!(back.stamps.iter().collect::<Vec<_>>(), stamps);
    assert_eq!(back.counts.iter().collect::<Vec<_>>(), counts);
    assert_eq!(back.temps.len(), 5);
    assert!(back.flag);
    assert_eq!(back.note.unwrap().iter().collect::<Vec<_>>(), note);
    assert_eq!(back.counts, Series::new(&counts)); // Buf vs Slice compare by value

    // a truncated region is rejected by the slot, whatever the bit codes inside would decode to
    for cut in 7..len {
        assert!(Block::from_ww_bytes(&region[..cut]).is_err(), "cut {cut}");
    }
}

#[test]
fn enum_variants() {
    let mut buf = [0u8; 64];
    let cases = [
        ColumnOwned::Percent(SeriesOwned(vec![100, 101, 101, 99])),
        ColumnOwned::Bytes {
            v: SeriesOwned(vec![8_000_000_000, 8_000_004_096]),
        },
        ColumnOwned::Float(SeriesOwned(vec![0.1, 0.1, 0.2])),
        ColumnOwned::None,
    ];
    for c in cases {
        let bytes = c.to_ww_bytes_owned().unwrap();
        assert_eq!(ColumnOwned::from_ww_bytes_owned(&bytes).unwrap(), c);
        let borrowed = Column::from_ww_bytes(&bytes).unwrap();
        assert_eq!(borrowed.to_ww_bytes(&mut buf).unwrap(), bytes);
    }
}

#[test]
fn random_bytes_never_panic() {
    // a cheap stand-in for the fuzz target: deterministic pseudo-random buffers through every decoder
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    for _ in 0..2000 {
        let mut buf = [0u8; 24];
        for b in &mut buf {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = x as u8;
        }
        let len = (x >> 8) as usize % buf.len();
        let _ = Block::from_ww_bytes(&buf[..len]);
        let _ = BlockOwned::from_ww_bytes_owned(&buf[..len]);
        let _ = Column::from_ww_bytes(&buf[..len]);
        let _ = Series::<u8>::from_ww_bytes(&buf[..len]);
        let _ = Series::<i64>::from_ww_bytes(&buf[..len]);
        let _ = SeriesOwned::<f64>::from_ww_bytes_owned(&buf[..len]);
    }
}

/// Every fixed codec (and the smallest), both float codecs.
fn policies() -> Vec<CodecPolicy> {
    let mut out = vec![CodecPolicy::SMALLEST];
    for predictor in [Predictor::Raw, Predictor::Delta, Predictor::DeltaOfDelta] {
        for coder in [Coder::Prefix, Coder::Packed, Coder::Constant, Coder::Plain] {
            for float in [FloatCodec::Plain, FloatCodec::Xor] {
                out.push(CodecPolicy::fixed(
                    NumericCodec::new(predictor, coder),
                    float,
                ));
            }
        }
    }
    out
}

fn round_trip<T>(values: &[T]) -> usize
where
    T: for<'i> SeriesColumns<'i> + Clone + PartialEq + core::fmt::Debug,
    T: SerializeShrinkWrapOwned + DeserializeShrinkWrapOwned,
{
    let mut smallest = usize::MAX;
    for policy in policies() {
        let bytes = Series::with_policy(values, policy)
            .to_ww_bytes_owned()
            .unwrap();
        let back = SeriesOwned::<T>::from_ww_bytes_owned(&bytes).unwrap();
        assert_eq!(back.0, values, "{policy:?}");
        smallest = smallest.min(bytes.len());
    }
    let bytes = Series::new(values).to_ww_bytes_owned().unwrap();
    assert_eq!(bytes.len(), smallest, "SMALLEST is the smallest");
    bytes.len()
}

/// The extremes of every integer width, with every codec: differences wrap at the width.
#[test]
fn extremes_every_type() {
    round_trip::<u8>(&[0, 255, 0, 128, 127, 1]);
    round_trip::<i8>(&[-128, 127, -128, 0, -1, 1]);
    round_trip::<u16>(&[0, u16::MAX, 1, 0x8000]);
    round_trip::<i16>(&[i16::MIN, i16::MAX, 0, -1]);
    round_trip::<u32>(&[0, u32::MAX, 1, 0x8000_0000, 0x7FFF_FFFF]);
    round_trip::<i32>(&[i32::MIN, i32::MAX, 0, -1, 1]);
    round_trip::<u64>(&[0, u64::MAX, 1, 1 << 63, (1 << 63) - 1, 12345]);
    round_trip::<i64>(&[i64::MIN, i64::MAX, 0, -1, 1, i64::MIN + 1]);
    round_trip::<u8>(&[10, 10, 11, 9, 200]);
}

/// A regular step costs nothing per item past the first two: delta of delta with the constant coder.
#[test]
fn regular_step_is_nearly_free() {
    let ts: Vec<u64> = (0..100).map(|i| 1_700_000_000 + i * 60).collect();
    // count 100 (3 nibbles) + header 4 + first 64 + the step 60 in the prefix code (10 bits): 90 bits
    assert_eq!(round_trip(&ts), 12);
    // raw with the constant coder: count, header, 42 once in the prefix code (9 bits)
    assert_eq!(round_trip(&[42u32; 63]), 3);
}
