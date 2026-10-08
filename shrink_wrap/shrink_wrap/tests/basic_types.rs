use hex_literal::hex;
use shrink_wrap::prelude::*;

#[test]
fn tuple() {
    let mut scratch = [0u8; 8];

    let bytes = (0xAA_u8, 0xBBCC_u16).to_ww_bytes(&mut scratch).unwrap();
    assert_eq!(bytes, hex!("AA CC BB"));

    let x: (u8, u16) = DeserializeShrinkWrap::from_ww_bytes(bytes).unwrap();
    assert_eq!(x.0, 0xAA);
    assert_eq!(x.1, 0xBBCC);
}

#[test]
fn tuple_of_bits() {
    let mut scratch = [0u8; 8];

    let bytes = (true, false, true, false, true, true, false, false)
        .to_ww_bytes(&mut scratch)
        .unwrap();
    assert_eq!(bytes.len(), 1);
    assert_eq!(bytes[0], 0b10101100);

    let x: (bool, bool, bool, bool, bool, bool, bool, bool) =
        DeserializeShrinkWrap::from_ww_bytes(bytes).unwrap();
    assert!(x.0);
    assert!(!x.1);
    assert!(x.2);
    assert!(!x.3);
    assert!(x.4);
    assert!(x.5);
    assert!(!x.6);
    assert!(!x.7);
}

#[test]
fn tuple_of_strings() {
    let mut scratch = [0u8; 9];

    let bytes = ("abc", "de").to_ww_bytes(&mut scratch).unwrap();
    assert_eq!(bytes, hex!("61 62 63 64 65 23"));

    let x: (&str, &str) = DeserializeShrinkWrap::from_ww_bytes(bytes).unwrap();
    assert_eq!(x.0, "abc");
    assert_eq!(x.1, "de");
}

#[test]
fn array() {
    let mut scratch = [0u8; 8];

    let bytes = [1u8, 2, 3, 4, 5].to_ww_bytes(&mut scratch).unwrap();
    assert_eq!(bytes, &[1, 2, 3, 4, 5]);

    let x: [u8; 5] = DeserializeShrinkWrap::from_ww_bytes(bytes).unwrap();
    assert_eq!(x, [1, 2, 3, 4, 5]);
}

#[test]
fn unit() {
    let _unit: () = DeserializeShrinkWrap::from_ww_bytes(&[]).unwrap();
}

#[test]
fn uvlq32_field() {
    #[derive_shrink_wrap(borrowed, final_structure, derive(Debug, PartialEq))]
    struct S {
        flag: bool,
        len: UVlq32,
    }

    let mut scratch = [0u8; 8];
    let s = S {
        flag: true,
        len: UVlq32(0x80),
    };
    let bytes = s.to_ww_bytes(&mut scratch).unwrap();
    assert_eq!(bytes, hex!("80 81 00"));
    assert_eq!(S::from_ww_bytes(bytes).unwrap(), s);
}

#[test]
fn uvlq64_field() {
    #[derive_shrink_wrap(borrowed, final_structure, derive(Debug, PartialEq))]
    struct S {
        flag: bool,
        len: UVlq64,
    }

    let mut scratch = [0u8; 16];
    let s = S {
        flag: true,
        len: UVlq64(u64::MAX),
    };
    let bytes = s.to_ww_bytes(&mut scratch).unwrap();
    assert_eq!(bytes, hex!("80 81 ff ff ff ff ff ff ff ff 7f"));
    assert_eq!(S::from_ww_bytes(bytes).unwrap(), s);
}

#[test]
fn uvlq32_backfill_first_field() {
    #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
    struct Message<'i> {
        seq: UVlq32Backfill,
        payload: &'i str,
    }

    let mut scratch = [0u8; 16];
    let bytes = Message {
        seq: UVlq32Backfill(0),
        payload: "ab",
    }
    .to_ww_bytes(&mut scratch)
    .unwrap();
    assert_eq!(bytes, hex!("80 80 80 80 00 61 62 02"));
    let len = bytes.len();
    let bytes = UVlq32Backfill::backfill(&mut scratch[..len], 300).unwrap();
    assert_eq!(bytes, hex!("82 2c 61 62 02"));
    assert_eq!(
        Message::from_ww_bytes(bytes).unwrap(),
        Message {
            seq: UVlq32Backfill(300),
            payload: "ab"
        }
    );
    assert_eq!(scratch[..len], hex!("80 80 80 82 2c 61 62 02"));
    assert_eq!(
        Message::from_ww_bytes(&scratch[..len]).unwrap().seq,
        UVlq32Backfill(300)
    );
}
