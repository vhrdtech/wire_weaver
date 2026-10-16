//! `RefVecBuilder` (SW-61): a `RefVec` written in place, element by element, gives the same bytes as the same
//! `RefVec::Slice` serialized normally, wherever it is written; a push that fails leaves the list as it was.

use hex_literal::hex;
use shrink_wrap::prelude::*;
use shrink_wrap::{BufReader, BufWriterOwned};

/// Evolvable (`Unsized`): its own size and the length of `name` go to the back, after its data.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
struct Item<'i> {
    id: u8,
    name: &'i str,
}

#[derive_shrink_wrap(borrowed, owned, sized, derive(Debug, PartialEq, Clone, Copy))]
struct Point {
    x: U4,
    y: U4,
    on: bool,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone))]
struct Frame<'i> {
    seq: u8,
    flag: bool,
    samples: RefVec<'i, u16>,
    names: RefVec<'i, &'i str>,
    tail: U7,
}

/// An element that fails to serialize after writing part of itself.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
struct Bounded<'i> {
    id: u8,
    name: MaxLen<&'i str, 2>,
}

fn out_of_bounds(e: &ShrinkWrapError) -> bool {
    format!("{e:?}").starts_with("OutOfBounds")
}

/// `items` through a builder, with `push` and with `extend`, gives the same bytes as `RefVec::Slice`: as the whole
/// buffer and between bit-sized fields.
fn same_bytes<'a, T>(items: &'a [T])
where
    T: SerializeShrinkWrap + DeserializeShrinkWrap<'a> + Clone,
{
    let slice = RefVec::Slice { slice: items };

    let mut buf = [0u8; 256];
    let mut wr = BufWriter::new(&mut buf);
    wr.write(&slice).unwrap();
    let expected = wr.finish().unwrap().to_vec();

    let mut buf = [0u8; 256];
    let mut wr = BufWriter::new(&mut buf);
    let mut b = RefVecBuilder::<T>::new(&mut wr).unwrap();
    for item in items {
        b.push(&mut wr, item).unwrap();
    }
    assert_eq!(b.len(), items.len());
    b.finish(&mut wr).unwrap();
    assert_eq!(wr.finish().unwrap(), expected, "root, push");

    let mut buf = [0u8; 256];
    let mut wr = BufWriter::new(&mut buf);
    let mut b = RefVecBuilder::<T>::new(&mut wr).unwrap();
    b.extend(&mut wr, items).unwrap();
    b.finish(&mut wr).unwrap();
    assert_eq!(wr.finish().unwrap(), expected, "root, extend");

    let mut buf = [0u8; 256];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_bool(true).unwrap();
    wr.write(&slice).unwrap();
    wr.write_bool(true).unwrap();
    wr.write(&U3::new(5).unwrap()).unwrap();
    let expected = wr.finish().unwrap().to_vec();

    let mut buf = [0u8; 256];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_bool(true).unwrap();
    let mut b = RefVecBuilder::<T>::new(&mut wr).unwrap();
    b.extend(&mut wr, items).unwrap();
    b.finish(&mut wr).unwrap();
    wr.write_bool(true).unwrap();
    wr.write(&U3::new(5).unwrap()).unwrap();
    assert_eq!(wr.finish().unwrap(), expected, "between bits");
}

/// [same_bytes] for [RefVecBuilderOwned], also against the borrowed builder.
fn same_bytes_owned<'a, T>(items: &'a [T])
where
    T: SerializeShrinkWrap + SerializeShrinkWrapOwned + DeserializeShrinkWrap<'a> + Clone,
{
    let mut buf = [0u8; 256];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_bool(true).unwrap();
    wr.write(&RefVec::Slice { slice: items }).unwrap();
    wr.write_bool(false).unwrap();
    let expected = wr.finish().unwrap().to_vec();

    let mut wr = BufWriterOwned::new();
    wr.write_bool(true).unwrap();
    wr.write(&RefVec::Slice { slice: items }).unwrap();
    wr.write_bool(false).unwrap();
    assert_eq!(wr.finish().unwrap(), expected, "owned writer, slice");

    let mut wr = BufWriterOwned::new();
    wr.write_bool(true).unwrap();
    let mut b = RefVecBuilderOwned::<T>::new(&mut wr).unwrap();
    for item in items {
        b.push(&mut wr, item).unwrap();
    }
    assert_eq!(b.len(), items.len());
    b.finish(&mut wr).unwrap();
    wr.write_bool(false).unwrap();
    assert_eq!(wr.finish().unwrap(), expected, "owned builder");
}

