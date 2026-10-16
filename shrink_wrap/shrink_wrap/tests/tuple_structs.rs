//! Tuple and unit structs (SW-59): the same bytes as a struct with the same fields named, in every layout, round trips,
//! readers and writers of different versions.

use hex_literal::hex;
use shrink_wrap::fixed_layout::FixedLayout;
use shrink_wrap::head_tail::HeadTail;
use shrink_wrap::prelude::*;

fn bytes<T: SerializeShrinkWrap>(value: &T) -> Vec<u8> {
    let mut buf = [0u8; 128];
    value.to_ww_bytes(&mut buf).unwrap().to_vec()
}

fn owned_bytes<T: SerializeShrinkWrapOwned>(value: &T) -> Vec<u8> {
    value.to_ww_bytes_owned().unwrap()
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
struct Celsius(f32);

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct CelsiusNamed {
    value: f32,
}

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
struct Rgb(u8, u8, u8);

#[derive_shrink_wrap(borrowed, final_structure, derive(Debug, PartialEq, Clone, Copy))]
struct RgbFinal(u8, u8, u8);

#[derive_shrink_wrap(borrowed, self_describing, derive(Debug, PartialEq, Clone, Copy))]
struct RgbSelfDescribing(u8, u8, u8);

#[derive_shrink_wrap(borrowed, owned, sized, derive(Debug, PartialEq, Clone, Copy))]
struct RgbSized(u8, u8, u8);

#[derive_shrink_wrap(borrowed, sized, derive(Debug, PartialEq, Clone, Copy))]
struct RgbSizedNamed {
    r: u8,
    g: u8,
    b: u8,
}

#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
struct Pair<'i>(u8, &'i str);

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone))]
struct PairNamed<'i> {
    a: u8,
    b: &'i str,
}

/// Flags of `Option` and `Result` fields in a tuple struct.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
struct Flags<'i>(Option<u8>, u16, Result<&'i str, u8>, Option<RefVec<'i, u8>>);

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone))]
struct FlagsNamed<'i> {
    a: Option<u8>,
    b: u16,
    c: Result<&'i str, u8>,
    d: Option<RefVec<'i, u8>>,
}

/// A tuple struct inside another one, and in a list.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
struct Nested<'i>(Rgb, RefVec<'i, Celsius>, RgbSized);

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct Marker;

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct WithMarker(u8, Marker, u8);

/// `Marker` after it grew a field: an older reader still reads it, as `Marker`.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct MarkerV2(u8);

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct WithMarkerV2(u8, MarkerV2, u8);

#[test]
fn newtype_same_bytes_as_named() {
    let value = Celsius(21.5);
    assert_eq!(bytes(&value), bytes(&CelsiusNamed { value: 21.5 }));
    assert_eq!(bytes(&value), hex!("0000ac41"));
    assert_eq!(Celsius::from_ww_bytes(&bytes(&value)).unwrap(), value);
    assert_eq!(value.0, 21.5);
}

#[test]
fn size_assumptions() {
    assert_eq!(bytes(&Rgb(1, 2, 3)), hex!("010203"));
    assert_eq!(bytes(&RgbFinal(1, 2, 3)), hex!("010203"));
    assert_eq!(bytes(&RgbSelfDescribing(1, 2, 3)), hex!("010203"));
    assert_eq!(bytes(&RgbSized(1, 2, 3)), hex!("010203"));
    // a sized struct gets the fixed-layout fast path without asking
    assert_eq!(RgbSized::_2_OFFSET, 2);
    assert_eq!(
        <RgbSized as SerializeShrinkWrap>::ELEMENT_SIZE,
        <RgbSizedNamed as SerializeShrinkWrap>::ELEMENT_SIZE
    );
    assert_eq!(
        <RgbSized as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::Sized { size_bits: 24 }
    );
    assert_eq!(
        <RgbFinal as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::UnsizedFinalStructure
    );
    assert_eq!(
        <RgbSelfDescribing as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::SelfDescribing
    );
    assert_eq!(
        <Rgb as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::Unsized
    );
    {
        let b = bytes(&Rgb(1, 2, 3));
        assert_eq!(Rgb::from_ww_bytes(&b).unwrap(), Rgb(1, 2, 3));
        assert_eq!(RgbFinal::from_ww_bytes(&b).unwrap(), RgbFinal(1, 2, 3));
        assert_eq!(
            RgbSelfDescribing::from_ww_bytes(&b).unwrap(),
            RgbSelfDescribing(1, 2, 3)
        );
        assert_eq!(RgbSized::from_ww_bytes(&b).unwrap(), RgbSized(1, 2, 3));
    }
}

#[test]
fn borrowed_and_owned() {
    let pair = Pair(7, "hi");
    let b = bytes(&pair);
    assert_eq!(b, bytes(&PairNamed { a: 7, b: "hi" }));
    assert_eq!(b, hex!("07686902"));
    assert_eq!(Pair::from_ww_bytes(&b).unwrap(), pair);
    let owned = PairOwned(7, "hi".into());
    assert_eq!(owned_bytes(&owned), b);
    assert_eq!(PairOwned::from_ww_bytes_owned(&b).unwrap(), owned);
}

