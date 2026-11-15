//! Series tracks: what a fixed-rate series is ([SeriesDef]), where its samples are in time ([SeriesSegment]) and
//! how its numbers come from wire_weaver's quantity types ([SeriesValue]).
//!
//! A series track ([TrackKind::Series](crate::TrackKind::Series)) holds samples at an exact period, with no time per
//! sample. Its [SeriesDef] says what a sample is: the type of one value as the API declares it (a [TypeRef]: an
//! exact_units `Fixed<Ampere, Milli, i32>`, a `ww_si` quantity, a plain float), the storage [Element] and channels
//! derived from that type ([SampleShape]), the period in ticks of the track's clock per sample (a [Ratio64], exact:
//! 48 kHz on a 1 ns clock is `62500/3`), the scale of the stored numbers at the start, the summaries the writer
//! builds ([SeriesSummarySpec]) and, for samples kept in a media file, the stream they are in ([MediaStream]).
//!
//! # Segments
//!
//! The records of a series track are its [SeriesSegment]s, one at the start and one at every gap, restart or change
//! of scale. A segment names its first sample's index, a time grid and the scale from there on:
//!
//! - sample `i` is at tick `grid_tick + (i - grid_index) · period`, rounded down;
//! - a gap or a restart on the same clock starts a new grid (`grid_index == first_index`) after the last tick of the
//!   one before;
//! - a change of scale (a prefix change of an autoranging device) keeps the grid of the segment before, so no sample
//!   time moves by rounding; the stored numbers mean `number · scale` of the sample type's unit from `first_index` on;
//! - indexes never go back and no sample is between two segments: segment `k` holds samples `first_index(k)` up to
//!   `first_index(k + 1)`;
//! - a device restart that is a new clock is a new track on that clock.
//!
//! A segment record's time is its first sample's tick, so record blocks code it like any record. The track is sticky:
//! every describe set repeats its newest segment, so each tape piece holds the segment its first samples are in. A
//! segment is self-contained (its grid and scale), so that one record is enough.
//!
//! A series chunk ([BlockKind::Series](crate::BlockKind::Series)) never spans two segments: its head's first and last
//! tick are its first and last sample's on the segment's grid ([SeriesTimeline::check_chunk]), and its numbers are at
//! one scale. A writer seals the open chunk at a segment.
//!
//! Summary nodes run over sample indexes, so a node may span a change of scale. It is kept at the common scale of its
//! samples, every number of the coarser ones multiplied by a whole factor, and each summary block states its scale
//! ([summary](crate::summary#scales)). The writer takes each segment with
//! [SeriesSummaries::segment](crate::summary::SeriesSummaries::segment).
//!
//! # Sample codecs and `Series<T>` codecs
//!
//! Two layers code the same numbers for different readers:
//!
//! | | `shrink_wrap` `Series<T>` (SW-60) | ww_tape [sample codecs](crate::samples) (STD-119) |
//! |---|---|---|
//! | where | on the wire, device to host, a batch of about 32 items | in a file, a chunk of up to megabytes |
//! | reads | from the start, item by item, all columns in step | any sample range, a group of 128 (delta) or 1024 (XOR) alone, in place |
//! | codecs | delta, delta of delta, packed, prefix code, XOR; picked per column and batch | raw device formats, delta and delta of delta bit-packed per group, XOR (Gorilla, Chimp) |
//! | costs | MCU-sized: a few hundred instructions per item, a few KB of flash | host-sized: SIMD unpack kernels, about 0.2 ns per value |
//! | scale | a prefix run and a number type per item (`Quantity`) or in the type (`Fixed`); a prefix change restarts the numeric column | one scale per segment; a prefix change starts a segment, and the chunk is sealed there |
//!
//! Neither is a container of the other. A batch is decoded once on the host and its numbers coded again for the
//! file: the wire codec is tuned for a small encoder, the file codec for random access and fast reads.
//!
//! # From a received `Series` batch to chunk columns
//!
//! The numbers stay integers all the way; nothing goes through a float:
//!
//! 1. Decode the batch with `Series::<T>::from_ww_bytes` and walk its items (`iter()`), zero-copy.
//! 2. `T` is a `Fixed<U, P, R>` (or a `Complex`): the stored number is `item.get()`, an `R`, the element is `R`'s and
//!    the scale is `P`'s ([SeriesValue::ELEMENT], [SeriesValue::SCALE]): [SeriesValue::write_parts] puts the
//!    numbers into the open chunk's column, channels interleaved. The scale never changes, so no segment does.
//! 3. `T` is a `ww_si` quantity: its prefix is the segment's scale (`Prefix::magnitude`); while it stays the same,
//!    the number of its `NumericValue` goes into the column as the track's element, widened exactly (an `i16` into
//!    an `i32` column), never narrowed. An item with another prefix seals the chunk and starts a segment with the
//!    same grid and the new scale ([SeriesSegment::rescaled]). The `Series` itself restarts its numeric column at
//!    the same item, so both layers break at the same place.
//! 4. A sealed column is coded with [samples::encode](crate::samples::encode) in the track's codec and written as a
//!    chunk whose head [SeriesTimeline::chunk_ticks] gives.
//!
//! The batch's time column (`Series<Timed<T>>`) is not stored: the grid gives every sample's tick. A host checks it
//! against the grid and starts a segment where the device's samples left it (a gap, a restart).

