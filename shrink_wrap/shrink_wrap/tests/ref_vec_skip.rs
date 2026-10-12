//! `RefVec<T>` of `u8`..`u128`, `i8`..`i128`, `f32`, `f64` finds its end without decoding the elements (SW-37).
//! That must not be observable: for every input the result (the list or the error) and the whole reader state
//! afterwards are the ones of the decode pass, which is kept here as [PassVec] to compare against.

use shrink_wrap::prelude::*;
use shrink_wrap::{BufReader, BufWriter, BufWriterOwned, ElementSize, Error};

/// `RefVec<T>` as it was decoded before SW-37, and still is for every other `T`: read the count, then every element.
#[derive(Clone, Copy)]
struct PassVec<'i, T>(RefVec<'i, T>);

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for PassVec<'i, T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let elements_count = rd.read_rev_len()?;
        let buf = *rd;
        for _ in 0..elements_count {
            let _item: T = rd.read()?;
        }
        Ok(PassVec(RefVec::Buf {
            buf,
            elements_count,
        }))
    }
}

impl<'i, T: SerializeShrinkWrap + DeserializeShrinkWrap<'i> + Clone> SerializeShrinkWrap
    for PassVec<'i, T>
{
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        self.0.ser_shrink_wrap(wr)
    }
}

/// What a decoded list is made of: where it starts (a whole reader) and its count.
fn parts<'i, T>(list: RefVec<'i, T>) -> (BufReader<'i>, usize) {
    match list {
        RefVec::Buf {
            buf,
            elements_count,
        } => (buf, elements_count),
        RefVec::Slice { .. } => panic!("a decoded RefVec is a Buf"),
    }
}

/// Decode a list of `T` from a copy of `rd` both ways and compare everything. Returns whether it decoded.
fn check<'i, T>(rd: BufReader<'i>, what: &str) -> Result<usize, Error>
where
    T: DeserializeShrinkWrap<'i> + Clone + 'i,
{
    let size = T::PLAIN_BYTES.expect("only called for the types in scope");
    assert_eq!(
        T::ELEMENT_SIZE,
        ElementSize::Sized {
            size_bits: size * 8
        }
    );

    let mut rd_skip = rd;
    let mut rd_pass = rd;
    let skip = rd_skip.read::<RefVec<'i, T>>().map(parts);
    let pass = rd_pass.read::<PassVec<'i, T>>().map(|list| parts(list.0));
    let ty = core::any::type_name::<T>();
    assert_eq!(skip, pass, "{ty}: result, {what}");
    assert_eq!(rd_skip, rd_pass, "{ty}: reader after, {what}");

    let (start, count) = skip?;
    // every element is there, and reading them ends where the list was found to end
    let mut walk = start;
    for _ in 0..count {
        walk.read::<T>()
            .unwrap_or_else(|e| panic!("{ty}: element after a successful decode: {e:?}, {what}"));
    }
    assert_eq!(walk.pos(), rd_skip.pos(), "{ty}: end of the list, {what}");
    assert_eq!(walk.bytes_left(), rd_skip.bytes_left(), "{ty}: {what}");
    Ok(count)
}

/// Runs `$body` with `$T` being every type in scope, and proves at compile time that each of them is.
macro_rules! for_each_plain {
    (|$T:ident| $body:block) => {
        for_each_plain!(@types $T $body; u8, u16, u32, u64, u128, i8, i16, i32, i64, i128, f32, f64);
    };
    (@types $T:ident $body:block; $($ty:ty),*) => {
        $({
            const _: () = assert!(<$ty as DeserializeShrinkWrap>::PLAIN_BYTES.unwrap() == size_of::<$ty>());
            #[allow(dead_code)]
            type $T = $ty;
            $body
        })*
    };
}

/// `count` elements of `size` bytes (a counter as content), then the count as it is written at the back.
fn list_bytes(count: usize, size: usize) -> Vec<u8> {
    let mut wr = BufWriterOwned::new();
    wr.write_rev_len(count).unwrap();
    for i in 0..count * size {
        wr.write_u8(i as u8).unwrap();
    }
    wr.finish_and_take().unwrap()
}

