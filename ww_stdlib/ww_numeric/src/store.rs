//! [NumericValueStore]: the `SeriesStore` of [NumericValue], by hand (SW-62).

use crate::NumericValue;
use core::ops::Range;
use shrink_wrap::Error;
use shrink_wrap::series::{
    BitSink, Level, SeriesPolicy, SeriesStore, write_enum_tags, write_float_column,
    write_numeric_column, write_numeric128_column,
};

/// The payload of every variant enabled, at the width of the widest one.
#[cfg(any(feature = "u128", feature = "i128"))]
type Payload = u128;
#[cfg(all(
    not(any(feature = "u128", feature = "i128")),
    any(
        feature = "u64",
        feature = "i64",
        feature = "f64",
        feature = "uvlq64",
        feature = "un",
        feature = "in"
    )
))]
type Payload = u64;
#[cfg(all(
    not(any(
        feature = "u128",
        feature = "i128",
        feature = "u64",
        feature = "i64",
        feature = "f64",
        feature = "uvlq64",
        feature = "un",
        feature = "in"
    )),
    any(
        feature = "u32",
        feature = "i32",
        feature = "f32",
        feature = "unib32",
        feature = "uvlq32"
    )
))]
type Payload = u32;
#[cfg(all(
    not(any(
        feature = "u128",
        feature = "i128",
        feature = "u64",
        feature = "i64",
        feature = "f64",
        feature = "uvlq64",
        feature = "un",
        feature = "in",
        feature = "u32",
        feature = "i32",
        feature = "f32",
        feature = "unib32",
        feature = "uvlq32"
    )),
    any(feature = "u16", feature = "i16")
))]
type Payload = u16;
#[cfg(not(any(
    feature = "u128",
    feature = "i128",
    feature = "u64",
    feature = "i64",
    feature = "f64",
    feature = "uvlq64",
    feature = "un",
    feature = "in",
    feature = "u32",
    feature = "i32",
    feature = "f32",
    feature = "unib32",
    feature = "uvlq32",
    feature = "u16",
    feature = "i16"
)))]
type Payload = u8;

/// `N` [NumericValue]s as a struct of arrays: which variant each one is (a byte), and its number in one array as
/// wide as the widest variant enabled (`U8`, `U32`, `I32`, `F32`: 4 bytes, so 5 bytes per value where the enum
/// takes 8; `u64` or `UN` / `IN`: 8 bytes, `u128`: 16). Signed numbers and floats are kept as their bits. `UN` and
/// `IN` add a byte per value for their width and bit count.
pub struct NumericValueStore<const N: usize> {
    variant: [u8; N],
    payload: [Payload; N],
    /// `UN` / `IN`: the width variant (0 to 3) in the two high bits, the bit count below.
    #[cfg(any(feature = "un", feature = "in"))]
    width: [u8; N],
}

// the store's own numbering of the variants, independent of the wire's discriminants; some are compiled out
#[allow(dead_code)]
mod variant {
    pub(super) const NIBBLE: u8 = 0;
    pub(super) const U8: u8 = 1;
    pub(super) const U16: u8 = 2;
    pub(super) const U32: u8 = 3;
    pub(super) const UNIB32: u8 = 4;
    pub(super) const U64: u8 = 5;
    pub(super) const I32: u8 = 6;
    pub(super) const F32: u8 = 7;
    pub(super) const UN: u8 = 8;
    pub(super) const IN: u8 = 9;
    pub(super) const U128: u8 = 10;
    pub(super) const I8: u8 = 11;
    pub(super) const I16: u8 = 12;
    pub(super) const I64: u8 = 13;
    pub(super) const I128: u8 = 14;
    pub(super) const F64: u8 = 15;
    pub(super) const UVLQ32: u8 = 16;
    pub(super) const UVLQ64: u8 = 17;
}
use variant::*;

#[cfg(any(feature = "un", feature = "in"))]
const fn width_byte(width: u8, bit_count: u8) -> u8 {
    width << 6 | bit_count
}

impl SeriesStore for NumericValue {
    type Store<const N: usize> = NumericValueStore<N>;

    fn empty_store<const N: usize>() -> NumericValueStore<N> {
        NumericValueStore {
            variant: [U8; N],
            payload: [0; N],
            #[cfg(any(feature = "un", feature = "in"))]
            width: [0; N],
        }
    }