#[test]
fn bytes() {
    same_bytes::<u8>(&[]);
    same_bytes(&[1u8]);
    same_bytes(&[1u8, 2, 3, 0xff]);
    same_bytes_owned(&[1u8, 2, 3, 0xff]);
    same_bytes_owned::<u8>(&[]);

    let mut buf = [0u8; 16];
    let mut wr = BufWriter::new(&mut buf);
    let mut b = RefVecBuilder::new(&mut wr).unwrap();
    b.extend(&mut wr, [0xAAu8, 0xBB]).unwrap();
    b.finish(&mut wr).unwrap();
    assert_eq!(wr.finish().unwrap(), hex!("AA BB 02"));
}

#[test]
fn bit_sized() {
    let u7: Vec<U7> = [1u8, 0x7f, 0x40, 0].map(|v| U7::new(v).unwrap()).to_vec();
    same_bytes(&u7);
    same_bytes_owned(&u7);
    same_bytes(&[true, false, true]);
    same_bytes_owned(&[true, false, true]);
    let points = [
        Point {
            x: U4::new(1).unwrap(),
            y: U4::new(2).unwrap(),
            on: true,
        },
        Point {
            x: U4::new(15).unwrap(),
            y: U4::new(0).unwrap(),
            on: false,
        },
    ];
    same_bytes(&points);
    same_bytes_owned(&points);

    let mut buf = [0u8; 16];
    let mut wr = BufWriter::new(&mut buf);
    let mut b = RefVecBuilder::<U7>::new(&mut wr).unwrap();
    b.extend(&mut wr, &u7[..2]).unwrap();
    b.finish(&mut wr).unwrap();
    // 0000001 1111111 and padding, then the count
    assert_eq!(wr.finish().unwrap(), hex!("03 FC 02"));
}

#[test]
fn strings() {
    same_bytes::<&str>(&[]);
    same_bytes(&["a", "", "bcd"]);
    same_bytes_owned(&["a", "", "bcd"]);
}

#[test]
fn evolvable_elements() {
    let items = [
        Item { id: 1, name: "ab" },
        Item { id: 2, name: "" },
        Item { id: 3, name: "xyz" },
    ];
    same_bytes(&items);

    // read back
    let mut buf = [0u8; 64];
    let mut wr = BufWriter::new(&mut buf);
    let mut b = RefVecBuilder::<Item>::new(&mut wr).unwrap();
    b.extend(&mut wr, &items).unwrap();
    b.finish(&mut wr).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    let list = RefVec::<Item>::from_ww_bytes(bytes).unwrap();
    assert_eq!(list.iter().collect::<Result<Vec<_>, _>>().unwrap(), items);

    let owned = [
        ItemOwned {
            id: 1,
            name: "ab".into(),
        },
        ItemOwned {
            id: 3,
            name: "xyz".into(),
        },
    ];
    let mut wr = BufWriterOwned::new();
    let mut b = RefVecBuilderOwned::<ItemOwned>::new(&mut wr).unwrap();
    b.extend(&mut wr, &owned).unwrap();
    b.finish(&mut wr).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    let mut buf = [0u8; 64];
    let expected = RefVec::Slice {
        slice: &[items[0].clone(), items[2].clone()],
    }
    .to_ww_bytes(&mut buf)
    .unwrap();
    assert_eq!(bytes, expected);
}

