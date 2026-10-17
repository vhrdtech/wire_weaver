//! `PhantomData<T>` fields (DER-15): zero bits, no columns, and a type parameter used only inside `PhantomData`
//! needs none of the bounds the derive puts on the other parameters, so a bare unit marker works.

use core::marker::PhantomData;
use shrink_wrap::prelude::*;
use shrink_wrap::series::{Series, SeriesOwned};
use shrink_wrap::{ElementSize, SerializeShrinkWrap};

/// A unit marker: no serdes, no `SeriesColumns`, only what the derives below ask of it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Volt;

#[derive_shrink_wrap(borrowed, owned, derive(Clone, Copy, Debug, PartialEq))]
struct Q<U> {
    a: u8,
    _u: PhantomData<U>,
}

#[derive_shrink_wrap(borrowed, owned, derive(Clone, Copy, Debug, PartialEq))]
struct Plain {
    a: u8,
}

#[derive_shrink_wrap(
    borrowed,
    owned,
    final_structure,
    series,
    derive(Clone, Copy, Debug, PartialEq)
)]
struct Quantity<U> {
    value: i32,
    _unit: PhantomData<U>,
}

#[derive_shrink_wrap(
    borrowed,
    owned,
    final_structure,
    series,
    derive(Clone, Copy, Debug, PartialEq)
)]
struct Value {
    value: i32,
}

/// A parameter used both in `PhantomData` and in a field keeps its bounds; `PhantomData` in the middle.
#[derive_shrink_wrap(borrowed, owned, derive(Clone, Copy, Debug, PartialEq))]
struct Mixed<T, U> {
    _u: PhantomData<(U, T)>,
    t: T,
    b: bool,
}

#[derive_shrink_wrap(borrowed, owned, ww_repr = u4, derive(Clone, Copy, Debug, PartialEq))]
enum Reading<U> {
    Empty,
    Value(u16, PhantomData<U>),
}

#[test]
fn plain_and_owned_same_bytes_as_without() {
    let q = Q::<Volt> {
        a: 7,
        _u: PhantomData,
    };
    let bytes = q.to_ww_bytes_owned().unwrap();
    assert_eq!(bytes, Plain { a: 7 }.to_ww_bytes_owned().unwrap());
    let mut buf = [0u8; 8];
    assert_eq!(q.to_ww_bytes(&mut buf).unwrap(), &bytes[..]);
    assert_eq!(Q::<Volt>::from_ww_bytes(&bytes).unwrap(), q);
    assert_eq!(Q::<Volt>::from_ww_bytes_owned(&bytes).unwrap(), q);
}

#[test]
fn zero_bits() {
    assert!(matches!(
        <PhantomData<Volt> as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::Sized { size_bits: 0 }
    ));
    assert!(matches!(
        <Quantity<Volt> as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::UnsizedFinalStructure
    ));
    let q = Quantity::<Volt> {
        value: -5,
        _unit: PhantomData,
    };
    let bytes = q.to_ww_bytes_owned().unwrap();
    assert_eq!(bytes, Value { value: -5 }.to_ww_bytes_owned().unwrap());
    assert_eq!(Quantity::<Volt>::from_ww_bytes(&bytes).unwrap(), q);
    assert_eq!(Quantity::<Volt>::from_ww_bytes_owned(&bytes).unwrap(), q);
}

#[test]
fn series_has_no_column_for_it() {
    let items: Vec<Quantity<Volt>> = (0..20)
        .map(|k| Quantity {
            value: 1000 - k * 3,
            _unit: PhantomData,
        })
        .collect();
    let bytes = Series::new(&items).to_ww_bytes_owned().unwrap();
    let plain: Vec<Value> = items.iter().map(|q| Value { value: q.value }).collect();
    assert_eq!(bytes, Series::new(&plain).to_ww_bytes_owned().unwrap());
    assert_eq!(
        SeriesOwned::<Quantity<Volt>>::from_ww_bytes_owned(&bytes)
            .unwrap()
            .0,
        items
    );
    let series = Series::<Quantity<Volt>>::from_ww_bytes(&bytes).unwrap();
    assert_eq!(series.iter().collect::<Vec<_>>(), items);
}

#[test]
fn mixed_and_enum() {
    let m = Mixed::<u16, Volt> {
        _u: PhantomData,
        t: 300,
        b: true,
    };
    let bytes = m.to_ww_bytes_owned().unwrap();
    assert_eq!(bytes, (300u16, true).to_ww_bytes_owned().unwrap());
    assert_eq!(Mixed::<u16, Volt>::from_ww_bytes(&bytes).unwrap(), m);

    let r = Reading::<Volt>::Value(5, PhantomData);
    let bytes = r.to_ww_bytes_owned().unwrap();
    assert_eq!(Reading::<Volt>::from_ww_bytes(&bytes).unwrap(), r);
    assert_eq!(Reading::<Volt>::from_ww_bytes_owned(&bytes).unwrap(), r);
}
