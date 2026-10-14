//! `U1`..`U64` and `I2`..`I64` fields of derived types are read with the error kept in the reader
//! (`BufReader::read_field`, `read_un*_latch`) and checked once per type, like the integers since SW-34 (SW-38).
//! That must not be observable: a read gives the value, the error and the reader state of the read with a
//! `Result`, a failed one gives 0 and keeps the first error, and a derived type gives the result of reading every
//! field with `?` (since SW-41 also when it has lists or nested types), and on success the same reader.

use shrink_wrap::prelude::*;
use shrink_wrap::{BufReader, Error};

/// One read of `T` from a copy of `rd` both ways: the plain `read` and the derive's `read_field` + `latched`.
/// Returns the result.
fn check<'i, T>(rd: BufReader<'i>, what: &str) -> Result<T, Error>
where
    T: DeserializeShrinkWrap<'i> + DeserializeShrinkWrapOwned + PartialEq + core::fmt::Debug,
{
    let ty = core::any::type_name::<T>();
    let mut rd_old = rd;
    let old = rd_old.read::<T>();

    let mut rd_new = rd;
    let new = rd_new.read_field::<T>();
    // never an error of its own: that is the point
    let new = new.unwrap_or_else(|e| panic!("{ty}: read_field returned {e:?}, {what}"));
    match &old {
        Ok(val) => assert_eq!(&new, val, "{ty}: value, {what}"),
        Err(_) => assert_eq!(
            new,
            T::from_ww_bytes_owned(&[0; 8]).unwrap(),
            "{ty}: 0 after an error, {what}"
        ),
    }
    assert_eq!(
        rd_new.latched(),
        old.as_ref().map(|_| ()).map_err(|e| *e),
        "{ty}: error, {what}"
    );
    assert_eq!(rd_new, rd_old, "{ty}: reader after, {what}");

    // the owned side reads the same way
    let mut rd_owned = rd;
    let owned = rd_owned.read_field_owned::<T>().unwrap();
    assert_eq!(
        rd_owned.latched(),
        old.as_ref().map(|_| ()).map_err(|e| *e),
        "{ty}: owned, {what}"
    );
    assert_eq!(owned, new, "{ty}: owned value, {what}");
    assert_eq!(rd_owned, rd_old, "{ty}: owned reader after, {what}");

    // an error kept before stays the first one
    let mut rd_first = rd;
    rd_first.read_un8_latch(9);
    rd_first.read_field::<T>().unwrap();
    assert_eq!(
        rd_first.latched(),
        Err(Error::InvalidBitCount),
        "{ty}: first error, {what}"
    );
    old
}

/// Runs `$body` with `$T` being every type read with a latch.
macro_rules! for_each_un {
    (|$T:ident| $body:block) => {
        for_each_un!(@types $T $body; U1, U2, U3, U4, U5, U6, U7, U8, U9, U10, U11, U12, U13, U14, U15, U16, U17, U18, U19, U20, U21, U22, U23, U24, U25, U26, U27, U28, U29, U30, U31, U32, U33, U34, U35, U36, U37, U38, U39, U40, U41, U42, U43, U44, U45, U46, U47, U48, U49, U50, U51, U52, U53, U54, U55, U56, U57, U58, U59, U60, U61, U62, U63, U64, I2, I3, I4, I5, I6, I7, I8, I9, I10, I11, I12, I13, I14, I15, I16, I17, I18, I19, I20, I21, I22, I23, I24, I25, I26, I27, I28, I29, I30, I31, I32, I33, I34, I35, I36, I37, I38, I39, I40, I41, I42, I43, I44, I45, I46, I47, I48, I49, I50, I51, I52, I53, I54, I55, I56, I57, I58, I59, I60, I61, I62, I63, I64);
    };
    (@types $T:ident $body:block; $($ty:ty),*) => {
        $({
            #[allow(dead_code)]
            type $T = $ty;
            $body
        })*
    };
}

#[test]
fn every_type_every_bit_offset_and_length() {
    let buf = [0xA5, 0x3C, 0xFF, 0x00, 0x81, 0x7E, 0x5A, 0xC3, 0x96];
    for len in 0..=buf.len() {
        for skip in 0..8 {
            let mut rd = BufReader::new(&buf[..len]);
            let _ = rd.read_un8(skip);
            for_each_un!(|T| {
                let _ = check::<T>(rd, &format!("{len} bytes, {skip} bits read"));
            });
        }
    }
}

