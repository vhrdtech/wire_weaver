#![no_main]

//! `Delta`, `DeltaOfDelta` and `XorFloat` (SW-31, SW-32): arbitrary integer / float sequences round-trip
//! bit-exactly through the borrowed and `Owned` encode/decode paths, also as fields of a derive struct with a
//! `TailSize<2>` slot and garbage appended after the value; decoders on random bytes never panic, and the `Owned`
//! decoders never allocate more elements than the input has bits for.

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use shrink_wrap::prelude::*;
use shrink_wrap::series::{DeltaInt, XorBits};

#[derive_shrink_wrap(owned, derive(Debug))]
struct SeriesOwned {
    ts: u32,
    size: TailSize<2>,
    counts: DeltaOwned<i32>,
    stamps: DeltaOfDeltaOwned<u64>,
    temps: XorFloatOwned<f32>,
    note: Option<DeltaOwned<u8>>,
}

/// Field-by-field equality with bit-exact floats: `#[derive(PartialEq)]` would compare `temps` with `==`, which
/// is false for NaN against itself even though the bytes round-tripped exactly.
fn assert_series_eq(a: &SeriesOwned, b: &SeriesOwned) {
    assert_eq!(a.ts, b.ts);
    assert_eq!(a.counts.0, b.counts.0);
    assert_eq!(a.stamps.0, b.stamps.0);
    assert!(
        a.temps
            .0
            .iter()
            .map(|v| v.to_bits())
            .eq(b.temps.0.iter().map(|v| v.to_bits())),
        "{:?} != {:?}",
        a.temps,
        b.temps
    );
    assert_eq!(
        a.note.as_ref().map(|d| d.0.clone()),
        b.note.as_ref().map(|d| d.0.clone())
    );
}

fn round_trip_delta<T: DeltaInt + PartialEq + core::fmt::Debug>(values: &[T]) {
    let max_bytes = values.len() * 16 + 64;

    let mut buf = vec![0u8; max_bytes];
    let bytes = Delta::new(values)
        .to_ww_bytes(&mut buf)
        .expect("ser delta")
        .to_vec();
    let back: Delta<T> = Delta::from_ww_bytes(&bytes).expect("de delta");
    assert_eq!(back.iter().collect::<Vec<_>>(), values.to_vec());
    let owned = DeltaOwned(values.to_vec());
    assert_eq!(owned.to_ww_bytes_owned().expect("ser delta owned"), bytes);
    let back_owned = DeltaOwned::<T>::from_ww_bytes_owned(&bytes).expect("de delta owned");
    assert_eq!(back_owned.0, values.to_vec());

    let mut buf = vec![0u8; max_bytes];
    let dod_bytes = DeltaOfDelta::new(values)
        .to_ww_bytes(&mut buf)
        .expect("ser dod")
        .to_vec();
    let back: DeltaOfDelta<T> = DeltaOfDelta::from_ww_bytes(&dod_bytes).expect("de dod");
    assert_eq!(back.iter().collect::<Vec<_>>(), values.to_vec());
    let owned = DeltaOfDeltaOwned(values.to_vec());
    assert_eq!(owned.to_ww_bytes_owned().expect("ser dod owned"), dod_bytes);
    let back_owned = DeltaOfDeltaOwned::<T>::from_ww_bytes_owned(&dod_bytes).expect("de dod owned");
    assert_eq!(back_owned.0, values.to_vec());
}

fn round_trip_xor<T: XorBits + core::fmt::Debug>(values: &[T]) {
    let max_bytes = values.len() * 16 + 64;
    let mut buf = vec![0u8; max_bytes];
    let bytes = XorFloat::new(values)
        .to_ww_bytes(&mut buf)
        .expect("ser xor")
        .to_vec();
    let back: XorFloat<T> = XorFloat::from_ww_bytes(&bytes).expect("de xor");
    assert_eq!(back.len(), values.len());
    assert!(
        back.iter()
            .map(T::to_bits)
            .eq(values.iter().map(|v| v.to_bits())),
        "{values:?}"
    );
    let owned = XorFloatOwned(values.to_vec());
    assert_eq!(owned.to_ww_bytes_owned().expect("ser xor owned"), bytes);
    let back_owned = XorFloatOwned::<T>::from_ww_bytes_owned(&bytes).expect("de xor owned");
    assert!(
        back_owned
            .iter()
            .map(|v| v.to_bits())
            .eq(values.iter().map(|v| v.to_bits()))
    );
}

macro_rules! check_owned_bound {
    ($owned:ident, $ty:ty, $data:expr) => {
        if let Ok(v) = $owned::<$ty>::from_ww_bytes_owned($data) {
            assert!(
                v.len() <= $data.len() * 8,
                "owned decoder produced more elements than bits in the input"
            );
        }
    };
}