#[test]
fn scope_is_exactly_the_plain_primitives() {
    assert_eq!(<bool as DeserializeShrinkWrap>::PLAIN_BYTES, None);
    assert_eq!(<Nibble as DeserializeShrinkWrap>::PLAIN_BYTES, None);
    assert_eq!(<U7 as DeserializeShrinkWrap>::PLAIN_BYTES, None);
    assert_eq!(<UNib32 as DeserializeShrinkWrap>::PLAIN_BYTES, None);
    assert_eq!(<&str as DeserializeShrinkWrap>::PLAIN_BYTES, None);
    assert_eq!(<Option<u8> as DeserializeShrinkWrap>::PLAIN_BYTES, None);
    assert_eq!(<[u8; 4] as DeserializeShrinkWrap>::PLAIN_BYTES, None);
    assert_eq!(<(u8, u8) as DeserializeShrinkWrap>::PLAIN_BYTES, None);
    assert_eq!(<RefVec<u8> as DeserializeShrinkWrap>::PLAIN_BYTES, None);
    assert_eq!(<Byte as DeserializeShrinkWrap>::PLAIN_BYTES, None);
    assert_eq!(<Kind as DeserializeShrinkWrap>::PLAIN_BYTES, None);
    for_each_plain!(|T| {
        assert!(T::PLAIN_BYTES.is_some());
    });
}

#[test]
fn counts_zero_one_many() {
    for_each_plain!(|T| {
        let size = size_of::<T>();
        for count in [0, 1, 2, 7, 8, 9, 63, 64, 300] {
            let buf = list_bytes(count, size);
            let rd = BufReader::new(&buf);
            assert_eq!(check::<T>(rd, &format!("count {count}")), Ok(count));
        }
    });
}

#[test]
fn truncated() {
    for_each_plain!(|T| {
        let size = size_of::<T>();
        for count in [1, 2, 7, 8, 9, 300] {
            let full = list_bytes(count, size);
            // one byte short, one element short, only the count left, and everything in between for small lists
            let mut cuts = vec![1, size, count * size];
            if count * size <= 64 {
                cuts.extend(1..=count * size);
            }
            for cut in cuts {
                let buf = &full[cut..];
                let res = check::<T>(BufReader::new(buf), &format!("count {count}, {cut} short"));
                let expected = if size == 1 {
                    Error::OutOfBoundsReadU8
                } else {
                    Error::OutOfBoundsReadRawSlice
                };
                assert_eq!(res, Err(expected), "count {count}, {cut} short");
            }
        }
    });
}

#[test]
fn huge_count() {
    // counts whose size in bytes does not fit the buffer by far, or a 32-bit usize at all
    for count in [
        u16::MAX as usize,
        0x1000_0000,
        0x7FFF_FFFF,
        0x8000_0000,
        u32::MAX as usize,
    ] {
        let mut wr = BufWriterOwned::new();
        wr.write_rev_len(count).unwrap();
        for i in 0..40u8 {
            wr.write_u8(i).unwrap();
        }
        let buf = wr.finish_and_take().unwrap();
        for_each_plain!(|T| {
            let res = check::<T>(BufReader::new(&buf), &format!("count {count}"));
            assert!(res.is_err(), "count {count}");
        });
    }
}

#[test]
fn mid_byte_start() {
    // a list after 1 to 8 bits or a nibble: the first element aligns to the next byte, an empty list does not
    for_each_plain!(|T| {
        let size = size_of::<T>();
        for count in [0, 1, 5] {
            for bits in 0..=8 {
                let mut wr = BufWriterOwned::new();
                for bit in 0..bits {
                    wr.write_bool(bit % 2 == 0).unwrap();
                }
                wr.write_rev_len(count).unwrap();
                for i in 0..count * size {
                    wr.write_u8(i as u8).unwrap();
                }
                wr.write_bool(true).unwrap();
                wr.write_u8(0xAA).unwrap();
                let full = wr.finish_and_take().unwrap();

                for cut in 0..=1 {
                    // cut: the same with the last data byte missing
                    let mut buf = full.clone();
                    if cut == 1 {
                        buf.remove(buf.len() - 2);
                    }
                    let mut rd = BufReader::new(&buf);
                    for _ in 0..bits {
                        rd.read_bool().unwrap();
                    }
                    let before = rd.pos();
                    let res =
                        check::<T>(rd, &format!("count {count} after {bits} bits, cut {cut}"));
                    if cut == 0 {
                        assert_eq!(res, Ok(count));
                    }
                    if count == 0 {
                        // nothing was read, so nothing was aligned: the flag after the list follows the bits
                        let mut rd_after = rd;
                        let _: RefVec<'_, T> = rd_after.read().unwrap();
                        assert_eq!(rd_after.pos(), before);
                    }
                }
            }
        }
    });
}

