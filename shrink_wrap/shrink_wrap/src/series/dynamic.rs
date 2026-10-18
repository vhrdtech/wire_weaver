//! Column readers and writers for code that knows `T` only at run time (`ww_self`'s dynamic decoder and encoder,
//! CLT-29): the same codecs [SeriesColumns](super::SeriesColumns) impls read and write with, one whole column per
//! call. The caller walks the type tree in column order (the rules of [Series](super::Series)) and knows each
//! column's items and restarts (from the columns before it). A reader leaves `rd` where the next column starts,
//! validates what it reads (malformed data is an [Error], never a panic) and does work bounded by the items it is
//! given. A writer writes the same bits the generated impls write for the same items and [SeriesPolicy](super::SeriesPolicy) codecs,
//! and nothing for no items (a `Series` has no empty columns).

use super::columns::float::{FloatReader, write_float as write_float_column};
use super::columns::numeric::{
    Numeric128Reader, NumericKind, NumericReader, write_numeric as write_numeric_column,
    write_numeric128 as write_numeric128_column,
};
use super::columns::run::{
    Opaque, RunReader, write_opaque_run as write_opaque_run_column, write_opaque_values,
    write_run as write_run_column,
};
use super::columns::{get_nib, put_nib};
use super::group::{Level, StructExtent, put_group_len};
use super::{BitSink, FloatCodec, NumericCodec, XorBits};
use crate::{BufReader, BufWriter, Error};

pub use super::columns::run::RunKind;
pub use super::series_type::SERIES_MAX_ITEMS;

/// Reads `n`, the item count at the start of a `Series` (`UNib32`): above [SERIES_MAX_ITEMS] is `MalformedSeries`.
/// After the group of `n` items, the caller aligns `rd` to a byte (the padding).
pub fn read_count(rd: &mut BufReader<'_>) -> Result<usize, Error> {
    let n = rd.read_unib32()? as usize;
    if n > SERIES_MAX_ITEMS {
        return Err(Error::MalformedSeries);
    }
    Ok(n)
}

/// A numeric column of `width` (1 to 64, else `MalformedSeries`) bit values, one per item of `restarts` (whether
/// the item restarts the column): `f` gets each value's bits, zero-extended (sign-extend `signed` ones yourself).
/// No items: no column, nothing is read.
pub fn read_numeric(
    rd: &mut BufReader<'_>,
    width: u8,
    signed: bool,
    restarts: impl IntoIterator<Item = bool>,
    mut f: impl FnMut(u64) -> Result<(), Error>,
) -> Result<(), Error> {
    if !(1..=64).contains(&width) {
        return Err(Error::MalformedSeries);
    }
    let mut restarts = restarts.into_iter().peekable();
    if restarts.peek().is_none() {
        return Ok(());
    }
    let mut reader = NumericReader::new(*rd, NumericKind::new(width, signed))?;
    for restart in restarts {
        f(reader.next(restart)?)?;
    }
    *rd = reader.rest();
    Ok(())
}

/// A numeric column of `u128` / `i128` values over `m` items: `f` gets each value's bits (two's complement for
/// `signed`).
pub fn read_numeric128(
    rd: &mut BufReader<'_>,
    signed: bool,
    m: usize,
    mut f: impl FnMut(u128) -> Result<(), Error>,
) -> Result<(), Error> {
    if m == 0 {
        return Ok(());
    }
    let mut reader = Numeric128Reader::new(*rd, signed)?;
    for _ in 0..m {
        f(reader.next()?)?;
    }
    *rd = reader.rest();
    Ok(())
}

/// A float column of `f32` or `f64` values, one per item of `restarts`.
pub fn read_float<T: XorBits>(
    rd: &mut BufReader<'_>,
    restarts: impl IntoIterator<Item = bool>,
    mut f: impl FnMut(T) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut restarts = restarts.into_iter().peekable();
    if restarts.peek().is_none() {
        return Ok(());
    }
    let mut reader = FloatReader::<T>::new(*rd)?;
    for restart in restarts {
        f(reader.next(restart)?)?;
    }
    *rd = reader.rest();
    Ok(())
}

/// A run of enum tags ([RunKind::Tag]), `Option` tags (0 or 1) or bools (0 or 1) over `m` items: `f` gets each
/// item's value.
pub fn read_run(
    rd: &mut BufReader<'_>,
    kind: RunKind,
    m: usize,
    mut f: impl FnMut(u32) -> Result<(), Error>,
) -> Result<(), Error> {
    if m == 0 {
        return Ok(());
    }
    let mut run = RunReader::new(*rd, kind, m)?;
    for _ in 0..m {
        f(run.next()?)?;
    }
    *rd = run.finish()?;
    Ok(())
}

