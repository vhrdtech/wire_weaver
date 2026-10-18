//! [SeriesStore] round trips (SW-62): every impl here and the derive's, against a model of what was set where.

use super::SeriesStore;
use super::type_check::*;
use crate::un::{I12, IN, U3, U5, UN};
use crate::{Nibble, RefVec, SerializeShrinkWrapOwned, UNib32, UVlq32, UVlq64, derive_shrink_wrap};
use core::fmt::Debug;
use core::marker::PhantomData;
use proptest::prelude::*;

const N: usize = 8;

/// Sets `items` at `slots` (mod `N`) in a fresh store, one after the other, then reads every index back: the item
/// set there last, or what a fresh store holds.
fn check_with<T: SeriesStore + Clone + Debug>(
    items: &[T],
    slots: &[usize],
    eq: impl Fn(&T, &T) -> bool,
) {
    let fresh = T::empty_store::<N>();
    let empty = T::get(&fresh, 0);
    for i in 1..N {
        assert!(eq(&T::get(&fresh, i), &empty), "fresh store at {i}");
    }
    // in place, over garbage: `init_store` writes every byte it needs
    let mut slot = core::mem::MaybeUninit::<T::Store<N>>::uninit();
    // SAFETY: bytes of a `MaybeUninit`, any value is fine
    unsafe {
        slot.as_mut_ptr()
            .cast::<u8>()
            .write_bytes(0xA5, core::mem::size_of::<T::Store<N>>())
    };
    let in_place = super::init_store_in::<T, N>(&mut slot);
    for i in 0..N {
        assert!(
            eq(&T::get(in_place, i), &empty),
            "store built in place at {i}"
        );
    }
    let mut store = T::empty_store::<N>();
    let mut model: [Option<T>; N] = Default::default();
    for (k, item) in items.iter().enumerate() {
        let i = slots.get(k).copied().unwrap_or(k) % N;
        T::set(&mut store, i, item);
        model[i] = Some(item.clone());
        let got = T::get(&store, i);
        assert!(eq(&got, item), "at {i}: {got:?} != {item:?}");
    }
    for (i, m) in model.iter().enumerate() {
        let got = T::get(&store, i);
        let want = m.as_ref().unwrap_or(&empty);
        assert!(eq(&got, want), "at {i} after all: {got:?} != {want:?}");
    }
}

fn check<T: SeriesStore + Clone + Debug + PartialEq>(items: &[T], slots: &[usize]) {
    check_with(items, slots, |a, b| a == b)
}

/// Equal as bytes on the wire: floats by their bits (NaN payloads, `-0.0`).
fn same_bytes<T: SerializeShrinkWrapOwned>(a: &T, b: &T) -> bool {
    a.to_ww_bytes_owned().unwrap() == b.to_ww_bytes_owned().unwrap()
}

/// An enum with data and a `#[future_version]` (it is `Unsized`): the discriminants as `u32`, every variant's
/// fields in a store of their own.
#[derive_shrink_wrap(owned, series, ww_repr = unib32, derive(Clone, Debug, PartialEq))]
enum ShapeOwned {
    Empty,
    Circle(u8),
    Rect { w: u16, h: u16, label: String },
    Line(),
    Dot {},
}

/// A variant compiled out: no store for it.
#[derive_shrink_wrap(owned, series, ww_repr = u4, derive(Clone, Debug, PartialEq))]
enum Mode {
    #[cfg(any())]
    Turbo(u16),
    Off,
    On(i8),
}

/// Type and const parameters, a phantom one.
#[derive_shrink_wrap(owned, series, derive(Clone, Debug, PartialEq))]
struct Window<T: Copy + Default, U, const M: usize> {
    samples: [T; M],
    tag: Option<T>,
    _unit: PhantomData<U>,
}

#[derive_shrink_wrap(owned, series, ww_repr = u4, derive(Clone, Debug, PartialEq))]
enum Either<T> {
    Left(T),
    Right { n: u8 },
}

fn shape(k: u8, v: u16) -> ShapeOwned {
    match k % 6 {
        0 => ShapeOwned::Empty,
        1 => ShapeOwned::Circle(v as u8),
        2 => ShapeOwned::Rect {
            w: v,
            h: v.rotate_left(3),
            label: "abc"[..(v % 4) as usize].into(),
        },
        3 => ShapeOwned::Line(),
        4 => ShapeOwned::Dot {},
        _ => ShapeOwned::Unknown(5 + v as u32),
    }
}

fn un(k: u8, v: u64) -> UN {
    match k % 4 {
        0 => UN::UN8 {
            bit_count: U3::new(k % 8).unwrap(),
            value: v as u8,
        },
        1 => UN::UN16 {
            bit_count: crate::un::U4::new(k % 16).unwrap(),
            value: v as u16,
        },
        2 => UN::UN32 {
            bit_count: U5::new(k % 32).unwrap(),
            value: v as u32,
        },
        _ => UN::UN64 {
            bit_count: crate::un::U6::new(k % 64).unwrap(),
            value: v,
        },
    }
}