fuzz_target!(|data: &[u8]| {
    // decoders on random bytes: errors are fine, panics are not
    let _ = Delta::<u8>::from_ww_bytes(data);
    let _ = Delta::<u32>::from_ww_bytes(data);
    let _ = Delta::<u64>::from_ww_bytes(data);
    let _ = Delta::<i32>::from_ww_bytes(data);
    let _ = Delta::<i64>::from_ww_bytes(data);
    let _ = DeltaOfDelta::<u8>::from_ww_bytes(data);
    let _ = DeltaOfDelta::<u32>::from_ww_bytes(data);
    let _ = DeltaOfDelta::<u64>::from_ww_bytes(data);
    let _ = DeltaOfDelta::<i32>::from_ww_bytes(data);
    let _ = DeltaOfDelta::<i64>::from_ww_bytes(data);
    let _ = XorFloat::<f32>::from_ww_bytes(data);
    let _ = XorFloat::<f64>::from_ww_bytes(data);
    let _ = SeriesOwned::from_ww_bytes_owned(data);

    // the Owned decoders must never allocate more than the input's bits can justify (every element costs at
    // least one bit), even on malformed data that fails later in the decode loop
    check_owned_bound!(DeltaOwned, u8, data);
    check_owned_bound!(DeltaOwned, u32, data);
    check_owned_bound!(DeltaOwned, u64, data);
    check_owned_bound!(DeltaOwned, i32, data);
    check_owned_bound!(DeltaOwned, i64, data);
    check_owned_bound!(DeltaOfDeltaOwned, u8, data);
    check_owned_bound!(DeltaOfDeltaOwned, u32, data);
    check_owned_bound!(DeltaOfDeltaOwned, u64, data);
    check_owned_bound!(DeltaOfDeltaOwned, i32, data);
    check_owned_bound!(DeltaOfDeltaOwned, i64, data);
    check_owned_bound!(XorFloatOwned, f32, data);
    check_owned_bound!(XorFloatOwned, f64, data);

    let mut u = Unstructured::new(data);
    let values_u8: Vec<u8> = u
        .arbitrary::<Vec<u8>>()
        .unwrap_or_default()
        .into_iter()
        .take(128)
        .collect();
    let values_u32: Vec<u32> = u
        .arbitrary::<Vec<u32>>()
        .unwrap_or_default()
        .into_iter()
        .take(128)
        .collect();
    let values_u64: Vec<u64> = u
        .arbitrary::<Vec<u64>>()
        .unwrap_or_default()
        .into_iter()
        .take(128)
        .collect();
    let values_i32: Vec<i32> = u
        .arbitrary::<Vec<i32>>()
        .unwrap_or_default()
        .into_iter()
        .take(128)
        .collect();
    let values_i64: Vec<i64> = u
        .arbitrary::<Vec<i64>>()
        .unwrap_or_default()
        .into_iter()
        .take(128)
        .collect();
    let values_f32: Vec<f32> = u
        .arbitrary::<Vec<f32>>()
        .unwrap_or_default()
        .into_iter()
        .take(128)
        .collect();
    let values_f64: Vec<f64> = u
        .arbitrary::<Vec<f64>>()
        .unwrap_or_default()
        .into_iter()
        .take(128)
        .collect();

    round_trip_delta(&values_u8);
    round_trip_delta(&values_u32);
    round_trip_delta(&values_u64);
    round_trip_delta(&values_i32);
    round_trip_delta(&values_i64);
    round_trip_xor(&values_f32);
    round_trip_xor(&values_f64);

    // as fields of a derive struct with a TailSize<2> slot, read back from a buffer longer than the value, and
    // unaffected by garbage appended after it
    let note: Option<Vec<u8>> = if u.arbitrary().unwrap_or(false) {
        Some(values_u8.clone())
    } else {
        None
    };
    let garbage: Vec<u8> = u.arbitrary().unwrap_or_default();
    let record = SeriesOwned {
        ts: u.arbitrary().unwrap_or(0),
        size: TailSize(0),
        counts: DeltaOwned(values_i32.clone()),
        stamps: DeltaOfDeltaOwned(values_u64.clone()),
        temps: XorFloatOwned(values_f32.clone()),
        note: note.clone().map(DeltaOwned),
    };

    let bytes = record.to_ww_bytes_owned().expect("serialize");
    let back = SeriesOwned::from_ww_bytes_owned(&bytes).expect("deserialize");
    assert_series_eq(&back, &record);
    // the slot counts the bytes after it to the end of the value; `ts` (4 bytes) comes before it
    assert_eq!(back.size.0 as usize, bytes.len() - 4 - TailSize::<2>::LEN);

    // garbage after the value does not change the result
    let mut longer = bytes.clone();
    longer.extend_from_slice(&garbage);
    let back_through_garbage = SeriesOwned::from_ww_bytes_owned(&longer).expect("bounded");
    assert_series_eq(&back_through_garbage, &back);
    // a truncated value is an error, never garbage
    if bytes.len() > 1 {
        assert!(SeriesOwned::from_ww_bytes_owned(&bytes[..bytes.len() - 1]).is_err());
    }
});
