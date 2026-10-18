//! Test types for `Series<T>` with `#[derive_shrink_wrap(.., series)]` (the bytes of the design's examples and
//! tables come out of the same impls the derive writes for any user type), and the round trip / random bytes checks shared by the unit tests and the `series-type` fuzz
//! target (`cargo fuzz` builds with `--cfg fuzzing`, which makes this module public): a mismatch or a panic is a
//! bug.

use super::{CodecPolicy, Series, SeriesColumns, SeriesOwned};
use crate::un::U5;
use crate::{
    BufReader, BufWriter, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, Nibble, RefVec,
    SerializeShrinkWrap, SerializeShrinkWrapOwned, derive_shrink_wrap,
};

/// `ww_si::Prefix`, the three variants the design's cases use, at their discriminants.
#[derive_shrink_wrap(borrowed, owned, series, self_describing, ww_repr = unib32, derive(Clone, Copy, Debug, PartialEq))]
pub enum Prefix {
    Unit,
    Milli,
    Micro,
}

// the derive doesn't keep a `#[default]` on a variant
#[allow(clippy::derivable_impls)]
impl Default for Prefix {
    fn default() -> Self {
        Prefix::Unit
    }
}

/// `ww_numeric::NumericValue`, some of its variants, at their discriminants.
#[derive_shrink_wrap(borrowed, owned, series, self_describing, ww_repr = unib32, derive(Clone, Copy, Debug, PartialEq))]
pub enum NumericValue {
    U8(u8) = 1,
    U16(u16) = 2,
    U32(u32) = 3,
    I32(i32) = 6,
    F32(f32) = 7,
}

impl Default for NumericValue {
    fn default() -> Self {
        NumericValue::U8(0)
    }
}

/// `ww_si::Volt`, `Ampere`, `Second`, ...: `final_structure`.
#[derive_shrink_wrap(
    borrowed,
    owned,
    series,
    final_structure,
    derive(Clone, Copy, Debug, Default, PartialEq)
)]
pub struct Quantity {
    pub prefix: Prefix,
    pub value: NumericValue,
}

impl Quantity {
    pub fn new(prefix: Prefix, value: NumericValue) -> Self {
        Quantity { prefix, value }
    }
}

/// The design's sample: evolvable.
#[derive_shrink_wrap(
    borrowed,
    owned,
    series,
    derive(Clone, Copy, Debug, Default, PartialEq)
)]
pub struct IvSample {
    pub v: Quantity,
    pub i: Quantity,
}

/// `ww_si::Timed<T>`: `final_structure`, the time first; generic (DER-14).
#[derive_shrink_wrap(
    borrowed,
    owned,
    series,
    final_structure,
    derive(Clone, Copy, Debug, Default, PartialEq)
)]
pub struct Timed<T> {
    pub t: Quantity,
    pub value: T,
}

/// The design's `Series<Timed<IvSample>>`.
pub type TimedIv = Timed<IvSample>;

/// [IvSample] with a field appended.
#[derive_shrink_wrap(
    borrowed,
    owned,
    series,
    derive(Clone, Copy, Debug, Default, PartialEq)
)]
pub struct IvSampleV2 {
    pub v: Quantity,
    pub i: Quantity,
    #[default = None]
    pub range_ok: Option<bool>,
}

/// Nested evolvable struct, version 1.
#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Debug, Default, PartialEq))]
pub struct InnerV1 {
    pub x: u16,
}

/// [InnerV1] with an enum and a string appended (the string aligns to a byte inside the group).
#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Debug, Default, PartialEq))]
pub struct InnerV2<'i> {
    pub x: u16,
    #[default = Prefix::Unit]
    pub unit: Prefix,
    #[default = Default::default()]
    pub label: &'i str,
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Debug, Default, PartialEq))]
pub struct OuterV1 {
    pub a: u8,
    pub inner: InnerV1,
    pub b: i32,
}

#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Debug, Default, PartialEq))]
pub struct OuterV2<'i> {
    pub a: u8,
    pub inner: InnerV2<'i>,
    pub b: i32,
}

