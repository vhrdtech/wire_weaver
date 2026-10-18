//! Observing properties (API-38): the host asks the device to send a trait level's properties at chosen rates with
//! [RequestKind::Observe](crate::RequestKind::Observe), the device answers with what it granted
//! ([ObserveGranted]) and then sends one [EventKind::PropertyUpdate](crate::EventKind::PropertyUpdate) per level
//! whenever any of the observed properties is due, carrying just those.
//!
//! A level is one trait: the root API or a trait implemented with `ww_impl!`, addressed by its path. Its properties
//! are named by their resource index within the level, the same numbers as in the introspection data and in request
//! paths. Only properties declared with `+observe` can be observed.
//!
//! Periods are in microseconds (`40` is 25 kHz, `1_000_000` once a second). The device clamps every setting to
//! what it can do and to the level's byte budget, and tells the host the result.
//!
//! **Every sample (API-39).** A property declared `+observe+batched` can also be observed in
//! [ObserveMode::Samples]: the device keeps every sample it takes of it (or one per `every_us`) and sends them in
//! batches, at most `max_age_us` old. Its slot in the level's update is a [Batched]: [Batched::One] with the other
//! modes (the update's `t` is its time, as for any property), [Batched::Samples] with `Samples`. The samples of a
//! level share one clock: one `sample()` call on the device is one row, so an update carries their times once, in
//! the level's `sample_times` slot (a `Series<Second>` past the level's last resource index), and every
//! `Batched::Samples` of the update has one item per time. A level's `Samples` entries share `every_us` and
//! `max_age_us` (the finest and the shortest asked for), the grant tells.

use core::fmt::Debug;
use wire_weaver::prelude::*;
#[cfg(feature = "std")]
use wire_weaver::shrink_wrap::series::SeriesOwned;
use wire_weaver::shrink_wrap::series::{Series, SeriesColumns};
use wire_weaver::shrink_wrap::{
    BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap,
};
#[cfg(feature = "std")]
use wire_weaver::shrink_wrap::{
    BufWriterOwned, DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned,
};
use ww_si::Second;

/// How one property is observed.
///
/// Evolvable: a device reading a mode from a newer host gets `Unknown(_)` and refuses that entry
/// ([ObserveRefusal::ModeNotSupported]).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = u4
)]
pub enum ObserveMode {
    /// Not sent.
    Off,
    /// Sent when the value changes, at most once every `min_us`.
    OnChange { min_us: UNib32 },
    /// Sent every `every_us`, changed or not.
    Periodic { every_us: UNib32 },
    /// Sent when the value changes, at most once every `min_us`, and at least once every `max_us` even when it
    /// doesn't (a heartbeat, so the host knows the value is still current).
    OnChangeOrBeat { min_us: UNib32, max_us: UNib32 },
    /// Every sample the device takes (`every_us = 0`), or the first one in each `every_us`, sent in batches that
    /// are at most `max_age_us` old (API-39): the host's choice of latency against overhead. Only properties
    /// declared `+observe+batched` take it ([ObserveRefusal::ModeNotSupported] otherwise); their samples come in
    /// [Batched::Samples] slots. The `Samples` entries of a level share one setting: the device grants each of them
    /// the smallest `every_us` and the smallest `max_age_us` of them all.
    Samples {
        every_us: UNib32,
        max_age_us: UNib32,
    },
}

/// One property of a level and how it is (to be) observed.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct ObserveEntry {
    /// Resource index of the property within its trait level.
    pub index: UNib32,
    pub mode: ObserveMode,
}

/// Body of [RequestKind::Observe](crate::RequestKind::Observe), sent to a trait level's path.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, Clone))]
pub struct ObserveRequest<'i> {
    /// Settings per property, at most one entry per index.
    pub entries: RefVec<'i, ObserveEntry>,
    /// `true`: properties not listed keep their current setting. `false`: properties not listed are `Off`, so
    /// `entries: [], patch: false` stops observing the whole level.
    pub patch: bool,
    /// Bytes per second the level's updates may take on the link (events as sent, with their headers); `None`
    /// leaves it to the device.
    pub budget: Option<UNib32>,
}

#[cfg(feature = "std")]
impl ObserveRequest<'_> {
    pub fn make_owned(&self) -> Result<ObserveRequestOwned, Error> {
        Ok(ObserveRequestOwned {
            entries: self.entries.iter().collect::<Result<Vec<_>, _>>()?,
            patch: self.patch,
            budget: self.budget,
        })
    }
}

