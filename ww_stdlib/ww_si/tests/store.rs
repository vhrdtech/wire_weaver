//! Quantities in a `SeriesStore` (SW-62): what a batch of `Timed<Ampere>` takes on a device, and a round trip.
#![cfg(all(
    feature = "time",
    feature = "electrical",
    feature = "u32",
    feature = "i32"
))]

use shrink_wrap::series::SeriesStore;
use ww_si::{Ampere, Prefix, Second, Timed};

type Store = <Timed<Ampere> as SeriesStore>::Store<32>;

#[test]
fn round_trip() {
    let mut store = Timed::<Ampere>::empty_store::<32>();
    for k in 0..32u32 {
        let item = Timed::new(
            Second::micro(k * 40),
            Ampere::new(Prefix::Milli, k as i32 - 16),
        );
        Timed::set(&mut store, k as usize, &item);
    }
    for k in 0..32u32 {
        let item = Timed::<Ampere>::get(&store, k as usize);
        assert_eq!(item.t, Second::micro(k * 40));
        assert_eq!(item.value, Ampere::new(Prefix::Milli, k as i32 - 16));
    }
}

/// With every number type (the default `std` build): 19 bytes per quantity (prefix, variant, a `u128` payload,
/// `UN` / `IN` width), 38 per item.
#[cfg(feature = "std")]
#[test]
fn size_all_number_types() {
    assert_eq!(size_of::<Store>(), 32 * 38);
}

/// A firmware with `U8`, `U32`, `I32` and `F32` only: 6 bytes per quantity (prefix, variant, a `u32` payload), 12
/// per item, where `[Timed<Ampere>; 32]` takes 24 per item (`cargo test -p ww_si --no-default-features --features
/// time,electrical,u32,i32,f32 --test store`, run by `just test`).
#[cfg(not(any(
    feature = "u64",
    feature = "i64",
    feature = "f64",
    feature = "uvlq64",
    feature = "un",
    feature = "in",
    feature = "u128",
    feature = "i128"
)))]
#[test]
fn size_up_to_32_bits() {
    assert_eq!(size_of::<Store>(), 32 * 12);
    assert_eq!(size_of::<[Timed<Ampere>; 32]>(), 32 * 24);
}
