use super::float::{FloatReader, write_float};
use super::numeric::{
    Numeric128Reader, NumericKind, NumericReader, smallest_numeric, write_numeric, write_numeric128,
};
use super::run::{Opaque, RunKind, RunReader, write_opaque_run, write_run};
use super::{
    BitCounter, Coder, FloatCodec, NumericCodec, Predictor, check, get_nib, golden, put_nib,
};
use crate::series::{BitSink, mask};
use crate::{BufReader, BufWriter, Error};
use proptest::prelude::*;

/// Runs `f` on a fresh writer, returns the bytes it touched and the bits it wrote.
fn write_with(f: impl FnOnce(&mut BufWriter<'_>) -> Result<(), Error>) -> (Vec<u8>, usize) {
    let mut buf = [0u8; 4096];
    let mut wr = BufWriter::new(&mut buf);
    f(&mut wr).unwrap();
    let (byte, bit) = wr.pos();
    let bits = byte * 8 + 7 - bit as usize;
    (wr.deinit()[..bits.div_ceil(8)].to_vec(), bits)
}

fn bit_string(bytes: &[u8], bits: usize) -> String {
    (0..bits)
        .map(|i| {
            if bytes[i / 8] & (0x80 >> (i % 8)) != 0 {
                '1'
            } else {
                '0'
            }
        })
        .collect()
}

fn bits_read(rd: &BufReader<'_>) -> usize {
    let (byte, bit) = rd.pos();
    byte * 8 + 7 - bit as usize
}

/// Bytes from a bit string with spaces, zero bits to the next byte.
fn from_bits(s: &str) -> Vec<u8> {
    let bits: Vec<bool> = s.chars().filter(|c| *c != ' ').map(|c| c == '1').collect();
    bits.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |b, (i, &x)| b | (x as u8) << (7 - i))
        })
        .collect()
}

fn codec_of(p: u8, c: u8) -> NumericCodec {
    let predictor = [Predictor::Raw, Predictor::Delta, Predictor::DeltaOfDelta][p as usize];
    let coder = [Coder::Prefix, Coder::Packed, Coder::Constant, Coder::Plain][c as usize];
    NumericCodec::new(predictor, coder)
}

fn items_of<'a>(
    values: &'a [i64],
    restarts: &'a [usize],
) -> impl Iterator<Item = (u64, bool)> + Clone + 'a {
    values
        .iter()
        .enumerate()
        .map(|(i, &v)| (v as u64, restarts.contains(&i)))
}

#[test]
fn nib_code() {
    for (value, bits) in [
        (0u32, "0000"),
        (7, "0111"),
        (8, "1000 0001"),
        (63, "1111 0111"),
        (64, "1000 1000 0001"),
        (511, "1111 1111 0111"),
        (
            u32::MAX,
            "1111 1111 1111 1111 1111 1111 1111 1111 1111 1111 0011",
        ),
    ] {
        let (bytes, n) = write_with(|wr| put_nib(wr, value));
        assert_eq!(bit_string(&bytes, n), bits.replace(' ', ""), "{value}");
        let mut rd = BufReader::new(&bytes);
        assert_eq!(get_nib(&mut rd), Ok(value));
        assert_eq!(bits_read(&rd), n);
    }
    // an 11th group with a continuation, or with bits above u32
    let mut rd = BufReader::new(&[0xFF; 8]);
    assert_eq!(get_nib(&mut rd), Err(Error::MalformedSeries));
    let bytes = from_bits(&("1000".repeat(10) + "0100"));
    assert_eq!(
        get_nib(&mut BufReader::new(&bytes)),
        Err(Error::MalformedSeries)
    );
    // out of bits
    assert!(get_nib(&mut BufReader::new(&[0x88])).is_err());
}