/// Every kind of leaf and group, borrowed (`Kitchen`) and owned (`KitchenOwned`).
#[derive_shrink_wrap(borrowed, owned, series, derive(Clone, Debug, PartialEq))]
pub struct Kitchen<'i> {
    pub flag: bool,
    pub q: Option<Quantity>,
    pub name: &'i str,
    pub pair: (u8, Prefix, i64),
    pub arr: [i16; 3],
    pub f: f32,
    pub d: f64,
    pub big: u128,
    pub nib: Nibble,
    pub u5: U5,
    pub bytes: &'i [u8],
    pub inner: Option<InnerV2<'i>>,
    pub prefixes: [Prefix; 2],
}

impl Default for KitchenOwned {
    fn default() -> Self {
        KitchenOwned {
            flag: false,
            q: None,
            name: String::new(),
            pair: (0, Prefix::Unit, 0),
            arr: [0; 3],
            f: 0.0,
            d: 0.0,
            big: 0,
            nib: Nibble::new_masked(0),
            u5: U5::zero(),
            bytes: Vec::new(),
            inner: None,
            prefixes: [Prefix::Unit; 2],
        }
    }
}

impl KitchenOwned {
    pub fn as_borrowed(&self) -> Kitchen<'_> {
        Kitchen {
            flag: self.flag,
            q: self.q,
            name: &self.name,
            pair: self.pair,
            arr: self.arr,
            f: self.f,
            d: self.d,
            big: self.big,
            nib: self.nib,
            u5: self.u5,
            bytes: RefVec::new_bytes(&self.bytes),
            inner: self.inner.as_ref().map(|i| InnerV2 {
                x: i.x,
                unit: i.unit,
                label: &i.label,
            }),
            prefixes: self.prefixes,
        }
    }
}

/// `bytes`, a `Series` of `n` items of `T`, are within [Series::max_bytes] when `T` has a bound.
pub fn assert_within_bound<'i, T: SeriesColumns<'i> + 'i>(n: usize, bytes: &[u8]) {
    if let Some(max) = Series::<T>::max_bytes(n) {
        assert!(
            bytes.len() <= max,
            "{} bytes for {n} items of {}, bound {max}",
            bytes.len(),
            core::any::type_name::<T>()
        );
    }
}

/// Bytes as a source of choices: runs out into zeros.
pub struct Picks<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Picks<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Picks { bytes, pos: 0 }
    }

    pub fn byte(&mut self) -> u8 {
        let b = self.bytes.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        b
    }

    pub fn u64(&mut self) -> u64 {
        (0..8).fold(0, |acc, _| acc << 8 | self.byte() as u64)
    }

    pub fn is_empty(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Mostly small steps from `prev`, sometimes a jump.
    fn walk(&mut self, prev: i64) -> i64 {
        match self.byte() % 8 {
            0 => self.u64() as i64,
            1 | 2 => prev,
            _ => prev.wrapping_add((self.byte() as i8 / 16) as i64),
        }
    }

    fn prefix(&mut self, prev: Prefix) -> Prefix {
        match self.byte() % 6 {
            0 => Prefix::Unit,
            1 => Prefix::Milli,
            2 => Prefix::Micro,
            _ => prev,
        }
    }
}

