//! [Series] (SW-60): a list of `T` written column by column, one column per leaf field, each with its own
//! predictor and coder, discrete leaves as runs of changes; see [SeriesColumns] for the types it takes. Numeric
//! columns use the SW-31 prefix code (plus packed/constant/plain), float columns the SW-32 XOR code (Gorilla),
//! both bit-level codes internal to this module and its `columns` submodule.

use crate::{BufReader, BufWriter, Error, SerializeShrinkWrap};

pub mod bound;
mod cells;
#[cfg(all(test, feature = "std"))]
mod cells_tests;
mod columns;
/// Round trips of the column codecs for the `series-columns` fuzz target.
#[cfg(fuzzing)]
#[doc(hidden)]
pub use columns::check as fuzz_columns;
pub mod dynamic;
#[cfg(test)]
mod golden_signals;
mod group;
mod leaves;
mod series_type;
mod store;
#[cfg(all(test, feature = "std"))]
mod store_tests;
#[cfg(all(test, feature = "std"))]
mod store_write_tests;
#[cfg(all(feature = "std", any(test, fuzzing)))]
#[doc(hidden)]
pub mod type_check;
/// Round trips and random bytes of `Series<T>` for the `series-type` fuzz target.
#[cfg(all(feature = "std", fuzzing))]
#[doc(hidden)]
pub use type_check as fuzz_type;
#[cfg(test)]
mod type_tests;
mod xor_float;

pub use cells::{CellWord, SeriesCells, UnCells, empty_cells};
pub use columns::{
    BitCounter, CodecPolicy, Coder, DeltaPacked, FixedCodecs, FloatCodec, NumericCodec,
    PACKED_BLOCK, Predictor, RawPlain, SeriesPolicy,
};
pub use group::{
    EnumTags, FieldItems, Level, OpaqueField, OpaqueReader, RootCtx, SeriesColumns, SeriesCtx,
    SiblingCtx, SiblingTags, Slot, StructExtent, TagRun, VariantCtx, changed_before, no_earlier,
    skim_struct_head, tag_changed, write_enum_tags, write_struct_head,
};
pub use leaves::{ArrayReader, OptionReader};
#[cfg(feature = "std")]
pub use series_type::SeriesOwned;
pub use series_type::{SERIES_MAX_ITEMS, Series, SeriesIter, SeriesTryIter};
pub use store::{
    SeriesStore, StoreWord, empty_store_from_init, fill_array, init_store_at, init_store_in,
    write_float_column, write_numeric_column, write_numeric128_column, write_store_series,
};

pub use xor_float::XorBits;

/// Bit-level writes shared by [BufWriter], `BufWriterOwned` and [BitCounter], so a codec is written once. The
/// writers of [SeriesColumns] groups are generic over it: the same code writes and counts.
pub trait BitSink {
    /// Bits written so far, from the start of the buffer (only its position in a byte matters to a codec).
    fn bit_pos(&self) -> usize;
    fn put_bit(&mut self, bit: bool) -> Result<(), Error>;
    /// `count` low bits of `value`, 0 to 64.
    fn put_bits(&mut self, count: u8, value: u64) -> Result<(), Error>;
    /// Zero bits up to the next byte boundary of the buffer.
    fn align_byte(&mut self);
    /// Whole bytes, after aligning to a byte.
    fn put_bytes(&mut self, bytes: &[u8]) -> Result<(), Error>;

    /// An opaque run value of `value`'s own bytes: `len: nib` of what [to_ww_bytes](SerializeShrinkWrap::to_ww_bytes)
    /// gives, zero bits to the next byte, then those bytes. Serialized into the writer's free space first, so
    /// nothing is allocated (`BufWriterOwned` and a [BitCounter] without [scratch](Self::scratch) use a `Vec`).
    fn put_serialized<V: SerializeShrinkWrap + ?Sized>(&mut self, value: &V) -> Result<(), Error>;

    /// Free memory after what is written, for a [BitCounter] to serialize opaque values into while it measures a
    /// group ([BitCounter::with_scratch]); its contents are not kept.
    fn scratch(&mut self) -> &mut [u8];
}

