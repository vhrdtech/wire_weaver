//! `#[derive_shrink_wrap]` accepts a field type that comes from a macro_rules `$T:ty` fragment (DER-11).
//!
//! When a `$T:ty` fragment is substituted into a macro's output, rustc wraps it in an invisible
//! `syn::Type::Group` for hygiene (and a literal `(u16)` parses as `syn::Type::Paren`); the transform used to
//! reject both with a debug dump of the `syn::Type`. `$T:ident` was the documented workaround, kept in
//! `ref_vec_skip.rs`.
//!
//! Every macro-generated type below has a hand-written twin with the same type spelled out directly, and the
//! tests check the two produce identical bytes, proving the `$T:ty` path is not just accepted but transparent.

use shrink_wrap::prelude::*;

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct Nested {
    a: u8,
    b: u16,
}

/// The shape of the original repro: `$T` inside `RefVec<'i, $T>`.
macro_rules! list_of {
    ($name:ident, $T:ty) => {
        #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
        struct $name<'i> {
            list: RefVec<'i, $T>,
        }
    };
}

list_of!(ListU16, u16);
list_of!(ListArray, [u8; 4]);
list_of!(ListOption, Option<u16>);
list_of!(ListRefVec, RefVec<'i, u8>);
list_of!(ListNested, Nested);
list_of!(ListParen, (u16));

// Hand-written twins: the same field, with the type spelled out directly instead of substituted by a macro.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct ListU16Hand<'i> {
    list: RefVec<'i, u16>,
}
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct ListArrayHand<'i> {
    list: RefVec<'i, [u8; 4]>,
}
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct ListOptionHand<'i> {
    list: RefVec<'i, Option<u16>>,
}
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct ListRefVecHand<'i> {
    list: RefVec<'i, RefVec<'i, u8>>,
}
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct ListNestedHand<'i> {
    list: RefVec<'i, Nested>,
}
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct ListParenHand<'i> {
    list: RefVec<'i, (u16)>,
}

#[test]
fn list_u16_matches_hand_written_and_round_trips() {
    let data = [1u16, 2, 0xFFFF];
    let a = ListU16 {
        list: RefVec::Slice { slice: &data },
    };
    let b = ListU16Hand {
        list: RefVec::Slice { slice: &data },
    };
    let mut buf_a = [0u8; 32];
    let mut buf_b = [0u8; 32];
    let bytes_a = a.to_ww_bytes(&mut buf_a).unwrap();
    let bytes_b = b.to_ww_bytes(&mut buf_b).unwrap();
    assert_eq!(bytes_a, bytes_b);
    let back = ListU16::from_ww_bytes(bytes_a).unwrap();
    assert_eq!(
        back.list.iter().collect::<Result<Vec<_>, _>>().unwrap(),
        data
    );
}

#[test]
fn list_array_matches_hand_written_and_round_trips() {
    let data = [[1u8, 2, 3, 4], [5, 6, 7, 8]];
    let a = ListArray {
        list: RefVec::Slice { slice: &data },
    };
    let b = ListArrayHand {
        list: RefVec::Slice { slice: &data },
    };
    let mut buf_a = [0u8; 32];
    let mut buf_b = [0u8; 32];
    let bytes_a = a.to_ww_bytes(&mut buf_a).unwrap();
    let bytes_b = b.to_ww_bytes(&mut buf_b).unwrap();
    assert_eq!(bytes_a, bytes_b);
    let back = ListArray::from_ww_bytes(bytes_a).unwrap();
    assert_eq!(
        back.list.iter().collect::<Result<Vec<_>, _>>().unwrap(),
        data
    );
}

#[test]
fn list_option_matches_hand_written_and_round_trips() {
    let data = [Some(1u16), None, Some(0xFFFF)];
    let a = ListOption {
        list: RefVec::Slice { slice: &data },
    };
    let b = ListOptionHand {
        list: RefVec::Slice { slice: &data },
    };
    let mut buf_a = [0u8; 32];
    let mut buf_b = [0u8; 32];
    let bytes_a = a.to_ww_bytes(&mut buf_a).unwrap();
    let bytes_b = b.to_ww_bytes(&mut buf_b).unwrap();
    assert_eq!(bytes_a, bytes_b);
    let back = ListOption::from_ww_bytes(bytes_a).unwrap();
    assert_eq!(
        back.list.iter().collect::<Result<Vec<_>, _>>().unwrap(),
        data
    );
}

#[test]
fn list_ref_vec_matches_hand_written_and_round_trips() {
    let inner_a = [1u8, 2];
    let inner_b = [3u8, 4, 5];
    let data = [
        RefVec::Slice { slice: &inner_a },
        RefVec::Slice { slice: &inner_b },
    ];
    let a = ListRefVec {
        list: RefVec::Slice { slice: &data },
    };
    let b = ListRefVecHand {
        list: RefVec::Slice { slice: &data },
    };
    let mut buf_a = [0u8; 32];
    let mut buf_b = [0u8; 32];
    let bytes_a = a.to_ww_bytes(&mut buf_a).unwrap();
    let bytes_b = b.to_ww_bytes(&mut buf_b).unwrap();
    assert_eq!(bytes_a, bytes_b);
    let back = ListRefVec::from_ww_bytes(bytes_a).unwrap();
    let back: Vec<Vec<u8>> = back
        .list
        .iter()
        .map(|v| v.unwrap().iter().collect::<Result<Vec<_>, _>>().unwrap())
        .collect();
    assert_eq!(back, vec![vec![1, 2], vec![3, 4, 5]]);
}