/// Items to write, generated from `picks`: slowly changing values with jumps, switches and gaps.
pub fn kitchen_items(picks: &mut Picks<'_>, max: usize) -> Vec<KitchenOwned> {
    let n = picks.byte() as usize % (max + 1);
    let mut items = Vec::new();
    let mut prev = KitchenOwned::default();
    let mut num = 0i64;
    for _ in 0..n {
        num = picks.walk(num);
        let mut k = prev.clone();
        if picks.byte().is_multiple_of(4) {
            k.flag = !k.flag;
        }
        k.q = match picks.byte() % 5 {
            0 => None,
            1 => Some(Quantity::new(
                picks.prefix(Prefix::Milli),
                NumericValue::F32(num as f32 / 7.0),
            )),
            2 => Some(Quantity::new(
                picks.prefix(Prefix::Micro),
                NumericValue::U8(num as u8),
            )),
            _ => Some(Quantity::new(
                picks.prefix(Prefix::Milli),
                NumericValue::I32(num as i32),
            )),
        };
        if picks.byte().is_multiple_of(8) {
            k.name = ["", "a", "volt", "ampere ✓"][picks.byte() as usize % 4].into();
        }
        k.pair = (num as u8, picks.prefix(k.pair.1), num.wrapping_mul(3));
        k.arr = [num as i16, (num >> 3) as i16, picks.byte() as i16 - 128];
        k.f = num as f32 / 3.0;
        k.d = num as f64 * 0.25;
        k.big = (num as u128) << 64 | picks.byte() as u128;
        k.nib = Nibble::new_masked(picks.byte());
        k.u5 = U5::new(picks.byte() % 32).unwrap_or(U5::zero());
        if picks.byte().is_multiple_of(8) {
            k.bytes = (0..picks.byte() % 5).map(|_| picks.byte()).collect();
        }
        k.inner = if picks.byte().is_multiple_of(3) {
            None
        } else {
            Some(InnerV2Owned {
                x: num as u16,
                unit: picks.prefix(Prefix::Micro),
                label: if picks.byte().is_multiple_of(2) {
                    "x".into()
                } else {
                    String::new()
                },
            })
        };
        k.prefixes = [picks.prefix(k.prefixes[0]), picks.prefix(k.prefixes[1])];
        items.push(k.clone());
        prev = k;
    }
    items
}

/// A codec policy from a pick: mostly the smallest, else a fixed one.
pub fn policy(pick: u8) -> CodecPolicy {
    use super::{Coder, FloatCodec, NumericCodec, Predictor};
    if pick < 128 {
        return CodecPolicy::SMALLEST;
    }
    let predictor = [
        Predictor::Raw,
        Predictor::Delta,
        Predictor::DeltaOfDelta,
        Predictor::Aligned,
    ][pick as usize % 4];
    let coder =
        [Coder::Prefix, Coder::Packed, Coder::Constant, Coder::Plain][(pick as usize / 4) % 4];
    let float = [FloatCodec::Plain, FloatCodec::Xor, FloatCodec::Aligned][pick as usize % 3];
    CodecPolicy::fixed(NumericCodec::new(predictor, coder), float)
}

/// `items` written with `policy` read back the same, borrowed and owned, through `BufWriter` and
/// `BufWriterOwned` (the same bytes), and as a field of a struct (with the length FIFO around it).
pub fn round_trip(items: &[KitchenOwned], policy: CodecPolicy) {
    let borrowed: Vec<Kitchen<'_>> = items.iter().map(|k| k.as_borrowed()).collect();
    let series = Series::with_policy(&borrowed, policy);
    let mut buf = vec![0u8; 64 + items.len() * 200];
    let bytes = series.to_ww_bytes(&mut buf).expect("write").to_vec();
    let owned_bytes = SerializeShrinkWrapOwned::to_ww_bytes_owned(&series).expect("write owned");
    assert_eq!(bytes, owned_bytes);
    // strings: no bound
    assert_eq!(Series::<Kitchen<'_>>::max_bytes(1), None);

    let back = Series::<Kitchen<'_>>::from_ww_bytes(&bytes).expect("read");
    assert_eq!(back.len(), items.len());
    let back: Vec<Kitchen<'_>> = back.try_iter().map(|k| k.expect("item")).collect();
    assert_eq!(back, borrowed);

    let owned = SeriesOwned::<KitchenOwned>::from_ww_bytes_owned(&bytes).expect("read owned");
    assert_eq!(owned.0, items);

    // a decoded Series serializes to the same bytes
    let again = Series::<Kitchen<'_>>::from_ww_bytes(&bytes).expect("read");
    let mut buf2 = vec![0u8; bytes.len() + 16];
    assert_eq!(
        again.to_ww_bytes(&mut buf2).expect("write again"),
        &bytes[..]
    );

    // as a field: Unsized, its length in the FIFO
    let mut wr = BufWriter::new(&mut buf);
    wr.write_u8(0xA5).unwrap();
    wr.write(&series).unwrap();
    wr.write_bool(true).unwrap();
    let framed = wr.finish_and_take().expect("frame").to_vec();
    let mut rd = BufReader::new(&framed);
    assert_eq!(rd.read_u8().unwrap(), 0xA5);
    let inner: Series<'_, Kitchen<'_>> = rd.read().expect("read field");
    assert!(rd.read_bool().unwrap());
    assert_eq!(inner.iter().collect::<Vec<_>>(), borrowed);
}

/// Random bytes into every reader: an error or items, never a panic, an endless loop or a big allocation.
pub fn random_bytes(bytes: &[u8]) {
    fn walk<'i, T: SeriesColumns<'i> + Clone + 'i>(bytes: &'i [u8]) {
        if let Ok(series) = Series::<T>::from_ww_bytes(bytes) {
            assert!(series.len() <= super::SERIES_MAX_ITEMS);
            // deserializing validated every column: iterating can't fail
            for item in series.try_iter() {
                item.expect("validated");
            }
        }
    }
    walk::<Kitchen<'_>>(bytes);
    walk::<IvSample>(bytes);
    walk::<TimedIv>(bytes);
    walk::<OuterV2<'_>>(bytes);
    walk::<Option<(u8, bool)>>(bytes);
    walk::<u32>(bytes);
    if let Ok(owned) = SeriesOwned::<KitchenOwned>::from_ww_bytes_owned(bytes) {
        assert!(owned.len() <= super::SERIES_MAX_ITEMS);
    }
    let _ = SeriesOwned::<OuterV1>::from_ww_bytes_owned(bytes);
}

