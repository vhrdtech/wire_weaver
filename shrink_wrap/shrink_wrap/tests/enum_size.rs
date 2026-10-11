//! `ELEMENT_SIZE` of enums with a size assumption (SW-35): the discriminant plus one variant, not the sum of all
//! variants, and user types anywhere inside a field are counted too.

use hex_literal::hex;
use shrink_wrap::prelude::*;
use shrink_wrap::{BufReader, BufWriter, ElementSize, Error};

const fn ser_size<T: SerializeShrinkWrap>() -> ElementSize {
    T::ELEMENT_SIZE
}

const fn des_size<'i, T: DeserializeShrinkWrap<'i>>() -> ElementSize {
    T::ELEMENT_SIZE
}

fn sized(size_bits: usize) -> ElementSize {
    ElementSize::Sized { size_bits }
}

#[test]
fn one_of() {
    assert_eq!(sized(8).one_of(sized(8)), sized(8));
    assert_eq!(sized(8).one_of(sized(32)), ElementSize::SelfDescribing);
    assert_eq!(sized(0).one_of(sized(0)), sized(0));
    assert_eq!(
        sized(8).one_of(ElementSize::SelfDescribing),
        ElementSize::SelfDescribing
    );
    assert_eq!(
        ElementSize::SelfDescribing.one_of(sized(8)),
        ElementSize::SelfDescribing
    );
    assert_eq!(sized(8).one_of(ElementSize::Unsized), ElementSize::Unsized);
    assert_eq!(
        ElementSize::Unsized.one_of(ElementSize::UnsizedFinalStructure),
        ElementSize::UnsizedFinalStructure
    );
    // sizes that differ stay SelfDescribing, whatever comes next
    assert_eq!(
        sized(8).one_of(sized(16)).one_of(sized(16)),
        ElementSize::SelfDescribing
    );
}

#[test]
fn variants_differ() {
    assert!(!ElementSize::variants_differ(&[]));
    assert!(!ElementSize::variants_differ(&[sized(8)]));
    assert!(!ElementSize::variants_differ(&[sized(8), sized(8)]));
    assert!(ElementSize::variants_differ(&[sized(8), sized(0)]));
    assert!(ElementSize::variants_differ(&[
        sized(8),
        sized(8),
        sized(9)
    ]));
    // not a size mismatch: the enum is not Sized for another reason
    assert!(!ElementSize::variants_differ(&[
        sized(8),
        ElementSize::SelfDescribing,
        sized(0)
    ]));
}

#[test]
fn repeat() {
    assert_eq!(sized(3).repeat(5), sized(15));
    assert_eq!(sized(3).repeat(0), sized(0));
    assert_eq!(ElementSize::Unsized.repeat(5), ElementSize::Unsized);
}

macro_rules! unit_enum {
    ($name:ident, $repr:ident) => {
        #[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy), ww_repr = $repr, sized)]
        enum $name {
            A,
            B,
        }
    };
}
unit_enum!(UnitU1, u1);
unit_enum!(UnitU2, u2);
unit_enum!(UnitU7, u7);
unit_enum!(UnitU13, u13);
unit_enum!(UnitU31, u31);
unit_enum!(UnitNib, nib);
unit_enum!(UnitU8, u8);
unit_enum!(UnitU16, u16);
unit_enum!(UnitU32, u32);

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = unib32, self_describing)]
enum UnitUNib32 {
    A,
    B,
}

/// The discriminant is counted for every repr.
#[test]
fn discriminant_of_every_repr() {
    assert_eq!(ser_size::<UnitU1>(), sized(1));
    assert_eq!(ser_size::<UnitU2>(), sized(2));
    assert_eq!(ser_size::<UnitU7>(), sized(7));
    assert_eq!(ser_size::<UnitU13>(), sized(13));
    assert_eq!(ser_size::<UnitU31>(), sized(31));
    assert_eq!(ser_size::<UnitNib>(), sized(4));
    assert_eq!(ser_size::<UnitU8>(), sized(8));
    assert_eq!(ser_size::<UnitU16>(), sized(16));
    assert_eq!(ser_size::<UnitU32>(), sized(32));
    assert_eq!(ser_size::<UnitUNib32>(), ElementSize::SelfDescribing);

    // all four traits agree
    assert_eq!(des_size::<UnitU7>(), sized(7));
    assert_eq!(<UnitU7 as SerializeShrinkWrapOwned>::ELEMENT_SIZE, sized(7));
    assert_eq!(
        <UnitU7 as DeserializeShrinkWrapOwned>::ELEMENT_SIZE,
        sized(7)
    );
    let _ = (
        UnitU1::A,
        UnitU1::B,
        UnitU31::A,
        UnitUNib32::A,
        UnitUNib32::B,
    );
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone), ww_repr = u8, sized)]
enum Equal {
    A(u32),
    B { x: u16, y: i16 },
    C(f32),
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone), ww_repr = u8, self_describing)]
enum Unequal {
    A(u8),
    B(u32),
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone), ww_repr = u2, self_describing)]
enum UnitAndData {
    Nothing,
    Byte(u8),
}

