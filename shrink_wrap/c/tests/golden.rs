//! Golden vectors for the C runtime (SW-43): fixed bytes for every primitive, the reverse length layouts and
//! finish(), the errors, and the latch. Each vector is checked against the Rust crate too, so a change of the wire
//! format shows up here.

use hex_literal::hex;
use shrink_wrap::Error;
use shrink_wrap_c::model::{Op, Val, check_reads, check_writes, expected};
use shrink_wrap_c::{CReader, CWriter, SwReader, SwUnsized, SwWriter, c_struct_sizes, code};

fn golden(name: &str, ops: &[Op], bytes: &[u8]) {
    assert_eq!(
        check_writes(ops, 256).as_deref(),
        Ok(bytes),
        "{name}: written"
    );
    let mut want = Vec::new();
    expected(ops, &mut want);
    assert_eq!(check_reads(bytes, ops), Ok(want), "{name}: read back");
}

#[test]
fn primitives() {
    use Op::*;
    golden("bools", &[Bool(true), Bool(false), Bool(true)], &hex!("A0"));
    golden("nibbles", &[Nib(0xA), Nib(0x5), Nib(0xF)], &hex!("A5 F0"));
    golden(
        "bool, then nibble aligned",
        &[Bool(true), Nib(0xC)],
        &hex!("8C"),
    );
    golden(
        "un8..un32",
        &[
            Bool(true),
            Un8(7, 0b010_1010),
            Un8(3, 0b110),
            Un16(12, 0b1011_1001_0100),
            Un32(17, 0b1_1010_1111_0101_0011),
        ],
        &hex!("AA D7 29 AF 53"),
    );
    golden(
        "un64",
        &[
            Un64(1, 1),
            Un64(64, 0x0123_4567_89AB_CDEF),
            Un64(35, 0x7_0000_0001),
        ],
        &hex!("80 91A2B3C4D5E6F7 F0 00000010"),
    );
    golden(
        "unsigned LE",
        &[U8(120), U16(420), U32(1_048_576), U64(u64::MAX - 123)],
        &hex!("78 A401 00001000 84FFFFFFFFFFFFFF"),
    );
    golden(
        "signed LE",
        &[I8(-120), I16(-420), I32(-1_048_576), I64(i64::MIN + 123)],
        &hex!("88 5CFE 0000F0FF 7B00000000000080"),
    );
    golden(
        "floats",
        &[F32(0.25f32.to_bits()), F64((-2.5f64).to_bits())],
        &hex!("0000803E 00000000000004C0"),
    );
    golden(
        "UNib32",
        &[
            UNib32(0),
            UNib32(7),
            UNib32(8),
            UNib32(0o777),
            UNib32(u32::MAX),
        ],
        &hex!("07 81 FF7 FFFFFFFFFF3"),
    );
    golden(
        "UVlq32",
        &[UVlq32(0), UVlq32(127), UVlq32(128), UVlq32(u32::MAX)],
        &hex!("00 7F 8100 8FFFFFFF7F"),
    );
    golden(
        "UVlq64",
        &[UVlq64(0), UVlq64(300), UVlq64(u64::MAX)],
        &hex!("00 822C 81FFFFFFFFFFFFFFFF7F"),
    );
    golden(
        "raw bytes, aligned",
        &[Bool(true), Raw(vec![1, 2, 3])],
        &hex!("80 010203"),
    );
    golden(
        "align",
        &[Bool(true), AlignByte, Bool(true)],
        &hex!("80 80"),
    );
}