#[test]
fn nested_lists() {
    let a = [1u8, 2];
    let b = [3u8];
    let lists = [
        RefVec::new_bytes(&a),
        RefVec::new_bytes(&[]),
        RefVec::new_bytes(&b),
    ];
    same_bytes(&lists);
    let s1 = ["a", "bc"];
    let s2 = ["def"];
    let strs = [RefVec::new_str_slice(&s1), RefVec::new_str_slice(&s2)];
    same_bytes(&strs);

    // the inner lists written with nested builders
    let mut buf = [0u8; 64];
    let mut wr = BufWriter::new(&mut buf);
    wr.write(&RefVec::Slice { slice: &strs }).unwrap();
    let expected = wr.finish().unwrap().to_vec();

    let mut buf = [0u8; 64];
    let mut wr = BufWriter::new(&mut buf);
    let mut outer = RefVecBuilder::<RefVec<&str>>::new(&mut wr).unwrap();
    for inner_items in [&s1[..], &s2[..]] {
        outer
            .push_with(&mut wr, |wr| {
                let mut inner = RefVecBuilder::<&str>::new(wr)?;
                inner.extend(wr, inner_items)?;
                inner.finish(wr)
            })
            .unwrap();
    }
    outer.finish(&mut wr).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    assert_eq!(bytes, expected);

    let read = RefVec::<RefVec<&str>>::from_ww_bytes(bytes).unwrap();
    let read: Vec<Vec<&str>> = read
        .iter()
        .map(|l| l.unwrap().iter().collect::<Result<Vec<_>, _>>().unwrap())
        .collect();
    assert_eq!(read, [vec!["a", "bc"], vec!["def"]]);
}

/// Fields of a struct written by hand, its lists through builders, as the derived code writes the struct.
fn write_frame_by_hand(
    wr: &mut BufWriter<'_>,
    seq: u8,
    samples: &[u16],
    names: &[&str],
) -> Result<(), ShrinkWrapError> {
    wr.write_u8(seq)?;
    wr.write_bool(true)?;
    let mut s = RefVecBuilder::<u16>::new(wr)?;
    for v in samples {
        s.push(wr, v)?;
    }
    s.finish(wr)?;
    let mut n = RefVecBuilder::<&str>::new(wr)?;
    n.extend(wr, names)?;
    n.finish(wr)?;
    wr.write(&U7::new(0x55).unwrap())
}

#[test]
fn inside_a_struct_written_by_hand() {
    let names = ["x", "yz"];
    let frame = Frame {
        seq: 7,
        flag: true,
        samples: RefVec::Slice {
            slice: &[0x1234, 0x5678],
        },
        names: RefVec::new_str_slice(&names),
        tail: U7::new(0x55).unwrap(),
    };
    let mut buf = [0u8; 64];
    let expected = frame.to_ww_bytes(&mut buf).unwrap().to_vec();

    let mut buf = [0u8; 64];
    let mut wr = BufWriter::new(&mut buf);
    write_frame_by_hand(&mut wr, 7, &[0x1234, 0x5678], &names).unwrap();
    assert_eq!(wr.finish().unwrap(), expected);

    // a list of such structs, each written by hand with nested builders inside
    let frames = [
        frame.clone(),
        Frame {
            seq: 8,
            samples: RefVec::Slice { slice: &[] },
            ..frame.clone()
        },
    ];
    let mut buf = [0u8; 128];
    let expected = RefVec::Slice { slice: &frames }
        .to_ww_bytes(&mut buf)
        .unwrap()
        .to_vec();

    let mut buf = [0u8; 128];
    let mut wr = BufWriter::new(&mut buf);
    let mut list = RefVecBuilder::<Frame>::new(&mut wr).unwrap();
    list.push_with(&mut wr, |wr| {
        write_frame_by_hand(wr, 7, &[0x1234, 0x5678], &names)
    })
    .unwrap();
    list.push_with(&mut wr, |wr| write_frame_by_hand(wr, 8, &[], &names))
        .unwrap();
    list.finish(&mut wr).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    assert_eq!(bytes, expected);
    let read = RefVec::<Frame>::from_ww_bytes(bytes).unwrap();
    assert_eq!(read.iter().collect::<Result<Vec<_>, _>>().unwrap(), frames);
}

/// For every buffer size: push until the first error, finish, and the result is what writing the elements that
/// got in as a slice gives in a buffer of the same size.
#[test]
fn out_of_buffer_keeps_the_pushed_elements() {
    let items = [
        Item { id: 1, name: "ab" },
        Item { id: 2, name: "" },
        Item { id: 3, name: "xyz" },
    ];
    let mut failed = 0;
    for size in 1..40 {
        let mut buf = vec![0xAAu8; size];
        let mut wr = BufWriter::new(&mut buf);
        let Ok(mut b) = RefVecBuilder::<Item>::new(&mut wr) else {
            continue;
        };
        let mut err = None;
        for item in &items {
            if let Err(e) = b.push(&mut wr, item) {
                err = Some(e);
                break;
            }
        }
        let pushed = b.len();
        if let Some(e) = err {
            failed += 1;
            assert!(out_of_bounds(&e), "{e:?}");
            assert!(pushed < items.len());
        }
        b.finish(&mut wr).unwrap();
        let got = wr.finish().map(|b| b.to_vec());

        let mut buf = vec![0x55u8; size];
        let mut wr = BufWriter::new(&mut buf);
        let expected = wr
            .write(&RefVec::Slice {
                slice: &items[..pushed],
            })
            .and_then(|_| wr.finish().map(|b| b.to_vec()));
        assert_eq!(got, expected, "buffer of {size} bytes, {pushed} pushed");
    }
    assert!(failed > 5);
}

