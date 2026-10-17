//! [Series]: a list of `T` written column by column (SW-60).

use super::group::{CodecPolicy, Level, RootCtx, SeriesColumns};
use crate::{BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap};

/// Most items a `Series` holds: an item can take no bits at all (a constant column, runs), so the count can't be
/// checked against the data and is capped instead.
pub const SERIES_MAX_ITEMS: usize = u16::MAX as usize;

/// A list of `T` (numbers, `bool`, `Option`, tuples, arrays, strings, and structs and enums with
/// `#[derive_shrink_wrap(.., series)]`, nested) written column by column, one column per leaf field: discrete leaves
/// as runs of changes, numbers with a predictor and coder picked per column. Much smaller than `Vec<T>` for
/// samples that change slowly; self-contained, so one lost `Series` never affects the next.
///
/// ```text
/// n:          UNib32                  number of items, 0 to 65 535; 0: nothing else follows
/// group(T):   the columns of T over all n items
/// padding:    zero bits to the next byte
/// ```
///
/// `ElementSize::Unsized`: as a field its byte length goes into the parent's length FIFO; nothing inside uses the
/// FIFO. Serializing walks the items once per column and allocates nothing. Deserializing reads `n` and validates
/// every column in one pass ([SeriesColumns::skim]), keeping a cursor per column; [iter](Self::iter) then decodes
/// the items one by one, all columns in parallel, zero-copy for `&str` / `&[u8]` leaves. Malformed data is an
/// `Error`, never a panic, and the work is bounded by `n` times the columns.
pub enum Series<'i, T: SeriesColumns<'i>> {
    /// Items to serialize, and how to pick the codecs of their columns.
    Slice { items: &'i [T], policy: CodecPolicy },
    /// Decoded from a buffer: the column cursors at their starts.
    Buf {
        n: usize,
        reader: Option<T::Reader>,
        /// The `Series`' own bytes when it started at a byte (always, when read as a field), to serialize again.
        raw: Option<&'i [u8]>,
    },
}

impl<'i, T: SeriesColumns<'i>> Series<'i, T> {
    /// Items to serialize, each column with its smallest codec.
    pub const fn new(items: &'i [T]) -> Self {
        Series::Slice {
            items,
            policy: CodecPolicy::SMALLEST,
        }
    }

    /// Items to serialize with a fixed [CodecPolicy] (a small MCU can skip trying every codec).
    pub const fn with_policy(items: &'i [T], policy: CodecPolicy) -> Self {
        Series::Slice { items, policy }
    }

    /// The most bytes a `Series` of `n` items of `T` takes, written from a byte boundary (as a field it always is)
    /// with any [CodecPolicy]: `n`, [MAX_FIXED_BITS](SeriesColumns::MAX_FIXED_BITS) and `n` ×
    /// [MAX_ITEM_BITS](SeriesColumns::MAX_ITEM_BITS), the padding. `None` for a `T` without a bound and for more
    /// than [SERIES_MAX_ITEMS].
    pub const fn max_bytes(n: usize) -> Option<usize> {
        if n > SERIES_MAX_ITEMS {
            return None;
        }
        let count = Some(super::bound::nib_bits(n as u32));
        let bits = if n == 0 {
            count
        } else {
            super::bound::add(
                super::bound::add(count, T::MAX_FIXED_BITS),
                super::bound::times(T::MAX_ITEM_BITS, n),
            )
        };
        match bits {
            Some(bits) => Some(bits.div_ceil(8)),
            None => None,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Series::Slice { items, .. } => items.len(),
            Series::Buf { n, .. } => *n,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The items, decoded one by one from a buffer, or cloned from a slice.
    pub fn iter(&self) -> SeriesIter<'_, 'i, T>
    where
        T: Clone,
    {
        match self {
            Series::Slice { items, .. } => SeriesIter::Slice { items, pos: 0 },
            Series::Buf { n, reader, .. } => SeriesIter::Buf {
                reader: reader.clone(),
                left: *n,
            },
        }
    }

    /// Like [iter](Self::iter), but an error in the data is returned instead of ending the iteration. Deserializing
    /// already validated the columns, so it only happens for a `Buf` built by hand.
    pub fn try_iter(&self) -> SeriesTryIter<'_, 'i, T>
    where
        T: Clone,
    {
        SeriesTryIter(self.iter())
    }

    /// `to_vec()` of the items.
    #[cfg(feature = "std")]
    pub fn to_owned_series(&self) -> SeriesOwned<T>
    where
        T: Clone,
    {
        SeriesOwned(self.iter().collect())
    }
}

impl<'i, T: SeriesColumns<'i>> Clone for Series<'i, T> {
    fn clone(&self) -> Self {
        match self {
            Series::Slice { items, policy } => Series::Slice {
                items,
                policy: *policy,
            },
            Series::Buf { n, reader, raw } => Series::Buf {
                n: *n,
                reader: reader.clone(),
                raw: *raw,
            },
        }
    }
}

impl<'i, T: SeriesColumns<'i>> Default for Series<'i, T> {
    fn default() -> Self {
        Series::new(&[])
    }
}