/// Every code of the reference encoder, bit for bit, and read back.
#[test]
fn numeric_golden() {
    for &(width, signed, values, restarts, p, c, bits) in golden::NUMERIC {
        let kind = NumericKind::new(width, signed);
        let codec = codec_of(p, c);
        let items = items_of(values, restarts);
        let (bytes, n) = write_with(|wr| {
            assert_eq!(write_numeric(wr, kind, items.clone(), Some(codec))?, codec);
            Ok(())
        });
        let case = format!("{width} {signed} {values:?} {restarts:?} {codec:?}");
        assert_eq!(bit_string(&bytes, n), bits, "{case}");
        let mut rd = NumericReader::new(BufReader::new(&bytes), kind).unwrap();
        assert_eq!(rd.codec(), codec);
        for (value, restart) in items {
            assert_eq!(rd.next(restart), Ok(value & mask(width)), "{case}");
        }
        assert_eq!(bits_read(&rd.rest()), n, "{case}");
    }
}

/// The smallest codec is the first of the shortest in trial order, and constant is offered exactly where the
/// reference encoder could write it.
#[test]
fn numeric_choice() {
    let mut cases = golden::NUMERIC;
    while let Some(&(width, signed, values, restarts, ..)) = cases.first() {
        let n = cases
            .iter()
            .take_while(|c| (c.0, c.1, c.2, c.3) == (width, signed, values, restarts))
            .count();
        let (set, rest) = cases.split_at(n);
        cases = rest;
        let kind = NumericKind::new(width, signed);
        let items = items_of(values, restarts);
        let mut best = set[0];
        for &case in set {
            if case.6.len() < best.6.len() {
                best = case;
            }
        }
        assert_eq!(
            smallest_numeric(kind, items.clone()),
            codec_of(best.4, best.5),
            "{values:?}"
        );
        for p in 0..3 {
            let in_reference = set.iter().any(|c| (c.4, c.5) == (p, 2));
            let bits = super::numeric::numeric_bits(kind, codec_of(p, 2), items.clone());
            assert_eq!(bits.is_some(), in_reference, "constant {p} {values:?}");
        }
    }
}

#[test]
fn fixed_constant_falls_back_to_packed() {
    let kind = NumericKind::new(16, false);
    let items = [(1u64, false), (5, false), (6, false)].into_iter();
    let (_, n) = write_with(|wr| {
        let codec = write_numeric(wr, kind, items.clone(), Some(codec_of(1, 2)))?;
        assert_eq!(codec, codec_of(1, 1));
        Ok(())
    });
    assert_eq!(
        Some(n),
        super::numeric::numeric_bits(kind, codec_of(1, 1), items)
    );
}

/// The two examples of the design, assembled from the column writers the way `Series<IvSample>` writes them.
#[test]
fn design_examples() {
    let tag =
        |wr: &mut BufWriter<'_>, tags: &[u32]| write_run(wr, RunKind::Tag, tags.iter().copied());
    let numeric = |wr: &mut BufWriter<'_>, width, signed, values: &[i64], restarts: &[usize]| {
        write_numeric(
            wr,
            NumericKind::new(width, signed),
            items_of(values, restarts),
            None,
        )
        .map(|_| ())
    };
    // IvSample { v: Volt { prefix, value: U16 }, i: Ampere { prefix, value: I32 } }
    let (bytes, _) = write_with(|wr| {
        put_nib(wr, 4)?; // n
        put_nib(wr, 2)?; // field count
        tag(wr, &[1; 4])?; // v.prefix: Milli
        tag(wr, &[2; 4])?; // v.value: U16
        numeric(wr, 16, false, &[3300, 3301, 3301, 3299], &[])?;
        tag(wr, &[2; 4])?; // i.prefix: Micro
        tag(wr, &[6; 4])?; // i.value: I32
        numeric(wr, 32, true, &[152000, 152002, 151999, 152001], &[])?;
        BitSink::align_byte(wr);
        Ok(())
    });
    assert_eq!(
        bytes,
        [
            0x42, 0x11, 0x14, 0x33, 0x90, 0x51, 0x91, 0x8a, 0x00, 0x04, 0xa3, 0x80, 0x1c, 0xb0
        ]
    );

    // i switches to Milli U16 at item 2: each variant's column holds its own items, entering one restarts it (v
    // is 3300, 3300, 3301, 3300 here, as in the reference encoder)
    let (bytes, _) = write_with(|wr| {
        put_nib(wr, 4)?;
        put_nib(wr, 2)?;
        tag(wr, &[1; 4])?;
        tag(wr, &[2; 4])?;
        numeric(wr, 16, false, &[3300, 3300, 3301, 3300], &[])?;
        tag(wr, &[2, 2, 1, 1])?;
        tag(wr, &[6, 6, 2, 2])?;
        numeric(wr, 16, false, &[120, 121], &[0])?; // the U16 items
        numeric(wr, 32, true, &[480, 482], &[])?; // the I32 items
        BitSink::align_byte(wr);
        Ok(())
    });
    assert_eq!(
        bytes,
        [
            0x42, 0x11, 0x14, 0x33, 0x90, 0x44, 0x94, 0x44, 0xd1, 0x20, 0x5e, 0x2f, 0x22, 0x57,
            0x81, 0xe2, 0x00
        ]
    );

    // and read back column by column
    let mut rd = BufReader::new(&bytes);
    assert_eq!(get_nib(&mut rd), Ok(4));
    assert_eq!(get_nib(&mut rd), Ok(2));
    assert_eq!(tag_run(&mut rd, 4), [1; 4]);
    assert_eq!(tag_run(&mut rd, 4), [2; 4]);
    let mut col = NumericReader::new(rd, NumericKind::new(16, false)).unwrap();
    let v: Vec<u64> = (0..4).map(|_| col.next(false).unwrap()).collect();
    assert_eq!(v, [3300, 3300, 3301, 3300]);
    rd = col.rest();
    assert_eq!(tag_run(&mut rd, 4), [2, 2, 1, 1]);
    assert_eq!(tag_run(&mut rd, 4), [6, 6, 2, 2]);
    // variants in discriminant order: U16 (2), then I32 (6)
    let mut col = NumericReader::new(rd, NumericKind::new(16, false)).unwrap();
    assert_eq!([col.next(true), col.next(false)], [Ok(120), Ok(121)]);
    let mut col = NumericReader::new(col.rest(), NumericKind::new(32, true)).unwrap();
    assert_eq!([col.next(false), col.next(false)], [Ok(480), Ok(482)]);
    assert_eq!(bits_read(&col.rest()), 129);
}

