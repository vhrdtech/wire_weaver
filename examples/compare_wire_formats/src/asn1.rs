//! What the ASN.1 types of every message share. They declare what an ASN.1 module would: the bounds the tuned
//! `shrink_wrap` types declare as value ranges, a size for a bounded list, and [`Natural`] for a 32 or 64-bit
//! number that usually holds a small value.

use rasn::prelude::*;

/// `INTEGER (0..MAX)`: UPER writes a length, then as few octets as the value needs.
#[derive(AsnType, Encode, Decode, Clone, PartialEq, Debug)]
#[rasn(delegate, value("0..="))]
pub struct Natural(pub Integer);

impl Natural {
    pub fn new(value: impl Into<u64>) -> Self {
        Self(Integer::from(value.into()))
    }
}

/// UPER bytes of a rasn value.
pub fn uper<T: rasn::Encode>(value: &T) -> Vec<u8> {
    rasn::uper::encode(value).expect("UPER encodes the example")
}

/// Defines an ASN.1 SEQUENCE twice from one field list: closed, and with an extension marker (`...`) that lets a
/// later revision append fields an old decoder skips. The extensible one costs one more bit on the wire.
macro_rules! asn1_pair {
    ($closed:ident, $extensible:ident { $($body:tt)* }) => {
        #[derive(AsnType, Encode, Decode, Clone, PartialEq, Debug)]
        #[rasn(automatic_tags)]
        pub struct $closed { $($body)* }

        #[derive(AsnType, Encode, Decode, Clone, PartialEq, Debug)]
        #[rasn(automatic_tags)]
        #[non_exhaustive]
        pub struct $extensible { $($body)* }
    };
}
pub(crate) use asn1_pair;