/// A run of opaque byte strings over `m` items: `f` gets each item's bytes and whether they start a new value (the
/// first item, and every change), so that a caller decodes each value once.
pub fn read_opaque_run<'i>(
    rd: &mut BufReader<'i>,
    m: usize,
    mut f: impl FnMut(&'i [u8], bool) -> Result<(), Error>,
) -> Result<(), Error> {
    if m == 0 {
        return Ok(());
    }
    let mut run = RunReader::new(*rd, Opaque, m)?;
    for i in 0..m {
        let changes = i == 0 || run.changes_next();
        f(run.next()?, changes)?;
    }
    *rd = run.finish()?;
    Ok(())
}

/// Where a struct's group ends, from what an evolvable struct with items writes before its fields (see
/// [StructExtent]); not evolvable, at [Level::Fields] or with no items: [StructExtent::All], nothing is read.
pub fn read_struct_head(
    rd: &mut BufReader<'_>,
    level: Level,
    evolvable: bool,
    m: usize,
) -> Result<StructExtent, Error> {
    if !evolvable || level == Level::Fields || m == 0 {
        return Ok(StructExtent::All);
    }
    let value = get_nib(rd)?;
    if level == Level::Outermost {
        return Ok(StructExtent::Count(value));
    }
    if value as usize > rd.bits_left() {
        return Err(Error::MalformedSeries);
    }
    Ok(StructExtent::End(rd.bit_pos() + value as usize))
}

/// Writes `n`, the item count at the start of a `Series` (`UNib32`): above [SERIES_MAX_ITEMS] is `LenTooLong`.
/// After the group of `n` items, the caller aligns `wr` to a byte.
pub fn write_count(wr: &mut BufWriter<'_>, n: usize) -> Result<(), Error> {
    wr.write_unib32(checked_count(n)?)
}

/// [write_count] into a `BufWriterOwned`.
#[cfg(feature = "std")]
pub fn write_count_owned(wr: &mut crate::BufWriterOwned, n: usize) -> Result<(), Error> {
    wr.write_unib32(checked_count(n)?)
}

fn checked_count(n: usize) -> Result<u32, Error> {
    if n > SERIES_MAX_ITEMS {
        return Err(Error::LenTooLong);
    }
    Ok(n as u32)
}

/// A numeric column of `width` (1 to 64, else `SubtypeOutOfRange`) bit values: `items` are each item's value bits
/// (bits above the width are ignored, so a signed value may be sign-extended) and whether it restarts the column,
/// walked once per codec tried. `codec` as [SeriesPolicy::numeric](super::SeriesPolicy::numeric) says: `None` the smallest, a fixed one as
/// written (a constant coder that can't encode the column: packed with the same predictor). Returns the codec
/// written, `None` for no items.
pub fn write_numeric<S: BitSink>(
    sink: &mut S,
    width: u8,
    signed: bool,
    items: impl Iterator<Item = (u64, bool)> + Clone,
    codec: Option<NumericCodec>,
) -> Result<Option<NumericCodec>, Error> {
    if !(1..=64).contains(&width) {
        return Err(Error::SubtypeOutOfRange);
    }
    if items.clone().next().is_none() {
        return Ok(None);
    }
    // the bits written don't depend on the word: a column up to 32 bits in a `u64` gives what a `u32` gives
    write_numeric_column(sink, NumericKind::new(width, signed), items, codec).map(Some)
}

/// A numeric column of `u128` / `i128` values (two's complement bits for `signed`), always raw + plain.
pub fn write_numeric128<S: BitSink>(
    sink: &mut S,
    signed: bool,
    items: impl Iterator<Item = u128> + Clone,
) -> Result<(), Error> {
    if items.clone().next().is_none() {
        return Ok(());
    }
    write_numeric128_column(sink, signed, items)
}

/// A float column of `f32` or `f64` values with the restart flag of each, `codec` as [SeriesPolicy::float](super::SeriesPolicy::float) says.
/// Returns the codec written, `None` for no items.
pub fn write_float<T: XorBits, S: BitSink>(
    sink: &mut S,
    items: impl Iterator<Item = (T, bool)> + Clone,
    codec: Option<FloatCodec>,
) -> Result<Option<FloatCodec>, Error> {
    if items.clone().next().is_none() {
        return Ok(None);
    }
    write_float_column(sink, items, codec).map(Some)
}

/// A run of enum tags ([RunKind::Tag]), `Option` tags or bools (0 or 1, else `SubtypeOutOfRange`).
pub fn write_run<S: BitSink>(
    sink: &mut S,
    kind: RunKind,
    items: impl Iterator<Item = u32>,
) -> Result<(), Error> {
    write_run_column(sink, kind, items)
}