/// Steps a tag run over `m` items and moves `rd` past it.
fn tag_run(rd: &mut BufReader<'_>, m: usize) -> Vec<u32> {
    let mut run = RunReader::new(*rd, RunKind::Tag, m).unwrap();
    let values = (0..m).map(|_| run.next().unwrap()).collect();
    *rd = run.finish().unwrap();
    values
}

#[test]
fn tag_runs_golden() {
    for &(tags, two_valued, bits) in golden::TAG_RUNS {
        let kind = if two_valued {
            RunKind::OptionTag
        } else {
            RunKind::Tag
        };
        let (bytes, n) = write_with(|wr| write_run(wr, kind, tags.iter().copied()));
        assert_eq!(bit_string(&bytes, n), bits, "{tags:?}");
        assert_eq!(check::run(&mut [0; 256], two_valued as u8, tags, 0), n);
    }
}

#[test]
fn bool_and_opaque_runs_golden() {
    let bools = [true, true, false, true, true, true, false, false];
    let (bytes, n) = write_with(|wr| write_run(wr, RunKind::Bool, bools.iter().map(|&b| b as u32)));
    assert_eq!(bit_string(&bytes, n), "11000110000100100");
    let mut rd = RunReader::new(BufReader::new(&bytes), RunKind::Bool, bools.len()).unwrap();
    for b in bools {
        assert_eq!(rd.next(), Ok(b as u32));
    }

    // after 3 bits: the padding before each value's bytes is to the buffer's byte
    let values: [&[u8]; 5] = [b"ab", b"ab", b"", b"xyz", b"xyz"];
    let (bytes, n) = write_with(|wr| {
        wr.write_un32(3, 0b101)?;
        write_opaque_run(wr, values.iter().copied())
    });
    assert_eq!(
        bit_string(&bytes, n),
        "101001000110000101100010100010000000000010000001100000000111100001111001011110100"
    );
    assert_eq!(check::opaque(&mut [0; 64], &values, 3), n - 3);
}

