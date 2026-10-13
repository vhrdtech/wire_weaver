//! `NumericValue` variants are behind cargo features, their numbers on the wire are not: every build agrees on them.
#![cfg(feature = "all")]

use shrink_wrap::prelude::*;
use ww_numeric::NumericValue;

#[test]
fn numbers_do_not_depend_on_features() {
    let expected = [
        (NumericValue::Nibble(Nibble::zero()), 0),
        (NumericValue::U8(0), 1),
        (NumericValue::U16(0), 2),
        (NumericValue::U32(0), 3),
        (NumericValue::UNib32(0), 4),
        (NumericValue::U64(0), 5),
        (NumericValue::I32(0), 6),
        (NumericValue::F32(0.0), 7),
        (NumericValue::U128(0), 10),
        (NumericValue::I8(0), 11),
        (NumericValue::I16(0), 12),
        (NumericValue::I64(0), 13),
        (NumericValue::I128(0), 14),
        (NumericValue::F64(0.0), 15),
    ];
    for (value, discriminant) in expected {
        assert_eq!(value.discriminant(), discriminant, "{value:?}");
    }
}

#[test]
fn f32_on_the_wire() {
    let mut buf = [0u8; 8];
    let bytes = NumericValue::F32(1.0).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, [0x70, 0x00, 0x00, 0x80, 0x3F]);
}
