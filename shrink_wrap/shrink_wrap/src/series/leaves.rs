//! [SeriesColumns] of the types `shrink_wrap` itself has: numbers as numeric or float columns, `bool` as a
//! two-valued run, `Option` as the enum `{ None, Some(X) }`, tuples and arrays as `final_structure` structs of
//! their elements, strings and byte lists as opaque runs.

use super::columns::float::{FloatReader, write_float};
use super::columns::numeric::{
    Numeric128Reader, NumericKind, NumericReader, write_numeric, write_numeric128,
};
use super::columns::run::{Opaque, RunKind, RunReader, write_opaque_run, write_run};
use super::group::{
    CodecPolicy, EnumTags, FieldItems, Level, SeriesColumns, SeriesCtx, SiblingCtx, SiblingTags,
    TagRun, VariantCtx, changed_before, no_earlier,
};
use super::{BitSink, XorBits, sign_extend};
use crate::{BufReader, Error, Nibble, RefVec, UNib32, UVlq32, UVlq64};

/// Skims a numeric column over `ctx`'s items: `None` for no items (no column is written).
fn skim_numeric<'i, C: SeriesCtx>(
    rd: &mut BufReader<'i>,
    ctx: &C,
    kind: NumericKind,
) -> Result<Option<NumericReader<'i>>, Error> {
    if ctx.present()? == 0 {
        return Ok(None);
    }
    let reader = NumericReader::new(*rd, kind)?;
    let mut walk = reader;
    ctx.for_each_present(|restart| walk.next(restart).map(|_| ()))?;
    *rd = walk.rest();
    Ok(Some(reader))
}

fn skim_float<'i, T: XorBits, C: SeriesCtx>(
    rd: &mut BufReader<'i>,
    ctx: &C,
) -> Result<Option<FloatReader<'i, T>>, Error> {
    if ctx.present()? == 0 {
        return Ok(None);
    }
    let reader = FloatReader::<T>::new(*rd)?;
    let mut walk = reader.clone();
    ctx.for_each_present(|restart| walk.next(restart).map(|_| ()))?;
    *rd = walk.rest();
    Ok(Some(reader))
}

fn skim_numeric128<'i, C: SeriesCtx>(
    rd: &mut BufReader<'i>,
    ctx: &C,
    signed: bool,
) -> Result<Option<Numeric128Reader<'i>>, Error> {
    if ctx.present()? == 0 {
        return Ok(None);
    }
    let reader = Numeric128Reader::new(*rd, signed)?;
    let mut walk = reader;
    ctx.for_each_present(|_| walk.next().map(|_| ()))?;
    *rd = walk.rest();
    Ok(Some(reader))
}

/// The reader of a column that exists: a missing one is a bug in the caller's item count, or malformed data.
fn column<R>(reader: &mut Option<R>) -> Result<&mut R, Error> {
    reader.as_mut().ok_or(Error::MalformedSeries)
}

macro_rules! numeric_leaf {
    ($ty:ty, $width:expr, $signed:expr, |$v:ident| $to:expr, |$b:ident| $from:expr) => {
        impl<'i> SeriesColumns<'i> for $ty {
            type Reader = Option<NumericReader<'i>>;

            fn write_group<'a, I, S>(
                items: I,
                sink: &mut S,
                policy: &CodecPolicy,
                _level: Level,
            ) -> Result<(), Error>
            where
                I: Iterator<Item = (&'a Self, bool)> + Clone,
                S: BitSink,
                Self: 'a,
            {
                if items.clone().next().is_none() {
                    return Ok(());
                }
                let kind = NumericKind::new($width, $signed);
                write_numeric(
                    sink,
                    kind,
                    items.map(|($v, restart): (&Self, bool)| ($to, restart)),
                    policy.numeric,
                )?;
                Ok(())
            }

            fn skim<C: SeriesCtx>(
                rd: &mut BufReader<'i>,
                ctx: C,
                _level: Level,
            ) -> Result<Self::Reader, Error> {
                skim_numeric(rd, &ctx, NumericKind::new($width, $signed))
            }

            fn next(reader: &mut Self::Reader, restart: bool) -> Result<Self, Error> {
                let $b = column(reader)?.next(restart)?;
                Ok($from)
            }
        }
    };
}

macro_rules! numeric_prim {
    ($($ty:ty: $width:literal, $signed:literal);* $(;)?) => {
        $(numeric_leaf!($ty, $width, $signed, |v| *v as u64, |b| if $signed {
            sign_extend(b, $width) as $ty
        } else {
            b as $ty
        });)*
    };
}

numeric_prim!(
    u8: 8, false; u16: 16, false; u32: 32, false; u64: 64, false;
    i8: 8, true; i16: 16, true; i32: 32, true; i64: 64, true;
);