/// Answer to [RequestKind::Observe](crate::RequestKind::Observe), in an
/// [EventKind::Value](crate::EventKind::Value): the level's settings as the device applies them now.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, Clone))]
pub struct ObserveGranted<'i> {
    /// Every property of the level that is observed now (not `Off`), with its period clamped to what the device can
    /// do, in index order.
    pub entries: RefVec<'i, ObserveEntry>,
    /// Entries of the request that were not applied, and why.
    pub refused: RefVec<'i, ObserveRefused>,
    /// Bytes per second the level's updates may take, as granted.
    pub budget: UNib32,
}

/// An entry of an [ObserveRequest] the device did not apply.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct ObserveRefused {
    pub index: UNib32,
    pub reason: ObserveRefusal,
}

/// Why an entry of an [ObserveRequest] was not applied.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = u4
)]
pub enum ObserveRefusal {
    /// No resource with this index in the level.
    NoSuchIndex,
    /// The resource is not a property, or a property without `+observe`.
    NotObservable,
    /// A mode this device doesn't know or doesn't support, or [ObserveMode::Samples] of a property that isn't
    /// `+batched`.
    ModeNotSupported,
    /// The index was listed more than once; the first entry was applied.
    Duplicate,
}

/// The slot of a `+observe+batched` property in its level's update (API-39): one value, as for any property, or
/// a batch of samples, their times in the update's `sample_times` slot (one per sample, the level's shared clock).
///
/// ```text
/// tag:     u4      0: One, 1: Samples, others: a form this host doesn't know (an error)
/// One:     T       as a field
/// Samples: zero bits to the next byte, then Series<T>, to the end of the slot
/// ```
///
/// The series starts at a byte, so a device copies a batch it encoded before as it is (`Series::Buf { raw }`). A
/// `Batched` is `Unsized`: in an update it is one slot, its length in the update's lengths.
pub enum Batched<'i, T>
where
    T: SeriesColumns<'i>,
{
    /// Observed with [ObserveMode::OnChange], `Periodic` or `OnChangeOrBeat`: the value at the update's `t`.
    One(T),
    /// Observed with [ObserveMode::Samples]: the samples since the previous batch, oldest first, at the times in
    /// the update's `sample_times`.
    Samples(Series<'i, T>),
}

const BATCHED_ONE: u8 = 0;
const BATCHED_SAMPLES: u8 = 1;

impl<'i, T: Debug> Debug for Batched<'i, T>
where
    T: SeriesColumns<'i> + Clone,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Batched::One(value) => f.debug_tuple("One").field(value).finish(),
            Batched::Samples(series) => f.debug_tuple("Samples").field(series).finish(),
        }
    }
}

impl<'i, T: SerializeShrinkWrap> SerializeShrinkWrap for Batched<'i, T>
where
    T: SeriesColumns<'i>,
{
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        match self {
            Batched::One(value) => {
                wr.write_nib_masked(BATCHED_ONE)?;
                wr.write(value)
            }
            Batched::Samples(series) => {
                wr.write_nib_masked(BATCHED_SAMPLES)?;
                wr.align_byte();
                series.ser_shrink_wrap(wr)
            }
        }
    }
}

impl<'i, T: DeserializeShrinkWrap<'i>> DeserializeShrinkWrap<'i> for Batched<'i, T>
where
    T: SeriesColumns<'i>,
{
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        match rd.read_nib()?.value() {
            BATCHED_ONE => Ok(Batched::One(rd.read()?)),
            BATCHED_SAMPLES => {
                rd.align_byte();
                Ok(Batched::Samples(Series::des_shrink_wrap(rd)?))
            }
            _ => Err(Error::EnumFutureVersionOrMalformedData),
        }
    }
}

/// Writes the bytes of [Batched::One] of a value it borrows: what a device writes for a `+batched` property
/// observed with another mode than `Samples`.
pub struct BatchedOne<'a, T>(pub &'a T);

impl<T: SerializeShrinkWrap> SerializeShrinkWrap for BatchedOne<'_, T> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_nib_masked(BATCHED_ONE)?;
        wr.write(self.0)
    }
}

/// Writes the bytes of [Batched::Samples] of a `Series<T>` encoded before (from a byte, as `to_ww_bytes` writes
/// it): a device copies the batches waiting in its outbox with it.
pub struct BatchedRaw<'a>(pub &'a [u8]);

impl SerializeShrinkWrap for BatchedRaw<'_> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;
    const PUSHES_LEN: bool = false;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.write_nib_masked(BATCHED_SAMPLES)?;
        wr.align_byte();
        wr.write_raw_slice(self.0)
    }
}