/// `len: nib`, zero bits to the next byte, the bytes: an opaque run value.
#[cfg(feature = "std")]
pub(crate) fn put_opaque_value(sink: &mut impl BitSink, bytes: &[u8]) -> Result<(), Error> {
    columns::put_nib(
        sink,
        u32::try_from(bytes.len()).map_err(|_| Error::LenTooLong)?,
    )?;
    sink.put_bytes(bytes)
}

/// `value`'s `to_ww_bytes` in a `Vec`, for writers without a buffer of their own: grows until it fits.
#[cfg(feature = "std")]
pub(crate) fn serialize_to_vec<V: SerializeShrinkWrap + ?Sized>(
    value: &V,
) -> Result<Vec<u8>, Error> {
    let mut buf = vec![0u8; 256];
    loop {
        match value.to_ww_bytes(&mut buf).map(|bytes| bytes.len()) {
            Ok(len) => {
                buf.truncate(len);
                return Ok(buf);
            }
            // the largest value a `BufWriter` can hold: lengths are 16 bits, nested ones too
            Err(e) if is_out_of_space(e) && buf.len() < 1 << 24 => buf.resize(buf.len() * 4, 0),
            Err(e) => return Err(e),
        }
    }
}

/// Errors a `BufWriter` gives when its buffer is too small.
#[cfg(feature = "std")]
fn is_out_of_space(e: Error) -> bool {
    matches!(
        e,
        Error::OutOfBoundsWriteBool
            | Error::OutOfBoundsWriteU4
            | Error::OutOfBoundsWriteU8
            | Error::OutOfBoundsWriteRawSlice
            | Error::OutOfBoundsWriteUN
            | Error::OutOfBoundsRev
            | Error::OutOfBoundsRevCompact
    )
}

impl BitSink for BufWriter<'_> {
    fn bit_pos(&self) -> usize {
        let (byte, bit) = self.pos();
        byte * 8 + 7 - bit as usize
    }

    fn put_bit(&mut self, bit: bool) -> Result<(), Error> {
        self.write_bool(bit)
    }

    fn put_bits(&mut self, count: u8, value: u64) -> Result<(), Error> {
        self.write_un64(count, value)
    }

    fn align_byte(&mut self) {
        self.align_byte()
    }

    fn put_bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.write_raw_slice(bytes)
    }

    fn put_serialized<V: SerializeShrinkWrap + ?Sized>(&mut self, value: &V) -> Result<(), Error> {
        // serialized past the room `len: nib` (at most 44 bits) and the padding can take, then moved in place
        let at = self.scratch_start();
        let len = value.to_ww_bytes(self.scratch())?.len();
        columns::put_nib(self, u32::try_from(len).map_err(|_| Error::LenTooLong)?)?;
        self.align_byte();
        self.move_in(at, len)
    }

    fn scratch(&mut self) -> &mut [u8] {
        let at = self.scratch_start();
        self.free_from(at)
    }
}

impl BitSink for crate::buf_writer::BitWriter<'_, '_> {
    fn bit_pos(&self) -> usize {
        self.bit_pos()
    }

    #[inline]
    fn put_bit(&mut self, bit: bool) -> Result<(), Error> {
        self.put_bit(bit)
    }

    #[inline]
    fn put_bits(&mut self, count: u8, value: u64) -> Result<(), Error> {
        self.put_bits(count, value)
    }

    fn align_byte(&mut self) {
        self.align_byte()
    }

    fn put_bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.with_writer(|wr| wr.write_raw_slice(bytes))
    }

    fn put_serialized<V: SerializeShrinkWrap + ?Sized>(&mut self, value: &V) -> Result<(), Error> {
        self.with_writer(|wr| BitSink::put_serialized(wr, value))
    }

    fn scratch(&mut self) -> &mut [u8] {
        BitSink::scratch(self.writer_after_open_byte())
    }
}

#[cfg(feature = "std")]
impl BitSink for crate::BufWriterOwned {
    fn bit_pos(&self) -> usize {
        let (byte, bit) = self.pos();
        byte * 8 + 7 - bit as usize
    }