#[test]
fn values() {
    let mut scratch = [0u8; 32];
    let mut wr = BufWriter::new(&mut scratch);
    wr.write(&U3::new(5).unwrap()).unwrap();
    wr.write(&I10::new(-300).unwrap()).unwrap();
    wr.write(&U64::new(u64::MAX).unwrap()).unwrap();
    wr.write(&I64::new(i64::MIN).unwrap()).unwrap();
    let bytes = wr.finish_no_lens().unwrap().to_vec();
    let mut rd = BufReader::new(&bytes);
    assert_eq!(check::<U3>(rd, "U3"), Ok(U3::new(5).unwrap()));
    rd.read_field::<U3>().unwrap();
    assert_eq!(check::<I10>(rd, "I10"), Ok(I10::new(-300).unwrap()));
    rd.read_field::<I10>().unwrap();
    assert_eq!(check::<U64>(rd, "U64"), Ok(U64::new(u64::MAX).unwrap()));
    rd.read_field::<U64>().unwrap();
    assert_eq!(check::<I64>(rd, "I64"), Ok(I64::new(i64::MIN).unwrap()));
    rd.read_field::<I64>().unwrap();
    assert_eq!(rd.latched(), Ok(()));
}

#[test]
fn bit_counts() {
    let buf = [0xFF; 9];
    for bits in 0..=70 {
        let rd = BufReader::new(&buf);
        let (mut a, mut b) = (rd, rd);
        assert_eq!(a.read_un8(bits).map(|_| ()), {
            b.read_un8_latch(bits);
            b.latched()
        });
        assert_eq!(a, b, "un8 {bits}");
        let (mut a, mut b) = (rd, rd);
        assert_eq!(a.read_un16(bits).map(|_| ()), {
            b.read_un16_latch(bits);
            b.latched()
        });
        assert_eq!(a, b, "un16 {bits}");
        let (mut a, mut b) = (rd, rd);
        assert_eq!(a.read_un32(bits).map(|_| ()), {
            b.read_un32_latch(bits);
            b.latched()
        });
        assert_eq!(a, b, "un32 {bits}");
        let (mut a, mut b) = (rd, rd);
        assert_eq!(a.read_un64(bits).map(|_| ()), {
            b.read_un64_latch(bits);
            b.latched()
        });
        assert_eq!(a, b, "un64 {bits}");
    }
}

/// Only fields read with a latch: the derived code, and the same fields read one by one with `?`.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Clone, Copy))]
struct Sample {
    ch: U3,
    on: bool,
    div: U12,
    off: I10,
    level: u8,
    raw: U20,
    flags: U5,
    big: U40,
    neg: I33,
}

fn sample_with_checks<'i>(rd: &mut BufReader<'i>) -> Result<Sample, Error> {
    Ok(Sample {
        ch: rd.read()?,
        on: rd.read_bool()?,
        div: rd.read()?,
        off: rd.read()?,
        level: rd.read_u8()?,
        raw: rd.read()?,
        flags: rd.read()?,
        big: rd.read()?,
        neg: rd.read()?,
    })
}

/// A list and a nested type among them: a type with error exits of its own, where the `U`/`I` fields are read
/// with `?` as before, so that a later error does not hide the first one.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone))]
struct Mixed<'i> {
    a: U7,
    list: RefVec<'i, U4>,
    b: I12,
    inner: Sample,
    c: U9,
}

fn mixed_with_checks<'i>(rd: &mut BufReader<'i>) -> Result<Mixed<'i>, Error> {
    Ok(Mixed {
        a: rd.read()?,
        list: rd.read()?,
        b: rd.read()?,
        inner: rd.read()?,
        c: rd.read()?,
    })
}

/// A latched variant, one with a list, one with a field that has a default (its flag falls back, its value is
/// read with `?`).
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone), ww_repr = u8, no_future_version)]
enum Ev<'i> {
    A {
        x: U5,
        y: u8,
        z: I7,
    },
    B(U3, I33, bool),
    C {
        z: U9,
        list: RefVec<'i, u8>,
        w: U2,
    },
    D {
        x: U11,
        #[default = None]
        late: Option<U2>,
    },
}

