//! `NumericValueStore` (SW-62): every variant set and read back, and its size per feature set.

use proptest::prelude::*;
use shrink_wrap::series::SeriesStore;
use ww_numeric::{NumericValue, NumericValueStore};

/// Every variant enabled in this build, from a pick and a value.
#[allow(clippy::unnecessary_cast)]
fn value(pick: u8, v: u128, bits: u8) -> NumericValue {
    let all: &[fn(u128, u8) -> Option<NumericValue>] = &[
        |v, _| Some(NumericValue::U8(v as u8)),
        #[cfg(feature = "nibble")]
        |v, _| {
            Some(NumericValue::Nibble(shrink_wrap::Nibble::new_masked(
                v as u8,
            )))
        },
        #[cfg(feature = "u16")]
        |v, _| Some(NumericValue::U16(v as u16)),
        #[cfg(feature = "u32")]
        |v, _| Some(NumericValue::U32(v as u32)),
        #[cfg(feature = "unib32")]
        |v, _| Some(NumericValue::UNib32(shrink_wrap::UNib32(v as u32))),
        #[cfg(feature = "u64")]
        |v, _| Some(NumericValue::U64(v as u64)),
        #[cfg(feature = "i32")]
        |v, _| Some(NumericValue::I32(v as i32)),
        #[cfg(feature = "f32")]
        |v, _| Some(NumericValue::F32(f32::from_bits(v as u32))),
        #[cfg(feature = "un")]
        |v, b| {
            use shrink_wrap::un::{U3, U4, U5, U6, UN};
            Some(NumericValue::UN(match b % 4 {
                0 => UN::UN8 {
                    bit_count: U3::new(b % 8)?,
                    value: v as u8,
                },
                1 => UN::UN16 {
                    bit_count: U4::new(b % 16)?,
                    value: v as u16,
                },
                2 => UN::UN32 {
                    bit_count: U5::new(b % 32)?,
                    value: v as u32,
                },
                _ => UN::UN64 {
                    bit_count: U6::new(b % 64)?,
                    value: v as u64,
                },
            }))
        },
        #[cfg(feature = "in")]
        |v, b| {
            use shrink_wrap::un::{IN, U3, U4, U5, U6};
            Some(NumericValue::IN(match b % 4 {
                0 => IN::IN8 {
                    bit_count: U3::new(b % 8)?,
                    value: v as i8,
                },
                1 => IN::IN16 {
                    bit_count: U4::new(b % 16)?,
                    value: v as i16,
                },
                2 => IN::IN32 {
                    bit_count: U5::new(b % 32)?,
                    value: v as i32,
                },
                _ => IN::IN64 {
                    bit_count: U6::new(b % 64)?,
                    value: v as i64,
                },
            }))
        },
        #[cfg(feature = "u128")]
        |v, _| Some(NumericValue::U128(v)),
        #[cfg(feature = "i8")]
        |v, _| Some(NumericValue::I8(v as i8)),
        #[cfg(feature = "i16")]
        |v, _| Some(NumericValue::I16(v as i16)),
        #[cfg(feature = "i64")]
        |v, _| Some(NumericValue::I64(v as i64)),
        #[cfg(feature = "i128")]
        |v, _| Some(NumericValue::I128(v as i128)),
        #[cfg(feature = "f64")]
        |v, _| Some(NumericValue::F64(f64::from_bits(v as u64))),
        #[cfg(feature = "uvlq32")]
        |v, _| Some(NumericValue::UVlq32(shrink_wrap::UVlq32(v as u32))),
        #[cfg(feature = "uvlq64")]
        |v, _| Some(NumericValue::UVlq64(shrink_wrap::UVlq64(v as u64))),
    ];
    all[pick as usize % all.len()](v, bits).unwrap()
}

/// The same value, floats by their bits.
fn same(a: &NumericValue, b: &NumericValue) -> bool {
    match (a, b) {
        #[cfg(feature = "f32")]
        (NumericValue::F32(a), NumericValue::F32(b)) => a.to_bits() == b.to_bits(),
        #[cfg(feature = "f64")]
        (NumericValue::F64(a), NumericValue::F64(b)) => a.to_bits() == b.to_bits(),
        _ => a == b,
    }
}

