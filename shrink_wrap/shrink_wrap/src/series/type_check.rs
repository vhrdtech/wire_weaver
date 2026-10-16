//! Test types for `Series<T>` with hand-written [SeriesColumns] impls (what `#[derive_shrink_wrap(.., series)]`
//! will generate), and the round trip / random bytes checks shared by the unit tests and the `series-type` fuzz
//! target (`cargo fuzz` builds with `--cfg fuzzing`, which makes this module public): a mismatch or a panic is a
//! bug.

use super::{
    BitCounter, BitSink, CodecPolicy, EnumTags, FieldItems, Level, Series, SeriesColumns,
    SeriesCtx, SeriesOwned, SiblingCtx, SiblingTags, TagRun, VariantCtx, changed_before,
    no_earlier, skim_struct_head, write_enum_tags, write_struct_head,
};
use crate::un::U5;
use crate::{
    BufReader, BufWriter, DeserializeShrinkWrap, DeserializeShrinkWrapOwned, Error, Nibble,
    SerializeShrinkWrap, SerializeShrinkWrapOwned,
};

/// A struct's impl: `evolvable` or not, its `N` fields with their indices and the default each takes when the
/// data has no column for it (an older writer).
macro_rules! series_struct {
    ($name:ident $(<$lt:lifetime>)?, $evolvable:expr, $n:literal; $($f:ident: $t:ty = $default:expr, $idx:tt);+ $(;)?) => {
        impl<'i> SeriesColumns<'i> for $name $(<$lt>)? {
            type Reader = (SiblingTags<$n>, ($(Option<<$t as SeriesColumns<'i>>::Reader>,)+));

            fn write_group<'a, I, S>(
                items: I,
                sink: &mut S,
                policy: &CodecPolicy,
                level: Level,
            ) -> Result<(), Error>
            where
                I: Iterator<Item = (&'a Self, bool)> + Clone,
                S: BitSink,
                Self: 'a,
            {
                if items.clone().next().is_none() {
                    return Ok(());
                }
                write_struct_head(sink, level, $evolvable, $n, |start| {
                    let mut counter = BitCounter::at(start);
                    Self::write_group(items.clone(), &mut counter, policy, Level::Fields)?;
                    Ok(counter.bits())
                })?;
                let tags = |s: &Self| [$(s.$f.series_tag()),+];
                $(
                    let field = FieldItems::new(
                        items.clone(),
                        |s: &'a Self| Some(&s.$f),
                        move |p: &'a Self, c: &'a Self| changed_before(&tags(p), &tags(c), $idx),
                    );
                    <$t as SeriesColumns>::write_group(field, sink, policy, Level::Nested)?;
                )+
                Ok(())
            }

            #[allow(unused_assignments)]
            fn skim<C: SeriesCtx>(
                rd: &mut BufReader<'i>,
                ctx: C,
                level: Level,
            ) -> Result<Self::Reader, Error> {
                let head = skim_struct_head(rd, &ctx, level, $evolvable)?;
                let mut runs: [Option<TagRun<'i>>; $n] = [None; $n];
                let readers = ($(
                    if head.has_field(rd, $idx) {
                        let r = <$t as SeriesColumns>::skim(rd, SiblingCtx::new(ctx.clone(), runs), Level::Nested)?;
                        runs[$idx] = <$t as SeriesColumns>::tag_run(&r);
                        Some(r)
                    } else {
                        None
                    },
                )+);
                head.finish(rd, $n)?;
                Ok((SiblingTags::new(), readers))
            }

            fn next(reader: &mut Self::Reader, restart: bool) -> Result<Self, Error> {
                let (sib, readers) = reader;
                sib.begin();
                Ok($name {
                    $($f: match &mut readers.$idx {
                        Some(r) => {
                            let v = <$t as SeriesColumns>::next(r, sib.restart(restart))?;
                            sib.record($idx, v.series_tag());
                            v
                        }
                        None => $default,
                    },)+
                })
            }
        }
    };
}

/// An enum's impl: unit variants and one-field variants (in discriminant order, with their index among the
/// one-field ones).
macro_rules! series_enum {
    ($name:ident; units: [$($uv:ident = $ud:literal),*]; data: [$($dv:ident($t:ty) = $dd:literal, $di:tt),*]) => {
        impl $name {
            fn discriminant(&self) -> u32 {
                match self {
                    $($name::$uv => $ud,)*
                    $($name::$dv(_) => $dd,)*
                }
            }
        }

        impl<'i> SeriesColumns<'i> for $name {
            type Reader = (EnumTags<'i>, ($(<$t as SeriesColumns<'i>>::Reader,)*));

            fn write_group<'a, I, S>(
                items: I,
                sink: &mut S,
                _policy: &CodecPolicy,
                _level: Level,
            ) -> Result<(), Error>
            where
                I: Iterator<Item = (&'a Self, bool)> + Clone,
                S: BitSink,
                Self: 'a,
            {
                write_enum_tags(sink, items.clone().map(|(e, _)| e.discriminant()))?;
                $(
                    let variant = FieldItems::new(
                        items.clone(),
                        |e: &'a Self| match e {
                            $name::$dv(x) => Some(x),
                            #[allow(unreachable_patterns)]
                            _ => None,
                        },
                        no_earlier::<Self>,
                    );
                    <$t as SeriesColumns>::write_group(variant, sink, _policy, Level::Nested)?;
                )*
                Ok(())
            }

            fn skim<C: SeriesCtx>(
                rd: &mut BufReader<'i>,
                ctx: C,
                _level: Level,
            ) -> Result<Self::Reader, Error> {
                #[allow(clippy::manual_range_patterns)]
                let tags = EnumTags::skim(rd, &ctx, |tag| match tag {
                    $($ud)|* $(| $dd)* => Ok(()),
                    _ => Err(Error::EnumFutureVersionOrMalformedData),
                })?;
                let readers = ($(
                    <$t as SeriesColumns>::skim(
                        rd,
                        VariantCtx::new(ctx.clone(), tags.tag_run(), $dd),
                        Level::Nested,
                    )?,
                )*);
                Ok((tags, readers))
            }

            fn next(reader: &mut Self::Reader, _restart: bool) -> Result<Self, Error> {
                let (tags, _readers) = reader;
                let (tag, _entered) = tags.next_tag()?;
                match tag {
                    $($ud => Ok($name::$uv),)*
                    $($dd => Ok($name::$dv(<$t as SeriesColumns>::next(&mut _readers.$di, _restart || _entered)?)),)*
                    _ => Err(Error::EnumFutureVersionOrMalformedData),
                }
            }

            fn series_tag(&self) -> Option<u32> {
                Some(self.discriminant())
            }

            fn tag_run(reader: &Self::Reader) -> Option<TagRun<'i>> {
                reader.0.tag_run()
            }
        }
    };
}