#[test]
fn float_columns() {
    // 1.0, 1.5, 1.25 as in the SW-32 window test, then 1.0 after a restart: full width again
    let values = [1.0f32, 1.5, 1.25, 1.0];
    let items = |restart_at: usize| {
        values
            .iter()
            .enumerate()
            .map(move |(i, &v)| (v, i == restart_at))
    };
    let (_, n) = write_with(|wr| write_float(wr, items(9), Some(FloatCodec::Xor)).map(|_| ()));
    assert_eq!(n, 4 + 32 + 13 + 14 + 4);
    let (bytes, n) = write_with(|wr| write_float(wr, items(3), Some(FloatCodec::Xor)).map(|_| ()));
    assert_eq!(n, 4 + 32 + 13 + 14 + 32);
    assert_eq!(bit_string(&bytes, 4), "0001");
    let mut rd = FloatReader::<f32>::new(BufReader::new(&bytes)).unwrap();
    for (v, restart) in items(3) {
        assert_eq!(rd.next(restart), Ok(v));
    }
    // a restart forgets the window: a window reuse right after one is malformed
    let (bytes, _) = write_with(|wr| {
        wr.write_un32(4, 0b0001)?;
        wr.write_un32(32, 0)?;
        wr.write_un32(2, 0b10)
    });
    let mut rd = FloatReader::<f32>::new(BufReader::new(&bytes)).unwrap();
    assert_eq!(rd.next(false), Ok(0.0));
    assert_eq!(rd.next(false), Err(Error::MalformedSeries));

    // plain: header and 64 bits per value, the smaller one picked
    let noise = [
        (0x1234_5678_9abc_def0u64, false),
        (0x0fed_cba9_8765_4321, false),
    ];
    assert_eq!(
        check::float::<f64>(&mut [0; 64], &noise, None),
        (FloatCodec::Plain, 4 + 128)
    );
    let steady = [(2.5f64.to_bits(), false); 10];
    assert_eq!(
        check::float::<f64>(&mut [0; 64], &steady, None),
        (FloatCodec::Xor, 4 + 64 + 9)
    );
}

#[test]
fn numeric128() {
    let values = [0u128, 1, u128::MAX, 1 << 100, 12345];
    check::numeric128(&mut [0; 128], false, &values);
    let signed = [0i128, -1, i128::MIN, i128::MAX, -5].map(|v| v as u128);
    check::numeric128(&mut [0; 128], true, &signed);
    let (bytes, _) = write_with(|wr| write_numeric128(wr, true, [-1i128 as u128].into_iter()));
    // zigzag(-1) = 1
    assert_eq!(
        bytes[..],
        [0x30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10]
    );
    for header in [0b0000, 0b0001, 0b0111, 0b1011, 0b1111] {
        let bytes = [header << 4, 0, 0];
        assert_eq!(
            Numeric128Reader::new(BufReader::new(&bytes), false).err(),
            Some(Error::MalformedSeries)
        );
    }
}

#[test]
fn malformed_numeric() {
    let kind8 = NumericKind::new(8, false);
    // the escape predictor with a reserved form (`11 11` is aligned plain)
    for header in [0b1100u8, 0b1101, 0b1110] {
        let bytes = [header << 4, 0];
        assert_eq!(
            NumericReader::new(BufReader::new(&bytes), kind8).err(),
            Some(Error::MalformedSeries)
        );
    }
    // raw packed, block width 9 > 8
    let bytes = from_bits("0001 1001");
    let mut rd = NumericReader::new(BufReader::new(&bytes), kind8).unwrap();
    assert_eq!(rd.next(false), Err(Error::MalformedSeries));
    // W = 4: a 3-bit width field, 5 > 4
    let bytes = from_bits("0001 101 0000");
    let mut rd = NumericReader::new(BufReader::new(&bytes), NumericKind::new(4, false)).unwrap();
    assert_eq!(rd.next(false), Err(Error::MalformedSeries));
    // more items than bits
    let bytes = from_bits("0011 11111111");
    let mut rd = NumericReader::new(BufReader::new(&bytes), kind8).unwrap();
    assert_eq!(rd.next(false), Ok(255));
    assert!(rd.next(false).is_err());
    // header cut off
    assert!(NumericReader::new(BufReader::new(&[]), kind8).is_err());
    // aligned plain, W = 12: bits above the width set
    let bytes = [0b1111_0000, 0x34, 0x12];
    let mut rd = NumericReader::new(BufReader::new(&bytes), NumericKind::new(12, false)).unwrap();
    assert_eq!(rd.next(false), Err(Error::MalformedSeries));
    // aligned plain, a value cut off
    let bytes = [0b1111_0000, 0x34];
    let mut rd = NumericReader::new(BufReader::new(&bytes), NumericKind::new(12, false)).unwrap();
    assert!(rd.next(false).is_err());
}

