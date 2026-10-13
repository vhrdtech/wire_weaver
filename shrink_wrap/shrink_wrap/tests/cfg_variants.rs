//! `#[cfg(..)]` on enum variants (DER-8): a variant compiled out keeps its discriminant, so both builds agree on
//! the bytes of every variant they share, and the smaller build reports the ones it lacks.
//!
//! `cfg(any())` is always false and `cfg(all())` always true, so no cargo feature is needed here.

use shrink_wrap::prelude::*;
use shrink_wrap::{ElementSize, Error};

#[derive_shrink_wrap(
    borrowed,
    derive(Debug, PartialEq),
    self_describing,
    ww_repr = unib32
)]
enum Full {
    A(u8),
    B(u128),
    C,
    D(u16),
    E { x: u64, y: u8 },
}

#[derive_shrink_wrap(
    borrowed,
    derive(Debug, PartialEq),
    self_describing,
    ww_repr = unib32
)]
enum Subset {
    A(u8),
    #[cfg(any())]
    B(u128),
    #[cfg(any())]
    C,
    #[cfg(all())]
    D(u16),
    // a type that does not exist where the variant is compiled out
    #[cfg(any())]
    E { x: NoSuchType, y: u8 },
}

#[test]
fn same_bytes_for_shared_variants() {
    let mut full = [0u8; 32];
    let mut subset = [0u8; 32];
    assert_eq!(
        Full::A(0xAA).to_ww_bytes(&mut full).unwrap(),
        Subset::A(0xAA).to_ww_bytes(&mut subset).unwrap()
    );
    // D is after two compiled out variants and still number 3
    assert_eq!(Subset::D(1).discriminant(), 3);
    let bytes = Full::D(0x1234).to_ww_bytes(&mut full).unwrap();
    assert_eq!(bytes, Subset::D(0x1234).to_ww_bytes(&mut subset).unwrap());
    assert_eq!(Subset::from_ww_bytes(bytes), Ok(Subset::D(0x1234)));
}

#[test]
fn compiled_out_variant_is_reported() {
    let mut buf = [0u8; 32];
    for value in [Full::B(7), Full::C, Full::E { x: 1, y: 2 }] {
        let bytes = value.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(
            Subset::from_ww_bytes(bytes),
            Err(Error::EnumVariantDisabled)
        );
    }
    // a discriminant nobody knows is still the old error
    assert_eq!(
        Subset::from_ww_bytes(&[0x70]),
        Err(Error::EnumFutureVersionOrMalformedData)
    );
}

#[test]
fn smaller_in_memory() {
    assert!(size_of::<Subset>() < size_of::<Full>());
    assert!(size_of::<Subset>() <= 8);
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), sized, ww_repr = u8)]
enum SizedFull {
    A(u8),
    B(u8),
    C(u8),
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), sized, ww_repr = u8)]
enum SizedSubset {
    #[cfg(any())]
    A(u8),
    B(u8),
    #[cfg(all())]
    C(u8),
}

#[test]
fn sized_enum_keeps_its_size() {
    const SIZE: ElementSize = <SizedSubset as SerializeShrinkWrap>::ELEMENT_SIZE;
    assert_eq!(SIZE, <SizedFull as SerializeShrinkWrap>::ELEMENT_SIZE);
    assert_eq!(SIZE, ElementSize::Sized { size_bits: 16 });

    let mut buf = [0u8; 8];
    let bytes = SizedFull::C(5).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(SizedSubset::from_ww_bytes(bytes), Ok(SizedSubset::C(5)));
    // variant 0 compiled out, discriminant read through the latch
    let bytes = SizedFull::A(5).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(
        SizedSubset::from_ww_bytes(bytes),
        Err(Error::EnumVariantDisabled)
    );
    // a failed read is 0 too: the read error comes first
    assert_eq!(
        SizedSubset::from_ww_bytes(&[]),
        Err(Error::OutOfBoundsReadU8)
    );
}

/// The only variant that pushes a length is behind `#[cfg]`.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq), ww_repr = u8)]
enum LenFullOwned {
    A(u8),
    #[cfg(all())]
    B(Vec<u8>),
}

#[derive_shrink_wrap(owned, derive(Debug, PartialEq), ww_repr = u8)]
enum LenSubsetOwned {
    A(u8),
    #[cfg(any())]
    B(Vec<u8>),
}

#[test]
fn owned_and_lengths() {
    let bytes = LenFullOwned::B(vec![1, 2, 3]).to_ww_bytes_owned().unwrap();
    assert_eq!(
        LenFullOwned::from_ww_bytes_owned(&bytes),
        Ok(LenFullOwned::B(vec![1, 2, 3]))
    );
    assert_eq!(
        LenSubsetOwned::from_ww_bytes_owned(&bytes),
        Err(Error::EnumVariantDisabled)
    );
    let bytes = LenFullOwned::A(9).to_ww_bytes_owned().unwrap();
    assert_eq!(bytes, LenSubsetOwned::A(9).to_ww_bytes_owned().unwrap());
    assert_eq!(
        LenSubsetOwned::from_ww_bytes_owned(&bytes),
        Ok(LenSubsetOwned::A(9))
    );
}
