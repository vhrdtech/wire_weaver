//! `TailSize<N>` (SW-18): a slot the derive fills in with the size of the rest of the value, and bounds the
//! value with when reading.

use hex_literal::hex;
use shrink_wrap::prelude::*;
use shrink_wrap::tail_bytes::TailBytes;

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone))]
struct Header<'i> {
    magic: u16,
    size: TailSize<2>,
    name: &'i str,
    count: u32,
}

/// Header after an evolution: a field appended, which readers of `Header` skip thanks to the slot.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct HeaderV2<'i> {
    magic: u16,
    size: TailSize<2>,
    name: &'i str,
    count: u32,
    #[default = None]
    extra: Option<&'i str>,
}

fn header() -> Header<'static> {
    Header {
        magic: 0xABCD,
        size: TailSize(0),
        name: "ab",
        count: 7,
    }
}

#[test]
fn bytes_and_round_trip() {
    let mut buf = [0u8; 32];
    let bytes = header().to_ww_bytes(&mut buf).unwrap();
    // magic, slot (7 bytes follow it), "ab", count, "ab"'s reversed length
    assert_eq!(bytes, hex!("CD AB 80 07 61 62 07 00 00 00 02"));
    let back = Header::from_ww_bytes(bytes).unwrap();
    assert_eq!(
        back,
        Header {
            size: TailSize(7),
            ..header()
        }
    );
}

#[test]
fn longer_buffer_is_bounded_by_the_slot() {
    let mut region = [0xFFu8; 64];
    let len = header().to_ww_bytes(&mut region).unwrap().len();
    let back = Header::from_ww_bytes(&region).unwrap();
    assert_eq!(back.size.0 as usize, len - 4);
    assert_eq!(back.name, "ab");
    assert_eq!(back.count, 7);
    // same through the owned side and a sub-slice that ends in the garbage
    let back = HeaderOwned::from_ww_bytes_owned(&region[..len + 3]).unwrap();
    assert_eq!(back.name, "ab");
}

#[test]
fn truncated_buffer_is_an_error() {
    let mut buf = [0u8; 32];
    let bytes = header().to_ww_bytes(&mut buf).unwrap().to_vec();
    assert!(matches!(
        Header::from_ww_bytes(&bytes[..bytes.len() - 1]),
        Err(ShrinkWrapError::OutOfBoundsSplit)
    ));
    // a zeroed region: the slot's bytes are not a 2-byte number
    assert_eq!(
        Header::from_ww_bytes(&[0u8; 16]),
        Err(ShrinkWrapError::MalformedUVlq32)
    );
}

#[test]
fn newer_fields_after_the_slot_are_skipped() {
    let mut region = [0xFFu8; 64];
    let v2 = HeaderV2 {
        magic: 0xABCD,
        size: TailSize(0),
        name: "ab",
        count: 7,
        extra: Some("more"),
    };
    v2.to_ww_bytes(&mut region).unwrap();
    // old reader, long buffer: stops at the value's end, ignores `extra`
    let old = Header::from_ww_bytes(&region).unwrap();
    assert_eq!(old.name, "ab");
    assert_eq!(old.count, 7);
    // new reader, old data: `extra` is missing and defaults
    let mut buf = [0u8; 32];
    header().to_ww_bytes(&mut buf).unwrap();
    let new = HeaderV2::from_ww_bytes(&buf).unwrap();
    assert_eq!(new.extra, None);
    assert_eq!(new.count, 7);
}