#[test]
fn failed_element_is_rolled_back() {
    let good = [
        Bounded {
            id: 1,
            name: MaxLen::new_unchecked("ab"),
        },
        Bounded {
            id: 3,
            name: MaxLen::new_unchecked("c"),
        },
    ];
    let bad = Bounded {
        id: 2,
        name: MaxLen::new_unchecked("long"),
    };
    let mut buf = [0u8; 64];
    let expected = RefVec::Slice { slice: &good }
        .to_ww_bytes(&mut buf)
        .unwrap()
        .to_vec();

    let mut buf = [0u8; 64];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_bool(true).unwrap(); // a partial byte the rollback must restore
    let mut b = RefVecBuilder::<Bounded>::new(&mut wr).unwrap();
    b.push(&mut wr, &good[0]).unwrap();
    assert_eq!(b.push(&mut wr, &bad), Err(ShrinkWrapError::MaxLenExceeded));
    b.push(&mut wr, &good[1]).unwrap();
    b.finish(&mut wr).unwrap();
    let got = wr.finish().unwrap().to_vec();
    let mut buf = [0u8; 64];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_bool(true).unwrap();
    wr.write(&RefVec::Slice { slice: &good }).unwrap();
    assert_eq!(got, wr.finish().unwrap());

    let good_owned = good.clone().map(|g| BoundedOwned {
        id: g.id,
        name: MaxLen::new_unchecked(g.name.to_string()),
    });
    let bad_owned = BoundedOwned {
        id: 2,
        name: MaxLen::new_unchecked("long".into()),
    };
    let mut wr = BufWriterOwned::new();
    let mut b = RefVecBuilderOwned::<BoundedOwned>::new(&mut wr).unwrap();
    b.push(&mut wr, &good_owned[0]).unwrap();
    assert_eq!(
        b.push(&mut wr, &bad_owned),
        Err(ShrinkWrapError::MaxLenExceeded)
    );
    b.push(&mut wr, &good_owned[1]).unwrap();
    b.finish(&mut wr).unwrap();
    assert_eq!(wr.finish().unwrap(), expected);

    // a bit-packed element whose closure fails after some bits, in the owned writer
    let mut wr = BufWriterOwned::new();
    wr.write_bool(true).unwrap();
    let mut b = RefVecBuilderOwned::<bool>::new(&mut wr).unwrap();
    b.push(&mut wr, &true).unwrap();
    let r = b.push_with(&mut wr, |wr| {
        wr.write_bool(true)?;
        Err(ShrinkWrapError::MaxLenExceeded)
    });
    assert_eq!(r, Err(ShrinkWrapError::MaxLenExceeded));
    b.push(&mut wr, &false).unwrap();
    b.finish(&mut wr).unwrap();
    let got = wr.finish().unwrap().to_vec();
    let mut wr = BufWriterOwned::new();
    wr.write_bool(true).unwrap();
    wr.write(&RefVec::Slice {
        slice: &[true, false],
    })
    .unwrap();
    // 1, the elements 1 0, padding to a nibble, the count 2
    assert_eq!(wr.finish().unwrap(), hex!("C2"));
    assert_eq!(got, hex!("C2"));
}

#[test]
fn latched_error_in_push_with() {
    let mut buf = [0u8; 4];
    let mut wr = BufWriter::new(&mut buf);
    let mut b = RefVecBuilder::<u32>::new(&mut wr).unwrap();
    // 2 bytes left for data
    let e = b
        .push_with(&mut wr, |wr| {
            wr.write_u32_latch(1);
            Ok(())
        })
        .unwrap_err();
    assert!(out_of_bounds(&e), "{e:?}");
    assert_eq!(b.len(), 0);
    b.finish(&mut wr).unwrap();
    assert_eq!(wr.finish().unwrap(), hex!("00"));
}