#[test]
fn flags() {
    let values = [
        (
            Flags(Some(1), 2, Ok("x"), Some(RefVec::new_bytes(&[5, 6]))),
            FlagsNamed {
                a: Some(1),
                b: 2,
                c: Ok("x"),
                d: Some(RefVec::new_bytes(&[5, 6])),
            },
        ),
        (
            Flags(None, 3, Err(4), None),
            FlagsNamed {
                a: None,
                b: 3,
                c: Err(4),
                d: None,
            },
        ),
    ];
    for (tuple, named) in values {
        let b = bytes(&tuple);
        assert_eq!(b, bytes(&named));
        assert_eq!(Flags::from_ww_bytes(&b).unwrap(), tuple);
    }
    let owned = FlagsOwned(Some(1), 2, Ok("x".into()), Some(vec![5, 6]));
    let b = owned_bytes(&owned);
    assert_eq!(
        b,
        bytes(&Flags(
            Some(1),
            2,
            Ok("x"),
            Some(RefVec::new_bytes(&[5, 6]))
        ))
    );
    assert_eq!(FlagsOwned::from_ww_bytes_owned(&b).unwrap(), owned);
}

#[test]
fn nested() {
    let temps = [Celsius(1.0), Celsius(-2.0)];
    let value = Nested(
        Rgb(1, 2, 3),
        RefVec::Slice { slice: &temps },
        RgbSized(4, 5, 6),
    );
    let b = bytes(&value);
    assert_eq!(Nested::from_ww_bytes(&b).unwrap(), value);
    let owned = NestedOwned(Rgb(1, 2, 3), temps.to_vec(), RgbSized(4, 5, 6));
    assert_eq!(owned_bytes(&owned), b);
    assert_eq!(NestedOwned::from_ww_bytes_owned(&b).unwrap(), owned);
}

#[test]
fn unit_structs() {
    // On its own nothing follows it, so no length is needed
    assert_eq!(bytes(&Marker), hex!(""));
    assert_eq!(Marker::from_ww_bytes(&[]).unwrap(), Marker);
    // Inside a parent it records its length, 0, in a nibble, so it can grow later
    let value = WithMarker(1, Marker, 2);
    let b = bytes(&value);
    assert_eq!(b, hex!("01 02 00"));
    assert_eq!(WithMarker::from_ww_bytes(&b).unwrap(), value);
    // A newer `Marker` with a field is read by older code as `Marker`; the newer reader needs `#[default]` on the
    // field to read old data (see `evolution`), without it it refuses
    let newer = bytes(&WithMarkerV2(1, MarkerV2(5), 2));
    assert_eq!(WithMarker::from_ww_bytes(&newer).unwrap(), value);
    assert!(WithMarkerV2::from_ww_bytes(&b).is_err());
}

/// `RateV1` with a field appended later: positional fields are only ever added at the end.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct RateV1(u16);

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone))]
struct RateV2<'i>(
    u16,
    #[default = 7] u8,
    #[default = None] Option<u8>,
    #[default = "none"] &'i str,
);

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone))]
struct Outer<'i>(RateV1, u8);

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone))]
struct OuterV2<'i>(RateV2<'i>, u8);

#[test]
fn evolution() {
    let old = bytes(&RateV1(1000));
    assert_eq!(
        RateV2::from_ww_bytes(&old).unwrap(),
        RateV2(1000, 7, None, "none")
    );
    let new = bytes(&RateV2(1000, 9, Some(3), "abc"));
    assert_eq!(RateV1::from_ww_bytes(&new).unwrap(), RateV1(1000));
    // nested: the length of the inner value lets the old reader skip the new fields
    let new = bytes(&OuterV2(RateV2(5, 9, Some(3), "abc"), 42));
    assert_eq!(Outer::from_ww_bytes(&new).unwrap(), Outer(RateV1(5), 42));
    let old = bytes(&Outer(RateV1(5), 42));
    assert_eq!(
        OuterV2::from_ww_bytes(&old).unwrap(),
        OuterV2(RateV2(5, 7, None, "none"), 42)
    );
}

fn check_even(value: &Even) -> Result<(), ShrinkWrapError> {
    if value.0.is_multiple_of(2) {
        Ok(())
    } else {
        Err(ShrinkWrapError::SubtypeOutOfRange)
    }
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), validate = check_even)]
struct Even(u8);

#[test]
fn validate() {
    assert_eq!(Even::from_ww_bytes(&bytes(&Even(4))).unwrap(), Even(4));
    assert!(Even::from_ww_bytes(&bytes(&Even(5))).is_err());
}

/// Line {0} is out of range, at most {1}
#[derive_shrink_wrap(borrowed, display, derive(Debug))]
struct LineError(u8, u8);