impl<'i, T: SeriesColumns<'i> + Clone + core::fmt::Debug> core::fmt::Debug for Series<'i, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(feature = "defmt")]
impl<'i, T: SeriesColumns<'i>> defmt::Format for Series<'i, T> {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "Series({} items)", self.len())
    }
}

impl<'i, T: SeriesColumns<'i> + Clone + PartialEq> PartialEq for Series<'i, T> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl<'s, 'i, T: SeriesColumns<'i> + Clone> IntoIterator for &'s Series<'i, T> {
    type Item = T;
    type IntoIter = SeriesIter<'s, 'i, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator of a [Series]; stops early on malformed data, which deserializing already rejected.
pub enum SeriesIter<'s, 'i, T: SeriesColumns<'i>> {
    Slice {
        items: &'s [T],
        pos: usize,
    },
    Buf {
        reader: Option<T::Reader>,
        left: usize,
    },
}

impl<'i, T: SeriesColumns<'i> + Clone> SeriesIter<'_, 'i, T> {
    fn try_next(&mut self) -> Option<Result<T, Error>> {
        match self {
            SeriesIter::Slice { items, pos } => {
                let item = items.get(*pos)?.clone();
                *pos += 1;
                Some(Ok(item))
            }
            SeriesIter::Buf { reader, left } => {
                if *left == 0 {
                    return None;
                }
                *left -= 1;
                let Some(reader) = reader else {
                    *left = 0;
                    return Some(Err(Error::MalformedSeries));
                };
                let item = T::next(reader, false);
                if item.is_err() {
                    *left = 0;
                }
                Some(item)
            }
        }
    }
}

impl<'i, T: SeriesColumns<'i> + Clone> Iterator for SeriesIter<'_, 'i, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.try_next()?.ok()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = match self {
            SeriesIter::Slice { items, pos } => items.len() - *pos,
            SeriesIter::Buf { left, .. } => *left,
        };
        (0, Some(left))
    }
}

/// [Series::try_iter].
pub struct SeriesTryIter<'s, 'i, T: SeriesColumns<'i>>(SeriesIter<'s, 'i, T>);

impl<'i, T: SeriesColumns<'i> + Clone> Iterator for SeriesTryIter<'_, 'i, T> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Result<T, Error>> {
        self.0.try_next()
    }
}

/// Writes `n` and the group of `items` into `sink`: the whole `Series` but for the final byte alignment.
fn write_series<'a, 'i, T, S>(
    sink: &mut S,
    items: &'a [T],
    policy: &CodecPolicy,
) -> Result<(), Error>
where
    T: SeriesColumns<'i> + 'a,
    S: super::BitSink,
{
    if items.len() > SERIES_MAX_ITEMS {
        return Err(Error::LenTooLong);
    }
    if items.is_empty() {
        return Ok(());
    }
    T::write_group(
        items.iter().map(|item| (item, false)),
        sink,
        policy,
        Level::Outermost,
    )
}

impl<'i, T: SeriesColumns<'i>> SerializeShrinkWrap for Series<'i, T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;
    const PUSHES_LEN: bool = false;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        match self {
            Series::Slice { items, policy } => {
                wr.write_unib32(items.len().min(u32::MAX as usize) as u32)?;
                write_series(wr, items, policy)?;
                wr.align_byte();
                Ok(())
            }
            Series::Buf { raw: Some(raw), .. } => wr.write_raw_slice(raw),
            // decoded at a nibble: its bits can't be copied as bytes
            Series::Buf { raw: None, .. } => Err(Error::MalformedSeries),
        }
    }
}