/// A `NumericValue` from a pick: any variant, any value.
fn numeric(picks: &mut Picks<'_>) -> NumericValue {
    let v = picks.u64();
    match picks.byte() % 5 {
        0 => NumericValue::U8(v as u8),
        1 => NumericValue::U16(v as u16),
        2 => NumericValue::U32(v as u32),
        3 => NumericValue::I32(v as i32),
        _ => NumericValue::F32(f32::from_bits(v as u32)),
    }
}

/// Items of a type with a bounded size, from `picks`: slowly changing values or anything at all (the worst case
/// of the bound: every column changing at every item).
pub fn timed_items(picks: &mut Picks<'_>, max: usize) -> Vec<TimedIv> {
    let n = picks.byte() as usize % (max + 1);
    let wild = picks.byte().is_multiple_of(2);
    let mut items = Vec::new();
    let mut num = 0i64;
    for _ in 0..n {
        num = picks.walk(num);
        let q = |picks: &mut Picks<'_>, prev: Prefix| {
            if wild {
                Quantity::new(picks.prefix(prev), numeric(picks))
            } else {
                Quantity::new(prev, NumericValue::I32(num as i32))
            }
        };
        items.push(Timed {
            t: q(picks, Prefix::Micro),
            value: IvSample {
                v: q(picks, Prefix::Milli),
                i: q(picks, Prefix::Micro),
            },
        });
    }
    items
}

/// Items of a bounded type round trip with `policy`, within [Series::max_bytes].
pub fn bounded_round_trip(items: &[TimedIv], policy: CodecPolicy) {
    let series = Series::with_policy(items, policy);
    let bytes = SerializeShrinkWrapOwned::to_ww_bytes_owned(&series).expect("write");
    let max = Series::<TimedIv>::max_bytes(items.len()).expect("bounded");
    assert!(bytes.len() <= max, "{} bytes, bound {max}", bytes.len());
    assert_eq!(series.encoded_len(), Ok(bytes.len()), "counted");
    let back = SeriesOwned::<TimedIv>::from_ww_bytes_owned(&bytes).expect("read");
    assert_eq!(back.len(), items.len());
    for (a, b) in back.iter().zip(items) {
        assert_eq!(
            a.to_ww_bytes_owned().unwrap(),
            b.to_ww_bytes_owned().unwrap()
        );
    }
}

/// The fuzz entry point: items from the bytes round trip, and the bytes themselves into every reader.
pub fn fuzz_one(bytes: &[u8]) {
    random_bytes(bytes);
    let mut picks = Picks::new(bytes);
    let policy = policy(picks.byte());
    let items = kitchen_items(&mut picks, 40);
    round_trip(&items, policy);
    let items = timed_items(&mut picks, 40);
    bounded_round_trip(&items, policy);
}
