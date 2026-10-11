//! `SerializeShrinkWrap::PUSHES_LEN` (SW-36): `to_ww_bytes` skips the length FIFO for types that never push a
//! length, and a type that does push one can never take that path. The bytes are the same on both paths.

use hex_literal::hex;
use shrink_wrap::prelude::*;
use shrink_wrap::{BufWriter, Error, write_pushes_len};

const fn pushes_len<T: SerializeShrinkWrap>() -> bool {
    T::PUSHES_LEN
}

/// `to_ww_bytes` (the path picked by `PUSHES_LEN`; in a debug build the no-FIFO one panics when a length was
/// pushed) gives the bytes of `finish()`, which always looks at the FIFO.
#[track_caller]
fn check<T: SerializeShrinkWrap>(value: &T, expected: &[u8]) {
    let mut buf = [0u8; 64];
    let bytes = value.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, expected);

    let mut buf = [0u8; 64];
    let mut wr = BufWriter::new(&mut buf);
    value.ser_shrink_wrap(&mut wr).unwrap();
    assert_eq!(wr.finish().unwrap(), expected);

    let mut buf = [0u8; 64];
    let mut wr = BufWriter::new(&mut buf);
    value.ser_shrink_wrap(&mut wr).unwrap();
    assert_eq!(wr.finish_for::<T>().unwrap(), expected);
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy, Default))]
struct Plain {
    a: u8,
    flag: bool,
    b: u16,
    c: u32,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), sized)]
struct PlainSized {
    a: u8,
    flag: bool,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), final_structure)]
struct PlainFinal {
    a: u8,
    n: UNib32,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone))]
struct Options {
    a: Option<u8>,
    b: Result<u16, u8>,
    c: [u8; 2],
    d: (u8, bool),
    e: core::ops::Range<u8>,
    f: UVlq32,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), ww_repr = u8)]
enum PlainEnum {
    Unit,
    Tuple(u8, u16),
    Named { a: u32, b: Option<u8> },
}

#[derive_shrink_wrap(derive(Debug, PartialEq, Clone, Copy))]
struct WithStr<'i> {
    a: u8,
    s: &'i str,
}

#[derive_shrink_wrap(derive(Debug, PartialEq, Clone, Copy))]
struct WithVec<'i> {
    a: u8,
    v: RefVec<'i, u16>,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct WithUnsized {
    a: u8,
    inner: Plain,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct WithSized {
    a: u8,
    inner: PlainSized,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct WithFinal {
    a: u8,
    inner: PlainFinal,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct WithOptionUnsized {
    a: u8,
    inner: Option<Plain>,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct WithOptionSized {
    a: u8,
    inner: Option<PlainSized>,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy))]
struct WithArrayUnsized {
    inner: [Plain; 2],
}

#[derive_shrink_wrap(derive(Debug, PartialEq, Clone, Copy), ww_repr = u8)]
enum EnumWithStr<'i> {
    Unit,
    Plain { a: u8 },
    Str(&'i str),
}

#[derive_shrink_wrap(derive(Debug, PartialEq, Clone, Copy), final_structure)]
struct FinalWithStr<'i> {
    a: u8,
    s: &'i str,
}

#[derive_shrink_wrap(derive(Debug, PartialEq, Clone, Copy))]
struct WithFinalWithStr<'i> {
    inner: FinalWithStr<'i>,
}

#[derive_shrink_wrap(derive(Debug, PartialEq, Clone, Copy))]
struct WithTailSize<'i> {
    a: u8,
    size: TailSize<1>,
    s: &'i str,
}

const PLAIN: Plain = Plain {
    a: 0xAA,
    flag: true,
    b: 0xBBCC,
    c: 0x11223344,
};
const PLAIN_SIZED: PlainSized = PlainSized {
    a: 0xAA,
    flag: true,
};
const PLAIN_FINAL: PlainFinal = PlainFinal {
    a: 0xAA,
    n: UNib32(5),
};