#[cfg(test)]
mod tests;
#[cfg(feature = "alloc")]
mod timeline;
#[cfg(feature = "alloc")]
pub use timeline::SeriesTimeline;

use exact_units::{
    Bounds, Complex, ComplexStorage, Fixed, FixedScale, Magnitude, Ratio64, Rounding, Storage,
    Unlimited,
};
use shrink_wrap::prelude::*;

#[cfg(feature = "std")]
use crate::TypeRefOwned;
use crate::summary::SeriesSummarySpec;
use crate::{Element, Sample, SampleShape, TypeRef};

/// Most channels of a series: a chunk column and a summary name a channel by a `u16`.
#[expect(
    clippy::as_conversions,
    reason = "u16 to u32 widens; From is not const"
)]
pub const SERIES_CHANNELS_MAX: u32 = u16::MAX as u32;

/// A fixed-rate series: scalar or vector samples of one element type, see the [module docs](self).
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone), validate = def_valid)]
pub struct SeriesDef<'i> {
    /// The type of one value of one channel, unit and scale included: `Fixed<Ampere, Milli, i32>`, a `ww_si`
    /// quantity, `f32`. None: a plain number with no unit.
    pub sample: Option<TypeRef<'i>>,
    /// The element the numbers are stored as, derived from [Self::sample] ([SeriesValue::ELEMENT]), and the
    /// channels of a sample, 1 to [SERIES_CHANNELS_MAX].
    pub shape: SampleShape,
    /// Ticks of the track's clock from one sample to the next, exact.
    pub period: Ratio64,
    /// What a stored number is worth in the sample type's unit at the start: `10⁻³` for `Fixed<Ampere, Milli, i32>`,
    /// 1 for a float. A [SeriesSegment] may change it.
    pub scale: Magnitude,
    /// The summaries the writer builds; None for none (a wide vector series).
    pub summaries: Option<SeriesSummarySpec>,
    /// The samples are in a media file's stream, decoded on demand, not in chunks.
    pub media: Option<MediaStream<'i>>,
}

/// Where a series' samples are in a media file.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone))]
pub struct MediaStream<'i> {
    /// Content hash of the media file, as the producer's media reference (a typed describe item) names it.
    pub media: RefVec<'i, u8>,
    /// The stream in the file.
    pub stream: UNib32,
}

/// Why a series definition, segment or chunk doesn't fit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SeriesError {
    /// A record doesn't decode as a [SeriesSegment].
    Decode(shrink_wrap::Error),
    /// The definition's shape, period or summaries break a rule: an element this reader doesn't know, no channels or
    /// more than [SERIES_CHANNELS_MAX], a summary spec [SeriesSummaries](crate::summary::SeriesSummaries) refuses.
    Def(&'static str),
    /// The grid starts after the segment's first sample.
    GridAfterFirst { first_index: u64, grid_index: u64 },
    /// A sample's tick doesn't fit in i64.
    TickOverflow { index: u64 },
    /// A segment's first index isn't above the one before.
    NotAscending { first_index: u64, previous: u64 },
    /// A segment with the same first index as one already there, but other content.
    Conflict { first_index: u64 },
    /// A new grid starts at or before the last sample of the segment before.
    TimeNotIncreasing { first_index: u64 },
    /// The record's time isn't its first sample's tick.
    RecordTime { first_index: u64 },
    /// A sample before the first segment.
    NoSegment { index: u64 },
    /// A chunk that isn't a series chunk of samples inside one segment.
    ChunkSpan { first_item: u64 },
    /// A chunk head's first or last tick isn't its samples' on the grid.
    ChunkTicks { first_item: u64 },
}