    fn put_bit(&mut self, bit: bool) -> Result<(), Error> {
        self.write_bool(bit)
    }

    fn put_bits(&mut self, count: u8, value: u64) -> Result<(), Error> {
        self.write_un64(count, value)
    }

    fn align_byte(&mut self) {
        self.align_byte()
    }

    fn put_bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.write_raw_slice(bytes)
    }

    fn put_serialized<V: SerializeShrinkWrap + ?Sized>(&mut self, value: &V) -> Result<(), Error> {
        put_opaque_value(self, &serialize_to_vec(value)?)
    }

    fn scratch(&mut self) -> &mut [u8] {
        &mut []
    }
}

/// One element codec: the same state drives encoding and decoding.
pub(crate) trait Codec<T>: Default {
    fn encode(&mut self, value: T, sink: &mut impl BitSink) -> Result<(), Error>;
    fn decode(&mut self, rd: &mut BufReader<'_>) -> Result<T, Error>;
}

/// Zigzag: small negative and positive numbers become small unsigned ones. Only [unzigzag] is needed outside
/// tests: every residual a column writes already starts as a zigzag value of the right width (the numeric
/// column's own `Word::zigzag`).
#[cfg(test)]
#[inline]
const fn zigzag(d: i64) -> u64 {
    ((d << 1) ^ (d >> 63)) as u64
}

#[inline]
pub(crate) const fn unzigzag(z: u64) -> i64 {
    ((z >> 1) as i64) ^ -((z & 1) as i64)
}

/// Bucket widths of the delta code before the full width of the type.
const BUCKETS: [u8; 5] = [7, 9, 12, 20, 32];

/// The delta code for a `width`-bit type: `0` for a zero; otherwise `1` per bucket skipped, then `0` and the
/// zigzag value in the bucket's width (7, 9, 12, 20 or 32 bits, buckets at or above the type's width are left
/// out), or, after a `1` for every bucket, the value at the full width with no `0` in between.
pub(crate) fn put_delta(sink: &mut impl BitSink, z: u64, width: u8) -> Result<(), Error> {
    if z == 0 {
        return sink.put_bit(false);
    }
    sink.put_bit(true)?;
    for &bucket in BUCKETS.iter().filter(|&&b| b < width) {
        if z < (1u64 << bucket) {
            sink.put_bit(false)?;
            return sink.put_bits(bucket, z);
        }
        sink.put_bit(true)?;
    }
    sink.put_bits(width, z)
}

/// Bits [put_delta] writes for a value that needs `needs` bits (0 for 0) at `width`.
pub(crate) fn delta_bits(needs: u8, width: u8) -> usize {
    if needs == 0 {
        return 1;
    }
    let mut bits = 1;
    for &bucket in BUCKETS.iter().filter(|&&b| b < width) {
        if needs <= bucket {
            return bits + 1 + bucket as usize;
        }
        bits += 1;
    }
    bits + width as usize
}

pub(crate) fn get_delta(rd: &mut BufReader<'_>, width: u8) -> Result<u64, Error> {
    if !rd.read_bool()? {
        return Ok(0);
    }
    for &bucket in BUCKETS.iter().filter(|&&b| b < width) {
        if !rd.read_bool()? {
            return rd.read_un64(bucket);
        }
    }
    rd.read_un64(width)
}

/// `width` low bits set.
#[inline]
pub(crate) const fn mask(width: u8) -> u64 {
    if width >= 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    }
}