#[cfg(feature = "std")]
impl<'i, T: SeriesColumns<'i>> crate::SerializeShrinkWrapOwned for Series<'i, T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
        match self {
            Series::Slice { items, policy } => {
                wr.write_unib32(items.len().min(u32::MAX as usize) as u32)?;
                write_series(wr, items, policy)?;
                wr.align_byte();
                Ok(())
            }
            Series::Buf { raw: Some(raw), .. } => wr.write_raw_slice(raw),
            Series::Buf { raw: None, .. } => Err(Error::MalformedSeries),
        }
    }
}

/// Reads `n` and skims the group: the item count and the column cursors, `rd` after the padding.
fn read_series<'i, T: SeriesColumns<'i>>(
    rd: &mut BufReader<'i>,
) -> Result<(usize, Option<T::Reader>), Error> {
    let n = rd.read_unib32()? as usize;
    if n > SERIES_MAX_ITEMS {
        return Err(Error::MalformedSeries);
    }
    let reader = if n == 0 {
        None
    } else {
        Some(T::skim(rd, RootCtx::new(n), Level::Outermost)?)
    };
    rd.align_byte();
    Ok((n, reader))
}

impl<'i, T: SeriesColumns<'i>> DeserializeShrinkWrap<'i> for Series<'i, T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let (start, bit) = rd.pos();
        let (n, reader) = read_series::<T>(rd)?;
        let raw = if bit == 7 {
            rd.bytes_at(start, rd.pos().0)
        } else {
            None
        };
        Ok(Series::Buf { n, reader, raw })
    }
}

/// Owned [Series]: a `Vec<T>` written the same way.
#[cfg(feature = "std")]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeriesOwned<T>(pub Vec<T>);

#[cfg(feature = "std")]
impl<T> SeriesOwned<T> {
    pub const fn new() -> Self {
        SeriesOwned(Vec::new())
    }

    pub fn into_inner(self) -> Vec<T> {
        self.0
    }
}

#[cfg(feature = "std")]
impl<T> From<Vec<T>> for SeriesOwned<T> {
    fn from(items: Vec<T>) -> Self {
        SeriesOwned(items)
    }
}

#[cfg(feature = "std")]
impl<T> FromIterator<T> for SeriesOwned<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        SeriesOwned(iter.into_iter().collect())
    }
}

#[cfg(feature = "std")]
impl<T> core::ops::Deref for SeriesOwned<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

#[cfg(feature = "std")]
impl<T> core::ops::DerefMut for SeriesOwned<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.0
    }
}

#[cfg(feature = "std")]
impl<T: for<'i> SeriesColumns<'i>> SeriesOwned<T> {
    /// Borrow as the zero-copy type, e.g. to serialize into a `BufWriter`.
    pub fn as_borrowed(&self) -> Series<'_, T> {
        Series::new(&self.0)
    }

    /// [as_borrowed](Self::as_borrowed) with a fixed [CodecPolicy].
    pub fn as_borrowed_with(&self, policy: CodecPolicy) -> Series<'_, T> {
        Series::with_policy(&self.0, policy)
    }
}

#[cfg(feature = "std")]
impl<T: for<'i> SeriesColumns<'i>> SerializeShrinkWrap for SeriesOwned<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;
    const PUSHES_LEN: bool = false;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        self.as_borrowed().ser_shrink_wrap(wr)
    }
}

#[cfg(feature = "std")]
impl<T: for<'i> SeriesColumns<'i>> crate::SerializeShrinkWrapOwned for SeriesOwned<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap_owned(&self, wr: &mut crate::BufWriterOwned) -> Result<(), Error> {
        self.as_borrowed().ser_shrink_wrap_owned(wr)
    }
}

#[cfg(feature = "std")]
impl<T: for<'i> SeriesColumns<'i>> crate::DeserializeShrinkWrapOwned for SeriesOwned<T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        let (n, reader) = read_series::<T>(rd)?;
        // grown as it decodes: n is data, not a reason to allocate up front
        let mut items = Vec::new();
        if let Some(mut reader) = reader {
            for _ in 0..n {
                items.push(T::next(&mut reader, false)?);
            }
        }
        Ok(SeriesOwned(items))
    }
}