#[test]
fn max_len() {
    type Bounded2<'i> = MaxLen<RefVec<'i, u16>, 2>;
    let mut buf = [0u8; 16];
    let mut wr = BufWriter::new(&mut buf);
    let mut b = RefVecBuilder::<u16>::with_max_len(&mut wr, Bounded2::MAX_LEN).unwrap();
    b.push(&mut wr, &1).unwrap();
    b.push(&mut wr, &2).unwrap();
    assert_eq!(b.push(&mut wr, &3), Err(ShrinkWrapError::MaxLenExceeded));
    b.finish(&mut wr).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    let read = Bounded2::from_ww_bytes(bytes).unwrap();
    assert_eq!(read.iter().collect::<Result<Vec<_>, _>>().unwrap(), [1, 2]);
    let mut buf = [0u8; 16];
    assert_eq!(
        MaxLen::<RefVec<u16>, 2>::new(RefVec::Slice { slice: &[1, 2] })
            .unwrap()
            .to_ww_bytes(&mut buf)
            .unwrap(),
        bytes
    );

    let mut wr = BufWriterOwned::new();
    let mut b = RefVecBuilderOwned::<u16>::with_max_len(&mut wr, 2).unwrap();
    b.extend(&mut wr, [1u16, 2]).unwrap();
    assert_eq!(b.push(&mut wr, &3), Err(ShrinkWrapError::MaxLenExceeded));
    b.finish(&mut wr).unwrap();
    assert_eq!(wr.finish().unwrap(), bytes);
}

#[test]
fn count_limit_of_buf_writer() {
    let mut buf = [0u8; 8];
    let mut wr = BufWriter::new(&mut buf);
    let mut b = RefVecBuilder::<()>::new(&mut wr).unwrap();
    for _ in 0..u16::MAX {
        b.push(&mut wr, &()).unwrap();
    }
    assert_eq!(b.push(&mut wr, &()), Err(ShrinkWrapError::LenTooLong));
    b.finish(&mut wr).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    assert_eq!(
        RefVec::<()>::from_ww_bytes(bytes).unwrap().len(),
        u16::MAX as usize
    );
}