/// `bits` as a `width`-bit two's complement number.
#[inline]
pub(crate) const fn sign_extend(bits: u64, width: u8) -> i64 {
    let shift = 64 - width as u32;
    ((bits << shift) as i64) >> shift
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same writes through a `BufWriter` and through a `BitWriter` on it give the same bytes (the buffer's
    /// old contents included), positions and errors, from any start in a byte, up to a full buffer.
    #[test]
    fn bit_writer_matches_buf_writer() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut rand = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..300 {
            let len = 1 + (rand() % 24) as usize;
            let mut fill = [0u8; 24];
            fill.iter_mut().for_each(|b| *b = rand() as u8);
            let (mut a, mut b) = (fill, fill);
            let mut plain = BufWriter::new(&mut a[..len]);
            let mut wr = BufWriter::new(&mut b[..len]);
            let lead = (rand() % 8) as u8;
            plain.write_un8(lead, 0x5A).unwrap();
            wr.write_un8(lead, 0x5A).unwrap();
            {
                let mut bits = crate::buf_writer::BitWriter::new(&mut wr);
                for _ in 0..12 {
                    let value = rand();
                    let (x, y) = match rand() % 6 {
                        0 => (plain.put_bit(value & 1 == 1), bits.put_bit(value & 1 == 1)),
                        1 => {
                            BitSink::align_byte(&mut plain);
                            BitSink::align_byte(&mut bits);
                            (Ok(()), Ok(()))
                        }
                        2 => {
                            let bytes = [value as u8, (value >> 8) as u8];
                            let n = (value >> 16) as usize % 3;
                            (plain.put_bytes(&bytes[..n]), bits.put_bytes(&bytes[..n]))
                        }
                        _ => {
                            let count = (rand() % 65) as u8;
                            (plain.put_bits(count, value), bits.put_bits(count, value))
                        }
                    };
                    assert_eq!(x, y, "round {round}");
                    assert_eq!(
                        BitSink::bit_pos(&plain),
                        BitSink::bit_pos(&bits),
                        "round {round}"
                    );
                    if x.is_err() {
                        break;
                    }
                }
            }
            assert_eq!(plain.pos(), wr.pos(), "round {round}");
            assert_eq!(a, b, "round {round}");
        }
    }

    #[test]
    fn zigzag_round_trip() {
        for d in [0i64, 1, -1, 2, -2, 63, -63, 64, -64, i64::MAX, i64::MIN] {
            assert_eq!(unzigzag(zigzag(d)), d);
        }
        assert_eq!(zigzag(0), 0);
        assert_eq!(zigzag(-1), 1);
        assert_eq!(zigzag(1), 2);
        assert_eq!(zigzag(-64), 127);
        assert_eq!(zigzag(64), 128);
    }

    #[test]
    fn sign_extend_widths() {
        assert_eq!(sign_extend(0xFF, 8), -1);
        assert_eq!(sign_extend(0x7F, 8), 127);
        assert_eq!(sign_extend(0x80, 8), -128);
        assert_eq!(sign_extend(u64::MAX, 64), -1);
        assert_eq!(sign_extend(1, 64), 1);
        assert_eq!(mask(8), 0xFF);
        assert_eq!(mask(64), u64::MAX);
    }

    #[test]
    fn delta_code_bits() {
        // (zigzag value, width) -> bits written
        let cases: [(u64, u8, &str); 9] = [
            (0, 32, "0"),
            (1, 32, "10 0000001"),
            (127, 32, "10 1111111"),
            (128, 32, "110 010000000"),
            (511, 32, "110 111111111"),
            (512, 32, "1110 001000000000"),
            (4096, 32, "11110 00000001000000000000"),
            (1 << 20, 32, "11111 00000000000100000000000000000000"),
            (200, 8, "11 11001000"), // u8: the 7-bit bucket, then the full 8 bits
        ];
        for (z, width, expected) in cases {
            let mut buf = [0u8; 16];
            let mut wr = BufWriter::new(&mut buf);
            put_delta(&mut wr, z, width).unwrap();
            let (byte, bit) = wr.pos();
            let bits_written = byte * 8 + (7 - bit as usize);
            let bytes = wr.finish().unwrap();
            let mut s = String::new();
            for i in 0..bits_written {
                s.push(if bytes[i / 8] & (0x80 >> (i % 8)) != 0 {
                    '1'
                } else {
                    '0'
                });
            }
            assert_eq!(s, expected.replace(' ', ""), "z = {z}, width = {width}");
            let needs = (64 - z.leading_zeros()) as u8;
            assert_eq!(
                delta_bits(needs, width),
                bits_written,
                "z = {z}, width = {width}"
            );
            let mut rd = BufReader::new(bytes);
            assert_eq!(get_delta(&mut rd, width).unwrap(), z);
        }
    }
}