numeric_leaf!(Nibble, 4, false, |v| v.value() as u64, |b| {
    Nibble::new_masked(b as u8)
});
numeric_leaf!(UNib32, 32, false, |v| v.0 as u64, |b| UNib32(b as u32));
numeric_leaf!(UVlq32, 32, false, |v| v.0 as u64, |b| UVlq32(b as u32));
numeric_leaf!(UVlq64, 64, false, |v| v.0, |b| UVlq64(b));

macro_rules! un_leaves {
    ($signed:literal, $prefix:ident: $($bits:literal),*) => {
        paste::paste! {
            $(numeric_leaf!(crate::un::[<$prefix $bits>], $bits, $signed, |v| v.__to_wire(), |b| {
                crate::un::[<$prefix $bits>]::__from_wire(b)
            });)*
        }
    };
}

un_leaves!(false, U: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54,
    55, 56, 57, 58, 59, 60, 61, 62, 63, 64);
un_leaves!(true, I: 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54,
    55, 56, 57, 58, 59, 60, 61, 62, 63, 64);

macro_rules! numeric128 {
    ($ty:ty, $signed:literal) => {
        impl<'i> SeriesColumns<'i> for $ty {
            type Reader = Option<Numeric128Reader<'i>>;

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
                if items.clone().next().is_none() {
                    return Ok(());
                }
                write_numeric128(sink, $signed, items.map(|(v, _)| *v as u128))
            }

            fn skim<C: SeriesCtx>(
                rd: &mut BufReader<'i>,
                ctx: C,
                _level: Level,
            ) -> Result<Self::Reader, Error> {
                skim_numeric128(rd, &ctx, $signed)
            }

            fn next(reader: &mut Self::Reader, _restart: bool) -> Result<Self, Error> {
                Ok(column(reader)?.next()? as $ty)
            }
        }
    };
}

numeric128!(u128, false);
numeric128!(i128, true);

macro_rules! float_leaf {
    ($ty:ty) => {
        impl<'i> SeriesColumns<'i> for $ty {
            type Reader = Option<FloatReader<'i, $ty>>;

            fn write_group<'a, I, S>(
                items: I,
                sink: &mut S,
                policy: &CodecPolicy,
                _level: Level,
            ) -> Result<(), Error>
            where
                I: Iterator<Item = (&'a Self, bool)> + Clone,
                S: BitSink,
                Self: 'a,
            {
                if items.clone().next().is_none() {
                    return Ok(());
                }
                write_float(sink, items.map(|(v, r)| (*v, r)), policy.float)?;
                Ok(())
            }

            fn skim<C: SeriesCtx>(
                rd: &mut BufReader<'i>,
                ctx: C,
                _level: Level,
            ) -> Result<Self::Reader, Error> {
                skim_float(rd, &ctx)
            }

            fn next(reader: &mut Self::Reader, restart: bool) -> Result<Self, Error> {
                column(reader)?.next(restart)
            }
        }
    };
}

float_leaf!(f32);
float_leaf!(f64);

impl<'i> SeriesColumns<'i> for bool {
    type Reader = Option<RunReader<'i, RunKind>>;

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
        write_run(sink, RunKind::Bool, items.map(|(b, _)| *b as u32))
    }

    fn skim<C: SeriesCtx>(
        rd: &mut BufReader<'i>,
        ctx: C,
        _level: Level,
    ) -> Result<Self::Reader, Error> {
        let m = ctx.present()?;
        if m == 0 {
            return Ok(None);
        }
        let run = RunReader::new(*rd, RunKind::Bool, m)?;
        *rd = run.finish()?;
        Ok(Some(run))
    }

    fn next(reader: &mut Self::Reader, _restart: bool) -> Result<Self, Error> {
        Ok(column(reader)?.next()? == 1)
    }
}

/// A type written as an opaque run of its bytes.
trait OpaqueBytes<'i>: Sized {
    fn bytes(&self) -> &[u8];
    /// From a run value: `MalformedUtf8` for a string that isn't one.
    fn from_bytes(bytes: &'i [u8]) -> Result<Self, Error>;
}

impl<'i> OpaqueBytes<'i> for &'i str {
    fn bytes(&self) -> &[u8] {
        self.as_bytes()
    }

    fn from_bytes(bytes: &'i [u8]) -> Result<Self, Error> {
        core::str::from_utf8(bytes).map_err(|_| Error::MalformedUtf8)
    }
}

impl<'i> OpaqueBytes<'i> for &'i [u8] {
    fn bytes(&self) -> &[u8] {
        self
    }

    fn from_bytes(bytes: &'i [u8]) -> Result<Self, Error> {
        Ok(bytes)
    }
}