#[test]
fn rev_len_layouts() {
    use Op::*;
    golden(
        "bytes and str, lengths at the end",
        &[Bytes(vec![1, 2, 3]), Str("ab".into())],
        &hex!("010203 6162 23"),
    );
    // 1 to 6 nibbles per length; the one pushed first is the last in the buffer, read first from the back
    golden(
        "length order and sizes",
        &[7u16, 8, 0o777, 0o1000, 0o77777, 0xFFFF].map(RevLen),
        &hex!("0 7FFFF9 7FFFF 0889 7FF 09 7"),
    );
    golden(
        "padding nibble after an odd count",
        &[
            Bool(true),
            Bool(true),
            Bool(true),
            Bool(true),
            Bool(true),
            RevLen(3),
        ],
        &hex!("F8 03"),
    );
    golden(
        "a length shares a byte with a bool",
        &[RevLen(1), U8(0x10), Bool(true)],
        &hex!("10 81"),
    );
    golden(
        "bits around a length (SW-40)",
        &[
            RevLen(3),
            Un8(3, 1),
            Bool(false),
            UNib32(0),
            Un8(4, 5),
            Un8(5, 5),
            Un32(17, 58_800),
            Bool(false),
        ],
        &hex!("20 52 B9 6C 03"),
    );
    golden(
        "Unsized value",
        &[
            U8(0xAA),
            Unsized(vec![Str("hi".into()), Nib(3)]),
            Bool(true),
        ],
        &hex!("AA 6869 32 83"),
    );
    golden(
        "nested Unsized values",
        &[Unsized(vec![Unsized(vec![U8(1)]), Bytes(vec![9])])],
        &hex!("01 09 11 03"),
    );
}

#[test]
fn finish() {
    // 0xFFFF takes 6 nibbles, one byte more than its slot
    let ops = [Op::RevLen(0xFFFF)];
    assert_eq!(check_writes(&ops, 2), Err(Error::OutOfBoundsRevCompact));
    assert_eq!(check_writes(&ops, 3).as_deref(), Ok(&hex!("7FFFF9")[..]));

    // finish() zeroes the bits after the last one written, and starts over
    let mut buf = [0xFF; 8];
    let mut wr = CWriter::new(&mut buf);
    wr.write_bool(true);
    wr.write_bool(false);
    assert_eq!(wr.finish(), Ok(&[0x80][..]));
    assert_eq!(wr.pos(), (0, 7));
    assert_eq!(wr.bytes_left(), 8);
    wr.write_u8(0x12);
    assert_eq!(wr.finish(), Ok(&[0x12][..]));
    assert_eq!(wr.finish(), Ok(&[][..]));
}

#[test]
fn write_errors() {
    let e = |ops: &[Op], cap: usize| check_writes(ops, cap).unwrap_err();
    assert_eq!(e(&[Op::Bool(true)], 0), Error::OutOfBoundsWriteBool);
    assert_eq!(e(&[Op::Nib(1)], 0), Error::OutOfBoundsWriteU4);
    assert_eq!(e(&[Op::U8(1)], 0), Error::OutOfBoundsWriteU8);
    assert_eq!(e(&[Op::U16(1)], 1), Error::OutOfBoundsWriteRawSlice);
    assert_eq!(e(&[Op::U64(1)], 7), Error::OutOfBoundsWriteRawSlice);
    assert_eq!(e(&[Op::Un16(9, 1)], 1), Error::OutOfBoundsWriteUN);
    assert_eq!(e(&[Op::UVlq32(128)], 1), Error::OutOfBoundsWriteRawSlice);
    assert_eq!(e(&[Op::RevLen(1)], 1), Error::OutOfBoundsRev);
    assert_eq!(
        e(&[Op::Bytes(vec![1, 2])], 3),
        Error::OutOfBoundsWriteRawSlice
    );
    assert_eq!(e(&[Op::Un8(9, 1)], 8), Error::InvalidBitCount);
    assert_eq!(e(&[Op::Un32(33, 1)], 8), Error::InvalidBitCount);
    assert_eq!(e(&[Op::Bytes(vec![0; 65536])], 70000), Error::LenTooLong);
}