fn in_(k: u8, v: i64) -> IN {
    match k % 4 {
        0 => IN::IN8 {
            bit_count: U3::new(k % 8).unwrap(),
            value: v as i8,
        },
        1 => IN::IN16 {
            bit_count: crate::un::U4::new(k % 16).unwrap(),
            value: v as i16,
        },
        2 => IN::IN32 {
            bit_count: U5::new(k % 32).unwrap(),
            value: v as i32,
        },
        _ => IN::IN64 {
            bit_count: crate::un::U6::new(k % 64).unwrap(),
            value: v,
        },
    }
}

#[test]
fn fresh_stores() {
    assert_eq!(u32::get(&u32::empty_store::<4>(), 3), 0);
    assert_eq!(<&str>::get(&<&str>::empty_store::<4>(), 3), "");
    assert_eq!(
        Option::<u8>::get(&Option::<u8>::empty_store::<4>(), 0),
        None
    );
    // an enum's first variant compiled in, its fields empty
    assert_eq!(Mode::get(&Mode::empty_store::<4>(), 2), Mode::Off);
    assert_eq!(
        ShapeOwned::get(&ShapeOwned::empty_store::<4>(), 1),
        ShapeOwned::Empty
    );
    assert_eq!(
        Quantity::get(&Quantity::empty_store::<4>(), 0),
        Quantity::new(Prefix::Unit, NumericValue::U8(0))
    );
}

/// Every kind of store built in place (and set, read back) without proptest, small enough for miri (`just miri`).
#[test]
fn in_place_stores() {
    check(&[7u32, 9], &[0, 5]);
    check(&[-3i8], &[2]);
    check(&[true], &[1]);
    check(&["ab", ""], &[3, 4]);
    check(&[Some(4u8), None], &[0, 1]);
    check(&[[1u16, 2, 3]], &[6]);
    check(&[(1u8, Some(true), [-1i32; 2])], &[7]);
    check(&[Mode::On(-2)], &[3]);
    check(&[shape(2, 300), shape(1, 5)], &[2, 4]);
    check(
        &[Window::<u8, u32, 2> {
            samples: [1, 2],
            tag: Some(3),
            _unit: PhantomData,
        }],
        &[5],
    );
    check(&[Either::Left(5u16), Either::Right { n: 1 }], &[0, 1]);
    check(&[Quantity::new(Prefix::Milli, NumericValue::U8(3))], &[1]);
}

/// A hand-written store whose `init_store` gives back another store than its slot.
#[derive(Clone, Copy, Debug)]
struct Liar;

impl SeriesStore for Liar {
    type Store<const N: usize> = [u8; N];

    fn empty_store<const N: usize>() -> [u8; N] {
        [0; N]
    }

    fn init_store<const N: usize>(_slot: &mut core::mem::MaybeUninit<[u8; N]>) -> &mut [u8; N] {
        Box::leak(Box::new([0; N]))
    }

    fn set<const N: usize>(store: &mut [u8; N], i: usize, _item: &Self) {
        store[i] = 1;
    }

    fn get<const N: usize>(_store: &[u8; N], _i: usize) -> Self {
        Liar
    }
}

/// Code building a bigger store in place relies on the slot being written, so a lying impl panics.
#[test]
#[should_panic(expected = "another store than its slot")]
fn init_store_of_another_store_panics() {
    let mut slot = core::mem::MaybeUninit::uninit();
    super::init_store_in::<Liar, 4>(&mut slot);
}

/// Two data variants: the store keeps both fields (docs/serdes/series.md).
#[derive_shrink_wrap(owned, series, ww_repr = u4, no_future_version, derive(Clone, Debug, PartialEq))]
enum Two {
    A(u32),
    B(u32),
}