fn ev_with_checks<'i>(rd: &mut BufReader<'i>) -> Result<Ev<'i>, Error> {
    Ok(match rd.read_u8()? {
        0 => Ev::A {
            x: rd.read()?,
            y: rd.read_u8()?,
            z: rd.read()?,
        },
        1 => Ev::B(rd.read()?, rd.read()?, rd.read_bool()?),
        2 => Ev::C {
            z: rd.read()?,
            list: rd.read()?,
            w: rd.read()?,
        },
        3 => Ev::D {
            x: rd.read()?,
            late: if rd.read_bool().unwrap_or(false) {
                Some(rd.read()?)
            } else {
                None
            },
        },
        _ => return Err(Error::EnumFutureVersionOrMalformedData),
    })
}

/// Compares the derived [Sample] with [sample_with_checks]: the same result, and on success the same reader.
fn check_sample(rd: BufReader<'_>, what: &str) -> Result<Sample, Error> {
    let mut rd_old = rd;
    let old = sample_with_checks(&mut rd_old);
    let mut rd_new = rd;
    let new = Sample::des_shrink_wrap(&mut rd_new);
    assert_eq!(new, old, "{what}");
    let mut rd_owned = rd;
    let owned = <Sample as DeserializeShrinkWrapOwned>::des_shrink_wrap_owned(&mut rd_owned);
    assert_eq!(owned.map(|s| s.ch), old.map(|s| s.ch), "owned, {what}");
    if old.is_ok() {
        assert_eq!(rd_new, rd_old, "reader after, {what}");
        assert_eq!(rd_owned, rd_old, "owned reader after, {what}");
    }
    // nothing is left in the latch for whoever reads next
    assert_eq!(rd_new.latched(), Ok(()), "{what}");
    new
}

const SAMPLE: Sample = Sample {
    ch: U3::max(),
    on: true,
    div: U12::one(),
    off: I10::minus_one(),
    level: 0xA5,
    raw: U20::max(),
    flags: U5::zero(),
    big: U40::max(),
    neg: I33::min(),
};

#[test]
fn derived_struct() {
    let mut scratch = [0u8; 64];
    let bytes = SAMPLE.to_ww_bytes(&mut scratch).unwrap().to_vec();
    assert_eq!(check_sample(BufReader::new(&bytes), "whole"), Ok(SAMPLE));
    for cut in 0..bytes.len() {
        assert!(check_sample(BufReader::new(&bytes[..cut]), &format!("cut at {cut}")).is_err());
    }
    for i in 0..bytes.len() {
        for xor in [0x01, 0x10, 0x80, 0xFF] {
            let mut buf = bytes.clone();
            buf[i] ^= xor;
            check_sample(BufReader::new(&buf), &format!("byte {i} ^ {xor:#x}")).unwrap();
        }
    }
}

/// [check_sample] for [Mixed] and [Ev].
fn check_mixed(rd: BufReader<'_>, what: &str) {
    let (mut rd_old, mut rd_new) = (rd, rd);
    let old = mixed_with_checks(&mut rd_old);
    assert_eq!(Mixed::des_shrink_wrap(&mut rd_new), old, "Mixed, {what}");
    if old.is_ok() {
        assert_eq!(rd_new, rd_old, "Mixed reader after, {what}");
    }
    let (mut rd_old, mut rd_new) = (rd, rd);
    let old = ev_with_checks(&mut rd_old);
    assert_eq!(Ev::des_shrink_wrap(&mut rd_new), old, "Ev, {what}");
    if old.is_ok() {
        assert_eq!(rd_new, rd_old, "Ev reader after, {what}");
    }
}

#[test]
fn derived_enum() {
    let list = [1, 2, 3];
    for value in [
        Ev::A {
            x: U5::max(),
            y: 7,
            z: I7::min(),
        },
        Ev::B(U3::one(), I33::minus_one(), true),
        Ev::C {
            z: U9::max(),
            list: RefVec::Slice { slice: &list },
            w: U2::one(),
        },
        Ev::D {
            x: U11::max(),
            late: Some(U2::max()),
        },
    ] {
        let mut scratch = [0u8; 64];
        let bytes = value.to_ww_bytes(&mut scratch).unwrap().to_vec();
        assert_eq!(Ev::from_ww_bytes(&bytes), Ok(value.clone()));
        for cut in 0..=bytes.len() {
            check_mixed(
                BufReader::new(&bytes[..cut]),
                &format!("{value:?} cut at {cut}"),
            );
        }
        for i in 0..bytes.len() {
            for xor in [0x01, 0x02, 0x10, 0x80, 0xFF] {
                let mut buf = bytes.clone();
                buf[i] ^= xor;
                check_mixed(
                    BufReader::new(&buf),
                    &format!("{value:?} byte {i} ^ {xor:#x}"),
                );
            }
        }
    }
}