impl<'i> OpaqueBytes<'i> for RefVec<'i, u8> {
    fn bytes(&self) -> &[u8] {
        self.as_slice()
    }

    fn from_bytes(bytes: &'i [u8]) -> Result<Self, Error> {
        Ok(RefVec::new_bytes(bytes))
    }
}

#[cfg(feature = "std")]
impl<'i> OpaqueBytes<'i> for String {
    fn bytes(&self) -> &[u8] {
        self.as_bytes()
    }

    fn from_bytes(bytes: &'i [u8]) -> Result<Self, Error> {
        Ok(<&str>::from_bytes(bytes)?.into())
    }
}

#[cfg(feature = "std")]
impl<'i> OpaqueBytes<'i> for Vec<u8> {
    fn bytes(&self) -> &[u8] {
        self
    }

    fn from_bytes(bytes: &'i [u8]) -> Result<Self, Error> {
        Ok(bytes.to_vec())
    }
}

macro_rules! opaque_leaf {
    ($ty:ty) => {
        impl<'i> SeriesColumns<'i> for $ty {
            type Reader = Option<RunReader<'i, Opaque>>;

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
                write_opaque_run(sink, items.map(|(v, _)| OpaqueBytes::bytes(v)))
            }

            fn skim<C: SeriesCtx>(
                rd: &mut BufReader<'i>,
                ctx: C,
                _level: Level,
            ) -> Result<Self::Reader, Error> {
                let m = ctx.present()?;
                if m == 0 {
                    return Ok(None);
                }
                let run = RunReader::new(*rd, Opaque, m)?;
                // every value is checked (UTF-8), so that iterating can't fail
                let mut walk = run;
                for _ in 0..m {
                    <$ty as OpaqueBytes>::from_bytes(walk.next()?)?;
                }
                *rd = walk.finish()?;
                Ok(Some(run))
            }

            fn next(reader: &mut Self::Reader, _restart: bool) -> Result<Self, Error> {
                <$ty as OpaqueBytes>::from_bytes(column(reader)?.next()?)
            }
        }
    };
}

opaque_leaf!(&'i str);
opaque_leaf!(&'i [u8]);
opaque_leaf!(RefVec<'i, u8>);
#[cfg(feature = "std")]
opaque_leaf!(String);
#[cfg(feature = "std")]
opaque_leaf!(Vec<u8>);

/// Reader of an `Option<X>`: its tag run and the group of `X` over the `Some` items.
#[derive(Clone, Debug)]
pub struct OptionReader<'i, R> {
    tags: EnumTags<'i>,
    some: R,
}

impl<'i, X: SeriesColumns<'i>> SeriesColumns<'i> for Option<X> {
    type Reader = OptionReader<'i, X::Reader>;

    fn write_group<'a, I, S>(
        items: I,
        sink: &mut S,
        policy: &CodecPolicy,
        _level: Level,
    ) -> Result<(), Error>
    where
        I: Iterator<Item = (&'a Self, bool)> + Clone,
        S: BitSink,
        Self: 'a,
    {
        write_run(
            sink,
            RunKind::OptionTag,
            items.clone().map(|(o, _)| o.is_some() as u32),
        )?;
        let some = FieldItems::new(items, |o: &'a Self| o.as_ref(), no_earlier::<Self>);
        X::write_group(some, sink, policy, Level::Nested)
    }

    fn skim<C: SeriesCtx>(
        rd: &mut BufReader<'i>,
        ctx: C,
        _level: Level,
    ) -> Result<Self::Reader, Error> {
        let tags = EnumTags::skim_kind(rd, &ctx, RunKind::OptionTag, |_| Ok(()))?;
        let some = X::skim(rd, VariantCtx::new(ctx, tags.tag_run(), 1), Level::Nested)?;
        Ok(OptionReader { tags, some })
    }

    fn next(reader: &mut Self::Reader, restart: bool) -> Result<Self, Error> {
        let (tag, entered) = reader.tags.next_tag()?;
        if tag == 0 {
            return Ok(None);
        }
        Ok(Some(X::next(&mut reader.some, restart || entered)?))
    }

    fn series_tag(&self) -> Option<u32> {
        Some(self.is_some() as u32)
    }

    fn tag_run(reader: &Self::Reader) -> Option<TagRun<'i>> {
        reader.tags.tag_run()
    }
}

macro_rules! tuple {
    ($n:literal: $($t:ident $idx:tt),+) => {
        impl<'i, $($t: SeriesColumns<'i>),+> SeriesColumns<'i> for ($($t,)+) {
            type Reader = (SiblingTags<$n>, ($($t::Reader,)+));

            fn write_group<'a, I, S>(
                items: I,
                sink: &mut S,
                policy: &CodecPolicy,
                _level: Level,
            ) -> Result<(), Error>
            where
                I: Iterator<Item = (&'a Self, bool)> + Clone,
                S: BitSink,
                Self: 'a,
            {
                let tags = |t: &Self| [$(t.$idx.series_tag()),+];
                $(
                    let field = FieldItems::new(
                        items.clone(),
                        |t: &'a Self| Some(&t.$idx),
                        move |p: &'a Self, c: &'a Self| changed_before(&tags(p), &tags(c), $idx),
                    );
                    $t::write_group(field, sink, policy, Level::Nested)?;
                )+
                Ok(())
            }

            // the last field's tag run is recorded but read by no later field
            #[allow(unused_assignments)]
            fn skim<C: SeriesCtx>(
                rd: &mut BufReader<'i>,
                ctx: C,
                _level: Level,
            ) -> Result<Self::Reader, Error> {
                let mut runs: [Option<TagRun<'i>>; $n] = [None; $n];
                let readers = ($({
                    let r = $t::skim(rd, SiblingCtx::new(ctx.clone(), runs), Level::Nested)?;
                    runs[$idx] = $t::tag_run(&r);
                    r
                },)+);
                Ok((SiblingTags::new(), readers))
            }

            fn next(reader: &mut Self::Reader, restart: bool) -> Result<Self, Error> {
                let (sib, readers) = reader;
                sib.begin();
                Ok(($({
                    let v = $t::next(&mut readers.$idx, sib.restart(restart))?;
                    sib.record($idx, v.series_tag());
                    v
                },)+))
            }
        }
    };
}

