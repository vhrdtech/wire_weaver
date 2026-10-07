//! User types with generic arguments as fields (DER-10): `Foo<u8>` is emitted as written, `Span<'i, T>` becomes
//! `SpanOwned<T>` in the owned variant.

use hex_literal::hex;
use shrink_wrap::prelude::*;
use shrink_wrap::{BufReader, BufWriter, ElementSize, Error};

/// Two values of the same type, Sized when `T` is.
#[derive(Debug, PartialEq, Clone, Copy)]
struct Twice<T>(T, T);

impl<T: SerializeShrinkWrap> SerializeShrinkWrap for Twice<T> {
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE.add(T::ELEMENT_SIZE);

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write(&self.0)?;
        wr.write(&self.1)
    }
}

impl<T: SerializeShrinkWrapOwned> SerializeShrinkWrapOwned for Twice<T> {
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE.add(T::ELEMENT_SIZE);

    fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), Error> {
        wr.write(&self.0)?;
        wr.write(&self.1)
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Twice<T> {
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE.add(T::ELEMENT_SIZE);

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        Ok(Twice(rd.read()?, rd.read()?))
    }
}

impl<T: DeserializeShrinkWrapOwned> DeserializeShrinkWrapOwned for Twice<T> {
    const ELEMENT_SIZE: ElementSize = T::ELEMENT_SIZE.add(T::ELEMENT_SIZE);

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        Ok(Twice(rd.read_owned()?, rd.read_owned()?))
    }
}

/// A sequence with a lifetime and a type argument, like the SW-31/SW-32 wrappers: borrowed over a `RefVec`,
/// owned over a `Vec`.
#[derive(Clone, Copy)]
struct Span<'i, T>(RefVec<'i, T>);

impl<'i, T: DeserializeShrinkWrap<'i> + PartialEq + Clone> PartialEq for Span<'i, T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.iter().eq(other.0.iter())
    }
}

impl<T> core::fmt::Debug for Span<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Span({} elements)", self.0.len())
    }
}

#[derive(Debug, PartialEq, Clone)]
struct SpanOwned<T>(Vec<T>);

impl<'i, T: SerializeShrinkWrap + DeserializeShrinkWrap<'i> + Clone> SerializeShrinkWrap
    for Span<'i, T>
{
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write(&self.0)
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Span<'i, T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        Ok(Span(rd.read()?))
    }
}

impl<T: SerializeShrinkWrapOwned> SerializeShrinkWrapOwned for SpanOwned<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), Error> {
        wr.write(&self.0)
    }
}

impl<T: DeserializeShrinkWrapOwned> DeserializeShrinkWrapOwned for SpanOwned<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        Ok(SpanOwned(rd.read_owned()?))
    }
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq))]
struct Rec<'i> {
    a: Twice<u8>,
    size: TailSize<1>, // Twice<u8> before the slot: the Sized assertion names the full type
    b: Span<'i, u16>,
    c: Option<Twice<u8>>,
    d: RefVec<'i, Twice<u8>>,
    e: (Twice<u8>, Span<'i, u8>),
}

#[derive_shrink_wrap(borrowed, owned, ww_repr = u4, derive(Debug, PartialEq))]
enum Msg<'i> {
    A(Twice<u8>),
    B { s: Span<'i, u8> },
    C,
}

/// `<Twice<u8> as SerializeShrinkWrap>::ELEMENT_SIZE` is summed into the const.
#[derive_shrink_wrap(borrowed, owned, sized, derive(Debug, PartialEq))]
struct Fixed {
    a: Twice<u8>,
    b: Twice<Twice<bool>>,
}

#[test]
fn struct_round_trip_borrowed_and_owned() {
    let d = [Twice(1u8, 2), Twice(3, 4)];
    let rec = Rec {
        a: Twice(0xAA, 0xBB),
        size: TailSize(0),
        b: Span(RefVec::Slice {
            slice: &[0x1234, 0x5678],
        }),
        c: Some(Twice(7, 8)),
        d: RefVec::Slice { slice: &d },
        e: (Twice(9, 10), Span(RefVec::Slice { slice: &[11] })),
    };
    let mut buf = [0u8; 64];
    let bytes = rec.to_ww_bytes(&mut buf).unwrap();
    let owned = RecOwned::from_ww_bytes_owned(bytes).unwrap();
    assert_eq!(
        owned,
        RecOwned {
            a: Twice(0xAA, 0xBB),
            size: TailSize(bytes.len() as u32 - 3),
            b: SpanOwned(vec![0x1234, 0x5678]),
            c: Some(Twice(7, 8)),
            d: vec![Twice(1, 2), Twice(3, 4)],
            e: (Twice(9, 10), SpanOwned(vec![11])),
        }
    );
    assert_eq!(owned.to_ww_bytes_owned().unwrap(), bytes);
    let back = Rec::from_ww_bytes(bytes).unwrap();
    assert_eq!(back.a, rec.a);
    assert_eq!(back.c, rec.c);
    assert_eq!(
        back.b.0.iter().collect::<Result<Vec<_>, _>>().unwrap(),
        vec![0x1234, 0x5678]
    );
    assert_eq!(back.d.iter().collect::<Result<Vec<_>, _>>().unwrap(), d);
    assert_eq!(back.e.0, Twice(9, 10));
    assert_eq!(
        back.e.1.0.iter().collect::<Result<Vec<_>, _>>().unwrap(),
        vec![11]
    );
}

#[test]
fn enum_round_trip() {
    let mut buf = [0u8; 16];
    let a = Msg::A(Twice(1, 2)).to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(
        MsgOwned::from_ww_bytes_owned(&a).unwrap(),
        MsgOwned::A(Twice(1, 2))
    );
    let b = Msg::B {
        s: Span(RefVec::Slice { slice: &[5, 6] }),
    }
    .to_ww_bytes(&mut buf)
    .unwrap()
    .to_vec();
    assert_eq!(
        MsgOwned::from_ww_bytes_owned(&b).unwrap(),
        MsgOwned::B {
            s: SpanOwned(vec![5, 6])
        }
    );
    assert_eq!(
        MsgOwned::B {
            s: SpanOwned(vec![5, 6])
        }
        .to_ww_bytes_owned()
        .unwrap(),
        b
    );
    assert!(matches!(Msg::from_ww_bytes(&b).unwrap(), Msg::B { .. }));
    let c = Msg::C.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(MsgOwned::from_ww_bytes_owned(&c).unwrap(), MsgOwned::C);
}

#[test]
fn sized_with_generic_fields() {
    assert_eq!(
        <Fixed as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::Sized { size_bits: 16 + 4 }
    );
    let mut buf = [0u8; 8];
    let fixed = Fixed {
        a: Twice(1, 2),
        b: Twice(Twice(true, false), Twice(false, true)),
    };
    let bytes = fixed.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, hex!("01 02 90"));
    assert_eq!(Fixed::from_ww_bytes(bytes).unwrap(), fixed);
}