/// The start of a stretch of a series: its first sample, its time grid and its scale. A record of the series track,
/// at its first sample's tick; see the [module docs](self#segments).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy, Hash)
)]
pub struct SeriesSegment {
    /// Index of the segment's first sample in the track.
    pub first_index: u64,
    /// The sample the grid counts from, at most [Self::first_index].
    pub grid_index: u64,
    /// Tick of sample [Self::grid_index].
    pub grid_tick: i64,
    /// What a stored number is worth in the sample type's unit, from [Self::first_index] on.
    pub scale: Magnitude,
}

impl SeriesSegment {
    /// A segment with a grid of its own: the start of a series, or after a gap or a restart.
    pub const fn start(first_index: u64, tick: i64, scale: Magnitude) -> Self {
        SeriesSegment {
            first_index,
            grid_index: first_index,
            grid_tick: tick,
            scale,
        }
    }

    /// The segment that changes the scale from `first_index` on and keeps this one's grid.
    pub const fn rescaled(&self, first_index: u64, scale: Magnitude) -> Self {
        SeriesSegment {
            first_index,
            grid_index: self.grid_index,
            grid_tick: self.grid_tick,
            scale,
        }
    }

    /// Tick of sample `index` on the grid, rounded down; None before the grid or past i64.
    pub fn tick(&self, period: Ratio64, index: u64) -> Option<i64> {
        let steps = i64::try_from(index.checked_sub(self.grid_index)?).ok()?;
        self.grid_tick
            .checked_add(period.apply_rounded(steps, Rounding::Floor)?)
    }

    /// Tick of the first sample, checked: the grid starts at or before it and the tick fits.
    pub fn first_tick(&self, period: Ratio64) -> Result<i64, SeriesError> {
        if self.grid_index > self.first_index {
            return Err(SeriesError::GridAfterFirst {
                first_index: self.first_index,
                grid_index: self.grid_index,
            });
        }
        self.tick(period, self.first_index)
            .ok_or(SeriesError::TickOverflow {
                index: self.first_index,
            })
    }

    /// The segment a series record's item holds.
    pub fn from_record(item: &[u8]) -> Result<Self, SeriesError> {
        SeriesSegment::from_ww_bytes(item).map_err(SeriesError::Decode)
    }

    /// Whether `other` is on the same time grid.
    pub fn same_grid(&self, other: &SeriesSegment) -> bool {
        self.grid_index == other.grid_index && self.grid_tick == other.grid_tick
    }
}

/// The parts of a [SeriesDef] its check on decode looks at, borrowed or owned.
trait DefParts {
    fn parts(&self) -> (SampleShape, Option<SeriesSummarySpec>);
}

impl DefParts for SeriesDef<'_> {
    fn parts(&self) -> (SampleShape, Option<SeriesSummarySpec>) {
        (self.shape, self.summaries)
    }
}

#[cfg(feature = "std")]
impl DefParts for SeriesDefOwned {
    fn parts(&self) -> (SampleShape, Option<SeriesSummarySpec>) {
        (self.shape, self.summaries)
    }
}

fn def_valid(d: &impl DefParts) -> Result<(), shrink_wrap::Error> {
    let (shape, summaries) = d.parts();
    check_def(shape, summaries).map_err(|_| shrink_wrap::Error::SubtypeOutOfRange)
}

