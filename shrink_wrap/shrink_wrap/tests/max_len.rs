//! `MaxLen<T, N>` (SW-19): lists and strings with a maximum length, checked when serializing and deserializing,
//! with the same bytes on the wire as without the bound.

use shrink_wrap::prelude::*;

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
struct Piece<'i> {
    offset: u32,
    data: MaxLen<RefVec<'i, u8>, 4>,
    name: MaxLen<&'i str, 3>,
    tags: MaxLen<RefVec<'i, &'i str>, 2>,
}

/// The same fields, no bounds: the wire form is the same.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
struct PieceUnbounded<'i> {
    offset: u32,
    data: RefVec<'i, u8>,
    name: &'i str,
    tags: RefVec<'i, &'i str>,
}

fn piece<'i>(data: &'i [u8], name: &'i str, tags: &'i [&'i str]) -> Piece<'i> {
    Piece {
        offset: 0x1234,
        data: MaxLen::new_unchecked(RefVec::new_bytes(data)),
        name: MaxLen::new_unchecked(name),
        tags: MaxLen::new_unchecked(RefVec::new_str_slice(tags)),
    }
}

#[test]
fn at_the_bound_round_trips_with_the_unbounded_bytes() {
    let tags = ["a", "bc"];
    let value = piece(&[1, 2, 3, 4], "xyz", &tags);
    let mut buf = [0u8; 64];
    let bytes = value.to_ww_bytes(&mut buf).unwrap();

    let unbounded = PieceUnbounded {
        offset: 0x1234,
        data: RefVec::new_bytes(&[1, 2, 3, 4]),
        name: "xyz",
        tags: RefVec::new_str_slice(&tags),
    };
    let mut buf_unbounded = [0u8; 64];
    assert_eq!(bytes, unbounded.to_ww_bytes(&mut buf_unbounded).unwrap());

    let read = Piece::from_ww_bytes(bytes).unwrap();
    assert_eq!(read.offset, 0x1234);
    assert_eq!(read.data.as_slice(), &[1, 2, 3, 4]);
    assert_eq!(*read.name, "xyz");
    assert_eq!(read.tags.len(), 2);

    let owned = PieceOwned::from_ww_bytes_owned(bytes).unwrap();
    assert_eq!(*owned.data, vec![1, 2, 3, 4]);
    assert_eq!(owned.name.as_str(), "xyz");
    assert_eq!(*owned.tags, vec!["a".to_string(), "bc".to_string()]);
    assert_eq!(owned.to_ww_bytes_owned().unwrap(), bytes);
}

#[test]
fn over_the_bound_fails_to_serialize() {
    let mut buf = [0u8; 64];
    let tags = ["a"];
    for value in [
        piece(&[1, 2, 3, 4, 5], "xyz", &tags),
        piece(&[1], "wxyz", &tags),
        piece(&[1], "x", &["a", "b", "c"]),
    ] {
        assert_eq!(
            value.to_ww_bytes(&mut buf).err(),
            Some(ShrinkWrapError::MaxLenExceeded)
        );
    }

    let owned = PieceOwned {
        offset: 0,
        data: MaxLen::new_unchecked(vec![0; 5]),
        name: MaxLen::new_unchecked(String::new()),
        tags: MaxLen::new_unchecked(vec![]),
    };
    assert_eq!(
        owned.to_ww_bytes_owned().err(),
        Some(ShrinkWrapError::MaxLenExceeded)
    );
}

#[test]
fn over_the_bound_fails_to_deserialize() {
    let mut buf = [0u8; 64];
    for (data, name, tags) in [
        (&[1u8, 2, 3, 4, 5][..], "xyz", &["a"][..]),
        (&[1], "wxyz", &["a"]),
        (&[1], "x", &["a", "b", "c"]),
    ] {
        let unbounded = PieceUnbounded {
            offset: 0,
            data: RefVec::new_bytes(data),
            name,
            tags: RefVec::new_str_slice(tags),
        };
        let bytes = unbounded.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(
            Piece::from_ww_bytes(bytes).err(),
            Some(ShrinkWrapError::MaxLenExceeded)
        );
        assert_eq!(
            PieceOwned::from_ww_bytes_owned(bytes).err(),
            Some(ShrinkWrapError::MaxLenExceeded)
        );
    }
}

#[test]
fn new_checks_the_length() {
    assert!(MaxLen::<&str, 3>::new("abc").is_ok());
    assert_eq!(
        MaxLen::<&str, 3>::new("abcd").err(),
        Some(ShrinkWrapError::MaxLenExceeded)
    );
    assert_eq!(MaxLen::<Vec<u8>, 3>::MAX_LEN, 3);
    let mut v = MaxLen::<Vec<u8>, 3>::new(vec![1, 2]).unwrap();
    v.push(3);
    assert_eq!(v.into_inner(), vec![1, 2, 3]);
}

#[test]
fn element_size_is_the_inner_one() {
    assert_eq!(
        <MaxLen<RefVec<'_, u8>, 4> as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::UnsizedFinalStructure
    );
    assert_eq!(
        <MaxLen<&str, 4> as DeserializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::UnsizedFinalStructure
    );
    const { assert!(<MaxLen<&str, 4> as SerializeShrinkWrap>::PUSHES_LEN) };
}

/// A bound in a const, and a path to the type.
const MAX_NAME: usize = 2;

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
struct ConstBound<'i> {
    name: shrink_wrap::MaxLen<&'i str, MAX_NAME>,
    list: MaxLen<RefVec<'i, MaxLen<&'i str, 1>>, { MAX_NAME * 2 }>,
}

#[test]
fn const_expression_bounds() {
    let names = [MaxLen::new_unchecked("a"), MaxLen::new_unchecked("bc")];
    let value = ConstBound {
        name: MaxLen::new("ab").unwrap(),
        list: MaxLen::new(RefVec::Slice { slice: &names }).unwrap(),
    };
    let mut buf = [0u8; 32];
    // the inner bound of the second name fails
    assert_eq!(
        value.to_ww_bytes(&mut buf).err(),
        Some(ShrinkWrapError::MaxLenExceeded)
    );
    let names = [MaxLen::new_unchecked("a"), MaxLen::new_unchecked("c")];
    let value = ConstBound {
        name: MaxLen::new("ab").unwrap(),
        list: MaxLen::new(RefVec::Slice { slice: &names }).unwrap(),
    };
    let bytes = value.to_ww_bytes(&mut buf).unwrap();
    let owned = ConstBoundOwned::from_ww_bytes_owned(bytes).unwrap();
    assert_eq!(owned.list.len(), 2);
    assert_eq!(owned.list[1].as_str(), "c");
}