/// `self_describing` is kept as asked, also when the variants happen to be of one size.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone), ww_repr = u8, self_describing)]
enum EqualButSelfDescribing {
    A(u8),
    B(i8),
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone), ww_repr = unib32, self_describing)]
enum EqualUNib32 {
    A(u8),
    B(i8),
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone), ww_repr = u8, final_structure)]
enum Final<'i> {
    A(u8),
    B(&'i str),
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone), ww_repr = u8)]
enum Evolvable {
    A(u8),
    B(u32),
}

#[test]
fn variants_are_not_summed() {
    // 8 bits of discriminant and 32 bits of whichever variant, it was 8 + 32 + 32 + 32 without the discriminant
    assert_eq!(ser_size::<Equal>(), sized(40));
    assert_eq!(des_size::<Equal>(), sized(40));
    assert_eq!(<Equal as SerializeShrinkWrapOwned>::ELEMENT_SIZE, sized(40));
    assert_eq!(ser_size::<Unequal>(), ElementSize::SelfDescribing);
    assert_eq!(
        <Unequal as DeserializeShrinkWrapOwned>::ELEMENT_SIZE,
        ElementSize::SelfDescribing
    );
    assert_eq!(ser_size::<UnitAndData>(), ElementSize::SelfDescribing);
    assert_eq!(
        ser_size::<EqualButSelfDescribing>(),
        ElementSize::SelfDescribing
    );
    assert_eq!(ser_size::<EqualUNib32>(), ElementSize::SelfDescribing);
    assert_eq!(ser_size::<Final>(), ElementSize::UnsizedFinalStructure);
    assert_eq!(ser_size::<Evolvable>(), ElementSize::Unsized);
}

#[test]
fn wire_format_is_the_same() {
    let mut buf = [0u8; 16];
    assert_eq!(
        Equal::A(0xAABBCCDD).to_ww_bytes(&mut buf).unwrap(),
        hex!("00 DDCCBBAA")
    );
    assert_eq!(
        Equal::B { x: 0x1122, y: -2 }.to_ww_bytes(&mut buf).unwrap(),
        hex!("01 2211 FEFF")
    );
    assert_eq!(
        Unequal::A(0xAA).to_ww_bytes(&mut buf).unwrap(),
        hex!("00 AA")
    );
    assert_eq!(
        Unequal::B(0xAABBCCDD).to_ww_bytes(&mut buf).unwrap(),
        hex!("01 DDCCBBAA")
    );
    assert_eq!(
        UnitAndData::Nothing.to_ww_bytes(&mut buf).unwrap(),
        hex!("00")
    );
    assert_eq!(
        UnitAndData::Byte(0xAA).to_ww_bytes(&mut buf).unwrap(),
        hex!("40 AA")
    );
    assert_eq!(UnitU2::B.to_ww_bytes(&mut buf).unwrap(), hex!("40"));
    assert_eq!(UnitNib::B.to_ww_bytes(&mut buf).unwrap(), hex!("10"));
    assert_eq!(UnitU16::B.to_ww_bytes(&mut buf).unwrap(), hex!("0100"));
    assert_eq!(UnitU32::B.to_ww_bytes(&mut buf).unwrap(), hex!("01000000"));
    assert_eq!(
        Unequal::from_ww_bytes(&hex!("01 DDCCBBAA")).unwrap(),
        Unequal::B(0xAABBCCDD)
    );
}

/// A user type the macro knows nothing about.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy, Default), sized)]
struct Point {
    x: u8,
    y: u8,
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone), ww_repr = u4, sized)]
enum Shape {
    Dot(Point),
    Tagged { tag: u16 },
    Pair(u8, UnitU8),
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone), ww_repr = u4, self_describing)]
enum ShapeOrNothing {
    Nothing,
    Dot(Point),
}

/// Sized enums inside a sized enum.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone), ww_repr = u1, sized)]
enum Nested {
    One(Shape, UnitU2),
    Other(UnitU2, Shape),
}

/// A self-describing enum inside makes the outer one self-describing as well.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone), ww_repr = u1, self_describing)]
enum NestedUnequal {
    A(Unequal),
    B(Unequal),
}