#[test]
fn read_errors() {
    let e = |bytes: &[u8], ops: &[Op]| check_reads(bytes, ops).unwrap_err();
    assert_eq!(e(&[], &[Op::Bool(true)]), Error::OutOfBoundsReadBool);
    assert_eq!(e(&[], &[Op::Nib(0)]), Error::OutOfBoundsReadU4);
    assert_eq!(e(&[], &[Op::U8(0)]), Error::OutOfBoundsReadU8);
    assert_eq!(e(&[1], &[Op::U16(0)]), Error::OutOfBoundsReadRawSlice);
    assert_eq!(e(&[1], &[Op::Un16(9, 0)]), Error::OutOfBoundsReadUN);
    assert_eq!(e(&[1], &[Op::Un8(9, 0)]), Error::InvalidBitCount);
    assert_eq!(e(&[], &[Op::RevLen(0)]), Error::OutOfBoundsRev);
    // a length of 5 with nothing after it
    assert_eq!(
        e(&[0x05], &[Op::Bytes(vec![])]),
        Error::OutOfBoundsReadRawSlice
    );
    assert_eq!(e(&[0x05], &[Op::Unsized(vec![])]), Error::OutOfBoundsSplit);
    // a 12th nibble for a u32
    assert_eq!(
        e(&hex!("FFFFFFFFFFF0"), &[Op::UNib32(0)]),
        Error::MalformedUNib32
    );
    assert_eq!(
        e(&hex!("0FFFFFFFFFFF"), &[Op::RevLen(0)]),
        Error::MalformedUNib32
    );
    assert_eq!(
        e(&hex!("8080808080"), &[Op::UVlq32(0)]),
        Error::MalformedUVlq32
    );
    assert_eq!(
        e(&hex!("9080808000"), &[Op::UVlq32(0)]),
        Error::MalformedUVlq32
    );
    assert_eq!(
        e(&hex!("82808080808080808000"), &[Op::UVlq64(0)]),
        Error::MalformedUVlq64
    );
}

#[test]
fn utf8() {
    let str_of = |s: &[u8]| {
        let mut b = s.to_vec();
        assert!(s.len() < 8);
        b.push(s.len() as u8);
        check_reads(&b, &[Op::Str(String::new())])
    };
    // valid: 1 to 4 bytes per char, the edges of each range
    for s in [
        &b""[..],
        b"a",
        &hex!("C280"),
        &hex!("DFBF"),
        &hex!("E0A080"),
        &hex!("ED9FBF"),
        &hex!("EE8080"),
        &hex!("F0908080"),
        &hex!("F48FBFBF"),
    ] {
        assert_eq!(str_of(s), Ok(vec![Val::B(s.to_vec())]), "{s:02X?}");
    }
    // overlong, surrogates, above U+10FFFF, stray or missing continuation bytes
    for s in [
        &hex!("80")[..],
        &hex!("C0AF"),
        &hex!("C1BF"),
        &hex!("E08080"),
        &hex!("EDA080"),
        &hex!("F08F8080"),
        &hex!("F4908080"),
        &hex!("F5808080"),
        &hex!("FF"),
        &hex!("C2"),
        &hex!("E0A0"),
        &hex!("C241"),
        &hex!("E0A041"),
    ] {
        assert_eq!(str_of(s), Err(Error::MalformedUtf8), "{s:02X?}");
    }
}

/// SW-41: after an error no read moves and reads give 0, until the error is taken.
#[test]
fn reader_latch() {
    let buf = [0x12];
    let mut rd = CReader::new(&buf);
    assert_eq!(rd.read_u16(), 0);
    assert_eq!(rd.error(), code(Error::OutOfBoundsReadRawSlice));
    assert_eq!(rd.read_u8(), 0, "the byte is there, but an error is kept");
    assert_eq!(rd.read_bytes(), b"");
    let mut child = rd.split(0);
    assert_eq!(
        child.error(),
        code(Error::OutOfBoundsReadRawSlice),
        "a split off reader has it too"
    );
    assert!(!child.read_bool());
    assert_eq!(rd.pos(), (0, 7));
    assert_eq!(rd.take_error(), code(Error::OutOfBoundsReadRawSlice));
    assert_eq!(rd.error(), 0);
    assert_eq!(rd.read_u8(), 0x12);
}

