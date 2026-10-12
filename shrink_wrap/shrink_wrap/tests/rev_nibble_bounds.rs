//! The last byte of a buffer can hold forward data in its high nibble and a length (read from the back) in its
//! low one. No read may take bits of the other side (SW-40): a forward read stops where the lengths begin, a
//! length is not read from bits the forward side has read past. Exhaustive per bit position in
//! `buf_reader.rs` (`tests::bounds_*`); here the public API and the round trip of what the writer produces.

use shrink_wrap::prelude::*;
use shrink_wrap::{BufReader, BufWriter, BufWriterOwned, Error};

/// The case that was found: 4 forward bits are left, a 6 bit read took 2 bits of the length.
#[test]
fn un_read_stops_at_the_length_nibble() {
    let buf = [0xA1];
    let mut rd = BufReader::new(&buf);
    assert_eq!(rd.read_rev_len(), Ok(1));
    assert_eq!(rd.bits_left(), 4);
    assert_eq!(rd.read_un8(6), Err(Error::OutOfBoundsReadUN));

    let mut rd = BufReader::new(&buf);
    rd.read_rev_len().unwrap();
    assert_eq!(rd.read_un8_latch(6), 0);
    assert_eq!(rd.latched(), Err(Error::OutOfBoundsReadUN));

    let mut rd = BufReader::new(&buf);
    rd.read_rev_len().unwrap();
    assert_eq!(rd.read::<U6>(), Err(Error::OutOfBoundsReadUN));

    // the 4 bits that are there can be read
    let mut rd = BufReader::new(&buf);
    rd.read_rev_len().unwrap();
    assert_eq!(rd.read_un8(4), Ok(0xA));
    assert_eq!(rd.read_bool(), Err(Error::OutOfBoundsReadBool));
}

/// The mirror case: the forward side read 6 bits of the byte, a length cannot be in its low nibble.
#[test]
fn length_is_not_read_from_forward_bits() {
    let buf = [0xA1];
    let mut rd = BufReader::new(&buf);
    assert_eq!(rd.read_un8(6), Ok(0b101000));
    assert_eq!(rd.read_rev_len(), Err(Error::OutOfBoundsRev));

    // 4 forward bits leave the low nibble to the length
    let mut rd = BufReader::new(&buf);
    assert_eq!(rd.read_un8(4), Ok(0xA));
    assert_eq!(rd.read_rev_len(), Ok(1));

    // the high nibble of the last byte as the second nibble of a length, after a forward read into that byte
    let buf = [0x00, 0x98];
    let mut rd = BufReader::new(&buf);
    rd.read_un8(8).unwrap();
    rd.read_bool().unwrap();
    assert_eq!(rd.read_rev_len(), Err(Error::OutOfBoundsRev));
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

    fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 33) % n
    }
}

/// One value of a message, written and read with the matching `BufWriter` / `BufReader` methods.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    Bool(bool),
    Un(u8, u64),
    Nib(u8),
    U8(u8),
    U16(u16),
    U32(u32),
    UNib32(u32),
    Bytes(Vec<u8>),
}

impl Item {
    fn random(rng: &mut Rng) -> Item {
        match rng.below(8) {
            0 => Item::Bool(rng.below(2) == 1),
            1 => {
                let bits = 1 + rng.below(64) as u8;
                Item::Un(bits, rng.next() >> (64 - bits))
            }
            2 => Item::Nib(rng.below(16) as u8),
            3 => Item::U8(rng.next() as u8),
            4 => Item::U16(rng.next() as u16),
            5 => Item::U32(rng.next() as u32),
            6 => Item::UNib32(rng.next() as u32 >> rng.below(32)),
            // lengths of 1 to 4 nibbles
            _ => Item::Bytes(
                (0..[0, 1, 7, 8, 63, 64, 300][rng.below(7) as usize])
                    .map(|i| i as u8)
                    .collect(),
            ),
        }
    }