#[test]
fn owned_writer_matches() {
    let mut buf = [0u8; 32];
    let bytes = header().to_ww_bytes(&mut buf).unwrap();
    let owned = HeaderOwned {
        magic: 0xABCD,
        size: TailSize(0),
        name: "ab".into(),
        count: 7,
    };
    assert_eq!(owned.to_ww_bytes_owned().unwrap(), bytes);
    assert_eq!(
        HeaderOwned::from_ww_bytes_owned(bytes).unwrap(),
        HeaderOwned {
            size: TailSize(7),
            ..owned
        }
    );
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct Inner<'i> {
    a: u8,
    size: TailSize<1>,
    s: &'i str,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct Outer<'i> {
    size: TailSize,
    inner: Inner<'i>,
    after: u8,
}

#[test]
fn nested_slots() {
    let mut buf = [0u8; 32];
    let outer = Outer {
        size: TailSize(0),
        inner: Inner {
            a: 1,
            size: TailSize(0),
            s: "xyz",
        },
        after: 9,
    };
    let bytes = outer.to_ww_bytes(&mut buf).unwrap();
    // outer slot (5 bytes, 8 follow): inner (a, slot, "xyz", its length: 6 bytes), after, inner's size (6) in
    // the outer's FIFO
    assert_eq!(bytes, hex!("80 80 80 80 08 01 04 78 79 7A 03 09 06"));
    let back = Outer::from_ww_bytes(bytes).unwrap();
    assert_eq!(back.size.0, 8);
    assert_eq!(back.inner.size.0, 4);
    assert_eq!(back.inner.s, "xyz");
    assert_eq!(back.after, 9);
    // from a longer buffer as well
    let mut region = [0xEEu8; 40];
    outer.to_ww_bytes(&mut region).unwrap();
    assert_eq!(Outer::from_ww_bytes(&region).unwrap(), back);
    // nested in another Unsized value written with `write`, the slot is just as valid
    let mut buf = [0u8; 32];
    let mut wr = BufWriter::new(&mut buf);
    wr.write(&outer).unwrap();
    wr.write_u8(0x55).unwrap();
    let bytes = wr.finish().unwrap();
    let mut rd = BufReader::new(bytes);
    assert_eq!(rd.read::<Outer>().unwrap(), back);
    assert_eq!(rd.read_u8(), Ok(0x55));
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct SlotLast {
    a: bool,
    b: Option<u8>,
    size: TailSize<1>,
}

#[test]
fn slot_after_bits_and_flags() {
    let mut buf = [0u8; 8];
    let v = SlotLast {
        a: true,
        b: Some(3),
        size: TailSize(0),
    };
    let bytes = v.to_ww_bytes(&mut buf).unwrap();
    // a, b's flag, b, then the byte-aligned slot with nothing after it
    assert_eq!(bytes, hex!("C0 03 00"));
    assert_eq!(SlotLast::from_ww_bytes(bytes).unwrap(), v);
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u8)]
enum Msg<'i> {
    Text { size: TailSize<2>, s: &'i str },
    Pair(u8, TailSize<1>, &'i str),
    Nothing,
}

#[test]
fn enum_variants() {
    let mut region = [0xFFu8; 32];
    let text = Msg::Text {
        size: TailSize(0),
        s: "hi",
    };
    let len = text.to_ww_bytes(&mut region).unwrap().len();
    assert_eq!(&region[..len], hex!("00 80 03 68 69 02"));
    assert_eq!(
        Msg::from_ww_bytes(&region).unwrap(),
        Msg::Text {
            size: TailSize(3),
            s: "hi"
        }
    );
    let pair = Msg::Pair(7, TailSize(0), "abc");
    let len = pair.to_ww_bytes(&mut region).unwrap().len();
    assert_eq!(&region[..len], hex!("01 07 04 61 62 63 03"));
    assert_eq!(
        Msg::from_ww_bytes(&region).unwrap(),
        Msg::Pair(7, TailSize(4), "abc")
    );
    let mut buf = [0u8; 4];
    let bytes = Msg::Nothing.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(Msg::from_ww_bytes(bytes).unwrap(), Msg::Nothing);
}

#[derive_shrink_wrap(borrowed, derive(Debug))]
struct Framed<'i> {
    size: TailSize<2>,
    payload: TailBytes<'i>,
}

#[test]
fn with_tail_bytes() {
    let mut region = [0xFFu8; 16];
    let framed = Framed {
        size: TailSize(0),
        payload: TailBytes(&[1, 2, 3]),
    };
    let len = framed.to_ww_bytes(&mut region).unwrap().len();
    assert_eq!(&region[..len], hex!("80 03 01 02 03"));
    let back = Framed::from_ww_bytes(&region).unwrap();
    assert_eq!(back.payload.as_slice(), &[1, 2, 3]);
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct Sequenced<'i> {
    seq: UVlq32Backfill,
    size: TailSize<1>,
    body: &'i str,
}

#[test]
fn with_backfilled_seq_in_front() {
    let mut buf = [0u8; 16];
    let len = Sequenced {
        seq: UVlq32Backfill(0),
        size: TailSize(0),
        body: "ab",
    }
    .to_ww_bytes(&mut buf)
    .unwrap()
    .len();
    let bytes = UVlq32Backfill::backfill(&mut buf[..len], 300).unwrap();
    assert_eq!(bytes, hex!("82 2C 03 61 62 02"));
    let back = Sequenced::from_ww_bytes(bytes).unwrap();
    assert_eq!(back.seq.0, 300);
    assert_eq!(back.size.0, 3);
    assert_eq!(back.body, "ab");
}

/// Unsized records at fixed file offsets: each is serialized into its own capacity, the reader is bounded
/// by the slot, and a record that outgrows the capacity fails to serialize instead of overflowing.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq))]
struct Record<'i> {
    ts: u32,
    size: TailSize<1>,
    values: RefVec<'i, f32>,
    label: &'i str,
}

