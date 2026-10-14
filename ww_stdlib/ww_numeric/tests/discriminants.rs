//! `NumericValue` variants are behind cargo features, their numbers on the wire are not: every build agrees on them.
#![cfg(feature = "all")]

use shrink_wrap::prelude::*;
use ww_numeric::{NumericValue, UVlq32, UVlq64};

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
        (NumericValue::UVlq32(UVlq32(0)), 16),
        (NumericValue::UVlq64(UVlq64(0)), 17),
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

/// Discriminants 16 and 17 take two nibbles, then the value is byte aligned.
#[test]
fn variable_length_on_the_wire() {
    let mut buf = [0u8; 16];
    let cases: [(NumericValue, &[u8]); 4] = [
        (NumericValue::UVlq32(UVlq32(5)), &[0x82, 0x05]),
        (NumericValue::UVlq32(UVlq32(300)), &[0x82, 0x82, 0x2C]),
        (NumericValue::UVlq64(UVlq64(5)), &[0x92, 0x05]),
        (
            NumericValue::UVlq64(UVlq64(u64::MAX)),
            &[
                0x92, 0x81, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F,
            ],
        ),
    ];
    for (value, expected) in cases {
        let bytes = value.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(bytes, expected, "{value:?}");
        assert_eq!(NumericValue::from_ww_bytes(bytes).unwrap(), value);
    }
}

#[test]
fn conversions() {
    assert_eq!(NumericValue::from(5u16), NumericValue::U16(5));
    assert_eq!(NumericValue::from(UVlq32(7)).to_i128(), Some(7));
    assert_eq!(NumericValue::I8(-3).to_f64(), -3.0);
    assert_eq!(NumericValue::F32(1.5).to_i128(), None);
    assert_eq!(NumericValue::U128(u128::MAX).to_i128(), None);
    assert_eq!(NumericValue::U16(0).unsigned_bits(), Some(16));
    assert_eq!(NumericValue::UVlq64(UVlq64(0)).unsigned_bits(), Some(64));
    assert_eq!(NumericValue::I32(0).unsigned_bits(), None);
    assert!(NumericValue::F64(0.0).is_float());
}