/// `ww_si::Prefix`, the three variants the design's cases use, at their discriminants.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Prefix {
    #[default]
    Unit,
    Milli,
    Micro,
}

series_enum!(Prefix; units: [Unit = 0, Milli = 1, Micro = 2]; data: []);

/// `ww_numeric::NumericValue`, some of its variants, at their discriminants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumericValue {
    U8(u8),
    U16(u16),
    U32(u32),
    I32(i32),
    F32(f32),
}

impl Default for NumericValue {
    fn default() -> Self {
        NumericValue::U8(0)
    }
}

series_enum!(NumericValue; units: []; data: [U8(u8) = 1, 0, U16(u16) = 2, 1, U32(u32) = 3, 2, I32(i32) = 6, 3, F32(f32) = 7, 4]);

/// `ww_si::Volt`, `Ampere`, `Second`, ...: `final_structure`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quantity {
    pub prefix: Prefix,
    pub value: NumericValue,
}

series_struct!(Quantity, false, 2; prefix: Prefix = Prefix::Unit, 0; value: NumericValue = NumericValue::U8(0), 1);

impl Quantity {
    pub fn new(prefix: Prefix, value: NumericValue) -> Self {
        Quantity { prefix, value }
    }
}

/// The design's sample: evolvable.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IvSample {
    pub v: Quantity,
    pub i: Quantity,
}

series_struct!(IvSample, true, 2; v: Quantity = Quantity::default(), 0; i: Quantity = Quantity::default(), 1);

/// `Timed<IvSample>`: `final_structure`, the time first.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimedIv {
    pub t: Quantity,
    pub value: IvSample,
}

series_struct!(TimedIv, false, 2; t: Quantity = Quantity::default(), 0; value: IvSample = IvSample::default(), 1);

/// [IvSample] with a field appended.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IvSampleV2 {
    pub v: Quantity,
    pub i: Quantity,
    pub range_ok: Option<bool>,
}

series_struct!(IvSampleV2, true, 3; v: Quantity = Quantity::default(), 0; i: Quantity = Quantity::default(), 1;
    range_ok: Option<bool> = None, 2);

