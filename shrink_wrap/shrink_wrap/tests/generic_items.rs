//! Derived structs and enums with type and const parameters and where clauses (DER-14): `Timed<T>`, a borrowed
//! `Tagged<'i, T>` with its owned `TaggedOwned<T>`, `Buf<const N: usize>`, `Reading<T>`, ...

use core::fmt::{Debug, Display};
use shrink_wrap::prelude::*;
use shrink_wrap::{
    DeserializeShrinkWrapOwned, ElementSize, SerializeShrinkWrap, SerializeShrinkWrapOwned,
};

#[derive_shrink_wrap(
    borrowed,
    owned,
    final_structure,
    derive(Clone, Copy, Debug, PartialEq)
)]
struct Timed<T> {
    t: u32,
    value: T,
}

#[derive_shrink_wrap(borrowed, owned, derive(Clone, Copy, Debug, PartialEq))]
struct Pair {
    a: u8,
    b: bool,
}

#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq))]
struct Tagged<'i, T: Copy> {
    name: &'i str,
    value: T,
}

#[test]
fn timed_round_trips_and_size() {
    let t = Timed {
        t: 7,
        value: 300u16,
    };
    let bytes = t.to_ww_bytes_owned().unwrap();
    assert_eq!(bytes, [7, 0, 0, 0, 44, 1]); // little-endian, as a plain struct of a u32 and a u16
    assert_eq!(Timed::<u16>::from_ww_bytes(&bytes).unwrap(), t);
    assert_eq!(Timed::<u16>::from_ww_bytes_owned(&bytes).unwrap(), t);

    // the size is the parameter's: an evolvable `Pair` makes it Unsized inside
    assert!(matches!(
        <Timed<u16> as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::UnsizedFinalStructure
    ));
    let nested = Timed {
        t: 1,
        value: Pair { a: 2, b: true },
    };
    let bytes = nested.to_ww_bytes_owned().unwrap();
    assert_eq!(Timed::<Pair>::from_ww_bytes(&bytes).unwrap(), nested);
}

#[test]
fn borrowed_and_owned_with_a_lifetime() {
    let owned = TaggedOwned {
        name: "volt".into(),
        value: -3i8,
    };
    let bytes = owned.to_ww_bytes_owned().unwrap();
    let borrowed = Tagged::<i8>::from_ww_bytes(&bytes).unwrap();
    assert_eq!(borrowed.name, "volt");
    assert_eq!(borrowed.value, -3);
    let mut buf = [0u8; 32];
    assert_eq!(borrowed.to_ww_bytes(&mut buf).unwrap(), &bytes[..]);
    assert_eq!(
        TaggedOwned::<i8>::from_ww_bytes_owned(&bytes).unwrap(),
        owned
    );
}

/// Round trips `value` through the owned serializer, returns the bytes.
fn round_trip<T>(value: &T) -> Vec<u8>
where
    T: SerializeShrinkWrapOwned + DeserializeShrinkWrapOwned + PartialEq + Debug,
{
    let bytes = value.to_ww_bytes_owned().unwrap();
    assert_eq!(&T::from_ww_bytes_owned(&bytes).unwrap(), value);
    bytes
}

#[derive_shrink_wrap(owned, sized, derive(Clone, Debug, PartialEq))]
struct Buf<const N: usize> {
    len: u8,
    data: [u8; N],
}

#[test]
fn const_parameter_sizes_the_array() {
    let buf = Buf {
        len: 2,
        data: [1, 2, 3],
    };
    assert_eq!(round_trip(&buf), [2, 1, 2, 3]);
    assert!(matches!(
        <Buf<3> as SerializeShrinkWrapOwned>::ELEMENT_SIZE,
        ElementSize::Sized { size_bits: 32 }
    ));
    assert!(matches!(
        <Buf<16> as SerializeShrinkWrapOwned>::ELEMENT_SIZE,
        ElementSize::Sized { size_bits: 136 }
    ));
}

#[derive_shrink_wrap(borrowed, owned, derive(Clone, Debug, PartialEq))]
struct Label<'i, const N: usize> {
    text: MaxLen<&'i str, N>,
}

#[test]
fn const_parameter_bounds_a_max_len() {
    let label = LabelOwned::<4> {
        text: MaxLen::new_unchecked("volt".into()),
    };
    let bytes = round_trip(&label);
    assert_eq!(
        Label::<4>::from_ww_bytes(&bytes).unwrap().text,
        MaxLen::new_unchecked("volt")
    );
    // the same bytes with a smaller bound don't read
    assert!(LabelOwned::<3>::from_ww_bytes_owned(&bytes).is_err());
}

/// A `bool` const parameter and a default type parameter.
#[derive_shrink_wrap(owned, final_structure, derive(Clone, Debug, PartialEq))]
struct Flagged<const STRICT: bool, T = u8> {
    value: T,
}

impl<const STRICT: bool, T> Flagged<STRICT, T> {
    fn strict(&self) -> bool {
        STRICT
    }
}

#[test]
fn bool_const_parameter_and_default() {
    let f: Flagged<true> = Flagged { value: 5 };
    assert!(f.strict());
    assert_eq!(round_trip(&f), [5]);
}

#[derive_shrink_wrap(borrowed, owned, derive(Clone, Copy, Debug, PartialEq))]
struct Bounded<T>
where
    T: Copy + PartialOrd,
{
    min: T,
    max: T,
}