#[test]
fn records_at_fixed_offsets() {
    const CAP: usize = 24;
    let mut file = [0u8; 4 * CAP];
    let values = [[1.0f32, 2.0].as_slice(), &[], &[3.0, 4.0, 5.0], &[6.0]];
    for (i, vals) in values.iter().enumerate() {
        let rec = RecordOwned {
            ts: i as u32,
            size: TailSize(0),
            values: vals.to_vec(),
            label: "r".into(),
        };
        let bytes = rec.to_ww_bytes_owned().unwrap();
        assert!(bytes.len() <= CAP);
        file[i * CAP..i * CAP + bytes.len()].copy_from_slice(&bytes);
    }
    for (i, vals) in values.iter().enumerate() {
        let rec = Record::from_ww_bytes(&file[i * CAP..][..CAP]).unwrap();
        assert_eq!(rec.ts, i as u32);
        assert_eq!(rec.label, "r");
        let got: Vec<f32> = rec.values.iter().collect::<Result<_, _>>().unwrap();
        assert_eq!(&got, vals);
    }
    // too big for the capacity: a writer over a CAP-sized buffer runs out of space
    let big = RecordOwned {
        ts: 0,
        size: TailSize(0),
        values: vec![0.0; 6],
        label: "r".into(),
    };
    assert!(big.to_ww_bytes_owned().unwrap().len() > CAP);
    let mut slot = [0u8; CAP];
    assert!(
        Header {
            name: "a name longer than the capacity",
            ..header()
        }
        .to_ww_bytes(&mut slot)
        .is_err()
    );
}

const N: usize = 3;

#[derive_shrink_wrap(borrowed, sized, derive(Debug, PartialEq))]
struct ConstArray {
    a: [u8; N],
    b: [f32; N * 2],
}

#[test]
fn const_array_length() {
    let mut buf = [0u8; 32];
    let v = ConstArray {
        a: [1, 2, 3],
        b: [0.5; 6],
    };
    let bytes = v.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes.len(), 3 + 6 * 4);
    assert_eq!(ConstArray::from_ww_bytes(bytes).unwrap(), v);
    assert_eq!(
        <ConstArray as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::Sized {
            size_bits: 3 * 8 + 6 * 32
        }
    );
}

/// Generated code names `shrink_wrap` items by path: nothing from the prelude is needed, and the user's own
/// `BufReader`, `Error` and friends don't get in the way.
mod no_prelude {
    #[allow(dead_code)]
    pub struct BufReader;
    #[allow(dead_code)]
    pub struct BufWriter;
    #[allow(dead_code)]
    pub struct Error;
    #[allow(dead_code)]
    pub struct ElementSize;
    #[allow(dead_code)]
    pub struct TailSize;
    // the macro knows `RefVec` by its bare name only (a `shrink_wrap::RefVec<..>` path is a user type to it) and
    // rewrites it to a full path, so the name needs no import here

    #[shrink_wrap::derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq))]
    pub struct Clash<'i> {
        pub size: shrink_wrap::TailSize<1>,
        pub n: shrink_wrap::UNib32,
        pub s: &'i str,
        pub v: RefVec<'i, u8>,
    }

    #[shrink_wrap::derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u2, sized)]
    pub enum Mode {
        A,
        B,
    }

    #[shrink_wrap::derive_shrink_wrap(
        borrowed,
        derive(Debug, PartialEq),
        crate_path(::shrink_wrap)
    )]
    pub struct Explicit {
        pub x: u8,
    }
}

#[test]
fn qualified_names_without_the_prelude() {
    let mut buf = [0u8; 16];
    let v = no_prelude::Clash {
        size: TailSize(0),
        n: UNib32(5),
        s: "q",
        v: RefVec::new_bytes(&[9]),
    };
    let bytes = v.to_ww_bytes(&mut buf).unwrap();
    let back = no_prelude::Clash::from_ww_bytes(bytes).unwrap();
    assert_eq!(back.s, "q");
    assert_eq!(back.n, UNib32(5));
    let bytes = no_prelude::Mode::B.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(
        no_prelude::Mode::from_ww_bytes(bytes).unwrap(),
        no_prelude::Mode::B
    );
    let bytes = no_prelude::Explicit { x: 3 }.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(no_prelude::Explicit::from_ww_bytes(bytes).unwrap().x, 3);
}

/// A plain field before the slot is read without a check of its own (`read_u16_latch`): its error must
/// surface before the slot is used, not get lost with the outer reader.
#[test]
fn truncated_before_the_slot_is_an_error() {
    assert_eq!(
        Header::from_ww_bytes(&[0xCD]),
        Err(ShrinkWrapError::OutOfBoundsReadRawSlice)
    );
}