#[test]
fn u8_builder_round_trip() {
    // plain bytes, the same as RefVec::new_bytes, also after bits and when empty
    for data in [&[][..], &[1u8][..], &[1, 2, 3, 4, 5][..]] {
        for lead_bits in 0..3 {
            let mut buf = [0u8; 32];
            let mut wr = BufWriter::new(&mut buf);
            for _ in 0..lead_bits {
                wr.write_bool(true).unwrap();
            }
            wr.write(&RefVec::new_bytes(data)).unwrap();
            wr.write_bool(true).unwrap();
            let expected = wr.finish().unwrap().to_vec();

            let mut buf = [0u8; 32];
            let mut wr = BufWriter::new(&mut buf);
            for _ in 0..lead_bits {
                wr.write_bool(true).unwrap();
            }
            let mut b = RefVecBuilder::<u8>::new(&mut wr).unwrap();
            b.write_bytes_with(&mut wr, |wr| wr.write_raw_slice(data))
                .unwrap();
            assert_eq!(b.len(), data.len());
            b.finish(&mut wr).unwrap();
            wr.write_bool(true).unwrap();
            let bytes = wr.finish_and_take().unwrap().to_vec();
            assert_eq!(
                bytes, expected,
                "{data:?} after {lead_bits} bits, write_bytes_with"
            );

            let mut buf = [0u8; 32];
            let mut wr = BufWriter::new(&mut buf);
            for _ in 0..lead_bits {
                wr.write_bool(true).unwrap();
            }
            let mut b = RefVecBuilder::<u8>::new(&mut wr).unwrap();
            b.extend_from_slice(&mut wr, data).unwrap();
            b.finish(&mut wr).unwrap();
            wr.write_bool(true).unwrap();
            assert_eq!(
                wr.finish().unwrap(),
                expected,
                "{data:?} after {lead_bits} bits, extend_from_slice"
            );

            let mut wr = BufWriterOwned::new();
            for _ in 0..lead_bits {
                wr.write_bool(true).unwrap();
            }
            let mut b = RefVecBuilderOwned::<u8>::new(&mut wr).unwrap();
            b.write_bytes_with(&mut wr, |wr| wr.write_raw_slice(data))
                .unwrap();
            b.extend_from_slice(&mut wr, &[]).unwrap();
            b.finish(&mut wr).unwrap();
            wr.write_bool(true).unwrap();
            assert_eq!(
                wr.finish().unwrap(),
                expected,
                "{data:?} after {lead_bits} bits, owned"
            );
            let bytes = &bytes[..];

            let mut rd = BufReader::new(bytes);
            for _ in 0..lead_bits {
                assert_eq!(rd.read_bool(), Ok(true));
            }
            let read: RefVec<u8> = rd.read().unwrap();
            assert_eq!(read.as_slice(), data);
            assert_eq!(rd.read_bool(), Ok(true));
        }
    }

    // a blob holding other values: their lengths are encoded inside it
    let mut buf = [0u8; 32];
    let mut wr = BufWriter::new(&mut buf);
    let mut b = RefVecBuilder::<u8>::new(&mut wr).unwrap();
    b.write_bytes_with(&mut wr, |wr| {
        wr.write(&"hi")?;
        wr.write_u8(0x42)
    })
    .unwrap();
    b.finish(&mut wr).unwrap();
    wr.write_u8(0x99).unwrap();
    let bytes = wr.finish_and_take().unwrap();
    // "hi", 0x42, the length of "hi" (after a padding nibble), 0x99, the blob size
    assert_eq!(bytes, hex!("68 69 42 02 99 04"));

    let mut wr = BufWriterOwned::new();
    let mut b = RefVecBuilderOwned::<u8>::new(&mut wr).unwrap();
    b.write_bytes_with(&mut wr, |wr| {
        wr.write(&"hi")?;
        wr.write_u8(0x42)
    })
    .unwrap();
    b.finish(&mut wr).unwrap();
    wr.write_u8(0x99).unwrap();
    assert_eq!(wr.finish().unwrap(), bytes, "owned");
    let mut rd = BufReader::new(bytes);
    let blob: RefVec<u8> = rd.read().unwrap();
    assert_eq!(rd.read_u8(), Ok(0x99));
    let mut inner = BufReader::new(blob.as_slice());
    assert_eq!(inner.read::<&str>(), Ok("hi"));
    assert_eq!(inner.read_u8(), Ok(0x42));
}

/// The examples of `docs/serdes/special_types.md#refvecbuilder-lists-written-in-place`.
#[test]
fn docs_examples() {
    let adc_read = |ch: u16| U12::new(0x100 * ch + 0x23).unwrap();
    let mut buf = [0u8; 32];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_u8(7).unwrap();
    let mut samples = RefVecBuilder::<U12>::new(&mut wr).unwrap();
    for ch in 0..3 {
        samples.push(&mut wr, &adc_read(ch)).unwrap(); // 0x023, 0x123, 0x223
    }
    samples.finish(&mut wr).unwrap();
    wr.write_bool(true).unwrap();
    assert_eq!(wr.finish_and_take().unwrap(), hex!("07 02 31 23 22 38 03"));

    let mut buf = [0u8; 32];
    let mut wr = BufWriter::new(&mut buf);
    let groups: [&[&str]; 2] = [&["a", "bc"], &["d"]];
    let mut outer = RefVecBuilder::<RefVec<&str>>::new(&mut wr).unwrap();
    for group in groups {
        outer
            .push_with(&mut wr, |wr| {
                let mut inner = RefVecBuilder::<&str>::new(wr)?;
                inner.extend(wr, group)?;
                inner.finish(wr)
            })
            .unwrap();
    }
    outer.finish(&mut wr).unwrap();
    assert_eq!(wr.finish_and_take().unwrap(), hex!("61 62 63 64 11 21 22"));
}