/// Writes a `Series` encoded before (from a byte, as `to_ww_bytes` writes it) as a field: what a device writes
/// into a level's `sample_times` slot, a `Series<Second>` from its outbox.
pub struct SeriesRaw<'a>(pub &'a [u8]);

impl SerializeShrinkWrap for SeriesRaw<'_> {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;
    const PUSHES_LEN: bool = false;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.align_byte();
        wr.write_raw_slice(self.0)
    }
}

/// Owned [Batched], same bytes.
#[cfg(feature = "std")]
#[derive(Debug, Clone, PartialEq)]
pub enum BatchedOwned<T> {
    One(T),
    Samples(SeriesOwned<T>),
}

#[cfg(feature = "std")]
impl<T> BatchedOwned<T> {
    /// The samples with their device times: one with the update's `t` for [BatchedOwned::One], every item of
    /// [BatchedOwned::Samples] with its time from the update's `sample_times`. An update without `sample_times`
    /// next to `Samples`, or with a different number of times than samples, is malformed: an error.
    pub fn into_samples(
        self,
        t: Option<Second>,
        sample_times: Option<&[Second]>,
    ) -> Result<Vec<(T, Option<Second>)>, Error> {
        match self {
            BatchedOwned::One(value) => Ok(vec![(value, t)]),
            BatchedOwned::Samples(series) => {
                let items = series.into_inner();
                match sample_times {
                    Some(times) if times.len() == items.len() => Ok(items
                        .into_iter()
                        .zip(times)
                        .map(|(value, t)| (value, Some(*t)))
                        .collect()),
                    _ => Err(Error::MalformedSeries),
                }
            }
        }
    }
}

#[cfg(feature = "std")]
impl<T: SerializeShrinkWrapOwned> SerializeShrinkWrapOwned for BatchedOwned<T>
where
    T: for<'i> SeriesColumns<'i>,
{
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), Error> {
        match self {
            BatchedOwned::One(value) => {
                wr.write_nib_masked(BATCHED_ONE)?;
                wr.write(value)
            }
            BatchedOwned::Samples(series) => {
                wr.write_nib_masked(BATCHED_SAMPLES)?;
                wr.align_byte();
                series.ser_shrink_wrap_owned(wr)
            }
        }
    }
}

#[cfg(feature = "std")]
impl<T: DeserializeShrinkWrapOwned> DeserializeShrinkWrapOwned for BatchedOwned<T>
where
    T: for<'i> SeriesColumns<'i>,
{
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        match rd.read_nib()?.value() {
            BATCHED_ONE => Ok(BatchedOwned::One(rd.read_owned()?)),
            BATCHED_SAMPLES => {
                rd.align_byte();
                Ok(BatchedOwned::Samples(SeriesOwned::des_shrink_wrap_owned(
                    rd,
                )?))
            }
            _ => Err(Error::EnumFutureVersionOrMalformedData),
        }
    }
}