/// SW-41 on the writer: after an error nothing is written, finish() returns it, reset() clears it.
#[test]
fn writer_latch() {
    let mut buf = [0u8; 1];
    let mut wr = CWriter::new(&mut buf);
    assert_eq!(wr.write_u16(0xABCD), code(Error::OutOfBoundsWriteRawSlice));
    assert_eq!(wr.write_u8(0x12), code(Error::OutOfBoundsWriteRawSlice));
    assert_eq!(wr.pos(), (0, 7));
    assert_eq!(wr.finish(), Err(code(Error::OutOfBoundsWriteRawSlice)));
    wr.reset();
    assert_eq!(wr.write_u8(0x12), 0);
    assert_eq!(wr.finish(), Ok(&[0x12][..]));
    assert_eq!(buf, [0x12]);
}

/// The C error numbers are the Rust ones.
#[test]
fn error_numbers() {
    use Error::*;
    let header = include_str!("../shrink_wrap.h");
    let names = [
        (OutOfBoundsWriteBool, "OUT_OF_BOUNDS_WRITE_BOOL"),
        (OutOfBoundsReadBool, "OUT_OF_BOUNDS_READ_BOOL"),
        (OutOfBoundsWriteU4, "OUT_OF_BOUNDS_WRITE_U4"),
        (OutOfBoundsReadU4, "OUT_OF_BOUNDS_READ_U4"),
        (OutOfBoundsWriteU8, "OUT_OF_BOUNDS_WRITE_U8"),
        (OutOfBoundsReadU8, "OUT_OF_BOUNDS_READ_U8"),
        (OutOfBoundsWriteRawSlice, "OUT_OF_BOUNDS_WRITE_RAW_SLICE"),
        (OutOfBoundsReadRawSlice, "OUT_OF_BOUNDS_READ_RAW_SLICE"),
        (OutOfBoundsWriteUN, "OUT_OF_BOUNDS_WRITE_UN"),
        (OutOfBoundsReadUN, "OUT_OF_BOUNDS_READ_UN"),
        (OutOfBoundsSplit, "OUT_OF_BOUNDS_SPLIT"),
        (OutOfBoundsRev, "OUT_OF_BOUNDS_REV"),
        (OutOfBoundsRevCompact, "OUT_OF_BOUNDS_REV_COMPACT"),
        (InternalSliceToArrayCast, "INTERNAL_SLICE_TO_ARRAY_CAST"),
        (MalformedUNib32, "MALFORMED_UNIB32"),
        (MalformedUVlq32, "MALFORMED_UVLQ32"),
        (MalformedUVlq64, "MALFORMED_UVLQ64"),
        (MalformedLeb, "MALFORMED_LEB"),
        (MalformedUtf8, "MALFORMED_UTF8"),
        (LenTooLong, "LEN_TOO_LONG"),
        (
            EnumFutureVersionOrMalformedData,
            "ENUM_FUTURE_VERSION_OR_MALFORMED_DATA",
        ),
        (InvalidBitCount, "INVALID_BIT_COUNT"),
        (SubtypeOutOfRange, "SUBTYPE_OUT_OF_RANGE"),
        (MalformedSeries, "MALFORMED_SERIES"),
        (LenFifoNotEmpty, "LEN_FIFO_NOT_EMPTY"),
        (EnumVariantDisabled, "ENUM_VARIANT_DISABLED"),
        (MaxLenExceeded, "MAX_LEN_EXCEEDED"),
        (MalformedOptionAnyVec, "MALFORMED_OPTION_ANY_VEC"),
        (OptionAnyVecWrongSlot, "OPTION_ANY_VEC_WRONG_SLOT"),
        (OptionAnyVecUnknownSlot, "OPTION_ANY_VEC_UNKNOWN_SLOT"),
    ];
    for (e, name) in names {
        let line = format!("SW_ERR_{name} = {},", e as u32);
        let last = format!("SW_ERR_{name} = {}\n", e as u32);
        assert!(header.contains(&line) || header.contains(&last), "{line}");
    }
    let count = header.matches("    SW_ERR_").count();
    assert_eq!(
        count,
        names.len(),
        "a C error without a Rust one, or the other way around"
    );
}

#[test]
fn struct_layout() {
    assert_eq!(
        c_struct_sizes(),
        (
            size_of::<SwReader>(),
            size_of::<SwWriter>(),
            size_of::<SwUnsized>()
        )
    );
}