/// Nested evolvable struct, version 1.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InnerV1 {
    pub x: u16,
}

series_struct!(InnerV1, true, 1; x: u16 = 0, 0);

/// [InnerV1] with an enum and a string appended (the string aligns to a byte inside the group).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InnerV2 {
    pub x: u16,
    pub unit: Prefix,
    pub label: String,
}

series_struct!(InnerV2, true, 3; x: u16 = 0, 0; unit: Prefix = Prefix::Unit, 1; label: String = String::new(), 2);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OuterV1 {
    pub a: u8,
    pub inner: InnerV1,
    pub b: i32,
}

series_struct!(OuterV1, false, 3; a: u8 = 0, 0; inner: InnerV1 = InnerV1::default(), 1; b: i32 = 0, 2);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OuterV2 {
    pub a: u8,
    pub inner: InnerV2,
    pub b: i32,
}

series_struct!(OuterV2, false, 3; a: u8 = 0, 0; inner: InnerV2 = InnerV2::default(), 1; b: i32 = 0, 2);

/// Every kind of leaf and group, borrowed.
#[derive(Clone, Debug, PartialEq)]
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
    pub inner: Option<InnerV2>,
    pub prefixes: [Prefix; 2],
}

series_struct!(Kitchen<'i>, true, 13; flag: bool = false, 0; q: Option<Quantity> = None, 1; name: &'i str = "", 2;
    pair: (u8, Prefix, i64) = (0, Prefix::Unit, 0), 3; arr: [i16; 3] = [0; 3], 4; f: f32 = 0.0, 5; d: f64 = 0.0, 6;
    big: u128 = 0, 7; nib: Nibble = Nibble::new_masked(0), 8; u5: U5 = U5::zero(), 9; bytes: &'i [u8] = &[], 10;
    inner: Option<InnerV2> = None, 11; prefixes: [Prefix; 2] = [Prefix::Unit; 2], 12);

/// [Kitchen], owned.
#[derive(Clone, Debug, PartialEq)]
pub struct KitchenOwned {
    pub flag: bool,
    pub q: Option<Quantity>,
    pub name: String,
    pub pair: (u8, Prefix, i64),
    pub arr: [i16; 3],
    pub f: f32,
    pub d: f64,
    pub big: u128,
    pub nib: Nibble,
    pub u5: U5,
    pub bytes: Vec<u8>,
    pub inner: Option<InnerV2>,
    pub prefixes: [Prefix; 2],
}

series_struct!(KitchenOwned, true, 13; flag: bool = false, 0; q: Option<Quantity> = None, 1;
    name: String = String::new(), 2; pair: (u8, Prefix, i64) = (0, Prefix::Unit, 0), 3; arr: [i16; 3] = [0; 3], 4;
    f: f32 = 0.0, 5; d: f64 = 0.0, 6; big: u128 = 0, 7; nib: Nibble = Nibble::new_masked(0), 8;
    u5: U5 = U5::zero(), 9; bytes: Vec<u8> = Vec::new(), 10; inner: Option<InnerV2> = None, 11;
    prefixes: [Prefix; 2] = [Prefix::Unit; 2], 12);

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
            bytes: &self.bytes,
            inner: self.inner.clone(),
            prefixes: self.prefixes,
        }
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
            Some(InnerV2 {
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
    let predictor = [Predictor::Raw, Predictor::Delta, Predictor::DeltaOfDelta][pick as usize % 3];
    let coder =
        [Coder::Prefix, Coder::Packed, Coder::Constant, Coder::Plain][(pick as usize / 3) % 4];
    let float = if pick & 1 == 0 {
        FloatCodec::Plain
    } else {
        FloatCodec::Xor
    };
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
    walk::<OuterV2>(bytes);
    walk::<Option<(u8, bool)>>(bytes);
    walk::<u32>(bytes);
    if let Ok(owned) = SeriesOwned::<KitchenOwned>::from_ww_bytes_owned(bytes) {
        assert!(owned.len() <= super::SERIES_MAX_ITEMS);
    }
    let _ = SeriesOwned::<OuterV1>::from_ww_bytes_owned(bytes);
}

/// The fuzz entry point: items from the bytes round trip, and the bytes themselves into every reader.
pub fn fuzz_one(bytes: &[u8]) {
    random_bytes(bytes);
    let mut picks = Picks::new(bytes);
    let policy = policy(picks.byte());
    let items = kitchen_items(&mut picks, 40);
    round_trip(&items, policy);
}