/// Aligned plain after 3 bits: the header, one zero bit to the byte, the values little-endian in whole bytes,
/// restarts changing nothing.
#[test]
fn aligned_plain_bytes() {
    let kind = NumericKind::new(12, true);
    let mut buf = [0u8; 16];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_un32(3, 0b111).unwrap();
    let items = [(0x123u32, false), ((-2i32) as u32, true), (0x7FF, false)];
    let codec = write_numeric(
        &mut wr,
        kind,
        items.iter().copied(),
        Some(NumericCodec::new(Predictor::Aligned, Coder::Packed)),
    )
    .unwrap();
    assert_eq!(codec, NumericCodec::ALIGNED_PLAIN);
    let (byte, bit) = wr.pos();
    assert_eq!((byte, bit), (7, 7));
    let bytes = wr.deinit()[..7].to_vec();
    assert_eq!(bytes, [0b1111_1110, 0x23, 0x01, 0xFE, 0x0F, 0xFF, 0x07]);
    let mut rd = BufReader::new(&bytes);
    rd.read_un32(3).unwrap();
    let mut rd = NumericReader::new(rd, kind).unwrap();
    assert_eq!(rd.next(false), Ok(0x123));
    assert_eq!(rd.next(true), Ok(0xFFE));
    assert_eq!(rd.next(false), Ok(0x7FF));
    assert_eq!(rd.rest().pos(), (7, 7));

    let mut buf = [0u8; 16];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_un32(1, 1).unwrap();
    write_float(&mut wr, [(1.0f32, false)].into_iter(), Some(FloatCodec::Aligned)).unwrap();
    assert_eq!(wr.pos(), (5, 7));
    let bytes = wr.deinit()[..5].to_vec();
    assert_eq!(bytes, [0b1001_0000, 0x00, 0x00, 0x80, 0x3F]);
    let mut rd = BufReader::new(&bytes);
    rd.read_un32(1).unwrap();
    let mut rd = FloatReader::<f32>::new(rd).unwrap();
    assert_eq!(rd.next(false), Ok(1.0));
}

#[test]
fn malformed_floats() {
    for codec in 3..16u8 {
        let bytes = [codec << 4, 0, 0, 0, 0];
        assert_eq!(
            FloatReader::<f32>::new(BufReader::new(&bytes)).err(),
            Some(Error::MalformedSeries)
        );
    }
}

#[test]
fn malformed_runs() {
    let rd = |s: &str| from_bits(s);
    // a change at item 2 of 2
    let bytes = rd("0001 1 0001 0010 0");
    assert_eq!(
        RunReader::new(BufReader::new(&bytes), RunKind::Tag, 2).err(),
        Some(Error::MalformedSeries)
    );
    assert!(RunReader::new(BufReader::new(&bytes), RunKind::Tag, 3).is_ok());
    // ... and the same found by finish only: the first change fits, the second is past the end
    let bytes = rd("0001 1 0000 0010 1 0001 0011 0");
    let r = RunReader::new(BufReader::new(&bytes), RunKind::Tag, 3).unwrap();
    assert_eq!(r.finish().err(), Some(Error::MalformedSeries));
    // a gap that would overflow usize on any target is past the end too
    let bytes = rd(&format!("0001 1 {} 0011 0010 0", "1111".repeat(10)));
    assert_eq!(
        RunReader::new(BufReader::new(&bytes), RunKind::Tag, 3).err(),
        Some(Error::MalformedSeries)
    );
    // Option's tag is 0 or 1
    let bytes = rd("0010 0");
    assert_eq!(
        RunReader::new(BufReader::new(&bytes), RunKind::OptionTag, 1).err(),
        Some(Error::MalformedSeries)
    );
    // no items, and stepping past them
    let bytes = rd("0001 0");
    assert_eq!(
        RunReader::new(BufReader::new(&bytes), RunKind::Tag, 0).err(),
        Some(Error::MalformedSeries)
    );
    let mut r = RunReader::new(BufReader::new(&bytes), RunKind::Tag, 1).unwrap();
    assert_eq!(r.next(), Ok(1));
    assert_eq!(r.next(), Err(Error::MalformedSeries));
    // out of bits in the gap
    let bytes = [0b0001_1000];
    assert!(RunReader::new(BufReader::new(&bytes), RunKind::Tag, 9).is_err());
    // opaque: padding not zero, or bytes past the end
    let bytes = rd("0001 1111 00000000 0");
    assert_eq!(
        RunReader::new(BufReader::new(&bytes), Opaque, 1).err(),
        Some(Error::MalformedSeries)
    );
    let bytes = rd("0010 0000 00000000");
    assert!(RunReader::new(BufReader::new(&bytes), Opaque, 1).is_err());
}