proptest! {
    #[test]
    fn set_get(items in prop::collection::vec(any::<(u8, u128, u8, usize)>(), 0..40)) {
        const N: usize = 8;
        let mut store = NumericValue::empty_store::<N>();
        let mut model = [NumericValue::U8(0); N];
        for (i, slot) in model.iter().enumerate() {
            prop_assert!(same(&NumericValue::get(&store, i), slot));
        }
        for &(pick, v, bits, at) in &items {
            let x = value(pick, v, bits);
            NumericValue::set(&mut store, at % N, &x);
            model[at % N] = x;
            for (i, slot) in model.iter().enumerate() {
                let got = NumericValue::get(&store, i);
                prop_assert!(same(&got, slot), "at {}: {:?} != {:?}", i, got, slot);
            }
        }
    }
}

/// Every variant there is (the default `std` build): a byte for the variant, a 16-byte payload (`u128`), a byte for
/// `UN` / `IN` widths.
#[cfg(feature = "std")]
#[test]
fn size_all_variants() {
    assert_eq!(size_of::<NumericValueStore<32>>(), 32 * (1 + 16 + 1));
}

/// `U8`, `U32`, `I32` and `F32`: 5 bytes per value, against 8 of the enum
/// (`cargo test -p ww_numeric --no-default-features --features u32,i32,f32`, run by `just test`).
#[cfg(all(
    feature = "u32",
    feature = "i32",
    feature = "f32",
    not(any(
        feature = "u64",
        feature = "i64",
        feature = "f64",
        feature = "uvlq64",
        feature = "un",
        feature = "in",
        feature = "u128",
        feature = "i128"
    ))
))]
#[test]
fn size_up_to_32_bits() {
    assert_eq!(size_of::<NumericValueStore<32>>(), 32 * 5);
    assert_eq!(size_of::<NumericValue>(), 8);
}

/// Rows written column by column ([write_store_series]) against the item walk: the same bytes under each policy,
/// and declined where the variant changes or for `UN` / `IN`.
fn store_vs_walk<P: shrink_wrap::series::SeriesPolicy + Copy>(items: &[NumericValue], policy: P) {
    use shrink_wrap::series::{Series, write_store_series};
    use shrink_wrap::{BufWriter, SerializeShrinkWrap};
    let mut store = NumericValue::empty_store::<40>();
    for (i, item) in items.iter().enumerate() {
        NumericValue::set(&mut store, i, item);
    }
    let mut a = [0u8; 2048];
    let mut wr = BufWriter::new(&mut a);
    let written =
        write_store_series::<NumericValue, P, 40>(&mut wr, &store, 0..items.len(), &policy)
            .unwrap();
    let one_variant = items
        .windows(2)
        .all(|w| w[0].discriminant() == w[1].discriminant());
    let bit_counts = items
        .iter()
        .any(|v| matches!(format!("{v:?}").get(..3), Some("UN(" | "IN(")));
    assert_eq!(
        written,
        items.is_empty() || one_variant && !bit_counts,
        "{items:?}"
    );
    if !written {
        return;
    }
    let a = wr.finish_and_take().unwrap().to_vec();
    let mut b = [0u8; 2048];
    let mut wr = BufWriter::new(&mut b);
    Series::with_policy(items, policy)
        .ser_shrink_wrap(&mut wr)
        .unwrap();
    assert_eq!(a, wr.finish_and_take().unwrap(), "{items:?}");
}

proptest! {
    #[test]
    fn store_writes_what_the_walk_writes(
        pick in any::<u8>(),
        values in prop::collection::vec(any::<(u128, u8)>(), 0..40),
        other in prop::option::of((any::<u8>(), 0..40usize)),
    ) {
        use shrink_wrap::series::{CodecPolicy, DeltaPacked, RawPlain};
        let mut items: Vec<NumericValue> = values.iter().map(|&(v, b)| value(pick, v, b)).collect();
        if let Some((p, at)) = other && at < items.len() {
            items[at] = value(p, 1, 3);
        }
        store_vs_walk(&items, CodecPolicy::SMALLEST);
        store_vs_walk(&items, DeltaPacked);
        store_vs_walk(&items, RawPlain);
    }
}

/// A device without `u64` that is sent a `U64` answers with an error instead of reading garbage or panicking: the
/// variant is compiled out and the bytes (the same as on a build that has it) read as `EnumVariantDisabled`.
#[cfg(all(feature = "u32", not(feature = "u64")))]
#[test]
fn u64_from_a_host_is_refused() {
    use shrink_wrap::{DeserializeShrinkWrap, Error};
    // discriminant 5 (U64) in the first nibble, then the 8 bytes of the number
    let bytes = [0x50, 0, 0, 0, 0, 0, 0, 0, 0x01, 0x20];
    let r = NumericValue::from_ww_bytes(&bytes);
    assert_eq!(r.err(), Some(Error::EnumVariantDisabled), "{r:?}");
}