/// A tuple struct: the where clause goes after the fields.
#[derive_shrink_wrap(owned, final_structure, derive(Clone, Debug, PartialEq))]
struct Wrapped<T>(T, u8)
where
    T: Clone;

#[test]
fn where_clauses() {
    let b = Bounded { min: -1i16, max: 7 };
    let bytes = round_trip(&b);
    assert_eq!(Bounded::<i16>::from_ww_bytes(&bytes).unwrap(), b);
    assert_eq!(round_trip(&Wrapped(3u16, 4)), [3, 0, 4]);
}

/// An evolvable generic enum, a borrowed variant next to one of the parameter.
#[derive_shrink_wrap(borrowed, owned, ww_repr = u4, discriminants, derive(Clone, Debug, PartialEq))]
enum Reading<'i, T: Copy> {
    Value(T),
    Missing,
    Labeled { label: &'i str, value: T },
}

#[test]
fn generic_enum_borrowed_and_owned() {
    for owned in [
        ReadingOwned::Value(300u16),
        ReadingOwned::Missing,
        ReadingOwned::Labeled {
            label: "t".into(),
            value: 7,
        },
    ] {
        let bytes = round_trip(&owned);
        let borrowed = Reading::<u16>::from_ww_bytes(&bytes).unwrap();
        assert_eq!(borrowed.discriminant(), owned.discriminant());
        let mut buf = [0u8; 32];
        assert_eq!(borrowed.to_ww_bytes(&mut buf).unwrap(), &bytes[..]);
    }
    // the discriminants enum has unit variants only, so no parameters
    assert_eq!(ReadingDiscriminants::Labeled.discriminant(), 2);
}

/// A sized generic enum: its size is the discriminant and the parameter's, so every variant is one size for any `T`.
#[derive_shrink_wrap(owned, sized, ww_repr = u8, derive(Clone, Copy, Debug, PartialEq))]
enum Either<T> {
    Left(T),
    Right(T),
}

/// Const parameters on an enum, with a where clause: an array is read into a default one.
#[derive_shrink_wrap(owned, ww_repr = u8, derive(Clone, Debug, PartialEq))]
enum Packet<T, const N: usize>
where
    T: Copy + Default,
{
    Data([T; N]),
    Empty,
}

#[test]
fn generic_enum_sizes_and_const_parameters() {
    assert_eq!(round_trip(&Either::Right(0x1234u16)), [1, 0x34, 0x12]);
    assert!(matches!(
        <Either<u16> as SerializeShrinkWrapOwned>::ELEMENT_SIZE,
        ElementSize::Sized { size_bits: 24 }
    ));
    round_trip(&Packet::<u8, 3>::Data([1, 2, 3]));
    round_trip(&Packet::<u8, 3>::Empty);
}

// `display` on generic types: the message's fields need `Display`, written as a bound
/// Out of range: {value}.
#[derive_shrink_wrap(owned, display, derive(Clone, Debug, PartialEq))]
struct OutOfRange<T: Display> {
    value: T,
}

#[derive_shrink_wrap(owned, display, ww_repr = u4, derive(Clone, Debug, PartialEq))]
enum Fault<T>
where
    T: Display,
{
    /// Over {0}.
    Over(T),
    /// Timed out.
    Timeout,
}

#[test]
fn display_of_generic_types() {
    assert_eq!(
        OutOfRange { value: 3.5f32 }.to_string(),
        "Out of range: 3.5"
    );
    assert_eq!(Fault::Over(12u8).to_string(), "Over 12");
    let e: &dyn core::error::Error = &Fault::<u8>::Timeout;
    assert_eq!(e.to_string(), "Timed out");
}

/// `validate` on a generic struct: the function is generic too.
#[derive_shrink_wrap(owned, validate = Ordered::check, derive(Clone, Debug, PartialEq))]
struct Ordered<T: PartialOrd> {
    lo: T,
    hi: T,
}

impl<T: PartialOrd> Ordered<T> {
    fn check(&self) -> Result<(), ShrinkWrapError> {
        if self.lo <= self.hi {
            Ok(())
        } else {
            Err(ShrinkWrapError::SubtypeOutOfRange)
        }
    }
}

#[test]
fn validate_on_a_generic_struct() {
    round_trip(&Ordered { lo: 1u8, hi: 2 });
    let bytes = Ordered { lo: 3u8, hi: 2 }.to_ww_bytes_owned().unwrap();
    assert!(Ordered::<u8>::from_ww_bytes_owned(&bytes).is_err());
}

/// A `char` const parameter, and a const parameter as the width of a `TailSize`.
#[derive_shrink_wrap(owned, derive(Clone, Debug, PartialEq))]
struct Sized2<const UNIT: char, const W: usize> {
    size: TailSize<W>,
    value: u16,
}

#[test]
fn char_const_parameter_and_tail_size_width() {
    let v = Sized2::<'V', 1> {
        size: TailSize::default(),
        value: 3,
    };
    let bytes = v.to_ww_bytes_owned().unwrap();
    // a one-byte slot holding the size of the rest, the u16
    assert_eq!(bytes, [2, 3, 0]);
    let back = Sized2::<'V', 1>::from_ww_bytes_owned(&bytes).unwrap();
    assert_eq!((back.size.0, back.value), (2, 3));
}