    fn write(&self, wr: &mut BufWriter) -> Result<(), Error> {
        match self {
            Item::Bool(v) => wr.write_bool(*v),
            Item::Un(bits, v) => wr.write_un64(*bits, *v),
            Item::Nib(v) => wr.write_nib(Nibble::new_masked(*v)),
            Item::U8(v) => wr.write_u8(*v),
            Item::U16(v) => wr.write_u16(*v),
            Item::U32(v) => wr.write_u32(*v),
            Item::UNib32(v) => wr.write_unib32(*v),
            Item::Bytes(v) => wr.write_bytes(v),
        }
    }

    fn read(&self, rd: &mut BufReader) -> Result<Item, Error> {
        Ok(match self {
            Item::Bool(_) => Item::Bool(rd.read_bool()?),
            Item::Un(bits, _) => Item::Un(*bits, rd.read_un64(*bits)?),
            Item::Nib(_) => Item::Nib(rd.read_nib()?.value()),
            Item::U8(_) => Item::U8(rd.read_u8()?),
            Item::U16(_) => Item::U16(rd.read_u16()?),
            Item::U32(_) => Item::U32(rd.read_u32()?),
            Item::UNib32(_) => Item::UNib32(rd.read_unib32()?),
            Item::Bytes(_) => Item::Bytes(rd.read_bytes()?.to_vec()),
        })
    }
}

/// Whatever the writer produces is read back, every value as written: the bounds do not reject a single
/// valid message. And into a buffer of any size the writer either fails or writes the same bytes, so it never
/// writes over the lengths it keeps at the end of the buffer (the mirror case on the writer side).
#[test]
fn every_written_message_reads_back() {
    let mut rng = Rng(0x5357_3430_0000_0001);
    let mut big = vec![0u8; 4096];
    let mut small = vec![0u8; 4096];
    for round in 0..5_000 {
        let items: Vec<Item> = (0..1 + rng.below(12))
            .map(|_| Item::random(&mut rng))
            .collect();
        let mut wr = BufWriter::new(&mut big);
        for item in &items {
            item.write(&mut wr).unwrap();
        }
        let bytes = wr.finish().unwrap().to_vec();

        let mut rd = BufReader::new(&bytes);
        for item in &items {
            assert_eq!(
                item.read(&mut rd).as_ref(),
                Ok(item),
                "round {round}: {items:?}, {bytes:02x?}"
            );
        }

        for size in 0..bytes.len() + 3 {
            // the buffer holds old data: nothing may depend on it
            small[..size].fill(rng.next() as u8);
            let mut wr = BufWriter::new(&mut small[..size]);
            let written = items
                .iter()
                .try_for_each(|item| item.write(&mut wr))
                .and_then(|_| wr.finish().map(|b| b.to_vec()));
            if let Ok(written) = written {
                assert_eq!(
                    written, bytes,
                    "round {round}, into {size} bytes: {items:?}"
                );
            }
        }
    }
}

/// The owned writer produces the same bytes, read back the same way.
#[test]
fn owned_writer_messages_read_back() {
    let mut rng = Rng(0x5357_3430_0000_0002);
    for _ in 0..2_000 {
        let items: Vec<Item> = (0..1 + rng.below(12))
            .map(|_| Item::random(&mut rng))
            .collect();
        let mut wr = BufWriterOwned::new();
        for item in &items {
            match item {
                Item::Bool(v) => wr.write_bool(*v).unwrap(),
                Item::Un(bits, v) => wr.write_un64(*bits, *v).unwrap(),
                Item::Nib(v) => wr.write_nib(Nibble::new_masked(*v)).unwrap(),
                Item::U8(v) => wr.write_u8(*v).unwrap(),
                Item::U16(v) => wr.write_u16(*v).unwrap(),
                Item::U32(v) => wr.write_u32(*v).unwrap(),
                Item::UNib32(v) => wr.write_unib32(*v).unwrap(),
                Item::Bytes(v) => wr.write_bytes(v).unwrap(),
            }
        }
        let bytes = wr.finish_and_take().unwrap();
        let mut rd = BufReader::new(&bytes);
        for item in &items {
            assert_eq!(
                item.read(&mut rd).as_ref(),
                Ok(item),
                "{items:?}, {bytes:02x?}"
            );
        }
    }
}