#[test]
fn user_types_in_variants() {
    assert_eq!(ser_size::<Point>(), sized(16));
    assert_eq!(ser_size::<Shape>(), sized(4 + 16));
    assert_eq!(<Shape as SerializeShrinkWrapOwned>::ELEMENT_SIZE, sized(20));
    assert_eq!(ser_size::<ShapeOrNothing>(), ElementSize::SelfDescribing);
    assert_eq!(ser_size::<Nested>(), sized(1 + 20 + 2));
    assert_eq!(des_size::<Nested>(), sized(23));
    assert_eq!(ser_size::<NestedUnequal>(), ElementSize::SelfDescribing);
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
struct Shapes<'i> {
    shapes: RefVec<'i, Shape>,
    units: RefVec<'i, UnitU2>,
}

#[test]
fn sized_enum_as_ref_vec_element() {
    let value = ShapesOwned {
        shapes: vec![
            Shape::Dot(Point { x: 1, y: 2 }),
            Shape::Tagged { tag: 0xAABB },
            Shape::Pair(3, UnitU8::B),
        ],
        units: vec![UnitU2::B, UnitU2::A, UnitU2::B],
    };
    let bytes = value.to_ww_bytes_owned().unwrap();
    assert_eq!(bytes, hex!("00 0102 10 BBAA 20 03 01 44 33"));
    let borrowed = Shapes::from_ww_bytes(&bytes).unwrap();
    let shapes: Vec<_> = borrowed.shapes.iter().map(|s| s.unwrap()).collect();
    assert_eq!(
        shapes,
        [
            Shape::Dot(Point { x: 1, y: 2 }),
            Shape::Tagged { tag: 0xAABB },
            Shape::Pair(3, UnitU8::B),
        ]
    );
    assert_eq!(ShapesOwned::from_ww_bytes_owned(&bytes).unwrap(), value);
}

/// User types inside arrays, tuples, ranges and options of a sized or self-describing type.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone), sized)]
struct Containers {
    array: [Point; 3],
    tuple: (Point, u8),
    range: core::ops::Range<UnitU8>,
    plain_range: core::ops::RangeInclusive<u16>,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone), self_describing)]
struct MaybePoint {
    point: Option<Point>,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone), ww_repr = u8, self_describing)]
enum MaybeShape {
    A(Option<Shape>),
    B(Result<Point, Shape>),
}

#[test]
fn user_types_inside_other_types() {
    assert_eq!(
        ser_size::<Containers>(),
        sized(3 * 16 + (16 + 8) + 2 * 8 + 2 * 16)
    );
    // one bit of flag, then the point or nothing
    assert_eq!(ser_size::<MaybePoint>(), ElementSize::SelfDescribing);
    assert_eq!(ser_size::<MaybeShape>(), ElementSize::SelfDescribing);
    assert_eq!(ser_size::<core::ops::Range<u8>>(), sized(16));
    assert_eq!(ser_size::<core::ops::RangeInclusive<Point>>(), sized(32));
}

/// An evolvable (Unsized) type.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone))]
struct Evolving {
    a: u8,
}

/// Holds its value through the generic `Option<T>` impl, not through the derive's own handling of `Option` fields.
#[derive(Debug, PartialEq)]
struct Holder<T>(T);

impl<T: SerializeShrinkWrap> SerializeShrinkWrap for Holder<T> {
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write(&self.0)
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Holder<T> {
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        Ok(Holder(rd.read()?))
    }
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct DeriveOption {
    before: u8,
    value: Option<Evolving>,
    after: u8,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct GenericOption {
    before: u8,
    value: Holder<Option<Evolving>>,
    after: u8,
}

/// `Option<T>` is SelfDescribing also for an Unsized `T`: the Unsized value gets its size slot from the Option's
/// own `wr.write()`, the Option needs none. Same bytes as an `Option` field of a derived type, apart from where
/// the flag is.
#[test]
fn option_of_unsized_is_self_describing() {
    assert_eq!(ser_size::<Evolving>(), ElementSize::Unsized);
    assert_eq!(ser_size::<Option<Evolving>>(), ElementSize::SelfDescribing);
    assert_eq!(des_size::<Option<Evolving>>(), ElementSize::SelfDescribing);

    let mut buf = [0u8; 16];
    let generic = GenericOption {
        before: 0xAA,
        value: Holder(Some(Evolving { a: 0xBB })),
        after: 0xCC,
    };
    let bytes = generic.to_ww_bytes(&mut buf).unwrap();
    // flag, the value (byte aligned), `after`, one size slot at the back
    assert_eq!(bytes, hex!("AA 80 BB CC 01"));
    assert_eq!(GenericOption::from_ww_bytes(bytes).unwrap(), generic);

    let mut buf = [0u8; 16];
    let derive = DeriveOption {
        before: 0xAA,
        value: Some(Evolving { a: 0xBB }),
        after: 0xCC,
    };
    let bytes = derive.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, hex!("AA 80 BB CC 01"));
    assert_eq!(DeriveOption::from_ww_bytes(bytes).unwrap(), derive);
}