    #[allow(clippy::unnecessary_cast)]
    fn set<const N: usize>(store: &mut NumericValueStore<N>, i: usize, item: &Self) {
        let (variant, payload) = match *item {
            #[cfg(feature = "nibble")]
            NumericValue::Nibble(x) => (NIBBLE, x.value() as Payload),
            NumericValue::U8(x) => (U8, x as Payload),
            #[cfg(feature = "u16")]
            NumericValue::U16(x) => (U16, x as Payload),
            #[cfg(feature = "u32")]
            NumericValue::U32(x) => (U32, x as Payload),
            #[cfg(feature = "unib32")]
            NumericValue::UNib32(x) => (UNIB32, x.0 as Payload),
            #[cfg(feature = "u64")]
            NumericValue::U64(x) => (U64, x as Payload),
            #[cfg(feature = "i32")]
            NumericValue::I32(x) => (I32, x as u32 as Payload),
            #[cfg(feature = "f32")]
            NumericValue::F32(x) => (F32, x.to_bits() as Payload),
            #[cfg(feature = "un")]
            NumericValue::UN(x) => {
                use shrink_wrap::un::UN::*;
                let (width, bits, value) = match x {
                    UN8 { bit_count, value } => (0, bit_count.value(), value as u64),
                    UN16 { bit_count, value } => (1, bit_count.value(), value as u64),
                    UN32 { bit_count, value } => (2, bit_count.value(), value as u64),
                    UN64 { bit_count, value } => (3, bit_count.value(), value),
                };
                store.width[i] = width_byte(width, bits);
                (UN, value as Payload)
            }
            #[cfg(feature = "in")]
            NumericValue::IN(x) => {
                use shrink_wrap::un::IN::*;
                let (width, bits, value) = match x {
                    IN8 { bit_count, value } => (0, bit_count.value(), value as u8 as u64),
                    IN16 { bit_count, value } => (1, bit_count.value(), value as u16 as u64),
                    IN32 { bit_count, value } => (2, bit_count.value(), value as u32 as u64),
                    IN64 { bit_count, value } => (3, bit_count.value(), value as u64),
                };
                store.width[i] = width_byte(width, bits);
                (IN, value as Payload)
            }
            #[cfg(feature = "u128")]
            NumericValue::U128(x) => (U128, x as Payload),
            #[cfg(feature = "i8")]
            NumericValue::I8(x) => (I8, x as u8 as Payload),
            #[cfg(feature = "i16")]
            NumericValue::I16(x) => (I16, x as u16 as Payload),
            #[cfg(feature = "i64")]
            NumericValue::I64(x) => (I64, x as u64 as Payload),
            #[cfg(feature = "i128")]
            NumericValue::I128(x) => (I128, x as u128 as Payload),
            #[cfg(feature = "f64")]
            NumericValue::F64(x) => (F64, x.to_bits() as Payload),
            #[cfg(feature = "uvlq32")]
            NumericValue::UVlq32(x) => (UVLQ32, x.0 as Payload),
            #[cfg(feature = "uvlq64")]
            NumericValue::UVlq64(x) => (UVLQ64, x.0 as Payload),
        };
        store.variant[i] = variant;
        store.payload[i] = payload;
    }

