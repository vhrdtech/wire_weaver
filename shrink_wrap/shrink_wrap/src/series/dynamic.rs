//! Column readers for decoders that know `T` only at run time (`ww_self`'s dynamic decoder, CLT-29): the same
//! codecs [SeriesColumns](super::SeriesColumns) impls read with, one whole column per call. The caller walks the
//! type tree in column order (the rules of [Series](super::Series)), knows each column's item count and restarts
//! from the columns it read before, and leaves `rd` where the next column starts. Every reader validates what it
//! reads (malformed data is an [Error], never a panic) and does work bounded by the items it is given.

use super::XorBits;
use super::columns::float::FloatReader;
use super::columns::get_nib;
use super::columns::numeric::{Numeric128Reader, NumericKind, NumericReader};
use super::columns::run::{Opaque, RunReader};
use super::group::{Level, StructExtent};
use crate::{BufReader, Error};

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
}