#[test]
fn bool_and_option_runs_take_0_or_1() {
    let mut buf = [0u8; 8];
    for kind in [RunKind::Bool, RunKind::OptionTag] {
        let mut wr = BufWriter::new(&mut buf);
        assert_eq!(
            write_run(&mut wr, kind, [0, 2].into_iter()),
            Err(Error::SubtypeOutOfRange)
        );
    }
    let (_, n) = write_with(|wr| write_run(wr, RunKind::Tag, core::iter::empty()));
    assert_eq!(n, 0);
}

#[test]
fn bit_counter() {
    let mut c = BitCounter::new();
    c.put_bits(3, 0).unwrap();
    c.put_bytes(&[1, 2]).unwrap();
    assert_eq!(c.bits(), 24);
}

fn numeric_codec() -> impl Strategy<Value = Option<NumericCodec>> {
    prop_oneof![
        Just(None),
        (0..3u8, 0..4u8).prop_map(|(p, c)| Some(codec_of(p, c)))
    ]
}

/// Values that are similar (steps, a regular rate) or not, so that every coder gets picked somewhere.
fn numeric_items() -> impl Strategy<Value = Vec<(u64, bool)>> {
    prop_oneof![
        prop::collection::vec((any::<u64>(), prop::bool::weighted(0.1)), 0..40),
        (
            any::<u64>(),
            -3i64..3,
            prop::collection::vec((-2i64..3, prop::bool::weighted(0.1)), 0..60)
        )
            .prop_map(|(start, rate, steps)| {
                let mut v = start;
                steps
                    .into_iter()
                    .map(|(noise, restart)| {
                        v = v.wrapping_add((rate + noise) as u64);
                        (v, restart)
                    })
                    .collect()
            }),
    ]
}

proptest! {
    #[test]
    fn numeric_round_trip(width in 1..=64u8, signed: bool, items in numeric_items(), codec in numeric_codec()) {
        check::numeric(&mut [0; 2048], width, signed, &items, codec);
    }

    #[test]
    fn float_round_trip(items in numeric_items(), codec in prop_oneof![Just(None), Just(Some(FloatCodec::Plain)), Just(Some(FloatCodec::Xor)), Just(Some(FloatCodec::Aligned))]) {
        check::float::<f32>(&mut [0; 2048], &items, codec);
        check::float::<f64>(&mut [0; 2048], &items, codec);
    }

    #[test]
    fn run_round_trip(kind in 0..3u8, values in prop::collection::vec(prop_oneof![0..3u32, any::<u32>()], 0..50), offset in 0..8u8) {
        check::run(&mut [0; 2048], kind, &values, offset);
    }

    #[test]
    fn opaque_round_trip(picks in prop::collection::vec(0..4usize, 0..30), offset in 0..8u8) {
        let set: [&[u8]; 4] = [b"", b"a", b"volt", &[0xFF; 20]];
        let values: Vec<&[u8]> = picks.iter().map(|&i| set[i]).collect();
        check::opaque(&mut [0; 2048], &values, offset);
    }

    #[test]
    fn numeric128_round_trip(signed: bool, values in prop::collection::vec(any::<u128>(), 0..10)) {
        check::numeric128(&mut [0; 256], signed, &values);
    }

    #[test]
    fn random_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..64), selector: u8, restarts in prop::collection::vec(prop::bool::weighted(0.2), 0..100)) {
        check::garbage(&bytes, selector, &restarts);
    }
}