#[test]
fn derived_const() {
    // plain fields, whatever the size assumption
    assert!(!pushes_len::<Plain>());
    assert!(!pushes_len::<PlainSized>());
    assert!(!pushes_len::<PlainFinal>());
    assert!(!pushes_len::<Options>());
    assert!(!pushes_len::<PlainEnum>());
    // nested types without a size slot that push nothing themselves
    assert!(!pushes_len::<WithSized>());
    assert!(!pushes_len::<WithFinal>());
    assert!(!pushes_len::<WithOptionSized>());

    // strings and lists
    assert!(pushes_len::<WithStr>());
    assert!(pushes_len::<WithVec>());
    assert!(pushes_len::<EnumWithStr>());
    assert!(pushes_len::<FinalWithStr>());
    assert!(pushes_len::<WithFinalWithStr>());
    assert!(pushes_len::<WithTailSize>());
    // the size slot of a nested Unsized value, also the one inside an Option or an array
    assert!(pushes_len::<WithUnsized>());
    assert!(pushes_len::<WithOptionUnsized>());
    assert!(pushes_len::<WithArrayUnsized>());
}

#[test]
fn library_types_const() {
    assert!(!pushes_len::<u8>());
    assert!(!pushes_len::<bool>());
    assert!(!pushes_len::<()>());
    assert!(!pushes_len::<UNib32>());
    assert!(!pushes_len::<UVlq32>());
    assert!(!pushes_len::<UVlq64>());
    assert!(!pushes_len::<Nibble>());
    assert!(!pushes_len::<Option<u8>>());
    assert!(!pushes_len::<Result<u8, u16>>());
    assert!(!pushes_len::<(u8, bool)>());
    assert!(!pushes_len::<[u8; 4]>());
    assert!(!pushes_len::<core::ops::Range<u8>>());
    assert!(!pushes_len::<&u8>());
    assert!(!pushes_len::<Option<PlainSized>>());
    assert!(!pushes_len::<[PlainFinal; 2]>());

    assert!(pushes_len::<&str>());
    assert!(pushes_len::<&[u8]>());
    assert!(pushes_len::<RefVec<'_, u8>>());
    assert!(pushes_len::<Option<&str>>());
    assert!(pushes_len::<Result<u8, &str>>());
    assert!(pushes_len::<(u8, &str)>());
    assert!(pushes_len::<[&str; 2]>());
    // a plain Unsized type pushes nothing as a root, and a size slot when written with `wr.write()`
    assert!(!write_pushes_len::<PlainSized>());
    assert!(!write_pushes_len::<PlainFinal>());
    assert!(write_pushes_len::<Plain>());
    assert!(pushes_len::<Option<Plain>>());
    assert!(pushes_len::<(u8, Plain)>());
    assert!(pushes_len::<[Plain; 2]>());
    assert!(pushes_len::<core::ops::Range<Plain>>());
}

#[cfg(feature = "std")]
#[test]
fn std_types_const() {
    assert!(pushes_len::<String>());
    assert!(pushes_len::<Vec<u8>>());
    assert!(pushes_len::<Box<&str>>());
    assert!(!pushes_len::<Box<u8>>());
}

#[test]
fn same_bytes_without_fifo() {
    check(&PLAIN, &hex!("AA 80 CC BB 44 33 22 11"));
    check(&PLAIN_SIZED, &hex!("AA 80"));
    check(&PLAIN_FINAL, &hex!("AA 50"));
    check(
        &Options {
            a: Some(1),
            b: Err(2),
            c: [3, 4],
            d: (5, true),
            e: 6..7,
            f: UVlq32(8),
        },
        &hex!("80 01 00 02 03 04 05 80 06 07 08"),
    );
    check(&PlainEnum::Unit, &hex!("00"));
    check(&PlainEnum::Tuple(1, 2), &hex!("01 01 02 00"));
    check(
        &PlainEnum::Named { a: 1, b: Some(2) },
        &hex!("02 01 00 00 00 80 02"),
    );
    check(
        &WithSized {
            a: 1,
            inner: PLAIN_SIZED,
        },
        &hex!("01 AA 80"),
    );
    check(
        &WithFinal {
            a: 1,
            inner: PLAIN_FINAL,
        },
        &hex!("01 AA 50"),
    );
    check(
        &WithOptionSized {
            a: 1,
            inner: Some(PLAIN_SIZED),
        },
        &hex!("01 80 AA 80"),
    );
    check(&(0xAA_u8, true), &hex!("AA 80"));
    check(&Some(0xAA_u8), &hex!("80 AA"));
    check(&[1u8, 2, 3], &hex!("01 02 03"));
    check(&UNib32(5), &hex!("50"));
    check(&(), &[]);
}

#[test]
fn same_bytes_with_fifo() {
    check(&WithStr { a: 1, s: "ab" }, &hex!("01 61 62 02"));
    check(
        &WithVec {
            a: 1,
            v: RefVec::Slice { slice: &[2, 3] },
        },
        &hex!("01 02 00 03 00 02"),
    );
    check(
        &WithUnsized { a: 1, inner: PLAIN },
        &hex!("01 AA 80 CC BB 44 33 22 11 09"),
    );
    check(
        &WithOptionUnsized {
            a: 1,
            inner: Some(PLAIN),
        },
        &hex!("01 80 AA 80 CC BB 44 33 22 11 09"),
    );
    check(&WithOptionUnsized { a: 1, inner: None }, &hex!("01 00"));
    check(&EnumWithStr::Unit, &hex!("00"));
    check(&EnumWithStr::Plain { a: 1 }, &hex!("01 01"));
    check(&EnumWithStr::Str("ab"), &hex!("02 61 62 02"));
    check(
        &WithFinalWithStr {
            inner: FinalWithStr { a: 1, s: "ab" },
        },
        &hex!("01 61 62 02"),
    );
    check(
        &WithTailSize {
            a: 1,
            size: TailSize(0),
            s: "ab",
        },
        &hex!("01 03 61 62 02"),
    );
    check(&"ab", &hex!("61 62 02"));
    check(&(1u8, "ab"), &hex!("01 61 62 02"));
    check(&Some("ab"), &hex!("80 61 62 02"));
}

/// A hand-written impl without the const: not Sized, so it takes the FIFO path
struct HandWritten<'i>(&'i str);