/// `push`, `extend_from_slice` and `write_bytes_with` on one `u8` list: the count is the number of bytes, the
/// bytes those of the whole list as one slice.
#[test]
fn u8_builder_mixed() {
    let all = [1u8, 2, 3, 4, 0x68, 0x69, 0x02, 5];
    let mut buf = [0u8; 32];
    let expected = RefVec::new_bytes(&all)
        .to_ww_bytes(&mut buf)
        .unwrap()
        .to_vec();

    let mut buf = [0u8; 32];
    let mut wr = BufWriter::new(&mut buf);
    let mut b = RefVecBuilder::<u8>::new(&mut wr).unwrap();
    b.push(&mut wr, &1).unwrap();
    b.extend_from_slice(&mut wr, &[2, 3]).unwrap();
    b.write_bytes_with(&mut wr, |wr| wr.write_u8(4)).unwrap();
    b.write_bytes_with(&mut wr, |_| Ok(())).unwrap();
    // a string with its length: 68 69 and the length 02 after a padding nibble
    b.write_bytes_with(&mut wr, |wr| wr.write(&"hi")).unwrap();
    b.push(&mut wr, &5).unwrap();
    assert_eq!(b.len(), all.len());
    b.finish(&mut wr).unwrap();
    assert_eq!(wr.finish().unwrap(), expected);

    let mut wr = BufWriterOwned::new();
    let mut b = RefVecBuilderOwned::<u8>::new(&mut wr).unwrap();
    b.push(&mut wr, &1).unwrap();
    b.extend_from_slice(&mut wr, &[2, 3]).unwrap();
    b.write_bytes_with(&mut wr, |wr| wr.write_u8(4)).unwrap();
    b.write_bytes_with(&mut wr, |wr| wr.write(&"hi")).unwrap();
    b.push(&mut wr, &5).unwrap();
    assert_eq!(b.len(), all.len());
    b.finish(&mut wr).unwrap();
    assert_eq!(wr.finish().unwrap(), expected, "owned");
}

/// A failed `write_bytes_with` or `extend_from_slice` leaves the list as it was, also past bits and lengths.
#[test]
fn u8_builder_errors() {
    let mut buf = [0u8; 32];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_bool(true).unwrap();
    wr.write(&RefVec::new_bytes(&[1, 2])).unwrap();
    wr.write_bool(true).unwrap();
    let expected = wr.finish().unwrap().to_vec();

    let mut buf = [0u8; 32];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_bool(true).unwrap();
    let mut b = RefVecBuilder::<u8>::with_max_len(&mut wr, 4).unwrap();
    b.push(&mut wr, &1).unwrap();
    let r = b.write_bytes_with(&mut wr, |wr| {
        wr.write(&"x")?;
        Err(ShrinkWrapError::MalformedUtf8)
    });
    assert_eq!(r, Err(ShrinkWrapError::MalformedUtf8));
    // 3 bytes plus a length would be 5 > 4
    assert_eq!(
        b.write_bytes_with(&mut wr, |wr| wr.write(&"abc")),
        Err(ShrinkWrapError::MaxLenExceeded)
    );
    assert_eq!(
        b.extend_from_slice(&mut wr, &[0; 4]),
        Err(ShrinkWrapError::MaxLenExceeded)
    );
    b.extend_from_slice(&mut wr, &[2]).unwrap();
    assert_eq!(b.len(), 2);
    b.finish(&mut wr).unwrap();
    wr.write_bool(true).unwrap();
    assert_eq!(wr.finish().unwrap(), expected);

    // out of buffer inside the blob, at every size: what got in equals the same bytes written as a slice
    for size in 1..12 {
        let mut buf = vec![0xAAu8; size];
        let mut wr = BufWriter::new(&mut buf);
        let Ok(mut b) = RefVecBuilder::<u8>::new(&mut wr) else {
            continue;
        };
        let mut pushed: Vec<u8> = Vec::new();
        if b.write_bytes_with(&mut wr, |wr| wr.write_raw_slice(&[1, 2, 3]))
            .is_ok()
        {
            pushed.extend([1, 2, 3]);
        }
        if b.write_bytes_with(&mut wr, |wr| wr.write(&"hi")).is_ok() {
            pushed.extend([0x68, 0x69, 0x02]);
        }
        assert_eq!(b.len(), pushed.len());
        b.finish(&mut wr).unwrap();
        let got = wr.finish().map(|b| b.to_vec());
        let mut buf = vec![0x55u8; size];
        let mut wr = BufWriter::new(&mut buf);
        let expected = wr
            .write(&RefVec::new_bytes(&pushed))
            .and_then(|_| wr.finish().map(|b| b.to_vec()));
        assert_eq!(got, expected, "buffer of {size} bytes");
    }
}
