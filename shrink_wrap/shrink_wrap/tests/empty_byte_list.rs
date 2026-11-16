//! An empty byte list (`RefVec<u8>` / `Vec<u8>`) writes its length and nothing else, not even the alignment to a
//! byte its elements would take (SW-81). The borrowed derive once aligned (`RefVec::ser_shrink_wrap_vec_u8`),
//! so a field after an empty list that starts mid-byte was read one byte early. Here the borrowed and the owned
//! derive must give the same bytes for every value, and both must read back.

use proptest::prelude::*;
use shrink_wrap::prelude::*;

/// The smallest type whose bytes changed: the list at the end, after a bit.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
struct End<'i> {
    a: bool,
    b: RefVec<'i, u8>,
}

/// A bit after the empty list: the borrowed derive put it in a padding byte, read back as `false`.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
struct Mid<'i> {
    a: bool,
    b: RefVec<'i, u8>,
    c: bool,
}

fn empty() -> RefVec<'static, u8> {
    RefVec::Slice { slice: &[] }
}

#[test]
fn end_writes_no_alignment() {
    let v = End {
        a: true,
        b: empty(),
    };
    let mut buf = [0u8; 16];
    assert_eq!(v.to_ww_bytes(&mut buf).unwrap(), &[0x80]);
    let owned = EndOwned { a: true, b: vec![] };
    assert_eq!(owned.to_ww_bytes_owned().unwrap(), [0x80]);
    assert_eq!(End::from_ww_bytes(&[0x80]), Ok(v));
}

/// What the borrowed derive wrote before the fix still reads: the reader takes the length from the back.
#[test]
fn end_with_the_old_padding_byte_still_reads() {
    let v = End::from_ww_bytes(&[0x80, 0x00]).unwrap();
    assert!(v.a);
    assert!(v.b.is_empty());
    assert_eq!(
        EndOwned::from_ww_bytes_owned(&[0x80, 0x00]),
        Ok(EndOwned { a: true, b: vec![] })
    );
}

#[test]
fn mid_reads_back() {
    let v = Mid {
        a: true,
        b: empty(),
        c: true,
    };
    let mut buf = [0u8; 16];
    let bytes = v.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, &[0xc0]);
    assert_eq!(Mid::from_ww_bytes(bytes), Ok(v));
}

/// A nibble enum without payload, evolvable.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
enum Ends {
    Instants,
    Spans,
}

/// An evolvable struct of sub-byte fields.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Eq, Clone, Copy))]
struct Events {
    ends: Ends,
    flags: bool,
}

/// A nibble enum with an `Unsized` payload.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone), ww_repr = u4)]
enum Kind<'i> {
    Events(Events),
    Bytes(RefVec<'i, u8>),
}

/// Nibbles and bits flattened into the parent, as a version number is.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone), final_structure)]
struct Ver<'i> {
    major: UNib32,
    minor: UNib32,
    pre: Option<&'i str>,
}

/// Byte lists between bits, nibbles, options and nested values: every way a list can start mid-byte.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
struct Shape<'i> {
    a: bool,
    n: U3,
    head: RefVec<'i, u8>,
    opt: Option<u8>,
    ver: Ver<'i>,
    bundle: RefVec<'i, u8>,
    path: RefVec<'i, UNib32>,
    item: Option<Events>,
    label: &'i str,
    sticky: bool,
    kind: Kind<'i>,
    tail: RefVec<'i, u8>,
}

/// The shape inside a nibble enum, as a record kind is.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone), ww_repr = u4)]
enum Record<'i> {
    Other(u8),
    Shape(Shape<'i>),
}

fn bytes_list() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![Just(vec![]), proptest::collection::vec(any::<u8>(), 0..4)]
}

fn kind() -> impl Strategy<Value = KindOwned> {
    prop_oneof![
        (any::<bool>(), any::<bool>()).prop_map(|(s, flags)| KindOwned::Events(Events {
            ends: if s { Ends::Spans } else { Ends::Instants },
            flags,
        })),
        bytes_list().prop_map(KindOwned::Bytes),
    ]
}

prop_compose! {
    fn shape()(
        a in any::<bool>(),
        n in 0u8..8,
        head in bytes_list(),
        opt in any::<Option<u8>>(),
        major in 0u32..40,
        minor in 0u32..40,
        pre in proptest::option::of("[a-z]{0,3}"),
        bundle in bytes_list(),
        path in proptest::collection::vec(0u32..40, 0..3),
        item in any::<Option<(bool, bool)>>(),
        label in "[a-z]{0,3}",
        sticky in any::<bool>(),
        kind in kind(),
        tail in bytes_list(),
    ) -> ShapeOwned {
        ShapeOwned {
            a,
            n: U3::new(n).unwrap(),
            head,
            opt,
            ver: VerOwned { major: UNib32(major), minor: UNib32(minor), pre },
            bundle,
            path: path.into_iter().map(UNib32).collect(),
            item: item.map(|(s, flags)| Events {
                ends: if s { Ends::Spans } else { Ends::Instants },
                flags,
            }),
            label,
            sticky,
            kind,
            tail,
        }
    }
}

proptest! {
    /// The borrowed derive writes the owned derive's bytes, and both read back.
    #[test]
    fn borrowed_and_owned_write_the_same(v in shape()) {
        let owned = v.to_ww_bytes_owned().unwrap();
        prop_assert_eq!(&ShapeOwned::from_ww_bytes_owned(&owned).unwrap(), &v);
        let borrowed = Shape::from_ww_bytes(&owned).unwrap();
        let mut buf = [0u8; 256];
        prop_assert_eq!(borrowed.to_ww_bytes(&mut buf).unwrap(), &owned[..]);

        let rec = RecordOwned::Shape(v);
        let owned = rec.to_ww_bytes_owned().unwrap();
        prop_assert_eq!(&RecordOwned::from_ww_bytes_owned(&owned).unwrap(), &rec);
        let borrowed = Record::from_ww_bytes(&owned).unwrap();
        prop_assert_eq!(borrowed.to_ww_bytes(&mut buf).unwrap(), &owned[..]);
    }
}