#[test]
fn list_nested_matches_hand_written_and_round_trips() {
    let data = [Nested { a: 1, b: 2 }, Nested { a: 3, b: 4 }];
    let a = ListNested {
        list: RefVec::Slice { slice: &data },
    };
    let b = ListNestedHand {
        list: RefVec::Slice { slice: &data },
    };
    let mut buf_a = [0u8; 32];
    let mut buf_b = [0u8; 32];
    let bytes_a = a.to_ww_bytes(&mut buf_a).unwrap();
    let bytes_b = b.to_ww_bytes(&mut buf_b).unwrap();
    assert_eq!(bytes_a, bytes_b);
    let back = ListNested::from_ww_bytes(bytes_a).unwrap();
    assert_eq!(
        back.list.iter().collect::<Result<Vec<_>, _>>().unwrap(),
        data
    );
}

#[test]
fn list_paren_matches_hand_written_and_round_trips() {
    let data = [1u16, 2, 0xFFFF];
    let a = ListParen {
        list: RefVec::Slice { slice: &data },
    };
    let b = ListParenHand {
        list: RefVec::Slice { slice: &data },
    };
    let mut buf_a = [0u8; 32];
    let mut buf_b = [0u8; 32];
    let bytes_a = a.to_ww_bytes(&mut buf_a).unwrap();
    let bytes_b = b.to_ww_bytes(&mut buf_b).unwrap();
    assert_eq!(bytes_a, bytes_b);
    let back = ListParen::from_ww_bytes(bytes_a).unwrap();
    assert_eq!(
        back.list.iter().collect::<Result<Vec<_>, _>>().unwrap(),
        data
    );
}

/// `$T` directly as a field, not wrapped in `RefVec`.
macro_rules! value_of {
    ($name:ident, $T:ty) => {
        #[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
        struct $name<'i> {
            value: $T,
            tail: RefVec<'i, u8>,
        }
    };
}

value_of!(ValueU16, u16);

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct ValueU16Hand<'i> {
    value: u16,
    tail: RefVec<'i, u8>,
}

#[test]
fn value_field_matches_hand_written_and_round_trips() {
    let tail = [9u8, 8, 7];
    let a = ValueU16 {
        value: 0x1234,
        tail: RefVec::Slice { slice: &tail },
    };
    let b = ValueU16Hand {
        value: 0x1234,
        tail: RefVec::Slice { slice: &tail },
    };
    let mut buf_a = [0u8; 32];
    let mut buf_b = [0u8; 32];
    let bytes_a = a.to_ww_bytes(&mut buf_a).unwrap();
    let bytes_b = b.to_ww_bytes(&mut buf_b).unwrap();
    assert_eq!(bytes_a, bytes_b);
    let back = ValueU16::from_ww_bytes(bytes_a).unwrap();
    assert_eq!(back.value, 0x1234);
    assert_eq!(
        back.tail.iter().collect::<Result<Vec<_>, _>>().unwrap(),
        tail
    );
}

/// `$T` as the payload of an enum variant.
macro_rules! enum_of {
    ($name:ident, $T:ty) => {
        #[derive_shrink_wrap(borrowed, ww_repr = u8, derive(Debug, PartialEq, Clone, Copy))]
        enum $name<'i> {
            A($T),
            B { tail: RefVec<'i, u8> },
        }
    };
}

enum_of!(EnumU16, u16);

#[derive_shrink_wrap(borrowed, ww_repr = u8, derive(Debug, PartialEq, Clone, Copy))]
enum EnumU16Hand<'i> {
    A(u16),
    B { tail: RefVec<'i, u8> },
}

#[test]
fn enum_variant_matches_hand_written_and_round_trips() {
    let a = EnumU16::A(0x1234);
    let b = EnumU16Hand::A(0x1234);
    let mut buf_a = [0u8; 16];
    let mut buf_b = [0u8; 16];
    let bytes_a = a.to_ww_bytes(&mut buf_a).unwrap();
    let bytes_b = b.to_ww_bytes(&mut buf_b).unwrap();
    assert_eq!(bytes_a, bytes_b);
    assert_eq!(EnumU16::from_ww_bytes(bytes_a).unwrap(), a);

    let tail = [1u8, 2];
    let a = EnumU16::B {
        tail: RefVec::Slice { slice: &tail },
    };
    let b = EnumU16Hand::B {
        tail: RefVec::Slice { slice: &tail },
    };
    let mut buf_a = [0u8; 16];
    let mut buf_b = [0u8; 16];
    let bytes_a = a.to_ww_bytes(&mut buf_a).unwrap();
    let bytes_b = b.to_ww_bytes(&mut buf_b).unwrap();
    assert_eq!(bytes_a, bytes_b);
}