impl SerializeShrinkWrap for HandWritten<'_> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_str(self.0)
    }
}

/// A hand-written impl that says it pushes no length, and does
struct Lying<'i>(&'i str);

impl SerializeShrinkWrap for Lying<'_> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;
    const PUSHES_LEN: bool = false;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_str(self.0)
    }
}

#[test]
fn hand_written_default() {
    assert!(pushes_len::<HandWritten>());
    check(&HandWritten("ab"), &hex!("61 62 02"));
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "PUSHES_LEN"))]
fn wrong_const_is_caught() {
    let mut buf = [0u8; 16];
    // debug: panics; release: an error instead of a message without its lengths
    assert_eq!(
        Lying("ab").to_ww_bytes(&mut buf),
        Err(Error::LenFifoNotEmpty)
    );
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "PUSHES_LEN"))]
fn finish_no_lens_with_a_length() {
    let mut buf = [0u8; 16];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_u8(1).unwrap();
    wr.write_rev_len(2).unwrap();
    assert_eq!(wr.finish_no_lens(), Err(Error::LenFifoNotEmpty));
}

#[test]
fn finish_no_lens() {
    let mut buf = [0xFF; 16];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_u8(0xAA).unwrap();
    wr.write_bool(true).unwrap();
    assert_eq!(wr.finish_no_lens().unwrap(), &hex!("AA 80"));
    // ready for the next message, like after finish()
    wr.write_u16(0x1234).unwrap();
    assert_eq!(wr.finish_no_lens().unwrap(), &hex!("34 12"));

    // a latched error comes out of it too
    let mut buf = [0u8; 1];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_u16_latch(1);
    assert_eq!(wr.finish_no_lens(), Err(Error::OutOfBoundsWriteRawSlice));
}