#[test]
fn shared_last_byte() {
    // The reverse length region and the data meet: lists that end right where the lengths begin, with one and
    // with two length nibbles taken from the back (the second list's count sits in the high nibble).
    for_each_plain!(|T| {
        let size = size_of::<T>();
        for (a, b) in [(0, 0), (1, 0), (0, 1), (2, 3), (7, 7), (9, 1), (70, 600)] {
            let mut wr = BufWriterOwned::new();
            wr.write_rev_len(a).unwrap();
            for i in 0..a * size {
                wr.write_u8(i as u8).unwrap();
            }
            wr.write_rev_len(b).unwrap();
            for i in 0..b * size {
                wr.write_u8(i as u8).unwrap();
            }
            let full = wr.finish_and_take().unwrap();
            for cut in 0..=full.len().min(3 * size) {
                let buf = &full[cut..];
                let rd = BufReader::new(buf);
                let what = format!("lists {a} + {b}, {cut} short");
                let first = check::<T>(rd, &what);
                if let Ok(count) = first {
                    // go on from where the first list ended, as a struct with two lists would
                    let mut rd = rd;
                    let _: RefVec<'_, T> = rd.read().unwrap();
                    let second = check::<T>(rd, &what);
                    if cut == 0 {
                        assert_eq!((count, second), (a, Ok(b)), "{what}");
                    }
                } else {
                    assert_ne!(cut, 0, "{what}");
                }
            }
        }
    });
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), sized)]
struct Byte {
    value: u8,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), ww_repr = u8, sized)]
enum Kind {
    A,
    B,
}

#[test]
fn out_of_scope_types_still_validate_at_decode_time() {
    // an enum of one byte looks like a u8 on the wire, but 7 is not a variant: rejected when the list is decoded
    let buf = [0, 1, 7, 3];
    assert!(RefVec::<Kind>::from_ww_bytes(&buf).is_err());
    assert_eq!(RefVec::<u8>::from_ww_bytes(&buf).unwrap().len(), 3);
    // a sized struct of one u8 goes through the pass too, with the same result as the bytes
    let list = RefVec::<Byte>::from_ww_bytes(&buf).unwrap();
    let values: Vec<u8> = list.iter().map(|b| b.unwrap().value).collect();
    assert_eq!(values, [0, 1, 7]);
    assert_eq!(
        RefVec::<Byte>::from_ww_bytes(&[0, 1, 3]),
        Err(Error::OutOfBoundsReadU8)
    );
}

macro_rules! in_struct {
    ($name:ident, $T:ident) => {
        mod $name {
            use super::*;

            #[derive_shrink_wrap(borrowed, derive(Clone, Copy))]
            pub struct Skip<'i> {
                pub flag: bool,
                pub list: RefVec<'i, $T>,
                pub after: u8,
                pub more: RefVec<'i, $T>,
                pub last: bool,
            }

            #[derive_shrink_wrap(borrowed, derive(Clone, Copy))]
            pub struct Pass<'i> {
                pub flag: bool,
                pub list: PassVec<'i, $T>,
                pub after: u8,
                pub more: PassVec<'i, $T>,
                pub last: bool,
            }

            /// (flag, list start and count, after, more start and count, last) or the error, for both structs.
            type Fields<'i> = (
                bool,
                (BufReader<'i>, usize),
                u8,
                (BufReader<'i>, usize),
                bool,
            );

            pub fn both<'i>(
                rd: BufReader<'i>,
            ) -> (
                Result<Fields<'i>, Error>,
                BufReader<'i>,
                Result<Fields<'i>, Error>,
                BufReader<'i>,
            ) {
                let mut rd_skip = rd;
                let mut rd_pass = rd;
                let skip = rd_skip
                    .read::<Skip<'i>>()
                    .map(|s| (s.flag, parts(s.list), s.after, parts(s.more), s.last));
                let pass = rd_pass
                    .read::<Pass<'i>>()
                    .map(|s| (s.flag, parts(s.list.0), s.after, parts(s.more.0), s.last));
                (skip, rd_skip, pass, rd_pass)
            }

            pub fn valid(count: usize, more: usize) -> Vec<u8> {
                let list: Vec<$T> = (0..count).map(|i| i as $T).collect();
                let more: Vec<$T> = (0..more).map(|i| (i * 3) as $T).collect();
                let value = Skip {
                    flag: true,
                    list: RefVec::Slice { slice: &list },
                    after: 0x5A,
                    more: RefVec::Slice { slice: &more },
                    last: true,
                };
                // as a field of a parent: with its size at the back, garbage can follow
                let mut buf = vec![0u8; 16 + (count + more.len()) * size_of::<$T>()];
                let mut wr = BufWriter::new(&mut buf);
                wr.write(&value).unwrap();
                wr.finish_and_take().unwrap().to_vec()
            }
        }
    };
}

