//! Derived structs with type parameters (DER-14): `Timed<T>`, a borrowed `Tagged<'i, T>` with its owned
//! `TaggedOwned<T>`, bounds written next to the parameter.

use shrink_wrap::prelude::*;
use shrink_wrap::{ElementSize, SerializeShrinkWrap};

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