/// A run of byte strings (strings and byte lists), a change where the bytes differ.
pub fn write_opaque_run<'a, S: BitSink>(
    sink: &mut S,
    items: impl Iterator<Item = &'a [u8]>,
) -> Result<(), Error> {
    write_opaque_run_column(sink, items)
}

/// The run of a field marked `#[series(opaque)]`: each item's bytes (`to_ww_bytes` of its value), a change where
/// the item is not `==` the one before (the value's own equality, as the generated writer compares: `0.0 ==
/// -0.0`, a NaN never equals).
pub fn write_opaque_field<'a, V, S>(
    sink: &mut S,
    items: impl Iterator<Item = &'a V>,
) -> Result<(), Error>
where
    V: PartialEq + AsRef<[u8]> + ?Sized + 'a,
    S: BitSink,
{
    write_opaque_values(sink, items, |sink, v| {
        let bytes = v.as_ref();
        put_nib(
            sink,
            u32::try_from(bytes.len()).map_err(|_| Error::LenTooLong)?,
        )?;
        sink.put_bytes(bytes)
    })
}

/// What a struct's group has before its fields, when it has items: an evolvable struct's field count `fields` at
/// [Level::Outermost], its group length at [Level::Nested]; nothing otherwise. `bits_at(at)` gives the bits the
/// struct's fields take when written from bit position `at` (of the same buffer: only `at % 8` matters, for
/// byte-aligned values), called once or a few times.
pub fn write_struct_head<S: BitSink>(
    sink: &mut S,
    level: Level,
    evolvable: bool,
    fields: u32,
    mut bits_at: impl FnMut(usize) -> Result<usize, Error>,
) -> Result<(), Error> {
    if !evolvable {
        return Ok(());
    }
    match level {
        Level::Fields => Ok(()),
        Level::Outermost => put_nib(sink, fields),
        Level::Nested => put_group_len(sink, |_, at| bits_at(at)),
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::SerializeShrinkWrapOwned;
    use crate::series::Series;

    #[test]
    fn numeric_width_out_of_range_is_an_error() {
        let mut rd = BufReader::new(&[0u8; 16]);
        assert_eq!(
            read_numeric(&mut rd, 0, false, [false], |_| Ok(())),
            Err(Error::MalformedSeries)
        );
        assert_eq!(
            read_numeric(&mut rd, 65, false, [false], |_| Ok(())),
            Err(Error::MalformedSeries)
        );
    }

    #[test]
    fn reads_a_series_of_tuples() {
        let items = [(1u16, true), (2, true), (4, false)];
        let bytes = Series::new(&items).to_ww_bytes_owned().unwrap();
        let mut rd = BufReader::new(&bytes);
        let n = read_count(&mut rd).unwrap();
        assert_eq!(n, 3);
        let mut a = vec![];
        read_numeric(&mut rd, 16, false, [false; 3], |v| {
            a.push(v);
            Ok(())
        })
        .unwrap();
        let mut b = vec![];
        read_run(&mut rd, RunKind::Bool, n, |v| {
            b.push(v);
            Ok(())
        })
        .unwrap();
        assert_eq!(a, [1, 2, 4]);
        assert_eq!(b, [1, 1, 0]);
    }

    /// The writers give what the typed `Series` writes, with the smallest codecs and with fixed ones.
    #[test]
    fn writes_a_series_of_tuples() {
        use crate::BufWriterOwned;
        use crate::series::{CodecPolicy, DeltaPacked};
        let items = [
            (1u16, true, 0.5f32),
            (2, true, 0.75),
            (4, false, 0.75),
            (4, false, -1.0),
        ];
        for policy in [CodecPolicy::SMALLEST, DeltaPacked.into()] {
            let typed = Series::with_policy(&items, policy)
                .to_ww_bytes_owned()
                .unwrap();
            let mut wr = BufWriterOwned::new();
            write_count_owned(&mut wr, items.len()).unwrap();
            write_numeric(
                &mut wr,
                16,
                false,
                items.iter().map(|t| (t.0 as u64, false)),
                policy.numeric,
            )
            .unwrap();
            write_run(&mut wr, RunKind::Bool, items.iter().map(|t| t.1 as u32)).unwrap();
            write_float(&mut wr, items.iter().map(|t| (t.2, false)), policy.float).unwrap();
            wr.align_byte();
            assert_eq!(wr.finish_and_take().unwrap(), typed);
        }
        let mut wr = BufWriterOwned::new();
        assert_eq!(
            write_count_owned(&mut wr, SERIES_MAX_ITEMS + 1),
            Err(Error::LenTooLong)
        );
        assert_eq!(
            write_numeric(&mut wr, 65, false, [(0, false)].into_iter(), None),
            Err(Error::SubtypeOutOfRange)
        );
        assert_eq!(
            write_numeric(&mut wr, 8, false, [].into_iter(), None),
            Ok(None)
        );
    }
}