/// Serialize an [EventKind::PropertyUpdate](crate::EventKind::PropertyUpdate) event of the level at `path` into
/// `scratch`: `data` writes the level's properties (an `OptionAnyVec` by resource index) at the start of the buffer
/// it is given and returns their length. Same bytes as serializing the event with the data in place.
pub fn ser_property_update_event<'a>(
    scratch: &'a mut [u8],
    path: &[UNib32],
    t: Option<Second>,
    missed: Option<UNib32>,
    data: impl FnOnce(&mut [u8]) -> Result<usize, Error>,
) -> Result<&'a [u8], Error> {
    use crate::EventKindDiscriminants;
    use crate::builder::{EventBuilder, EventKindBuilder};
    let mut header_buf = [0u8; 64];
    let mut wr = BufWriter::new(&mut header_buf);
    let event_builder = EventBuilder::new(0, &mut wr)?;
    let event_kind_builder = EventKindBuilder::new(&mut wr)?;
    wr.write(&RefVec::Slice { slice: path })?;
    t.ser_shrink_wrap(&mut wr)?;
    missed.ser_shrink_wrap(&mut wr)?;
    event_kind_builder.finish_with_kind(EventKindDiscriminants::PropertyUpdate, &mut wr);
    event_builder.finish(true, &mut wr);
    wr.align_byte();
    let head_len = wr.pos().0;
    let header = wr.finish_and_take()?;
    if head_len > scratch.len() {
        return Err(Error::OutOfBoundsWriteRawSlice);
    }
    let data_len = data(&mut scratch[head_len..])?;
    let sizes = &header[head_len..];
    let end = head_len + data_len + sizes.len();
    if end > scratch.len() {
        return Err(Error::OutOfBoundsWriteRawSlice);
    }
    scratch[..head_len].copy_from_slice(&header[..head_len]);
    scratch[head_len + data_len..end].copy_from_slice(sizes);
    Ok(&scratch[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, EventKind, PathKind, Request, RequestKind};
    use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
    use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, UVlq32, UVlq32Backfill};
    use ww_si::Prefix;
    use ww_si::ww_numeric::NumericValue;

    const MICROS: Second = Second::from_parts(Prefix::Micro, NumericValue::U32(0x1234_5678));

    #[test]
    fn request_round_trip() {
        let entries = [
            ObserveEntry {
                index: UNib32(0),
                mode: ObserveMode::Periodic {
                    every_us: UNib32(40),
                },
            },
            ObserveEntry {
                index: UNib32(3),
                mode: ObserveMode::OnChangeOrBeat {
                    min_us: UNib32(1_000),
                    max_us: UNib32(1_000_000),
                },
            },
        ];
        let path = [UNib32(1)];
        let req = Request {
            seq: UVlq32Backfill(5),
            path_kind: PathKind::absolute(&path),
            kind: RequestKind::Observe {
                cfg: ObserveRequest {
                    entries: RefVec::Slice { slice: &entries },
                    patch: false,
                    budget: Some(UNib32(20_000)),
                },
            },
        };
        let mut buf = [0u8; 64];
        let b = req.to_ww_bytes(&mut buf).unwrap().to_vec();
        let back = Request::from_ww_bytes(&b).unwrap();
        let RequestKind::Observe { cfg } = back.kind else {
            panic!("{back:?}");
        };
        let back_entries: Vec<_> = cfg.entries.iter().map(|e| e.unwrap()).collect();
        assert_eq!(back_entries, entries);
        assert!(!cfg.patch);
        assert_eq!(cfg.budget, Some(UNib32(20_000)));
        // 5 of them are the seq placeholder, sent as 1 (Request::set_seq)
        assert_eq!(b.len(), 26, "{b:02x?}");
    }

    #[test]
    fn granted_round_trip() {
        let entries = [ObserveEntry {
            index: UNib32(2),
            mode: ObserveMode::OnChange {
                min_us: UNib32(100),
            },
        }];
        let refused = [ObserveRefused {
            index: UNib32(7),
            reason: ObserveRefusal::NotObservable,
        }];
        let granted = ObserveGranted {
            entries: RefVec::Slice { slice: &entries },
            refused: RefVec::Slice { slice: &refused },
            budget: UNib32(1000),
        };
        let mut buf = [0u8; 64];
        let b = granted.to_ww_bytes(&mut buf).unwrap().to_vec();
        let back = ObserveGranted::from_ww_bytes(&b).unwrap();
        assert_eq!(back.entries.iter().next().unwrap().unwrap(), entries[0]);
        assert_eq!(back.refused.iter().next().unwrap().unwrap(), refused[0]);
        assert_eq!(back.budget, UNib32(1000));
    }

    #[test]
    fn unknown_mode_is_read() {
        // a newer host's mode reads as Unknown, so the device can refuse just that entry
        let mut buf = [0u8; 16];
        let mode = ObserveMode::Unknown(9);
        let b = mode.to_ww_bytes(&mut buf).unwrap().to_vec();
        assert_eq!(
            ObserveMode::from_ww_bytes(&b).unwrap(),
            ObserveMode::Unknown(9)
        );
    }

    #[test]
    fn update_event_in_place() {
        let path = [UNib32(2)];
        let data = [0xaa, 0xbb, 0xcc];
        let event = Event {
            seq: UVlq32(0),
            result: Ok(EventKind::PropertyUpdate {
                path: RefVec::Slice { slice: &path },
                t: Some(MICROS),
                missed: Some(UNib32(3)),
                data: TailBytes(&data),
            }),
        };
        let mut buf = [0u8; 64];
        let expected = event.to_ww_bytes(&mut buf).unwrap().to_vec();

        let mut scratch = [0u8; 64];
        let built =
            ser_property_update_event(&mut scratch, &path, Some(MICROS), Some(UNib32(3)), |buf| {
                buf[..3].copy_from_slice(&data);
                Ok(3)
            })
            .unwrap();
        assert_eq!(built, expected);
        let back = Event::from_ww_bytes(built).unwrap();
        let Ok(EventKind::PropertyUpdate {
            path: p,
            t,
            missed,
            data: d,
        }) = back.result
        else {
            panic!("{back:?}");
        };
        assert_eq!(p.iter().next().unwrap().unwrap(), UNib32(2));
        assert!(t.is_some());
        assert_eq!(missed, Some(UNib32(3)));
        assert_eq!(d.0, data);
    }

    #[test]
    fn samples_mode_round_trip() {
        let mode = ObserveMode::Samples {
            every_us: UNib32(0),
            max_age_us: UNib32(20_000),
        };
        let mut buf = [0u8; 16];
        let b = mode.to_ww_bytes(&mut buf).unwrap().to_vec();
        assert_eq!(ObserveMode::from_ww_bytes(&b).unwrap(), mode);
    }

    fn samples() -> Vec<u32> {
        (0..10u32).map(|i| 500 + i).collect()
    }

    fn times() -> Vec<Second> {
        (0..10u32).map(|i| Second::micro(1_000 + i * 40)).collect()
    }

    /// Both forms, borrowed and owned, same bytes; a decoded series is copied as it is.
    #[test]
    fn batched_round_trip() {
        let items = samples();
        let mut buf = [0u8; 128];
        for (borrowed, owned) in [
            (Batched::One(7u32), BatchedOwned::One(7u32)),
            (
                Batched::Samples(Series::new(&items)),
                BatchedOwned::Samples(items.clone().into()),
            ),
        ] {
            let b = borrowed.to_ww_bytes(&mut buf).unwrap().to_vec();
            assert_eq!(owned.to_ww_bytes_owned().unwrap(), b);
            assert_eq!(BatchedOwned::<u32>::from_ww_bytes_owned(&b).unwrap(), owned);
            let back = Batched::<u32>::from_ww_bytes(&b).unwrap();
            let mut again = [0u8; 128];
            assert_eq!(back.to_ww_bytes(&mut again).unwrap(), b);
        }
        // what a device writes: the same bytes
        let raw = Batched::Samples(Series::new(&items))
            .to_ww_bytes(&mut buf)
            .unwrap()
            .to_vec();
        let mut again = [0u8; 128];
        assert_eq!(
            BatchedOne(&7u32).to_ww_bytes(&mut again).unwrap(),
            Batched::One(7u32).to_ww_bytes(&mut buf).unwrap()
        );
        let series = Series::new(&items).to_ww_bytes(&mut buf).unwrap().to_vec();
        assert_eq!(BatchedRaw(&series).to_ww_bytes(&mut again).unwrap(), raw);
        // the tag nibble and its padding: one byte before the series
        let series = Series::new(&items).to_ww_bytes(&mut buf).unwrap().to_vec();
        let b = Batched::Samples(Series::new(&items))
            .to_ww_bytes(&mut buf)
            .unwrap()
            .to_vec();
        assert_eq!(b.len(), series.len() + 1);
        assert_eq!(&b[1..], series);
        assert!(BatchedOwned::<u32>::from_ww_bytes_owned(&[0x20]).is_err());
    }

    #[test]
    fn batched_samples_and_times() {
        let t = Second::micro(5u32);
        assert_eq!(
            BatchedOwned::One(3u32).into_samples(Some(t), None).unwrap(),
            vec![(3, Some(t))]
        );
        let batch = BatchedOwned::Samples(samples().into());
        let times = times();
        let got = batch.clone().into_samples(None, Some(&times)).unwrap();
        assert_eq!(got[9], (509, Some(Second::micro(1_360u32))));
        // device data: a batch without its times, or with as many, is malformed, not a panic
        assert!(batch.clone().into_samples(None, None).is_err());
        assert!(batch.into_samples(None, Some(&times[..9])).is_err());
    }

    /// The `sample_times` slot as a device copies it from its outbox: the series' own bytes, from a byte.
    #[test]
    fn series_raw_is_the_series() {
        let times = times();
        let mut buf = [0u8; 128];
        let series = Series::new(&times).to_ww_bytes(&mut buf).unwrap().to_vec();
        let mut again = [0u8; 128];
        assert_eq!(SeriesRaw(&series).to_ww_bytes(&mut again).unwrap(), series);
    }

    /// Without a timestamp and nothing missed, an update of a level one deep costs 4 bytes plus its data.
    #[test]
    fn update_event_overhead() {
        let path = [UNib32(2)];
        let mut scratch = [0u8; 64];
        let built = ser_property_update_event(&mut scratch, &path, None, None, |buf| {
            buf[0] = 0xaa;
            Ok(1)
        })
        .unwrap();
        assert_eq!(built.len(), 4 + 1, "{built:02x?}");
    }
}