tuple!(1: T0 0);
tuple!(2: T0 0, T1 1);
tuple!(3: T0 0, T1 1, T2 2);
tuple!(4: T0 0, T1 1, T2 2, T3 3);
tuple!(5: T0 0, T1 1, T2 2, T3 3, T4 4);
tuple!(6: T0 0, T1 1, T2 2, T3 3, T4 4, T5 5);
tuple!(7: T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6);
tuple!(8: T0 0, T1 1, T2 2, T3 3, T4 4, T5 5, T6 6, T7 7);

/// Reader of an array `[X; N]`: one group per element.
#[derive(Clone, Debug)]
pub struct ArrayReader<R, const N: usize> {
    sib: SiblingTags<N>,
    elements: [Option<R>; N],
}

/// `[X; N]` like a `final_structure` struct of `N` fields. `X: Default` as for the plain array impls.
impl<'i, X: SeriesColumns<'i> + Default, const N: usize> SeriesColumns<'i> for [X; N] {
    type Reader = ArrayReader<X::Reader, N>;

    fn write_group<'a, I, S>(
        items: I,
        sink: &mut S,
        policy: &CodecPolicy,
        _level: Level,
    ) -> Result<(), Error>
    where
        I: Iterator<Item = (&'a Self, bool)> + Clone,
        S: BitSink,
        Self: 'a,
    {
        for j in 0..N {
            let field = FieldItems::new(
                items.clone(),
                move |a: &'a Self| Some(&a[j]),
                move |p: &'a Self, c: &'a Self| {
                    (0..j).any(|k| {
                        let (p, c) = (p[k].series_tag(), c[k].series_tag());
                        p.is_some() && c.is_some() && p != c
                    })
                },
            );
            X::write_group(field, sink, policy, Level::Nested)?;
        }
        Ok(())
    }

    fn skim<C: SeriesCtx>(
        rd: &mut BufReader<'i>,
        ctx: C,
        _level: Level,
    ) -> Result<Self::Reader, Error> {
        let mut runs: [Option<TagRun<'i>>; N] = [None; N];
        let mut elements: [Option<X::Reader>; N] = core::array::from_fn(|_| None);
        for j in 0..N {
            let r = X::skim(rd, SiblingCtx::new(ctx.clone(), runs), Level::Nested)?;
            runs[j] = X::tag_run(&r);
            elements[j] = Some(r);
        }
        Ok(ArrayReader {
            sib: SiblingTags::new(),
            elements,
        })
    }

    fn next(reader: &mut Self::Reader, restart: bool) -> Result<Self, Error> {
        let mut out: [X; N] = core::array::from_fn(|_| X::default());
        reader.sib.begin();
        for (j, (slot, element)) in out.iter_mut().zip(&mut reader.elements).enumerate() {
            let v = X::next(column(element)?, reader.sib.restart(restart))?;
            reader.sib.record(j, v.series_tag());
            *slot = v;
        }
        Ok(out)
    }
}