/// A struct with fields that are not latched: whatever the buffer, decoding is an error or the value, never a
/// panic; a buffer cut short is always an error.
#[test]
fn derived_struct_with_a_list() {
    let list = [U4::max(), U4::one(), U4::zero()];
    let value = Mixed {
        a: U7::max(),
        list: RefVec::Slice { slice: &list },
        b: I12::min(),
        inner: SAMPLE,
        c: U9::one(),
    };
    let mut scratch = [0u8; 64];
    let bytes = value.to_ww_bytes(&mut scratch).unwrap().to_vec();
    assert_eq!(Mixed::from_ww_bytes(&bytes), Ok(value.clone()));
    for cut in 0..bytes.len() {
        if let Ok(other) = Mixed::from_ww_bytes(&bytes[..cut]) {
            // cut inside the lengths at the end: other valid data
            assert_ne!(other, value, "cut at {cut}");
        }
    }
    for i in 0..bytes.len() {
        for xor in [0x01, 0x10, 0x80, 0xFF] {
            let mut buf = bytes.clone();
            buf[i] ^= xor;
            check_mixed(BufReader::new(&buf), &format!("byte {i} ^ {xor:#x}"));
        }
    }
    for cut in 0..bytes.len() {
        check_mixed(BufReader::new(&bytes[..cut]), &format!("cut at {cut}"));
    }
}

/// xorshift64*: the repo has no proptest, and a fixed seed keeps a failure reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() >> 33) as usize % n
    }
}

/// A reader over `buf` in a random state: some bits, nibbles, bytes or lengths already read, or a split of it.
fn random_reader<'i>(rng: &mut Rng, buf: &'i [u8]) -> BufReader<'i> {
    let mut rd = BufReader::new(buf);
    for _ in 0..rng.below(5) {
        // errors are fine here: they are one more state to start from
        match rng.below(6) {
            0 => {
                let _ = rd.read_bool();
            }
            1 => {
                let _ = rd.read_nib();
            }
            2 => {
                let _ = rd.read_u8();
            }
            3 => {
                let _ = rd.read_rev_len();
            }
            4 => {
                let _ = rd.read_un8(1 + rng.below(8) as u8);
            }
            _ => {
                let len = rng.below(buf.len() + 2);
                if let Ok(split) = rd.split(len) {
                    rd = split;
                }
            }
        }
    }
    rd
}

#[test]
fn random_buffers() {
    let mut rng = Rng(0x5357_4c41_5443_4801);
    let (mut ok, mut err) = (0, 0);
    for round in 0..20_000 {
        let len = rng.below(if round % 8 == 0 { 40 } else { 14 });
        let buf: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        let rd = random_reader(&mut rng, &buf);
        let what = format!("round {round}, {buf:02x?}, at {:?}", rd.pos());
        for_each_un!(|T| {
            let _ = check::<T>(rd, &what);
        });
        match check_sample(rd, &what) {
            Ok(_) => ok += 1,
            Err(_) => err += 1,
        }
        check_mixed(rd, &what);
    }
    // the loop is worth something only if both outcomes happen
    assert!(ok > 1000 && err > 1000, "{ok} decoded, {err} errors");
}

/// Every other type is read by `read_field` as by `read`: its error is returned, not kept.
#[test]
fn other_types_are_read_as_before() {
    fn same<'i, T: DeserializeShrinkWrap<'i> + PartialEq + core::fmt::Debug>(buf: &'i [u8]) {
        let (mut a, mut b) = (BufReader::new(buf), BufReader::new(buf));
        assert_eq!(
            a.read::<T>(),
            b.read_field::<T>(),
            "{}",
            core::any::type_name::<T>()
        );
        assert_eq!(a, b);
    }
    for buf in [
        &[][..],
        &[0x80],
        &[0xFF, 0xFF],
        &[0x12, 0x34, 0x56, 0x78, 0x9A],
    ] {
        same::<u8>(buf);
        same::<u32>(buf);
        same::<bool>(buf);
        same::<Nibble>(buf);
        same::<UNib32>(buf);
        same::<Option<U4>>(buf);
        same::<(U4, U4)>(buf);
        same::<Sample>(buf);
        same::<Ev>(buf);
    }
}