    #[allow(clippy::unnecessary_cast)]
    fn get<const N: usize>(store: &NumericValueStore<N>, i: usize) -> Self {
        let p = store.payload[i];
        match store.variant[i] {
            #[cfg(feature = "nibble")]
            NIBBLE => NumericValue::Nibble(shrink_wrap::Nibble::new_masked(p as u8)),
            #[cfg(feature = "u16")]
            U16 => NumericValue::U16(p as u16),
            #[cfg(feature = "u32")]
            U32 => NumericValue::U32(p as u32),
            #[cfg(feature = "unib32")]
            UNIB32 => NumericValue::UNib32(shrink_wrap::UNib32(p as u32)),
            #[cfg(feature = "u64")]
            U64 => NumericValue::U64(p as u64),
            #[cfg(feature = "i32")]
            I32 => NumericValue::I32(p as u32 as i32),
            #[cfg(feature = "f32")]
            F32 => NumericValue::F32(f32::from_bits(p as u32)),
            #[cfg(feature = "un")]
            UN => {
                use shrink_wrap::un::{U3, U4, U5, U6, UN::*};
                let w = store.width[i];
                let bits = w & 0x3F;
                NumericValue::UN(match w >> 6 {
                    0 => UN8 {
                        bit_count: U3::__from_wire(bits as u64),
                        value: p as u8,
                    },
                    1 => UN16 {
                        bit_count: U4::__from_wire(bits as u64),
                        value: p as u16,
                    },
                    2 => UN32 {
                        bit_count: U5::__from_wire(bits as u64),
                        value: p as u32,
                    },
                    _ => UN64 {
                        bit_count: U6::__from_wire(bits as u64),
                        value: p as u64,
                    },
                })
            }
            #[cfg(feature = "in")]
            IN => {
                use shrink_wrap::un::{IN::*, U3, U4, U5, U6};
                let w = store.width[i];
                let bits = w & 0x3F;
                NumericValue::IN(match w >> 6 {
                    0 => IN8 {
                        bit_count: U3::__from_wire(bits as u64),
                        value: p as u8 as i8,
                    },
                    1 => IN16 {
                        bit_count: U4::__from_wire(bits as u64),
                        value: p as u16 as i16,
                    },
                    2 => IN32 {
                        bit_count: U5::__from_wire(bits as u64),
                        value: p as u32 as i32,
                    },
                    _ => IN64 {
                        bit_count: U6::__from_wire(bits as u64),
                        value: p as u64 as i64,
                    },
                })
            }
            #[cfg(feature = "u128")]
            U128 => NumericValue::U128(p as u128),
            #[cfg(feature = "i8")]
            I8 => NumericValue::I8(p as u8 as i8),
            #[cfg(feature = "i16")]
            I16 => NumericValue::I16(p as u16 as i16),
            #[cfg(feature = "i64")]
            I64 => NumericValue::I64(p as u64 as i64),
            #[cfg(feature = "i128")]
            I128 => NumericValue::I128(p as u128 as i128),
            #[cfg(feature = "f64")]
            F64 => NumericValue::F64(f64::from_bits(p as u64)),
            #[cfg(feature = "uvlq32")]
            UVLQ32 => NumericValue::UVlq32(shrink_wrap::UVlq32(p as u32)),
            #[cfg(feature = "uvlq64")]
            UVLQ64 => NumericValue::UVlq64(shrink_wrap::UVlq64(p as u64)),
            // `U8`, and what only `set` writes: every other byte is a variant compiled in
            _ => NumericValue::U8(p as u8),
        }
    }
    /// One variant over the rows, and not `UN` / `IN` (their bit counts vary per value).
    fn columnar<const N: usize>(store: &NumericValueStore<N>, rows: Range<usize>) -> bool {
        let variants = &store.variant[rows];
        let v = variants[0];
        variants.iter().all(|&x| x == v) && v != UN && v != IN
    }

    /// The tag run, then the one variant's column from the payloads.
    fn write_store<S: BitSink, P: SeriesPolicy + ?Sized, const N: usize>(
        store: &NumericValueStore<N>,
        rows: Range<usize>,
        sink: &mut S,
        policy: &P,
        _level: Level,
    ) -> Result<(), Error> {
        let tag = Self::get(store, rows.start).discriminant() as u32;
        write_enum_tags(sink, core::iter::repeat_n(tag, rows.len()))?;
        let variant = store.variant[rows.start];
        let p = &store.payload[rows];
        match variant {
            NIBBLE => write_numeric_column(sink, 4, false, p, policy),
            U8 => write_numeric_column(sink, 8, false, p, policy),
            U16 => write_numeric_column(sink, 16, false, p, policy),
            U32 | UNIB32 | UVLQ32 => write_numeric_column(sink, 32, false, p, policy),
            U64 | UVLQ64 => write_numeric_column(sink, 64, false, p, policy),
            I8 => write_numeric_column(sink, 8, true, p, policy),
            I16 => write_numeric_column(sink, 16, true, p, policy),
            I32 => write_numeric_column(sink, 32, true, p, policy),
            I64 => write_numeric_column(sink, 64, true, p, policy),
            F32 => write_float_column::<f32, _, _, _>(sink, p, policy),
            F64 => write_float_column::<f64, _, _, _>(sink, p, policy),
            U128 => write_numeric128_column(sink, false, p),
            I128 => write_numeric128_column(sink, true, p),
            // `columnar` said no
            _ => Err(Error::MalformedSeries),
        }
    }
}