in_struct!(st_u8, u8);
in_struct!(st_u16, u16);
in_struct!(st_u32, u32);
in_struct!(st_u64, u64);
in_struct!(st_u128, u128);
in_struct!(st_i8, i8);
in_struct!(st_i16, i16);
in_struct!(st_i32, i32);
in_struct!(st_i64, i64);
in_struct!(st_i128, i128);
in_struct!(st_f32, f32);
in_struct!(st_f64, f64);

macro_rules! for_each_struct {
    (|$m:ident| $body:block) => {
        for_each_struct!(@mods $m $body; st_u8, st_u16, st_u32, st_u64, st_u128, st_i8, st_i16, st_i32, st_i64,
            st_i128, st_f32, st_f64);
    };
    (@mods $m:ident $body:block; $($name:ident),*) => {
        $({
            use $name as $m;
            $body
        })*
    };
}

#[test]
fn inside_a_struct() {
    for_each_struct!(|m| {
        for (count, more) in [(0, 0), (1, 0), (0, 1), (3, 2), (9, 70)] {
            let valid = m::valid(count, more);
            let (skip, rd_skip, pass, rd_pass) = m::both(BufReader::new(&valid));
            assert_eq!(skip, pass);
            assert_eq!(rd_skip, rd_pass);
            let fields = skip.unwrap();
            assert_eq!((fields.0, fields.1.1, fields.2), (true, count, 0x5A));
            assert_eq!((fields.3.1, fields.4), (more, true));

            // garbage after the value: the struct is a field of something larger
            let mut wr = BufWriterOwned::new();
            wr.write_rev_len(valid.len()).unwrap();
            wr.write_raw_slice(&valid).unwrap();
            wr.write_raw_slice(&[0xFF, 0x00, 0xA5, 0x77]).unwrap();
            let outer = wr.finish_and_take().unwrap();
            let mut rd = BufReader::new(&outer);
            let size = rd.read_rev_len().unwrap();
            let mut inner = rd.split(size).unwrap();
            let skip_alone = inner.read::<m::Skip<'_>>().unwrap();
            assert_eq!(skip_alone.list.len(), count);
            assert_eq!(skip_alone.after, 0x5A);
            assert_eq!(rd.read_raw_slice(4), Ok(&[0xFF, 0x00, 0xA5, 0x77][..]));

            // every byte of the message damaged in turn, and every truncation from both ends
            for i in 0..valid.len() {
                for xor in [0x01, 0x10, 0x80, 0xFF] {
                    let mut buf = valid.clone();
                    buf[i] ^= xor;
                    let (skip, rd_skip, pass, rd_pass) = m::both(BufReader::new(&buf));
                    assert_eq!(skip, pass, "byte {i} ^ {xor:#x}");
                    assert_eq!(rd_skip, rd_pass, "byte {i} ^ {xor:#x}");
                }
                for buf in [&valid[i..], &valid[..i]] {
                    let (skip, rd_skip, pass, rd_pass) = m::both(BufReader::new(buf));
                    assert_eq!(skip, pass, "cut at {i}");
                    assert_eq!(rd_skip, rd_pass, "cut at {i}");
                }
            }
        }
    });
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
    let mut rng = Rng(0x5357_3337_0000_0001);
    let mut decoded = 0;
    for round in 0..40_000 {
        let len = rng.below(if round % 8 == 0 { 400 } else { 40 });
        let mut buf: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        // random lengths are large more often than not: make half of them small, so lists decode too
        if round % 2 == 0 {
            for byte in buf.iter_mut().rev().take(2) {
                *byte &= 0x77;
            }
        }
        let rd = random_reader(&mut rng, &buf);
        for_each_plain!(|T| {
            if check::<T>(rd, &format!("round {round}, {buf:02x?}")).is_ok() {
                decoded += 1;
            }
        });
        for_each_struct!(|m| {
            let (skip, rd_skip, pass, rd_pass) = m::both(rd);
            assert_eq!(skip, pass, "round {round}, {buf:02x?}");
            assert_eq!(rd_skip, rd_pass, "round {round}, {buf:02x?}");
        });
    }
    // the loop is worth something only if both outcomes happen
    assert!(decoded > 50_000, "{decoded} lists decoded");
}