#[test]
fn display() {
    assert_eq!(
        LineError(9, 4).to_string(),
        "Line 9 is out of range, at most 4"
    );
}

#[derive_shrink_wrap(sparse, owned, derive(Debug, PartialEq, Clone, Default))]
struct Sparse<'i>(Option<u16>, #[slot = 3] Option<&'i str>, Option<bool>);

#[derive_shrink_wrap(sparse, owned, derive(Debug, PartialEq, Clone, Default))]
struct SparseNamed<'i> {
    a: Option<u16>,
    #[slot = 3]
    b: Option<&'i str>,
    c: Option<bool>,
}

#[test]
fn sparse() {
    let value = Sparse(Some(5), None, Some(true));
    let b = bytes(&value);
    assert_eq!(
        b,
        bytes(&SparseNamed {
            a: Some(5),
            b: None,
            c: Some(true),
        })
    );
    assert_eq!(Sparse::from_ww_bytes(&b).unwrap(), value);
    let owned = SparseOwned(Some(5), Some("x".into()), None);
    let b = owned_bytes(&owned);
    assert_eq!(
        Sparse::from_ww_bytes(&b).unwrap(),
        Sparse(Some(5), Some("x"), None)
    );
    assert_eq!(SparseOwned::from_ww_bytes_owned(&b).unwrap(), owned);
}

#[derive_shrink_wrap(borrowed, fixed_layout, derive(Debug, PartialEq, Clone, Copy))]
struct Px(u8, bool, u16);

#[derive_shrink_wrap(borrowed, fixed_layout, derive(Debug, PartialEq, Clone, Copy))]
struct PxNamed {
    a: u8,
    b: bool,
    c: u16,
}

#[test]
fn fixed_layout() {
    let value = Px(1, true, 0x0203);
    let b = bytes(&value);
    assert_eq!(
        b,
        bytes(&PxNamed {
            a: 1,
            b: true,
            c: 0x0203
        })
    );
    assert_eq!(Px::ENCODED_LEN, PxNamed::ENCODED_LEN);
    assert_eq!(Px::_2_OFFSET, PxNamed::C_OFFSET);
    let mut buf = [0u8; Px::ENCODED_LEN];
    let len = value.to_fixed(&mut buf).unwrap();
    assert_eq!(&buf[..len], b.as_slice());
    assert_eq!(Px::from_fixed(&buf).unwrap(), value);
    assert_eq!(<Px as FixedLayout>::read_fixed(&buf).unwrap(), value);
}

#[derive_shrink_wrap(head_tail, derive(Debug, PartialEq, Clone, Copy))]
struct Ht<'i>(u8, Option<u16>, &'i str);

#[derive_shrink_wrap(head_tail, derive(Debug, PartialEq, Clone, Copy))]
struct HtNamed<'i> {
    a: u8,
    b: Option<u16>,
    c: &'i str,
}

fn ht_bytes<'i, T: HeadTail<'i>>(value: &T) -> Vec<u8> {
    let mut buf = [0u8; 64];
    let len = shrink_wrap::head_tail::write(value, &mut buf).unwrap();
    buf[..len].to_vec()
}

#[test]
fn head_tail() {
    let value = Ht(1, Some(0x0203), "abc");
    let b = ht_bytes(&value);
    assert_eq!(
        b,
        ht_bytes(&HtNamed {
            a: 1,
            b: Some(0x0203),
            c: "abc"
        })
    );
    assert_eq!(shrink_wrap::head_tail::read::<Ht>(&b).unwrap(), value);
    assert_eq!(Ht::HEAD_LEN, HtNamed::HEAD_LEN);
}

/// C names of positional fields are `_0`, `_1`, ... after the type's name.
#[test]
fn c_names() {
    let mut c = String::new();
    <Px as FixedLayout>::write_c("", &mut c).unwrap();
    <Ht as HeadTail>::write_c_ht("", &mut c).unwrap();
    for line in [
        "#define PX__2_OFFSET 2u",
        "static inline uint16_t px__2(const uint8_t *p) { return sw_fixed_u16(p + 2u); }",
        "#define HT__1_FLAG_BIT_OFFSET 8u",
        "    const uint8_t *_2; /* the bytes, not 0-terminated */",
    ] {
        assert!(c.lines().any(|l| l == line), "{line} not in:\n{c}");
    }
}

#[derive_shrink_wrap(head_tail, derive(Debug, PartialEq, Clone, Copy))]
struct UnitHt;

#[derive_shrink_wrap(sparse, borrowed, owned, derive(Debug, PartialEq, Clone, Default))]
struct UnitSparse;

#[test]
fn unit_struct_layouts() {
    assert_eq!(UnitHt::HEAD_LEN, 0);
    assert_eq!(shrink_wrap::head_tail::read::<UnitHt>(&[]).unwrap(), UnitHt);
    let b = bytes(&UnitSparse);
    assert_eq!(UnitSparse::from_ww_bytes(&b).unwrap(), UnitSparse);
}