#[test]
fn store_sizes() {
    use core::mem::size_of;
    assert_eq!(size_of::<<Two as SeriesStore>::Store<32>>(), 32 * 9);
    assert_eq!(size_of::<Two>(), 8);
    // a struct of arrays: no padding between fields
    assert_eq!(size_of::<<(u8, u32) as SeriesStore>::Store<32>>(), 32 * 5);
    assert_eq!(size_of::<<Option<u16> as SeriesStore>::Store<32>>(), 32 * 3);
    assert_eq!(
        size_of::<<PhantomData<String> as SeriesStore>::Store<32>>(),
        0
    );
    // three variants: one byte of discriminant
    assert_eq!(size_of::<<Prefix as SeriesStore>::Store<32>>(), 32);
    // derived, not the hand store of ww_numeric: a discriminant and every variant's field
    assert_eq!(
        size_of::<<NumericValue as SeriesStore>::Store<32>>(),
        32 * (1 + 1 + 2 + 4 + 4 + 4)
    );
    assert_eq!(size_of::<<Quantity as SeriesStore>::Store<32>>(), 32 * 17);
    assert_eq!(
        size_of::<<TimedIv as SeriesStore>::Store<32>>(),
        32 * 17 * 3
    );
    // a #[future_version]: discriminants as u32
    assert_eq!(
        size_of::<<ShapeOwned as SeriesStore>::Store<8>>(),
        8 * (4 + 1 + 2 + 2 + size_of::<String>())
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn numbers(
        a in prop::collection::vec(any::<(u8, u16, u32, u64, u128)>(), 0..20),
        b in prop::collection::vec(any::<(i8, i16, i32, i64, i128)>(), 0..20),
        slots in prop::collection::vec(any::<usize>(), 0..20),
    ) {
        check(&a, &slots);
        check(&b, &slots);
        let floats: Vec<(f32, f64)> = a
            .iter()
            .map(|t| (f32::from_bits(t.2), f64::from_bits(t.3)))
            .collect();
        check_with(&floats, &slots, |x, y| {
            x.0.to_bits() == y.0.to_bits() && x.1.to_bits() == y.1.to_bits()
        });
        let wire: Vec<(Nibble, UNib32, UVlq32, UVlq64, U5, I12)> = a
            .iter()
            .map(|t| {
                (
                    Nibble::new_masked(t.0),
                    UNib32(t.2),
                    UVlq32(t.2.rotate_left(7)),
                    UVlq64(t.3),
                    U5::new(t.0 % 32).unwrap(),
                    I12::new((t.1 % 4096) as i16 - 2048).unwrap(),
                )
            })
            .collect();
        check(&wire, &slots);
        let bit_counts: Vec<(UN, IN)> = a
            .iter()
            .zip(&b)
            .map(|(t, s)| (un(t.0, t.3), in_(t.0.wrapping_add(1), s.3)))
            .collect();
        check(&bit_counts, &slots);
    }

    #[test]
    fn bools_options_arrays(
        items in prop::collection::vec(any::<(bool, Option<u8>, Option<Option<i16>>, [Option<u8>; 3])>(), 0..20),
        slots in prop::collection::vec(any::<usize>(), 0..20),
    ) {
        check(&items, &slots);
        let phantom: Vec<PhantomData<String>> = items.iter().map(|_| PhantomData).collect();
        check(&phantom, &slots);
    }

    #[test]
    fn strings_and_bytes(
        items in prop::collection::vec(any::<(String, Vec<u8>)>(), 0..20),
        slots in prop::collection::vec(any::<usize>(), 0..20),
    ) {
        check(&items, &slots);
        let borrowed: Vec<(&str, &[u8], RefVec<'_, u8>)> = items
            .iter()
            .map(|(s, b)| (s.as_str(), b.as_slice(), RefVec::new_bytes(b)))
            .collect();
        check_with(&borrowed, &slots, |x, y| {
            x.0 == y.0 && x.1 == y.1 && x.2.as_slice() == y.2.as_slice()
        });
    }

    #[test]
    fn derived(
        bytes in prop::collection::vec(any::<u8>(), 0..600),
        slots in prop::collection::vec(any::<usize>(), 0..60),
    ) {
        let mut picks = Picks::new(&bytes);
        let kitchen = kitchen_items(&mut picks, 30);
        check(&kitchen, &slots);
        let borrowed: Vec<Kitchen<'_>> = kitchen.iter().map(|k| k.as_borrowed()).collect();
        check(&borrowed, &slots);
        let timed = timed_items(&mut picks, 30);
        check_with(&timed, &slots, same_bytes);
        let inner: Vec<Option<InnerV2Owned>> = kitchen.iter().map(|k| k.inner.clone()).collect();
        check(&inner, &slots);
        let outer: Vec<OuterV1> = kitchen
            .iter()
            .map(|k| OuterV1 { a: k.pair.0, inner: InnerV1 { x: k.arr[0] as u16 }, b: k.pair.2 as i32 })
            .collect();
        check(&outer, &slots);
    }

    #[test]
    fn derived_enums_and_generics(
        items in prop::collection::vec(any::<(u8, u16, i8)>(), 0..20),
        slots in prop::collection::vec(any::<usize>(), 0..20),
    ) {
        let shapes: Vec<ShapeOwned> = items.iter().map(|&(k, v, _)| shape(k, v)).collect();
        check(&shapes, &slots);
        let modes: Vec<Mode> = items
            .iter()
            .map(|&(k, _, s)| if k % 2 == 0 { Mode::Off } else { Mode::On(s) })
            .collect();
        check(&modes, &slots);
        let windows: Vec<Window<i8, (), 3>> = items
            .iter()
            .map(|&(k, v, s)| Window {
                samples: [s, k as i8, v as i8],
                tag: (k % 3 != 0).then_some(s),
                _unit: PhantomData,
            })
            .collect();
        check(&windows, &slots);
        let eithers: Vec<Either<Option<u16>>> = items
            .iter()
            .map(|&(k, v, _)| match k % 3 {
                0 => Either::Left(None),
                1 => Either::Left(Some(v)),
                _ => Either::Right { n: k },
            })
            .collect();
        check(&eithers, &slots);
    }
}
