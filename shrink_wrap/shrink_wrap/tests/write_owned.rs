//! `BufWriter::write_owned`, `ser_owned` and `OwnedSer` (API-21): an owned value written into a fixed buffer gives the same
//! bytes as its borrowed counterpart written with `write` / `ser_shrink_wrap`, at every bit position and inside
//! an enclosing `Unsized` value that encodes the lengths it pushes.

use shrink_wrap::buf_writer::UnsizedBuilder;
use shrink_wrap::prelude::*;
use shrink_wrap::un::U3;
use shrink_wrap::{BufReader, BufWriter, Error, OwnedSer};

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
struct Rec<'i> {
    flag: bool,
    small: U3,
    name: &'i str,
    data: RefVec<'i, u8>,
    words: RefVec<'i, &'i str>,
    maybe: Option<&'i str>,
    inner: Inner<'i>,
    kind: Kind<'i>,
}

#[derive_shrink_wrap(borrowed, owned, final_structure, derive(Debug, PartialEq, Clone))]
struct Inner<'i> {
    a: u16,
    b: RefVec<'i, u16>,
}

#[derive_shrink_wrap(borrowed, owned, ww_repr = u4, derive(Debug, PartialEq, Clone))]
enum Kind<'i> {
    Empty,
    Text(&'i str),
    Nested { list: RefVec<'i, Inner<'i>> },
}

#[derive_shrink_wrap(borrowed, owned, sparse, derive(Debug, PartialEq, Clone))]
struct Sparse<'i> {
    #[id = 0]
    a: Option<u8>,
    #[id = 3]
    b: Option<&'i str>,
}

fn rec<'i>(words: &'i [&'i str], inners: &'i [Inner<'i>]) -> Rec<'i> {
    Rec {
        flag: true,
        small: U3::new(5).unwrap(),
        name: "name",
        data: RefVec::Slice { slice: &[1, 2, 3] },
        words: RefVec::Slice { slice: words },
        maybe: Some("x"),
        inner: Inner {
            a: 0xabcd,
            b: RefVec::Slice { slice: &[1, 2] },
        },
        kind: Kind::Nested {
            list: RefVec::Slice { slice: inners },
        },
    }
}

fn rec_owned() -> RecOwned {
    RecOwned {
        flag: true,
        small: U3::new(5).unwrap(),
        name: "name".into(),
        data: vec![1, 2, 3],
        words: vec!["ab".into(), "".into(), "cde".into()],
        maybe: Some("x".into()),
        inner: InnerOwned {
            a: 0xabcd,
            b: vec![1, 2],
        },
        kind: KindOwned::Nested {
            list: vec![
                InnerOwned { a: 1, b: vec![] },
                InnerOwned {
                    a: 2,
                    b: vec![3, 4, 5],
                },
            ],
        },
    }
}

/// `prefix` bits, then the value written by `f`, then a byte, all in an `Unsized` value (which encodes the
/// lengths the value pushed) when `nested`.
fn bytes_of(
    buf: &mut [u8],
    prefix: u8,
    nested: bool,
    f: impl FnOnce(&mut BufWriter) -> Result<(), Error>,
) -> Vec<u8> {
    let mut wr = BufWriter::new(buf);
    for i in 0..prefix {
        wr.write_bool(i % 2 == 0).unwrap();
    }
    let builder = nested.then(|| UnsizedBuilder::new(&mut wr).unwrap());
    f(&mut wr).unwrap();
    wr.write_u8(0x5a).unwrap();
    if let Some(builder) = builder {
        builder.finish(&mut wr).unwrap();
    }
    wr.finish().unwrap().to_vec()
}

fn check<B: SerializeShrinkWrap, O: SerializeShrinkWrapOwned>(borrowed: &B, owned: &O) {
    for prefix in 0..9 {
        for nested in [false, true] {
            let (mut b, mut o) = ([0xffu8; 256], [0xffu8; 256]);
            assert_eq!(
                bytes_of(&mut o, prefix, nested, |wr| wr.write_owned(owned)),
                bytes_of(&mut b, prefix, nested, |wr| wr.write(borrowed)),
                "write, {prefix} bits before, nested: {nested}"
            );
            assert_eq!(
                bytes_of(&mut o, prefix, nested, |wr| wr.ser_owned(owned)),
                bytes_of(&mut b, prefix, nested, |wr| borrowed.ser_shrink_wrap(wr)),
                "ser, {prefix} bits before, nested: {nested}"
            );
            assert_eq!(
                bytes_of(&mut o, prefix, nested, |wr| wr.write(&OwnedSer(owned))),
                bytes_of(&mut b, prefix, nested, |wr| wr.write(borrowed)),
                "OwnedSer, {prefix} bits before, nested: {nested}"
            );
        }
    }
}

#[test]
fn same_bytes_as_borrowed() {
    let words = ["ab", "", "cde"];
    let inners = [
        Inner {
            a: 1,
            b: RefVec::Slice { slice: &[] },
        },
        Inner {
            a: 2,
            b: RefVec::Slice { slice: &[3, 4, 5] },
        },
    ];
    let borrowed = rec(&words, &inners);
    let owned = rec_owned();
    check(&borrowed, &owned);
    check(&borrowed.inner, &owned.inner);
    check(&borrowed.kind, &owned.kind);
    check(&Kind::Text("t"), &KindOwned::Text("t".into()));
    check(&Kind::Empty, &KindOwned::Empty);
    check(&"str", &String::from("str"));
    check(&borrowed.words, &owned.words);
    check(
        &Sparse {
            a: None,
            b: Some("b"),
        },
        &SparseOwned {
            a: None,
            b: Some("b".into()),
        },
    );
    check(&Some(borrowed.clone()), &Some(owned.clone()));
}

#[test]
fn reads_back() {
    let owned = rec_owned();
    let mut buf = [0u8; 256];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_bool(true).unwrap();
    wr.write_owned(&owned).unwrap();
    wr.write_owned(&owned.words).unwrap();
    let bytes = wr.finish().unwrap();
    let mut rd = BufReader::new(bytes);
    assert!(rd.read_bool().unwrap());
    assert_eq!(rd.read_owned::<RecOwned>().unwrap(), owned);
    assert_eq!(rd.read_owned::<Vec<String>>().unwrap(), owned.words);
}

#[test]
fn too_small_buffer_is_an_error() {
    let owned = rec_owned();
    for len in 0..40 {
        let mut buf = vec![0u8; len];
        let mut wr = BufWriter::new(&mut buf);
        match wr.write_owned(&owned).and_then(|_| wr.finish().map(|_| ())) {
            Ok(()) => assert!(len > 30, "{len} bytes are not enough"),
            Err(e) => assert!(
                matches!(e, Error::OutOfBoundsWriteRawSlice | Error::OutOfBoundsRev),
                "{len}: {e:?}"
            ),
        }
    }
}