/// The rules of a definition's shape and summaries; the period and scale are valid by their types.
fn check_def(shape: SampleShape, summaries: Option<SeriesSummarySpec>) -> Result<(), SeriesError> {
    if Element::from_code(shape.element.code()).is_none() {
        return Err(SeriesError::Def("an element this reader doesn't know"));
    }
    if shape.channels.0 == 0 || shape.channels.0 > SERIES_CHANNELS_MAX {
        return Err(SeriesError::Def("channels out of range"));
    }
    if let Some(s) = summaries {
        if !(SeriesSummarySpec::BASE_SHIFT_MIN..=SeriesSummarySpec::BASE_SHIFT_MAX)
            .contains(&s.base_shift)
        {
            return Err(SeriesError::Def("summary base shift out of range"));
        }
        if s.stats.is_empty() {
            return Err(SeriesError::Def("summaries with no statistics"));
        }
        if (s.stats.power_mean || s.stats.power_max) && shape.element.parts() != 2 {
            return Err(SeriesError::Def("power of a real element"));
        }
    }
    Ok(())
}

impl<'i> SeriesDef<'i> {
    /// A series of `channels` channels of `V`, whose element and scale `V` gives, one sample per `period` ticks.
    pub fn of<V: SeriesValue>(sample: Option<TypeRef<'i>>, channels: u32, period: Ratio64) -> Self {
        SeriesDef {
            sample,
            shape: SampleShape::new(V::ELEMENT, channels),
            period,
            scale: V::SCALE,
            summaries: None,
            media: None,
        }
    }

    /// Check the definition as a decoded one is checked: a known element, 1 to [SERIES_CHANNELS_MAX] channels, a
    /// summary spec within its ranges.
    pub fn check(&self) -> Result<(), SeriesError> {
        check_def(self.shape, self.summaries)
    }

    /// The first segment: sample 0 at `tick`, at the definition's scale.
    pub fn first_segment(&self, tick: i64) -> SeriesSegment {
        SeriesSegment::start(0, tick, self.scale)
    }
}

/// A type whose values a series stores as numbers of an [Element] at a fixed scale: a plain number, an exact_units
/// `Fixed` or `Complex`. It gives a [SeriesDef] its element and scale and puts a value's numbers into a column.
pub trait SeriesValue: Copy {
    /// The scalar a column holds.
    type Scalar: Sample;
    /// The stored element: the scalar's, or a complex one.
    const ELEMENT: Element;
    /// What a stored number is worth in the type's unit.
    const SCALE: Magnitude;

    /// Put the value's numbers (1, or 2 for a complex value: re, im) at the start of `out`; the count put, fewer when
    /// `out` is shorter.
    fn write_parts(self, out: &mut [Self::Scalar]) -> usize;
}

macro_rules! plain_value {
    ($($t:ty),* $(,)?) => {$(
        impl SeriesValue for $t {
            type Scalar = $t;
            const ELEMENT: Element = <$t as Sample>::ELEMENT;
            const SCALE: Magnitude = Magnitude::ONE;
            #[inline(always)]
            fn write_parts(self, out: &mut [$t]) -> usize {
                put(out, [self])
            }
        }
    )*};
}

plain_value!(i8, i16, i32, i64, u8, u16, u32, u64, f32, f64, bool);

/// Put `parts` at the start of `out`, as many as fit.
#[inline(always)]
fn put<T: Copy, const N: usize>(out: &mut [T], parts: [T; N]) -> usize {
    for (o, p) in out.iter_mut().zip(parts) {
        *o = p;
    }
    out.len().min(N)
}

impl<U: Copy, P: FixedScale, T: Storage + Sample, B: Bounds<T> + Copy> SeriesValue
    for Fixed<U, P, T, B>
{
    type Scalar = T;
    const ELEMENT: Element = T::ELEMENT;
    const SCALE: Magnitude = P::SCALE;

    #[inline(always)]
    fn write_parts(self, out: &mut [T]) -> usize {
        put(out, [self.get()])
    }
}

macro_rules! complex_value {
    ($($t:ty => $e:ident),* $(,)?) => {$(
        impl<U: Copy, P: FixedScale, B: Unlimited<$t> + Copy> SeriesValue for Complex<U, P, $t, B>
        where
            $t: ComplexStorage,
        {
            type Scalar = $t;
            const ELEMENT: Element = Element::$e;
            const SCALE: Magnitude = P::SCALE;

            #[inline(always)]
            fn write_parts(self, out: &mut [$t]) -> usize {
                let (re, im) = self.get();
                put(out, [re, im])
            }
        }
    )*};
}

complex_value!(i16 => ComplexI16, f32 => ComplexF32);
